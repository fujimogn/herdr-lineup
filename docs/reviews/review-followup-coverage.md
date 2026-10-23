# Review follow-up coverage (2026-10-23 reviews)

This record maps every adopted, conditional, and rejected finding from the
2026-10-23 whole-repository review and latency investigation onto the local
integration snapshot named below. It does not accept, integrate, push, or
ship that snapshot.

The review originals live outside the repository:

- `reviews/whole-repo-2026-10-23/consolidated.md`
- `reviews/latency-2026-10-23/consolidated.md`
- `reviews/implementation-queue-2026-10-23.md`

Those files are the primary evidence for what the reviews said. This record
keeps only the finding ID, the code evidence, the remaining limit, and the
judgment condition.

## Source snapshot

- Source snapshot: `17873866afedfa21f73315e2e4d39af8ce03a0dc`, the commander's
  latest local integration foundation (draft; the final source SHA is fixed
  after the publisher and diagnostics wiring lands). Line numbers were read
  from `d95e5b5b9fe703e5e0259887d47a52470d6055a2` unless a row names a later
  commit. A documentation commit is never an implementation SHA.
- Review target of both reviews: `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`.
  The previous version of this record used `96829df7e6fc51af327b448f5b8b3596f0a96d3f`.
- `develop` and `origin/develop` are `b9d1a6d290d7a153f4743d284a5d9839d1658233`
  (a hook configuration move, `d3ccbc17..b9d1a6d`). The foundation keeps that
  move, but `develop` does not contain the foundation.
- `Cargo.toml` and `herdr-plugin.toml` stay at 0.2.2. Nothing below is
  released. Shipment stays on hold until the owner lifts it.
- Work in progress on other lanes is not counted here: publisher runtime
  wiring (cycle and stage samples from `MetadataPublisher`, the Git cache,
  shared deadlines, and one `ReadCycle` per refresh; plan in
  `docs/plans/publisher-runtime-wiring.md`) and, after it, reader diagnostics
  in control and `doctor`.

## Status classes

| Class | Meaning | Completion rule |
| --- | --- | --- |
| `fixed-in-candidate` | The reviewed behavior is present at the snapshot and a regression check exists in tree. | Not shipped. Do not describe it as released or as accepted by the ship gate. |
| `partial` | Part of the finding is in the snapshot. The named remainder is still open. | Do not mark the finding closed. |
| `api-unwired` | A tested internal API exists at the snapshot, but production refresh does not call it. | Do not claim a runtime effect until the wiring lands with its own evidence. |
| `measurement-only` | An isolated fixture result is recorded. Product behavior is unchanged. | Do not treat the numbers as a budget, a speed-up, or a macOS or live Herdr result. |
| `conditional` | The review proposed it only under a condition that is not met. | Not an implementation obligation. Keep the reason and the open condition. |
| `separate-task` | Owner-assigned to a different task outside this follow-up. | Keep current behavior and the open item. Never mark it resolved here. |
| `spec-waiting` | The safe behavior is not decided. | Do not code a guess, and do not mark the row done. |
| `rejected-wrong-basis` | The original claim does not match the reviewed code. | Keep the rejection. Do not reopen it as an implementation task. |

## Whole-repository findings

Agreement denominator: 4.

