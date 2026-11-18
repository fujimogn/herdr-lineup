# Populated source reader baseline on Linux, 2026-11-18

The no-change runs show room to coalesce reader work by filesystem call
count. Kimi makes 2,212 attributed read/open/stat calls per refresh in
all three no-change traces, or 93.8% of the four readers' calls in those
classes. CPU and memory savings remain unmeasured.

This measures the existing periodic reader and publication path of Lineup
v0.3.7 at `3ab6b0912210608c71f55d4fd9c62106d2dd9e98`. The four workloads use
synthetic context records and one private, headless Herdr 0.9.5 server at
a time. [Candidate 4 in the Linux
baseline](performance-cpu-memory-baseline-2026-11-18.md#candidates-to-test)

## Fixture and timing

Each workload starts a fresh daemon with one workspace, one private Git repository, and
four working panes in Claude, Codex, Grok, Kimi creation order. The repository has a
single empty `main` commit dated 2026-11-18 00:00 UTC and a local
`refs/remotes/origin/main` pointing to it. The controller reports a native session ID
for each pane and waits for all four stored `lineup_context_pct` values to equal 10. For
Claude, Codex and Grok it uses a synthetic lifecycle source and separate
`herdr:<provider>` native-session reports. Kimi's startup identity requires a recognized
foreground process. A private copy of `/usr/bin/sleep` named `kimi` supplies that
process, then the controller reports Kimi's startup identity and working state. [Herdr
0.9.5 startup-session
handling](https://github.com/herdrdev/herdr/blob/v0.9.5/src/terminal/state.rs#L1462)

| Provider | Fixed tree and active source |
| --- | --- |
| Claude | 256 encoded-key JSON records in the plugin state's `statusline/` directory. The reader addresses one exact record. |
| Codex | 128 plain rollouts with 96 across 16 live calendar-day directories and 32 in `archived_sessions`. Each contains a bound session header, 128 synthetic event lines and a final token count. The active rollout is in the live tree. Token counts use the reader's 12,000-token baseline offset. |
| Grok | 192 `signals.json` records under 16 working-directory keys. Each is approximately 1 KiB, with an exact session ID and a usage number. |
| Kimi | 192 session trees under 16 working-directory keys. Each has a padded binding `state.json` and `agents/main/wire.jsonl` with a model bind, 64 synthetic lines and a completed main-agent turn. A private `config.toml` supplies one 100,000-token model window. The active binding uses the private checkout's exact cwd. |

The seeded case trees share one shape and one payload recipe. Each provider's normalized
fixture digest matched across all four cases. The absolute matched Kimi cwd and source
initialization timestamps are normalized in the fixture digest. The tree populations are
workload assumptions. The Codex and Kimi session counts lie within the earlier study's
candidate counts. That study used smaller synthetic payloads. [Real-disk reader
matrix](reader-discovery-real-disk-2026-11-04.md) Case files are warm from fixture
creation, first publication and a subsequent 23 s wait. The writer changes just the four
active sources, one per provider at each scheduled tick. Each update produces a distinct
integer percentage within an observation window. Ticks are scheduled against
`CLOCK_MONOTONIC`, starting 2 s after opening each window. The raw write logs record the
planned times and actual completion times.
The maximum scheduled-to-completed lag over all windows was 103 ms for
steady, 38 ms for burst and 610 ms for rename.

| Rate | Schedule per provider | Write operation |
| --- | --- | --- |
| none | 0 writes | Initialized sources remain unchanged. |
| steady | One write every 4 s | Claude and Grok overwrite their small records. Codex and Kimi append a JSONL observation. |
| burst | Eight writes at 125 ms spacing, starting every 20 s | The same in-place and append operations as steady. Each wave spans 875 ms. |
| rename | One write every 4 s | Write a sibling temporary file and replace the active name with `rename`. JSONL replacements contain the fixed prefix and one fresh observation. |

| Seeded files | Count | Apparent bytes | Bytes per file, min–max |
| --- | ---: | ---: | ---: |
| Claude | 256 | 37,376 | 146–146 |
| Codex | 128 | 12,843,008 | 100,336–100,336 |
| Grok | 192 | 192,384 | 1,002–1,002 |
| Kimi | 385 | 6,623,179 | 67–33,949 |

The table gives the no-change case before timed writes. Active Kimi binding bytes depend
on the length of the case's private checkout path.

The controller runs the no-change A/A first, then steady, burst and rename in that
order. Each case runs three nominal 60 s perf windows, three separate 35 s
provider-attribution traces, and one separate 35 s aggregate syscall trace. Each write
case additionally runs three 34 s write windows with token polling through 13 s of
drain. The append files accumulate records across windows within a case. A/A spread is
`(max - min) / median`. Each triplet contains three chronological observations of the
same executable and configuration. The baseline's 120 s perf windows were shortened to
60 s for this four-rate matrix. Fixed-time windows can straddle refresh boundaries.

The private settings retain the default 10 s refresh interval and set `reader_timeout_ms
= 10000` for every pass. A pilot with the default 2 s budget recorded a Kimi deadline
while attached to strace. The wider budget accommodates tracer and scheduling overhead
in this fixture. The release executable, provider parsers and refresh logic are those of
the measured commit. CPU affinity assigns the controller, writer, perf and strace to CPU
11, Lineup and its descendants to CPU 16, and Herdr to CPU 12. Affinity does not reserve
those CPUs.

## Isolation and reproduction

The shared dev host has an AMD Ryzen 9 9955HX, 24 online logical CPUs, Linux
`7.3.3-1-cachyos` x86-64 and 4,096-byte pages. The fixture lives on ext4
(`rw,relatime`) on `/dev/sda2`. Tools are Rust 1.100.0, Git 2.56.1, Python
3.14.9, perf 7.3.1-1 and strace 7.3. The observed perf paranoid and Yama
ptrace levels are both 1. The controller authorizes its own descendants to
trace its foreground Lineup child with `PR_SET_PTRACER`.

| Input | SHA-256 |
| --- | --- |
| Measured release executable | `eb18d681c1de935d731ab2c0d3478d8c20b878515654ef68c17223c986da9df9` |
| `Cargo.lock` | `6e77f9c5fbea1f4427eac715b6f9f892339e83c189e10fb9850e682c2dff5737` |
| Herdr 0.9.5 executable | `32f81aa0797c6361d074263d22db478762ec9950c0315e32f9b4309359affc66` |
| Synthetic Kimi foreground executable from `/usr/bin/sleep` | `623d3f2980edf12133ace55d083cea2f652ae02df28e0e91bc6f4dd03664d6fb` |
| `bench.py` | `1cd06bcf1639d46e2a913969b011152bbecb84929c3cb25faa9ff9a5d7279083` |

The child environment uses an allowlisted PATH, C locale, UTC and private HOME, XDG,
provider roots and plugin state/settings. Herdr uses a non-login `/bin/sh` and disables
version checks, manifest checks and agent restoration. Lineup uses its default network
opt-out. Each case uses a unique named Herdr session, a short
`/proc/<controller>/fd/<fd>` alias for Unix socket paths, and a canonical-path and
file-type guard before control, measurement attachments and cleanup. The controller
matches the foreground child's PID file and checks daemon CPU affinity. The run uses
synthetic records, provider-free child processes and a private checkout.

In a linked worktree at the measured source, build with the pinned Rust 1.100.0
toolchain and put the retained `bench.py` and `summarize.py` in `target/source-reads/`.
The retained controller selects the Herdr 0.9.5 binary and Linux perf/strace at the
recorded paths. Reproduction uses the following commands:

```sh
env CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked -j 3
dev-task-temp run --root "$HOME/.cache/dev-task-temp" \
  --task-id herdr-lineup-source-reads-20261118 --bytes 256000000 --inodes 25000 -- \
  /usr/bin/python3 "$PWD/target/source-reads/bench.py" nominal
/usr/bin/python3 "$PWD/target/source-reads/summarize.py" "$RUN_WORK/nominal"
```

Here `RUN_WORK` is the managed run's `work` directory. The controller runs these
measurement commands against the verified private foreground PID:

```sh
perf stat --no-big-num -x , -o "$PERF_CSV" \
  -e instructions:u,task-clock -p "$LINEUP_PID" --timeout 60000
strace -f -qq -o "$AGGREGATE" -c -p "$LINEUP_PID"
strace -f -qq -o "$DETAIL" -C -ttt -y -s 0 \
  -e trace=read,readv,pread64,preadv,preadv2,open,openat,openat2,stat,lstat,fstat,newfstatat,statx,getdents,getdents64 \
  -p "$LINEUP_PID"
```

- Daemon CPU uses boundary deltas of `utime + stime` in `/proc/PID/stat`. Waited
  children use `cutime + cstime` separately. The process clock has 100 ticks per second.
  `instructions:u` and task-clock are perf's attached target counters. RSS and PSS
  values are medians of `smaps_rollup` samples taken at the boundaries and about every 5
  s. The host is shared, with a one-minute load average of 51.22–117.82 at window
  boundaries. The busy percentages come from the daemon CPU's `/proc/stat` row,
  excluding idle and I/O wait.
- Doctor samples are outside the timed perf and strace windows. Refresh and per-provider
  worker deltas come from those samples. Worker completion describes the worker wrapper
  returning a result. Publication is checked separately from those counters.
- Path attribution includes completed syscall returns naming a provider directory or a
  decoded descriptor within it. It includes failed named probes and zero-byte reads.
  Opens of directories are included. Directory-entry syscalls are counted separately in
  the raw data. Ancestor probes outside the provider roots, unrelated Git work, sockets
  and the bare plugin state root are outside provider totals. The detailed trace's
  summary is restricted to the selected syscall classes. The separate `-c` pass counts
  the process and descendants across all syscall classes. Division by doctor refresh
  deltas normalizes a timed window. Attach and detach can include partial refreshes, so
  these ratios are approximate per-refresh counts.
- Delay starts when the provider's synthetic file update closes or its rename completes.
  The writer uses buffered file operations without an fsync. The first `herdr api
  snapshot` result matching that write's percentage gives an upper bound using the
  response time. The request-start time of the last mismatching response gives a lower
  bound. Snapshots are requested about every 250 ms in the delay-only pass. Median
  response spacing was 251–252 ms by workload, with a maximum of 373 ms. An intermediate
  version can be superseded before observation, or can be missed between polls. Delay
  quantiles and spreads use observed versions. The final version is checked after the
  drain. This observes the value stored by Herdr's server and requires no renderer.

## Results

In the tables, C/Cx/G/K gives counts in Claude, Codex, Grok, Kimi order.
R/O/S denotes attributed read, open and stat calls. Spread columns report
minimum / median / maximum, followed by relative spread.

### Resident process and worker counts

| Rate / run | Actual s | Updates C/Cx/G/K | Refresh attempts | Worker attempts C/Cx/G/K | Instructions:u | Task-clock ms | Daemon CPU ms | Waited child CPU ms | Med. RSS KiB | Med. PSS KiB | Samples | Daemon CPU core busy % |
| --- | ---: | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| none 1 | 60.136 | 0/0/0/0 | 6 | 6/6/6/6 | 50,125,211 | 608.14 | 550 | 60 | 5,536.0 | 3,312.0 | 14 | 74.0 |
| none 2 | 60.108 | 0/0/0/0 | 5 | 5/5/5/5 | 42,036,670 | 730.08 | 690 | 30 | 5,552.0 | 3,328.0 | 13 | 91.7 |
| none 3 | 60.200 | 0/0/0/0 | 6 | 6/6/6/6 | 50,115,209 | 500.63 | 450 | 40 | 5,132.0 | 2,908.0 | 14 | 99.7 |
| steady 1 | 60.145 | 15/15/15/15 | 6 | 6/6/6/6 | 60,448,531 | 311.29 | 200 | 50 | 5,572.0 | 3,328.0 | 14 | 88.3 |
| steady 2 | 60.144 | 15/15/15/15 | 6 | 6/6/6/6 | 60,502,994 | 204.05 | 150 | 50 | 5,480.0 | 3,236.0 | 14 | 97.7 |
| steady 3 | 60.091 | 15/15/15/15 | 6 | 6/6/6/6 | 61,852,000 | 197.90 | 150 | 40 | 5,440.0 | 3,196.0 | 14 | 100.0 |
| burst 1 | 60.142 | 24/24/24/24 | 6 | 6/6/6/6 | 54,986,809 | 189.88 | 150 | 40 | 5,622.0 | 3,386.0 | 14 | 88.0 |
| burst 2 | 60.151 | 24/24/24/24 | 6 | 6/6/6/6 | 56,013,151 | 255.31 | 160 | 60 | 5,180.0 | 2,944.0 | 14 | 94.4 |
| burst 3 | 60.147 | 24/24/24/24 | 6 | 6/6/6/6 | 57,206,899 | 367.29 | 320 | 40 | 5,176.0 | 2,940.0 | 14 | 96.7 |
| rename 1 | 60.355 | 15/15/15/15 | 6 | 6/6/6/6 | 58,334,037 | 683.28 | 630 | 40 | 5,208.0 | 2,984.0 | 14 | 92.7 |
| rename 2 | 60.157 | 15/15/15/15 | 5 | 5/5/5/5 | 48,723,897 | 566.85 | 500 | 40 | 5,216.0 | 2,992.0 | 14 | 91.8 |
| rename 3 | 60.088 | 15/15/15/15 | 6 | 6/6/6/6 | 58,352,714 | 420.92 | 370 | 50 | 5,220.0 | 2,996.0 | 14 | 90.4 |

| Rate | Instructions:u | Task-clock ms | Daemon CPU ms | RSS KiB | PSS KiB | Instructions:u per refresh |
| --- | --- | --- | --- | --- | --- | --- |
| none | 42,036,670 / 50,115,209 / 50,125,211 (16.14%) | 500.63 / 608.14 / 730.08 (37.73%) | 450 / 550 / 690 (43.64%) | 5,132.0 / 5,536.0 / 5,552.0 (7.59%) | 2,908.0 / 3,312.0 / 3,328.0 (12.68%) | 8,352,535 / 8,354,202 / 8,407,334 (0.66%) |
| steady | 60,448,531 / 60,502,994 / 61,852,000 (2.32%) | 197.90 / 204.05 / 311.29 (55.57%) | 150 / 150 / 200 (33.33%) | 5,440.0 / 5,480.0 / 5,572.0 (2.41%) | 3,196.0 / 3,236.0 / 3,328.0 (4.08%) | 10,074,755 / 10,083,832 / 10,308,667 (2.32%) |
| burst | 54,986,809 / 56,013,151 / 57,206,899 (3.96%) | 189.88 / 255.31 / 367.29 (69.49%) | 150 / 160 / 320 (106.25%) | 5,176.0 / 5,180.0 / 5,622.0 (8.61%) | 2,940.0 / 2,944.0 / 3,386.0 (15.15%) | 9,164,468 / 9,335,525 / 9,534,483 (3.96%) |
| rename | 48,723,897 / 58,334,037 / 58,352,714 (16.51%) | 420.92 / 566.85 / 683.28 (46.28%) | 370 / 500 / 630 (52.00%) | 5,208.0 / 5,216.0 / 5,220.0 (0.23%) | 2,984.0 / 2,992.0 / 2,996.0 (0.40%) | 9,722,340 / 9,725,452 / 9,744,779 (0.23%) |

### File calls by provider

| Rate / trace | Actual s | Updates C/Cx/G/K | Refresh attempts | Worker attempts C/Cx/G/K | Claude | Codex | Grok | Kimi | Global R/O/S (same trace) |
| --- | ---: | --- | ---: | --- | --- | --- | --- | --- | --- |
| none 1 | 35.126 | 0/0/0/0 | 2 | 2/2/2/2 | 10/2/4 | 0/42/170 | 14/4/48 | 0/812/3612 | 88/1207/4589 |
| none 2 | 35.154 | 0/0/0/0 | 3 | 3/3/3/3 | 15/3/6 | 0/63/255 | 21/6/72 | 0/1218/5418 | 185/2505/7733 |
| none 3 | 35.328 | 0/0/0/0 | 2 | 2/2/2/2 | 10/2/4 | 0/42/170 | 14/4/48 | 0/812/3612 | 126/1673/5247 |
| steady 1 | 35.669 | 8/8/8/8 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 1/621/2455 | 87/956/3379 |
| steady 2 | 35.069 | 8/8/8/8 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 2/812/3616 | 111/1458/4969 |
| steady 3 | 35.115 | 8/8/8/8 | 3 | 3/3/3/3 | 15/3/6 | 3/63/261 | 21/6/72 | 3/1218/5424 | 173/2380/7613 |
| burst 1 | 35.729 | 16/16/16/16 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 2/812/3616 | 92/1276/4678 |
| burst 2 | 35.131 | 16/16/16/16 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 2/812/3616 | 97/1290/4748 |
| burst 3 | 35.269 | 16/16/16/16 | 3 | 3/3/3/3 | 15/3/6 | 2/63/259 | 21/6/72 | 2/1218/5422 | 186/2507/7765 |
| rename 1 | 35.428 | 8/8/8/8 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 1/911/3116 | 85/1293/4053 |
| rename 2 | 35.245 | 8/8/8/8 | 2 | 2/2/2/2 | 10/2/4 | 2/42/174 | 14/4/48 | 0/557/1809 | 67/824/2580 |
| rename 3 | 35.085 | 8/8/8/8 | 1 | 2/2/2/2 | 10/2/4 | 2/62/214 | 14/4/48 | 0/313/942 | 80/665/1882 |

| Rate | Provider | R/refresh spread | O/refresh spread | S/refresh spread | R+O+S/refresh spread | Read bytes, each window |
| --- | --- | --- | --- | --- | --- | --- |
| none | claude | 5.0 / 5.0 / 5.0 (0.00%) | 1.0 / 1.0 / 1.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 8.0 / 8.0 / 8.0 (0.00%) | 292/438/292 |
| none | codex | 0.0 / 0.0 / 0.0 (n/a) | 21.0 / 21.0 / 21.0 (0.00%) | 85.0 / 85.0 / 85.0 (0.00%) | 106.0 / 106.0 / 106.0 (0.00%) | 0/0/0 |
| none | grok | 7.0 / 7.0 / 7.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 24.0 / 24.0 / 24.0 (0.00%) | 33.0 / 33.0 / 33.0 (0.00%) | 2,004/3,006/2,004 |
| none | kimi | 0.0 / 0.0 / 0.0 (n/a) | 406.0 / 406.0 / 406.0 (0.00%) | 1,806.0 / 1,806.0 / 1,806.0 (0.00%) | 2,212.0 / 2,212.0 / 2,212.0 (0.00%) | 0/0/0 |
| steady | claude | 5.0 / 5.0 / 5.0 (0.00%) | 1.0 / 1.0 / 1.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 8.0 / 8.0 / 8.0 (0.00%) | 292/292/438 |
| steady | codex | 1.0 / 1.0 / 1.0 (0.00%) | 21.0 / 21.0 / 21.0 (0.00%) | 87.0 / 87.0 / 87.0 (0.00%) | 109.0 / 109.0 / 109.0 (0.00%) | 218,751/221,436/336,450 |
| steady | grok | 7.0 / 7.0 / 7.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 24.0 / 24.0 / 24.0 (0.00%) | 33.0 / 33.0 / 33.0 (0.00%) | 2,004/2,004/3,006 |
| steady | kimi | 0.5 / 1.0 / 1.0 (50.00%) | 310.5 / 406.0 / 406.0 (23.52%) | 1,227.5 / 1,808.0 / 1,808.0 (32.11%) | 1,538.5 / 2,215.0 / 2,215.0 (30.54%) | 38,599/78,779/120,261 |
| burst | claude | 5.0 / 5.0 / 5.0 (0.00%) | 1.0 / 1.0 / 1.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 8.0 / 8.0 / 8.0 (0.00%) | 292/292/438 |
| burst | codex | 0.7 / 1.0 / 1.0 (33.33%) | 21.0 / 21.0 / 21.0 (0.00%) | 86.3 / 87.0 / 87.0 (0.77%) | 108.0 / 109.0 / 109.0 (0.92%) | 230,759/236,494/242,222 |
| burst | grok | 7.0 / 7.0 / 7.0 (0.00%) | 2.0 / 2.0 / 2.0 (0.00%) | 24.0 / 24.0 / 24.0 (0.00%) | 33.0 / 33.0 / 33.0 (0.00%) | 2,004/2,004/3,006 |
| burst | kimi | 0.7 / 1.0 / 1.0 (33.33%) | 406.0 / 406.0 / 406.0 (0.00%) | 1,807.3 / 1,808.0 / 1,808.0 (0.04%) | 2,214.0 / 2,215.0 / 2,215.0 (0.05%) | 83,522/86,498/89,474 |
| rename | claude | 5.0 / 5.0 / 10.0 (100.00%) | 1.0 / 1.0 / 2.0 (100.00%) | 2.0 / 2.0 / 4.0 (100.00%) | 8.0 / 8.0 / 16.0 (100.00%) | 292/292/292 |
| rename | codex | 1.0 / 1.0 / 2.0 (100.00%) | 21.0 / 21.0 / 62.0 (195.24%) | 87.0 / 87.0 / 214.0 (145.98%) | 109.0 / 109.0 / 278.0 (155.05%) | 200,672/200,672/200,672 |
| rename | grok | 7.0 / 7.0 / 14.0 (100.00%) | 2.0 / 2.0 / 4.0 (100.00%) | 24.0 / 24.0 / 48.0 (100.00%) | 33.0 / 33.0 / 66.0 (100.00%) | 2,004/2,004/2,004 |
| rename | kimi | 0.0 / 0.0 / 0.5 (n/a) | 278.5 / 313.0 / 455.5 (56.55%) | 904.5 / 942.0 / 1,558.0 (69.37%) | 1,183.0 / 1,255.0 / 2,014.0 (66.22%) | 33,949/0/0 |

| Rate | Actual s | Writes C/Cx/G/K | Refreshes | Reads | Opens | Stats | All syscalls |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| none | 35.011 | 0/0/0/0 | 3 | 181 | 2,364 | 7,592 | 14,839 |
| steady | 35.004 | 8/8/8/8 | 3 | 168 | 2,408 | 7,293 | 14,550 |
| burst | 35.013 | 16/16/16/16 | 2 | 87 | 1,344 | 4,966 | 10,006 |
| rename | 35.008 | 8/8/8/8 | 3 | 132 | 1,549 | 5,124 | 10,177 |

Kimi worker traces reaching final model-window validation give a second view
of the attributed file calls. Grouping the same trace by TID requires the
provider-home stat at the start and all six model-config path probes through
the successful final descriptor stat. The timed-window totals above retain
workers captured only in part. This grouping verifies the traced path, while
stored token values are checked in the separate delay windows.

| Rate | Worker traces spanning both markers | Reads / opens / stats / directory reads by worker |
| --- | ---: | --- |
| none | 7 | 0/406/1,806/418 (n=7) |
| steady | 6 | 1/406/1,808/418 (n=6) |
| burst | 7 | 0/406/1,806/418 (n=1), 1/406/1,808/418 (n=6) |
| rename | 1 | 1/615/2,226/836 (n=1) |

### Source writes to visible stored tokens

| Rate / run | Provider | Observed / writes | Observed percentage and lower–upper delay bounds (s) | Final / last written |
| --- | --- | --- | --- | --- |
| steady 1 | claude | 3 / 8 | 91%: 0.000–0.090, 93%: 1.877–2.160, 96%: 0.000–0.195 | 96 / 96 |
| steady 1 | codex | 3 / 8 | 91%: 0.000–0.090, 93%: 1.876–2.160, 96%: 0.000–0.195 | 96 / 96 |
| steady 1 | grok | 3 / 8 | 91%: 0.000–0.090, 93%: 1.876–2.160, 96%: 0.000–0.195 | 96 / 96 |
| steady 1 | kimi | 3 / 8 | 91%: 0.000–0.090, 93%: 1.876–2.160, 96%: 0.176–0.437 | 96 / 96 |
| steady 2 | claude | 4 / 8 | 97%: 1.012–1.282, 11%: 3.299–3.558, 14%: 1.342–1.604, 16%: 3.363–3.626 | 16 / 16 |
| steady 2 | codex | 4 / 8 | 97%: 1.012–1.282, 11%: 3.299–3.558, 14%: 1.342–1.604, 16%: 3.363–3.626 | 16 / 16 |
| steady 2 | grok | 4 / 8 | 97%: 1.012–1.282, 11%: 3.299–3.558, 14%: 1.342–1.604, 16%: 3.363–3.626 | 16 / 16 |
| steady 2 | kimi | 4 / 8 | 97%: 1.262–1.526, 11%: 3.299–3.558, 14%: 1.342–1.604, 16%: 3.363–3.625 | 16 / 16 |
| steady 3 | claude | 4 / 8 | 18%: 0.267–0.523, 20%: 2.129–2.387, 23%: 0.418–0.678, 24%: 6.721–6.984 | 24 / 24 |
| steady 3 | codex | 4 / 8 | 18%: 0.267–0.523, 20%: 2.380–2.641, 23%: 0.418–0.678, 24%: 6.721–6.983 | 24 / 24 |
| steady 3 | grok | 4 / 8 | 18%: 0.266–0.523, 20%: 2.380–2.641, 23%: 0.418–0.678, 24%: 6.721–6.983 | 24 / 24 |
| steady 3 | kimi | 4 / 8 | 18%: 0.266–0.523, 20%: 2.380–2.641, 23%: 0.668–0.951, 24%: 6.721–6.983 | 24 / 24 |
| burst 1 | claude | 2 / 16 | 67%: 4.675–4.949, 75%: 5.073–5.334 | 75 / 75 |
| burst 1 | codex | 2 / 16 | 67%: 4.675–4.949, 75%: 5.073–5.334 | 75 / 75 |
| burst 1 | grok | 2 / 16 | 67%: 4.927–5.203, 75%: 5.073–5.334 | 75 / 75 |
| burst 1 | kimi | 2 / 16 | 67%: 4.927–5.203, 75%: 5.073–5.334 | 75 / 75 |
| burst 2 | claude | 3 / 16 | 83%: 8.449–8.716, 85%: 0.000–0.259, 91%: 9.302–9.590 | 91 / 91 |
| burst 2 | codex | 3 / 16 | 83%: 8.449–8.716, 85%: 0.000–0.258, 91%: 9.301–9.590 | 91 / 91 |
| burst 2 | grok | 3 / 16 | 83%: 8.449–8.716, 85%: 0.000–0.258, 91%: 9.301–9.590 | 91 / 91 |
| burst 2 | kimi | 3 / 16 | 83%: 9.202–9.480, 85%: 0.000–0.258, 91%: 9.301–9.590 | 91 / 91 |
| burst 3 | claude | 2 / 16 | 11%: 2.917–3.201, 19%: 3.265–3.554 | 19 / 19 |
| burst 3 | codex | 2 / 16 | 11%: 2.917–3.201, 19%: 3.265–3.553 | 19 / 19 |
| burst 3 | grok | 2 / 16 | 11%: 2.917–3.200, 19%: 3.265–3.553 | 19 / 19 |
| burst 3 | kimi | 2 / 16 | 11%: 2.917–3.200, 19%: 3.516–3.786 | 19 / 19 |
| rename 1 | claude | 4 / 8 | 89%: 0.763–1.022, 91%: 2.783–3.041, 94%: 0.842–1.103, 96%: 2.888–3.155 | 96 / 96 |
| rename 1 | codex | 4 / 8 | 89%: 0.762–1.022, 91%: 2.783–3.040, 94%: 0.841–1.103, 96%: 2.888–3.155 | 96 / 96 |
| rename 1 | grok | 4 / 8 | 89%: 0.762–1.022, 91%: 2.782–3.040, 94%: 0.839–1.100, 96%: 2.888–3.154 | 96 / 96 |
| rename 1 | kimi | 4 / 8 | 89%: 0.762–1.022, 91%: 2.782–3.040, 94%: 0.839–1.100, 96%: 2.887–3.154 | 96 / 96 |
| rename 2 | claude | 4 / 8 | 97%: 3.761–4.024, 12%: 1.789–2.051, 14%: 3.831–4.100, 16%: 5.863–6.126 | 16 / 16 |
| rename 2 | codex | 4 / 8 | 97%: 3.756–4.019, 12%: 1.789–2.050, 14%: 3.831–4.100, 16%: 5.863–6.126 | 16 / 16 |
| rename 2 | grok | 4 / 8 | 97%: 3.755–4.018, 12%: 1.788–2.050, 14%: 3.830–4.099, 16%: 5.862–6.126 | 16 / 16 |
| rename 2 | kimi | 4 / 8 | 97%: 3.755–4.018, 12%: 1.788–2.050, 14%: 3.830–4.099, 16%: 5.862–6.125 | 16 / 16 |
| rename 3 | claude | 4 / 8 | 18%: 2.770–3.040, 21%: 0.795–1.056, 23%: 3.065–3.327, 24%: 9.090–9.352 | 24 / 24 |
| rename 3 | codex | 4 / 8 | 18%: 2.769–3.040, 21%: 0.794–1.056, 23%: 3.065–3.326, 24%: 9.089–9.351 | 24 / 24 |
| rename 3 | grok | 4 / 8 | 18%: 2.769–3.040, 21%: 0.794–1.056, 23%: 3.064–3.326, 24%: 9.089–9.350 | 24 / 24 |
| rename 3 | kimi | 4 / 8 | 18%: 2.767–3.037, 21%: 1.044–1.310, 23%: 3.064–3.326, 24%: 9.089–9.350 | 24 / 24 |

| Rate | Provider | Upper bound median per run (s), A/A spread |
| --- | --- | --- |
| steady | claude | 0.195 / 1.533 / 2.581 (155.67%) |
| steady | codex | 0.195 / 1.660 / 2.581 (143.76%) |
| steady | grok | 0.195 / 1.659 / 2.581 (143.76%) |
| steady | kimi | 0.437 / 1.796 / 2.581 (119.34%) |
| burst | claude | 3.377 / 5.141 / 8.716 (103.84%) |
| burst | codex | 3.377 / 5.141 / 8.716 (103.84%) |
| burst | grok | 3.377 / 5.268 / 8.716 (101.34%) |
| burst | kimi | 3.493 / 5.268 / 9.480 (113.63%) |
| rename | claude | 2.072 / 3.184 / 4.062 (62.52%) |
| rename | codex | 2.071 / 3.183 / 4.060 (62.46%) |
| rename | grok | 2.070 / 3.183 / 4.059 (62.47%) |
| rename | kimi | 2.070 / 3.182 / 4.058 (62.50%) |

The no-change case has no source-to-token delays. All four warm-up values
were present at its final snapshot. For each provider, the delay pass
observed 11 of 24 steady values, 7 of 48 burst values, and 12 of 24 rename
values. All 36 provider-by-window final values agreed with their final
writes. The longest first-observation upper bounds were 6.984 s for steady,
9.590 s for burst and 9.352 s for rename. These times depend on the phase
of a write within the periodic refresh and on snapshot polling. The burst
counts show how many intermediate versions the current periodic path
already passes over at this observation resolution.

## Interpretation

- **The largest repeated cost is Kimi's filesystem discovery and validation.**
  Kimi accounted for 77.7–93.8% of the attributed read/open/stat calls across
  the 12 traces. In the no-change traces, each refresh did 406 opens, 1,806
  stats and 418 directory-entry calls under the Kimi root. No payload read
  occurred for Kimi in those windows. The reader already caches unchanged
  state bindings and wire content while opening and revalidating the
  candidate paths. In each write workload, Codex transferred the most
  attributed read bytes. [Kimi state validation](../../src/reader_kimi.rs#L73)
- **An unchanged-source fast path has observable call-count headroom.**
  Avoiding one equivalent no-change Kimi invocation would avoid up to 2,212
  attributed read/open/stat calls before the cost of any change detection
  or reconciliation. This exceeds the observed no-change A/A spread of zero
  calls per refresh in those three traces. Fixed 35 s windows captured
  different numbers of refreshes. Raw Kimi counts ranged from 4,424 to
  6,636, a one-invocation difference, so one omitted invocation would tie
  the raw fixed-window spread. A comparison at equal refresh counts
  resolves that ambiguity. A candidate would have to keep the exact
  provider/session binding and periodic token renewal while proving that
  reuse is valid.
- **The rename trace shows the cost of a changing directory tree.**
  One rename worker reached its final model-window validation and made
  836 directory-entry calls, twice the no-change count. The discovery
  code retries a traversal when an observed directory or ancestor stamp
  changes. Two other rename attribution windows each had one Kimi deadline,
  and the final one captured two worker attempts across one reported refresh.
  All raw windows remain in the tables. The rename per-refresh ratios include
  boundary and deadline effects. The Kimi deadlines were observed only in
  the detailed trace pass. Their presence limits any comparison of rename
  syscall totals with the other rates. [Discovery revalidation](../../src/model.rs#L254)
- **Whole-daemon CPU and memory gains remain unmeasured.** No-change daemon
  CPU was 450–690 ms per nominal 60 s (median 550 ms, absolute spread
  240 ms). Whole-daemon instructions per refresh had a 0.66% no-change A/A
  spread while 60 s instruction totals had a 16.14% spread. The 60 s
  window held five or six refreshes. CPU, RSS and PSS are whole-process
  observations with sequential workloads on a shared host. The call-count
  headroom supports a reader-scoped trial, with CPU and RSS/PSS effects
  requiring paired measurements of that implementation.

## Evidence and validation

The managed run started at 12:02:26 UTC and ended at 12:32:41 UTC on
2026-11-18. `dev-task-temp` recorded exit 0, state `complete` and no child
failure for run `e0cf00f4c2e5f8b9d22b056aae886159`. All four private
Lineup and Herdr server processes exited 0 and all four private session
directories were deleted.

The retained artifact root is
`$HOME/.cache/dev-task-temp/runs/e0cf00f4c2e5f8b9d22b056aae886159/work`.
It contains `nominal/bench.py`, `summarize.py`, the measured release binary,
and one `nominal/<rate>/` directory per workload. The files retain host and
process samples, strace summaries and path-attributed traces, doctor readings,
the matching-token snapshots and write times, normalized fixture digests, and
cleanup results. Pilot runs were excluded from the fixed-window results.

The retained `raw.sha256` inventory checks all 218 listed artifacts. Its
SHA-256 is `1def499b5ad622caf9f765f77f08a4b26ad00063b12bd7e9810c0e05dc0e2e25`.
The command-result manifest at `checks/checks.json` has SHA-256
`5f3936e98181374a18aa330767ff7e5c99143b2d8e401e059e479875c82de87a`.

| Pinned-toolchain check | Exit | Result |
| --- | ---: | --- |
| `cargo fmt --all --check` | 0 | Formatted |
| `cargo clippy --locked --all-targets -- -D warnings` | 0 | All targets checked |
| `cargo test --locked` | 0 | 495 passed, 0 failed, 6 ignored |
| `cargo build --release --locked` | 0 | Executable SHA-256 matched the measured build |
| `target/release/herdr-lineup --help` | 0 | Help emitted |
