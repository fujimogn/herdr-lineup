# Performance review — 2026-10-18 (Kimi)

## Conclusion

The highest-value work is in the daemon's refresh path, not in CLI argument
parsing. In particular, remove the unnecessary serialization of independent
workspace Git reads, and avoid rescanning every Kimi session for every Kimi
pane. Large, actively appended JSONL files and idle polling are the next tier. The ordinary small statusLine path was already fast in the isolated
measurements; its clear scaling issue was the allowed near-1 MiB payload with
large ignored fields.

This is a review and measurement report only. No implementation or code change
is proposed as landed here, and no source file was edited.

- Base: `f678ad7d3426b9c791bbdd10660d75534758c2b9` (`origin/main` when the
  worktree was created).
- Priorities: **P1** = address first for common refresh latency, **P2** =
  address next for scaling or background cost, **P3** = bounded, conditional
  improvement.
- Effort estimates include focused tests and review, not a broad redesign.

## Purpose, architecture, and processing paths

Lineup is a single Rust crate for macOS and Linux. Its purpose is to publish one
Herdr sidebar row per agent pane, containing vendor identity, Herdr-owned state,
session context occupancy, and opt-in weekly quota, plus a workspace repository
name and the latest local `origin/main` commit time. It is a local metadata
publisher, not a DB, Web API, or UI-framework application.

The specifications explicitly define the token, reader, and daemon contracts.
The following lifecycle grouping is **inferred from the code**, rather than from
a separate formal domain-model document:

- `Pane` / `Occupant` / `SessionIdentity` associate an authoritative Herdr pane
  with an agent and, where available, its exact native session.
- `UsageReading` carries independently validated context and weekly outcomes.
  `WeeklySample` is the non-secret, in-memory account observation supplied by a
  provider worker.
- `Repository` is the workspace heading. `Tokens`, `ResourceId`, and the token
  `Publisher` turn desired display state into sequence-safe, TTL-bounded patches.
- A local reader proves session association; an account worker proves only the
  configured account sample. The publisher overlays the latter only under each
  provider's attribution rules.

The performance-sensitive paths are:

1. **statusLine CLI:** `src/lib.rs:66-77` dispatches to
   `src/statusline.rs:317-374`. The process reads stdin once, parses at most
   1 MiB, writes a small allowlisted observation under the plugin state
   directory, and optionally runs the existing command through `/bin/sh`.
   Process startup and this synchronous work directly precede the agent's
   status-line display.
2. **Daemon refresh:** `src/daemon.rs:301-363` periodically calls
   `MetadataPublisher::refresh`. `src/publish.rs:728-1083` takes one snapshot,
   reconciles checkpoint rows, resolves every workspace heading, reads local
   usage with one shared reader deadline, overlays quota-cache snapshots, and
   publishes pane and workspace patches.
3. **Publication and socket:** each changed or renewable report reserves and
   persists a sequence before sending (`src/publish.rs:625-697`). Herdr 0.9.3
   closes after each response, so `src/socket.rs` opens a request-scoped Unix
   connection, pins the socket device/inode, and applies bounded frame I/O.
4. **Local readers:** Codex and Kimi use bounded discovery, JSONL tail reads,
   fingerprint caches, and exact-session checks. Grok probes exact session
   candidates rather than recursively reading unrelated artifacts. Claude and
   Antigravity read small statusLine records. Each local read runs through the
   deadline mechanism in `src/model.rs:198-302`.
5. **Quota workers:** Codex, Grok, and Kimi account queries are independent of
   refresh. Refresh takes nonblocking in-memory samples; it does not wait for
   app-server or HTTP I/O (`src/model.rs:120-190`,
   `src/publish.rs:330-486,765-798`).

Existing safeguards worth retaining are the 2 MiB plain-JSONL tail bound, the
16 MiB decompressed bound, exact-session/duplicate checks, independent quota
workers, and endpoint identity checks. The recommendations below do not call
for persistent Herdr connections, weaker session association, or fewer safety
checks.

