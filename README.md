# herdr-lineup

See each AI coding agent's state, context use, and weekly quota in one sidebar
row in [Herdr](https://herdr.dev), alongside each repository's latest local
`origin/main` commit time.

<img src="https://raw.githubusercontent.com/fujimogn/herdr-lineup/v0.1.0/docs/images/sidebar-agents.png" alt="Herdr Agents sidebar showing Lineup agent rows, context use, and weekly quota" width="420">

- **One row per agent.** Keep vendor logos, state, context, and weekly remaining quota together in the sidebar.
- **Session-bound usage.** Read local Codex/Grok files and opt-in Claude/Antigravity statusLine observations for the matching pane.
- **Account quota by choice.** Enable Codex/Grok account queries with `allow_network = true`; network access is off by default.

## Documentation

- [Releasing](docs/releasing.md) — version checks, release notes, and the release workflow.
- [Testing](docs/testing.md) — isolated lifecycle and metadata verification.
- [Usage sources](docs/research/usage-sources.md) — provider inputs and availability.
- [Codex context drift](docs/research/codex-context-drift.md) — evidence for the one-point display gap.
- [Plugin conventions](docs/research/plugin-conventions.md) — packaging and installation research.
- [Security](SECURITY.md) — local data, credentials, and network boundaries.
- [Changelog](CHANGELOG.md) — user-visible changes.

## What it does

Each agent row shows the vendor logo, a Herdr state dot, context use as a percentage,
and weekly remaining quota for Claude, Codex, Grok, Antigravity, Kimi, and
Hermes. A weekly triplet such as `7d 67% 6d9h` shows the quota period, the share
remaining, and the time until reset. Unavailable usage shows `—`.

| Agent | Context | Weekly remaining |
| --- | --- | --- |
| Claude | Opt-in statusLine observation | Opt-in statusLine seven-day observation |
| Codex | Exact-session local rollout, with the 12,000-token UI baseline | Account quota worker when `allow_network = true` |
| Grok | Exact-session local `signals.json` | Shared billing worker when `allow_network = true`; WEEKLY only |
| Antigravity | Opt-in statusLine observation | Active-model weekly pool from statusLine |
| Kimi | Local session file | Unavailable |
| Hermes | Unavailable | Unavailable |

See [Sidebar tokens](#sidebar-tokens) for display formatting and terminal-cell
alignment, and [Source research](docs/research/usage-sources.md) for usage inputs.

## Install

Requires macOS or Linux, Herdr 0.9.3 or later, Git, and
[rustup](https://rustup.rs) with the toolchain pinned in
`rust-toolchain.toml` (Rust 1.99.0). Make sure `cargo` is available on `PATH`.
The source build during installation can take several minutes.

1. Install from the public GitHub repository:

   ```sh
   herdr plugin install fujimogn/herdr-lineup
   ```

   Review Herdr's source and command preview before confirming. The manifest's
   `cargo build --release --locked` command runs after confirmation and before
   registration; if the build fails, Herdr does not register Lineup. To pin the
   v0.1.0 release, add `--ref v0.1.0`. Use `--yes` only when you already trust
   the source and want to skip the interactive confirmation. If you previously
   linked a local checkout, run `herdr plugin unlink herdr-lineup` before
   installing; installation over a local link is refused.

2. In a pane of the Herdr session you want to use, start Lineup:

   ```sh
   herdr plugin action invoke herdr-lineup.start
   ```

3. Check the daemon and read the action's output in the plugin log:

   ```sh
   herdr plugin action invoke herdr-lineup.doctor
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   Look for the completed `doctor` command, its exit status, and the diagnostic
   output. Healthy output includes `daemon/control/lock healthy`. If doctor says
   `daemon is starting; the first refresh has not completed yet`, wait a few
   seconds and run the doctor action again. This initial exit 1 is not a refresh
   failure. Missing optional usage is expected. Next, [configure the sidebar](#sidebar)
   to display the rows.

When installing or upgrading:

- Reinstalling replaces the managed checkout (`HERDR_PLUGIN_ROOT`). Never store
  state there; the plugin keeps runtime state in `HERDR_PLUGIN_STATE_DIR` and
  user settings in `HERDR_PLUGIN_CONFIG_DIR`.
- There is no `herdr plugin update`. To upgrade, install again.
- Pin a version with a release tag:
  `herdr plugin install fujimogn/herdr-lineup --ref v0.1.0`.

The [Herdr marketplace](https://herdr.dev/docs/marketplace/) is an automatic,
unreviewed index of public repositories, not an endorsement or code review.

### Develop from a checkout

Clone and build a checkout that you will keep:

```sh
git clone https://github.com/fujimogn/herdr-lineup.git
cd herdr-lineup
rustup toolchain install
cargo build --release --locked
herdr plugin link .
```

`plugin link` does not build or copy the checkout. Release archives contain a
built binary, `LICENSE`, `NOTICE.md`, `THIRD-PARTY-LICENSES.md`, and `README.md`;
they are not standalone plugin installations. The third-party file includes the
licenses of Rust dependencies in the binary.

## Usage

Inspect the built binary from the checkout (or use `herdr-lineup` after adding
it to your `PATH` as shown in [statusLine setup](#claude-and-antigravity-statusline-opt-in)):

```sh
./target/release/herdr-lineup --help
./target/release/herdr-lineup --version
./target/release/herdr-lineup doctor --help
./target/release/herdr-lineup statusline --help
```

### Common commands

| Command | What it does |
| --- | --- |
| `ensure` | Start one detached publisher per Herdr session if needed. |
| `daemon` | Run the publisher in the foreground. |
| `stop` | Stop the plugin publisher, leaving the Herdr server running. |
| `doctor` | Check local setup, distinguish startup from refresh failure, and explain missing data. |
| `configure` | Opt in to adding plugin sidebar rows. |
| `unconfigure` | Remove only the plugin-managed sidebar rows. |
| `install-font` | Opt in to installing the bundled icon font and Ghostty/kitty codepoint maps. |
| `uninstall-font` | Remove only the icon font and codepoint maps that Lineup installed. |
| `statusline` | Record Claude or Antigravity usage for the sidebar. Prints nothing unless `--then` runs an existing command. |

Herdr actions: `herdr-lineup.start`, `herdr-lineup.stop`,
`herdr-lineup.doctor`, `herdr-lineup.configure`, `herdr-lineup.unconfigure`,
`herdr-lineup.install-font`, and `herdr-lineup.uninstall-font`.
For example, check the daemon and inspect its output:

```sh
herdr plugin action invoke herdr-lineup.doctor
herdr plugin log list --plugin herdr-lineup --limit 10
```

Run these actions from a Herdr pane, and use
`herdr plugin log list --plugin herdr-lineup --limit 10` to inspect their results.
Logs are JSON: find the relevant `command`, its completed `status` and
`exit_code`, then read `stdout` and `stderr`. If it is still running, run the
log command again; continue to the next setup step only after success.
Herdr supplies the plugin environment to action processes; an ordinary pane's
shell does not receive `HERDR_PLUGIN_*`. Direct `install-font` and
`uninstall-font` need absolute `HERDR_PLUGIN_STATE_DIR` and `HOME` paths.
Direct `doctor`, `ensure`, `daemon`, and `stop` additionally need absolute
`HERDR_SOCKET_PATH` and `HERDR_PLUGIN_CONFIG_DIR`.

`configure` and `unconfigure` can also run directly, for example
`./target/release/herdr-lineup configure`: they resolve the Herdr config from
`HERDR_CONFIG_PATH` or `~/.config/herdr/config.toml` and use
`~/.local/state/herdr-lineup` for transaction locks when
`HERDR_PLUGIN_STATE_DIR` is unset or empty. This fallback is for sidebar
transactions, not the daemon's state directory. Startup and focus hooks never
configure the sidebar or install fonts.
Direct `configure` and `unconfigure` need an absolute `HOME` when either
`HERDR_CONFIG_PATH` or `HERDR_PLUGIN_STATE_DIR` is unset.

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
| `auto` | Currently the same ASCII labels as `text`, even after font installation. |
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

### Sidebar

1. Check the target Herdr configuration: `HERDR_CONFIG_PATH` when set, otherwise
   `~/.config/herdr/config.toml`. A relative override is resolved from the
   command's working directory. For actions, this is the plugin's environment,
   not a shell variable exported only in your pane. The action prints its target
   in the log. If the file is a symlink or already defines `ui.sidebar.agents`
   or `ui.sidebar.spaces`, use the manual setup below.

2. Opt in to the managed layout:

   ```sh
   herdr plugin action invoke herdr-lineup.configure
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   This appends a block between `# BEGIN herdr-lineup managed sidebar v1` and
   `# END herdr-lineup managed sidebar v1`, preserving unrelated settings. It
   runs `herdr config check` on a staged file, saves a unique sibling backup
   such as `config.toml.<random>.bak`, replaces the config, and runs
   `herdr server reload-config`. Reload failure triggers restoration and a
   second reload attempt; check the log for any restoration failure.

3. To undo the managed layout:

   ```sh
   herdr plugin action invoke herdr-lineup.unconfigure
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   An unchanged block is removed; a modified or ambiguous block is refused.
   Repeated configure with the same block makes no change. Each actual
   configure/unconfigure change creates another `.bak`; backups are not pruned.
   Keep them until you have checked the result. For a modified block, compare
   the backup with current settings and resolve only the Lineup rows manually.

For managed dotfiles or an existing layout, edit the source configuration
yourself. These are the tables and rows generated by `configure`; merge the
rows into existing tables instead of defining a table twice. Leave out the
ownership markers for manual setup, and undo your manual edits manually:

```toml
[ui.sidebar.agents]
rows = [["$lineup_logo", { token = "$lineup_state_idle", fg = "#808080" }, { token = "$lineup_state_working", fg = "#89b4fa" }, { token = "$lineup_state_blocked", fg = "#f38ba8" }, { token = "$lineup_state_done", fg = "#a6e3a1" }, { token = "$lineup_state_unknown", fg = "#6c7086" }, "$lineup_context_text", "$lineup_weekly_text"]]

[ui.sidebar.spaces]
rows = [["$lineup_repo", "$lineup_ship_text"]]
```

After saving a manual edit, run `herdr config check` and, only if it succeeds,
`herdr server reload-config` with the same `HERDR_CONFIG_PATH` when overridden.
Neither setup method requires restarting the Herdr server.

### Icons

The bundled font is **Herdr Agent Icons Max**. See [NOTICE.md](NOTICE.md) for
sources and licensing. Font actions install on the machine where they run.
Over SSH, installing on the remote host does not install anything on your local
terminal's machine; install the font and configure the terminal there too.

#### Enable icons

1. Install the font and supported terminal mappings explicitly:

   ```sh
   herdr plugin action invoke herdr-lineup.install-font
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   The action lists each changed font, terminal config, and ownership record
   path in its log output. The font file is
   `herdr-lineup-HerdrAgentIconsMax-1b819949.ttf` in
   `~/Library/Fonts` on macOS, or `$XDG_DATA_HOME/fonts` on Linux
   (`~/.local/share/fonts` when no absolute `XDG_DATA_HOME` is set).

2. Run `herdr plugin config-dir herdr-lineup`, then create or edit `config.toml`
   inside the printed directory. Preserve other keys and set this key once:

   ```toml
   logo_mode = "font"
   ```

3. Apply the mode by stopping and starting only the Lineup daemon:

   ```sh
   herdr plugin action invoke herdr-lineup.stop
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   Wait for the stop command to finish successfully before starting again:

   ```sh
   herdr plugin action invoke herdr-lineup.start
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

4. Reload the terminal configuration or reopen the terminal. On Linux, run
   `fc-cache` if needed. The terminal must map **U+E1A0–U+E1BA** to
   **Herdr Agent Icons Max**; other terminals require this mapping manually.
   `auto` currently renders the same text labels as `text`.

Only Ghostty and kitty mappings are automatic. Let `<config>` mean an absolute
`XDG_CONFIG_HOME`, or `~/.config` otherwise. The installer checks all five paths:

| Terminal | Candidate configuration file |
| --- | --- |
| kitty | `<config>/kitty/kitty.conf` |
| Ghostty | `<config>/ghostty/config` |
| Ghostty | `<config>/ghostty/config.ghostty` |
| Ghostty | `~/Library/Application Support/com.mitchellh.ghostty/config` |
| Ghostty | `~/Library/Application Support/com.mitchellh.ghostty/config.ghostty` |

It appends a `# BEGIN herdr-lineup font` / `# END herdr-lineup font` block
only to existing files, leaving other bytes and the primary font unchanged.
It does not create missing terminal configs or backups. An existing symlinked
candidate config (or a symlink beneath a resolved configuration root) makes the
whole action refuse before changing font or terminal bytes; the error names
the symlink path. Use manual terminal
setup for managed dotfiles; the bundled font is at
`assets/fonts/HerdrAgentIconsMax-Regular.ttf` in the checkout. Manual font and
mapping changes must also be removed manually.

The appended mapping is `symbol_map U+E1A0-U+E1BA Herdr Agent Icons Max` for
kitty, or `font-codepoint-map = U+E1A0-U+E1BA=Herdr Agent Icons Max` for Ghostty.

#### Disable icons

1. Change `logo_mode` to `"text"` in the plugin's `config.toml` and repeat the
   Lineup stop/start commands above so no glyphs are published.
2. Remove the owned installation and read the result:

   ```sh
   herdr plugin action invoke herdr-lineup.uninstall-font
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

3. Reload or reopen your terminal (and run `fc-cache` on Linux if needed).

Uninstall removes only recorded, byte-identical blocks and a font file that
Lineup installed. If it removes no owned files, it reports that nothing managed
was removed and still exits successfully. A preexisting identical font is
preserved. If you removed a whole managed block by hand, retry either font
action: it drops the stale
ownership entry and continues. An install interrupted after a terminal write
can also be retried because the ownership record is saved first. A partly
removed or edited block still causes a conflict. Keep
`$HERDR_PLUGIN_STATE_DIR/font-ownership.json`: each `entries` item contains the
config `path` and exact `owned` text, including newlines. Restore that text
exactly once in the matching file, preserving your unrelated edits, then retry
uninstall. If a terminal config is temporarily missing during uninstall, its
ownership entry is retained so its managed block can be removed after the file
returns. Keep the same home, XDG paths, and state directory used for install;
do not delete the ownership record to bypass a conflict.

### Account quota (opt-in)

Network access defaults to disabled. To opt in, add this to the plugin's
`$HERDR_PLUGIN_CONFIG_DIR/config.toml`, then stop/start only the Lineup daemon:

```toml
allow_network = true
```

This starts `codex app-server` for account limits using the daemon's `CODEX_HOME`
and reads Grok's existing auth key (`GROK_AUTH_FILE`, then `GROK_HOME/auth.json`,
then `~/.grok/auth.json`) for the authenticated HTTPS billing endpoint. No
credentials are displayed or persisted by Lineup. Do not restart the Herdr server.

### Claude and Antigravity statusLine (opt-in)

Codex, Grok, and Kimi context are read from local session files. Claude and
Antigravity supply observations through an opt-in statusLine command. Save your
existing agent settings before changing them; startup does not edit them.

1. Find the daemon's actual state directory through its `doctor` action:

   ```sh
   herdr plugin action invoke herdr-lineup.doctor
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   In the completed doctor's output, find `State directory: <absolute path>`.
   Copy the path after the colon exactly. Do not use the plugin config directory
   or `HERDR_PLUGIN_ROOT`. Before launching the agent, export that path in its
   launch environment (replace the placeholder):

   ```sh
   export HERDR_PLUGIN_STATE_DIR="/absolute/path/copied/from/doctor"
   ```

   The statusLine process and daemon must use the same directory. An existing
   agent does not inherit a later shell export; set this in its launch wrapper
   or shell configuration and launch the agent with that environment.

2. Put the binary on the agent's `PATH`. For a GitHub installation, first run
   `herdr plugin list --plugin herdr-lineup --json` and copy the absolute
   `plugin_root` value for `herdr-lineup`. This is Herdr's managed checkout,
   **not** the path printed by `herdr plugin config-dir herdr-lineup`.
   Check that its built binary exists, then create a link if the destination
   does not already exist (replace the placeholder with the copied path):

   ```sh
   LINEUP_ROOT="/absolute/plugin_root/from/plugin-list"
   test -x "$LINEUP_ROOT/target/release/herdr-lineup"
   mkdir -p "$HOME/.local/bin"
   ln -s "$LINEUP_ROOT/target/release/herdr-lineup" "$HOME/.local/bin/herdr-lineup"
   export PATH="$HOME/.local/bin:$PATH"
   herdr-lineup --version
   ```

   Inspect an existing destination instead of overwriting it. Persist the `PATH`
   change in the environment that launches the agent. Reinstalling replaces the
   managed checkout: run `herdr plugin list --plugin herdr-lineup --json` again
   after an upgrade, check `plugin_root` and `herdr-lineup --version`, and update
   the link if the root changed. Do not copy the managed binary and assume it
   will update automatically.

   Alternatively, download the archive for your OS and architecture and
   `SHA256SUMS.txt` from the [release](https://github.com/fujimogn/herdr-lineup/releases/tag/v0.1.0).
   Verify the archive as described in the
   [release instructions](https://github.com/fujimogn/herdr-lineup/blob/v0.1.0/docs/releasing.md), extract it,
   and put its `herdr-lineup` executable on `PATH` (replace the placeholder with
   the extraction directory):

   ```sh
   mkdir -p "$HOME/.local/bin"
   install -m 755 /absolute/path/to/extracted/herdr-lineup "$HOME/.local/bin/herdr-lineup"
   export PATH="$HOME/.local/bin:$PATH"
   herdr-lineup --version
   ```

   Inspect an existing destination before replacing it. This archive binary
   serves the statusLine hook; the Herdr plugin still needs the GitHub install
   above. Repeat the archive installation when upgrading it, and keep its
   version aligned with the installed plugin.

3. Merge a `statusLine` entry into `~/.claude/settings.json` (or the project's
   Claude settings). Replace the placeholder with your existing command to
   preserve its display:

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "herdr-lineup statusline claude --then '<existing statusLine command>'"
     }
   }
   ```

   Without an existing command, omit `--then`:

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "herdr-lineup statusline claude"
     }
   }
   ```

   Antigravity CLI uses the same shape in
   `~/.gemini/antigravity-cli/settings.json`:

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "herdr-lineup statusline agy --then '<existing statusLine command>'"
     }
   }
   ```

   Preserve other settings in those files. The `--then` value is one shell
   argument, subsequently run through `sh -c`. For an existing command containing
   single quotes, replace each inner `'` with `'"'"'` when enclosing the argument
   in single quotes; JSON also needs the double quotes escaped. For example,
   chaining the existing command `printf '%s\n' ready` becomes:

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "herdr-lineup statusline claude --then 'printf '\"'\"'%s\\n'\"'\"' ready'"
     }
   }
   ```

   For a complex existing command, keep it in a shell script and pass
   `--then 'sh /absolute/path/to/statusline.sh'` instead.

4. Let the agent produce a statusLine update, then run the doctor action and
   inspect the plugin log again. `StatusLine records: 0` means no readable
   regular records were found (also check the state directory's permissions);
   otherwise `StatusLine records: <count> (latest: <N>s ago)` reports the count
   and newest record age. The scan examines at most 1,024 directory entries
   without following symlinks. This is a storage diagnostic, not proof that a
   record belongs to the current pane. Check that the relevant pane gains usage data.

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

Context stays `—` until an observation matches the pane, then retains the last
valid value while that exact session or pane binding matches. Idle Claude,
Antigravity, Codex, and Grok context does not expire by age. Kimi context expires
after `usage_max_age_ms`. Weekly quota with a reset time expires at that time.
StatusLine weekly without a reset uses the default maximum age of 60 seconds;
Codex quota without a reset follows its client interval and clears after a failed
query.

### Environment variables

Herdr supplies plugin variables to actions and hooks. Provider paths are read
from the daemon's environment, not individually from each pane. Use absolute
paths for state, socket, config, and provider homes.

| Variable | Used for / default |
| --- | --- |
| `HERDR_SOCKET_PATH` | Session socket; required by `ensure`, `daemon`, `stop`, and `doctor`. |
| `HERDR_PLUGIN_STATE_DIR` | Runtime files, font ownership, sidebar transaction locks, and `statusline/` records. Required by daemon commands and font actions; statusLine recording needs the same absolute path. Only configure/unconfigure fall back to `~/.local/state/herdr-lineup`. |
| `HERDR_PLUGIN_CONFIG_DIR` | Directory containing Lineup's optional `config.toml`; required by daemon commands and `doctor`. Discover with `herdr plugin config-dir herdr-lineup`. |
| `HERDR_CONFIG_PATH` | Herdr config file for configure/unconfigure; defaults to `~/.config/herdr/config.toml`. Empty is rejected; a relative path resolves from the command's working directory. |
| `HERDR_BIN_PATH` | Herdr executable for sidebar validation and reload; defaults to `herdr` on `PATH`. |
| `HERDR_PANE_ID` | Herdr-provided pane identity; binds Antigravity statusLine records to the pane. Claude uses JSON `session_id` instead. |
| `XDG_DATA_HOME` | Linux font base directory; an absolute value gives `<value>/fonts`, otherwise `~/.local/share/fonts`. Ignored for the macOS font destination. |
| `XDG_CONFIG_HOME` | Font actions' kitty/Ghostty config base; an absolute value replaces `~/.config`. Does not select Herdr's sidebar config. |
| `CODEX_HOME` | Codex local session home and account-quota client home; defaults to `~/.codex`. A non-absolute override makes the local home unavailable. |
| `GROK_HOME` | Grok local session home (default `~/.grok`) and fallback base for `auth.json`. A non-absolute override makes the local session home unavailable. |
| `KIMI_CODE_HOME` | Kimi local session home; defaults to `~/.kimi-code`. An empty value uses that default. A non-absolute override makes the local home unavailable. |
| `GROK_AUTH_FILE` | Explicit Grok billing auth file, before `GROK_HOME/auth.json` and `~/.grok/auth.json`; used only with network opt-in. |

`HOME` supplies the per-user defaults above and must be absolute for font
actions. `HERDR_PLUGIN_ROOT` is the plugin checkout, never a destination for
runtime state or user settings.

## Uninstall

Keep the plugin, its settings, and ownership records until cleanup actions have
succeeded. Before starting, save the state path from the doctor's
`State directory:` line and the config path from
`herdr plugin config-dir herdr-lineup` for the final cleanup. Run each action
from a Herdr pane and check its completed `status`, `exit_code`, and output with
`herdr plugin log list --plugin herdr-lineup --limit 10` before continuing.

1. Stop only Lineup in every Herdr session where it is running:

   ```sh
   herdr plugin action invoke herdr-lineup.stop
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   Wait for a successful completed stop command in each session. Avoid switching
   workspaces after stopping: the `workspace.focused` hook can start Lineup
   again. If it does, repeat the stop before uninstalling.

