# Bounded reader worker observations

Status: integrated on the local line as `98a57de` (not shipped); production wiring is a separate, pending task.
Base: `4f7044ad3f9066798bf55aff00bd6f17094022e2`.
The candidate commit is the commit containing this document (or its reviewed successor).
This addresses the observation portion of latency finding L5, not worker cancellation.

## API and ownership

`bounded_read` preserves its existing signature and returns the same `UsageReading`.
It uses the common implementation with diagnostics disabled. Existing readers still
call that wrapper: **production reader, publisher, control and doctor wiring is
not implemented in this candidate**. No setting, interval or persistent state is added.

`bounded_read_observed(request, source, busy, &Arc<ReaderDiagnostics>, job)` adds
optional observation without changing the caller's result contract. The caller must
use the existing single static BUSY flag for its provider, consistently through the
observed API when diagnostics are enabled. Diagnostics do not replace that permit.
A later publisher task should own one diagnostics Arc across refreshes and pass it
through `ReadCycle::with_diagnostics`; `diagnostics()` borrows that Arc. Default,
`new`, and existing readers remain compatible and observation-free. Clones share
only the explicitly supplied owner. No global diagnostics singleton is introduced.

`snapshots()` returns six typed `(Provider, Result<ReaderSnapshot,
DiagnosticUnavailable>)` entries in Claude, Agy, Codex, Grok, Hermes, Kimi order.
Hermes is a fixed empty slot unless a future caller explicitly observes it.
A later Serde control adapter must preserve the provider, availability and units;
it must not serialize an unavailable result as a zero-valued successful snapshot.
No identity, path, usage, wall-clock timestamp, error text or credentials are retained.

## Counter and state contract

All counters are cumulative saturating `u64` values, scoped to one diagnostics owner.
Time uses `Instant` only; `_ns` fields are elapsed nanoseconds clamped to `u64::MAX`.

| Field | Meaning |
| --- | --- |
| `attempts` | Entries into the observed API, including early rejection |
| `busy_rejections` | Nonzero-budget calls rejected by the existing BUSY compare/exchange |
| `deadline_ends` | Caller wait ended by deadline, including zero timeout |
| `shutdown_ends` | Caller wait ended by the observed shutdown flag |
| `spawn_failures` | Thread creation was rejected; the closure/permit were dropped |
| `worker_failures` | Worker unwound with a panic, even after caller return |
| `completed` | A worker result was accepted before deadline/shutdown, regardless of its usage outcome |
| `invalid` | Caller timeout could not form an absolute `Instant` deadline |
| `caller_wait_count` | Sampled caller exits, including rejected calls and disconnected receivers |
| `caller_wait_total_ns` | Sum of sampled observed-call elapsed time, including preparation and spawn |
| `caller_wait_max_ns` | Maximum sampled observed-call elapsed time |
| `sampling_skips` | Diagnostic mutations skipped because try_lock was contended or poisoned |
| `worker_active` | Latest registered worker has not published its release marker |
| `timed_out_worker_active` | That worker remains active after its caller ended by deadline |
| `shutdown_worker_active` | That worker remains active after its caller ended by shutdown |

These gauges describe wrapper lifetimes, not proof of a blocked OS read or a live
thread after it has released its permit. Release precedes result-channel send.
An accepted worker may already be released when its caller records a deadline;
the active gauge is then false. A late result never replaces the caller's timeout.

Each accepted worker has its own Arc of atomic bits. Caller deadline/shutdown bits
and worker release are monotonic OR operations, so either ordering yields an idle
gauge after release. An old caller cannot write the next worker's state. A guard
publishes release before the production permit drops, including unwind and rejected
spawn. Diagnostic mutation never releases that permit and never changes a result.

Only short `try_lock` operations protect each fixed slot; reader work, filesystem,
control I/O and waiting run outside those locks. Contended/poisoned snapshots return
`Contended`/`Poisoned`. If worker registration was skipped, subsequent unlocked
snapshots return `Incomplete` until a new registration succeeds: an old idle slot
must not masquerade as the current worker. Counter skips remain visible after
recovery. Counts can be incomplete, and separately sampled fields/providers do not
form a transaction; consumers must display skip/unavailable status explicitly.
The registry retains at most one worker Arc per provider. Caller/worker clones live
only as long as their calls; no history, labels or dynamically keyed map is retained.

## Isolated measurements

Environment: Linux `7.2.10-1-cachyos`, x86_64 GNU/Linux, Rust `1.99.1`
(`58cf8f9c3`, 2026-10-13), debug test build. No real provider files, account API,
Herdr session, user configuration or credentials were accessed by the new fixtures.
The reader closure is deliberately blocked on a channel and released by the test.
This simulates noncooperative work; it does not identify an OS read as stuck.

One focused run of `cargo test --locked observed_ -- --nocapture` observed:

| Fixture | Measured result |
| --- | --- |
| Deadline caller | 30,126,666 ns wall elapsed for a 30 ms budget |
| Same-provider retry | attempts=2, deadline=1, BUSY rejection=1, caller exits=2 |
| Sampled waits for those two calls | total=30,122,456 ns; maximum=30,117,998 ns |
| Shutdown caller | 100,170,361 ns wall elapsed; shutdown=1, deadline=0 |
| Worker after caller return | active until channel release; inactive before BUSY becomes reusable |

The successful retry returns its own value after release, and another provider
completes while the first worker remains active. Additional fixtures cover immediate
worker panic, rejected spawn, diagnostic lock contention/poison, old-caller/new-worker
state isolation in both release orders, saturation, optional cycle ownership, and
queued-result discard / disconnected-receiver precedence. The pre-existing queued
late-result and nonobserved timeout fixtures remain in place.

These are individual unoptimized fixture observations under uncontrolled host load,
not percentiles, frequency estimates, performance improvements, or production bounds.
The shutdown loop still uses its original 100 ms slice. Scheduling can extend elapsed
wall time. OS-stalled reads, process isolation, cancellation, macOS and real-world
worker occupancy frequencies were not measured. No production wiring is claimed.

## Candidate verification

Formatting, strict all-target Clippy, the full locked test suite (368 unit passes,
6 pre-existing ignored tests, 1 integration pass), locked release build, release
`--help`, encoding and whitespace checks passed on Linux. Three temporary mutations
were rejected by the focused fixtures: omitting worker release, updating the newest
slot from an old caller, and reporting an idle snapshot after skipped registration.
The original source was restored and all six observation fixtures passed again.
The dedicated target is retained under the no-manual-clean instruction; no live
frequency or production rollout claim follows from these local checks.
