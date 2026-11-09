# TODO

Work after v0.3.2. Historical finding IDs, implementation snapshots,
fixtures, and limits are mapped in
[review follow-up coverage](docs/reviews/review-followup-coverage.md).
Working rules are in [the follow-up plan](docs/plans/review-followups-plan.md).
Publication and artifacts are recorded by the
[v0.3.2 tag and GitHub Release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.3.2).
Work waiting for the next release is in the `Unreleased` section of
[CHANGELOG.md](CHANGELOG.md).

## Open

- [ ] Long-term OS-stall frequency (L5) remains unmeasured. A ten-minute
  sample does not establish an event rate or whether isolation is needed.
- [ ] Remaining sequence-window failure rows (L1/N2). Publisher ownership
  serialization and handover, a same-source writer no-op negative control,
  and the in-flight loaded config with an explicitly reloaded provider clear
  are verified by fixtures. Kernel crash/power loss, full daemon ensure
  spawn/inheritance/readiness, competing-writer protection, automatic reload
  or a restarted publisher clearing state, old-binary rollback, and macOS
  disks remain open.
- [ ] Investigate the macOS-only failure of
  `publish::tests::kimi_production_wiring_covers_opt_in_success_rotation_expiry_and_reset`:
  the isolated child failed with empty stderr in the v0.3.2 platform check
  (run 41847126534, first attempt) and passed on the job rerun. Its stdout is
  piped but not reported, so the cause is unknown; Linux passed.
- [ ] Assert that `publish::tests::fast_pane_publishes_before_subsequent_slow_reader`
  reaches the slow reader, so a changed session ID or a skipped reader cannot
  pass on pane order alone (non-blocking review finding).
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

Observation records from v0.3.2 keep their own procedures and limits. The N1
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
