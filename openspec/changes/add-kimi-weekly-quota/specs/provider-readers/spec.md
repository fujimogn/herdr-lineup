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
| `kimi` | Completed-turn context from the exact local session, or unavailable | Unavailable until separately opted in with proved account binding; otherwise managed rolling-seven-day remaining or unavailable | Local context from `KIMI_CODE_HOME`; proposed quota from a separately authenticated Kimi server worker |
| `hermes` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |

#### Scenario: Kimi weekly stays unavailable
- **WHEN** a Kimi pane has a published context percentage but the separate opt-in or proved account binding is absent
- **THEN** weekly remaining still displays `—` and its numeric key is cleared

#### Scenario: Opt-in without account proof
- **WHEN** `kimi_weekly_enabled = true` and `allow_network = true`, but the
  pane's exact session cannot be proved to use the queried server's managed
  account
- **THEN** Kimi weekly is unavailable with no numeric or reset key
- **AND** valid local Kimi context remains independently available

### Requirement: Account quota queries run independently of refresh
`allow_network` SHALL default to false. Only explicit true SHALL enable a
single background worker per configured Codex/Grok provider. A Kimi worker
SHALL additionally require explicit `kimi_weekly_enabled = true` and a proved
pane-to-managed-account binding; it SHALL be limited to one worker per proved
account/server binding. Refresh SHALL
read quota memory without waiting for a provider request. Repeated refreshes
and panes sharing one proved binding SHALL NOT spawn additional quota
workers. At most one Kimi account/server binding SHALL be configured and
polled in the first implementation; other Kimi accounts remain unavailable.
The clients SHALL preserve
their bounded 60..=3600 second query intervals, with a five-minute default,
independently of publication. Missing initial quota SHALL display `—`.

#### Scenario: Slow quota request does not delay sidebar refresh
- **WHEN** both provider workers are inside pending requests
- **THEN** refresh publishes cached weekly values without waiting for them
- **AND** expired reset times clear weekly instead of retaining stale quota

Transient failures SHALL retain the last successful sample only until its
known reset. Authentication failures SHALL clear it. Passed reset times SHALL
be unavailable even while another request is pending. Codex samples without a
known reset SHALL be discarded after a failed query. Existing client outcome
classification SHALL be preserved; MONTHLY Grok data SHALL NOT become weekly.

Codex quota SHALL be attributed only to an exact native session verified in
the daemon's `CODEX_HOME`. Snapshots do not expose pane-specific home/profile;
different homes SHALL remain unavailable. Profiles sharing a home cannot be
distinguished; authentication/profile changes require restarting Lineup to
discard cached account quota. Grok has one configured auth source and SHALL
share its weekly sample across all Grok panes, including those without context.
Account quota SHALL NOT infer conversation context.

Shutdown SHALL stop scheduling queries. Daemon exit SHALL NOT join a pending
HTTP request; process exit ends that request. Shutdown SHALL kill the Codex
app-server process group and reap the direct child, synchronized with spawn
so a racing child cannot be orphaned. The existing five-second stop deadline
SHALL remain unchanged. Error exits SHALL use the same worker cleanup.

#### Scenario: Network access is not implicitly enabled
- **WHEN** `allow_network` is omitted or false
- **THEN** no quota worker queries the providers
- **AND** Codex/Grok weekly is unavailable while local context remains enabled

#### Scenario: Stop while a provider request is pending
- **WHEN** shutdown is requested during HTTP or app-server I/O
- **THEN** stop finishes within its five-second deadline without joining HTTP
- **AND** no Codex app-server child remains after daemon exit

#### Scenario: Network alone does not enable Kimi quota
- **WHEN** `allow_network = true` but `kimi_weekly_enabled` is absent or false
- **THEN** no Kimi quota request is made and weekly remains unavailable

#### Scenario: Account binding is lost
- **WHEN** the pane session, server instance, or managed account differs from
  the proved binding, or any binding link cannot be checked
- **THEN** the cached Kimi weekly result is cleared before publication
- **AND** no other Kimi pane inherits that result without its own proof

The Kimi worker SHALL use a trusted transient server Bearer-token handoff and
only `GET /api/v1/oauth/usage` for `managed:kimi-code`. It SHALL not start a
server, resume a session, initiate login, read OAuth credentials, or retain or
log tokens, personal account identity, raw responses, or upstream errors.

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
opted-in quota worker MAY supply weekly only under the Kimi account binding
requirements in this change.

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

### Requirement: Kimi seven-day response is validated independently

An authenticated, proved Kimi quota response SHALL be accepted only when the
envelope has `code = 0`, `data.kind = "ok"`, and
`data.quota.usages.limit7d.usedRatio` is finite in [0, 1]. Remaining percentage
SHALL be `100 × (1 - usedRatio)` with rolling-seven-day window kind. Optional
`resetAt` SHALL be parsed as RFC3339 and must be in the future. Other quota
windows and wallet values SHALL NOT substitute for `limit7d`.

#### Scenario: Convert managed seven-day usage
- **WHEN** a proved response returns `limit7d.usedRatio = 0.25` and a future
  RFC3339 `resetAt`
- **THEN** Kimi weekly remaining is 75% with that reset time

#### Scenario: Reject an error envelope or wrong window
- **WHEN** the server returns `data.kind = "error"`, or only `limit5h` or
  `monthTotal` without `limit7d`, and no still-valid cached weekly sample exists
- **THEN** Kimi weekly is unavailable and no numeric or reset value is published

#### Scenario: Expire a sample without a reset
- **WHEN** a successful `limit7d` sample lacks `resetAt` and its receipt age
  exceeds the bounded cache age
- **THEN** Kimi weekly becomes unavailable without inventing a reset time

#### Scenario: Poll without resuming a cold session
- **WHEN** the Kimi weekly worker queries account usage
- **THEN** it never calls a session status or other session endpoint to prove
  association, so a cold session is not resumed by Lineup

### Requirement: Kimi quota polling is bounded and expires

The Kimi worker SHALL poll by default every five minutes with a configured
60–3600 second interval, 1–30 second timeout, and no overlapping request.
A missing reset SHALL expire after the greater of `usage_max_age_ms`
and the poll interval plus request timeout plus one sidebar refresh interval.
A known reset SHALL expire at that reset. Authentication, schema, or identity
failures SHALL clear cached weekly. Transient errors MAY retain only a
still-valid sample.

#### Scenario: Authentication failure clears a cached sample
- **WHEN** a Kimi quota request fails authentication
- **THEN** the previous weekly value and reset are unavailable on the next
  refresh, while local context remains independent