## Isolated measurements

### Method and environment

All measurements used the release binary built from the base commit with Rust
1.99.1 on Linux x86_64. The temporary filesystem was `tmpfs`. Fixtures, HOME,
provider homes, plugin state/config, Git repositories, and the fake Herdr Unix
socket were newly synthesized under `/tmp/herdr-lineup-perf-kimi.8giATA`.
No real agent home, credential, session file, provider endpoint, network
service, or running Herdr server was read or contacted. Every synthetic daemon
was stopped through its own control endpoint; all stop and daemon exit statuses
were zero.

The machine was shared and timing varied between runs. Values below are local
evidence, not portable SLAs. Medians are used for comparisons; code-derived
complexity and expected improvements are labelled separately from measurements.

### CLI and statusLine latency

An interleaved benchmark ran 80 measured iterations per case after warm-up.
`statusline` used a temporary state directory and valid synthetic input.
`shell cat` is the direct `/bin/sh -c 'cat >/dev/null'` comparison.

| Case | Payload | Median | p95 |
| --- | ---: | ---: | ---: |
| `--version` | 0 B | 2.03 ms | 8.03 ms |
| `statusline claude`, persistent record | 71 B | 2.04 ms | 6.45 ms |
| `statusline claude`, persistent record | 1,015,083 B | 5.86 ms | 17.74 ms |
| `statusline claude --then 'cat >/dev/null'` | 71 B | 6.35 ms | 22.51 ms |
| Direct shell `cat >/dev/null` | 71 B | 5.35 ms | 18.05 ms |

Two additional sequential runs also ranked the near-1 MiB persistent case
slower, with medians of 12.18 ms and 12.53 ms, but their small-input/process
baselines varied substantially. The interleaved run is therefore the better
comparison; the absolute milliseconds should still be treated as
machine/load-specific. The chained-command comparison did not justify a
finding against Lineup's thread or shell forwarding.

### Daemon refresh, period, memory, and CPU

The fake server implemented ping, snapshot, and metadata acknowledgement over
request-scoped Unix connections. The configuration used a 1,000 ms refresh
interval so several complete cycles could be sampled, `providers = ["hermes"]`
to keep pane rows live without provider file reads, and network disabled. Each
workspace pointed at a synthetic empty local Git repository. A refresh duration
is from snapshot arrival to the last metadata response for that cycle; period
is from one snapshot start to the next. Each steady row summarizes the last
five complete cycles. RSS and CPU are short 2.1 s samples of the daemon process,
not its Git children.

| Scenario | Reports/refresh | Median refresh | Median period | RSS | CPU over sample |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 workspaces, 0 panes | 0 | 0.073 ms | 1,007.8 ms | 4,456 KiB | 0.95% |
| 1 workspace, 0 panes | 1 | 39.9 ms | 1,040.8 ms | 4,624 KiB | 0.95% |
| 8 workspaces, 0 panes | 8 | 383.9 ms | 1,403.6 ms | 4,496 KiB | 4.69% |
| 1 workspace, 8 panes | 9 | 53.8 ms | 1,067.7 ms | 4,580 KiB | 0.95% |
| 1 workspace, 32 panes | 33 | 70.5 ms | 1,081.8 ms | 4,876 KiB | 1.90% |

All expected report counts were observed. The measured period approximately
equals the configured interval plus refresh duration and loop jitter, matching
`src/daemon.rs:335-362`. Thus refresh work directly adds to sidebar reflection
latency. The idle CPU figure is a short Linux tick-accounting estimate, not a
battery measurement; RSS figures are not leak tests.

### Reader scaling and JSONL cache behavior

A separate release harness called the public readers with synthetic homes.
Each warm-cache cell ran 20 iterations in each of two processes; the table shows
the range of the two medians. Unrelated files were never opened by the harness
itself. The target rollout/wire was unchanged during warm reads.

