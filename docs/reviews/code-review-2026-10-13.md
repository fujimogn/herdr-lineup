# Agent review: herdr-lineup — 2026-10-13

Record of the first agent-versus-agent review of herdr-lineup. This file records the ballots. It does not change the product.

Line numbers are the ones in the ballots and refer to reviewed commit `fca5bbcbdb06627534726f03099831061d4ac77f` (`main` at review time). Later status uses `git log fca5bbc..origin/main` as of tag `v0.1.1` `cacef43efab070cd17e5a5f6448aabc5bdb039dc`.

## Summary

| Item | Record |
| --- | --- |
| Event | First agent-versus-agent herdr-lineup review |
| Target | Repository-wide review of `main` `fca5bbcbdb06627534726f03099831061d4ac77f` |
| Ballots | @claude `reviews/claude-2026-10-13-1230.md`. @codex main ballot `reviews/2026-10-13-1205-01825bde.md` and first, shallower ballot `reviews/2026-10-13-1149-c9c803dd.md`. |
| Agreement | Counted as n/2. Agy submitted no ballot, so the denominator is 2. Dedup key is file:line plus kind. No pair shared both, so every finding below is 1/2. Overlapping substance with a different kind or line range is kept separate and called out under Differences. |
| Counts | 6 High, 15 Medium, 1 Low–Medium, 21 Low. 43 findings after the dedup key. |
| Highest priority before a tag | The ballots' pre-tag priorities (C1–C8, the Codex uninstall and manifest notes, and Codex `account/read` failure classes) were addressed by v0.1.0. Remaining follow-ups are identified in the statuses below. |
| Status at this main | 36 findings addressed, 2 deferred, and 5 with shipped and deferred parts; ballot severity and agreement counts above are unchanged. |
| Agy | Absent for both rounds. agent-room rejected the runs with `headless_payload_malformed` and `headless_payload_invalid_terminal` before a ballot existed. Those format errors are recorded here and are not treated as a review of the repository. |

Status vocabulary: **Shipped** plus a short sha names a commit on `main` that addresses the finding or the named part. **Already fixed in** names an earlier commit whose behavior was confirmed later. **Deferred** names a remaining part and gives the reason. A mixed status lists each part separately. Status describes the implementation and documentation, not a new ballot.

## Reviewer cost

Self-reported at the end of each ballot. Nothing here was measured again.

| Ballot | Elapsed | Tokens |
| --- | --- | --- |
| @claude `claude-2026-10-13-1230.md` | About 45 minutes, including two subagents run in parallel | About 90,000 in the main session, and about 370,000 across the two subagents (144,000 + 227,000) |
| @codex `2026-10-13-1205-01825bde.md` (main ballot) | About 42 minutes | About 14,000 |
| @codex `2026-10-13-1149-c9c803dd.md` (first pass) | About 1 minute 10 seconds | About 13,000 |
| @agy | Unknown. No ballot. | Unknown. No ballot. |

## Findings

Ordered by severity. High, then Medium, then Low–Medium, then Low. Ballot order is preserved inside a severity. Each entry keeps the ballot's content in English.

### High

#### 1. Repository-local Git config runs an arbitrary program

- File:line: `src/ship_time.rs:72-81`, `src/ship_time.rs:116-130`
- Kind: security
- Severity: High
- Agreement: 1/2 by @claude
- Content: The ship-time heading is read with `git log -1 --format=%ct refs/remotes/origin/main --`. Even with `--format`, `git log` honors `log.showSignature`, and a signed commit starts `gpg.program`.
- Evidence: Reproduced on Git 2.56.0. A scratch repo set `log.showSignature=true` and `gpg.program=<script>` in `.git/config`, pointed `refs/remotes/origin/main` at a commit with `gpgsig`, and ran the same arguments and environment as `ship_time.rs`. The script ran (the launch record grew from one line to two). `git for-each-ref --format='%(committerdate:unix)' refs/remotes/origin/main` did not launch it. `core.fsmonitor`, `core.pager`, and `alias.log` did not launch it.
- Impact: If a directory whose `.git/config` someone else prepared (an unpacked archive, a shared directory) is the pane or workspace cwd, code runs every 5 seconds with the daemon's authority. The user does not have to type `git`. `git clone` does not carry `.git/config`, so the path does not hold for a clone.
- Proposed change: Switch to `for-each-ref`. Or pass `-c log.showSignature=false`.
- Test: A fixture with a hostile config does not create the marker file.
- Status: Shipped `57facbf`

#### 2. Long Codex sessions always lose context

- File:line: `src/model.rs:419-427`, `src/reader_codex.rs:54-100`
- Kind: bug / performance
- Severity: High
- Agreement: 1/2 by @claude
- Content: Each refresh reads the whole rollout from the start, and a file over 16 MiB returns `SourceError`. At 16 MiB or under, every Codex pane still parses every line as JSON every 5 seconds.
- Evidence: Of 1,559 rollouts on this machine, 118 were over 16 MiB (`fd -S +16m`).
- Impact: The long sessions where context matters most render `—`. That breaks the "Codex context" promise in the README and CHANGELOG.
- Proposed change: Read only the last few MiB and take the last `token_count`. Skip the read when `(dev, ino, size, mtime)` is unchanged.
- Test: A fixture over 16 MiB whose `token_count` is at the tail.
- Status: Shipped `024bcb0`, `c9a56e6` (bounded tail reads, unchanged-file cache, and stat-window append recovery).

#### 3. `install-font` and `doctor` fail when typed as the README shows

