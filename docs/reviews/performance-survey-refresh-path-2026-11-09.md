# Refresh path performance survey (2026-11-09)

Status: investigation and documentation only. The inspected source is
`9fdac8d45c98df4ea034ad69c538765b8ce77e8a` (`origin/develop` at worktree
creation). No source, protocol, Herdr server, running session, or user settings
were changed. Git heading and reader internals belong to survey B.

## Prior evidence and boundary

The [isolated latency investigation](latency-investigation-2026-11-04.md)
already establishes a post-completion 10 s refresh wait, phase-dependent
synthetic report-to-stored-token delay, overlapping stage intervals, and no
rendered-latency or stable-tail measurement. The
[alternating stage record](stage-timing-comparison-2026-11-04.md) establishes
no comparative speed-up. The
[wired Git heading record](git-heading-latency-wired-2026-11-04.md),
[real-disk reader record](reader-discovery-real-disk-2026-11-04.md), and
[short live reader observation](stuck-worker-frequency-2026-11-04.md) have
different fixtures and do not isolate publisher socket time. The
[Codex reader trial](codex-reader-perf-trial-2026-11-04.md) was rejected by the
owner; its append-only premise and relaxed read guarantee are not proposals
here. `TODO.md` Conditional already names period/event refresh and socket reuse
with explicit preconditions; neither is a completed optimization.

The commander supplied an **unverified doctor transcription** for a running
daemon: 8,247 refresh attempts, accumulated socket about 7,463 s, Git about
819 s, reader about 521 s, persist about 457 s, snapshot about 205 s, and 381
deadline-skip attempts. Dividing each cumulative figure by 8,247 gives about
905, 99, 63, 55, and 25 ms per attempt respectively, and 381/8,247 = 4.62%.
These are cumulative-stage ratios, **not** a partition of refresh wall time:
snapshot includes socket work; persist happens inside report processing;
samples can include failed attempts and may describe a different binary.
There is no source identity, topology, per-report count, histogram, or
event-correlated sample for this transcription. This lane did not query that
daemon or its Herdr session.

## Current refresh path and round trips

1. The daemon calls `refresh` and starts its next interval only after the
   attempt finishes (`src/daemon.rs:1075-1113`). The publisher starts a shared
   envelope of snapshot 2 s + Git 1 s + configured reader timeout (default
   2 s) + metadata 2 s, hence 7 s by default (`src/publish.rs:68-85`,
   `src/publish.rs:2361-2376`). These are admission/cap terms, not measured
   stage allocations or a strict wall-clock bound.
2. One `session.snapshot` request/response is made before planning. The
   response is parsed as authoritative workspace/pane identities and stored
   keys (`src/publish.rs:1495-1527`, `src/protocol.rs:105-194`). Snapshot
   absence prunes local sequence state and may persist a checkpoint
   (`src/publish.rs:1694-1719`). The snapshot stage includes its socket exchange
   and checkpoint load; the separate socket stage includes the exchange.
3. Git headings are resolved before the send queue; each pane is observed and
   planned when visited. Reports are sent in a fair pass over panes and
   workspaces (`src/publish.rs:2354-2464`). `needs_report` suppresses an empty
   patch, but a changed occupant/value, missing live key, or renewal after one
   refresh interval can require a report (`src/tokens.rs:459-474`,
   `src/tokens.rs:536-562`). A pane report uses `pane.report_metadata`; a
   workspace report uses `workspace.report_metadata`, each with one resource,
   source, TTL, sequence, and token map (`src/protocol.rs:48-83`). There is no
   multi-resource request in this protocol adapter.
4. Each admitted metadata report gets one request and one acknowledgement,
   checked for the matching id and `ok` result (`src/publish.rs:1628-1691`,
   `src/protocol.rs:86-102`). Thus a successful cycle with P eligible panes
   and W eligible workspaces has **1 + P + W** application round trips;
   unchanged resources can reduce this to one snapshot round trip. This count
   excludes initial connection and any daemon recovery health exchange. It
   also assumes no capacity/deadline refusal or error. Herdr v0.9.3 closes
   after each response, so the next request reconnects to the pinned endpoint
   (`src/socket.rs:133-145`, `src/daemon.rs:1040-1047`). A one-pane,
   one-workspace cold fixture therefore has three round trips; the 256-pane
   fixture has 258.
5. Sequence windows amortize durable writes: on exhaustion, an upper bound is
   persisted before the corresponding report is sent; acknowledgement does
   not rewrite the same checkpoint (`src/publish.rs:1453-1462`,
   `src/publish.rs:1606-1611`, `src/publish.rs:1684-1690`). The checkpoint
   writer serializes rows, flushes and syncs a temporary file, renames it,
   and syncs the directory (`src/publish.rs:1383-1435`). A failed write
   poisons further publication because the on-disk upper bound is uncertain.

