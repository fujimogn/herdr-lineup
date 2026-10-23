//! Exact-session Grok signals; file modification time is the local observation time.
use crate::model::*;
use serde_json::Value;
use std::path::Path;

pub struct GrokReader;
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl GrokReader {
    #[cfg(test)]
    fn read_with_hook(
        &self,
        request: &ReadRequest<'_>,
        after_discovery: impl FnOnce(&Path) + Send + 'static,
    ) -> UsageReading {
        self.read_in_cycle_with_hook(request, &ReadCycle::new(), after_discovery)
    }

    fn read_in_cycle_with_hook(
        &self,
        request: &ReadRequest<'_>,
        cycle: &ReadCycle,
        after_discovery: impl FnOnce(&Path) + Send + 'static,
    ) -> UsageReading {
        let source = SourceKind::GrokSignals;
        if !associated(request, Provider::Grok)
            || !safe_component(&request.identity.native_session_id)
        {
            return UsageReading::missing(request, source, MissingReason::Unassociated);
        }
        let job = move |request: &ReadRequest<'_>| {
            let started = std::time::Instant::now();
            match read_session_with_hook(request, after_discovery) {
                Ok((percent, observed)) => {
                    let evaluated = ReadRequest {
                        now: request.now_after(started),
                        ..*request
                    };
                    UsageReading::sample(&evaluated, source, percent, observed)
                }
                Err(reason) => UsageReading::missing(request, source, reason),
            }
        };
        match cycle.diagnostics() {
            Some(diagnostics) => bounded_read_observed(request, source, &BUSY, diagnostics, job),
            None => bounded_read(request, source, &BUSY, job),
        }
    }
}

