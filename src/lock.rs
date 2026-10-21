//! Session-scoped OS lock with continuous descriptor transfer.

use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::{AsRawFd, FromRawFd},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) struct StartupLockContention;
impl std::fmt::Display for StartupLockContention {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("another daemon owns this session lock")
    }
}
impl std::error::Error for StartupLockContention {}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub lock: PathBuf,
    pub control: PathBuf,
    pub pid: PathBuf,
    pub log: PathBuf,
    pub key: String,
}
impl RuntimePaths {
    pub fn new(state: &Path, socket: &Path) -> io::Result<Self> {
        if !socket.is_absolute() || !state.is_absolute() {
            return Err(io::Error::other(
                "socket and state directory must be absolute paths",
            ));
        }
        // Stable FNV-1a hashes bind state to the lexical absolute socket path.
        let key = format!(
            "{:016x}",
            socket
                .as_os_str()
                .as_bytes()
                .iter()
                .fold(0xcbf29ce484222325u64, |hash, byte| (hash
                    ^ u64::from(*byte))
                .wrapping_mul(0x100000001b3))
        );
        let root = state.join(format!("session-{key}"));
        Ok(Self {
            lock: root.join("daemon.lock"),
            control: root.join("control.sock"),
            pid: root.join("daemon.pid"),
            log: root.join("daemon.log"),
            root,
            key,
        })
    }
    pub(crate) fn validate_control_socket_path(&self) -> io::Result<()> {
        // UnixListener::bind needs one sun_path byte for the trailing NUL.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let capacity = address.sun_path.len();
        let maximum = capacity - 1;
        let actual = self.control.as_os_str().as_bytes().len();
        if actual > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "control socket path {} is {actual} bytes; maximum is {maximum} path bytes ({capacity}-byte Unix socket limit includes a NUL terminator)",
                    self.control.display()
                ),
            ));
        }
        Ok(())
    }
    pub fn create(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let metadata = std::fs::symlink_metadata(&self.root)?;
        // Private state excludes cross-user control requests and symlink roots.
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other(
                "runtime directory must be owned by this user",
            ));
        }
        std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
    }
}
/// Closing the last shared open-file description releases flock. Never unlock
/// explicitly in the parent: that would also unlock the inherited child's fd.
pub struct SessionLock {
    file: File,
}
impl SessionLock {
    pub fn acquire(paths: &RuntimePaths) -> io::Result<Option<Self>> {
        paths.create()?;
        Self::acquire_file(&paths.lock)
    }
    fn acquire_file(path: &Path) -> io::Result<Option<Self>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
    /// Adopt only the descriptor explicitly transferred by ensure's pre_exec.
    pub fn inherited(paths: &RuntimePaths) -> io::Result<Self> {
        // SAFETY: only called by the detached entry point after ensure transfers fd 3.
        let file = unsafe { File::from_raw_fd(3) };
        let actual = file.metadata()?;
        let expected = std::fs::symlink_metadata(&paths.lock)?;
        if !actual.is_file()
            || !expected.is_file()
            || actual.dev() != expected.dev()
            || actual.ino() != expected.ino()
        {
            return Err(io::Error::other(
                "inherited lock descriptor does not match session lock",
            ));
        }
        // Only the acquisition attempt identifies contention. Metadata,
        // descriptor validation, and later setup retain their own errors.
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    StartupLockContention,
                ));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
        // Prevent later subprocesses from keeping this ownership alive.
        unsafe {
            if libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { file })
    }
    pub fn raw_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
    pub fn is_held(paths: &RuntimePaths) -> io::Result<bool> {
        // Doctor never creates files or directories to check health.
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&paths.lock)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        match file.try_lock() {
            Ok(()) => Ok(false),
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

/// Serialize lifecycle commands separately from the daemon's lifetime flock.
/// Holding this through stop's process-exit wait prevents ensure from accepting
/// the old daemon while a shutdown request is being prepared or processed.
pub(crate) struct LifecycleLock {
    _lock: SessionLock,
}
impl LifecycleLock {
    pub(crate) fn acquire(paths: &RuntimePaths, deadline: Instant) -> io::Result<Self> {
        paths.create()?;
        let path = paths.root.join("lifecycle.lock");
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "daemon lifecycle operation timed out; no signal sent",
                ));
            }
            if let Some(lock) = SessionLock::acquire_file(&path)? {
                return Ok(Self { _lock: lock });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for LifecycleLock {
    fn drop(&mut self) {
        // This lock is never deliberately transferred to a daemon. Release it
        // even if an unrelated concurrent fork inherited the descriptor.
        let _ = self._lock.file.unlock();
    }
}

/// Observe process termination through a kernel handle, without sending signals
/// or trusting the diagnostic PID file. Register before requesting shutdown so
/// later PID reuse cannot change the process being observed.
pub(crate) struct ProcessExit {
    file: File,
}
impl ProcessExit {
    pub(crate) fn observe(pid: u32) -> io::Result<Self> {
        let pid = i32::try_from(pid)
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| io::Error::other("invalid daemon process identity"))?;
        #[cfg(target_os = "linux")]
        {
            // SAFETY: pidfd_open takes scalar arguments and returns an owned fd.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                file: unsafe { File::from_raw_fd(fd as i32) },
            })
        }
        #[cfg(target_os = "macos")]
        {
            // SAFETY: kqueue returns an owned descriptor; File closes all errors.
            let fd = unsafe { libc::kqueue() };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { File::from_raw_fd(fd) };
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let event = libc::kevent {
                ident: pid as libc::uintptr_t,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
                fflags: libc::NOTE_EXIT,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            if unsafe { libc::kevent(fd, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { file })
        }
    }

    pub(crate) fn exited(&self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let mut descriptor = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // A pidfd becomes readable only after the last process thread exits.
            let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
            if result < 0 {
                let error = io::Error::last_os_error();
                return if error.kind() == io::ErrorKind::Interrupted {
                    Ok(false)
                } else {
                    Err(error)
                };
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(io::Error::other("daemon process monitor failed"));
            }
            Ok(descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0)
        }
        #[cfg(target_os = "macos")]
        {
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            let timeout = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let result = unsafe {
                libc::kevent(
                    self.file.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &mut event,
                    1,
                    &timeout,
                )
            };
            if result < 0 {
                let error = io::Error::last_os_error();
                return if error.kind() == io::ErrorKind::Interrupted {
                    Ok(false)
                } else {
                    Err(error)
                };
            }
            if result > 0 && event.flags & libc::EV_ERROR != 0 {
                return Err(io::Error::other("daemon process monitor failed"));
            }
            Ok(result > 0 && event.fflags & libc::NOTE_EXIT != 0)
        }
    }
}