| Reader case | Result |
| --- | ---: |
| Codex, 0 unrelated rollouts | 0.13–0.69 ms |
| Codex, 1,000 unrelated rollouts | 1.22–2.62 ms |
| Codex, 2,000 unrelated rollouts | 2.50–5.70 ms |
| Kimi, 0 unrelated states | 0.24–1.37 ms |
| Kimi, 500 unrelated states | 6.71–7.31 ms |
| Kimi, 1,000 unrelated states | 11.75–15.54 ms |
| Kimi, 2,000 unrelated states | 27.35–31.95 ms |
| Kimi, 2,197,306-byte wire, cold read | 32.48–35.99 ms |
| Same large wire, unchanged cached read | 0.28–0.47 ms |

The cache is effective for an unchanged large wire. The scaling rows show that
wire-content caching does not remove session discovery. A changed large file is
not represented by the warm-cache row; its code path performs the bounded tail
read and parse again.

## Findings

### 1. P1 — Serial Git heading reads dominate multi-workspace refresh

- **Location:** `src/ship_time.rs:53-89` (`read_repository_before`),
  `src/ship_time.rs:177-267` (`git_output`), and
  `src/publish.rs:807-829` (the workspace loop).
- **Problem and condition:** every refresh runs `rev-parse
  --is-inside-work-tree`, `rev-parse --git-common-dir`, `worktree list`, and
  `for-each-ref` for every workspace, sequentially. Independent workspaces
  therefore wait for all earlier workspaces even though each heading has its own
  one-second deadline and no result depends on another workspace.
- **Evidence and impact:** the isolated eight-workspace refresh had a 383.9 ms
  median versus 39.9 ms for one workspace, with all expected reports and no
  provider reads or network. That is roughly 43 ms of additional wall time per
  extra workspace in this fixture. Since workspaces are resolved before panes,
  this also delays agent usage and all metadata publication.
- **Improvement:** evaluate independent workspace headings through a small,
  bounded worker pool and apply the results back in snapshot order. Share the
  shutdown flag, preserve a separate monotonic deadline and process-group
  cleanup for each heading, and cap concurrency rather than creating one
  unbounded child group per workspace. Keep all four local read-only queries in
  the initial change; do not cache ship timestamps or fetch. Query consolidation
  can be considered separately only after its exact bare/linked-worktree
  behavior is proved.
- **Expected effect and trade-offs:** with a cap of four, independent heading
  reads could approach a fourfold wall-time reduction on a multi-core host,
  **not an implemented measurement**. This avoids stale-cache correctness risk
  and keeps the query semantics intact. It adds scheduling code and bounded
  peak CPU/process usage; result ordering and timeout cleanup must remain
  deterministic.
- **Priority / effort / risk:** P1; approximately 1–2 days; medium risk.
- **Verification:** repeat the 1/8-workspace harness at concurrency one and at
  the chosen cap, recording child count, peak CPU, and output order. Cover
  linked worktrees, missing `origin/main`, a changed ref, repository
  deletion/recreation at the same cwd, cwd changes, local midnight, Git timeout,
  and shutdown with children still running. Assert no network and no change to
  heading tokens.

### 2. P1 — Kimi rereads every session state per pane despite the wire cache

- **Location:** `src/reader_kimi.rs:55-104` (`read_session`),
  `src/reader_kimi.rs:107-132` (`state_binding`),
  `src/model.rs:402-435` (`Budget::files`), and
  `src/publish.rs:852-930` (per-pane dispatch). Config reparsing is at
  `src/reader_kimi.rs:345-364`; the wire-only cache hit is at
  `src/reader_kimi.rs:191-204`.
- **Problem and condition:** each Kimi read recursively discovers every
  `state.json`, then opens, stats, reads, and parses each one to prove exact
  id/cwd association and duplicates. With multiple Kimi panes this whole scan
  repeats per pane. An unchanged wire avoids only its own content read; the
  state scan and `config.toml` read/TOML parse still occur.
