# Changelog

Full release notes with details on each version: [GitHub Releases](https://github.com/fujimogn/herdr-lineup/releases)

## Unreleased

## 0.2.0 (2026-10-19)

Lineup adds opt-in Kimi weekly quota and improves daemon recovery, local usage reads, and sidebar update latency.

- Feature: **Kimi weekly quota** adds a separate, default-off managed account query, shared across Kimi panes with bounded request and cache expiry; doctor records fixed result categories including expired credentials.
- Feature: **Kimi weekly quota** now has a verified live response shape; the Kimi CLI access token lasts 15 minutes, so weekly quota becomes unavailable after the CLI closes and its token expires.
- Fix: **Daemon recovery** retries transient reconnect and transport-ping failures without dropping the publisher; it exits if the socket stays missing beyond the token TTL and waits for a replaced endpoint owner before starting another publisher.
- Fix: **Daemon stop** preserves the metadata clear when shutdown races with control-worker completion.
- Fix: **Grok context and quota** accepts a valid context observation changed during discovery, and refuses blocking FIFO or other non-regular credential files while retaining final-symlink support.
- Fix: **Kimi opt-out** avoids credential metadata reads when weekly quota is disabled and keeps the isolated harness inside its fixture home.
- Fix: **Codex context** rejects array-shaped token records instead of treating them as valid usage.
- Fix: **Repository headings** preserve common-directory paths containing newlines.
- Maintenance: **Daemon idle work** reduces wakeups through poll-based control listening and longer main-loop waits.
- Maintenance: **StatusLine** removes unnecessary fsyncs before the chained display command, reducing its measured latency while retaining atomic record replacement.
- Maintenance: **Repository headings** poll Git children instead of repeatedly sleeping, combine repository checks, and reuse results for the same cwd within each refresh cycle.
- Maintenance: **Local usage readers** cache Kimi state and model metadata and parse Codex and Kimi session tails with typed records, reducing repeated file work while keeping malformed completed records unavailable.
- Maintenance: **Toolchain** is pinned to Rust 1.99.1.
- Security: **Dependencies** update rustls to 0.23.46 for RUSTSEC-2026-0346 to 0349.
- Maintenance: **Release workflow** pins actions to commit SHAs, verifies before build that release tags point to successful main CI commits and match changelog dates, and does not fail on reruns for existing tags.
- Security: **Security policy** adds a supported versions section to SECURITY.md, clarifying that only the latest release receives security fixes.

## 0.1.1 (2026-10-14)

Lineup reads Kimi context from the matching local session, keeps stop and doctor inside their existing limits, and ships dependency licenses with release archives.

- Feature: **Kimi context** reads context from one matching regular, nonsymlinked local session's completed turn, and stays unavailable when that session is missing, duplicated, unreadable, or still in progress. A completed turn does not expire by age, including while the pane is idle, and weekly quota stays unavailable with no network I/O.
- Feature: **Installation** uses `herdr plugin install fujimogn/herdr-lineup` from the public repository. Herdr previews the source and command, registers Lineup only after the build succeeds, and replaces that checkout on a later install; `--ref` pins a release tag, a local link blocks install until removed, and the marketplace is unreviewed.
- Fix: **Daemon stop** still waits for the accepted daemon to exit without signaling it, and still fails after five seconds if that process is alive. A stalled usage read is not killed, but the following metadata clear still fits in that five-second deadline, and stop still does not join a pending Grok HTTP request.
- Fix: **Doctor** reports `daemon is starting; the first refresh has not completed yet` when no refresh has finished, separately from an unhealthy refresh. Capacity skips do not by themselves fail doctor; a real refresh failure still exits nonzero, and an overlong socket path is rejected before connect with its byte length and the platform maximum.
- Fix: **Context** drops the previous local context when directory discovery passes 4,096 entries, unlike a read timeout, which keeps it; exactly 4,096 entries still succeeds. Without the proven session binding, that pane does not receive the Codex account-quota overlay, and Grok's shared weekly value is applied when network access is enabled.
- Fix: **Grok quota** keeps a weekly percentage that has no reset time visible, without a countdown, for a bounded time after the last successful query. A later transient failure clears a value with no reset, a value with a known future reset is still kept until that reset, and monthly periods stay unavailable.
- Fix: **Publisher** keeps each endpoint's highest reserved sequence when removing checkpoint rows for absent resources, so a reused id gets a newer sequence across restarts. If returning to 0.1.0 fails refresh because it cannot read an endpoint floor in `sequences.json`, keep the checkpoint and reinstall 0.1.1: deleting it restarts sequence numbers at 1 and can leave Herdr rejecting older updates until they surpass the previous high-water mark.
- Fix: **Font setup** can be retried after a terminal write had started, and lists changed paths. Conflicts and symlinks leave bytes unchanged; uninstall removes only recorded byte-identical blocks and a font Lineup installed, still succeeds if nothing managed remains, and drops a stale ownership entry only when the whole block was removed by hand.
- Fix: **StatusLine** runs `--then` with the fixed shell `/bin/sh -c`, so an entry on `PATH` cannot choose the shell. The chained command still receives the original stdin bytes unchanged, and its stdout and exit status are what that command itself returns.
- Fix: **Daemon logs** record one UTC timestamp and fixed `event` and `reason` categories per line, never error text, paths, credentials, or conversation data. Past 1 MiB the log is renamed to one previous generation, and Git children for repository headings keep a reduced environment and Git safety settings without fetching or contacting a remote.
- Fix: **Release distribution** adds `THIRD-PARTY-LICENSES.md` to each `herdr-lineup-<tag>-<os>-<arch>.tar.gz` beside the binary, `LICENSE`, `NOTICE.md`, and `README.md`. The release build and CI check that file against the locked binary dependency sources before packaging, and the archives are still not standalone plugin installations.
- Fix: **Dependencies** replace the `fs2` file lock with the standard library lock. The session lock remains an advisory lock released when the last shared open-file description closes, and lifecycle commands do not unlock the daemon's inherited descriptor.

## 0.1.0 (2026-10-13)

Lineup brings agent status, session-bound usage, and repository headings to the Herdr sidebar with opt-in setup and reliable background refresh.

- Feature: **Agent rows** show vendor logos and state dots for Claude, Codex, Grok, Antigravity, Kimi, and Hermes, with context usage, weekly remaining quota, and reset countdowns when available.
- Feature: **Local usage** reads Codex and Grok context from the pane's exact native session; Claude and Antigravity provide context and weekly remaining through opt-in statusLine observations, while Kimi and Hermes usage stays unavailable.
- Feature: **Codex context** matches the UI's 12,000-token baseline and keeps the last bound observation through incomplete updates. Large uncompressed rollout files still show context when a recent usage sample is present in their bounded tail.
- Feature: **Row alignment** aligns unavailable context with two-digit percentages under narrow-ambiguous rendering, keeps full `100%` context visible, and pads weekly percentages to at least two digits. Opt-in `non_first_pane_logo_indent` aligns later agent logos beneath repository headings with 0–16 columns of padding.
- Feature: **StatusLine integration** records Claude and Antigravity usage without printing a status line or storing conversation text; `--then` preserves an existing command's input, output, and exit status even when usage capture fails.
- Feature: **Account quota** adds opt-in Codex and Grok weekly queries with `allow_network = true`; refresh continues independently of network requests, and local context still updates with network access disabled. Codex quota requires an exact session verified in the daemon's home; Codex and Grok retain weekly values with known reset times through transient query failures until reset, while authentication failures and explicit Codex account errors clear them; Grok shares its configured account quota across Grok panes and supports weekly periods and environment proxies.
- Feature: **Quota diagnostics** let `doctor` explain the last weekly query result without exposing secrets, including missing, malformed, ambiguous, or unreadable Grok authentication and HTTP failures.
- Feature: **Repository headings** show the repository name and latest local `origin/main` commit time without fetching, at workspace level and on the first pane. Headings follow pane reordering, clear when the repository becomes unavailable, and retain the last value when a read is interrupted in an unchanged workspace; Git queries for one heading share a one-second deadline, disable pagers, filesystem monitors, lazy fetch, and remote protocols, and do not run aliases, hooks, credential helpers, or remote commands.
- Feature: **Background refresh** keeps rows visible through slow reads, renews unchanged rows, and restores missing metadata without letting older updates overwrite newer ones across restarts. Stalled usage reads retain the previous valid observation for the same pane and source, late results are discarded, and required context text takes priority when Herdr's stored-key limit is reached.
- Feature: **Daemon controls** provide `ensure`, `daemon`, `stop`, and read-only `doctor`, with one publisher per Herdr session even during overlapping starts and stops. Startup and workspace focus recover the publisher, controls stay responsive during refresh, settings are bounded, and `doctor` shows the absolute state directory, bounded statusLine record count, and age of the latest record; state lives outside the managed plugin checkout.
- Feature: **Safe shutdown** makes `stop` wait for the accepted daemon to exit without signaling it and fail after five seconds if it is still alive, leaving the Herdr server running. Shutdown ends quota scheduling, cancels Git and Codex query processes, and does not wait for pending Grok HTTP requests.
- Feature: **Compatibility** supports Linux and macOS with Herdr 0.9.3 or later, with bounded socket waits and connections that handle Herdr closing each response.
- Feature: **Sidebar setup** adds explicit `configure` and `unconfigure` actions that serialize edits, back up configuration, preserve unrelated settings, and remove only an unmodified plugin-managed block; startup never edits the user's sidebar.
- Feature: **Icon setup** bundles the HerdrAgentIconsMax icon font with explicit, opt-in install and uninstall commands and `logo_mode` settings; text labels remain available and automatic mode uses them until terminal glyph support is known.
- Feature: **Development and releases** provide Linux and macOS build checks, weekly dependency audits, and version-checked tag releases with dated, previewable notes; missing or empty version notes stop Release creation. Release archives named `herdr-lineup-<tag>-<os>-<arch>.tar.gz` contain the binary, `LICENSE`, `NOTICE.md`, and `README.md`, with SHA-256 checksums; an isolated harness verifies Linux metadata publication without touching the user's Herdr session, authenticated panes need provider credentials, and public plugin installation remains planned.
