# daemon-lifecycle Specification

## Purpose
Own one Lineup daemon per Herdr session, including start and stop, read-only
diagnostics, bounded user configuration, and opt-in sidebar and font actions.

## Requirements

### Requirement: One daemon per Herdr session
The manifest SHALL invoke an idempotent one-shot `ensure` from `[[startup]]` and
from `[[events]] on = "workspace.focused"`. A public `start` action SHALL invoke
the same ensure path. Startup SHALL exit after confirming one detached daemon;
it SHALL NOT occupy a Herdr pane or attach a client to a session.

#### Scenario: Establish session ownership
- **WHEN** ensure starts a daemon for a socket
- **THEN** ownership and endpoint identity follow these rules

The daemon SHALL own an exclusive OS lock for its lifetime in a runtime directory
under `HERDR_PLUGIN_STATE_DIR`, keyed by a hash of the absolute
`HERDR_SOCKET_PATH`. Lock ownership SHALL remain continuous during detached
spawn. A PID file SHALL be diagnostic only. Concurrent ensures SHALL yield one
daemon; stale PID files with no live lock/control identity SHALL not suppress
startup. The daemon SHALL verify the socket endpoint and session identity before
reading or publishing and SHALL terminate if that socket is replaced. It SHALL
not silently connect to a replacement server or a different session.

Herdr v0.9.3 closes the connection after each JSON response to
`session.snapshot` and `report_metadata`. The daemon SHALL open a new
connection for each of those requests and SHALL treat end-of-file after a
complete response as normal. Before each connect it SHALL read the socket's
device and inode and SHALL stop without connecting when either value differs
from the identity pinned at startup. A changed device or inode is a replaced
server, including a Herdr restart. The absolute path and control nonce name
the session; they do not by themselves prove the pinned inode.

Frame deadlines SHALL use non-blocking I/O and `poll` against one monotonic
deadline. The daemon SHALL NOT apply those deadlines with `setsockopt`. On
macOS, `setsockopt` on a socket the peer has closed returns `EINVAL`. `fcntl`
SHALL be limited to the local descriptor, and the following read or write
SHALL observe buffered bytes, end-of-file, or the real error after `poll`
returns.

#### Scenario: Startup and focus race
- **WHEN** startup ensure, focus ensure, and start run concurrently for one socket
- **THEN** one daemon retains the lock and the other calls succeed as no-ops

#### Scenario: Session separation and socket replacement
- **WHEN** two Herdr sessions use distinct socket paths
- **THEN** each may have its own daemon without sharing tokens or locks
- **AND WHEN** one socket endpoint is replaced
- **THEN** its old daemon stops and a new server startup ensure establishes a new
  daemon identity instead of allowing the old daemon to retarget

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
compatibility, daemon/control/lock health, last refresh/error, token-capacity
issues, font-mode caveats, and provider availability reasons. It SHALL report
unhealthy configuration/daemon conditions with a nonzero exit status. Missing
optional provider data alone SHALL be a warning, not a daemon health failure.
Diagnostics SHALL redact credentials and conversation contents. Logs SHALL go
to session-local `daemon.log` under `HERDR_PLUGIN_STATE_DIR`; no runtime file
SHALL be written into `HERDR_PLUGIN_ROOT`.

#### Scenario: Reused PID
- **WHEN** a stale PID file names an unrelated process
- **THEN** stop sends no signal to it and doctor reports the stale record

#### Scenario: Missing usage is expected
- **WHEN** the daemon is healthy and Kimi usage is unsupported
- **THEN** doctor succeeds with an availability warning, without starting a reader
  network call, editing config, or restarting any server

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
| `providers` | `["claude", "codex", "grok", "agy", "kimi", "hermes"]` | Unique supported ids; an empty list disables all lineup agent rows |
| `allow_network` | `false` | No network-dependent reader runs unless true and its researched adapter is enabled |

All selected intervals SHALL keep TTL within Herdr's 1–86400000 ms bound.
Config edits SHALL take effect on a deliberate stop/start, not partially during
a refresh. `providers` controls pane rows; repository headings remain independent.
No credentials or sidebar layout SHALL be stored in this plugin config.

