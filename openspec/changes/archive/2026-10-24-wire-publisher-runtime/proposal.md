# Wire the publisher runtime to a bounded refresh cycle

## Why

One refresh has no cycle end today. Readers share `reader_timeout_ms`, but the
snapshot and every metadata exchange use unbounded socket calls with only
local per-frame caps, so a refresh with N slow but live reports can take
N × 2 s. Git headings run in snapshot order with no cycle budget, and an
interrupted heading is kept with no age limit. Production reports no refresh
stage timings although tests measure them.

## What Changes

- Specify a cooperative refresh envelope
  `E = cycle start + 2 s + 1 s + reader_timeout_ms + 2 s` (7 s by default) that
  every snapshot, metadata exchange and implicit reconnect shares through the
  caller-deadline socket API, with a 2 s admission threshold before each
  metadata request.
- Specify a not-admitted metadata request as a deadline skip (no frame, no
  error, no reconnect) and an admitted request that fails as an ambiguous
  error whose sequence is burned and never replayed.
- Specify fair queues for Git headings (by workspace) and metadata sends (by
  resource) so no resource or workspace starves behind earlier ones.
- Specify one shared Git budget per cycle and three heading states:
  Confirmed, a 30 s Fallback after an interrupted or unattempted read, and
  Unknown, which publishes null instead of `—`.
- Specify the degraded expiry gap when a cycle runs long: the key expires and
  disappears and never shows a wrong value. Refresh period and TTL do not change.
- Specify one reader cycle per refresh and a reader budget bounded by `E`.
- Specify that daemon health distinguishes deadline skips from capacity skips
  and errors, and that production reports refresh stage timings.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `sidebar-tokens`: heading states, refresh envelope, admission, fair order and
  the degraded expiry gap.
- `provider-readers`: reader budget within the refresh envelope and one reader
  cycle per refresh.
- `daemon-lifecycle`: deadline skip classification and production stage timings.

## Impact

`src/publish.rs`, `src/daemon.rs` and `src/doctor.rs`. No on-disk format changes: the
sequence checkpoint is untouched and queues, heading states and stage values
are memory-only. The clear path keeps its own three-second deadline. Reader
diagnostics are exposed as optional advisory control data and fixed doctor text,
with one live sample per response and health-preserving 8 KiB frame fallback.
