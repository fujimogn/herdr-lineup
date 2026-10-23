//! Session-bound local usage observations. No account or conversation data is returned.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
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
    KimiSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeeklyWindow {
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
    /// When set, the caller stops waiting as soon as daemon shutdown is set.
    /// The in-flight read is not cancelled; its provider slot stays busy until
    /// the read finishes, same as a timeout.
    pub shutdown: Option<&'a AtomicBool>,
}

impl ReadRequest<'_> {
    /// Advance the injected clock through a read without trusting wall-clock jumps.
    pub(crate) fn now_after(&self, started: Instant) -> SystemTime {
        self.now.checked_add(started.elapsed()).unwrap_or(self.now)
    }
}

pub trait UsageReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading;

    /// Share candidate discovery only within the caller's current refresh.
    /// Local readers also take the cycle's diagnostics; Hermes has no worker
    /// and ignores the cycle.
    fn read_in_cycle(&self, request: &ReadRequest<'_>, _cycle: &ReadCycle) -> UsageReading {
        self.read(request)
    }
}

/// Create one per refresh and pass it to `UsageReader::read_in_cycle`.
/// Only names are shared; observations and Kimi state bindings still undergo
/// their normal per-read validation. Never retain this across refreshes.
#[derive(Clone, Default)]
pub struct ReadCycle {
    diagnostics: Option<std::sync::Arc<ReaderDiagnostics>>,
    discoveries: std::sync::Arc<std::sync::Mutex<Vec<Discovery>>>,
    #[cfg(test)]
    scans: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscoveryKind {
    Codex,
    Kimi,
}

impl DiscoveryKind {
    fn roots(self) -> &'static [(&'static str, usize)] {
        match self {
            Self::Codex => &[("sessions", 3), ("archived_sessions", 3)],
            Self::Kimi => &[("sessions", 2)],
        }
    }

    fn accepts(self, path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| match self {
                Self::Codex => {
                    name.starts_with("rollout-")
                        && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
                }
                Self::Kimi => name == "state.json",
            })
    }
}

#[derive(Clone)]
struct Discovery {
    home: PathBuf,
    kind: DiscoveryKind,
    paths: Vec<PathBuf>,
    entries: usize,
    directories: Vec<(PathBuf, Option<DirectoryStamp>)>,
}

// ctime also detects permission/ownership changes and timestamp restoration.
// Each traversed directory is checked, including empty directories and missing
// roots; checking only the home or previous matching path misses duplicates.
#[derive(Clone, PartialEq, Eq)]
struct DirectoryStamp {
    fingerprint: Fingerprint,
    ctime: (i64, i64),
    mode: u32,
    uid: u32,
    gid: u32,
}

fn directory_stamp(path: &Path) -> Result<Option<DirectoryStamp>, MissingReason> {
    #[cfg(test)]
    if let Some(stamp) = tests::EXTERNAL_ANCESTOR_STAMPS.with(|stamps| {
        stamps
            .borrow()
            .iter()
            .find(|(ancestor, _)| ancestor == path)
            .map(|(_, stamp)| stamp.clone())
    }) {
        return Ok(stamp);
    }
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(DirectoryStamp {
            fingerprint: Fingerprint::of(&meta),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_reason(&error)),
    }
}

impl Discovery {
    fn unchanged(&self, budget: &Budget) -> Result<bool, MissingReason> {
        for (path, stamp) in &self.directories {
            budget.check()?;
            if directory_stamp(path)?.as_ref() != stamp.as_ref() {
                return Ok(false);
            }
        }
        budget.check()?;
        Ok(true)
    }
}

impl ReadCycle {
    #[cfg(test)]
    pub(crate) fn discovery_scan_count(&self) -> usize {
        self.scans.load(Ordering::Relaxed)
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Optional owner-retained diagnostics; default/new keep observation disabled.
    #[allow(dead_code)]
    pub(crate) fn with_diagnostics(diagnostics: std::sync::Arc<ReaderDiagnostics>) -> Self {
        Self {
            diagnostics: Some(diagnostics),
            ..Self::default()
        }
    }

    #[allow(dead_code)]
    pub(crate) fn diagnostics(&self) -> Option<&std::sync::Arc<ReaderDiagnostics>> {
        self.diagnostics.as_ref()
    }

    pub(crate) fn files(
        &self,
        budget: &mut Budget,
        home: &Path,
        kind: DiscoveryKind,
    ) -> Result<Vec<PathBuf>, MissingReason> {
        self.files_with_hooks(budget, home, kind, |_| {}, || {})
    }

    fn files_with_hooks(
        &self,
        budget: &mut Budget,
        home: &Path,
        kind: DiscoveryKind,
        mut before_directory: impl FnMut(&Path),
        after_scan: impl FnOnce(),
    ) -> Result<Vec<PathBuf>, MissingReason> {
        budget.check()?;
        // Never hold a mutex across filesystem I/O: a stalled worker retains
        // its provider BUSY permit, but must not block another provider.
        let cached = {
            let mut discoveries = self
                .discoveries
                .lock()
                .map_err(|_| MissingReason::SourceError)?;
            discoveries
                .iter()
                .position(|entry| entry.home == home && entry.kind == kind)
                .map(|index| discoveries.remove(index))
        };
        if let Some(cached) = cached
            && cached.unchanged(budget)?
        {
            budget.entries = budget
                .entries
                .checked_add(cached.entries)
                .ok_or(MissingReason::EntryLimit)?;
            if budget.entries > MAX_DIRECTORY_ENTRIES {
                return Err(MissingReason::EntryLimit);
            }
            let paths = cached.paths.clone();
            self.remember(cached)?;
            return Ok(paths);
        }
        #[cfg(test)]
        self.scans.fetch_add(1, Ordering::Relaxed);
        let mut discovery = Discovery {
            home: home.to_path_buf(),
            kind,
            paths: Vec::new(),
            entries: 0,
            directories: Vec::new(),
        };
        // Ancestor replacement (including a symlinked home) invalidates names.
        for ancestor in home.ancestors() {
            budget.check()?;
            discovery
                .directories
                .push((ancestor.to_path_buf(), directory_stamp(ancestor)?));
        }
        let before = budget.entries;
        for (root, depth) in kind.roots() {
            discovery.paths.extend(budget.files_tracked(
                &home.join(root),
                *depth,
                |path| kind.accepts(path),
                &mut discovery.directories,
                &mut before_directory,
            )?);
        }
        discovery.entries = budget.entries - before;
        after_scan();
        // Discard a mixed listing, including ancestor rename/restore (ABA).
        // An unrelated sibling update can also change an ancestor stamp. In
        // either case reprove association with the original full traversal,
        // under the remaining deadline, and never cache that fallback result.
        if !discovery.unchanged(budget)? {
            let mut paths = Vec::new();
            for (root, depth) in kind.roots() {
                paths.extend(budget.files(&home.join(root), *depth, |path| kind.accepts(path))?);
            }
            return Ok(paths);
        }
        let paths = discovery.paths.clone();
        self.remember(discovery)?;
        Ok(paths)
    }

    fn remember(&self, discovery: Discovery) -> Result<(), MissingReason> {
        let mut discoveries = self
            .discoveries
            .lock()
            .map_err(|_| MissingReason::SourceError)?;
        // Bound retention even if a caller supplies many different homes.
        if discoveries.len() < 128 {
            discoveries.push(discovery);
        }
        Ok(())
    }
}

/// Account quota only; no credentials, raw responses or conversation data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WeeklySample {
    pub used_percent: f64,
    pub resets_at: Option<SystemTime>,
}

