//! Session-bound local usage observations. No account or conversation data is returned.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Agy,
    Codex,
    Grok,
    Hermes,
    Kimi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    pub provider: Provider,
    pub native_session_id: String,
    /// Opaque repository identity supplied by the caller, never used as a session fallback.
    pub repository_identity: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingReason {
    Unsupported,
    NotFound,
    PermissionDenied,
    Unassociated,
    Invalid,
    Timeout,
    Stale,
    SourceError,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UsageOutcome {
    Available(f64),
    Unavailable(MissingReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    ClaudeStatusline,
    AgyStatusline,
    CodexRollout,
    GrokSignals,
    HermesUnsupported,
    KimiUnsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeeklyWindow {
    CalendarWeek,
    RollingSevenDays,
}

/// Exact association established by a reader, independently of the provider name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageBinding {
    NativeSession(String),
    Pane(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageReading {
    pub identity: SessionIdentity,
    pub binding: Option<UsageBinding>,
    pub observed_at: Option<SystemTime>,
    pub source_kind: SourceKind,
    pub context: UsageOutcome,
    pub weekly_remaining: UsageOutcome,
    pub weekly_window: Option<WeeklyWindow>,
    /// Absolute reset time, independent of the observation timestamp.
    pub weekly_resets_at: Option<SystemTime>,
}

/// Homes are explicitly passed from the pane's environment; readers do not access global homes.
pub struct ReadRequest<'a> {
    pub identity: &'a SessionIdentity,
    pub home: &'a Path,
    pub now: SystemTime,
    pub max_age: Duration,
    pub timeout: Duration,
}

pub trait UsageReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading;
}

/// The caller's deadline covers discovery, reads and decompression, including stalled I/O.
/// A timed-out job is not killed (Rust cannot safely cancel a blocked OS read), but its
/// provider slot stays occupied until it finishes. Thus repeated polls cannot leak workers.
pub(crate) fn bounded_read<F>(
    request: &ReadRequest<'_>,
    source: SourceKind,
    busy: &'static AtomicBool,
    job: F,
) -> UsageReading
where
    F: FnOnce(&ReadRequest<'_>) -> UsageReading + Send + 'static,
{
    let Some(deadline) = Instant::now().checked_add(request.timeout) else {
        return UsageReading::missing(request, source, MissingReason::Invalid);
    };
    if request.timeout.is_zero()
        || busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return UsageReading::missing(request, source, MissingReason::Timeout);
    }
    struct Permit(&'static AtomicBool);
    impl Drop for Permit {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let permit = Permit(busy);
    let identity = request.identity.clone();
    let home = request.home.to_path_buf();
    let now = request.now;
    let max_age = request.max_age;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("lineup-reader".into())
        .spawn(move || {
            let owned = ReadRequest {
                identity: &identity,
                home: &home,
                now,
                max_age,
                timeout: deadline.saturating_duration_since(Instant::now()),
            };
            let result = job(&owned);
            drop(permit);
            // Late results are discarded after the caller returns unavailable.
            let _ = sender.send(result);
        });
    if worker.is_err() {
        return UsageReading::missing(request, source, MissingReason::SourceError);
    }
    receive_by_deadline(request, source, deadline, receiver)
}

fn receive_by_deadline(
    request: &ReadRequest<'_>,
    source: SourceKind,
    deadline: Instant,
    receiver: std::sync::mpsc::Receiver<UsageReading>,
) -> UsageReading {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) if Instant::now() < deadline => result,
        // recv_timeout can dequeue a ready message even when its wait duration is zero.
        Ok(_) => UsageReading::missing(request, source, MissingReason::Timeout),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            UsageReading::missing(request, source, MissingReason::Timeout)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            UsageReading::missing(request, source, MissingReason::SourceError)
        }
    }
}

impl UsageReading {
    pub(crate) fn missing(
        request: &ReadRequest<'_>,
        source: SourceKind,
        reason: MissingReason,
    ) -> Self {
        Self {
            identity: request.identity.clone(),
            binding: None,
            observed_at: None,
            source_kind: source,
            context: UsageOutcome::Unavailable(reason),
            weekly_remaining: UsageOutcome::Unavailable(
                if matches!(
                    source,
                    SourceKind::ClaudeStatusline | SourceKind::AgyStatusline
                ) {
                    reason
                } else {
                    MissingReason::Unsupported
                },
            ),
            weekly_window: None,
            weekly_resets_at: None,
        }
    }
    pub(crate) fn sample(
        request: &ReadRequest<'_>,
        source: SourceKind,
        percent: f64,
        observed_at: SystemTime,
    ) -> Self {
        let mut reading = Self::missing(request, source, MissingReason::Invalid);
        reading.observed_at = Some(observed_at);
        if !request.identity.native_session_id.trim().is_empty() {
            reading.binding = Some(UsageBinding::NativeSession(
                request.identity.native_session_id.clone(),
            ));
        }
        reading.context = match request.now.duration_since(observed_at) {
            Err(_) => UsageOutcome::Unavailable(MissingReason::Invalid),
            Ok(age) if age > request.max_age => UsageOutcome::Unavailable(MissingReason::Stale),
            Ok(_) if percent.is_finite() && (0.0..=100.0).contains(&percent) => {
                UsageOutcome::Available(percent)
            }
            _ => UsageOutcome::Unavailable(MissingReason::Invalid),
        };
        reading
    }
}

pub(crate) fn associated(request: &ReadRequest<'_>, provider: Provider) -> bool {
    request.identity.provider == provider
        && !request.identity.native_session_id.is_empty()
        && !request.identity.repository_identity.is_empty()
}

pub(crate) fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

pub(crate) fn io_reason(error: &io::Error) -> MissingReason {
    match error.kind() {
        io::ErrorKind::NotFound => MissingReason::NotFound,
        io::ErrorKind::PermissionDenied => MissingReason::PermissionDenied,
        _ => MissingReason::SourceError,
    }
}

/// Limits both discovery and decompressed input; symlinks are never followed.
pub(crate) struct Budget {
    start: Instant,
    timeout: Duration,
    entries: usize,
}
impl Budget {
    pub(crate) fn new(request: &ReadRequest<'_>) -> Self {
        Self {
            start: Instant::now(),
            timeout: request.timeout,
            entries: 0,
        }
    }
    pub(crate) fn check(&self) -> Result<(), MissingReason> {
        if self.start.elapsed() >= self.timeout {
            Err(MissingReason::Timeout)
        } else {
            Ok(())
        }
    }
    pub(crate) fn files(
        &mut self,
        root: &Path,
        depth: usize,
        accept: fn(&Path) -> bool,
    ) -> Result<Vec<PathBuf>, MissingReason> {
        self.check()?;
        let meta = match fs::symlink_metadata(root) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_reason(&e)),
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(MissingReason::Invalid);
        }
        let mut result = Vec::new();
        for entry in fs::read_dir(root).map_err(|e| io_reason(&e))? {
            self.check()?;
            self.entries += 1;
            if self.entries > 4096 {
                return Err(MissingReason::Timeout);
            }
            let entry = entry.map_err(|e| io_reason(&e))?;
            let kind = entry.file_type().map_err(|e| io_reason(&e))?;
            if kind.is_dir() && depth > 0 {
                result.extend(self.files(&entry.path(), depth - 1, accept)?);
            } else if kind.is_file() && accept(&entry.path()) {
                result.push(entry.path());
            }
        }
        Ok(result)
    }
    pub(crate) fn text(&self, path: &Path, compressed: bool) -> Result<String, MissingReason> {
        self.check()?;
        let meta = fs::symlink_metadata(path).map_err(|e| io_reason(&e))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(MissingReason::Invalid);
        }
        let file = File::open(path).map_err(|e| io_reason(&e))?;
        let reader: Box<dyn Read> = if compressed {
            Box::new(zstd::stream::read::Decoder::new(file).map_err(|e| io_reason(&e))?)
        } else {
            Box::new(file)
        };
        let mut text = String::new();
        reader
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| io_reason(&e))?;
        self.check()?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(MissingReason::SourceError);
        }
        Ok(text)
    }
}

