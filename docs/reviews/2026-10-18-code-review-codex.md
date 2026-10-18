# Codex repository review — 2026-10-18

## Scope and result

Reviewed `f678ad7d3426b9c791bbdd10660d75534758c2b9` from `origin/main`, including the Kimi weekly worker. This is a source and specification review, not a live runtime measurement. I found two actionable issues below. I did not change implementation, configuration, credentials, or sessions.

I read the README, manifest, `AGENTS.md`, `SECURITY.md`, OpenSpec specifications and the Kimi change, testing/releasing instructions and research, prior review statuses, Rust source, tests, and CI/release workflows. I traced the daemon startup, refresh, publication and checkpoint, per-provider readers, quota workers, statusLine capture, socket/control, shutdown, and release gates. No database, Web API, or UI framework is part of this plugin.

## System model (inferred from code and documentation)

The plugin's unit of publication is a Herdr pane or workspace. An authoritative `session.snapshot` supplies pane occupancy and workspace location. The daemon maps each pane to a provider reader, overlays optional account quota, builds `lineup_*` display tokens, reserves a monotonic metadata sequence in `sequences.json`, then sends the patch to the pinned local Herdr socket. A workspace heading reads the latest local `origin/main` commit time through a bounded Git child. One daemon owns a per-session lock and a control socket; `ensure` starts or finds it, while `stop` requests bounded cleanup without signalling a PID. These are source-level relationships, not measured behavior in a live Herdr session.

Local context is associated with a native session for Codex, Grok, Kimi, and Claude; Antigravity statusLine uses a pane ID. Weekly quota is separate: Claude/Antigravity take statusLine observations, Codex runs an app-server child, and Grok/Kimi use opt-in HTTP workers. Missing observations render `—`. Runtime state belongs in `HERDR_PLUGIN_STATE_DIR`, user settings in `HERDR_PLUGIN_CONFIG_DIR`.

## Findings

### 1. Antigravity can reuse the previous occupant's statusLine observation

- **Priority / effort / change risk:** Medium; small-to-medium effort. Changing the binding or freshness rule can make an idle Antigravity row temporarily unavailable, so the acceptance rule needs an explicit decision.
- **Location:** `src/statusline.rs:167-170`, `src/statusline.rs:290-305`; `src/reader_agy.rs:10-22`; `src/publish.rs:741-764`, `src/publish.rs:861-902`; `src/tokens.rs:169-179`.
- **Trigger and impact:** A new Antigravity occupant takes over the same Herdr pane before it emits a statusLine callback. Capture stores the old observation under `HERDR_PANE_ID` only. The reader accepts that same key for the new occupant, and publication constructs a fresh `Usage` with the new occupant and the old observation's pane binding. `pane_tokens` therefore accepts the old context for the new occupant. Context has no age expiry, and weekly with a future reset also remains visible. Clearing `local_readings` when native identity changes does not help because the persisted pane-keyed record is reread immediately. This can attribute another session or account's usage percentages to the new row.
- **Evidence:** The code path above is a deterministic source-level inference; I did not attach to a live pane. `openspec/specs/sidebar-tokens/spec.md:100-106` requires previous usage to be invalidated on agent or native-session replacement, while `openspec/specs/provider-readers/spec.md:326-331` says observations must not be shared across unrelated sessions or accounts. The pane-only binding itself is intentional; its reuse across occupant generations is the gap. The 2026-10-13 review has no matching finding.
- **Improvement:** Give the persisted Antigravity observation an occupant generation or reliable native-session binding when one is available, and reject an observation from an earlier generation. If the hook cannot observe such an identity, explicitly define a conservative transition rule that withholds usage until a callback after the occupant change. Avoid expiring all idle context merely to solve pane reuse.
- **Verification after a fix:** In an isolated fake snapshot, publish Antigravity occupant A and one statusLine record, replace it with occupant B on the same pane without a callback, and assert context/weekly are unavailable. Emit B's callback and assert its values appear. Repeat with no native ID and with a future weekly reset.

