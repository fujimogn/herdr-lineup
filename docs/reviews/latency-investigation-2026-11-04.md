# Isolated latency investigation, 2026-11-04

## Scope and definitions

This record measures the unchanged source at `80e11d1604a2c4bb56d0fc008c157b3b4b179d0d`
on Linux. It is a documentation-only candidate. The method was submitted as
BLOCKED (Formation revision 184), then approved by commander note
`note_lat_method` (source event revision 188). No user session was attached,
stopped, restarted, configured, or used as a measurement endpoint.

"State change" below means a synthetic `herdr pane report-agent` request,
not a real provider beginning work. "Token observed" means a subsequent
`api snapshot` contains the new `lineup_state`. It is not a rendered
sidebar frame. Event-to-visible latency remains unmeasured.

For an event not yet included in a publisher snapshot, the path is:
remaining wait for the next eligible snapshot, snapshot acquisition,
workspace Git heading work, relevant pane reader/planning work, checkpoint
reservation when needed, metadata exchange, then Herdr's storage and UI path.
The event may arrive after that cycle's snapshot and wait for a later cycle.
Earlier panes and fair-queue admission can affect the relevant pane.

The daemon sets `refresh_at = Instant::now() + refresh_interval` **after**
a refresh completes (`src/daemon.rs`). The 10 s default is a post-cycle
wait, not a fixed start-to-start period or a 10 s upper bound. The idle loop
sleeps 100 ms, subject to scheduling. With no overrun, deferral or failure,
waiting depends on phase; an event missed by an in-flight snapshot can also
incur that cycle's remaining work. This is code analysis, not a measured
bound. The cooperative default envelope remains 7 s, may be overrun by
synchronous filesystem work or reaping, and must not be added to 10 s as a
guarantee.

Live-key TTL is 30 s (`3 * 10 s`), not an extra initial publication delay.
Unchanged live keys become eligible for renewal one refresh interval after
their last successful publication. Missing live keys are eligible for repair
on the next authoritative snapshot. Admission deferrals and long cycles can
allow expiry. A changed state need not wait for TTL expiration.

## Existing evidence and gaps

| Evidence | What it measures | What it excludes |
| --- | --- | --- |
| [Historical stage record](refresh-stage-latency-2026-10-23.md) | Cold synthetic refreshes, five pane counts; debug test; fake socket and SampleReader; several explicitly separated source snapshots | Provider state-change timestamps, periodic phase waiting, real provider discovery, real Herdr storage/rendering, macOS |
| [Alternating stage comparison](stage-timing-comparison-2026-11-04.md) | Old `4afc9d42b9bab850576ffb3bd42d44accf2d21c2` and new `bd62eb512d7708346591e8df84f05bec0bd5e645`; tmpfs/ext4; six rows per version/size/filesystem; 120 rows | An isolated effect of one change, stable tails, actual UI or provider transitions |
| Coverage N5 | Describes Linux/macOS report-complete and visible-delay evidence still needed | Existing synthetic report completion does not establish rendered latency |
| Coverage L6 | Historical 5 s/default and 100 ms idle-loop follow-up | The 5 s wording is stale at this source: config, README and token specification now use 10 s/30 s; freshness/load acceptance remains undefined |

Stage intervals overlap. Snapshot includes socket work, persistence occurs
inside report processing, Git includes formatting/cache work, and injected
reader time excludes complete pane processing. Never sum stage columns.
Doctor histograms summarize completed attempts in coarse buckets; they do
not correlate a particular provider event, report acknowledgment and UI frame.
Live reader diagnostics are advisory and nontransactional. A healthy result
or an unavailable measurement is not a measured zero.

## Isolation and procedure

All temporary artifacts were created through `dev-task-temp run` with
root `/home/fujimogn/.cache/dev-task-temp`, task ID
`lineup-latency-20261104`, a 5,000,000,000-byte request and 50,000-inode request.
Preflight returned sufficient (ext4; quotas disabled); it does not reserve
capacity. The >1 GB build estimate was reported before starting.
The dedicated worktree is
`.claude/worktrees/latency-fm-20261104`; no Rust source, TODO or coverage file
was edited.

The supervised Python controller retained directory descriptors and used
`/proc/<controller-pid>/fd/<fd>` aliases to keep Unix socket addresses short.
Their canonical targets were directories in the managed run, not in `/tmp`.
No `/tmp/hl-lat-*` exception directory was needed. The controller remained
alive throughout measurement and cleanup.

