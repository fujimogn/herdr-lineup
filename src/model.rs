//! Session-bound local usage observations. No account or conversation data is returned.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
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

#[derive(Clone, Debug, PartialEq)]
pub struct UsageReading {
    pub identity: SessionIdentity,
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

impl UsageReading {
    pub(crate) fn missing(
        request: &ReadRequest<'_>,
        source: SourceKind,
        reason: MissingReason,
    ) -> Self {
        Self {
            identity: request.identity.clone(),
            observed_at: None,
            source_kind: source,
            context: UsageOutcome::Unavailable(reason),
            weekly_remaining: UsageOutcome::Unavailable(MissingReason::Unsupported),
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
