//! Kimi's live status events have no confirmed persistent context source in v1.
use crate::model::*;

pub struct KimiReader;
impl UsageReader for KimiReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        // agent.status.updated is observable, not confirmed durable. Never probe server.token.
        let reason = if associated(request, Provider::Kimi) {
            MissingReason::Unsupported
        } else {
            MissingReason::Unassociated
        };
        UsageReading::missing(request, SourceKind::KimiUnsupported, reason)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{identity, request};
    #[test]
    fn unsupported_even_with_fresh_stale_or_mismatched_dummy_events() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Kimi);
        let mut r = request(&id, dir.path());
        let path = dir.path().join("sessions/cwd/session-1/agents/main");
        std::fs::create_dir_all(&path).unwrap();
        for dummy in [
            None,
            Some(r#"{"type":"agent.status.updated","sessionId":"session-1","contextUsage":0.07}"#),
            Some(r#"{"type":"agent.status.updated","sessionId":"other","contextUsage":0.07}"#),
        ] {
            if let Some(dummy) = dummy {
                std::fs::write(path.join("wire.jsonl"), dummy).unwrap();
            }
            for age in [0, 61] {
                r.now += std::time::Duration::from_secs(age);
                let reading = KimiReader.read(&r);
                assert_eq!(
                    reading.context,
                    UsageOutcome::Unavailable(MissingReason::Unsupported)
                );
                assert_eq!(
                    reading.weekly_remaining,
                    UsageOutcome::Unavailable(MissingReason::Unsupported)
                );
                assert!(reading.observed_at.is_none());
                assert!(reading.weekly_resets_at.is_none());
            }
        }
        let other = identity(Provider::Codex);
        assert_eq!(
            KimiReader.read(&request(&other, dir.path())).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }
}