2. Remove the managed sidebar block:

   ```sh
   herdr plugin action invoke herdr-lineup.unconfigure
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   For a manually configured sidebar, remove only the rows you added, then run
   `herdr config check` and `herdr server reload-config`. Resolve modified managed
   blocks as described in [Sidebar](#sidebar) before continuing.

3. If icons were enabled, remove the owned font and terminal mappings:

   ```sh
   herdr plugin action invoke herdr-lineup.uninstall-font
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   Check the action log for removed paths or `nothing managed`. Resolve ownership
   conflicts with [Disable icons](#disable-icons) before continuing. Remove
   manual font/mapping installations manually, and reload the terminal
   configuration.

4. Restore the previous `statusLine` entries in your Claude/Antigravity settings
   (or remove the entries you added). Inspect those settings to confirm no
   `herdr-lineup statusline` command remains. Remove the launch-environment
   export and the `~/.local/bin/herdr-lineup` link or copied archive binary only
   if you created them for Lineup; check `command -v herdr-lineup` in a new agent
   launch environment to confirm the intended command is gone. Close any agent
   still using the old statusLine command before removing the plugin.

5. Immediately before uninstalling, repeat the stop action in every Herdr
   session where Lineup was running and check its successful completed log entry.
   This also catches a daemon restarted by a `workspace.focused` hook during
   the preceding steps:

   ```sh
   herdr plugin action invoke herdr-lineup.stop
   herdr plugin log list --plugin herdr-lineup --limit 10
   ```

   After every stop completes successfully, uninstall the GitHub-managed plugin
   without switching workspaces again:

   ```sh
   herdr plugin uninstall herdr-lineup
   herdr plugin list --plugin herdr-lineup --json
   ```

   The list's `result.plugins` array should be empty. Uninstall unregisters the
   plugin and removes its managed checkout. If you linked a local checkout for
   development instead, use
   `herdr plugin unlink herdr-lineup`; it unregisters the plugin but leaves
   your checkout in place. Config and state are separate from either checkout.

6. After no session daemon or agent hook uses Lineup, inspect the saved plugin
   state/config paths and remove only the Lineup data you no longer need. Keep
   `font-ownership.json` until font cleanup succeeds. Inspect and remove unwanted
   `config.toml.<random>.bak` files beside the Herdr config separately; they
   contain earlier settings and are never automatically pruned. Direct
   configure/unconfigure may also have left locks in `~/.local/state/herdr-lineup`.

## How it works

The repository heading reads the newest local `origin/main` commit without
fetching: `HH:MM` for today, `M/D` for earlier dates. This is a commit-time label,
not proof of a deployment.

<img src="https://raw.githubusercontent.com/fujimogn/herdr-lineup/v0.1.0/docs/images/sidebar-spaces.png" alt="Herdr Spaces sidebar showing repository names and local last-ship times" width="420">

### Account quota behavior

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
periods remain unavailable for 7d. Without a reset, Grok's remaining percentage
is shown without a countdown and expires after the greater of `usage_max_age_ms`
and Grok's five-minute query interval plus its query timeout and one sidebar
refresh interval from the last successful query. Codex quota without a reset
follows its client interval and clears after a failed query. Environment proxies
are inherited; redirects stay disabled. Disabled network access also keeps Codex
and Grok weekly at `—`
while preserving local context.

Codex context uses `max(total_tokens - 12000, 0) / (model_context_window - 12000)`.
A window of 12,000 tokens or less is unavailable. The snapshot has no pane-specific
home or profile, so Codex weekly is attributed only to a pane whose exact native
session is verified in the daemon's home. Panes using a different home remain
unavailable. Profiles sharing one home cannot be distinguished; stop/start the
Lineup daemon after changing authentication or profiles to discard cached account
quota. Grok has one configured auth source, so its weekly quota is shared by all
Grok panes, even without local context.

Kimi context uses one regular, nonsymlinked `state.json` under `KIMI_CODE_HOME`
whose `id` equals the pane's nonempty native session id and whose normalized
`cwd` equals the pane cwd. Duplicate readable matches are unavailable. An
unreadable `state.json` hides that match only when its session directory name
is the pane's native session id, because that file could be a duplicate.
`KIMI_CODE_HOME` may itself be a symlink, and symlinks underneath it are not
followed. From
that session's regular `agents/main/wire.jsonl`, opened without following a
symlink at `agents` or `main`, the reader uses the last
`token_counting.turn_recorded.tokens` for the main agent and the model alias
bound before that turn. A binding after the turn, an alias that is outside the
read tail, compaction, a token rebase, or a turn still in progress makes the
value unavailable. The alias resolves through the `toml` crate to `max_input_size` in
`config.toml`, or `max_context_size` when the input size is absent. Only those
integers are kept. A value that is not an integer of at least 1 makes the
window unavailable and does not fall back to the other size. Unknown aliases,
nonpositive
windows, and an input size above the context size are unavailable. The
percentage is `100 * tokens / window`, and the sidebar rounds it like the other
context labels. The reader does not use the network. The `toml` crate parses
`config.toml` and the reader keeps only that alias's `max_input_size` and
`max_context_size`; credential,
token, and API-key values are not retained or logged. A timeout keeps a fresh
Kimi context only until `usage_max_age_ms`, through the same retention path as
Codex and Grok. Kimi weekly remains unavailable.

Workers stop scheduling queries on daemon shutdown. Stop does not join a pending
Grok HTTP request (process exit ends it), and it kills/reaps the in-flight Codex
app-server process group. The existing five-second stop deadline is preserved.

### Sidebar tokens

Context and weekly remaining percentages use at least two digits (`07%`, `7d 07% 6d9h`). Missing context displays `—` followed
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

`doctor` distinguishes metadata capacity skips from refresh failures. It shows
the number of resources skipped during the latest refresh; skips alone do not
make it fail. A real refresh failure still exits nonzero. It also checks the
control socket path length before trying to connect.

### Per-session publisher

Herdr startup hooks run once. Startup and `workspace.focused` therefore both
invoke `ensure`; the per-session lock prevents duplicate daemons.
The detached daemon changes to its private session state directory. Its
`daemon.log` records UTC timestamps and fixed event/reason categories and keeps
one rotated generation (`daemon.log.1`) when the current file reaches 1 MiB.
After an external SIGTERM, `doctor` reports a stale PID record until `ensure`
starts a new daemon; `stop` does not signal that stale PID.
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

## Privacy

Lineup reads usage fields from local session files and publishes display
metadata to the local Herdr socket. It does not store conversation text,
transcript paths, or credentials. StatusLine records contain only percentages,
an optional reset time, the provider, an opaque binding key, and observation time.
The Kimi reader uses session `id` and `cwd`, the main-agent turn's token count,
time, and model alias, and that model's window keys. It does not retain other
`config.toml` values.

Network access stays disabled unless you set `allow_network = true`. That opt-in
enables only the configured Codex account-quota and Grok billing workers.
Lineup sends no conversation/session data or telemetry. Codex app-server uses
its existing authentication and may maintain its own authentication state;
Lineup never opens Codex `auth.json`. Grok reads its existing auth key without
writing or refreshing credentials.

Account-quota values stay in memory. For quota diagnostics, the daemon saves
only fixed query-result categories for `doctor`, without values, account
identities, credentials, paths, response bodies, or error text. Keep runtime state in `HERDR_PLUGIN_STATE_DIR` and user
settings in `HERDR_PLUGIN_CONFIG_DIR`; reinstall replaces `HERDR_PLUGIN_ROOT`.
Sidebar edits, font setup, and statusLine hooks all require explicit opt-in.
See [SECURITY.md](SECURITY.md) for the full boundaries.

## Troubleshooting

- **Doctor reports that the first refresh has not completed yet:** Lineup is
  starting. Wait a few seconds, then run `herdr plugin action invoke
  herdr-lineup.doctor` again. A failed refresh is reported separately as
  `daemon refresh is unhealthy`.
- **Doctor reports metadata capacity exhausted:** One or more resources were
  skipped during the latest refresh. Other resources may still have updated;
  reduce competing metadata keys. The count is for the latest refresh only.
- **Doctor reports an overlong control socket path:** Use a shorter plugin
  state directory. The diagnostic shows the path length and platform limit.
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
- **Codex or Grok weekly quota stays `—`:** check `allow_network = true`, the
  selected `providers`, and the last query category with
  `herdr plugin action invoke herdr-lineup.doctor`, then read the plugin log.
  No successful query, an expired reset, or Grok MONTHLY data leaves weekly
  unavailable. Restart only Lineup after authentication or profile changes.
- **Icon rendering shows boxes:** run the opt-in
  `herdr-lineup.install-font` action and check the terminal's codepoint map,
  or set `logo_mode = "text"`. `auto` currently uses the same text labels as
  `text`, even after font installation.
- **A font action reports a managed-block conflict:** inspect the terminal
  config named in the ownership record. For a partly removed or edited block,
  use the recovery steps in [Disable icons](#disable-icons). A wholly removed
  block is handled automatically when you retry the action.
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
