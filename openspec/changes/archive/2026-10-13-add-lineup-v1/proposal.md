# Add lineup v1

## Why

Herdr sidebar information currently comes from separate radar and quota plugins,
while repository shipment timestamps have no implementation. One self-authored
Rust plugin will publish a consistent metadata contract for six coding agents.

## What Changes

- Introduce pane tokens for vendor identity, semantic state, context percentage,
  and weekly remaining quota, plus workspace tokens for repository name and the
  latest locally known `origin/main` commit time.
- Bundle the verified radar icon font and provide explicit opt-in font actions, without automatically installing fonts or
  modifying terminal configuration; provide readable text fallback.
- Define a per-Herdr-session daemon, start/stop/doctor actions, user config, and
  reversible opt-in sidebar configuration.
- Define provider reader results and unavailable-data handling. Source adapters
  are a separate research dependency; this proposal does not claim availability.

## Capabilities

### New Capabilities

- `sidebar-tokens`: metadata scopes, formats, expiry, logos, state, and headings.
- `daemon-lifecycle`: session ownership, actions, configuration, and diagnostics.
- `provider-readers`: session-bound usage results and missing-data semantics.

### Modified Capabilities

None. There are no accepted current capabilities yet.

## Impact

The initial change is specification-only. Future implementation affects the
Rust crate, plugin manifest, tests, and public documentation. No `package.json`
is introduced; OpenSpec is run with `bunx @fission-ai/openspec@1.14.1`.

Out of scope: animation, automatic font installation or terminal codepoint-map changes,
automatic sidebar edits at startup, fetching Git refs, and establishing each
provider's usage source. Kimi and Hermes readers remain unavailable in stage one
until research confirms a source. The plugin does not report agent lifecycle
authority or send credentials or usage data to Herdr as metadata.
