# Linux release checks for 0.3.7 (2026-11-18)

All eleven commands in [Releasing, Step 4](../releasing.md#steps) exited 0 on
release commit `2d6c0617ac7fe7f3e9771eb962bceba025b8d1b0`. This run covers local
Linux verification.

## Execution boundary

- Source commit: `2d6c0617ac7fe7f3e9771eb962bceba025b8d1b0` (version 0.3.7).
  The locally observed `origin/develop` matched this SHA at worktree creation.
- Dedicated linked worktree:
  `/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/release-check-037`.
- Host: Linux `7.3.3-1-cachyos`, x86_64. Tools: Herdr 0.9.5, Python 3.15.2,
  `rustc 1.100.0 (0ef1bc386 2026-11-09)`, and
  `cargo 1.100.0 (3acc79c0c 2026-11-05)`.
- Execution window: 2026-11-18, 11:17:57–11:23:03 UTC
  (20:17:57–20:23:03 JST).
- The commands ran sequentially with Cargo's target directory set to this
  worktree's `target`. The recorder verified HEAD and the absence of tracked
  changes before each command and after command 11. The worktree was clean
  immediately after the checks. The documentation record was added afterward.

The ignored `target/release-check-037/` directory holds `results.json` with
each command, full source SHA, UTC start and finish, elapsed time and exit code.
Combined stdout and stderr are in `01.log` through `11.log` in the same directory.

## Commands and results

| # | Command | Exit | Key output or result |
| --- | --- | --- | --- |
| 1 | `.develop-tools/resolve ci check-release-version` | 0 | No output. `Cargo.toml`, `Cargo.lock` and `herdr-plugin.toml` declare 0.3.7 at the tested SHA. |
| 2 | `scripts/extract-release-notes.sh 0.3.7` | 0 | Printed `## 0.3.7 (2026-11-18)`, the summary and three bullets covering sidebar headings, foreground startup tests and daemon idle wakeups. |
| 3 | `scripts/test-check-release-gate.sh` | 0 | `release gate tests passed`. The test uses fake Git and GitHub commands. |
| 4 | `cargo fmt --all --check` | 0 | No output. |
| 5 | `cargo clippy --locked --all-targets -- -D warnings` | 0 | Checked `herdr-lineup v0.3.7` and finished the dev profile in 36.62 s without warnings or errors. |
| 6 | `cargo test --locked` | 0 | Library: 479 passed, 0 failed, 6 ignored. Lifecycle integration: 1 passed, 0 failed. Binary tests and doc tests: 0 tests. |
| 7 | `cargo build --release --locked` | 0 | Compiled `herdr-lineup v0.3.7` and finished the optimized release profile in 2 min 39 s. |
| 8 | `target/release/herdr-lineup --help` | 0 | Printed `Usage: herdr-lineup <COMMAND>`, the command list and help/version options. |
| 9 | `python3 scripts/generate-third-party-licenses.py --check` | 0 | `THIRD-PARTY-LICENSES.md matches pinned crate license texts`. |
| 10 | `bash scripts/e2e-isolated.sh --self-test` | 0 | Passed token and checkpoint validation, main and symlink endpoint rejection, safe-path acceptance and removal of inherited `HERDR_*` variables. The two endpoint refusal lines came from the negative checks. |
| 11 | `bash scripts/e2e-isolated.sh` | 0 | Passed private workspace and pane creation, concurrent ensure with one PID, synthetic publication, TTL renewal, doctor, stop and idempotent stop, token removal and temporary session cleanup. Real-agent pane verification was explicitly skipped. |

The library run includes `heading_timeout_retains_only_the_same_workspace_and_occupant`,
`foreground_wrapper_rejects_relative_state_path`,
`foreground_wrapper_rejects_relative_socket_path`,
`dropping_idle_control_worker_wakes_the_listener`, and
`control_stop_wakes_daemon_before_a_distant_refresh`. The six ignored library
tests are excluded from the pass count.

## Isolated runtime observations

Command 11 used the [isolated harness](../testing.md) with a private temporary
home, configuration, clone and socket. It passed its endpoint guards and
operated on its own `lineup-e2e-*` session, including stopping and removing
that session during cleanup. The user's running Herdr sessions were outside
the test boundary.

```text
PASS: private workspace and root pane created
PASS: ensure, concurrent ensure single PID
PASS: workspace heading and synthetic pane display tokens received
SKIP: real agent pane; isolated HOME has no provider authentication; synthetic occupant only
Sequence checkpoint: {"endpoint": 32} -> {"endpoint": 32}
PASS: tokens retained after default TTL; checkpoint floor non-decreasing
Refresh attempts: 5; ok 5; error 0; capacity skip 0
Refresh counts: reports 10; checkpoints 1
PASS: stop and idempotent stop
PASS: all lineup_ tokens removed after stop
PASS: isolated session and temporary directory removed
```

Doctor reported a compatible Herdr JSON API, valid configuration and healthy
daemon, control endpoint and lock. The optional Codex, Grok and Kimi weekly
queries were disabled. The checkpoint floor stayed at 32 across the renewal
observation. The retained tokens after the 31-second wait are the renewal
evidence.

## Verification limits

- This run provides no macOS build, test or runtime result. The third-party
  license check covers the pinned texts for the script's target set.
- Step 5's `.develop-tools/resolve release release check` and its
  `.github/workflows/platform-check.yml` workflow remain unverified by this
  lane. The platform check also requires verification of the release commit
  on the remote refs and its CI result.
- The synthetic occupant covers token publication and test-session lifecycle.
  Authenticated provider behavior, real-agent usage, visual sidebar and font
  rendering, and behavior in an existing user session remain unverified.
- Tagging, pushes, workflow dispatch and publication are outside this record.
