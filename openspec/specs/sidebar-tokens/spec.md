# sidebar-tokens Specification

## Purpose
Publish pane and workspace sidebar metadata for agent identity, state, context,
weekly quota, logos, and the locally known repository ship time.

## Requirements

### Requirement: Pane token contract
The plugin SHALL publish the following closed set of pane tokens using
`pane.report_metadata`, explicit `pane_id`, and source `herdr-lineup`. It SHALL
support only `claude`, `codex`, `grok`, `agy`, `kimi`, and `hermes` in v1.

#### Scenario: Initialize a supported pane's contract
- **WHEN** a supported agent occupant is reconciled
- **THEN** its tokens conform to this table and the following formatting and
  clearing rules

| Name | Value | Source of value |
| --- | --- | --- |
| `lineup_agent` | One supported lowercase agent id | Authoritative Herdr pane identity |
| `lineup_logo` | One PUA glyph or ASCII fallback label | Verified vendor table and logo mode |
| `lineup_state` | `idle`, `working`, `blocked`, `done`, or `unknown` | Herdr pane `agent_status` |
| `lineup_state_idle` | `●` only while idle; otherwise cleared | Normalized state |
| `lineup_state_working` | `●` only while working; otherwise cleared | Normalized state |
| `lineup_state_blocked` | `●` only while blocked; otherwise cleared | Normalized state |
| `lineup_state_done` | `●` only while done; otherwise cleared | Normalized state |
| `lineup_state_unknown` | `●` only while unknown; otherwise cleared | Normalized state |
| `lineup_context_pct` | Finite unit-less decimal in [0, 100], or cleared | Session-bound context reader |
| `lineup_context_text` | Rounded integer with at least two digits plus `%`, or `—` followed by two U+2800 characters | Context percentage |
| `lineup_weekly_pct` | Finite unit-less remaining percentage in [0, 100], or cleared | Session-bound weekly reader |
| `lineup_weekly_text` | `7d` plus at least two-digit rounded remaining percentage and optional reset countdown, or `—` | Weekly remaining percentage and reset timestamp |
| `lineup_group` | Repository name on the first pane only, otherwise cleared | Same value as workspace `lineup_repo` when a repository resolves |
| `lineup_group_ship_text` | Ship-time text on the first pane only, otherwise cleared | Same value as workspace `lineup_ship_text` when a repository resolves |

Decimals SHALL use ASCII digits, optional decimal point, no whitespace or unit.
Display rounding SHALL be nearest integer with halves rounded upward. Missing
numeric values SHALL be JSON null, not `0`, `—`, an empty numeric sentinel, or
the previous occupant's value. Exactly one state dot SHALL be set. Sidebar
consumers SHALL use separate static colors for the five dot token names or
Herdr's semantic `state_icon`; numeric rules SHALL consume numeric tokens,
not `%` display strings. Colors and row order are user configuration.

Context unavailable text SHALL be U+2014 EM DASH followed by two U+2800
BRAILLE PATTERN BLANK characters. Under the narrow-ambiguous terminal policy,
these each occupy one cell, matching the three-cell `00%` through `99%` display.
U+2800 padding SHALL survive token whitespace normalization. Cell widths SHALL
be tested with a fixed table for this policy, independently of the host locale.
[Unicode East Asian Width data](https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt)
classifies U+2014 as Ambiguous (`A`) and U+2800 as Neutral (`N`); terminals with
wide ambiguous rendering may display the dash as two cells, so the three-cell
alignment contract assumes narrow ambiguous rendering. Rounded context `100%`,
including values at or above 99.5%, SHALL remain a four-cell exception rather
than hiding full usage. Its following weekly column moves one cell right.

