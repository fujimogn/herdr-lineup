# Lineup v1 design

## Status and boundaries

This is a proposed implementation contract, not evidence of a running plugin.
The daemon publishes presentation metadata only. Herdr owns pane identity and
semantic lifecycle state. Explicit opt-in font actions install the bundled icon font and manage terminal
codepoint maps; user sidebar configuration owns row order and colors. On managed machines,
dotfiles owns that sidebar configuration and configure/unconfigure is not used.

## Verified protocol references

The following pinned Herdr v0.9.3 documents were checked on 2026-10-12:

- [Socket API](https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/socket-api.mdx):
  metadata patches, `ttl_ms`, `seq`, limits, state, and snapshot reconciliation.
- [Plugins](https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/plugins.mdx):
  one-shot startup hooks, workspace events, and plugin runtime directories.
- [Configuration](https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/configuration.mdx):
  Agent and Space custom tokens; numeric rules require unit-less values and
  cannot read a different token from the token being styled.

These are baseline references, not a claim that every older Herdr version has
identical behavior. Runtime doctor must check protocol compatibility; the
manifest's minimum version is validated during implementation.

## Metadata layout and color

Pane tokens describe one agent occupant, never a workspace aggregate. Workspace
tokens describe the workspace repository. Use a Space heading for repo and time,
then Agent rows containing logo, the one nonempty state dot token, context text,
and weekly text. Herdr may insert separators between cells; exact spacing and
colors remain sidebar concerns. Do not promise a single combined row can have
different colors within one token.

`lineup_state` is the normalized state string. Five separate state dot tokens
allow static per-token colors; only the active state's dot is nonempty. The
numeric percentage tokens are intended for rows that use numeric color rules.
The display tokens include `%` and must not be used with numeric rules. Context
text uses a minimum two-digit decimal (`07%`, `42%`, `100%`) because Herdr trims
leading spaces. Weekly text combines the period, remaining percentage, and reset countdown
(`7d 67% 6d9h`); a missing reset omits the countdown (`7d 67%`). Missing display
data is the em dash `—`; missing numeric data is cleared with JSON null.

Only dirty keys are patched, except that unchanged live keys are renewed before
expiry. With the default 5-second refresh, TTL is 15 seconds and renewal occurs
no later than 10 seconds after the previous successful report. The daemon
persists a per-resource sequence high-water mark under the session lock before
sending each report; restart cannot reuse an accepted sequence number. Never
use wall-clock time as the sole sequence source. Clear all owned keys for a
departed/unsupported occupant; never clear another plugin's keys.

## Font evidence

The inspected install was
`~/.config/herdr/plugins/github/hhdebb.herdr-radar-afabda667cbe/`.
`tools/codepoints.toml` declares family `Herdr Agent Icons Max`, version `1.8.0`;
`lib/logos.js` agrees with its six vendor assignments.

The actual `dist/HerdrAgentIconsMax-Regular.ttf` SHA-256 was
`1b8199496b223d0761fc3a49d5b2510bda38deab82cb3758bf3980495bf05272`.
Reading its binary cmap confirmed glyph ids 1 (Claude), 2 (Codex), 7 (Kimi),
11 (Hermes), 18 (Grok), and 19 (Antigravity). Fontconfig independently confirmed:

```text
fc-query --format '%{family}\n%{charset}\n' <install>/dist/HerdrAgentIconsMax-Regular.ttf
Herdr Agent Icons Max
e1a0-e1ba e1c0-e1c5
```

