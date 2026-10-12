## 1. Specification setup (this change preparation)

- [x] Initialize OpenSpec without agent tool integrations or package.json.
- [x] Check radar source assignments and the installed font's actual cmap.
- [x] Define pane/workspace tokens, state, reader, and daemon contracts.
- [x] Run OpenSpec 1.14.1 strict validation with no findings.

## 2. Daemon and configuration implementation (future work)

- [ ] Prove concurrent ensure ownership and safe stop with failing tests, then
  implement lock transfer, control identity, socket replacement, and doctor.
- [ ] Prove default/bounded config parsing and failure behavior, then implement
  startup/focus ensure hooks and documented actions; verify minimum Herdr version.
- [ ] Prove config preservation, conflicts, repeat actions, and reload rollback,
  then implement opt-in configure/unconfigure and verify the validation API.

## 3. Sidebar and reader implementation (future work)

- [ ] Prove token budget, sequence restart, TTL renewal, occupant changes, and
  missing numeric clearing, then implement metadata publication.
- [ ] Prove each state, font/text mode, date boundary, and absent origin/main
  case, then implement presentation and headings without Git fetches.
- [ ] Accept usage-source research, prove session identity and unavailable/stale
  handling, then implement confirmed provider adapters; keep Kimi/Hermes missing
  until their sources are confirmed.
- [ ] Verify an isolated Herdr session end to end, document font attribution and
  opt-in examples, and archive only after the implementation tasks pass.
