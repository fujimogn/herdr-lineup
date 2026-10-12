//! Detached session-local daemon lifecycle and replaceable publication hooks.

use crate::{
    config::Config,
    doctor,
    lock::{RuntimePaths, SessionLock},
    socket::{EndpointIdentity, Transport, read_frame},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufReader, Read, Write},
    os::unix::{
        fs::OpenOptionsExt,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const PLUGIN_ID: &str = "herdr-lineup";
const START_DEADLINE: Duration = Duration::from_secs(5);
const STOP_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const LOCK_CONTENDED_EXIT_CODE: u8 = 75;

#[derive(Debug)]
pub(crate) struct StartupLockContention;
impl std::fmt::Display for StartupLockContention {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("another daemon owns this session lock")
    }
}
impl std::error::Error for StartupLockContention {}

fn startup_lock_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::WouldBlock {
        io::Error::new(io::ErrorKind::WouldBlock, StartupLockContention)
    } else {
        error
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub socket: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
}
impl Options {
    pub fn from_env() -> io::Result<Self> {
        let required = |key| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or_else(|| io::Error::other(format!("{key} must be an absolute path")))
        };
        Ok(Self {
            socket: required("HERDR_SOCKET_PATH")?,
            state: required("HERDR_PLUGIN_STATE_DIR")?,
            config: required("HERDR_PLUGIN_CONFIG_DIR")?,
        })
    }
}

/// Implementations must bound I/O and clear only plugin-owned metadata.
pub trait Publisher {
    fn refresh(&mut self, transport: &mut Transport, config: &Config) -> io::Result<()>;
    fn clear(&mut self, transport: &mut Transport) -> io::Result<()>;
    /// Drop any cached pane associations before authoritative reconciliation.
    fn reconnected(&mut self) {}
}
pub struct NoopPublisher;
impl Publisher for NoopPublisher {
    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
        Ok(())
    }
    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub plugin: String,
    pub socket_path: PathBuf,
    pub socket_key: String,
    pub endpoint: EndpointIdentity,
    pub nonce: String,
    pub pid: u32,
    pub last_refresh_ms: Option<u64>,
    pub refresh_failed: bool,
}