Build commands, completed before measurement:

```sh
cargo test --locked --lib --no-run -j 2
cargo build --release --locked -j 2
```

Environment overrides: `CARGO_TARGET_DIR=<run>/target`,
`RUSTC_WRAPPER=`, `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`.
Thus A is a debug test profile with debug information disabled; B is release.
No build or verification was started by this lane while A/B ran. Other host
activity, kernel caches and storage scheduling were uncontrolled.

A ran the existing ignored library test, without source instrumentation changes:

```sh
LINEUP_BENCH_ROOT=/proc/<controller-pid>/fd/<fixture-fd> \
LINEUP_BENCH_REPEATS=3 <library-test-binary> \
  publish::tests::measure_refresh_stages --exact --ignored --nocapture
```

The fixture creates a new publisher, checkpoint, one local Git repository,
one workspace and 1/8/32/128/256 synthetic Codex panes for each repetition.
SampleReader returns fixed data. A fake Unix socket acknowledges requests.
Each fixture has a fresh directory; this does not imply cold kernel caches.
It is one cold refresh per publisher, not periodic operation or a state-change
latency sample. Filesystem is ext4 only in this run.

B used the release binary and one dedicated headless Herdr server. It removed
every inherited `HERDR_*` value, then set only private environment values.
HOME, XDG data/state/cache/config roots and Codex/Grok/Kimi homes were inside
the managed run. The private Herdr config disabled update/manifest checks,
selected non-login `/bin/sh`, and contained no user configuration or plugin
hooks. No provider authentication was copied; Lineup used an empty private
settings directory, retaining its defaults and network-disabled behavior.

Before every Herdr or Lineup operation the harness required:
the session name is `latency`, never `main`; the canonical socket parent is
the expected private session directory; the socket resolves inside the managed
run; the socket address is under 104 bytes; and it is outside the user's main
session directory. CLI invocations always specified `--session latency`.
Lineup received only that private socket plus private plugin state/config.
The only lifecycle commands were for the server and publisher created here.

A synthetic workspace used an empty local Git commit with `origin/main`.
B reported a synthetic Codex occupant, alternating idle/working without
binding a native provider session. After the initial idle token appeared,
each trial waited 0.2, 5 or 9 s after the preceding new-token observation,
in that order, three repetitions. These are **observation-relative phase
proxies**, not exact positions within the daemon's refresh schedule.
Monotonic timestamps surround report-agent submission and acknowledgment.
Snapshot polling sleeps 100 ms after each completed unsuccessful query, so
the actual gap also includes CLI execution and scheduling.

For each trial, the lower latency bound is
`max(0, last-old-poll-start - event-ack)`; the upper bound is
`first-new-poll-end - event-start`. Snapshot evaluation and state mutation
occur somewhere within their respective RPC intervals. These conservative
bounds include polling uncertainty; they are not exact server acceptance or
render timestamps. The final unchanged state was sampled for 35 s, longer
than one 30 s TTL, to test token retention through renewal. No client renderer
was attached.



## Source, host and binary identity

```json
{
  "source": "80e11d1604a2c4bb56d0fc008c157b3b4b179d0d",
  "test_binary": "/home/fujimogn/.cache/dev-task-temp/runs/4ee0a98a4a62c627b247712672dc193c/work/target/debug/deps/herdr_lineup-aa8a03e3a907570a",
  "test_sha256": "022bc0320f737ea16233b459c9168948336fcf91495966cfd165f2a4c30d1d74",
  "release_sha256": "1807f27cb3665993d1bdd1ddef549681f6d81821a88c7e31bfb40d68b9331ceb",
  "uname": {
    "system": "Linux",
    "node": "dev",
    "release": "7.3.1-1-cachyos",
    "version": "#1 SMP PREEMPT_DYNAMIC Sat, 31 Oct 2026 06:43:52 +0000",
    "machine": "x86_64",
    "processor": ""
  },
  "rustc": "rustc 1.99.1 (58cf8f9c3 2026-10-13)",
  "herdr": "herdr 0.9.5",
  "loadavg_before_a": [
    65.06005859375,
    83.6552734375,
    79.52783203125
  ],
  "bench_root": "/proc/1662459/fd/3",
  "canonical_bench_root": "/home/fujimogn/.cache/dev-task-temp/runs/4ee0a98a4a62c627b247712672dc193c/work/a-fixtures"
}
```

Host load was high and uncontrolled (load averages above). No affinity or cache manipulation was used. These observations do not support a speed comparison.

