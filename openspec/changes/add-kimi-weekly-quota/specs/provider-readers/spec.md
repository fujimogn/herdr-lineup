## MODIFIED Requirements

### Requirement: Provider source availability remains explicit
The plugin SHALL support six provider identities for pane rows and invoke
local readers for the five implemented adapters only.

#### Scenario: Enumerate reader availability
- **WHEN** a supported provider is selected
- **THEN** its adapter obeys the stage and source-detail status in this table

The publisher SHALL invoke local readers only for Claude, Codex, Grok,
Antigravity, and Kimi. Hermes SHALL retain unavailable usage without invoking
its unsupported reader type. Source implementation details SHALL be
resolved by the usage-source research before enabling another adapter.

| Reader | Context outcome | Weekly remaining outcome | Source detail status |
| --- | --- | --- | --- |
| `claude` | Validated statusLine context or unavailable | Session-bound seven-day remaining or unavailable | Implemented capture and local reader; hook opt-in required |
| `codex` | Exact native-session rollout context with the 12,000-token UI baseline or unavailable | Subscription seven-day remaining from the daemon home, with network opt-in | Implemented local context and background app-server quota; Lineup never opens Codex auth files |
| `grok` | Exact native-session `signals.json` context or unavailable | Account-wide WEEKLY billing remaining with network opt-in | Implemented local context and background authenticated billing; MONTHLY is unavailable |
| `agy` | Validated statusLine context or unavailable | Active-pool weekly remaining or unavailable | Implemented capture and local reader; hook opt-in required |
| `kimi` | Completed-turn context from the exact local session, or unavailable | Account-wide rolling-seven-day remaining from the daemon's Kimi home when separately opted in, or unavailable | Implemented local context; proposed credential-read and background managed API quota worker |
| `hermes` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |

#### Scenario: Kimi weekly stays unavailable
- **WHEN** a Kimi pane has a published context percentage but the separate
  opt-in is absent, `allow_network` is false, or the managed quota is invalid
- **THEN** weekly remaining displays `—` with no numeric or reset key

### Requirement: Account quota queries run independently of refresh
`allow_network` SHALL default to false. Only explicit true SHALL enable a
single background worker per configured Codex/Grok provider. A Kimi worker
SHALL additionally require explicit `kimi_weekly_enabled = true` and `kimi`
in `providers`. It SHALL use the managed account selected by the daemon's
`KIMI_CODE_HOME` (or `~/.kimi-code` when unset) and share one sample across
all Kimi panes, including panes without local context. Pane session and cwd
SHALL NOT select the quota account. Refresh SHALL read quota memory without
waiting for a provider request. Repeated refreshes and panes SHALL NOT spawn
additional workers. The clients SHALL preserve their bounded 60..=3600
second query intervals, with a five-minute default, independently of
publication. Missing initial quota SHALL display `—`.

#### Scenario: Slow quota request does not delay sidebar refresh
- **WHEN** provider workers are inside pending requests
- **THEN** refresh publishes only still-valid cached weekly values without
  waiting for them
- **AND** expired reset times clear weekly instead of retaining stale quota

Transient failures SHALL retain the last successful sample only while valid.
Authentication failures SHALL clear it. Passed reset times SHALL be
unavailable even while another request is pending. Codex samples without a
known reset SHALL be discarded after a failed query. Existing client outcome
classification SHALL be preserved; MONTHLY Grok data SHALL NOT become weekly.

Codex quota SHALL be attributed only to an exact native session verified in
the daemon's `CODEX_HOME`. Snapshots do not expose pane-specific home/profile;
different homes SHALL remain unavailable. Profiles sharing a home cannot be
distinguished; authentication/profile changes require restarting Lineup to
discard cached account quota. Grok has one configured auth source and SHALL
share its weekly sample across all Grok panes, including those without context.
Kimi SHALL likewise share the selected daemon-home managed account sample
across all Kimi panes. Account quota SHALL NOT infer conversation context.

