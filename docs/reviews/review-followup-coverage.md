# Review follow-up coverage (2026-10-23)

This record maps every adopted, conditional, and rejected finding from the
2026-10-23 whole-repository review and latency investigation onto local
candidate `96829df7e6fc51af327b448f5b8b3596f0a96d3f`. It does not accept,
integrate, or ship that candidate.

The review originals live outside the repository:

- `reviews/whole-repo-2026-10-23/consolidated.md`
- `reviews/latency-2026-10-23/consolidated.md`
- `reviews/implementation-queue-2026-10-23.md`

Those files are the primary evidence for what the reviews said. Line numbers
below were read from the candidate with `git show 96829df7e6fc51af327b448f5b8b3596f0a96d3f:path`.
Uncommitted work in other worktrees, including
`.claude/worktrees/refresh-measure-resume`, is not evidence that a finding is
done. `develop` and `origin/develop` are `d3ccbc17d3f8344d466015fac580cd0fe3b60269`,
which does not contain this candidate. Shipment stays on hold.

## Status classes

| Class | Meaning | Completion rule |
| --- | --- | --- |
| `fixed-in-candidate` | The reviewed behavior is present at the candidate and a regression check exists in tree. | Not shipped. Do not describe it as released or as independently reviewed for shipment. |
| `partial` | Part of the finding is in the candidate. The named remainder is still open. | Do not mark the finding closed. |
| `measurement-only` | An isolated fixture result is recorded. Product behavior is unchanged. | Do not treat the numbers as a budget or as a macOS or live Herdr result. |
| `design-only` | A contract is written and explicitly not implemented. | Do not implement from the prose alone, and do not mark the design as approved behavior. |
| `unimplemented` | The candidate still has the reviewed shape. | A later change needs its own regression evidence. |
| `spec-waiting` | The safe behavior is not decided. | Do not code a guess, and do not mark the row done. |
| `rejected-wrong-basis` | The original claim does not match the reviewed code. | Keep the rejection. Do not reopen it as an implementation task. |

## Whole-repository findings

Review target: `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`. Agreement denominator: 4.

