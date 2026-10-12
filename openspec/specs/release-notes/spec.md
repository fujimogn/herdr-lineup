# release-notes Specification

## Purpose
Define user-facing changelog formatting, deterministic release-note extraction,
and the maintainer checks that precede human-triggered release publication.

## Requirements

### Requirement: User-facing changelog format

The changelog SHALL start with `# Changelog` and a link to this repository's
GitHub Releases. Pending changes SHALL use `## Unreleased`; released versions
SHALL use `## X.Y.Z (YYYY-MM-DD)`. Each populated section SHALL contain a
one-sentence summary followed by `- Fix: ` and/or `- Feature: ` bullets with
bold areas and user-visible behavior, without trailing PR or commit references.

#### Scenario: Prepare a dated release section

- **WHEN** a maintainer prepares version X.Y.Z for release
- **THEN** the existing Unreleased heading becomes `## X.Y.Z (YYYY-MM-DD)`
  using the release date, and a new empty `## Unreleased` section is placed above it
- **AND** the released summary and bullets remain under the dated heading

### Requirement: Deterministic release-note extraction

`scripts/extract-release-notes.sh <version> [changelog]` SHALL default to
`CHANGELOG.md` and write the requested dated version section to standard output.
The output SHALL include the version heading and stop before the next `## `
heading or end of file, with surrounding blank lines removed and internal
content preserved. The extractor SHALL exit nonzero when the requested section
is absent or its date heading does not match the `YYYY-MM-DD` format.

#### Scenario: Extract one section among adjacent versions

- **WHEN** the requested version has a valid dated heading followed by another
  `## ` heading
- **THEN** output contains the requested heading, summary, and bullets only
- **AND** output excludes surrounding blank lines and the following section

#### Scenario: Extract the final section from an alternate changelog

- **WHEN** an explicit changelog path contains the requested version as its last section
- **THEN** the extractor reads that file and includes content through end of file,
  trimming trailing blank lines

#### Scenario: Reject missing release notes

- **WHEN** the requested version is absent, including a changelog containing only Unreleased
- **THEN** extraction exits nonzero instead of substituting another section

#### Scenario: Reject a malformed dated heading

- **WHEN** the requested version heading lacks a date in the `YYYY-MM-DD` format
- **THEN** extraction exits nonzero

### Requirement: Human-triggered release publication

The release workflow SHALL run on a human-pushed `v*` tag, verify version
consistency using `scripts/check-version.sh`, and create a GitHub Release titled
`vX.Y.Z` with the extractor output as its body. It SHALL attach Linux and macOS
binaries and `SHA256SUMS.txt`, and SHALL NOT create or push tags itself or publish
to a marketplace. Failed extraction SHALL prevent Release creation.

#### Scenario: Publish a prepared release

- **WHEN** a human pushes vX.Y.Z with matching manifest and lockfile versions
  and a valid dated changelog section
- **THEN** the workflow builds both platform binaries and creates the Release
  with title vX.Y.Z, heading-inclusive notes, and checksum attachments

#### Scenario: Prevent publication without a matching section

- **WHEN** release-note extraction fails for the pushed tag's version
- **THEN** the workflow fails before creating the GitHub Release

### Requirement: Maintainer preflight and automated checks

The release guide SHALL use numbered bold Steps, Version numbering, and
Supported versions sections. It SHALL document version and lockfile alignment,
dated changelog preparation, both preflight scripts, development checks,
reviewed integration, human-owned tagging, and release verification. CI SHALL
exercise extraction success, missing versions, Unreleased-only input, malformed
dates, adjacent-section boundaries, and surrounding blank lines.

#### Scenario: Preview the publication body before tagging

- **WHEN** a maintainer follows the guide to prepare X.Y.Z
- **THEN** they run `scripts/check-version.sh vX.Y.Z` and
  `scripts/extract-release-notes.sh X.Y.Z` before committing and tagging
- **AND** the preview uses the same extraction contract as publication
