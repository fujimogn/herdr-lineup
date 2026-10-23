# provider-readers Specification

## Purpose
Define session-bound usage results for local provider readers and for the
opt-in Codex and Grok account quota workers, including unavailable data.

## Requirements

### Requirement: Session-bound provider reader result
Each invoked local provider reader SHALL return a result bound to the
authoritative pane identity.

#### Scenario: Return a validated session sample
- **WHEN** an associated provider is read
- **THEN** the reader result conforms to the independent outcome and measurement
  definitions below

The result SHALL bind `{provider, native_session_id, repository_identity}` from
authoritative Herdr pane/session information. A missing or ambiguous native
session SHALL return unavailable without guessing from a globally newest file.
The publisher SHALL evaluate each selected Claude, Codex, Grok, or Antigravity
pane independently. When its required local source path and association are
available, it SHALL invoke that pane's local reader even for an identity shared
with another pane.
Account quota worker samples MAY be shared across eligible panes.

The result SHALL contain `observed_at`, a non-secret `source_kind`, and independent
context and weekly outcomes. Each outcome SHALL be either `available` with a
finite percentage in [0,100], or `unavailable` with a bounded reason code:
`unsupported`, `not_found`, `permission_denied`, `unassociated`, `invalid`,
`timeout`, `entry_limit`, `stale`, or `source_error`. Context available data SHALL
mean current
conversation occupancy of the active context window, with Codex's explicit UI
baseline convention below, not rate quota.
Weekly available data SHALL mean **remaining** quota percentage over an explicit
calendar-week or rolling-seven-day window; it SHALL include window kind and
reset time when supplied by the source. A shorter or unidentified window SHALL
not be mislabeled weekly. Used percentage SHALL be converted to remaining by
`100 - used`. Readers SHALL reject out-of-range/nonfinite data rather than clamp it.

#### Scenario: Global quota without a context association
- **WHEN** an account-wide weekly sample can be attributed to the pane's provider
  and account, but no current conversation context sample is available
- **THEN** weekly may be available with account-wide scope recorded, while context
  remains unavailable; context is not inferred from weekly usage

#### Scenario: Unsupported window or invalid sample
- **WHEN** a source provides only a five-hour allowance, infinity, or 120%
- **THEN** that weekly result is unavailable and publishes `—` with no numeric key

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
| `kimi` | Completed-turn context from the exact local session, or unavailable | Account-wide rolling-seven-day remaining from the daemon's Kimi home when separately opted in, or unavailable | Implemented local context and default-off, read-only managed API quota worker |
| `hermes` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |

#### Scenario: Kimi weekly stays unavailable
- **WHEN** a Kimi pane has a published context percentage but the separate
  opt-in is absent, `allow_network` is false, or the managed quota is invalid
- **THEN** weekly remaining displays `—` with no numeric or reset key

### Requirement: Freshness and failure do not retain misleading usage
Local reader calls SHALL share a bounded refresh-cycle deadline and retain only
valid, identity-matched observations.

#### Scenario: Validate cached usage before renewal
- **WHEN** a cached sample is considered for the current occupant
- **THEN** it passes these age, identity, permission, and privacy constraints

Local reader calls in one refresh cycle SHALL share one `reader_timeout_ms`
deadline. A slow read MAY exhaust the remaining budget for later panes,
regardless of provider. Account quota queries SHALL run in background workers
without blocking local refresh. A sample that arrives after the local deadline
SHALL be discarded. When the read times out and a previous local observation
exists for the same pane identity and source, that observation SHALL be kept,
and its weekly fields SHALL still expire at their reset or `usage_max_age_ms`.
Other unavailable fields SHALL clear numeric keys and show `—` on the next
refresh.

Samples whose timestamps are invalid, or later than that read's comparison
clock, or whose identity no longer matches the occupant SHALL be unavailable.
The comparison clock SHALL be taken for the pane being read and advanced
through the read, so an observation written while an earlier pane was processed
is not future. Context for Claude, Antigravity, Codex, Grok, and Kimi SHALL retain
the last valid exact session or statusLine-bound observation regardless of its
age. Weekly samples with a reset time SHALL expire at that time; statusLine weekly
samples without a reset time SHALL expire after `usage_max_age_ms`. Codex quota without a reset
SHALL follow the client interval and be cleared after a failed query. The daemon SHALL replace expired or failed
fields with unavailable display and clear numeric values on the next refresh.