- **Evidence and impact:** with an unchanged target wire, 1,000 unrelated
  synthetic states produced 11.75–15.54 ms medians and 2,000 produced
  27.35–31.95 ms, versus 0.24–1.37 ms with none. The existing research recorded
  715 real state files at inspection time, so a large session home is a
  documented condition rather than a purely theoretical bound. Complexity is
  approximately O(Kimi panes × total session states), plus per-read config
  parsing.
- **Improvement:** build a provider-specific session-discovery result once per
  refresh and let each pane perform its own exact match against that bounded
  result. Cache individual parsed state bindings and the model-window document
  by their full file fingerprints, preserving duplicate, symlink, unreadable,
  and concurrent-change rules. This is not a cwd-only shortcut and should not
  be shared with Codex's differently shaped discovery.
- **Expected effect and trade-offs:** one bounded discovery/index pass per
  refresh changes the dominant work toward O(states + panes); four panes would
  avoid roughly three repeated full scans, **not an implemented measurement**.
  A small explicit index type can improve readability. Snapshot semantics and
  cache invalidation add moderate maintenance and correctness cost.
- **Priority / effort / risk:** P1; approximately 2–3 days; medium-high risk.
- **Verification:** use 0/500/1,000/2,000-state fixtures and multiple distinct
  Kimi panes. Assert byte-for-byte equivalent outcomes for duplicates, a broken
  unrelated state, an unreadable same-id directory, symlinks at every component,
  changed cwd/id, config changes, and a file changing during a read. Verify both
  cold and warm latency and the existing 4,096-entry limit.

### 3. P2 — Codex discovery repeats the full rollout walk for every pane

- **Location:** `src/reader_codex.rs:68-95` (`read_session_with_hooks`), especially
  the collect-then-retain sequence at lines 70–80; recursion is in
  `src/model.rs:402-435`.
- **Problem and condition:** every Codex read walks both `sessions` and
  `archived_sessions`, collects every rollout path, and only then retains names
  for the requested native id. The fingerprint cache skips rollout content but
  deliberately not discovery, so unchanged files still incur the full walk per
  pane per refresh.
- **Evidence and impact:** warm reads with 2,000 unrelated synthetic rollouts
  took 2.50–5.70 ms medians versus 0.13–0.69 ms with none. Multiple Codex panes
  multiply this discovery cost. The impact is smaller than the Kimi state scan
  in the same fixture but grows with both archive size and pane count.
- **Improvement:** create one bounded, provider-specific rollout index per
  refresh and query every requested id against it, retaining duplicate and
  symlink rejection. As a smaller complementary change, filter id-relevant
  filenames during discovery instead of allocating a `PathBuf` for every
  rollout and retaining afterwards. Do not assume a globally newest file or
  bypass archived-session duplicates.
- **Expected effect and trade-offs:** shared discovery changes repeated work
  toward O(rollouts + panes); filename prefiltering reduces allocations even
  for one pane. The index adds lifecycle code but can remain local to the
  publisher's refresh context. Concurrent directory changes must not turn a
  cached association into a false unique match.
- **Priority / effort / risk:** P2; approximately 1–2 days; medium risk.
- **Verification:** measure one and several Codex panes over the same large
  fixture. Cover plain/zstd siblings, duplicates across sessions/archive,
  symlinks, entry-limit behavior, a rollout appearing or disappearing during a
  refresh, and unchanged fingerprint-cache hits.

### 4. P2 — An active large JSONL reparses the whole 2 MiB tail after each change

- **Location:** `src/model.rs:521-599` (`read_jsonl_tail`),
  `src/reader_kimi.rs:185-235,238-289`, and
  `src/reader_codex.rs:85-138,141-201`.
- **Problem and condition:** the fingerprint cache hits only when device, inode,
  size, and mtime are unchanged. For an uncompressed file larger than 2 MiB,
  an append between refreshes therefore causes another 256 KiB header read,
  2 MiB tail read, UTF-8 decode, and forward JSON parse. Active sessions append
  regularly; idle unchanged files are the favorable cache case. Codex follows
  the same bounded-tail pattern. Compressed Codex streams are a separate case
  and should retain their full bounded decode.
