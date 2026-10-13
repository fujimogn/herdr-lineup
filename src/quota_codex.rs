//! Bounded Codex account quota queries through app-server stdio.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(300);
pub const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
pub const MAX_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);

/// Subscription-account usage, independent of any conversation or pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weekly {
    pub used_percent: f64,
    /// Absolute Unix reset time supplied by Codex, never inferred from a duration.
    pub resets_at: Option<SystemTime>,
}

/// An authenticated query, scheduled only by an explicitly enabled quota worker.
/// Codex resolves its own credentials; Lineup never opens an authentication file.
/// Errors contain only fixed diagnostics, never app-server output or account metadata.
/// PermissionDenied means the cache must discard its prior account observation.
/// Other errors are transient and may retain a prior value until its known reset.
pub fn read_weekly(codex_home: &Path, timeout: Duration) -> io::Result<Option<Weekly>> {
    read_weekly_controlled(codex_home, timeout, &QueryControl::default())
}

fn read_weekly_controlled(
    codex_home: &Path,
    timeout: Duration,
    control: &QueryControl,
) -> io::Result<Option<Weekly>> {
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Codex quota query already running",
        ));
    }
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    let _permit = Permit;
    read_with_control(codex_home, timeout, Command::new("codex"), control)
}

/// A cache for a single caller-resolved Codex home/profile. Construct a new client
/// when that profile changes. No cache files or raw account identities are stored.
/// refresh() is synchronous and bounded; the daemon schedules it on one worker,
/// independently of publication, once per configured home rather than per pane.
pub struct WeeklyClient {
    codex_home: PathBuf,
    interval: Duration,
    timeout: Duration,
    last_attempt: Option<Instant>,
    cached: Option<Weekly>,
    last_result: crate::doctor::WeeklyResult,
}

impl WeeklyClient {
    pub fn new(codex_home: PathBuf, interval: Duration, timeout: Duration) -> Self {
        Self {
            codex_home,
            interval: interval.clamp(MIN_REFRESH_INTERVAL, MAX_REFRESH_INTERVAL),
            timeout: timeout.clamp(Duration::from_millis(1), MAX_TIMEOUT),
            last_attempt: None,
            cached: None,
            last_result: crate::doctor::WeeklyResult::Pending,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Last actual query category, independent of the display cache.
    pub(crate) fn last_result(&self) -> crate::doctor::WeeklyResult {
        self.last_result
    }

    /// Returns a cached value between attempts, including after transient errors.
    /// Authentication failures and absent subscriptions clear it immediately.
    pub fn refresh(&mut self) -> Option<Weekly> {
        self.refresh_controlled(&QueryControl::default())
    }

    pub(crate) fn refresh_controlled(&mut self, control: &QueryControl) -> Option<Weekly> {
        let home = self.codex_home.clone();
        let timeout = self.timeout;
        self.refresh_with(Instant::now(), SystemTime::now(), || {
            read_weekly_controlled(&home, timeout, control)
        });
        self.expire(SystemTime::now());
        self.cached
    }

    fn expire(&mut self, now: SystemTime) {
        if self
            .cached
            .is_some_and(|weekly| weekly.resets_at.is_some_and(|reset| now >= reset))
        {
            self.cached = None;
        }
    }

    fn refresh_with(
        &mut self,
        now: Instant,
        wall: SystemTime,
        query: impl FnOnce() -> io::Result<Option<Weekly>>,
    ) -> Option<Weekly> {
        self.expire(wall);
        if self
            .last_attempt
            .is_some_and(|last| now.saturating_duration_since(last) < self.interval)
        {
            return self.cached;
        }
        self.last_attempt = Some(now);
        use crate::doctor::WeeklyResult;
        match query() {
            Ok(value) => {
                self.last_result = if value.is_some() {
                    WeeklyResult::Ok
                } else {
                    WeeklyResult::NonWeekly
                };
                self.cached = value;
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                self.last_result = WeeklyResult::Unauthorized;
                self.cached = None;
            }
            Err(_) => {
                self.last_result = WeeklyResult::Transient;
                // An unknown reset cannot justify retaining a failed observation.
                if self.cached.is_some_and(|weekly| weekly.resets_at.is_none()) {
                    self.cached = None;
                }
            }
        }
        self.expire(wall);
        self.cached
    }
}

// An unreaped child retains its PID, so killing its process group cannot target
// a recycled PID. No background I/O threads or timeout workers can accumulate.
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: Command::process_group(0) created this child's own process group.
        unsafe {
            libc::killpg(self.0.id() as libc::pid_t, libc::SIGKILL);
        }
        let _ = self.0.kill();
        loop {
            match self.0.wait() {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => break,
            }
        }
    }
}

