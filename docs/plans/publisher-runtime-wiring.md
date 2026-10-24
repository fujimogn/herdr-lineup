# Publisher runtime wiring plan

**Status:** P runtime wiring, D control/doctor diagnostics, and all local reader
worker producers are included in the v0.3.0 release contents. Historical
implementation evidence is at
`6be18304d6e39a0eb6d88164a866b449d9396946`, after their recorded review chains.
The owner authorized this release batch on 2026-10-24. Delivery is recorded
by the [v0.3.0 tag and GitHub Release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.3.0);
this implementation record does not establish publication or live verification.
The commander's decisions in
`note-publisher-design-decisions-01` are incorporated. Design GO on `86ea771`
(`note-publisher-design-go-handoff-01`). The synchronized spec delta is archived
at `openspec/changes/archive/2026-10-24-wire-publisher-runtime/`.

D exposes live best-effort, nontransactional provider samples independently of
completed-attempt refresh metrics; see `docs/plans/reader-diagnostics-control.md`.
Claude, Agy, Grok, Codex and Kimi select observed reads when diagnostics are
present; Hermes has no worker, so its empty slot is not a successful read
measurement. Codex retains the combined 4096-entry cap and cumulative fallback
accounting (C1). The separate Herdr host task (C2) remains unresolved: no host
change or live acceptance is claimed here. The envelope is cooperative and does
not guarantee interruption of synchronous OS work. Git cache hits and misses
still run three children; no speed improvement has been measured for this
candidate. The historical base, design sequence and remaining constraints below
are preserved as design history, not claims of outstanding P/D implementation.

Task P differs from the text below in these points, each approved by the
commander:

- Deadline skips are not added to `RefreshObservation`. They travel in a
  separate optional control field, `refresh_deadline_skips`, so older peers
  keep parsing the frame and doctor's existing literals stay valid
  (`note-publisher-runtime-api-go-01`). Deadline-skip attempts are in the
  attempt total, duration and stage histograms, but not in the success, error
  or capacity counts. A mixed cycle keeps both counts.
- `ControlView` is unchanged; `daemon::deadline_skips_from_frame` is the typed
  seam that task D consumes.
- Task P already builds each refresh's `ReadCycle` with the publisher-owned
  `Arc<ReaderDiagnostics>`. Task D only exposes the snapshots.
- Clear also passes its own three-second deadline to the socket calls.
- An Unknown heading publishes null for all three workspace keys.
- `ship_time::read_repository_controlled` keeps a local `allow(dead_code)` as
  the compatible uncached seam (touching approval for `src/ship_time.rs`).
- F10 checks that both panes of a refresh share one cycle and that each
  refresh starts from an empty one; reuse of the scan inside a refresh is the
  reader's choice, as the reader tests already allow.

**Historical design base:** local integration `64ace031ab5b17615a2cebef46f916bf4defd79c`.

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

Every behavior change below that a spec states differently needs an openspec
change delta in the same task, validated before the code. The approved change is now archived at
`openspec/changes/archive/2026-10-24-wire-publisher-runtime/` with deltas under
`specs/sidebar-tokens/`, `specs/provider-readers/` and
`specs/daemon-lifecycle/`.

## 2. Historical state at the design base

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
   resent on the same or a new stream. Its sequence is burned. The cycle ends with an error and the
   daemon reconnects.
5. Endpoint pin: the endpoint bound by the first snapshot never changes.
6. Shutdown and clear: refresh does not start a new frame after shutdown; clear
   keeps its own 3 s deadline and is not governed by the refresh budget.
7. Per-pane order: each pane runs observe → plan → send before the next pane is
   observed. A reader for pane k+1 may see files written while pane k was sent.

## 4. Cycle budget (adopted as initial cooperative budget)

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

Adopted (option A): at cycle start `t0`, the envelope is
`E = t0 + 2 s + 1 s + reader_timeout + 2 s` (7 s by default, 35 s at the
maximum reader timeout). Every real I/O receives
`min(its local cap, E − now)`.

