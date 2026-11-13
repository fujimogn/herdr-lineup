# Linux verification for v0.3.5 preparation

All eight assigned development and isolated runtime checks from Step 4 of
[Releasing](../releasing.md) exited 0 on Linux on 2026-11-13.
This is a verification record, not a release approval or evidence of publication.

## Tested source and environment

- Tested commit: `7ffbf9aab87bacac21f05115d3b5b21fa270cf6a`, fetched from
  `origin/develop` before creating a dedicated clean worktree.
- This record was subsequently applied to `origin/develop` at
  `701d71d64c7ca1bd4e7a074a8a7c58fe629e16bb`, whose only difference from the
  tested commit is seven added lines in `CHANGELOG.md`; the code is identical.
- Source package and plugin version: `0.3.4`. These are checks for the upcoming
  v0.3.5 preparation, before the release version and changelog changes land;
  this run does not claim to have tested a final version-bumped release commit.
- OS: Linux `7.3.3-1-cachyos`, `x86_64`, GNU/Linux.
- Pinned compiler: `rustc 1.100.0 (0ef1bc386 2026-11-09)`.
- Herdr: `0.9.5` (the harness requires 0.9.3 or later).
- Commands ran from 14:17:35 UTC through approximately 14:21:55 UTC.
- `CARGO_TARGET_DIR` was explicitly set to this worktree's `target` directory,
  outside `/tmp`. Tests used at most eight threads per test binary.

## Command results

| Command | Exit | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | 0 | Formatting matched. |
| `cargo clippy --locked --all-targets -- -D warnings` | 0 | All targets checked without warnings. |
| `cargo test --locked -- --test-threads=8` | 0 | Main library suite: 465 passed, 0 failed, 6 ignored; resource lifecycle integration test: 1 passed, 0 failed. Binary and doc-test targets had no tests. Nested subprocess fixtures also passed. |
| `cargo build --release --locked` | 0 | Optimized release binary built with the pinned lockfile. |
| `target/release/herdr-lineup --help` | 0 | Printed command and option help successfully. |
| `python3 scripts/generate-third-party-licenses.py --check` | 0 | `THIRD-PARTY-LICENSES.md` matched pinned crate license texts. |
| `bash scripts/e2e-isolated.sh --self-test` | 0 | Main endpoint and symlink redirection rejected; isolated endpoint accepted; inherited `HERDR_*` variables cleared; token and checkpoint validation passed. The two endpoint refusal messages were expected negative-test output. |
| `bash scripts/e2e-isolated.sh` | 0 | Isolated lifecycle and metadata checks passed, including cleanup. Runtime was approximately 44.4 seconds. Its internal locked release build also succeeded. |

The six ignored library tests were subprocess entry points or manual performance
measurements, as declared by the suite; they were not newly skipped failures.

## Isolated runtime observations

The harness created its own headless session, `lineup-e2e-142110-3809875`,
under a private temporary HOME, with private XDG directories, configuration,
state, settings, and a local clone. It did not attach a client to any user
session, stop or restart the user's Herdr server, install or link the plugin,
or change user configuration. Only the harness-owned isolated server/session
was started, stopped, and deleted.

- Workspace and root-pane creation passed.
- Repeated and concurrent `ensure` retained one unchanged daemon PID record.
- Workspace heading and synthetic idle Codex pane display tokens matched.
  Missing context was U+2014 followed by two U+2800 cells; missing weekly
  usage was U+2014, with numeric usage tokens absent.
- The private clone's local `origin/main` was
  `426626298b604b26e67f04b1c033a1575d653b9d`. Its expected ship timestamp was
  `1794561428`, displayed as `18:17` in the harness environment. This heading
  refers to that local ref, not the tested source commit or a deployment.
- Tokens remained present after the 31-second wait, beyond the default
  30-second TTL. The endpoint checkpoint floor remained valid and unchanged:
  `32` to `32`; advancement is not required within one reservation window.
- Doctor reported compatible JSON API, valid configuration, and healthy
  daemon/control/lock. It observed five successful refreshes, zero refresh
  errors, zero capacity skips, ten reports, and one checkpoint.
- Stop and idempotent stop passed; all `lineup_` keys were removed afterward.
- The harness confirmed deletion of its isolated session and private temporary
  directory. No harness session or runtime directory remained from this run.

## Limits and remaining release gates

This Linux run does not establish macOS runtime behavior. The commander will
check macOS build and test results through the release platform check.
Authenticated real-agent pane behavior was explicitly skipped by the harness:
the isolated HOME had no provider authentication and used a synthetic occupant.
No provider API, visible sidebar rendering, or font rendering was verified.
The unchanged checkpoint floor establishes validity and non-regression, not
per-resource sequence ordering or a rendered-latency guarantee.

The Step 4 version check, v0.3.5 release-note extraction, release-gate shell
tests, and verification of the final integrated release commit are outside
these eight assigned commands. This record does not claim those gates passed.
The commander retains responsibility for final version/changelog consistency,
platform checks, release approval, tagging, and publication.

This candidate adds only this record. It changes no implementation, manifest,
lockfile, configuration, or changelog. Build artifacts remain in the dedicated
worktree for commander-managed cleanup; primary command logs are retained
separately for the commander's factual review.
