# Linux resident daemon idle wakeups, 2026-11-18

Retained the candidate for review.

| Idle metric | AB reduction | BA reduction | Median reduction | A/A span |
| --- | ---: | ---: | ---: | ---: |
| Syscalls per 35 s | 1,360 | 1,263 | 1,311.5 | 7 |
| Userspace instructions per 120 s | 1,701,941 | 1,806,100 | 1,754,020.5 | 10,932 |
| Daemon CPU ms per 120 s | 500 | 1,180 | 840 | 570 |

## Revisions and fixture

The comparison follows candidate 2 of the
[CPU and memory baseline](performance-cpu-memory-baseline-2026-11-18.md).
A was the unchanged `origin/develop` at
`4c09399bde9fba4ca9657dd2658869b540a1df9f`. B was A plus the retained
`candidate.patch` for `src/daemon.rs`. Both used the same Rust 1.100.0
toolchain, lockfile and release build command.

| Input | SHA-256 |
| --- | --- |
| `Cargo.lock` | `1c45b9615e8fa56316555bd4cbb399844838360dad92d453da8ef3d1ae8d82ea` |
| A release executable | `9270efb3f25debb6ec5d7ef5ff879ab7a7b70de1d3f582b8f8581a2df43f7326` |
| B release executable | `995ff0fc38b7047ba949a265c33a4a4a47031cc4d157a1722a0474d0763320cb` |
| B patch | `bbc69e22e5a91ef6fc1e9c5f91fa65a4f2ae397a582361c84159761fd3952534` |
| Herdr 0.9.5 | `32f81aa0797c6361d074263d22db478762ec9950c0315e32f9b4309359affc66` |

B uses one Unix socket pair for control worker shutdown and foreground
notification. A valid shutdown sets the cancellation flag before the worker
closes its side. Foreground teardown signals the other direction before
joining the worker. On Linux, supported local directory paths can wake the
foreground on an inotify event. That event triggers a fresh endpoint identity
check and does not trigger a refresh. Paths without reliable watcher setup,
including symlink paths, use the existing 100 ms check interval. macOS uses
that path as well. The 250 ms reconnect retry, three-interval missing-socket
TTL, and transport identity checks at I/O boundaries remain in the paths
exercised by the tests. The checks here ran on Linux.

Each trial started a fresh daemon and private headless Herdr 0.9.5 server
without an attached client. The idle snapshot was empty. The loaded snapshot
contained the baseline's four Git repositories with an empty `main` commit and
a matching `refs/remotes/origin/main`, four workspaces, and 16 working Codex
panes. Provider homes and plugin settings were empty. Git author and committer
dates were fixed at 2026-11-18 00:00 UTC to make heading text reproducible.
All four fixture repositories used commit `fee70c47704e14ae0faa0ab05ffdf2d963fa5faa`. The retained
launcher checks the canonical socket path, private state and config directories,
daemon PID file, and ownership of both child processes.

The baseline used an ext4 fixture under a short `/proc` descriptor alias.
This comparison used identical short, canonical fixture paths on local tmpfs
for A and B to exercise the Linux watcher. The binary hash of A matches the
baseline's binary. Use the local A/A controls for comparisons to B. The
earlier ext4 results refer to a different measurement setup.

## Measurement

The same shared Linux host used kernel `7.3.3-1-cachyos`, perf 7.3.1-1,
strace 7.3, 4 KiB pages and 100 CPU ticks per second. The controller and
tracers ran on CPU 20, Herdr on CPU 21 and Lineup and its descendants on
CPU 22. Affinity did not reserve these CPUs. The launcher first captured
A1 and A2, then ran A3/B1 and B2/A4. Case order was idle then loaded in
A1, A3 and B1, and loaded then idle in A2, B2 and A4.

Each daemon warmed for 23 s. The perf window was nominally 120 s, followed
by a separate 35 s `strace -f -c` window, with doctor counters read at each
boundary. Perf counted inherited `instructions:u` and task-clock. Daemon
CPU is the delta of `/proc/<pid>/stat` user and system ticks, including
resident threads. Waited child ticks are shown separately. RSS is the
median of the resident process's periodic `smaps_rollup` samples.
CPU busy is derived from the CPU 22 line in `/proc/stat`, excluding idle
and I/O wait. Both windows include work that crosses a boundary. Syscall
counts include followed threads and children.