Admission refusal is specifically `remaining envelope < 2 s`, checked after
any reservation persistence and before a metadata frame
(`src/publish.rs:1639-1667`). It counts as `deadline_skipped` for that resource
and the fair pass can continue to count other refusals. Capacity skips are
separate. A request admitted and then timed out or otherwise left without a
response is an **error**, not a skip; its sequence is burned and the cycle
stops (`src/publish.rs:1674-1691`, `src/publish.rs:2446-2463`,
`src/daemon.rs:486-495`). This means the 381 supplied skip attempts cannot be
attributed to socket I/O alone. Earlier snapshot, Git, reader, synchronous
planning, persistence, prior reports, or host scheduling could consume the
budget. No per-attempt traces were available to distinguish them.

Each exchange serializes JSON and sends one newline frame, then reads and
parses one response (`src/publish.rs:2612-2622`). The same absolute deadline
passes through an implicit reconnect, write, and read. Connect is locally
capped at 1 s; each frame at 500 ms (`src/socket.rs:123-178`,
`src/socket.rs:212-216`, `src/socket.rs:382-436`). The transport checks the
socket pathname identity around I/O and refuses uncertain replay. These caps
do not bound synchronous filesystem checks, JSON work, or scheduling.

## New isolated observation

The existing ignored `publish::tests::measure_refresh_stages` was run from a
dedicated worktree, with pinned Rust 1.99.1 and `cargo test --locked --lib
--no-run -j 2`. Build exited 0 in 36.47 s; its `target` stayed in this
worktree (719,631,138 apparent bytes after the run). A test subprocess removed
all inherited `HERDR_*` variables, set `LINEUP_BENCH_ROOT=/tmp` and
`LINEUP_BENCH_REPEATS=2`, then invoked the identified library binary with
`--exact --ignored --nocapture --test-threads=1`. The test exited 0 in
2.148 s (one test passed). The existing fixture binds its own fake Unix
socket, creates a fresh local Git repository and publisher per row, returns
fixed `SampleReader` data, and touches no actual Herdr session
(`src/publish.rs:6362-6478`). Its temporary fixtures self-cleaned.

Host: shared Linux `dev`, ext4 worktree target and tmpfs `/tmp`. Load averages
were 2.84/5.21/8.38 at pre-build inspection, 5.92/5.73/8.36 immediately
before test, and 6.89/5.94/8.42 immediately after. Other lanes and host
scheduling were uncontrolled; no cache flush or affinity was used. The test
started 2026-11-09 11:40:12 UTC and ended 11:40:14 UTC. There are only two
cold rows per pane count: these observations are not tail quantiles or a
performance comparison with another revision.

| Panes | Total ms (two rows) | Socket ms (two rows) | Snapshot ms | Persist ms | Acked reports |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.138 / 8.814 | 2.985 / 3.073 | 0.373 / 0.257 | 0.062 / 0.053 | 2 |
| 8 | 24.120 / 24.817 | 17.435 / 17.668 | 0.605 / 1.093 | 0.053 / 0.070 | 9 |
| 32 | 82.397 / 80.933 | 68.124 / 67.175 | 2.666 / 2.046 | 0.062 / 0.096 | 33 |
| 128 | 310.038 / 297.388 | 262.134 / 263.926 | 7.404 / 6.735 | 0.195 / 0.103 | 129 |
| 256 | 588.362 / 595.752 | 532.269 / 539.592 | 13.129 / 18.840 | 0.088 / 0.091 | 257 |

Every row had one checkpoint row. This is **not** a count of checkpoint writes.
Socket is the largest recorded stage at 8–256 panes in this fixture. Stage
columns overlap and must not be summed. The fake socket does not reproduce
Herdr scheduling, storage, rendering, or a real provider's transitions.
Production socket latency distribution, cause of the 381 skip attempts,
macOS behavior, rendered event-to-visible latency, and stable tail behavior
remain unmeasured.

## Candidate investigations, ordered by potential effect

These are proposals for separately scoped work, not changes approved or
implemented by this survey. Ordering is based on possible reduction of the
dominant per-resource exchange path in high-report cycles; the actual gain
needs an isolated comparison and real topology evidence.

### 1. Investigate an atomic multi-resource metadata operation with Herdr

- **(a) Basis:** Measured: the 256-pane fake-socket cycle acknowledged 257
  reports and spent 532–540 ms in socket exchanges. Code: one report contains
  one resource (`src/protocol.rs:48-83`), with one serial exchange per report
  (`src/publish.rs:2401-2455`). Inference: reducing per-resource round trips
  could materially reduce high-pane socket time; this requires a host protocol
  capability and is not a plugin-only change.
- **(b) Effect/uncertainty:** Potentially the largest improvement when many
  reports are eligible, perhaps reducing application round trips from
  `1 + P + W` to a snapshot plus bounded batches. No numeric speed-up is
  justified: host processing and acknowledgement cost are unmeasured, and
  typical live eligible-report counts are unknown.
- **(c) Guarantees/risk:** Atomic or per-item acknowledgement semantics,
  sequence ordering, TTL, capacity, other plugins' keys, crash recovery,
  fairness, and ambiguous partial acceptance all need a specified contract.
  One failed batch must not cause blind replay of accepted reports.
