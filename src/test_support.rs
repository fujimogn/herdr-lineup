use std::{
    ffi::OsStr,
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Absolute directory that owns every Herdr path a test daemon may use.
pub(crate) const ISOLATION_ROOT: &str = "LINEUP_TEST_ISOLATION_ROOT";

const HERDR_RUNTIME_KEYS: &[&str] = &[
    "HERDR_SOCKET_PATH",
    "HERDR_PLUGIN_STATE_DIR",
    "HERDR_PLUGIN_CONFIG_DIR",
];

/// Publish a script without opening its write descriptor in the test process.
/// Concurrent forks can retain even CLOEXEC descriptors until they exec,
/// making a script written by the parent temporarily fail with ETXTBSY.
pub(crate) fn write_executable(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let mut writer = Command::new("/bin/sh")
        .args([
            "-c",
            "tmp=\"$1.tmp\"; /bin/cat > \"$tmp\" && /bin/chmod 700 \"$tmp\" && /bin/mv \"$tmp\" \"$1\"",
            "lineup-test-script",
        ])
        .arg(path)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    // Only a pipe is writable in this process. Closing it sends EOF to cat.
    let written = writer.stdin.take().unwrap().write_all(contents.as_ref());
    let status = writer.wait()?;
    written?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "script writer exited with {status}"
        )));
    }
    Ok(())
}

/// Start a daemon child with no inherited `HERDR_*` values.
/// The socket, state, and config paths must stay inside `root`.
pub(crate) fn isolated_daemon_command(
    root: &Path,
    socket: &Path,
    state: &Path,
    config: &Path,
) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("HOME", root)
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HERDR_SOCKET_PATH", socket)
        .env("HERDR_PLUGIN_STATE_DIR", state)
        .env("HERDR_PLUGIN_CONFIG_DIR", config)
        .env(ISOLATION_ROOT, root);
    command
}

/// Reject a test daemon whose environment still points at another session.
/// Call this before opening the socket. A symlink that resolves outside `root`
/// is the same refusal as a foreign absolute path.
pub(crate) fn require_isolated_runtime(options: &crate::daemon::Options) -> io::Result<()> {
    for (key, _) in std::env::vars_os() {
        if !key.as_bytes().starts_with(b"HERDR_") {
            continue;
        }
        let allowed = key
            .to_str()
            .is_some_and(|name| HERDR_RUNTIME_KEYS.contains(&name));
        if !allowed {
            return Err(io::Error::other(
                "refusing an inherited HERDR_ variable outside the isolated runtime",
            ));
        }
    }
    let root = std::env::var_os(ISOLATION_ROOT).ok_or_else(|| {
        io::Error::other("LINEUP_TEST_ISOLATION_ROOT must be an absolute directory")
    })?;
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err(io::Error::other(
            "LINEUP_TEST_ISOLATION_ROOT must be an absolute directory",
        ));
    }
    let root = std::fs::canonicalize(root)?;
    for path in [&options.socket, &options.state, &options.config] {
        contained_path(&root, path)?;
    }
    Ok(())
}

fn contained_path(root: &Path, path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::other("isolated runtime path must be absolute"));
    }
    let mut pending = Vec::new();
    let mut cursor = path.to_path_buf();
    let existing = loop {
        match std::fs::canonicalize(&cursor) {
            Ok(found) => break found,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = cursor.file_name().ok_or_else(|| {
                    io::Error::other("refusing a runtime path outside the isolated test root")
                })?;
                if name == OsStr::new("..") || name == OsStr::new(".") {
                    return Err(io::Error::other(
                        "refusing a runtime path outside the isolated test root",
                    ));
                }
                pending.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .ok_or_else(|| {
                        io::Error::other("refusing a runtime path outside the isolated test root")
                    })?
                    .to_path_buf();
            }
            Err(error) => return Err(error),
        }
    };
    let mut resolved = existing;
    for name in pending.iter().rev() {
        resolved.push(name);
    }
    if resolved != root && !resolved.starts_with(root) {
        return Err(io::Error::other(
            "refusing a runtime path outside the isolated test root",
        ));
    }
    Ok(resolved)
}
