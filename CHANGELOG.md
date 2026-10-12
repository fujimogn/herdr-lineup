# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Secret-free per-session weekly query diagnostics displayed by `doctor`,
  including distinct Grok auth-file, schema, ambiguity and HTTP failure categories.
- Grok billing compatibility with case-insensitive prefixed weekly period names,
  optional reset times with bounded freshness, and inherited environment proxies.

- Opt-in Codex account and Grok billing quota workers (`allow_network = true`).
  Refresh reads memory without waiting for quota queries. Grok weekly is shared
  across its panes; Codex weekly requires an exact session in the daemon home.
  Shutdown stops scheduling, skips pending HTTP joins, and kills/reaps the Codex
  app-server child group while preserving the five-second stop deadline.

- First-pane repository heading tokens `lineup_group` and
  `lineup_group_ship_text`, sharing snapshot-order selection with logo
  indentation and clearing when pane order or repository availability changes.

- Opt-in `non_first_pane_logo_indent` (0..=16, default 0) to align agent rows
  when the repository heading occupies the first pane's first row. Only later
  panes' `lineup_logo` values receive U+2800 padding, following snapshot order.

- Rust crate and command-line scaffold for the planned Lineup sidebar publisher.
- Herdr manifest with startup, workspace-focus recovery, and explicit actions.
- Library scaffold with declared runtime modules.
- Pure sidebar token formatting and patch planning.
- Local repository ship-time reading and workspace heading formatting.
- Session-bound local usage readers.
- Tag-triggered release workflow with version-consistency checks and release
  documentation.
- Research on public Herdr plugin conventions.
- Linux and macOS build checks, weekly dependency audit, and public project documentation.
- Pure Herdr metadata protocol and snapshot parsing.
- Session-scoped daemon lifecycle, bounded configuration, and doctor checks.
- Opt-in configure and unconfigure actions that preserve unrelated sidebar configuration.
- Bundled icon font with owned opt-in install-font and uninstall-font actions.
- Durable metadata publication from the session daemon, including sequence
  checkpoints and TTL renewal.
- Isolated Herdr lifecycle harness (`scripts/e2e-isolated.sh`) and the procedure
  in `docs/testing.md`. Linux evidence covers workspace heading tokens arriving,
  remaining after a TTL window, and clearing after stop. Real agent panes stay
  skipped when the isolated home has no provider authentication.
- `statusline` subcommand for opt-in Claude and Antigravity statusLine capture.
  It records usage for the sidebar and prints nothing unless `--then` returns
  the existing command's display.
- Claude and Antigravity usage readers. Context comes from Codex, Grok, Claude,
  and Antigravity. Claude and Antigravity weekly remaining comes from local
  observations; Codex and Grok account quota requires separate network opt-in.

### Fixed

- Match Codex context UI percentages by subtracting the 12,000-token baseline
  from both usage and capacity, returning unavailable for windows at/below it.

- Bound the caller wait for stalled reader I/O and discard queued usage after
  the reader deadline.
- Resolve trusted font roots before rejecting descendant symlinks.
- Serialize sidebar configuration transactions.
- Keep accepted control sockets blocking on macOS, where accept inherits the
  listener's O_NONBLOCK.
- Reconnect after each Herdr JSON response. Herdr v0.9.3 closes the socket
  after `session.snapshot` and `report_metadata`.
- Apply socket deadlines with non-blocking I/O and `poll`. On macOS,
  `setsockopt` returns `EINVAL` after the peer closes the socket.
- Unlock font action locks on drop so an inherited descriptor cannot keep
  `flock` after the action returns.
- Run the session lock-release check in its own test binary so a concurrent
  fork cannot inherit that lock's file descriptor.

[Unreleased]: https://github.com/fujimogn/herdr-lineup/commits/main
