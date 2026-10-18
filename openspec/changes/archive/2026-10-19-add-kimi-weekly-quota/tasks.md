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
- [x] Verify the installed Kimi version and nonpublic response schema in an
      approved environment, then update schema-dependent user documentation and
      run release checks before archiving. On 2026-10-19, the owner-authorized
      `kimi --version` reported 2.2.0 and the live GET returned HTTP 200.
      `usages.limit_7d` had exactly `used_ratio`
      (number in [0, 1]) and `reset_time` (future RFC3339 string), matching
      the strict parser. The credential file had `access_token`, `expires_at`,
      `expires_in`, `refresh_token`, `scope`, and `token_type`; `expires_in` was
      900 seconds. No credential or quota values are recorded.

The standalone client and opt-in worker wiring are implemented. The
owner-authorized live schema check was completed on 2026-10-19; the client and
worker tests still use synthetic fixtures. Release checks for this archive
candidate are recorded in its Formation report.