/// Own the in-flight process independently of the query thread. Shutdown takes
/// and drops the child under this lock, killing its group and reaping its PID.
/// Spawn and stop use the same lock: even a shutdown/spawn race cannot orphan a
/// newly started app-server or signal a recycled process-group ID.
#[derive(Default)]
pub(crate) struct QueryControl {
    stopped: AtomicBool,
    server: Mutex<Option<Server>>,
}
impl QueryControl {
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let mut server = self
            .server
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        drop(server.take());
    }
}

struct QueryGuard<'a>(&'a QueryControl);
impl Drop for QueryGuard<'_> {
    fn drop(&mut self) {
        let mut server = self
            .0
            .server
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        drop(server.take());
    }
}

#[cfg(test)]
fn read_with_command(
    codex_home: &Path,
    timeout: Duration,
    command: Command,
) -> io::Result<Option<Weekly>> {
    read_with_control(codex_home, timeout, command, &QueryControl::default())
}

fn read_with_control(
    codex_home: &Path,
    timeout: Duration,
    mut command: Command,
    control: &QueryControl,
) -> io::Result<Option<Weekly>> {
    if timeout.is_zero() || !codex_home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Codex quota query options",
        ));
    }
    let deadline = Instant::now() + timeout.min(MAX_TIMEOUT);
    let mut active = control
        .server
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if control.stopped.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Codex quota stopped",
        ));
    }
    let child = command
        .arg("app-server")
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| io::Error::other("Cannot start Codex app-server"))?;
    let mut server = Server(child);
    let input = server
        .0
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing Codex stdin"))?;
    let output = server
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing Codex stdout"))?;
    *active = Some(server);
    drop(active);
    let _guard = QueryGuard(control);
    nonblocking(input.as_raw_fd())?;
    nonblocking(output.as_raw_fd())?;
    let mut rpc = Rpc {
        input,
        output,
        pending: Vec::new(),
        received: 0,
        deadline,
    };
    rpc.request(
        1,
        "initialize",
        json!({
            "clientInfo":{"name":"herdr-lineup","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{}
        }),
    )?;
    rpc.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}))?;
    // An explicit account failure invalidates a prior observation rather than
    // keeping another account's subscription. refreshToken=false does not log in.
    let account = rpc.request(2, "account/read", json!({"refreshToken":false}))?;
    match account.get("account") {
        Some(Value::Object(details)) => match details.get("type").and_then(Value::as_str) {
            Some("chatgpt") => {}
            Some(_) => return Ok(None),
            None => return Err(io::Error::other("Invalid Codex account response")),
        },
        Some(Value::Null) => return Ok(None),
        _ => return Err(io::Error::other("Invalid Codex account response")),
    }
    let limits = rpc.request(3, "account/rateLimits/read", json!({}))?;
    Ok(parse_weekly(&limits))
}

