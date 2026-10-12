//! Claude observations require the pane's exact native session ID.
use crate::{
    model::*,
    statusline::{self, Agent},
};
use std::sync::atomic::AtomicBool;
pub struct ClaudeReader;
impl UsageReader for ClaudeReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let source = SourceKind::ClaudeStatusline;
        if request.identity.provider != Provider::Claude
            || !statusline::valid_key(&request.identity.native_session_id)
        {
            let mut r = UsageReading::missing(request, source, MissingReason::Unassociated);
            r.weekly_remaining = UsageOutcome::Unavailable(MissingReason::Unassociated);
            return r;
        }
        bounded_read(request, source, &BUSY, move |request| {
            statusline::read_record(
                request,
                Agent::Claude,
                &request.identity.native_session_id,
                source,
            )
        })
    }
}