| ID | Status | Evidence at the candidate | Remaining work | Planned owner | Doc update | Judgment condition |
| --- | --- | --- | --- | --- | --- | --- |
| H1 font rollback can restore pre-write bytes over a later external edit | `partial` | `f319f52` and `ead231d` stop automatic rollback. On failure, `src/font.rs:526-530` reports that no automatic rollback or deletion was attempted and names the journal for manual inspection. `CHANGELOG.md` Unreleased and survey item 16 say the same. | Check-then-rename and check-then-unlink races remain. `src/font.rs:486-491` stamps the font and then calls `remove_file`. `docs/reviews/font-atomicity-policy-2026-10-23.md` still says the conservative policy was not implemented; that sentence is stale against this candidate and must be corrected in a later docs commit. | No current lane. Do not reopen destructive recovery unless the owner asks. | `CHANGELOG.md` already describes the no-rollback policy. Later: correct the stale policy doc under `docs/reviews/`. | Closed only if a test shows an external edit made after a successful owned write survives a later failure, and the doc no longer claims an atomic rename. The residual race stays open until an owner-approved design says otherwise. |
| M1 uninstall can delete a font replaced after the content check | `partial` | `75865cd` rechecks identity immediately before removal: `src/font.rs:485-491` calls `before_font_remove`, compares `stamp` with `original_font`, and conflicts on mismatch. | A replacement after that stamp and before `remove_file` can still be deleted. The policy doc records this gap. | No current lane. | Same stale policy doc as H1. Survey item 16 already states the race. | Not closed while removal is a separate pathname operation. A future fix needs an injected replacement after the recheck hook. |
| M2 Kimi `openat` lacked `O_CLOEXEC` | `fixed-in-candidate` | `2aa1ddc`. `src/reader_kimi.rs:203-204` ORs `O_CLOEXEC` into both directory and file `openat` calls. | None in product code. Historical independent review of the integration slice was not established at closeout. | None. | `CHANGELOG.md` Unreleased does not mention this fix. Add one bullet when the integration candidate is documented. | Stays fixed only while both `openat` sites keep `O_CLOEXEC` and a descriptor-flag regression test still passes. This record did not re-run the suite. |
| M3 Kimi transient credential stat cleared the weekly sample | `fixed-in-candidate` | `800cef5`. `src/quota_kimi.rs:280-283` and `:347-361` return `SourceState::Transient` for non-`NotFound` stat errors. `src/publish.rs:317` does not treat credential-less transient state as a stamp change. Regression: `source_state_distinguishes_present_missing_symlink_and_transient_permission_error`. | None in product code. Same review-gap note as M2. | None. | Add a CHANGELOG bullet with the integration docs. | Stays fixed only while transient I/O keeps the previous sample until expiry and a confirmed missing file can still clear it. |
| M4 Grok weekly kind used substring match | `fixed-in-candidate` | `2aa1ddc` and `bd83a22`. `src/quota_grok.rs:306-308` accepts only `WEEKLY` and `BILLING_PERIOD_TYPE_WEEKLY`. Tests cover `NON_WEEKLY`, `BIWEEKLY`, and `WEEK`. | None unless a real API adds another confirmed weekly name. | None. | CHANGELOG bullet plus `SECURITY.md` if the shipped text still describes `contains`. | Stays fixed only while unknown kinds stay unavailable. Do not widen the match without a confirmed API name. |
| M5 full fingerprint cache cleared on an existing-key update | `partial` | `ea41b3c`. `src/reader_cache.rs:26-31` clears only when the key is new and `len >= limit`. `full_cache_updates_existing_key_without_clearing_other_entries` covers the reviewed case. | A new key at capacity still clears every entry (`src/reader_cache.rs:88-92`). The review asked for a measured eviction choice before keeping that full clear. No hit-rate measurement is recorded. | Commander assigns after measurement. Not part of the current four lanes. | CHANGELOG should mention the existing-key fix and say the new-key full clear is still unmeasured. | The reviewed bug is fixed. The finding is closed only after a recorded measurement either keeps the full clear or replaces it, with a test at the limit. |
| M6 `refresh_with_readers` mixes inventory, heading, observation, desired report, and send | `unimplemented` | `src/publish.rs:1264-1302` still enters `refresh_with_readers_inner`. Per-report `persist` remains at `src/publish.rs:1191-1193`. | Split into explicit snapshot, heading, observation, desired-report, and send stages without changing sequence reservation, capacity skip, partial reader failure, in-flight generation, or ambiguous-response stop. | `lane_sequence` assignment `asg_publisher_stages`, in progress on its own worktree. This coverage commit does not count that work. | Stage names belong in the publisher spec only after the split lands. | Done only when the stage types are in the reviewed diff and the existing boundary tests pass. A file split that preserves one mixed procedure is not enough. |
| M7 font journal invariants live in dynamic JSON | `unimplemented` | `src/font.rs:200` still requires `font_owned` to be a JSON boolean and returns `serde_json::Value`. Pending refusal is `src/font.rs:317`. | Type the journal and entries at load. Keep version 1, unknown fields, pending refusal, original bytes, and manual recovery. Do not change the no-automatic-rollback policy. | `lane_font` assignment `asg_font_journal_types`, in progress on its own worktree. | Survey item 16 stays as written until the typed load boundary exists. | Done only when malformed journals fail at load, version and unknown fields survive, and pending state still refuses another action. |
| M8 socket reconnect test required an immediate EOF | `fixed-in-candidate` | `ea41b3c`. `src/socket.rs:376-386` waits until a 500 ms monotonic deadline for `check_connection` to observe peer EOF, then checks endpoint identity. | Re-run the default parallel test on the final integration head before shipment. This record did not re-run it. | None for the code change. | CHANGELOG bullet with the integration docs. | Stays fixed only while the test waits for EOF and still rejects a replaced socket path. |

## Conditional findings