Readers SHALL be read-only with respect to agent files and sessions. Network or
credential requirements SHALL be explicit in accepted source research; readers
SHALL NOT acquire new permissions, refresh credentials, or silently enable
network access. With default `allow_network = false`, a network-dependent reader
SHALL return unavailable. Reader results, metadata, and diagnostics SHALL NOT
contain credential values, conversation contents, or personal account identifiers.

#### Scenario: Exact-session context stops updating while idle
- **WHEN** the last valid Claude, Antigravity, Codex, Grok, or Kimi context observation
  exceeds 60 seconds but its binding still matches the current occupant
- **THEN** context retains the last observed percentage
- **AND** weekly without a reset time expires after its maximum age
- **AND** weekly with a reset time remains available only until that reset

#### Scenario: Unrelated Grok session artifacts accumulate
- **WHEN** thousands of unrelated artifacts exist beneath session directories
- **THEN** discovery probes only exact native-session directories for signals
- **AND** duplicate exact matches remain unavailable without a cwd fallback

#### Scenario: Network-disabled reader
- **WHEN** a confirmed reader needs an authenticated network call but network
  access is disabled in user configuration
- **THEN** it returns unavailable and performs no network request

### Requirement: Account quota queries run independently of refresh
`allow_network` SHALL default to false. Only explicit true SHALL enable a
single background worker per configured Codex/Grok provider. A Kimi worker
SHALL additionally require explicit `kimi_weekly_enabled = true` and `kimi`
in `providers`. Refresh SHALL read quota memory without waiting for a provider
request. Missing initial quota SHALL display `—`.

#### Scenario: Slow quota request does not delay sidebar refresh
- **WHEN** provider workers are inside pending requests
- **THEN** refresh publishes only still-valid cached weekly values without
  waiting for them
- **AND** expired reset times clear weekly instead of retaining stale quota
- **AND** repeated refreshes and panes do not spawn additional workers; clients
  preserve bounded 60..=3600 second query intervals, defaulting to five minutes,
  independently of publication

Transient failures SHALL retain the last successful sample only until its
known reset. A sample without a known reset SHALL clear after a failed query.
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
- **THEN** the worker uses `KIMI_CODE_HOME` (or `~/.kimi-code` when unset) to
  select the managed account; pane session and cwd do not select it
- **AND** each Kimi pane receives that weekly sample, regardless of whether
  its local context is available
- **AND** no pane causes another quota request

### Requirement: Kimi context comes from one completed turn
The local Kimi reader SHALL derive context from the selected session's last
valid completed turn and model window, independently of account quota.

#### Scenario: Apply the Kimi local-source contract
- **WHEN** the publisher reads a Kimi pane's local session
- **THEN** source selection, parsing, freshness, and privacy follow these rules:

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

### Requirement: Codex context matches its UI baseline
Codex context SHALL calculate `max(total_tokens - 12000, 0) /
(model_context_window - 12000) * 100`, matching the retired quota reader's
12,000-token baseline in both numerator and denominator. Nonfinite or negative
usage and windows of 12,000 tokens or less SHALL be unavailable.

#### Scenario: Baseline and capacity boundaries
- **WHEN** usage is 26,000 and capacity is 212,000
- **THEN** context is 7%
- **AND** usage at/below 12,000 with that capacity is 0%
- **AND** capacity at/below 12,000 is unavailable

A valid window
with usage at/below the baseline SHALL report 0%. Out-of-range final percentages
SHALL be unavailable rather than clamped. An unfinished final rollout record,
including a split UTF-8 sequence without a newline, SHALL NOT hide a completed
observation earlier in the scanned region. A completed malformed record within
that region SHALL make the source unavailable.

#### Scenario: Bound uncompressed rollout reads
- **WHEN** an uncompressed Codex rollout is at most 2 MiB
- **THEN** the reader scans the full file

#### Scenario: Bound large uncompressed rollout reads
- **WHEN** an uncompressed rollout exceeds 2 MiB
- **THEN** the reader verifies the exact native session against the first
  `session_meta` record and its newline within 256 KiB
- **AND** it scans only the final 2 MiB, discarding an initial partial record
  before decoding UTF-8
