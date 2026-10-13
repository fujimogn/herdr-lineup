# Releasing herdr-lineup

Maintainer-facing release process, following the structure of
[graphify's release guide](https://github.com/Graphify-Labs/graphify/blob/v8/RELEASING.md).

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds the
binaries and creates a GitHub Release in this public repository. Marketplace
discovery is automatic for public repositories with the `herdr-plugin` topic
and a valid manifest; a listing is not a Herdr review. A human creates and
pushes the tag. The workflow never tags or pushes on its own.

## Steps

1. **Bump the version.** Pick `X.Y.Z` and update `package.version` in
   `Cargo.toml` and the top-level `version` in `herdr-plugin.toml` together.
   Check the static values in the README badges against `herdr-plugin.toml` and `rust-toolchain.toml`.
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
   python3 scripts/generate-third-party-licenses.py --check
   ```
   Run the isolated lifecycle and metadata harness on Linux with Herdr 0.9.3
   or later:
   ```sh
   bash scripts/e2e-isolated.sh --self-test
   bash scripts/e2e-isolated.sh
   ```
   It creates its own session and does not attach to a user's session;
   `--tokens` is an alias for the default metadata check. Check the archive
   license file on the final lockfile even when dependency versions did not
   change. See [Testing](testing.md) for the harness requirements and limits;
   the Linux run does not establish macOS runtime behavior.
   Do not stop or restart the Herdr server or attach a client to a user's
   running session during verification.
5. **Land the release preparation.** Commit the version, changelog, and lockfile
   together using the conventions in `AGENTS.md`, and land the reviewed changes
   on `main` through the repository's approved integration path. Confirm CI is
   green for that release commit.
6. **Push the release tag.** Before tagging, confirm that the changelog heading's
   date matches the day the tag will be created. If release preparation happened
   earlier, update the date and land that reviewed change first. A human tags
   the verified release commit on `main` and pushes only the tag:
   ```sh
   git tag vX.Y.Z <release-commit>
   git push origin vX.Y.Z
   ```
   The workflow checks the tag against the manifests and lockfile, builds on
   `ubuntu-latest` and `macos-latest`, and creates a GitHub Release titled
   `vX.Y.Z`. The body is the extractor's output, including
   `## X.Y.Z (YYYY-MM-DD)`, and the attachments are the Linux and macOS archives
   plus `SHA256SUMS.txt`. Missing release notes fail publication.
7. **Verify the release.** Confirm the workflow succeeded and the GitHub Release
   has the expected title, dated notes, both platform archives, and checksums.
   Compare the downloaded artifacts with `SHA256SUMS.txt` before using them.
   Confirm that the public repository still has the `herdr-plugin` GitHub topic
   and that its default branch contains the released manifest. Then check
   [Herdr's marketplace](https://herdr.dev/plugins/) for the repository and
   plugin version. Its unreviewed index refreshes every 30 minutes and rescans
   when the default-branch head changes; allow for that interval before treating
   an absent or old listing as a publication problem.

## Release artifacts

Each archive is named `herdr-lineup-<tag>-<os>-<arch>.tar.gz`, for example
`herdr-lineup-v0.1.1-linux-x86_64.tar.gz` or
`herdr-lineup-v0.1.1-macos-arm64.tar.gz`. The build reads the runner's actual OS
and CPU architecture with `uname`; unsupported combinations fail packaging.
Linux and macOS support `x86_64` and `arm64` labels (`aarch64` on Linux is
normalized to `arm64`). The runner matrix produces one archive per OS, not
every architecture combination. Each archive contains these files at its root:

- `herdr-lineup` — the executable, with its executable permission preserved.
- `LICENSE` — the project license.
- `NOTICE.md` — bundled font and adapted-code attribution.
- `THIRD-PARTY-LICENSES.md` — pinned Rust binary dependency licenses and
  attribution for both release platforms.
- `README.md` — usage and installation instructions.

When `Cargo.lock` changes, regenerate the file with
`python3 scripts/generate-third-party-licenses.py` using Python 3.11+ and the pinned
Rust toolchain, then review the generated text. Always run the script with
`--check` on the final release commit. The release workflow and CI run that
check too. It fails if the file differs from the locked crate sources.
Cargo may fetch missing pinned crate sources; the script installs no additional tool.

`SHA256SUMS.txt` covers every `.tar.gz` archive. From the download directory,
select the archive you downloaded and check only its entry (replace the example
filename). On Linux:

```sh
ARCHIVE=herdr-lineup-v0.1.1-linux-x86_64.tar.gz
CHECKSUM_LINE=$(awk -v archive="$ARCHIVE" '$2 == archive { print; count++ } END { if (count != 1) exit 1 }' SHA256SUMS.txt) &&
  printf '%s\n' "$CHECKSUM_LINE" | sha256sum -c -
```

On macOS:

```sh
ARCHIVE=herdr-lineup-v0.1.1-macos-arm64.tar.gz
CHECKSUM_LINE=$(awk -v archive="$ARCHIVE" '$2 == archive { print; count++ } END { if (count != 1) exit 1 }' SHA256SUMS.txt) &&
  printf '%s\n' "$CHECKSUM_LINE" | shasum -a 256 -c -
```

Missing or duplicate entries fail the selection; use the archive only after the
check prints `OK`. Selecting the exact entry also rejects a manifest that does
not list the archive you chose. Only the publish job receives `contents: write`;
builds use `contents: read`.

Plugin installation builds from a checkout using the manifest's Cargo build
command. The archives are not standalone plugin installations: they do not
include the manifest or Rust sources. Put the extracted executable on the
agent's `PATH` for commands such as `statusline`, following the README's shared
`HERDR_PLUGIN_STATE_DIR` setup. Install from GitHub, or build and link a local
checkout for development, as described in the README.

The macOS executable is not Developer ID signed or notarized. Browser downloads
receive macOS quarantine metadata, so Gatekeeper may block execution even after
the checksum matches. The documented plugin installation builds a GitHub
checkout locally; an archive checksum alone does not establish publisher trust.

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
