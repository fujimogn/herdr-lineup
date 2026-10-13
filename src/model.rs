//! Session-bound local usage observations. No account or conversation data is returned.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
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
    /// The deadline passed, or another read still holds this provider's slot.
    /// Callers may keep the previous observation.
    Timeout,
    /// A directory listing passed `MAX_DIRECTORY_ENTRIES`. The condition lasts
    /// until entries are removed, so callers must not keep the previous observation.
    EntryLimit,
    Stale,
    SourceError,
}

/// Discovery stops after this many directory entries. One more is `EntryLimit`.
pub(crate) const MAX_DIRECTORY_ENTRIES: usize = 4096;

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

/// The caller injects the daemon home; the snapshot provides no pane-specific home.
pub struct ReadRequest<'a> {
    pub identity: &'a SessionIdentity,
    pub home: &'a Path,
    pub now: SystemTime,
    pub max_age: Duration,
    pub timeout: Duration,
}

impl ReadRequest<'_> {
    /// Advance the injected clock through a read without trusting wall-clock jumps.
    pub(crate) fn now_after(&self, started: Instant) -> SystemTime {
        self.now.checked_add(started.elapsed()).unwrap_or(self.now)
    }
}

pub trait UsageReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading;
}

/// Account quota only; no credentials, raw responses or conversation data.
#[derive(Clone, Copy)]
pub(crate) struct WeeklySample {
    pub used_percent: f64,
    pub resets_at: Option<SystemTime>,
}

/// One long-lived query loop per provider. The query owns its existing client,
/// including its transient/authentication cache policy. Publication only tries
/// the cache lock; it never takes a client lock or waits for a request.
pub(crate) struct WeeklyWorker {
    cache: std::sync::Arc<std::sync::Mutex<Option<WeeklySample>>>,
    done: std::sync::Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl WeeklyWorker {
    pub(crate) fn start(
        shutdown: std::sync::Arc<AtomicBool>,
        interval: Duration,
        mut query: impl FnMut() -> Option<WeeklySample> + Send + 'static,
    ) -> io::Result<Self> {
        let cache = std::sync::Arc::new(std::sync::Mutex::new(None));
        let shared = cache.clone();
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let stopped = done.clone();
        // Clients enforce their 60..=3600 second provider request cadence.
        // The smaller loop floor also permits fast injected queries in tests.
        let interval = interval.clamp(Duration::from_millis(100), Duration::from_secs(3600));
        let handle = std::thread::Builder::new()
            .name("lineup-weekly".into())
            .spawn(move || {
                while !shutdown.load(Ordering::Acquire) && !stopped.load(Ordering::Acquire) {
                    let result = query();
                    if shutdown.load(Ordering::Acquire) || stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if let Ok(mut cache) = shared.lock() {
                        *cache = result;
                    }
                    let next = Instant::now() + interval;
                    while Instant::now() < next
                        && !shutdown.load(Ordering::Acquire)
                        && !stopped.load(Ordering::Acquire)
                    {
                        std::thread::park_timeout(
                            next.saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(100)),
                        );
                    }
                }
            })
            .map_err(|_| io::Error::other("cannot start weekly worker"))?;
        Ok(Self {
            cache,
            done,
            handle: Some(handle),
        })
    }