Shutdown SHALL stop scheduling queries. Daemon exit SHALL NOT join a pending
HTTP request; process exit ends that request. Shutdown SHALL kill the Codex
app-server process group and reap the direct child, synchronized with spawn
so a racing child cannot be orphaned. The existing five-second stop deadline
SHALL remain unchanged. Error exits SHALL use the same worker cleanup.

#### Scenario: Network access is not implicitly enabled
- **WHEN** `allow_network` is omitted or false
- **THEN** no quota worker queries the providers
- **AND** account weekly is unavailable while local context remains enabled

#### Scenario: Stop while a provider request is pending
- **WHEN** shutdown is requested during HTTP or app-server I/O
- **THEN** stop finishes within its five-second deadline without joining HTTP
- **AND** no Codex app-server child remains after daemon exit

#### Scenario: Network alone does not enable Kimi quota
- **WHEN** `allow_network = true` but `kimi_weekly_enabled` is absent or false
- **THEN** no Kimi quota request is made and weekly remains unavailable

#### Scenario: One managed quota sample is shared
- **WHEN** a valid managed quota sample belongs to the daemon's selected Kimi
  home and multiple Kimi panes are present
- **THEN** each Kimi pane receives that weekly sample, regardless of whether
  its local context is available
- **AND** no pane causes another quota request

### Requirement: Kimi context comes from one completed turn
When `kimi` is enabled, the publisher SHALL read the daemon's Kimi home. The
home SHALL be `KIMI_CODE_HOME` when that variable is a nonempty absolute path,
and `~/.kimi-code` when the variable is unset or empty. A non-absolute override
SHALL make the home unavailable. The reader SHALL select exactly one regular,
nonsymlinked `state.json` under `sessions/` whose `id` equals the pane's
nonempty native session id and whose lexically normalized `cwd` equals the pane
cwd. Normalization SHALL NOT resolve symlinks. Zero matches, mismatches, and
more than one readable match SHALL be unavailable. An unreadable `state.json`
whose session directory name equals the pane's native session id SHALL make
context unavailable, because that file could be a duplicate. An unreadable
`state.json` in a differently named session directory SHALL NOT hide one
readable match.

The reader SHALL then read that session's regular, nonsymlinked
`agents/main/wire.jsonl` from its tail, using the same fingerprint and
append-during-read rules as the Codex rollout reader. Directory components,
including `agents` and `main`, SHALL be opened with `O_NOFOLLOW` and SHALL NOT
be followed when they are symlinks. It SHALL use the last main-agent
`token_counting.turn_recorded.tokens` and the model alias bound before that
turn. A binding after that turn, an alias that falls outside the read tail, or
an alias that cannot be determined uniquely SHALL make context unavailable. A
later event that changes context, including `token_counting.rebased`,
`context.apply_compaction`, or an in-progress turn, SHALL make context
unavailable. The alias SHALL resolve to that model's `max_input_size` in
`config.toml`, or `max_context_size` when `max_input_size` is absent. The
reader SHALL parse the file with the `toml` crate and retain only those two
integer fields. An integer below 1, or a value that is not an integer, SHALL
make the window unavailable and SHALL NOT fall back to the other size. The
Kimi home MAY be a symlink, matching Codex resolution of its home. A symlink
at `sessions/` or below SHALL NOT be followed. Unknown
aliases, nonpositive windows, ambiguous model entries, and an input size
greater than the context size SHALL be unavailable. The percentage SHALL be
`100 * tokens / window`, bounded like the other context labels. A completed
turn SHALL remain available regardless of age. A timestamp later than the
read's comparison clock SHALL be unavailable. When a timeout restores the
previous local reading, that restore SHALL use the same retention function as
Codex and Grok and SHALL keep Kimi context regardless of age. An unchanged
`wire.jsonl` SHALL NOT be reread; the reader SHALL use the same fingerprint
cache as the Codex rollout reader.