/// Shared 60..=3600 second account-query cadence. Codex keeps its own timeout floor.
pub(crate) fn clamp_account_interval(interval: Duration) -> Duration {
    interval.clamp(Duration::from_secs(60), Duration::from_secs(3600))
}

pub(crate) fn clamp_account_cadence(interval: Duration, timeout: Duration) -> (Duration, Duration) {
    (
        clamp_account_interval(interval),
        timeout.clamp(Duration::from_secs(1), Duration::from_secs(30)),
    )
}

/// Grok and Kimi request cadence. The public provider settings are thin wrappers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AccountCadence {
    interval: Duration,
    timeout: Duration,
}

impl AccountCadence {
    pub(crate) fn new(interval: Duration, timeout: Duration) -> Self {
        let (interval, timeout) = clamp_account_cadence(interval, timeout);
        Self { interval, timeout }
    }

    /// Test fixture cadence. Production settings keep [`Self::new`]'s clamps.
    #[cfg(test)]
    pub(crate) fn exact(interval: Duration, timeout: Duration) -> Self {
        Self { interval, timeout }
    }

    pub(crate) fn interval(self) -> Duration {
        self.interval
    }

    pub(crate) fn timeout(self) -> Duration {
        self.timeout
    }

    pub(crate) fn default_account() -> Self {
        Self::new(Duration::from_secs(300), Duration::from_secs(10))
    }
}

/// Drop a sample whose known reset is no longer in the future.
/// A reset-less sample stays; callers apply their own age or failure rule.
pub(crate) fn clear_expired_reset(
    sample: Option<WeeklySample>,
    now: SystemTime,
) -> Option<WeeklySample> {
    sample.filter(|sample| sample.resets_at.is_none_or(|reset| reset > now))
}

/// After a failed query, keep only a sample with a still-future reset.
pub(crate) fn retain_known_reset_after_failure(
    sample: Option<WeeklySample>,
    now: SystemTime,
) -> Option<WeeklySample> {
    clear_expired_reset(sample, now).filter(|sample| sample.resets_at.is_some())
}

/// A sample with a reset stays for the reset check. Without one, it stays only
/// through `max_age` after the observation instant.
pub(crate) fn fresh_until_reset_or_age(
    sample: Option<WeeklySample>,
    observed: Option<Instant>,
    now: Instant,
    max_age: Duration,
) -> Option<WeeklySample> {
    sample.filter(|sample| {
        sample.resets_at.is_some()
            || observed.is_some_and(|observed| now.saturating_duration_since(observed) <= max_age)
    })
}