Weekly display SHALL use `7d NN%` with at least two digits (`00%`, `07%`, `100%`),
with the nearest integer remaining percentage and halves rounded upward. When a session-bound reset timestamp is available,
append a space and the remaining duration: `XdYh` for at least one day, `XhYm`
for at least one hour, otherwise `Xm`. Duration components SHALL be floored;
past reset timestamps SHALL show `0m`. Examples are `7d 67% 6d9h`,
`7d 67% 3h26m`, and `7d 67% 12m`. A missing reset timestamp SHALL omit the
countdown (`7d 67%`); a missing remaining percentage SHALL show only `—`, even
if a reset timestamp is available. Current Unix time SHALL be injected into
formatting so date and duration boundaries can be tested deterministically.
The separate weekly numeric token SHALL remain unit-less and contain no period
label or countdown.

#### Scenario: Missing context preserves the weekly column
- **WHEN** context is unavailable on a supported pane
- **THEN** context numeric is cleared and context text is `—` followed by two
  U+2800 characters, occupying three cells under narrow ambiguous rendering
- **AND** `07%`, `42%`, and `99%` occupy the same three cells
- **AND** context rounding to `100%` preserves all four cells as an exception

#### Scenario: Single-digit weekly remaining preserves countdown position
- **WHEN** weekly remaining is 7.2% and reset is twelve minutes away
- **THEN** weekly numeric remains `7.2` and weekly text is `7d 07% 12m`
- **AND** the countdown starts at the same cell as in `7d 67% 12m`
- **AND WHEN** weekly data is unavailable
- **THEN** weekly numeric is cleared and text stays the unpadded `—`

#### Scenario: Weekly reset countdown
- **WHEN** weekly remaining is 67% and reset is six days and nine hours away
- **THEN** weekly numeric is `67` and weekly text is `7d 67% 6d9h`
- **AND WHEN** the reset timestamp is unavailable
- **THEN** weekly numeric remains `67` and weekly text is `7d 67%`

#### Scenario: Partial usage availability
- **WHEN** a Claude pane is working with context 7.2% and no readable weekly data
- **THEN** context tokens are `7.2` and `07%`, weekly numeric is cleared, weekly
  text is `—`, state is `working`, only `lineup_state_working` is `●`, and the
  logo corresponds to Claude rather than another provider

#### Scenario: Measured zero and full context
- **WHEN** context is 100% and weekly remaining is a measured 0%
- **THEN** context text is `100%` and weekly numeric/text are `0` and `7d 00%`
  when the reset timestamp is unavailable,
  distinguishable from unavailable data

#### Scenario: Occupant replacement
- **WHEN** a pane changes agent or native session, becomes unsupported, or loses
  its authoritative occupant identity
- **THEN** all previous occupant usage is invalidated before publication; an
  unsupported/unidentified pane has all twelve agent keys cleared; its two
  heading keys follow the workspace heading rules independently, and a new
  supported occupant receives its own logo/state with unavailable usage until read

### Requirement: Workspace heading contract
The plugin SHALL publish exactly these workspace keys using
`workspace.report_metadata`, explicit `workspace_id`, and source `herdr-lineup`.

#### Scenario: Resolve a workspace repository
- **WHEN** a workspace is reconciled
- **THEN** its heading follows this token table, repository-resolution rules,
  heading states, and local timestamp formatting

| Name | Value | Source of value |
| --- | --- | --- |
| `lineup_repo` | Repository name, or cleared when the heading is Unknown | Local Git repository identity |
| `lineup_ship_unix` | Unit-less integer Unix seconds, or cleared | Latest commit of local `refs/remotes/origin/main` |
| `lineup_ship_text` | `HH:MM` for today, otherwise `M/D`, `—` for a confirmed repository without that ref, or cleared when the heading is Unknown | Commit timestamp converted to local timezone |