- **Evidence and impact:** one cold 2,197,306-byte Kimi wire took 32.48–35.99
  ms; an unchanged cached read took 0.28–0.47 ms. The current tail bound keeps
  work independent of total file size, which is good, but every changed large
  file still pays the fixed tail-parse cost. Several active large panes can add
  tens of milliseconds to refresh and delay all later publication.
- **Improvement:** for the same inode with verified append-only growth, retain a
  provider-specific parser checkpoint and read only complete newly appended
  records, carrying any incomplete final record. Reuse the existing final
  path/file stat checks and fall back to a full bounded tail read after
  replacement, truncation, eviction, ambiguous state, or any append-during-read
  uncertainty. Keep compressed streams on the existing full-read path.
- **Expected effect and trade-offs:** work can scale with bytes appended since
  the last refresh instead of a fixed 2 MiB, potentially avoiding most of the
  measured cold-tail parse for small appends. This is **code-estimated**, not an
  implemented result. Incremental parser state is less readable and has the
  highest correctness burden among these proposals; isolate it in a small type
  with exhaustive boundary tests rather than spreading offsets through the
  readers.
- **Priority / effort / risk:** P2; approximately 2–4 days; high risk.
- **Verification:** compare full and incremental outcomes for random append
  prefixes, split UTF-8 and split JSON, malformed completed records, alias
  changes, turn/compaction/rebase events, token-count ordering, header binding,
  truncation/replacement, and appends at every stat boundary. Benchmark small
  appends to 2–16 MiB files and prove identical cache invalidation.

### 5. P2 — Empty daemon still polls hundreds of times per second

- **Location:** the main daemon loop sleeps for 20 ms at
  `src/daemon.rs:301-363`; the control worker sleeps for 5 ms at
  `src/daemon.rs:377-400`; each idle quota worker wakes in 100 ms slices at
  `src/model.rs:141-161`.
- **Problem and condition:** with no quota workers, the two fixed loops can wake
  about 250 times per second even with no workspaces, no panes, and no socket
  input. Enabling all three account workers adds up to about 30 more wakeups
  per second. These loops also determine shutdown observation latency.
- **Evidence and impact:** the empty synthetic daemon used an estimated 0.95% of
  one logical CPU over a 2.1 s sample with 4,456 KiB RSS. The percentage is a
  short, coarse measurement, but the fixed wakeup rates are direct code facts.
  On a long-running session this is unnecessary background work and potential
  battery impact.
- **Improvement:** replace fixed sleeps with deadline-aware event waits and
  explicit wakeups: wait until the next refresh or control shutdown in the main
  loop, use `poll`/an equivalent wakeup for the listener, and unpark quota
  workers on shutdown. Preserve the existing stop deadline and spurious-wakeup
  handling; do not merely lengthen the sleeps and hide stop latency.
- **Expected effect and trade-offs:** removes most of the 250 idle wakeups per
  second and should reduce idle CPU below the sampled figure, **subject to
  implementation measurement**. Cross-platform wakeups add some complexity, but
  centralizing them in one small wait mechanism is preferable to scattered
  timing constants.
- **Priority / effort / risk:** P2; approximately 1–2 days; medium risk.
- **Verification:** measure idle CPU and wakeups for at least 60 s with zero and
  three workers. Exercise immediate stop, stop during refresh/request, repeated
  ensure/stop, spurious wakeups, and refresh scheduling on both Linux and
  macOS. Require the existing five-second stop bound and no missed control
  request.

### 6. P3 — Large statusLine payloads build a full JSON DOM for ignored content

- **Location:** `src/statusline.rs:157-187` (`capture`) and the input stage at
  `src/statusline.rs:329-345`.
- **Problem and condition:** after reading the allowed payload, `capture` calls
  `serde_json::from_slice::<Value>`, allocating a tree for every unknown nested
  field before selecting the binding key, context, and weekly fields. The
  measured near-1 MiB fixture contained large ignored objects; ordinary small
  statusLine input does not exhibit the same cost.
