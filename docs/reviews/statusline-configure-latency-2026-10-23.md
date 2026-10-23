# Statusline and configure synchronous latency, 2026-10-23

## Scope and source

This is a record-only measurement of `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0` on Linux 7.2.10 x86_64. It reads the README, `src/statusline.rs`, `src/configure.rs`, CLI dispatch in `src/lib.rs`, and the plugin manifest at that commit. The fixture ran the release binary from a dedicated linked worktree. It did not invoke a real Herdr binary, server, or session, access user configuration, use the network, or change plugin behavior. `/tmp` was tmpfs on this host; all fixture homes, configuration, state, command scripts, and event logs were inside one temporary directory that the harness removed after the run. These numbers are not a macOS, disk-backed, or live Herdr measurement.

## Synchronous contract and boundaries

| Path | Current boundary at the fixed commit |
| --- | --- |
| `statusline claude` | Reads stdin to EOF, capped at 1 MiB plus one byte; parses a JSON object, extracts an allowlisted observation, and writes a temporary record before returning. Recording failure is silent and returns success without `--then`. |
| `statusline claude --then <command>` | After the capture attempt, starts `/bin/sh -c <command>`, forwards the original stdin bytes (and any remainder) on a writer thread, copies child stdout, waits for the child, and returns its exit status or mapped signal status. The child therefore cannot start before reaching EOF or acquiring 1 MiB + 1 byte (`MAX_INPUT + 1`) and completing the capture attempt. The wrapper does not impose a child deadline. |
| `configure` | Resolves `HERDR_CONFIG_PATH` or `$HOME/.config/herdr/config.toml`, obtains a plugin-state transaction lock, reads and plans the config, stages and syncs a candidate, runs `HERDR_BIN_PATH config check` with the staged path, creates and syncs a sibling backup, checks for a concurrent edit, atomically replaces the target, then runs `HERDR_BIN_PATH server reload-config` with the target path. It waits for both child commands. An unchanged plan returns without either child. On reload failure it may restore the original and attempt a second reload. |
| Timeouts | Neither `statusline --then` nor either `configure` child has a timeout in these paths. `reader_timeout_ms` in the README (default 2000 ms) bounds usage readers in the publisher, not these commands. Any statusLine host or Herdr action deadline is outside this source and was not measured. |

## Isolated measurement

The harness used Python `time.monotonic_ns()` around each release-binary invocation. It repeated each case 12 times with a new absolute `HOME`, `HERDR_PLUGIN_STATE_DIR`, and `HERDR_PLUGIN_CONFIG_DIR`. The statusline input was synthetic JSON with `session_id=synthetic-1`: a 70-byte small record and a 1,048,570-byte valid JSON record with padding. For `--then`, the command was an absolute synthetic Python executable that logged its entry and exit, read stdin, and printed `synthetic-output`. The harness asserted the forwarded byte count, stdout, exit status, and one isolated observation record. No actual statusLine command was invoked.

For configure, `HERDR_CONFIG_PATH` pointed inside each synthetic home and `HERDR_BIN_PATH` pointed to a fake executable accepting only `config check` and `server reload-config`. The fake verified the path was inside the fixture and the managed marker was present, logged entry and exit, and returned success. The change case started with `theme = "synthetic"` and asserted one backup. The unchanged case ran configure once as setup, cleared the event log, then timed a second invocation and asserted there were no check/reload calls. The fake did no real configuration validation or server reload. All figures below are milliseconds (min / median / max); they include process startup and scheduler effects. They are observations for this fixture, not service latency estimates.

| Case, 12 runs each | Whole invocation | Entry to child | Child body | Child exit to parent exit |
| --- | ---: | ---: | ---: | ---: |
| Small record, no `--then` | 0.911 / 0.981 / 1.534 | — | — | — |
| Small record, `--then` | 14.484 / 16.609 / 17.745 | 12.153 / 13.721 / 14.403 | 0.028 / 0.042 / 0.515 | 2.066 / 2.426 / 3.538 |
| 1,048,570-byte record, no `--then` | 4.823 / 5.270 / 5.832 | — | — | — |
| 1,048,570-byte record, `--then` | 23.549 / 24.692 / 27.084 | 16.677 / 18.195 / 19.592 | 3.493 / 3.877 / 4.305 | 2.234 / 2.842 / 3.613 |

`Entry to child` is wrapper launch to the synthetic Python child's first timestamp. It includes stdin reading, JSON capture, record persistence, shell launch, and Python startup; those parts were **not measured separately**. `Child body` is only the synthetic child's own timestamp interval. The remaining column includes child teardown and wrapper exit. A faster native child would change the totals, so the difference between the no-`--then` and `--then` rows is not an isolated shell cost.

| Configure case, 12 runs each | Min | Median | Max |
| --- | ---: | ---: | ---: |
| Changed: whole invocation | 29.401 | 31.388 | 34.536 |
| Launch to fake check entry | 12.713 | 13.965 | 14.932 |
| Fake check entry to exit | 0.021 | 0.025 | 0.038 |
| Fake check exit to fake reload entry | 14.299 | 15.012 | 17.596 |
| Fake reload entry to exit | 0.018 | 0.020 | 0.037 |
| Fake reload exit to parent exit | 2.265 | 2.386 | 3.203 |
| Unchanged: whole invocation | 1.030 | 1.217 | 1.606 |

The interval before check includes plan, staging, file sync, and Python process startup. The interval between check and reload includes check process teardown, backup creation and sync, replacement, and another Python startup. These individual operations were **not timed separately**. The fake check/reload body times do not predict live Herdr command or server latency. No failure/rollback path, concurrent lock wait, real disk, macOS, or actual host timeout was measured.

## Decision and follow-up boundary

The isolated runs do not justify changing the current behavior or adding a timeout now. Small captured records took about 1 ms without chaining; most of the chained fixture's elapsed time was before the synthetic Python child's first timestamp, with capture, shell launch, and Python startup unresolved within that interval. Configure's no-change path took about 1 ms, while the changed fixture paid for two synthetic child launches and file transaction work. Actual command and reload distributions are still unknown. The existing full-JSON parse near the 1 MiB cap remains a possible optimization only if real input-size data shows that payloads are common and material; the fixture's 5.270 ms median alone is insufficient.

If later evidence supports an optimization, preserve exact stdin and stdout bytes, child exit and signal mapping, silent record failures, 1 MiB capture cap, accepted JSON field aliases, managed-block ownership, validation-before-replacement, unique backup, lock scope, concurrent-edit refusal, and reload rollback. A statusLine timeout would change the existing child-wins contract and must define child/process-group termination, output already forwarded, and return status. A configure timeout must specify separate check and reload budgets and what happens after replacement, including rollback and its second reload. `reader_timeout_ms` must not be reused for these operations without a separate contract decision. Measure actual host deadlines and live command latency through an approved, isolated method before selecting those budgets.
