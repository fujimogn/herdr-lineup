//! Antigravity observations require the exact Herdr pane ID.
use crate::{
    model::*,
    statusline::{self, Agent},
};
use std::sync::atomic::AtomicBool;
pub struct AgyReader {
    pub pane_id: String,
}
static BUSY: AtomicBool = AtomicBool::new(false);
impl UsageReader for AgyReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        self.read_in_cycle(request, &ReadCycle::new())
    }

    fn read_in_cycle(&self, request: &ReadRequest<'_>, cycle: &ReadCycle) -> UsageReading {
        let source = SourceKind::AgyStatusline;
        if request.identity.provider != Provider::Agy || !statusline::valid_key(&self.pane_id) {
            let mut r = UsageReading::missing(request, source, MissingReason::Unassociated);
            r.weekly_remaining = UsageOutcome::Unavailable(MissingReason::Unassociated);
            return r;
        }
        let pane_id = self.pane_id.clone();
        let job = move |request: &ReadRequest<'_>| {
            statusline::read_record(request, Agent::Agy, &pane_id, source)
        };
        match cycle.diagnostics() {
            Some(diagnostics) => bounded_read_observed(request, source, &BUSY, diagnostics, job),
            None => bounded_read(request, source, &BUSY, job),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{identity, request};
    use std::{sync::atomic::Ordering, time::Duration};

    #[test]
    fn diagnostic_routing_isolated() {
        const CHILD: &str = "HERDR_LINEUP_AGY_ROUTING_CHILD";
        if std::env::var(CHILD).as_deref() != Ok("agy") {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "reader_agy::tests::diagnostic_routing_isolated",
                    "--nocapture",
                ])
                .env(CHILD, "agy")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Agy);
        let base = request(&id, dir.path());
        install(dir.path(), &base);
        let diagnostics = std::sync::Arc::new(ReaderDiagnostics::default());
        let cycle = ReadCycle::with_diagnostics(diagnostics.clone());

        let observed = reader().read_in_cycle(&base, &cycle);
        wait_for_worker();
        assert_eq!(observed.context, UsageOutcome::Available(23.5));
        let plain = reader().read_in_cycle(&base, &ReadCycle::new());
        wait_for_worker();
        assert_eq!(observed, plain);
        assert_eq!(observed, reader().read(&base));
        wait_for_worker();
        let snapshot = diagnostics.snapshot(Provider::Agy).unwrap();
        assert_eq!((snapshot.attempts, snapshot.completed), (1, 1));
        assert!(!snapshot.worker_active);

        // Rejection before bounded_read starts has no diagnostic attempt.
        let mut unassociated = id.clone();
        unassociated.provider = Provider::Hermes;
        assert_eq!(
            reader()
                .read_in_cycle(&request(&unassociated, dir.path()), &cycle)
                .context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        assert_eq!(diagnostics.snapshot(Provider::Agy).unwrap().attempts, 1);
        // Invalid timeout is detected inside bounded_read, so it is counted.
        let invalid = ReadRequest {
            timeout: Duration::MAX,
            ..base
        };
        assert_eq!(
            reader().read_in_cycle(&invalid, &cycle).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        let snapshot = diagnostics.snapshot(Provider::Agy).unwrap();
        assert_eq!(
            (snapshot.attempts, snapshot.completed, snapshot.invalid),
            (2, 1, 1)
        );

        // Observed and plain reads share one static permit.
        BUSY.store(true, Ordering::Release);
        let busy = UsageOutcome::Unavailable(MissingReason::Timeout);
        assert_eq!(reader().read_in_cycle(&base, &cycle).context, busy);
        assert_eq!(reader().read(&base).context, busy);
        let snapshot = diagnostics.snapshot(Provider::Agy).unwrap();
        assert_eq!((snapshot.attempts, snapshot.busy_rejections), (3, 1));
        BUSY.store(false, Ordering::Release);
        assert_eq!(observed, reader().read_in_cycle(&base, &cycle));
        wait_for_worker();
        let snapshot = diagnostics.snapshot(Provider::Agy).unwrap();
        assert_eq!((snapshot.attempts, snapshot.completed), (4, 2));
        assert_eq!(snapshot.sampling_skips, 0);
    }

    fn install(state: &std::path::Path, request: &ReadRequest<'_>) {
        let observed_ms = request
            .now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let record = statusline::Record {
            version: 1,
            agent: Agent::Agy,
            key: PANE.into(),
            observed_ms,
            context: Some(23.5),
            weekly_remaining: Some(73.0),
            weekly_reset_ms: Some(observed_ms + 3_600_000),
        };
        std::fs::create_dir_all(state.join("statusline")).unwrap();
        std::fs::write(
            statusline::record_path(state, Agent::Agy, &record.key),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }

    const PANE: &str = "w58:p3";

    fn reader() -> AgyReader {
        AgyReader {
            pane_id: PANE.into(),
        }
    }

    fn wait_for_worker() {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while BUSY.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
