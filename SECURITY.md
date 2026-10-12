# Security

## Current scaffold

The executable only parses command-line arguments and prints help, version,
or a not-implemented message. It does not read agent sessions or configuration,
write files, connect to Herdr, or send data anywhere.

## Planned local data access

The planned usage readers will read only local agent session and configuration
files for the associated pane. These files may contain sensitive content;
readers must extract only the usage fields they need, leave the files unchanged,
and never log session contents or secrets. They will not upload file contents,
credentials, or usage data to remote services. No telemetry is planned.

The planned publisher will send display metadata only to the local Herdr socket.
Repository labels will use local Git refs without fetching. Plugin configuration
and state will live in Herdr's plugin directories. The only planned user-config
writes are the explicit, opt-in `configure` and `unconfigure` actions.

This is a design boundary, not an implemented security guarantee. Update this
document with exact paths, extracted fields, permissions, and limitations when
the readers and publisher are implemented. Provider integrations that require
remote requests need a separate design decision and updated documentation.

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
