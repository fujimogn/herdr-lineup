## 1. Documentation and specification

- [x] Define the release-notes SHALL requirements and WHEN/THEN scenarios.
- [x] Align the maintainer guide with graphify's Steps, Version numbering, and
  Supported versions structure and the agreed tag-triggered release process.
- [x] Validate this change with `openspec validate graphify-release-notes --strict`.

## 2. Changelog and extractor implementation

- [ ] Rewrite CHANGELOG.md with the GitHub Releases link, unbracketed headings,
  one-sentence summaries, and user-facing Fix/Feature bullets.
- [ ] Implement `scripts/extract-release-notes.sh <version> [changelog]` with
  heading-inclusive output, section boundaries, and failure behavior.
- [ ] Verify normal extraction, missing versions, Unreleased-only input,
  malformed dates, adjacent sections, and surrounding blank lines.

## 3. Workflow integration

- [ ] Use the extractor in release.yml while preserving the vX.Y.Z title,
  platform binaries, checksum attachment, and human-owned tag push trigger.
- [ ] Run extractor tests in the CI check job and validate workflow syntax.
- [ ] Confirm the integrated guide, changelog, scripts, and workflows agree
  before archiving this change; do not create a tag or Release for verification.
