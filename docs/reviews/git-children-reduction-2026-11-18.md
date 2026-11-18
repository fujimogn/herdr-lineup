# Git child process reduction, 2026-11-18

The experiment at `e32242af02a6ad1ef33bf44586bdda18e1d86701` reduced Git
process creation and user-space instructions on the loaded Linux fixture.
The combined CPU median improvement was smaller than the new A/A spread.
This change is retained as a measurement record. The experimental commit
remains available for inspection.

## Experimental heading contract

An eligible cache hit in the experiment ran two Git children. The workspace's
native `for-each-ref` performs Git's repository setup and reads the current ref and
commit object. The daemon checks the setup records from that same child with
`GIT_TRACE_SETUP`. It requires the normal main-worktree layout, the cached
working root as both Git's worktree and current directory, and `.git` as both
Git's private and common directory. A second native `rev-parse`, run from the
common directory, verifies its canonical identity and files backend. This
preserves the separate ownership and configuration rules for the two paths.
[Git's builtin setup](https://github.com/git/git/blob/v2.56.1/git.c),
[repository discovery](https://github.com/git/git/blob/v2.56.1/setup.c),
[setup trace](https://github.com/git/git/blob/v2.56.1/trace.c)

The cache entry must be younger than 30 seconds at query start. The cwd must
match the cached canonical root with a real `.git` directory. The compound
fingerprint covers the common directory, config, worktree registry, loose
`origin/main`, packed refs, alternates, and the replacement and reftable
directories. File fingerprints include device, inode, size, mode, modification
time and change time. The fingerprint is checked before and after Git runs, and
the cached root's fingerprint must match after the queries. Git still checks
HEAD during discovery and reads the current ref and object on each observation.
The cache retains the main root and name and keeps its original verification
instant.

Cold or expired entries, changed dependencies, other workspace layouts and
unknown trace output use the existing three-command path. The ref read still
runs with the workspace cwd, including for linked worktrees and subdirectories.
An unsuccessful combined attempt can run extra probes before the ordinary
path determines the result. A change during the verified read
discards the result. The shared deadline, no-fetch and process-group cleanup
rules remain in effect. Cache hits retain the original verification age used by
the publisher's heading fallback.

## Measurement protocol

The base is `92e3e63d6860b9efffc599c7d5cc08c915c06ce6` on
`origin/develop`. The locally rebuilt release binary for A matched the
[baseline record](performance-cpu-memory-baseline-2026-11-18.md) at
`9270efb3f25debb6ec5d7ef5ff879ab7a7b70de1d3f582b8f8581a2df43f7326`.
The B release binary was
`67ae8fbc2f4df6877f748ac4ddd949e76bf0a3153f70ec6452ba78d42627eb6a`.
Both used the pinned Rust 1.100.0 toolchain and the same `Cargo.lock`
SHA-256, `1c45b9615e8fa56316555bd4cbb399844838360dad92d453da8ef3d1ae8d82ea`.

Each window started a fresh private Herdr 0.9.5 server and a foreground
Lineup daemon with the baseline's four distinct local Git repositories,
four workspaces, 16 synthetic working Codex panes and empty provider homes.
The single empty commit in each repository used a fixed committer time
of 2024-01-01 00:00 UTC and was assigned to local `origin/main`. All four
repositories used commit `0010297db1ac2ad7780647789a6eadde2d6ae4da`. The controller,
server and Lineup used CPUs 20, 21 and 22 respectively. No renderer, client,
real agent or network quota worker was started. The controller checked its
private Unix socket, config and state roots and the foreground daemon PID
before attaching. Each case warmed for 23 seconds, then ran a 120-second
window. A/A preceded the interleaved AB/BA series in that order.

`perf stat` attached to Lineup with inherited `instructions:u`, `task-clock`
and `syscalls:sys_enter_execve` counters. The last measures execve entries in
the daemon tree. The fixture's children are Git. An execve-only strace
preflight confirmed the executable and subcommands. That preflight reached
the Git deadline under host pressure and observed missing headings after
tracing, so syscall counting in the comparison used the perf tracepoint.
Daemon and waited-child CPU come from the process's separate `/proc/PID/stat`
counter deltas at 100 ticks per second. Instructions include inherited
child work. The raw artifacts include the start and end loads, doctor
counters, tracepoint counts and CPU samples.

Heading equivalence compares complete, ordered snapshots of the private
Herdr workspace and first-pane heading tokens after warmup and after each
perf window. Generated workspace and pane IDs are replaced by fixture order.
Every snapshot is compared with the expected four repository names and
timestamps, four first-pane headings and 12 empty later-pane headings.

## Results

| Series | Case | Refreshes | Git execve | Waited-child CPU (ms) | Daemon CPU (ms) | Instructions | Inherited task-clock (ms) | Host 1-min load, start to end |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| A/A | A1 | 11 | 132 | 320 | 390 | 166,717,771 | 730.36 | 92.25 → 105.95 |
| A/A | A2 | 11 | 132 | 350 | 390 | 166,700,880 | 753.87 | 95.12 → 70.04 |
| A/B | A | 11 | 132 | 360 | 860 | 166,788,516 | 1,252.98 | 59.72 → 121.25 |
| A/B | B | 11 | 104 | 240 | 1,020 | 151,568,464 | 1,270.42 | 97.96 → 89.57 |
| B/A | B | 11 | 104 | 250 | 980 | 151,742,033 | 1,270.33 | 91.73 → 65.73 |
| B/A | A | 12 | 144 | 280 | 850 | 182,065,313 | 1,119.76 | 72.27 → 31.69 |

A/A produced 12 Git execve calls per refresh in both windows. The waited-child
CPU range was 29.09–31.82 ms per refresh, a spread of 2.73 ms (9.0% of the
midpoint). In the AB/BA series, totals divided by completed refreshes give
12.00 → 9.45 execve calls (−21.2%) and 27.83 → 22.27 ms waited-child CPU
(−20.0%). The pooled 5.55 ms decrease exceeds the A/A spread. The first pair
decreased by 10.91 ms per refresh and the reverse pair decreased by 0.61 ms
per refresh. The reverse pair's decrease falls inside the A/A spread.
Instructions fell from 15.17 million to 13.79 million per refresh (−9.1%).

Daemon-only CPU rose in both paired windows, from 74.35 to 90.91 ms per
refresh across the series. Inherited task-clock also rose, from 103.16 to
115.49 ms per refresh. The same A binary used 390 ms of daemon CPU per A/A
window and 850–860 ms in the AB/BA windows, under varying host load. These
windows establish the reduction in Git execve calls and pooled waited-child
CPU. The daemon CPU difference has an unresolved cause, and total CPU
improvement remains unverified.

All 12 normalized snapshots were byte-identical at 882 bytes with SHA-256
`4f24aee2422cd2ca1ecf7df75e4a1ab5df2471b938d58175f06a74210b311d4f`.
The first A/A window recorded 200 reports. The other windows recorded
20 reports per refresh. Heading equivalence is established at the recorded
snapshot boundaries. All six private daemon and server pairs exited 0.

## Follow-up gate for combined CPU

The same release binaries, CPU affinity, fixed ref inputs and 120-second
windows were used for a new A/A comparison and four additional pairs in
AB, BA, AB, BA order. The decision metrics were the sum of daemon and
waited-child CPU and the inherited `instructions:u` count. Each window
was normalized by completed refreshes. The median of signed B-minus-A
pair deltas was compared with the absolute A/A spread for that metric.

| Case | Variant | Refreshes | Git execve | Daemon CPU (ms) | Waited-child CPU (ms) | Combined CPU (ms) | Instructions | CPU 22 idle (%) | Load, start to end |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | A | 11 | 132 | 470 | 380 | 850 | 166,773,866 | 16.6 | 41.94 → 71.89 |
| 2 | A | 12 | 144 | 460 | 380 | 840 | 176,467,086 | 14.7 | 81.09 → 62.08 |
| 3 | A | 12 | 144 | 370 | 320 | 690 | 181,942,897 | 21.1 | 61.54 → 49.44 |
| 4 | B | 11 | 104 | 390 | 270 | 660 | 151,533,335 | 0.2 | 63.55 → 94.44 |
| 5 | B | 12 | 112 | 780 | 250 | 1030 | 164,724,750 | 27.3 | 85.63 → 52.30 |
| 6 | A | 12 | 144 | 1210 | 360 | 1570 | 181,935,783 | 5.1 | 53.82 → 86.86 |
| 7 | A | 11 | 132 | 1300 | 330 | 1630 | 166,904,240 | 9.0 | 84.10 → 63.13 |
| 8 | B | 11 | 112 | 1180 | 300 | 1480 | 162,387,337 | 2.4 | 64.07 → 86.28 |
| 9 | B | 11 | 104 | 1130 | 250 | 1380 | 151,523,794 | 15.1 | 89.40 → 65.57 |
| 10 | A | 12 | 144 | 840 | 280 | 1120 | 181,753,207 | 32.8 | 59.84 → 35.07 |

| Pair | Order | Combined CPU delta (ms per refresh) | Instruction delta per refresh |
| --- | --- | ---: | ---: |
| 1 | AB | +2.50 | -1,386,150 |
| 2 | BA | -45.00 | -1,434,253 |
| 3 | AB | -13.64 | -410,628 |
| 4 | BA | +32.12 | -1,371,210 |
| Median | | -5.57 | -1,378,680 |
| New A/A spread | | 7.27 | 455,670 |

The combined CPU reduction was 5.57 ms per refresh against a 7.27 ms
A/A spread. The median instruction decrease of 1,378,680 per refresh
exceeded its 455,670-instruction spread. The gate required both metrics.
As a sensitivity check, pooling the two earlier pairs with these four
gives a combined CPU median delta of +3.07 ms and an instruction median
delta of −1,380,510 per refresh. All 20 normalized follow-up snapshots
matched the original 882-byte heading payload.

The first run completed cases 1–8. After case 8 finished its 120-second
window and final heading check, the private Lineup `stop` command returned
exit 1. The controller sent SIGTERM only to its owned foreground child
and reaped it with status −15. Its private Herdr server exited 0. The
harness propagated the control command error even after cleanup, so the
last BA pair never started. The exception output retained the command
and exit code. The CLI's captured stderr was unavailable, and the kernel
journal query for that period returned no OOM entries. Memory pressure
as the cause of this failure could not be established.

The repaired harness records the private control error and continues
when it confirms that both owned processes have exited through an
accepted cleanup path. A simulated stop exit 1 confirmed the private
fallback and retained stderr. Cases 9–10 reran only the missing BA pair.
The two binaries and timed body were unchanged, and both replacement
fixtures stopped normally. The new harness additionally records host
memory and pressure at the window boundaries. During the replacement BA
pair, the available memory samples ranged from 26.8 to 28.2 million kB.
The memory PSI `some` total increased by 1.56 s during B and 0.31 s
during A. CPU 22 idle share also differed. The experiment provides
fixed workload and placement with observed shared-host variation.

In the replacement BA pair, B's child CPU fell by 0.61 ms per refresh
while its daemon CPU rose by 32.73 ms per refresh. The daemon's system
time accounted for 27.12 ms of the increase and user time for 5.61 ms.
The corresponding user-space instruction count fell by 1,371,210 per
refresh. The combined CPU increase in this pair came from the daemon,
mostly from system time.

| Follow-up evidence | Managed run ID |
| --- | --- |
| Original follow-up with cases 1–8 retained | `9e9ac20e148cfd29e65d8b50ba117b9d` |
| Remaining BA pair with repaired cleanup | `5194d82a661ae6a853ef6a93353f6cdb` |

The repaired series harness SHA-256 is
`10bd8d4b32f8ac92e9b4be5a03650f7c8269eb4c8a77f1df37073f624ee013a2`.

## Daemon CPU attribution

A separate AB/BA diagnostic used the same fixture and binaries with
50-second `perf record` windows. It counted selected syscall entries
and grouped them by process ID so daemon threads and Git children can
be compared separately. Counts and CPU deltas from these instrumented
windows are diagnostic. Each completed five refreshes.

| Case | Daemon system CPU (ms) | Daemon read | Daemon poll | Daemon waitid | Daemon wait4 | Child execve | Child write |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 390 | 164 | 730 | 70 | 48 | 48 | 48 |
| B | 300 | 201 | 798 | 65 | 48 | 48 | 108 |
| B | 340 | 196 | 798 | 68 | 48 | 48 | 108 |
| A | 390 | 198 | 780 | 85 | 60 | 60 | 60 |

The B windows recorded more child writes and daemon polls. Extra Git
writes are consistent with the setup trace sent through the output pipe.
Daemon reads varied between the A cases. The diagnostic B cases both
spent less daemon system CPU than either A case, so the observed syscall
changes do not attribute the increase in the replacement BA pair to a
specific syscall or Git workload change. The timing comparisons isolate
that pair's excess to daemon system time and show that it did not repeat
under the diagnostic conditions.

Managed run `687670a0f59dbb9145973d2002821672` reached `complete`.
All four private fixture pairs exited 0 and the eight recorded heading
snapshots matched. The retained diagnostic harness SHA-256 is
`78eb442d85a1f2c966b4405ded3750a2d90cc75ee554a7de283982fc3db3b57f`.

## Evidence and validation

The retained `bench.py` and raw results are under
`$HOME/.cache/dev-task-temp/runs/<run>/work`. Both comparison runs reached
`complete` and retained the same harness SHA-256,
`3ea52b6cc4f2e89fac802b4d22dc1536a0f592a6066da113a47271a41a6175e1`.
Running this harness through `dev-task-temp` uses these positional arguments.

```text
bench.py <worktree> <agent-room checkout> <Herdr binary> <Bun binary> <AA|ABBA>
```

The harness expects A at `target/herdr-lineup-baseline` and B at
`target/release/herdr-lineup`. It derives the fixture from the baseline
harness and leaves its input identities, snapshots, CSV, CPU samples and
cleanup results next to the copied script.

| Run | Managed run ID |
| --- | --- |
| A/A | `7653fbd54860a954699893d906eff0e0` |
| AB/BA | `068bc84cb052dbf68cdba4fd7e36a400` |
| Execve trace preflight under host pressure | `53f1a2a7ef1609ecfec8e4257922ed15` |

The experimental tests count Git commands on warm hits, expiry, ref updates and
replacements, worktree topology changes, linked worktrees and permission
changes. They also verify the native workspace and common-directory
ownership checks and reject unexpected setup records. Removing the
fingerprint comparison caused the ref update, permission change and
worktree topology tests to fail. Removing the age check caused the expiry
test to fail. All existing Git heading tests passed with their original assertions.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked` | 476 unit tests and 1 integration test passed, 6 ignored, 0 failed |
| `cargo build --release --locked` | Passed |
| `target/release/herdr-lineup --help` | Passed |

The record-only branch differs from its base by this document. The same
five commands passed there. The branch's test run passed 470 unit tests
and one integration test, with six ignored. The rebuilt release SHA-256
matched the A binary.