impl UsageReader for GrokReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        self.read_in_cycle(request, &ReadCycle::new())
    }

    fn read_in_cycle(&self, request: &ReadRequest<'_>, cycle: &ReadCycle) -> UsageReading {
        self.read_in_cycle_with_hook(request, cycle, |_| {})
    }
}
fn signals_paths(
    root: &Path,
    id: &str,
    budget: &Budget,
) -> Result<Vec<std::path::PathBuf>, MissingReason> {
    fn regular_directory(path: &Path) -> Result<bool, MissingReason> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) => Ok(meta.is_dir() && !meta.file_type().is_symlink()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_reason(&error)),
        }
    }
    fn candidate(
        directory: &Path,
        paths: &mut Vec<std::path::PathBuf>,
    ) -> Result<(), MissingReason> {
        if !regular_directory(directory)? {
            return Ok(());
        }
        let path = directory.join("signals.json");
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => paths.push(path),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_reason(&error)),
        }
        Ok(())
    }
    budget.check()?;
    if !regular_directory(root)? {
        return match std::fs::symlink_metadata(root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            _ => Err(MissingReason::Invalid),
        };
    }
    let mut paths = Vec::new();
    candidate(&root.join(id), &mut paths)?;
    // Only enumerate working-directory keys. Probing the exact native ID avoids
    // charging every unrelated session artifact against the discovery budget.
    for (index, entry) in std::fs::read_dir(root)
        .map_err(|error| io_reason(&error))?
        .enumerate()
    {
        budget.check()?;
        if index >= MAX_DIRECTORY_ENTRIES {
            // The 4,097th entry is a full directory, not a deadline.
            return Err(MissingReason::EntryLimit);
        }
        let entry = entry.map_err(|error| io_reason(&error))?;
        if entry.file_name() != id
            && entry
                .file_type()
                .map_err(|error| io_reason(&error))?
                .is_dir()
        {
            candidate(&entry.path().join(id), &mut paths)?;
        }
    }
    Ok(paths)
}
fn read_session_with_hook(
    request: &ReadRequest<'_>,
    after_discovery: impl FnOnce(&Path),
) -> Result<(f64, std::time::SystemTime), MissingReason> {
    let budget = Budget::new(request);
    let paths = signals_paths(
        &request.home.join("sessions"),
        &request.identity.native_session_id,
        &budget,
    )?;
    if paths.len() > 1 {
        return Err(MissingReason::Unassociated);
    }
    let path = paths.first().ok_or(MissingReason::NotFound)?;
    after_discovery(path);
    let before = std::fs::symlink_metadata(path).map_err(|e| io_reason(&e))?;
    let observed = before.modified().map_err(|e| io_reason(&e))?;
    let value: Value =
        serde_json::from_str(&budget.text(path, false)?).map_err(|_| MissingReason::Invalid)?;
    // A concurrent write cannot make old parsed bytes look newer.
    let after = std::fs::symlink_metadata(path).map_err(|e| io_reason(&e))?;
    if after.modified().map_err(|e| io_reason(&e))? != observed || after.len() != before.len() {
        return Err(MissingReason::SourceError);
    }
    if let Some(id) = value.get("sessionId").or_else(|| value.get("session_id"))
        && id.as_str() != Some(request.identity.native_session_id.as_str())
    {
        return Err(MissingReason::Unassociated);
    }
    let percent = if let Some(percent) = value
        .get("contextWindowUsage")
        .or_else(|| value.get("context_window_usage"))
    {
        percent.as_f64().ok_or(MissingReason::Invalid)?
    } else {
        let used = value
            .get("contextTokensUsed")
            .or_else(|| value.get("context_tokens_used"))
            .and_then(Value::as_f64)
            .ok_or(MissingReason::NotFound)?;
        let capacity = value
            .get("contextWindowTokens")
            .or_else(|| value.get("context_window_tokens"))
            .and_then(Value::as_f64)
            .ok_or(MissingReason::Invalid)?;
        if used < 0.0 || capacity <= 0.0 {
            return Err(MissingReason::Invalid);
        }
        used / capacity * 100.0
    };
    budget.check()?;
    Ok((percent, observed))
}
#[cfg(test)]
mod tests {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn assert_percent(outcome: UsageOutcome) {
        let UsageOutcome::Available(percent) = outcome else {
            panic!("context unavailable");
        };
        assert!((percent - 7.0).abs() < 1e-9);
    }
    use super::*;
    use crate::model::tests::{identity, request};
    use std::{fs, time::Duration};
    #[test]
    fn unrelated_session_files_do_not_exhaust_discovery() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let r = request(&id, dir.path());
        fixture(dir.path(), r#"{"contextWindowUsage":7}"#, r.now);
        // Real homes contain thousands of unrelated session artifacts under
        // percent-encoded working-directory keys. Do not enumerate those files.
        let unrelated = dir
            .path()
            .join("sessions/%2Fhome%2Fuser%2Frepo/other-session");
        fs::create_dir_all(&unrelated).unwrap();
        for index in 0..4200 {
            fs::write(unrelated.join(format!("artifact-{index}.json")), "{}").unwrap();
        }
        assert_percent(GrokReader.read(&r).context);
    }
    fn fixture(home: &Path, value: &str, time: std::time::SystemTime) {
        let dir = home.join("sessions/cwd/session-1");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("signals.json");
        fs::write(&path, value).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(time))
            .unwrap();
    }
    #[test]
    fn signals_updated_during_read_use_advanced_comparison_clock() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let r = request(&id, dir.path());
        fixture(dir.path(), r#"{"contextWindowUsage":1}"#, r.now);
        let observed = r.now + Duration::from_millis(100);
        let reading = GrokReader.read_with_hook(&r, move |path| {
            fs::write(path, r#"{"contextWindowUsage":7}"#).unwrap();
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(observed))
                .unwrap();
            std::thread::sleep(Duration::from_millis(150));
        });
        assert_percent(reading.context);
    }
    #[test]
    fn normal_missing_idle_and_mismatch() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let r = request(&id, dir.path());
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
        fixture(dir.path(), r#"{"contextWindowUsage":7}"#, r.now);
        assert_percent(GrokReader.read(&r).context);
        assert_eq!(
            GrokReader.read(&r).weekly_remaining,
            UsageOutcome::Unavailable(MissingReason::Unsupported)
        );
        fixture(
            dir.path(),
            r#"{"contextTokensUsed":70,"contextWindowTokens":1000}"#,
            r.now - Duration::from_secs(61),
        );
        assert_percent(GrokReader.read(&r).context);
        fixture(
            dir.path(),
            r#"{"sessionId":"session-2","contextWindowUsage":7}"#,
            r.now,
        );
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        let mut other = identity(Provider::Grok);
        other.native_session_id = "other".into();
        assert_eq!(
            GrokReader.read(&request(&other, dir.path())).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
    }
    #[test]
    fn rejects_invalid_future_ambiguous_and_timeout() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let mut r = request(&id, dir.path());
        for value in [
            r#"{"contextWindowUsage":101}"#,
            r#"{"contextTokensUsed":10,"contextWindowTokens":0}"#,
            r#"{"contextWindowUsage":null}"#,
            "{",
        ] {
            fixture(dir.path(), value, r.now);
            assert_eq!(
                GrokReader.read(&r).context,
                UsageOutcome::Unavailable(MissingReason::Invalid)
            );
        }
        fixture(
            dir.path(),
            r#"{"context_window_usage":7}"#,
            r.now + Duration::from_secs(1),
        );
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        fixture(
            dir.path(),
            r#"{"context_tokens_used":70,"context_window_tokens":1000}"#,
            r.now,
        );
        assert_percent(GrokReader.read(&r).context);
        fs::create_dir_all(dir.path().join("sessions/other-cwd/session-1")).unwrap();
        fs::write(
            dir.path().join("sessions/other-cwd/session-1/signals.json"),
            "{}",
        )
        .unwrap();
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        r.timeout = Duration::ZERO;
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
    }
    #[cfg(unix)]
    #[test]
    fn never_follows_session_or_file_symlinks() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let r = request(&id, dir.path());
        fixture(dir.path(), r#"{"contextWindowUsage":7}"#, r.now);
        let path = dir.path().join("sessions/cwd/session-1/signals.json");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.path().join("auth.json"), path).unwrap();
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
    }
    #[cfg(unix)]
    #[test]
    fn discovery_does_not_follow_working_directory_or_session_links() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let r = request(&id, dir.path());
        fixture(outside.path(), r#"{"contextWindowUsage":7}"#, r.now);
        let cwd = dir.path().join("sessions/cwd");
        fs::create_dir_all(&cwd).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("sessions/cwd/session-1"),
            cwd.join("session-1"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("sessions/cwd"),
            dir.path().join("sessions/linked-cwd"),
        )
        .unwrap();
        assert_eq!(
            GrokReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
    }
    #[test]
    fn session_directory_entry_cap_is_not_a_timeout() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        for index in 0..(MAX_DIRECTORY_ENTRIES - 1) {
            fs::write(sessions.join(format!("e{index:04}")), b"").unwrap();
        }
        let id = identity(Provider::Grok);
        let mut request = request(&id, dir.path());
        request.timeout = Duration::from_secs(10);
        fixture(dir.path(), r#"{"contextWindowUsage":7}"#, request.now);
        assert_eq!(
            fs::read_dir(&sessions).unwrap().count(),
            MAX_DIRECTORY_ENTRIES
        );
        assert_percent(GrokReader.read(&request).context);
        fs::write(sessions.join("extra"), b"").unwrap();
        assert_eq!(
            fs::read_dir(&sessions).unwrap().count(),
            MAX_DIRECTORY_ENTRIES + 1
        );
        assert_eq!(
            GrokReader.read(&request).context,
            UsageOutcome::Unavailable(MissingReason::EntryLimit)
        );
    }
    #[test]
    fn diagnostic_routing_isolated() {
        const CHILD: &str = "HERDR_LINEUP_GROK_ROUTING_CHILD";
        if std::env::var(CHILD).as_deref() != Ok("grok") {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "reader_grok::tests::diagnostic_routing_isolated",
                    "--nocapture",
                ])
                .env(CHILD, "grok")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        use std::sync::atomic::Ordering;
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Grok);
        let base = request(&id, dir.path());
        fixture(dir.path(), r#"{"contextWindowUsage":7}"#, base.now);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let cycle = ReadCycle::with_diagnostics(diagnostics.clone());

        let observed = GrokReader.read_in_cycle(&base, &cycle);
        wait_for_worker();
        assert_percent(observed.context.clone());
        let plain = GrokReader.read_in_cycle(&base, &ReadCycle::new());
        wait_for_worker();
        assert_eq!(observed, plain);
        assert_eq!(observed, GrokReader.read(&base));
        wait_for_worker();
        let snapshot = diagnostics.snapshot(Provider::Grok).unwrap();
        assert_eq!((snapshot.attempts, snapshot.completed), (1, 1));
        assert!(!snapshot.worker_active);

        // Rejection before bounded_read starts has no diagnostic attempt.
        let mut unassociated = id.clone();
        unassociated.provider = Provider::Hermes;
        assert_eq!(
            GrokReader
                .read_in_cycle(&request(&unassociated, dir.path()), &cycle)
                .context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        assert_eq!(diagnostics.snapshot(Provider::Grok).unwrap().attempts, 1);
        // Invalid timeout is detected inside bounded_read, so it is counted.
        let invalid = ReadRequest {
            timeout: Duration::MAX,
            ..base
        };
        assert_eq!(
            GrokReader.read_in_cycle(&invalid, &cycle).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        let snapshot = diagnostics.snapshot(Provider::Grok).unwrap();
        assert_eq!(
            (snapshot.attempts, snapshot.completed, snapshot.invalid),
            (2, 1, 1)
        );

        // Observed and plain reads share one static permit.
        BUSY.store(true, Ordering::Release);
        let busy = UsageOutcome::Unavailable(MissingReason::Timeout);
        assert_eq!(GrokReader.read_in_cycle(&base, &cycle).context, busy);
        assert_eq!(GrokReader.read(&base).context, busy);
        let snapshot = diagnostics.snapshot(Provider::Grok).unwrap();
        assert_eq!((snapshot.attempts, snapshot.busy_rejections), (3, 1));
        BUSY.store(false, Ordering::Release);
        assert_eq!(observed, GrokReader.read_in_cycle(&base, &cycle));
        wait_for_worker();
        let snapshot = diagnostics.snapshot(Provider::Grok).unwrap();
        assert_eq!((snapshot.attempts, snapshot.completed), (4, 2));
        assert_eq!(snapshot.sampling_skips, 0);
    }

    fn wait_for_worker() {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while BUSY.load(std::sync::atomic::Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
