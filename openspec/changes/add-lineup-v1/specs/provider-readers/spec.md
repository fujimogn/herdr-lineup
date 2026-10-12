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
resolved by the usage-source research before enabling an adapter; the following
table is a contract, not a statement that the fields can currently be obtained.

#### Scenario: Enumerate reader availability
- **WHEN** a supported provider is selected
- **THEN** its adapter obeys the stage and source-detail status in this table

| Reader | Context outcome | Weekly remaining outcome | Source detail status |
| --- | --- | --- | --- |
| `claude` | Common context contract or unavailable | Common weekly contract or unavailable | TBD research |
| `codex` | Common context contract or unavailable | Common weekly contract or unavailable | TBD research |
| `grok` | Common context contract or unavailable | Common weekly contract or unavailable | TBD research |
| `agy` | Common context contract or unavailable | Common weekly contract or unavailable | TBD research |
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