The reader SHALL NOT use the network. It SHALL parse `config.toml` with the
`toml` crate and retain only `id`, `cwd`, event type, `agentId`, `tokens`,
`time`, the alias bound to the selected turn, and that alias's `max_input_size`
and `max_context_size`.
Credential, token, and API-key values SHALL NOT be retained or logged. The local Kimi context reader SHALL keep weekly unavailable. A separately
opted-in account quota worker MAY supply weekly to all Kimi panes under this
change. Local context SHALL remain independent.

#### Scenario: Completed turn publishes context
- **WHEN** one regular session matches the pane's native session id and cwd
- **AND** its main-agent wire ends on a completed turn whose model window is positive
- **AND** the observation is older than `usage_max_age_ms` and not later than the comparison clock
- **THEN** context is `100 * tokens / window`
- **AND** the local reader supplies no weekly result; any weekly value must
  independently satisfy the opt-in account quota requirements

#### Scenario: Binding and file shape reject a value
- **WHEN** the native session id or normalized cwd does not match, more than one
  session matches, or `state.json` or `wire.jsonl` is a symlink
- **THEN** context is unavailable

#### Scenario: Later context changes and unknown models reject a value
- **WHEN** a turn is still in progress, a compaction or token rebase follows the
  recorded turn, a model binding follows the selected turn without a new
  completed turn, the alias is outside the read tail, or the model alias has no
  positive unambiguous window
- **THEN** context is unavailable

#### Scenario: A model switch uses the alias in effect for the new turn
- **WHEN** one completed turn is bound to alias A and a later completed turn is
  bound to alias B
- **THEN** context uses alias B's window

#### Scenario: Old Kimi observation stays
- **WHEN** the completed-turn observation is older than `usage_max_age_ms`
- **AND** no later event changes context and the timestamp is not in the future
- **THEN** context retains `100 * tokens / window`

#### Scenario: Future Kimi observation is rejected
- **WHEN** the completed-turn timestamp is later than the read's comparison clock
- **THEN** context is unavailable

## ADDED Requirements

### Requirement: Kimi managed credential is read only from the daemon home
The Kimi worker SHALL use the same `KIMI_CODE_HOME` resolution as the local
reader: a nonempty absolute override or `~/.kimi-code` when unset; a relative
override is unavailable. It SHALL select the managed OAuth credential from
that home and read only its `access_token` and `expires_at` from a regular,
nonsymlinked file. It SHALL never refresh, log, or persist the token.

#### Scenario: Select the configured credential slot
- **WHEN** `providers["managed:kimi-code"].oauth.key` is present in that
  home's `config.toml`
- **THEN** map it with the installed CLI's `resolveKimiTokenStorageName` rule
- **AND** do not guess the default file when another slot is selected
- **AND** the default `oauth/kimi-code` key maps to
  `credentials/kimi-code.json`

#### Scenario: Selected token is missing or expired
- **WHEN** the selected credential file is absent, unreadable, malformed,
  revoked, empty, or expired
- **THEN** no Kimi request is made and weekly has no numeric or reset key
- **AND** local Kimi context remains independent

#### Scenario: Credential stays transient
- **WHEN** an access token is used for a request
- **THEN** only request memory may hold it to form the Authorization header
- **AND** `refresh_token` is not read; login, refresh, and Kimi-file writes
  are never initiated
- **AND** no token or account value enters Lineup state, logs, diagnostics,
  or arguments

#### Scenario: Credential changes after a sample
- **WHEN** the selected home, OAuth key, or credential changes
- **THEN** the old quota sample is cleared before publication
- **AND** a late response from the old request is discarded

### Requirement: Kimi managed request is bounded and destination restricted
When both opt-in gates are true, the worker MAY issue only `GET
{KIMI_CODE_BASE_URL}/usages`, defaulting to
`https://api.kimi.com/coding/v1/usages`, with `Accept: application/json` and
transient OAuth Bearer authorization. The destination SHALL be an approved
HTTPS managed API with a matching OAuth credential slot. Requests SHALL be
bounded and SHALL not delay refresh.