- File:line: `src/font.rs:408-415`, `src/daemon.rs:39-51`, `README.md:106-108`, `README.md:424`
- Kind: docs disagree with the implementation
- Severity: High
- Agreement: 1/2 by @claude
- Content: The README says four commands can be "run directly", and Troubleshooting tells the reader to run `herdr-lineup doctor`. The implementation requires `HERDR_PLUGIN_STATE_DIR` for `install-font` / `uninstall-font`, and also `HERDR_SOCKET_PATH` and `HERDR_PLUGIN_CONFIG_DIR` for `doctor`. A Herdr 0.9.3 pane shell has no `HERDR_PLUGIN_*` (measured). Only `configure` falls back to `~/.local/state/herdr-lineup` and works (`src/configure.rs:60-66`).
- Evidence: As in the content. The missing variables were measured in a Herdr 0.9.3 pane shell.
- Impact: Following the README fails with `An absolute HERDR_PLUGIN_STATE_DIR is required`.
- Proposed change: Change the README so these run through actions. Adding a direct-run fallback would split the ownership ledger, so the safer change is the document.
- Test: Not given as a separate line. The failure mode is the required absolute `HERDR_PLUGIN_STATE_DIR`.
- Status: Shipped `207cd97`

#### 4. Install is not written as one path in the README

- File:line: `README.md:43-66` (Install), `README.md:68-108` (Usage), `README.md:440-457` (Development)
- Kind: documentation
- Severity: High
- Agreement: 1/2 by @claude
- Content: The Install section only has the Planned `herdr plugin install`. The path that works today (clone, `cargo build --release --locked`, `herdr plugin link .`, start, `configure`, `doctor`) is only fragments in Development. The Usage example is `cargo run --locked -- --help`, which is not aimed at a user. There is no uninstall section (`herdr plugin unlink` / `uninstall`, cleaning state and backups, and running `unconfigure` and `uninstall-font` first).
- Evidence: The section layout above.
- Impact: A first-time user cannot get it running from the README alone, and cannot see how to undo it.
- Proposed change: Numbered "Install from a checkout", "First run", and "Uninstall" sections.
- Test: Not given as a separate line.
- Status: Shipped `207cd97`

#### 5. Enabling the icon font is not written as one path

- File:line: `README.md:136-149`, `README.md:170-175`, `README.md:427-430`, `src/publish.rs:549-553`, `src/publish.rs:767`, `src/font.rs:45-52`, `src/font.rs:89-104`, `src/font.rs:403`
- Kind: documentation
- Severity: High (install path)
- Agreement: 1/2 by @claude
- Content: Icons need all of (1) `install-font`, (2) `logo_mode = "font"` in the plugin `config.toml`, (3) stop / start of the Lineup daemon, (4) reload the terminal config or reopen the terminal (`fc-cache` on Linux when needed). `auto` is always the same as text in the implementation (`font_verified` is always false). The README splits the fragments across sections. Also absent from the README: font locations (macOS `~/Library/Fonts`, Linux `$XDG_DATA_HOME/fonts` or `~/.local/share/fonts`); five terminal-config candidates that may be rewritten (1 kitty, 4 Ghostty); only Ghostty and kitty are handled automatically; an existing file is appended with a marker and no backup is made; `install-font` installs on the machine that runs it, so an SSH session does not install it on the local terminal; `uninstall-font` while `logo_mode = "font"` produces tofu.
- Evidence: The implementation points above, including `font_verified` always false.
- Impact: Not stated as a separate sentence. The finding is that a user cannot enable icons from the README as written.
- Proposed change: Numbered "Enable icons" and "Disable icons", including supported terminals, files rewritten, and the SSH note.
- Test: Not given as a separate line.
- Status: Shipped `207cd97`

#### 6. No way to find the `HERDR_PLUGIN_STATE_DIR` that statusLine needs

- File:line: `README.md:221-227`, `README.md:263-268`, `src/statusline.rs:318`, `src/statusline.rs:343-345`
- Kind: documentation
- Severity: High
- Agreement: 1/2 by @claude
- Content: The README only says `export HERDR_PLUGIN_STATE_DIR="/path/to/herdr-lineup/state"`. It does not say how to learn the real path. `herdr plugin` has no command that prints the state location (`config-dir` only). A missing or mismatched variable fails the record with no message. The README also does not say how to put `herdr-lineup` on the agent's `PATH`.
- Evidence: `src/statusline.rs:318` and `:343-345`, and the absence of a state-path command.
- Impact: Claude / Antigravity context and weekly usage cannot be configured from the README alone, and they stay `—` with no cause shown.
- Proposed change: Document how to obtain the real path and how to put the binary on `PATH`. Have `doctor` print the absolute state path and the statusLine record count so a user can check.
- Test: Not given as a separate line.
- Status: Shipped `207cd97`, `3bb2d2b`

### Medium

#### 7. User-visible text still says "planned"

- File:line: `herdr-plugin.toml:4`, `herdr-plugin.toml:7`, `src/lib.rs:1`, `src/lib.rs:64`
- Kind: documentation
- Severity: Medium (should be fixed in the release artifacts)
- Agreement: 1/2 by @claude
- Content: The manifest description says `… in the sidebar (planned).` That string shows up in `herdr plugin list` and similar surfaces. Comments still say "planned sidebar metadata publisher" and "scaffold".
- Evidence: Those lines.
- Impact: Not stated as a separate sentence. The finding is that implemented behavior is still labeled planned.
- Proposed change: Remove "(planned)".
- Test: Not given as a separate line.
- Status: Shipped `4c4fb31`