## A: cold refresh stage observations

Times are independent medians in microseconds (three rows per pane count).

| Panes | Total | Snapshot | Git/heading | Injected reader | Persist | Socket |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 22034 | 641 | 7523 | 10 | 8933 | 3883 |
| 8 | 54450 | 1145 | 19658 | 89 | 11961 | 17983 |
| 32 | 158205 | 7168 | 31817 | 308 | 12803 | 88707 |
| 128 | 438318 | 22445 | 34187 | 951 | 12492 | 343081 |
| 256 | 846766 | 33671 | 37501 | 2053 | 13134 | 689816 |

All 15 rows recorded reports = panes + 1 and checkpoint_rows = 1. Row count is not a separate persistence-write counter. The socket median is the largest stage at 32/128/256 panes in this fixture; this does not establish a real-agent or UI bottleneck.

Raw rows, microseconds:

```csv
panes,run,total_us,pipeline_us,snapshot_us,git_us,reader_us,persist_us,socket_us,reports,checkpoint_rows,external_us
1,0,20172,19914,641,7082,9,8722,3021,2,1,20199
1,1,22888,22700,597,8451,10,8933,4326,2,1,22903
1,2,22034,21891,678,7523,10,9345,3883,2,1,22049
8,0,43528,43271,1440,10587,131,9477,17983,9,1,43547
8,1,67767,67620,845,25683,89,17616,16856,9,1,67785
8,2,54450,54026,1145,19658,81,11961,18099,9,1,54466
32,0,132945,132455,7168,16693,282,12803,88707,33,1,132965
32,1,173104,172953,7757,31817,349,12958,105167,33,1,173117
32,2,158205,158032,6000,45349,308,12385,88596,33,1,158221
128,0,466111,465958,13419,30450,1124,11932,343987,129,1,466126
128,1,436146,435888,22445,35495,951,15254,329512,129,1,436157
128,2,438318,438094,28818,34187,894,12492,343081,129,1,438333
256,0,846766,846558,33671,37293,1904,13134,689816,257,1,846779
256,1,841728,841451,51855,43676,2053,12040,686940,257,1,841742
256,2,973093,972788,28528,37501,2208,60807,734781,257,1,973109
```

## B: synthetic state request to stored-token observation

| Repetition | Phase proxy | State | Lower bound ms | Upper bound ms | First-new RPC ms |
| ---: | --- | --- | ---: | ---: | ---: |
| 0 | early | working | 9666.440 | 9817.305 | 22.927 |
| 0 | mid | idle | 5056.736 | 5197.640 | 14.828 |
| 0 | late | working | 929.137 | 1069.051 | 10.697 |
| 1 | early | idle | 9712.303 | 9865.191 | 25.459 |
| 1 | mid | working | 4843.923 | 5019.592 | 23.143 |
| 1 | late | idle | 964.535 | 1114.501 | 14.659 |
| 2 | early | working | 9832.867 | 9972.786 | 12.489 |
| 2 | mid | idle | 5013.911 | 5155.522 | 4.730 |
| 2 | late | working | 948.110 | 1063.543 | 7.421 |

Raw monotonic timestamps in nanoseconds, sharing one Python clock:

```csv
repetition,phase,state,event_start_ns,event_ack_ns,last_old_poll_start_ns,last_old_poll_end_ns,first_new_poll_start_ns,first_new_poll_end_ns
0,early,working,10922675469263,10922686923981,10932353364161,10932367038686,10932469847178,10932492774569
0,mid,idle,10937493507737,10937505306298,10942562042420,10942575922008,10942676319862,10942691147641
0,late,working,10951691301961,10951705912987,10952635050023,10952648285628,10952749655532,10952760352626
1,early,idle,10952960606848,10952973422489,10962685725394,10962698462980,10962800339353,10962825797894
1,mid,working,10967830751491,10967854854520,10972698777608,10972727054143,10972827200628,10972850343924
1,late,idle,10981853097559,10981865978143,10982830513240,10982851089190,10982952939442,10982967598067
2,early,working,10983169790893,10983183608939,10993016475675,10993028881673,10993130087443,10993142576868
2,mid,idle,10998145494753,10998161081664,11003174992351,11003195634857,11003296286868,11003301017105
2,late,working,11012301125177,11012305351183,11013253460999,11013257144902,11013357246637,11013364667933
```

