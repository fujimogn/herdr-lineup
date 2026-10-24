## ADDED Requirements

### Requirement: Refresh outcomes distinguish deadline skips
A refresh SHALL return success, a skip carrying capacity-skipped and
deadline-skipped resource counts, or an error. The daemon SHALL classify
shutdown interruption first, then a skip, then errors. A deadline skip SHALL
NOT mark the refresh as failed, SHALL NOT reconnect, and SHALL log
`refresh_skipped` with reason `cycle_deadline`. Persistence poison and
possibly delivered requests SHALL remain errors.

#### Scenario: Deadline skip is not a failure
- **WHEN** a refresh ends with resources skipped by its deadline
- **THEN** health shows no refresh failure and the daemon does not reconnect

### Requirement: Deadline skips use a separate optional control field
The existing health fields SHALL keep their meaning. Deadline skips SHALL be
reported in an optional control field next to the refresh observation, omitted
until the first deadline skip, so an older peer still reads the frame.
Deadline skip attempts SHALL count in the attempt total but not as successes,
errors or capacity skips.

#### Scenario: Older peer reads a frame after a deadline skip
- **WHEN** a peer that does not know the field reads the control frame
- **THEN** it still parses health and the refresh observation

### Requirement: Production refresh stage timings
The production publisher SHALL report, after every refresh attempt including
errors and skips, snapshot, Git, reader, persist and socket durations in
microseconds, acknowledged reports, and successful checkpoint writes for that
attempt only, measured with a monotonic clock. Stages overlap, so their sum is
not the attempt duration.

#### Scenario: Stages after an error
- **WHEN** a refresh fails after some reports were acknowledged
- **THEN** the stage sample for that attempt is still reported, with counts
  for that attempt only

### Requirement: Live reader diagnostics are optional and advisory
Control health SHALL optionally include typed diagnostics for the six fixed
providers from the publisher-owned registry, sampled once per valid health
request after releasing the control-state mutex. Sampling SHALL be nonblocking, advisory, live best-effort and nontransactional
across providers. Health and refresh metrics SHALL retain completed-attempt
observation times.

#### Scenario: Worker release without another refresh
- **WHEN** a timed-out worker releases before the next refresh
- **THEN** a subsequent control request reports it inactive using the same registry

#### Scenario: Optional version skew or provider contention
- **WHEN** reader diagnostics are absent, unsupported, malformed or contended
- **THEN** valid health remains readable and valid sibling provider metrics survive
- **AND** unknown input labels and sensitive data are never printed
- **AND** unavailable, unmeasured wait, measured zero and inactive stay distinct
- **AND** diagnostics do not affect refresh, reconnect, shutdown or permits

### Requirement: Control frames preserve health within the existing limit
A response SHALL fit 8192 bytes including its newline. It SHALL sample readers
only once, omit the whole reader extension first if oversized, then omit refresh
observation and deadline extensions together. Health SHALL never be truncated;
oversized health alone SHALL fail the control exchange.

#### Scenario: Maximum counters and near-limit health
- **WHEN** all six providers and stage histograms contain maximum counters
- **THEN** exact serialization and fallback preserve valid health within the limit
- **AND** a frame of 8192 bytes including newline is accepted and 8193 is rejected

### Requirement: Doctor distinguishes diagnostic observation times and categories
Doctor SHALL use fixed count/ns labels, marking readers live best-effort and
nontransactional. It SHALL distinguish reader deadlines from retained totals
and resource counts of the latest deadline-skip attempt. Diagnostics SHALL
precede existing health errors without changing exit status or identity checks.

#### Scenario: Failed refresh still prints available diagnostics
- **WHEN** health reports a failed or incomplete first refresh
- **THEN** optional diagnostic text prints and the existing error remains an error
- **AND** retained deadline-skip counts are not labeled as latest-refresh counts
