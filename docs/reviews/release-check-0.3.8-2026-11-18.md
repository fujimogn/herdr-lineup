# Linux release checks for 0.3.8 (2026-11-18)

All eleven commands in [Releasing, Step 4](../releasing.md#steps) exited 0 on
release commit `be603f25b8b0248c5d5a9f025b46d3ec83131a04`.

## Execution boundary

- Source commit `be603f25b8b0248c5d5a9f025b46d3ec83131a04` declares version 0.3.8.
  The local `origin/develop` ref matched this SHA at worktree creation and
  after the checks.
- Dedicated linked worktree
  `/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/release-check-038`.
- Host was Linux `7.3.3-1-cachyos`, x86_64, with Herdr 0.9.5, Python 3.15.2,
  `rustc 1.100.0 (0ef1bc386 2026-11-09)`, and
  `cargo 1.100.0 (3acc79c0c 2026-11-05)`.
- Execution window was 17:18:00–17:21:15 UTC on 2026-11-18
  (02:18:00–02:21:15 JST on 2026-11-19).
- The commands ran sequentially with Cargo's target directory set to this
  worktree's `target`. The recorder checked HEAD and clean Git status before
  each command and after command 11. This record was added after the checks.

The ignored `target/release-check-038/` directory holds `results.json` with
each command, source SHA, UTC start and finish, elapsed time, and exit code.
Combined stdout and stderr are in `01.log` through `11.log`.

## Commands and results

| # | Command | Exit | Key output or result |
| --- | --- | --- | --- |
| 1 | `.develop-tools/resolve ci check-release-version` | 0 | No output. `Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml` declare 0.3.8 at the tested SHA. |
| 2 | `scripts/extract-release-notes.sh 0.3.8` | 0 | Printed `## 0.3.8 (2026-11-18)`, a summary on reducing filesystem checks for unchanged Kimi session state, and one maintenance bullet on reusing verified bindings. |
| 3 | `scripts/test-check-release-gate.sh` | 0 | `release gate tests passed`. The test uses fake Git and GitHub commands. |
| 4 | `cargo fmt --all --check` | 0 | No output. |
| 5 | `cargo clippy --locked --all-targets -- -D warnings` | 0 | Checked `herdr-lineup v0.3.8` and finished the dev profile in 26.39 s without warnings or errors. |
| 6 | `cargo test --locked` | 0 | Library recorded 490 passed, 0 failed, and 6 ignored. Lifecycle integration recorded 1 passed and 0 failed. Binary unit tests and doc tests each recorded 0 tests. |
| 7 | `cargo build --release --locked` | 0 | Compiled `herdr-lineup v0.3.8` and finished the optimized release profile in 1 min 5 s. |
| 8 | `target/release/herdr-lineup --help` | 0 | Printed `Usage: herdr-lineup <COMMAND>`, the command list, and help and version options. |
| 9 | `python3 scripts/generate-third-party-licenses.py --check` | 0 | `THIRD-PARTY-LICENSES.md matches pinned crate license texts`. |
| 10 | `bash scripts/e2e-isolated.sh --self-test` | 0 | Passed token and checkpoint validation, main and symlink endpoint rejection, safe-path acceptance, and removal of inherited `HERDR_*` variables. The two endpoint refusal lines came from the negative checks. |
| 11 | `bash scripts/e2e-isolated.sh` | 0 | Passed private workspace and pane creation, concurrent ensure with one PID, synthetic token publication, TTL renewal, doctor, stop and idempotent stop, token removal, and private session cleanup. The real-agent pane check was skipped. |

The library run passed eleven `reader_kimi::tests::unchanged_trial_*` cases,
including native binding switches, session directory changes, wire appends,
and atomic replacements. The six ignored library tests are outside the pass
count.

## Isolated runtime observations

Command 11 used the [isolated harness](../testing.md) with a private home,
configuration, clone, and socket. The harness operated on its own
`lineup-e2e-*` session and stopped and deleted it during cleanup. Existing user
sessions were outside the test boundary.

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

Doctor reported a compatible Herdr JSON API, valid configuration, and a healthy
daemon, control endpoint, and lock. The optional Codex, Grok, and Kimi weekly
queries were disabled. The checkpoint floor stayed at 32 across the renewal
observation. The retained tokens after the 31-second wait are the renewal
evidence.

## Verification limits

- macOS build, test, and runtime behavior remain unverified by this Linux run.
  The license check covers the pinned texts for the script's target set.
- Step 5's `.develop-tools/resolve release release check` and
  `.github/workflows/platform-check.yml` are separate release gates. This record
  provides no workflow result or confirmation that both remote branches point
  to the release commit.
- The synthetic occupant covers token publication and test-session lifecycle.
  Authenticated providers, real-agent behavior, visual sidebar and font
  rendering, and behavior in existing user sessions remain unverified.
- Tagging, pushes, workflow dispatch, and publication are outside this record.