The repository SHALL be resolved from workspace cwd; when that is unavailable,
use the cwd of the lexicographically first pane id with an available cwd in that
workspace. The repository name SHALL be the basename of the canonical working
repository root derived from Git's common directory, so linked worktrees use
the same repository name.
The plugin SHALL read the latest local `origin/main` commit's committer timestamp
with `git -C <workspace-cwd> --no-pager -c core.fsmonitor=false for-each-ref
--format='%(refname) %(committerdate:unix)' refs/remotes/origin/main` and accept
only output whose refname exactly equals `refs/remotes/origin/main` (excluding
descendant refs such as `refs/remotes/origin/main/topic`). It SHALL do so without
fetching or contacting a remote. It SHALL NOT substitute HEAD, mtime, author date, or
another branch when that ref is absent. "Last ship" means this locally known
commit time, not proof of deployment or remote freshness.
Each Git query SHALL run within `min(1 s, G − now)`, where `G` is the cycle's
shared Git deadline: one second after heading resolution starts, and never
later than the refresh envelope. All queries SHALL stop when the daemon is
shutting down. Each Git child SHALL run in its own
process group. That group, including descendants, SHALL be killed and the
leader reaped; the leader SHALL stay unreaped until descendants are killed,
including when stdout has already closed. Workspaces SHALL be read in a fair
queue keyed by workspace ID: a workspace whose read starts moves to the tail
of the queue, workspaces sharing one cwd share one read per cycle, and
workspaces not reached before `G` are not attempted in that cycle.

Each workspace heading SHALL be in exactly one state per refresh:

| State | Condition | Workspace keys | First-pane group keys |
| --- | --- | --- | --- |
| Confirmed | Git returned a repository together with its verification instant | name; ship text, or `—` when `origin/main` is confirmed absent | copied |
| Fallback | The read was interrupted or not attempted, the previous heading was Confirmed with the same cwd and occupants, and less than 30 s passed since its original verification instant | previous values | copied |
| Unknown | Every other case, including a completed read that returned no repository | null | null |

An interrupted or unattempted read SHALL NOT be treated as a confirmed missing
repository. A completed read without a repository also covers rejection
(`safe.directory`, ownership) and unreadable state; it SHALL discard the
previous heading and SHALL NOT start a fallback. A fallback SHALL NOT renew the
verification instant, and a cached Git result SHALL age from its original
verification, not from the refresh that reused it.

Formatting SHALL use the daemon host's local timezone and date. `HH:MM` uses
two digits per field; `M/D` has no leading zeros. All dates other than today,
including future dates, SHALL use `M/D`.

The plugin SHALL also publish `lineup_group` and `lineup_group_ship_text` on
exactly the first pane of each workspace whose heading is Confirmed or
Fallback, copying `lineup_repo` and `lineup_ship_text` respectively. First pane
SHALL mean the first record for that workspace in the snapshot's `panes` array,
including unsupported or unidentified occupants, without sorting pane IDs and
independently of any queue order. One shared
selection function SHALL drive both pane headings and logo indentation.
Every refresh SHALL recompute this selection. Both heading keys SHALL be JSON
null on later panes and on all panes of workspaces whose heading is Unknown.
A confirmed repository without a local `origin/main` SHALL retain its pane
repository name and show `—` for its pane ship-time text.

#### Scenario: Pane heading moves after reordering
- **WHEN** the snapshot order changes from `w1:p10, w1:p2` to `w1:p2, w1:p10`
- **THEN** `w1:p10` receives null for both heading keys and `w1:p2` receives
  the workspace repository name and ship-time text, even if the old first
  pane has no supported agent

#### Scenario: Workspace loses its repository
- **WHEN** a formerly resolved workspace becomes non-Git, unresolved, or is
  rejected by Git
- **THEN** both pane heading keys and the workspace `lineup_repo` and
  `lineup_ship_text` are cleared immediately, without a fallback

#### Scenario: Interrupted heading falls back for a bounded time
- **WHEN** a heading was confirmed at `t0` and later reads are interrupted or
  not attempted while cwd and occupants stay the same
- **THEN** the previous heading is published until `t0 + 30 s` and cleared
  from then on, even though the occupants are unchanged

#### Scenario: New workspace not yet read
- **WHEN** a new workspace is not reached by the Git queue in its first cycle
- **THEN** its heading keys are null, never `—`

#### Scenario: Today's shipment
- **WHEN** the latest local origin/main commit is today at 09:05 local time
- **THEN** the workspace's ship text is `09:05`, numeric time is the same
  commit's Unix seconds, and no fetch occurs

