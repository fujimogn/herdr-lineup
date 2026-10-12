# Align release notes with graphify

## Why

The current changelog and release workflow use bracketed version headings and
omit the heading from the GitHub Release body. A consistent, user-facing format
and a shared extractor will make release previews match published notes and
fail publication when the requested version is absent.

## What Changes

- Use `## Unreleased` for pending changes and `## X.Y.Z (YYYY-MM-DD)` for
  releases, with a one-sentence summary and user-facing Fix/Feature bullets.
- Extract the dated version section including its heading, trim surrounding
  blank lines, and reject missing sections or malformed date headings.
- Reuse the extractor in the tag-triggered release workflow and exercise its
  boundary and failure cases in CI.
- Structure the maintainer guide as numbered, bold Steps, Version numbering,
  and Supported versions, with human-owned tagging and preflight commands.

## Capabilities

### New Capabilities

- `release-notes`: changelog formatting, extraction boundaries, release
  publication, and maintainer preparation.

### Modified Capabilities

None.

## Impact

The coordinated implementation affects `CHANGELOG.md`, `docs/releasing.md`,
release-note scripts, and the release and CI workflows. This change defines the
target contract; executable implementation tasks remain pending until verified.
It does not change versions, create tags or GitHub Releases, publish to a
marketplace, or operate the Herdr server.