A/A spread is the maximum minus minimum among the two same-version
measurements for each metric. The retention gate requires reductions in both
AB/BA pairs for idle CPU, instructions and syscalls, and a median paired
reduction larger than the corresponding A/A spread. Refresh outcomes and
normalized sidebar tokens must agree. RSS and loaded measurements describe
costs under load.

### Resident measurements

| Trial | Case | Instructions:u | Task-clock ms | Daemon CPU ms | Waited-child CPU ms | RSS KiB | CPU 22 busy % | Actual s |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A1 | idle | 2,497,488 | 836.23 | 830 | 0 | 2,784 | 99.9 | 120.362 |
| A1 | loaded | 179,959,866 | 1643.98 | 1310 | 330 | 4,206 | 92.4 | 120.230 |
| A2 | idle | 2,508,420 | 278.16 | 260 | 0 | 4,592 | 91.3 | 120.167 |
| A2 | loaded | 179,738,070 | 1534.35 | 1110 | 380 | 5,304 | 94.7 | 120.160 |
| A3 | idle | 2,513,081 | 622.58 | 630 | 0 | 4,740 | 66.1 | 120.150 |
| A3 | loaded | 179,973,661 | 1173.91 | 840 | 310 | 5,370 | 94.1 | 120.165 |
| B1 | idle | 811,140 | 139.88 | 130 | 0 | 4,672 | 67.1 | 120.150 |
| B1 | loaded | 178,248,481 | 606.17 | 270 | 340 | 4,316 | 63.4 | 120.163 |
| B2 | idle | 698,016 | 94.62 | 90 | 0 | 4,372 | 80.7 | 120.158 |
| B2 | loaded | 178,266,559 | 548.32 | 270 | 300 | 5,228 | 74.1 | 120.166 |
| A4 | idle | 2,504,116 | 1278.94 | 1270 | 0 | 4,436 | 66.9 | 120.155 |
| A4 | loaded | 179,928,897 | 1488.39 | 1160 | 320 | 5,224 | 77.5 | 120.167 |

### Traced syscalls

| Trial | Case | Total | poll | statx | geteuid | clock_nanosleep | connect | execve | openat | newfstatat | Actual s |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A1 | idle | 1,394 | 346 | 345 | 345 | 326 | 3 | 0 | 0 | 0 | 35.013 |
| A1 | loaded | 12,354 | 576 | 619 | 539 | 250 | 42 | 24 | 3,094 | 3,139 | 35.024 |
| A2 | idle | 1,401 | 347 | 347 | 347 | 328 | 3 | 0 | 0 | 0 | 35.016 |
| A2 | loaded | 12,911 | 480 | 680 | 568 | 275 | 42 | 24 | 3,246 | 3,332 | 35.036 |
| A3 | idle | 1,441 | 351 | 359 | 359 | 340 | 3 | 0 | 0 | 0 | 35.009 |
| A3 | loaded | 12,512 | 527 | 662 | 582 | 288 | 43 | 24 | 3,128 | 3,176 | 35.009 |
| B1 | idle | 81 | 8 | 21 | 21 | 0 | 3 | 0 | 0 | 0 | 35.012 |
| B1 | loaded | 12,466 | 186 | 432 | 320 | 0 | 43 | 24 | 3,246 | 3,332 | 35.081 |
| B2 | idle | 115 | 9 | 22 | 22 | 0 | 3 | 0 | 0 | 0 | 35.014 |
| B2 | loaded | 17,779 | 223 | 573 | 405 | 0 | 55 | 36 | 4,863 | 4,992 | 35.013 |
| A4 | idle | 1,378 | 345 | 340 | 340 | 321 | 3 | 0 | 0 | 0 | 35.013 |
| A4 | loaded | 12,782 | 520 | 653 | 558 | 266 | 42 | 24 | 3,230 | 3,302 | 35.009 |

### Refreshes and control stop