| ID | Status | Evidence at the candidate | Remaining work | Planned owner | Doc update | Judgment condition |
| --- | --- | --- | --- | --- | --- | --- |
| C1 Codex `sessions` and `archived_sessions` share one 4,096 entry budget | `spec-waiting` | `src/model.rs:46` `MAX_DIRECTORY_ENTRIES = 4096`. `src/reader_codex.rs:80` spends that budget on `archived_sessions` as well. The shared cap is still intentional until specified. | Choose per-root versus shared cap, and the duplicate-id rule, before editing the limit. Add 4,096+1 and duplicate-id tests only for the chosen rule. | Commander, after `lane_git_reader` finishes the separate Git contract. Reader discovery measurement is `measurement-only` and does not decide the cap. | `docs/reviews/2026-10-18-survey.md` item 18 stays partial. TODO: record the chosen cap. Do not mark item 18 implemented. | Done only when the spec names the cap and a fixture exceeds it without dropping an already accepted session unless the spec says to drop it. |
| C2 Agy statusLine observations stay bound to a pane id across occupant replacement | `spec-waiting` | Survey item 15 is still Open. No occupant generation is read. | Confirm which occupant generation Herdr exposes. Design the binding before changing `src/reader_agy.rs`. | Commander. No current lane. | Survey item 15 stays Open. | Done only when a fixture swaps occupants in one pane and B cannot observe A's sample after refresh and restart. Unknown generation must fail closed. |
| C3 Grok transient HTTP failure after a credential switch kept the previous sample | `fixed-in-candidate` | `6551aa7`, clarified by `43ea14e` and `12f4584`. `src/publish.rs:197-203` clears the sample when the observed `SourceStamp` changes and bumps `generation`. `publish` accepts a new sample only for the same generation and source (`src/publish.rs:213-216`). | None in product code. The chosen contract is immediate clear on observed source change, not "wait for process restart". Historical review of this slice was not established. | None. | CHANGELOG does not mention the clear. Add a bullet. Do not reopen the restart-versus-generation question unless the owner rejects this contract. | Stays fixed only while a changed stamp cannot publish the previous account's sample. A later successful fetch for the new stamp may publish. |
| C4 `doctor` statusLine count stops at 1,024 entries but looked exact | `partial` | `5fe4ad6`. `src/doctor.rs:47-51` says `in scanned entries` and `scan limit reached` when the scan stops early. | `readdir` still treats a null pointer as the end (`src/doctor.rs:95-99`) and does not distinguish an error from a clean end. | Commander, low priority, after the current wave. | README already describes the scan cap. CHANGELOG should mention the capped doctor line. | The display finding is fixed. The row is closed only if error-versus-end is either implemented or explicitly left out of scope by the commander. |
| C5 release-gate GitHub API failures collapsed to one message | `fixed-in-candidate` | `380b770`. `scripts/check-release-gate.sh:33-49` separates rate limit, authentication, 404, 5xx, and connection failure. Unclassified failures still say `cannot read CI workflow runs`. Shell fixture: `scripts/test-check-release-gate.sh`. | None unless a new failure class is specified. Do not print response bodies. | None. | CHANGELOG bullet for the classifier. | Stays fixed only while the classified messages stay free of response bodies and the shell fixture passes. This record did not re-run it. |

## Rejected findings

| ID | Status | Evidence | Remaining work | Planned owner | Doc update | Judgment condition |
| --- | --- | --- | --- | --- | --- | --- |
| R1 `src/statusline.rs:236-243` `write_record` leaves a `.tmp` file | `rejected-wrong-basis` | The review found no such function on `b5fc311`. `git grep write_record 96829df7e6fc51af327b448f5b8b3596f0a96d3f -- src/statusline.rs` still finds no match. The cited range is a read path. | None. | None. | Do not add a TODO. | Reopen only if a write path with that temporary-file behavior exists. Absence on this candidate keeps the rejection. |
| R2 `git rev-parse` is replaced by a same-named user alias | `rejected-wrong-basis` | The review ran `git -c alias.rev-parse='!false' rev-parse --is-inside-work-tree` on the reviewed worktree and got `true`. Builtin precedence was the reason for rejection. This record does not repeat that command. Other Git config effects were not certified then and are still not certified. | None for this alias claim. Real invalidation conditions are latency row L2, not this rejection. | None. | Do not add a TODO for alias overriding `rev-parse`. | Reopen only with a command that shows this Git replacing builtin `rev-parse` before the builtin. |