`E` is a conservative cooperative budget. It is not an SLO, not a strict
wall-clock bound, and not a bound on physical fsync time. Synchronous
filesystem work (checkpoint fsync and rename, reader `stat`/read, Git kill and
reap) and scheduler delay are checked before and after, never interrupted, and
can overrun `E`.

### 4.2 Admission threshold, skips and errors

A metadata request is admitted only while `E − now ≥ 2 s`, checked at the last
point before the exchange (after any reservation persist, beside today's
shutdown check). The threshold only reads the remaining budget. Synchronous I/O
or scheduling delay between the check and the exchange can still let `E`
expire while the request is in flight, so the design does **not** guarantee
that the cycle deadline never cuts an in-flight exchange.

- Not admitted: a deadline skip. No frame was written. Not an error, no
  reconnect. The resource counts once in `deadline_skipped` (§7). Whether it
  was rotated is decided separately by §5: a pane whose reader call already
  started stays rotated; nothing is undone.
- Admitted, then the exchange fails for any reason, including `E` or a local
  cap expiring after the frame may have been written: an ambiguous error. The
  prepared sequence is burned, the request is never resent (invariant 4), the
  cycle ends with an error, and the daemon reconnects.

The snapshot request is always started at cycle start with deadline `E`.

### 4.3 Expiry gap in degraded cycles

Live keys carry `ttl_ms = 3 × refresh_interval_ms` (15 s by default) and are
renewed once a resource is at least one interval old. The daemon schedules the
next refresh one interval after the previous one ends. When the next cycle
does send the resource, the gap between the two sends is at most `D + I + b`,
where `D` is the cycle duration, `I` the interval and `b` the resource's
offset in that next cycle. At defaults with admission ending at `E − 2 s`,
that gap can reach 17 s, above the 15 s TTL, and rotation (§5) makes that
ordering likely when cycles run long. When a resource is deferred for one or
more cycles (deadline skip, or a cycle that ends early on an error), the gap
grows by a further `D + I` per deferred cycle and has no fixed bound.

Both cases are accepted degraded behavior and go into the spec delta under
these conditions: the key expires and disappears; it never shows a wrong
value. The refresh period, the
TTL multiplier and their defaults are not changed. The clamp alternative was
rejected. A `timeout` executable, process isolation and socket reuse stay out
of scope.

## 5. Fair queues

Both queues use one private helper type `FairQueue<K>` in `publish.rs`.

- **Sync:** at cycle start, take the current ID set from the snapshot. IDs that
  disappeared are removed; survivors keep their relative order; new IDs are
  appended at the tail in snapshot order.
- **Pass:** iteration walks a copy of the queue order taken right after sync
  (`pass`), once, with a `visited` set for this cycle. Rotation moves items
  inside the real queue but never adds to `pass`, so no item is revisited in
  the same cycle and the loop ends after at most N steps.
- **Attempt:** when an item's first budgeted I/O starts (defined per queue in
  §5.1 and §5.2), move it to the tail of the real queue at that moment, even if
  the attempt then times out or fails. A rotation is never undone.
- **Not started:** an item whose budgeted I/O does not start keeps its place.
- **Rotation and skip counting are separate.** Rotation records who started
  I/O. `deadline_skipped` (§7) records which resources the deadline prevented
  from completing their processing or a needed send, once per ID per cycle,
  whether or not the item was rotated.
- **Stopping:** the Git queue stops starting reads once `G` is exhausted. The
  send queue does not stop at the first skip; §5.2 defines what each later item
  still does.
- **Progress (conditional):** if the cycle reaches the stage with its initial
  budget, is not shut down, and the publisher is not poisoned, the first item of
  `pass` that needs budgeted I/O is started, so at least one item rotates per
  such cycle and every such item is started within N such cycles. A cycle that
  ends earlier (snapshot error, shutdown, poison, ambiguous exchange) gives no
  progress guarantee.

### 5.1 Git queue (key: workspace ID)

- Cycle Git deadline `G = min(t_git + 1 s, E)`. Each query runs with
  `min(1 s, G − now)`.
