//! Detached session-local daemon lifecycle and replaceable publication hooks.

use crate::{
    config::Config,
    doctor,
    lock::{LifecycleLock, ProcessExit, RuntimePaths, SessionLock, StartupLockContention},
    socket::{EndpointIdentity, Transport, read_frame, write_frame},
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufReader, Read, Write},
    os::unix::{
        fs::OpenOptionsExt,
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const PLUGIN_ID: &str = "herdr-lineup";
const START_DEADLINE: Duration = Duration::from_secs(5);
const STOP_DEADLINE: Duration = Duration::from_secs(5);
const IDLE_WAIT: Duration = Duration::from_millis(100);
const RECONNECT_RETRY: Duration = Duration::from_millis(250);
const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub(crate) const LOCK_CONTENDED_EXIT_CODE: u8 = 75;

#[derive(Debug, Clone)]
pub struct Options {
    pub socket: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
}
impl Options {
    pub fn from_env() -> io::Result<Self> {
        let required = |key| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or_else(|| io::Error::other(format!("{key} must be an absolute path")))
        };
        Ok(Self {
            socket: required("HERDR_SOCKET_PATH")?,
            state: required("HERDR_PLUGIN_STATE_DIR")?,
            config: required("HERDR_PLUGIN_CONFIG_DIR")?,
        })
    }
}

/// Opaque memory-only access to advisory reader observations.
/// Cloning this handle does not join workers or acquire provider locks.
#[derive(Clone)]
pub struct ReaderDiagnosticsHandle {
    diagnostics: Arc<crate::model::ReaderDiagnostics>,
    #[cfg(test)]
    samples: Arc<std::sync::atomic::AtomicUsize>,
}

impl std::fmt::Debug for ReaderDiagnosticsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderDiagnosticsHandle(opaque)")
    }
}

impl ReaderDiagnosticsHandle {
    pub(crate) fn new(diagnostics: Arc<crate::model::ReaderDiagnostics>) -> Self {
        Self {
            diagnostics,
            #[cfg(test)]
            samples: Arc::default(),
        }
    }

    fn sample(&self) -> ReaderDiagnosticsData {
        #[cfg(test)]
        self.samples.fetch_add(1, Ordering::Relaxed);
        let providers = self
            .diagnostics
            .snapshots()
            .into_iter()
            .map(|(provider, result)| {
                use crate::model::{DiagnosticUnavailable, Provider};
                let label = match provider {
                    Provider::Claude => "claude",
                    Provider::Agy => "agy",
                    Provider::Codex => "codex",
                    Provider::Grok => "grok",
                    Provider::Hermes => "hermes",
                    Provider::Kimi => "kimi",
                };
                let entry = match result {
                    Ok(sample) => ReaderEntry::Available {
                        sample: ReaderSample::from(sample),
                    },
                    Err(DiagnosticUnavailable::Contended) => ReaderEntry::Contended,
                    Err(DiagnosticUnavailable::Poisoned) => ReaderEntry::Poisoned,
                    Err(DiagnosticUnavailable::Incomplete) => ReaderEntry::Incomplete,
                };
                (label, entry)
            })
            .collect();
        ReaderDiagnosticsData {
            version: 1,
            sampling: "live_best_effort",
            providers,
        }
    }
}

pub(crate) const READER_LABELS: [&str; 6] = ["claude", "agy", "codex", "grok", "hermes", "kimi"];

/// Non-sensitive wire DTO: never serialize a model object or arbitrary labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReaderSample {
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

impl From<crate::model::ReaderSnapshot> for ReaderSample {
    fn from(s: crate::model::ReaderSnapshot) -> Self {
        Self {
            attempts: s.attempts,
            busy_rejections: s.busy_rejections,
            deadline_ends: s.deadline_ends,
            shutdown_ends: s.shutdown_ends,
            spawn_failures: s.spawn_failures,
            worker_failures: s.worker_failures,
            completed: s.completed,
            invalid: s.invalid,
            caller_wait_count: s.caller_wait_count,
            caller_wait_total_ns: s.caller_wait_total_ns,
            caller_wait_max_ns: s.caller_wait_max_ns,
            sampling_skips: s.sampling_skips,
            worker_active: s.worker_active,
            timed_out_worker_active: s.timed_out_worker_active,
            shutdown_worker_active: s.shutdown_worker_active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ReaderEntry {
    Available { sample: ReaderSample },
    Contended,
    Poisoned,
    Incomplete,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReaderDiagnosticsData {
    version: u64,
    sampling: &'static str,
    pub providers: std::collections::BTreeMap<&'static str, ReaderEntry>,
}

fn reader_diagnostics_from_raw(raw: &serde_json::Value) -> Option<ReaderDiagnosticsData> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        version: u64,
        sampling: String,
        providers: serde_json::Map<String, serde_json::Value>,
    }
    let envelope: Envelope = serde_json::from_value(raw.clone()).ok()?;
    if envelope.version != 1 || envelope.sampling != "live_best_effort" {
        return None;
    }
    let providers = READER_LABELS
        .into_iter()
        .map(|label| {
            let entry = envelope
                .providers
                .get(label)
                .and_then(|value| serde_json::from_value::<ReaderEntry>(value.clone()).ok())
                .filter(|entry| match entry {
                    ReaderEntry::Available { sample } => {
                        sample.worker_active
                            || !(sample.timed_out_worker_active || sample.shutdown_worker_active)
                    }
                    _ => true,
                })
                .unwrap_or(ReaderEntry::Unavailable);
            (label, entry)
        })
        .collect();
    Some(ReaderDiagnosticsData {
        version: 1,
        sampling: "live_best_effort",
        providers,
    })
}

/// Implementations must bound I/O and clear only plugin-owned metadata.
pub trait Publisher {
    /// Advisory reader state; unavailable for publishers without a registry.
    fn reader_diagnostics(&self) -> Option<ReaderDiagnosticsHandle> {
        None
    }
    /// Observe shutdown between bounded reader calls and complete metadata frames.
    fn set_shutdown(&mut self, _shutdown: Arc<AtomicBool>) {}
    /// Stop owned background queries on every exit, without joining HTTP I/O.
    /// Separate provider child processes must be killed and reaped here.
    fn shutdown(&mut self) {}
    fn refresh(&mut self, transport: &mut Transport, config: &Config) -> io::Result<()>;
    fn clear(&mut self, transport: &mut Transport) -> io::Result<()>;
    /// Drop any cached pane associations before authoritative reconciliation.
    fn reconnected(&mut self) {}
    /// Timings and counts for the refresh that just finished.
    ///
    /// `None` means this publisher does not measure stages. Callers must treat
    /// that as unavailable, not as a zero sample. Durations are microseconds.
    /// Counts are report and checkpoint totals of that refresh only.
    fn refresh_stages(&self) -> Option<RefreshStages> {
        None
    }
}
#[cfg(test)]
struct NoopPublisher;
#[cfg(test)]
impl Publisher for NoopPublisher {
    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
        Ok(())
    }
    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub plugin: String,
    pub socket_path: PathBuf,
    pub socket_key: String,
    pub endpoint: EndpointIdentity,
    pub nonce: String,
    pub pid: u32,
    pub last_refresh_ms: Option<u64>,
    pub refresh_failed: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub capacity_skipped_resources: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Histogram bounds in milliseconds. Comparisons use microseconds so a
/// sub-millisecond sample stays in `<1ms` without rounding `last_us`.
const REFRESH_DURATION_UPPER_US: [u64; 8] = [
    1_000, 5_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 5_000_000,
];
pub(crate) const REFRESH_DURATION_BUCKETS: usize = 9;
pub(crate) const REFRESH_DURATION_BUCKET_LABELS: [&str; REFRESH_DURATION_BUCKETS] = [
    "<1ms", "<5ms", "<10ms", "<50ms", "<100ms", "<500ms", "<1000ms", "<5000ms", ">=5000ms",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshStageDurations {
    pub snapshot_us: u64,
    pub git_us: u64,
    pub reader_us: u64,
    pub persist_us: u64,
    pub socket_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshStageCounts {
    pub reports: u64,
    pub checkpoints: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshStages {
    pub durations: RefreshStageDurations,
    pub counts: RefreshStageCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshDuration {
    pub last_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub histogram: [u64; REFRESH_DURATION_BUCKETS],
}

/// One measured stage, in microseconds, using the same buckets as the total.
///
/// `count` is the number of completed attempts that supplied this stage.
/// `sum_us` saturates. Absence of the parent aggregate is unmeasured, not zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageDurationStats {
    pub count: u64,
    pub sum_us: u64,
    pub duration: RefreshDuration,
}

/// Histograms for the five measured refresh stages.
///
/// `latest_measured` is false when the most recent completed attempt returned
/// no stage sample. The histograms still describe earlier measured attempts.
/// `reports` and `checkpoints` are saturating sums of those measured attempts,
/// not extra stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshStageAggregate {
    pub snapshot: StageDurationStats,
    pub git: StageDurationStats,
    pub reader: StageDurationStats,
    pub persist: StageDurationStats,
    pub socket: StageDurationStats,
    pub latest_measured: bool,
    pub reports: u64,
    pub checkpoints: u64,
}

/// Memory-only totals for completed refresh attempts.
///
/// Absence of this object on the control wire means the peer did not measure
/// attempts. Absence of `duration` means no monotonic sample exists yet.
/// Absence of `stages` means no stage sample has been measured yet.
/// None of those cases is a zero measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshObservation {
    pub attempt_count: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub capacity_skip_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<RefreshDuration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stages: Option<RefreshStageAggregate>,
}

/// Memory-only totals for refresh attempts classified as deadline skips.
///
/// Sent as the optional `refresh_deadline_skips` control field beside
/// `refresh_observation`, so `Health` and `RefreshObservation` keep their
/// shape and an older peer still parses the frame. Absence means no deadline
/// skip has happened or the peer does not report them, never a measured zero.
/// These attempts are included in `refresh_observation.attempt_count` and its
/// duration and stage histograms, but not in its success, error or capacity
/// skip counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeadlineSkipObservation {
    pub attempt_count: u64,
    /// Resources the latest deadline-skip attempt skipped by its deadline.
    pub last_deadline_skipped: u64,
    /// Resources the same attempt skipped for capacity. Zero is measured.
    pub last_capacity_skipped: u64,
}

#[derive(Debug, Clone)]
struct ControlState {
    health: Health,
    observation: Option<RefreshObservation>,
    deadline_skips: Option<DeadlineSkipObservation>,
    reader_diagnostics: Option<ReaderDiagnosticsHandle>,
}

impl From<Health> for ControlState {
    fn from(health: Health) -> Self {
        Self {
            health,
            observation: None,
            deadline_skips: None,
            reader_diagnostics: None,
        }
    }
}

#[derive(Serialize)]
struct ControlResponse<'a> {
    #[serde(flatten)]
    health: &'a Health,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_observation: Option<&'a RefreshObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_deadline_skips: Option<&'a DeadlineSkipObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reader_diagnostics: Option<&'a ReaderDiagnosticsData>,
}

#[derive(Debug)]
pub(crate) struct ControlView {
    pub health: Health,
    pub observation: Option<RefreshObservation>,
    pub deadline_skips: Option<DeadlineSkipObservation>,
    pub reader_diagnostics: Option<ReaderDiagnosticsData>,
}

struct MeasuredRefresh<'a> {
    result: &'a io::Result<()>,
    uptime_ms: u64,
    duration_us: Option<u64>,
    stages: Option<RefreshStages>,
}

