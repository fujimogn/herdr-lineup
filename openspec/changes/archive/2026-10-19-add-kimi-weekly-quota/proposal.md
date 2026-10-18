# Add opt-in Kimi weekly quota

## Why

Lineup can show Kimi context from an exact local session, but cannot currently
show weekly remaining. The installed Kimi CLI reads the managed account's
rolling seven-day usage through its OAuth access token. Lineup can use the
account logged in under the daemon's Kimi home, as it does for other
account-wide quota sources.

## What Changes

- Specify a separate, default-off Kimi weekly opt-in in plugin configuration,
  additionally gated by `allow_network = true`.
- Specify a bounded background `GET /usages` query using the access token from
  the daemon's Kimi home and conversion of `limit_7d` into weekly remaining.
- Share one account-wide sample across all Kimi panes, independently of their
  local context readings.
- Define credential expiry, cache expiry, failure, and API-change behavior
  without retaining credential, account, or response values in Lineup state or logs.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `provider-readers`: optional Kimi account quota worker and shared publication.
- `daemon-lifecycle`: Kimi query diagnostics and the separate default-off option.

## Impact

The default-off client and daemon worker are implemented. It does not enable
network access or change user configuration. An owner-authorized live schema
check on 2026-10-19 matched the strict `limit_7d` parser; no authenticated
Kimi endpoint was called during implementation or tests.
