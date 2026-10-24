# Bounded reader worker observations

Status: implemented in the local unshipped candidate. Implementation source:
`0220da64f8545f89b6daca211553f486b271057f`. The original observation API was
based on `4f7044ad3f9066798bf55aff00bd6f17094022e2` and integrated as
`98a57de`; production wiring followed in P/D and the worker-producer tasks.
This addresses L5 observation, not cancellation or process isolation.

## API, production wiring and ownership

`bounded_read` keeps its signature and observation-free behavior. All five
local readers (Claude, Agy, Codex, Grok, Kimi) now choose
`bounded_read_observed` when the refresh's `ReadCycle` carries diagnostics;
the nonobserved path remains compatible. The publisher owns one Arc across
refreshes and passes it through `ReadCycle::with_diagnostics`. Each provider
keeps its single existing static BUSY permit. Diagnostics never replace it.
No global singleton, setting, interval or persistent state is added.

Control samples the shared handle on request, independently of the completed
refresh metrics; doctor shows the provider, availability, counts, nanosecond
waits and worker gauges before a health error. See
`../plans/reader-diagnostics-control.md` for optional/legacy wire validation,
frame-size fallback and unavailable semantics. Snapshots are best-effort,
nontransactional and may describe a newer worker state than the last refresh.
An accepted result increments `completed` even when its usage is missing;
it does not prove a successful usage read.

`snapshots()` returns six fixed provider slots. Hermes has no local worker:
its empty slot and zero counters are not a read-success or latency measurement.
Unavailable results stay unavailable, rather than becoming successful zero
snapshots. No identity, path, usage, wall-clock timestamp, error text or
credentials are retained.

Current fixtures include each reader's `diagnostic_routing_isolated`,
`one_reader_cycle_per_refresh_shares_discovery_and_diagnostics`,
`reader_control_live_worker_release_is_visible_without_another_refresh`,
`reader_handle_survives_actual_daemon_completed_state_publication`, and
`reader_doctor_uses_fixed_labels_units_and_retained_deadline_meaning`.
Actual Codex/Kimi multi-pane fixtures assert shared discovery scans as well
as attempt/completed counters. They are bounded local regressions, not live
worker-frequency or OS-stall measurements.

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

## Historical isolated measurements (API candidate)

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
worker occupancy frequencies were not measured. Production wiring was added later; these measurements were not repeated for it.

## Historical API candidate verification

Formatting, strict all-target Clippy, the full locked test suite (368 unit passes,
6 pre-existing ignored tests, 1 integration pass), locked release build, release
`--help`, encoding and whitespace checks passed on Linux. Three temporary mutations
were rejected by the focused fixtures: omitting worker release, updating the newest
slot from an old caller, and reporting an idle snapshot after skipped registration.
The original source was restored and all six observation fixtures passed again.
The dedicated target is retained under the no-manual-clean instruction; no live
frequency or production rollout claim follows from these local checks.
