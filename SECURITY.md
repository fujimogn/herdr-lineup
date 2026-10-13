# Security

## Runtime data boundaries

Local usage reads and explicit configuration actions are described in the README.
`allow_network` defaults to false. Only explicit `allow_network = true` enables
one Codex account worker and one Grok billing worker for configured providers.
Install/startup never edits this setting or the user's credentials. Quota queries
run on background threads; refresh only reads their non-secret in-memory cache.

On shutdown, no further queries are scheduled. Stop keeps its five-second
deadline and does not join a pending Grok HTTP request; daemon process exit ends
that I/O. When a live daemon accepts shutdown, stop records that process before
the request and returns only after it has exited. It does not signal the
process. If the process is still alive at five seconds, stop fails. After
ensure's own child loses the session lock, ensure waits for that child to exit
before treating another healthy daemon as ready. Codex has a separate child process, so shutdown synchronously kills
its process group and reaps the direct child through a spawn/stop synchronized
owner. Error exits use the same cleanup, without touching the Herdr server.

## Local data read by the Grok billing client

`quota_grok::Client` is polled by the dedicated daemon worker only with network
opt-in. It resolves `GROK_AUTH_FILE`, then
`GROK_HOME/auth.json`, then `~/.grok/auth.json`. It reads at most 64 KiB and
retains only one unique `key` and optional `user_id`, including nested objects.
Ambiguous multiple keys fail closed. It never writes or refreshes credentials.
Other auth fields are discarded during deserialization. Credential strings and
temporary input bytes exist only in memory; they are not persisted or displayed.

Calling `poll` sends an HTTPS GET to
`https://cli-chat-proxy.grok.com/v1/billing?format=credits` with the key as a
Bearer token, `X-XAI-Token-Auth: xai-grok-cli`, and optional `x-userid`.
TLS uses rustls with certificate validation. Redirects are disabled. Environment
proxy settings are inherited, matching the retired quota client. Dependency wire logging is disabled at compile time using
`log/max_level_off`; errors expose only fixed categories, never HTTP bodies,
headers, credential paths or underlying library error messages. No telemetry
or conversation/session data is sent.

Quota values are held only in memory. Case-insensitive period names containing
`WEEKLY` expose used percent and optional reset time; `MONTHLY` returns
`NonWeekly`, so weekly displays stay missing. Without a reset, publication
expires the value at `usage_max_age_ms` after its last successful query;
transient failures do not retain a value with an unknown reset.
The default request interval is five minutes, clamped to one minute through one
hour, independent of the daemon cadence. The total HTTP timeout defaults to ten
seconds and is clamped to one through thirty seconds. Transient connection,
timeout and 5xx failures keep the last successful weekly usage until its reset
time. Authentication failures (401/403 or unreadable/invalid
credentials), invalid responses, and failures without an earlier success return
missing data. All failure paths are throttled; expired reset times clear cached
usage. The synchronous client runs only on its worker. Its one configured auth
source supplies weekly quota to all Grok panes independently of local context.
Restart the Lineup daemon after account changes to discard cached quota.

The daemon saves only fixed query-result categories in private per-session
`weekly-codex.json` and `weekly-grok.json` files under `HERDR_PLUGIN_STATE_DIR`.
Atomic writes replace records without following destination symlinks. `doctor`
reads at most 128 bytes per regular file without following symlinks and validates
the closed enum before displaying it. No quota values, account identities,
authentication paths, tokens, headers, bodies or upstream error messages are saved.
The last query category may differ from retained cache availability and can be
stale after shutdown. Diagnostics do not make provider queries.

Development tests use only temporary fake credentials and loopback HTTP servers.
Authenticated production provider requests are not run during development.

## Local usage data access

Usage readers and the publisher have separate local-data boundaries. This
section describes the implemented contract: extract usage fields only,
never log session contents or secrets, and publish display metadata only to the
local Herdr socket. Plugin settings and state belong in Herdr plugin directories.
Configuration writes require the explicit opt-in actions. See the README for
implemented runtime behavior. Other provider API access is outside this opt-in.

## Repository heading Git queries

Repository headings use local, read-only Git built-in queries without signature
verification. The commands disable Git pagers and filesystem monitors and remove
inherited `GIT_*` environment variables. Lazy object fetches and all remote
protocols are disabled. These queries do not invoke aliases, hooks, credential
helpers, or remote commands.

## Codex app-server quota client

`src/quota_codex.rs` implements the account-quota client called by the dedicated
daemon worker when network access is explicitly enabled. It starts
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

The in-memory `WeeklyClient` is bound to the daemon's resolved `CODEX_HOME`.
Only panes whose exact native session is verified by the local rollout reader
in that home receive its quota. Snapshots do not identify pane-specific homes
or profiles; different homes remain unavailable, and profiles sharing a home
cannot be distinguished. Restart the Lineup daemon after auth/profile changes. Its query interval is independent of
publication cadence: default five minutes, clamped to one minute through one
hour. Successful observations are reused between queries. Transient failures
retain the last success only until its known reset time. Authentication/account
failures, absent subscription quota, unknown reset after failure, and passed
reset times clear the cached value. Failure does not panic or stop a publication
loop. The daemon owns one worker for this home and never queries once per pane.
Lineup does not change user configuration or credentials.

Development tests use synthetic app-server children and injected temporary
homes only. No authenticated provider query is run during development; live
accuracy checks are reserved for the user.

## Reporting a vulnerability

Use this repository's GitHub Security page to report a vulnerability privately
when private reporting is available. Otherwise open an issue asking for a
private contact channel, without including secrets or exploit details.
