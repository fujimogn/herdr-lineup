# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Fixed

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
