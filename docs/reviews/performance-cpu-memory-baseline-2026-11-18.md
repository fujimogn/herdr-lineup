# Linux CPU and memory baseline, 2026-11-18

The Lineup release build and agent-room relay are both unchanged. This is a
baseline on one shared Linux host. It covers an empty and a populated
private Herdr session and one synthetic Claude statusLine command per sample.
The private sessions had no attached client, renderer, real agent process,
credentials, or network quota workers.

The largest measured opportunities are the short-lived Bun relay and Git
child work in the loaded refresh. The empty daemon generated roughly
40 syscalls per second while using about 0.18% of one CPU. Tune polling
against that absolute CPU budget and against the freshness and shutdown
behavior it provides.

## Revisions, host and isolation

| Input | Identity |
| --- | --- |
| herdr-lineup v0.3.6 | `f01598e9e34843210850ee1eea0ce642d94fc453` |
| `Cargo.lock` SHA-256 | `1c45b9615e8fa56316555bd4cbb399844838360dad92d453da8ef3d1ae8d82ea` |
| Lineup release executable SHA-256 | `9270efb3f25debb6ec5d7ef5ff879ab7a7b70de1d3f582b8f8581a2df43f7326` |
| agent-room checkout | `c35a3313fc82c9d4eaf291d95d0e308c5b58bb9a` |
| `scripts/claude-statusline-relay.ts` SHA-256 | `4a15d2026f7ee38f67889768ed7c50f4b56b76fe90cadd4f864c86a53294e52f` |
| Bun 1.4.4 SHA-256 | `10702245766b225e70bbf6235fd6b47214fd94b4760d00ffaed8ab0df57ac235` |
| Herdr 0.9.5 SHA-256 | `32f81aa0797c6361d074263d22db478762ec9950c0315e32f9b4309359affc66` |
| statusLine stdin SHA-256 | `c46fc31a7f1b65df42c38f561937171cd0c79f7eb8ca553d393de8795806477a` |

The dev host reported AMD Ryzen 9 9955HX, 24 online logical CPUs, x86-64,
Linux `7.3.3-1-cachyos`, 4,096-byte pages and 100 `CLK_TCK`. Measurement
used ext4 (`rw,relatime`), perf 7.3.1-1, strace 7.3 with libunwind,
Python 3.14.9, GNU Time 1.10 and Git 2.56.1. `perf_event_paranoid=1`,
`perf_event_max_sample_rate=1000` and Yama `ptrace_scope=1` were unchanged.
Counting does not require a sampled function profile. No Valgrind run was
needed. The host's other work and CPU frequency were uncontrolled. Affinity
restricted the work to a CPU without reserving it.

The launcher used an allowlisted environment around every fixture command:
`PATH=/home/linuxbrew/.linuxbrew/bin:/usr/bin:/bin`, `LC_ALL=C`, `TZ=UTC`,
`SHELL=/bin/sh`, `TERM=xterm-256color`, `GIT_CONFIG_NOSYSTEM=1`,
`GIT_CONFIG_GLOBAL=/dev/null`. Each case supplied a private `HOME`, the
four XDG roots, `TMPDIR`, and `CODEX_HOME`, `KIMI_CODE_HOME` and
`GROK_HOME` beneath that home. Daemon setup used `HERDR_CONFIG_PATH`
under that home, an empty plugin settings directory and a private state
directory. No inherited `HERDR_*` or provider environment was forwarded.
The Lineup and Herdr lifecycles were confined to the fixture. The controller
held one directory FD per fixture and used `/proc/<controller>/fd/<fd>`
as a short alias for the canonical fixture directory. It required the socket
to be a Unix socket that resolves to exactly
`<fixture>/h/c/herdr/sessions/lineup-bench/herdr.sock` and verified private
state/config roots before Lineup CLI operations and measurement attachments.
The controller also checked its foreground child's `daemon.pid` before
sampling. It enabled tracing of that child for its own descendants with
`PR_SET_PTRACER`.

### Reproduction

From the dedicated worktree at the recorded source SHA, using the pinned Rust
1.100.0 toolchain:

```sh
env CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -j 3

dev-task-temp run --root "$HOME/.cache/dev-task-temp" \
  --task-id herdr-lineup-perf-baseline-20261118 --bytes 100000000 --inodes 10000 -- \
  /usr/bin/python3 "$BENCH_SCRIPT" "$PWD" "$HOME/Develop/agent-room" \
  /home/linuxbrew/.linuxbrew/Cellar/herdr/0.9.5/bin/herdr \
  "$HOME/.local/share/mise/installs/bun/1.4.4/bin/bun"

dev-task-temp run --root "$HOME/.cache/dev-task-temp" \
  --task-id herdr-lineup-perf-baseline-rss-20261118 --bytes 100000000 --inodes 10000 -- \
  /usr/bin/python3 "$RSS_SCRIPT" "$PRIMARY_ARTIFACT_ROOT/statusline"
```

`BENCH_SCRIPT` and `RSS_SCRIPT` are the retained `bench.py` and `rss.py`
identified below. The commands that follow give their measurement and fixture
parameters. In the command blocks, variables refer to the paths in the
allowlisted environment, `LINEUP` is the release binary, `HERDR` and
`BUN` are the executables above, and `RELAY` is the TypeScript entry
point in the recorded agent-room checkout.

**Daemon.** For each case start a fresh headless `lineup-bench` server
from an empty directory, with the private config:

```toml
[terminal]
default_shell = "/bin/sh"
shell_mode = "non_login"
[update]
version_check = false
manifest_check = false
```

The idle case leaves the snapshot empty. For the loaded case, create four
different local repositories with one empty `main` commit each, and
`git update-ref refs/remotes/origin/main HEAD` in each. Create one
workspace per repository with `workspace create --cwd <repo> --label
fixture-<i> --no-focus`. Split each root pane three times using the last
created pane as the anchor, `pane split <anchor> --direction right --cwd
<repo> --no-focus`. Report all 16 panes with `pane report-agent <id>
--source lineup-bench --agent codex --state working --agent-session-id
fixture-<i>-<j>` (zero-based). Provider homes were empty. This is a
steady publication workload with live-token renewals and four independent
Git roots, with synthetic state fixed during measurement.

```sh
taskset -c 21 "$HERDR" --session lineup-bench server
taskset -c 22 "$LINEUP" daemon
taskset -c 20 "$LINEUP" doctor
/usr/bin/perf stat --no-big-num -x , -o "$PERF_CSV" \
  -e instructions:u,task-clock -p "$LINEUP_PID" --timeout 120000
/usr/bin/strace -f -c -qq -o "$STRACE_SUMMARY" -p "$LINEUP_PID"
/usr/bin/strace -f -qq -k --stack-trace-frame-limit=14 \
  -e trace=poll,clock_nanosleep,newfstatat,statx,connect,execve,fsync,openat \
  -s 80 -o "$STACK_LOG" -p "$LINEUP_PID"
taskset -c 20 "$LINEUP" stop
taskset -c 20 "$HERDR" --session lineup-bench session stop lineup-bench
```

The controller, perf and strace run on CPU 20. The foreground Lineup
daemon and its inherited threads/children run on CPU 22. Herdr runs on
CPU 21. The controller warmed each daemon for 23 s after checking the PID
file. An API snapshot via the private CLI verified all 16 loaded panes
published the `lineup_state=working` token. The controller ran one fixture
at a time, idle before loaded. Within each fixture it recorded three
sequential 120 s perf windows, three separate 35 s strace summaries and
one 13 s stack trace. It requested SIGINT on the tracer alone to detach
after the scheduled trace interval (trace return code `-2` and a complete
summary). It recorded doctor counters
before and after each window outside the timed samples. Doctor calls were
excluded from the target workload except for the negligible wakeup/boundary
offset. The CPU and RSS sampler ran at each perf boundary and about every
10 s during the window. Tracing can alter the refresh cadence and syscall
timing. The traced counts and perf ranges describe separate windows.

Instructions and task-clock attach to the foreground process with its
threads and default child inheritance. The `/proc/PID/stat` deltas for
`utime + stime` measure the daemon process (all its threads) at 10 ms
granularity. `cutime + cstime` is reported separately for waited-for
children. RSS, PSS and private pages come from `/proc/PID/smaps_rollup`.
Each window reports the median across its resident samples. A/A spread
is `(maximum - minimum) / median`; tables give the actual ranges as well.
Wall windows and wall times provide context and regression checks.

