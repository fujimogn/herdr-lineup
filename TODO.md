# TODO

Open work after the 2026-10-23 reviews. Finding IDs, implementation source
`0220da64f8545f89b6daca211553f486b271057f`, fixtures and limits are mapped in
`docs/reviews/review-followup-coverage.md`. Working rules are in
`docs/plans/review-followups-plan.md`. The implemented items below are included
in the v0.3.0 release contents.
Publication and artifacts are recorded by the [v0.3.0 tag and GitHub Release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.3.0);
this preparation record does not establish publication.

## Open

- [ ] Controlled old/new stage timing comparison, with binaries alternated
  on one host and filesystem (N1). Historical and new-candidate synthetic
  observations are separate records, not a speed-up or SLO.
- [ ] Wired-head Git heading latency and child-count matrix (L2, N3).
- [ ] Real-disk reader discovery with independent pane/candidate counts
  (L4, N4); the historical matrix is tmpfs-only.
- [ ] Stuck-worker frequency using the connected diagnostics (L5). Bounded
  channel fixtures are not OS-stall or live-frequency measurements.
- [ ] Remaining sequence-window failure rows: kernel crash/power loss,
  concurrent ensure or second allocator, competing same-source writer,
  config change during refresh, old-binary rollback, macOS disks (L1/N2).
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

## Implemented in the v0.3.0 release contents

- [x] Commander-owned inclusive GPT-6.1 Sol high GO for fixed range
  `d3ccbc17d3f8344d466015fac580cd0fe3b60269..91268267f46061f5f0aaf46711ea10158bad0873`;
  the Dplan NO-GO is resolved. Mandatory source checks passed: fmt, strict
  locked Clippy, default parallel tests (441 unit, 1 integration, 6 existing
  ignored), locked release build and help; documentation checks also passed.
  Local review GO is not automated queue acceptance or shipment.
- [x] M2 close-on-exec, M3 transient Kimi stat, M4 exact Grok weekly kinds,
  C3 credential-change invalidation, C5 release-gate failure categories.
- [x] M5 existing-key replacement and FIFO single eviction; M6 typed publisher
  stages; M7 typed font journal/plan; M8 bounded EOF-wait regression; C4
  capped counts and directory EOF/error distinction.
- [x] L1 sequence windows, uncertain-write poison and verified failure rows.
- [x] L2 publisher-owned Git cache, attempt-based fair queue, 1 s cycle Git
  budget and finite 30 s confirmed fallback. Hit and miss still use three
  children; no measured speed improvement is claimed.
- [x] L3 total/stage histograms, production stage samples and counts, shared
  socket deadlines, default 7 s cooperative refresh envelope and separate
  deadline skips. Unavailable/legacy diagnostics are distinct from zero.
- [x] L4 one `ReadCycle` per refresh and actual-reader shared-scan fixtures.
- [x] L5 all five observed worker producers, control live sampling and doctor
  output. Hermes has no worker; an empty slot is not a successful read.
- [x] C1 combined 4,096-entry cap, duplicate-ID refusal and cumulative
  fallback accounting documented and covered by boundary fixtures.
- [x] Canonical runtime specs synchronized and change archived; P/D plans
  record implemented status and historical source evidence.
- [x] H1/M1 partial mitigation: no automatic rollback, pre-removal recheck;
  the residual races remain open above.
- [x] N1 wired synthetic cold-refresh observations: tmpfs and ext4, five pane
  counts, n=3 each. The new results are separated from historical baselines
  in `docs/reviews/refresh-stage-latency-2026-10-23.md`; controlled timing
  comparison remains open above.