## Latency findings

Latency target: the same `b5fc311` candidate. Default `refresh_interval_ms` remains 5,000 (`src/config.rs:25`). Do not change it in these follow-ups.

| ID | Status | Evidence at the candidate | Remaining work | Planned owner | Doc update | Judgment condition |
| --- | --- | --- | --- | --- | --- | --- |
| L1 P1 per-report `persist` rewrites and syncs the whole ledger | `design-only` | `src/publish.rs:1054-1089` still clones, writes, and `sync_all`s the file and directory. `src/publish.rs:1191-1193` still persists before each report. `docs/reviews/sequence-reservation-design-2026-10-23.md` says to keep that behavior. `a01cc92` is design, not code. | Implement one endpoint reservation window only after isolated fixtures cover crash, partial send, ambiguous response, restart, endpoint change, resource reuse, pruning, and monotonicity. | Commander assigns after `lane_sequence` lands the stage split without changing this behavior. | Survey item 17 stays Open. The design doc stays the contract until a new reviewed doc replaces it. | Not done while `persist` runs once per report. A benchmark without the failure table is not acceptance. |
| L2 P1 three sequential Git children per unique cwd, every cycle | `measurement-only` | `src/ship_time.rs:62`, `:72`, and `:81` still run `rev-parse`, `worktree list`, and `for-each-ref`. `docs/reviews/git-heading-latency-2026-10-23.md` records the 0/1/8/32 isolated Linux fixture. No cross-cycle cache exists. HEAD-mtime caching is not acceptable. | Write the full invalidation contract, then implement the chosen low-risk design. Keep the 1 second child budget, kill, and reap. | `lane_git_reader` assignment `asg_git_cache_design` is the design task. Implementation waits for a commander decision on that contract. | The Git latency doc is the measurement record. A new `docs/reviews/git-cache-design.md` is that lane's touching path, not this commit. | Design is done only when every invalidation listed in that assignment is accepted or explicitly unknown. Code is done only when fixtures show a cache miss after ref, config, and worktree changes. |
| L3 P1 no whole-refresh deadline or stage histogram in product behavior | `measurement-only` | `src/daemon.rs:387` arms the next interval only after refresh returns. `last_refresh_ms` is uptime (`src/daemon.rs:90-109`). Test-only stage timers exist (`src/publish.rs:1210-1216`) and `docs/reviews/refresh-stage-latency-2026-10-23.md` records Linux fake-socket runs from `4afc9d4`, not a remeasure of this candidate. | Remeasure 1/8/32/128/256 on this candidate, tmpfs and a real disk, before choosing a deadline. Preserve sequence durability and endpoint identity. | Commander assigns after L1's design gate. | Refresh doc stays a record of `4afc9d4`. Do not copy its medians into a budget. | A product deadline is unimplemented until the contract and tests exist. The carried numbers are not that contract. |
| L4 P2 Codex and Kimi discovery repeats per pane | `measurement-only` | Discovery still walks candidates per pane. `docs/reviews/reader-discovery-latency-2026-10-23.md` is a tmpfs fixture (`dc637c3`). Fail-closed duplicate and symlink behavior stayed in place. | Separate pane count from candidate count on a real disk. Do not skip the full walk by trusting the previous path. Cap changes wait on C1. | Commander after C1 is specified. | Reader doc stays the tmpfs record. | Sharing or an index is unimplemented until moved, duplicate, and invalid-file fixtures still fail closed. |
| L5 P2 a stuck provider read can hold the BUSY permit | `unimplemented` | `src/model.rs` still holds the provider permit until the worker returns. No new stuck-worker counter is in this candidate. | Observe wait, BUSY, timeout, and stuck-worker frequency in an isolated fixture before any process isolation. | Commander. No current lane. | Add a measurement note under `docs/reviews/` only after a fixture exists. | Isolation stays unimplemented. The row is not done by a comment. |
| L6 P2 5 second refresh and 100 ms idle poll | `spec-waiting` | Default remains 5,000 ms. Idle polling remains. `9aa1bfc` measured statusline and configure, not this cycle's visible delay. | Define event-to-visible on an isolated fixture before debounce or a default change. | Commander. No current lane. | README keeps 5 seconds. | Not done by lowering the default. A change needs a written freshness and load contract. |
| L7 P3 `statusline --then` and `configure` have no child timeout | `measurement-only` | `docs/reviews/statusline-configure-latency-2026-10-23.md` (`9aa1bfc`) records synthetic Linux medians: statusline 0.981 ms, `--then` 16.609 ms, configure change 31.388 ms. Neither path grew a timeout. | Decide the host deadline and failure recovery before adding a timeout. Direct exec of `--then` is out of scope until compatibility is specified. | Commander. No current lane. | The measurement doc is the record. Do not put these medians in user docs as service latency. | A timeout is unimplemented. The row stays open until the contract and tests exist. |
| L8 P3 one socket connection per response | `spec-waiting` | `src/socket.rs` still reconnects and checks path identity. The replacement test still rejects a substituted socket. Per-response cost is unmeasured. | Syscall trace and an adversarial replacement fixture come before any connection reuse. `fstat` on a live fd must not replace the path check. | Commander. No current lane. | None until a contract exists. | Not done while each response connects again. Reuse without the replacement test is a failure. |