#### Scenario: Absent and invalid configuration
- **WHEN** the user config is absent
- **THEN** the daemon uses the six defaults above and never writes a config file
- **AND WHEN** refresh interval is 1000 but reader timeout remains 2000
- **THEN** config validation fails with an actionable bounded-value diagnostic

### Requirement: Opt-in reversible sidebar configuration
Only explicit `configure` and `unconfigure` actions SHALL modify the user's
Herdr `config.toml`. Start, ensure, stop, doctor, install hooks, and refresh SHALL
not modify it. The actions SHALL preserve unrelated user data and refuse
conflicting ownership.

#### Scenario: Apply an owned sidebar block transaction
- **WHEN** a user explicitly invokes configure or unconfigure
- **THEN** the action obeys these ownership, backup, validation, and rollback rules

Configure SHALL identify the exact target using Herdr's config
location/interface, show the target in diagnostics, validate a proposed TOML
layout, preserve unrelated tables/comments, and make a unique non-overwriting
`.bak` backup before atomic replacement. It SHALL add only a marked
`herdr-lineup` owned block using `toml_edit`. It SHALL refuse existing conflicting
sidebar definitions or ambiguous ownership rather than overwrite them.

Repeated configure with the same unmodified block SHALL be a no-op. Unconfigure
SHALL remove only the matching unmodified owned block, preserve subsequent user
edits elsewhere, and leave backups intact. Modified owned blocks SHALL cause a
conflict requiring manual resolution, with no write. After a successful change,
the action SHALL call `herdr server reload-config`; on reload failure it SHALL
restore the pre-action bytes atomically, attempt to reload the restored config,
and report both failures if necessary. It SHALL never stop or restart the server.
Managed dotfiles installations SHALL use manual sidebar setup instead of these
actions. The plugin SHALL not edit terminal font configuration in either action.

#### Scenario: Opt-in setup and removal
- **WHEN** configure then unconfigure run against a nonconflicting user config
- **THEN** only the owned sidebar block is added then removed, unrelated comments
  and values survive, backups exist, and successful changes reload config

#### Scenario: Modified block or rejected reload
- **WHEN** a user has changed the owned block
- **THEN** unconfigure refuses without writing
- **AND WHEN** a newly configured layout is rejected on reload
- **THEN** configure restores the exact pre-action file and reports failure

### Requirement: Explicit owned font setup and removal
The plugin SHALL bundle only `HerdrAgentIconsMax-Regular.ttf`, not
JetBrainsMonoHerdr. Explicit `install-font` and `uninstall-font` actions SHALL
install/remove the icon font in the Linux XDG data `fonts` directory or macOS
`~/Library/Fonts`. Startup and event hooks SHALL NOT invoke these actions.
Ownership records SHALL live under `HERDR_PLUGIN_STATE_DIR`.

#### Scenario: Repeated opt-in font setup and removal
- **WHEN** install-font runs twice then uninstall-font runs twice in a temporary HOME
- **THEN** the second invocation of each action is a no-op for font/terminal bytes
- **AND** unrelated terminal settings and edits made outside the managed block survive
- **AND** missing terminal configuration files are never created

Install SHALL edit only existing Ghostty (`config` and `config.ghostty`) and
kitty (`kitty.conf`) files, appending a marked U+E1A0–U+E1BA codepoint-map block.
It SHALL preserve all existing bytes outside that block, including comments and
trailing newlines. It SHALL NOT map the reserved U+E1C0–U+E1C5 range or change
primary terminal fonts. Other terminals SHALL receive manual instructions in logs.
Uninstall SHALL remove only recorded, unmodified blocks and newly installed,
byte-identical font files; preexisting fonts SHALL remain. Modified blocks,
unrecorded markers, or symlinked paths SHALL cause a conflict before writes.

#### Scenario: Preserve a manually changed managed block
- **WHEN** an owned terminal block has been modified by the user
- **THEN** install-font and uninstall-font refuse without changing font or terminal bytes