### 2. A nonregular Grok credential path can block its quota worker indefinitely

- **Priority / effort / change risk:** Medium; small effort. Stricter file checks may reject a symlink-based credential setup that currently works, so document that compatibility change.
- **Location:** `src/quota_grok.rs:90-103`, `src/quota_grok.rs:175-183`, `src/quota_grok.rs:206-233`, `src/quota_grok.rs:369-398`; `src/model.rs:129-162`, `src/model.rs:181-190`.
- **Trigger and impact:** With network quota enabled, `GROK_AUTH_FILE` or the selected `auth.json` can name a FIFO or another blocking special file. `File::open` follows links and can wait for a FIFO writer before the 64 KiB `take(...).read_to_end(...)` limit applies. This call happens before the HTTP client's timeout. The single Grok worker remains in its query and cannot produce a new weekly result until the file operation completes; worker drop deliberately does not join or cancel it. The refresh loop should remain responsive, but Grok weekly quota can stay unavailable or stale for the daemon lifetime.
- **Evidence:** This is inferred from the unguarded `File::open` and Unix FIFO semantics; no real credential file or FIFO was opened during this review. By comparison, `src/quota_kimi.rs:396-411` opens with `O_NOFOLLOW | O_NONBLOCK` and verifies a regular file. The 2026-10-13 review did not list this Grok path.
- **Improvement:** Require a regular credential file and open it without following the final symlink or blocking on special files. Validate the opened descriptor before reading, retain the byte cap, and return a fixed `AuthShape`/`AuthIo` category on rejection. Apply the same rule to the explicit and fallback paths.
- **Verification after a fix:** Under a synthetic HOME, test a regular JSON file, a symlink, a FIFO with no writer, and an oversized file. The FIFO case must return promptly without a provider request; a fake loopback server should receive only the regular-file case. Check stop while the worker encounters the invalid path.

## Prior findings and decisions kept separate

The previous review's finding 9 (Codex `account/read` transient error classification) is marked **Shipped** in `docs/reviews/code-review-2026-10-13.md`; this review did not find a recurrence. The same record marks finding 31 (statusLine record accumulation) and finding 34 (old font removal after a replacement) **Deferred**. I did not relabel them as new findings. Findings 22 and 36 are marked **Shipped**, and their workflow/security documentation was present at this base.

The Kimi worker invalidates a cached sample when credential/config stamps change and discards a response if stamps differ across the poll (`src/publish.rs:443-468`). Requery can wait for the next interval, but the Kimi change specification explicitly permits one request per interval; I did not treat that wait as a defect. A daemon-wide Kimi account sample shared among Kimi panes is also an explicit design choice, not evidence of pane-specific account identity.

## Other areas checked and limits

- **Daemon, socket, sequences:** Checked endpoint pinning, control identity, stop path, worker shutdown, bounded framing, and durable sequence reservation. No additional issue met the evidence threshold. The earlier macOS end-to-end coverage gap remains deferred in the prior review.
- **Readers and statusLine:** Checked exact-session association, local file bounds and symlink handling, timeout retention, weekly reset handling, and transparent `--then` path. Finding 1 is the occupant transition concern; live statusLine accuracy was not measured.
- **Dependencies and CI:** Checked the locked Rust dependency declarations, pinned GitHub actions, CI checks, release gate, version alignment instructions, and packaging path. No dependency advisory scan, action run, release publication, or external API check was performed here.
- **Unverified:** Real provider response shapes, quota accuracy, account-to-pane attribution, macOS runtime behavior, visible font/sidebar rendering, and a full isolated Herdr lifecycle. No authenticated provider query, user HOME credential/session read, external network measurement, or action against the running Herdr server was used.

The findings are source-level inferences. The review contains no measured runtime performance numbers.