#### Scenario: Previous date or missing ref
- **WHEN** the commit date is October 11 and today is October 12
- **THEN** ship text is `10/11`
- **AND WHEN** origin/main is absent from a confirmed repository
- **THEN** ship numeric is cleared and ship text is `—`, while the repository
  name remains visible

#### Scenario: Ref and date changes
- **WHEN** another process updates origin/main or the daemon crosses local midnight
- **THEN** the next refresh recomputes the heading from local refs and the current
  date, even without a workspace focus event

### Requirement: Limits, patches, expiry, and refresh
All token names SHALL start with `lineup_`, match `[A-Za-z0-9_-]{1,32}`, and values
SHALL be normalized single-line strings of at most 80 Unicode characters,
truncated at character boundaries. Empty normalized values and JSON null SHALL
clear keys; omitted keys SHALL remain unchanged.

#### Scenario: Publish and renew a resource patch
- **WHEN** a resource has new values or live keys approaching their deadline
- **THEN** its patch obeys the following expiry, sequencing, refresh, and capacity
  rules

Publication SHALL use the daemon lifecycle connection rules: one new
connection per `session.snapshot` or `report_metadata` request, because Herdr
v0.9.3 closes after each JSON response, and no connect when the socket device
or inode differs from the startup identity. Frame deadlines follow that same
contract (non-blocking I/O and `poll`, not `setsockopt`, because macOS returns
`EINVAL` after the peer closes the socket).

Every live-key report SHALL
carry `ttl_ms = 3 * refresh_interval_ms`, source `herdr-lineup`, and a strictly
increasing per-resource `seq` covered by a durable per-endpoint upper bound
persisted before sending. Clearing reports SHALL also use that source and
increasing sequence. Restart SHALL discard unused reservation windows and continue
strictly above the durable per-endpoint upper bound for the same socket/session
and resource.

After an authoritative snapshot, checkpoint rows for resources absent from that
snapshot SHALL be removed only after their maximum reserved sequence is retained
as a durable floor for the same endpoint. The publisher SHALL maintain a durable
upper bound per endpoint covering all allocated sequences. When an allocation
exceeds the durable upper bound, a new reservation window (sized to the authoritative
pane and workspace counts, bounded within 32..=4096) SHALL be fsynced with its
parent directory before any report using that sequence is sent. If any
checkpoint write fails, including a directory sync after a successful rename, the
publisher SHALL send no further report and write no further checkpoint, across
refresh, clear, and reconnect, until a new publisher instance reloads the
checkpoint. A resource ID that reappears on that endpoint SHALL use a sequence
greater than the endpoint cursor within the same process, and greater than the
loaded durable upper bound after a restart. Rows and floors for a different
socket endpoint SHALL be removed once the current endpoint identity is verified;
its metadata cannot be updated through the replaced endpoint. Empty patches and resources skipped due to
capacity limits SHALL not allocate or consume sequences.

The default refresh interval SHALL be 5,000 ms and TTL 15,000 ms. Startup and
workspace focus SHALL invoke `ensure`; the publisher SHALL NOT subscribe to
pane or workspace change events. Each periodic refresh SHALL obtain an
authoritative session snapshot, invalidate cached usage when pane identity or
cwd changes, check local refs and reader freshness, and reconcile changes in
pane agent/status/session, workspace membership, and cwd. Send changed keys
only except TTL renewal and snapshot repair. Unchanged live keys SHALL be
re-published once at least one refresh interval has elapsed since their last
successful publication when the refresh admission rules permit a frame. A desired
live key absent from the authoritative snapshot SHALL be scheduled for repair
on that refresh even before the interval elapses, subject to the same admission
rules. A long or deferred cycle MAY leave a live key expired until its next
accepted report, as specified by degraded expiry under long cycles. Missing or closed resources SHALL
not be recreated.