#### 8. Release binaries have no documented use, and the bundle is incomplete

- File:line: `.github/workflows/release.yml:38-52`, `README.md:43-66`, `docs/releasing.md`, `herdr-plugin.toml:11-14`
- Kind: incomplete release artifacts
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: A Release attaches two bare binaries and `SHA256SUMS.txt` only. The manifest invokes `target/release/herdr-lineup`, so a downloaded binary is not installable as a plugin. The README does not say what the binaries are for. The binary embeds a font (`src/font.rs:10`), but `LICENSE` and `NOTICE.md` are not distributed with it, which is a weak fit for MIT's requirement that copies keep the notice. Artifact names have no CPU architecture (`macos-latest` is arm64, Linux is x86_64 from ubuntu-latest's glibc). The macOS binary is not signed or notarized.
- Evidence: The workflow, manifest command, and embedded font.
- Impact: A downloaded binary cannot be installed as the plugin, and the font notice is not beside the binary.
- Proposed change: Document a use (for example, put it on `PATH` for statusLine) or stop attaching binaries. If they are attached, ship an archive that includes `LICENSE` and `NOTICE.md`, and put the architecture in the name.
- Test: Not given as a separate line.
- Status: Shipped `4c4fb31`, `207cd97`, `852293c` for the named archive, its documented use, and the corrected member list. Deferred: macOS signing and notarization require a separate release credential and distribution decision.

#### 9. A transient Codex RPC failure is treated as an authentication failure

- File:line: `src/quota_codex.rs:290-292`
- Kind: bug
- Severity: Medium
- Agreement: 1/2 by @codex
- Content: Any error from `account/read`'s `rpc.request` is replaced with `authentication_error()`, regardless of kind. `WeeklyClient` records `PermissionDenied` as an authentication failure and deletes the cache (`src/quota_codex.rs:143-147`).
- Evidence: Those branches.
- Impact (inference in the ballot): A transient timeout or a bad app-server response makes the diagnostic look like `unauthorized` and drops a quota value that had been kept. The weekly display can be missing until the next query, 5 minutes by default.
- Proposed change: Separate authentication errors from transport, timeout, and malformed responses. Keep the cache for the latter until the known reset.
- Test: An authentication refusal on `account/read` clears the cache. A timeout or transient error keeps the pre-reset value.
- Status: Shipped `bbcefea` (transient account-read failures retain the pre-reset quota value).

#### 10. The plugin list describes implemented behavior as Planned

- File:line: `herdr-plugin.toml:4-7`
- Kind: stale code or stale text
- Severity: Medium
- Agreement: 1/2 by @codex
- Content: Manifest comments and the description call the sidebar publisher / agent integrations Planned. The README 0.1.0 section and the implementation have the daemon, the publisher, and config and font operations. The README line that public installation is Planned is a different, correct constraint about the private repository.
- Evidence: Manifest text versus the README 0.1.0 section and the implementation.
- Impact (inference in the ballot): A user who sees the manifest description will think current features are unimplemented.
- Proposed change: Match the manifest description and comments to the current implementation. Leave "public installation is Planned" in the README.
- Test: Check the manifest description on plugin list and similar surfaces, and check that the planned-versus-implemented boundary does not contradict the README.
- Status: Shipped `4c4fb31`

#### 11. The README has no safe disable and uninstall procedure