#[derive(Clone, Copy)]
enum RefreshOutcome {
    Success,
    Error,
    CapacitySkip,
    /// At least one resource was skipped by the cycle deadline. Capacity skips
    /// in the same attempt keep their own count.
    DeadlineSkip(crate::publish::RefreshSkip),
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Subtract the monotonic samples first, then convert. Truncating each side to
/// milliseconds before subtraction collapses a 10.9 ms to 11.1 ms attempt to 1 ms.
fn attempt_duration_us(started: Duration, finished: Duration) -> Option<u64> {
    finished.checked_sub(started).map(duration_micros)
}

fn refresh_outcome(result: &io::Result<()>) -> RefreshOutcome {
    match result {
        Ok(()) => RefreshOutcome::Success,
        // Shutdown, poison and ambiguous exchanges return their own errors and
        // never carry a skip payload.
        Err(error) => match crate::publish::refresh_skip(error) {
            Some(skip) if skip.deadline_skipped > 0 => RefreshOutcome::DeadlineSkip(skip),
            Some(_) => RefreshOutcome::CapacitySkip,
            None => RefreshOutcome::Error,
        },
    }
}

pub(crate) fn duration_bucket(duration_us: u64) -> usize {
    REFRESH_DURATION_UPPER_US
        .iter()
        .position(|upper| duration_us < *upper)
        .unwrap_or(REFRESH_DURATION_BUCKETS - 1)
}

fn observe_duration(stats: &mut RefreshDuration, duration_us: u64) {
    stats.last_us = duration_us;
    stats.min_us = stats.min_us.min(duration_us);
    stats.max_us = stats.max_us.max(duration_us);
    let index = duration_bucket(duration_us);
    stats.histogram[index] = stats.histogram[index].saturating_add(1);
}

fn new_stage_stats(duration_us: u64) -> StageDurationStats {
    let mut duration = RefreshDuration {
        last_us: duration_us,
        min_us: duration_us,
        max_us: duration_us,
        histogram: [0; REFRESH_DURATION_BUCKETS],
    };
    observe_duration(&mut duration, duration_us);
    StageDurationStats {
        count: 1,
        sum_us: duration_us,
        duration,
    }
}

fn observe_stage(stats: &mut StageDurationStats, duration_us: u64) {
    stats.count = stats.count.saturating_add(1);
    stats.sum_us = stats.sum_us.saturating_add(duration_us);
    observe_duration(&mut stats.duration, duration_us);
}

fn stage_aggregate_from_sample(sample: RefreshStages) -> RefreshStageAggregate {
    RefreshStageAggregate {
        snapshot: new_stage_stats(sample.durations.snapshot_us),
        git: new_stage_stats(sample.durations.git_us),
        reader: new_stage_stats(sample.durations.reader_us),
        persist: new_stage_stats(sample.durations.persist_us),
        socket: new_stage_stats(sample.durations.socket_us),
        latest_measured: true,
        reports: sample.counts.reports,
        checkpoints: sample.counts.checkpoints,
    }
}

fn observe_stages(aggregate: &mut RefreshStageAggregate, sample: RefreshStages) {
    observe_stage(&mut aggregate.snapshot, sample.durations.snapshot_us);
    observe_stage(&mut aggregate.git, sample.durations.git_us);
    observe_stage(&mut aggregate.reader, sample.durations.reader_us);
    observe_stage(&mut aggregate.persist, sample.durations.persist_us);
    observe_stage(&mut aggregate.socket, sample.durations.socket_us);
    aggregate.latest_measured = true;
    aggregate.reports = aggregate.reports.saturating_add(sample.counts.reports);
    aggregate.checkpoints = aggregate
        .checkpoints
        .saturating_add(sample.counts.checkpoints);
}

pub(crate) fn record_refresh(health: &mut Health, result: &io::Result<()>, elapsed_ms: u64) {
    match refresh_outcome(result) {
        RefreshOutcome::Success => {
            health.last_refresh_ms = Some(elapsed_ms);
            health.refresh_failed = false;
            health.capacity_skipped_resources = 0;
        }
        RefreshOutcome::CapacitySkip => {
            let count = result
                .as_ref()
                .err()
                .and_then(crate::publish::refresh_skip)
                .map_or(0, |skip| skip.capacity_skipped);
            health.last_refresh_ms = Some(elapsed_ms);
            health.refresh_failed = false;
            health.capacity_skipped_resources = count;
        }
        RefreshOutcome::DeadlineSkip(skip) => {
            health.last_refresh_ms = Some(elapsed_ms);
            health.refresh_failed = false;
            health.capacity_skipped_resources = skip.capacity_skipped;
        }
        RefreshOutcome::Error => {
            health.refresh_failed = true;
            health.capacity_skipped_resources = 0;
        }
    }
}

fn note_refresh_attempt(
    health: &mut Health,
    observation: &mut Option<RefreshObservation>,
    measured: MeasuredRefresh<'_>,
) {
    record_refresh(health, measured.result, measured.uptime_ms);
    let mut current = observation.take().unwrap_or(RefreshObservation {
        attempt_count: 0,
        success_count: 0,
        error_count: 0,
        capacity_skip_count: 0,
        duration: None,
        stages: None,
    });
    current.attempt_count = current.attempt_count.saturating_add(1);
    match refresh_outcome(measured.result) {
        RefreshOutcome::Success => {
            current.success_count = current.success_count.saturating_add(1);
        }
        RefreshOutcome::Error => {
            current.error_count = current.error_count.saturating_add(1);
        }
        RefreshOutcome::CapacitySkip => {
            current.capacity_skip_count = current.capacity_skip_count.saturating_add(1);
        }
        // Counted only in the attempt total here; see `note_deadline_skip`.
        RefreshOutcome::DeadlineSkip(_) => {}
    }
    if let Some(duration_us) = measured.duration_us {
        let mut stats = current.duration.take().unwrap_or(RefreshDuration {
            last_us: duration_us,
            min_us: duration_us,
            max_us: duration_us,
            histogram: [0; REFRESH_DURATION_BUCKETS],
        });
        observe_duration(&mut stats, duration_us);
        current.duration = Some(stats);
    }
    match measured.stages {
        Some(sample) => match current.stages.as_mut() {
            Some(aggregate) => observe_stages(aggregate, sample),
            None => current.stages = Some(stage_aggregate_from_sample(sample)),
        },
        None => {
            if let Some(aggregate) = current.stages.as_mut() {
                aggregate.latest_measured = false;
            }
        }
    }
    *observation = Some(current);
}

fn note_deadline_skip(
    deadline_skips: &mut Option<DeadlineSkipObservation>,
    result: &io::Result<()>,
) {
    if let RefreshOutcome::DeadlineSkip(skip) = refresh_outcome(result) {
        let attempt_count = deadline_skips
            .as_ref()
            .map_or(0, |previous| previous.attempt_count)
            .saturating_add(1);
        *deadline_skips = Some(DeadlineSkipObservation {
            attempt_count,
            last_deadline_skipped: skip.deadline_skipped,
            last_capacity_skipped: skip.capacity_skipped,
        });
    }
}

fn stage_parts(aggregate: &RefreshStageAggregate) -> [&StageDurationStats; 5] {
    [
        &aggregate.snapshot,
        &aggregate.git,
        &aggregate.reader,
        &aggregate.persist,
        &aggregate.socket,
    ]
}

fn histogram_total(histogram: &[u64; REFRESH_DURATION_BUCKETS]) -> Option<u64> {
    histogram
        .iter()
        .try_fold(0u64, |total, bucket| total.checked_add(*bucket))
}

/// Occupied buckets must overlap `[min_us, max_us]`, and the buckets that
/// contain last, min, and max must hold a real observation. A saturated
/// `u64::MAX` count does not make an impossible bucket legal.
fn buckets_match_extrema(duration: &RefreshDuration) -> bool {
    let min_bucket = duration_bucket(duration.min_us);
    let max_bucket = duration_bucket(duration.max_us);
    let last_bucket = duration_bucket(duration.last_us);
    if duration.histogram[min_bucket] == 0
        || duration.histogram[max_bucket] == 0
        || duration.histogram[last_bucket] == 0
    {
        return false;
    }
    duration
        .histogram
        .iter()
        .enumerate()
        .all(|(index, count)| *count == 0 || (min_bucket..=max_bucket).contains(&index))
}

fn stage_duration_consistent(stats: &StageDurationStats) -> bool {
    let duration = &stats.duration;
    if stats.count == 0 {
        return stats.sum_us == 0
            && duration.last_us == 0
            && duration.min_us == 0
            && duration.max_us == 0
            && duration.histogram.iter().all(|count| *count == 0);
    }
    if duration.min_us > duration.max_us
        || duration.last_us < duration.min_us
        || duration.last_us > duration.max_us
        || !buckets_match_extrema(duration)
    {
        return false;
    }
    if stats.count == 1 && stats.sum_us != duration.last_us {
        return false;
    }
    if let Some(lower) = duration.min_us.checked_mul(stats.count)
        && stats.sum_us != u64::MAX
        && stats.sum_us < lower
    {
        return false;
    }
    if let Some(upper) = duration.max_us.checked_mul(stats.count)
        && stats.sum_us > upper
    {
        return false;
    }
    match histogram_total(&duration.histogram) {
        Some(total) if total == stats.count => true,
        // `u64::MAX - 1` samples in one bucket and two in the next overflow the
        // stored total without any single bucket reaching `u64::MAX`. The
        // attempt count is saturated. Extrema checks above still reject an
        // occupied bucket outside min/max.
        None if stats.count == u64::MAX => true,
        _ => false,
    }
}

fn stage_aggregate_consistent(aggregate: &RefreshStageAggregate) -> bool {
    let parts = stage_parts(aggregate);
    if !parts.iter().all(|stage| stage_duration_consistent(stage)) {
        return false;
    }
    let count = parts[0].count;
    count > 0 && parts.iter().all(|stage| stage.count == count)
}

fn stage_aggregate_from_value(value: &serde_json::Value) -> Option<RefreshStageAggregate> {
    let aggregate: RefreshStageAggregate = serde_json::from_value(value.clone()).ok()?;
    stage_aggregate_consistent(&aggregate).then_some(aggregate)
}

/// A missing, legacy, unknown, or contradictory `stages` object is unavailable.
/// It does not erase a valid total, and it is not stored as a zero histogram.
fn observation_from_raw(raw: &serde_json::Value) -> Option<RefreshObservation> {
    let mut object = raw.as_object()?.clone();
    let stages_value = object.remove("stages");
    let mut observation: RefreshObservation =
        serde_json::from_value(serde_json::Value::Object(object)).ok()?;
    observation.stages = match stages_value {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => stage_aggregate_from_value(&value),
    };
    Some(observation)
}

fn control_frame(state: &ControlState) -> io::Result<Vec<u8>> {
    // Exactly one live sample per response, reused throughout size fallback.
    let readers = state
        .reader_diagnostics
        .as_ref()
        .map(ReaderDiagnosticsHandle::sample);
    let mut response = ControlResponse {
        health: &state.health,
        refresh_observation: state.observation.as_ref(),
        refresh_deadline_skips: state.deadline_skips.as_ref(),
        reader_diagnostics: readers.as_ref(),
    };
    for step in 0..3 {
        let bytes = serde_json::to_vec(&response)
            .map_err(|_| io::Error::other("invalid daemon control snapshot"))?;
        // write_frame appends one newline, and read_frame counts that byte.
        if bytes.len() < 8192 {
            return Ok(bytes);
        }
        match step {
            0 => response.reader_diagnostics = None,
            1 => {
                response.refresh_observation = None;
                response.refresh_deadline_skips = None;
            }
            _ => break,
        }
    }
    Err(io::Error::other(
        "daemon control health exceeds frame limit",
    ))
}

pub(crate) fn control_view_from_frame(frame: &[u8]) -> io::Result<ControlView> {
    let value: serde_json::Value = serde_json::from_slice(frame)
        .map_err(|_| io::Error::other("invalid daemon control identity"))?;
    let health: Health = serde_json::from_value(value.clone())
        .map_err(|_| io::Error::other("invalid daemon control identity"))?;
    let observation = match value.get("refresh_observation") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => observation_from_raw(raw),
    };
    let deadline_skips = value
        .get("refresh_deadline_skips")
        .and_then(|raw| serde_json::from_value(raw.clone()).ok());
    let reader_diagnostics = value
        .get("reader_diagnostics")
        .and_then(reader_diagnostics_from_raw);
    Ok(ControlView {
        health,
        observation,
        deadline_skips,
        reader_diagnostics,
    })
}

/// Typed seam for doctor: the optional deadline skip field of a control
/// frame. Absent, null or invalid reads as unavailable, never as zero.
#[cfg(test)]
pub(crate) fn deadline_skips_from_frame(frame: &[u8]) -> Option<DeadlineSkipObservation> {
    let value: serde_json::Value = serde_json::from_slice(frame).ok()?;
    serde_json::from_value(value.get("refresh_deadline_skips")?.clone()).ok()
}

