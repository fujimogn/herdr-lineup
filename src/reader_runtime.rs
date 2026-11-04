//! Bounded local reader execution and advisory worker diagnostics.

use crate::model::{MissingReason, Provider, ReadRequest, SourceKind, UsageReading};
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

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

// All five local reader workers record here; control and doctor expose snapshots.
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

// Synthetic source state for sibling control/doctor boundary tests only.
#[cfg(test)]
impl ReaderDiagnostics {
    pub(crate) fn test_with_slot(&self, provider: Provider, run: impl FnOnce()) {
        let _guard = self.slots[Self::index(provider)].lock().unwrap();
        run();
    }

    pub(crate) fn test_seed(&self, provider: Provider, sample: ReaderSnapshot) {
        self.slots[Self::index(provider)].lock().unwrap().totals = sample;
    }

    pub(crate) fn test_unknown(&self, provider: Provider) {
        self.unknown_worker[Self::index(provider)].store(true, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ReadCycle, UsageOutcome,
        tests::{identity, request},
    };
    use std::path::Path;

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
}
