# Review follow-ups implementation plan

**Goal:** Finish the accepted 2026-10-23 review findings that are not yet in
the local candidate, without marking partial, measured, designed, or
unspecified rows as complete.

**Architecture:** See `docs/plans/review-followups-design.md`.

**Coverage:** `docs/reviews/review-followup-coverage.md` is the status table.
Check a task off only when that table's judgment condition is true for the
task's commit.

**Tech stack:** Rust 1.99.1 crate, locked dependencies.

## Global constraints

Repository files and commit subjects are English. Conversation with the
commander is Japanese. Support Linux and macOS. Keep source and endpoint
identity. Do not touch a live Herdr session, user settings, secrets, or
network APIs. Do not push, ACK, or ship from a member lane. Keep
`refresh_interval_ms` at 5000. Independent review is GPT-6.1 Sol high,
read-only, with native subagent fork none. Members do not review each other.
Questions go to the commander through a Formation report. A path outside the
assignment is a `BLOCKED` touching request before any edit. Do not `cargo clean`
by hand and do not recursively delete. Report a build that is likely to exceed
1 GiB before starting it.

## Current wave

These assignments already exist on Formation
`fm-lineup-reviews-resume-20261023`. Do not duplicate them.

- [ ] `lane_font` / `asg_font_journal_types`: M7 typed journal load. Leave H1
  and M1 residuals as `partial`.
- [ ] `lane_sequence` / `asg_publisher_stages`: M6 stage split. Leave L1
  per-report persist unchanged.
- [ ] `lane_git_reader` / `asg_git_cache_design`: L2 invalidation contract and
  isolated fixtures. Stop for a commander decision before implementing a cache.
- [ ] `lane_inventory` / `asg_review_coverage`: this coverage record, this
  design, and this plan.

## Ordered work after the current wave

1. [ ] Integrate the current wave only after each lane has a clean commit and
   the commander accepts the batch. Do not push that integration.
2. [ ] L1 sequence reservation. Implement only after the existing failure table
   is demonstrated on an isolated socket fixture. Until then the status stays
   `design-only`.
3. [ ] L2 Git cache implementation, only after the commander accepts the
   invalidation contract. HEAD-mtime caching is not an acceptable shortcut.
4. [ ] C1 budget specification. No cap change before that decision.
5. [ ] L4 reader discovery sharing, only with fail-closed move, duplicate, and
   invalid-file fixtures, and only within the C1 cap.
6. [ ] L5 stuck-worker measurement. Process isolation stays unimplemented.
7. [ ] N1 and N5 remeasure on the integration head, including one macOS
   isolated run if a Mac is available. No live Herdr.
8. [ ] L3 and L6 deadline or event-driven refresh, only after those
   measurements and a written freshness contract. The 5 second default stays
   until that contract exists.
9. [ ] L7 statusline and configure timeouts, only after the host deadline and
   recovery contract exist.
10. [ ] L8 socket reuse, only after an adversarial replacement fixture. Path
    identity stays.
11. [ ] M5 new-key eviction, only after a recorded hit-rate measurement. The
    existing-key fix stays.
12. [ ] C4 `readdir` error versus end, or an explicit commander waiver.
13. [ ] C2 occupant generation, only after Herdr's generation source is known.
    Unknown generation fails closed.
14. [ ] Documentation, TODO, changelog, and remnant audit, in that phase and
    not before the behavior commits they describe.

H1 and M1 stay `partial` across this list. R1 and R2 stay rejected.

## Documentation, TODO, and changelog order

Run this phase once per integration batch, after the behavior commits in that
batch exist.

1. Update `docs/reviews/review-followup-coverage.md` from the new HEAD. Move a
   row only when its judgment condition holds. Leave `spec-waiting` rows open.
2. Correct stale review docs in the same batch:
   - `docs/reviews/font-atomicity-policy-2026-10-23.md` still says the
     no-rollback policy was not implemented.
   - Survey item 15 stays Open until C2 is done. Item 17 stays Open until L1
     is implemented. Item 16 already matches the partial font fix.
3. Update `CHANGELOG.md` Unreleased for behavior that is in the batch and
   missing today: M2, M3, M4, the M5 existing-key fix, M8, C3, the C4 capped
   doctor line, and C5. Do not add a shipped version heading.
4. Add a repository TODO file only if the commander asks. Until then, open
   rows in the coverage table are the TODO. Do not invent a second list that
   can drift.
5. User-facing README or `SECURITY.md` changes wait until the batch changes a
   contract the user can observe.

## Remnant audit

At each batch boundary, before the documentation commit, inventory the
repository and record the result in the coverage doc:

- open pull requests
- local branches and worktrees
- uncommitted or untracked files
- active Formation and TGL sessions
- temporary caches under `/tmp` whose names start with `herdr-lineup-`

For each item record owner, whether it is contained in the integration head,
and whether it is in use. Do not delete a dirty tree, a tree with unknown
owner, or a tree another session is using. Removal of unused clean worktrees
waits for an explicit commander instruction.

## Shipment

Members do not push and do not request CI. The commander batches one
integration candidate. Per-task pushes are out of policy because they spend
Actions minutes. The current shipment hold remains until the owner lifts it.
ACK, merge to `develop` or `main`, push, CI, and deploy are not implied by a
green local test or by this plan.

## Handoff cycle

After one assignment:

1. Commit the assignment's own worktree and report `CANDIDATE_READY` with the
   full SHA. Wait for the commander ACK.
2. Write a handoff, clear the session, and leave the lane.
3. Join again only with a new challenge and a new claim.
4. If no assignment arrives, inspect Formation status on a regular cadence and
   report to the commander. Do not prompt another pane.

## Record-only candidates

A record-only ship path, under the current Formation rule, is a commit whose
changed paths are only below `docs/reviews/`. It still needs a full SHA, a
clean worktree, and a commander ACK. It does not replace the behavior
judgment conditions above.

This coverage assignment also adds `docs/plans/review-followups-design.md` and
`docs/plans/review-followups-plan.md`, so its commit is not record-only.
Later measurement or design tasks that should stay record-only must not add
files outside `docs/reviews/`.

A record is a clean candidate only when `git status` is empty and
`git rev-parse HEAD` is the commit that contains it. Repo-external review
notes are not that candidate.

## Per-task close

Each task ends in its own commit on its own worktree. The commit message
states the row id. The next Formation report includes the full SHA, the
verification commands that actually ran, and the independent review run id
when the assignment requires one. A task box above stays unchecked in git
until that evidence exists. This plan does not check boxes for the current
wave in advance.
