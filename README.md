# herdr-lineup

One sidebar row per AI coding agent in [Herdr](https://herdr.dev): vendor logo,
state, context, weekly quota, and when each repository last shipped.

A per-session daemon publishes `lineup_` sidebar metadata. Authenticated
provider usage and a runtime screenshot are not available yet. `--help` and
`--version` work.

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
```

| Command | Behavior |
| --- | --- |
| `ensure` | Start one detached publisher per Herdr session if needed. |
| `daemon` | Run the publisher in the foreground. |
| `stop` | Stop the plugin publisher, leaving the Herdr server running. |
| `doctor` | Check local setup and explain missing data. |
| `configure` | Opt in to adding plugin sidebar rows. |
| `unconfigure` | Remove only the plugin-managed sidebar rows. |

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
shows `—`. [Source research](docs/research/usage-sources.md) records conditional
availability. Codex and Grok context readers are implemented. The other agents,
and every weekly quota, stay unavailable without an authenticated source.

Each agent row shows the vendor logo, a state dot, context use as a percentage,
and a weekly quota triplet such as `7d 67% 6d9h`: the quota period, the share
remaining, and the time until the quota resets. Each workspace heading shows
the repository name and the newest local `origin/main` commit time.

In v1, only Codex and Grok context use produce values. Claude, Antigravity,
Kimi, and Hermes context, and every agent's weekly quota, show `—`: those
numbers need authenticated API calls or a statusLine hook integration that are
not supported yet.

The repository heading reads the newest local `origin/main` commit without
fetching: `HH:MM` for today, `M/D` for earlier dates. This is a commit-time label,
not proof of a deployment.

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
