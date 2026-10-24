# Releasing herdr-lineup

Maintainer-facing release process, following the structure of
[graphify's release guide](https://github.com/Graphify-Labs/graphify/blob/v8/RELEASING.md).

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which
builds the binaries and creates a GitHub Release in this public repository. Marketplace
discovery is automatic for public repositories with the `herdr-plugin` topic
and a valid manifest; a listing is not a Herdr review. A human creates and
pushes the tag. The workflow never tags or pushes on its own.

## Steps

1. **Bump the version.** Pick `X.Y.Z` and update `package.version` in
   `Cargo.toml` and the top-level `version` in `herdr-plugin.toml` together.
   Check the static values in the README badges against `herdr-plugin.toml` and `rust-toolchain.toml`.
   The README uses images from `main`; replacing one changes it for every published README that links to that URL.
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
   .develop-tools/resolve ci check-release-version
   scripts/extract-release-notes.sh X.Y.Z
   scripts/test-check-release-gate.sh
   ```
   The version check must exit 0; before the tag exists it compares the version
   files declared under `release.versionFiles` in `.develop-tools.json`. The
   resolver downloads the pinned public develop-tools release and verifies its
   SHA-256 against `.develop-tools.json`. This repository does not apply the
   shared documentation rules of `develop-docs-check`; the Develop tools workflow
   only checks that the resolver fetches the pinned tools. The extractor accepts
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
   together using the conventions in `AGENTS.md`, and push them to `develop`,
   where day-to-day changes land and `ci-develop.yml` runs the light checks.
   Then fast-forward `main` to that reviewed `develop` commit and push `main`;
   the push runs the full checks in `ci.yml`. Confirm CI is green for that
   release commit. CI runs on Linux only; check every declared OS
   on the release commit before tagging:
   ```sh
   .develop-tools/resolve release release check
   ```
   It dispatches `.github/workflows/platform-check.yml` (the
   `release.preflightWorkflow` in `.develop-tools.json`) on the integration
   branch HEAD (`develop`) and waits for that run only. Before dispatch,
   confirm `origin/develop` and `origin/main` both point to the same verified
   release commit, and keep them unchanged through the check and tagging.
   The workflow calls the shared platform check, which runs Clippy, a release
   build, nextest, and doc tests on each OS in `platforms` (`linux` and
   `macos`). The commit must already be on `origin/main`, and `gh` must be
   authenticated. Tag only after that run succeeds.
6. **Push the release tag.** Before tagging, confirm that the changelog heading's
   date matches the tag date: the tagger date of an annotated tag, or the commit
   date of a lightweight tag. If release preparation happened
   earlier, update the date and land that reviewed change first. A human tags
   the verified release commit on `main` and pushes only the tag:
   ```sh
   notes_file=$(mktemp /tmp/herdr-lineup-release-notes.XXXXXX)
   scripts/extract-release-notes.sh X.Y.Z > "$notes_file" &&
     git tag -a vX.Y.Z <release-commit> -F "$notes_file" &&
     git push origin vX.Y.Z
   rm "$notes_file"
   ```
   Before any build, the workflow requires the tag's commit to be reachable
   from `origin/main`, the latest CI `push` run for that commit on `main` to
   have succeeded, and the `## X.Y.Z (YYYY-MM-DD)` heading to match the
   tag date. The gate explicitly refetches the tag
   because checkout may replace a pushed annotated tag with a lightweight
   local ref; it also checks that the fetched tag points to the checked-out
   commit. It reads CI runs with `actions: read`; if a
   check fails or the API is unavailable, the release stops. The workflow
   then checks the tag against the manifests and lockfile, runs
   `cargo test --locked`, builds on `ubuntu-latest` and `macos-latest`, and
   creates a GitHub Release titled
   `vX.Y.Z`. A nonempty annotated tag message takes precedence for the body;
   the command above uses the full extracted notes, including
   `## X.Y.Z (YYYY-MM-DD)`. Lightweight tags and empty annotations fall back
   to the dated changelog section. The attachments are the Linux and macOS archives
   plus `SHA256SUMS.txt`. Missing release notes fail publication. Re-running
   the same tag rebuilds and verifies the artifacts; if its GitHub Release
   already exists, publication succeeds without changing its notes or assets.
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
`herdr-lineup-v0.2.2-linux-x86_64.tar.gz` or
`herdr-lineup-v0.2.2-macos-arm64.tar.gz`. The build reads the runner's actual OS
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
ARCHIVE=herdr-lineup-v0.2.2-linux-x86_64.tar.gz
CHECKSUM_LINE=$(awk -v archive="$ARCHIVE" '$2 == archive { print; count++ } END { if (count != 1) exit 1 }' SHA256SUMS.txt) &&
  printf '%s\n' "$CHECKSUM_LINE" | sha256sum -c -
```

On macOS:

```sh
ARCHIVE=herdr-lineup-v0.2.2-macos-arm64.tar.gz
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
`check-release-version`, run through `.develop-tools/resolve`, enforces consistency
in CI and the release workflow. On a tagged commit it also requires the tag to
match the declared version and the commit to be on `main`.
An unpublished local version bump does not constitute a release; choose the next
version against the actual tags and GitHub Releases.

## Supported versions

Release artifacts target Linux and macOS. Build and test with the Rust toolchain
pinned in `rust-toolchain.toml`; the minimum supported Herdr version is declared
in `herdr-plugin.toml`. This guide does not establish a separate maintenance
promise for older release branches. See [SECURITY.md](../SECURITY.md) for security
boundaries and vulnerability reporting.
