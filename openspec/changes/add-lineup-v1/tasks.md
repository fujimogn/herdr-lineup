## 1. Specification setup (this change preparation)

- [x] Initialize OpenSpec without agent tool integrations or package.json.
- [x] Check radar source assignments and the installed font's actual cmap.
- [x] Define pane/workspace tokens, state, reader, and daemon contracts.
- [x] Run OpenSpec 1.14.1 strict validation with no findings.

## 2. Daemon and configuration implementation

- [x] Prove concurrent ensure ownership and safe stop with failing tests, then
  implement lock transfer, control identity, socket replacement, and doctor.
- [x] Prove default/bounded config parsing and failure behavior, then implement
  startup/focus ensure hooks and documented actions; verify minimum Herdr version.
- [x] Prove config preservation, conflicts, repeat actions, and reload rollback,
  then implement opt-in configure/unconfigure and verify the validation API.

## 3. Sidebar and reader implementation

- [x] Prove token budget, sequence restart, TTL renewal, occupant changes, and
  missing numeric clearing.
- [x] Implement metadata publication wiring from the daemon publisher hook.
- [x] Prove each state, font/text mode, date boundary, and absent origin/main
  case, then implement presentation and headings without Git fetches.
- [x] Accept usage-source research, prove session identity and unavailable/stale
  handling, then implement confirmed provider adapters; keep Kimi/Hermes missing
  until their sources are confirmed.
- [x] Prove statusLine capture and `--then` chaining, then implement
  `herdr-lineup statusline <claude|agy>` and the Claude and Antigravity readers.
  Context values come from Codex, Grok, Claude, and Antigravity. Weekly values
  come from Claude and Antigravity only. Kimi and Hermes stay unavailable.
- [x] Bundle the icon font and implement owned opt-in install-font and
  uninstall-font, including attribution.
- [x] Verify an isolated Herdr session end to end with `scripts/e2e-isolated.sh`
  and `docs/testing.md`. Workspace heading tokens arrive, stay updated across
  the TTL window, and are gone after stop. A real agent pane stays SKIP because
  the isolated home has no provider authentication.
- [ ] Document remaining opt-in examples, and archive only after the
  implementation tasks pass.
