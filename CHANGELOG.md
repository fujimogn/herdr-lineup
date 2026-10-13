# Changelog

Full release notes with details on each version: [GitHub Releases](https://github.com/fujimogn/herdr-lineup/releases)

## Unreleased

## 0.1.0 (2026-10-13)

Lineup brings agent status, session-bound usage, and repository headings to the Herdr sidebar with opt-in setup and reliable background refresh.

- Feature: **Agent rows** show vendor logos and state dots for Claude, Codex, Grok, Antigravity, Kimi, and Hermes, with context usage, weekly remaining quota, and reset countdowns when available.
- Feature: **Local usage** reads Codex and Grok context from the pane's exact native session; Claude and Antigravity provide context and weekly remaining through opt-in statusLine observations, while Kimi and Hermes usage stays unavailable.
- Feature: **Codex context** matches the UI's 12,000-token baseline and keeps the last bound observation through incomplete updates.
- Feature: **Row alignment** aligns unavailable context with two-digit percentages under narrow-ambiguous rendering, keeps full `100%` context visible, and pads weekly percentages to at least two digits. Opt-in `non_first_pane_logo_indent` aligns later agent logos beneath repository headings with 0–16 columns of padding.
- Feature: **StatusLine integration** records Claude and Antigravity usage without printing a status line or storing conversation text; `--then` preserves an existing command's input, output, and exit status even when usage capture fails.
- Feature: **Account quota** adds opt-in Codex and Grok weekly queries with `allow_network = true`; refresh continues independently of network requests, and local context still updates with network access disabled. Codex quota requires an exact session verified in the daemon's home; Grok shares its configured account quota across Grok panes and supports weekly periods and environment proxies.
- Feature: **Quota diagnostics** let `doctor` explain the last weekly query result without exposing secrets, including missing, malformed, ambiguous, or unreadable Grok authentication and HTTP failures.
- Feature: **Repository headings** show the repository name and latest local `origin/main` commit time without fetching, at workspace level and on the first pane. Headings follow pane reordering, clear when the repository becomes unavailable, and retain the last value when a read is interrupted in an unchanged workspace; Git queries are limited to one second.
- Feature: **Background refresh** keeps rows visible through slow reads, renews unchanged rows, and restores missing metadata without letting older updates overwrite newer ones across restarts. Stalled usage reads retain the previous valid observation for the same pane and source, late results are discarded, and required context text takes priority when Herdr's stored-key limit is reached.
- Feature: **Daemon controls** provide `ensure`, `daemon`, `stop`, and read-only `doctor`, with one publisher per Herdr session even during overlapping starts and stops. Startup and workspace focus recover the publisher, controls stay responsive during refresh, settings are bounded, and state lives outside the managed plugin checkout.
- Feature: **Safe shutdown** makes `stop` wait for the accepted daemon to exit without signaling it and fail after five seconds if it is still alive, leaving the Herdr server running. Shutdown ends quota scheduling, cancels Git and Codex query processes, and does not wait for pending Grok HTTP requests.
- Feature: **Compatibility** supports Linux and macOS with Herdr 0.9.3 or later, with bounded socket waits and connections that handle Herdr closing each response.
- Feature: **Sidebar setup** adds explicit `configure` and `unconfigure` actions that serialize edits, back up configuration, preserve unrelated settings, and remove only an unmodified plugin-managed block; startup never edits the user's sidebar.
- Feature: **Icon setup** bundles the HerdrAgentIconsMax icon font with explicit, opt-in install and uninstall commands and `logo_mode` settings; text labels remain available and automatic mode uses them until terminal glyph support is known.
- Feature: **Development and releases** provide Linux and macOS build checks, weekly dependency audits, and version-checked tag releases with dated, previewable notes; missing or empty version notes stop Release creation. A documented isolated harness checks publication without touching the user's Herdr session; Linux publication is verified, authenticated panes need provider credentials, and public installation remains planned.