**statusLine.** The 113-byte stdin fixture is one JSON line followed by
a newline:

```json
{"session_id":"lineup-fixture","context_window":{"used_percentage":23.5},"model":{"id":"claude-sonnet-fixture"}}
```

The direct command is `"$LINEUP" statusline claude`. The relay command is
`"$BUN" --no-env-file "$RELAY" --command "exec $LINEUP statusline claude"`
(paths in this run had no spaces). The relay still executes the same
Lineup command through its child shell and additionally writes its own
observation. After one warmup per mode, both paths replaced the Lineup
record on every invocation. The controller checked the observation values
and fresh inode, plus the relay record for relay invocations. Both paths
returned 0 with empty stdout/stderr. Timestamp and inode changes are
expected for atomically replaced observations. There was no existing
custom display command in the fixture.

For each pass, the controller alternated direct/relay then relay/direct.
There were 10 invocations per mode for each of perf and strace, 15 per
mode for CPU and wall time via `wait4`, and one stack trace per mode. Both mode series use fixed binaries and serve as
baseline configurations. Each command got the same input, under warmed files and the same private
state. The statusLine CPU/wall pass launches each command on CPU 22 via
inherited affinity and obtains user and system time from `os.wait4`
on the child. Wall time starts immediately before setting affinity and
spawning the process, and ends immediately after reaping it. The
PMU and syscall passes pin the outer tool and command as follows.
`TARGET...` stands for the direct or relay command above.

```sh
taskset -c 22 /usr/bin/perf stat --no-big-num -x , -o "$PERF_CSV" \
  -e instructions:u,task-clock -- TARGET... < "$INPUT"
taskset -c 22 /usr/bin/strace -f -c -qq -o "$STRACE_SUMMARY" TARGET... < "$INPUT"
taskset -c 22 /usr/bin/strace -f -k -qq --stack-trace-frame-limit=10 \
  -e trace=execve,clone,clone3,mmap,mprotect,futex,fsync,openat,newfstatat,statx \
  -s 80 -o "$STACK_LOG" TARGET... < "$INPUT"
taskset -c 22 /usr/bin/time -f %M -o "$RSS_LOG" -- TARGET... < "$INPUT"
taskset -c 22 /usr/bin/strace -f -k -qq --stack-trace-frame-limit=10 \
  -e trace=write,rename,renameat,renameat2 -s 80 -o "$WRITE_STACK_LOG" \
  TARGET... < "$INPUT"
```

The same fixture was rerun for the peak RSS pass (15 paired invocations
and five `/usr/bin/true` controls) under a new private filesystem root.
This avoids the Python launcher setting a roughly 23 MiB pre-exec
high-water mark in the statusLine `wait4` samples. GNU time measures a
peak RSS in KiB for the waited command and reaped descendants under Linux
accounting. This is a maximum, rather than the sum of simultaneous RSS
across processes. It differs from resident daemon instantaneous samples.
The `true` controls expose the lightweight launcher's measurement floor.

## Results

### Resident daemon

| 120 s window | Instructions:u | Task-clock ms | Daemon CPU ms | Waited child CPU ms | RSS KiB | PSS KiB | Actual s | Attempts | Reports | Checkpoints | Deadline-skip attempts | CPU 22 busy % |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| idle 1 | 2,559,812 | 231.28 | 210 | 0 | 4,262.0 | 2,145.0 | 120.317 | 12 | 0 | 0 | 0 | 94.9 |
| idle 2 | 2,559,800 | 224.50 | 220 | 0 | 3,922.0 | 1,805.0 | 120.407 | 12 | 0 | 0 | 0 | 92.6 |
| idle 3 | 2,560,518 | 229.80 | 210 | 0 | 3,870.0 | 1,752.5 | 120.243 | 12 | 0 | 0 | 0 | 92.6 |
| loaded 1 | 167,159,445 | 739.80 | 360 | 350 | 5,322.0 | 3,077.0 | 120.711 | 11 | 212 | 7 | 1 | 92.6 |
| loaded 2 | 167,437,486 | 764.79 | 380 | 350 | 4,586.0 | 2,341.0 | 120.426 | 11 | 220 | 7 | 0 | 97.4 |
| loaded 3 | 182,500,766 | 801.40 | 400 | 390 | 4,598.0 | 2,353.0 | 120.429 | 12 | 240 | 8 | 0 | 92.6 |

