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

- Implementation source snapshot: `0220da64f8545f89b6daca211553f486b271057f`.
  It contains integration base `9d2b4a23eca9d0e46839d54a0a84d9920c749c7f`
  and the independent comment-only alignment commit. Later documentation
  commits do not stand in for this source SHA. Evidence below uses symbols
  and fixture names at this snapshot rather than historical line numbers.
- Both original reviews targeted `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`.
  Older snapshots (`96829df`, `d95e5b5`, `1787386`) are historical foundations,
  not the current implementation status.
- The candidate is local and unshipped; it is not acceptance, integration into
  `develop`, a push, or a live observation. All release versions stay 0.2.2.
- P runtime wiring, D control/doctor wiring, and all five local reader worker
  producers are connected. Canonical contracts are in `openspec/specs/`;
  the synchronized delta is archived at
  `openspec/changes/archive/2026-10-24-wire-publisher-runtime/`.
  Implementation details are in `docs/plans/publisher-runtime-wiring.md` and
  `docs/plans/reader-diagnostics-control.md`.

## Status classes

| Class | Meaning | Completion rule |
| --- | --- | --- |
| `fixed-in-candidate` | The reviewed behavior is present at the snapshot and a regression check exists in tree. | Not shipped. Do not describe it as released or as accepted by the ship gate. |
| `partial` | Part of the finding is in the snapshot. The named remainder is still open. | Do not mark the finding closed. |
| `measurement-only` | An isolated fixture result is recorded. Product behavior is unchanged. | Do not treat the numbers as a budget, a speed-up, or a macOS or live Herdr result. |
| `conditional` | The review proposed it only under a condition that is not met. | Not an implementation obligation. Keep the reason and the open condition. |
| `separate-task` | Owner-assigned to a different task outside this follow-up. | Keep current behavior and the open item. Never mark it resolved here. |
| `rejected-wrong-basis` | The original claim does not match the reviewed code. | Keep the rejection. Do not reopen it as an implementation task. |

## Whole-repository findings

Agreement denominator: 4.

