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

### Fixed

- Bound the caller wait for stalled reader I/O and discard queued usage after
  the reader deadline.
- Resolve trusted font roots before rejecting descendant symlinks.
- Serialize sidebar configuration transactions.
- Keep accepted control sockets blocking on macOS, where accept inherits the
  listener's O_NONBLOCK.

[Unreleased]: https://github.com/fujimogn/herdr-lineup/commits/main