Each RSS/PSS entry is the median of 14 samples in that window. CPU 22
busy is derived from the first eight per-CPU counters in `/proc/stat`,
excluding idle and I/O wait from busy time. The busy percentage includes
other host work. The counters are rounded, and adjacent windows on one
daemon are repeated observations of that process.

| A/A metric, min / median / max (spread) | Idle | Loaded |
| --- | ---: | ---: |
| Instructions:u | 2,559,800 / 2,559,812 / 2,560,518 (0.03%) | 167,159,445 / 167,437,486 / 182,500,766 (9.16%) |
| Task-clock ms | 224.50 / 229.80 / 231.28 (2.95%) | 739.80 / 764.79 / 801.40 (8.05%) |
| Daemon CPU ms | 210 / 210 / 220 (4.76%) | 360 / 380 / 400 (10.53%) |
| Waited child CPU ms | 0 / 0 / 0 (n/a) | 350 / 350 / 390 (11.43%) |
| RSS KiB | 3,870.0 / 3,922.0 / 4,262.0 (9.99%) | 4,586.0 / 4,598.0 / 5,322.0 (16.01%) |
| PSS KiB | 1,752.5 / 1,805.0 / 2,145.0 (21.75%) | 2,341.0 / 2,353.0 / 3,077.0 (31.28%) |

Each 35 s trace window is separate from the perf and RSS pass:

| 35 s trace | All calls | Errors | Refreshes | Reports | Connect | Execve |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| idle 1 | 1384 | 0 | 3 | 0 | 3 | 0 |
| idle 2 | 1418 | 0 | 4 | 0 | 4 | 0 |
| idle 3 | 1394 | 0 | 3 | 0 | 3 | 0 |
| loaded 1 | 17029 | 7642 | 2 | 40 | 47 | 35 |
| loaded 2 | 13906 | 5701 | 3 | 60 | 46 | 24 |
| loaded 3 | 19439 | 8538 | 3 | 60 | 63 | 36 |

A/A total calls are 1,384 / 1,394 / 1,418 (2.44%) for idle and
13,906 / 17,029 / 19,439 (32.49%) for loaded.
Per-syscall errors include expected kernel-probe failures. Refresh outcomes
come from the separate doctor counters.

### Single statusLine invocation

| Per-call metric, min / median / max (spread) | Direct | With relay | n per mode |
| --- | ---: | ---: | ---: |
| Instructions:u | 470,446 / 471,106 / 471,798 (0.29%) | 95,736,225 / 96,510,524 / 96,707,172 (1.01%) | 10 |
| Task-clock ms | 1.57 / 2.17 / 2.61 (47.93%) | 48.93 / 60.87 / 69.52 (33.83%) | 10 |
| CPU ms | 1.250 / 1.585 / 2.644 (87.95%) | 47.086 / 55.397 / 70.383 (42.05%) | 15 |
| User ms | 0.000 / 0.796 / 2.644 (332.16%) | 36.409 / 46.145 / 57.486 (45.68%) | 15 |
| System ms | 0.000 / 0.776 / 2.572 (331.44%) | 7.057 / 10.209 / 13.430 (62.43%) | 15 |
| Wall ms | 2.421 / 5.756 / 16.419 (243.21%) | 86.128 / 116.407 / 150.262 (55.09%) | 15 |
| Syscalls | 122 / 122 / 122 (0.00%) | 1,177 / 1,199 / 1,283 (8.84%) | 10 |
| Syscall errors | 2 / 2 / 2 (0.00%) | 51 / 56 / 66 (27.03%) | 10 |
| Peak RSS KiB | 4,244 / 4,560 / 4,732 (10.70%) | 38,060 / 39,596 / 40,236 (5.50%) | 15 |