| Trial | Case | Perf attempts / reports / checkpoints | Trace attempts / reports / checkpoints | Stop ms |
| --- | --- | --- | --- | ---: |
| A1 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 73.7 |
| A1 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 134.1 |
| A2 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 45.3 |
| A2 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 104.1 |
| A3 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 86.1 |
| A3 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 65.6 |
| B1 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 26.2 |
| B1 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 119.0 |
| B2 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 23.6 |
| B2 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 48.0 |
| A4 | idle | 12 / 0 / 0 | 3 / 0 / 0 | 87.1 |
| A4 | loaded | 12 / 240 / 8 | 2 / 40 / 1 | 67.4 |

The idle CPU reduction in the AB pair was 500 ms, inside the 570 ms A/A
spread. The BA pair drove the larger median. A4 reached 1,270 ms, above the
original A/A range of 260–830 ms. All four A CPU measurements span 1,010 ms.
Both B values were below every A value. These windows establish the
direction on this fixture with less certainty about the absolute CPU saving
under a different load.

The loaded trace totals varied across windows. B2 recorded 36
`execve` calls and two completed refreshes; the other loaded traces
recorded 24 `execve` calls and two completed refreshes. A traced call may
belong to an unfinished or already-running refresh. Every loaded perf
window completed 12 refreshes and acknowledged 240 reports. RSS varied
within both revisions. Raw boundary samples and PSS are in the evidence
archive.

## Behavior and checks

All completed refreshes were successful. The attempts, acknowledged
reports and checkpoint counts are shown separately for each window.
The normalized token snapshots were byte-identical within each fixture
case across all six trials. The stop time includes the CLI, token clear
and process exit. Every private daemon cleared its owned tokens on stop.

In a separate loaded trial for each binary, the controller paused only
its own Lineup process after the 23 s warmup and sampled tokens every
250 ms. All 20 resources carried tokens at the start. The last sample
with all 20 and the first sample with zero bound the observed expiry.

| Revision | Last full (s) | First empty (s) |
| --- | ---: | ---: |
| A | 27.186 | 27.457 |
| B | 27.048 | 27.315 |

After sampling, the controller resumed the owned process and ended both
private processes normally. This observation is relative to the pause
after warmup. The configured token TTL is 30 s from its last renewal.

B passed the daemon tests for shutdown before a distant refresh,
unrelated directory activity and cadence, socket unlink and ancestor
rename wakeups, socket replacement without connecting to the new
endpoint, and missing-socket TTL. Removing the foreground-to-worker
shutdown wake caused the idle worker test to time out. Removing the
foreground endpoint check caused the replacement test to fail. Each
mutation was tested separately and the retained source patch contains
neither mutation.

| Check | B prototype | Final candidate |
| --- | ---: | ---: |
| `cargo fmt --all --check` | 0 | 0 |
| `cargo clippy --locked --all-targets -- -D warnings` | 0 | 0 |
| `cargo test --locked` | 0 | 0 |
| `cargo build --release --locked` | 0 | 0 |
| `target/release/herdr-lineup --help` | 0 | 0 |

Test output counted 474 passing library tests, one passing integration
test and six ignored library tests for B.
The final release binary matched the measured SHA-256 for its source.

## Evidence

The evidence archive contains the launchers, raw counters and traces, all
fixed-window and smoke trials, token snapshots, cleanup results, the source
patch, binary identities, and check logs. The smoke runs established response
shape and the required observation window. The shortest B idle smoke windows
had zero traced syscalls or zero scheduled task time. Smoke-only checks were
adjusted to cover a refresh, while fixed-window durations, workload, CPU
affinity and metric collection stayed the same.

Managed run `760c51eb59538470dce27d61b0c2026b` contains the archive under
`work/evidence`.
The inventory covers 442 files, with SHA-256
`261bdce7ba59682bd2e228fc9aba6ea7697d77383e48b299f965982f1b4e9e21`.

Normalized token snapshot SHA-256 values

- Idle `0ac87c41338d631b5d777a9820b5a21914273c4093be84e936f2d6801f7460ee`
- Loaded `927306bdda6cc2f1ea191bd90b426c5e3aab7c99c70314950ac0617a90add158`

These results describe a private synthetic Linux session on one shared host.
They leave macOS measured performance and live-session effects open.
