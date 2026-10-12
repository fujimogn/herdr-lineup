//! Detached session-local daemon lifecycle and replaceable publication hooks.

use crate::{
    config::Config,
    doctor,
    lock::{LifecycleLock, ProcessExit, RuntimePaths, SessionLock, StartupLockContention},
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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const PLUGIN_ID: &str = "herdr-lineup";
const START_DEADLINE: Duration = Duration::from_secs(5);
const STOP_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const LOCK_CONTENDED_EXIT_CODE: u8 = 75;

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
    /// Observe shutdown between bounded reader calls and complete metadata frames.
    fn set_shutdown(&mut self, _shutdown: Arc<AtomicBool>) {}
    /// Stop owned background queries on every exit, without joining HTTP I/O.
    /// Separate provider child processes must be killed and reaped here.
    fn shutdown(&mut self) {}
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
    let _lifecycle = LifecycleLock::acquire(&paths, deadline)?;
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
                        if health.pid == child.id() {
                            return Ok(());
                        }
                        // A healthy competing daemon does not prove our child
                        // has exited. Reap the typed loser before returning.
                    } else {
                        return Err(io::Error::other("daemon endpoint identity mismatch"));
                    }
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
    let lock = if inherited {
        SessionLock::inherited(&paths)?
    } else {
        SessionLock::acquire(&paths)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, StartupLockContention))?
    };
    run_locked(options, inherited, config, paths, lock, publisher)
}