pub fn ensure(options: &Options) -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("daemon");
    ensure_command(options, command)
}
fn ensure_command(options: &Options, mut command: Command) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    paths.validate_control_socket_path()?;
    Config::load(&options.config)?;
    let endpoint = EndpointIdentity::read(&options.socket)?;
    let deadline = Instant::now() + START_DEADLINE;
    let _lifecycle = LifecycleLock::acquire(&paths, deadline)?;
    let mut may_spawn = true;
    loop {
        if may_spawn && let Some(lock) = SessionLock::acquire(&paths)? {
            // A reused PID record is never inspected or signalled. Stale control
            // paths are removed only after exclusive session ownership is acquired.
            log_event(&paths, "daemon_start", "ensure")?;
            command
                .current_dir(&paths.root)
                .env("HERDR_SOCKET_PATH", &options.socket)
                .env("HERDR_PLUGIN_STATE_DIR", &options.state)
                .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
                .env("LINEUP_INHERITED_LOCK", "3")
                .env("LINEUP_EXPECTED_DEVICE", endpoint.device.to_string())
                .env("LINEUP_EXPECTED_INODE", endpoint.inode.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let descriptor = lock.raw_fd();
            // SAFETY: after fork, only async-signal-safe libc functions execute.
            // dup2 preserves flock's shared open-file description across exec.
            unsafe {
                command.pre_exec(move || {
                    if libc::setsid() < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if descriptor != 3 && libc::dup2(descriptor, 3) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn()?;
            // Close the parent's descriptor; never issue LOCK_UN on a transfer.
            drop(lock);
            loop {
                if let Some(health) = probe_starting(&paths, &options.socket)? {
                    if health.endpoint == endpoint {
                        if health.pid == child.id() {
                            return Ok(());
                        }
                        // A healthy competing daemon does not prove our child
                        // has exited. Reap the typed loser before returning.
                    } else {
                        return Err(io::Error::other("daemon endpoint identity mismatch"));
                    }
                }
                if let Some(status) = child.try_wait()? {
                    if status.code() == Some(i32::from(LOCK_CONTENDED_EXIT_CODE)) {
                        // Only the typed lock-acquisition failure uses this
                        // status. Wait for the winner; do not spawn another
                        // child or reuse pre_exec's transferred descriptor.
                        may_spawn = false;
                        break;
                    }
                    let _ = log_event(&paths, "daemon_exit", "startup_failure");
                    return Err(io::Error::other(
                        "daemon exited before readiness; inspect session daemon.log",
                    ));
                }
                if Instant::now() >= deadline {
                    let _ = log_event(&paths, "daemon_exit", "startup_timeout");
                    return Err(io::Error::other(
                        "daemon startup timed out; inspect session daemon.log",
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        if let Some(health) = probe_starting(&paths, &options.socket)? {
            if health.endpoint == endpoint {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "live daemon belongs to a replaced endpoint",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "live lock has no healthy control endpoint within startup deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn foreground(options: &Options) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    paths.create()?;
    std::env::set_current_dir(&paths.root)?;
    let mut publisher = crate::publish::MetadataPublisher::new(
        paths.root.clone(),
        paths.key.clone(),
        crate::publish::Homes::from_env(),
    )?;
    let result = run_foreground(
        options,
        std::env::var_os("LINEUP_INHERITED_LOCK").as_deref() == Some(std::ffi::OsStr::new("3")),
        &mut publisher,
    );
    if result.is_err() {
        let _ = log_event(&paths, "daemon_exit", "runtime_error");
    }
    result
}
pub fn run_foreground(
    options: &Options,
    inherited: bool,
    publisher: &mut impl Publisher,
) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let lock = if inherited {
        SessionLock::inherited(&paths)?
    } else {
        SessionLock::acquire(&paths)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, StartupLockContention))?
    };
    run_locked(options, inherited, config, paths, lock, publisher)
}

fn run_locked(
    options: &Options,
    inherited: bool,
    config: Config,
    paths: RuntimePaths,
    _lock: SessionLock,
    publisher: &mut impl Publisher,
) -> io::Result<()> {
    let mut transport = Transport::connect(&options.socket)?;
    if inherited {
        let parse = |key| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| io::Error::other("missing inherited endpoint identity"))
        };
        let expected = EndpointIdentity {
            device: parse("LINEUP_EXPECTED_DEVICE")?,
            inode: parse("LINEUP_EXPECTED_INODE")?,
        };
        if transport.identity() != expected {
            return Err(io::Error::other("socket replaced during detached startup"));
        }
    }
    doctor::check_transport(&mut transport)?;
    remove_if_present(&paths.control)?;
    let listener = UnixListener::bind(&paths.control)?;
    listener.set_nonblocking(true)?;
    let _cleanup = RuntimeCleanup {
        paths: paths.clone(),
    };
    let mut random = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut state = ControlState::from(Health {
        plugin: PLUGIN_ID.into(),
        socket_path: options.socket.clone(),
        socket_key: paths.key.clone(),
        endpoint: transport.identity(),
        nonce,
        pid: std::process::id(),
        last_refresh_ms: None,
        refresh_failed: false,
        capacity_skipped_resources: 0,
    });
    state.reader_diagnostics = publisher.reader_diagnostics();
    fs::write(&paths.pid, format!("{}\n", state.health.pid))?;
    let control = ControlWorker::start(listener, state.clone());
    publisher.set_shutdown(control.shutdown.clone());
    let started = Instant::now();
    let mut refresh_at = Instant::now();
    let mut reconnect = false;
    let mut socket_missing_since: Option<Instant> = None;
    let socket_missing_ttl = Duration::from_millis(config.refresh_interval_ms.saturating_mul(3));
    let result = (|| {
        loop {
            if control.shutdown.load(Ordering::Acquire) {
                // Finish the current bounded operation before clearing complete frames.
                if transport.check_connection().is_ok() {
                    publisher.clear(&mut transport)?;
                }
                return Ok(());
            }
            if control_worker_exited_unexpectedly(&control.shutdown, || {
                control
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished())
            }) {
                return Err(io::Error::other(
                    "daemon control worker exited unexpectedly",
                ));
            }
            if reconnect || transport.check_connection().is_err() {
                // Transport treats v0.9.3 EOF after a response as normal and opens
                // each next request on the pinned dev/inode. This recovery path is
                // for unexpected failures; a replaced server socket stops us.
                let retrying_refresh = reconnect;
                if let Err(error) = transport
                    .reconnect_same_endpoint()
                    .and_then(|_| doctor::check_transport(&mut transport))
                {
                    if transport.endpoint_replaced() {
                        return Err(error);
                    }
                    let now = Instant::now();
                    match fs::symlink_metadata(&options.socket) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            let since = socket_missing_since.get_or_insert(now);
                            if now.duration_since(*since) >= socket_missing_ttl {
                                let _ = log_event(&paths, "daemon_exit", "socket_missing_ttl");
                                return Ok(());
                            }
                        }
                        _ => socket_missing_since = None,
                    }
                    reconnect = true;
                    *control.state.lock().unwrap() = state.clone();
                    std::thread::sleep(RECONNECT_RETRY);
                    continue;
                }
                socket_missing_since = None;
                publisher.reconnected();
                reconnect = false;
                if !retrying_refresh {
                    refresh_at = Instant::now();
                }
            }
            if Instant::now() >= refresh_at {
                let attempt_started = started.elapsed();
                let result = publisher.refresh(&mut transport, &config);
                let finished = started.elapsed();
                note_refresh_attempt(
                    &mut state.health,
                    &mut state.observation,
                    MeasuredRefresh {
                        result: &result,
                        uptime_ms: duration_millis(finished),
                        duration_us: attempt_duration_us(attempt_started, finished),
                        stages: publisher.refresh_stages(),
                    },
                );
                note_deadline_skip(&mut state.deadline_skips, &result);
                if let Err(error) = result {
                    if let Some(skip) = crate::publish::refresh_skip(&error) {
                        // Neither skip reconnects; the frame was never written.
                        if skip.deadline_skipped > 0 {
                            let _ = log_event(&paths, "refresh_skipped", "cycle_deadline");
                        }
                        if skip.capacity_skipped > 0 {
                            let _ = log_event(&paths, "refresh_skipped", "metadata_capacity");
                        }
                    } else if error.kind() == io::ErrorKind::Interrupted
                        && control.shutdown.load(Ordering::Acquire)
                    {
                        let _ = log_event(&paths, "refresh_interrupted", "shutdown");
                    } else {
                        reconnect = error.kind() != io::ErrorKind::InvalidInput;
                        let reason = match error.kind() {
                            io::ErrorKind::InvalidInput => "invalid_input",
                            io::ErrorKind::Interrupted => "interrupted",
                            _ => "transport_or_reader",
                        };
                        let _ = log_event(&paths, "refresh_failed", reason);
                    }
                }
                refresh_at = Instant::now() + Duration::from_millis(config.refresh_interval_ms);
            }
            *control.state.lock().unwrap() = state.clone();
            if control.shutdown.load(Ordering::Acquire) {
                continue;
            }
            std::thread::sleep(IDLE_WAIT);
        }
    })();
    control.shutdown.store(true, Ordering::Release);
    publisher.shutdown();
    result
}
fn control_worker_exited_unexpectedly(
    shutdown: &AtomicBool,
    is_finished: impl FnOnce() -> bool,
) -> bool {
    is_finished() && !shutdown.load(Ordering::Acquire)
}
/// Own the control listener independently of publication. Drop joins the worker
/// before runtime paths are removed and session ownership is released.
struct ControlWorker {
    state: Arc<Mutex<ControlState>>,
    shutdown: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<io::Result<()>>>,
}
impl ControlWorker {
    fn start(listener: UnixListener, state: ControlState) -> Self {
        let state = Arc::new(Mutex::new(state));
        let shutdown = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_shutdown = shutdown.clone();
        let worker_done = done.clone();
        let worker = std::thread::spawn(move || {
            use std::os::fd::AsRawFd;
            while !worker_done.load(Ordering::Acquire) {
                let mut descriptor = libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // Bound drop's join while sleeping until a client arrives.
                let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
                if ready < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                if ready == 0 {
                    continue;
                }
                if descriptor.revents & libc::POLLIN == 0 {
                    return Err(io::Error::other("daemon control listener poll failed"));
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Never hold the snapshot mutex during I/O or a refresh.
                        let snapshot = worker_state.lock().unwrap().clone();
                        if serve_control(stream, &snapshot)? {
                            worker_shutdown.store(true, Ordering::Release);
                            return Ok(());
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        });
        Self {
            state,
            shutdown,
            done,
            worker: Some(worker),
        }
    }
}
impl Drop for ControlWorker {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn remove_if_present(path: &std::path::Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
fn log_event(paths: &RuntimePaths, event: &'static str, reason: &'static str) -> io::Result<()> {
    let line = format!(
        "{} event={event} reason={reason}\n",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    );
    let existing = match fs::symlink_metadata(&paths.log) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => return Err(io::Error::other("daemon log is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if existing.saturating_add(line.len() as u64) > MAX_LOG_BYTES {
        fs::rename(&paths.log, paths.root.join("daemon.log.1"))?;
    }
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&paths.log)?
        .write_all(line.as_bytes())
}
struct RuntimeCleanup {
    paths: RuntimePaths,
}
impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        let _ = remove_if_present(&self.paths.control);
        let _ = remove_if_present(&self.paths.pid);
    }
}
fn control_stream(paths: &RuntimePaths) -> io::Result<Option<BufReader<UnixStream>>> {
    match crate::socket::connect_stream(&paths.control) {
        Ok(stream) => {
            // Frame deadlines are poll-based inside read_frame and write_frame.
            // setsockopt timeouts return EINVAL on macOS after the peer closes.
            Ok(Some(BufReader::new(stream)))
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
#[derive(Debug)]
struct ReplacedEndpoint;
impl std::fmt::Display for ReplacedEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("daemon belongs to a replaced socket endpoint")
    }
}
impl std::error::Error for ReplacedEndpoint {}
fn read_control(
    stream: &mut BufReader<UnixStream>,
    paths: &RuntimePaths,
    socket: &std::path::Path,
) -> io::Result<ControlView> {
    write_frame(stream.get_mut(), b"health")?;
    let view = control_view_from_frame(&read_frame(stream, 8192)?)?;
    let health = &view.health;
    if health.plugin != PLUGIN_ID
        || health.socket_key != paths.key
        || health.socket_path != socket
        || health.nonce.len() != 64
        || !health.nonce.bytes().all(|c| c.is_ascii_hexdigit())
        || !SessionLock::is_held(paths)?
    {
        return Err(io::Error::other(
            "daemon control identity or live lock mismatch",
        ));
    }
    if let Ok(current) = EndpointIdentity::read(socket)
        && current != health.endpoint
    {
        return Err(io::Error::other(ReplacedEndpoint));
    }
    Ok(view)
}
fn read_identity(
    stream: &mut BufReader<UnixStream>,
    paths: &RuntimePaths,
    socket: &std::path::Path,
) -> io::Result<Health> {
    read_control(stream, paths, socket).map(|view| view.health)
}
pub(crate) fn probe_view(
    paths: &RuntimePaths,
    socket: &std::path::Path,
) -> io::Result<Option<ControlView>> {
    let Some(mut stream) = control_stream(paths)? else {
        return Ok(None);
    };
    read_control(&mut stream, paths, socket).map(Some)
}
pub fn probe(paths: &RuntimePaths, socket: &std::path::Path) -> io::Result<Option<Health>> {
    probe_view(paths, socket).map(|view| view.map(|view| view.health))
}
fn transient_control_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
    )
}
fn probe_starting(paths: &RuntimePaths, socket: &std::path::Path) -> io::Result<Option<Health>> {
    match probe(paths, socket) {
        // The serial handler may be busy with another probe/stop or refresh.
        // Wait for a typed endpoint replacement while its old owner exits;
        // all other invalid identities remain fatal.
        Err(error) if transient_control_error(&error) => Ok(None),
        Err(error)
            if error
                .get_ref()
                .is_some_and(|source| source.downcast_ref::<ReplacedEndpoint>().is_some()) =>
        {
            Ok(None)
        }
        result => result,
    }
}
pub fn stop(options: &Options) -> io::Result<()> {
    stop_with_lock(options, || {})
}
fn stop_with_lock(options: &Options, after_lock: impl FnOnce()) -> io::Result<()> {
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let deadline = Instant::now() + STOP_DEADLINE;
    let _lifecycle = LifecycleLock::acquire(&paths, deadline)?;
    after_lock();
    let Some(mut stream) = control_stream(&paths)? else {
        return if SessionLock::is_held(&paths)? {
            Err(io::Error::other(
                "session lock is live but control is unavailable; no signal sent",
            ))
        } else {
            Ok(())
        };
    };
    let identity = read_identity(&mut stream, &paths, &options.socket)?;
    verify_peer_pid(stream.get_ref(), identity.pid)?;
    let process = ProcessExit::observe(identity.pid)?;
    let request = serde_json::json!({"stop":identity.nonce,"plugin":PLUGIN_ID,"socket_key":paths.key,"endpoint":identity.endpoint});
    write_frame(stream.get_mut(), &serde_json::to_vec(&request)?)?;
    if read_frame(&mut stream, 8192)? != b"stopping" {
        return Err(io::Error::other("daemon refused shutdown identity"));
    }
    loop {
        // Termination includes destruction after flock release. A new ensure
        // may already own the lock, so do not wait on that unrelated owner.
        if process.exited()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "daemon did not stop within five seconds; no signal sent",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn verify_peer_pid(stream: &UnixStream, expected: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    let pid = {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::addr_of_mut!(credentials).cast(),
                &mut length,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        credentials.pid
    };
    #[cfg(target_os = "macos")]
    let pid = {
        let mut pid: libc::pid_t = 0;
        let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                std::ptr::addr_of_mut!(pid).cast(),
                &mut length,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        pid
    };
    if u32::try_from(pid).ok() != Some(expected) {
        return Err(io::Error::other("daemon control peer process mismatch"));
    }
    Ok(())
}
fn serve_control(stream: UnixStream, snapshot: &ControlState) -> io::Result<bool> {
    // Accept inherits O_NONBLOCK. read_frame and write_frame keep non-blocking
    // I/O and one poll deadline. Do not apply that deadline with setsockopt:
    // on macOS it returns EINVAL after the peer has closed, and that error
    // would exit the control worker. The snapshot is already detached from
    // the control mutex.
    let mut reader = BufReader::new(stream);
    // Broken or slow clients cannot terminate the daemon.
    let Ok(frame) = read_frame(&mut reader, 8192) else {
        return Ok(false);
    };
    if frame != b"health" {
        return Ok(false);
    }
    let Ok(bytes) = control_frame(snapshot) else {
        return Ok(false);
    };
    if write_frame(reader.get_mut(), &bytes).is_err() {
        return Ok(false);
    }
    let Ok(frame) = read_frame(&mut reader, 8192) else {
        return Ok(false);
    };
    let Ok(request) = serde_json::from_slice::<serde_json::Value>(&frame) else {
        return Ok(false);
    };
    let health = &snapshot.health;
    if request.get("stop").and_then(|v| v.as_str()) != Some(&health.nonce)
        || request.get("plugin").and_then(|v| v.as_str()) != Some(PLUGIN_ID)
        || request.get("socket_key").and_then(|v| v.as_str()) != Some(&health.socket_key)
        || request.get("endpoint") != Some(&serde_json::to_value(health.endpoint)?)
    {
        return Ok(false);
    }
    let _ = write_frame(reader.get_mut(), b"stopping");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::ffi::OsStrExt;

    fn fixture() -> (
        tempfile::TempDir,
        Options,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let dir = crate::lock::socket_test_dir();
        let options = Options {
            socket: dir.path().join("herdr.sock"),
            state: dir.path().join("state"),
            config: dir.path().join("config"),
        };
        std::fs::create_dir(&options.config).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&options.socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let workspace = options.state.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        std::io::BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                        if request["method"] == "ping"
                            && fs::remove_file(workspace.join("fail-next-ping")).is_ok()
                        {
                            continue;
                        }
                        let result = if workspace.join("snapshot-workspace").exists()
                            && request["method"] == "session.snapshot"
                        {
                            serde_json::json!({"type":"session_snapshot", "snapshot":{
                                "version":"0.9.3", "protocol":17, "panes":[],
                                "workspaces":[{"workspace_id":"fake", "worktree":{
                                    "checkout_path":workspace
                                }}]
                            }})
                        } else if request["method"] == "workspace.report_metadata" {
                            serde_json::json!({"type":"ok"})
                        } else {
                            serde_json::json!({"type":"pong", "version":"0.9.3", "protocol":17})
                        };
                        writeln!(
                            stream,
                            "{}",
                            serde_json::json!({"id":request["id"], "result":result})
                        )
                        .unwrap();
                        // Herdr v0.9.3 ends each ordinary JSON response with EOF.
                        drop(stream);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        (dir, options, done, handle)
    }
    #[test]
    fn health_fixture_closes_after_one_json_response() {
        let (_dir, options, done, server) = fixture();
        let mut stream = std::os::unix::net::UnixStream::connect(&options.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .write_all(b"{\"id\":\"lineup-health\",\"method\":\"ping\",\"params\":{}}\n")
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        crate::doctor::validate_pong(response.trim_end().as_bytes()).unwrap();
        response.clear();
        let eof = reader.read_line(&mut response);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(eof.unwrap(), 0, "ordinary JSON responses must end in EOF");
    }

    fn child_command() -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "daemon::tests::child_entry",
            "--ignored",
            "--nocapture",
        ]);
        command
            .env_clear()
            .env("HOME", "/nonexistent-lineup-test-home");
        command
    }

    #[test]
    fn ensure_explains_overlong_control_socket_path() {
        let (_dir, mut options, done, server) = fixture();
        options.state = options.state.join("a".repeat(110));
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let error = ensure_command(&options, child_command()).unwrap_err();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        let message = error.to_string();
        assert!(
            message.contains(&paths.control.display().to_string()),
            "{message}"
        );
        let actual = paths.control.as_os_str().as_bytes().len();
        let capacity = if cfg!(target_os = "macos") { 104 } else { 108 };
        assert!(message.contains(&format!("is {actual} bytes")), "{message}");
        assert!(
            message.contains(&format!("maximum is {} path bytes", capacity - 1)),
            "{message}"
        );
        assert!(
            !paths.root.exists(),
            "ensure created state before validation"
        );
    }
    #[test]
    #[ignore = "isolated daemon subprocess entry point"]
    fn child_entry() {
        if std::env::var_os("LINEUP_INHERITED_LOCK").is_some()
            || std::env::var_os("LINEUP_TEST_FOREGROUND").is_some()
        {
            let options = Options::from_env().unwrap();
            if std::env::var_os("LINEUP_TEST_RECORD_CWD").is_some() {
                fs::write(
                    options.state.join("daemon-cwd"),
                    std::env::current_dir()
                        .unwrap()
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                )
                .unwrap();
            }
            if std::env::var_os("LINEUP_TEST_RECORD_START").is_some() {
                let mut record = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(options.state.join("starts"))
                    .unwrap();
                writeln!(record, "{}", std::process::id()).unwrap();
            }
            if std::env::var_os("LINEUP_TEST_FAIL_STARTUP").is_some() {
                let error = io::Error::new(io::ErrorKind::WouldBlock, "synthetic socket failure");
                std::process::exit(i32::from(crate::failure_exit_code(&error)));
            }
            if std::env::var_os("LINEUP_TEST_CONTEND").is_some() {
                // Reproduce a failed inherited-lock claim after a competitor
                // wins the session. Every handshake is a bounded condition wait.
                assert_eq!(unsafe { libc::close(3) }, 0);
                fs::write(
                    options.state.join("released"),
                    std::process::id().to_string(),
                )
                .unwrap();
                wait_until(|| options.state.join("winner-locked").exists());
                // The path still matches, but this independent description
                // cannot inherit ownership once the winner holds the flock.
                let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&paths.lock)
                    .unwrap();
                use std::os::fd::IntoRawFd;
                let descriptor = file.into_raw_fd();
                if descriptor != 3 {
                    assert_eq!(unsafe { libc::dup2(descriptor, 3) }, 3);
                    assert_eq!(unsafe { libc::close(descriptor) }, 0);
                }
                let error = run_foreground(&options, true, &mut NoopPublisher).unwrap_err();
                fs::write(options.state.join("contended"), error.to_string()).unwrap();
                std::thread::sleep(Duration::from_millis(800));
                std::process::exit(i32::from(crate::failure_exit_code(&error)));
            }
            if std::env::var_os("LINEUP_TEST_QUOTA_STOP").is_some() {
                struct QuotaPublisher {
                    home: PathBuf,
                    control: Arc<crate::quota_codex::QueryControl>,
                    workers: Vec<crate::model::WeeklyWorker>,
                }
                impl Publisher for QuotaPublisher {
                    fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
                        let mut client = crate::quota_codex::WeeklyClient::new(
                            self.home.clone(),
                            Duration::from_secs(60),
                            Duration::from_secs(30),
                        );
                        let control = self.control.clone();
                        self.workers.push(
                            crate::model::WeeklyWorker::start(
                                shutdown.clone(),
                                Duration::from_secs(60),
                                move || {
                                    client.refresh_controlled(&control);
                                    None
                                },
                            )
                            .unwrap(),
                        );
                        let home = self.home.clone();
                        self.workers.push(
                            crate::model::WeeklyWorker::start(
                                shutdown,
                                Duration::from_secs(60),
                                move || {
                                    fs::write(home.join("http-entered"), "").unwrap();
                                    std::thread::sleep(Duration::from_secs(30));
                                    None
                                },
                            )
                            .unwrap(),
                        );
                    }
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        Ok(())
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        Ok(())
                    }
                    fn shutdown(&mut self) {
                        self.control.stop();
                        self.workers.clear();
                    }
                }
                run_foreground(
                    &options,
                    std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                    &mut QuotaPublisher {
                        home: options.state.clone(),
                        control: Arc::new(crate::quota_codex::QueryControl::default()),
                        workers: Vec::new(),
                    },
                )
                .unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_QUOTA_ONCE").is_some() {
                let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
                let mut publisher = crate::publish::MetadataPublisher::new(
                    paths.root,
                    paths.key,
                    crate::publish::Homes::from_env(),
                )
                .unwrap();
                run_foreground(
                    &options,
                    std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                    &mut publisher,
                )
                .unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_READER_DIAGNOSTICS").is_some() {
                struct ReaderPublisher(Arc<crate::model::ReaderDiagnostics>);
                impl Publisher for ReaderPublisher {
                    fn reader_diagnostics(&self) -> Option<ReaderDiagnosticsHandle> {
                        Some(ReaderDiagnosticsHandle::new(self.0.clone()))
                    }
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        self.0.test_seed(
                            crate::model::Provider::Codex,
                            crate::model::ReaderSnapshot {
                                attempts: 42,
                                ..Default::default()
                            },
                        );
                        Ok(())
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        Ok(())
                    }
                }
                let diagnostics = Arc::new(crate::model::ReaderDiagnostics::default());
                run_foreground(&options, true, &mut ReaderPublisher(diagnostics)).unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_REFRESH_DEADLINE_SKIP").is_some() {
                struct DeadlineSkip(PathBuf);
                impl Publisher for DeadlineSkip {
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        fs::write(self.0.join("refresh-entered"), "")?;
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            crate::publish::RefreshSkip {
                                capacity_skipped: 1,
                                deadline_skipped: 2,
                            },
                        ))
                    }
                    fn reconnected(&mut self) {
                        let _ = fs::write(self.0.join("reconnected"), "");
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        Ok(())
                    }
                }
                run_foreground(&options, true, &mut DeadlineSkip(options.state.clone())).unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_REFRESH_INTERRUPTED").is_some() {
                struct InterruptedRefresh(Arc<AtomicBool>, PathBuf);
                impl Publisher for InterruptedRefresh {
                    fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
                        self.0 = shutdown;
                    }
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        fs::write(self.1.join("refresh-entered"), "")?;
                        while !self.0.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(io::Error::new(io::ErrorKind::Interrupted, "private detail"))
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        Ok(())
                    }
                }
                run_foreground(
                    &options,
                    true,
                    &mut InterruptedRefresh(
                        Arc::new(AtomicBool::new(false)),
                        options.state.clone(),
                    ),
                )
                .unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_RETRY").is_some() {
                struct RetryPublisher(PathBuf, usize);
                impl Publisher for RetryPublisher {
                    fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                        self.1 += 1;
                        fs::write(self.0.join("refresh-count"), self.1.to_string())?;
                        if self.1 == 1 {
                            fs::rename(Options::from_env()?.socket, self.0.join("hidden-socket"))?;
                            fs::write(self.0.join("fail-next-ping"), "")?;
                            return Err(io::Error::other("synthetic refresh failure"));
                        }
                        Ok(())
                    }
                    fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                        fs::write(self.0.join("clear-called"), "")
                    }
                }
                run_foreground(
                    &options,
                    true,
                    &mut RetryPublisher(options.state.clone(), 0),
                )
                .unwrap();
                return;
            }
            if std::env::var_os("LINEUP_TEST_IDLE").is_some() {
                run_foreground(&options, true, &mut NoopPublisher).unwrap();
                return;
            }
            struct SlowRefresh(PathBuf, Option<Arc<AtomicBool>>);
            impl Drop for SlowRefresh {
                fn drop(&mut self) {
                    if std::env::var_os("LINEUP_TEST_LATE_DROP").is_some() {
                        let paths =
                            RuntimePaths::new(&self.0, &Options::from_env().unwrap().socket)
                                .unwrap();
                        assert!(!SessionLock::is_held(&paths).unwrap());
                        fs::write(self.0.join("drop-entered"), "").unwrap();
                        let delay = std::env::var("LINEUP_TEST_LATE_DROP_MS")
                            .ok()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(800);
                        std::thread::sleep(Duration::from_millis(delay));
                        fs::write(self.0.join("drop-completed"), "").unwrap();
                    }
                }
            }
            impl Publisher for SlowRefresh {
                fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
                    self.1 = Some(shutdown);
                }
                fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                    if self.0.join("slow-refresh").exists() {
                        fs::remove_file(self.0.join("slow-refresh"))?;
                        fs::write(self.0.join("refresh-entered"), "")?;
                        let deadline = Instant::now() + Duration::from_secs(6);
                        while Instant::now() < deadline {
                            if self
                                .1
                                .as_ref()
                                .is_some_and(|flag| flag.load(Ordering::Acquire))
                            {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                    Ok(())
                }
                fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                    fs::write(self.0.join("clear-called"), "")
                }
            }
            run_foreground(
                &options,
                std::env::var_os("LINEUP_TEST_FOREGROUND").is_none(),
                &mut SlowRefresh(options.state.clone(), None),
            )
            .unwrap();
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + START_DEADLINE;
        while !condition() {
            assert!(Instant::now() < deadline, "test handshake timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_pid(path: &std::path::Path) -> i32 {
        let mut pid = None;
        wait_until(|| {
            pid = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
                .filter(|pid| *pid > 0);
            pid.is_some()
        });
        pid.unwrap()
    }

    #[test]
    fn deadline_skip_is_healthy_logged_and_reported_beside_the_observation() {
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_REFRESH_DEADLINE_SKIP", "1");
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        let mut frame = Vec::new();
        wait_until(|| {
            let Ok(mut stream) = UnixStream::connect(&paths.control) else {
                return false;
            };
            if write_frame(&mut stream, b"health").is_err() {
                return false;
            }
            frame = read_frame(&mut BufReader::new(stream), 8192).unwrap_or_default();
            deadline_skips_from_frame(&frame).is_some()
        });
        stop(&options).unwrap();
        let log = fs::read_to_string(&paths.log).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(
            deadline_skips_from_frame(&frame),
            Some(DeadlineSkipObservation {
                attempt_count: 1,
                last_deadline_skipped: 2,
                last_capacity_skipped: 1,
            })
        );
        let view = control_view_from_frame(&frame).unwrap();
        assert!(!view.health.refresh_failed);
        assert_eq!(view.health.capacity_skipped_resources, 1);
        let observation = view.observation.unwrap();
        assert_eq!(observation.attempt_count, 1);
        assert_eq!(
            (
                observation.success_count,
                observation.error_count,
                observation.capacity_skip_count
            ),
            (0, 0, 0)
        );
        assert!(
            log.contains("event=refresh_skipped reason=cycle_deadline"),
            "{log}"
        );
        assert!(
            log.contains("event=refresh_skipped reason=metadata_capacity"),
            "{log}"
        );
        assert!(!log.contains("event=refresh_failed"), "{log}");
        assert!(
            !options.state.join("reconnected").exists(),
            "a deadline skip must not reconnect"
        );
    }

    #[test]
    fn shutdown_interruption_is_timestamped_and_not_logged_as_failure() {
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_REFRESH_INTERRUPTED", "1");
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        stop(&options).unwrap();
        let log = fs::read_to_string(&paths.log).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(log.contains("reason=shutdown"), "{log}");
        assert!(!log.contains("event=refresh_failed"), "{log}");
        assert!(
            log.lines()
                .all(|line| line.starts_with("20") && line.contains('Z')),
            "{log}"
        );
    }

    #[test]
    fn sigterm_stale_runtime_files_do_not_block_lifecycle() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let old = probe(&paths, &options.socket).unwrap().unwrap();
        // This PID belongs to the isolated child started above.
        assert_eq!(unsafe { libc::kill(old.pid as i32, libc::SIGTERM) }, 0);
        wait_until(|| !SessionLock::is_held(&paths).unwrap());
        assert!(paths.control.exists());
        assert!(paths.pid.exists());
        stop(&options).unwrap();
        assert!(
            doctor::run(&options)
                .unwrap_err()
                .to_string()
                .contains("stale diagnostic PID")
        );
        ensure_command(&options, child_command()).unwrap();
        let new = probe(&paths, &options.socket).unwrap().unwrap();
        assert_ne!(new.pid, old.pid);
        wait_until(|| {
            probe(&paths, &options.socket)
                .unwrap()
                .unwrap()
                .last_refresh_ms
                .is_some()
        });
        doctor::run(&options).unwrap();
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn detached_daemon_uses_private_runtime_directory_and_rotates_log() {
        let (_dir, options, done, server) = fixture();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        paths.create().unwrap();
        fs::write(&paths.log, "x".repeat(MAX_LOG_BYTES as usize)).unwrap();
        let mut command = child_command();
        command.env("LINEUP_TEST_RECORD_CWD", "1");
        ensure_command(&options, command).unwrap();
        wait_until(|| options.state.join("daemon-cwd").exists());
        assert_eq!(
            fs::canonicalize(fs::read_to_string(options.state.join("daemon-cwd")).unwrap())
                .unwrap(),
            fs::canonicalize(&paths.root).unwrap()
        );
        assert_eq!(
            fs::metadata(paths.root.join("daemon.log.1")).unwrap().len(),
            MAX_LOG_BYTES
        );
        assert!(fs::metadata(&paths.log).unwrap().len() < 1024);
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn ensure_during_pending_stop_starts_exactly_one_successor() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let old = probe(&paths, &options.socket).unwrap().unwrap();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let stop_options = options.clone();
        let stopper = std::thread::spawn(move || {
            stop_with_lock(&stop_options, || {
                locked_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            })
        });
        locked_rx.recv_timeout(START_DEADLINE).unwrap();
        // Pause stop before connecting so ensures can probe the old daemon.
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let options = options.clone();
                std::thread::spawn(move || {
                    let mut command = child_command();
                    command.env("LINEUP_TEST_RECORD_START", "1");
                    ensure_command(&options, command)
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        resume_tx.send(()).unwrap();
        let stopped = stopper.join().unwrap();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let successor = probe(&paths, &options.socket).ok().flatten();
        let starts = fs::read_to_string(options.state.join("starts")).unwrap_or_default();
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        stopped.unwrap();
        for result in results {
            result.unwrap();
        }
        let successor = successor.expect("ensure returned but pending stop removed the daemon");
        assert_ne!(old.pid, successor.pid);
        assert_eq!(
            starts.lines().count(),
            1,
            "parallel ensures spawned multiple successors"
        );
        assert_eq!(starts.trim(), successor.pid.to_string());
    }

    #[test]
    fn stop_waits_for_process_after_lock_release_before_immediate_start() {
        let (_dir, options, done, server) = fixture();
        fs::create_dir(&options.state).unwrap();
        let mut old = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_LATE_DROP", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let started = Instant::now();
        let stopped = stop(&options);
        let elapsed = started.elapsed();
        let drop_completed = options.state.join("drop-completed").exists();
        let mut command = child_command();
        command.env("LINEUP_TEST_RECORD_START", "1");
        let restarted = ensure_command(&options, command);
        let new = probe(&paths, &options.socket).unwrap().unwrap();
        // Reap only the child owned by this fixture, even on a regression.
        let old_exited_at_start = old.try_wait().unwrap().is_some();
        old.wait().unwrap();
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        stopped.unwrap();
        restarted.unwrap();
        assert!(drop_completed, "stop returned before post-lock destruction");
        assert!(old_exited_at_start, "old daemon overlapped immediate start");
        assert!(elapsed >= Duration::from_millis(800));
        assert!(elapsed < STOP_DEADLINE);
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1, "ensure spawned more than once");
        assert_eq!(starts.trim(), new.pid.to_string());
    }

    #[test]
    fn stop_deadline_does_not_signal_a_process_still_tearing_down() {
        let (_dir, options, done, server) = fixture();
        fs::create_dir(&options.state).unwrap();
        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_LATE_DROP", "1")
            .env("LINEUP_TEST_LATE_DROP_MS", "10000")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let started = Instant::now();
        let stopped = stop(&options);
        let elapsed = started.elapsed();
        let still_running = child.try_wait().unwrap().is_none();
        // Test cleanup owns the Child; production stop never sends a signal.
        if still_running {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(stopped.unwrap_err().to_string().contains("no signal sent"));
        assert!(still_running);
        assert!(elapsed >= STOP_DEADLINE);
        assert!(elapsed < STOP_DEADLINE + Duration::from_millis(500));
    }

    #[test]
    fn stop_rejects_health_pid_that_does_not_match_control_peer() {
        let (_dir, options, done, server) = fixture();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let lock = SessionLock::acquire(&paths).unwrap().unwrap();
        let listener = UnixListener::bind(&paths.control).unwrap();
        let health = Health {
            plugin: PLUGIN_ID.into(),
            socket_path: options.socket.clone(),
            socket_key: paths.key.clone(),
            endpoint: EndpointIdentity::read(&options.socket).unwrap(),
            nonce: "a".repeat(64),
            pid: std::process::id() + 1,
            last_refresh_ms: None,
            refresh_failed: false,
            capacity_skipped_resources: 0,
        };
        let fake = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            assert!(!serve_control(stream, &ControlState::from(health)).unwrap());
        });
        let stopped = stop(&options);
        fake.join().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped
                .unwrap_err()
                .to_string()
                .contains("peer process mismatch")
        );
        assert!(SessionLock::is_held(&paths).unwrap());
        drop(lock);
    }

    #[test]
    fn stop_interrupts_stalled_repository_query_before_process_exit() {
        let (_dir, options, done, server) = fixture();
        fs::write(options.config.join("config.toml"), "providers = []\n").unwrap();
        fs::create_dir(&options.state).unwrap();
        fs::write(options.state.join("snapshot-workspace"), "").unwrap();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("git");
        crate::test_support::write_executable(
            &script,
            "#!/bin/sh\necho $$ > \"$2/git-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();

        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_QUOTA_ONCE", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .env("PATH", &bin)
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let git_pid = wait_for_pid(&options.state.join("git-pid"));
        let started = Instant::now();
        let stopped = stop(&options);
        let mut exited = None;
        while started.elapsed() < Duration::from_secs(5) {
            exited = child.try_wait().unwrap();
            if exited.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let lock_released = !SessionLock::is_held(&paths).unwrap();
        if exited.is_none() {
            // Only the exact synthetic child group owned by this test is killed.
            unsafe {
                libc::killpg(git_pid, libc::SIGKILL);
                libc::killpg(child.id() as i32, libc::SIGKILL);
            }
        }
        child.wait().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped.is_ok(),
            "stop blocked on repository query: {stopped:?}"
        );
        assert!(exited.is_some_and(|status| status.success()));
        assert!(lock_released);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(unsafe { libc::kill(git_pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn stop_waits_for_slow_quota_daemon_process_exit_and_releases_lock() {
        let (_dir, options, done, server) = fixture();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("codex");
        crate::test_support::write_executable(
            &script,
            "#!/bin/sh\necho $$ > \"$CODEX_HOME/app-server-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();

        fs::create_dir(&options.state).unwrap();
        let mut child = child_command()
            .env("LINEUP_TEST_FOREGROUND", "1")
            .env("LINEUP_TEST_QUOTA_STOP", "1")
            .env("HERDR_SOCKET_PATH", &options.socket)
            .env("HERDR_PLUGIN_STATE_DIR", &options.state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &options.config)
            .env("PATH", &bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        let app_server = wait_for_pid(&options.state.join("app-server-pid"));
        wait_until(|| options.state.join("http-entered").exists());
        assert!(SessionLock::acquire(&paths).unwrap().is_none());
        let started = Instant::now();
        let result = stop(&options);
        let mut exited = None;
        while started.elapsed() < Duration::from_secs(5) {
            exited = child.try_wait().unwrap();
            if exited.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Own/reap the exact child, including on a regression. PID absence alone
        // would mistake a recycled PID or an unreaped zombie for a live daemon.
        if exited.is_none() {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        let lock_released = !SessionLock::is_held(&paths).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        result.unwrap();
        assert!(exited.is_some_and(|status| status.success()));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(lock_released);
        assert_eq!(unsafe { libc::kill(app_server, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn stop_does_not_join_http_and_reaps_codex_app_server() {
        let (_dir, options, done, server) = fixture();
        let bin = options.config.join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("codex");
        crate::test_support::write_executable(
            &script,
            "#!/bin/sh\n: > \"$CODEX_HOME/app-server-pid\"\n/bin/sleep 0.2\necho $$ > \"$CODEX_HOME/app-server-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();

        let mut command = child_command();
        command.env("LINEUP_TEST_QUOTA_STOP", "1").env("PATH", &bin);
        ensure_command(&options, command).unwrap();
        let pid = wait_for_pid(&options.state.join("app-server-pid"));
        wait_until(|| options.state.join("http-entered").exists());
        let started = Instant::now();
        let result = stop(&options);
        let elapsed = started.elapsed();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        result.unwrap();
        assert!(elapsed < Duration::from_secs(5));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn daemon_quota_is_opt_in_and_repeated_refreshes_keep_one_worker() {
        for enabled in [false, true] {
            let (_dir, options, done, server) = fixture();
            fs::write(options.config.join("config.toml"), format!("allow_network = {enabled}\nproviders = ['codex']\nrefresh_interval_ms = 1000\nreader_timeout_ms = 100\n")).unwrap();
            let bin = options.config.join("bin");
            fs::create_dir(&bin).unwrap();
            let script = bin.join("codex");
            crate::test_support::write_executable(
                &script,
                "#!/bin/sh\nprintf 'attempt\\n' >> \"$CODEX_HOME/attempts\"\nexit 0\n",
            )
            .unwrap();

            let mut command = child_command();
            command
                .env("LINEUP_TEST_QUOTA_ONCE", "1")
                .env("PATH", &bin)
                .env("CODEX_HOME", &options.state);
            ensure_command(&options, command).unwrap();
            // The fake server rejects snapshots with a pong. Each bounded
            // refresh retries/reconnects, exercising the actual startup guard.
            std::thread::sleep(Duration::from_millis(2200));
            stop(&options).unwrap();
            done.store(true, Ordering::Relaxed);
            server.join().unwrap();
            let attempts = fs::read_to_string(options.state.join("attempts")).unwrap_or_default();
            assert_eq!(attempts.lines().count(), usize::from(enabled));
        }
    }

    #[test]
    fn stop_during_refresh_then_immediate_ensure_replaces_daemon() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let old = probe(&paths, &options.socket).unwrap().unwrap();
        fs::write(options.state.join("slow-refresh"), "").unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        let stopped = stop(&options);
        let cleared = options.state.join("clear-called").exists();
        let restarted = ensure_command(&options, child_command());
        let new = probe(&paths, &options.socket).unwrap().unwrap();
        // Clean up even when the regression assertion fails.
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            stopped.is_ok(),
            "stop failed during bounded refresh: {stopped:?}"
        );
        assert!(cleared, "shutdown skipped the publisher clear hook");
        assert!(restarted.is_ok(), "immediate ensure failed: {restarted:?}");
        assert_ne!(old.nonce, new.nonce, "ensure reused the old daemon");
        assert_ne!(old.pid, new.pid, "ensure did not spawn a new daemon");
    }

    #[test]
    fn health_responds_while_refresh_is_blocked() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        fs::write(options.state.join("slow-refresh"), "").unwrap();
        wait_until(|| options.state.join("refresh-entered").exists());
        let started = Instant::now();
        let health = probe(&paths, &options.socket);
        let elapsed = started.elapsed();
        // The baseline's long refresh can outlast the first stop attempt.
        let _ = stop(&options);
        let _ = stop(&options);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(health.unwrap().is_some());
        assert!(elapsed < Duration::from_millis(500));
    }

    #[test]
    fn ensure_waits_for_winner_after_its_child_loses_the_session_lock() {
        let (_dir, options, done, server) = fixture();
        let winning_options = options.clone();
        let winner = std::thread::spawn(move || {
            wait_for_pid(&winning_options.state.join("released"));
            let paths = RuntimePaths::new(&winning_options.state, &winning_options.socket).unwrap();
            let mut lock = None;
            wait_until(|| {
                lock = SessionLock::acquire(&paths).unwrap();
                lock.is_some()
            });
            fs::write(winning_options.state.join("winner-locked"), "").unwrap();
            wait_until(|| winning_options.state.join("contended").exists());
            // The winner becomes healthy while the losing child is still in
            // its post-contention teardown. Ensure must reap that child first.
            // Preserve this exact ownership even if another unit test's fork
            // inherited a descriptor: dropping and reacquiring would race it.
            let lock = lock.unwrap();
            let descriptor = lock.raw_fd();
            let endpoint = EndpointIdentity::read(&winning_options.socket).unwrap();
            let mut command = child_command();
            command
                .env("HERDR_SOCKET_PATH", &winning_options.socket)
                .env("HERDR_PLUGIN_STATE_DIR", &winning_options.state)
                .env("HERDR_PLUGIN_CONFIG_DIR", &winning_options.config)
                .env("LINEUP_INHERITED_LOCK", "3")
                .env("LINEUP_EXPECTED_DEVICE", endpoint.device.to_string())
                .env("LINEUP_EXPECTED_INODE", endpoint.inode.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            unsafe {
                command.pre_exec(move || {
                    if descriptor != 3 && libc::dup2(descriptor, 3) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn().unwrap();
            drop(lock);
            assert!(child.wait().unwrap().success());
        });
        let mut command = child_command();
        command
            .env("LINEUP_TEST_CONTEND", "1")
            .env("LINEUP_TEST_RECORD_START", "1");
        let started = Instant::now();
        let result = ensure_command(&options, command);
        let elapsed = started.elapsed();
        let losing_pid = wait_for_pid(&options.state.join("released"));
        let loser_reaped = unsafe { libc::kill(losing_pid, 0) } < 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| probe(&paths, &options.socket).ok().flatten().is_some());
        stop(&options).unwrap();
        winner.join().unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            result.is_ok(),
            "losing ensure rejected the winner: {result:?}"
        );
        assert!(loser_reaped, "ensure left its losing daemon process behind");
        assert!(elapsed >= Duration::from_millis(800));
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1, "ensure respawned a losing child");
        assert_eq!(starts.trim(), losing_pid.to_string());
    }

    #[test]
    fn ensure_rejects_a_child_socket_failure_instead_of_waiting_for_a_winner() {
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_FAIL_STARTUP", "1");
        let error = ensure_command(&options, command).unwrap_err();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            error
                .to_string()
                .starts_with("daemon exited before readiness")
        );
    }
    #[test]
    fn ensure_waits_for_a_busy_control_endpoint_within_startup_deadline() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        // A client preparing stop can occupy the serial control handler for
        // its entire frame deadline. Ensure must still wait for readiness.
        let mut pending_stop = control_stream(&paths).unwrap().unwrap();
        read_identity(&mut pending_stop, &paths, &options.socket).unwrap();
        let result = ensure_command(&options, child_command());
        drop(pending_stop);
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            result.is_ok(),
            "busy healthy daemon rejected ensure: {result:?}"
        );
    }

    #[test]
    fn parallel_ensure_starts_one_daemon_and_stop_ignores_pid_record() {
        let (_dir, options, done, server) = fixture();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let options = options.clone();
                std::thread::spawn(move || {
                    let mut command = child_command();
                    command.env("LINEUP_TEST_RECORD_START", "1");
                    ensure_command(&options, command).unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let identity = probe(&paths, &options.socket).unwrap().unwrap();
        let starts = fs::read_to_string(options.state.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1);
        assert_eq!(starts.trim(), identity.pid.to_string());
        assert!(SessionLock::acquire(&paths).unwrap().is_none());
        std::fs::write(&paths.pid, "1").unwrap();
        stop(&options).unwrap();
        assert!(SessionLock::acquire(&paths).unwrap().is_some());
        assert!(probe(&paths, &options.socket).unwrap().is_none());
        assert!(!identity.nonce.is_empty());
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn socket_replacement_stops_daemon_without_connecting_to_replacement() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        std::fs::remove_file(&options.socket).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&options.socket).unwrap();
        replacement.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if SessionLock::acquire(&paths).unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(replacement.accept().is_err());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn ensure_waits_for_old_daemon_after_missing_socket_is_replaced() {
        let (_dir, options, done, server) = fixture();
        let mut old_command = child_command();
        old_command.env("LINEUP_TEST_RETRY", "1");
        old_command.env("HOME", &options.state);
        old_command.env("CODEX_HOME", &options.state);
        ensure_command(&options, old_command).unwrap();
        wait_until(|| options.state.join("hidden-socket").exists());
        let replacement = UnixListener::bind(&options.socket).unwrap();
        replacement.set_nonblocking(true).unwrap();
        let replacement_done = done.clone();
        let replacement_server = std::thread::spawn(move || {
            while !replacement_done.load(Ordering::Relaxed) {
                match replacement.accept() {
                    Ok((mut stream, _)) => {
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                        writeln!(
                            stream,
                            "{}",
                            serde_json::json!({
                                "id": request["id"],
                                "result": {"type":"pong", "version":"0.9.3", "protocol":17}
                            })
                        )
                        .unwrap();
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        let mut new_command = child_command();
        new_command.env("HOME", &options.state);
        new_command.env("CODEX_HOME", &options.state);
        let ensured = ensure_command(&options, new_command);
        if ensured.is_ok() {
            stop(&options).unwrap();
        }
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        replacement_server.join().unwrap();
        assert!(
            ensured.is_ok(),
            "ensure did not wait for old daemon: {ensured:?}"
        );
    }
    #[test]
    fn transient_reconnect_and_ping_failures_retry_then_clear_on_stop() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        let mut command = child_command();
        command.env("LINEUP_TEST_RETRY", "1");
        command.env("HOME", &options.state);
        command.env("CODEX_HOME", &options.state);
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| options.state.join("hidden-socket").exists());
        // Restore the pinned inode after multiple retries but before its 3s TTL.
        std::thread::sleep(Duration::from_millis(1500));
        fs::rename(options.state.join("hidden-socket"), &options.socket).unwrap();
        wait_until(|| {
            fs::read_to_string(options.state.join("refresh-count"))
                .is_ok_and(|value| value.trim() == "2")
        });
        stop(&options).unwrap();
        assert!(options.state.join("clear-called").exists());
        assert!(!SessionLock::is_held(&paths).unwrap());
        assert!(!options.state.join("fail-next-ping").exists());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn missing_socket_past_token_ttl_exits_without_clear() {
        let (_dir, options, done, server) = fixture();
        fs::write(
            options.config.join("config.toml"),
            "refresh_interval_ms = 1000\nreader_timeout_ms = 100\n",
        )
        .unwrap();
        let mut command = child_command();
        command.env("LINEUP_TEST_RETRY", "1");
        command.env("HOME", &options.state);
        command.env("CODEX_HOME", &options.state);
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        wait_until(|| options.state.join("hidden-socket").exists());
        let started = Instant::now();
        let deadline = started + Duration::from_millis(4500);
        let mut exited = false;
        while Instant::now() < deadline {
            if !SessionLock::is_held(&paths).unwrap() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !exited {
            // Clean up the isolated child even when the regression assertion fails.
            fs::rename(options.state.join("hidden-socket"), &options.socket).unwrap();
            stop(&options).unwrap();
        }
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(exited, "daemon remained alive past the missing socket TTL");
        assert!(started.elapsed() >= Duration::from_millis(2900));
        assert!(!options.state.join("clear-called").exists());
    }

    #[test]
    fn completed_stop_wins_worker_exit_race() {
        let shutdown = AtomicBool::new(false);
        assert!(!control_worker_exited_unexpectedly(&shutdown, || {
            shutdown.store(true, Ordering::Release);
            true
        }));
        assert!(control_worker_exited_unexpectedly(
            &AtomicBool::new(false),
            || true
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "manual isolated idle context-switch measurement"]
    fn measure_idle_context_switches() {
        fn switches(pid: u32) -> u64 {
            fs::read_dir(format!("/proc/{pid}/task"))
                .unwrap()
                .map(|entry| {
                    fs::read_to_string(entry.unwrap().path().join("status"))
                        .unwrap()
                        .lines()
                        .filter(|line| {
                            line.starts_with("voluntary_ctxt_switches:")
                                || line.starts_with("nonvoluntary_ctxt_switches:")
                        })
                        .map(|line| {
                            line.split_whitespace()
                                .last()
                                .unwrap()
                                .parse::<u64>()
                                .unwrap()
                        })
                        .sum::<u64>()
                })
                .sum()
        }
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_IDLE", "1");
        command.env("HOME", &options.state);
        command.env("CODEX_HOME", &options.state);
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let pid = probe(&paths, &options.socket).unwrap().unwrap().pid;
        let before = switches(pid);
        let started = Instant::now();
        std::thread::sleep(Duration::from_secs(2));
        let per_second = (switches(pid) - before) as f64 / started.elapsed().as_secs_f64();
        let stop_started = Instant::now();
        stop(&options).unwrap();
        let stop_elapsed = stop_started.elapsed();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(stop_elapsed < STOP_DEADLINE);
        eprintln!("isolated idle context switches/s: {per_second:.1}, stop: {stop_elapsed:?}");
    }
    #[test]
    fn stopped_daemon_with_unrelated_pid_is_noop_and_doctor_does_not_create_state() {
        let (_dir, options, done, server) = fixture();
        assert!(doctor::run(&options).is_err());
        assert!(!options.state.exists());
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        paths.create().unwrap();
        std::fs::write(&paths.pid, "1").unwrap();
        stop(&options).unwrap();
        assert_eq!(std::fs::read_to_string(&paths.pid).unwrap(), "1");
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn inherited_nonblocking_control_stream_waits_for_shutdown_after_identity() {
        let health = Health {
            plugin: PLUGIN_ID.into(),
            socket_path: PathBuf::from("/isolated/herdr.sock"),
            socket_key: "isolated".into(),
            endpoint: EndpointIdentity {
                device: 1,
                inode: 2,
            },
            nonce: "a".repeat(64),
            pid: 1,
            last_refresh_ms: Some(0),
            refresh_failed: false,
            capacity_skipped_resources: 0,
        };
        let (mut client, server) = UnixStream::pair().unwrap();
        // BSD/macOS accept inherits the listener's O_NONBLOCK flag. Explicitly
        // reproduce that state on Linux and queue health before starting I/O.
        server.set_nonblocking(true).unwrap();
        client.write_all(b"health\n").unwrap();
        let server_state = ControlState::from(health.clone());
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = serve_control(server, &server_state).unwrap();
            completed_tx.send(result).unwrap();
        });
        let mut client = BufReader::new(client);
        let identity: Health =
            serde_json::from_slice(&read_frame(&mut client, 8192).unwrap()).unwrap();
        assert_eq!(identity.nonce, health.nonce);
        // Wait for the server's completion signal, rather than racing a write
        // against close. A valid exchange must remain open while we prepare stop.
        let early_completion = completed_rx.recv_timeout(Duration::from_millis(50));
        let write_after_early_close = early_completion
            .as_ref()
            .ok()
            .map(|_| client.get_mut().write_all(b"{\"stop\":\"pending\"}\n"));
        assert!(
            matches!(
                early_completion,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "control closed after identity before shutdown was sent: {early_completion:?}, write after close: {write_after_early_close:?}"
        );
        let request = serde_json::json!({"stop": identity.nonce, "plugin": PLUGIN_ID, "socket_key": identity.socket_key, "endpoint": identity.endpoint});
        crate::socket::write_frame(client.get_mut(), &serde_json::to_vec(&request).unwrap())
            .unwrap();
        assert_eq!(read_frame(&mut client, 8192).unwrap(), b"stopping");
        assert!(completed_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn wrong_control_nonce_cannot_stop_live_daemon() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let mut stream = control_stream(&paths).unwrap().unwrap();
        let identity = read_identity(&mut stream, &paths, &options.socket).unwrap();
        stream
            .get_mut()
            .write_all(b"{\"stop\":\"forged\"}\n")
            .unwrap();
        assert!(read_frame(&mut stream, 8192).is_err());
        assert_eq!(
            probe(&paths, &options.socket).unwrap().unwrap().nonce,
            identity.nonce
        );
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
    #[test]
    fn invalid_config_fails_before_runtime_state_is_created() {
        let (_dir, options, done, server) = fixture();
        std::fs::write(options.config.join("config.toml"), "unknown = 'PRIVATE'").unwrap();
        assert!(ensure_command(&options, child_command()).is_err());
        assert!(!options.state.exists());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    fn sample_health() -> Health {
        Health {
            plugin: PLUGIN_ID.into(),
            socket_path: PathBuf::from("/isolated/herdr.sock"),
            socket_key: "isolated".into(),
            endpoint: EndpointIdentity {
                device: 1,
                inode: 2,
            },
            nonce: "a".repeat(64),
            pid: 1,
            last_refresh_ms: Some(0),
            refresh_failed: false,
            capacity_skipped_resources: 0,
        }
    }

    #[test]
    fn capacity_skip_is_healthy_but_other_refresh_errors_are_not() {
        let mut health = sample_health();
        record_refresh(
            &mut health,
            &Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata capacity exhausted; affected resources skipped",
            )),
            11,
        );
        assert!(
            health.refresh_failed,
            "an untyped InvalidInput is not a capacity skip"
        );
        record_refresh(
            &mut health,
            &Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::publish::RefreshSkip {
                    capacity_skipped: 2,
                    deadline_skipped: 0,
                },
            )),
            12,
        );
        assert!(!health.refresh_failed);
        assert_eq!(health.last_refresh_ms, Some(12));
        assert_eq!(health.capacity_skipped_resources, 2);
        record_refresh(
            &mut health,
            &Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "other invalid input",
            )),
            13,
        );
        assert!(health.refresh_failed);
        assert_eq!(health.capacity_skipped_resources, 0);
        record_refresh(&mut health, &Err(io::Error::other("transport")), 13);
        assert!(health.refresh_failed);
        assert_eq!(health.capacity_skipped_resources, 0);
        record_refresh(&mut health, &Ok(()), 14);
        assert!(!health.refresh_failed);
        assert_eq!(health.capacity_skipped_resources, 0);
        assert_eq!(health.last_refresh_ms, Some(14));
    }

    #[test]
    fn serve_control_rejects_a_peer_that_is_already_closed() {
        let (client, server) = UnixStream::pair().unwrap();
        // Reproduce the non-blocking flag inherited from the listener.
        server.set_nonblocking(true).unwrap();
        drop(client);
        let started = Instant::now();
        assert!(!serve_control(server, &ControlState::from(sample_health())).unwrap());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn clients_that_close_immediately_do_not_stop_the_daemon() {
        let (_dir, options, done, server) = fixture();
        ensure_command(&options, child_command()).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let identity = probe(&paths, &options.socket).unwrap().unwrap();
        for _ in 0..24 {
            drop(UnixStream::connect(&paths.control).unwrap());
        }
        // Let the control worker accept the closed sockets. A setsockopt
        // failure would finish that worker and take the daemon with it.
        std::thread::sleep(Duration::from_millis(400));
        let after = probe(&paths, &options.socket).unwrap().unwrap();
        assert_eq!(after.pid, identity.pid);
        assert_eq!(after.nonce, identity.nonce);
        assert_eq!(unsafe { libc::kill(identity.pid as i32, 0) }, 0);
        let started = Instant::now();
        stop(&options).unwrap();
        assert!(started.elapsed() < STOP_DEADLINE);
        assert!(probe(&paths, &options.socket).unwrap().is_none());
        assert!(SessionLock::acquire(&paths).unwrap().is_some());
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    fn note(
        health: &mut Health,
        observation: &mut Option<RefreshObservation>,
        result: &io::Result<()>,
        uptime_ms: u64,
        duration_us: Option<u64>,
        stages: Option<RefreshStages>,
    ) {
        note_refresh_attempt(
            health,
            observation,
            MeasuredRefresh {
                result,
                uptime_ms,
                duration_us,
                stages,
            },
        );
    }

    #[test]
    fn injected_clock_separates_uptime_from_attempt_duration() {
        let uptime_ms = 10_012u64;
        let mut health = sample_health();
        health.last_refresh_ms = None;
        let mut observation = None;
        note(
            &mut health,
            &mut observation,
            &Ok(()),
            uptime_ms,
            Some(12_400),
            None,
        );
        assert_eq!(health.last_refresh_ms, Some(10_012));
        assert!(!health.refresh_failed);
        let duration = observation.unwrap().duration.unwrap();
        assert_eq!(duration.last_us, 12_400);
        assert_eq!(duration.min_us, 12_400);
        assert_eq!(duration.max_us, 12_400);
        assert_ne!(duration.last_us, health.last_refresh_ms.unwrap());
        assert_eq!(duration_bucket(duration.last_us), 3);
    }

    #[test]
    fn sub_millisecond_crossing_is_not_the_truncated_uptime_delta() {
        let started = Duration::from_micros(10_900);
        let finished = Duration::from_micros(11_100);
        let duration_us = attempt_duration_us(started, finished);
        assert_eq!(duration_us, Some(200));
        assert_eq!(duration_millis(finished) - duration_millis(started), 1);
        let mut health = sample_health();
        health.last_refresh_ms = None;
        let mut observation = None;
        note(
            &mut health,
            &mut observation,
            &Ok(()),
            duration_millis(finished),
            duration_us,
            None,
        );
        assert_eq!(health.last_refresh_ms, Some(11));
        let stats = observation.unwrap().duration.unwrap();
        assert_eq!(stats.last_us, 200);
        assert_eq!(stats.min_us, 200);
        assert_eq!(stats.max_us, 200);
        assert_eq!(stats.histogram[0], 1);
        assert_eq!(stats.histogram.iter().sum::<u64>(), 1);
    }

    #[test]
    fn refresh_outcomes_and_monotonic_failures_do_not_invent_durations() {
        let mut health = sample_health();
        health.last_refresh_ms = Some(100);
        let mut observation = None;
        note(
            &mut health,
            &mut observation,
            &Err(io::Error::other("transport")),
            130,
            Some(6_500),
            None,
        );
        assert!(health.refresh_failed);
        assert_eq!(health.last_refresh_ms, Some(100));
        assert_eq!(health.capacity_skipped_resources, 0);
        let observed = observation.clone().unwrap();
        assert_eq!(observed.error_count, 1);
        assert_eq!(observed.success_count, 0);
        assert_eq!(observed.capacity_skip_count, 0);
        assert_eq!(observed.duration.unwrap().last_us, 6_500);

        note(
            &mut health,
            &mut observation,
            &Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::publish::RefreshSkip {
                    capacity_skipped: 2,
                    deadline_skipped: 0,
                },
            )),
            140,
            None,
            None,
        );
        assert!(!health.refresh_failed);
        assert_eq!(health.last_refresh_ms, Some(140));
        assert_eq!(health.capacity_skipped_resources, 2);
        let skipped = observation.clone().unwrap();
        assert_eq!(skipped.capacity_skip_count, 1);
        assert_eq!(skipped.attempt_count, 2);
        assert_eq!(skipped.duration.unwrap().last_us, 6_500);

        note(
            &mut health,
            &mut observation,
            &Ok(()),
            150,
            Some(3_000),
            None,
        );
        let measured = observation.unwrap();
        assert_eq!(measured.success_count, 1);
        assert_eq!(measured.error_count, 1);
        assert_eq!(measured.capacity_skip_count, 1);
        let duration = measured.duration.unwrap();
        assert_eq!(duration.last_us, 3_000);
        assert_eq!(duration.min_us, 3_000);
        assert_eq!(duration.max_us, 6_500);
        assert_eq!(health.last_refresh_ms, Some(150));
    }

    #[test]
    fn duration_histogram_bounds_and_counters_saturate() {
        let samples = [
            (0, 0),
            (999, 0),
            (1_000, 1),
            (4_999, 1),
            (5_000, 2),
            (9_999, 2),
            (10_000, 3),
            (49_999, 3),
            (50_000, 4),
            (99_999, 4),
            (100_000, 5),
            (499_999, 5),
            (500_000, 6),
            (999_999, 6),
            (1_000_000, 7),
            (4_999_999, 7),
            (5_000_000, 8),
            (u64::MAX, 8),
        ];
        for (duration_us, bucket) in samples {
            assert_eq!(duration_bucket(duration_us), bucket, "{duration_us}");
        }
        let mut health = sample_health();
        let mut observation = Some(RefreshObservation {
            attempt_count: u64::MAX,
            success_count: u64::MAX,
            error_count: u64::MAX,
            capacity_skip_count: u64::MAX,
            duration: Some(RefreshDuration {
                last_us: 1,
                min_us: 1,
                max_us: 1,
                histogram: [u64::MAX; REFRESH_DURATION_BUCKETS],
            }),
            stages: None,
        });
        note(
            &mut health,
            &mut observation,
            &Ok(()),
            8,
            Some(5_000_000),
            None,
        );
        let observed = observation.unwrap();
        assert_eq!(observed.attempt_count, u64::MAX);
        assert_eq!(observed.success_count, u64::MAX);
        assert_eq!(observed.duration.unwrap().histogram[8], u64::MAX);
    }

    #[test]
    fn legacy_and_malformed_diagnostics_stay_unavailable() {
        let health = sample_health();
        let legacy = serde_json::to_vec(&health).unwrap();
        let view = control_view_from_frame(&legacy).unwrap();
        assert!(view.observation.is_none());
        assert_eq!(view.health.last_refresh_ms, health.last_refresh_ms);

        let mut value = serde_json::to_value(&health).unwrap();
        value["refresh_observation"] = serde_json::json!({
            "attempt_count": 4,
            "success_count": 4,
            "error_count": 0,
            "capacity_skip_count": 0,
            "duration": {"last_ms": 1, "min_ms": 1, "max_ms": 1, "histogram": [0, 0, 0, 0, 0, 0, 0, 0, 0]}
        });
        let wrong_unit = control_view_from_frame(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(wrong_unit.observation.is_none());
        assert_eq!(wrong_unit.health.pid, health.pid);
        value["refresh_observation"]["duration"] =
            serde_json::json!({"last_us": 1, "min_us": 1, "max_us": 1, "histogram": [1]});
        let malformed = control_view_from_frame(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(malformed.observation.is_none());
        assert_eq!(malformed.health.pid, health.pid);

        value["refresh_observation"]["extra"] = serde_json::json!(1);
        value["refresh_observation"]["duration"]["histogram"] =
            serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let unknown = control_view_from_frame(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(unknown.observation.is_none());
    }

    #[test]
    fn one_attempt_publishes_health_and_metrics_together() {
        let mut state = ControlState::from(sample_health());
        state.health.last_refresh_ms = None;
        note(
            &mut state.health,
            &mut state.observation,
            &Ok(()),
            20,
            Some(10_400),
            Some(RefreshStages {
                durations: RefreshStageDurations {
                    snapshot_us: 1,
                    git_us: 0,
                    reader_us: 2,
                    persist_us: 3,
                    socket_us: 4,
                },
                counts: RefreshStageCounts {
                    reports: 2,
                    checkpoints: 1,
                },
            }),
        );
        let first = state.clone();
        note(
            &mut state.health,
            &mut state.observation,
            &Err(io::Error::other("transport")),
            99,
            Some(7_000),
            None,
        );
        let first_view = control_view_from_frame(&control_frame(&first).unwrap()).unwrap();
        assert_eq!(first_view.health.last_refresh_ms, Some(20));
        assert!(!first_view.health.refresh_failed);
        let first_observation = first_view.observation.unwrap();
        assert_eq!(first_observation.duration.unwrap().last_us, 10_400);
        let first_stages = first_observation.stages.unwrap();
        assert!(first_stages.latest_measured);
        assert_eq!(first_stages.git.duration.last_us, 0);
        assert_eq!(first_stages.git.count, 1);
        assert_eq!(first_stages.git.duration.histogram[0], 1);
        assert_eq!(first_stages.reports, 2);
        assert_eq!(first_stages.checkpoints, 1);
        assert_eq!(first_observation.success_count, 1);

        let current = control_view_from_frame(&control_frame(&state).unwrap()).unwrap();
        assert!(current.health.refresh_failed);
        assert_eq!(current.health.last_refresh_ms, Some(20));
        let current_observation = current.observation.unwrap();
        let kept_stages = current_observation.stages.unwrap();
        assert!(!kept_stages.latest_measured);
        assert_eq!(kept_stages.git.count, 1);
        assert_eq!(kept_stages.git.duration.histogram.iter().sum::<u64>(), 1);
        assert_eq!(kept_stages.reports, 2);
        assert_eq!(current_observation.error_count, 1);
        assert_eq!(current_observation.duration.unwrap().last_us, 7_000);
        let legacy: Health = serde_json::from_slice(&control_frame(&state).unwrap()).unwrap();
        assert_eq!(legacy.last_refresh_ms, Some(20));
        assert!(legacy.refresh_failed);
    }

    #[test]
    fn default_stages_are_not_zero_measurements() {
        assert!(NoopPublisher.refresh_stages().is_none());
        struct Staged;
        impl Publisher for Staged {
            fn refresh(&mut self, _: &mut Transport, _: &Config) -> io::Result<()> {
                Ok(())
            }
            fn clear(&mut self, _: &mut Transport) -> io::Result<()> {
                Ok(())
            }
            fn refresh_stages(&self) -> Option<RefreshStages> {
                Some(RefreshStages {
                    durations: RefreshStageDurations {
                        snapshot_us: 0,
                        git_us: 0,
                        reader_us: 0,
                        persist_us: 0,
                        socket_us: 0,
                    },
                    counts: RefreshStageCounts {
                        reports: 0,
                        checkpoints: 0,
                    },
                })
            }
        }
        let measured = Staged.refresh_stages().unwrap();
        assert_eq!(measured.durations.git_us, 0);
        assert_eq!(measured.counts.checkpoints, 0);
        let mut health = sample_health();
        let mut observation = None;
        note(&mut health, &mut observation, &Ok(()), 4, Some(0), None);
        assert!(observation.unwrap().stages.is_none());
    }

    fn stages_us(
        snapshot: u64,
        git: u64,
        reader: u64,
        persist: u64,
        socket: u64,
        reports: u64,
        checkpoints: u64,
    ) -> RefreshStages {
        RefreshStages {
            durations: RefreshStageDurations {
                snapshot_us: snapshot,
                git_us: git,
                reader_us: reader,
                persist_us: persist,
                socket_us: socket,
            },
            counts: RefreshStageCounts {
                reports,
                checkpoints,
            },
        }
    }

    #[test]
    fn stage_histograms_keep_measured_attempts_and_ignore_missing_ones() {
        let mut health = sample_health();
        health.last_refresh_ms = None;
        let mut observation = None;
        note(
            &mut health,
            &mut observation,
            &Ok(()),
            40,
            Some(12_400),
            Some(stages_us(999, 1_000, 5_000, 50_000, 5_000_000, 2, 1)),
        );
        note(
            &mut health,
            &mut observation,
            &Err(io::Error::other("transport")),
            80,
            Some(200),
            Some(stages_us(0, 4_999, 9_999, 99_999, u64::MAX, u64::MAX, 3)),
        );
        assert!(health.refresh_failed);
        assert_eq!(health.last_refresh_ms, Some(40));
        note(&mut health, &mut observation, &Ok(()), 90, None, None);
        assert!(!health.refresh_failed);
        assert_eq!(health.last_refresh_ms, Some(90));
        let unmeasured = observation.clone().unwrap();
        let unmeasured_stages = unmeasured.stages.unwrap();
        assert!(!unmeasured_stages.latest_measured);
        assert_eq!(unmeasured_stages.snapshot.count, 2);
        assert_eq!(unmeasured_stages.reports, u64::MAX);
        assert_eq!(
            unmeasured.duration.unwrap().histogram.iter().sum::<u64>(),
            2
        );
        note(
            &mut health,
            &mut observation,
            &Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::publish::RefreshSkip {
                    capacity_skipped: 1,
                    deadline_skipped: 0,
                },
            )),
            100,
            Some(3_000),
            Some(stages_us(10, 10, 10, 10, 10, 1, 0)),
        );

        let observed = observation.unwrap();
        assert_eq!(observed.success_count, 2);
        assert_eq!(observed.error_count, 1);
        assert_eq!(observed.capacity_skip_count, 1);
        assert_eq!(health.last_refresh_ms, Some(100));
        assert!(!health.refresh_failed);
        let total = observed.duration.unwrap();
        assert_eq!(total.last_us, 3_000);
        assert_eq!(total.min_us, 200);
        assert_eq!(total.max_us, 12_400);
        assert_eq!(total.histogram[0], 1);
        assert_eq!(total.histogram[1], 1);
        assert_eq!(total.histogram[3], 1);
        assert_eq!(total.histogram.iter().sum::<u64>(), 3);

        let stages = observed.stages.unwrap();
        assert!(stages.latest_measured);
        assert_eq!(stages.reports, u64::MAX);
        assert_eq!(stages.checkpoints, 4);
        assert_eq!(stages.snapshot.count, 3);
        assert_eq!(stages.snapshot.duration.last_us, 10);
        assert_eq!(stages.snapshot.duration.min_us, 0);
        assert_eq!(stages.snapshot.duration.max_us, 999);
        assert_eq!(stages.snapshot.sum_us, 1_009);
        assert_eq!(stages.snapshot.duration.histogram[0], 3);
        assert_eq!(stages.git.duration.histogram[0], 1);
        assert_eq!(stages.git.duration.histogram[1], 2);
        assert_eq!(stages.git.sum_us, 6_009);
        assert_eq!(stages.reader.duration.histogram[0], 1);
        assert_eq!(stages.reader.duration.histogram[2], 2);
        assert_eq!(stages.persist.duration.histogram[0], 1);
        assert_eq!(stages.persist.duration.histogram[4], 2);
        assert_eq!(stages.socket.duration.histogram[0], 1);
        assert_eq!(stages.socket.duration.histogram[8], 2);
        assert_eq!(stages.socket.sum_us, u64::MAX);
        assert_eq!(duration_bucket(999), 0);
        assert_eq!(duration_bucket(1_000), 1);
    }

    #[test]
    fn stage_histograms_saturate_without_zeroing_history() {
        let saturated = StageDurationStats {
            count: u64::MAX,
            sum_us: u64::MAX,
            duration: RefreshDuration {
                last_us: 5,
                min_us: 5,
                max_us: 5,
                histogram: [u64::MAX; REFRESH_DURATION_BUCKETS],
            },
        };
        let mut health = sample_health();
        let mut observation = Some(RefreshObservation {
            attempt_count: 4,
            success_count: 4,
            error_count: 0,
            capacity_skip_count: 0,
            duration: None,
            stages: Some(RefreshStageAggregate {
                snapshot: saturated.clone(),
                git: saturated.clone(),
                reader: saturated.clone(),
                persist: saturated.clone(),
                socket: saturated,
                latest_measured: true,
                reports: u64::MAX,
                checkpoints: u64::MAX,
            }),
        });
        note(
            &mut health,
            &mut observation,
            &Err(io::Error::other("transport")),
            11,
            None,
            Some(stages_us(0, 1, 2, 3, 4, 9, 8)),
        );
        let stages = observation.unwrap().stages.unwrap();
        assert!(stages.latest_measured);
        assert_eq!(stages.snapshot.count, u64::MAX);
        assert_eq!(stages.snapshot.sum_us, u64::MAX);
        assert_eq!(stages.snapshot.duration.histogram[0], u64::MAX);
        assert_eq!(stages.snapshot.duration.last_us, 0);
        assert_eq!(stages.snapshot.duration.min_us, 0);
        assert_eq!(stages.snapshot.duration.max_us, 5);
        assert_eq!(stages.reports, u64::MAX);
        assert_eq!(stages.checkpoints, u64::MAX);
        assert!(health.refresh_failed);
        assert_eq!(health.last_refresh_ms, sample_health().last_refresh_ms);
    }

    #[test]
    fn stage_wire_rejects_legacy_malformed_and_contradictions() {
        let mut base = serde_json::to_value(sample_health()).unwrap();
        base["refresh_observation"] = serde_json::json!({
            "attempt_count": 1,
            "success_count": 1,
            "error_count": 0,
            "capacity_skip_count": 0,
            "duration": {
                "last_us": 0,
                "min_us": 0,
                "max_us": 0,
                "histogram": [1, 0, 0, 0, 0, 0, 0, 0, 0]
            }
        });
        let missing = control_view_from_frame(&serde_json::to_vec(&base).unwrap()).unwrap();
        let missing_observation = missing.observation.unwrap();
        assert!(missing_observation.stages.is_none());
        assert_eq!(missing_observation.duration.unwrap().last_us, 0);

        let mut legacy = base.clone();
        legacy["refresh_observation"]["stages"] = serde_json::json!({
            "durations": {
                "snapshot_us": 1,
                "git_us": 1,
                "reader_us": 1,
                "persist_us": 1,
                "socket_us": 1
            },
            "counts": {"reports": 1, "checkpoints": 1}
        });
        let legacy_view = control_view_from_frame(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(legacy_view.observation.unwrap().stages.is_none());
        assert_eq!(legacy_view.health.nonce, sample_health().nonce);

        let mut health = sample_health();
        let mut observation = None;
        note(
            &mut health,
            &mut observation,
            &Ok(()),
            5,
            Some(0),
            Some(stages_us(0, 0, 0, 0, 0, 0, 0)),
        );
        let state = ControlState {
            health,
            observation,
            deadline_skips: None,
            reader_diagnostics: None,
        };
        let roundtrip = control_view_from_frame(&control_frame(&state).unwrap()).unwrap();
        let measured = roundtrip.observation.unwrap().stages.unwrap();
        assert_eq!(measured.snapshot.count, 1);
        assert_eq!(measured.snapshot.duration.last_us, 0);
        assert_eq!(measured.snapshot.duration.histogram[0], 1);
        assert!(measured.latest_measured);

        let mut valid = serde_json::to_value(&state.health).unwrap();
        valid["refresh_observation"] =
            serde_json::to_value(state.observation.as_ref().unwrap()).unwrap();
        let mut short = valid.clone();
        short["refresh_observation"]["stages"]["snapshot"]["duration"]["histogram"] =
            serde_json::json!([1]);
        let short_view = control_view_from_frame(&serde_json::to_vec(&short).unwrap()).unwrap();
        let short_observation = short_view.observation.unwrap();
        assert!(short_observation.stages.is_none());
        assert_eq!(short_observation.duration.unwrap().last_us, 0);

        let mut unknown = valid.clone();
        unknown["refresh_observation"]["stages"]["extra"] = serde_json::json!(1);
        assert!(
            control_view_from_frame(&serde_json::to_vec(&unknown).unwrap())
                .unwrap()
                .observation
                .unwrap()
                .stages
                .is_none()
        );

        let mut contradictory = valid.clone();
        contradictory["refresh_observation"]["stages"]["git"]["duration"]["min_us"] =
            serde_json::json!(8);
        contradictory["refresh_observation"]["stages"]["git"]["duration"]["max_us"] =
            serde_json::json!(1);
        assert!(
            control_view_from_frame(&serde_json::to_vec(&contradictory).unwrap())
                .unwrap()
                .observation
                .unwrap()
                .stages
                .is_none()
        );

        let mut histogram_mismatch = valid.clone();
        histogram_mismatch["refresh_observation"]["stages"]["reader"]["duration"]["histogram"] =
            serde_json::json!([0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(
            control_view_from_frame(&serde_json::to_vec(&histogram_mismatch).unwrap())
                .unwrap()
                .observation
                .unwrap()
                .stages
                .is_none()
        );
        let mut sum_below_max = valid.clone();
        sum_below_max["refresh_observation"]["stages"]["persist"]["sum_us"] = serde_json::json!(0);
        sum_below_max["refresh_observation"]["stages"]["persist"]["duration"] = serde_json::json!({
            "last_us": 30,
            "min_us": 30,
            "max_us": 30,
            "histogram": [1, 0, 0, 0, 0, 0, 0, 0, 0]
        });
        assert!(
            control_view_from_frame(&serde_json::to_vec(&sum_below_max).unwrap())
                .unwrap()
                .observation
                .unwrap()
                .stages
                .is_none()
        );

        let mut unequal = valid;
        unequal["refresh_observation"]["stages"]["socket"]["count"] = serde_json::json!(2);
        let unequal_view = control_view_from_frame(&serde_json::to_vec(&unequal).unwrap()).unwrap();
        assert_eq!(unequal_view.health.endpoint, sample_health().endpoint);
        assert!(unequal_view.observation.unwrap().stages.is_none());
    }

    fn stage_distribution(
        count: u64,
        sum_us: u64,
        last_us: u64,
        min_us: u64,
        max_us: u64,
        histogram: [u64; REFRESH_DURATION_BUCKETS],
    ) -> serde_json::Value {
        let stage = serde_json::json!({
            "count": count,
            "sum_us": sum_us,
            "duration": {
                "last_us": last_us,
                "min_us": min_us,
                "max_us": max_us,
                "histogram": histogram,
            }
        });
        let mut health = serde_json::to_value(sample_health()).unwrap();
        health["refresh_observation"] = serde_json::json!({
            "attempt_count": count,
            "success_count": count,
            "error_count": 0,
            "capacity_skip_count": 0,
            "duration": {
                "last_us": 0,
                "min_us": 0,
                "max_us": 0,
                "histogram": [1, 0, 0, 0, 0, 0, 0, 0, 0]
            },
            "stages": {
                "snapshot": stage.clone(),
                "git": stage.clone(),
                "reader": stage.clone(),
                "persist": stage.clone(),
                "socket": stage,
                "latest_measured": true,
                "reports": 0,
                "checkpoints": 0
            }
        });
        health
    }

    fn assert_stages_unavailable(frame: &serde_json::Value) {
        let view = control_view_from_frame(&serde_json::to_vec(frame).unwrap()).unwrap();
        assert!(view.observation.as_ref().unwrap().stages.is_none());
        assert_eq!(view.health.nonce, sample_health().nonce);
        assert_eq!(view.health.endpoint, sample_health().endpoint);
        assert_eq!(
            view.observation
                .as_ref()
                .unwrap()
                .duration
                .as_ref()
                .unwrap()
                .last_us,
            0
        );
    }

    #[test]
    fn stage_histogram_buckets_must_cover_min_max_and_last() {
        let zeros = [0; REFRESH_DURATION_BUCKETS];
        let mut outside = zeros;
        outside[8] = 1;
        // count 1, sum 0, last = min = max = 0, but the only observation is `>=5000ms`.
        assert_stages_unavailable(&stage_distribution(1, 0, 0, 0, 0, outside));

        let mut above_max = zeros;
        above_max[0] = 1;
        above_max[1] = 1;
        assert_stages_unavailable(&stage_distribution(2, 1_000, 500, 0, 999, above_max));

        let mut missing_last = zeros;
        missing_last[0] = 1;
        missing_last[2] = 1;
        assert_stages_unavailable(&stage_distribution(
            2,
            10_000,
            1_000,
            0,
            10_000,
            missing_last,
        ));

        let mut missing_min = zeros;
        missing_min[1] = 1;
        missing_min[2] = 1;
        assert_stages_unavailable(&stage_distribution(
            2,
            11_000,
            10_000,
            0,
            10_000,
            missing_min,
        ));

        let mut saturated_outside = zeros;
        saturated_outside[8] = u64::MAX;
        assert_stages_unavailable(&stage_distribution(
            u64::MAX,
            u64::MAX,
            0,
            0,
            0,
            saturated_outside,
        ));

        let mut saturated_small_count = zeros;
        saturated_small_count[0] = u64::MAX;
        assert_stages_unavailable(&stage_distribution(2, 0, 0, 0, 0, saturated_small_count));

        let mut crossing = zeros;
        crossing[0] = 1;
        crossing[1] = 1;
        let accepted = control_view_from_frame(
            &serde_json::to_vec(&stage_distribution(2, 1_999, 1_000, 999, 1_000, crossing))
                .unwrap(),
        )
        .unwrap();
        let stages = accepted.observation.unwrap().stages.unwrap();
        assert_eq!(stages.snapshot.duration.histogram[0], 1);
        assert_eq!(stages.snapshot.duration.histogram[1], 1);
        assert_eq!(stages.snapshot.duration.max_us, 1_000);
        assert_eq!(
            accepted.health.last_refresh_ms,
            sample_health().last_refresh_ms
        );

        let mut saturated = zeros;
        saturated[0] = u64::MAX;
        saturated[1] = 1;
        let saturated_view = control_view_from_frame(
            &serde_json::to_vec(&stage_distribution(
                u64::MAX,
                u64::MAX,
                1_000,
                0,
                1_000,
                saturated,
            ))
            .unwrap(),
        )
        .unwrap();
        assert!(saturated_view.observation.unwrap().stages.is_some());
        assert_eq!(saturated_view.health.pid, sample_health().pid);

        // `u64::MAX - 1` zeros and two 1000 us samples saturate the count
        // without saturating either bucket. The histogram sum does not fit.
        let mut spread = zeros;
        spread[0] = u64::MAX - 1;
        spread[1] = 2;
        let parsed = control_view_from_frame(
            &serde_json::to_vec(&stage_distribution(
                u64::MAX,
                2_000,
                1_000,
                0,
                1_000,
                spread,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            parsed
                .observation
                .as_ref()
                .unwrap()
                .duration
                .as_ref()
                .unwrap()
                .last_us,
            0
        );
        let state = ControlState {
            health: parsed.health.clone(),
            observation: parsed.observation.clone(),
            deadline_skips: None,
            reader_diagnostics: None,
        };
        let roundtrip = control_view_from_frame(&control_frame(&state).unwrap()).unwrap();
        assert_eq!(roundtrip.health.nonce, parsed.health.nonce);
        assert_eq!(roundtrip.health.endpoint, parsed.health.endpoint);
        let stages = roundtrip.observation.unwrap().stages.unwrap();
        assert_eq!(stages.snapshot.count, u64::MAX);
        assert_eq!(stages.snapshot.sum_us, 2_000);
        assert_eq!(stages.snapshot.duration.last_us, 1_000);
        assert_eq!(stages.snapshot.duration.min_us, 0);
        assert_eq!(stages.snapshot.duration.max_us, 1_000);
        assert_eq!(stages.snapshot.duration.histogram[0], u64::MAX - 1);
        assert_eq!(stages.snapshot.duration.histogram[1], 2);
        assert!(
            stages.snapshot.duration.histogram[2..]
                .iter()
                .all(|count| *count == 0)
        );
        assert!(!stages.snapshot.duration.histogram.contains(&u64::MAX));
        assert_eq!(stages.socket.duration.histogram[1], 2);

        assert_stages_unavailable(&stage_distribution(
            u64::MAX - 1,
            2_000,
            1_000,
            0,
            1_000,
            spread,
        ));
        let mut spread_outside = spread;
        spread_outside[8] = 1;
        assert_stages_unavailable(&stage_distribution(
            u64::MAX,
            2_000,
            1_000,
            0,
            1_000,
            spread_outside,
        ));
    }

    #[test]
    fn serve_control_uses_a_snapshot_without_the_state_mutex() {
        let state = Arc::new(Mutex::new(ControlState::from(sample_health())));
        {
            let mut guard = state.lock().unwrap();
            let mut health = guard.health.clone();
            let mut observation = guard.observation.clone();
            note(
                &mut health,
                &mut observation,
                &Ok(()),
                30,
                Some(5_400),
                Some(stages_us(1, 2, 3, 4, 5, 1, 1)),
            );
            guard.health = health;
            guard.observation = observation;
        }
        let snapshot = state.lock().unwrap().clone();
        {
            let mut guard = state.lock().unwrap();
            guard.health.last_refresh_ms = Some(999);
            guard
                .observation
                .as_mut()
                .unwrap()
                .duration
                .as_mut()
                .unwrap()
                .last_us = 999;
            guard
                .observation
                .as_mut()
                .unwrap()
                .stages
                .as_mut()
                .unwrap()
                .snapshot
                .duration
                .last_us = 999;
        }
        let held = state.lock().unwrap();
        let (mut client, server) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        client.write_all(b"health\n").unwrap();
        let started = Instant::now();
        let worker = std::thread::spawn(move || serve_control(server, &snapshot));
        let mut client = BufReader::new(client);
        let view = control_view_from_frame(&read_frame(&mut client, 8192).unwrap()).unwrap();
        assert!(started.elapsed() < Duration::from_millis(400));
        assert_eq!(view.health.last_refresh_ms, Some(30));
        let observed = view.observation.unwrap();
        assert_eq!(observed.duration.unwrap().last_us, 5_400);
        assert_eq!(observed.stages.unwrap().snapshot.duration.last_us, 1);
        drop(client);
        drop(held);
        assert!(!worker.join().unwrap().unwrap());
    }

    fn skip(capacity_skipped: u64, deadline_skipped: u64) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::publish::RefreshSkip {
                capacity_skipped,
                deadline_skipped,
            },
        ))
    }

    #[test]
    fn mixed_skip_keeps_both_counts_and_errors_never_become_skips() {
        let mut state = ControlState::from(sample_health());
        let attempt = |state: &mut ControlState, result: &io::Result<()>| {
            note(
                &mut state.health,
                &mut state.observation,
                result,
                1,
                Some(10),
                Some(stages_us(1, 1, 1, 1, 1, 0, 0)),
            );
            note_deadline_skip(&mut state.deadline_skips, result);
        };
        attempt(&mut state, &skip(3, 2));
        assert!(!state.health.refresh_failed);
        assert_eq!(state.health.capacity_skipped_resources, 3);
        assert_eq!(
            state.deadline_skips,
            Some(DeadlineSkipObservation {
                attempt_count: 1,
                last_deadline_skipped: 2,
                last_capacity_skipped: 3,
            })
        );
        // A capacity-only skip keeps its own classification and leaves the
        // deadline totals as they were.
        attempt(&mut state, &skip(1, 0));
        assert_eq!(state.deadline_skips.as_ref().unwrap().attempt_count, 1);
        for error in [
            io::Error::new(io::ErrorKind::Interrupted, "shutdown"),
            io::Error::other("sequence checkpoint persistence is uncertain; restart required"),
            io::Error::new(io::ErrorKind::TimedOut, "ambiguous exchange"),
        ] {
            attempt(&mut state, &Err(error));
            assert!(state.health.refresh_failed);
        }
        attempt(&mut state, &skip(0, 4));
        let observation = state.observation.as_ref().unwrap();
        // Deadline skips are in the attempt total, duration and stage samples,
        // and in none of the other outcome counts.
        assert_eq!(observation.attempt_count, 6);
        assert_eq!(observation.success_count, 0);
        assert_eq!(observation.error_count, 3);
        assert_eq!(observation.capacity_skip_count, 1);
        assert_eq!(
            observation
                .duration
                .as_ref()
                .unwrap()
                .histogram
                .iter()
                .sum::<u64>(),
            6
        );
        assert_eq!(observation.stages.as_ref().unwrap().snapshot.count, 6);
        assert_eq!(
            state.deadline_skips,
            Some(DeadlineSkipObservation {
                attempt_count: 2,
                last_deadline_skipped: 4,
                last_capacity_skipped: 0,
            })
        );
        assert!(!state.health.refresh_failed);
        assert_eq!(state.health.capacity_skipped_resources, 0);
    }

    #[test]
    fn deadline_skip_field_is_optional_and_does_not_break_existing_readers() {
        let mut state = ControlState::from(sample_health());
        let fresh = control_frame(&state).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fresh).unwrap();
        assert!(
            value.get("refresh_deadline_skips").is_none(),
            "absent until the first skip"
        );
        assert_eq!(deadline_skips_from_frame(&fresh), None);
        note(
            &mut state.health,
            &mut state.observation,
            &skip(0, 1),
            1,
            Some(10),
            None,
        );
        note_deadline_skip(&mut state.deadline_skips, &skip(0, 1));
        let frame = control_frame(&state).unwrap();
        // The existing reader and the bare `Health` shape still parse the frame.
        let view = control_view_from_frame(&frame).unwrap();
        assert_eq!(view.observation.unwrap().attempt_count, 1);
        let legacy: Health = serde_json::from_slice(&frame).unwrap();
        assert!(!legacy.refresh_failed);
        assert_eq!(
            deadline_skips_from_frame(&frame)
                .unwrap()
                .last_deadline_skipped,
            1
        );
        let mut value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        for invalid in [
            serde_json::Value::Null,
            serde_json::json!({"attempt_count": 1}),
            serde_json::json!({"attempt_count": 1, "last_deadline_skipped": 1,
                "last_capacity_skipped": 0, "unknown": 1}),
        ] {
            value["refresh_deadline_skips"] = invalid;
            let frame = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                deadline_skips_from_frame(&frame),
                None,
                "unavailable, not zero"
            );
            assert!(
                control_view_from_frame(&frame)
                    .unwrap()
                    .observation
                    .is_some()
            );
        }
    }
    #[test]
    fn reader_frame_rejects_oversized_health_without_truncating_identity() {
        let mut state = ControlState::from(sample_health());
        state.health.socket_path = PathBuf::from("/".to_owned() + &"x".repeat(8192));
        assert!(
            control_frame(&state).is_err(),
            "health alone must fit including newline"
        );
    }
    fn reader_test_state() -> (ControlState, Arc<crate::model::ReaderDiagnostics>) {
        let diagnostics = Arc::new(crate::model::ReaderDiagnostics::default());
        let mut state = ControlState::from(sample_health());
        state.reader_diagnostics = Some(ReaderDiagnosticsHandle::new(diagnostics.clone()));
        (state, diagnostics)
    }

    fn reader_entry(view: &ControlView, label: &str) -> ReaderSample {
        match &view.reader_diagnostics.as_ref().unwrap().providers[label] {
            ReaderEntry::Available { sample } => sample.clone(),
            other => panic!("expected available, got {other:?}"),
        }
    }

    fn reader_view(state: &ControlState) -> ControlView {
        control_view_from_frame(&control_frame(state).unwrap()).unwrap()
    }

    // Removing per-slot try_lock or flattening unavailable into zero breaks this.
    #[test]
    fn reader_control_reports_real_contention_poison_and_incomplete_independently() {
        use crate::model::Provider;
        let (state, diagnostics) = reader_test_state();
        diagnostics.test_with_slot(Provider::Codex, || {
            let view = reader_view(&state);
            assert!(matches!(
                view.reader_diagnostics.unwrap().providers["codex"],
                ReaderEntry::Contended
            ));
        });
        let clone = diagnostics.clone();
        assert!(
            std::thread::spawn(
                move || clone.test_with_slot(Provider::Grok, || panic!("synthetic poison"))
            )
            .join()
            .is_err()
        );
        diagnostics.test_unknown(Provider::Kimi);
        let view = reader_view(&state);
        let readers = view.reader_diagnostics.as_ref().unwrap();
        assert!(matches!(readers.providers["grok"], ReaderEntry::Poisoned));
        assert!(matches!(readers.providers["kimi"], ReaderEntry::Incomplete));
        assert_eq!(reader_entry(&view, "codex").attempts, 0);
        assert!(!reader_entry(&view, "codex").worker_active);
        assert_eq!(view.health.nonce, state.health.nonce);
    }

    // Sampling a frozen refresh snapshot would keep active=true after release.
    #[test]
    fn reader_control_live_worker_release_is_visible_without_another_refresh() {
        use crate::model::{
            self, Provider, ReadRequest, SessionIdentity, SourceKind, UsageReading,
        };
        static BUSY: AtomicBool = AtomicBool::new(false);
        let (state, diagnostics) = reader_test_state();
        let identity = SessionIdentity {
            provider: Provider::Codex,
            native_session_id: "synthetic".into(),
            repository_identity: "synthetic".into(),
        };
        let request = ReadRequest {
            identity: &identity,
            home: std::path::Path::new("/unused"),
            now: std::time::SystemTime::now(),
            max_age: Duration::from_secs(60),
            timeout: Duration::from_millis(20),
            shutdown: None,
        };
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, running) = std::sync::mpsc::channel();
        let result = model::bounded_read_observed(
            &request,
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
            model::UsageOutcome::Unavailable(model::MissingReason::Timeout)
        );
        let before = reader_view(&state);
        let sample = reader_entry(&before, "codex");
        assert!(sample.worker_active && sample.timed_out_worker_active);
        assert_eq!(sample.deadline_ends, 1);
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while BUSY.load(Ordering::Acquire) && Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(!BUSY.load(Ordering::Acquire));
        let after = reader_view(&state);
        let sample = reader_entry(&after, "codex");
        assert!(!sample.worker_active && !sample.timed_out_worker_active);
        assert_eq!(sample.attempts, 1);
        assert_eq!(after.health.last_refresh_ms, before.health.last_refresh_ms);
        assert_eq!(after.health.nonce, before.health.nonce);
    }

    #[test]
    fn reader_wire_skew_does_not_erase_health_or_valid_siblings() {
        let (state, _) = reader_test_state();
        let base: serde_json::Value =
            serde_json::from_slice(&control_frame(&state).unwrap()).unwrap();
        let decode = |value: &serde_json::Value| {
            control_view_from_frame(&serde_json::to_vec(value).unwrap()).unwrap()
        };
        for bad in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"version":2,"sampling":"live_best_effort","providers":{}}),
            serde_json::json!({"version":1,"sampling":"frozen","providers":{}}),
            serde_json::json!({"version":1,"sampling":"live_best_effort","providers":{},"path":"do-not-display"}),
        ] {
            let mut value = base.clone();
            value["reader_diagnostics"] = bad;
            let view = decode(&value);
            assert!(view.reader_diagnostics.is_none());
            assert_eq!(view.health.nonce, state.health.nonce);
        }
        let mut bad_samples = Vec::new();
        for bad in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("4"),
            serde_json::json!(true),
        ] {
            let mut value = base.clone();
            value["reader_diagnostics"]["providers"]["codex"]["sample"]["attempts"] = bad;
            bad_samples.push(value);
        }
        for field in ["attempts", "worker_active"] {
            let mut value = base.clone();
            value["reader_diagnostics"]["providers"]["codex"]["sample"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            bad_samples.push(value);
        }
        for (field, value) in [
            ("extra", serde_json::json!("nonce-path-usage-secret")),
            ("worker_active", serde_json::json!(0)),
            ("timed_out_worker_active", serde_json::json!(true)),
        ] {
            let mut bad = base.clone();
            bad["reader_diagnostics"]["providers"]["codex"]["sample"][field] = value;
            bad_samples.push(bad);
        }
        let mut missing = base.clone();
        missing["reader_diagnostics"]["providers"]
            .as_object_mut()
            .unwrap()
            .remove("codex");
        bad_samples.push(missing);
        let mut unknown = base.clone();
        unknown["reader_diagnostics"]["providers"]["codex"] =
            serde_json::json!({"status":"nonce-path-usage-secret"});
        bad_samples.push(unknown);
        for mut value in bad_samples {
            value["reader_diagnostics"]["providers"]["arbitrary-secret-label"] =
                serde_json::json!({"status":"available"});
            let view = decode(&value);
            let readers = view.reader_diagnostics.as_ref().unwrap();
            assert_eq!(readers.providers.len(), 6);
            assert!(matches!(
                readers.providers["codex"],
                ReaderEntry::Unavailable
            ));
            assert_eq!(reader_entry(&view, "agy").attempts, 0);
            assert_eq!(view.health.nonce, state.health.nonce);
        }
        let mut invalid_health = base;
        invalid_health["pid"] = serde_json::json!("invalid");
        assert!(control_view_from_frame(&serde_json::to_vec(&invalid_health).unwrap()).is_err());
    }

    fn maximum_reader_state() -> ControlState {
        use crate::model::{Provider, ReaderSnapshot};
        let (mut state, diagnostics) = reader_test_state();
        let maximum = ReaderSnapshot {
            attempts: u64::MAX,
            busy_rejections: u64::MAX,
            deadline_ends: u64::MAX,
            shutdown_ends: u64::MAX,
            spawn_failures: u64::MAX,
            worker_failures: u64::MAX,
            completed: u64::MAX,
            invalid: u64::MAX,
            caller_wait_count: u64::MAX,
            caller_wait_total_ns: u64::MAX,
            caller_wait_max_ns: u64::MAX,
            sampling_skips: u64::MAX,
            ..ReaderSnapshot::default()
        };
        for provider in [
            Provider::Claude,
            Provider::Agy,
            Provider::Codex,
            Provider::Grok,
            Provider::Hermes,
            Provider::Kimi,
        ] {
            diagnostics.test_seed(provider, maximum);
        }
        let duration = RefreshDuration {
            last_us: u64::MAX,
            min_us: 0,
            max_us: u64::MAX,
            histogram: [u64::MAX; REFRESH_DURATION_BUCKETS],
        };
        let stage = StageDurationStats {
            count: u64::MAX,
            sum_us: u64::MAX,
            duration: duration.clone(),
        };
        state.observation = Some(RefreshObservation {
            attempt_count: u64::MAX,
            success_count: u64::MAX,
            error_count: u64::MAX,
            capacity_skip_count: u64::MAX,
            duration: Some(duration),
            stages: Some(RefreshStageAggregate {
                snapshot: stage.clone(),
                git: stage.clone(),
                reader: stage.clone(),
                persist: stage.clone(),
                socket: stage,
                latest_measured: true,
                reports: u64::MAX,
                checkpoints: u64::MAX,
            }),
        });
        state.deadline_skips = Some(DeadlineSkipObservation {
            attempt_count: u64::MAX,
            last_deadline_skipped: u64::MAX,
            last_capacity_skipped: u64::MAX,
        });
        state
    }

    // Wrong budget, partial provider removal or resampling on fallback fails here.
    #[test]
    fn reader_maximum_frame_and_fallback_sample_once_preserving_health() {
        let mut state = maximum_reader_state();
        let handle = state.reader_diagnostics.as_ref().unwrap().clone();
        let normal = control_frame(&state).unwrap();
        assert!(normal.len() < 8192);
        let parsed = control_view_from_frame(&normal).unwrap();
        assert_eq!(
            reader_entry(&parsed, "codex").caller_wait_total_ns,
            u64::MAX
        );
        assert_eq!(
            parsed.observation.unwrap().stages.unwrap().reader.count,
            u64::MAX
        );
        assert_eq!(
            parsed.deadline_skips.unwrap().last_capacity_skipped,
            u64::MAX
        );
        assert_eq!(handle.samples.load(Ordering::Relaxed), 1);
        state.health.socket_path = PathBuf::from("/".to_owned() + &"\"".repeat(1200));
        let fallback = control_frame(&state).unwrap();
        let parsed = control_view_from_frame(&fallback).unwrap();
        assert!(parsed.reader_diagnostics.is_none());
        assert!(parsed.observation.is_some());
        assert!(parsed.deadline_skips.is_some());
        assert_eq!(parsed.health.socket_path, state.health.socket_path);
        assert_eq!(handle.samples.load(Ordering::Relaxed), 2);
        state.health.socket_path = PathBuf::from("/".to_owned() + &"\"".repeat(3200));
        let fallback = control_frame(&state).unwrap();
        let parsed = control_view_from_frame(&fallback).unwrap();
        assert!(
            parsed.reader_diagnostics.is_none()
                && parsed.observation.is_none()
                && parsed.deadline_skips.is_none()
        );
        assert_eq!(parsed.health.nonce, state.health.nonce);
        assert_eq!(parsed.health.endpoint, state.health.endpoint);
        assert_eq!(parsed.health.socket_path, state.health.socket_path);
        assert_eq!(handle.samples.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn reader_frame_exact_newline_boundary_and_oversized_health_failure() {
        let (mut state, _) = reader_test_state();
        // With Health alone at exactly 8192 bytes, both extension tiers disappear.
        let original = serde_json::to_vec(&state.health).unwrap().len();
        let padding = 8191 - original;
        state.health.socket_key.push_str(&"x".repeat(padding));
        let frame = control_frame(&state).unwrap();
        assert_eq!(frame.len() + 1, 8192);
        assert!(
            control_view_from_frame(&frame)
                .unwrap()
                .reader_diagnostics
                .is_none()
        );
        let mut line = frame.clone();
        line.push(b'\n');
        let read_line = |line: Vec<u8>| {
            let (mut sender, receiver) = UnixStream::pair().unwrap();
            sender.write_all(&line).unwrap();
            drop(sender);
            read_frame(&mut BufReader::new(receiver), 8192)
        };
        assert_eq!(read_line(line).unwrap(), frame);
        let mut over = frame.clone();
        over.push(b' ');
        over.push(b'\n');
        assert!(read_line(over).is_err());
        state.health.socket_key.push('x');
        assert!(control_frame(&state).is_err());
        assert_eq!(
            state
                .reader_diagnostics
                .as_ref()
                .unwrap()
                .samples
                .load(Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn reader_sampling_and_slow_client_do_not_hold_control_mutex() {
        use crate::model::Provider;
        let (state, diagnostics) = reader_test_state();
        let shared = Arc::new(Mutex::new(state));
        let snapshot = shared.lock().unwrap().clone();
        diagnostics.test_with_slot(Provider::Claude, || {
            assert!(shared.try_lock().is_ok());
            assert!(matches!(
                reader_view(&snapshot).reader_diagnostics.unwrap().providers["claude"],
                ReaderEntry::Contended
            ));
            assert!(shared.try_lock().is_ok());
        });
        let (mut client, server) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        client.write_all(b"health\n").unwrap();
        let worker = std::thread::spawn(move || serve_control(server, &snapshot));
        let mut client = BufReader::new(client);
        let frame = read_frame(&mut client, 8192).unwrap();
        assert!(
            control_view_from_frame(&frame)
                .unwrap()
                .reader_diagnostics
                .is_some()
        );
        assert!(shared.try_lock().is_ok());
        drop(client);
        assert!(!worker.join().unwrap().unwrap());
        assert!(NoopPublisher.reader_diagnostics().is_none());
    }
    #[test]
    fn reader_handle_survives_actual_daemon_completed_state_publication() {
        let (_dir, options, done, server) = fixture();
        let mut command = child_command();
        command.env("LINEUP_TEST_READER_DIAGNOSTICS", "1");
        ensure_command(&options, command).unwrap();
        let paths = RuntimePaths::new(&options.state, &options.socket).unwrap();
        let mut observed = None;
        wait_until(|| {
            observed = probe_view(&paths, &options.socket).unwrap();
            observed
                .as_ref()
                .is_some_and(|view| view.health.last_refresh_ms.is_some())
        });
        let view = observed.unwrap();
        assert_eq!(reader_entry(&view, "codex").attempts, 42);
        assert!(view.observation.is_some());
        stop(&options).unwrap();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
