## ADDED Requirements

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
