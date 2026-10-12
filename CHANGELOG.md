# Changelog

Full release notes with details on each version: [GitHub Releases](https://github.com/fujimogn/herdr-lineup/releases)

## Unreleased

Lineup brings agent status, session-bound usage, and repository headings to the Herdr sidebar with opt-in setup and reliable background refresh.

- Feature: **Agent rows** show vendor logos and state dots for Claude, Codex, Grok, Antigravity, Kimi, and Hermes, with context usage, weekly remaining quota, and reset countdowns when available.
- Feature: **Local usage** reads Codex and Grok context from the pane's exact native session; Claude and Antigravity provide context and weekly remaining through opt-in statusLine observations, while Kimi and Hermes usage stays unavailable.
- Feature: **Codex context** matches the UI's 12,000-token baseline calculation and retains the last bound observation through null token-count events or an incomplete final rollout record.
- Feature: **Usage columns** align unavailable context with two-digit percentages under narrow-ambiguous terminal rendering and pad weekly remaining to at least two digits; full `100%` context stays visible as a four-cell exception.
- Feature: **Usage refresh** bounds waits for stalled readers, discards results past their deadline, and retains the previous valid observation for the same pane and source on timeout.
- Feature: **StatusLine integration** records Claude and Antigravity usage without printing a status line or storing conversation text; `--then` preserves an existing command's input, output, and exit status even when usage capture fails.
- Feature: **Account quota** adds opt-in Codex and Grok weekly queries with `allow_network = true`; sidebar refresh continues independently of network requests, and local context remains available with network access disabled.
- Feature: **Quota attribution** binds Codex account quota to exact sessions verified in the daemon's home, while Grok shares its configured account quota across Grok panes; Grok accepts prefixed, case-insensitive weekly periods, bounds freshness without a reset time, and inherits environment proxies.
- Feature: **Quota diagnostics** let `doctor` explain the last weekly query result without exposing secrets, including missing, malformed, ambiguous, or unreadable Grok authentication and HTTP failures.
- Feature: **Repository headings** show the repository name and latest local `origin/main` commit time without fetching, both at workspace level and on the first pane; headings follow pane reordering and clear when the repository becomes unavailable.
- Feature: **Repository refresh** bounds Git queries to one second and cancels their process groups on shutdown, preserving the last heading when a read is interrupted and the workspace is unchanged.
- Feature: **Row alignment** adds opt-in `non_first_pane_logo_indent` from 0 to 16 columns, leaving the first pane unchanged so later agent rows can align beneath a repository heading.
- Feature: **Live metadata** renews unchanged rows and restores missing keys during slow refresh cycles, preserves update ordering across restarts, and prioritizes required context text when Herdr's stored-key limit is reached.
- Feature: **Daemon controls** provide `ensure`, `daemon`, `stop`, and read-only `doctor`, with one publisher per Herdr session, startup and workspace-focus recovery, responsive control requests during refresh, bounded settings, and state stored outside the managed plugin checkout.
- Feature: **Daemon lifecycle** serializes overlapping stop and start actions so a new start retains one healthy publisher; `ensure` waits for its losing child to exit before accepting the winning daemon.
- Feature: **Daemon shutdown** makes `stop` wait for the accepted daemon to exit without signaling it and fail after five seconds if it is still alive; shutdown stops quota scheduling, skips pending Grok HTTP waits, and terminates the Codex query process group.
- Feature: **Herdr compatibility** supports Linux and macOS with Herdr 0.9.3 or later, reconnecting after each JSON response and bounding socket I/O while keeping daemon control connections compatible with macOS.
- Feature: **Sidebar setup** adds explicit `configure` and `unconfigure` actions that serialize edits, back up configuration, preserve unrelated settings, and remove only an unmodified plugin-managed block; startup never edits the user's sidebar.
- Feature: **Icon setup** bundles the HerdrAgentIconsMax icon font with explicit, opt-in install and uninstall commands and `logo_mode` settings; text labels remain available and automatic mode uses them until terminal glyph support is known.
- Feature: **Development and releases** provide Linux and macOS build checks, weekly dependency audits, version-checked tag releases, and documented plugin conventions and usage sources; public installation remains planned.
- Feature: **Release notes** use consistent dated version headings and a documented preview before tagging; release bodies include the version heading, and a missing or empty version section stops Release creation.
- Feature: **Isolated testing** provides a documented lifecycle and metadata harness that checks heading publication, renewal, and clearing without touching the user's Herdr session; Linux publication is verified, while authenticated agent panes are skipped without provider credentials.
