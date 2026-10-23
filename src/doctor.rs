//! Read-only endpoint and daemon compatibility diagnostics.

use crate::{
    config::Config,
    daemon::{
        ControlView, Health, Options, REFRESH_DURATION_BUCKET_LABELS, RefreshObservation,
        probe_view,
    },
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

/// Fixed doctor category for a statusLine directory read that did not finish.
/// The line never includes a partial count, errno, path, or record bytes.
const STATUSLINE_SCAN_INCOMPLETE: &str = "StatusLine records: scan incomplete";

/// Counts regular files directly under `state/statusline` without following
/// symlinks or reading file bytes. A missing directory, a symlink, or a
/// directory that cannot be opened reports zero records. A directory read or
/// entry stat error reports scan incomplete instead of a partial count. The
/// scan stops after `MAX_STATUSLINE_ENTRIES` entries and marks that limit.
pub(crate) fn statusline_records_line(state: &Path, now: SystemTime) -> String {
    statusline_records_line_limited(state, now, MAX_STATUSLINE_ENTRIES)
}

fn statusline_records_line_limited(state: &Path, now: SystemTime, limit: usize) -> String {
    render_statusline_scan(scan_statusline_records(
        &state.join("statusline"),
        now,
        limit,
    ))
}

#[cfg(test)]
fn statusline_records_line_with(
    state: &Path,
    now: SystemTime,
    limit: usize,
    ops: &StatusLineScanOps,
) -> String {
    render_statusline_scan(scan_statusline_records_with(
        &state.join("statusline"),
        now,
        limit,
        ops,
    ))
}

fn render_statusline_scan(scan: Option<StatusLineScan>) -> String {
    match scan {
        Some(StatusLineScan::Limit { count, latest_age }) if count > 0 => {
            format!(
                "StatusLine records: {count} in scanned entries (latest in scanned entries: {latest_age}s ago; scan limit reached)"
            )
        }
        Some(StatusLineScan::Limit { count: 0, .. }) => {
            "StatusLine records: 0 in scanned entries (latest unavailable; scan limit reached)"
                .to_string()
        }
        Some(StatusLineScan::Complete { count, latest_age }) if count > 0 => {
            format!("StatusLine records: {count} (latest: {latest_age}s ago)")
        }
        Some(StatusLineScan::Incomplete) => STATUSLINE_SCAN_INCOMPLETE.to_string(),
        _ => "StatusLine records: 0".to_string(),
    }
}

enum StatusLineScan {
    Complete { count: usize, latest_age: u64 },
    Limit { count: usize, latest_age: u64 },
    Incomplete,
}

struct StatusLineScanOps {
    readdir: unsafe extern "C" fn(*mut libc::DIR) -> *mut libc::dirent,
    fstatat: unsafe extern "C" fn(
        libc::c_int,
        *const libc::c_char,
        *mut libc::stat,
        libc::c_int,
    ) -> libc::c_int,
    closedir: unsafe extern "C" fn(*mut libc::DIR) -> libc::c_int,
}

unsafe extern "C" fn scan_readdir(dir: *mut libc::DIR) -> *mut libc::dirent {
    unsafe { libc::readdir(dir) }
}

unsafe extern "C" fn scan_fstatat(
    dirfd: libc::c_int,
    path: *const libc::c_char,
    buf: *mut libc::stat,
    flags: libc::c_int,
) -> libc::c_int {
    unsafe { libc::fstatat(dirfd, path, buf, flags) }
}

unsafe extern "C" fn scan_closedir(dir: *mut libc::DIR) -> libc::c_int {
    unsafe { libc::closedir(dir) }
}

const REAL_SCAN_OPS: StatusLineScanOps = StatusLineScanOps {
    readdir: scan_readdir,
    fstatat: scan_fstatat,
    closedir: scan_closedir,
};

fn scan_statusline_records(
    directory: &Path,
    now: SystemTime,
    limit: usize,
) -> Option<StatusLineScan> {
    scan_statusline_records_with(directory, now, limit, &REAL_SCAN_OPS)
}

fn scan_statusline_records_with(
    directory: &Path,
    now: SystemTime,
    limit: usize,
    ops: &StatusLineScanOps,
) -> Option<StatusLineScan> {
    if limit == 0 {
        return Some(StatusLineScan::Limit {
            count: 0,
            latest_age: 0,
        });
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
    let mut failed = false;
    loop {
        if examined >= limit {
            break;
        }
        // A null `readdir` is EOF only when errno is unchanged. Set it to
        // zero first so an earlier error is not reported as a short directory.
        set_errno(0);
        // SAFETY: `dir` is open and this thread is its only user. The entry
        // pointer is valid until the next call.
        let entry = unsafe { (ops.readdir)(dir) };
        if entry.is_null() {
            failed = read_errno() != 0;
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
            unsafe { (ops.fstatat)(dirfd, name.as_ptr(), &mut info, libc::AT_SYMLINK_NOFOLLOW) };
        if rc != 0 {
            failed = true;
            break;
        }
        if (info.st_mode & libc::S_IFMT) != libc::S_IFREG {
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
    // SAFETY: `dir` came from fdopendir and this is its one close.
    let close_rc = unsafe { (ops.closedir)(dir) };
    if failed || close_rc != 0 {
        return Some(StatusLineScan::Incomplete);
    }
    let latest_age = newest
        .map(|modified| {
            now.duration_since(modified)
                .map(|age| age.as_secs())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    if examined >= limit {
        Some(StatusLineScan::Limit { count, latest_age })
    } else {
        Some(StatusLineScan::Complete { count, latest_age })
    }
}

fn set_errno(value: i32) {
    // SAFETY: `errno_slot` points at this thread's errno cell.
    unsafe {
        *errno_slot() = value;
    }
}

fn read_errno() -> i32 {
    // SAFETY: `errno_slot` points at this thread's errno cell.
    unsafe { *errno_slot() }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("statusLine errno checks support Linux and macOS only");

#[cfg(target_os = "linux")]
fn errno_slot() -> *mut i32 {
    // SAFETY: glibc and musl expose the thread-local errno address.
    unsafe { libc::__errno_location() }
}

#[cfg(target_os = "macos")]
fn errno_slot() -> *mut i32 {
    // SAFETY: libSystem exposes the thread-local errno address.
    unsafe { libc::__error() }
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
pub(crate) fn refresh_diagnostic(health: &Health) -> io::Result<u64> {
    if health.refresh_failed {
        return Err(io::Error::other("daemon refresh is unhealthy"));
    }
    health.last_refresh_ms.ok_or_else(|| {
        io::Error::other("daemon is starting; the first refresh has not completed yet")
    })
}

pub(crate) fn capacity_diagnostic(health: &Health) -> Option<String> {
    (health.capacity_skipped_resources > 0).then(|| {
        let noun = if health.capacity_skipped_resources == 1 {
            "resource"
        } else {
            "resources"
        };
        format!(
            "Metadata capacity exhausted: {} {noun} skipped",
            health.capacity_skipped_resources
        )
    })
}

pub(crate) fn refresh_metrics_report(observation: Option<&RefreshObservation>) -> String {
    let Some(observation) = observation else {
        return "\
Refresh duration: unavailable
Refresh attempts: unavailable
Refresh histogram: unavailable
Refresh stages: unavailable"
            .to_owned();
    };
    let duration = match &observation.duration {
        Some(stats) => format!(
            "Refresh duration: last {} us; min {} us; max {} us",
            stats.last_us, stats.min_us, stats.max_us
        ),
        None => "Refresh duration: unavailable".to_owned(),
    };
    let attempts = format!(
        "Refresh attempts: {}; ok {}; error {}; capacity skip {}",
        observation.attempt_count,
        observation.success_count,
        observation.error_count,
        observation.capacity_skip_count
    );
    let histogram = match &observation.duration {
        Some(stats) => format!("Refresh histogram: {}", histogram_counts(stats.histogram)),
        None => "Refresh histogram: unavailable".to_owned(),
    };
    let stages = match &observation.stages {
        Some(stages) => format!(
            "{}\n{}\n{}\n{}\n{}\nRefresh stages latest: {}\nRefresh counts: reports {}; checkpoints {}",
            stage_metrics_line("snapshot", &stages.snapshot),
            stage_metrics_line("git", &stages.git),
            stage_metrics_line("reader", &stages.reader),
            stage_metrics_line("persist", &stages.persist),
            stage_metrics_line("socket", &stages.socket),
            if stages.latest_measured {
                "measured"
            } else {
                "unmeasured"
            },
            stages.reports,
            stages.checkpoints
        ),
        None => "Refresh stages: unavailable".to_owned(),
    };
    format!("{duration}\n{attempts}\n{histogram}\n{stages}")
}

fn histogram_counts(histogram: [u64; crate::daemon::REFRESH_DURATION_BUCKETS]) -> String {
    REFRESH_DURATION_BUCKET_LABELS
        .iter()
        .zip(histogram)
        .map(|(label, count)| format!("{label}={count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn stage_metrics_line(label: &str, stats: &crate::daemon::StageDurationStats) -> String {
    format!(
        "Refresh stage {label}: count {count}; last {last} us; min {min} us; max {max} us; sum {sum} us\nRefresh stage {label} histogram: {buckets}",
        count = stats.count,
        last = stats.duration.last_us,
        min = stats.duration.min_us,
        max = stats.duration.max_us,
        sum = stats.sum_us,
        buckets = histogram_counts(stats.duration.histogram),
    )
}

fn daemon_diagnostic_text(view: &ControlView) -> Result<String, (String, io::Error)> {
    let metrics = refresh_metrics_report(view.observation.as_ref());
    let last_refresh_ms = match refresh_diagnostic(&view.health) {
        Ok(last_refresh_ms) => last_refresh_ms,
        Err(error) => return Err((metrics, error)),
    };
    let mut text = format!(
        "{metrics}\nHerdr JSON API compatible; config valid; daemon/control/lock healthy; last refresh {last_refresh_ms} ms after startup"
    );
    if let Some(diagnostic) = capacity_diagnostic(&view.health) {
        text.push('\n');
        text.push_str(&diagnostic);
    }
    Ok(text)
}

pub fn run(options: &Options) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    paths.validate_control_socket_path()?;
    println!(
        "{}",
        crate::quota_diagnostic::weekly_diagnostics(&paths.root)
    );
    println!("{}", local_diagnostics(&options.state, SystemTime::now()));
    let mut transport = Transport::connect(&options.socket)?;
    check_transport(&mut transport)?;
    let health = probe_view(&paths, &options.socket)?;
    if let Some(health) = health {
        match daemon_diagnostic_text(&health) {
            Ok(text) => println!("{text}"),
            Err((text, error)) => {
                println!("{text}");
                return Err(error);
            }
        }
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
    use crate::socket::EndpointIdentity;

    #[test]
    fn capacity_skip_survives_health_wire_round_trip_without_hiding_refresh_failure() {
        let old_health = serde_json::json!({
            "plugin": "herdr-lineup",
            "socket_path": "/tmp/lineup-test.sock",
            "socket_key": "test",
            "endpoint": {"device": 1, "inode": 2},
            "nonce": "test",
            "pid": 1,
            "last_refresh_ms": 9,
            "refresh_failed": false
        });
        let mut skipped = old_health.clone();
        skipped["capacity_skipped_resources"] = serde_json::json!(1);
        let health: Health = serde_json::from_value(skipped).unwrap();
        assert_eq!(refresh_diagnostic(&health).unwrap(), 9);
        assert_eq!(
            capacity_diagnostic(&health).unwrap(),
            "Metadata capacity exhausted: 1 resource skipped"
        );
        assert_eq!(
            serde_json::to_value(&health).unwrap()["capacity_skipped_resources"],
            1
        );
        let legacy: Health = serde_json::from_value(old_health).unwrap();
        assert_eq!(refresh_diagnostic(&legacy).unwrap(), 9);
        assert!(capacity_diagnostic(&legacy).is_none());
        let mut future = serde_json::to_value(&health).unwrap();
        future["unknown_future_field"] = serde_json::json!({"value": 2});
        let compatible: Health = serde_json::from_value(future).unwrap();
        assert_eq!(compatible.capacity_skipped_resources, 1);
        #[derive(serde::Deserialize)]
        struct LegacyDoctorHealth {
            last_refresh_ms: Option<u64>,
            refresh_failed: bool,
        }
        let legacy_reader: LegacyDoctorHealth =
            serde_json::from_value(serde_json::to_value(&compatible).unwrap()).unwrap();
        assert_eq!(legacy_reader.last_refresh_ms, Some(9));
        assert!(!legacy_reader.refresh_failed);
        let mut failed = compatible;
        failed.refresh_failed = true;
        assert_eq!(
            refresh_diagnostic(&failed).unwrap_err().to_string(),
            "daemon refresh is unhealthy"
        );
    }

    #[test]
    fn doctor_explains_overlong_control_socket_path_before_connecting() {
        let root = tempfile::tempdir().unwrap();
        let options = Options {
            socket: root.path().join("herdr.sock"),
            state: root.path().join("a".repeat(110)),
            config: root.path().join("config"),
        };
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let error = run(&options).unwrap_err().to_string();
        assert!(error.contains("control socket path"), "{error}");
        assert!(
            error.contains(&paths.control.display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn first_refresh_pending_has_distinct_diagnostic_from_refresh_failure() {
        let mut health = Health {
            plugin: "herdr-lineup".into(),
            socket_path: "/tmp/lineup-test.sock".into(),
            socket_key: "test".into(),
            endpoint: EndpointIdentity {
                device: 1,
                inode: 1,
            },
            nonce: "test".into(),
            pid: 1,
            last_refresh_ms: None,
            refresh_failed: false,
            capacity_skipped_resources: 0,
        };
        let pending = refresh_diagnostic(&health).unwrap_err().to_string();
        assert!(pending.contains("first refresh has not completed yet"));
        assert!(!pending.contains("unhealthy"));

        health.refresh_failed = true;
        assert_eq!(
            refresh_diagnostic(&health).unwrap_err().to_string(),
            "daemon refresh is unhealthy"
        );
        health.refresh_failed = false;
        health.last_refresh_ms = Some(9);
        assert_eq!(refresh_diagnostic(&health).unwrap(), 9);
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
        assert_eq!(
            bounded,
            "StatusLine records: 2 in scanned entries (latest in scanned entries: 5s ago; scan limit reached)"
        );
        assert!(!bounded.contains(secret));
        assert!(!bounded.contains("extra-"));

        let mixed_state = tempfile::tempdir().unwrap();
        let mixed_status = mixed_state.path().join("statusline");
        std::fs::create_dir(&mixed_status).unwrap();
        let regular = mixed_status.join("regular.json");
        std::fs::write(&regular, secret).unwrap();
        std::fs::File::open(&regular)
            .unwrap()
            .set_modified(now - Duration::from_secs(25))
            .unwrap();
        std::fs::create_dir(mixed_status.join("nonregular")).unwrap();
        let mixed = statusline_records_line_limited(mixed_state.path(), now, 2);
        assert_eq!(
            mixed,
            "StatusLine records: 1 in scanned entries (latest in scanned entries: 25s ago; scan limit reached)"
        );
        assert!(!mixed.contains(secret));

        let uncapped = statusline_records_line_limited(mixed_state.path(), now, 3);
        assert_eq!(uncapped, "StatusLine records: 1 (latest: 25s ago)");
    }

    #[test]
    fn statusline_scan_separates_eof_errors_and_limit_and_releases_the_directory() {
        // File-descriptor checks share the process table. Run them in a
        // one-test child so another test cannot reuse the closed number.
        if std::env::var_os("LINEUP_TEST_STATUSLINE_SCAN_FD").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::statusline_scan_separates_eof_errors_and_limit_and_releases_the_directory",
                    "--nocapture",
                ])
                .env_clear()
                .env("LINEUP_TEST_STATUSLINE_SCAN_FD", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        use std::cell::Cell;
        thread_local! {
            static FILES_RETURNED: Cell<usize> = const { Cell::new(0) };
            static CLOSE_CALLS: Cell<usize> = const { Cell::new(0) };
            static CLOSE_RELEASED: Cell<bool> = const { Cell::new(false) };
        }

        unsafe extern "C" fn readdir_eio_after_first_file(
            dir: *mut libc::DIR,
        ) -> *mut libc::dirent {
            let entry = unsafe { libc::readdir(dir) };
            if entry.is_null() {
                return entry;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            let bytes = name.to_bytes();
            if bytes == b"." || bytes == b".." {
                return entry;
            }
            let seen = FILES_RETURNED.get();
            if seen >= 1 {
                set_errno(libc::EIO);
                return std::ptr::null_mut();
            }
            FILES_RETURNED.set(seen + 1);
            entry
        }

        unsafe extern "C" fn fstatat_eio(
            _dirfd: libc::c_int,
            _path: *const libc::c_char,
            _buf: *mut libc::stat,
            _flags: libc::c_int,
        ) -> libc::c_int {
            set_errno(libc::EIO);
            -1
        }

        unsafe extern "C" fn fstatat_dirties_errno(
            dirfd: libc::c_int,
            path: *const libc::c_char,
            buf: *mut libc::stat,
            flags: libc::c_int,
        ) -> libc::c_int {
            let rc = unsafe { libc::fstatat(dirfd, path, buf, flags) };
            if rc == 0 {
                set_errno(libc::EIO);
            }
            rc
        }

        unsafe extern "C" fn closedir_tracking(dir: *mut libc::DIR) -> libc::c_int {
            let fd = unsafe { libc::dirfd(dir) };
            let rc = unsafe { libc::closedir(dir) };
            CLOSE_CALLS.set(CLOSE_CALLS.get() + 1);
            let still_open = unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1;
            CLOSE_RELEASED.set(rc == 0 && !still_open);
            rc
        }

        fn assert_directory_released() {
            assert_eq!(CLOSE_CALLS.get(), 1);
            assert!(CLOSE_RELEASED.get());
        }

        fn reset_probes() {
            FILES_RETURNED.set(0);
            CLOSE_CALLS.set(0);
            CLOSE_RELEASED.set(false);
        }

        use std::time::{Duration, UNIX_EPOCH};

        fn open_fd_count() -> usize {
            let max = unsafe { libc::getdtablesize() }.clamp(0, 4096);
            (0..max)
                .filter(|fd| unsafe { libc::fcntl(*fd, libc::F_GETFD) } != -1)
                .count()
        }

        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let secret = "binding-key-DO-NOT-PRINT-9f3a";
        let state = tempfile::tempdir().unwrap();
        let status = state.path().join("statusline");
        std::fs::create_dir(&status).unwrap();
        for name in ["one.json", "two.json"] {
            let path = status.join(name);
            std::fs::write(&path, secret).unwrap();
            std::fs::File::open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(15))
                .unwrap();
        }
        let empty_state = tempfile::tempdir().unwrap();
        std::fs::create_dir(empty_state.path().join("statusline")).unwrap();

        let eio_ops = StatusLineScanOps {
            readdir: readdir_eio_after_first_file,
            fstatat: scan_fstatat,
            closedir: closedir_tracking,
        };
        let eof_ops = StatusLineScanOps {
            readdir: scan_readdir,
            fstatat: fstatat_dirties_errno,
            closedir: closedir_tracking,
        };
        let limit_ops = StatusLineScanOps {
            readdir: scan_readdir,
            fstatat: scan_fstatat,
            closedir: closedir_tracking,
        };
        let fstat_ops = StatusLineScanOps {
            readdir: scan_readdir,
            fstatat: fstatat_eio,
            closedir: closedir_tracking,
        };

        let before = open_fd_count();
        for _ in 0..32 {
            reset_probes();
            set_errno(libc::EIO);
            let empty = statusline_records_line(empty_state.path(), now);
            assert_eq!(empty, "StatusLine records: 0");
            assert!(!empty.contains(secret));

            reset_probes();
            let eof =
                statusline_records_line_with(state.path(), now, MAX_STATUSLINE_ENTRIES, &eof_ops);
            assert_eq!(eof, "StatusLine records: 2 (latest: 15s ago)");
            assert_directory_released();
            assert!(!eof.contains(secret));
            assert!(!eof.contains("one.json"));

            reset_probes();
            let limited = statusline_records_line_with(state.path(), now, 1, &limit_ops);
            assert_eq!(
                limited,
                "StatusLine records: 1 in scanned entries (latest in scanned entries: 15s ago; scan limit reached)"
            );
            assert_directory_released();
            assert!(!limited.contains("scan incomplete"));
            assert!(!limited.contains(secret));

            reset_probes();
            let failed =
                statusline_records_line_with(state.path(), now, MAX_STATUSLINE_ENTRIES, &eio_ops);
            assert_eq!(failed, STATUSLINE_SCAN_INCOMPLETE);
            assert!(!failed.chars().any(|character| character.is_ascii_digit()));
            assert!(!failed.contains(secret));
            assert!(!failed.contains("one.json") && !failed.contains("two.json"));
            assert!(!failed.contains("EIO"));
            assert_directory_released();

            reset_probes();
            let stat_failed =
                statusline_records_line_with(state.path(), now, MAX_STATUSLINE_ENTRIES, &fstat_ops);
            assert_eq!(stat_failed, STATUSLINE_SCAN_INCOMPLETE);
            assert!(
                !stat_failed
                    .chars()
                    .any(|character| character.is_ascii_digit())
            );
            assert!(!stat_failed.contains(secret));
            assert!(!stat_failed.contains("EIO"));
            assert_directory_released();
        }
        let after = open_fd_count();
        assert_eq!(
            before, after,
            "directory fd leaked: before {before} after {after}"
        );

        let zero_limit = statusline_records_line_limited(state.path(), now, 0);
        assert_eq!(
            zero_limit,
            "StatusLine records: 0 in scanned entries (latest unavailable; scan limit reached)"
        );
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

    #[test]
    fn refresh_metrics_use_fixed_labels_and_hide_secrets() {
        use crate::daemon::{
            REFRESH_DURATION_BUCKETS, RefreshDuration, RefreshStageAggregate, StageDurationStats,
            control_view_from_frame,
        };
        fn one_stage(duration_us: u64) -> StageDurationStats {
            let mut histogram = [0; REFRESH_DURATION_BUCKETS];
            histogram[0] = 1;
            StageDurationStats {
                count: 1,
                sum_us: duration_us,
                duration: RefreshDuration {
                    last_us: duration_us,
                    min_us: duration_us,
                    max_us: duration_us,
                    histogram,
                },
            }
        }
        fn measured_stage_report() -> RefreshStageAggregate {
            RefreshStageAggregate {
                snapshot: one_stage(10),
                git: one_stage(0),
                reader: one_stage(20),
                persist: one_stage(30),
                socket: one_stage(40),
                latest_measured: true,
                reports: 2,
                checkpoints: 1,
            }
        }
        let nonce = "a".repeat(64);
        let path = "/tmp/lineup-secret-not-for-doctor.sock";
        let mut legacy = serde_json::json!({
            "plugin": "herdr-lineup",
            "socket_path": path,
            "socket_key": "test",
            "endpoint": {"device": 1, "inode": 2},
            "nonce": nonce,
            "pid": 7,
            "last_refresh_ms": 9,
            "refresh_failed": false
        });
        let missing = control_view_from_frame(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(missing.observation.is_none());
        let unavailable = refresh_metrics_report(None);
        assert_eq!(
            unavailable,
            "\
Refresh duration: unavailable
Refresh attempts: unavailable
Refresh histogram: unavailable
Refresh stages: unavailable"
        );
        assert!(!unavailable.contains('0'));
        legacy["refresh_observation"] = serde_json::json!({"attempt_count": 1, "histogram": [0]});
        let malformed = control_view_from_frame(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(malformed.health.last_refresh_ms, Some(9));
        assert!(malformed.observation.is_none());
        legacy["refresh_observation"] = serde_json::Value::Null;
        assert!(
            control_view_from_frame(&serde_json::to_vec(&legacy).unwrap())
                .unwrap()
                .observation
                .is_none()
        );
        let mut histogram = [0; REFRESH_DURATION_BUCKETS];
        histogram[0] = 1;
        histogram[3] = 1;
        histogram[4] = 1;
        let observation = RefreshObservation {
            attempt_count: 3,
            success_count: 2,
            error_count: 1,
            capacity_skip_count: 0,
            duration: Some(RefreshDuration {
                last_us: 12_400,
                min_us: 400,
                max_us: 80_000,
                histogram,
            }),
            stages: Some(measured_stage_report()),
        };
        let report = refresh_metrics_report(Some(&observation));
        assert_eq!(
            report,
            "\
Refresh duration: last 12400 us; min 400 us; max 80000 us
Refresh attempts: 3; ok 2; error 1; capacity skip 0
Refresh histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=1 <100ms=1 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stage snapshot: count 1; last 10 us; min 10 us; max 10 us; sum 10 us
Refresh stage snapshot histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=0 <100ms=0 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stage git: count 1; last 0 us; min 0 us; max 0 us; sum 0 us
Refresh stage git histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=0 <100ms=0 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stage reader: count 1; last 20 us; min 20 us; max 20 us; sum 20 us
Refresh stage reader histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=0 <100ms=0 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stage persist: count 1; last 30 us; min 30 us; max 30 us; sum 30 us
Refresh stage persist histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=0 <100ms=0 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stage socket: count 1; last 40 us; min 40 us; max 40 us; sum 40 us
Refresh stage socket histogram: <1ms=1 <5ms=0 <10ms=0 <50ms=0 <100ms=0 <500ms=0 <1000ms=0 <5000ms=0 >=5000ms=0
Refresh stages latest: measured
Refresh counts: reports 2; checkpoints 1"
        );
        let mut zero_histogram = [0; REFRESH_DURATION_BUCKETS];
        zero_histogram[0] = 1;
        let zero = refresh_metrics_report(Some(&RefreshObservation {
            attempt_count: 1,
            success_count: 1,
            error_count: 0,
            capacity_skip_count: 0,
            duration: Some(RefreshDuration {
                last_us: 0,
                min_us: 0,
                max_us: 0,
                histogram: zero_histogram,
            }),
            stages: None,
        }));
        assert!(zero.starts_with("Refresh duration: last 0 us; min 0 us; max 0 us\n"));
        assert!(zero.contains("Refresh histogram: <1ms=1 "));
        assert!(zero.ends_with("Refresh stages: unavailable"));
        assert!(!zero.contains("Refresh duration: unavailable"));
        assert!(!report.contains(&nonce));
        assert!(!report.contains(path));
        assert!(!report.contains("credential"));
        assert!(!report.contains("identity"));
        assert!(!report.contains("usage"));
        let mut unmeasured = measured_stage_report();
        unmeasured.latest_measured = false;
        let kept = refresh_metrics_report(Some(&RefreshObservation {
            stages: Some(unmeasured),
            ..observation
        }));
        assert!(kept.contains("Refresh stages latest: unmeasured"));
        assert!(
            kept.contains("Refresh stage git: count 1; last 0 us; min 0 us; max 0 us; sum 0 us")
        );
        assert!(kept.contains("Refresh stage git histogram: <1ms=1 "));
        assert!(!kept.contains(&nonce));
        assert!(!kept.contains(path));
        legacy["refresh_observation"] = serde_json::json!({
            "attempt_count": 1,
            "success_count": 1,
            "error_count": 0,
            "capacity_skip_count": 0,
            "duration": {"last_us": 0, "min_us": 0, "max_us": 0, "histogram": [1, 0, 0, 0, 0, 0, 0, 0, 0]},
            "stages": {
                "durations": {
                    "snapshot_us": 1,
                    "git_us": 1,
                    "reader_us": 1,
                    "persist_us": 1,
                    "socket_us": 1
                },
                "counts": {"reports": 1, "checkpoints": 1}
            }
        });
        let legacy_stages = control_view_from_frame(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        let legacy_report = refresh_metrics_report(legacy_stages.observation.as_ref());
        assert!(legacy_report.contains("Refresh duration: last 0 us"));
        assert!(legacy_report.ends_with("Refresh stages: unavailable"));
        assert!(!legacy_report.contains("Refresh stage snapshot"));
        assert!(!legacy_report.contains(&nonce));
    }

    #[test]
    fn failed_refresh_prints_metrics_before_the_existing_error() {
        use crate::daemon::{REFRESH_DURATION_BUCKETS, RefreshDuration};
        let mut histogram = [0; REFRESH_DURATION_BUCKETS];
        histogram[0] = 1;
        let observation = RefreshObservation {
            attempt_count: 1,
            success_count: 0,
            error_count: 1,
            capacity_skip_count: 0,
            duration: Some(RefreshDuration {
                last_us: 200,
                min_us: 200,
                max_us: 200,
                histogram,
            }),
            stages: None,
        };
        let mut health = Health {
            plugin: "herdr-lineup".into(),
            socket_path: "/tmp/lineup-secret-not-for-doctor.sock".into(),
            socket_key: "test".into(),
            endpoint: EndpointIdentity {
                device: 1,
                inode: 2,
            },
            nonce: "ab".repeat(32),
            pid: 7,
            last_refresh_ms: Some(9),
            refresh_failed: true,
            capacity_skipped_resources: 1,
        };
        let failed = daemon_diagnostic_text(&ControlView {
            health: health.clone(),
            observation: Some(observation.clone()),
        })
        .unwrap_err();
        assert_eq!(failed.1.to_string(), "daemon refresh is unhealthy");
        assert!(failed.0.starts_with("Refresh duration: last 200 us"));
        assert!(failed.0.contains("Refresh histogram: <1ms=1 "));
        assert!(failed.0.contains("Refresh stages: unavailable"));
        assert!(!failed.0.contains("last refresh"));
        assert!(!failed.0.contains(&health.nonce));
        assert!(!failed.0.contains(&health.socket_path.display().to_string()));
        health.refresh_failed = false;
        health.last_refresh_ms = None;
        let starting = daemon_diagnostic_text(&ControlView {
            health: health.clone(),
            observation: None,
        })
        .unwrap_err();
        assert!(
            starting
                .1
                .to_string()
                .contains("first refresh has not completed yet")
        );
        assert!(starting.0.contains("Refresh duration: unavailable"));
        assert!(!starting.0.contains("Herdr JSON API compatible"));
        health.last_refresh_ms = Some(9);
        let ready = daemon_diagnostic_text(&ControlView {
            health,
            observation: Some(observation),
        })
        .unwrap();
        let metrics_at = ready.find("Refresh duration").unwrap();
        let uptime_at = ready.find("last refresh 9 ms after startup").unwrap();
        assert!(metrics_at < uptime_at);
        assert!(ready.contains("Metadata capacity exhausted: 1 resource skipped"));
    }
}