fn run_locked(
    options: &Options,
    inherited: bool,
    config: Config,
    paths: RuntimePaths,
    _lock: SessionLock,
    publisher: &mut impl Publisher,
) -> io::Result<()> {
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
    let control = ControlWorker::start(listener, health.clone());
    publisher.set_shutdown(control.shutdown.clone());
    let started = Instant::now();
    let mut refresh_at = Instant::now();
    let mut reconnect = false;
    let result = (|| {
        loop {
            if control.shutdown.load(Ordering::Acquire) {
                // Finish the current bounded operation before clearing complete frames.
                if transport.check_connection().is_ok() {
                    publisher.clear(&mut transport)?;
                }
                return Ok(());
            }
            if control
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
            {
                return Err(io::Error::other(
                    "daemon control worker exited unexpectedly",
                ));
            }
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
            *control.health.lock().unwrap() = health.clone();
            if control.shutdown.load(Ordering::Acquire) {
                continue;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    control.shutdown.store(true, Ordering::Release);
    publisher.shutdown();
    result
}
/// Own the control listener independently of publication. Drop joins the worker
/// before runtime paths are removed and session ownership is released.
struct ControlWorker {
    health: Arc<Mutex<Health>>,
    shutdown: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<io::Result<()>>>,
}
impl ControlWorker {
    fn start(listener: UnixListener, health: Health) -> Self {
        let health = Arc::new(Mutex::new(health));
        let shutdown = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let worker_health = health.clone();
        let worker_shutdown = shutdown.clone();
        let worker_done = done.clone();
        let worker = std::thread::spawn(move || {
            while !worker_done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Never hold the health mutex during I/O or a refresh.
                        let snapshot = worker_health.lock().unwrap().clone();
                        if serve_control(stream, &snapshot)? {
                            worker_shutdown.store(true, Ordering::Release);
                            return Ok(());
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                    Err(error) => return Err(error),
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        Self {
            health,
            shutdown,
            done,
            worker: Some(worker),
        }
    }
}
impl Drop for ControlWorker {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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
fn transient_control_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
    )
}
fn probe_starting(paths: &RuntimePaths, socket: &std::path::Path) -> io::Result<Option<Health>> {
    match probe(paths, socket) {
        // The serial handler may be busy with another probe/stop or refresh.
        // Retry only transient transport failures, never invalid identities.
        Err(error) if transient_control_error(&error) => Ok(None),
        result => result,
    }
}
pub fn stop(options: &Options) -> io::Result<()> {
    stop_with_lock(options, || {})
}
fn stop_with_lock(options: &Options, after_lock: impl FnOnce()) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let deadline = Instant::now() + STOP_DEADLINE;
    let _lifecycle = LifecycleLock::acquire(&paths, deadline)?;
    after_lock();
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
    verify_peer_pid(stream.get_ref(), identity.pid)?;
    let process = ProcessExit::observe(identity.pid)?;
    let request = serde_json::json!({"stop":identity.nonce,"plugin":PLUGIN_ID,"socket_key":paths.key,"endpoint":identity.endpoint});
    serde_json::to_writer(stream.get_mut(), &request)?;
    stream.get_mut().write_all(b"\n")?;
    if read_frame(&mut stream, 8192)? != b"stopping" {
        return Err(io::Error::other("daemon refused shutdown identity"));
    }
    loop {
        // Termination includes destruction after flock release. A new ensure
        // may already own the lock, so do not wait on that unrelated owner.
        if process.exited()? {
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
fn verify_peer_pid(stream: &UnixStream, expected: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    let pid = {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::addr_of_mut!(credentials).cast(),
                &mut length,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        credentials.pid
    };
    #[cfg(target_os = "macos")]
    let pid = {
        let mut pid: libc::pid_t = 0;
        let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                std::ptr::addr_of_mut!(pid).cast(),
                &mut length,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        pid
    };
    if u32::try_from(pid).ok() != Some(expected) {
        return Err(io::Error::other("daemon control peer process mismatch"));
    }
    Ok(())
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
        let workspace = options.state.clone();
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
                        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                        let result = if workspace.join("snapshot-workspace").exists()
                            && request["method"] == "session.snapshot"
                        {
                            serde_json::json!({"type":"session_snapshot", "snapshot":{
                                "version":"0.9.3", "protocol":17, "panes":[],
                                "workspaces":[{"workspace_id":"fake", "worktree":{
                                    "checkout_path":workspace
                                }}]
                            }})
                        } else if request["method"] == "workspace.report_metadata" {
                            serde_json::json!({"type":"ok"})
                        } else {
                            serde_json::json!({"type":"pong", "version":"0.9.3", "protocol":17})
                        };
                        writeln!(
                            stream,
                            "{}",
                            serde_json::json!({"id":request["id"], "result":result})
                        )
                        .unwrap();
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
        if std::env::var_os("LINEUP_INHERITED_LOCK").is_some()
            || std::env::var_os("LINEUP_TEST_FOREGROUND").is_some()
        {
            let options = Options::from_env().unwrap();
            if std::env::var_os("LINEUP_TEST_RECORD_START").is_some() {
                let mut record = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(options.state.join("starts"))
                    .unwrap();
                writeln!(record, "{}", std::process::id()).unwrap();
            }
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
                std::thread::sleep(Duration::from_millis(800));
                std::process::exit(i32::from(crate::failure_exit_code(&error)));
            }
            if std::env::var_os("LINEUP_TEST_QUOTA_STOP").is_some() {
                struct QuotaPublisher {
                    home: PathBuf,
                    control: Arc<crate::quota_codex::QueryControl>,
                    workers: Vec<crate::model::WeeklyWorker>,
                }
                impl Publisher for QuotaPublisher {
                    fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
                        let mut client = crate::quota_codex::WeeklyClient::new(
                            self.home.clone(),
                            Duration::from_secs(60),
                            Duration::from_secs(30),
                        );
                        let control = self.control.clone();
                        self.workers.push(
                            crate::model::WeeklyWorker::start(
                                shutdown.clone(),
                                Duration::from_secs(60),
                                move || {
                                    client.refresh_controlled(&control);
                                    None
                                },
                            )
                            .unwrap(),
                        );
                        let home = self.home.clone();
                        self.workers.push(
                            crate::model::WeeklyWorker::start(
                                shutdown,
                                Duration::from_secs(60),
                                move || {
                                    fs::write(home.join("http-entered"), "").unwrap();
                                    std::thread::sleep(Duration::from_secs(30));
                                    None
                                },
                            )
                            .unwrap(),
                        );
                    }
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        Ok(())
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        Ok(())
                    }
                    fn shutdown(&mut self) {
                        self.control.stop();
                        self.workers.clear();
                    }
                }
                run_foreground(
                    &options,
                    std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                    &mut QuotaPublisher {
                        home: options.state.clone(),
                        control: Arc::new(crate::quota_codex::QueryControl::default()),
                        workers: Vec::new(),
                    },
                )
                .unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_QUOTA_ONCE").is_some() {
                let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
                let mut publisher = crate::publish::MetadataPublisher::new(
                    paths.root,
                    paths.key,
                    crate::publish::Homes::from_env(),
                )
                .unwrap();
                run_foreground(
                    &options,
                    std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                    &mut publisher,
                )
                .unwrap();
                return;
            }
            struct SlowRefresh(PathBuf, Option<Arc<AtomicBool>>);
            impl Drop for SlowRefresh {
                fn drop(&mut self) {
                    if std::env::var_os("LINEUP_TEST_LATE_DROP").is_some() {
                        let paths =
                            RuntimePaths::new(&self.0, &Options::from_env().unwrap().socket)
                                .unwrap();
                        assert!(!SessionLock::is_held(&paths).unwrap());
                        fs::write(self.0.join("drop-entered"), "").unwrap();
                        let delay = std::env::var("LINEUP_TEST_LATE_DROP_MS")
                            .ok()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(800);
                        std::thread::sleep(Duration::from_millis(delay));
                        fs::write(self.0.join("drop-completed"), "").unwrap();
                    }
                }
            }
            impl Publisher for SlowRefresh {
                fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
                    self.1 = Some(shutdown);
                }
                fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                    if self.0.join("slow-refresh").exists() {
                        fs::remove_file(self.0.join("slow-refresh"))?;
                        fs::write(self.0.join("refresh-entered"), "")?;
                        let deadline = Instant::now() + Duration::from_secs(6);
                        while Instant::now() < deadline {
                            if self
                                .1
                                .as_ref()
                                .is_some_and(|flag| flag.load(Ordering::Acquire))
                            {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                    Ok(())
                }
                fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                    fs::write(self.0.join("clear-called"), "")
                }
            }
            run_foreground(
                &options,
                std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                &mut SlowRefresh(options.state.clone(), None),
            )
            .unwrap();
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + START_DEADLINE;
        while !condition() {
            assert!(Instant::now() < deadline, "test handshake timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_pid(path: &std::path::Path) -> i32 {
        let mut pid = None;
        wait_until(|| {
            pid = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
                .filter(|pid| *pid > 0);
            pid.is_some()
        });
        pid.unwrap()
    }

    #[test]
    fn ensure_during_pending_stop_starts_exactly_one_successor() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let old = probe(&paths, &options.socket).unwrap().unwrap();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let stop_options = options.clone();
        let stopper = std::thread::spawn(move || {
            stop_with_lock(&stop_options, || {
                locked_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            })
        });
        locked_rx.recv_timeout(START_DEADLINE).unwrap();
        // Pause stop before connecting so ensures can probe the old daemon.
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let options = options.clone();
                std::thread::spawn(move || {
                    let mut command = child_command();
                    command.env("LINEUP_TEST_RECORD_START", "1");
                    ensure_command(&options, command)
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        resume_tx.send(()).unwrap();
        let stopped = stopper.join().unwrap();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let successor = probe(&paths, &options.socket).ok().flatten();
        let starts = fs::read_to_string(options.state.join("starts")).unwrap_or_default();
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        stopped.unwrap();
        for result in results {
            result.unwrap();
        }
        let successor = successor.expect("ensure returned but pending stop removed the daemon");
        assert_ne!(old.pid, successor.pid);
        assert_eq!(
            starts.lines().count(),
            1,
            "parallel ensures spawned multiple successors"
        );
        assert_eq!(starts.trim(), successor.pid.to_string());
    }

    #[test]
    fn stop_waits_for_process_after_lock_release_before_immediate_start() {
        let (_dir, options, done, server) = fixture();
        fs::create_dir(&options.state).unwrap();
        let mut old = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_LATE_DROP", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let started = Instant::now();
        let stopped = stop(&options);
        let elapsed = started.elapsed();
        let drop_completed = options.state.join("drop-completed").exists();
        let mut command = child_command();
        command.env("LINEUP_TEST_RECORD_START", "1");
        let restarted = ensure_command(&options, command);
        let new = probe(&paths, &options.socket).unwrap().unwrap();
        // Reap only the child owned by this fixture, even on a regression.
        let old_exited_at_start = old.try_wait().unwrap().is_some();
        old.wait().unwrap();
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        stopped.unwrap();
        restarted.unwrap();
        assert!(drop_completed, "stop returned before post-lock destruction");
        assert!(old_exited_at_start, "old daemon overlapped immediate start");
        assert!(elapsed >= Duration::from_millis(800));
        assert!(elapsed < STOP_DEADLINE);
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1, "ensure spawned more than once");
        assert_eq!(starts.trim(), new.pid.to_string());
    }

    #[test]
    fn stop_deadline_does_not_signal_a_process_still_tearing_down() {
        let (_dir, options, done, server) = fixture();
        fs::create_dir(&options.state).unwrap();
        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_LATE_DROP", "1")
            .env("LINEUP_TEST_LATE_DROP_MS", "10000")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let started = Instant::now();
        let stopped = stop(&options);
        let elapsed = started.elapsed();
        let still_running = child.try_wait().unwrap().is_none();
        // Test cleanup owns the Child; production stop never sends a signal.
        if still_running {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(stopped.unwrap_err().to_string().contains("no signal sent"));
        assert!(still_running);
        assert!(elapsed >= STOP_DEADLINE);
        assert!(elapsed < STOP_DEADLINE + Duration::from_millis(500));
    }

    #[test]
    fn stop_rejects_health_pid_that_does_not_match_control_peer() {
        let (_dir, options, done, server) = fixture();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let lock = SessionLock::acquire(&paths).unwrap().unwrap();
        let listener = UnixListener::bind(&paths.control).unwrap();
        let health = Health {
            plugin: PLUGIN_ID.into(),
            socket_path: options.socket.clone(),
            socket_key: paths.key.clone(),
            endpoint: EndpointIdentity::read(&options.socket).unwrap(),
            nonce: "a".repeat(64),
            pid: std::process::id() + 1,
            last_refresh_ms: None,
            refresh_failed: false,
        };
        let fake = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            assert!(!serve_control(stream, &health).unwrap());
        });
        let stopped = stop(&options);
        fake.join().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped
                .unwrap_err()
                .to_string()
                .contains("peer process mismatch")
        );
        assert!(SessionLock::is_held(&paths).unwrap());
        drop(lock);
    }

    #[test]
    fn stop_interrupts_stalled_repository_query_before_process_exit() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, options, done, server) = fixture();
        fs::write(options.config.join("config.toml"), "providers = []\n").unwrap();
        fs::create_dir(&options.state).unwrap();
        fs::write(options.state.join("snapshot-workspace"), "").unwrap();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("git");
        fs::write(
            &script,
            "#!/bin/sh\necho $$ > \"$2/git-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_QUOTA_ONCE", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .env("PATH", &bin)
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let git_pid = wait_for_pid(&options.state.join("git-pid"));
        let started = Instant::now();
        let stopped = stop(&options);
        let mut exited = None;
        while started.elapsed() < Duration::from_secs(5) {
            exited = child.try_wait().unwrap();
            if exited.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let lock_released = !SessionLock::is_held(&paths).unwrap();
        if exited.is_none() {
            // Only the exact synthetic child group owned by this test is killed.
            unsafe {
                libc::killpg(git_pid, libc::SIGKILL);
                libc::killpg(child.id() as i32, libc::SIGKILL);
            }
        }
        child.wait().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped.is_ok(),
            "stop blocked on repository query: {stopped:?}"
        );
        assert!(exited.is_some_and(|status| status.success()));
        assert!(lock_released);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(unsafe { libc::kill(git_pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn stop_waits_for_slow_quota_daemon_process_exit_and_releases_lock() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, options, done, server) = fixture();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("codex");
        fs::write(
            &script,
            "#!/bin/sh\necho $$ > \"$CODEX_HOME/app-server-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(&options.state).unwrap();
        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_QUOTA_STOP", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .env("PATH", &bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let app_server = wait_for_pid(&options.state.join("app-server-pid"));
        wait_until(|| options.state.join("http-entered").exists());
        assert!(SessionLock::acquire(&paths).unwrap().is_none());
        let started = Instant::now();
        let result = stop(&options);
        let mut exited = None;
        while started.elapsed() < Duration::from_secs(5) {
            exited = child.try_wait().unwrap();
            if exited.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Own/reap the exact child, including on a regression. PID absence alone
        // would mistake a recycled PID or an unreaped zombie for a live daemon.
        if exited.is_none() {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        let lock_released = !SessionLock::is_held(&paths).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        result.unwrap();
        assert!(exited.is_some_and(|status| status.success()));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(lock_released);
        assert_eq!(unsafe { libc::kill(app_server, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn stop_does_not_join_http_and_reaps_codex_app_server() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, options, done, server) = fixture();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("codex");
        fs::write(
            &script,
            "#!/bin/sh\n: > \"$CODEX_HOME/app-server-pid\"\n/bin/sleep 0.2\necho $$ > \"$CODEX_HOME/app-server-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = child_command();
        command.env("LINEUP_TEST_QUOTA_STOP", "1").env("PATH", &bin);
        ensure_command(&options, command).unwrap();
        let pid = wait_for_pid(&options.state.join("app-server-pid"));
        wait_until(|| options.state.join("http-entered").exists());
        let started = Instant::now();
        let result = stop(&options);
        let elapsed = started.elapsed();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        result.unwrap();
        assert!(elapsed < Duration::from_secs(5));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn daemon_quota_is_opt_in_and_repeated_refreshes_keep_one_worker() {
        use std::os::unix::fs::PermissionsExt;
        for enabled in [false, true] {
            let (_dir, options, done, server) = fixture();
            fs::write(options.config.join("config.toml"), format!("allow_network = {enabled}\nproviders = ['codex']\nrefresh_interval_ms = 1000\nreader_timeout_ms = 100\n")).unwrap();
            let bin = options.config.join("bin");
            fs::create_dir(&bin).unwrap();
            let script = bin.join("codex");
            fs::write(
                &script,
                "#!/bin/sh\nprintf 'attempt\\n' >> \"$CODEX_HOME/attempts\"\nexit 0\n",
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
            let mut command = child_command();
            command
                .env("LINEUP_TEST_QUOTA_ONCE", "1")
                .env("PATH", &bin)
                .env("CODEX_HOME", &options.state);
            ensure_command(&options, command).unwrap();
            // The fake server rejects snapshots with a pong. Each bounded
            // refresh retries/reconnects, exercising the actual startup guard.
            std::thread::sleep(Duration::from_millis(2200));
            stop(&options).unwrap();
            done.store(true, Ordering::Relaxed);
            server.join().unwrap();
            let attempts = fs::read_to_string(options.state.join("attempts")).unwrap_or_default();
            assert_eq!(attempts.lines().count(), usize::from(enabled));
        }
    }

    #[test]
    fn stop_during_refresh_then_immediate_ensure_replaces_daemon() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let old = probe(&paths, &options.socket).unwrap().unwrap();
        fs::write(options.state.join("slow-refresh"), "").unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        let stopped = stop(&options);
        let cleared = options.state.join("clear-called").exists();
        let restarted = ensure_command(&options, child_command());
        let new = probe(&paths, &options.socket).unwrap().unwrap();
        // Clean up even when the regression assertion fails.
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped.is_ok(),
            "stop failed during bounded refresh: {stopped:?}"
        );
        assert!(cleared, "shutdown skipped the publisher clear hook");
        assert!(restarted.is_ok(), "immediate ensure failed: {restarted:?}");
        assert_ne!(old.nonce, new.nonce, "ensure reused the old daemon");
        assert_ne!(old.pid, new.pid, "ensure did not spawn a new daemon");
    }

    #[test]
    fn health_responds_while_refresh_is_blocked() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        fs::write(options.state.join("slow-refresh"), "").unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        let started = Instant::now();
        let health = probe(&paths, &options.socket);
        let elapsed = started.elapsed();
        // The baseline's long refresh can outlast the first stop attempt.
        let _ = stop(&options);
        let _ = stop(&options);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(health.unwrap().is_some());
        assert!(elapsed < Duration::from_millis(500));
    }

    #[test]
    fn ensure_waits_for_winner_after_its_child_loses_the_session_lock() {
        let (_dir, options, done, server) = fixture();
        let winning_options = options.clone();
        let winner = std::thread::spawn(move || {
            wait_for_pid(&winning_options.state.join("released"));
            let paths = RuntimePaths::new(&winning_options.state, &winning_options.socket).unwrap();
            let mut lock = None;
            wait_until(|| {
                lock = SessionLock::acquire(&paths).unwrap();
                lock.is_some()
            });
            fs::write(winning_options.state.join("winner-locked"), "").unwrap();
            wait_until(|| winning_options.state.join("contended").exists());
            // The winner becomes healthy while the losing child is still in
            // its post-contention teardown. Ensure must reap that child first.
            // Preserve this exact ownership even if another unit test's fork
            // inherited a descriptor: dropping and reacquiring would race it.
            let lock = lock.unwrap();
            let descriptor = lock.raw_fd();
            let endpoint = EndpointIdentity::read(&winning_options.socket).unwrap();
            let mut command = child_command();
            command
                .env("HERDR_SOCKET_PATH", &winning_options.socket)
                .env("HERDR_PLUGIN_STATE_DIR", &winning_options.state)
                .env("HERDR_PLUGIN_CONFIG_DIR", &winning_options.config)
                .env("LINEUP_INHERITED_LOCK", "3")
                .env("LINEUP_EXPECTED_DEVICE", endpoint.device.to_string())
                .env("LINEUP_EXPECTED_INODE", endpoint.inode.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            unsafe {
                command.pre_exec(move || {
                    if descriptor != 3 && libc::dup2(descriptor, 3) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn().unwrap();
            drop(lock);
            assert!(child.wait().unwrap().success());
        });
        let mut command = child_command();
        command
            .env("LINEUP_TEST_CONTEND", "1")
            .env("LINEUP_TEST_RECORD_START", "1");
        let started = Instant::now();
        let result = ensure_command(&options, command);
        let elapsed = started.elapsed();
        let losing_pid = wait_for_pid(&options.state.join("released"));
        let loser_reaped = unsafe { libc::kill(losing_pid, 0) } < 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
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
        assert!(loser_reaped, "ensure left its losing daemon process behind");
        assert!(elapsed >= Duration::from_millis(800));
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1, "ensure respawned a losing child");
        assert_eq!(starts.trim(), losing_pid.to_string());
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
                std::thread::spawn(move || {
                    let mut command = child_command();
                    command.env("LINEUP_TEST_RECORD_START", "1");
                    ensure_command(&options, command).unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let identity = probe(&paths, &options.socket).unwrap().unwrap();
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1);
        assert_eq!(starts.trim(), identity.pid.to_string());
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
