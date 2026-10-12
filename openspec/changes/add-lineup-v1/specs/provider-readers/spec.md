## ADDED Requirements

### Requirement: Session-bound provider reader result
Each supported provider SHALL implement the same reader contract, returning a
result bound to `{provider, native_session_id, repository_identity}` obtained
from authoritative Herdr pane/session information. A missing or ambiguous
native-session association SHALL return unavailable without guessing a session
from a globally newest file. Multiple panes for one identity SHALL share one
bounded read per refresh cycle.

#### Scenario: Return a validated session sample
- **WHEN** an associated provider is read
- **THEN** the reader result conforms to the independent outcome and measurement
  definitions below

The result SHALL contain `observed_at`, a non-secret `source_kind`, and independent
context and weekly outcomes. Each outcome SHALL be either `available` with a
finite percentage in [0,100], or `unavailable` with a bounded reason code:
`unsupported`, `not_found`, `permission_denied`, `unassociated`, `invalid`,
`timeout`, `stale`, or `source_error`. Context available data SHALL mean current
conversation tokens used divided by the active context window, not rate quota.
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
The plugin SHALL register these six reader slots and treat unimplemented or
unconfirmed adapters as `unsupported`. Source implementation details SHALL be
resolved by the usage-source research before enabling an adapter. The following
table is the contract for the outcomes each adapter obtains.

#### Scenario: Enumerate reader availability
- **WHEN** a supported provider is selected
- **THEN** its adapter obeys the stage and source-detail status in this table

| Reader | Context outcome | Weekly remaining outcome | Source detail status |
| --- | --- | --- | --- |
| `claude` | Validated statusLine context or unavailable | Session-bound seven-day remaining or unavailable | Implemented capture and local reader; hook opt-in required |
| `codex` | Exact native-session rollout context (`total_tokens / model_context_window`) or unavailable | Unavailable (`unsupported`) | Implemented local reader; no app-server, auth file, or weekly quota |
| `grok` | Exact native-session `signals.json` context or unavailable | Unavailable (`unsupported`) | Implemented local reader; no billing API or auth file |
| `agy` | Validated statusLine context or unavailable | Active-pool weekly remaining or unavailable | Implemented capture and local reader; hook opt-in required |
| `kimi` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |
| `hermes` | Unavailable in stage one | Unavailable in stage one | TBD; enable only after confirmed source |

#### Scenario: Kimi before research acceptance
- **WHEN** a Kimi pane exists but its usage adapter has no confirmed source
- **THEN** its logo and Herdr state still render, while context and weekly display
  are both `—` and their numeric keys are cleared

### Requirement: Freshness and failure do not retain misleading usage
Reader calls SHALL obey `reader_timeout_ms`; a timeout SHALL not block other
providers or daemon TTL renewal. Unavailable fields SHALL clear numeric keys
and show `—` on the next refresh.

#### Scenario: Validate cached usage before renewal
- **WHEN** a cached sample is considered for the current occupant
- **THEN** it passes these age, identity, permission, and privacy constraints

Samples older than `usage_max_age_ms`, whose
timestamps are invalid/future, or whose identity no longer matches the occupant
SHALL be unavailable. The daemon SHALL replace expired or failed fields with
unavailable display and clear numeric values on the next refresh. A fresh cached
sample may be reused only while identity and age remain valid.

Readers SHALL be read-only with respect to agent files and sessions. Network or
credential requirements SHALL be explicit in accepted source research; readers
SHALL NOT acquire new permissions, refresh credentials, or silently enable
network access. With default `allow_network = false`, a network-dependent reader
SHALL return unavailable. Reader results, metadata, and diagnostics SHALL NOT
contain credential values, conversation contents, or personal account identifiers.

#### Scenario: Source stops updating
- **WHEN** the last sample exceeds the default 60-second maximum age
- **THEN** the next refresh publishes `—` and clears the affected numeric key,
  rather than renewing the old percentage indefinitely

#### Scenario: Network-disabled reader
- **WHEN** a confirmed reader needs an authenticated network call but network
  access is disabled in user configuration
- **THEN** it returns unavailable and performs no network request

### Requirement: Local statusLine capture and transparent chaining
`herdr-lineup statusline <claude|agy>` SHALL accept at most 1 MiB for JSON
parsing and SHALL atomically replace a private observation under
`$HERDR_PLUGIN_STATE_DIR/statusline/`. The root MUST be absolute. Missing or
invalid state configuration SHALL disable recording. The wrapper SHALL NOT
render usage. Stdout stays empty unless `--then` copies the chained command.
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

#### Scenario: Broken JSON does not interrupt the existing display
- **WHEN** stdin is invalid JSON or the observation cannot be written
- **AND** `--then` is supplied
- **THEN** the existing command receives the same bytes and its stdout and exit
  status are preserved

#### Scenario: Session or pane replacement does not reuse context
- **WHEN** a reader key differs from the observation's provider or binding key
- **THEN** both outcomes are unavailable, with no fallback to a latest record

### Requirement: Reader association is explicit and freshness is independent
`UsageReading` SHALL carry the association type (`native_session` or `pane`)
and matched value. Tokens SHALL accept usage only for the same occupant and
an association matching that pane's current native session or pane ID; they
SHALL NOT branch on agent names to bypass session validation. Native-session
association SHALL continue to require a nonempty exact native ID.

Readers SHALL reject future observations, observations older than `max_age`,
oversized or malformed records, nonregular files, and symlinked observation
roots/files. The read SHALL use the existing bounded timeout contract. An
observation whose supplied weekly reset is at or before `now` SHALL have
unavailable weekly data, while a fresh context observation may remain available.
Unavailable weekly SHALL clear window/reset values. A missing reset SHALL omit
the countdown without inventing a timestamp. The publisher SHALL select both
local adapters and propagate validated weekly remaining and reset values.

#### Scenario: Idle callback expires
- **WHEN** the last statusLine callback is older than the configured maximum age
- **THEN** context and weekly display `—`, and numeric/reset values are absent

#### Scenario: Receipt age and weekly reset differ
- **WHEN** context is fresh but the supplied weekly reset has already passed
- **THEN** context remains available and weekly becomes unavailable, without
  synthesizing a full renewed allowance
