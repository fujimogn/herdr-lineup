# Publisher runtime wiring plan

**Status:** design candidate only. No production code in this commit implements
any part of it. The deadline values in "Cycle budget" are a proposal awaiting
the commander's decision; they are not adopted.

**Base:** local integration `64ace031ab5b17615a2cebef46f916bf4defd79c`.

**Goal:** Connect the bounded-I/O pieces that already exist as APIs or test-only
code to the production refresh path of `MetadataPublisher`, without weakening
the sequence, endpoint, or shutdown guarantees that are already proven.

## 1. Purpose, inputs, outputs, sources of truth

| Item | Content |
|---|---|
| Purpose | One refresh has a bounded, cooperative end time; no resource or workspace starves forever; partial cycles never publish values that were not observed; production reports the stage timings that tests measure today. |
| Inputs | `session.snapshot`, local Git (`ship_time`), local readers (`UsageReader`), account worker samples, `Config` (`refresh_interval_ms`, `reader_timeout_ms`, `usage_max_age_ms`), the sequence checkpoint. |
| Outputs | `report_metadata` frames, `sequences.json`, `daemon::RefreshStages`, a typed refresh skip outcome for daemon health, reader diagnostics snapshot. |
| Sources of truth | Token, heading and expiry behavior: `openspec/specs/sidebar-tokens/spec.md`. Reader deadline: `openspec/specs/provider-readers/spec.md`. Socket frames and daemon health: `openspec/specs/daemon-lifecycle/spec.md`. Git cache contract: commander note `note-git-cache-nogo-01`, which supersedes `docs/reviews/git-cache-design.md` v7 where they differ. Sequence safety: `docs/reviews/sequence-reservation-design-2026-10-23.md`. |

Every behavior change below that a spec states differently needs an
`openspec/changes/<change>/` delta in the same task, validated before the code.

## 2. Current state at the base

| Piece | State at `64ace03` | Production gap |
|---|---|---|
| Cycle deadline | Only readers share `reader_timeout_ms`, measured from after headings (`refresh_with_readers_inner`). Snapshot and every metadata exchange use `exchange()`, which calls `send_line`/`receive_line` without a caller deadline. | A refresh with N slow-but-live reports takes up to N × 2 s; there is no cycle end. |
| Socket `*_before` | `Transport::{connect_before, reconnect_same_endpoint_before, send_line_before, receive_line_before}` exist, each capped locally (connect 1 s, write 0.5 s, read 0.5 s). | Not called by `publish.rs`. |
| Git | `read_repository_controlled` gives each heading its own 1 s deadline; workspaces run in snapshot order; `Interrupted` keeps the previous heading with no age limit. | No cycle Git budget, no fairness, unbounded fallback age. |
| `ReadCycle` | `UsageReader::read_in_cycle` exists; Codex and Kimi readers implement it. | `observe_pane` calls `reader.read()`, so each read builds a fresh cycle. |
| Reader diagnostics | The `ReaderDiagnostics` source API is implemented on another lane and under independent review; it is not in this base. | No owner, no consumer. |
| Stage timing | `StageTimes` is `cfg(test)`. `Publisher::refresh_stages` returns `None`. Doctor already prints `RefreshStages` when present. | Production reports "unavailable". |

## 3. Invariants that must survive

1. Lazy reservation barrier: a sequence window is persisted before the first
   frame that uses it; nothing else is persisted per report.
2. Poison: any failed checkpoint write blocks every later reservation, report,
   snapshot, refresh, clear and reconnect of the instance. A deadline skip never
   clears or masks poison.
3. The durable upper bound `U` never decreases.
4. Ambiguous exchange, no replay: a request that may have reached Herdr is never
   resent on the same or a new stream. The cycle ends with an error and the
   daemon reconnects.
5. Endpoint pin: the endpoint bound by the first snapshot never changes.
6. Shutdown and clear: refresh does not start a new frame after shutdown; clear
   keeps its own 3 s deadline and is not governed by the refresh budget.
7. Per-pane order: each pane runs observe → plan → send before the next pane is
   observed. A reader for pane k+1 may see files written while pane k was sent.

## 4. Cycle budget (proposal, decision requested)

### 4.1 Derivation from existing local caps

These values are worst-case local caps already in the code or spec, not
measurements. The synthetic measurements in
`docs/reviews/refresh-stage-latency-2026-10-23.md` are not an SLO and are not
used to size anything here.

