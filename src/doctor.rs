//! Read-only endpoint and daemon compatibility diagnostics.

use crate::{
    config::Config,
    daemon::{Options, probe},
    lock::{RuntimePaths, SessionLock},
    socket::Transport,
};
use std::{
    ffi::CStr,
    fs::{self, OpenOptions},
    io,
    os::unix::{fs::OpenOptionsExt, io::IntoRawFd},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Persisted diagnostics contain only closed categories, never upstream data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeeklyResult {
    Ok,
    Pending,
    AuthMissing,
    AuthIo,
    AuthShape,
    AuthAmbiguous,
    Unauthorized,
    Transient,
    InvalidResponse,
    Request,
    NonWeekly,
    Disabled,
}
impl WeeklyResult {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Pending => "pending",
            Self::AuthMissing => "auth_missing",
            Self::AuthIo => "auth_io",
            Self::AuthShape => "auth_shape",
            Self::AuthAmbiguous => "auth_ambiguous",
            Self::Unauthorized => "unauthorized",
            Self::Transient => "transient",
            Self::InvalidResponse => "invalid_response",
            Self::Request => "request",
            Self::NonWeekly => "non_weekly",
            Self::Disabled => "disabled",
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum WeeklyProvider {
    Codex,
    Grok,
}
impl WeeklyProvider {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Grok => "grok",
        }
    }
    fn path(self, root: &std::path::Path) -> std::path::PathBuf {
        root.join(format!("weekly-{}.json", self.name()))
    }
}

pub(crate) fn write_weekly_result(
    root: &std::path::Path,
    provider: WeeklyProvider,
    result: WeeklyResult,
) -> io::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(file.as_file_mut(), &result)?;
    file.flush()?;
    file.persist(provider.path(root))
        .map_err(|_| io::Error::other("cannot save weekly diagnostic"))?;
    Ok(())
}

/// Bound and validate local diagnostics before displaying any bytes. Symlinks,
/// unknown strings and malformed records are never echoed to the terminal.
fn read_weekly_result(root: &std::path::Path, provider: WeeklyProvider) -> &'static str {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(provider.path(root))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return "not_observed",
        Err(_) => return "invalid_diagnostic",
    };
    if !file
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() <= 128)
    {
        return "invalid_diagnostic";
    }
    let mut bytes = Vec::new();
    if file.take(129).read_to_end(&mut bytes).is_err() || bytes.len() > 128 {
        return "invalid_diagnostic";
    }
    serde_json::from_slice::<WeeklyResult>(&bytes)
        .map(WeeklyResult::label)
        .unwrap_or("invalid_diagnostic")
}

pub(crate) fn weekly_diagnostics(root: &std::path::Path) -> String {
    format!(
        "Last saved weekly query results: codex={}; grok={}",
        read_weekly_result(root, WeeklyProvider::Codex),
        read_weekly_result(root, WeeklyProvider::Grok)
    )
}

pub const MIN_HERDR_VERSION: &str = "0.9.3";
/// Directory entries examined while summarizing statusLine files.
const MAX_STATUSLINE_ENTRIES: usize = 1024;

pub(crate) fn state_directory_line(state: &Path) -> String {
    format!("State directory: {}", state.display())
}

pub(crate) fn font_mode_line(mode: &str) -> String {
    let explanation = match mode {
        "auto" => "same as text (ASCII labels)",
        "font" => "assumes the terminal font and codepoint map are already configured",
        "text" => "ASCII labels",
        _ => "unrecognized; logos are unchanged",
    };
    format!("Font mode: {mode}; {explanation}.")
}

/// Counts regular files directly under `state/statusline` without following
/// symlinks or reading file bytes. Missing or unreadable directories report
/// zero records. The scan stops after `MAX_STATUSLINE_ENTRIES` entries.
pub(crate) fn statusline_records_line(state: &Path, now: SystemTime) -> String {
    statusline_records_line_limited(state, now, MAX_STATUSLINE_ENTRIES)
}

fn statusline_records_line_limited(state: &Path, now: SystemTime, limit: usize) -> String {
    match scan_statusline_records(&state.join("statusline"), now, limit) {
        Some((count, latest_age)) if count > 0 => {
            format!("StatusLine records: {count} (latest: {latest_age}s ago)")
        }
        _ => "StatusLine records: 0".to_string(),
    }
}