## Next latency checks

The investigation listed five checks. Their state at this candidate:

| Check | Status | Evidence | Judgment condition |
| --- | --- | --- | --- |
| N1 Stage timers for 1/8/32/128/256 on tmpfs and a real disk | `measurement-only` | Recorded from `4afc9d4` and carried forward. Not remeasured on `96829df`. macOS and live Herdr were not measured. n=3 at 256 panes. | Repeat on the current integration head before the numbers justify a budget. |
| N2 Compare a batch sequence reservation with the current code, including crash and lost responses | `design-only` | The failure table is in the sequence design doc. No reservation implementation and no comparison run exist. | L1's judgment condition. |
| N3 Git fixtures for 0/1/8/32 cwd, same and different repos, before and after changes, and a slow Git | `measurement-only` | The Git heading doc covers the count matrix on Linux. Slow-Git and invalidation fixtures for a cache are not done. | L2's judgment condition. |
| N4 Orthogonal Codex and Kimi candidate counts and pane counts, including fail-closed cases | `measurement-only` | Tmpfs matrix only. | L4's judgment condition. |
| N5 Linux and macOS isolated report-complete time and visible delay, without a live Herdr | `unimplemented` | Linux synthetic pieces exist. No macOS run and no visible-delay definition. | L3 and L6 stay open until this fixture exists. |

## Already assigned work

Formation `fm-lineup-reviews-resume-20261023` has four implementation lanes. Their
uncommitted or later commits are outside this coverage snapshot:

| Lane | Assignment | Rows | Base |
| --- | --- | --- | --- |
| `lane_font` | `asg_font_journal_types` | M7, while keeping H1 and M1 residuals | `96829df7e6fc51af327b448f5b8b3596f0a96d3f` |
| `lane_sequence` | `asg_publisher_stages` | M6, without changing L1 | same |
| `lane_git_reader` | `asg_git_cache_design` | L2 design first | same |
| `lane_inventory` | `asg_review_coverage` | this record | same |

## Document drift to correct later

Do not mark these corrections as done in this commit. They are the ordered
documentation follow-up in `docs/plans/review-followups-plan.md`.

- `CHANGELOG.md` Unreleased omits M2, M3, M4, the M5 existing-key fix, M8, C3, the C4 display fix, and C5.
- `docs/reviews/font-atomicity-policy-2026-10-23.md` says the no-rollback policy was not implemented. `src/font.rs:526-530` and the changelog contradict that sentence.
- Survey item 15 stays Open. Item 16 matches the partial font fix. Item 17 stays Open.
- The external implementation queue says `develop` was one commit behind `d3ccbc17`. At this inspection, `develop` is `d3ccbc17d3f8344d466015fac580cd0fe3b60269`. The unshipped line is this candidate, not a stale `develop` branch.
- There is no repository `TODO` file at this candidate. Follow-ups live in this record and the plan until a real TODO file is introduced on purpose.