- Results are shared between workspaces with the same cwd within one cycle
  (today's `repository_read_once`). A workspace served from that shared result
  starts no Git I/O, is marked visited, and is not rotated. Only the workspace
  that started the read is rotated.
- Workspaces left in `pass` get `HeadingState::NotAttempted`.

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
runs plan → send from the already-resolved headings.

Every item of `pass` is visited once, in order, while the cycle has not ended
on an error, shutdown or poison. Two budgets apply independently:

- **Reader budget** (`min(reader deadline, E)`): if it is positive, the pane
  calls the reader API. Calling it is the attempt and rotates the pane, even
  when the reader returns immediately (for example its internal BUSY result).
  If it is zero, the reader API is not called and the pane gets a `Timeout`
  observation. That observation goes through today's timeout handling: a valid
  cached observation for the same identity and source is restored, otherwise
  the numeric keys clear. Reader expiry therefore skips only reader I/O; it
  does not end the pass.
- **Admission** (`E − now ≥ 2 s`, §4.2): if the planned report needs a frame
  and admission passes, the metadata exchange starts. Starting it is the
  attempt and rotates the item if it was not already rotated by a reader call.
  If admission fails, no frame is written and the resource counts once in
  `deadline_skipped`. Later items are still visited: a later item whose
  planner needs no frame completes without I/O and is not counted.

So a pane, workspace, no-reader pane or reader-expired pane that sends a frame
is an attempt; one that neither calls the reader nor sends a frame is not an
attempt and keeps its place. A pane whose reader ran and whose needed report
was then refused admission (for example after a reservation persist consumed
the budget) stays rotated and is also counted. No item is revisited this
cycle.

`first_pane_ids` keeps using snapshot order, never queue order, so heading
placement and logo indentation do not move with rotation.

## 6. Headings under partial observation

`HeadingCache` gains `confirmed: Instant`, set only from a real
`verified_at`. A workspace's heading for this cycle is one of these states:

| State | Condition | Workspace keys | First-pane group keys |
|---|---|---|---|
| Confirmed | `Complete(Some(repository))` with `verified_at` | name; ship text, or `—` when `ship_unix` is `None` (confirmed absence of `origin/main`) | copied |
| Fallback | `Interrupted` or `NotAttempted`, previous Confirmed, same cwd and occupants, `now − confirmed < 30 s` | previous values | copied |
| Unknown | everything else, including `Complete(None)` | null | null |

- `Complete(None)` from the Git API also covers rejection (`safe.directory`,
  ownership) and unreadable filesystem state, and carries no `verified_at`. It
  is therefore not treated as a confirmed non-repository: it sends null, not
  `—`, and it discards the previous heading rather than starting a fallback.
- Only `Interrupted` and `NotAttempted` may use the fallback, and only for 30 s
  from the last real confirmation. Fallback never refreshes `confirmed`. A
  cache hit that inherits an old root `verified_at` expires 30 s after that
  root check, not after the hit.
- A stale heading with the same occupant is cleared at 30 s instead of being
  retained indefinitely, as it is today.
- This changes the sidebar-tokens requirements "An unresolved/non-working
  repository SHALL show `—`" and "The previous heading SHALL be kept when the
  workspace cwd and occupants are unchanged", and needs a spec delta.

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
    /// Resources whose needed send the deadline prevented this cycle,
    /// counted once per resource ID, independent of rotation.
    pub deadline_skipped: u64,
}
```

Reader timeouts caused by an expired reader budget are not counted here; they
are ordinary `Timeout` observations under the provider-readers contract. A
workspace whose Git read was not started is not counted either; its heading
state (§6) decides what it sends, and only a refused admission for that send
counts.

The daemon classifies by payload before error kind. `deadline_skipped > 0`
does not set `refresh_failed`, does not reconnect, and logs
`refresh_skipped: cycle_deadline`. Shutdown still yields `Interrupted`,
checked before skip classification. Poison and ambiguous-exchange errors stay
errors.

The existing `Health` Rust struct keeps its fields and meaning. The deadline
count is added as optional diagnostics next to the existing observation data,
compatible with the typed skip, so a deadline skip is distinguishable from both
a capacity skip and an error. The control wire field is optional, matching how
`stages` was added; check the version-skew behavior of `deny_unknown_fields`
against that precedent before adding it.

## 8. Stage timing in production

Replace the `cfg(test)` `StageTimes` with an always-built value that is reset at
the start of each refresh and returned by `refresh_stages()` after every
attempt, including errors and skips.

| Field | Measures | Overlaps with |
|---|---|---|
| `snapshot_us` | `session.snapshot` request and parse | includes the snapshot's share of `socket_us` |
| `git_us` | heading resolution stage, wall time | none |
| `reader_us` | sum of reader calls | none |
| `persist_us` | sum of successful and failed checkpoint writes, including reservation and pruning | inside the pane/workspace loop, not inside `socket_us` |
| `socket_us` | snapshot and metadata exchanges | snapshot exchange also counted in `snapshot_us` |
| `reports` | acknowledged metadata reports | — |
| `checkpoints` | successful `sequences.json` writes in this refresh, not rows | — |

Stages are not additive and their sum is not the cycle duration; doctor's
wording must say so. Measurement uses `Instant` only and writes no files.

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
- Task P builds the per-refresh cycle without diagnostics. Task D (after P)
  makes `MetadataPublisher` own one `Arc<ReaderDiagnostics>` for its lifetime,
  builds each cycle with the diagnostics constructor, and adds a `Publisher`
  method with a `None` default, like `refresh_stages`, that exposes the
  provider snapshots to the daemon control state and doctor. An unavailable or
  skipped sample must stay distinct from zero. Selecting
  `bounded_read_observed` inside readers belongs to another member's reader
  task.

## 10. Tasks, ownership and order

| Task | Files | Starts after | Owner of fixtures |
|---|---|---|---|
| P publish runtime wiring: cycle budget, admission, socket `*_before`, send fair queue, Git fair queue and heading states on the Git observation API, per-refresh `ReadCycle`, production stages, `RefreshSkip` classification | `src/publish.rs`, `src/daemon.rs`, the openspec change of §1 | design GO, a new task, a base that contains the completed stage histogram lane and the integrated Git observation API | publish tests (fake socket, real temporary Git) and daemon health tests |
| D reader diagnostics consumer: publisher-owned `Arc<ReaderDiagnostics>`, `Publisher` accessor, daemon control state, doctor output | `src/publish.rs`, `src/daemon.rs`, `src/doctor.rs` | P integrated and the diagnostics source API integrated | daemon and doctor tests |
| Reader selection of `bounded_read_observed` (another member) | the two reader-side files that member owns | its own assignment | that member |

P and D overlap on `publish.rs` and `daemon.rs`, so they run in series. P runs
in parallel with the reader task because their files do not overlap. P and D
do not edit `model.rs`, the readers, or `ship_time.rs`; those belong to the
reader and Git lanes. Clear is unchanged. Its existing 1 s admission guard is
smaller than the 2 s worst-case round trip; that is a pre-existing overrun,
reported here, not fixed in this plan.

### Rollback

No on-disk format changes: the checkpoint is untouched, and the queues, heading
states and stage values are memory-only. Rolling back is reverting the task's
commit. A daemon restart then returns to the previous behavior, and the
checkpoint written by the new code is valid for the old code.

## 11. Required fixtures

| # | Task | Fixture | Proves |
|---|---|---|---|
| F1 | P | Fake socket delays every report by 400 ms; 12 panes; reader timeout set so few reports fit | Result has `deadline_skipped` equal to the number of resources whose needed frame was refused, each ID once; no reconnect; every resource is acknowledged within ⌈N/k⌉ cycles; items that started no I/O keep their order |
| F2 | P | Server reads a report and never replies | Error, not skip; single request ID observed; sequence burned, not reused; daemon reconnects |
| F3 | P | Last pane of `pass`: its reader runs, then an injected slow reservation persist leaves `E − now < 2 s` | No frame written; `deadline_skipped` counts that pane once; the pane stays rotated (not undone); `U` unchanged or higher; next cycle continues above it |
| F4 | P | Injected delay between admission and exchange so `E` expires in flight | Ambiguous error, never a skip; no resend; sequence burned |
| F5 | P | Checkpoint fault during a budgeted cycle | Poisoned; later cycles return the poison error, never a skip |
| F6 | P | Shutdown set mid-cycle | `Interrupted`, not a deadline skip; clear still uses its own deadline |
| F7 | P | Workspaces placed after many panes, budget for half | Workspace reports are sent within the queue bound |
| F8 | P | Reader for pane 2 checks a file written by the fake socket when it acknowledges pane 1 | observe → plan → send order kept |
| F9 | P | Pane whose reader runs but needs no report, followed by a refused admission | Reader pane rotated and not counted; refused pane counted once; panes that started no I/O keep order; no item visited twice in the cycle |
| F9a | P | Pane A consumes the reader deadline; then pane B with a valid cached observation, workspace W, a no-reader pane, and a pane whose planner needs no frame, all while `E` admission remains | B gets a `Timeout` observation without a reader call, restores its cache and sends its renewal; W, the no-reader pane and B send and rotate; the no-frame pane neither rotates nor counts; `deadline_skipped` is 0 |
| F9b | P | Reader API returns BUSY immediately with a positive reader budget | Counts as an attempt and rotates the pane |
| F10 | P | Two Codex panes, same home | One discovery scan per refresh; a new refresh starts with an empty cycle |
| F11 | P | Three repositories, the first stalls Git past 1 s | Queue `[A,B,C]` → `[B,C,A]` after cycle 1, B and C not rotated while skipped; every workspace started within 3 cycles that reach Git with full budget |
| F12 | P | Two workspaces share one cwd that stalls Git | One Git read per cycle; only the starting workspace rotates; the loop ends after N steps |
| F13 | P | Queue sync with removed and new workspaces | Survivors keep order; new IDs at the tail |
| F14 | P | Injected clock: confirmed at t0, then `Interrupted` | Heading kept at 29 s, null at 30 s; a cache hit whose root was verified at t0 − 25 s expires at t0 + 5 s |
| F15 | P | New workspace not attempted in its first cycle | Null keys, never `—` |
| F16 | P | Confirmed heading, then `Complete(None)` from a `safe.directory` rejection | Null immediately, never `—`, no fallback |
| F17 | P | Repository without `origin/main` | Name plus `—` ship text (confirmed absence) |
| F18 | P | Same occupant, Git interrupted for 31 s | Heading cleared; not retained by occupant equality |
| F19 | P | One refresh with known reports and one reservation; one refresh that errors | `reports` equals acknowledgements; `checkpoints` equals 1; stages present after the error |
| F20 | D | Diagnostics sample skipped or `try_lock` failed | Doctor shows unavailable, not 0 |

## 12. Commander decisions applied

`note-publisher-design-decisions-01`: option A adopted as a cooperative initial
budget with the expiry gap as specified degraded behavior; Unknown headings
send null and `Complete(None)` is Unknown; `publish.rs` and `daemon.rs` change
together in task P from a base after the stage histogram lane; reader
diagnostics wiring is the later task D. Open item: confirm the openspec change
path proposed in §1.

`note-publisher-design-nogo-01` (review
`reviews/publisher-runtime-design-sol61-high.md`, NO-GO on `831552f`): rotation
and `deadline_skipped` are separate and a rotation is never undone; reader
expiry skips only reader I/O and yields a `Timeout` observation while the pass
continues under `E` admission; attempts are reader API calls with a positive
budget or started metadata exchanges; the `D + I + b` gap holds only when the
next cycle sends the resource, and multi-cycle deferral is unbounded degraded
behavior. Confirmed by the commander (`note-publisher-design-go-handoff-01`):
a pane that completes its send after a reader-expired `Timeout` observation is
not counted in `deadline_skipped`, and a workspace whose Git read was not
started is not counted when its metadata processing completes.