The contract uses **14** pane keys (longest name `lineup_group_ship_text`, 22
characters) and **3** workspace keys (longest `lineup_ship_unix`/`lineup_ship_text`,
16 characters), each full patch fitting Herdr's **16 keys per report** and each
resource fitting **32 stored keys**. The daemon SHALL count existing unrelated
keys before adding new keys. Pane publication SHALL first drop optional populated
keys in reverse priority order: `lineup_weekly_pct`, `lineup_context_pct`,
`lineup_state`, then `lineup_agent`. Dropped keys SHALL be cleared with JSON null
so previously published values do not survive capacity pressure. Populated
`lineup_logo`, the active state dot, `lineup_context_text`, `lineup_weekly_text`,
and the first-pane `lineup_group`/`lineup_group_ship_text` SHALL be retained.
Insufficient capacity for these required keys SHALL produce a diagnostic and
skip that resource without evicting other sources. Workspace capacity exhaustion
SHALL still skip the resource. Optional keys SHALL return when capacity permits.
No state change needs
more than fourteen keys, including clears. It SHALL publish no undocumented v1 keys.

#### Scenario: Daemon disappears
- **WHEN** the default daemon stops publishing without clearing tokens
- **THEN** each live token expires no later than 15 seconds after its last accepted
  update, and a restart republishes current values with newer sequences

#### Scenario: No changes while alive
- **WHEN** usage and state remain unchanged for one refresh interval after publication
  and the next refresh admits their reports
- **THEN** that refresh renews those live keys instead of waiting for a second interval
- **AND** a report deferred by the deadline follows the degraded expiry contract

#### Scenario: Reuse a disappeared pane ID after a publisher restart
- **WHEN** a pane disappears, its checkpoint row is compacted, and the same pane
  ID later reappears on the same socket endpoint after a publisher restart
- **THEN** the new report sequence exceeds the compacted pane's last reserved
  sequence, including a sequence whose send outcome was ambiguous

#### Scenario: Other plugin consumes capacity
- **WHEN** adding lineup keys would exceed 32 keys on a resource
- **THEN** the daemon clears lower-priority optional pane keys until the patch
  fits, preserving all populated required fields and all unrelated tokens
- **AND** if required fields still do not fit, it reports capacity exhaustion
  and skips that resource without clearing or replacing unrelated tokens

### Requirement: Vendor logo mapping and fallback
The plugin SHALL use the following verified Herdr Agent Icons Max mapping.

#### Scenario: Select a vendor glyph
- **WHEN** a supported agent row is rendered
- **THEN** its logo comes from this exact vendor table and mode selection rules

| Herdr agent id | Vendor label / text fallback | Codepoint | Binary cmap glyph id |
| --- | --- | --- | --- |
| `claude` | `Claude` | U+E1A0 | 1 |
| `codex` | `Codex` | U+E1A1 | 2 |
| `grok` | `Grok` | U+E1B1 | 18 |
| `agy` | `Antigravity` | U+E1B2 | 19 |
| `kimi` | `Kimi` | U+E1A6 | 7 |
| `hermes` | `Hermes` | U+E1AA | 11 |

These vendor slots SHALL remain within U+E1A0–U+E1BA. U+E1C0–U+E1C5 belong to
radar state glyphs and SHALL NOT be assigned to vendors; lineup v1 uses U+25CF
`●` for its dots. The design records the inspected binary hash and range check.
The plugin SHALL bundle the verified HerdrAgentIconsMax icon font and SHALL
install it or edit terminal maps only through explicit opt-in font actions.
Those actions SHALL map U+E1A0–U+E1BA only, leaving U+E1C0–U+E1C5 reserved. In `text` mode it SHALL emit the labels above. In `auto` mode it SHALL use
text unless usable terminal font rendering is affirmatively known; font presence
alone SHALL NOT count as terminal-map verification. Explicit `font` mode SHALL
emit the PUA glyph and doctor SHALL explain the required font/map setup.

#### Scenario: Font is unavailable or rendering cannot be verified
- **WHEN** default auto mode cannot confirm terminal rendering support
- **THEN** the Codex logo token is `Codex`, never a missing-glyph box

### Requirement: Semantic state is owned by Herdr
The plugin SHALL read pane `agent_status` and normalize only the effective Herdr
states listed below; missing or unrecognized values SHALL become `unknown`.
It SHALL NOT infer state from quota, terminal-title animation, inactivity age,
or its own provider reader, and SHALL NOT call `pane.report_agent`.

