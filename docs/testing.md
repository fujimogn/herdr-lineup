# Isolated runtime testing

Run the harness from a checkout with the pinned Rust toolchain, Herdr 0.9.3 or
later, Bash, and Python 3 available. It builds the release binary with the lock
file and creates a headless session without attaching a client:

```sh
bash scripts/e2e-isolated.sh --self-test
bash scripts/e2e-isolated.sh
```

The self-test needs no running Herdr server. It rejects a main-session socket
and a symlink into main, accepts an isolated path, and checks that even an
unknown inherited `HERDR_*` variable is removed. `shellcheck
scripts/e2e-isolated.sh` and `bash -n scripts/e2e-isolated.sh` check the harness.

The runtime test creates a unique `lineup-e2e-<UTC time>-<PID>` session under a
private temporary HOME. Its XDG directories, Herdr configuration, workspace,
plugin state, and plugin settings are temporary too. Pane shells use `/bin/sh`
without login startup; background version and manifest checks are disabled.
No real coding agent or authenticated provider API is used. The daemon sees
empty Codex and Grok homes rather than the user's provider files.

Every Herdr command removes **all** inherited exported `HERDR_*` variables with
`env -u`, then explicitly selects `--session <unique name>`. Plugin commands
receive only the discovered isolated socket, isolated session name, and private
state/settings directories. Before each command, a canonical-path check rejects
endpoints outside the isolated session, redirected session directories, and
the user's main-session directories (including symlink aliases). The harness
does not install or link plugins or edit the user's configuration. Changing the
session name or injecting a socket override is intentionally unsupported.

The test creates a workspace and its root pane, runs `ensure`, repeats `ensure`
concurrently, requires exactly one unchanged daemon PID record, runs `doctor`,
then runs `stop` twice and checks that the PID record has gone. PID records are
diagnostic only; the harness never sends a signal to a daemon PID.

Metadata publication is a separate, explicit check. After publication wiring
has landed in the checkout being tested, run:

```sh
bash scripts/e2e-isolated.sh --tokens
```

This reports a synthetic idle Codex occupant on the temporary pane, polls
`herdr --session <unique name> api snapshot` for a `lineup_` metadata key, and
uses a ten-second polling budget. It starts no Codex process and supplies
no native provider session. Snapshot contents remain private and are not printed.
The default lifecycle run prints `SKIP` for this check; lifecycle success alone
does not establish metadata publication or visible sidebar rendering.

An exit trap stops the plugin through its control endpoint, runs explicitly
targeted `session stop` and `session delete`, verifies the session directory is
gone, and removes only this invocation's private temporary directory. SIGINT
and SIGTERM use the same cleanup. If an endpoint safety check or shutdown fails,
the harness fails and retains the directory, printing its path for investigation.
It never falls back to main or uses an unqualified `server stop`. SIGKILL and
machine failure cannot execute the trap; inspect the retained private directory
and use its isolated HOME/config and exact session name for recovery.

Before handing off code changes, also run:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
target/release/herdr-lineup --help
```

Record the tested commit and distinguish lifecycle, metadata, and visual proof.
Never stop, restart, or attach to a user's Herdr session for verification.

## Baseline evidence

On Linux with Herdr 0.9.3, the lifecycle run against the implementation at
`c8b42f96059c150f6a987535a65f333d9c91dbe5` passed workspace/pane creation,
repeated and concurrent ensure with one PID, doctor, and two stops. The exit
trap confirmed session deletion and temporary-directory removal. The optional
token run failed as expected on that pre-publication baseline and still cleaned
up its session and temporary directory. These results do not establish macOS
runtime behavior or metadata publication; rerun against the wired checkout.
