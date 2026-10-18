# Implementation tasks

- [x] Record the owner's endpoint, credential-access, and failure decisions in
      `design.md`; keep the Kimi worker disabled until its wiring assignment.
- [x] Add the default-off Kimi-specific option to plugin config and document its
      conjunction with `allow_network` without changing user settings. Plan
      compatibility for older binaries because config rejects unknown keys.
- [x] Add the standalone read-only Kimi OAuth client with bounded `GET
      /usages` and a strict `limit_7d` parser using synthetic fixtures. Worker
      wiring and opt-in gates are implemented separately.
- [x] Add opt-out, credential change, expiry, cache freshness, failure category,
      and all-Kimi-pane sharing tests. Keep local context independent.
- [x] Document in `SECURITY.md` that the standalone client reads the Kimi OAuth access token
      from disk, unlike the Codex quota path, and describe the network destination,
      memory lifetime, and log exclusions.
- [x] Reconcile `docs/research/kimi-usage.md` and `docs/research/usage-sources.md`
      with the implemented default-off daemon-home worker and its account scope.
- [x] Add the `daemon-lifecycle` delta for the Kimi doctor category, the
      `kimi_weekly_enabled` config default, and the eight-default count.
- [ ] Verify the installed Kimi version and nonpublic response schema in an
      approved environment, then update schema-dependent user documentation and
      run release checks. Do not archive the change before this check.

The standalone client and opt-in worker wiring are implemented. Live response
schema verification and release checks remain open. No authenticated Kimi
endpoint was called for the client or worker tests.