- **AND** a `token_count` outside that region is unavailable
- **AND** a header whose newline exceeds 256 KiB produces `SourceError`

#### Scenario: Bound compressed rollout reads
- **WHEN** a zstd rollout is read
- **THEN** the reader scans its decompressed content in full up to 16 MiB
- **AND** exceeding that limit produces `SourceError` because the stream cannot
  be sought

#### Scenario: Reuse an unchanged rollout
- **WHEN** the path, native session ID, device, inode, size, and modification
  time match a cached result
- **THEN** the reader reuses the result without rereading content

#### Scenario: Rollout changes during a read
- **WHEN** the same file grows between the initial file stat and the final path
  and file stats, including between those final stats
- **THEN** the reader returns the complete observation from the scanned prefix
  without caching it

#### Scenario: Reject non-append rollout changes
- **WHEN** the file is replaced, truncated, or modified without size growth
- **THEN** the reader produces `SourceError`

### Requirement: Local statusLine capture and transparent chaining
`herdr-lineup statusline <claude|agy>` SHALL accept at most 1 MiB for JSON
parsing and SHALL atomically replace a private observation under
`$HERDR_PLUGIN_STATE_DIR/statusline/`. The root MUST be absolute. Missing or
invalid state configuration SHALL disable recording. The wrapper SHALL NOT
render usage. Stdout stays empty unless `--then` copies the chained command.

#### Scenario: Broken JSON does not interrupt the existing display
- **WHEN** stdin is invalid JSON or the observation cannot be written
- **AND** `--then` is supplied
- **THEN** the existing command receives the same bytes and its stdout and exit
  status are preserved

The observation SHALL retain only the provider, exact opaque binding key,
receipt time, context percentage, weekly remaining percentage, and optional
absolute reset time. Raw stdin, model names, counters, transcript paths,
account configuration, credentials, and conversation content SHALL NOT be
persisted. Readers SHALL NOT read agent credential files or call provider APIs.

Claude SHALL bind `session_id` to the authoritative pane native session.
Antigravity SHALL bind the inherited `HERDR_PANE_ID` to the authoritative pane ID,
independently of any conversation ID in stdin. These keys SHALL be nonempty,
at most 100 ASCII bytes, and contain only letters, digits, `.`, `_`, `-`, or `:`.
Filenames SHALL encode the key instead of interpolating it as a path component.
No observation SHALL be shared across accounts or unrelated sessions/panes.

Context SHALL accept `context_window`/`contextWindow` and
`used_percentage`/`usedPercentage`, otherwise
`remaining_percentage`/`remainingPercentage` converted to used. Counters alone
SHALL NOT establish context occupancy. Claude weekly SHALL use only
`rate_limits.seven_day.used_percentage` converted to remaining, and
`resets_at` as Unix seconds or RFC3339. Antigravity SHALL choose only the active
model's `gemini-weekly` or `3p-weekly` bucket; an unknown model with two pools
SHALL leave weekly unavailable. `remaining_fraction`/`remainingFraction` SHALL
use [0,1], while `remaining_percent`/`remainingPercentage` SHALL use [0,100],
even for the value 1. Reset fields SHALL accept RFC3339
`reset_time`/`resetTime` or receipt-relative
`reset_in_seconds`/`resetInSeconds`. Five-hour and API buckets SHALL NOT become
weekly. Invalid context and weekly fields SHALL be unavailable independently.

With `--then <command>`, the wrapper SHALL run the explicitly supplied shell
command with the original stdin bytes (including oversized or invalid JSON),
copy its stdout unchanged, inherit its stderr, and return its exit status.
Signals SHALL map to `128 + signal` (capped at 255); command spawn, wait, or
stdout-copy failures SHALL return 1. Recording failure SHALL be silent.
Without `--then`, the wrapper SHALL output nothing and return 0, even if
recording fails. Oversized input SHALL be streamed to the child without being
parsed or stored. Hook installation and user configuration changes remain
explicit opt-in work outside capture.

#### Scenario: A different session or pane binding key rejects context
- **WHEN** a reader key differs from the observation's provider or binding key
- **THEN** both outcomes are unavailable, with no fallback to a latest record

