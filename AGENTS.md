# Agent instructions

- This is a single Rust crate for a Herdr plugin on macOS and Linux.
- Read `README.md`, `herdr-plugin.toml`, and any relevant specifications before changing behavior.
- Use the toolchain pinned in `rust-toolchain.toml`. Keep documentation and comments in English.
- Preserve the distinction between implemented behavior and **Planned** features.
- Before handing off changes, run `cargo fmt --all --check`,
  `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`,
  `cargo build --release --locked`, and `target/release/herdr-lineup --help`.
- Check manifest changes against the [official plugin documentation](https://herdr.dev/docs/plugins/).
  Use `actionlint` for workflow changes when available.
- Keep versions in `Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml` aligned.
- Never stop, restart, or quit the Herdr server, or attach a client to a user's running session during verification.
- Do not change user configuration without explicit opt-in. Store plugin state in
  `HERDR_PLUGIN_STATE_DIR` and user settings in `HERDR_PLUGIN_CONFIG_DIR`, never in `HERDR_PLUGIN_ROOT`.
- Do not commit credentials, session contents, local-only instructions, or bundled icon fonts.
