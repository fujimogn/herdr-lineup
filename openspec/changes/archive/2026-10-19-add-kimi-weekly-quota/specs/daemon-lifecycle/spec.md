## MODIFIED Requirements

### Requirement: Safe stop and read-only doctor
A `stop` action SHALL request shutdown through the session-local daemon control
endpoint after validating plugin/socket identity. It SHALL NOT signal an
unverified PID, another daemon, or the Herdr server. A `doctor` action SHALL be
read-only, report health and availability, and redact secrets.

#### Scenario: Perform bounded shutdown and diagnosis
- **WHEN** stop or doctor is invoked
- **THEN** it obeys the following cleanup, deadline, health, and logging rules

Shutdown SHALL clear only
lineup-owned tokens when the original endpoint is reachable, release the lock,
and exit within a bounded deadline of five seconds; failure SHALL be reported.
When a live daemon accepts shutdown, `stop` SHALL register that process before
sending the request and SHALL return only after the process has exited. It
SHALL NOT signal the process. If the process is still alive at five seconds,
stop SHALL fail. After `ensure`'s own child loses the session lock, `ensure`
SHALL wait for that child to exit before treating a different healthy daemon as
ready. The startup deadline still bounds that wait.
If the endpoint is unreachable, TTL SHALL clear tokens without contacting a
replacement. Stop on an already stopped daemon SHALL succeed as a no-op.
Persistent sequence state SHALL remain available for a later restart.

A `doctor` action SHALL be read-only and report config validity, endpoint/protocol
compatibility, daemon/control/lock health, last refresh health, font-mode
caveats, bounded statusLine record counts, and the saved result categories of
the last Codex, Grok, and Kimi weekly queries. Metadata capacity exhaustion SHALL be
recorded in the session-local daemon log. Doctor SHALL give a generic warning
for unavailable optional provider data. It SHALL report
unhealthy configuration/daemon conditions with a nonzero exit status. Missing
optional provider data alone SHALL be a warning, not a daemon health failure.
Diagnostics SHALL redact credentials and conversation contents. Logs SHALL go
to session-local `daemon.log` under `HERDR_PLUGIN_STATE_DIR`; no runtime file
SHALL be written into `HERDR_PLUGIN_ROOT`.

#### Scenario: Reused PID
- **WHEN** a stale PID file names an unrelated process
- **THEN** stop sends no signal to it and doctor reports the stale record

#### Scenario: Missing usage is expected
- **WHEN** the daemon is healthy and Kimi account quota is disabled or unavailable
- **THEN** doctor succeeds with a generic optional-provider warning, without starting a
  network query, editing config, or restarting any server

### Requirement: Explicit user configuration with bounded defaults
The plugin SHALL read `$HERDR_PLUGIN_CONFIG_DIR/config.toml`, use the following
defaults when absent, and document discovery through
`herdr plugin config-dir herdr-lineup`. Unknown keys, malformed TOML, or values
outside these bounds SHALL fail validation before spawning or publishing.

#### Scenario: Load a valid configuration
- **WHEN** config is absent or its keys are supplied explicitly
- **THEN** these defaults, bounds, and application rules define daemon behavior

| Key | Default | Contract |
| --- | --- | --- |
| `refresh_interval_ms` | `5000` | Integer 1000–3600000; TTL is exactly three times this value |
| `reader_timeout_ms` | `2000` | Integer 100–30000, and no greater than refresh interval |
| `usage_max_age_ms` | `60000` | Integer at least refresh interval and at most 86400000 |
| `logo_mode` | `"auto"` | `auto`, `font`, or `text`; conservative auto fallback |
| `non_first_pane_logo_indent` | `0` | Integer 0–16; prefix later-pane logos with that many U+2800 cells |
| `providers` | `["claude", "codex", "grok", "agy", "kimi", "hermes"]` | Unique supported ids; an empty list disables all lineup agent rows |
| `allow_network` | `false` | No network-dependent reader runs unless true and its researched adapter is enabled |
| `kimi_weekly_enabled` | `false` | Kimi managed-account queries also require this true and `kimi` in `providers` |

All selected intervals SHALL keep TTL within Herdr's 1–86400000 ms bound.
Config edits SHALL take effect on a deliberate stop/start, not partially during
a refresh. `providers` controls pane rows; repository headings remain independent.
No credentials or sidebar layout SHALL be stored in this plugin config.

#### Scenario: Absent and invalid configuration
- **WHEN** the user config is absent
- **THEN** the daemon uses the eight defaults above and never writes a config file
- **AND WHEN** refresh interval is 1000 but reader timeout remains 2000
- **THEN** config validation fails with an actionable bounded-value diagnostic