- **(d) Size:** Large, cross-project host protocol and plugin work.
- **(e) Verification:** First obtain a host protocol design and acceptance
  fixture. Compare fixed-topology report counts, socket time and deadline
  skips using the same isolated server and AB/BA runs. Test partial failure,
  stale sequence, endpoint replacement, resource capacity and stop/clear;
  measure report-to-stored-token and rendered latency separately.

### 2. Investigate connection reuse only after endpoint and syscall evidence

- **(a) Basis:** Code: completed responses trigger a reconnect for the next
  request (`src/socket.rs:133-145`); the daemon documents host one-response
  closure (`src/daemon.rs:1040-1043`). Measured socket totals above include
  these reconnects but do not isolate connect cost. `TODO.md` already makes
  socket reuse conditional on a syscall trace and adversarial endpoint
  replacement fixture.
- **(b) Effect/uncertainty:** Could reduce connection overhead across a
  many-report cycle if Herdr supports it. Benefit may be small if response
  processing dominates; no connection-only timing was captured here.
- **(c) Guarantees/risk:** Reuse must preserve pathname device/inode checks,
  endpoint generation binding, absolute deadlines, one-response framing,
  reconnect failure behavior, and no replay of ambiguous requests. Host
  support is required for a persistent connection.
- **(d) Size:** Medium to large, depending on host protocol support.
- **(e) Verification:** On a private socket fixture, trace connect/poll/read
  counts and durations without payloads. Add adversarial socket replacement,
  EOF, backpressure, incomplete response, and failed reconnect cases before
  an isolated Herdr comparison on Linux and macOS.

### 3. Attribute socket and admission tails before tuning deadlines

- **(a) Basis:** Measured: supplied doctor totals are unverified and this
  survey's two-row fixture has no tail distribution. Code: the socket stage
  combines snapshot and report exchanges (`src/publish.rs:1513-1523`,
  `src/publish.rs:1674-1677`); admission observes only the residual envelope
  (`src/publish.rs:1647-1663`). A skip does not identify its earlier cause.
- **(b) Effect/uncertainty:** Direct latency gain is zero. The potential
  payoff is choosing the right subsequent optimization and quantifying where
  skips arise; instrumentation overhead is unknown.
- **(c) Guarantees/risk:** Diagnostics must exclude session contents, tokens,
  native identities and request payloads. Counter updates must not delay a
  refresh enough to create skips or obscure shutdown behavior.
- **(d) Size:** Small to medium, plugin-only if limited to aggregate timings.
- **(e) Verification:** Add aggregate connect/write/host-response/parse and
  report-count buckets to an isolated fixture, with deterministic clock and
  admission tests. Compare instrumented and uninstrumented AB/BA runs before
  retaining diagnostics; sample per-attempt outcome and stage distributions
  without accessing a user session.

### 4. Revisit checkpoint cost only for demonstrated write tails

- **(a) Basis:** Code: reservation writes sync file and directory before send
  (`src/publish.rs:1383-1462`), and pruning may write on an authoritative
  snapshot (`src/publish.rs:1694-1719`). Measured: the fake-socket cold rows
  recorded 0.05–0.20 ms persist time; the supplied unverified doctor total is
  about 55 ms/attempt. Different storage and workloads could explain the gap,
  but this is an inference, not a demonstrated bottleneck.
- **(b) Effect/uncertainty:** At most the measured persist component of a
  matched workload, with a possible effect on admission tails. Current data
  do not justify a faster persistence format or less durability.
- **(c) Guarantees/risk:** The durable upper bound must precede sends, remain
  monotonic across crashes/restarts and endpoint changes, and poison on an
  uncertain write. Delaying or omitting sync would weaken this guarantee.
- **(d) Size:** Medium for measurement and any safe implementation; high
  validation cost.
- **(e) Verification:** Measure write counts, row counts and sync latency on
  isolated ext4/APFS fixtures with fixed sequence-window demand; inject
  file-sync, rename and directory-sync failures, then restart and verify
  monotonic sequences before considering a design change.

## Changes to avoid on present evidence

- Do not merely increase the 7 s envelope or 2 s metadata admission term:
  this can conceal queue pressure, lengthen refresh cycles, and delay the
  post-completion polling schedule without reducing per-report work. It also
  changes skip versus admitted-error behavior.
- Do not parallelize independent metadata sends without a host ordering and
  acknowledgement contract. Fair queue rotation, per-resource sequences,
  restart recovery and no replay of uncertain requests currently depend on
  explicit serial decisions.
- Do not skip authoritative snapshots, extend renewal to the 30 s TTL edge,
  or cache a previous snapshot as truth. Snapshot identities and stored-key
  repair are safety inputs; long cycles and jitter can otherwise expire keys.
- Do not drop file/directory sync or write an upper bound after sending.
  Sequence reuse after a crash would violate the publisher's durability rule.
- Do not revive the rejected Codex append-only reader proposal, nor infer a
  production or visible-latency speed-up from this fake-socket run.
