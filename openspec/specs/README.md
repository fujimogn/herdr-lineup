# Current specifications

Accepted behavior lives in these capability directories. Archived proposals live in
`../changes/archive/`, including `2026-10-13-add-lineup-v1/` and
`2026-10-13-graphify-release-notes/`.

- `daemon-lifecycle`: one daemon per Herdr session, diagnostics, bounded
  configuration, and opt-in sidebar and font actions.
- `provider-readers`: session-bound local usage and opt-in account quota.
- `sidebar-tokens`: pane and workspace metadata for logos, state, context,
  weekly quota, and repository ship time.
- `release-notes`: user-facing changelog formatting, deterministic extraction,
  human-triggered publication, and maintainer preflight checks.