/// Test fixtures that bind a control socket need a short root: the macOS
/// per-user TMPDIR (`/private/var/folders/...`) leaves no room under the
/// 103-byte socket path limit once `state/session-<key>/control.sock` is added.
#[cfg(test)]
pub(crate) fn socket_test_dir() -> tempfile::TempDir {
    tempfile::tempdir_in("/tmp").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lock_uses_std_file_locking() {
        assert!(!include_str!("../Cargo.toml").contains("fs2 ="));
        let dir = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::new(dir.path(), Path::new("/synthetic-herdr.sock")).unwrap();
        let _lock = SessionLock::acquire(&paths).unwrap().unwrap();
        let independent = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.lock)
            .unwrap();
        assert!(matches!(
            independent.try_lock(),
            Err(TryLockError::WouldBlock)
        ));
    }

    #[test]
    fn lifecycle_lock_wait_is_bounded_and_does_not_own_daemon_lock() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::new(dir.path(), Path::new("/synthetic-herdr.sock")).unwrap();
        let lifecycle =
            LifecycleLock::acquire(&paths, Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(SessionLock::acquire(&paths).unwrap().is_some());
        let started = Instant::now();
        let error = LifecycleLock::acquire(&paths, started + Duration::from_millis(40))
            .err()
            .expect("a concurrent lifecycle operation acquired the lock");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(40));
        assert!(started.elapsed() < Duration::from_secs(1));
        let inherited = lifecycle._lock.file.try_clone().unwrap();
        drop(lifecycle);
        LifecycleLock::acquire(&paths, Instant::now() + Duration::from_secs(1)).unwrap();
        drop(inherited);
    }

    #[test]
    fn paths_are_session_specific_and_require_absolute_socket() {
        let dir = tempfile::tempdir().unwrap();
        let a = RuntimePaths::new(dir.path(), std::path::Path::new("/a")).unwrap();
        let b = RuntimePaths::new(dir.path(), std::path::Path::new("/b")).unwrap();
        assert_ne!(a.root, b.root);
        assert!(RuntimePaths::new(dir.path(), std::path::Path::new("relative")).is_err());
    }
}