    pub(crate) fn sample(&self, now: SystemTime) -> Option<WeeklySample> {
        let sample = *self.cache.try_lock().ok()?;
        sample.filter(|sample| {
            sample.used_percent.is_finite()
                && (0.0..=100.0).contains(&sample.used_percent)
                && sample.resets_at.is_none_or(|reset| reset > now)
        })
    }
}
impl Drop for WeeklyWorker {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            // A pending HTTP request is bounded by its client's timeout. Do not
            // extend daemon stop by joining it; process exit ends its I/O.
            // Codex child processes have a separate synchronous QueryControl.
        }
    }
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
    let started = Instant::now();
    let Some(deadline) = started.checked_add(request.timeout) else {
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
                // Include thread scheduling before the provider starts reading.
                now: now.checked_add(started.elapsed()).unwrap_or(now),
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
            // Exact-session context is the last known occupancy, not a heartbeat.
            // Weekly observations apply their own age/reset policy independently.
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
            if self.entries > MAX_DIRECTORY_ENTRIES {
                // Distinct from `Timeout`: a full directory does not become readable
                // on the next attempt, so retaining the previous observation hides it.
                return Err(MissingReason::EntryLimit);
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
        String::from_utf8(self.bytes(path, compressed)?).map_err(|_| MissingReason::SourceError)
    }
    /// Decode append-only JSONL while excluding a split final UTF-8 character.
    pub(crate) fn jsonl_text(
        &self,
        path: &Path,
        compressed: bool,
    ) -> Result<String, MissingReason> {
        Self::decode_jsonl(self.bytes(path, compressed)?)
    }
    pub(crate) fn decode_jsonl(mut bytes: Vec<u8>) -> Result<String, MissingReason> {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_some() {
                return Err(MissingReason::SourceError);
            }
            let complete = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |i| i + 1);
            bytes.truncate(complete);
        }
        String::from_utf8(bytes).map_err(|_| MissingReason::SourceError)
    }
    pub(crate) fn open(&self, path: &Path) -> Result<File, MissingReason> {
        self.check()?;
        let meta = fs::symlink_metadata(path).map_err(|e| io_reason(&e))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(MissingReason::Invalid);
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| io_reason(&e))?;
        if !file.metadata().map_err(|e| io_reason(&e))?.is_file() {
            return Err(MissingReason::Invalid);
        }
        self.check()?;
        Ok(file)
    }
    /// Read only a bounded region of a regular file already opened without following symlinks.
    pub(crate) fn read_range(
        &self,
        file: &mut File,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, MissingReason> {
        self.check()?;
        if length > 16 * 1024 * 1024 {
            return Err(MissingReason::SourceError);
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_reason(&e))?;
        let mut bytes = vec![0; length];
        #[cfg(test)]
        tests::CONTENT_READS.with(|count| count.set(count.get() + 1));
        file.read_exact(&mut bytes).map_err(|e| io_reason(&e))?;
        self.check()?;
        Ok(bytes)
    }
    fn bytes(&self, path: &Path, compressed: bool) -> Result<Vec<u8>, MissingReason> {
        let file = self.open(path)?;
        let reader: Box<dyn Read> = if compressed {
            Box::new(zstd::stream::read::Decoder::new(file).map_err(|e| io_reason(&e))?)
        } else {
            Box::new(file)
        };
        let mut text = Vec::new();
        #[cfg(test)]
        tests::CONTENT_READS.with(|count| count.set(count.get() + 1));
        reader
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut text)
            .map_err(|e| io_reason(&e))?;
        self.check()?;
        if text.len() > 16 * 1024 * 1024 {
            // In particular, zstd rollouts exceeding 16 MiB after decompression
            // are unavailable with SourceError; compressed streams cannot seek.
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
    thread_local! {
        pub(crate) static CONTENT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    #[test]
    fn weekly_worker_cache_never_waits_for_query_and_expires_during_query() {
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let reset = SystemTime::now() + Duration::from_secs(60);
        let mut calls = 0;
        let mut worker = WeeklyWorker::start(shutdown.clone(), Duration::ZERO, move || {
            calls += 1;
            if calls == 2 {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            Some(WeeklySample {
                used_percent: 27.0,
                resets_at: Some(reset),
            })
        })
        .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        assert_eq!(worker.sample(SystemTime::now()).unwrap().used_percent, 27.0);
        assert!(worker.sample(reset).is_none());
        assert!(start.elapsed() < Duration::from_millis(100));
        shutdown.store(true, Ordering::Release);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.handle.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        worker.handle.take().unwrap().join().unwrap();
    }

    #[test]
    fn weekly_worker_shutdown_ends_idle_wait_without_another_query() {
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let mut worker =
            WeeklyWorker::start(shutdown.clone(), Duration::from_secs(3600), move || {
                tx.send(()).unwrap();
                None
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        shutdown.store(true, Ordering::Release);
        worker.handle.as_ref().unwrap().thread().unpark();
        worker.handle.take().unwrap().join().unwrap();
        assert!(rx.try_recv().is_err());
    }
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
    #[test]
    fn directory_entry_cap_is_distinct_from_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for index in 0..MAX_DIRECTORY_ENTRIES {
            fs::write(root.join(format!("e{index:04}")), b"").unwrap();
        }
        let id = identity(Provider::Codex);
        let mut request = request(&id, root);
        request.timeout = Duration::from_secs(10);
        let mut budget = Budget::new(&request);
        let found = budget.files(root, 0, |_| true).unwrap();
        assert_eq!(found.len(), MAX_DIRECTORY_ENTRIES);
        fs::write(root.join("extra"), b"").unwrap();
        let mut budget = Budget::new(&request);
        assert_eq!(
            budget.files(root, 0, |_| true),
            Err(MissingReason::EntryLimit)
        );
    }
}
