## 1. Documentation and specification

- [x] Define the release-notes SHALL requirements and WHEN/THEN scenarios.
- [x] Align the maintainer guide with graphify's Steps, Version numbering, and
  Supported versions structure and the agreed tag-triggered release process.
- [x] Validate this change with `openspec validate graphify-release-notes --strict`.

## 2. Changelog and extractor implementation

- [x] Rewrite CHANGELOG.md with the GitHub Releases link, unbracketed headings,
  one-sentence summaries, and user-facing Fix/Feature bullets.
- [x] Implement `scripts/extract-release-notes.sh <version> [changelog]` with
  heading-inclusive output, section boundaries, and failure behavior.
- [x] Verify normal extraction, missing versions, Unreleased-only input,
  malformed dates, adjacent sections, and surrounding blank lines.

## 3. Workflow integration

- [x] Use the extractor in release.yml while preserving the vX.Y.Z title,
  platform binaries, checksum attachment, and human-owned tag push trigger.
- [x] Run extractor tests in the CI check job and validate workflow syntax.
- [x] Confirm the integrated guide, changelog, scripts, and workflows agree
  before archiving this change; do not create a tag or Release for verification.

## Verification evidence

Verified against shipped main commit
`a8be4f0846000e6467938f150db9bfbb1e9d6c93` on 2026-10-13:

- `CHANGELOG.md` has the GitHub Releases link, an Unreleased heading, a
  one-sentence summary, and user-facing Fix/Feature bullets with bold areas.
- `scripts/extract-release-notes.sh` includes the dated heading, preserves
  internal content, stops at the next level-two heading, and fails closed.
- `scripts/test-extract-release-notes.sh` passed all extraction, failure,
  boundary, default-path, alternate-path, and blank-line cases.
- `mise exec actionlint@1.7.12 -- actionlint .github/workflows/ci.yml
  .github/workflows/release.yml` passed using the installed actionlint version.
- `openspec validate graphify-release-notes --strict` passed.
- `scripts/extract-release-notes.sh 0.1.0` exited 1 with no release notes,
  as required for the current Unreleased-only changelog.
- `release.yml` uses the extractor before Release creation and retains the
  human-pushed tag trigger, tag title, Linux/macOS binaries, and checksums.
  `ci.yml` runs the extractor tests in the check job.
- `docs/releasing.md` agrees with the changelog, extractor, and workflows,
  including preflight commands and human-owned tagging. No tag or Release
  was created for verification.