fn parse_weekly(result: &Value) -> Option<Weekly> {
    // Prefer the canonical Codex bucket. Never substitute another model's bucket.
    let limits = result
        .get("rateLimits")
        .filter(|value| value.is_object())
        .or_else(|| result.pointer("/rateLimitsByLimitId/codex"))?;
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| limits.get(key))
        .find_map(|window| {
            if window.get("windowDurationMins").and_then(Value::as_u64) != Some(7 * 24 * 60) {
                return None;
            }
            let used_percent = window.get("usedPercent")?.as_f64()?;
            if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
                return None;
            }
            let resets_at = match window.get("resetsAt") {
                None | Some(Value::Null) => None,
                Some(value) => Some(UNIX_EPOCH.checked_add(Duration::from_secs(value.as_u64()?))?),
            };
            Some(Weekly {
                used_percent,
                resets_at,
            })
        })
}

fn authentication_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Codex subscription account unavailable",
    )
}

fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Codex quota query timed out",
        ))
    } else {
        Ok(())
    }
}

fn nonblocking(fd: i32) -> io::Result<()> {
    // SAFETY: fd belongs to a live pipe, and fcntl does not take ownership.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::other("Cannot configure Codex pipe"));
    }
    Ok(())
}

fn wait_ready(fd: i32, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    loop {
        check_deadline(deadline)?;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let millis = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1)
            .min(i32::MAX as u128) as i32;
        // SAFETY: the descriptor is initialized and the pipe remains owned by Rpc.
        let ready = unsafe { libc::poll(&mut descriptor, 1, millis) };
        if ready > 0 {
            return Ok(());
        }
        if ready < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::other("Codex pipe polling failed"));
        }
    }
}

struct Rpc {
    input: ChildStdin,
    output: ChildStdout,
    pending: Vec<u8>,
    received: usize,
    deadline: Instant,
}

