# herdr-lineup

One sidebar row per AI coding agent in [Herdr](https://herdr.dev): vendor logo,
state, context, weekly quota, and when each repository last shipped.

A per-session daemon publishes `lineup_` sidebar metadata. Usage numbers come
from local session files, opt-in statusLine commands, and explicitly enabled
Codex/Grok account quota queries. `--help` and `--version` work.

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
| `install-font` | Opt in to installing the bundled icon font and Ghostty/kitty codepoint maps. |
| `uninstall-font` | Remove only the icon font and codepoint maps that Lineup installed. |
| `statusline` | Record Claude or Antigravity usage for the sidebar. Prints nothing unless `--then` runs an existing command. |

Herdr actions: `herdr-lineup.start`, `herdr-lineup.stop`,
`herdr-lineup.doctor`, `herdr-lineup.configure`, `herdr-lineup.unconfigure`,
`herdr-lineup.install-font`, and `herdr-lineup.uninstall-font`.
For example:

```sh
herdr plugin action invoke herdr-lineup.start
herdr plugin action invoke herdr-lineup.doctor
herdr plugin action invoke herdr-lineup.configure
herdr plugin action invoke herdr-lineup.unconfigure
herdr plugin action invoke herdr-lineup.install-font
herdr plugin action invoke herdr-lineup.uninstall-font
```

The same commands run directly as `herdr-lineup configure`,
`herdr-lineup unconfigure`, `herdr-lineup install-font`, and
`herdr-lineup uninstall-font`. Startup and focus hooks never run them.

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

`logo_mode` chooses the logo bytes. The default is `auto`.

```toml
logo_mode = "text"
```

| Value | Logos |
| --- | --- |
| `auto` | Text labels until terminal glyph support is affirmatively known. The publisher does not treat font installation as that proof, so `auto` stays on text. |
| `font` | Vendor glyphs (U+E1A0–U+E1BA). This asserts that the icon font and the terminal codepoint map are already configured. |
| `text` | ASCII labels: `Claude`, `Codex`, `Grok`, `Antigravity`, `Kimi`, `Hermes`. |

Any other value fails config validation. Stop and start only the Lineup daemon
after changing it. Do not restart the Herdr server.

These other keys are implemented and optional. Omit `config.toml` to keep the
defaults. Unknown keys fail validation.

| Key | Default | Bounds |
| --- | --- | --- |
| `refresh_interval_ms` | `5000` | Integer 1000–3600000. Sidebar TTL is three times this value. |
| `reader_timeout_ms` | `2000` | Integer 100–30000, and no greater than `refresh_interval_ms`. |
| `usage_max_age_ms` | `60000` | Integer at least `refresh_interval_ms` and at most 86400000. |
| `providers` | `claude`, `codex`, `grok`, `agy`, `kimi`, `hermes` | Unique ids from that list. An empty list publishes no agent rows. Repository headings stay independent. |

```toml
providers = ["claude", "codex", "grok"]
```

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

| Agent | Context | Weekly remaining |
| --- | --- | --- |
| Claude | Opt-in statusLine observation | Opt-in statusLine seven-day observation |
| Codex | Exact-session local rollout, with the 12,000-token UI baseline | Account quota worker when `allow_network = true` |
| Grok | Exact-session local `signals.json` | Shared billing worker when `allow_network = true`; WEEKLY only |
| Antigravity | Opt-in statusLine observation | Active-model weekly pool from statusLine |
| Kimi / Hermes | Unavailable | Unavailable |

Network access defaults to disabled. To opt in, add this to the plugin's
`$HERDR_PLUGIN_CONFIG_DIR/config.toml`, then stop/start only the Lineup daemon:

```toml
allow_network = true
```

This starts `codex app-server` for account limits using the daemon's `CODEX_HOME`
and reads Grok's existing auth key (`GROK_AUTH_FILE`, then `GROK_HOME/auth.json`,
then `~/.grok/auth.json`) for the authenticated HTTPS billing endpoint. No
credentials are displayed or persisted by Lineup. Do not restart the Herdr server.

