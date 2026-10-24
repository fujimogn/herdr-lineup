# Review follow-ups implementation plan

**Goal:** Finish the accepted 2026-10-23 review findings without marking
partial, measured, conditional, or separate-task rows as complete.

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

## Implemented in the v0.3.1 release contents

The implementation source is
`0220da64f8545f89b6daca211553f486b271057f`. P runtime wiring, D control/doctor
wiring and all five worker producers are included in v0.3.1 alongside the
first-wave fixes. The coverage table maps every original finding to source,
fixtures and remaining limits; `TODO.md` keeps the open and completed items.
The runtime plan and archived spec are linked there. This source SHA is historical
implementation evidence, not a release HEAD. No timing improvement,
strict wall-clock guarantee, host C2 fix, or shipment follows from this status.

## Final inclusive review

Owner: `lane_commander`. This is a completion condition for the integration
candidate. A member lane does not launch it.

Recorded outcome: inclusive Sol 6.1 high GO for fixed range
`d3ccbc17d3f8344d466015fac580cd0fe3b60269..91268267f46061f5f0aaf46711ea10158bad0873`,
with the Dplan NO-GO resolved. Mandatory checks passed as recorded in the
coverage table. Automated queue acceptance and shipment are not established.
The general procedure below applies to subsequent candidates.

- Review the full diff from `d3ccbc17d3f8344d466015fac580cd0fe3b60269` to the
  final head fixed when the review starts, with GPT-6.1 Sol high.
- Include every commit that an accepted review does not already cover, and
  resolve every NO-GO in that history with a later GO in the same chain.
- Treat the local Sol high reads as evidence only. They are not queue
  acceptance.

## Order of remaining work

`TODO.md` lists the open items. Take them in this order of dependency:

1. The commander's final synthetic observations are recorded separately from
   historical results. Controlled timing comparison, Git/reader real-disk
   matrices and stuck-worker frequency remain the measurement work in
   `TODO.md`.
2. Conditional proposals (L6 period/event changes and N5 event-to-visible,
   L7 child timeouts/direct exec, L8 reuse, L5 isolation) start only when their
   written condition is met. They are not obligations. The L3 cooperative
   envelope and C1 combined-cap contract are already implemented.
3. C2 belongs to the separate Herdr host task and stays unresolved here.
4. H1/M1 residual races stay partial unless the owner approves a design.
   R1/R2 stay rejected.

## Documentation, TODO, and changelog order

Run this phase once per integration batch, after the behavior commits in that
batch exist.

1. Update `docs/reviews/review-followup-coverage.md` from the new integration
   SHA. A documentation commit is never the source snapshot.
2. Move finished items from the open sections of `TODO.md` to its completed
   section in the same commit. Do not leave a completed item open.
3. Correct stale review docs in the same batch. Keep historical text only
   under a heading that says it is historical.
4. Update `CHANGELOG.md` Unreleased for behavior in a development batch. For
   an authorized release, date that section, add a fresh empty Unreleased
   section, and align the manifest and lockfile versions.
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
integration candidate. The owner lifted the shipment hold on 2026-10-24 for
one release batch. Maintainers selected v0.3.1 after the failed v0.3.0 macOS
fixture attempt; its immutable tag has no GitHub Release. Delivery is recorded by the
[v0.3.1 tag and GitHub Release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.3.1);
this preparation plan does not establish publication.
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
