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
plugin state, and plugin settings are temporary too. The workspace cwd is a
private local clone named `lineup-e2e`, with `origin/main` set to the tested
checkout's locally known ref. Cloning never fetches from a remote. Pane shells
use `/bin/sh`
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
concurrently, requires exactly one unchanged daemon PID record, validates
publication, runs `doctor`, then runs `stop` twice and checks that the PID
record has gone. PID records are diagnostic only; the harness never sends a signal to a daemon PID.

Metadata checks run by default; `--tokens` remains an accepted alias:

```sh
bash scripts/e2e-isolated.sh --tokens
```

The harness reports a **synthetic** idle Codex occupant on the private pane.
It supplies no native session, launches no agent, reads no authentication, and
calls no provider API. A real authenticated agent pane is explicitly `SKIP`
because the isolated HOME has no provider authentication; the synthetic check
is separate evidence of publication and missing-value presentation.

Within twelve seconds, snapshots from
`herdr --session <unique name> api snapshot` must show the expected repository
name, the exact local `origin/main` commit timestamp, its local date/time label,
and the synthetic pane's logo, state, context, and weekly text. Missing context
and weekly values must show `—`, with their numeric tokens absent. Output is
limited to `lineup_` display tokens; paths and full snapshots are not printed.

A second snapshot is taken after sixteen seconds, longer than the default
15,000 ms TTL. It must still contain the expected tokens. The isolated plugin
state's `sequences.json` is sampled before and after that wait; the tested
workspace and pane sequences must both advance. Herdr snapshots expose token
values, not the publisher's per-source sequence. A checkpoint alone records
reserved sequences and does not prove an accepted report; the later snapshot
provides the complementary evidence that tokens remain live after their first
TTL. This checks the default 5,000 ms refresh without changing settings.

After `stop`, another snapshot must contain no `lineup_` keys on any pane or
workspace. Other plugins' keys are allowed. Self-tests exercise token rejection,
sequence progression, and detection of uncleared tokens without a real socket.
These checks establish metadata publication, not visible sidebar/font rendering.

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
