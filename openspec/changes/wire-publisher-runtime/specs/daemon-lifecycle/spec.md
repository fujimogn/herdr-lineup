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
