# TODO

Work for the next release. Historical finding IDs, implementation snapshots,
fixtures, and limits are mapped in
[review follow-up coverage](docs/reviews/review-followup-coverage.md).
Working rules are in [the follow-up plan](docs/plans/review-followups-plan.md).
Completed work below does not establish publication; release versions are
chosen during release preparation.

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
- [ ] Investigate timing dependence in
  `publish::tests::fast_pane_publishes_before_subsequent_slow_reader`:
  the unchanged base failed 1 of 10 default-parallel full-suite runs on
  2026-11-04 (commander-supplied observation; not yet investigated).
- [ ] Scope Git cache invalidation generations per common directory, with a
  global epoch for `Rejected`. At `d75e988`, a repository without `origin/main`
  can suppress another repository's cache update in the same parallel batch;
  displayed values and child counts are unchanged (`RepositoryCache::apply`
  in `src/ship_time.rs`).
- [ ] Refine nonblocking stop boundaries after `51be817`: the wait-loop order
  can report timeout for a daemon that exits in the final 20 ms, and fixed
  `LifecycleLock` sleeps can delay observation by about 20 ms (`src/daemon.rs`).
- [ ] Handle thread-spawn `EAGAIN` without failing the entire heading stage
  with `io::Error` (`resolve_headings_in_budget` in `src/publish.rs`, `d75e988`).
  Preserve the guarantee that all started observations are joined.
- [ ] Font check-then-rename and check-then-unlink races (H1, M1). Closing
  them needs an owner-approved design; partial-state recovery stays manual.

## Completed for the next release

- [x] Wired-head Git heading latency and child-count matrix (L2, N3):
  [separate wired-head observation record](docs/reviews/git-heading-latency-wired-2026-11-04.md).
- [x] Real-disk reader discovery with independent pane/candidate counts
  (L4, N4): [ext4 observations and missing-context limits](docs/reviews/reader-discovery-real-disk-2026-11-04.md).
- [x] Short live observation using the connected reader diagnostics (L5):
  121 samples over ten minutes; BUSY and retained-worker gauges stayed zero,
  with sampling and provenance limits in
  [stuck-worker-frequency-2026-11-04.md](docs/reviews/stuck-worker-frequency-2026-11-04.md).
- [x] Make `envelope_expiring_after_admission_is_an_error_not_a_skip`
  deterministic (`b924a54`): 20/20 runs passed under load; the old fixture
  failed 17/20 (commander-supplied comparison). Production deadlines are unchanged.
- [x] Close stale N1 references in the historical stage record and follow-up
  plan using the [alternating comparison](docs/reviews/stage-timing-comparison-2026-11-04.md).
- [x] Record the [Codex reader performance trial](docs/reviews/codex-reader-perf-trial-2026-11-04.md):
  median improvement 4.5%, below the 10% criterion. The owner declined the
  append-only assumption and relaxed read guarantees; no runtime change landed.
- [x] Record the [isolated latency investigation](docs/reviews/latency-investigation-2026-11-04.md);
  rendered latency, macOS, and stable tails remain unmeasured.
- [x] Record the [source review](docs/reviews/code-review-2026-11-04.md) and
  resolve CR-01 (`a9b4b84`) and CR-02 (`51be817`).
- [x] Split bounded reader execution and diagnostics out of `model.rs` into
  private modules without changing behavior (`0a37245`).

- [x] **Fix timing-dependent `publish::tests` failures.** Publisher fixtures
  inject the shared reader-budget clock and explicitly advance it for timeout
  restoration. Boundary coverage checks that the budget starts after headings
  and skips reader I/O at exactly zero remaining time. Discovery uses the normal
  worker budget, and the reader-only fixture uses the normal cycle envelope.
  Production deadline checks retain real monotonic time.

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

## Completed measurements after v0.3.1

- [x] N1 old/new synthetic stage timing with binaries alternated on one host
  and filesystem; [procedure, raw rows, and limits](docs/reviews/stage-timing-comparison-2026-11-04.md).
  Historical and new observations remain separate; no speed-up or SLO is established.

## Implemented in the v0.3.1 release contents

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
  in `docs/reviews/refresh-stage-latency-2026-10-23.md`; the later alternating
  old/new comparison is recorded under Completed measurements after v0.3.1 above.