| ID | Status | Evidence at the snapshot | Remaining limit | Judgment condition |
| --- | --- | --- | --- | --- |
| H1 font rollback can restore pre-write bytes over a later external edit | `partial` | `f319f52`, `ead231d`: automatic rollback is gone. `apply_with_font_and_remove` in `src/font.rs` keep partial files and report that no automatic rollback or deletion was attempted, naming the journal for manual recovery. Planned original bytes are compared before each write. | Each check stays separate from the following pathname `rename`, so an external edit between them can still be replaced. Recovery after a failure is manual, never automatic. | Not closed while a check and its `rename` are separate operations. A closing design needs owner approval. |
| M1 uninstall can delete a font replaced after the content check | `partial` | `75865cd`: `apply_with_font_and_remove` calls `before_font_remove`, compares `stamp` with the planned original, conflicts on mismatch, and only then calls `remove_file`. | A replacement after that stamp and before `remove_file` can still be deleted. | Same as H1: not closed while removal is a separate pathname operation. |
| M2 Kimi `openat` lacked `O_CLOEXEC` | `fixed-in-candidate` | `2aa1ddc`. the `openat` helper in `src/reader_kimi.rs` ORs `O_CLOEXEC` into the shared `openat` helper used for the directory and the file. | None in code. | Stays fixed while every `openat` in that reader keeps `O_CLOEXEC`. |
| M3 Kimi transient credential stat cleared the weekly sample | `fixed-in-candidate` | `800cef5`. `src/quota_kimi.rs` returns `SourceState::Transient` for non-`NotFound` stat errors; publication does not treat that as a stamp change. Regression: `source_state_distinguishes_present_missing_symlink_and_transient_permission_error`. | None in code. | Stays fixed while transient I/O keeps the sample until expiry and a confirmed missing file still clears it. |
| M4 Grok weekly kind used substring match | `fixed-in-candidate` | `2aa1ddc`, `bd83a22`. `src/quota_grok.rs` accepts only `WEEKLY` and `BILLING_PERIOD_TYPE_WEEKLY`. Tests cover `NON_WEEKLY`, `BIWEEKLY`, and `WEEK`. `SECURITY.md` states the exact rule. | Another confirmed API name would need its own change. | Stays fixed while unknown kinds stay unavailable. |
| M5 full fingerprint cache cleared on an existing-key update | `fixed-in-candidate` | `ea41b3c` replaces an existing key in place. `2c162fb` evicts one entry in insertion (FIFO) order for a new key at capacity (`src/reader_cache.rs`). Tests: `full_cache_updates_existing_key_without_clearing_other_entries`, `capacity_boundaries_preserve_all_but_the_oldest_entry`, `fixed_trace_preserves_hot_entries_during_new_key_churn`. | The fixed 128-entry trace records 4,096 hits / 64 misses for FIFO against 4,032 / 128 for the old full clear. That is a deterministic trace only, not a runtime hit-rate or speed claim. | Stays fixed while an update never evicts and a new key evicts exactly one entry. |
| M6 `refresh_with_readers` mixes inventory, heading, observation, desired report, and send | `fixed-in-candidate` | `90c0630`, `e5555ec`, `6d357ad`: the production refresh pipeline (`src/publish.rs`) passes typed stages (`RefreshInventory`, `Headings` from `resolve_headings`, per-pane observations, send). Sequence, capacity-skip, and ambiguous-response behavior are kept by the existing boundary tests. | The production path now uses these stages; the test-only cycle wrapper is not the runtime entry point. | Stays fixed while the stages remain typed and the boundary tests pass. |
| M7 font journal invariants live in dynamic JSON | `fixed-in-candidate` | `885194b`: `load_journal` returns a typed `OwnershipJournal` (`src/font.rs`), planning (`JournalPlan`) is separate from `apply`, and `pending_presence_always_requires_manual_recovery` keeps the refusal. The no-rollback policy is unchanged. | The H1 and M1 residual races are unchanged by typing. | Stays fixed while malformed journals fail at load and pending state still refuses another action. |
| M8 socket reconnect test required an immediate EOF | `fixed-in-candidate` | `ea41b3c`: the test waits up to a 500 ms monotonic deadline for `check_connection` to observe EOF, then checks endpoint identity. | The final candidate must pass the default parallel suite; historical serial runs alone do not satisfy this. | Stays fixed while the test waits for EOF and still rejects a replaced socket path. |

## Conditional and specification findings

