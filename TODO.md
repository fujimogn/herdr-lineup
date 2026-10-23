# TODO

Open work after the 2026-10-23 whole-repository review and latency
investigation. IDs refer to `docs/reviews/review-followup-coverage.md`, which
holds the evidence and judgment conditions. `docs/plans/review-followups-plan.md`
holds the working rules. Nothing here is released: the integration line is
local and unshipped, and the version stays 0.2.2.

## In progress on other lanes (not integrated)

- Stage histogram fix for the doctor refresh-stage lines.
- Publication wiring of the Git heading cache, the fair queue, the cycle
  budget, the 30 s confirmed fallback, and the shared socket deadlines (L2,
  L3).
- Publication wiring of one `ReadCycle` per refresh and of reader worker
  diagnostics (L4, L5).

## Open

- [ ] Final inclusive GPT-6.1 Sol high review from `d3ccbc17` to the fixed
  final head, owned by the commander. Re-run the default parallel test suite
  on that head (M8).
- [ ] Remeasure stage timings on the wired head, running old and new binaries
  alternately on one host, tmpfs and a real disk (N1). No timing claim before
  that.
- [ ] Measure Git heading latency and child counts on the wired head (L2, N3).
- [ ] Measure reader discovery on a real disk with pane and candidate counts
  separated (L4, N4).
- [ ] Record stuck-worker frequency from the wired diagnostics (L5). The
  bounded fixtures are not that measurement.
- [ ] Specify the Codex discovery cap (per-root or shared) and the duplicate-ID
  rule before changing it (C1, survey item 19).
- [ ] Verify the remaining sequence-window failure rows where feasible: second
  allocator, competing same-source writer, config change during refresh,
  rollback to an older binary, macOS disk (L1).
- [ ] Font check-then-rename and check-then-unlink races (H1, M1) stay open.
  Any closing design needs owner approval; recovery stays manual.

## Conditional (not obligations)

Start only when the stated condition is met.

- Refresh period or event-driven refresh (L6): needs an event-to-visible
  definition and a Linux and macOS measurement without a live Herdr (N5).
- Whole-refresh deadline (L3): needs a written contract that keeps sequence
  durability and endpoint identity.
- `statusline --then` and `configure` child timeouts or direct exec (L7):
  needs a host deadline, recovery, and compatibility contract.
- Socket connection reuse (L8): needs a syscall trace and an adversarial
  replacement fixture; path identity stays.
- Reader process isolation (L5): needs a measured stuck-worker frequency.

## Separate task

- Agy occupant generation (C2, survey item 15): owner-assigned to a separate
  Herdr host task. Current behavior and the constraint in
  `openspec/specs/provider-readers/spec.md` stay; this list never marks it
  resolved. See `docs/reviews/agy-occupant-generation.md`.

## Completed in the local integration line (unshipped)

Integrated up to `d95e5b5`; see the coverage table for evidence.

- M2 Kimi `O_CLOEXEC`, M3 Kimi transient stat, M4 exact Grok weekly kinds,
  C3 Grok credential change, C5 release-gate failure classes.
- M5 existing-key update and FIFO single eviction.
- M6 typed publisher stages; M7 typed font journal with separate planning.
- M8 socket test waits for EOF.
- C4 doctor capped scan and `readdir` error versus end.
- L1 endpoint sequence window, poison after an uncertain write, and failure
  fixtures.
- L3 refresh duration, histogram, and outcome counts in doctor; socket
  absolute-deadline API and refusal of ambiguous reuse.
- API only, unwired: L2 Git heading cache, L4 `ReadCycle`, L5 reader
  diagnostics.
- H1 and M1 partial: automatic rollback removed and a recheck before font
  removal; the residual races above remain.