| Term | Value | Origin |
|---|---|---|
| Snapshot request | 2 s | `connect_stream_before` 1 s + `write_frame_before` 0.5 s + `read_frame_before` 0.5 s |
| Git for the cycle | 1 s initial | sidebar-tokens spec: one heading shares one second; reused as the whole-cycle Git budget |
| Readers | `reader_timeout_ms` (default 2 s, at most `refresh_interval_ms`) | provider-readers spec, starts after heading resolution |
| One metadata request | 2 s | same caps as the snapshot request |

Proposal A (commander's candidate): at cycle start `t0`, the envelope is
`E = t0 + 2 s + 1 s + reader_timeout + 2 s` (7 s by default, 35 s at the
maximum reader timeout). Every real I/O receives
`min(its local cap, E − now)`.

### 4.2 Admission rule that separates skips from errors

A metadata request is admitted only while `E − now ≥ 2 s`, checked at the last
point before the exchange (after any reservation persist, beside today's
shutdown check). Because an admitted request always has its whole local-cap
worst case left, the cycle deadline is never what cuts an in-flight exchange.
Therefore:

- Not admitted: `DeadlineSkip`. No bytes were written. Not an error, no
  reconnect, the resource is not rotated (§5).
- Admitted, then `TimedOut` or any other failure inside the exchange: a real,
  possibly ambiguous error. Invariant 4 applies.

The snapshot request is always started at cycle start with deadline `E`; with
`E − t0 ≥ 5 s` its local caps bind first.

Synchronous filesystem work (checkpoint fsync and rename, reader `stat`/read,
Git kill and reap) is cooperative: it is checked before and after, never
interrupted. `E` is therefore not a strict wall-clock guarantee.

### 4.3 Conflict with key expiry (finding)

Live keys carry `ttl_ms = 3 × refresh_interval_ms` (15 s by default) and are
renewed once a resource is at least one interval old. The daemon schedules the
next refresh one interval after the previous one ends. For a resource sent at
offset `a` in cycle k and offset `b` in cycle k+1, the gap between sends is at
most `D + I + b`, where `D` is the cycle duration and `I` the interval. With
admission ending at `E − 2 s`:

- A at defaults: `D ≤ 7 s`, `b ≤ 5 s`, gap ≤ 17 s > 15 s TTL. A resource sent
  first in one degraded cycle and last in the next can expire for up to 2 s.
  Rotation (§5) makes exactly that ordering likely.
- Clamping (alternative B) to keep the gap ≤ TTL needs the admission end at
  `t0 + min(3 s + reader_timeout, I − 1 s)`. It holds the bound at defaults
  only under these caps, can admit nothing in the worst case for `I ≤ 3 s` unless a floor of one
  admitted request per cycle is added, and with that floor it cannot hold the
  bound for small intervals.
- Under a uniform slowdown, more admitted requests per cycle shorten the time
  until every resource is renewed again. A longer envelope (A) is better for
  that, and B is better only for the reordering case above.

Both options fail safe: an expired key disappears, it never shows a wrong
value. My recommendation is A, with the expiry gap written into the spec as a
degraded-mode property, because it does not change the reader deadline's
meaning and gives more renewals under uniform slowness. The commander decides.
Changing the refresh period, a `timeout` executable, process isolation and
socket reuse stay out of scope as instructed.

## 5. Fair queues

Both queues use the same rule set, so one helper type `FairQueue<K>` (private
to `publish.rs`) serves both.

- **Sync:** each cycle takes the current ID set from the snapshot. IDs that
  disappeared are removed; survivors keep their relative order; new IDs are
  appended at the tail in snapshot order.
- **Attempt:** iterate from the head. When an item's first budgeted I/O starts,
  move it to the tail at that moment, even if the attempt then times out or
  fails.
- **Skip:** an item that is not started because the budget is exhausted stays
  where it is. Iteration stops at the first such skip.
- **Progress:** the head always gets the full initial budget of its stage, so
  each cycle rotates at least one item, and every item reaches the head within
  N cycles.

### 5.1 Git queue (key: workspace ID)

- Cycle Git deadline `G = min(t_git + 1 s, E)`. Each query runs with
  `min(1 s, G − now)`.
- `repository_read_once` keeps sharing results between workspaces with the same
  cwd within one cycle. A shared result is not a new attempt and does not rotate
  the second workspace.
- Workspaces left in the queue this cycle get `HeadingState::NotAttempted`.

Git cache contract (`note-git-cache-nogo-01` and
`note-publisher-design-contract-update-01`, owned by `lane_git_implementation`,
still under implementation and review): native `rev-parse` and `for-each-ref`
run on every read; only the worktree root and name are cached, 64 entries,
30 s TTL. A hit also verifies the native identity and `safe.directory` of the
common-directory cwd every time, separately from the workspace cwd check. Hit
and miss both run three Git children; the cache only elides `worktree list`.
This plan does not claim fewer children per heading or less total subprocess
work, so the 1 s per-query and 1 s per-cycle Git deadlines and the fair queue
stay as specified. A hit returns only after the post fingerprint and root
check, and inherits the root's original `verified_at` (the minimum over root
and ref) without extending its TTL. Any ref Git failure discards the entry and
returns no `verified_at`; only a clean empty output means absence.
`Interrupted` returns no `verified_at`. This plan consumes that API and does
not change it.