fn scan_statusline_records(
    directory: &Path,
    now: SystemTime,
    limit: usize,
) -> Option<(usize, u64)> {
    if limit == 0 {
        return Some((0, 0));
    }
    let metadata = fs::symlink_metadata(directory).ok()?;
    // `symlink_metadata` reports the link itself. A symlinked directory is not
    // a directory here, so its target is neither listed nor read.
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(directory)
        .ok()?;
    let raw = file.into_raw_fd();
    // SAFETY: `raw` is a directory opened with O_DIRECTORY | O_NOFOLLOW. On
    // success, fdopendir owns it. On failure the descriptor is still open.
    let dir = unsafe { libc::fdopendir(raw) };
    if dir.is_null() {
        unsafe { libc::close(raw) };
        return None;
    }
    let mut count = 0usize;
    let mut newest: Option<SystemTime> = None;
    let mut examined = 0usize;
    loop {
        if examined >= limit {
            break;
        }
        // SAFETY: `dir` is open and this thread is its only user. A null result
        // ends the listing; the entry pointer is valid until the next call.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        examined += 1;
        let mut info = unsafe { std::mem::zeroed::<libc::stat>() };
        // SAFETY: `dirfd` belongs to `dir`. `name` is NUL-terminated. The
        // AT_SYMLINK_NOFOLLOW flag stats the entry itself and does not read it.
        let dirfd = unsafe { libc::dirfd(dir) };
        let rc =
            unsafe { libc::fstatat(dirfd, name.as_ptr(), &mut info, libc::AT_SYMLINK_NOFOLLOW) };
        if rc != 0 || (info.st_mode & libc::S_IFMT) != libc::S_IFREG {
            continue;
        }
        count += 1;
        let Ok(secs) = u64::try_from(info.st_mtime) else {
            continue;
        };
        let modified = UNIX_EPOCH + Duration::from_secs(secs);
        newest = Some(match newest {
            Some(current) if current >= modified => current,
            _ => modified,
        });
    }
    // SAFETY: `dir` came from fdopendir and has not been closed.
    unsafe { libc::closedir(dir) };
    let latest_age = newest
        .map(|modified| {
            now.duration_since(modified)
                .map(|age| age.as_secs())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    Some((count, latest_age))
}

pub(crate) fn local_diagnostics(state: &Path, now: SystemTime) -> String {
    format!(
        "{}\n{}",
        state_directory_line(state),
        statusline_records_line(state, now)
    )
}
pub fn check_transport(transport: &mut Transport) -> io::Result<()> {
    transport.send_line(br#"{"id":"lineup-health","method":"ping","params":{}}"#)?;
    validate_pong(&transport.receive_line()?)
}
pub fn validate_pong(frame: &[u8]) -> io::Result<()> {
    let response: serde_json::Value = serde_json::from_slice(frame)
        .map_err(|_| io::Error::other("invalid Herdr health response"))?;
    let result = &response["result"];
    if response["id"] != "lineup-health"
        || result["type"] != "pong"
        || result["protocol"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= u64::from(u32::MAX))
            .is_none()
    {
        return Err(io::Error::other(
            "Herdr JSON protocol health response is incompatible",
        ));
    }
    let version = result["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("Herdr version is missing"))?;
    let version = version.split('+').next().unwrap_or(version);
    if version.contains('-') {
        return Err(io::Error::other(
            "Herdr prerelease compatibility is unverified",
        ));
    }
    let numbers: Option<Vec<u64>> = version.split('.').map(|part| part.parse().ok()).collect();
    let numbers = numbers
        .filter(|v| v.len() == 3)
        .ok_or_else(|| io::Error::other("invalid Herdr version"))?;
    if numbers.as_slice() < [0, 9, 3].as_slice() {
        return Err(io::Error::other("Herdr 0.9.3 or later is required"));
    }
    // JSON API methods are build independent. Binary protocol number equality
    // would incorrectly reject a compatible newer server (official stability contract).
    Ok(())
}
pub fn run(options: &Options) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    println!("{}", weekly_diagnostics(&paths.root));
    println!("{}", local_diagnostics(&options.state, SystemTime::now()));
    let mut transport = Transport::connect(&options.socket)?;
    check_transport(&mut transport)?;
    let health = probe(&paths, &options.socket)?;
    if let Some(health) = health {
        if health.refresh_failed || health.last_refresh_ms.is_none() {
            return Err(io::Error::other("daemon refresh is unhealthy"));
        }
        println!(
            "Herdr JSON API compatible; config valid; daemon/control/lock healthy; last refresh {} ms after startup",
            health.last_refresh_ms.unwrap_or(0)
        );
    } else {
        if SessionLock::is_held(&paths)? {
            return Err(io::Error::other(
                "daemon lock is live but control is unavailable",
            ));
        }
        if paths.pid.exists() {
            return Err(io::Error::other(
                "stale diagnostic PID record; no process was signalled",
            ));
        }
        return Err(io::Error::other(
            "daemon is stopped; invoke ensure explicitly",
        ));
    }
    println!("{}", font_mode_line(&config.logo_mode));
    println!("Optional provider data may be unavailable; no reader or network call was made.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekly_diagnostics_round_trip_only_fixed_categories() {
        let dir = tempfile::tempdir().unwrap();
        for result in [
            WeeklyResult::Ok,
            WeeklyResult::AuthMissing,
            WeeklyResult::AuthShape,
            WeeklyResult::AuthAmbiguous,
            WeeklyResult::Unauthorized,
            WeeklyResult::Transient,
            WeeklyResult::NonWeekly,
            WeeklyResult::Disabled,
        ] {
            write_weekly_result(dir.path(), WeeklyProvider::Grok, result).unwrap();
            let text = weekly_diagnostics(dir.path());
            assert!(text.contains(&format!("grok={}", result.label())));
            assert!(text.contains("codex=not_observed"));
        }
        std::fs::write(
            dir.path().join("weekly-grok.json"),
            r#""fake-private-body""#,
        )
        .unwrap();
        assert!(!weekly_diagnostics(dir.path()).contains("fake-private-body"));
        assert!(weekly_diagnostics(dir.path()).contains("grok=invalid_diagnostic"));
        std::fs::remove_file(dir.path().join("weekly-grok.json")).unwrap();
        std::os::unix::fs::symlink("/fake/private", dir.path().join("weekly-grok.json")).unwrap();
        assert!(weekly_diagnostics(dir.path()).contains("grok=invalid_diagnostic"));
    }

    #[test]
    fn local_diagnostics_show_state_and_bounded_statusline_counts_without_secrets() {
        use std::{
            os::unix::ffi::OsStrExt,
            time::{Duration, UNIX_EPOCH},
        };
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let state = tempfile::tempdir().unwrap();
        let report = local_diagnostics(state.path(), now);
        assert_eq!(
            report,
            format!(
                "State directory: {}\nStatusLine records: 0",
                state.path().display()
            )
        );
        assert!(state.path().is_absolute());
        assert!(!state.path().join("statusline").exists());
        assert!(!report.contains("binding") && !report.contains("secret"));

        let status = state.path().join("statusline");
        std::fs::create_dir(&status).unwrap();
        let secret = "binding-key-DO-NOT-PRINT-9f3a";
        let older = status.join("older.json");
        let newer = status.join("newer.json");
        std::fs::write(&older, format!(r#"{{"key":"{secret}","context":12.5}}"#)).unwrap();
        std::fs::write(&newer, format!(r#"{{"key":"{secret}","weekly":7}}"#)).unwrap();
        std::fs::File::open(&older)
            .unwrap()
            .set_modified(now - Duration::from_secs(40))
            .unwrap();
        std::fs::File::open(&newer)
            .unwrap()
            .set_modified(now - Duration::from_secs(15))
            .unwrap();
        std::fs::create_dir(status.join("nested")).unwrap();
        std::fs::write(status.join("nested").join("hidden.json"), secret).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let leaked = outside.path().join("leaked.json");
        std::fs::write(&leaked, secret).unwrap();
        std::os::unix::fs::symlink(&leaked, status.join("linked.json")).unwrap();
        let pipe = status.join("blocked.pipe");
        let pipe_name = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe_name.as_ptr(), 0o600) }, 0);

        let report = local_diagnostics(state.path(), now);
        assert_eq!(
            report,
            format!(
                "State directory: {}\nStatusLine records: 2 (latest: 15s ago)",
                state.path().display()
            )
        );
        assert!(!report.contains(secret));
        assert!(!report.contains("linked.json"));
        assert!(!report.contains("nested"));
        assert_eq!(std::fs::read_to_string(&leaked).unwrap(), secret);

        let linked_state = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&status, linked_state.path().join("statusline")).unwrap();
        let linked = local_diagnostics(linked_state.path(), now);
        assert!(linked.ends_with("StatusLine records: 0"));
        assert!(!linked.contains(secret));
        assert!(status.join("newer.json").is_file());

        let bounded_state = tempfile::tempdir().unwrap();
        let bounded_status = bounded_state.path().join("statusline");
        std::fs::create_dir(&bounded_status).unwrap();
        for index in 0..3 {
            let path = bounded_status.join(format!("extra-{index}.json"));
            std::fs::write(&path, secret).unwrap();
            std::fs::File::open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(5))
                .unwrap();
        }
        let bounded = statusline_records_line_limited(bounded_state.path(), now, 2);
        assert_eq!(bounded, "StatusLine records: 2 (latest: 5s ago)");
        assert!(!bounded.contains(secret));
        assert!(!bounded.contains("extra-"));
    }

    #[test]
    fn font_mode_explanation_matches_the_selected_mode() {
        assert_eq!(
            font_mode_line("auto"),
            "Font mode: auto; same as text (ASCII labels)."
        );
        assert_eq!(font_mode_line("text"), "Font mode: text; ASCII labels.");
        assert_eq!(
            font_mode_line("font"),
            "Font mode: font; assumes the terminal font and codepoint map are already configured."
        );
        for mode in ["auto", "text", "font"] {
            assert!(!font_mode_line(mode).contains("auto uses conservative text fallback"));
        }
    }

    #[test]
    fn validates_minimum_version_and_required_health_response() {
        assert!(validate_pong(br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":22}}"#).is_ok());
        for reply in [
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.2","protocol":22}}"#
                .as_slice(),
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":0}}"#
                .as_slice(),
            br#"{"id":"wrong","result":{"type":"pong","version":"0.9.3","protocol":22}}"#
                .as_slice(),
        ] {
            assert!(validate_pong(reply).is_err());
        }
    }
}
