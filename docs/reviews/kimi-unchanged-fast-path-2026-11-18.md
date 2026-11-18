# Kimi unchanged-state fast path on Linux, 2026-11-18

The trial calls for shipping code when Kimi file calls drop, daemon CPU
or user-space instructions improve beyond the A/A spread, and stored tokens
match. The experiment meets those conditions on this fixture. The ordinary
Kimi path drops from 2,212 to 1,828 read, open and stat calls per refresh
(17.4%). Across three refreshes, B uses 99,482 fewer instructions in AB
and 145,721 fewer in BA with identical checkpoint tokens. The A/A instruction spread
is 341.

## Candidate

A is `324c6714c9c603ec573ced83e3d743222a1e679e`. B is experiment commit
`8c9cb26f59079a450f1b889b7c68cf39c180f219`.
[The populated source reader baseline](source-reads-populated-baseline-2026-11-18.md)
observed 2,212 attributed Kimi read, open, and stat calls per complete unchanged
refresh, plus 418 directory-entry calls. No Kimi payload bytes were read in
those warm traces.

The [provider reader contract](../../openspec/specs/provider-readers/spec.md#requirement-one-reader-cycle-per-refresh)
limits discovery sharing to a single refresh. The trial leaves candidate
discovery in that cycle and optimizes reuse of previously parsed Kimi
`state.json` bindings. Each state file must still be freshly opened for
reading with no-follow and regular-file checks. B takes the fingerprint from
the descriptor metadata already obtained by that open. A cache hit must
match the descriptor's device, inode, size, mtime and ctime. A final
non-following path stat must still match that descriptor snapshot, and the
reader must still have time left. Since the cache hit reads no payload from
the descriptor, that final pathname observation establishes which file
supplied the cached bytes. Cache misses keep path and descriptor validation
after the payload read.

The reader still checks provider association, enumerates sessions, and
matches the requested native session and lexical cwd. Matching wire
files and model windows still follow their ordinary validation paths. The
daemon remains on the existing periodic publication and token renewal path.

## Invalidation tests

The test runner used UID 1000. The permission test changed a warm state file
to mode 000. The deadline test used a zero-duration request after warming
the cache. Neither test depends on a timed write. Each test builds its own
synthetic tree and starts from a successful read.

| Change after a warm read | Expected result |
| --- | --- |
| Rewrite the state binding ID | Unassociated |
| Switch the requested native session | Second session's usage |
| Append a completed wire turn | New usage |
| Atomically replace the state file | Unassociated |
| Atomically replace the wire file | New usage |
| Create a second matching session directory | Unassociated |
| Remove the only matching session directory | Not found |
| Change the pane cwd | Unassociated |
| Revoke read permission on the matching state | Unassociated |
| Expire the reader deadline | Timeout |
| Change the selected model window | Recalculated usage |

In an uncommitted B variant the Kimi reader returned the first result on each
test thread before checking identity, files, or budget. `cargo test --locked
unchanged_trial --lib` finished with 11 assertion failures and exit 101.
Restoring B gave 11 passing tests in the full suite. The variant patch and
test logs are under the experiment worktree's `target/kimi-unchanged-trial/`.
The zero-duration request exercises deadline rejection before discovery.
An expiry during the cache hit and a same-size rewrite with restored mtime
remain outside the mutation coverage.

## Comparison method

The controller derives from the baseline's retained `work/nominal/bench.py`
(SHA-256 `1cd06bcf1639d46e2a913969b011152bbecb84929c3cb25faa9ff9a5d7279083`).
It reuses the same four-provider synthetic fixture, Herdr 0.9.5, shell and
CPU affinities (controller 11, daemon 16, Herdr 12), the 10 s refresh
interval and the 10 s reader timeout. Every case starts its own guarded
Herdr server with a private session, home, socket, checkout and state.
All four normalized fixture digests matched the original baseline in all
six cases. The Kimi fixture has 385 files and normalized SHA-256
`fb2b6eef9e1ae5c3922c33ae1664232706b50e0d15dd127d690201e4d5487fee`.
No user sessions or provider homes are part of the run.

A/A runs first in order A1, A2. The comparison follows in order A3, B1,
B2, A4. A completion counter aligns each window after a completed refresh.
CPU/instructions and latency run in separate nominal 36 s windows. The
strace window stops after the third successful refresh, confirmed by the
completion counter beginning 27 s after attach. The read-only daemon
health counters check
that each window has exactly three successful refreshes and three completed
Kimi workers. For file call attribution a complete worker trace starts at
the Kimi home stat and ends at model-window validation.

CPU time uses the daemon's own `/proc/<pid>/stat` user and system tick delta
with 100 ticks per second. It excludes waited-child CPU. The perf window
captures inherited `instructions:u` and `task-clock`. RSS and PSS are
medians of the `smaps_rollup` samples within each perf window. File calls
are from `strace -f`, attributed by paths under `.kimi-code`. The delay
window appends four updates per provider at 4 s intervals while polling
stored context via the private Herdr snapshot. A delay interval runs
from the previous negative poll's start to the first matching poll's end,
relative to the source write. A value superseded before a refresh is counted
as unobserved. Stored token maps are compared at the end of the unchanged
and delay windows.

## Equal-count results

All entries represent three completed refreshes. Process CPU, task-clock and
instructions are totals for those refreshes. File calls are per refresh.

| Run | Daemon CPU ms | Instructions:u | Task-clock ms | RSS KiB | PSS KiB | Kimi reads | Kimi opens | Kimi stats | Kimi read/open/stat | Kimi dirent calls |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A1 | 280 | 25,029,575 | 300.09 | 5,568 | 3,324 | 0.0 | 406.0 | 1,806.0 | 2,212.0 | 418.0 |
| A2 | 160 | 25,029,916 | 181.90 | 5,620 | 3,451 | 0.0 | 545.3 | 2,084.7 | 2,630.0 | 696.7 |
| A3 | 460 | 25,035,942 | 484.77 | 5,556 | 3,312 | 0.0 | 475.7 | 1,945.3 | 2,421.0 | 557.3 |
| B1 | 460 | 24,936,460 | 489.81 | 5,548 | 3,305 | 0.0 | 406.0 | 1,422.0 | 1,828.0 | 418.0 |
| B2 | 230 | 24,920,074 | 264.57 | 5,564 | 3,320 | 0.0 | 406.0 | 1,422.0 | 1,828.0 | 418.0 |
| A4 | 340 | 25,065,795 | 382.48 | 5,420 | 3,176 | 0.0 | 475.7 | 1,945.3 | 2,421.0 | 557.3 |

| Run | Complete Kimi workers by shape (read/open/stat/dirent) | CPU 16 busy % | Load (1m start → end) | Window elapsed s (perf/trace/delay) |
| --- | --- | ---: | --- | --- |
| A1 | 3 × 0/406/1806/418 | 80.25 | 76.56 → 62.06 | 36.1 / 34.4 / 36.2 |
| A2 | 1 × 0/406/1806/418, 2 × 0/615/2224/836 | 97.91 | 58.58 → 65.72 | 36.5 / 55.8 / 36.1 |
| A3 | 2 × 0/406/1806/418, 1 × 0/615/2224/836 | 73.82 | 55.85 → 49.90 | 36.4 / 52.2 / 36.2 |
| B1 | 3 × 0/406/1422/418 | 55.47 | 66.00 → 57.51 | 36.1 / 34.0 / 36.1 |
| B2 | 3 × 0/406/1422/418 | 88.36 | 64.94 → 62.07 | 36.1 / 35.3 / 36.0 |
| A4 | 2 × 0/406/1806/418, 1 × 0/615/2224/836 | 79.86 | 35.40 → 34.15 | 36.1 / 55.5 / 36.1 |

Positive paired deltas mean B used less. A/A spread is the absolute difference
between A1 and A2. AB is A3 minus B1. BA is A4 minus B2.

| Metric | A/A spread | AB delta | BA delta | Median paired delta |
| --- | ---: | ---: | ---: | ---: |
| Kimi read/open/stat per refresh | 418.0 | 593.0 | 593.0 | 593.0 |
| Daemon CPU ms per window | 120 | 0 | 110 | 55 |
| Instructions:u per window | 341 | 99,482 | 145,721 | 122,602 |
| Task-clock ms per window | 118.19 | -5.04 | 117.91 | 56.44 |
| RSS KiB median | 52 | 8 | -144 | -68 |
| PSS KiB median | 127 | 7 | -144 | -68.5 |

The usual trace shape matches the original baseline. B removes 384 stats
per ordinary refresh, or two per state file across the 192 sessions. Open
and directory-entry counts in that shape are equal. Four A workers showed a
second traversal signature, with 209 extra opens, 418 extra stats and 418
extra directory-entry calls. This is consistent with discovery's validation
fallback. The specific invalidating path is unobserved. Those workers are
included in the totals, so the aggregate reduction of 593 calls per refresh
in each pair includes the difference in traversal frequency. All 18 traces
reached the end of the Kimi model-window check, and attributed payload reads
were zero.

The instruction decreases are 0.40% in AB and 0.58% in BA. Both exceed the
341-instruction A/A spread. Across all four A windows the spread is 36,220
instructions, and the two B windows differ by 16,386. The lowest A count is
93,115 instructions higher than the highest B count. Daemon CPU changes of
0 ms and 110 ms fall within the 120 ms A/A spread. CPU and memory
measurements include the effects of a shared host with unreserved cores.

## Source-to-token delay

Intervals are in seconds after the write. The polling cadence was approximately
250 ms. Each window made four updates per provider. The last write and final
stored value were 15% in every case.

| Run | Kimi observations, percentage with delay bounds | Unobserved percentages | Polls |
| --- | --- | --- | ---: |
| A1 | 13% [3.922, 4.183], 15% [5.938, 6.198] | 12%, 14% | 143 |
| A2 | 14% [0.000, 0.031], 15% [6.040, 6.299] | 12%, 13% | 144 |
| A3 | 14% [0.000, 0.041], 15% [6.109, 6.381] | 12%, 13% | 144 |
| B1 | 14% [0.000, 0.024], 15% [6.510, 6.767] | 12%, 13% | 144 |
| B2 | 13% [3.767, 4.022], 15% [6.026, 6.285] | 12%, 14% | 144 |
| A4 | 14% [0.000, 0.052], 15% [6.059, 6.314] | 12%, 13% | 144 |

For the common 15% write, the A/A lower and upper endpoints span 0.102 s and
0.101 s. Subtracting the A observation interval from the B interval gives
+0.129 to +0.658 s in AB and −0.288 to +0.226 s in BA. Intermediate 13% and
14% observations also differ in A/A as the scheduled write and the refresh
race. The full stored pane-token maps matched byte for byte across cases
at the unchanged checkpoints (10%) and the final delay checkpoint (15%).
The unchanged checkpoints span longer than the 30 s token TTL. Canonical
UTF-8 JSON with sorted token keys hashes to
`2ad125c66ab0fcc61c6cc822b8677998f7c2f255684fcd707ca73d47153c6ca2` at
the unchanged checkpoints and
`01d96666d66b345b197b65d0048292769a852c5d04e1793307a32e50a90b0865` at
the final delay checkpoint.

## Artifacts


The raw comparison is retained under `$HOME/.cache/dev-task-temp/runs/`.
The measurement commands run on Linux 7.3.3 with ext4, perf 7.3.1-1 and
strace 7.3. Each accepted case exited with code 0 for Lineup and the
private Herdr server and deleted its private session.

| Series | Managed run | Relative path |
| --- | --- | --- |
| Original fixture and controller | `e0cf00f4c2e5f8b9d22b056aae886159` | `work/nominal` |
| A/A | `69742abadcbfa7741991d75e7010a313` | `work/aa` |
| AB/BA | `6eed5833288a60761c0de012fb1f9b3e` | `work/abba` |

Both accepted run directories retain the comparison controller (SHA-256
`7cb87a28c930e02b86c1a732895abfb08938601336aa0977631bfad8ff4963e6`),
fixture inventories, raw traces, perf samples, status counters and polling
records. A and B binary SHA-256 digests are
`eb18d681c1de935d731ab2c0d3478d8c20b878515654ef68c17223c986da9df9` and
`7681c994790bcace302d1455c0c79992bfa6f065a84914979806135d4de8fcce`.

The retained pilots `6d88e7ba271667541bee09673b098c11` and
`9109806d45f7d8e0dddfe13b375909a7` recorded two refreshes in fixed-length trace
windows. Pilot `9a19b06f2c9c9af17a80afa22939c4a0` captured two complete
worker traces and part of a third, then failed a controller variable check.
The CLI health probes exited early in `3d6c16cf3d271daa9aff7a9b1039a9db`
and `064e5b102c05d07f59728df7503b1f7c`. The accepted series uses the guarded
read-only daemon health frame and closes the trace on the third completion.

The candidate passed `cargo fmt --all --check`, `cargo clippy --locked
--all-targets -- -D warnings`, `cargo test --locked` (490 library tests
and one integration test passed, six ignored), `cargo build --release
--locked` and `target/release/herdr-lineup --help`.
