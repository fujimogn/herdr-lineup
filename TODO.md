# TODO

Work after v0.3.3. Historical finding IDs, implementation snapshots,
fixtures, and limits are mapped in
[review follow-up coverage](docs/reviews/review-followup-coverage.md).
Working rules are in [the follow-up plan](docs/plans/review-followups-plan.md).
Publication and artifacts are recorded by the
[v0.3.3 tag and GitHub Release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.3.3).
Work waiting for the next release is in the `Unreleased` section of
[CHANGELOG.md](CHANGELOG.md).

## Open

- [ ] Long-term OS-stall frequency (L5) remains unmeasured. A ten-minute
  sample does not establish an event rate or whether isolation is needed.
- [ ] Remaining sequence-window failure rows (L1/N2). Fixtures now cover
  publisher ownership and handover, detached ensure with lock inheritance and
  readiness, and a new publisher clearing a disabled provider. Kernel
  crash/power loss, old-binary rollback, macOS disks, the `foreground` wrapper
  setup, scheduler interleavings, and a detached daemon restart after a config
  change remain unverified.
  Competing-writer protection and automatic config reload are not implemented,
  so there is no behavior to verify; see the
  [sequence reservation record](docs/reviews/sequence-reservation-design-2026-10-23.md).
- [ ] Find the cause of the macOS-only failure of
  `publish::tests::kimi_production_wiring_covers_opt_in_success_rotation_expiry_and_reset`:
  the isolated child failed with empty stderr in the v0.3.2 platform check
  (run 41847126534, first attempt) and passed on the job rerun; Linux passed.
  The test now reports the child's status, stdout, and stderr, so start from
  that output on the next failure. A passing rerun is not evidence of a fix.
- [ ] Font check-then-rename and check-then-unlink races (H1, M1). Closing
  them needs an owner-approved design; partial-state recovery stays manual.

## Conditional

Start only when the named condition is met; these are not obligations.

- Period/event-driven refresh (L6/N5): event-to-visible definition and Linux
  and macOS measurements without contacting a live Herdr.
- `statusline --then` / `configure` child timeouts or direct exec (L7): host
  deadline, recovery and compatibility contract.
- Socket reuse (L8): syscall trace and adversarial endpoint replacement
  fixture, preserving the pathname identity check.
- Reader process isolation (L5): measured stuck-worker frequency and a
  resource/privacy/restart contract.
- Multi-resource metadata reports: a Herdr API that accepts several resources
  in one request. The Herdr 0.9.5 API schema checked on 2026-11-09 has none,
  so this cannot be done in Lineup alone; see the
  [refresh path survey](docs/reviews/performance-survey-refresh-path-2026-11-09.md).
- Socket and admission tail attribution: a concrete decision that depends on
  where deadline skips come from.

Not planned: refreshing early on statusLine updates (item 1 of the
[observer survey](docs/reviews/performance-survey-observers-2026-11-09.md)).
The owner decided against it on 2026-11-09.

## Separate Herdr host task

- [ ] C2 / survey item 15 remains unresolved by owner assignment: distribute
  occupant generation, apply only to the same generation, and invalidate
  displayed metadata on replacement. Current pane-bound Agy behavior stays.
  See `docs/reviews/agy-occupant-generation.md` for the planned host contract.

## Shipped

Completed work is listed in [CHANGELOG.md](CHANGELOG.md) and each
[GitHub Release](https://github.com/fujimogn/herdr-lineup/releases), and review
findings are mapped in
[review follow-up coverage](docs/reviews/review-followup-coverage.md).

Observation records keep their own procedures and limits. The N1
comparison establishes no speed-up or SLO, and the isolated latency
investigation leaves rendered latency, macOS, and stable tails unmeasured:

- [Alternating stage timing (N1)](docs/reviews/stage-timing-comparison-2026-11-04.md)
- [Wired-head Git heading latency (L2, N3)](docs/reviews/git-heading-latency-wired-2026-11-04.md)
- [Real-disk reader discovery (L4, N4)](docs/reviews/reader-discovery-real-disk-2026-11-04.md)
- [Short live reader-diagnostics sample (L5)](docs/reviews/stuck-worker-frequency-2026-11-04.md)
- [Isolated latency investigation](docs/reviews/latency-investigation-2026-11-04.md)
- [Codex reader performance trial](docs/reviews/codex-reader-perf-trial-2026-11-04.md):
  below the 10% criterion; the owner declined the append-only assumption and
  relaxed read guarantees, so no runtime change landed.
- [Source review](docs/reviews/code-review-2026-11-04.md)
- [Refresh path performance survey](docs/reviews/performance-survey-refresh-path-2026-11-09.md)
  and [observer-side latency survey](docs/reviews/performance-survey-observers-2026-11-09.md)
  (2026-11-09): improvement candidates only; nothing was changed.
- `envelope_expiring_after_admission_is_an_error_not_a_skip` determinism:
  20/20 runs passed under load, while the old fixture failed 17/20
  (commander-supplied comparison). Production deadlines are unchanged.
