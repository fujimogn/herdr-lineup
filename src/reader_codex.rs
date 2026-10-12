//! Raw context occupancy from exact-session Codex rollout token_count observations.
use crate::model::*;
use serde_json::Value;
use std::path::Path;

pub struct CodexReader;
impl UsageReader for CodexReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        let source = SourceKind::CodexRollout;
        if !associated(request, Provider::Codex)
            || !safe_component(&request.identity.native_session_id)
        {
            return UsageReading::missing(request, source, MissingReason::Unassociated);
        }
        match read_session(request) {
            Ok((percent, time)) => UsageReading::sample(request, source, percent, time),
            Err(reason) => UsageReading::missing(request, source, reason),
        }
    }
}
fn rollout(path: &Path) -> bool {
    path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
        s.starts_with("rollout-") && (s.ends_with(".jsonl") || s.ends_with(".jsonl.zst"))
    })
}
fn read_session(request: &ReadRequest<'_>) -> Result<(f64, std::time::SystemTime), MissingReason> {
    let mut budget = Budget::new(request);
    // Discovery reads names only. Other sessions and credential files are never opened.
    let mut paths = budget.files(&request.home.join("sessions"), 3, rollout)?;
    paths.extend(budget.files(&request.home.join("archived_sessions"), 3, rollout)?);
    let id = &request.identity.native_session_id;
    paths.retain(|path| {
        path.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| {
                name.ends_with(&format!("-{id}.jsonl"))
                    || name.ends_with(&format!("-{id}.jsonl.zst"))
            })
    });
    if paths.len() > 1 {
        return Err(MissingReason::Unassociated);
    }
    let path = paths.first().ok_or(MissingReason::NotFound)?;
    let text = budget.text(path, path.extension().is_some_and(|e| e == "zst"))?;
    let mut bound = false;
    let mut observation = None;
    for line in text.lines() {
        budget.check()?;
        let value: Value = serde_json::from_str(line).map_err(|_| MissingReason::Invalid)?;
        match value["type"].as_str() {
            Some("session_meta") => {
                if value["payload"]["id"].as_str() != Some(id) || bound {
                    return Err(MissingReason::Unassociated);
                }
                bound = true;
            }
            Some("event_msg") if value["payload"]["type"] == "token_count" => {
                let info = &value["payload"]["info"];
                let used = info["last_token_usage"]["total_tokens"]
                    .as_f64()
                    .ok_or(MissingReason::Invalid)?;
                let capacity = info["model_context_window"]
                    .as_f64()
                    .ok_or(MissingReason::Invalid)?;
                if used < 0.0 || capacity <= 0.0 {
                    return Err(MissingReason::Invalid);
                }
                // No quota-plugin baseline deduction: the contract requires raw occupancy.
                observation = Some((
                    used / capacity * 100.0,
                    timestamp(&value["timestamp"]).ok_or(MissingReason::Invalid)?,
                ));
            }
            _ => {}
        }
    }
    if !bound {
        return Err(MissingReason::Unassociated);
    }
    observation.ok_or(MissingReason::NotFound)
}
#[cfg(test)]
mod tests {
    fn assert_percent(outcome: UsageOutcome) {
        let UsageOutcome::Available(percent) = outcome else {
            panic!("context unavailable");
        };
        assert!((percent - 7.0).abs() < 1e-9);
    }
    use super::*;
    use crate::model::tests::{identity, request};
    use std::fs;
    fn fixture(home: &Path, session: &str, time: &str, used: i64) {
        fs::create_dir_all(home.join("sessions/2026/10/12")).unwrap();
        let text = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session}\"}}}}\n{{\"type\":\"event_msg\",\"timestamp\":\"{time}\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"total_tokens\":{used}}},\"model_context_window\":1000}}}}}}\n"
        );
        fs::write(
            home.join("sessions/2026/10/12/rollout-test-session-1.jsonl"),
            text,
        )
        .unwrap();
    }
    #[test]
    fn normal_missing_stale_and_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 70);
        let reading = CodexReader.read(&r);
        assert_percent(reading.context);
        assert_eq!(
            reading.weekly_remaining,
            UsageOutcome::Unavailable(MissingReason::Unsupported)
        );
        assert!(reading.weekly_resets_at.is_none());
        fixture(dir.path(), "session-1", "2026-10-11T23:58:59Z", 70);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Stale)
        );
        fixture(dir.path(), "session-2", "2026-10-12T00:00:00Z", 70);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }
    #[test]
    fn compressed_archive_and_ambiguous_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 70);
        let original = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        fs::create_dir(dir.path().join("archived_sessions")).unwrap();
        let archive = dir
            .path()
            .join("archived_sessions/rollout-test-session-1.jsonl.zst");
        fs::write(
            &archive,
            zstd::stream::encode_all(fs::read(&original).unwrap().as_slice(), 1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        fs::remove_file(original).unwrap();
        assert_percent(CodexReader.read(&r).context);
    }
    #[test]
    fn rejects_invalid_future_timeout_and_partial_tail() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let mut r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 1001);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        fixture(dir.path(), "session-1", "2026-10-12T00:00:01Z", 10);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        r.timeout = std::time::Duration::ZERO;
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        r.timeout = std::time::Duration::from_secs(2);
        fs::write(
            dir.path()
                .join("sessions/2026/10/12/rollout-test-session-1.jsonl"),
            "{\"type\":",
        )
        .unwrap();
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
    }
}