pub fn ensure(options: &Options) -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("daemon");
    ensure_command(options, command)
}
fn ensure_command(options: &Options, mut command: Command) -> io::Result<()> {
    Config::load(&options.config)?;
    let endpoint = EndpointIdentity::read(&options.socket)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let deadline = Instant::now() + START_DEADLINE;
    let mut may_spawn = true;
    loop {
        if may_spawn && let Some(lock) = SessionLock::acquire(&paths)? {
            // A reused PID record is never inspected or signalled. Stale control
            // paths are removed only after exclusive session ownership is acquired.
            let log = OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&paths.log)?;
            command
                .env("HERDR_SOCKET_PATH", &options.socket)
                .env("HERDR_PLUGIN_STATE_DIR", &options.state)
                .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
                .env("LINEUP_INHERITED_LOCK", "3")
                .env("LINEUP_EXPECTED_DEVICE", endpoint.device.to_string())
                .env("LINEUP_EXPECTED_INODE", endpoint.inode.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log));
            let descriptor = lock.raw_fd();
            // SAFETY: after fork, only async-signal-safe libc functions execute.
            // dup2 preserves flock's shared open-file description across exec.
            unsafe {
                command.pre_exec(move || {
                    if libc::setsid() < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if descriptor != 3 && libc::dup2(descriptor, 3) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn()?;
            // Close the parent's descriptor; never issue LOCK_UN on a transfer.
            drop(lock);
            loop {
                if let Some(health) = probe_starting(&paths, &options.socket)? {
                    if health.endpoint == endpoint {
                        return Ok(());
                    }
                    return Err(io::Error::other("daemon endpoint identity mismatch"));
                }
                if let Some(status) = child.try_wait()? {
                    if status.code() == Some(i32::from(LOCK_CONTENDED_EXIT_CODE)) {
                        // Only the typed lock-acquisition failure uses this
                        // status. Wait for the winner; do not spawn another
                        // child or reuse pre_exec's transferred descriptor.
                        may_spawn = false;
                        break;
                    }
                    return Err(io::Error::other(
                        "daemon exited before readiness; inspect session daemon.log",
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other(
                        "daemon startup timed out; inspect session daemon.log",
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        if let Some(health) = probe_starting(&paths, &options.socket)? {
            if health.endpoint == endpoint {
                return Ok(());
            }
            return Err(io::Error::other(
                "live daemon belongs to a replaced endpoint",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "live lock has no healthy control endpoint within startup deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn foreground(options: &Options) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let mut publisher = crate::publish::MetadataPublisher::new(
        paths.root,
        paths.key,
        crate::publish::Homes::from_env(),
    )?;
    run_foreground(
        options,
        std::env::var_os("LINEUP_INHERITED_LOCK").as_deref() == Some(std::ffi::OsStr::new("3")),
        &mut publisher,
    )
}
pub fn run_foreground(
    options: &Options,
    inherited: bool,
    publisher: &mut impl Publisher,
) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let _lock = if inherited {
        SessionLock::inherited(&paths).map_err(startup_lock_error)?
    } else {
        SessionLock::acquire(&paths)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, StartupLockContention))?
    };
    let mut transport = Transport::connect(&options.socket)?;
    if inherited {
        let parse = |key| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("missing inherited endpoint identity"))
        };
        let expected = EndpointIdentity {
            device: parse("LINEUP_EXPECTED_DEVICE")?,
            inode: parse("LINEUP_EXPECTED_INODE")?,
        };
        if transport.identity() != expected {
            return Err(io::Error::other("socket replaced during detached startup"));
        }
    }
    doctor::check_transport(&mut transport)?;
    let mut log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&paths.log)?;
    remove_if_present(&paths.control)?;
    let listener = UnixListener::bind(&paths.control)?;
    listener.set_nonblocking(true)?;
    let _cleanup = RuntimeCleanup {
        paths: paths.clone(),
    };
    let mut random = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut health = Health {
        plugin: PLUGIN_ID.into(),
        socket_path: options.socket.clone(),
        socket_key: paths.key.clone(),
        endpoint: transport.identity(),
        nonce,
        pid: std::process::id(),
        last_refresh_ms: None,
        refresh_failed: false,
    };
    fs::write(&paths.pid, format!("{}\n", health.pid))?;
    let started = Instant::now();
    let mut refresh_at = Instant::now();
    let mut reconnect = false;
    loop {
        if reconnect || transport.check_connection().is_err() {
            // Transport treats v0.9.3 EOF after a response as normal and opens
            // each next request on the pinned dev/inode. This recovery path is
            // for unexpected failures; a replaced server socket stops us.
            let retrying_refresh = reconnect;
            transport.reconnect_same_endpoint()?;
            doctor::check_transport(&mut transport)?;
            publisher.reconnected();
            reconnect = false;
            if !retrying_refresh {
                refresh_at = Instant::now();
            }
        }
        if Instant::now() >= refresh_at {
            match publisher.refresh(&mut transport, &config) {
                Ok(()) => {
                    health.last_refresh_ms =
                        Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                    health.refresh_failed = false;
                }
                Err(error) => {
                    health.refresh_failed = true;
                    reconnect = error.kind() != io::ErrorKind::InvalidInput;
                    if error.kind() == io::ErrorKind::InvalidInput {
                        let _ = writeln!(
                            log,
                            "Lineup metadata capacity exhausted; affected resources skipped"
                        );
                    }
                    let _ = writeln!(
                        log,
                        "Lineup refresh failed; metadata will expire unless renewed"
                    );
                }
            }
            refresh_at = Instant::now() + Duration::from_millis(config.refresh_interval_ms);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if serve_control(stream, &health)? {
                    // Never reconnect or clear against a replacement endpoint.
                    if transport.check_connection().is_ok() {
                        publisher.clear(&mut transport)?;
                    }
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
            Err(error) => return Err(error),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn remove_if_present(path: &std::path::Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
struct RuntimeCleanup {
    paths: RuntimePaths,
}
impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        let _ = remove_if_present(&self.paths.control);
        let _ = remove_if_present(&self.paths.pid);
    }
}
fn control_stream(paths: &RuntimePaths) -> io::Result<Option<BufReader<UnixStream>>> {
    match crate::socket::connect_stream(&paths.control) {
        Ok(stream) => {
            stream.set_read_timeout(Some(Duration::from_millis(500)))?;
            stream.set_write_timeout(Some(Duration::from_millis(500)))?;
            Ok(Some(BufReader::new(stream)))
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn read_identity(
    stream: &mut BufReader<UnixStream>,
    paths: &RuntimePaths,
    socket: &std::path::Path,
) -> io::Result<Health> {
    stream.get_mut().write_all(b"health\n")?;
    let health: Health = serde_json::from_slice(&read_frame(stream, 8192)?)
        .map_err(|_| io::Error::other("invalid daemon control identity"))?;
    if health.plugin != PLUGIN_ID
        || health.socket_key != paths.key
        || health.socket_path != socket
        || health.nonce.len() != 64
        || !health.nonce.bytes().all(|c| c.is_ascii_hexdigit())
        || !SessionLock::is_held(paths)?
    {
        return Err(io::Error::other(
            "daemon control identity or live lock mismatch",
        ));
    }
    if let Ok(current) = EndpointIdentity::read(socket)
        && current != health.endpoint
    {
        return Err(io::Error::other(
            "daemon belongs to a replaced socket endpoint",
        ));
    }
    Ok(health)
}
pub fn probe(paths: &RuntimePaths, socket: &std::path::Path) -> io::Result<Option<Health>> {
    let Some(mut stream) = control_stream(paths)? else {
        return Ok(None);
    };
    read_identity(&mut stream, paths, socket).map(Some)
}
fn probe_starting(paths: &RuntimePaths, socket: &std::path::Path) -> io::Result<Option<Health>> {
    match probe(paths, socket) {
        // The serial handler may be busy with another probe/stop or refresh.
        // A single frame deadline is not the five-second startup deadline.
        // Retry only transient transport failures, never invalid identities.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut
                    | io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
            ) =>
        {
            Ok(None)
        }
        result => result,
    }
}
pub fn stop(options: &Options) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let deadline = Instant::now() + STOP_DEADLINE;
    let Some(mut stream) = control_stream(&paths)? else {
        return if SessionLock::is_held(&paths)? {
            Err(io::Error::other(
                "session lock is live but control is unavailable; no signal sent",
            ))
        } else {
            Ok(())
        };
    };
    let identity = read_identity(&mut stream, &paths, &options.socket)?;
    let request = serde_json::json!({"stop":identity.nonce,"plugin":PLUGIN_ID,"socket_key":paths.key,"endpoint":identity.endpoint});
    serde_json::to_writer(stream.get_mut(), &request)?;
    stream.get_mut().write_all(b"\n")?;
    if read_frame(&mut stream, 8192)? != b"stopping" {
        return Err(io::Error::other("daemon refused shutdown identity"));
    }
    loop {
        if !SessionLock::is_held(&paths)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "daemon did not stop within five seconds; no signal sent",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn serve_control(stream: UnixStream, health: &Health) -> io::Result<bool> {
    // BSD/macOS accept inherits O_NONBLOCK from our listener. A timeout does
    // not clear that flag; explicitly block so the identity/stop exchange waits
    // for the next frame within its deadline instead of closing on WouldBlock.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let mut reader = BufReader::new(stream);
    // Broken or slow clients cannot terminate the daemon.
    let Ok(frame) = read_frame(&mut reader, 8192) else {
        return Ok(false);
    };
    if frame != b"health" {
        return Ok(false);
    }
    if serde_json::to_writer(reader.get_mut(), health).is_err()
        || reader.get_mut().write_all(b"\n").is_err()
    {
        return Ok(false);
    }
    let Ok(frame) = read_frame(&mut reader, 8192) else {
        return Ok(false);
    };
    let Ok(request) = serde_json::from_slice::<serde_json::Value>(&frame) else {
        return Ok(false);
    };
    if request.get("stop").and_then(|v| v.as_str()) != Some(&health.nonce)
        || request.get("plugin").and_then(|v| v.as_str()) != Some(PLUGIN_ID)
        || request.get("socket_key").and_then(|v| v.as_str()) != Some(&health.socket_key)
        || request.get("endpoint") != Some(&serde_json::to_value(health.endpoint)?)
    {
        return Ok(false);
    }
    let _ = reader.get_mut().write_all(b"stopping\n");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn fixture() -> (
        tempfile::TempDir,
        Options,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let options = Options {
            socket: dir.path().join("herdr.sock"),
            state: dir.path().join("state"),
            config: dir.path().join("config"),
        };
        std::fs::create_dir(&options.config).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&options.socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        std::io::BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        stream.write_all(b"{\"id\":\"lineup-health\",\"result\":{\"type\":\"pong\",\"version\":\"0.9.3\",\"protocol\":17}}\n").unwrap();
                        // Herdr v0.9.3 ends each ordinary JSON response with EOF.
                        drop(stream);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        (dir, options, done, handle)
    }
    #[test]
    fn health_fixture_closes_after_one_json_response() {
        let (_dir, options, done, server) = fixture();
        let mut stream = std::os::unix::net::UnixStream::connect(&options.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .write_all(b"{\"id\":\"lineup-health\",\"method\":\"ping\",\"params\":{}}\n")
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        crate::doctor::validate_pong(response.trim_end().as_bytes()).unwrap();
        response.clear();
        let eof = reader.read_line(&mut response);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(eof.unwrap(), 0, "ordinary JSON responses must end in EOF");
    }

    fn child_command() -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "daemon::tests::child_entry",
            "--ignored",
            "--nocapture",
        ]);
        command
            .env_clear()
            .env("HOME", "/nonexistent-lineup-test-home");
        command
    }
    #[test]
    #[ignore = "isolated daemon subprocess entry point"]
    fn child_entry() {
        if std::env::var_os("LINEUP_INHERITED_LOCK").is_some() {
            let options = Options::from_env().unwrap();
            if std::env::var_os("LINEUP_TEST_FAIL_STARTUP").is_some() {
                let error = io::Error::new(io::ErrorKind::WouldBlock, "synthetic socket failure");
                std::process::exit(i32::from(crate::failure_exit_code(&error)));
            }
            if std::env::var_os("LINEUP_TEST_CONTEND").is_some() {
                // Reproduce a failed inherited-lock claim after a competitor
                // wins the session. Every handshake is a bounded condition wait.
                assert_eq!(unsafe { libc::close(3) }, 0);
                fs::write(
                    options.state.join("released"),
                    std::process::id().to_string(),
                )
                .unwrap();
                wait_until(|| options.state.join("winner-locked").exists());
                // The path still matches, but this independent description
                // cannot inherit ownership once the winner holds the flock.
                let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&paths.lock)
                    .unwrap();
                use std::os::fd::IntoRawFd;
                let descriptor = file.into_raw_fd();
                if descriptor != 3 {
                    assert_eq!(unsafe { libc::dup2(descriptor, 3) }, 3);
                    assert_eq!(unsafe { libc::close(descriptor) }, 0);
                }
                let error = run_foreground(&options, true, &mut NoopPublisher).unwrap_err();
                fs::write(options.state.join("contended"), error.to_string()).unwrap();
                std::process::exit(i32::from(crate::failure_exit_code(&error)));
            }
            run_foreground(&options, true, &mut NoopPublisher).unwrap();
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + START_DEADLINE;
        while !condition() {
            assert!(Instant::now() < deadline, "test handshake timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn ensure_waits_for_winner_after_its_child_loses_the_session_lock() {
        let (_dir, options, done, server) = fixture();
        let winning_options = options.clone();
        let winner = std::thread::spawn(move || {
            wait_until(|| winning_options.state.join("released").exists());
            let paths = RuntimePaths::new(&winning_options.state, &winning_options.socket).unwrap();
            let mut lock = None;
            wait_until(|| {
                lock = SessionLock::acquire(&paths).unwrap();
                lock.is_some()
            });
            fs::write(winning_options.state.join("winner-locked"), "").unwrap();
            wait_until(|| winning_options.state.join("contended").exists());
            let pid: i32 = fs::read_to_string(winning_options.state.join("released"))
                .unwrap()
                .parse()
                .unwrap();
            // Readiness is withheld until ensure has reaped the losing child.
            // Thus success cannot bypass the child-exit branch under test.
            wait_until(|| unsafe { libc::kill(pid, 0) } < 0
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
            drop(lock);
            run_foreground(&winning_options, false, &mut NoopPublisher).unwrap();
        });
        let mut command = child_command();
        command.env("LINEUP_TEST_CONTEND", "1");
        let result = ensure_command(&options, command);
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        stop(&options).unwrap();
        winner.join().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            result.is_ok(),
            "losing ensure rejected the winner: {result:?}"
        );
    }

    #[test]
    fn ensure_rejects_a_child_socket_failure_instead_of_waiting_for_a_winner() {
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_FAIL_STARTUP", "1");
        let error = ensure_command(&options, command).unwrap_err();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            error
                .to_string()
                .starts_with("daemon exited before readiness")
        );
    }
    #[test]
    fn ensure_waits_for_a_busy_control_endpoint_within_startup_deadline() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        // A client preparing stop can occupy the serial control handler for
        // its entire frame deadline. Ensure must still wait for readiness.
        let mut pending_stop = control_stream(&paths).unwrap().unwrap();
        read_identity(&mut pending_stop, &paths, &options.socket).unwrap();
        let result = ensure_command(&options, child_command());
        drop(pending_stop);
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            result.is_ok(),
            "busy healthy daemon rejected ensure: {result:?}"
        );
    }

    #[test]
    fn parallel_ensure_starts_one_daemon_and_stop_ignores_pid_record() {
        let (_dir, options, done, server) = fixture();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let options = options.clone();
                std::thread::spawn(move || ensure_command(&options, child_command()).unwrap())
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let identity = probe(&paths, &options.socket).unwrap().unwrap();
        assert!(SessionLock::acquire(&paths).unwrap().is_none());
        std::fs::write(&paths.pid, "1").unwrap();
        stop(&options).unwrap();
        assert!(SessionLock::acquire(&paths).unwrap().is_some());
        assert!(probe(&paths, &options.socket).unwrap().is_none());
        assert!(!identity.nonce.is_empty());
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn socket_replacement_stops_daemon_without_connecting_to_replacement() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        std::fs::remove_file(&options.socket).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&options.socket).unwrap();
        replacement.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if SessionLock::acquire(&paths).unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(replacement.accept().is_err());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn stopped_daemon_with_unrelated_pid_is_noop_and_doctor_does_not_create_state() {
        let (_dir, options, done, server) = fixture();
        assert!(doctor::run(&options).is_err());
        assert!(!options.state.exists());
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        paths.create().unwrap();
        std::fs::write(&paths.pid, "1").unwrap();
        stop(&options).unwrap();
        assert_eq!(std::fs::read_to_string(&paths.pid).unwrap(), "1");
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn inherited_nonblocking_control_stream_waits_for_shutdown_after_identity() {
        let health = Health {
            plugin: PLUGIN_ID.into(),
            socket_path: PathBuf::from("/isolated/herdr.sock"),
            socket_key: "isolated".into(),
            endpoint: EndpointIdentity {
                device: 1,
                inode: 2,
            },
            nonce: "a".repeat(64),
            pid: 1,
            last_refresh_ms: Some(0),
            refresh_failed: false,
        };
        let (mut client, server) = UnixStream::pair().unwrap();
        // BSD/macOS accept inherits the listener's O_NONBLOCK flag. Explicitly
        // reproduce that state on Linux and queue health before starting I/O.
        server.set_nonblocking(true).unwrap();
        client.write_all(b"health\n").unwrap();
        let server_health = health.clone();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = serve_control(server, &server_health).unwrap();
            completed_tx.send(result).unwrap();
        });
        let mut client = BufReader::new(client);
        let identity: Health =
            serde_json::from_slice(&read_frame(&mut client, 8192).unwrap()).unwrap();
        assert_eq!(identity.nonce, health.nonce);
        // Wait for the server's completion signal, rather than racing a write
        // against close. A valid exchange must remain open while we prepare stop.
        let early_completion = completed_rx.recv_timeout(Duration::from_millis(50));
        let write_after_early_close = early_completion
            .as_ref()
            .ok()
            .map(|_| client.get_mut().write_all(b"{\"stop\":\"pending\"}\n"));
        assert!(
            matches!(
                early_completion,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "control closed after identity before shutdown was sent: {early_completion:?}, write after close: {write_after_early_close:?}"
        );
        let request = serde_json::json!({"stop": identity.nonce, "plugin": PLUGIN_ID, "socket_key": identity.socket_key, "endpoint": identity.endpoint});
        crate::socket::write_frame(client.get_mut(), &serde_json::to_vec(&request).unwrap())
            .unwrap();
        assert_eq!(read_frame(&mut client, 8192).unwrap(), b"stopping");
        assert!(completed_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn wrong_control_nonce_cannot_stop_live_daemon() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let mut stream = control_stream(&paths).unwrap().unwrap();
        let identity = read_identity(&mut stream, &paths, &options.socket).unwrap();
        stream
            .get_mut()
            .write_all(b"{\"stop\":\"forged\"}\n")
            .unwrap();
        assert!(read_frame(&mut stream, 8192).is_err());
        assert_eq!(
            probe(&paths, &options.socket).unwrap().unwrap().nonce,
            identity.nonce
        );
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn invalid_config_fails_before_runtime_state_is_created() {
        let (_dir, options, done, server) = fixture();
        std::fs::write(options.config.join("config.toml"), "unknown = 'PRIVATE'").unwrap();
        assert!(ensure_command(&options, child_command()).is_err());
        assert!(!options.state.exists());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