| ID | Status | Evidence at the snapshot | Remaining limit | Judgment condition |
| --- | --- | --- | --- | --- |
| H1 font rollback can restore pre-write bytes over a later external edit | `partial` | `f319f52`, `ead231d`: automatic rollback is gone. `src/font.rs:608` and `:650` keep partial files and report that no automatic rollback or deletion was attempted, naming the journal for manual recovery. Planned original bytes are compared before each write. | Each check stays separate from the following pathname `rename`, so an external edit between them can still be replaced. Recovery after a failure is manual, never automatic. | Not closed while a check and its `rename` are separate operations. A closing design needs owner approval. |
| M1 uninstall can delete a font replaced after the content check | `partial` | `75865cd`: `src/font.rs:632-638` calls `before_font_remove`, compares `stamp` with the planned original, conflicts on mismatch, and only then calls `remove_file`. | A replacement after that stamp and before `remove_file` can still be deleted. | Same as H1: not closed while removal is a separate pathname operation. |
| M2 Kimi `openat` lacked `O_CLOEXEC` | `fixed-in-candidate` | `2aa1ddc`. `src/reader_kimi.rs:214-215` ORs `O_CLOEXEC` into the shared `openat` helper used for the directory and the file. | None in code. | Stays fixed while every `openat` in that reader keeps `O_CLOEXEC`. |
| M3 Kimi transient credential stat cleared the weekly sample | `fixed-in-candidate` | `800cef5`. `src/quota_kimi.rs` returns `SourceState::Transient` for non-`NotFound` stat errors; publication does not treat that as a stamp change. Regression: `source_state_distinguishes_present_missing_symlink_and_transient_permission_error`. | None in code. | Stays fixed while transient I/O keeps the sample until expiry and a confirmed missing file still clears it. |
| M4 Grok weekly kind used substring match | `fixed-in-candidate` | `2aa1ddc`, `bd83a22`. `src/quota_grok.rs:308` accepts only `WEEKLY` and `BILLING_PERIOD_TYPE_WEEKLY`. Tests cover `NON_WEEKLY`, `BIWEEKLY`, and `WEEK`. `SECURITY.md` states the exact rule. | Another confirmed API name would need its own change. | Stays fixed while unknown kinds stay unavailable. |
| M5 full fingerprint cache cleared on an existing-key update | `fixed-in-candidate` | `ea41b3c` replaces an existing key in place. `2c162fb` evicts one entry in insertion (FIFO) order for a new key at capacity (`src/reader_cache.rs:35-45`). Tests: `full_cache_updates_existing_key_without_clearing_other_entries`, `capacity_boundaries_preserve_all_but_the_oldest_entry`, `fixed_trace_preserves_hot_entries_during_new_key_churn`. | The fixed 128-entry trace records 4,096 hits / 64 misses for FIFO against 4,032 / 128 for the old full clear. That is a deterministic trace only, not a runtime hit-rate or speed claim. | Stays fixed while an update never evicts and a new key evicts exactly one entry. |
| M6 `refresh_with_readers` mixes inventory, heading, observation, desired report, and send | `fixed-in-candidate` | `90c0630`, `e5555ec`, `6d357ad`: `refresh_cycle_with_readers` (`src/publish.rs:1438`) passes typed stages (`RefreshInventory`, `Headings` from `resolve_headings`, per-pane observations, send). Sequence, capacity-skip, and ambiguous-response behavior are kept by the existing boundary tests. | None for the split. Later wiring tasks add to these stages. | Stays fixed while the stages remain typed and the boundary tests pass. |
| M7 font journal invariants live in dynamic JSON | `fixed-in-candidate` | `885194b`: `load_journal` returns a typed `OwnershipJournal` (`src/font.rs:241`), planning (`JournalPlan`) is separate from `apply`, and `pending_presence_always_requires_manual_recovery` keeps the refusal. The no-rollback policy is unchanged. | The H1 and M1 residual races are unchanged by typing. | Stays fixed while malformed journals fail at load and pending state still refuses another action. |
| M8 socket reconnect test required an immediate EOF | `fixed-in-candidate` | `ea41b3c`: the test waits up to a 500 ms monotonic deadline for `check_connection` to observe EOF, then checks endpoint identity. | Re-run the default parallel suite on the final integration head before shipment. | Stays fixed while the test waits for EOF and still rejects a replaced socket path. |

## Conditional and specification findings

| ID | Status | Evidence at the snapshot | Remaining limit | Judgment condition |
| --- | --- | --- | --- | --- |
| C1 Codex `sessions` and `archived_sessions` share one 4,096-entry budget | `spec-waiting` | `src/model.rs` `DiscoveryKind::Codex` walks both roots under one budget. `894d2f2` keeps consumed budget during fallback scans. | Choose per-root or shared cap and the duplicate-ID rule before editing the limit. Survey item 19 stays partial. | Done only when the spec names the cap and a fixture over the cap behaves as specified. |
| C2 Agy statusLine observations stay bound to a pane across occupant replacement | `separate-task` | `docs/reviews/agy-occupant-generation.md` (`8af53c1`, `6f02a36`) records the host limits; `openspec/specs/provider-readers/spec.md` states the current constraint. No occupant generation is read. | The owner assigned host generation, conditional apply, and retirement on replacement to a separate Herdr host task. Current behavior is kept. Survey item 15 stays Open. | Never resolved by this follow-up. Done only when a fixture swaps occupants in one pane and B cannot observe A's sample after refresh and restart, with unknown generation failing closed. |
| C3 Grok transient HTTP failure after a credential switch kept the previous sample | `fixed-in-candidate` | `6551aa7`, `43ea14e`, `12f4584`: an observed `SourceStamp` change clears the sample and bumps the generation; a sample is accepted only for the same generation and source. | Contract chosen: immediate clear on an observed source change. | Stays fixed while a changed stamp cannot publish the previous account's sample. |
| C4 `doctor` statusLine count stopped at 1,024 entries but looked exact | `fixed-in-candidate` | `5fe4ad6` marks a capped scan (`in scanned entries`, `scan limit reached`). `3576412` treats a null `readdir` as EOF only when `errno` stays clear and prints `StatusLine records: scan incomplete` on a read or `fstat` error (`src/doctor.rs:41-90`). `0fe9f37` isolates the fd-release check. | None. | Stays fixed while a partial count is never printed as exact. |
| C5 release-gate GitHub API failures collapsed to one message | `fixed-in-candidate` | `380b770`: `scripts/check-release-gate.sh` separates rate limit, authentication, 404, 5xx, and connection failure without printing response bodies. Fixture: `scripts/test-check-release-gate.sh`. | None unless a new class is specified. | Stays fixed while messages stay free of response bodies and the fixture passes. |

