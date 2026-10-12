//! Hermes has no confirmed persistent current-context observation in v1.
use crate::model::*;

pub struct HermesReader;
impl UsageReader for HermesReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        // Cumulative state.db counters are throughput, not current context occupancy.
        let reason = if associated(request, Provider::Hermes) {
            MissingReason::Unsupported
        } else {
            MissingReason::Unassociated
        };
        UsageReading::missing(request, SourceKind::HermesUnsupported, reason)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{identity, request};
    #[test]
    fn unsupported_even_with_fresh_stale_or_mismatched_dummy_state() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Hermes);
        let mut r = request(&id, dir.path());
        for dummy in [
            None,
            Some(r#"{"sessionId":"session-1","context_percent":7}"#),
            Some(r#"{"sessionId":"other","context_percent":7}"#),
        ] {
            if let Some(dummy) = dummy {
                std::fs::write(dir.path().join("state.db"), dummy).unwrap();
            }
            for age in [0, 61] {
                r.now += std::time::Duration::from_secs(age);
                let reading = HermesReader.read(&r);
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
            HermesReader.read(&request(&other, dir.path())).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }
}
