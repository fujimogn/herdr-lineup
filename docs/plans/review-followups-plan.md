# Review follow-ups implementation plan

**Goal:** Finish the accepted 2026-10-23 review findings without marking
partial, unwired, measured, conditional, or separate-task rows as complete.

**Architecture:** See `docs/plans/review-followups-design.md`.

**Status:** `docs/reviews/review-followup-coverage.md` is the status table at
the latest integration snapshot. Open work is listed once, in `TODO.md`. This
plan keeps only the rules for doing that work.

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

## Done in the integration snapshot

The first wave and its follow-ups are integrated locally (not pushed, not
shipped): typed font journal (M7), publisher stages (M6), sequence window with
poison and failure fixtures (L1), FIFO single eviction (M5), doctor scan
errors (C4), Git cache API (L2), shared socket deadline API (L3 part),
`ReadCycle` discovery sharing API (L4), reader worker diagnostics API (L5),
and refresh duration observation (L3 part). The coverage table has the
evidence and remaining limits. API-only rows have no runtime effect until
wired.

## Final inclusive review

Owner: `lane_commander`. This is a completion condition for the integration
candidate. A member lane does not launch it.

- Review the full diff from `d3ccbc17d3f8344d466015fac580cd0fe3b60269` to the
  final head fixed when the review starts, with GPT-6.1 Sol high.
- Include every commit that an accepted review does not already cover, and
  resolve every NO-GO in that history with a later GO in the same chain.
- Treat the local Sol high reads as evidence only. They are not queue
  acceptance.

## Order of remaining work

`TODO.md` lists the open items. Take them in this order of dependency:

1. Land the in-progress wiring (Git cache, shared deadlines, `ReadCycle`, and
   reader diagnostics into publication) and the stage-histogram fix, each with
   its own fixtures. Until then those rows stay `api-unwired`.
2. Measure on the wired head (N1, N3, N4, L5 frequency) before any budget,
   cap, or isolation decision. Linux numbers are not extrapolated to macOS.
3. Decide C1 before any discovery cap change.
4. Conditional proposals (L6 period change and N5 event-to-visible, L7 child
   timeouts and direct exec, L8 socket reuse, L5 process isolation) start only
   when their written condition is met. They are not obligations.
5. C2 belongs to the separate Herdr host task. Keep current behavior here.
6. H1 and M1 residual races stay `partial` unless the owner approves a design.
   R1 and R2 stay rejected.

## Documentation, TODO, and changelog order

Run this phase once per integration batch, after the behavior commits in that
batch exist.

1. Update `docs/reviews/review-followup-coverage.md` from the new integration
   SHA. A documentation commit is never the source snapshot.
2. Move finished items from the open sections of `TODO.md` to its completed
   section in the same commit. Do not leave a completed item open.
3. Correct stale review docs in the same batch. Keep historical text only
   under a heading that says it is historical.
4. Update `CHANGELOG.md` Unreleased for behavior in the batch. Do not add a
   version heading; the manifest version stays until a release.
5. Change README or `SECURITY.md` only where the batch changes behavior a user
   can observe.

## Remnant audit

At each batch boundary, before the documentation commit, inventory open pull
requests, local branches and worktrees, uncommitted or untracked files,
active Formation and TGL sessions, and `/tmp/herdr-lineup-*` caches. For each,
record owner, whether the integration head contains it, and whether it is in
use. Do not delete a dirty tree, a tree with unknown owner, or a tree another
session is using. Removal of unused clean worktrees waits for an explicit
commander instruction.

## Shipment

Members do not push and do not request CI. The commander batches one
integration candidate. The shipment hold remains until the owner lifts it.
ACK, merge to `develop` or `main`, push, CI, and deploy are not implied by a
green local test or by this plan.

## Handoff cycle

After one assignment: commit on the assignment's own worktree, report
`CANDIDATE_READY` with the full SHA, wait for the commander ACK, write a
handoff, clear, and leave the lane. Join again only with a new challenge. If
no assignment arrives, check Formation status on a regular cadence and report
to the commander.

## Record-only candidates

A record-only ship path is a commit whose changed paths are only below
`docs/reviews/`. It still needs a full SHA, a clean worktree, and a commander
ACK. A record is a clean candidate only when `git status` is empty and
`git rev-parse HEAD` is the commit that contains it.