**Existing limitation: Antigravity pane reuse has no capture generation**
The implemented Antigravity statusLine record is version 1 and binds only the
inherited pane ID. It does not store the capture-time native session, terminal
identity, or occupant generation. A replacement using the same pane ID can
therefore read the previous occupant's record; context does not expire by age.
The identity requirements above do not establish generation safety for that
case. A different native ID in the current snapshot cannot validate an old
pane-only record's origin. This is an existing limitation, not a guarantee that
replacement clears usage.

**Planned: Generation-bound Antigravity observations**
Not implemented. A strict path requires a host-owned launch identity delivered
to the capture process and exposed with the authoritative occupant. It must
change for each replacement, including resume of the same conversation, and
remain distinct across endpoint restarts. Missing or mismatched generation
would make usage unavailable. Native session, terminal ID, timestamps and
snapshot revisions are not proven substitutes in the inspected v0.9.3 contract.
Preventing a replacement between read and publication additionally requires
host-side atomic conditional acceptance for that occupant; pre/post snapshots
alone do not prove it.

Migration would preserve existing observation files without promoting them to
generation-bound records or changing user settings automatically. The strict
path, record version/namespace, capability gate, host lifecycle guarantees and
rollback behavior require separate approval and fixture verification. See
[the generation research record](../../../docs/reviews/agy-occupant-generation.md)
for primary evidence, unknowns, production seam and Planned adversarial fixtures.

### Requirement: Reader association is explicit and freshness is independent
`UsageReading` SHALL carry the association type (`native_session` or `pane`)
and matched value. Tokens SHALL accept usage only for the same occupant and
an association matching that pane's current native session or pane ID; they
SHALL NOT branch on agent names to bypass session validation. Native-session
association SHALL continue to require a nonempty exact native ID.

#### Scenario: Idle callback retains context independently of weekly expiry
- **WHEN** the last statusLine callback is older than the configured maximum age
- **THEN** its bound context remains available
- **AND** weekly without a reset displays `—` with no numeric/reset values
- **AND** weekly with a reset remains available until that reset

Readers SHALL reject future observations, oversized or malformed records,
nonregular files, and symlinked observation
roots/files. The read SHALL use the existing bounded timeout contract. An
observation whose supplied weekly reset is at or before `now` SHALL have
unavailable weekly data, while a bound Claude, Antigravity, Codex, Grok, or Kimi
context observation may remain available.
Those context observations SHALL NOT expire by `max_age`. Weekly without a reset SHALL still obey
`max_age`; weekly with a reset SHALL use that reset as its expiry.
Unavailable weekly SHALL clear window/reset values. A missing reset SHALL omit
the countdown without inventing a timestamp. The publisher SHALL select both
local adapters and propagate validated weekly remaining and reset values.

#### Scenario: Receipt age and weekly reset differ
- **WHEN** context is fresh but the supplied weekly reset has already passed
- **THEN** context remains available and weekly becomes unavailable, without
  synthesizing a full renewed allowance

### Requirement: Kimi managed credential is read only from the daemon home
The Kimi worker SHALL use the same `KIMI_CODE_HOME` resolution as the local
reader: a nonempty absolute override or `~/.kimi-code` when unset; a relative
override is unavailable. It SHALL select the managed OAuth credential from
that home and read only its `access_token` and `expires_at` from a regular,
nonsymlinked file. It SHALL never refresh, log, or persist the token.

#### Scenario: Select the default managed credential slot
- **WHEN** the selected account uses the default `oauth/kimi-code` key
- **THEN** read `credentials/kimi-code.json`
- **AND** a configured different OAuth key is unavailable rather than
  guessing another slot

#### Scenario: Selected token is missing or expired
- **WHEN** the selected credential file is absent, unreadable, malformed,
  revoked, empty, or expired
- **THEN** no Kimi request is made and weekly has no numeric or reset key
- **AND** local Kimi context remains independent

#### Scenario: Oversized credential file is unavailable
- **WHEN** the selected credential file exceeds 64 KiB
- **THEN** no token is read and no Kimi request is made

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
{KIMI_CODE_BASE_URL}/usages`, with the only approved base URL
`https://api.kimi.com/coding/v1` and endpoint
`https://api.kimi.com/coding/v1/usages`, with `Accept: application/json` and
transient OAuth Bearer authorization. The destination SHALL be an approved
HTTPS managed API with a matching OAuth credential slot. Requests SHALL be
bounded and SHALL not delay refresh.

