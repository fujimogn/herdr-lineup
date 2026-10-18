# Kimi weekly quota design

## Status and source

**Proposed; no Kimi weekly implementation exists.** The shipped Kimi reader
supplies completed-turn context for an exact local session and always returns
unavailable weekly. This design instead uses the managed Kimi account logged in
under the daemon's Kimi home for an account-wide weekly sample shared by every
Kimi pane. A pane's session does not identify its account; the selected daemon
home is the account scope, as with the existing Grok quota source. Local context
remains session-specific and independent of account quota.

The installed Kimi CLI 2.1.1 bundle at
`~/.bun/install/global/node_modules/@moonshot-ai/kimi-code/dist/main.mjs`
provides the structural evidence: `packages/oauth/src/managed-usage.ts`
(`kimiCodeBaseUrl`, `kimiCodeUsageUrl`, `fetchManagedUsage`,
`parseQuotaUsages`, and `parseQuotaEntry`), `storage.ts` (`FileTokenStorage`),
`types.ts` (`tokenFromWire`), and `managed-kimi-code.ts` /
`toolkit.ts` (`resolveKimiCodeRuntimeAuth`, `resolveKimiCodeOAuthKey`,
`resolveKimiTokenStorageName`). This is evidence for this installed version,
not a stable public API guarantee. No credential value or account data was read,
and no Kimi endpoint was called for this design.

## Opt-in and account scope

Add `kimi_weekly_enabled = false` to Lineup's `config.toml` under
`HERDR_PLUGIN_CONFIG_DIR`. A Kimi quota worker may start only when this key and
existing `allow_network = true` are both explicit and `kimi` is in
`providers`. Neither gate alone enables Kimi quota. Disabling either gate
clears the in-memory sample and publishes `—` with no numeric or reset key;
local context continues. Startup must not edit user configuration.

Resolve `KIMI_CODE_HOME` exactly as the shipped local Kimi reader does:
nonempty absolute override, otherwise `~/.kimi-code`; a relative override is
unavailable. One worker reads the managed account selected by that home and
shares one weekly result across all Kimi panes, including panes without a
valid local context reading. Do not use pane cwd, native session ID, local
server, or a per-pane credential to choose the quota account. If the daemon
home or selected OAuth credential changes, clear the cached sample before
publication. A different Kimi home is outside this worker's scope.

## Read-only credential route

Kimi's `FileTokenStorage` stores `<name>.json` under
`<KIMI_CODE_HOME>/credentials/`. For the default managed OAuth key
`oauth/kimi-code`, `resolveKimiTokenStorageName` selects `kimi-code.json`.
For a region or environment-specific login, read
`providers["managed:kimi-code"].oauth.key` from that home's `config.toml`
and apply the same storage-name mapping; do not guess the default file when a
configured key or environment selects another slot. The token file's wire
keys are `access_token` and `expires_at` (Unix seconds). Read only these
fields from a regular, nonsymlinked file under the selected home. Missing,
malformed, empty, revoked, unreadable, or expired credentials make weekly
unavailable. Bound the credential read to 64 KiB; larger files are
unavailable. Do not use `refresh_token`, call Kimi login/refresh, or write the
credential or config. Never place the access token in plugin
config, persistent state, diagnostics, command arguments, or logs. Hold it only
long enough in request memory to set `Authorization: Bearer <access_token>`;
clear request-owned memory on completion as far as the implementation permits.
A 401/403 response clears the quota immediately; the next poll may only use a
currently valid token read from disk. Do not emit a credential or response
value in errors. Inspect file metadata and key names during implementation;
no real credential contents are needed for design or fixtures.

## Bounded request and response

