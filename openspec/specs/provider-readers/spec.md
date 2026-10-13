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
| `kimi` | Completed-turn context from the exact local session, or unavailable | Unavailable | Local session file under `KIMI_CODE_HOME` (default `~/.kimi-code`); no network and no credential values |
| `hermes` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |

#### Scenario: Kimi weekly stays unavailable
- **WHEN** a Kimi pane has a published context percentage
- **THEN** weekly remaining still displays `—` and its numeric key is cleared

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
is not future. Context for Claude, Antigravity, Codex, and Grok SHALL retain
the last valid exact session or statusLine-bound observation regardless of its
age. Kimi completed-turn context SHALL expire after `usage_max_age_ms`. Weekly samples
with a reset time SHALL expire at that time; statusLine weekly samples without a
reset time SHALL expire after `usage_max_age_ms`. Codex quota without a reset
SHALL follow the client interval and be cleared after a failed query. The daemon SHALL replace expired or failed
fields with unavailable display and clear numeric values on the next refresh.

Readers SHALL be read-only with respect to agent files and sessions. Network or
credential requirements SHALL be explicit in accepted source research; readers
SHALL NOT acquire new permissions, refresh credentials, or silently enable
network access. With default `allow_network = false`, a network-dependent reader
SHALL return unavailable. Reader results, metadata, and diagnostics SHALL NOT
contain credential values, conversation contents, or personal account identifiers.

#### Scenario: Exact-session context stops updating while idle
- **WHEN** the last valid Claude, Antigravity, Codex, or Grok context observation
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
single background worker per configured Codex/Grok provider. Refresh SHALL
read quota memory without waiting for a provider request. Repeated refreshes
and panes SHALL NOT spawn additional quota workers. The clients SHALL preserve
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
`100 * tokens / window`, bounded like the other context labels. An observation
older than `usage_max_age_ms` SHALL be unavailable, including when a timeout
restores the previous local reading through the same retention function as
Codex and Grok. That function SHALL keep Codex and Grok context regardless of
age.

The reader SHALL NOT use the network. It SHALL parse `config.toml` with the
`toml` crate and retain only `id`, `cwd`, event type, `agentId`, `tokens`,
`time`, the alias bound to the selected turn, and that alias's `max_input_size`
and `max_context_size`.
Credential, token, and API-key values SHALL NOT be retained or logged. Kimi
weekly remaining SHALL stay unavailable.

#### Scenario: Completed turn publishes context
- **WHEN** one regular session matches the pane's native session id and cwd
- **AND** its main-agent wire ends on a completed turn whose model window is positive
- **AND** the observation is within `usage_max_age_ms`
- **THEN** context is `100 * tokens / window`
- **AND** weekly remaining is unavailable

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

#### Scenario: Old Kimi observation expires
- **WHEN** the completed-turn observation is older than `usage_max_age_ms`
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

#### Scenario: Session or pane replacement does not reuse context
- **WHEN** a reader key differs from the observation's provider or binding key
- **THEN** both outcomes are unavailable, with no fallback to a latest record

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
unavailable weekly data, while a bound Claude, Antigravity, Codex, or Grok
context observation may remain available.
Those context observations SHALL NOT expire by `max_age`. Kimi completed-turn
context SHALL expire after `usage_max_age_ms`. Weekly without a reset SHALL still obey
`max_age`; weekly with a reset SHALL use that reset as its expiry.
Unavailable weekly SHALL clear window/reset values. A missing reset SHALL omit
the countdown without inventing a timestamp. The publisher SHALL select both
local adapters and propagate validated weekly remaining and reset values.

#### Scenario: Receipt age and weekly reset differ
- **WHEN** context is fresh but the supplied weekly reset has already passed
- **THEN** context remains available and weekly becomes unavailable, without
  synthesizing a full renewed allowance