pub(crate) fn timestamp(value: &serde_json::Value) -> Option<SystemTime> {
    let date = chrono::DateTime::parse_from_rfc3339(value.as_str()?).ok()?;
    let millis = u64::try_from(date.timestamp_millis()).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_millis(millis))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn identity(provider: Provider) -> SessionIdentity {
        SessionIdentity {
            provider,
            native_session_id: "session-1".into(),
            repository_identity: "repo-1".into(),
        }
    }
    pub fn request<'a>(identity: &'a SessionIdentity, home: &'a Path) -> ReadRequest<'a> {
        ReadRequest {
            identity,
            home,
            now: UNIX_EPOCH + Duration::from_secs(1791763200),
            max_age: Duration::from_secs(60),
            timeout: Duration::from_secs(2),
        }
    }
    #[test]
    fn queued_available_result_is_discarded_after_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender
            .send(UsageReading::sample(
                &r,
                SourceKind::CodexRollout,
                7.0,
                r.now,
            ))
            .unwrap();
        let result = receive_by_deadline(&r, SourceKind::CodexRollout, Instant::now(), receiver);
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
    }
    #[test]
    fn stalled_io_returns_by_deadline_without_spawning_repeated_workers() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        static OTHER_PROVIDER: AtomicBool = AtomicBool::new(false);
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let mut r = request(&id, dir.path());
        r.timeout = Duration::from_millis(20);
        let (release, blocked) = std::sync::mpsc::channel();
        let start = Instant::now();
        let result = bounded_read(&r, SourceKind::CodexRollout, &BUSY, move |request| {
            // Simulate an OS read that does not respond to cooperative Budget checks.
            blocked.recv().unwrap();
            UsageReading::sample(request, SourceKind::CodexRollout, 7.0, request.now)
        });
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(BUSY.load(Ordering::Acquire));
        let retry = bounded_read(&r, SourceKind::CodexRollout, &BUSY, |_| {
            panic!("a timed-out job must retain its provider slot")
        });
        assert_eq!(
            retry.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        r.timeout = Duration::from_secs(2);
        let other = bounded_read(&r, SourceKind::GrokSignals, &OTHER_PROVIDER, |request| {
            UsageReading::sample(request, SourceKind::GrokSignals, 7.0, request.now)
        });
        assert_eq!(other.context, UsageOutcome::Available(7.0));
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while BUSY.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!BUSY.load(Ordering::Acquire));
        let recovered = bounded_read(&r, SourceKind::CodexRollout, &BUSY, |request| {
            UsageReading::sample(request, SourceKind::CodexRollout, 8.0, request.now)
        });
        assert_eq!(recovered.context, UsageOutcome::Available(8.0));
    }
    #[test]
    fn sample_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, tmp.path());
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 100.1] {
            assert_eq!(
                UsageReading::sample(&r, SourceKind::CodexRollout, invalid, r.now).context,
                UsageOutcome::Unavailable(MissingReason::Invalid)
            );
        }
        assert_eq!(
            UsageReading::sample(
                &r,
                SourceKind::CodexRollout,
                7.0,
                r.now + Duration::from_secs(1)
            )
            .context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
    }
}