One background worker per enabled provider polls its client every five minutes,
independently of refresh. Client request intervals are bounded to 1..=60 minutes.
Refresh reads memory only for quota and never waits for a provider request.
`doctor` reads the session state directory and prints the last saved Codex/Grok
query categories (`ok`, `auth_missing`, `auth_io`, `auth_shape`,
`auth_ambiguous`, `unauthorized`, `transient`, `invalid_response`, `request`,
`non_weekly`, `disabled`, or `pending`). `not_observed` means no saved result;
`invalid_diagnostic` means the local record cannot be safely decoded. These are
last query results, not cache availability or a new provider query. Values,
credentials, paths, response bodies and error text are never recorded.
Temporary failures retain the last success until its known reset; authentication
failures clear it. Before the first successful query, after reset, and for Grok
MONTHLY allowances, weekly stays `—`. Period names containing `WEEKLY`
(case-insensitive, including enum prefixes) supply weekly usage; `MONTHLY`
periods remain unavailable for 7d. Without a reset, the remaining percentage
is shown without a countdown and expires after `usage_max_age_ms` from the
last successful query. Environment proxies are inherited; redirects stay disabled. Disabled network access also keeps Codex
and Grok weekly at `—` while preserving local context.

Codex context uses `max(total_tokens - 12000, 0) / (model_context_window - 12000)`.
A window of 12,000 tokens or less is unavailable. The snapshot has no pane-specific
home or profile, so Codex weekly is attributed only to a pane whose exact native
session is verified in the daemon's home. Panes using a different home remain
unavailable. Profiles sharing one home cannot be distinguished; stop/start the
Lineup daemon after changing authentication or profiles to discard cached account
quota. Grok has one configured auth source, so its weekly quota is shared by all
Grok panes, even without local context.

Workers stop scheduling queries on daemon shutdown. Stop does not join a pending
Grok HTTP request (process exit ends it), and it kills/reaps the in-flight Codex
app-server process group. The existing five-second stop deadline is preserved.

Each agent row shows the vendor logo, a state dot, context use as a percentage,
and a weekly quota triplet such as `7d 67% 6d9h`: the quota period, the share
remaining, and the time until the quota resets. Both displayed percentages use
at least two digits (`07%`, `7d 07% 6d9h`). Missing context displays `—` followed
by two U+2800 BRAILLE PATTERN BLANK characters, preserving the three terminal
cells used by `00%` through `99%` after token whitespace normalization.
Rounded `100%` (including values at or above 99.5%) stays four cells so full
context use is visible; the following weekly column moves one cell to the right
in that case. Weekly remaining `100%` similarly uses three digits; unavailable
weekly data stays an unpadded `—` at the end of the row.

Cell widths follow the narrow-ambiguous terminal policy: `—` (U+2014), U+2800,
ASCII digits and `%` each occupy one cell. The
[Unicode East Asian Width data](https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt)
classifies U+2014 as Ambiguous (`A`) and U+2800 as Neutral (`N`). Terminals that
render ambiguous characters as two cells can render `—` two cells wide, so the
missing-context alignment assumes narrow ambiguous rendering. The tests use a
fixed cell-width table for this policy, independent of the host locale.

Each workspace heading shows the repository name and the newest local `origin/main` commit time.

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

Context stays `—` until an observation matches the pane, then retains the last
valid value while that exact session or pane binding matches. Idle context does
not expire by age. Weekly quota with a reset time expires at that time. StatusLine
weekly without a reset uses the default maximum age of 60 seconds; Codex quota
without a reset follows its client interval and clears after a failed query.

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
| `lineup_context_text` | At least two-digit rounded context percentage, or `—` plus two U+2800 cells |
| `lineup_weekly_pct` | Unit-less weekly remaining percentage, or cleared |
| `lineup_weekly_text` | `7d NN%` (at least two digits) with optional reset countdown, or `—` |
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
  or set `logo_mode = "text"`. Automatic mode already uses those text labels
  until terminal glyph support is known.
- **A build fails:** verify rustup can install the pinned toolchain and that Cargo
  can download the locked dependencies. No credentials or remote API setup are
  required for local context; account quota queries need the explicit opt-in above.

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
