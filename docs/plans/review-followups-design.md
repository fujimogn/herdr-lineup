# Review follow-up design

## Scope

Keep the local candidate `96829df7e6fc51af327b448f5b8b3596f0a96d3f` as the base
for follow-ups from the 2026-10-23 whole-repository review and latency
investigation. The mapping of every finding is
`docs/reviews/review-followup-coverage.md`. This design does not reopen
`rejected-wrong-basis` rows R1 and R2, and it does not treat `spec-waiting`
rows as done.

Several reviewed defects are already `fixed-in-candidate` or `partial`. Later
tasks must not reimplement them. `develop` does not contain this candidate.
Shipment stays on hold until the owner asks for one batch.

## Approach

Land one behavior at a time on a dedicated worktree, with a regression test
before broadening the change. Review each fixed range with the owner-selected
GPT-6.1 Sol high reviewer, read-only, with no native subagent fork. A single
unreviewed rewrite would hide safety regressions. Changing the 5 second
default without removing repeated work would increase load.

Rows that are only measured or only designed stay in `docs/reviews/` until
their judgment condition is met. A measurement document is not an
implementation approval.

## Boundaries that still hold

- Font ownership may become typed at the load boundary. Version 1, unknown
  fields, pending-state refusal, original bytes, and manual recovery stay.
  The candidate no longer rolls back automatically. Do not restore automatic
  rollback. The check-then-rename and check-then-unlink race remains open and
  is not called closed.
- Publisher stages may separate inventory, headings, observations, desired
  reports, and send. Capacity skips stay per resource. An ambiguous socket
  result stops the cycle. Sequence reservation behavior does not change in
  the stage split.
- An endpoint reservation window is design-only. The candidate still persists
  each sequence before send. Restart must not reuse a number that might have
  reached Herdr. Resource pruning must not lower the durable floor. Wire and
  checkpoint formats stay compatible until a reviewed migration says otherwise.
- Git and reader caches must name their invalidation rules. A cache hit must
  not skip association or ownership checks. HEAD-only mtime and
  previous-path-only discovery are rejected.
- Stage timings and bounded waits are measured before any new budget. The
  5 second default and endpoint path identity stay. No task replaces a live
  Herdr server, reads real credentials, or writes user configuration.
- Unknown occupant generation fails closed. OS-blocked filesystem I/O is not
  given a false strict wall-clock deadline.

## Validation and rollback

Each behavior task starts from a failing regression or an existing invariant
suite, then runs focused tests. The integration candidate later runs
`cargo fmt --all --check`, locked Clippy for all targets with warnings denied,
locked tests, a locked release build, and binary help. A fresh worktree build
can exceed 1 GiB; report that estimate before starting it. The commander
permitted one such run on the coverage worktree. The commander also owns the
final GPT-6.1 Sol high review from `d3ccbc17d3f8344d466015fac580cd0fe3b60269`
to the final head fixed when that review starts. A local GO is not queue
acceptance, and an unconfirmed review is not completion.

Benchmark fixtures use synthetic data and must not delete a non-`/tmp` root.
Each task is its own commit. Rollback is the previous local commit, without
rewriting history. Shipment is a separate owner decision.