| ID | Status | Evidence at the snapshot | Remaining limit | Judgment condition |
| --- | --- | --- | --- | --- |
| C1 Codex `sessions` and `archived_sessions` share one 4,096-entry budget | `fixed-in-candidate` | `ReadCycle::files` in `src/model.rs` keeps the combined 4,096-entry cap and charges fallback to the same cumulative budget and original deadline. Fixtures: `cycle_discovery_counts_both_roots_and_rejects_symlinks_and_expiry`, `cycle_discovery_fallback_keeps_consumed_entries_and_original_deadline`; reader fixtures preserve duplicate-ID refusal. The canonical provider spec states this contract. | This preserves the existing resource bound; no separate per-root allowance or performance gain is claimed. | Exactly 4,096 combined entries succeed; 4,097 fails closed; fallback never resets the budget. |
| C2 Agy statusLine observations stay bound to a pane across occupant replacement | `separate-task` | `docs/reviews/agy-occupant-generation.md` (`8af53c1`, `6f02a36`) records the host limits; `openspec/specs/provider-readers/spec.md` states the current constraint. No occupant generation is read. | The owner assigned host generation, conditional apply, and retirement on replacement to a separate Herdr host task. Current behavior is kept. Survey item 15 stays Open. | Never resolved by this follow-up. Done only when a fixture swaps occupants in one pane and B cannot observe A's sample after refresh and restart, with unknown generation failing closed. |
| C3 Grok transient HTTP failure after a credential switch kept the previous sample | `fixed-in-candidate` | `6551aa7`, `43ea14e`, `12f4584`: an observed `SourceStamp` change clears the sample and bumps the generation; a sample is accepted only for the same generation and source. | Contract chosen: immediate clear on an observed source change. | Stays fixed while a changed stamp cannot publish the previous account's sample. |
| C4 `doctor` statusLine count stopped at 1,024 entries but looked exact | `fixed-in-candidate` | `5fe4ad6` marks a capped scan (`in scanned entries`, `scan limit reached`). `3576412` treats a null `readdir` as EOF only when `errno` stays clear and prints `StatusLine records: scan incomplete` on a read or `fstat` error (`src/doctor.rs`). `0fe9f37` isolates the fd-release check. | None. | Stays fixed while a partial count is never printed as exact. |
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
| L2 P1 three sequential Git children per unique cwd, every cycle | `fixed-in-candidate` (wiring) | `MetadataPublisher::read_heading` owns a persistent `RepositoryCache`; `resolve_headings_in_budget` shares same-cwd results within a cycle and rotates attempted workspace IDs fairly under a 1 s cycle Git budget. `heading_repository` expires fallback 30 s after original verification. Fixtures: `git_queue_rotates_a_stalled_repository_and_unread_headings_are_null`, `shared_stalled_cwd_is_read_once_and_rotates_only_its_starter`, `heading_fallback_expires_thirty_seconds_after_the_original_verification`. See `docs/reviews/git-cache-design.md` §9. | A hit and miss both run three children. Only worktree root/name is cached; refs and native safety checks still run. No speed improvement has been measured. | Keep refusal/invalidation, nonrenewing confirmation age, finite fallback, kill/reap and fair-attempt fixtures green. |
| L3 P1 no whole-refresh deadline or stage histogram in product behavior | `fixed-in-candidate` (cooperative contract) | Production `MetadataPublisher::refresh_stages` supplies snapshot/Git/reader/persist/socket samples and report/checkpoint counts; daemon and doctor expose total and stage histograms. Refresh uses one envelope: 2 s snapshot + 1 s Git + reader timeout + 2 s metadata (7 s default). Socket `_before` APIs share it. A required frame with less than 2 s remaining is deferred without writing; an admitted ambiguous exchange burns its seq and ends the cycle. Fixtures: `production_stages_count_reports_and_checkpoints_and_survive_errors`, `refused_admission_after_reader_keeps_rotation_and_upper_bound`, `poison_is_never_reported_as_a_deadline_skip`, daemon stage/wire fixtures. | Cooperative checks cannot interrupt synchronous filesystem work, fsync, kill/reap or scheduling delay. Degraded deferral can exceed TTL; expired keys disappear. Zero deadline skips means measured zero only when the optional diagnostic field is valid; absent/malformed/legacy means unavailable. This is not an SLO or a strict wall-clock guarantee. | Preserve durability, endpoint identity, separate capacity/deadline/error outcomes, and the 5 s default. |
| L4 P2 Codex and Kimi discovery repeats per pane | `fixed-in-candidate` | `refresh_with_readers_inner` passes one diagnostics-bearing `ReadCycle` to all panes per refresh. Readers re-prove ancestors, uniqueness and file validity. Fixtures: `one_reader_cycle_per_refresh_shares_discovery_and_diagnostics`, each actual reader `diagnostic_routing_isolated`, and model discovery mutation/fallback tests. | A new refresh starts with a new cycle; cache instability may require another bounded scan. Historical tmpfs timing is not a measurement of this wiring. | Keep moved, duplicate, symlink, invalid-file, and C1 cumulative-budget refusal cases. |
| L5 P2 a stuck provider read can hold the BUSY permit | `partial` (observation implemented) | All five workers (Claude, Agy, Codex, Grok, Kimi) choose `bounded_read_observed` when diagnostics are present. Publisher owns the Arc; control samples it independently of completed refreshes and doctor displays fixed provider labels, units and unavailable states. Evidence: all five reader `diagnostic_routing_isolated` fixtures; `reader_control_live_worker_release_is_visible_without_another_refresh`, `reader_handle_survives_actual_daemon_completed_state_publication`, `reader_wire_skew_does_not_erase_health_or_valid_siblings`, and `reader_doctor_uses_fixed_labels_units_and_retained_deadline_meaning`. | Hermes has no local worker: its empty slot/zero counters are not read success or a measurement. Snapshots are best-effort and nontransactional; `completed` counts accepted results including missing usage. Stuck OS read frequency, cancellation and isolation are unmeasured/unimplemented. | Observation wiring is done. Isolation remains conditional on a measured stuck-worker frequency; no strict OS-I/O deadline is claimed. |
| L6 P2 5 second refresh and 100 ms idle poll | `conditional` | Default 5,000 ms and idle polling are unchanged. | The review proposed a change only after an event-to-visible definition and measurement. None exists, so no change is owed. | Not done by lowering the default. A change needs a written freshness and load contract. |
| L7 P3 `statusline --then` and `configure` have no child timeout | `conditional` | `docs/reviews/statusline-configure-latency-2026-10-23.md` records synthetic Linux medians. Neither path has a timeout. | A timeout or direct exec needs a host deadline, recovery contract, and compatibility review first. | Not owed until that contract exists. |
| L8 P3 one socket connection per response | `conditional` | Each response still reconnects and checks path identity. `da1242c` forbids reuse after a failed or ambiguous exchange; it does not add connection reuse. | Reuse needs a syscall trace and an adversarial replacement fixture. `fstat` on a live fd must not replace the path check. | Not owed until those exist. Reuse without the replacement test is a failure. |