#### Scenario: Select a bounded destination
- **WHEN** `KIMI_CODE_BASE_URL` overrides the default
- **THEN** require the exact base `https://api.kimi.com/coding/v1`
- **AND** `https://api.kimi.ai/coding/v1` and custom origins are unavailable
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
RFC3339. Unexpected `limit_7d` fields, types, or semantics SHALL fail closed
without rejecting unrelated quota windows in an otherwise valid response.

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
still-valid sample with a known future reset; a sample without a reset SHALL
clear on a failed query. Authentication and invalid source/schema SHALL clear
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

#### Scenario: Failure clears a reset-less sample
- **WHEN** a transient query failure follows a valid sample without
  `reset_time`
- **THEN** Kimi weekly is unavailable on the next refresh

### Requirement: Reader budget follows the refresh envelope
The shared `reader_timeout_ms` deadline SHALL start after heading resolution
and SHALL never extend past the refresh envelope `E`. When
`min(reader deadline, E)` has passed, the reader SHALL NOT be called and the
pane SHALL receive a `Timeout` observation under the existing timeout rules.
Reader expiry SHALL skip only reader I/O; the pane's report and later
resources SHALL still follow the refresh admission rules.

#### Scenario: Reader budget spent by an earlier pane
- **WHEN** one pane's read consumes the whole reader deadline and a later pane
  has a valid cached observation
- **THEN** the later pane's reader is not called, its cached observation is
  restored as a timeout, and its report is still sent while the envelope
  admits it

### Requirement: One reader cycle per refresh
Each refresh SHALL create exactly one reader cycle and pass it to every local
reader call of that refresh, so candidate discovery is shared only within the
refresh. A reader cycle SHALL NOT be retained across refreshes. The publisher
MAY attach diagnostics it owns for its lifetime; diagnostics SHALL NOT change
reader results.

#### Scenario: Two Codex panes in one refresh
- **WHEN** two Codex panes share one home in a refresh
- **THEN** both reads use the same reader cycle, so the second read may reuse
  the first read's discovery, and the next refresh starts with a fresh cycle

### Requirement: Every local reader worker reports observed diagnostics
The Claude, Agy, Grok, Codex, and Kimi readers SHALL use the observed
bounded read when the reader cycle carries diagnostics, and the unobserved
one otherwise, as a plain `read` does. Both SHALL share the provider's busy
permit and its association, timeout, and result rules, so diagnostics SHALL
NOT change a result. Hermes has no worker and SHALL record no attempt.

#### Scenario: Claude, Agy, and Grok in an observed refresh
- **WHEN** a refresh with diagnostics reads a valid Claude, Agy, or Grok pane
- **THEN** the provider's diagnostics count one attempt and one completion,
  and the reading equals the reading of the same pane without diagnostics

#### Scenario: Busy permit shared by observed and plain reads
- **WHEN** a provider's worker still holds its busy permit
- **THEN** both an observed and a plain read of that provider end as a
  timeout without starting a worker, and only the observed read counts a busy
  rejection

### Requirement: Observed attempts start at the bounded read
A rejection before the bounded read starts, such as an unassociated identity,
SHALL NOT count as a diagnostic attempt. An invalid timeout detected inside
the bounded read SHALL count as one attempt and one invalid end.

#### Scenario: Unassociated and invalid observed reads
- **WHEN** an observed read is rejected as unassociated, and another observed
  read carries a timeout that overflows the deadline
- **THEN** the first adds no attempt, and the second adds one attempt and one
  invalid end and returns invalid

### Requirement: Codex discovery shares one entry cap across roots
Codex discovery SHALL charge entries from `sessions` and `archived_sessions`
against one cap of 4096 directory entries per read. A read that needs a
fallback traversal after a mixed listing SHALL charge the fallback against
the same budget as the first traversal. The 4097th charged entry SHALL make
the context unavailable as an entry limit, not a timeout. Two rollout files
that match the same exact session SHALL make the context unavailable.

#### Scenario: Sessions and archived sessions at the cap
- **WHEN** `sessions` and `archived_sessions` hold 4096 entries together
- **THEN** discovery completes, and one more entry in either root makes the
  context unavailable as an entry limit