## Rejected findings

| ID | Status | Evidence | Judgment condition |
| --- | --- | --- | --- |
| R1 `src/statusline.rs:236-243` `write_record` leaves a `.tmp` file | `rejected-wrong-basis` | No such function on `b5fc311`; `git grep write_record` on the snapshot still finds none in `src/statusline.rs`. The cited range is a read path. | Reopen only if a write path with that behavior appears. |
| R2 `git rev-parse` is replaced by a same-named user alias | `rejected-wrong-basis` | The review showed `git -c alias.rev-parse='!false' rev-parse --is-inside-work-tree` returns `true`: the builtin wins. This record does not repeat the command. Other Git configuration effects are not certified. | Reopen only with a Git that runs the alias before the builtin. |

## Latency findings

The default `refresh_interval_ms` stays 5,000 (`src/config.rs:25`).

| ID | Status | Evidence at the snapshot | Remaining limit | Judgment condition |
| --- | --- | --- | --- | --- |
| L1 P1 per-report `persist` rewrites and syncs the whole ledger | `fixed-in-candidate` | `32d5863` reserves an endpoint-wide sequence window (32 to 4,096 slots, `src/publish.rs:728-733`) and persists the durable upper bound once per window. `027bade` poisons the publisher after an uncertain checkpoint write until a new instance starts. `64ace03` adds lost-response, process-exit, clear-deadline, and pruning-failure fixtures. The verified rows are listed in `docs/reviews/sequence-reservation-design-2026-10-23.md`. | Kernel crash or power loss, a second allocator, a competing same-source writer, config change during refresh, rollback to an older binary, and macOS disks are not verified. A cold cycle writes once instead of `panes + 1` times; no elapsed-time speed-up is claimed. | Stays fixed while every verified failure row passes. The unverified rows stay design intent. |
| L2 P1 three sequential Git children per unique cwd, every cycle | `api-unwired` | `223de0a`, `8e8639e`, `d95e5b5`: `RepositoryCache::observe` in `src/ship_time.rs` caches only the worktree root and name (64 entries, 30 s TTL). `rev-parse` and `for-each-ref` run on every read, and a hit replaces `worktree list` with a native common-directory `rev-parse`, so a hit and a miss both run 3 children. `src/publish.rs:1557` still calls `read_repository_controlled`, a fresh cache per call. | Publication wiring, the fair queue, the cycle budget, and the 30 s confirmed fallback (`docs/reviews/git-cache-design.md` §9) are not in the snapshot. No process-count reduction and no latency improvement are claimed. | Runtime effect only after the wiring lands with the F-13 to F-18 fixtures. |
| L3 P1 no whole-refresh deadline or stage histogram in product behavior | `partial` | `4f7044a` records each refresh's monotonic duration with a fixed histogram and outcome counts; `doctor` prints them before any failure exit, and `last refresh ... ms after startup` stays uptime. `b79e84e`..`1787386` aggregate per-stage histograms (snapshot, Git, reader, persist, socket) in the daemon and `doctor`, rejecting inconsistent or out-of-range samples. `da1242c` adds absolute-deadline socket APIs (`Transport::connect_before`, `reconnect_same_endpoint_before`, `send_line_before`, `receive_line_before`) and refuses reuse after a failed or ambiguous exchange. | No whole-refresh deadline exists. `MetadataPublisher` does not yet supply cycle or stage samples, so the stage lines stay `unavailable` in production, and publication does not call the `_before` socket APIs. | A product deadline needs a written contract, the wiring, and tests that keep sequence durability and endpoint identity. |
| L4 P2 Codex and Kimi discovery repeats per pane | `api-unwired` | `f7abe35`, `988aea7`, `894d2f2`: `ReadCycle` (`src/model.rs:123`) shares validated discovery within one cycle and re-proves ancestors. Readers consume a supplied cycle (`a83d501`, `cefe544`), but publication does not pass one: `read` creates `ReadCycle::new()` for each pane. | Wiring one `ReadCycle` per refresh is not in the snapshot. Cap changes wait on C1. | Runtime effect only after wiring, with moved, duplicate, and invalid-file fixtures still failing closed. |
| L5 P2 a stuck provider read can hold the BUSY permit | `api-unwired` | `98a57de`: `bounded_read_observed` and `ReaderDiagnostics` count attempts, BUSY rejections, deadline and shutdown ends, and active workers (`docs/reviews/reader-worker-observations.md`). `a83d501` makes the Codex and Kimi readers choose the observed path when their `ReadCycle` carries diagnostics. No production caller supplies diagnostics, and control and `doctor` do not report them. | The bounded fixtures (about 30 ms for a 30 ms budget) are single debug-build observations of a blocked closure, not an OS-stalled read and not a live frequency. No cancellation or process isolation. | Measured frequency needs the wiring and a recorded run. Process isolation stays `conditional` on that measurement. |
| L6 P2 5 second refresh and 100 ms idle poll | `conditional` | Default 5,000 ms and idle polling are unchanged. | The review proposed a change only after an event-to-visible definition and measurement. None exists, so no change is owed. | Not done by lowering the default. A change needs a written freshness and load contract. |
| L7 P3 `statusline --then` and `configure` have no child timeout | `conditional` | `docs/reviews/statusline-configure-latency-2026-10-23.md` records synthetic Linux medians. Neither path has a timeout. | A timeout or direct exec needs a host deadline, recovery contract, and compatibility review first. | Not owed until that contract exists. |
| L8 P3 one socket connection per response | `conditional` | Each response still reconnects and checks path identity. `da1242c` forbids reuse after a failed or ambiguous exchange; it does not add connection reuse. | Reuse needs a syscall trace and an adversarial replacement fixture. `fstat` on a live fd must not replace the path check. | Not owed until those exist. Reuse without the replacement test is a failure. |

