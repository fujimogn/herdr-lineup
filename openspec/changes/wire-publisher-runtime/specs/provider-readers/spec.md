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
