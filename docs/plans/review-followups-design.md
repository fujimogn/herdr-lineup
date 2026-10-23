# Review follow-up design

## Scope

Follow-ups from the 2026-10-23 whole-repository review and latency
investigation build on the local integration line. The mapping of every
finding, and the integration snapshot it was read from, is
`docs/reviews/review-followup-coverage.md`. This design does not reopen
`rejected-wrong-basis` rows R1 and R2, and it does not treat `partial`,
`conditional`, or `separate-task` rows as done.

`develop` does not contain the integration line. Shipment stays on hold until
the owner asks for one batch.

## Approach

Land one behavior at a time on a dedicated worktree, with a regression test
before broadening the change. Review each fixed range with the owner-selected
GPT-6.1 Sol high reviewer, read-only, with no native subagent fork. The final
candidate connects the P/D APIs and all five worker producers; implementation
does not imply a measured runtime improvement. A measurement document is not
an implementation approval.

## Boundaries that still hold

- Font ownership is typed at load. Version 1, unknown fields, pending-state
  refusal, original bytes, and manual recovery stay. A failure after the
  `pending` journal leaves partial state for manual recovery; automatic
  rollback is not restored. The check-then-rename and check-then-unlink races
  remain and are not called closed.
- Publisher stages separate inventory, headings, observations, desired
  reports, and send. Capacity skips stay per resource. An ambiguous socket
  result stops the cycle.
- The endpoint sequence window persists the durable upper bound before any
  number in the window is sent. An uncertain checkpoint write poisons the
  publisher until a new instance. Restart never reuses a number that might
  have reached Herdr, and pruning never lowers the durable floor. Rows of the
  failure table without test evidence stay design intent.
- Git and reader caches name their invalidation rules. A cache hit does not
  skip association, ownership, or `safe.directory` checks. HEAD-only mtime and
  previous-path-only discovery are rejected.
- Production stage samples and total/stage histograms are connected. The
  default 7 s envelope is derived from existing local caps, not an empirical
  SLO; see `publisher-runtime-wiring.md` for admission and expiry semantics.
  The 5 second refresh default and endpoint path identity stay. No task
  replaces a live Herdr server, reads real credentials, or writes user configuration.
- C2 generation binding remains unresolved in a separate Herdr host task; its
  planned contract requires unknown generation to fail closed. Current
  pane-bound Agy behavior is kept. OS-blocked filesystem I/O is not given a
  false strict wall-clock deadline.

## Validation and rollback

Each behavior task starts from a failing regression or an existing invariant
suite, then runs focused tests. The integration candidate runs
`cargo fmt --all --check`, locked Clippy for all targets with warnings denied,
locked tests, a locked release build, and binary help. A fresh worktree build
can exceed 1 GiB; report that estimate before starting it. A documentation-only
change runs encoding, whitespace, and link checks instead of a Rust build.

Benchmark fixtures use synthetic data and must not delete a non-`/tmp` root.
Each task is its own commit. Rollback is the previous local commit, without
rewriting history. Shipment is a separate owner decision.
