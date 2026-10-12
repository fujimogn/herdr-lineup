//! Antigravity observations require the exact Herdr pane ID.
use crate::{
    model::*,
    statusline::{self, Agent},
};
use std::sync::atomic::AtomicBool;
pub struct AgyReader {
    pub pane_id: String,
}
impl UsageReader for AgyReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let source = SourceKind::AgyStatusline;
        if request.identity.provider != Provider::Agy || !statusline::valid_key(&self.pane_id) {
            let mut r = UsageReading::missing(request, source, MissingReason::Unassociated);
            r.weekly_remaining = UsageOutcome::Unavailable(MissingReason::Unassociated);
            return r;
        }
        let pane_id = self.pane_id.clone();
        bounded_read(request, source, &BUSY, move |request| {
            statusline::read_record(request, Agent::Agy, &pane_id, source)
        })
    }
}
