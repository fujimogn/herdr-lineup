# Security

## Current scaffold

Runtime local reads and explicit configuration actions are described in the
README. The new standalone billing client below adds a narrow, separately
authorized remote credential boundary; it is not connected to the runtime.

## Local data read by the Grok billing client

`quota_grok::Client` is implemented as a standalone client; the daemon and Grok
reader do not call it yet. It resolves `GROK_AUTH_FILE`, then
`GROK_HOME/auth.json`, then `~/.grok/auth.json`. It reads at most 64 KiB and
retains only one unique `key` and optional `user_id`, including nested objects.
Ambiguous multiple keys fail closed. It never writes or refreshes credentials.
Other auth fields are discarded during deserialization. Credential strings and
temporary input bytes exist only in memory; they are not persisted or displayed.

Calling `poll` sends an HTTPS GET to
`https://cli-chat-proxy.grok.com/v1/billing?format=credits` with the key as a
Bearer token, `X-XAI-Token-Auth: xai-grok-cli`, and optional `x-userid`.
TLS uses rustls with certificate validation. Redirects and environment proxies
are disabled. Dependency wire logging is disabled at compile time using
`log/max_level_off`; errors expose only fixed categories, never HTTP bodies,
headers, credential paths or underlying library error messages. No telemetry
or conversation/session data is sent.

Results are held only in memory. Only exact `WEEKLY` periods expose used percent
and reset time; `MONTHLY` returns `NonWeekly`, so weekly displays stay missing.
The default request interval is five minutes, clamped to one minute through one
hour, independent of the daemon cadence. The total HTTP timeout defaults to ten
seconds and is clamped to one through thirty seconds. Transient connection,
timeout and 5xx failures keep the last successful weekly usage until its reset
time. Authentication failures (401/403 or unreadable/invalid
credentials), invalid responses, and failures without an earlier success return
missing data. All failure paths are throttled; expired reset times clear cached
usage. The synchronous client must be scheduled independently if a daemon tick
must not wait. Keep a separate client per configured account and recreate it
on account/profile changes before integration.

Development tests use only temporary fake credentials and loopback HTTP servers.
Authenticated production provider requests are not run during development.
The client is not connected to a runtime entry point in this change.

## Planned local data access

Usage readers and the publisher have separate local-data boundaries. This
section retains the scaffold's planned requirements: extract usage fields only,
never log session contents or secrets, and publish display metadata only to the
local Herdr socket. Plugin settings and state belong in Herdr plugin directories.
Configuration writes require the explicit opt-in actions. See the README for
implemented runtime behavior; no broader provider request permission is implied
by the standalone Grok billing client.

## Codex app-server quota client

`src/quota_codex.rs` implements an explicit account-quota client, not yet
connected to the daemon or the local Codex context reader. It starts
`codex app-server` with a caller-supplied absolute `CODEX_HOME` and exchanges
stdio JSON-RPC: `initialize`, `initialized`, `account/read` (without requesting
a token refresh), and `account/rateLimits/read`. It never requests threads or
conversation contents and never opens `auth.json`. Codex uses its own existing
authentication and may contact its provider or maintain its authentication state.
This authenticated quota access was approved on 2026-10-12; other authenticated
provider integrations remain outside this boundary.

Only a seven-day window's used percentage and optional absolute reset time are
returned. API-key and signed-out accounts return no subscription quota. Raw
account metadata, provider errors and child stderr are not logged or persisted.
Responses have per-line and total byte limits. Nonblocking pipe I/O shares one
monotonic deadline (at most 30 seconds), and every completion path kills the
child process group and waits for the direct child. Overlapping public queries
are rejected, so timed-out workers cannot accumulate.

The optional in-memory `WeeklyClient` is bound to one caller-resolved profile;
replace it when that profile changes. Its query interval is independent of
publication cadence: default five minutes, clamped to one minute through one
hour. Successful observations are reused between queries. Transient failures
retain the last success only until its known reset time. Authentication/account
failures, absent subscription quota, unknown reset after failure, and passed
reset times clear the cached value. Failure does not panic or stop a publication
loop. Scheduling and opt-in configuration remain integration work; no daemon
connection or user configuration change is made by this module.

Development tests use synthetic app-server children and injected temporary
homes only. No authenticated provider query is run during development; live
accuracy checks are reserved for the user.

## Reporting a vulnerability

Use this repository's GitHub Security page to report a vulnerability privately
when private reporting is available. Otherwise open an issue asking for a
private contact channel, without including secrets or exploit details.