### 5.2 Send queue (key: `ResourceId`, panes and workspaces together)

Panes and workspaces share one queue so workspace reports cannot starve behind
panes. A pane item runs observe → plan → send (invariant 7). A workspace item
runs plan → send from the already-resolved headings. An item's first budgeted
I/O is its reader call or, if it has none, its metadata exchange. An item whose
planner needs no report does no socket I/O and is not an attempt.

`first_pane_ids` keeps using snapshot order, never queue order, so heading
placement and logo indentation do not move with rotation.

## 6. Headings under partial observation

`HeadingCache` gains `confirmed: Instant`, set only from a real
`verified_at`. A workspace's heading for this cycle is one of these states:

| State | Condition | Workspace keys | First-pane group keys |
|---|---|---|---|
| Confirmed | `Complete(Some)` with `verified_at` | name and ship text | copied |
| Absent | `Complete(None)` (confirmed non-repository) | `—` | null |
| Fallback | `Interrupted` or `NotAttempted`, same cwd and occupants, `now − confirmed < 30 s` | previous values | copied |
| Unknown | otherwise | null | null |

- Fallback never refreshes `confirmed`. A cache hit that inherits an old root
  `verified_at` expires 30 s after that root check, not after the hit.
- Unknown sends null, not `—`. `—` claims a confirmed absence; null says
  nothing. A stale heading with the same occupant is therefore cleared at 30 s
  instead of being retained indefinitely, as it is today.
- This changes the sidebar-tokens requirement "the previous heading SHALL be
  kept when the workspace cwd and occupants are unchanged" and needs a spec
  delta.

Panes not reached in a cycle send nothing. Their keys keep the expiry from
their last accepted report. No workspace value is derived from pane
observations, so a truncated pane loop cannot produce a partial workspace
aggregate.

## 7. Outcome and daemon health

`refresh` returns one typed skip payload in place of today's `CapacitySkip`
alone:

```rust
pub(crate) struct RefreshSkip {
    pub capacity_skipped: u64,
    pub deadline_skipped: u64, // queue items not started this cycle
}
```

The daemon classifies by payload before error kind. `deadline_skipped > 0`
does not set `refresh_failed`, does not reconnect, and logs
`refresh_skipped: cycle_deadline`. Shutdown still yields `Interrupted`,
checked before skip classification. Poison and ambiguous-exchange errors stay
errors. Doctor prints both counts; the control wire field is optional, matching
how `stages` was added. Check the version-skew behavior of
`deny_unknown_fields` against that precedent before adding it.

## 8. Stage timing in production

Replace the `cfg(test)` `StageTimes` with an always-built value that is reset at
the start of each refresh and returned by `refresh_stages()` after every
attempt, including errors and skips.

| Field | Measures |
|---|---|
| `snapshot_us` | `session.snapshot` request and parse |
| `git_us` | heading resolution stage, wall time |
| `reader_us` | sum of reader calls |
| `persist_us` | sum of successful and failed checkpoint writes |
| `socket_us` | snapshot and metadata exchanges |
| `reports` | acknowledged metadata reports |
| `checkpoints` | successful `sequences.json` writes in this refresh, not rows |

Stages overlap (`snapshot_us` includes its socket time) and are not additive;
doctor's wording must say so. Measurement uses `Instant` only and writes no
files.

## 9. Reader cycle and diagnostics

- Create one `ReadCycle` per refresh in `refresh_with_readers_inner`, pass it to
  `observe_pane`, call `read_in_cycle`, and drop it when the refresh returns.
  It is never stored on the publisher.
- The approved diagnostics API (`note-publisher-design-contract-update-01`;
  source under independent review, callers not wired) is: a compatible
  `ReadCycle` constructor and accessor that carry an optional
  `Arc<ReaderDiagnostics>` (default and `new` stay without diagnostics),
  `bounded_read_observed(..., &ReaderDiagnostics, job)`, and snapshots for the
  fixed provider set where a failed `try_lock` reads as unavailable. Names are
  taken from the integrated commit, not from `b2239ff`.