The peak RSS controls, `/usr/bin/true` (n=5), measured 1,648 / 1,652 / 1,656 (0.48%) KiB.
Task-clock and wait4 CPU time come from separate invocation sets.

### Cost attribution

- **StatusLine relay dominates its own invocation.** The median difference
  between relay and direct was 96.04 million instructions, 53.812 ms CPU,
  1,077 syscalls and 35,036 KiB peak RSS. The PMU and syscall differences
  are much larger than their within-mode spreads. The relay's largest
  syscall class by count was `futex` (median 275 per call). The stack
  trace shows Bun startup and worker synchronization, followed by the
  child shell, Lineup and a relay-store `fsync`. These measurements cover
  the real relay entry point with one synthetic record, and include the
  cost of its additional observation. They give no function-level CPU
  or allocation breakdown.
- **Git child work is a major loaded-daemon CPU cost.** The loaded
  foreground daemon and waited children used median 380 ms and 350 ms
  per nominal 120 s, respectively. That puts waited children at around
  48% of their summed tick deltas in the median window values. The
  corresponding empty windows measured 210 ms daemon CPU and zero waited
  child ticks. Loaded trace counts include 24–36 `execve` calls. The
  stack sample identifies Git launches and loader filesystem probes.
  `openat` and `newfstatat` together were 49–54% of all loaded calls
  across the three traces. Some failed probes were normal ENOENT paths
  in the Git dynamic loader. Git's installed build and the phase of
  each trace affect that distribution. The source verifies each
  distinct repository with three children even on a heading-cache hit.
  [Git query path](../../src/ship_time.rs#L153)
- **Idle wakeups dominate empty-daemon syscall counts.** Median counts
  in 35 s were 346 `poll`, 345 `statx`, 345 `geteuid` and 326
  `clock_nanosleep` calls. The stacks show the control thread polling
  with a 100 ms timeout and the foreground thread alternating a 100 ms
  sleep with a stat of the private socket. Endpoint identity also checks
  the effective UID. These classes account for almost all empty-case
  calls. Median resident CPU was 0.175% of one CPU over the nominal
  window, so even a large count reduction has a small absolute CPU
  budget here. [Daemon loop and control worker](../../src/daemon.rs#L1040),
  [Socket identity](../../src/socket.rs#L20)
- **Loaded publication adds round trips and serialized records.**
  Loaded traces observed 46–63 `connect` calls in 35 s, versus 3–4 in
  the empty snapshot case. The trace confirms AF_UNIX connections to
  the private Herdr socket. A complete four-workspace cycle can accept
  20 reports plus the snapshot exchange. The perf windows recorded
  212–240 reports and 7–8 successful checkpoint writes. Daemon CPU increased
  by 170 ms at the median relative to idle. That difference includes
  planning, Git-parent work, socket activity, and persistence. The
  isolated Herdr server process used 80 ms CPU at the empty median
  and 640 ms at the loaded median, on its own CPU in the same windows.
  The change in its own CPU is separate from Lineup CPU. These
  observations do not apportion CPU among the overlapping stages.
  [Publication pass](../../src/publish.rs#L2354)
- **Resident memory increased modestly in this fixture.** The median of
  Lineup's RSS window medians rose from 3,922 to 4,598 KiB, with PSS from
  1,805 to 2,353 KiB. Both modes showed within-run page residency changes.
  This measures the resident process and its threads while sampling, so
  transient Git child memory is outside these RSS/PSS numbers. The
  statusLine peak is a separate short-lived process measurement.
- **Direct statusLine serialization is a syscall target with limited
  absolute headroom.** All ten direct traces recorded 47 `write` calls
  out of 122 total. In the focused stack trace, 47 small writes went to
  the same file descriptor before `renameat` moved the temporary record.
  The source serializes JSON directly into that file and keeps the
  atomic rename. [StatusLine record writer](../../src/statusline.rs#L189)

The loaded windows ran 11, 11 and 12 refresh attempts, compared with 12 in
each idle window. Loaded window 1 recorded one deadline-skip attempt,
skipping admission for eight resource reports. It remains in the baseline. Dividing
loaded instructions by recorded attempts gives 15.196–15.222 million per
attempt across those windows, including between-refresh work in the
numerator. This explains much of the fixed-window instruction spread.
The traced counts include partial cycles at attach/detach and extra ptrace
overhead. Their `execve` and `connect` totals should not be divided by
doctor's completed-attempt counts as exact per-cycle costs.

The primary sampler ran from 06:16:55 to 06:33:59 UTC on 2026-11-18.
First and last `/proc` boundaries for the idle series were in
06:17:33–06:23:35 UTC and for the loaded series in 06:25:57–06:31:59 UTC.
The subsequent RSS pass ran from 06:34:38 to 06:34:42 UTC. Raw JSON has
the exact UTC and monotonic boundaries. Across perf-window boundaries
the host's one-minute load average ranged from 65.98 to 106.93. That
load includes other runnable or uninterruptible tasks on the shared host.

## Candidates to test

The [observer survey](performance-survey-observers-2026-11-09.md)
explains the freshness edges around event-driven reads. The
[refresh-path survey](performance-survey-refresh-path-2026-11-09.md)
describes safe Git identity checks, endpoint checks, sequence reservation
and metadata renewal. The
[latency investigation](latency-investigation-2026-11-04.md)
measured a synthetic event to a stored token. It did not time a rendered frame.
The earlier wall-stage shares cannot be used as CPU shares.

Compare unchanged A with the candidate B using the same locked toolchain,
warmup, fixed input and topology, CPU affinity and isolated filesystem.
Collect same-version A/A first, then interleave AB/BA. Record raw window
counts and qualify a claimed effect against the A/A range. For each daemon
trial, require equivalent normalized tokens and refresh outcomes and check
stored-token delay and TTL under both modes. Verify error paths before timing.

1. **Bypass the Bun relay where agent-room does not require its Claude observation, or use a lighter observer when it does.**

   Bypassing this wrapper removes about 96 million instructions, 54 ms CPU and
   34 MiB of peak RSS per invocation on this fixture. The relay persists a
   separate observation that consumers may depend on. A replacement must
   preserve optional observation failures, display forwarding, input bounds
   and prior-command exit/timeout behavior.

   Use identical non-empty and empty display fixtures and warmed record
   fixtures. Compare output bytes and exit status, then instructions, CPU,
   syscall classes and peak RSS. Confirm the consumer can still read the relay
   observation where required. Retain a separate cold-start trial.

2. **Wake the daemon's control worker on an explicit shutdown signal and drive the main loop from a timer or verified source events.**

   The four loop signatures total about 39 syscalls/s in the empty case. They
   are an upper bound on the count that could disappear from that fixture. The
   entire resident Lineup idle CPU budget was only about 0.21 s per 120 s.
   Losing a wakeup or delaying a socket-identity check could affect control
   shutdown, replace detection or the time before stale keys are cleared.

   Compare fixed-window idle and loaded CPU/syscalls, control stop latency and
   refresh cadence. Exercise endpoint removal/replacement, lost notifications,
   expiration and cancellation. Retain endpoint validation at each required
   I/O boundary and verify socket identity handling under failures.

3. **Reduce repeated Git children after equivalent invalidation is demonstrated.**

   The waited-child CPU budget was approximately 0.35 s per 120 s, about 48%
   of the median daemon-plus-child tick total. Removing only a subset of
   repeated queries would recover less. Native Git verifies repository
   ownership/identity and dependency topology today. Loose/packed refs, linked
   worktrees, configuration/includes and ref rewrites must retain their
   current behavior.

   Use zero, one and four distinct roots and a linked-worktree case. Compare
   Git `execve` counts, `cutime+cstime`, inherited instructions and foreground
   CPU with equivalent heading results. Exercise stale cache, replaced refs
   and directory permission changes.

4. **Coalesce source changes and retain periodic reconciliation and token renewal.**

   The current fixture cannot quantify reader savings. Reducing repeated reads
   could help populated homes, while waking a full refresh on every event
   could increase Git and socket work. Lost or coalesced events, atomic rename
   and platform-specific watchers can weaken freshness.

   Add fixed, populated synthetic provider trees and timed writes. Compare
   reads, worker attempts, CPU and RSS at equal update rates, including
   bursts, rename, watcher overflow and no-change periods on Linux and macOS.
   Measure source-to-stored-token separately from rendered time.

5. **Review host metadata exchange overhead before proposing protocol changes.**

   The loaded fixture reached roughly 20 report exchanges per complete refresh
   in addition to the snapshot. Reducing connections can cut socket
   operations. The CPU saved remains unmeasured. Batching or reusing a
   connection needs Herdr support and preserves acknowledgement, per-resource
   sequence ordering, fairness, deadlines, TTL and no replay after ambiguous
   sends.

   Compare the same 16-pane renewal fixture and a larger fixture. Track reports per
   cycle, connects, endpoint stats, instructions and CPU on both
   sides. Exercise EOF, endpoint replacement, partial acknowledgement and
   capacity rejection.

6. **Group the direct statusLine record writes.**

   One write in place of 47 would remove up to 46 measured calls, about 38% of
   direct-path syscalls if other calls remain equal. The entire direct path
   is small, with a median of 1.585 ms CPU. Grouping writes can introduce
   another allocation and must preserve atomic replacement and existing error
   outcomes.

   Compare write count and CPU against direct-path A/A. Verify equivalent
   normalized observation, file mode, bounded input, unchanged stdout/stderr,
   and failed-write behavior.

The loaded fixture exercises Git and socket publication alongside baseline
daemon wakeups. The empty provider fixtures and zero observed Codex reader
attempts leave populated provider costs open. This Linux run supplies no
macOS, authenticated provider, production topology, rendered-sidebar, or
production call-frequency measurement. Per-call statusLine savings should be
multiplied by an observed invocation rate before a rollout decision.

## Evidence and validation

The evidence and harness sources were retained in dedicated
`$HOME/.cache/dev-task-temp/runs/<run>/work` directories.
Results retain their original run order. The short smoke attempts were
used to establish setup, output parsing and safe lifecycle behavior. Two
preflights encountered the Herdr CLI's response shape, and a third caught
strace's SIGINT exit status. They ended the owned private services and are
excluded from the fixed measurement windows.

| Managed run | Run ID | Identified harness SHA-256 |
| --- | --- | --- |
| Primary baseline | `c3a602740e3467d6b13e7691edaa33b7` | `2eaa8f9f490312a7cbcb9925d1ae84039d26428f0830b71951e738884d2798a5` |
| Peak RSS and record-write stacks | `14ad71a93ac93860d2797766f09d6938` | `80bf9650a5774875678c9b53d88cda595b44410c55e0677ee3ad59c8f28a86e8` |

The primary and RSS raw-file inventory digests are
`07ab98c977b56cd8afeea5c5f426be8a49448b17f76095abccb8397ed051f26e`
(143 files) and
`3de9711534cffc6ad93df1c7e73489e114bb2a22c91c028e94a65008dfaed412`
(40 files), respectively. Both managed runs reached `complete` with no
reported child failure.

The primary run includes the raw perf CSVs, boundary samples, doctor counters,
trace summaries and stack logs, source identities, topology and cleanup
results. The RSS run includes the input bytes, `time` results and write stack
captures. The two full private daemon lifecycles returned 0 for both Lineup
and the Herdr server. No client was attached. Their measurement output and
private fixture paths remain available for audit.

The pinned-toolchain checks ran after measurement with inherited `HERDR_*`
removed and `CARGO_TARGET_DIR` set to the worktree's target. Cargo used three
build jobs.

| Command | Exit | Evidence |
| --- | ---: | --- |
| `cargo fmt --all --check` | 0 | Formatting check passed |
| `cargo clippy --locked --all-targets -- -D warnings` | 0 | All targets checked |
| `cargo test --locked` | 0 | 484 passed, 6 ignored, 0 failed |
| `cargo build --release --locked` | 0 | Release executable retained its measured SHA-256 |
| `target/release/herdr-lineup --help` | 0 | Help emitted |

The command-result manifest (`checks.json`) has SHA-256
`0c5cfc8e115903f5b9d0d3ac196ea4c759512cf6ee6235c33f5a7d5adb98bc94`.
