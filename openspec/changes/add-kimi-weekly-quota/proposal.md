# Add opt-in Kimi weekly quota

## Why

Lineup can show Kimi context from an exact local session, but cannot currently
show weekly remaining. Kimi's experimental server API exposes a managed account's
seven-day usage. A quota read is useful only when the account can be proved to
belong to the pane; the present session files do not provide that proof.

## What Changes

- Specify a separate, default-off Kimi weekly opt-in in plugin configuration,
  additionally gated by `allow_network = true`.
- Specify a bounded, authenticated background query and conversion of the
  managed account's `limit7d` result into weekly remaining.
- Require explicit pane/session/server/account provenance before publication;
  otherwise keep weekly unavailable while local context continues to work.
- Define expiry, failure, and experimental API change behavior without keeping
  credentials, account identifiers, or response bodies in Lineup state or logs.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `provider-readers`: optional Kimi account quota worker and pane attribution.

## Impact

This change is a design and specification only. It does not implement a worker,
enable network access, read credentials, call Kimi endpoints, or change user
configuration. Implementation remains gated on the account-binding and
credential-route questions in `design.md`.