The phase dependence is consistent with periodic polling after refresh completion. Nine trials do not establish a distribution, stable tail or workload bound. B has no event-correlated per-stage timestamps; A cannot be subtracted from B to isolate waiting because profile, fixture and workload differ.

Renewal: 296 snapshots, elapsed 6.905 through 34904.323 ms after the final token observation, all state working. Maximum completion-to-completion sample gap: 286.822 ms. The loop ran for at least 35 s, beyond the 30 s TTL. This proves token retention at sampled points through renewal, not uninterrupted visibility, an exact renewal timestamp or degraded-cycle expiry.

### Private lifecycle cleanup and failed attempts

The first supervisor stopped at terminal setup before build or measurement; its own controller was terminated. Its managed run was df766240170691251168165285cb00b3. A queued overlong controller line was rejected by JSON parsing before B executed; it produced no B data. The first B preflight then started private services but misread the integer PID record as an object. Its finally block stopped both services successfully. PIDs 1867095/1867178 were subsequently checked using ps -o pid,ppid,stat,args -p 1867095,1867178: header only, exit 1. No transition samples came from that attempt. Only the Python harness was corrected; successful B used a new b2 directory and the same binary. No successful measurement was discarded or rerun.

Successful B cleanup captured after stopping its private services:

```json
{
  "lineup_stop": "",
  "session_stop": "stopped session latency\n",
  "server_exit": 0,
  "owned_pids": [
    1870970,
    1871028
  ],
  "ps_exit": 1,
  "ps_output": "    PID    PPID STAT COMMAND\n",
  "socket_path": "/proc/1662459/fd/6/herdr/sessions/latency/herdr.sock",
  "canonical_socket_path": "/home/fujimogn/.cache/dev-task-temp/runs/4ee0a98a4a62c627b247712672dc193c/work/b2/config/herdr/sessions/latency/herdr.sock",
  "tmp_socket_dir": null
}
```

The private server exited with 0 and ps found neither owned PID. No client renderer was attached. No cleanup request targeted a user endpoint. Proc-fd aliases need no filesystem deletion and disappear when the controller exits.

## Verification and retained artifacts

The verification-before-completion workflow was used. These checks ran after A/B with the pinned toolchain and unchanged source. Commander note note_lat_tmp explicitly permitted normal full-suite /tmp fixtures; A/B fixtures and data stayed in the managed ext4 run.

```json
[
  {
    "name": "fmt",
    "command": [
      "cargo",
      "fmt",
      "--all",
      "--check"
    ],
    "exit": 0
  },
  {
    "name": "clippy",
    "command": [
      "cargo",
      "clippy",
      "--locked",
      "--all-targets",
      "-j",
      "2",
      "--",
      "-D",
      "warnings"
    ],
    "exit": 0
  },
  {
    "name": "test",
    "command": [
      "cargo",
      "test",
      "--locked"
    ],
    "exit": 0
  },
  {
    "name": "release",
    "command": [
      "cargo",
      "build",
      "--release",
      "--locked",
      "-j",
      "2"
    ],
    "exit": 0
  },
  {
    "name": "help",
    "command": [
      "/home/fujimogn/.cache/dev-task-temp/runs/4ee0a98a4a62c627b247712672dc193c/work/target/release/herdr-lineup",
      "--help"
    ],
    "exit": 0
  }
]
```

A second parser matched all 15 embedded A rows to the original output, independently recalculated 30 median cells and 18 bounds from nine B rows, and confirmed source/binary identities. Fact-check artifact SHA256: c69828a3e5c2d1d7571c2dbf075635e32b5a70d606eb850358e8cc9a9c944d6c.

Full-test summaries:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.03s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.03s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 1.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.03s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out; finished in 1.00s
test result: ok. 446 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 9.69s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Directory-entry comparison of /tmp before/after verification:

```json
{
  "new_entry_count": 136,
  "missing_entry_count": 36,
  "new_tempfile_style_entries": [
    ".tmpXryrQp",
    ".tmpp7dYum",
    ".tmpqn2Fjy"
  ],
  "ownership": "not attributable from directory snapshots; no deletion"
}
```

Concurrent host activity changed /tmp. Three new tempfile-style directories cannot be attributed to this suite from directory snapshots alone; absence of suite-owned /tmp residue is therefore unconfirmed. None was deleted. This is not an atomic audit of unrelated host activity. No recursive cleanup was run. The controller retained data/build output for commander review, separate from plugin state or user settings.

Managed evidence root: `/home/fujimogn/.cache/dev-task-temp/runs/4ee0a98a4a62c627b247712672dc193c/work`.