#### Scenario: Select a bounded destination
- **WHEN** `KIMI_CODE_BASE_URL` overrides the default
- **THEN** trim trailing slashes and require an approved HTTPS origin/path
  and matching OAuth credential slot
- **AND** recognize `https://api.kimi.ai/coding/v1` only with its matching
  global-region slot; custom origins remain unavailable pending approval
- **AND** reject userinfo, query, fragment, nonstandard ports, redirects,
  and fallback hosts

#### Scenario: Enforce request bounds
- **WHEN** the Kimi worker polls
- **THEN** interval defaults to five minutes and clamps to 60–3600 seconds;
  timeout clamps to 1–30 seconds and response size to 64 KiB
- **AND** at most one request is in flight, with no retry inside an interval
- **AND** refresh never waits for HTTP; late responses after timeout,
  opt-out, or credential change are discarded
- **AND** it does not contact Kimi server, session, user-info, login, or
  credential endpoints

#### Scenario: Unapproved base URL cannot receive a token
- **WHEN** `KIMI_CODE_BASE_URL` names an unapproved destination or its OAuth
  credential slot cannot be matched
- **THEN** no request is made and weekly is unavailable

### Requirement: Kimi seven-day response is validated independently
The managed `/usages` JSON SHALL be accepted only after HTTP success and
strict shape validation of `usages.limit_7d.used_ratio` as a finite value in
[0, 1]. Remaining percentage SHALL be `100 × (1 - used_ratio)` with
rolling-seven-day window kind. A present `reset_time` SHALL parse as future
RFC3339. Unexpected fields, types, or semantics SHALL fail closed.

#### Scenario: Convert managed seven-day usage
- **WHEN** a valid managed response has `limit_7d.used_ratio = 0.25` and a
  future RFC3339 `reset_time`
- **THEN** Kimi weekly remaining is 75% with that reset time in every Kimi pane
- **AND** only percentage, receipt time, and validated reset are retained

#### Scenario: Parse the installed managed API shape
- **WHEN** `/usages` returns raw `limit_7d.used_ratio` and optional
  `reset_time`
- **THEN** interpret them as the installed CLI's `limit7d.usedRatio` and
  `resetAt`, accepting a numeric string only by the same finite conversion
- **AND** do not parse the local Server API's `code` / `data.kind` envelope
- **AND** omit the countdown when `reset_time` is absent

#### Scenario: Reject a wrong window or response shape
- **WHEN** the response lacks `limit_7d`, has only `limit_5h` or a monthly
  window, has only wallet values, or changes the expected schema
- **THEN** Kimi weekly is unavailable with no numeric or reset key

### Requirement: Kimi quota polling expires and fails closed
A sample with a known reset SHALL expire at that reset. A reset-less sample
SHALL expire after the greater of `usage_max_age_ms` and interval + timeout +
one sidebar refresh interval. Transient network/5xx failure MAY retain only a
still-valid sample. Authentication and invalid source/schema SHALL clear
cached weekly immediately. Local context SHALL remain independently available.

#### Scenario: Authentication failure clears a cached sample
- **WHEN** a Kimi managed request fails authentication
- **THEN** the previous weekly value and reset are unavailable on the next
  refresh, while local context remains independent

#### Scenario: Invalid source clears the sample
- **WHEN** credentials are missing, expired, or changed, a request returns
  401/403, the schema is malformed or lacks `limit_7d`, the user opts out,
  or the destination is unapproved
- **THEN** cached weekly is cleared immediately
- **AND** diagnostics use fixed categories without URL, account, token,
  response body, or upstream message

#### Scenario: Reset-less sample ages out
- **WHEN** a successful sample has no `reset_time` and exceeds its age cap
- **THEN** Kimi weekly is unavailable without inventing a reset
