# Linux release checks for 0.3.6 (2026-11-18)

All eleven commands in [Releasing, Step 4](../releasing.md#steps) ran on
`88ff31ed310dc00407ef0a0bd00fd7d751733eae` and exited 0. This records local
Linux verification only; it does not approve tagging or establish release
publication, macOS runtime behavior, or completion of the platform release check.

## Execution boundary

- Source: version 0.3.6 at `88ff31ed310dc00407ef0a0bd00fd7d751733eae`, which
  matched `origin/develop` when the worktree was created.
- Dedicated worktree:
  `/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/release-check-036`.
- Host: Linux `7.3.3-1-cachyos`, x86_64 GNU/Linux; Herdr 0.9.5;
  Python 3.15.2. The pinned compiler was
  `rustc 1.100.0 (0ef1bc386 2026-11-09)`.
- Execution window: 2026-11-18, 02:29:58–02:32:38 UTC
  (11:29:58–11:32:38 JST).
- Commands ran sequentially. Before each, the recorder checked that HEAD was
  the full source SHA above and `git diff --exit-code` succeeded. The checkout
  was still at that SHA with no tracked changes after command 11. This record
  was written afterward; its commit changes documentation only.
- An initial recorder setup syntax error occurred before any Step 4 command
  launched. The recorder was corrected first; none of the eleven checks was
  retried or fixed, and no product source was changed.

The ignored local evidence directory is `target/release-check-036/` in that
worktree. `results.json` records each command, source HEAD, UTC start, elapsed
time and exit code. Combined stdout/stderr is retained as `01.log` through
`11.log`, matching the numbered commands below. These local artifacts are not
part of the documentation commit.

## Commands and results

| # | Command | Exit | Key output / result |
| --- | --- | --- | --- |
| 1 | `.develop-tools/resolve ci check-release-version` | 0 | No stdout/stderr. The version check completed successfully; `Cargo.toml`, `Cargo.lock` and `herdr-plugin.toml` at the source SHA declare 0.3.6. |
| 2 | `scripts/extract-release-notes.sh 0.3.6` | 0 | Printed `## 0.3.6 (2026-11-18)`, the release summary, and the Codex header tests, foreground startup tests, Kimi wiring tests, dependencies and OpenSpec CLI bullets. |
| 3 | `scripts/test-check-release-gate.sh` | 0 | `release gate tests passed`. This uses the script's fake Git/GitHub commands; it is not a check of a real release tag or GitHub CI run. |
| 4 | `cargo fmt --all --check` | 0 | No stdout/stderr; formatting check passed. |
| 5 | `cargo clippy --locked --all-targets -- -D warnings` | 0 | Checked `herdr-lineup v0.3.6`; finished the dev profile without warnings or errors. |
| 6 | `cargo test --locked` | 0 | Library: 468 passed, 0 failed, 6 ignored. Lifecycle integration: 1 passed, 0 failed. Binary tests and doc tests: 0 tests. |
| 7 | `cargo build --release --locked` | 0 | Compiled `herdr-lineup v0.3.6`; `Finished release profile [optimized]`. |
| 8 | `target/release/herdr-lineup --help` | 0 | Printed the description, `Usage: herdr-lineup <COMMAND>`, command list and options. |
| 9 | `python3 scripts/generate-third-party-licenses.py --check` | 0 | `THIRD-PARTY-LICENSES.md matches pinned crate license texts`. This checks the locked license closure for the script's Linux/macOS target set; it does not execute a macOS binary. |
| 10 | `bash scripts/e2e-isolated.sh --self-test` | 0 | Passed token and checkpoint validation, main/symlink endpoint rejection, safe-path acceptance and clearing all inherited `HERDR_*`. The two endpoint-refusal stderr lines are expected negative checks. |
| 11 | `bash scripts/e2e-isolated.sh` | 0 | Passed private workspace/pane creation, concurrent ensure with one PID, synthetic publication, TTL renewal, doctor, stop/idempotent stop, post-stop token removal and isolated session/temp-directory cleanup. Real-agent pane verification was explicitly `SKIP`. |

The successful library run includes
`foreground_wrapper_reports_private_runtime_setup_errors`,
`kimi_production_wiring_covers_opt_in_success_rotation_expiry_and_reset`,
`unix_fixture_clears_inherited_nonblocking_mode`, and
`real_herdr_one_response_per_connection_publishes_heading`.
The six ignored library tests were left unchanged and were not counted as
successful executions.

## Isolated runtime observations

Command 11 used the existing harness's private temporary HOME, XDG paths,
configuration, clone, socket and `lineup-e2e-*` session. Its guards select only
that endpoint and clear inherited Herdr variables. No client attached to a
user's session, and no existing/user Herdr server was stopped or restarted.
The harness stopped and removed its own private test session during cleanup.

The key runtime output was:

```text
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

Doctor reported a compatible Herdr JSON API, valid config and healthy
daemon/control/lock. Codex/Grok/Kimi weekly queries were disabled; optional
provider data was unavailable and no reader or network call was made.
The checkpoint floor stayed at 32 across the renewal observation; advancement
is not required within a reserved window. The harness waits 31 seconds before
the renewal snapshot, beyond the default 30-second TTL. These are one synthetic
Linux observation's results, not performance or production reliability claims.

## What this does not establish

- No macOS build, test or runtime observation was performed. In particular,
  Linux fixture success does not prove that the historical macOS Kimi failure
  is eliminated on macOS.
- `.develop-tools/resolve release release check` was not run and
  `.github/workflows/platform-check.yml` was not dispatched. The local Linux
  commands do not substitute for the release-commit Linux/macOS platform check
  described in Step 5, or establish the required remote-ref and CI conditions.
- No tag was created, no branch was pushed, no workflow was dispatched, and no
  GitHub Release or marketplace result was checked by this lane.
- The isolated synthetic occupant establishes metadata publication and
  lifecycle behavior only. It does not establish authenticated provider quota
  accuracy, real-agent usage, visual sidebar/font rendering, or behavior in a
  user's existing session. The harness's explicit real-agent `SKIP` remains.

The record and its independent review are separate from commander ACK,
integration, release preflight, tagging and shipment.