| Artifact relative to evidence root | SHA256 |
| --- | --- |
| `build-env.json` | `a423b26b7ba88588880b78e1a6c79334471f657786d2a84a92667ee320ffd487` |
| `identity.json` | `85d04f1b3614295f0975efb82b437bd2383636712ba03630309f0e9f55919425` |
| `a-harness.py` | `8e55c00846514927fba1d69645a70a0c5098570ca92d35e7de0d488cdb88746b` |
| `a.stdout` | `135fbb2cd14732bf5f25bfc1a06c8226c93ffb66cbceebe81a661526eea695ce` |
| `a.stderr` | `6ba0486cd7d71400e8f708e6fd7036c4a56a3aa963448d580a2a29595cfe4f79` |
| `b-harness.py` | `68a725e22dbf471de2d75e2b3174fb734e258568597bc9efa06d4ab1891bc294` |
| `b/cleanup.json` | `1a55909431286d93ea2a407f16ede74083d6a7cbb19213d370b21ec26af7e36c` |
| `b2-harness.py` | `07b2afd486b6049f2d572c96c85693fefc8f8d113cca3a69a78acb4937f00383` |
| `b2/events.json` | `68166a4d24677ff9eed2294a2c7346bfd363094ee99f01003abd0d478fce7af1` |
| `b2/renewal.json` | `fd073474a1e71a23d958fec8092a9c662aba5f629ad4935c37f3da0a194e5f09` |
| `b2/doctor.txt` | `f90c427cac7b5d7ff054767ef725fdb1d19d41fca07df49b1abe6293918a9ce3` |
| `b2/cleanup.json` | `972d37f5d4fa541cdcc920f3241ed9b3c98e004b9cb4b637cdfe9a08ae09ccd1` |
| `verify.py` | `7903a60b0e9b7aae8317826313bfc539dfe3472524d9cc562b6659f4c6e26e7a` |
| `checks.json` | `eb2d8a4a2aa4e4cb34d65888fc9e7b4ad82fb19f6531772d98ec0d0b2d6b5db9` |
| `tmp-check.json` | `c664aed70bf488e2e36cfff233fa932a9f40d4cb8ba242d666884c8cae37486b` |
| `build.log` | `27ecbaa04cff3d1939bb85f601f9256f56667d414b1a0e13960ea1817957ec20` |
| `fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `clippy.log` | `7b3e97951537712353ead54cbcabd8295fce5dfbe7a5a48db4cc915e48c9bae5` |
| `test.log` | `e8607997ec9b0cc41d8cd2af10ea307bf1ab9e4f99dd0ffe52ee08b761778752` |
| `release.log` | `6af795cac580b85a0d569f4ea28394820d327abaeb7294f6a39d56c531ad4eae` |
| `help.log` | `b63d634987a86e1f90edb2bd16b2e2bd4763a279b43efbef32cccbc26b187eb6` |

The successful managed run and the initial failed run are retained; final apparent sizes are reported with dust in the Formation candidate report. No managed payload is reclaimed by this lane.

## Limits and suggested follow-up wording

No real provider process, authenticated account, populated provider-history discovery, statusLine update, account-worker latency, macOS, renderer, user session, crash recovery, stalled reader/socket, deferral, degraded TTL expiry or exact renewal send time was measured. Empty-home release reader work in B is not representative of populated provider homes. Three repeats per phase/size are descriptive only. B changes lineup_state; context/quota tokens have additional source update and availability delays. This record changes no runtime behavior and defines no freshness/load acceptance contract.

Suggested TODO/coverage wording only (those files are unchanged):

- **N5:** Linux cold-refresh stages at fixed 80e11d1 and synthetic request-to-stored-state-token intervals recorded here (private Herdr 0.9.5, release publisher, 10 s post-cycle wait; three repeats/phase). Provider event timestamps, rendering, macOS and tails remain unmeasured. Keep conditional.
- **L6:** Historical 5 s wording is superseded by the implemented 10,000 ms default and 30,000 ms TTL at 80e11d1; idle polling remains 100 ms. This record separates refresh waiting, overlapping stage samples and stored-token observations. Freshness/load acceptance and event-to-render evidence remain open; do not infer completion from the interval change.
- **TODO:** Retain follow-up for actual provider-source event to rendered sidebar timing on Linux/macOS, exact renewal/expiry during degraded cycles, and an agreed freshness/load contract; link this record as limited Linux synthetic evidence.
