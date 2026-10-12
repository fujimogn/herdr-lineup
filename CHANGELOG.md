# Changelog

Full release notes with details on each version: [GitHub Releases](https://github.com/fujimogn/herdr-lineup/releases)

## Unreleased

Lineup brings agent status, session-bound usage, and repository headings to the Herdr sidebar with opt-in setup and reliable background refresh.

- Feature: **Agent rows** show vendor logos and state dots for Claude, Codex, Grok, Antigravity, Kimi, and Hermes, with context usage, weekly remaining quota, and reset countdowns when available.
- Feature: **Local usage** reads Codex and Grok context from the pane's exact native session; Claude and Antigravity provide context and weekly remaining through opt-in statusLine observations, while Kimi and Hermes usage stays unavailable.
- Feature: **StatusLine integration** records Claude and Antigravity usage without printing a status line or storing conversation text; `--then` preserves an existing command's input, output, and exit status even when usage capture fails.
- Feature: **Account quota** adds opt-in Codex and Grok weekly queries with `allow_network = true`, keeping sidebar refresh independent of network requests; Codex requires an exact session verified in the daemon's home, while Grok shares its configured account quota across Grok panes.
- Feature: **Quota diagnostics** let `doctor` explain the last weekly query result without exposing secrets, including missing, malformed, ambiguous, or unreadable Grok authentication and HTTP failures.
- Feature: **Repository headings** show the repository name and latest local `origin/main` commit time without fetching, both at workspace level and on the first pane; headings follow pane reordering and clear when the repository becomes unavailable.
- Feature: **Row alignment** adds opt-in `non_first_pane_logo_indent` from 0 to 16 columns, leaving the first pane unchanged so later agent rows can align beneath a repository heading.
- Feature: **Daemon controls** provide `ensure`, `daemon`, `stop`, and read-only `doctor`, with one publisher per Herdr session, startup and workspace-focus recovery, bounded settings, and state stored outside the managed plugin checkout.
- Feature: **Sidebar setup** adds explicit `configure` and `unconfigure` actions that back up configuration, preserve unrelated settings, and remove only an unmodified plugin-managed block; startup never edits the user's sidebar.
- Feature: **Icon setup** bundles the HerdrAgentIconsMax icon font with explicit, owned install and uninstall actions; text labels remain available and automatic mode uses them until terminal glyph support is known.
- Feature: **Development and releases** provide Linux and macOS build checks, weekly dependency audits, version-checked tag releases, and documented plugin conventions and usage sources; public installation remains planned.
- Feature: **Isolated testing** provides a documented lifecycle and metadata harness that checks heading publication, renewal, and clearing without touching the user's Herdr session; Linux publication is verified, while authenticated agent panes are skipped without provider credentials.
- Fix: **Usage alignment** keeps unavailable context aligned with two-digit percentages under narrow-ambiguous terminal rendering and pads weekly remaining to at least two digits; full `100%` context stays visible as a four-cell exception.
- Fix: **Codex context** matches the UI calculation by subtracting the 12,000-token baseline from usage and capacity, retains the last bound context after null token-count events, and keeps complete observations when the final rollout record is still being written.
- Fix: **Usage freshness** accepts observations written while earlier panes are being read, bounds waits for stalled readers, and discards late or queued results after their deadline while retaining the previous valid observation for the same pane and source on timeout.
- Fix: **Live metadata** renews unchanged rows during slow refresh cycles and republishes keys missing from Herdr's snapshot, preventing valid sidebar data from disappearing at its expiry.
- Fix: **Daemon shutdown** makes `stop` wait for the accepted daemon to exit without signaling it and fail after five seconds if it is still alive, and makes `ensure` wait for its own losing child to exit before accepting another healthy daemon; shutdown stops quota scheduling, skips pending Grok HTTP waits, and terminates the Codex query process group.
- Fix: **Daemon readiness** retries transient control failures only during startup probes, keeps control requests responsive during refresh, and waits for the winning daemon after lock contention instead of spawning another child.
- Fix: **Repository refresh** bounds Git queries to one second and cancels their entire process groups on shutdown, preserving the last heading when a read is interrupted and the workspace is unchanged.
- Fix: **Metadata ordering** preserves publication sequences across restarts and reserves each update before sending it, including shutdown clearing within its deadline.
- Fix: **Crowded panes** clear optional metadata in priority order when Herdr's stored-key limit is reached, preserving required context text instead of leaving stale values behind.
- Fix: **Grok quota** accepts case-insensitive prefixed weekly period names, supports missing reset times with bounded freshness, and inherits environment proxies; duplicate authentication fields are rejected even when their first value is null.
- Fix: **Herdr connections** reconnect after each JSON response and use bounded non-blocking I/O, keeping publication compatible with Herdr v0.9.3 and macOS closed-socket behavior.
- Fix: **macOS controls** keep accepted control connections blocking so inherited listener flags do not break daemon requests.
- Fix: **Configuration and fonts** serialize sidebar edits, resolve trusted font roots before checking descendant symlinks, and release font action locks even when child processes inherit their descriptors.
- Fix: **Release checks** pass tags safely to version validation and keep isolated test socket paths within macOS limits; session-lock tests remain isolated from concurrent forks.