The installed `managed-usage.ts` uses `GET
{KIMI_CODE_BASE_URL}/usages`, defaulting to
`https://api.kimi.com/coding/v1/usages` after trimming trailing slashes.
Lineup shall send only this GET with `Accept: application/json` and the
transient OAuth Bearer header. `KIMI_CODE_BASE_URL` is an input to the
selected daemon environment, not an arbitrary URL from a pane. Accept only an
HTTPS base URL whose normalized origin and path are explicitly approved for
this Kimi managed API and whose OAuth key matches the selected credential
slot; reject userinfo, query, fragment, nonstandard ports, redirects, and
unapproved hosts. The installed bundle also names
`https://api.kimi.ai/coding/v1` for its global region; an override to that
origin needs the matching scoped OAuth key. Other custom overrides remain
unavailable until their destination and key mapping are explicitly approved.
Never send a token to a fallback host after a failure.

Use the existing quota worker bounds: five-minute default interval, configured
interval clamped to 60–3600 seconds, request timeout clamped to 1–30 seconds,
64 KiB maximum response, no redirects, no retry within an interval, and at
most one in-flight request. Refresh reads memory and never waits for HTTP.
Do not start a Kimi server or query session, user-info, login, or credential
endpoints. A request response arriving after timeout, opt-out, or credential
change must be discarded.

The raw JSON response used by the CLI has `usages.limit_7d.used_ratio` and
optional `reset_time`; `parseQuotaUsages` maps those into
`limit7d.usedRatio` and `resetAt`. Unlike the local Server API, this route
has no `code = 0` / `data.kind = "ok"` envelope. Require HTTP success, a
JSON object, `usages.limit_7d`, and a finite ratio in [0, 1] (a numeric
string may be parsed only as the installed CLI does). Remaining percentage is
`100 × (1 - used_ratio)`, with rolling-seven-day window kind. Parse a present
`reset_time` as future RFC3339; malformed or past reset is invalid. If absent,
publish without a countdown and apply the bounded age cap below. Do not
substitute `limit_5h`, monthly windows, or `boosterWallet` for weekly usage.
Keep only percentage, receipt time, and validated reset in quota memory.

## Freshness, failure, and API change

Before the first valid response, show `—`. A sample with a reset expires at
that time. A reset-less sample expires after the greater of
`usage_max_age_ms` and interval + timeout + one sidebar refresh interval.
A transient network/5xx failure may retain only a sample with a known future
reset; a reset-less sample clears on the failed query. Credential
absence/expiry/change, 401/403, malformed or incompatible
schema, missing `limit_7d`, opt-out, or unapproved endpoint clears it
immediately. Never infer a reset or a fresh 100% allowance after expiry.
Diagnostics use fixed categories without URL, token, account identifier,
response body, or upstream message. As with Codex/Grok, a private
`weekly-kimi.json` under `HERDR_PLUGIN_STATE_DIR` may store only the fixed
query-result category, never quota or credential values; the category may
differ from retained cache availability. Local Kimi context is unaffected.

`/usages` and its response shape are not a documented stable Lineup API.
Fail closed on changed `limit_7d` fields, types, or semantics, while allowing
unrelated quota windows in an otherwise valid response;
keep the feature default-off. Check the installed CLI version and synthetic
fixtures before implementation and again for upgrades. Disabling the Kimi
worker must leave local context intact. No live Kimi API or credential
endpoint was called for this design.

## SECURITY.md follow-up

Before enabling the feature, `SECURITY.md` should disclose that explicit
Kimi weekly opt-in makes Lineup read the selected Kimi OAuth `access_token`
from the daemon home's credential file and send it to the approved Kimi
managed API; it does not refresh or persist that token. This differs from the
Codex quota worker, for which Lineup never opens Codex auth files. Document
scope across all Kimi panes, destination restrictions, transient in-memory
handling, error/log exclusions, and the behavior of an expired or rotated
token. This assignment only lists the required disclosure; `SECURITY.md` is
outside its touching scope.

## Open questions for the owner

1. Which exact official Kimi origins beyond the default `.com` host should be
   enabled initially, and how will a custom `KIMI_CODE_BASE_URL` be approved?
2. Should any transient failure clear a reset-bearing sample immediately,
   rather than retaining it until the known reset? The existing Codex/Grok
   worker boundary permits retention until expiry.
3. How should a credential change be detected promptly between polls without
   retaining a token or account identifier? File identity/fingerprint can be
   compared, but the implementation and race behavior need fixture coverage.
