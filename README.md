# herdr-lineup

One sidebar row per AI coding agent in [Herdr](https://herdr.dev): vendor logo,
state, context, weekly quota, and when each repository last shipped.

**Planned.** This repository currently contains a buildable command-line scaffold.
It does not publish sidebar data or run a daemon. All subcommands print a
not-implemented message and exit with status 1; `--help` and `--version` work.
No release or runtime screenshot is available yet.

## Install

Requires macOS or Linux, Herdr 0.9.0 or later, and Rust/Cargo. The build uses the
toolchain pinned in `rust-toolchain.toml` (Rust 1.99.0); install it with rustup.

**Planned public installation**, after the repository is published:

```sh
herdr plugin install fujimogn/herdr-lineup
```

Herdr builds the plugin with `cargo build --release --locked` during installation.
Installing this scaffold will not produce sidebar rows. See Development to
build the current code locally.

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

| Command | Planned behavior |
| --- | --- |
| `ensure` | Start one detached publisher per Herdr session if needed. |
| `daemon` | Run the publisher in the foreground. |
| `stop` | Stop the plugin publisher, leaving the Herdr server running. |
| `doctor` | Check local setup and explain missing data. |
| `configure` | Opt in to adding plugin sidebar rows. |
| `unconfigure` | Remove only the plugin-managed sidebar rows. |

**Planned Herdr actions:** `herdr-lineup.start`, `herdr-lineup.stop`,
`herdr-lineup.doctor`, `herdr-lineup.configure`, `herdr-lineup.unconfigure`,
`herdr-lineup.install-font`, and `herdr-lineup.uninstall-font`.
For example, once implemented:

```sh
herdr plugin action invoke herdr-lineup.start
herdr plugin action invoke herdr-lineup.doctor
```

## Configuration

**Planned:** user settings will live in
`$HERDR_PLUGIN_CONFIG_DIR/config.toml`, with defaults when the file is absent.
Find the directory of an installed or linked plugin with:

```sh
herdr plugin config-dir herdr-lineup
```

There are no implemented settings yet. Sidebar rows will be added by hand or
through the explicit, opt-in `configure` action; startup will not edit the
user's Herdr configuration. Planned configuration edits use marker comments
and a backup, preserving unrelated settings. `unconfigure` will remove only
plugin-managed entries.

**Planned:** the plugin bundles the HerdrAgentIconsMax icon font and emits icon
codepoints. The font is installed only through the explicit, opt-in
`herdr-lineup.install-font` action and removed with
`herdr-lineup.uninstall-font`; startup never touches font setup. A text
fallback is planned. See [NOTICE.md](NOTICE.md) for sources and licensing.

## How it works

**Planned:** combine vendor logos, agent state, context use, and weekly remaining
quota for Claude, Codex, Grok, Antigravity, Kimi, and Hermes. Unavailable usage
shows `—`. [Source research](docs/research/usage-sources.md) records conditional
availability; provider readers and account binding are not implemented yet.

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
invoke `ensure`; the planned per-session lock prevents duplicate daemons.
The publisher will use the local Herdr socket, send changed `lineup_` metadata
with an expiry, and keep its state under `$HERDR_PLUGIN_STATE_DIR`.
See the [Herdr plugin documentation](https://herdr.dev/docs/plugins/)
and [SECURITY.md](SECURITY.md). None of these runtime behaviors are implemented yet.

## Troubleshooting

- **A command says “Planned” and exits with status 1:** expected for this scaffold.
- **No sidebar data after install or link:** the publisher is not implemented yet.
- **Local link cannot find the binary:** build first; `herdr plugin link` does not build.
- **Planned icon rendering shows boxes:** run the opt-in
  `herdr-lineup.install-font` action and check the terminal's codepoint map,
  or use the planned text fallback.
- **A build fails:** verify rustup can install the pinned toolchain and that Cargo
  can download the locked dependencies. No credentials or remote API setup are
  required by the current executable.

Do not stop or restart the Herdr server to troubleshoot this scaffold.

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

Linking registers the manifest but does not make the planned commands functional.
The CI workflow runs formatting, Clippy, tests, and release builds on Linux and
macOS, with a dependency audit on pushes and weekly. Keep package versions in
`Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml` aligned; release tags use `v*`.
Documents and comments are written in English. See [AGENTS.md](AGENTS.md).

## License

[MIT](LICENSE). Third-party font attribution is in [NOTICE.md](NOTICE.md).

Not affiliated with Herdr or the supported AI providers.