## Next latency checks

| Check | Status | Evidence | Judgment condition |
| --- | --- | --- | --- |
| N1 Stage timers for 1/8/32/128/256 on tmpfs and a real disk | `measurement-only` (observation done) | `docs/reviews/refresh-stage-latency-2026-10-23.md` separates historical runs from the wired `9d2b4a23eca9d0e46839d54a0a84d9920c749c7f` synthetic debug run: n=3 per size/filesystem, 30 rows, reports=panes+1 and one checkpoint row. Initial ext4 overlong socket root failed before measurement; only that case was rerun at a shorter root with the same executable. | Observations are complete. Controlled alternating old/new timing, macOS, live Herdr and event-to-visible remain unmeasured; no speed-up/SLO is established. |
| N2 Compare batch sequence reservation with the old code, including crash and lost responses | `fixed-in-candidate` for the verified rows | L1 fixtures. | Unverified rows stay open as listed in L1. |
| N3 Git fixtures for 0/1/8/32 cwd, changes, and slow Git | `measurement-only` | `docs/reviews/git-heading-latency-2026-10-23.md` records historical counts. Current source has cache/invalidation, fair queue, slow-read and fallback fixtures (L2). | A wired-head latency and child-count matrix remains open; no reduction in children or speed claim follows from cache wiring. |
| N4 Orthogonal Codex and Kimi candidate and pane counts | `measurement-only` | Tmpfs matrix only (`dc637c3`). | Real disk after L4 wiring. |
| N5 Linux and macOS report-complete time and visible delay, without a live Herdr | `conditional` | No event-to-visible definition or macOS run. Synthetic report completion excludes Herdr rendering. | L6 needs that evidence. The implemented L3 cooperative budget does not establish visible latency. |

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

The source delta after the integration base is comment-only in two Rust files
and the release workflow. The final candidate still requires the mandatory
Rust checks, Actionlint, strict canonical spec validation, encoding, whitespace,
local links and scope checks. The commander's verification receipt records the
actual final head and results; pending checks are not passes. Inclusive review,
ACK, integration, push and shipment remain separate gates.
