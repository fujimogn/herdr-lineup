# Releasing

Releases are built by `.github/workflows/release.yml` when a `v*` tag is
pushed. The repository stays private: the workflow only creates a GitHub
Release in this repository, and nothing is published to a marketplace.

Version numbers follow Semantic Versioning and must stay identical in the
release tag, `Cargo.toml`, `herdr-plugin.toml`, and `Cargo.lock`.
`scripts/check-version.sh` enforces this and runs both in CI (check job) and
in the release workflow.

## Steps

1. Pick the next version `X.Y.Z` and update it in both files:
   - `Cargo.toml`: `package.version`
   - `herdr-plugin.toml`: top-level `version`
2. Refresh the lockfile so its `herdr-lineup` entry matches:
   `cargo check --locked` will fail until you run `cargo check` (or
   `cargo build`) once to rewrite `Cargo.lock`.
3. Update `CHANGELOG.md`:
   - Rename the `## [Unreleased]` section to `## [X.Y.Z] - YYYY-MM-DD` and
     add a fresh empty `## [Unreleased]` section above it.
   - Add the corresponding link at the bottom, e.g.
     `[X.Y.Z]: https://github.com/fujimogn/herdr-lineup/compare/vPREV...vX.Y.Z`,
     and point `[Unreleased]` at `compare/vX.Y.Z...HEAD`.
   - The release workflow uses this section verbatim as the release notes,
     so the release fails if the section is missing.
4. Verify consistency before committing:
   `scripts/check-version.sh vX.Y.Z` must print `version check passed`.
5. Commit the changes on `main` (see the commit conventions in
   `AGENTS.md`), then tag the release commit:
   `git tag vX.Y.Z`
6. Push the branch and the tag:
   `git push origin main` and `git push origin vX.Y.Z`

Pushing the tag triggers the Release workflow, which:

- runs `scripts/check-version.sh` with the tag and fails on any mismatch,
- builds release binaries on `ubuntu-latest` and `macos-latest`,
- packages each binary with a `SHA256SUMS.txt` covering all artifacts,
- creates a GitHub Release titled `vX.Y.Z` with the CHANGELOG section as
  the body and the binaries plus checksums attached.

Tag creation and pushing are always done by a human; the workflow never
tags or pushes on its own.