/// One long-lived query loop per provider. The query owns its existing client,
/// including its transient/authentication cache policy. Publication only tries
/// the cache lock; it never takes a client lock or waits for a request.
pub(crate) struct WeeklyWorker {
    cache: Option<std::sync::Arc<std::sync::Mutex<Option<WeeklySample>>>>,
    done: std::sync::Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl WeeklyWorker {
    pub(crate) fn start(
        shutdown: std::sync::Arc<AtomicBool>,
        interval: Duration,
        query: impl FnMut() -> Option<WeeklySample> + Send + 'static,
    ) -> io::Result<Self> {
        let cache = std::sync::Arc::new(std::sync::Mutex::new(None));
        Self::spawn(shutdown, interval, Some(cache), query)
    }

    /// Query loop whose publication state lives outside this worker.
    /// Kimi reads its credential snapshot directly, so a second sample cache
    /// would never be observed.
    pub(crate) fn start_effect(
        shutdown: std::sync::Arc<AtomicBool>,
        interval: Duration,
        mut query: impl FnMut() + Send + 'static,
    ) -> io::Result<Self> {
        Self::spawn(shutdown, interval, None, move || {
            query();
            None
        })
    }

    fn spawn(
        shutdown: std::sync::Arc<AtomicBool>,
        interval: Duration,
        cache: Option<std::sync::Arc<std::sync::Mutex<Option<WeeklySample>>>>,
        mut query: impl FnMut() -> Option<WeeklySample> + Send + 'static,
    ) -> io::Result<Self> {
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let stopped = done.clone();
        let shared = cache.clone();
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
                    if let Some(shared) = &shared
                        && let Ok(mut cache) = shared.lock()
                    {
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
        let cache = self.cache.as_ref()?;
        let sample = *cache.try_lock().ok()?;
        clear_expired_reset(sample, now).filter(|sample| {
            sample.used_percent.is_finite() && (0.0..=100.0).contains(&sample.used_percent)
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

// Diagnostic state contains only fixed categories, counters and elapsed nanoseconds.
const WORKER_RELEASED: u8 = 1;
const CALLER_DEADLINE: u8 = 2;
const CALLER_SHUTDOWN: u8 = 4;
const READER_PROVIDERS: [Provider; 6] = [
    Provider::Claude,
    Provider::Agy,
    Provider::Codex,
    Provider::Grok,
    Provider::Hermes,
    Provider::Kimi,
];

#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)] // Typed fields reserved for the subsequent control-wire task.
pub(crate) struct ReaderSnapshot {
    pub attempts: u64,
    pub busy_rejections: u64,
    pub deadline_ends: u64,
    pub shutdown_ends: u64,
    pub spawn_failures: u64,
    pub worker_failures: u64,
    pub completed: u64,
    pub invalid: u64,
    pub caller_wait_count: u64,
    pub caller_wait_total_ns: u64,
    pub caller_wait_max_ns: u64,
    pub sampling_skips: u64,
    pub worker_active: bool,
    pub timed_out_worker_active: bool,
    pub shutdown_worker_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticUnavailable {
    Contended,
    Poisoned,
    Incomplete,
}

#[derive(Default)]
struct ReaderSlot {
    totals: ReaderSnapshot,
    worker: Option<std::sync::Arc<std::sync::atomic::AtomicU8>>,
}

/// One owner may retain this across refreshes. No reader or control I/O runs locked.
#[derive(Default)]
pub(crate) struct ReaderDiagnostics {
    slots: [std::sync::Mutex<ReaderSlot>; 6],
    skips: [std::sync::atomic::AtomicU64; 6],
    unknown_worker: [AtomicBool; 6],
}

// Every local reader worker records here; the snapshot seam is not wired into control yet.
#[allow(dead_code)]
impl ReaderDiagnostics {
    fn index(provider: Provider) -> usize {
        match provider {
            Provider::Claude => 0,
            Provider::Agy => 1,
            Provider::Codex => 2,
            Provider::Grok => 3,
            Provider::Hermes => 4,
            Provider::Kimi => 5,
        }
    }

    fn update(&self, provider: Provider, update: impl FnOnce(&mut ReaderSlot)) {
        let index = Self::index(provider);
        if let Ok(mut slot) = self.slots[index].try_lock() {
            update(&mut slot);
        } else {
            let _ = self.skips[index].try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(1))
            });
        }
    }

    pub(crate) fn snapshot(
        &self,
        provider: Provider,
    ) -> Result<ReaderSnapshot, DiagnosticUnavailable> {
        let index = Self::index(provider);
        let slot = self.slots[index].try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => DiagnosticUnavailable::Contended,
            std::sync::TryLockError::Poisoned(_) => DiagnosticUnavailable::Poisoned,
        })?;
        if self.unknown_worker[index].load(Ordering::Acquire) {
            return Err(DiagnosticUnavailable::Incomplete);
        }
        let bits = slot
            .worker
            .as_ref()
            .map_or(WORKER_RELEASED, |state| state.load(Ordering::Acquire));
        let active = bits & WORKER_RELEASED == 0;
        Ok(ReaderSnapshot {
            worker_active: active,
            timed_out_worker_active: active && bits & CALLER_DEADLINE != 0,
            shutdown_worker_active: active && bits & CALLER_SHUTDOWN != 0,
            sampling_skips: self.skips[index].load(Ordering::Relaxed),
            ..slot.totals
        })
    }

    pub(crate) fn snapshots(
        &self,
    ) -> [(Provider, Result<ReaderSnapshot, DiagnosticUnavailable>); 6] {
        READER_PROVIDERS.map(|provider| (provider, self.snapshot(provider)))
    }
}

#[derive(Clone)]
struct ReaderObservation {
    diagnostics: std::sync::Arc<ReaderDiagnostics>,
    provider: Provider,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl ReaderObservation {
    fn finish(&self, end: ReaderEnd, started: Instant) {
        // Each caller writes only its own worker, even after the slot is reused.
        let bit = match end {
            ReaderEnd::Deadline => CALLER_DEADLINE,
            ReaderEnd::Shutdown => CALLER_SHUTDOWN,
            _ => 0,
        };
        self.state.fetch_or(bit, Ordering::AcqRel);
        self.diagnostics.update(self.provider, |slot| {
            let counters = &mut slot.totals;
            let count = match end {
                ReaderEnd::Completed => &mut counters.completed,
                ReaderEnd::Busy => &mut counters.busy_rejections,
                ReaderEnd::Deadline => &mut counters.deadline_ends,
                ReaderEnd::Shutdown => &mut counters.shutdown_ends,
                ReaderEnd::SpawnFailure => &mut counters.spawn_failures,
                ReaderEnd::Invalid => &mut counters.invalid,
                ReaderEnd::Disconnected => return,
            };
            *count = count.saturating_add(1);
        });
        self.diagnostics.update(self.provider, |slot| {
            let ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            slot.totals.caller_wait_count = slot.totals.caller_wait_count.saturating_add(1);
            slot.totals.caller_wait_total_ns = slot.totals.caller_wait_total_ns.saturating_add(ns);
            slot.totals.caller_wait_max_ns = slot.totals.caller_wait_max_ns.max(ns);
        });
    }
}

struct WorkerObservation(Option<ReaderObservation>);
impl Drop for WorkerObservation {
    fn drop(&mut self) {
        if let Some(observation) = &self.0 {
            // Run before dropping the production permit, including unwinding.
            observation
                .state
                .fetch_or(WORKER_RELEASED, Ordering::AcqRel);
            if std::thread::panicking() {
                observation
                    .diagnostics
                    .update(observation.provider, |slot| {
                        slot.totals.worker_failures = slot.totals.worker_failures.saturating_add(1);
                    });
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ReaderEnd {
    Completed,
    Busy,
    Deadline,
    Shutdown,
    SpawnFailure,
    Disconnected,
    Invalid,
}

/// How often a reader wait rechecks daemon shutdown. This matches the weekly
/// worker's idle park slice: long enough to avoid a spin, short enough that
/// stop's five-second deadline still covers the following metadata clear.
const READER_SHUTDOWN_SLICE: Duration = Duration::from_millis(100);

/// The caller's deadline covers discovery, reads and decompression, including stalled I/O.
/// A timed-out or shutdown-interrupted job is not killed (Rust cannot safely cancel a
/// blocked OS read), but its provider slot stays occupied until it finishes. Thus
/// repeated polls cannot leak workers.
pub(crate) fn bounded_read<F>(
    request: &ReadRequest<'_>,
    source: SourceKind,
    busy: &'static AtomicBool,
    job: F,
) -> UsageReading
where
    F: FnOnce(&ReadRequest<'_>) -> UsageReading + Send + 'static,
{
    bounded_read_inner(request, source, busy, None, job, spawn_reader)
}

#[allow(dead_code)]
pub(crate) fn bounded_read_observed<F>(
    request: &ReadRequest<'_>,
    source: SourceKind,
    busy: &'static AtomicBool,
    diagnostics: &std::sync::Arc<ReaderDiagnostics>,
    job: F,
) -> UsageReading
where
    F: FnOnce(&ReadRequest<'_>) -> UsageReading + Send + 'static,
{
    bounded_read_inner(request, source, busy, Some(diagnostics), job, spawn_reader)
}

fn spawn_reader(job: Box<dyn FnOnce() + Send>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("lineup-reader".into())
        .spawn(job)
        .map(|_| ())
}

fn bounded_read_inner<F>(
    request: &ReadRequest<'_>,
    source: SourceKind,
    busy: &'static AtomicBool,
    diagnostics: Option<&std::sync::Arc<ReaderDiagnostics>>,
    job: F,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> UsageReading
where
    F: FnOnce(&ReadRequest<'_>) -> UsageReading + Send + 'static,
{
    let started = Instant::now();
    let observation = diagnostics.map(|diagnostics| {
        diagnostics.update(request.identity.provider, |slot| {
            slot.totals.attempts = slot.totals.attempts.saturating_add(1)
        });
        ReaderObservation {
            diagnostics: diagnostics.clone(),
            provider: request.identity.provider,
            state: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(WORKER_RELEASED)),
        }
    });
    let finish = |reading, end| {
        if let Some(observation) = &observation {
            observation.finish(end, started);
        }
        reading
    };
    let Some(deadline) = started.checked_add(request.timeout) else {
        return finish(
            UsageReading::missing(request, source, MissingReason::Invalid),
            ReaderEnd::Invalid,
        );
    };
    if request.timeout.is_zero() {
        return finish(
            UsageReading::missing(request, source, MissingReason::Timeout),
            ReaderEnd::Deadline,
        );
    }
    if busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return finish(
            UsageReading::missing(request, source, MissingReason::Timeout),
            ReaderEnd::Busy,
        );
    }
    if let Some(observation) = &observation {
        observation.state.store(0, Ordering::Release);
        let index = ReaderDiagnostics::index(observation.provider);
        observation.diagnostics.unknown_worker[index].store(true, Ordering::Release);
        observation
            .diagnostics
            .update(observation.provider, |slot| {
                slot.worker = Some(observation.state.clone());
                observation.diagnostics.unknown_worker[index].store(false, Ordering::Release);
            });
    }
    struct Permit(&'static AtomicBool);
    impl Drop for Permit {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    // Fields drop in declaration order on return, unwind and rejected spawn:
    // publish diagnostic release before allowing another production worker.
    struct WorkerGuard {
        _observation: WorkerObservation,
        _permit: Permit,
    }
    let guard = WorkerGuard {
        _observation: WorkerObservation(observation.clone()),
        _permit: Permit(busy),
    };
    let identity = request.identity.clone();
    let home = request.home.to_path_buf();
    let now = request.now;
    let max_age = request.max_age;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = spawn(Box::new(move || {
        let owned = ReadRequest {
            identity: &identity,
            home: &home,
            // Include thread scheduling before the provider starts reading.
            now: now.checked_add(started.elapsed()).unwrap_or(now),
            max_age,
            timeout: deadline.saturating_duration_since(Instant::now()),
            // The caller observes shutdown while waiting. This worker cannot
            // hold that borrowed flag, and a blocked read cannot be killed.
            shutdown: None,
        };
        let result = job(&owned);
        drop(guard);
        // Late results are discarded after the caller returns unavailable.
        let _ = sender.send(result);
    }));
    if worker.is_err() {
        return finish(
            UsageReading::missing(request, source, MissingReason::SourceError),
            ReaderEnd::SpawnFailure,
        );
    }
    let (reading, end) = receive_outcome(request, source, deadline, receiver);
    finish(reading, end)
}

#[cfg(test)]
fn receive_by_deadline(
    request: &ReadRequest<'_>,
    source: SourceKind,
    deadline: Instant,
    receiver: std::sync::mpsc::Receiver<UsageReading>,
) -> UsageReading {
    receive_outcome(request, source, deadline, receiver).0
}

fn receive_outcome(
    request: &ReadRequest<'_>,
    source: SourceKind,
    deadline: Instant,
    receiver: std::sync::mpsc::Receiver<UsageReading>,
) -> (UsageReading, ReaderEnd) {
    let stopped = || {
        if request
            .shutdown
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            ReaderEnd::Shutdown
        } else {
            ReaderEnd::Deadline
        }
    };
    let unavailable = |reason| UsageReading::missing(request, source, reason);
    loop {
        // Same observation point as a Git command wait: shutdown and the
        // deadline both end the wait, and a ready result after either is
        // discarded. recv_timeout can dequeue a message even for a zero wait.
        let shutdown = request
            .shutdown
            .is_some_and(|flag| flag.load(Ordering::Acquire));
        if shutdown || Instant::now() >= deadline {
            return match receiver.try_recv() {
                Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => {
                    (unavailable(MissingReason::Timeout), stopped())
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => (
                    unavailable(MissingReason::SourceError),
                    ReaderEnd::Disconnected,
                ),
            };
        }
        let slice = deadline
            .saturating_duration_since(Instant::now())
            .min(READER_SHUTDOWN_SLICE);
        match receiver.recv_timeout(slice) {
            Ok(result)
                if Instant::now() < deadline
                    && !request
                        .shutdown
                        .is_some_and(|flag| flag.load(Ordering::Acquire)) =>
            {
                return (result, ReaderEnd::Completed);
            }
            Ok(_) => return (unavailable(MissingReason::Timeout), stopped()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return (
                    unavailable(MissingReason::SourceError),
                    ReaderEnd::Disconnected,
                );
            }
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
            // A timestamp later than this read's clock is not a context sample.
            Err(_) => UsageOutcome::Unavailable(MissingReason::Invalid),
            // Exact-session context is the last known occupancy, not a heartbeat.
            // Claude, Antigravity, Codex, Grok, and Kimi do not expire by age.
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
        accept: impl Fn(&Path) -> bool + Copy,
    ) -> Result<Vec<PathBuf>, MissingReason> {
        self.files_tracked(root, depth, accept, &mut Vec::new(), &mut |_| {})
    }

    fn files_tracked(
        &mut self,
        root: &Path,
        depth: usize,
        accept: impl Fn(&Path) -> bool + Copy,
        directories: &mut Vec<(PathBuf, Option<DirectoryStamp>)>,
        before_directory: &mut impl FnMut(&Path),
    ) -> Result<Vec<PathBuf>, MissingReason> {
        self.check()?;
        directories.push((root.to_path_buf(), directory_stamp(root)?));
        let meta = match fs::symlink_metadata(root) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_reason(&e)),
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(MissingReason::Invalid);
        }
        before_directory(root);
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
                result.extend(self.files_tracked(
                    &entry.path(),
                    depth - 1,
                    accept,
                    directories,
                    before_directory,
                )?);
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

/// Bytes kept from the end of a plain JSONL file. The header check and its
/// newline must fit in `JSONL_HEADER_BYTES`; a larger first line is `SourceError`.
pub(crate) const JSONL_TAIL_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const JSONL_HEADER_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub dev: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime: (i64, i64),
}
impl Fingerprint {
    pub(crate) fn of(meta: &Metadata) -> Self {
        Self {
            dev: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotChange {
    Stable,
    Appended,
    Rejected,
}

/// An append leaves the bytes already read intact. A rewrite, truncation, or
/// replacement does not, so the caller must drop that result.
pub(crate) fn snapshot_change(
    before: Fingerprint,
    path_after: Fingerprint,
    file_after: Fingerprint,
) -> SnapshotChange {
    if path_after == before && file_after == before {
        return SnapshotChange::Stable;
    }
    if path_after.dev == before.dev
        && path_after.inode == before.inode
        && file_after.dev == before.dev
        && file_after.inode == before.inode
        && path_after.size >= before.size
        && file_after.size >= before.size
        && (path_after.size > before.size || file_after.size > before.size)
    {
        return SnapshotChange::Appended;
    }
    SnapshotChange::Rejected
}

/// Read a whole small JSONL file, or the header plus a complete tail.
/// `bind_header` runs only when the file is larger than the tail. The returned
/// text drops a partial first line of a tail and a partial trailing UTF-8 byte.
pub(crate) fn read_jsonl_tail(
    budget: &Budget,
    file: &mut File,
    size: u64,
    bind_header: impl FnOnce(&[u8]) -> Result<(), MissingReason>,
) -> Result<String, MissingReason> {
    if size <= JSONL_TAIL_BYTES as u64 {
        return Budget::decode_jsonl(budget.read_range(file, 0, size as usize)?);
    }
    let header = budget.read_range(file, 0, JSONL_HEADER_BYTES)?;
    let end = header
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(MissingReason::SourceError)?;
    bind_header(&header[..end])?;
    let start = size - JSONL_TAIL_BYTES as u64 - 1;
    let bytes = budget.read_range(file, start, JSONL_TAIL_BYTES + 1)?;
    let complete = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |index| index + 1);
    let mut bytes = bytes;
    bytes.drain(..complete);
    Budget::decode_jsonl(bytes)
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
        pub(super) static EXTERNAL_ANCESTOR_STAMPS: std::cell::RefCell<Vec<(PathBuf, Option<DirectoryStamp>)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    // Exact-cap fixtures cannot afford a legitimate fallback caused by another
    // test creating/removing a sibling under /tmp. Freeze only ancestors outside
    // the fixture home; every directory in its owned subtree still uses real
    // metadata. The injection is thread-local and restored even on panic.
    fn stable_external_ancestors(home: &Path) -> impl Drop {
        struct Restore(
            Vec<(PathBuf, Option<DirectoryStamp>)>,
            std::marker::PhantomData<std::rc::Rc<()>>,
        );
        impl Drop for Restore {
            fn drop(&mut self) {
                EXTERNAL_ANCESTOR_STAMPS.with(|stamps| {
                    stamps.replace(std::mem::take(&mut self.0));
                });
            }
        }
        let snapshots = home
            .ancestors()
            .skip(1)
            .map(|path| (path.to_path_buf(), directory_stamp(path).unwrap()))
            .collect();
        Restore(
            EXTERNAL_ANCESTOR_STAMPS.with(|stamps| stamps.replace(snapshots)),
            std::marker::PhantomData,
        )
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

    #[test]
    fn retention_keeps_future_resets_and_ages_resetless_samples() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        let future = Some(WeeklySample {
            used_percent: 25.0,
            resets_at: Some(now + Duration::from_secs(30)),
        });
        let resetless = Some(WeeklySample {
            used_percent: 25.0,
            resets_at: None,
        });
        let expired = Some(WeeklySample {
            used_percent: 25.0,
            resets_at: Some(now),
        });
        for (sample, expired_reset, after_failure) in [
            (future, future, future),
            (resetless, resetless, None),
            (expired, None, None),
            (None, None, None),
        ] {
            assert_eq!(clear_expired_reset(sample, now), expired_reset);
            assert_eq!(retain_known_reset_after_failure(sample, now), after_failure);
        }
        let observed = Instant::now();
        assert_eq!(
            fresh_until_reset_or_age(resetless, Some(observed), observed, Duration::from_secs(5)),
            resetless
        );
        assert_eq!(
            fresh_until_reset_or_age(
                resetless,
                Some(observed),
                observed + Duration::from_secs(6),
                Duration::from_secs(5)
            ),
            None
        );
        assert_eq!(
            fresh_until_reset_or_age(
                future,
                None,
                observed + Duration::from_secs(60),
                Duration::from_secs(1)
            ),
            future
        );
    }

    #[test]
    fn effect_worker_does_not_retain_a_sample_cache() {
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let worker =
            WeeklyWorker::start_effect(shutdown.clone(), Duration::from_secs(3600), move || {
                let _ = tx.send(());
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(worker.sample(SystemTime::now()).is_none());
        assert!(worker.cache.is_none());
        shutdown.store(true, Ordering::Release);
        worker.handle.as_ref().unwrap().thread().unpark();
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
            shutdown: None,
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
    // Removing worker release tracking must fail the post-release snapshot assertion.
    #[test]
    fn observed_timeout_tracks_busy_and_clears_only_after_worker_release() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        static OTHER: AtomicBool = AtomicBool::new(false);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let id = identity(Provider::Codex);
        let mut r = request(&id, Path::new("/unused-fixture"));
        r.timeout = Duration::from_millis(30);
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, running) = std::sync::mpsc::channel();
        let start = Instant::now();
        let result = bounded_read_observed(
            &r,
            SourceKind::CodexRollout,
            &BUSY,
            &diagnostics,
            move |r| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                UsageReading::sample(r, SourceKind::CodexRollout, 7.0, r.now)
            },
        );
        running.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        let wait = start.elapsed();
        assert!(wait >= Duration::from_millis(30));
        assert!(wait < Duration::from_secs(2));
        let before = diagnostics.snapshot(Provider::Codex).unwrap();
        assert_eq!(
            (
                before.attempts,
                before.deadline_ends,
                before.busy_rejections
            ),
            (1, 1, 0)
        );
        assert!(before.worker_active && before.timed_out_worker_active);
        let retry =
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |_| {
                panic!("BUSY must reject")
            });
        assert_eq!(
            retry.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        let other_id = identity(Provider::Grok);
        let other = request(&other_id, r.home);
        assert_eq!(
            bounded_read_observed(&other, SourceKind::GrokSignals, &OTHER, &diagnostics, |r| {
                UsageReading::sample(r, SourceKind::GrokSignals, 9.0, r.now)
            })
            .context,
            UsageOutcome::Available(9.0)
        );
        release.send(()).unwrap();
        wait_for_reader_release(&BUSY);
        let after = diagnostics.snapshot(Provider::Codex).unwrap();
        assert!(!after.worker_active && !after.timed_out_worker_active);
        assert_eq!(
            (after.attempts, after.deadline_ends, after.busy_rejections),
            (2, 1, 1)
        );
        assert_eq!(after.caller_wait_count, 2);
        assert!(after.caller_wait_total_ns >= 30_000_000);
        r.timeout = Duration::from_secs(2);
        assert_eq!(
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |r| {
                UsageReading::sample(r, SourceKind::CodexRollout, 8.0, r.now)
            })
            .context,
            UsageOutcome::Available(8.0)
        );
        eprintln!(
            "reader fixture: timeout_wait_ns={} attempts={} busy={} deadline={} wait_total_ns={} wait_max_ns={}",
            wait.as_nanos(),
            after.attempts,
            after.busy_rejections,
            after.deadline_ends,
            after.caller_wait_total_ns,
            after.caller_wait_max_ns
        );
    }

    fn wait_for_reader_release(busy: &AtomicBool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while busy.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!busy.load(Ordering::Acquire));
    }

    // Contention must not manufacture an idle worker or affect production results.
    #[test]
    fn observed_diagnostic_contention_and_poison_do_not_change_reader_results() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let id = identity(Provider::Codex);
        let r = request(&id, Path::new("/unused-fixture"));
        let lock = diagnostics.slots[2].lock().unwrap();
        assert!(matches!(
            diagnostics.snapshot(Provider::Codex),
            Err(DiagnosticUnavailable::Contended)
        ));
        let result =
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |r| {
                UsageReading::sample(r, SourceKind::CodexRollout, 7.0, r.now)
            });
        assert_eq!(result.context, UsageOutcome::Available(7.0));
        assert!(!BUSY.load(Ordering::Acquire));
        drop(lock);
        assert!(matches!(
            diagnostics.snapshot(Provider::Codex),
            Err(DiagnosticUnavailable::Incomplete)
        ));
        let result =
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |r| {
                UsageReading::sample(r, SourceKind::CodexRollout, 8.0, r.now)
            });
        assert_eq!(result.context, UsageOutcome::Available(8.0));
        assert!(
            diagnostics
                .snapshot(Provider::Codex)
                .unwrap()
                .sampling_skips
                > 0
        );
        let shared = diagnostics.clone();
        assert!(
            std::thread::spawn(move || {
                let _lock = shared.slots[2].lock().unwrap();
                panic!("fixture poison");
            })
            .join()
            .is_err()
        );
        assert_eq!(
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |r| {
                UsageReading::sample(r, SourceKind::CodexRollout, 9.0, r.now)
            })
            .context,
            UsageOutcome::Available(9.0)
        );
        assert!(matches!(
            diagnostics.snapshot(Provider::Codex),
            Err(DiagnosticUnavailable::Poisoned)
        ));
        assert!(!BUSY.load(Ordering::Acquire));
    }

    // A panicking worker must release diagnostics before releasing its BUSY permit.
    #[test]
    fn observed_panic_and_rejected_spawn_release_the_permit() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let id = identity(Provider::Codex);
        let r = request(&id, Path::new("/unused-fixture"));
        let result =
            bounded_read_observed(&r, SourceKind::CodexRollout, &BUSY, &diagnostics, |_| {
                panic!("fixture worker panic")
            });
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::SourceError)
        );
        wait_for_reader_release(&BUSY);
        let snapshot = diagnostics.snapshot(Provider::Codex).unwrap();
        assert_eq!(snapshot.worker_failures, 1);
        assert!(!snapshot.worker_active);
        let result = bounded_read_inner(
            &r,
            SourceKind::CodexRollout,
            &BUSY,
            Some(&diagnostics),
            |_| panic!("rejected spawn must not run"),
            |job| {
                drop(job);
                Err(io::Error::other("fixture spawn failure"))
            },
        );
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::SourceError)
        );
        assert!(!BUSY.load(Ordering::Acquire));
        let snapshot = diagnostics.snapshot(Provider::Codex).unwrap();
        assert_eq!(
            (
                snapshot.attempts,
                snapshot.spawn_failures,
                snapshot.worker_failures,
                snapshot.caller_wait_count
            ),
            (2, 1, 1, 2)
        );
        assert!(!snapshot.worker_active);
    }

    #[test]
    fn observed_shutdown_retains_worker_until_channel_release() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let (release, blocked) = std::sync::mpsc::channel();
        let shared = shutdown.clone();
        let id = identity(Provider::Codex);
        let mut r = request(&id, Path::new("/unused-fixture"));
        r.shutdown = Some(&shutdown);
        let start = Instant::now();
        let result = bounded_read_observed(
            &r,
            SourceKind::CodexRollout,
            &BUSY,
            &diagnostics,
            move |r| {
                shared.store(true, Ordering::Release);
                blocked.recv().unwrap();
                UsageReading::sample(r, SourceKind::CodexRollout, 7.0, r.now)
            },
        );
        let wait = start.elapsed();
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        assert!(wait < Duration::from_secs(2));
        let before = diagnostics.snapshot(Provider::Codex).unwrap();
        assert_eq!((before.shutdown_ends, before.deadline_ends), (1, 0));
        assert!(before.shutdown_worker_active && before.worker_active);
        release.send(()).unwrap();
        wait_for_reader_release(&BUSY);
        assert!(
            !diagnostics
                .snapshot(Provider::Codex)
                .unwrap()
                .shutdown_worker_active
        );
        eprintln!(
            "reader fixture: shutdown_wait_ns={} shutdown_ends={}",
            wait.as_nanos(),
            before.shutdown_ends
        );
    }

    // An old caller must never mutate the next accepted worker, in either release order.
    #[test]
    fn observed_old_caller_cannot_mark_new_worker_timed_out() {
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        for release_first in [false, true] {
            let old = ReaderObservation {
                diagnostics: diagnostics.clone(),
                provider: Provider::Codex,
                state: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
            };
            let next = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
            if release_first {
                drop(WorkerObservation(Some(old.clone())));
            }
            diagnostics.update(Provider::Codex, |slot| slot.worker = Some(next.clone()));
            old.finish(ReaderEnd::Deadline, Instant::now());
            if !release_first {
                drop(WorkerObservation(Some(old)));
            }
            let snapshot = diagnostics.snapshot(Provider::Codex).unwrap();
            assert!(snapshot.worker_active);
            assert!(!snapshot.timed_out_worker_active);
            drop(WorkerObservation(Some(ReaderObservation {
                diagnostics: diagnostics.clone(),
                provider: Provider::Codex,
                state: next,
            })));
            assert!(!diagnostics.snapshot(Provider::Codex).unwrap().worker_active);
        }
        diagnostics.update(Provider::Codex, |slot| {
            slot.totals.attempts = u64::MAX;
            slot.totals.deadline_ends = u64::MAX;
            slot.totals.caller_wait_total_ns = u64::MAX;
            slot.totals.caller_wait_count = u64::MAX;
        });
        let id = identity(Provider::Codex);
        let mut r = request(&id, Path::new("/unused-fixture"));
        r.timeout = Duration::ZERO;
        static BUSY: AtomicBool = AtomicBool::new(false);
        assert_eq!(
            bounded_read_observed(
                &r,
                SourceKind::CodexRollout,
                &BUSY,
                &diagnostics,
                |_| panic!("zero deadline")
            )
            .context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        let snapshot = diagnostics.snapshot(Provider::Codex).unwrap();
        assert_eq!(
            (
                snapshot.attempts,
                snapshot.deadline_ends,
                snapshot.caller_wait_total_ns,
                snapshot.caller_wait_count
            ),
            (u64::MAX, u64::MAX, u64::MAX, u64::MAX)
        );
        assert_eq!(diagnostics.snapshots().len(), 6);
        assert!(ReadCycle::new().diagnostics().is_none());
        let cycle = ReadCycle::with_diagnostics(diagnostics.clone());
        assert!(std::sync::Arc::ptr_eq(
            cycle.clone().diagnostics().unwrap(),
            &diagnostics
        ));
    }

    #[test]
    fn observed_wait_preserves_discard_and_disconnect_precedence() {
        let id = identity(Provider::Codex);
        let shutdown = AtomicBool::new(true);
        let mut r = request(&id, Path::new("/unused-fixture"));
        r.shutdown = Some(&shutdown);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender
            .send(UsageReading::sample(
                &r,
                SourceKind::CodexRollout,
                7.0,
                r.now,
            ))
            .unwrap();
        let (reading, end) =
            receive_outcome(&r, SourceKind::CodexRollout, Instant::now(), receiver);
        assert_eq!(
            reading.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        assert!(matches!(end, ReaderEnd::Shutdown));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        drop(sender);
        let (reading, end) =
            receive_outcome(&r, SourceKind::CodexRollout, Instant::now(), receiver);
        assert_eq!(
            reading.context,
            UsageOutcome::Unavailable(MissingReason::SourceError)
        );
        assert!(matches!(end, ReaderEnd::Disconnected));
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
        let mut kimi = identity(Provider::Kimi);
        kimi.repository_identity = "/synthetic/repo".into();
        let mut kimi_request = request(&kimi, tmp.path());
        kimi_request.max_age = Duration::from_secs(60);
        let stale_at = kimi_request.now - Duration::from_secs(61);
        assert_eq!(
            UsageReading::sample(&kimi_request, SourceKind::KimiSession, 7.5, stale_at).context,
            UsageOutcome::Available(7.5)
        );
        assert_eq!(
            UsageReading::sample(
                &kimi_request,
                SourceKind::KimiSession,
                7.5,
                kimi_request.now + Duration::from_secs(1)
            )
            .context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        assert_eq!(
            UsageReading::sample(&r, SourceKind::CodexRollout, 7.5, stale_at).context,
            UsageOutcome::Available(7.5)
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

    #[test]
    fn cycle_discovery_reuses_names_but_detects_nested_changes_and_missing_roots() {
        let dir = tempfile::tempdir().unwrap();
        let _stable = stable_external_ancestors(dir.path());
        let id = identity(Provider::Codex);
        let request = request(&id, dir.path());
        let cycle = ReadCycle::new();
        let discover = || cycle.files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex);
        assert!(discover().unwrap().is_empty());
        let nested = dir.path().join("sessions/2026/10/24");
        fs::create_dir_all(&nested).unwrap();
        let first = nested.join("rollout-a-session-1.jsonl");
        fs::write(&first, b"").unwrap();
        assert_eq!(discover().unwrap(), vec![first.clone()]);
        let scans = cycle.scans.load(Ordering::Relaxed);
        assert_eq!(discover().unwrap(), vec![first.clone()]);
        assert_eq!(cycle.scans.load(Ordering::Relaxed), scans);
        let duplicate = nested.join("rollout-b-session-1.jsonl");
        fs::write(&duplicate, b"").unwrap();
        assert_eq!(discover().unwrap().len(), 2);
        fs::rename(&first, nested.join("other.jsonl")).unwrap();
        assert_eq!(discover().unwrap(), vec![duplicate]);
        assert!(
            ReadCycle::new()
                .files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Kimi)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cycle_discovery_counts_both_roots_and_rejects_symlinks_and_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let mut request = request(&id, dir.path());
        request.timeout = Duration::from_secs(10);
        let sessions = dir.path().join("sessions");
        let archive = dir.path().join("archived_sessions");
        fs::create_dir(&sessions).unwrap();
        fs::create_dir(&archive).unwrap();
        for index in 0..MAX_DIRECTORY_ENTRIES / 2 {
            fs::write(sessions.join(format!("a{index}")), b"").unwrap();
            fs::write(archive.join(format!("a{index}")), b"").unwrap();
        }
        let cycle = ReadCycle::new();
        let _stable = stable_external_ancestors(dir.path());
        assert!(
            cycle
                .files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex)
                .unwrap()
                .is_empty()
        );
        let mut charged = Budget::new(&request);
        cycle
            .files(&mut charged, dir.path(), DiscoveryKind::Codex)
            .unwrap();
        assert_eq!(charged.entries, MAX_DIRECTORY_ENTRIES);
        fs::write(archive.join("extra"), b"").unwrap();
        assert_eq!(
            cycle.files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex),
            Err(MissingReason::EntryLimit)
        );
        request.timeout = Duration::ZERO;
        assert_eq!(
            cycle.files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex),
            Err(MissingReason::Timeout)
        );
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&sessions, other.path().join("sessions")).unwrap();
        request.timeout = Duration::from_secs(10);
        assert_eq!(
            cycle.files(
                &mut Budget::new(&request),
                other.path(),
                DiscoveryKind::Codex
            ),
            Err(MissingReason::Invalid)
        );
    }

    #[test]
    fn cycle_discovery_sibling_churn_recharges_full_budget() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let sessions = home.join("sessions");
        let archive = home.join("archived_sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir(&archive).unwrap();
        for index in 0..MAX_DIRECTORY_ENTRIES / 2 {
            fs::write(sessions.join(format!("a{index}")), b"").unwrap();
            fs::write(archive.join(format!("a{index}")), b"").unwrap();
        }
        // Force a distinguishable pre-churn mtime without relying on a sleep
        // or on the filesystem's timestamp resolution.
        File::open(dir.path())
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        let parent_before = directory_stamp(dir.path()).unwrap();
        let home_before = directory_stamp(&home).unwrap();
        let sessions_before = directory_stamp(&sessions).unwrap();
        let archive_before = directory_stamp(&archive).unwrap();
        let id = identity(Provider::Codex);
        let mut request = request(&id, &home);
        request.timeout = Duration::from_secs(10);
        let cycle = ReadCycle::new();
        let mut budget = Budget::new(&request);
        let result = cycle.files_with_hooks(
            &mut budget,
            &home,
            DiscoveryKind::Codex,
            |_| {},
            || {
                std::thread::scope(|scope| {
                    scope.spawn(|| {
                        let sibling = dir.path().join("unrelated-test");
                        fs::create_dir(&sibling).unwrap();
                        fs::remove_dir(&sibling).unwrap();
                    });
                });
                assert!(directory_stamp(dir.path()).unwrap() != parent_before);
                assert!(directory_stamp(&home).unwrap() == home_before);
                assert!(directory_stamp(&sessions).unwrap() == sessions_before);
                assert!(directory_stamp(&archive).unwrap() == archive_before);
            },
        );
        // Each traversal alone fits. The ancestor change requires a fallback
        // against the SAME budget, so the first revisited entry exceeds the cap.
        assert_eq!(result, Err(MissingReason::EntryLimit));
        assert_eq!(budget.entries, MAX_DIRECTORY_ENTRIES + 1);
        assert!(cycle.discoveries.lock().unwrap().is_empty());

        File::open(dir.path())
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        {
            let _stable = stable_external_ancestors(&home);
            let mut budget = Budget::new(&request);
            let result = cycle.files_with_hooks(
                &mut budget,
                &home,
                DiscoveryKind::Codex,
                |_| {},
                || {
                    std::thread::scope(|scope| {
                        scope.spawn(|| {
                            let sibling = dir.path().join("unrelated-test");
                            fs::create_dir(&sibling).unwrap();
                            fs::remove_dir(&sibling).unwrap();
                        });
                    });
                    assert!(
                        fs::metadata(dir.path()).unwrap().modified().unwrap()
                            != UNIX_EPOCH + Duration::from_secs(1)
                    );
                },
            );
            assert_eq!(result, Ok(Vec::new()));
            assert_eq!(budget.entries, MAX_DIRECTORY_ENTRIES);
            assert_eq!(cycle.discoveries.lock().unwrap().len(), 1);
            let mut cached = Budget::new(&request);
            assert_eq!(
                cycle.files(&mut cached, &home, DiscoveryKind::Codex),
                result
            );
            assert_eq!(cached.entries, MAX_DIRECTORY_ENTRIES);
            assert_eq!(cycle.scans.load(Ordering::Relaxed), 2);
        }
        // The production stamp path resumes after the fixture's scoped override.
        assert!(directory_stamp(dir.path()).unwrap() != parent_before);
    }

    #[test]
    fn cycle_discovery_reproves_after_mutation_without_retaining_partial_names() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let request = request(&id, dir.path());
        let root = dir.path().join("sessions");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("rollout-a-session-1.jsonl"), b"").unwrap();
        let cycle = ReadCycle::new();
        let result = cycle.files_with_hooks(
            &mut Budget::new(&request),
            dir.path(),
            DiscoveryKind::Codex,
            |_| {},
            || {
                fs::write(root.join("rollout-b-session-1.jsonl"), b"").unwrap();
            },
        );
        assert_eq!(result.unwrap().len(), 2);
        assert!(cycle.discoveries.lock().unwrap().is_empty());
        assert_eq!(
            cycle
                .files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex)
                .unwrap()
                .len(),
            2
        );
        // Replacement of a formerly valid root by a symlink must not reuse
        // the previously cached listing, even if its target has those files.
        let moved = dir.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &root).unwrap();
        assert_eq!(
            cycle.files(&mut Budget::new(&request), dir.path(), DiscoveryKind::Codex),
            Err(MissingReason::Invalid)
        );
    }

    #[test]
    fn cycle_discovery_reproves_uniqueness_after_temporary_ancestor_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let alternate = dir.path().join("alternate");
        let saved = dir.path().join("saved");
        for root in [&home, &alternate] {
            fs::create_dir_all(root.join("sessions")).unwrap();
            fs::write(root.join("sessions/rollout-a-session-1.jsonl"), b"").unwrap();
        }
        fs::write(home.join("sessions/rollout-b-session-1.jsonl"), b"").unwrap();
        let id = identity(Provider::Codex);
        let request = request(&id, &home);
        let cycle = ReadCycle::new();
        let paths = cycle
            .files_with_hooks(
                &mut Budget::new(&request),
                &home,
                DiscoveryKind::Codex,
                |path| {
                    if path == home.join("sessions") {
                        fs::rename(&home, &saved).unwrap();
                        fs::rename(&alternate, &home).unwrap();
                    }
                },
                || {
                    fs::rename(&home, &alternate).unwrap();
                    fs::rename(&saved, &home).unwrap();
                },
            )
            .unwrap();
        // Returning only A would hide B from the exact-session adapter.
        assert_eq!(paths.len(), 2);
        assert!(cycle.discoveries.lock().unwrap().is_empty());
    }

    #[test]
    fn cycle_discovery_fallback_keeps_consumed_entries_and_original_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        fs::create_dir(&root).unwrap();
        for index in 0..MAX_DIRECTORY_ENTRIES / 2 {
            fs::write(root.join(format!("entry-{index}")), b"").unwrap();
        }
        let id = identity(Provider::Codex);
        let mut request = request(&id, dir.path());
        request.timeout = Duration::from_secs(10);
        let cycle = ReadCycle::new();
        let mut budget = Budget::new(&request);
        let start = budget.start;
        let result = cycle.files_with_hooks(
            &mut budget,
            dir.path(),
            DiscoveryKind::Codex,
            |_| {},
            || {
                fs::write(root.join("extra"), b"").unwrap();
            },
        );
        // 2048 consumed + 2049 on the fallback, although each walk alone fits.
        assert_eq!(result, Err(MissingReason::EntryLimit));
        assert_eq!(budget.entries, MAX_DIRECTORY_ENTRIES + 1);
        assert_eq!(budget.start, start);
        assert!(cycle.discoveries.lock().unwrap().is_empty());

        request.timeout = Duration::from_millis(20);
        let mut budget = Budget::new(&request);
        let start = budget.start;
        let result = cycle.files_with_hooks(
            &mut budget,
            dir.path(),
            DiscoveryKind::Codex,
            |_| {},
            || {
                fs::write(root.join("another"), b"").unwrap();
                std::thread::sleep(Duration::from_millis(30));
            },
        );
        assert_eq!(result, Err(MissingReason::Timeout));
        assert_eq!(budget.start, start);
        assert_eq!(budget.timeout, request.timeout);
        assert!(cycle.discoveries.lock().unwrap().is_empty());
    }
}