The sidebar spec records the exact vendor table. U+E1C0–U+E1C5 are radar lifecycle
glyphs, reserved here; lineup v1 uses ordinary U+25CF `●` for state. No new PUA
assignments are invented. The font originates in
[hhdebb/herdr-radar](https://github.com/hhdebb/herdr-radar); the inspected install
contains MIT license/third-party notices. The plugin bundles this exact icon font at
`assets/fonts/HerdrAgentIconsMax-Regular.ttf`; NOTICE.md includes attribution,
the MIT license, lobe-icons mark origins, and vendor non-affiliation.
JetBrainsMonoHerdr is not bundled.

Font presence is not proof of a terminal codepoint map. `logo_mode = "auto"`
selects text unless terminal rendering support is affirmatively established;
the initial implementation is allowed to remain conservative. Explicit `font`
is the user's assertion that both font and terminal mapping are configured.
Text fallback uses the ASCII labels in the spec rather than lookalike symbols.

Explicit `install-font` and `uninstall-font` actions manage only the icon font
and U+E1A0–U+E1BA maps; startup never invokes them. Linux uses the XDG data
fonts directory and macOS uses `~/Library/Fonts`. Existing Ghostty configs
(`config` or `config.ghostty`, including the macOS Application Support path)
and kitty configs receive a marked block; missing configs are not created.
A plugin-state ownership record distinguishes newly installed files from
preexisting fonts. Uninstall removes only byte-identical owned files and
unmodified owned blocks, preserving unrelated terminal bytes and later edits.
Modified blocks or symlinked paths produce a conflict without writing.
Other terminals receive manual codepoint-map instructions in logs. Managed
machines use dotfiles for fonts and terminal settings instead of these actions.

## Daemon implementation outline

Use synchronous Rust threads and a persistent local socket connection. Bind
runtime files to a hash of the absolute `HERDR_SOCKET_PATH` under
`HERDR_PLUGIN_STATE_DIR`; obtain an exclusive OS file lock before spawning a
detached `setsid` daemon. Transfer lock ownership without a release/reacquire
gap. A PID file is diagnostic, never sufficient proof of ownership or a safe
signal target. A session-local control endpoint answers identity/health probes
and shutdown requests. Check socket identity (device/inode on Unix) and session
handshake identity; replacement terminates this daemon instead of retargeting.
Herdr startup on the new server starts a new ensure cycle.

`[[startup]]` and `[[events]] on = "workspace.focused"` invoke one-shot `ensure`.
Startup is not supervision and link/enable do not trigger it; the `start` action
is another explicit ensure entry point. Subscribe before authoritative snapshot
reconciliation, treat events as invalidations, and poll for TTL renewal and Git
ref changes. Disconnect does not justify using stale pane associations.

Cache reader results by provider/session/repository identity and bound reads by
timeout. Do not run the same reader once per pane when panes share a session.
Provider-specific paths, credentials, APIs, and parsing are **TBD** until the
parallel usage-source research is accepted. A missing adapter returns
unavailable, never a synthetic zero. Existing tokens cannot outlive the occupant
whose session produced them.

## Configuration and rollback

User config lives in `$HERDR_PLUGIN_CONFIG_DIR/config.toml`; defaults and bounds
are normative in the daemon spec. Invalid config is a diagnostic error before
spawning or changing metadata. Do not store runtime state in `HERDR_PLUGIN_ROOT`.

Configure/unconfigure only run as explicit actions. Use `toml_edit`, a marked
owned block, a unique non-overwriting `.bak` backup, and atomic replace. Preserve
unrelated config and reject ambiguous/conflicting blocks. Validate the candidate
before replacing it and reload config after replacement; on reload failure,
restore the pre-action file atomically and report failure. Unconfigure removes
only a matching unmodified owned block, never restores a whole old backup over
new user edits. A modified block requires manual resolution. Implementation
must verify the installed Herdr config validation/reload interface.

Stopping clears lineup-owned keys when the original endpoint is reachable;
otherwise TTL expires them. Removing the opt-in block and stopping the daemon
rolls back sidebar integration without changing fonts or other plugins.

## Open questions for implementation

- Provider usage-source details and agent session association: TBD research.
- Affirmative terminal glyph capability detection: TBD; conservative text works.
- Supported minimum Herdr version and config validation entry point: verify
  against the pinned protocol before implementation; no server is restarted
  or attached to by this specification task.