## Next latency checks

| Check | Status | Evidence | Judgment condition |
| --- | --- | --- | --- |
| N1 Stage timers for 1/8/32/128/256 on tmpfs and a real disk | `measurement-only` | `docs/reviews/refresh-stage-latency-2026-10-23.md`: `4afc9d4` baseline and a 2026-10-24 window run on Linux. Elapsed times were not taken under controlled conditions; only the checkpoint write count is a controlled comparison. | Repeat both binaries alternately on one host before any timing claim. |
| N2 Compare batch sequence reservation with the old code, including crash and lost responses | `fixed-in-candidate` for the verified rows | L1 fixtures. | Unverified rows stay open as listed in L1. |
| N3 Git fixtures for 0/1/8/32 cwd, changes, and slow Git | `measurement-only` | `docs/reviews/git-heading-latency-2026-10-23.md` (counts) and the cache API fixtures (invalidation). | Runtime comparison waits for L2 wiring. |
| N4 Orthogonal Codex and Kimi candidate and pane counts | `measurement-only` | Tmpfs matrix only (`dc637c3`). | Real disk after L4 wiring. |
| N5 Linux and macOS report-complete time and visible delay, without a live Herdr | `conditional` | No visible-delay definition and no macOS run. | L6 and the L3 deadline stay open until this exists. |

## Local Sol high evidence and the queue

Owner-authorized local GPT-6.1 Sol high reads are evidence only. None is a
queue run or acceptance by the automated ship gate.

- `reviews/sol-high-code-review-2026-10-23.md`: `d3ccbc17..12f4584` NO-GO on
  font wording, resolved by `12f4584..2dc1ef1` GO; `2dc1ef1..a6cddcc` GO for
  five records.
- `reviews/sol-high-record-review-2026-10-23.md`: `e10cfca..9f5d194` NO-GO,
  resolved by `9f5d194..0d801d0` GO.
- `reviews/refresh-stage-carry-forward-sol-high-2026-10-23.md`:
  `a6cddcc..819ede0` NO-GO, resolved by `a6cddcc..96829df` GO.
- `reviews/git-cache-code-sol61-high.md`: `da1242c..b00da5b` NO-GO,
  `b00da5b..f1c627e` NO-GO, `f1c627e..d7183b8` GO (integrated as
  `223de0a..d95e5b5`).
- Each other lane integrated after `96829df` has its own local record under
  `reviews/` (font journal types, publisher stages, reader cycle, Agy
  generation, doctor scan, model fixtures, single eviction, socket deadline,
  runtime metrics, sequence window, reader worker diagnostics). Their NO-GO
  and GO chains are collected by the commander, not restated here.

The commander closes the remaining gap with one GPT-6.1 Sol high review of the
full diff from `d3ccbc17d3f8344d466015fac580cd0fe3b60269` to the final head
fixed when that review starts.

## Verification of this record

This record changes documentation only. It was checked with
`check-encoding`, `git diff --check`, and a link check against the snapshot.
No Rust build or test suite was re-run for it.