- After that API is integrated, `MetadataPublisher` owns one
  `Arc<ReaderDiagnostics>` for its lifetime and builds each refresh's cycle with
  the diagnostics constructor. Selecting `bounded_read_observed` inside readers
  belongs to the reader wiring task, not to W2. A new `Publisher` method with a
  `None` default, like `refresh_stages`, exposes the provider snapshots to the
  daemon control state and doctor. An unavailable or skipped sample must stay
  distinct from zero.

## 10. Tasks, ownership and order

All tasks touch `src/publish.rs`, so they run one after another, never in
parallel. Each is one commit with its own spec delta and can be reverted alone.

| Task | Files | Depends on | Owner of fixtures |
|---|---|---|---|
| W1 cycle budget, send queue, `RefreshSkip`, socket `*_before` | `publish.rs`, `daemon.rs`, spec deltas | commander's §4 decision | publish tests (fake socket) and daemon health tests |
| W2 per-refresh `ReadCycle` | `publish.rs` | none | publish tests |
| W3 Git queue and heading states | `publish.rs`, sidebar-tokens delta | `lane_git_implementation` observation API integrated | publish tests with real temporary Git |
| W4 runtime stages and reader diagnostics consumer | `publish.rs`, `daemon.rs`, `doctor.rs` | W1; reader observability integrated | daemon and doctor tests |

`model.rs`, the readers and `ship_time.rs` belong to the reader and Git lanes
and are not edited by these tasks. Clear is unchanged. Its existing 1 s
admission guard is smaller than the 2 s worst-case round trip. That is a
pre-existing overrun, reported here, not fixed in this plan.

### Rollback

No on-disk format changes: the checkpoint is untouched, and the queues, heading
states and stage values are memory-only. Rolling back is reverting the task's
commit. A daemon restart then returns to today's behavior, and the checkpoint
written by the new code is valid for the old code.

## 11. Required fixtures

| # | Task | Fixture | Proves |
|---|---|---|---|
| F1 | W1 | Fake socket delays every report by 400 ms; 12 panes, a 5 s envelope | Result is `deadline_skipped > 0`; no reconnect; every resource is acknowledged within ⌈N/k⌉ cycles; unstarted items keep their order |
| F2 | W1 | Server reads a report and never replies | Error, not skip; single request ID observed; sequence not reused; daemon reconnects |
| F3 | W1 | Admission evaluated after a reservation persist that consumes the budget (injected slow persist) | Skip with no frame written; `U` unchanged or higher; next cycle continues above it |
| F4 | W1 | Checkpoint fault during a budgeted cycle | Poisoned; later cycles return the poison error, never a skip |
| F5 | W1 | Shutdown set mid-cycle | `Interrupted`, not a deadline skip; clear still uses its own deadline |
| F6 | W1 | Workspaces placed after many panes, budget for half | Workspace reports are sent within the queue bound |
| F7 | W1 | Reader for pane 2 checks a file written by the fake socket when it acknowledges pane 1 | observe → plan → send order kept |
| F8 | W2 | Two Codex panes, same home | One discovery scan per refresh; a new refresh starts with an empty cycle |
| F9 | W3 | Three repositories, the first stalls Git past 1 s | Queue `[A,B,C]` → `[B,C,A]` after cycle 1, with B and C not rotated while skipped; every workspace is attempted within 3 cycles |
| F10 | W3 | Queue sync with removed and new workspaces | Survivors keep order; new IDs at the tail |
| F11 | W3 | Injected clock: confirmed at t0, then `Interrupted` | Heading kept at 29 s, null at 30 s; a cache hit whose root was verified at t0 − 25 s expires at t0 + 5 s |
| F12 | W3 | New workspace not attempted in its first cycle | Null keys, never `—` |
| F13 | W3 | Same occupant, Git failing for 31 s | Heading cleared; not retained by occupant equality |
| F14 | W4 | One refresh with known reports and one reservation | `reports` equals acknowledgements; `checkpoints` equals 1; stages present after an error |
| F15 | W4 | Diagnostics skipped sample | Doctor shows unavailable, not 0 |

## 12. Decisions for the commander

1. §4: proposal A (recommended) or clamp B, and whether the expiry gap is
   written into the spec as degraded-mode behavior.
2. §6: null rather than `—` for the Unknown heading state.
3. §10: whether W1 may touch `daemon.rs` together with `publish.rs`, so that a
   deadline skip is never recorded as a failure in between commits.