impl Rpc {
    fn send(&mut self, value: Value) -> io::Result<()> {
        let mut bytes =
            serde_json::to_vec(&value).map_err(|_| io::Error::other("Invalid Codex request"))?;
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            check_deadline(self.deadline)?;
            match self.input.write(&bytes[offset..]) {
                Ok(0) => return Err(io::Error::other("Codex stdin closed")),
                Ok(written) => offset += written,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(self.input.as_raw_fd(), libc::POLLOUT, self.deadline)?
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(io::Error::other("Codex request write failed")),
            }
        }
        Ok(())
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> io::Result<Value> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            check_deadline(self.deadline)?;
            let value = self.line()?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                // Classify but never return, log or retain raw provider diagnostics.
                let code = error.get("code").and_then(Value::as_i64);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(code, Some(401 | 403))
                    || ["401", "403", "unauthorized", "forbidden", "authentication"]
                        .iter()
                        .any(|marker| message.contains(marker))
                {
                    return Err(authentication_error());
                }
                if method == "account/read" {
                    return Err(authentication_error());
                }
                return Err(io::Error::other("Codex app-server request failed"));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| io::Error::other("Missing Codex response result"));
        }
    }

    fn line(&mut self) -> io::Result<Value> {
        loop {
            check_deadline(self.deadline)?;
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                if end > MAX_LINE_BYTES {
                    return Err(io::Error::other("Codex response exceeds size limit"));
                }
                let value = serde_json::from_slice(&self.pending[..end])
                    .map_err(|_| io::Error::other("Invalid Codex response"))?;
                self.pending.drain(..=end);
                return Ok(value);
            }
            if self.pending.len() > MAX_LINE_BYTES {
                return Err(io::Error::other("Codex response exceeds size limit"));
            }
            let mut chunk = [0; 4096];
            match self.output.read(&mut chunk) {
                Ok(0) => return Err(io::Error::other("Codex app-server exited before response")),
                Ok(count) => {
                    self.received += count;
                    if self.received > MAX_RESPONSE_BYTES {
                        return Err(io::Error::other("Codex output exceeds size limit"));
                    }
                    self.pending.extend_from_slice(&chunk[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(self.output.as_raw_fd(), libc::POLLIN, self.deadline)?
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(io::Error::other("Codex response read failed")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_for_pid(path: &std::path::Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(pid) = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
                .filter(|pid| *pid > 0)
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "test PID handshake timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn last_query_result_is_separate_from_retained_cache() {
        use crate::doctor::WeeklyResult;
        let now = Instant::now();
        let wall = SystemTime::now();
        let mut client = WeeklyClient::new(
            PathBuf::from("/fake"),
            MIN_REFRESH_INTERVAL,
            DEFAULT_TIMEOUT,
        );
        let sample = Some(Weekly {
            used_percent: 35.0,
            resets_at: Some(wall + Duration::from_secs(600)),
        });
        assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
        assert_eq!(client.last_result(), WeeklyResult::Ok);
        assert_eq!(
            client.refresh_with(now + MIN_REFRESH_INTERVAL, wall, || Err(io::Error::other(
                "fake private error"
            ))),
            sample
        );
        assert_eq!(client.last_result(), WeeklyResult::Transient);
        client.refresh_with(now + MIN_REFRESH_INTERVAL * 2, wall, || {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fake private error",
            ))
        });
        assert_eq!(client.last_result(), WeeklyResult::Unauthorized);
        client.refresh_with(now + MIN_REFRESH_INTERVAL * 3, wall, || Ok(None));
        assert_eq!(client.last_result(), WeeklyResult::NonWeekly);
    }

    #[test]
    fn shutdown_reaps_inflight_app_server_without_waiting_for_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("blocked-codex");
        fs::write(
            &script,
            "#!/bin/sh\n: > \"$CODEX_HOME/pid\"\n/bin/sleep 0.2\necho $$ > \"$CODEX_HOME/pid\"\nexec sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let control = std::sync::Arc::new(QueryControl::default());
        let query_control = control.clone();
        let home = dir.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            // Interpret the fixture to avoid ETXTBSY from concurrent forks retaining write FDs.
            let mut command = Command::new("/bin/sh");
            command.arg(script);
            read_with_control(&home, MAX_TIMEOUT, command, &query_control)
        });
        let pid = wait_for_pid(&dir.path().join("pid"));
        let started = Instant::now();
        control.stop();
        assert!(handle.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert!(
            read_with_control(
                dir.path(),
                MAX_TIMEOUT,
                Command::new("must-not-run"),
                &control
            )
            .is_err()
        );
    }
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    struct Fake {
        dir: tempfile::TempDir,
        executable: std::path::PathBuf,
    }

    impl Fake {
        fn new(account: Value, limits: Value) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let executable = dir.path().join("fake-codex");
            let script = format!(
                "#!/bin/sh\n[ \"$1\" = app-server ] || exit 10\n[ \"$HOME\" = \"$CODEX_HOME\" ] || exit 11\necho $$ > \"$HOME/pid\"\nIFS= read -r request || exit 0\nprintf '%s\\n' \"$request\" >> \"$HOME/requests\"\nprintf '%s\\n' '{}'\nIFS= read -r request || exit 0\nprintf '%s\\n' \"$request\" >> \"$HOME/requests\"\nIFS= read -r request || exit 0\nprintf '%s\\n' \"$request\" >> \"$HOME/requests\"\nprintf '%s\\n' '{}'\nIFS= read -r request || exit 0\nprintf '%s\\n' \"$request\" >> \"$HOME/requests\"\nprintf '%s\\n' '{}'\nexec sleep 30\n",
                json!({"id":1,"result":{"userAgent":"synthetic"}}),
                json!({"id":2,"result":account}),
                json!({"id":3,"result":limits}),
            );
            fs::write(&executable, script).unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            Self { dir, executable }
        }

        fn read(&self, timeout: Duration) -> io::Result<Option<Weekly>> {
            // Interpret the fixture to avoid ETXTBSY from concurrent forks retaining write FDs.
            let mut command = Command::new("/bin/sh");
            command.arg(&self.executable);
            command.env("HOME", self.dir.path());
            read_with_command(self.dir.path(), timeout, command)
        }

        fn methods(&self) -> Vec<String> {
            fs::read_to_string(self.dir.path().join("requests"))
                .unwrap()
                .lines()
                .map(|line| {
                    serde_json::from_str::<Value>(line).unwrap()["method"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .collect()
        }

        fn assert_reaped(&self) {
            let pid = wait_for_pid(&self.dir.path().join("pid"));
            // SAFETY: signal zero only tests whether this synthetic child still exists.
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        }
    }

    fn subscription() -> Value {
        json!({"account":{"type":"chatgpt","email":"synthetic@example.invalid","planType":"plus"}})
    }

    fn window(minutes: u64, used: f64) -> Value {
        json!({"windowDurationMins":minutes,"usedPercent":used,"resetsAt":1792400000})
    }

    #[test]
    fn fake_server_exits_on_eof_without_recording_empty_requests() {
        let methods = ["initialize", "initialized", "account/read"];
        for count in (0..=methods.len()).rev() {
            let fake = Fake::new(subscription(), json!({}));
            let mut child = Command::new("/bin/sh")
                .arg(&fake.executable)
                .arg("app-server")
                .env("HOME", fake.dir.path())
                .env("CODEX_HOME", fake.dir.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            for method in &methods[..count] {
                writeln!(input, "{}", json!({"method":method})).unwrap();
            }
            // Close stdin before terminating the child to expose every EOF boundary.
            drop(input);
            let deadline = Instant::now() + Duration::from_secs(2);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let requests = fs::read_to_string(fake.dir.path().join("requests")).unwrap_or_default();
            assert!(
                requests.lines().all(|line| !line.is_empty()),
                "fake recorded an empty request after {count} requests: {requests:?}"
            );
            assert!(status.is_some_and(|status| status.success()));
            if count > 0 {
                assert_eq!(fake.methods(), methods[..count]);
            } else {
                assert!(requests.is_empty());
            }
            fake.assert_reaped();
        }
    }

    #[test]
    fn fake_server_selects_week_by_duration_in_either_position() {
        for (primary, secondary) in [
            (window(300, 12.0), window(10080, 61.0)),
            (window(10080, 61.0), window(300, 12.0)),
        ] {
            let fake = Fake::new(
                subscription(),
                json!({"rateLimits":{"primary":primary,"secondary":secondary}}),
            );
            assert_eq!(
                fake.read(Duration::from_secs(2)).unwrap(),
                Some(Weekly {
                    used_percent: 61.0,
                    resets_at: Some(UNIX_EPOCH + Duration::from_secs(1792400000)),
                })
            );
            assert_eq!(
                fake.methods(),
                [
                    "initialize",
                    "initialized",
                    "account/read",
                    "account/rateLimits/read"
                ]
            );
            fake.assert_reaped();
        }
    }

    #[test]
    fn api_key_and_signed_out_accounts_do_not_request_limits() {
        for account in [
            json!({"account":{"type":"apiKey"}}),
            json!({"account":null}),
        ] {
            let fake = Fake::new(account, json!({}));
            assert_eq!(fake.read(Duration::from_secs(2)).unwrap(), None);
            let now = Instant::now();
            let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
            let sample = Some(Weekly {
                used_percent: 12.0,
                resets_at: Some(wall + Duration::from_secs(1000)),
            });
            let mut client = WeeklyClient::new(
                fake.dir.path().to_owned(),
                MIN_REFRESH_INTERVAL,
                DEFAULT_TIMEOUT,
            );
            assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
            assert_eq!(
                client.refresh_with(now + MIN_REFRESH_INTERVAL, wall, || {
                    fake.read(Duration::from_secs(2))
                }),
                None
            );
            assert_eq!(
                fake.methods(),
                [
                    "initialize",
                    "initialized",
                    "account/read",
                    "initialize",
                    "initialized",
                    "account/read"
                ]
            );
            fake.assert_reaped();
        }
    }

    #[test]
    fn fake_server_rejects_non_weekly_and_invalid_usage() {
        for primary in [
            window(300, 12.0),
            window(43200, 12.0),
            window(10080, -1.0),
            window(10080, 101.0),
        ] {
            let fake = Fake::new(
                subscription(),
                json!({"rateLimits":{"primary":primary,"secondary":null}}),
            );
            assert_eq!(fake.read(Duration::from_secs(2)).unwrap(), None);
            fake.assert_reaped();
        }
    }

    #[test]
    fn timeout_kills_and_reaps_child_on_every_attempt() {
        let fake = Fake::new(subscription(), json!({}));
        fs::write(
            &fake.executable,
            "#!/bin/sh\necho $$ > \"$HOME/pid\"\nIFS= read -r request\nexec sleep 30\n",
        )
        .unwrap();
        for _ in 0..3 {
            let start = Instant::now();
            assert_eq!(
                fake.read(Duration::from_millis(150)).unwrap_err().kind(),
                io::ErrorKind::TimedOut
            );
            assert!(start.elapsed() < Duration::from_secs(2));
            fake.assert_reaped();
        }
    }

    #[test]
    fn rpc_errors_never_include_server_content() {
        let fake = Fake::new(subscription(), json!({}));
        fs::write(&fake.executable, "#!/bin/sh\necho $$ > \"$HOME/pid\"\nIFS= read -r request\nprintf '%s\\n' '{\"id\":1,\"error\":{\"message\":\"synthetic-secret-marker\"}}'\nexec sleep 30\n").unwrap();
        let error = fake.read(Duration::from_secs(2)).unwrap_err();
        assert!(!error.to_string().contains("synthetic-secret-marker"));
        fake.assert_reaped();
    }

    #[test]
    fn refresh_is_throttled_and_transient_failure_keeps_success_until_reset() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = WeeklyClient::new(
            dir.path().to_owned(),
            Duration::ZERO,
            Duration::from_secs(2),
        );
        assert_eq!(client.interval(), MIN_REFRESH_INTERVAL);
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
        let sample = Some(Weekly {
            used_percent: 12.0,
            resets_at: Some(wall + Duration::from_secs(180)),
        });
        assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
        assert_eq!(
            client.refresh_with(
                now + Duration::from_secs(1),
                wall + Duration::from_secs(1),
                || panic!("must use cache")
            ),
            sample
        );
        assert_eq!(
            client.refresh_with(
                now + MIN_REFRESH_INTERVAL,
                wall + MIN_REFRESH_INTERVAL,
                || Err(io::Error::other("synthetic"))
            ),
            sample
        );
        assert_eq!(
            client.refresh_with(
                now + MIN_REFRESH_INTERVAL + Duration::from_secs(1),
                wall + MIN_REFRESH_INTERVAL + Duration::from_secs(1),
                || panic!("must throttle failure")
            ),
            sample
        );
        assert_eq!(
            client.refresh_with(
                now + MIN_REFRESH_INTERVAL * 2,
                wall + MIN_REFRESH_INTERVAL * 2,
                || Err(io::Error::new(io::ErrorKind::TimedOut, "synthetic"))
            ),
            sample
        );
        assert_eq!(
            client.refresh_with(
                now + Duration::from_secs(179),
                wall + Duration::from_secs(180),
                || panic!("must use interval")
            ),
            None
        );
    }

    #[test]
    fn authentication_failure_or_absent_subscription_clears_cache() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
        let sample = Some(Weekly {
            used_percent: 12.0,
            resets_at: Some(wall + Duration::from_secs(1000)),
        });
        for failure in [Err(authentication_error()), Ok(None)] {
            let mut client =
                WeeklyClient::new(dir.path().to_owned(), MIN_REFRESH_INTERVAL, DEFAULT_TIMEOUT);
            assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
            assert_eq!(
                client.refresh_with(
                    now + MIN_REFRESH_INTERVAL,
                    wall + MIN_REFRESH_INTERVAL,
                    || failure
                ),
                None
            );
            assert_eq!(
                client.refresh_with(
                    now + MIN_REFRESH_INTERVAL + Duration::from_secs(1),
                    wall,
                    || panic!("must throttle")
                ),
                None
            );
        }
    }

    #[test]
    fn unknown_reset_or_no_success_cannot_survive_transient_failure() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
        let mut client = WeeklyClient::new(
            dir.path().to_owned(),
            DEFAULT_REFRESH_INTERVAL,
            DEFAULT_TIMEOUT,
        );
        assert_eq!(
            client.refresh_with(now, wall, || Err(io::Error::other("synthetic"))),
            None
        );
        let sample = Some(Weekly {
            used_percent: 12.0,
            resets_at: None,
        });
        assert_eq!(
            client.refresh_with(now + DEFAULT_REFRESH_INTERVAL, wall, || Ok(sample)),
            sample
        );
        assert_eq!(
            client.refresh_with(now + DEFAULT_REFRESH_INTERVAL * 2, wall, || Err(
                io::Error::other("synthetic")
            )),
            None
        );
        assert_eq!(
            client.refresh_with(now + DEFAULT_REFRESH_INTERVAL * 3, wall, || Ok(sample)),
            sample
        );
    }

    #[test]
    fn fake_server_bounded_noise_and_partial_response_cannot_stall_or_leak() {
        let fake = Fake::new(subscription(), json!({}));
        for body in [
            "printf '%s' '{\"id\":1'; exec sleep 30",
            "while :; do printf '%s\\n' '{\"method\":\"syntheticNotification\"}'; done",
            "while :; do printf '%s' 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done",
            "printf '%s\\n' 'not-json'; exec sleep 30",
            "exit 1",
        ] {
            fs::write(
                &fake.executable,
                format!("#!/bin/sh\necho $$ > \"$HOME/pid\"\nIFS= read -r request\n{body}\n"),
            )
            .unwrap();
            let start = Instant::now();
            assert!(fake.read(Duration::from_millis(150)).is_err());
            assert!(start.elapsed() < Duration::from_secs(2));
            fake.assert_reaped();
        }
    }

    #[test]
    fn account_read_errors_are_permanent_and_rate_limit_auth_errors_are_sanitized() {
        use crate::doctor::WeeklyResult;

        let fake = Fake::new(
            subscription(),
            json!({"rateLimits":{"primary":window(10080, 20.0)}}),
        );
        let original = fs::read_to_string(&fake.executable).unwrap();
        let account_reply = json!({"id":2,"result":subscription()}).to_string();
        let limits_reply =
            json!({"id":3,"result":{"rateLimits":{"primary":window(10080, 20.0)}}}).to_string();
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
        let sample = Some(Weekly {
            used_percent: 12.0,
            resets_at: Some(wall + Duration::from_secs(1000)),
        });
        for (reply, code) in [
            (&account_reply, 500),
            (&limits_reply, 401),
            (&limits_reply, 403),
        ] {
            let replacement = json!({"id":if reply == &account_reply {2} else {3}, "error":{"code":code,"message":"synthetic-private-marker"}}).to_string();
            fs::write(&fake.executable, original.replace(reply, &replacement)).unwrap();
            let error = fake.read(Duration::from_secs(2)).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(!error.to_string().contains("synthetic-private-marker"));
            let mut client = WeeklyClient::new(
                fake.dir.path().to_owned(),
                MIN_REFRESH_INTERVAL,
                DEFAULT_TIMEOUT,
            );
            assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
            assert_eq!(
                client.refresh_with(now + MIN_REFRESH_INTERVAL, wall, || {
                    fake.read(Duration::from_secs(2))
                }),
                None
            );
            assert_eq!(client.last_result(), WeeklyResult::Unauthorized);
            fake.assert_reaped();
        }
        let replacement =
            json!({"id":3,"error":{"code":500,"message":"synthetic-private-marker"}}).to_string();
        fs::write(
            &fake.executable,
            original.replace(&limits_reply, &replacement),
        )
        .unwrap();
        assert_eq!(
            fake.read(Duration::from_secs(2)).unwrap_err().kind(),
            io::ErrorKind::Other
        );
        fake.assert_reaped();
    }

    #[test]
    fn account_read_transport_failures_retain_cache_only_until_reset() {
        use crate::doctor::WeeklyResult;

        let fake = Fake::new(subscription(), json!({}));
        let original = fs::read_to_string(&fake.executable).unwrap();
        let account_reply = json!({"id":2,"result":subscription()}).to_string();
        let account_line = format!("printf '%s\\n' '{account_reply}'");
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(1792000000);
        let sample = Some(Weekly {
            used_percent: 12.0,
            resets_at: Some(wall + Duration::from_secs(180)),
        });
        for (reply, expected_kind) in [
            ("exec sleep 30", io::ErrorKind::TimedOut),
            ("printf '%s\\n' 'not-json'", io::ErrorKind::Other),
            ("exit 1", io::ErrorKind::Other),
            (
                "printf '%s\\n' '{\"id\":2,\"result\":{}}'",
                io::ErrorKind::Other,
            ),
        ] {
            fs::write(&fake.executable, original.replace(&account_line, reply)).unwrap();
            let mut client = WeeklyClient::new(
                fake.dir.path().to_owned(),
                MIN_REFRESH_INTERVAL,
                Duration::from_millis(150),
            );
            assert_eq!(client.refresh_with(now, wall, || Ok(sample)), sample);
            assert_eq!(
                client.refresh_with(
                    now + MIN_REFRESH_INTERVAL,
                    wall + MIN_REFRESH_INTERVAL,
                    || {
                        let error = fake.read(Duration::from_millis(150)).unwrap_err();
                        assert_eq!(error.kind(), expected_kind);
                        Err(error)
                    }
                ),
                sample
            );
            assert_eq!(client.last_result(), WeeklyResult::Transient);
            fake.assert_reaped();
            assert_eq!(
                client.refresh_with(
                    now + MIN_REFRESH_INTERVAL + Duration::from_secs(1),
                    wall + Duration::from_secs(180),
                    || panic!("must use cache until the next query")
                ),
                None
            );
        }
    }

    #[test]
    fn fake_server_preserves_missing_reset_and_selects_only_codex_bucket() {
        let fake = Fake::new(
            subscription(),
            json!({"rateLimits":null,"rateLimitsByLimitId":{
                "codex":{"primary":{"windowDurationMins":10080,"usedPercent":0}},
                "unrelated":{"primary":window(10080, 90.0)}
            }}),
        );
        assert_eq!(
            fake.read(Duration::from_secs(2)).unwrap(),
            Some(Weekly {
                used_percent: 0.0,
                resets_at: None
            })
        );
        fake.assert_reaped();
    }

    #[test]
    fn request_options_are_bounded_and_invalid_options_never_spawn() {
        let fake = Fake::new(subscription(), json!({}));
        assert_eq!(
            fake.read(Duration::ZERO).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!fake.dir.path().join("pid").exists());
        let mut command = Command::new("/bin/sh");
        command.arg(&fake.executable);
        assert_eq!(
            read_with_command(Path::new("relative"), DEFAULT_TIMEOUT, command)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!fake.dir.path().join("pid").exists());
        let client = WeeklyClient::new(fake.dir.path().to_owned(), Duration::MAX, Duration::MAX);
        assert_eq!(client.interval(), MAX_REFRESH_INTERVAL);
        assert_eq!(client.timeout, MAX_TIMEOUT);
    }
}