#### Scenario: Normalize a known semantic state
- **WHEN** Herdr reports one of the five effective states
- **THEN** lineup preserves that state with the following meaning and suggested
  user color

| State | Meaning from Herdr | Suggested user color |
| --- | --- | --- |
| `working` | Agent is active | Blue |
| `blocked` | Agent requires attention | Red |
| `done` | Agent is idle and not yet seen | Green |
| `idle` | Agent is idle | Gray |
| `unknown` | No known effective state | Muted gray |

Suggested colors are not fixed hex values. There are no radar idle-fresh/stale
substates in lineup v1. State text remains available so color is not the only
way to distinguish states.

#### Scenario: Unknown state from a newer server
- **WHEN** a supported agent pane reports an unrecognized agent_status
- **THEN** lineup_state is `unknown`, only the unknown dot is set, and Herdr's
  lifecycle authority is untouched

### Requirement: Cooperative refresh envelope
Each refresh SHALL compute one envelope at its start `t0`:
`E = t0 + 2 s + 1 s + reader_timeout_ms + 2 s` (7 s by default). The snapshot
request, every metadata request and any reconnect they perform SHALL receive
the remaining time to `E`, further capped by their local connect, write and
read limits. `E` is cooperative: synchronous filesystem work, Git child
reaping and scheduling delay MAY overrun it.

#### Scenario: Slow snapshot and reports share one end
- **WHEN** the snapshot and several reports are slow but live
- **THEN** every exchange uses `E` as its I/O waiting deadline, subject to
  synchronous OS work and scheduling overruns under the cooperative envelope

### Requirement: Metadata admission threshold
A metadata request SHALL start only while at least 2 s remain before `E`,
checked after any reservation persist and immediately before the exchange. A
request that is not admitted SHALL write no frame, SHALL NOT be an error, and
SHALL NOT cause a reconnect; its resource SHALL be counted once per refresh as
deadline skipped. A refused admission SHALL NOT stop later resources whose
reports need no frame.

#### Scenario: Slow reports exhaust the envelope
- **WHEN** every metadata response is slow and only some reports fit before
  the admission threshold
- **THEN** the remaining resources needing a frame are counted as deadline
  skipped, no frame is written for them, and the daemon does not reconnect

### Requirement: Admitted requests are never replayed
An admitted metadata request that fails for any reason, including `E`
expiring in flight, SHALL be treated as possibly delivered. Its sequence SHALL
NOT be reused, the request SHALL NOT be resent on any stream, and the refresh
SHALL end with an error.

#### Scenario: Envelope expires during an exchange
- **WHEN** an admitted metadata request is written and the response does not
  arrive before `E`
- **THEN** the refresh ends with an error, the request is not resent, and the
  next report for that resource uses a higher sequence

### Requirement: Fair send order
Panes and workspaces SHALL share one send queue keyed by resource. Each
refresh SHALL keep survivors in order, append new resources in snapshot order,
and walk the queue once without revisiting a resource. A resource SHALL move
to the tail when its reader call or metadata exchange starts, even if that
attempt fails, and SHALL otherwise keep its place. Each pane
SHALL complete observe, plan and send before the next pane is observed.

#### Scenario: Deferred resources go first next time
- **WHEN** a refresh sends only the first resources of its queue
- **THEN** the next refresh starts with the resources that did not get an attempt

#### Scenario: Refused admission preserves attempt-based order
- **WHEN** a resource is refused metadata admission
- **THEN** refusal alone does not rotate it, while an earlier reader attempt
  stays rotated

### Requirement: Degraded expiry under long cycles
When a cycle runs long or defers a resource, the gap between two sends of a
live key MAY exceed its TTL. The key SHALL then expire and disappear and SHALL
NOT show a value that was not observed. The refresh interval and the TTL
multiplier SHALL NOT change to compensate.

#### Scenario: Shutdown during a budgeted refresh
- **WHEN** shutdown is requested during a refresh
- **THEN** no new frame starts, the refresh reports interruption rather than a
  deadline skip, and clear uses its own deadline
