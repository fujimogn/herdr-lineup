# herdr-lineup

One sidebar row per AI coding agent in [Herdr](https://herdr.dev): vendor logo,
state, context, weekly quota, and when each repository last shipped.

A per-session daemon publishes `lineup_` sidebar metadata. Usage numbers come
from local session files and opt-in statusLine commands, not from provider
APIs. `--help` and `--version` work.

## Install

Requires macOS or Linux, Herdr 0.9.0 or later, and Rust/Cargo. The build uses the
toolchain pinned in `rust-toolchain.toml` (Rust 1.99.0); install it with rustup.

**Planned public installation**, after the repository is published:

```sh
herdr plugin install fujimogn/herdr-lineup
```

Herdr builds the plugin with `cargo build --release --locked` during installation.
A public install is not published yet. A local release build can publish rows
after `ensure`. See Development to build the current code locally.

Three install facts from the Herdr plugin documentation, worth knowing before
you rely on this plugin:

- Reinstalling replaces the managed checkout (`HERDR_PLUGIN_ROOT`). Never store
  state there; the plugin keeps runtime state in `HERDR_PLUGIN_STATE_DIR` and
  user settings in `HERDR_PLUGIN_CONFIG_DIR`.
- There is no `herdr plugin update`. To upgrade, install again.
- Pin a version with a release tag:
  `herdr plugin install fujimogn/herdr-lineup --ref vX.Y.Z`.

## Usage

The current CLI exposes:

```sh
cargo run --locked -- --help
cargo run --locked -- doctor --help
cargo run --locked -- statusline --help
```

| Command | Behavior |
| --- | --- |
| `ensure` | Start one detached publisher per Herdr session if needed. |
| `daemon` | Run the publisher in the foreground. |
| `stop` | Stop the plugin publisher, leaving the Herdr server running. |
| `doctor` | Check local setup and explain missing data. |
| `configure` | Opt in to adding plugin sidebar rows. |
| `unconfigure` | Remove only the plugin-managed sidebar rows. |
| `statusline` | Record Claude or Antigravity usage for the sidebar. Prints nothing unless `--then` runs an existing command. |

Herdr actions: `herdr-lineup.start`, `herdr-lineup.stop`,
`herdr-lineup.doctor`, `herdr-lineup.configure`, `herdr-lineup.unconfigure`,
`herdr-lineup.install-font`, and `herdr-lineup.uninstall-font`.
For example:

```sh
herdr plugin action invoke herdr-lineup.start
herdr plugin action invoke herdr-lineup.doctor
```

## Configuration

User settings live in `$HERDR_PLUGIN_CONFIG_DIR/config.toml`. A missing file
uses the built-in defaults and is not created. Find the directory of an
installed or linked plugin with:

```sh
herdr plugin config-dir herdr-lineup
```

`non_first_pane_logo_indent` is an opt-in integer from 0 to 16 (default 0).
It prefixes only `lineup_logo` on the second and later panes in each workspace
with that many U+2800 BRAILLE PATTERN BLANK characters, each occupying one
terminal column. Use this when your sidebar places the repository heading on
the first pane's first row: Herdr v0.9.3 indents later entry rows two columns
more deeply, so `2` aligns the agent rows:

```toml
non_first_pane_logo_indent = 2
```

The first pane follows the order in `session.snapshot`'s `panes` array within
each workspace, including panes without a supported agent. Each refresh
recomputes that order. A value of 0 preserves the original token bytes; no
other token changes. Missing or unsupported logos remain cleared.

Sidebar rows are added by hand or through the explicit, opt-in `configure`
action. Startup does not edit the user's Herdr configuration. Configuration
edits use marker comments and a backup, preserving unrelated settings.
`unconfigure` removes only an unmodified plugin-managed block.

The plugin bundles the HerdrAgentIconsMax icon font and can emit its icon
codepoints. The font is installed only through the explicit, opt-in
`herdr-lineup.install-font` action and removed with
`herdr-lineup.uninstall-font`; startup never touches font setup. Text labels
remain available, and automatic mode stays on text until terminal glyph support
is known. See [NOTICE.md](NOTICE.md) for sources and licensing.

## How it works

Rows combine vendor logos, agent state, context use, and weekly remaining
quota for Claude, Codex, Grok, Antigravity, Kimi, and Hermes. Unavailable usage
shows `—`. [Source research](docs/research/usage-sources.md) records where each
number can come from.

Context values come from Codex, Grok, Claude, and Antigravity. Weekly remaining
comes from Claude and Antigravity. Kimi and Hermes stay unavailable, and Codex
and Grok do not supply weekly quota.

Each agent row shows the vendor logo, a state dot, context use as a percentage,
and a weekly quota triplet such as `7d 67% 6d9h`: the quota period, the share
remaining, and the time until the quota resets. Each workspace heading shows
the repository name and the newest local `origin/main` commit time.

Codex and Grok context are read from local session files for the pane's native
session. Claude and Antigravity usage is not read from those agents' config or
credential files. Each agent sends it by running this plugin as its statusLine
command:

```sh
herdr-lineup statusline claude
herdr-lineup statusline agy
```

Usage is shown in the sidebar only. `herdr-lineup statusline` does not render
a status line and does not print a pace label. `herdr-lineup statusline --help`
accepts `claude` or `agy`, plus `--then <command>`. The command reads one JSON
object from stdin. It stores only the context percentage, the weekly remaining
percentage, an optional reset time, the agent, an opaque binding key, and the
observation time under `$HERDR_PLUGIN_STATE_DIR/statusline/`. It does not store
conversation text, transcript paths, or credentials. Invalid JSON, an oversized
payload, or a failed write does not stop the agent's status line. With
`--then`, the shell command receives the original stdin bytes, and its stdout
and exit status are kept, including when the input is broken. That stdout is
the chained command's output alone. Without `--then`, this command prints
nothing and exits 0.

Claude's binding key is the stdin field `session_id`, matched to the pane's
native session. Antigravity's binding key is `HERDR_PANE_ID`, matched to the
pane. Antigravity ignores any conversation id in the JSON. Herdr panes provide
`HERDR_PANE_ID`.

The statusLine process and the session daemon must use the same absolute
`HERDR_PLUGIN_STATE_DIR`. Herdr sets that variable for plugin processes. A
statusLine command runs inside the agent, so the agent's environment has to
provide the same directory. Do not point it at `HERDR_PLUGIN_ROOT`. The release
binary is `target/release/herdr-lineup` in the plugin directory; `herdr-lineup`
must be on the agent's `PATH`. Startup does not edit agent settings. Hook
installation is opt-in.

Claude Code reads the command from `~/.claude/settings.json` or from the
project's Claude settings. This keeps an existing status line:

```json
{
  "statusLine": {
    "type": "command",
    "command": "herdr-lineup statusline claude --then '<existing statusLine command>'"
  }
}
```

Omit `--then` when there is no existing command:

```json
{
  "statusLine": {
    "type": "command",
    "command": "herdr-lineup statusline claude"
  }
}
```

Antigravity CLI uses the same shape in `~/.gemini/antigravity-cli/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "herdr-lineup statusline agy --then '<existing statusLine command>'"
  }
}
```

Set `HERDR_PLUGIN_STATE_DIR` in the environment that launches the agent, using
the plugin state directory the daemon already uses:

```sh
export HERDR_PLUGIN_STATE_DIR="/path/to/herdr-lineup/state"
```

Numbers stay `—` until a fresh observation matches the pane. The default
maximum age is 60 seconds.

The repository heading reads the newest local `origin/main` commit without
fetching: `HH:MM` for today, `M/D` for earlier dates. This is a commit-time label,
not proof of a deployment.

### Sidebar tokens

All tokens use source `herdr-lineup`. Pane reports explicitly target a pane;
workspace reports explicitly target a workspace. Missing numeric values and
inactive state dots are cleared with JSON null.

| Pane token | Value |
| --- | --- |
| `lineup_agent` | Supported lowercase agent id |
| `lineup_logo` | Vendor glyph or text label |
| `lineup_state` | `idle`, `working`, `blocked`, `done`, or `unknown` |
| `lineup_state_idle` | `●` while idle, otherwise cleared |
| `lineup_state_working` | `●` while working, otherwise cleared |
| `lineup_state_blocked` | `●` while blocked, otherwise cleared |
| `lineup_state_done` | `●` while done, otherwise cleared |
| `lineup_state_unknown` | `●` while unknown, otherwise cleared |
| `lineup_context_pct` | Unit-less context percentage, or cleared |
| `lineup_context_text` | Rounded context percentage, or `—` |
| `lineup_weekly_pct` | Unit-less weekly remaining percentage, or cleared |
| `lineup_weekly_text` | `7d N%` with optional reset countdown, or `—` |
| `lineup_group` | Workspace repository name on its first pane, otherwise cleared |
| `lineup_group_ship_text` | Workspace ship-time text on its first pane, otherwise cleared |

| Workspace token | Value |
| --- | --- |
| `lineup_repo` | Repository name, or `—` outside Git |
| `lineup_ship_unix` | Local `origin/main` commit timestamp, or cleared |
| `lineup_ship_text` | `HH:MM` today, otherwise `M/D`, or `—` |

The two `lineup_group` tokens copy the workspace heading onto its first pane
for sidebar layouts that place repository headings in the Agents section.
They use the same snapshot order as logo indentation, including panes without
supported agents. Every refresh clears the old first pane and updates the new
one after a reorder. Workspaces without a resolved repository clear both pane
heading tokens. A resolved repository without `origin/main` keeps its name
and uses `—` for its ship-time text. The full pane contract has 14 keys, within
Herdr's 16 keys per report and 32 stored keys per resource; unrelated keys are
preserved and counted before publication.

Herdr startup hooks run once. Startup and `workspace.focused` therefore both
invoke `ensure`; the per-session lock prevents duplicate daemons.
The publisher keeps one daemon per Herdr session and stores state under
`$HERDR_PLUGIN_STATE_DIR`. Herdr v0.9.3 closes the socket after each
`session.snapshot` and `report_metadata` JSON response. An isolated session
confirmed that close, so the daemon connects again for every request. Before
each connect it reads the socket's device and inode and stops without
connecting if either value has changed since startup, so it does not attach
to a replaced server. Deadlines use
non-blocking I/O and `poll` against one monotonic deadline. On macOS,
`setsockopt` on a socket the peer has closed returns `EINVAL`, so the deadline
is not applied with `setsockopt`.
See the [Herdr plugin documentation](https://herdr.dev/docs/plugins/)
and [SECURITY.md](SECURITY.md).

## Troubleshooting

- **A command fails:** it prints an error and exits nonzero. `stop` leaves the
  Herdr server running.
- **No sidebar data after install or link:** confirm `ensure` is running for
  that session's socket and that the socket device and inode still match the
  daemon started against it. A replaced socket stops the old daemon.
- **Local link cannot find the binary:** build first; `herdr plugin link` does not build.
- **Claude or Antigravity usage stays `—`:** confirm the statusLine command is
  `herdr-lineup statusline claude` or `herdr-lineup statusline agy`, that
  `HERDR_PLUGIN_STATE_DIR` is the same absolute directory the daemon uses, and
  that Claude's `session_id` or Antigravity's `HERDR_PANE_ID` matches the pane.
- **Icon rendering shows boxes:** run the opt-in
  `herdr-lineup.install-font` action and check the terminal's codepoint map,
  or use the planned text fallback.
- **A build fails:** verify rustup can install the pinned toolchain and that Cargo
  can download the locked dependencies. No credentials or remote API setup are
  required by the current executable.

Do not stop or restart the Herdr server to troubleshoot this plugin.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
target/release/herdr-lineup --help
```

Optional local registration, after building:

```sh
herdr plugin link .
```

Linking registers the manifest. Build the release binary before expecting
commands to run.
The CI workflow runs formatting, Clippy, tests, and release builds on Linux and
macOS, with a dependency audit on pushes and weekly. Keep package versions in
`Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml` aligned; release tags use `v*`.
Documents and comments are written in English. See [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE). Third-party font attribution is in [NOTICE.md](NOTICE.md).

Not affiliated with Herdr or the supported AI providers.
