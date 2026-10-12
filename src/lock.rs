//! Session-scoped OS lock with continuous descriptor transfer.

use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::{AsRawFd, FromRawFd},
    },
    path::{Path, PathBuf},
};

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
        use std::os::unix::ffi::OsStrExt;
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&paths.lock)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
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
        file.try_lock_exclusive()?;
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
        match file.try_lock_exclusive() {
            Ok(()) => Ok(false),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(true),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_session_specific_and_require_absolute_socket() {
        let dir = tempfile::tempdir().unwrap();
        let a = RuntimePaths::new(dir.path(), std::path::Path::new("/a")).unwrap();
        let b = RuntimePaths::new(dir.path(), std::path::Path::new("/b")).unwrap();
        assert_ne!(a.root, b.root);
        assert!(RuntimePaths::new(dir.path(), std::path::Path::new("relative")).is_err());
    }
}
