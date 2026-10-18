# Kimi weekly quota design

## Status and evidence

**Proposed; no Kimi weekly implementation exists.** The shipped Kimi reader
uses a completed turn from one exact local session and always returns unavailable
weekly. That session match does not prove which managed OAuth account a separate
server uses. [Local research](../../../docs/research/kimi-usage.md) records this
gap and distinguishes completed-turn context from account quota.

The [Kimi Server API reference](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html)
documents Bearer authentication, `GET /api/v1/oauth/usage`, the `limit7d`
response, cold-session resumption by the status endpoint, and the experimental
API warning. The [sessions guide](https://www.kimi.com/code/docs/en/kimi-code-cli/guides/sessions.html)
describes local session metadata and recovery files. These are documentation
contracts, not live endpoint or account verification.

## Opt-in and configuration

Add `kimi_weekly_enabled = false` to Lineup's `config.toml` under
`HERDR_PLUGIN_CONFIG_DIR`. A Kimi quota worker may start only when that key and
the existing `allow_network = true` are both explicit, and `kimi` is in
`providers`. `allow_network` alone must not enable Kimi quota. Disabling either
gate drops the in-memory quota and publishes `—` with no numeric or reset key;
local Kimi context continues to work. Startup must not edit configuration.
This follows the existing [configuration and quota-worker contract](../../../openspec/specs/provider-readers/spec.md)
and the [README usage source](../../../README.md).

## Credential and account route

The Kimi server requires `Authorization: Bearer <token>` for the usage route.
The Bearer token authenticates the *local server*; that server's
`managed:kimi-code` OAuth account supplies the quota. They are distinct
credentials. The worker must never request login, refresh a credential itself,
read the OAuth credential store, or put a token in plugin config. An explicit,
owner-approved runtime credential handoff must identify one loopback server
instance and supply its Bearer token only for a request. The worker may hold the
token transiently in process memory to send that request, but must not persist,
log, include in errors, or place it in diagnostics. Do not scan ports or pick the
first instance: the reference allows multiple servers under one home and a port
fallback. Reject non-loopback origins, redirects, ambiguous instances, and a
server that reports auth bypass. The precise handoff mechanism is an Open
question because the documented API does not prescribe a Lineup-safe token
source. No endpoint is called until it is settled and tested.

The only planned quota request is `GET /api/v1/oauth/usage` with the managed
provider fixed to `managed:kimi-code`. A successful `code = 0` envelope can
still contain `data.kind = "error"`, so both levels must be checked. Do not
query `userinfo` merely to obtain an identifier: it returns personal account
fields and does not itself prove which account owns the pane. The quota response
contains no account binding. [API reference: authentication and usage](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html#authentication).

## Pane, session, and account attribution

Require a provenance chain captured from the *same running Kimi process*:
Herdr pane ID and exact native `agent_session` ID → validated local Kimi
`state.json` ID and cwd → server instance ID → managed OAuth account identity.
Each link needs a trusted, contemporaneous source. An exact local state match
alone proves only the session link. Cwd, a session directory name, a server
found under the same home, a user-selected URL, and successful Bearer auth do
not prove the server/account links. Herdr's current pane snapshot has no PID;
the documented usage response has no account or session ID. Therefore the
current sources cannot satisfy the entire chain and **weekly remains
unavailable even after opt-in** until a binding mechanism is approved and
verified. A future Kimi process or host integration could provide a signed or
otherwise trusted, short-lived binding record at launch; Lineup must check
pane/session/server/account identity on every publication and invalidate the
cache when any link changes, including account switch or server restart.
An explicit user's assertion alone is not a proof of this chain.

For the first implementation, configure and poll at most one proved
account/server binding; panes for any other account stay unavailable.
Account-wide quota may then be reused only across panes with independently
proved bindings to that same account and server. Never infer a weekly result
from local context. This stricter rule avoids transferring one managed account's
quota to unrelated Kimi panes. See [local research: Server API and binding](../../../docs/research/kimi-usage.md#server-api-and-binding).

## Passive queries and quota conversion

Do not call `GET /api/v1/sessions/{session_id}/status` to verify attribution:
the [API reference](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html)
says that reading it resumes a cold session. Do not call session messages,
snapshot, runtime, or login endpoints as part of quota polling. The quota
worker is passive with respect to sessions and must not start a Kimi server.

After a proved binding, accept only HTTP success with envelope `code = 0`,
`data.kind = "ok"`, and a present `quota.usages.limit7d.usedRatio` that is a
finite number in [0, 1]. Compute remaining percentage as
`100 × (1 - usedRatio)`; do not use `limit5h`, monthly windows, or `extraUsage`.
Parse optional `resetAt` as RFC3339. A malformed or past reset is invalid;
when absent, publish without a countdown and apply an age cap. Never infer a
reset or a full allowance at expiry. Keep only percentage, receipt time, and
validated reset in memory. This follows the [documented usage shape](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html)
and the [provider-reader percentage contract](../../../openspec/specs/provider-readers/spec.md).

## Cadence, freshness, and failure

Use one background worker for the single configured, proved account/server
binding, never per pane. Multiple bindings require a separate design decision.
Default poll interval: five minutes; clamp configured interval to 60–3600
seconds and request timeout to 1–30 seconds, matching the existing Codex/Grok
quota-worker bounds. No retry inside an interval and at most one in-flight
request. Refresh reads memory and never waits for HTTP. Bound response size to
64 KiB, reject redirects, and discard a late response after timeout or binding
change. These values are proposed Lineup policy, not Kimi API guarantees.

Before the first valid response, show `—`. A successful sample with `resetAt`
expires at that time; one without reset expires after the greater of
`usage_max_age_ms` and interval + timeout + one sidebar refresh interval.
Transient network/5xx or in-band upstream errors may retain a sample until
its known reset, or until the age cap when reset is absent. Authentication
failure, malformed schema, missing
`limit7d`, identity loss, opt-out, or account/server change clears it
immediately. Every unavailable state clears the numeric and reset keys while
leaving Kimi context independent. Diagnostics use fixed categories only, with
no URL, account ID, response body, token, or upstream message.

## Experimental API guard

The server API is explicitly experimental and its live authenticated OpenAPI
schema takes precedence over the prose reference. Before implementation,
verify the intended installed Kimi version and response shape in a controlled
fixture or explicitly authorized environment. Runtime parsing must fail
closed on changed envelopes, fields, types, or window semantics and never
interpret another quota bucket as weekly. Feature rollout remains default-off;
a schema change can be handled by disabling the Kimi worker without affecting
local context. No live API or credential endpoint was called for this design.

## Open questions for the owner

1. Which trusted integration will attest the pane → running Kimi process →
   server instance → managed account chain? The currently documented and local
   data do not prove it. Is adding such an integration acceptable, or should
   Kimi weekly remain unavailable?
2. What approved runtime channel will supply the server's Bearer token without
   storing it in Lineup configuration/state or exposing it in process arguments
   or logs? How will server rotation and account switching invalidate a binding?
3. Is retaining a reset-less sample for about five minutes plus timeout and
   one refresh, or a reset-bearing sample until its known reset (potentially
   days) after transient failures acceptable for this experimental API? If
   not, should every failed poll clear the value immediately?
