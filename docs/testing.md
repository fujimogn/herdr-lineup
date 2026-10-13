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
text is `—` followed by two U+2800 cells; missing weekly text is an unpadded
`—`. Numeric tokens are absent. Output is
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

The recorded snapshots below predate context-column padding. Current missing
context text is `—` followed by two U+2800 cells.

## Publication evidence on the transport fix

On Linux with Herdr 0.9.3, the isolated harness passed against
`8ab7342` (the harness commit applied to transport fix
`52f42f71f29d4d650fff728c6a1505c2808b93be`, before that fix landed on main).
The private Git clone's local `origin/main` timestamp was `1791801285`.
Only display tokens are reproduced below; no private paths or native sessions
are included:

```json
{
  "workspace": {
    "lineup_repo": "lineup-e2e",
    "lineup_ship_unix": "1791801285",
    "lineup_ship_text": "19:34"
  },
  "synthetic_pane": {
    "lineup_agent": "codex",
    "lineup_logo": "Codex",
    "lineup_state": "idle",
    "lineup_state_idle": "●",
    "lineup_context_text": "—",
    "lineup_weekly_text": "—"
  }
}
```

After sixteen seconds, the second snapshot retained the expected values;
workspace sequence advanced from `1` to `3` and pane sequence from `2` to `3`
in the isolated checkpoint. `doctor` reported a healthy daemon, two `stop`
calls passed, and the subsequent snapshot contained no `lineup_` tokens.
The trap confirmed deletion of the isolated session and private temporary
directory. Real-agent pane verification was `SKIP`: the isolated HOME had no
provider authentication. The synthetic pane is publication evidence only.
This does not establish macOS behavior, real-agent usage, sidebar rendering,
or shipment of the transport fix. An older main-based rerun is recorded below.
The current rerun follows it.

## Main-based rerun

The required single Linux runtime rerun passed against `c58799a1a18b0f7f52afa505d0bcf42fbc1c4735`,
with the two harness/documentation commits applied to main
`697daefe77733069e10a8aa54c504ff9eea05166`. Its workspace snapshot contained
`lineup_repo="lineup-e2e"`, `lineup_ship_unix="1791802881"`, and
`lineup_ship_text="20:01"`. Synthetic pane tokens matched the example above,
including absent numeric usage tokens and `—` for context and weekly text.

The snapshot after sixteen seconds retained the expected tokens. The isolated
checkpoint advanced from workspace `1` / pane `2` to workspace `3` / pane `3`.
Doctor, idempotent stop, and the post-stop snapshot with no `lineup_` keys all
passed. The trap confirmed isolated session and temporary-directory removal.
Real-agent verification remained `SKIP` for the unauthenticated isolated HOME;
no credentials, real provider sessions, or authenticated API calls were used.
This rerun establishes Linux metadata behavior at that older commit; it adds no
macOS runtime or visual rendering evidence.

## Current main rerun

On 2026-10-13, on Linux with Herdr 0.9.3, the checks that do not attach to a
user's session were re-run against
`bb4d362d4aa9fd690b6185e71dfca6a8bbc3aa35`, including the harness update in
this change.

`bash -n scripts/e2e-isolated.sh`, `bash scripts/e2e-isolated.sh --self-test`,
and `shellcheck scripts/e2e-isolated.sh` passed. The self-test rejected a
main-session socket and a symlink into that session, accepted an isolated
path, and cleared inherited `HERDR_*` variables. It also rejected a bare em
dash for `lineup_context_text`.

`bash scripts/e2e-isolated.sh` and `bash scripts/e2e-isolated.sh --tokens`
each created a private workspace and root pane. Repeated and concurrent
`ensure` kept a single daemon PID. Publication, the sixteen-second renewal,
doctor, idempotent stop, and the post-stop snapshot with no `lineup_` tokens
all passed. The exit trap deleted the isolated session and its private
temporary directory. Real-agent verification remained `SKIP` for the
unauthenticated isolated HOME.

An earlier rerun the same day stopped at publication. The harness still
expected a bare em dash for `lineup_context_text`, while `pane_tokens` has
published U+2014 followed by two U+2800 cells for missing context since
`40ff85d`, before v0.1.0. That failure was a stale harness expectation, not a
product regression.

On the same base, `cargo fmt --all --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`,
and `target/release/herdr-lineup --help` passed. The library tests were 265
passed and 2 ignored; the lifecycle integration test passed. The release
binary used by the harness was built with `cargo build --release --locked`.

Not re-run: macOS on a real machine, an authenticated provider pane, real-agent
usage, and visual sidebar or font rendering. The isolated home has no provider
authentication, so those checks stay outside this rerun.
