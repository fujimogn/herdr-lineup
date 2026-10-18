# Implementation tasks (future work)

- [ ] Resolve the endpoint and credential-access Open questions in `design.md`;
      keep the Kimi worker disabled until those decisions are recorded.
- [ ] Add the default-off Kimi-specific option to plugin config and document its
      conjunction with `allow_network` without changing user settings. Plan
      compatibility for older binaries because config rejects unknown keys.
- [ ] Read only the selected Kimi OAuth credential from the daemon home without
      login, refresh, or writes; implement bounded `GET /usages` and a strict
      `limit_7d` parser using synthetic fixtures.
- [ ] Add opt-out, credential change, expiry, cache freshness, failure category,
      and all-Kimi-pane sharing tests. Keep local context independent.
- [ ] Document in `SECURITY.md` that Lineup reads the Kimi OAuth access token
      from disk, unlike the Codex quota path, and describe the network destination,
      memory lifetime, and log exclusions.
- [ ] Verify the installed Kimi version and nonpublic response schema in an
      approved environment, then update user documentation and run release checks.

These are future implementation tasks. This change writes no executable code.