- File:line: `README.md:78-108`
- Kind: documentation that will breed bugs
- Severity: Medium
- Agreement: 1/2 by @codex
- Content: The README explains `stop`, `unconfigure`, and `uninstall-font` separately. It does not give an order, how to remove a link, or how to remove a managed install. A `logo_mode = "font"` user is not told to switch the display back to text before deleting the font. The Herdr CLI has `plugin unlink` and `plugin uninstall` (official CLI reference, https://herdr.dev/docs/cli-reference/).
- Evidence: The README sections and the official CLI commands.
- Impact (inference in the ballot): Deleting the checkout first can leave the daemon running. Removing the font while still in font mode can show missing glyphs. A user who set statusLine by hand also has no place that says how to put that setting back.
- Proposed change: Document a safe order: `stop`, then `logo_mode = "text"` when needed, then `uninstall-font`, then `unconfigure`, then undo a manual statusLine setting, then `herdr plugin unlink herdr-lineup` for a linked plugin or `herdr plugin uninstall herdr-lineup` for a managed install. Also explain which backups and state are kept.
- Test: Follow the README against a temporary `HOME` for both a linked install and a managed install, and check that the daemon stops, rows and the font are removed, and user settings are kept.
- Status: Shipped `207cd97`

#### 12. Release-binary install is missing from the README

- File:line: `README.md:48-57,450-453`
- Kind: release install / missing documentation
- Severity: Medium
- Agreement: 1/2 by @codex (first pass)
- Content: The only real install in the README is a local Rust/Cargo build and `herdr plugin link .`. There is no procedure for people who use the Linux/macOS binaries from the release workflow. An artifact by itself does not show how it relates to the manifest or to the expected `target/release/herdr-lineup` path, so the README procedure cannot install from a release binary. If the user is not assumed to build locally, the install path is missing.
- Evidence: Those README ranges versus the release workflow.
- Impact: Users who are not building locally have no install path.
- Proposed change: Add fetch, placement, plugin link, update, and removal for the binary, or state that this release does not support installing from the binary.
- Test: Confirm a clean environment can install using the README alone.
- Status: Shipped `207cd97` (the README now says plugin installation assumes a checkout build, and describes the release archive separately)

#### 13. Each report fsyncs a checkpoint twice, and the second write is the same bytes

- File:line: `src/publish.rs:426`, `src/publish.rs:452`, `src/tokens.rs:373-378`
- Kind: performance
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: `acknowledge()` does not change `high_water`, so `persist()` at line 452 writes the same contents as line 426. That is `(pane count + workspace count) × 4` fsyncs every 5 seconds.
- Evidence: The two `persist()` call sites and `acknowledge()`.
- Impact: Not stated as a separate sentence beyond the extra fsyncs.
- Proposed change: Remove `persist()` at line 452. Reserve sequence numbers in blocks.
- Test: Not given as a separate line.
- Status: Shipped `8b8f2c4`

#### 14. Checkpoint rows are never deleted

- File:line: `src/tokens.rs:307-315`, `src/publish.rs:455-485`
- Kind: performance
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: Rows for panes that disappeared, and for old endpoints, stay in `sequences.json` (95 rows on a live machine). This multiplies finding 13.
- Evidence: Those ranges and a live 95-row file.
- Impact: The checkpoint file keeps growing, and each refresh rewrites it.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line. The ballot's later test-gap list includes checkpoint growth.
- Status: Shipped `37c23ca` (closed-resource rows are pruned while endpoint sequence floors remain monotonic).

#### 15. On macOS, a failed `setsockopt` on a control connection can exit the daemon (inference, not checked on a device)

- File:line: `src/daemon.rs:564-565`, `src/daemon.rs:410-411`, `openspec/specs/daemon-lifecycle/spec.md:37-39`
- Kind: bug
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: The repository itself says that on macOS `setsockopt` on a socket the peer has closed returns `EINVAL`, and the spec says the deadline is not applied with `setsockopt`. Only the control path returns `set_read_timeout` / `set_write_timeout` with `?`.
- Evidence: Those call sites versus the spec. Not reproduced on macOS hardware.
- Impact: A control client that closes immediately can fail the worker through that `?`.
- Proposed change: Delete those two lines, and do not stop the worker when `serve_control` fails. On macOS CI, connect a client that closes immediately.
- Test: That macOS CI case.
- Status: Shipped `8b8f2c4`

#### 16. `control.sock` path length is not checked

- File:line: `src/lock.rs:52-55`, `src/daemon.rs:250`, `scripts/e2e-isolated.sh:180-181`
- Kind: bug
- Severity: Medium (the real macOS state-dir path was not checked)
- Agreement: 1/2 by @claude
- Content: The daemon binds `state dir + /session-<16 hex>/control.sock`. The limit is 104 bytes on macOS. Past that, the only symptom is `daemon exited before readiness`.
- Evidence: The bind path and the macOS sockaddr limit.
- Impact: A long state directory fails readiness with no reason.
- Proposed change: Check the length at the start of ensure, and return an error that states the reason.
- Test: The ballot's later test-gap list includes `control.sock` path length.
- Status: Shipped `f2e9a30` (control socket path length fails with an explicit diagnostic).

#### 17. The entry cap returns `Timeout`, and old context stays forever

- File:line: `src/model.rs:371-374`, `src/reader_grok.rs:68-70`, `src/publish.rs:675-685`
- Kind: bug
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: More than 4,096 directory entries returns `Timeout`. On `Timeout`, the publisher keeps the previous value. This machine has 1,591 entries and is under the cap today.
- Evidence: The cap, the publisher's `Timeout` branch, and the live count.
- Impact: Once the cap is crossed, stale context is kept with no expiry.
- Proposed change: Not stated as a separate sentence.
- Test: The ballot's later test-gap list includes the 4,096-entry boundary.
- Status: Shipped `47c181f` (the directory entry limit clears stale context and has boundary coverage).

#### 18. The daemon's worst case is longer than stop's 5 seconds

- File:line: `src/daemon.rs:29`, `src/publish.rs:600`, `src/publish.rs:848`, `src/config.rs:57`
- Kind: bug
- Severity: Medium (arithmetic inference)
- Agreement: 1/2 by @claude
- Content: A reader wait does not observe shutdown. `reader_timeout_ms` can be set up to 30,000. A 3-second clear follows that.
- Evidence: Those constants and the reader wait.
- Impact: `stop` can give up before the daemon has finished the in-flight read and the clear.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `80e9af9` (reader waits observe shutdown within the stop deadline).

#### 19. `install-font` is brittle

- File:line: `src/font.rs:144-158`, `src/font.rs:290-301`, `src/font.rs:319-323`, `src/font.rs:335-338`, `src/font.rs:353-373`, `src/font.rs:402-403`
- Kind: bug / maintainability
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: (a) If any terminal config is a symlink, the whole operation is refused, including the font copy, and the path is not named (stow, chezmoi, and home-manager users cannot use it). (b) Deleting the managed block by hand makes both install and uninstall fail forever. The only escape is hand-editing `font-ownership.json`, which is not documented anywhere. (c) A crash after writing terminal config and before saving the ledger gets stuck next time on "Unrecorded font block". (d) No backup is made, and the rewritten files are not printed.
- Evidence: Those ranges.
- Impact: Font setup fails closed without a path, or gets stuck with no documented recovery.
- Proposed change: Put the path in the error. Drop a marker from the ledger when the count is zero. Write the ledger first. Print each changed file.
- Test: Not given as a separate line.
- Status: Shipped `89f84dc`, `088059d` (interrupted font setup recovers and action results reflect changed paths).

#### 20. `configure` behavior is absent from the README

- File:line: `README.md:165-168`, `src/configure.rs:28-50`, `src/configure.rs:74-75`, `src/configure.rs:150-172`, `src/configure.rs:354-365`
- Kind: documentation
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: Missing from the README: which file is edited and `HERDR_CONFIG_PATH`; that it runs `herdr config check` and `herdr server reload-config`; that backups accumulate as `config.toml.<random>.bak`; that symlinks are refused; that an existing `ui.sidebar.agents` / `spaces` is refused with "use manual setup". The README has no TOML example for that manual setup.
- Evidence: Those ranges versus the README section.
- Impact: A user who already has sidebar rows, or who needs to undo backups, has no procedure.
- Proposed change: Not stated as a separate sentence beyond documenting those behaviors and a manual TOML example.
- Test: Not given as a separate line.
- Status: Shipped `207cd97`

#### 21. Specs disagree with the implementation

- File:line: `openspec/specs/sidebar-tokens/spec.md:208-211`, `openspec/specs/provider-readers/spec.md:14-15`, `:46`, `:70-71`, `openspec/specs/daemon-lifecycle/spec.md:78-80`, `:106-113`
- Kind: documentation
- Severity: Medium
- Agreement: 1/2 by @claude
- Content: Cache invalidation by event subscription (the implementation does not subscribe); shared reads for the same identity (each pane reads); a timeout does not block other providers (one budget covers the whole cycle); six reader slots (Kimi and Hermes are not called); doctor prints capacity and provider reasons (it does not); config is six keys (`non_first_pane_logo_indent` exists).
- Evidence: Those spec lines versus the implementation.
- Impact: A reader of the specs will expect behavior the binary does not have.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `024bcb0`, `c9a56e6`, `00d5756`, `4a56afa`, `623a126` (Codex reader behavior and the named spec mismatches now match the implementation, including Kimi context).

### Low–Medium

#### 22. Release workflow details

- File:line: `.github/workflows/release.yml:8-9`, `:27`, `:22`, `:75-81`
- Kind: maintainability / security
- Severity: Low–Medium
- Agreement: 1/2 by @claude
- Content: `contents: write` is granted to the whole workflow, including the build job (the publish job is enough). Actions are referenced by major tag, and `dtolnay/rust-toolchain@master` is not pinned. Nothing checks that the tag commit is on main or that CI is green (the procedure says a person checks). Nothing checks that the heading date matches the tag date. `gh release create` fails when the Release already exists, so a rerun is fragile. The release workflow does not run tests.
- Evidence: Those workflow lines.
- Impact: The build job holds a write token, and a rerun or a tag that is not on a green main is not rejected by the workflow.
- Proposed change: Not stated as a separate sentence beyond the permissions, pins, and checks named above.
- Test: Not given as a separate line.
- Status: Shipped `4c4fb31`, `8683282`, `8161fac` for publish-job-only write permission, action pinning, automated main/CI/date gates, rerun idempotence, and release-workflow tests (no deferred items remain).

### Low

#### 23. The manifest says "planned" where the README says implemented

- File:line: `herdr-plugin.toml:4-7`
- Kind: manifest disagrees with the implementation and the docs
- Severity: Low
- Agreement: 1/2 by @codex (first pass)
- Content: The manifest comment and `description` show the sidebar publisher / integrations as "planned", while the same manifest registers `ensure`, events, and actions, and the README and changelog present them as implemented. Herdr's plugin list or detail view will be read as "not implemented yet".
- Evidence: Manifest text versus README, changelog, and the registered commands.
- Impact: The plugin list mislabels implemented features.
- Proposed change: Update the description to the implementation at release time.
- Test: Compare the manifest's displayed text with the README feature description.
- Status: Shipped `4c4fb31`

#### 24. Environment variables missing from the docs

- File:line: `src/daemon.rs:47`, `src/configure.rs:55`, `src/configure.rs:59`, `src/font.rs:416-417`
- Kind: documentation
- Severity: Low
- Agreement: 1/2 by @claude
- Content: `HERDR_SOCKET_PATH` (required), `HERDR_CONFIG_PATH`, `HERDR_BIN_PATH`, `XDG_DATA_HOME`, and `XDG_CONFIG_HOME` are not in the README. `configure` does not read `XDG_CONFIG_HOME`; font does (they do not match).
- Evidence: Those reads versus the README.
- Impact: Required variables are easy to miss, and the two XDG behaviors differ.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `207cd97` for documenting the variables. Deferred: unifying Herdr plugin config and terminal font config lookup would change their separate ownership paths.

#### 25. Daemon log has no time, no reason, and no rotation

- File:line: `src/daemon.rs:312-325`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: The log has no timestamp and no reason, and it does not rotate. An interruption from shutdown is also written as `refresh failed`.
- Evidence: That range.
- Impact: Not stated as a separate sentence.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `6262a38` (bounded timestamped logs record fixed reasons and keep one rotated generation).

#### 26. SIGTERM does not clear runtime files

- File:line: `src/daemon.rs` (no signal handler; no line number in the ballot)
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: SIGTERM does not clear, so `control.sock` and `daemon.pid` remain.
- Evidence: Absence of a signal handler.
- Impact: A signal stop leaves the socket and pid file behind.
- Proposed change: Not stated as a separate sentence.
- Test: The later test-gap list includes SIGTERM.
- Status: Shipped `6262a38` (the existing stale-runtime detection and recovery path is verified by a SIGTERM test; no signal-specific cleanup handler was added).

#### 27. Skipping one resource for capacity makes doctor exit non-zero

- File:line: `src/daemon.rs:313`, `src/doctor.rs:161`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: Skipping a single resource because capacity is short makes doctor exit non-zero.
- Evidence: Those lines.
- Impact: Doctor reports failure for one skipped resource.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `d1ee90d` (capacity skips no longer make an otherwise healthy doctor check fail).

#### 28. The daemon does not `chdir`, and git and codex inherit the full environment

- File:line: `src/daemon.rs:89-137`, `src/quota_codex.rs:61`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: The daemon does not `chdir`. `git` and `codex` are resolved on `PATH`, and the environment is inherited in full.
- Evidence: Those ranges.
- Impact: Not stated as a separate sentence. Finding 1 is the concrete program launch this environment inheritance allowed for Git.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `57facbf`, `6262a38` for Git query isolation and the daemon's private working directory. Deferred: narrowing the Codex child environment would risk executable lookup, proxy, certificate, and authentication compatibility.

#### 29. Grok weekly with no reset expires in 60 seconds but is fetched every 300

- File:line: `src/publish.rs:66-78`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: A Grok weekly value with no reset expires in 60 seconds, but it is fetched every 300 seconds.
- Evidence: Those lines.
- Impact: The value is expired for most of the interval.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `9b149b5` (a reset-less Grok weekly value stays visible through the next scheduled query).

#### 30. `metadata_daemon_child` can publish to a real session

- File:line: `src/publish.rs:2545-2554`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: `metadata_daemon_child` has no environment guard and can publish to a real session with `--include-ignored`.
- Evidence: That range.
- Impact: A test helper can write into a real session.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `c72e242` (metadata daemon tests use an isolated session and reject inherited Herdr paths).

#### 31. statusLine records grow per session and are never removed

- File:line: `src/statusline.rs:42-52`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: statusLine records accumulate per session, and nothing deletes them.
- Evidence: That range.
- Impact: The record directory grows without a bound.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Deferred: no reliable source identifies which statusLine sessions remain active, and unbounded context retention would change the current freshness contract.

#### 32. `--then` resolves `sh` from `PATH`

- File:line: `src/statusline.rs:347`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: `--then` resolves `sh` from `PATH`.
- Evidence: That line.
- Impact: Not stated as a separate sentence.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `917c21d` (`--then` invokes `/bin/sh` directly).

#### 33. Uninstall reports success when it removed nothing

- File:line: `src/font.rs:402`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: Uninstall prints `removed owned installation` even when it deleted nothing.
- Evidence: That line.
- Impact: The message overstates what happened.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `088059d` (uninstall reports when it manages no font or terminal config).

#### 34. The font file name includes a hash, so an old font cannot be removed after a swap

- File:line: `src/font.rs:11`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: The file name includes a hash, so a build that swaps the font cannot delete the previous font file.
- Evidence: That line.
- Impact: Replaced font files are left behind.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Deferred: no release has replaced the bundled font, so safe old-font removal cannot yet be verified against a replacement.

#### 35. Doctor appends the `auto` explanation regardless of mode

- File:line: `src/doctor.rs:183-186`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: The `auto` explanation is attached no matter what the mode is.
- Evidence: That range.
- Impact: Doctor text describes `auto` even when another mode is set.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `3bb2d2b` (doctor explains `auto` only when that mode is selected).

#### 36. `SECURITY.md` gaps

- File:line: `SECURITY.md` (no line numbers in the ballot)
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: No description of the git child process; no statement that a signal exit does not clean up; no supported-versions section. An internal approval date (`approved on 2026-10-12`) remains in a user-facing document.
- Evidence: The document as read for the ballot.
- Impact: The security document does not match the process model, and it exposes an internal date.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `4e187c7`, `6262a38`, `246f50e` for Git child-process boundaries, signal-exit recovery documentation, a supported-versions policy, and removal of the internal approval date (no deferred items remain).

#### 37. Testing evidence is from an older commit

- File:line: `docs/testing.md:99-163`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: The recorded evidence is from an older commit. There is no record of a rerun at HEAD.
- Evidence: That range.
- Impact: The testing document does not show a run at the reviewed commit.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `c72e242`, `86cb4c2` (testing record, and the isolated harness from publication through post-stop deletion).

#### 38. `fs2` is unmaintained

- File:line: `Cargo.toml:14`
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: `fs2` is no longer updated. `File::try_lock` in std can replace it.
- Evidence: That dependency line.
- Impact: Not stated as a separate sentence.
- Proposed change: Replace it with `File::try_lock`.
- Test: Not given as a separate line.
- Status: Shipped `95a5982` (`fs2` is removed in favor of standard-library file locking).

#### 39. Dependency licenses are not in the binary distribution

- File:line: dependency crates (rustls, ring, zstd, and others); no single line in the ballot
- Kind: not stated separately (Low list)
- Severity: Low
- Agreement: 1/2 by @claude
- Content: License notices for dependency crates such as rustls, ring, and zstd are not in the binary distribution.
- Evidence: The release bundle as reviewed.
- Impact: Third-party notices are missing from the bytes that are shipped.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `c7c3aa5`, `852293c` (dependency license texts are recorded, checked, and packaged in the release archive).

#### 40. Kimi and Hermes readers are only used by their own tests

- File:line: `src/reader_kimi.rs:4`, `src/reader_hermes.rs:4`
- Kind: unused code
- Severity: Low (placed with the unused-code notes, which the ballot did not give a separate severity)
- Agreement: 1/2 by @claude
- Content: Nothing in the program calls these modules. Only their own tests use them.
- Evidence: Those modules.
- Impact: Not stated as a separate sentence.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `4a56afa`, `623a126` for a called Kimi context reader. Deferred: Hermes has no verified local usage source to enable its reader.

#### 41. Public items that are unused or test-only

- File:line: `WeeklyWindow::CalendarWeek` (`src/model.rs:57`), `tokens::DEFAULT_REFRESH_MS` (`src/tokens.rs:7`), `Config::ttl_ms` (`src/config.rs:84`), `quota_codex::read_weekly` (`src/quota_codex.rs:36`), `ship_time::read_repository` (`src/ship_time.rs:24`), `NoopPublisher` (`src/daemon.rs:66`), `ErrorClass` (`src/protocol.rs:14-25`)
- Kind: unused code
- Severity: Low (same grouping as finding 40)
- Agreement: 1/2 by @claude
- Content: These public items are unused or exist for tests only.
- Evidence: Those definitions.
- Impact: Not stated as a separate sentence.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `65859a3` (unused or test-only public items are removed or restricted).

#### 42. Agent-name mapping is copied in three places

- File:line: `src/tokens.rs:92-99`, `src/protocol.rs:175-183`, `src/publish.rs:931-940`
- Kind: unused code (the ballot filed this under the unused-code heading)
- Severity: Low (same grouping as finding 40)
- Agreement: 1/2 by @claude
- Content: The agent-name mapping exists in three places.
- Evidence: Those three ranges.
- Impact: Not stated as a separate sentence.
- Proposed change: Not stated as a separate sentence.
- Test: Not given as a separate line.
- Status: Shipped `65859a3` (agent metadata is centralized).

#### 43. Missing tests

- File:line: no single line. The ballot names hostile Git config, a rollout over 16 MiB, the 4,096-entry boundary, `control.sock` path length, a control client that closes immediately, SIGTERM, and checkpoint growth.
- Kind: test gap
- Severity: Low (the ballot's heading is "テストの抜け" and does not assign another severity)
- Agreement: 1/2 by @claude
- Content: Those cases are not covered. macOS in CI is unit tests only. End-to-end runs on Linux only and outside CI.
- Evidence: The test suite as read for the ballot.
- Impact: The failures in findings 1, 2, 16, 17, and 26 can land without a test.
- Proposed change: Not stated as a separate sentence beyond adding those cases.
- Test: The cases listed in the content.
- Status: Shipped `57facbf`, `024bcb0`, `47c181f`, `f2e9a30`, `8b8f2c4`, `6262a38`, `37c23ca` for the named Git, rollout, entry-limit, socket-path, immediate-close, SIGTERM, and checkpoint tests. Deferred: macOS end-to-end coverage needs a dedicated runner and isolated Herdr session.

## Differences

The dedup key is file:line plus kind. These overlaps describe the same area and were not merged.

- Planned wording. @claude finding 7 is kind "documentation" at `herdr-plugin.toml:4`, `herdr-plugin.toml:7`, `src/lib.rs:1`, and `src/lib.rs:64`, severity Medium, and includes the crate comments. @codex finding 10 is kind "stale code or stale text" at `herdr-plugin.toml:4-7`, severity Medium, and treats the README's "public installation is Planned" as a separate true statement. @codex finding 23 is kind "manifest disagrees with the implementation and the docs" at the same `herdr-plugin.toml:4-7`, severity Low. The first Codex pass did not mention `src/lib.rs`.
- Install and removal. @claude finding 4 (High, documentation) says the checkout install, the user-facing usage example, and uninstall are missing as one path. @codex finding 12 (Medium, first pass) is only the release-binary install, and accepts either a binary procedure or an explicit "not supported". @codex finding 11 (Medium) is the disable order: text mode before font removal, unlink versus uninstall, and manual statusLine. The line ranges overlap and the kinds differ, so they stay three findings.
- @codex finding 9 (quota cache cleared on every `account/read` error) is not in the Claude ballot.
- @claude findings 1, 2, 3, 5, 6, and 13–22 and 24–43 are not in either Codex ballot. The Codex main ballot says that, inside the range it checked, it did not confirm any further release-blocking issue. The first Codex pass says it did not confirm any further concrete low-severity issue.
- Scope of what was run. @claude read the release bits, workflow, scripts, README, SECURITY, and NOTICE directly; two subagents read the body (font / configure / statusline / doctor / config, and daemon / lock / socket / publish / readers / quota / ship_time); High findings were re-read or reproduced by the parent. Cargo was not run; green main CI was the basis. @codex's main ballot matched the named HEAD, ran `scripts/check-version.sh v0.1.0` and `scripts/extract-release-notes.sh 0.1.0`, and did not run Cargo fmt, clippy, test, release build, or `--help`. It did not touch the Herdr server, a running daemon, or user settings, and it did not change files or create a worktree. @codex's first pass read the brief, README, manifest, release guide, changelog, security doc, OpenSpec, implementation, release scripts and workflow, and the font notice. `cargo test --locked` could not open `target/debug/.cargo-build-lock` on a read-only sandbox filesystem. Other Cargo checks, a macOS device check, and the official plugin docs were not finished (the official page was not reachable from the browser tool).
- Both ballots say the CLI commands and manifest actions largely correspond, and that `allow_network` defaults to false. @claude additionally checked that outbound traffic and `codex app-server` do not start when `allow_network = false` (worker construction is only after the return at `src/publish.rs:207-209`), that logs are fixed strings, HTTP/RPC errors are fixed categories, and `log` is `max_level_off`. @codex's main ballot says the Codex and Grok quota workers are opt-in and, in the implementation, do not log or store conversation text or credential values, and that config changes and font operations happen only through explicit actions.

## Spec checks

Questions of contract, taken from the ballots without adding a new judgment.

- Public installation stays Planned. Both Codex notes treat that README statement as the correct constraint for a private repository. The disputed "planned" text is the manifest description of the sidebar publisher and agent integrations.
- `logo_mode = "auto"` is specified by behavior as always equal to text, because `font_verified` is always false (finding 5).
- Spec versus code in finding 21: event-subscription cache invalidation is specified and not implemented; same-identity reads are not shared; one cycle budget means a timeout can block other providers; six reader slots are specified while Kimi and Hermes are not called; doctor is specified to print capacity and provider reasons and does not; the config key count omits `non_first_pane_logo_indent`.
- Daemon lifecycle spec `openspec/specs/daemon-lifecycle/spec.md:37-39` says a deadline is not applied with `setsockopt`. At review time the control path still used `set_read_timeout` / `set_write_timeout` with `?` (finding 15).
- `configure` does not consult `XDG_CONFIG_HOME`; font installation does (finding 24).
- Checked and consistent in the Claude ballot: version check and note extraction (`v0.1.0` passes; `v0.1.1`, `v0.1.0-rc1`, and `vv0.1.0` fail; extraction is 19 lines from the heading); `NOTICE.md` font SHA-256, family name, and mapped range match the implementation; the config key list, defaults, and ranges, and the subcommand-to-action map, match the README; README `herdr plugin install --ref`, `link`, `config-dir`, and `action invoke` exist in the Herdr 0.9.3 CLI.
- Checked and consistent in the Codex main ballot: `statusline`, `ensure`, `daemon`, `stop`, `doctor`, `configure`, `unconfigure`, `install-font`, and `uninstall-font` largely match the manifest actions; the setting keys (`refresh_interval_ms`, `reader_timeout_ms`, `usage_max_age_ms`, `logo_mode`, `non_first_pane_logo_indent`, `providers`, `allow_network`), statusLine usage, the main environment variables, and the sidebar tokens are in the README, SECURITY, or specs; `LICENSE`, `NOTICE.md`, `SECURITY.md`, and the CHANGELOG 0.1.0 section exist, and the font attribution and MIT text are included. Daemon stop uses socket identity and the lock rather than a signal to a pid that was only checked once. `scripts/check-version.sh v0.1.0` succeeded and the three version sites match. `scripts/extract-release-notes.sh 0.1.0` extracted `## 0.1.0 (2026-10-13)`. The release workflow is set up to build Linux and macOS from a tag and attach checksums and extracted notes. That ballot did not run the build or the publish.
- Checked and consistent in the Codex first pass: commands, actions, setting keys, sidebar tokens, and environment variables are mostly in the README; font license attribution is in `NOTICE.md`; the release-notes heading matches the requested version. `scripts/check-version.sh v0.1.0` succeeded and the notes extractor printed the section. HEAD matched `fca5bbcbdb06627534726f03099831061d4ac77f`.

## Awards

Solo findings, ordered by severity and by how hard the point is to see from a normal reading. A finding both ballots described, even under a different kind, is not in this list. Emoji are only the rank marks.

1. 🥇 @claude — repository-local Git config runs a program (finding 1). High, and it stays invisible until `log.showSignature` and `gpg.program` are actually set. The ballot reproduced it on Git 2.56.0 and showed that `for-each-ref` does not take that path.
2. 🥈 @claude — long Codex sessions always lose context (finding 2). High, and it is the opposite of the documented promise: the sessions that most need context are the ones over 16 MiB. The ballot counted 118 of 1,559 local rollouts over that cap.
3. 🥉 @claude — `install-font` and `doctor` fail when typed as the README shows (finding 3). High, and the README says the commands run directly. The ballot measured that a Herdr 0.9.3 pane shell does not have `HERDR_PLUGIN_*`, while `configure` alone has a fallback.
4. 4️⃣ @claude — icons need four steps, and `auto` is text (finding 5). High as an install gap, and the non-obvious parts are easy to miss in separate README sections: `font_verified` is always false, only Ghostty and kitty are rewritten, no backup is taken, SSH installs on the far machine, and uninstalling the font in font mode yields tofu.
5. 5️⃣ @codex — every `account/read` error clears the weekly cache (finding 9). Medium, and it is the only behavioral bug in the Codex ballots that the Claude ballot does not contain. `PermissionDenied` is the signal `WeeklyClient` uses to drop a value that the docs say should survive a transient failure until reset.

## Impressions

### Claude

I was not fully convinced that a Git setting could run an arbitrary program until I reproduced it. Adding a pass that checks the docs against the implementation, one item at a time, caught gaps that reading the code alone does not, such as the (planned) wording and what the archives include.

### Codex

The first pass stayed on the release documents and finished in about a minute, so it only reported the missing binary install path and the planned manifest text. On the second pass I held a tag back for three items: every `account/read` error clearing the quota cache, the planned manifest wording, and the lack of an ordered uninstall. I did not run Cargo, and those two passes did not surface the `log.showSignature` launch path or the 16 MiB rollout cutoff.

### Agy

Absent. Both rounds failed in agent-room before a ballot, with `headless_payload_malformed` and `headless_payload_invalid_terminal`.

Status last updated: 2026-10-14 at tag `v0.1.1` `cacef43efab070cd17e5a5f6448aabc5bdb039dc`.