- **Evidence and impact:** in the interleaved measurement, the persistent
  near-1 MiB case had a 5.86 ms median versus 2.04 ms for 71 bytes, a 3.82 ms
  median difference directly on the agent's status-line path. The p95 values
  were 17.74 ms and 6.45 ms. Actual provider payload-size distribution was not
  measured, so this is a bounded worst-case finding, not a claim about every
  callback.
- **Improvement:** deserialize into allowlisted typed structures, or a small
  custom visitor, that validates and skips unknown values without constructing
  a complete `Value` tree. Preserve the current snake/camel-case aliases,
  independent context/weekly failures, Agy pool selection, 1 MiB limit, and the
  original buffered bytes used by `--then`.
- **Expected effect and trade-offs:** for ignored-field-heavy payloads, latency
  and temporary allocation should move toward the small-payload case; the
  measured gap was 3.82 ms, but the achievable saving is **unverified**. Typed
  alias structures may be more verbose than the current helpers, so keep the
  change confined to capture and avoid a new generic parser abstraction.
- **Priority / effort / risk:** P3; approximately 0.5–1 day; medium risk.
- **Verification:** benchmark 1 KiB, 64 KiB, and 1 MiB payloads with irrelevant
  arrays/objects and both providers. Run the full parser compatibility suite,
  invalid/oversized JSON chaining tests, and allocation/latency comparisons;
  verify byte-identical chained stdin/stdout and unchanged stored records.

## Deferred or intentionally not pursued

- **Full checkpoint rewriting:** `src/publish.rs:549-587,625-697` serializes all
  checkpoint rows and performs file/directory `sync_all` before each report, so
  naive serialized bytes can grow quadratically with resource count. The
  measured 33-resource case remained modest (70.5 ms median including one
  workspace's Git work), so a journal or new durability design is not justified
  by this review. First profile a much larger realistic resource count on a
  persistent filesystem. Any later design must still persist the exact reserved
  sequence before sending; an in-memory-only or post-send checkpoint is not an
  acceptable shortcut.
- **Refresh scheduling semantics:** the measured period includes refresh
  duration because the next interval is scheduled after completion. This review
  reports that sidebar-latency effect but does not prescribe start-to-start
  scheduling: token renewal is explicitly based on time since successful
  publication, so changing cadence requires a coordinated contract/planner
  decision, not a one-line timer change.
- **Socket connection reuse and endpoint-check reduction:** Herdr 0.9.3 requires
  request-scoped connections, and endpoint pinning is a safety boundary. No
  persistent-connection proposal is made. Micro-optimizing token normalization,
  hex formatting, or frame buffers is also deferred while process, discovery,
  and tail-parse costs dominate.
- **Grok discovery and quota concurrency:** exact-id probing and independent
  nonblocking quota snapshots already address the main concerns. No provider
  network query was run, so HTTP/app-server latency is not assessed.
- **One-shot actions and unrelated architecture:** configure, font install, and
  release tooling are not daemon/statusLine hot paths. No DB, Web API, UI
  framework, DDD, or broad common-abstraction proposal is appropriate here.

## Unverified and boundaries

- macOS behavior, persistent-disk `fsync` cost, visible Herdr sidebar rendering,
  and end-to-end latency through a real Herdr server were not measured.
- Real provider payload distributions, real session homes, credentials, and
  authenticated quota workers were deliberately excluded. Network-worker CPU
  figures beyond the three-worker wakeup calculation are therefore unverified.
- Compressed `.zst` rollout latency, long-running memory/leak behavior, and
  multi-hour refresh cadence were not measured. Short RSS samples establish only
  the synthetic baseline.
- The exact effect of every proposed improvement remains to be measured after
  implementation. No proposal should be treated as permission to weaken exact
  session binding, duplicate detection, symlink refusal, sequence durability,
  endpoint identity, shutdown deadlines, or network opt-in.
- Measurement artifacts and harnesses remain outside the repository under the
  temporary path named above. The repository deliverable is this Markdown file
  only.
