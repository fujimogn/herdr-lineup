# Releasing herdr-lineup

Maintainer-facing release process, following the structure of
[graphify's release guide](https://github.com/Graphify-Labs/graphify/blob/v8/RELEASING.md).

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds the
binaries and creates a GitHub Release in this repository. The repository stays
private; nothing is published to a marketplace. A human creates and pushes the
tag. The workflow never tags or pushes on its own.

## Steps

1. **Bump the version.** Pick `X.Y.Z` and update `package.version` in
   `Cargo.toml` and the top-level `version` in `herdr-plugin.toml` together.
2. **Update the changelog.** Rename `## Unreleased` to
   `## X.Y.Z (YYYY-MM-DD)` using the release date, then add a fresh empty
   `## Unreleased` section above it. Each populated section starts with a
   one-sentence summary followed by `- Fix: ` and/or `- Feature: ` bullets.
   Use **bold** areas and describe user-visible behavior without trailing PR or
   commit references. Keep the GitHub Releases link at the top of the file.
3. **Refresh the lock.** Run `cargo check` with the pinned toolchain so the
   `herdr-lineup` entry in `Cargo.lock` records the new version. Keep all three
   version declarations aligned; subsequent checks use `--locked`.
4. **Verify before committing.** Check the version and preview the exact release
   notes from the repository root:
   ```sh
   scripts/check-version.sh vX.Y.Z
   scripts/extract-release-notes.sh X.Y.Z
   ```
   The version check must print `version check passed`. The extractor accepts
   an optional changelog path: `scripts/extract-release-notes.sh X.Y.Z CHANGELOG.md`.
   Its output includes the dated version heading through the line before the
   next `## ` heading, trimming surrounding blank lines. A missing section or
   malformed date heading fails; `Unreleased` is not a substitute.
   Run the full development checks on Linux and macOS with the pinned toolchain:
   ```sh
   cargo fmt --all --check
   cargo clippy --locked --all-targets -- -D warnings
   cargo test --locked
   cargo build --release --locked
   target/release/herdr-lineup --help
   ```
   Do not stop or restart the Herdr server or attach a client to a user's
   running session during verification.
5. **Land the release preparation.** Commit the version, changelog, and lockfile
   together using the conventions in `AGENTS.md`, and land the reviewed changes
   on `main` through the repository's approved integration path. Confirm CI is
   green for that release commit.
6. **Push the release tag.** A human tags the verified release commit on `main`
   and pushes only the tag:
   ```sh
   git tag vX.Y.Z <release-commit>
   git push origin vX.Y.Z
   ```
   The workflow checks the tag against the manifests and lockfile, builds on
   `ubuntu-latest` and `macos-latest`, and creates a GitHub Release titled
   `vX.Y.Z`. The body is the extractor's output, including
   `## X.Y.Z (YYYY-MM-DD)`, and the attachments are the Linux and macOS binaries
   plus `SHA256SUMS.txt`. Missing release notes fail publication.
7. **Verify the release.** Confirm the workflow succeeded and the GitHub Release
   has the expected title, dated notes, both platform binaries, and checksums.
   Compare the downloaded artifacts with `SHA256SUMS.txt` before using them.

## Version numbering

Follow Semantic Versioning. The release tag `vX.Y.Z`, `Cargo.toml`,
`herdr-plugin.toml`, and the `herdr-lineup` entry in `Cargo.lock` must agree.
`scripts/check-version.sh` enforces consistency in CI and the release workflow.
An unpublished local version bump does not constitute a release; choose the next
version against the actual tags and GitHub Releases.

## Supported versions

Release artifacts target Linux and macOS. Build and test with the Rust toolchain
pinned in `rust-toolchain.toml`; the minimum supported Herdr version is declared
in `herdr-plugin.toml`. This guide does not establish a separate maintenance
promise for older release branches. See [SECURITY.md](../SECURITY.md) for security
boundaries and vulnerability reporting.
