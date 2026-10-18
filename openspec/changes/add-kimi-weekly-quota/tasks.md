# Implementation tasks

- [x] Record the owner's endpoint, credential-access, and failure decisions in
      `design.md`; keep the Kimi worker disabled until its wiring assignment.
- [ ] Add the default-off Kimi-specific option to plugin config and document its
      conjunction with `allow_network` without changing user settings. Plan
      compatibility for older binaries because config rejects unknown keys.
- [x] Add the standalone read-only Kimi OAuth client with bounded `GET
      /usages` and a strict `limit_7d` parser using synthetic fixtures. Worker
      wiring and opt-in gates remain future work.
- [ ] Add opt-out, credential change, expiry, cache freshness, failure category,
      and all-Kimi-pane sharing tests. Keep local context independent.
- [x] Document in `SECURITY.md` that the standalone client reads the Kimi OAuth access token
      from disk, unlike the Codex quota path, and describe the network destination,
      memory lifetime, and log exclusions.
- [ ] Update `docs/research/kimi-usage.md` to replace the former Server API
      and pane/server/account binding proposal with the approved daemon-home
      credential route and all-Kimi-pane account scope.
- [ ] Verify the installed Kimi version and nonpublic response schema in an
      approved environment, then update user documentation and run release checks.

The standalone client is complete; opt-in configuration, worker wiring, and
live release verification remain future work. No authenticated Kimi endpoint
was called for the client tests.
