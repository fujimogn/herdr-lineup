# Live bounded-reader observations (2026-11-04)

Status: measurement record only. This is a short observation of one existing
Lineup daemon, not an isolation implementation, cancellation guarantee, SLO,
release, or acceptance decision.

## Scope and authorization

Formation assignment `asg_stuck_worker_l5` authorized only this document.
The method was submitted as BLOCKED before editing or measuring. Commander
notes `note_m2c_method` and `note_m2c_prov` approved the live observation and
the explicitly inferred provenance described below.

The approved target is one already running Lineup daemon for the user's
`main` Herdr session, anonymously labeled `main-lineup-A`. There was one
doctor availability preflight, followed by a planned 121 calls at five-second
intervals over ten minutes. No new daemon or synthetic stalled worker was
started.

## Environment and provenance

- Host: Linux `7.3.1-1-cachyos`, x86_64 GNU/Linux; temporary storage: ext4.
- Documentation/verification worktree base:
  `bd47305c7fa901b852d076390f4ac57153482e67`, the local
  `origin/develop` tip at worktree creation. This is **not** the live binary's
  proven source commit.
- Primary live binary identity (SHA-256):
  `ada64ead33b9ec56199a2c714a6d87fa02f2b9fef0e31f9ef3d6de38c1a03f52`.
  The running executable and the existing canonical release binary both
  matched this hash; size: 3,255,792 bytes.
- Inferred source commit:
  `2530402e34e645506fa048bc60c885615aa386d1`. Binary mtime
  `1792824253572430246` ns corresponds to 2026-10-24 15:44:13 JST.
  The canonical HEAD reflog stays at that commit from 10:28:54 until the next
  recorded change at 19:52:37 that day. **Whether the worktree was clean at
  build time is unknown.** Neither package version nor Cargo fingerprints
  prove source identity. The commander explicitly approved this inference;
  no reproducible-build equivalence is claimed.
- Both preflight and observation execute `/proc/8032/exe doctor`, the same
  existing binary as the target daemon. This avoids rebuilding or replacing
  the running executable. PID 8032 and its Linux process start token are
  acquisition details, not a reusable command target for a later run.
- Rust for subsequent worktree checks: `1.99.1 (58cf8f9c3 2026-10-13)`,
  matching `rust-toolchain.toml`.
- The current config file exists and has no explicit `refresh_interval_ms`
  or `reader_timeout_ms`. The inferred source defaults are 5,000 ms and
  2,000 ms. Current file contents do not establish what was loaded when the
  daemon started. The newer documentation base defaults to 10,000 ms;
  it must not be substituted for live provenance.

## Access boundary and procedure

1. Identify the existing daemon from `/proc`, reading only its executable,
   command name, start token and the three required Herdr/plugin path
   environment values. Do not print unrelated environment variables.
2. Compare the running binary hash with the canonical binary; inspect Cargo
   fingerprint metadata and the canonical HEAD reflog for provenance.
3. Use a dedicated worktree from `origin/develop`. `worktree-init --check`
   and its normal invocation both returned `identity-only` with
   `config_distribution_denied`; no user configuration was distributed.
4. Create the explicitly approved private root
   `/home/fujimogn/.cache/herdr-lineup-l5` (mode 0700). Use
   `dev-task-temp run --root <root> --task-id <task> --bytes 5000000000 --inodes 50000 -- python3 -`.
   Two initial launcher failures (missing root, then mode 0755) occurred before
   any doctor child was launched. Setting this newly created task root to 0700
   resolved them. No measurement data was placed in `/tmp`. A later explicit
   commander exception for ordinary test temporary sockets is described in
   the verification section; it did not change the measurement procedure.
5. Run doctor once to establish Reader diagnostic availability. Only then run
   the approved ten-minute series. Use monotonic scheduled start times,
   not a five-second sleep after each command. Record actual UTC, monotonic
   start, lateness, duration and exit code.
6. Before each call, verify the process start token and binary hash. Stop
   sampling on an identity mismatch. Invoke only `doctor`; its normal
   read-only Herdr protocol probe and Lineup control health request sample
   diagnostics without scheduling reader/account queries.
7. Keep stdout/stderr only in the collector's process memory. Persist the
   fixed Reader diagnostic lines, an anonymous daemon label and coarse health
   classification. Do not persist raw wire identity, nonce, session contents,
   transcript paths, credentials, unrelated output or stderr text.
8. Wait until observation ends before this lane runs Cargo checks, so those
   checks do not add load during its own observation. Other users, panes,
   lanes, filesystem activity and host scheduling remain uncontrolled.

At the inferred source, `doctor::run` loads config read-only, counts/stats
statusline entries, reads saved quota diagnostic categories, probes the Herdr
JSON API, and requests existing Lineup control health. Its normal local
read-only diagnostic access is retained; unrelated output is discarded.
The collector does not issue metadata writes, attach a Herdr client, stop or
restart Herdr, call daemon `ensure`/`stop`, modify user settings, inject
stalls, or add reader/account queries. Config was read to establish whether
the two timing overrides were present, without retaining its other values.

## Interpretation rules

The five local workers are Claude, Agy, Codex, Grok and Kimi. Hermes has no
bounded worker and is excluded from frequency denominators. An available
Kimi slot with zero attempts means no observed calls, not successful reads.

- `delta busy / delta attempts`: observed BUSY rejection proportion in this
  interval, not the number of independent stalled workers.
- `delta deadline / delta attempts`: caller deadline proportion, including
  zero-budget exits; a deadline does not identify a blocked OS read.
- `timed_out_worker_active` true samples / available samples: sampled retained
  occupancy, not event frequency or exact duration.
- Consecutive active samples establish only a sampled observed span. Short
  stalls between five-second samples can be missed; one worker can produce
  multiple BUSY rejections.
- `completed` counts accepted results, including missing usage. It is not a
  successful context-read count.
- Provider counters are advisory, saturating, cumulative and nontransactional.
  Check decreases, saturation, skips, availability and process continuity
  before computing deltas. Missing/incomplete/contended/poisoned diagnostics
  are unavailable, never measured zeros. Subtracting cumulative maxima does
  not give an interval maximum.

The collector's coarse `not_healthy_or_unavailable` classification means the
healthy summary text was absent; stderr text and the specific failure cause
are deliberately not retained. Reader availability is assessed independently
of overall doctor health.

## Collector used for the observation

The following is the exact Python child supplied through stdin to
`dev-task-temp run` for the observation. It captures the already verified
process identity before the series; a later run must first identify its own
target and receive authorization for it.

```python
import os,hashlib,subprocess,json,time,datetime
from pathlib import Path
p=Path('/proc/8032')
expected='ada64ead33b9ec56199a2c714a6d87fa02f2b9fef0e31f9ef3d6de38c1a03f52'
start_identity=(p/'stat').read_text().split(') ',1)[1].split()[19]
raw=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v)
env=os.environ.copy()
for k in ('HERDR_SOCKET_PATH','HERDR_PLUGIN_STATE_DIR','HERDR_PLUGIN_CONFIG_DIR'): env[k]=raw[k.encode()].decode()
out=Path(os.environ['DEV_TASK_TEMP_DIR'])/'samples.jsonl'
origin=time.monotonic_ns()
print('samples_path='+str(out),flush=True)
for i in range(121):
 target=origin+i*5_000_000_000
 delay=(target-time.monotonic_ns())/1e9
 if delay>0: time.sleep(delay)
 ns=time.monotonic_ns()
 row={'index':i,'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'monotonic_ns':ns,'scheduled_offset_ns':i*5_000_000_000,'lateness_ns':ns-target,'daemon':'main-lineup-A'}
 same=(p/'stat').read_text().split(') ',1)[1].split()[19]==start_identity and hashlib.sha256((p/'exe').read_bytes()).hexdigest()==expected
 row['identity_matches']=same
 if not same:
  row['exit_code']=None;row['health']='identity_changed';row['reader_lines']=[]
 else:
  try:
   r=subprocess.run(['/proc/8032/exe','doctor'],env=env,capture_output=True,text=True,timeout=15)
   row['exit_code']=r.returncode
   row['reader_lines']=[s for s in r.stdout.splitlines() if s.startswith('Reader diagnostics:') or s.startswith(tuple('Reader '+k+':' for k in ('claude','agy','codex','grok','hermes','kimi')))]
   row['health']='healthy' if 'daemon/control/lock healthy' in r.stdout else 'not_healthy_or_unavailable'
   row['stderr_present']=bool(r.stderr)
  except subprocess.TimeoutExpired:
   row['exit_code']=None;row['reader_lines']=[];row['health']='doctor_timeout'
 row['duration_ns']=time.monotonic_ns()-ns
 with out.open('a') as f: f.write(json.dumps(row)+'\n')
 if i%12==0 or not same: print(json.dumps({k:row[k] for k in ('index','utc','health','identity_matches')}),flush=True)
 if not same: break
print('complete',flush=True)
```



## Results for the observed ten-minute window

Actual first/last starts: 2026-11-04T11:11:32.990253+00:00 through 2026-11-04T11:21:32.993701+00:00
(600.003440502 seconds by monotonic clock). All 121 planned observations
were collected, with seven Reader lines each and no unavailable provider
entries. All per-call process identity/hash checks matched.

Doctor exited zero in 119 calls and nonzero in
2 calls. Reader diagnostics remained available
in those nonzero calls. Their specific health failure cause was not retained.
Maximum start lateness: 6023448 ns; maximum
collector call duration: 189395385 ns. These are
observed maxima, not latency guarantees.

| Provider | Attempt delta | BUSY delta / attempts | Deadline delta / attempts | Completed delta | Active samples | Timed-out active samples | Shutdown-active samples |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| claude | 442 | 0/442 (0.0000%) | 0/442 (0.0000%) | 442 | 0/121 | 0/121 | 0/121 |
| agy | 170 | 0/170 (0.0000%) | 0/170 (0.0000%) | 170 | 1/121 | 0/121 | 0/121 |
| codex | 1880 | 0/1880 (0.0000%) | 3/1880 (0.1596%) | 1877 | 2/121 | 0/121 | 0/121 |
| grok | 433 | 0/433 (0.0000%) | 0/433 (0.0000%) | 433 | 0/121 | 0/121 | 0/121 |
| hermes | 0 | unmeasured (0 attempts) | unmeasured (0 attempts) | 0 | 0/121 | 0/121 | 0/121 |
| kimi | 0 | unmeasured (0 attempts) | unmeasured (0 attempts) | 0 | 0/121 | 0/121 | 0/121 |

Hermes rows are shown only for auditability and excluded from worker
frequency interpretation. Kimi had no observed bounded calls; its zero gauges
do not establish successful reads or stall-free operation under activity.

All available counters were nonsaturated and nondecreasing; sampling skips
were zero throughout. Spawn failures, worker failures, shutdown ends and
invalid-call counters had zero interval increments for all providers.
Ordinary active workers were sampled once for Agy (index 103) and twice for
Codex (indices 64 and 97). Each was surrounded by inactive samples; no pair
of consecutive samples was active. No timed-out or shutdown-retained worker
sample was observed. Five-second sampling cannot bound the duration of an
unobserved event. Baselines include historical deadlines; only endpoint
differences above belong to this window. The two nonzero doctor exits were
at indices 8 and 9 (exit 1); these were not Reader diagnostic gaps.

This is the frequency of recorded indicators within one observed ten-minute
window. It does not establish long-term OS-stuck-worker frequency, root cause,
or whether process isolation is warranted. A resource/privacy/restart
contract remains a prerequisite for any isolation work.

## Retained primary artifacts

The task root is `/home/fujimogn/.cache/herdr-lineup-l5`. Each run reserves
at most 5,000,000,000 bytes with `dev-task-temp`; no manual cleanup is used.
Run work directories under that root are:

| Run | Work artifact |
| --- | --- |
| `25675886f4a102e12233a13894331893` | `work/preflight.json` |
| `e00e46c15c8cb575a51f5dcc0450578f` | `work/samples.jsonl` (original selected diagnostic lines) |
| `4738c031cd8e6f81dc691303c456a3bc` | `work/analysis.json` |
| `0d3ec25214862364a0407eaf682ed9a7` | `work/check-*.log`, `work/results.json` |
| `c1de4ce3d4b7b09d922997519289f111` | Release build/help logs and results |

These are retained for commander fact checking. Independent review checks
the committed record and repository consistency; external primary evidence
requires commander confirmation. The final Formation report carries residue
sizes and the fixed documentation commit. Build output remains inside the
dedicated worktree's `target`, separate from the live daemon's binary.

## Selected raw data and transcription

Original selected-line JSONL SHA-256:

`30c741b39972765a1d99910d90ba71bf9cbacaccbe1e332415b7e06026351f34`.

The compact rows below transcribe every printed numeric/boolean Reader field
without rounding. Original selected lines are retained in the task run for
commander fact checking. Non-Reader stdout/stderr and protocol identity were
not retained. Provider order in each `p` array is:
`claude, agy, codex, grok, hermes, kimi`. Each provider vector contains:

```text
attempts, busy_rejections, deadline_ends, shutdown_ends, spawn_failures,
worker_failures, completed, invalid, caller_wait_count, caller_wait_total_ns,
caller_wait_max_ns, sampling_skips, worker_active, timed_out_worker_active,
shutdown_worker_active
```

A printed `wait unmeasured` becomes `[0,null,null]` for the three wait fields:
total/max were not printed and are not invented. Integers are counts except
explicit nanosecond fields. `i` is index, `t` UTC, `m` monotonic start ns,
`l` lateness ns, `d` duration ns, `rc` exit code, `h` is `ok` for the healthy
summary and `other` otherwise; `id` is the per-call identity check. All provider
entries were available. Their header matches the preflight header below.

### Availability preflight (excluded from interval deltas)

```json
{
  "utc": "2026-11-04T11:11:12.079439+00:00",
  "monotonic_ns": 8153191246170,
  "duration_ns": 85544820,
  "exit_code": 0,
  "daemon": "main-lineup-A",
  "binary_sha256": "ada64ead33b9ec56199a2c714a6d87fa02f2b9fef0e31f9ef3d6de38c1a03f52",
  "reader_lines": [
    "Reader diagnostics: live best-effort; sampled during control request; providers nontransactional; refresh metrics describe completed attempts; counters advisory and saturating; zero attempts means no observed bounded calls",
    "Reader claude: attempts 6894 count; busy 0 count; deadline 3 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 6891 count; invalid 0 count; wait count 6894; total 5533085937 ns; max 173540119 ns; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false",
    "Reader agy: attempts 2760 count; busy 0 count; deadline 1 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 2759 count; invalid 0 count; wait count 2760; total 1969836190 ns; max 83096357 ns; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false",
    "Reader codex: attempts 32844 count; busy 0 count; deadline 18 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 32826 count; invalid 0 count; wait count 32844; total 64651525928 ns; max 274731171 ns; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false",
    "Reader grok: attempts 6373 count; busy 0 count; deadline 2 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 6371 count; invalid 0 count; wait count 6373; total 9432655491 ns; max 206261935 ns; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false",
    "Reader hermes: attempts 0 count; busy 0 count; deadline 0 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 0 count; invalid 0 count; wait unmeasured; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false; no bounded worker; empty slot is not a read measurement",
    "Reader kimi: attempts 0 count; busy 0 count; deadline 0 count; shutdown 0 count; spawn failures 0 count; worker failures 0 count; completed 0 count; invalid 0 count; wait unmeasured; sampling skips 0 count; worker active false; timed-out worker active false; shutdown worker active false"
  ],
  "diagnostics_available": true,
  "health": "healthy",
  "stderr_present": false
}
```

### Observation samples

```jsonl
{"i":0,"t":"2026-11-04T11:11:32.990253+00:00","m":8174102064027,"l":69561,"d":49085869,"rc":0,"h":"ok","id":true,"p":[[6904,0,3,0,0,0,6901,0,6904,5694530566,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32863,0,18,0,0,0,32845,0,32863,64867055882,274731171,0,false,false,false],[6383,0,2,0,0,0,6381,0,6383,9504839443,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":1,"t":"2026-11-04T11:11:37.990275+00:00","m":8179102077947,"l":83481,"d":22598808,"rc":0,"h":"ok","id":true,"p":[[6906,0,3,0,0,0,6903,0,6906,5696357477,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32869,0,18,0,0,0,32851,0,32869,64937202891,274731171,0,false,false,false],[6385,0,2,0,0,0,6383,0,6385,9508975440,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":2,"t":"2026-11-04T11:11:42.990267+00:00","m":8184102071531,"l":77065,"d":75506917,"rc":0,"h":"ok","id":true,"p":[[6910,0,3,0,0,0,6907,0,6910,5701193833,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32885,0,18,0,0,0,32867,0,32885,64993854456,274731171,0,false,false,false],[6387,0,2,0,0,0,6385,0,6387,9511510347,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":3,"t":"2026-11-04T11:11:47.990251+00:00","m":8189102053745,"l":59279,"d":24611679,"rc":0,"h":"ok","id":true,"p":[[6912,0,3,0,0,0,6909,0,6912,5701559288,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32891,0,18,0,0,0,32873,0,32891,65052979343,274731171,0,false,false,false],[6390,0,2,0,0,0,6388,0,6390,9524081879,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":4,"t":"2026-11-04T11:11:52.990541+00:00","m":8194102338478,"l":344012,"d":43094009,"rc":0,"h":"ok","id":true,"p":[[6913,0,3,0,0,0,6910,0,6913,5702238409,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32903,0,18,0,0,0,32885,0,32903,65102805843,274731171,0,false,false,false],[6391,0,2,0,0,0,6389,0,6391,9526945391,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":5,"t":"2026-11-04T11:11:57.990367+00:00","m":8199102166256,"l":171790,"d":26720777,"rc":0,"h":"ok","id":true,"p":[[6917,0,3,0,0,0,6914,0,6917,5708544628,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32916,0,18,0,0,0,32898,0,32916,65155413135,274731171,0,false,false,false],[6393,0,2,0,0,0,6391,0,6393,9531587442,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":6,"t":"2026-11-04T11:12:02.991229+00:00","m":8204103033530,"l":1039064,"d":19821407,"rc":0,"h":"ok","id":true,"p":[[6922,0,3,0,0,0,6919,0,6922,5716110810,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32943,0,18,0,0,0,32925,0,32943,65297872483,274731171,0,false,false,false],[6398,0,2,0,0,0,6396,0,6398,9539839207,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":7,"t":"2026-11-04T11:12:07.990270+00:00","m":8209102070009,"l":75543,"d":8290879,"rc":0,"h":"ok","id":true,"p":[[6924,0,3,0,0,0,6921,0,6924,5719312221,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32947,0,18,0,0,0,32929,0,32947,65306286383,274731171,0,false,false,false],[6399,0,2,0,0,0,6397,0,6399,9541544366,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":8,"t":"2026-11-04T11:12:12.993424+00:00","m":8214105229253,"l":3234787,"d":25251747,"rc":1,"h":"other","id":true,"p":[[6933,0,3,0,0,0,6930,0,6933,5728263227,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32978,0,18,0,0,0,32960,0,32978,65377714356,274731171,0,false,false,false],[6405,0,2,0,0,0,6403,0,6405,9558520488,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":9,"t":"2026-11-04T11:12:17.990244+00:00","m":8219102047477,"l":53011,"d":23839585,"rc":1,"h":"other","id":true,"p":[[6933,0,3,0,0,0,6930,0,6933,5728263227,173540119,0,false,false,false],[2762,0,1,0,0,0,2761,0,2762,1973395114,83096357,0,false,false,false],[32978,0,18,0,0,0,32960,0,32978,65377714356,274731171,0,false,false,false],[6405,0,2,0,0,0,6403,0,6405,9558520488,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":10,"t":"2026-11-04T11:12:22.991275+00:00","m":8224103062784,"l":1068318,"d":9730739,"rc":0,"h":"ok","id":true,"p":[[6934,0,3,0,0,0,6931,0,6934,5729085495,173540119,0,false,false,false],[2766,0,1,0,0,0,2765,0,2766,2021944790,83096357,0,false,false,false],[33003,0,19,0,0,0,32984,0,33003,66006422719,531191516,0,false,false,false],[6409,0,2,0,0,0,6407,0,6409,9574188203,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":11,"t":"2026-11-04T11:12:27.990236+00:00","m":8229102041820,"l":47354,"d":8785272,"rc":0,"h":"ok","id":true,"p":[[6934,0,3,0,0,0,6931,0,6934,5729085495,173540119,0,false,false,false],[2770,0,1,0,0,0,2769,0,2770,2024120413,83096357,0,false,false,false],[33016,0,19,0,0,0,32997,0,33016,66037308997,531191516,0,false,false,false],[6411,0,2,0,0,0,6409,0,6411,9575335305,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":12,"t":"2026-11-04T11:12:32.990257+00:00","m":8234102062561,"l":68095,"d":9643037,"rc":0,"h":"ok","id":true,"p":[[6940,0,3,0,0,0,6937,0,6940,5738408706,173540119,0,false,false,false],[2770,0,1,0,0,0,2769,0,2770,2024120413,83096357,0,false,false,false],[33047,0,19,0,0,0,33028,0,33047,66121134116,531191516,0,false,false,false],[6418,0,2,0,0,0,6416,0,6418,9582055476,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":13,"t":"2026-11-04T11:12:37.990254+00:00","m":8239102059340,"l":64874,"d":11736302,"rc":0,"h":"ok","id":true,"p":[[6950,0,3,0,0,0,6947,0,6950,5743802286,173540119,0,false,false,false],[2774,0,1,0,0,0,2773,0,2774,2025665597,83096357,0,false,false,false],[33091,0,19,0,0,0,33072,0,33091,66205087397,531191516,0,false,false,false],[6427,0,2,0,0,0,6425,0,6427,9591186786,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":14,"t":"2026-11-04T11:12:42.990305+00:00","m":8244102097094,"l":102628,"d":12790323,"rc":0,"h":"ok","id":true,"p":[[6950,0,3,0,0,0,6947,0,6950,5743802286,173540119,0,false,false,false],[2774,0,1,0,0,0,2773,0,2774,2025665597,83096357,0,false,false,false],[33091,0,19,0,0,0,33072,0,33091,66205087397,531191516,0,false,false,false],[6427,0,2,0,0,0,6425,0,6427,9591186786,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":15,"t":"2026-11-04T11:12:47.992537+00:00","m":8249104341784,"l":2347318,"d":34196650,"rc":0,"h":"ok","id":true,"p":[[6950,0,3,0,0,0,6947,0,6950,5743802286,173540119,0,false,false,false],[2778,0,1,0,0,0,2777,0,2778,2033004275,83096357,0,false,false,false],[33104,0,19,0,0,0,33085,0,33104,66260304363,531191516,0,false,false,false],[6430,0,2,0,0,0,6428,0,6430,9594809222,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":16,"t":"2026-11-04T11:12:52.994683+00:00","m":8254106481836,"l":4487370,"d":23218048,"rc":0,"h":"ok","id":true,"p":[[6950,0,3,0,0,0,6947,0,6950,5743802286,173540119,0,false,false,false],[2778,0,1,0,0,0,2777,0,2778,2033004275,83096357,0,false,false,false],[33104,0,19,0,0,0,33085,0,33104,66260304363,531191516,0,false,false,false],[6430,0,2,0,0,0,6428,0,6430,9594809222,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":17,"t":"2026-11-04T11:12:57.990241+00:00","m":8259102047515,"l":53049,"d":21366235,"rc":0,"h":"ok","id":true,"p":[[6957,0,3,0,0,0,6954,0,6957,5750700286,173540119,0,false,false,false],[2780,0,1,0,0,0,2779,0,2780,2035745963,83096357,0,false,false,false],[33124,0,19,0,0,0,33105,0,33124,66372950623,531191516,0,false,false,false],[6435,0,2,0,0,0,6433,0,6435,9604688244,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":18,"t":"2026-11-04T11:13:02.991548+00:00","m":8264103352852,"l":1358386,"d":20583462,"rc":0,"h":"ok","id":true,"p":[[6957,0,3,0,0,0,6954,0,6957,5750700286,173540119,0,false,false,false],[2780,0,1,0,0,0,2779,0,2780,2035745963,83096357,0,false,false,false],[33125,0,19,0,0,0,33106,0,33125,66377018959,531191516,0,false,false,false],[6437,0,2,0,0,0,6435,0,6437,9607023446,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":19,"t":"2026-11-04T11:13:07.990972+00:00","m":8269102777253,"l":782787,"d":24497092,"rc":0,"h":"ok","id":true,"p":[[6964,0,3,0,0,0,6961,0,6964,5762347619,173540119,0,false,false,false],[2780,0,1,0,0,0,2779,0,2780,2035745963,83096357,0,false,false,false],[33139,0,19,0,0,0,33120,0,33139,66470764653,531191516,0,false,false,false],[6440,0,2,0,0,0,6438,0,6440,9615588796,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":20,"t":"2026-11-04T11:13:12.991233+00:00","m":8274103040768,"l":1046302,"d":18219541,"rc":0,"h":"ok","id":true,"p":[[6967,0,3,0,0,0,6964,0,6967,5766140174,173540119,0,false,false,false],[2780,0,1,0,0,0,2779,0,2780,2035745963,83096357,0,false,false,false],[33147,0,19,0,0,0,33128,0,33147,66504676302,531191516,0,false,false,false],[6445,0,2,0,0,0,6443,0,6445,9626822539,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":21,"t":"2026-11-04T11:13:17.990528+00:00","m":8279102333189,"l":338723,"d":17379128,"rc":0,"h":"ok","id":true,"p":[[6972,0,3,0,0,0,6969,0,6972,5771195406,173540119,0,false,false,false],[2784,0,1,0,0,0,2783,0,2784,2038927773,83096357,0,false,false,false],[33175,0,20,0,0,0,33155,0,33175,66584178926,531191516,0,false,false,false],[6449,0,2,0,0,0,6447,0,6449,9630082816,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":22,"t":"2026-11-04T11:13:22.994914+00:00","m":8284106710543,"l":4716077,"d":35574018,"rc":0,"h":"ok","id":true,"p":[[6979,0,3,0,0,0,6976,0,6979,5779790511,173540119,0,false,false,false],[2784,0,1,0,0,0,2783,0,2784,2038927773,83096357,0,false,false,false],[33192,0,20,0,0,0,33172,0,33192,66660317669,531191516,0,false,false,false],[6454,0,2,0,0,0,6452,0,6454,9636087065,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":23,"t":"2026-11-04T11:13:27.990340+00:00","m":8289102145673,"l":151207,"d":46191103,"rc":0,"h":"ok","id":true,"p":[[6979,0,3,0,0,0,6976,0,6979,5779790511,173540119,0,false,false,false],[2784,0,1,0,0,0,2783,0,2784,2038927773,83096357,0,false,false,false],[33192,0,20,0,0,0,33172,0,33192,66660317669,531191516,0,false,false,false],[6454,0,2,0,0,0,6452,0,6454,9636087065,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":24,"t":"2026-11-04T11:13:32.991531+00:00","m":8294103330563,"l":1336097,"d":132216078,"rc":0,"h":"ok","id":true,"p":[[6986,0,3,0,0,0,6983,0,6986,5786012469,173540119,0,false,false,false],[2788,0,1,0,0,0,2787,0,2788,2041090427,83096357,0,false,false,false],[33218,0,20,0,0,0,33198,0,33218,66802443618,531191516,0,false,false,false],[6463,0,2,0,0,0,6461,0,6463,9657208010,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":25,"t":"2026-11-04T11:13:37.990254+00:00","m":8299102057991,"l":63525,"d":11797902,"rc":0,"h":"ok","id":true,"p":[[6986,0,3,0,0,0,6983,0,6986,5786012469,173540119,0,false,false,false],[2788,0,1,0,0,0,2787,0,2788,2041090427,83096357,0,false,false,false],[33218,0,20,0,0,0,33198,0,33218,66802443618,531191516,0,false,false,false],[6463,0,2,0,0,0,6461,0,6463,9657208010,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":26,"t":"2026-11-04T11:13:42.990281+00:00","m":8304102077008,"l":82542,"d":10303632,"rc":0,"h":"ok","id":true,"p":[[6994,0,3,0,0,0,6991,0,6994,5795361674,173540119,0,false,false,false],[2792,0,1,0,0,0,2791,0,2792,2043727545,83096357,0,false,false,false],[33247,0,20,0,0,0,33227,0,33247,66927683025,531191516,0,false,false,false],[6472,0,2,0,0,0,6470,0,6472,9674049204,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":27,"t":"2026-11-04T11:13:47.990252+00:00","m":8309102058345,"l":63879,"d":7841921,"rc":0,"h":"ok","id":true,"p":[[7001,0,3,0,0,0,6998,0,7001,5797409986,173540119,0,false,false,false],[2792,0,1,0,0,0,2791,0,2792,2043727545,83096357,0,false,false,false],[33263,0,20,0,0,0,33243,0,33263,66956422167,531191516,0,false,false,false],[6477,0,2,0,0,0,6475,0,6477,9680365896,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":28,"t":"2026-11-04T11:13:52.991229+00:00","m":8314103034427,"l":1039961,"d":13106069,"rc":0,"h":"ok","id":true,"p":[[7002,0,3,0,0,0,6999,0,7002,5797580624,173540119,0,false,false,false],[2796,0,1,0,0,0,2795,0,2796,2045944170,83096357,0,false,false,false],[33290,0,20,0,0,0,33270,0,33290,67006145575,531191516,0,false,false,false],[6481,0,2,0,0,0,6479,0,6481,9684698521,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":29,"t":"2026-11-04T11:13:57.990223+00:00","m":8319102029925,"l":35459,"d":6292399,"rc":0,"h":"ok","id":true,"p":[[7012,0,3,0,0,0,7009,0,7012,5803793536,173540119,0,false,false,false],[2800,0,1,0,0,0,2799,0,2800,2048442538,83096357,0,false,false,false],[33334,0,20,0,0,0,33314,0,33334,67085619408,531191516,0,false,false,false],[6490,0,2,0,0,0,6488,0,6490,9692832941,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":30,"t":"2026-11-04T11:14:02.990237+00:00","m":8324102043040,"l":48574,"d":6179141,"rc":0,"h":"ok","id":true,"p":[[7022,0,3,0,0,0,7019,0,7022,5806030177,173540119,0,false,false,false],[2804,0,1,0,0,0,2803,0,2804,2049262914,83096357,0,false,false,false],[33378,0,20,0,0,0,33358,0,33378,67121628342,531191516,0,false,false,false],[6499,0,2,0,0,0,6497,0,6499,9697932515,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":31,"t":"2026-11-04T11:14:07.994378+00:00","m":8329106166299,"l":4171833,"d":23377930,"rc":0,"h":"ok","id":true,"p":[[7022,0,3,0,0,0,7019,0,7022,5806030177,173540119,0,false,false,false],[2804,0,1,0,0,0,2803,0,2804,2049262914,83096357,0,false,false,false],[33378,0,20,0,0,0,33358,0,33378,67121628342,531191516,0,false,false,false],[6499,0,2,0,0,0,6497,0,6499,9697932515,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":32,"t":"2026-11-04T11:14:12.990282+00:00","m":8334102088097,"l":93631,"d":6646011,"rc":0,"h":"ok","id":true,"p":[[7030,0,3,0,0,0,7027,0,7030,5809499274,173540119,0,false,false,false],[2808,0,1,0,0,0,2807,0,2808,2051376438,83096357,0,false,false,false],[33405,0,20,0,0,0,33385,0,33405,67223099713,531191516,0,false,false,false],[6508,0,2,0,0,0,6506,0,6508,9708765869,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":33,"t":"2026-11-04T11:14:17.990625+00:00","m":8339102431226,"l":436760,"d":7192318,"rc":0,"h":"ok","id":true,"p":[[7040,0,3,0,0,0,7037,0,7040,5810911700,173540119,0,false,false,false],[2812,0,1,0,0,0,2811,0,2812,2052079279,83096357,0,false,false,false],[33449,0,20,0,0,0,33429,0,33449,67265973662,531191516,0,false,false,false],[6517,0,2,0,0,0,6515,0,6517,9712840094,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":34,"t":"2026-11-04T11:14:22.990884+00:00","m":8344102689915,"l":695449,"d":10555468,"rc":0,"h":"ok","id":true,"p":[[7047,0,3,0,0,0,7044,0,7047,5817855786,173540119,0,false,false,false],[2812,0,1,0,0,0,2811,0,2812,2052079279,83096357,0,false,false,false],[33467,0,20,0,0,0,33447,0,33467,67326968821,531191516,0,false,false,false],[6522,0,2,0,0,0,6520,0,6522,9717846573,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":35,"t":"2026-11-04T11:14:27.990233+00:00","m":8349102039965,"l":45499,"d":6850153,"rc":0,"h":"ok","id":true,"p":[[7048,0,3,0,0,0,7045,0,7048,5818043444,173540119,0,false,false,false],[2816,0,1,0,0,0,2815,0,2816,2054202993,83096357,0,false,false,false],[33493,0,20,0,0,0,33473,0,33493,67392902657,531191516,0,false,false,false],[6526,0,2,0,0,0,6524,0,6526,9725217564,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":36,"t":"2026-11-04T11:14:32.990243+00:00","m":8354102047577,"l":53111,"d":6715122,"rc":0,"h":"ok","id":true,"p":[[7058,0,3,0,0,0,7055,0,7058,5820221220,173540119,0,false,false,false],[2820,0,1,0,0,0,2819,0,2820,2055071260,83096357,0,false,false,false],[33537,0,20,0,0,0,33517,0,33537,67460531165,531191516,0,false,false,false],[6535,0,2,0,0,0,6533,0,6535,9730418824,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":37,"t":"2026-11-04T11:14:37.990290+00:00","m":8359102093335,"l":98869,"d":8886193,"rc":0,"h":"ok","id":true,"p":[[7068,0,3,0,0,0,7065,0,7068,5822629786,173540119,0,false,false,false],[2824,0,1,0,0,0,2823,0,2824,2055773839,83096357,0,false,false,false],[33581,0,20,0,0,0,33561,0,33581,67501109224,531191516,0,false,false,false],[6544,0,2,0,0,0,6542,0,6544,9736744268,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":38,"t":"2026-11-04T11:14:42.990229+00:00","m":8364102033373,"l":38907,"d":7814720,"rc":0,"h":"ok","id":true,"p":[[7072,0,3,0,0,0,7069,0,7072,5824231397,173540119,0,false,false,false],[2824,0,1,0,0,0,2823,0,2824,2055773839,83096357,0,false,false,false],[33594,0,20,0,0,0,33574,0,33594,67519104116,531191516,0,false,false,false],[6549,0,2,0,0,0,6547,0,6549,9740994750,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":39,"t":"2026-11-04T11:14:47.990224+00:00","m":8369102031285,"l":36819,"d":5963784,"rc":0,"h":"ok","id":true,"p":[[7078,0,3,0,0,0,7075,0,7078,5827553004,173540119,0,false,false,false],[2828,0,1,0,0,0,2827,0,2828,2056681880,83096357,0,false,false,false],[33625,0,20,0,0,0,33605,0,33625,67575971041,531191516,0,false,false,false],[6553,0,2,0,0,0,6551,0,6553,9744478474,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":40,"t":"2026-11-04T11:14:52.990274+00:00","m":8374102081784,"l":87318,"d":6317934,"rc":0,"h":"ok","id":true,"p":[[7088,0,3,0,0,0,7085,0,7088,5828926821,173540119,0,false,false,false],[2832,0,1,0,0,0,2831,0,2832,2057188904,83096357,0,false,false,false],[33669,0,20,0,0,0,33649,0,33669,67616464409,531191516,0,false,false,false],[6562,0,2,0,0,0,6560,0,6562,9747821279,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":41,"t":"2026-11-04T11:14:57.990232+00:00","m":8379102037132,"l":42666,"d":6458226,"rc":0,"h":"ok","id":true,"p":[[7098,0,3,0,0,0,7095,0,7098,5830465461,173540119,0,false,false,false],[2836,0,1,0,0,0,2835,0,2836,2058184238,83096357,0,false,false,false],[33713,0,20,0,0,0,33693,0,33713,67670755475,531191516,0,false,false,false],[6571,0,2,0,0,0,6569,0,6571,9752136215,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":42,"t":"2026-11-04T11:15:02.990230+00:00","m":8384102036224,"l":41758,"d":6761692,"rc":0,"h":"ok","id":true,"p":[[7108,0,3,0,0,0,7105,0,7108,5832592253,173540119,0,false,false,false],[2840,0,1,0,0,0,2839,0,2840,2059172278,83096357,0,false,false,false],[33757,0,20,0,0,0,33737,0,33757,67723060908,531191516,0,false,false,false],[6580,0,2,0,0,0,6578,0,6580,9759220024,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":43,"t":"2026-11-04T11:15:07.990220+00:00","m":8389102027428,"l":32962,"d":6276208,"rc":0,"h":"ok","id":true,"p":[[7115,0,3,0,0,0,7112,0,7115,5834064604,173540119,0,false,false,false],[2840,0,1,0,0,0,2839,0,2840,2059172278,83096357,0,false,false,false],[33772,0,20,0,0,0,33752,0,33772,67746720178,531191516,0,false,false,false],[6585,0,2,0,0,0,6583,0,6585,9761838403,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":44,"t":"2026-11-04T11:15:12.990394+00:00","m":8394102200805,"l":206339,"d":7191634,"rc":0,"h":"ok","id":true,"p":[[7118,0,3,0,0,0,7115,0,7118,5834780588,173540119,0,false,false,false],[2844,0,1,0,0,0,2843,0,2844,2060193911,83096357,0,false,false,false],[33801,0,20,0,0,0,33781,0,33801,67774757750,531191516,0,false,false,false],[6589,0,2,0,0,0,6587,0,6589,9764001172,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":45,"t":"2026-11-04T11:15:17.990274+00:00","m":8399102078973,"l":84507,"d":5819909,"rc":0,"h":"ok","id":true,"p":[[7128,0,3,0,0,0,7125,0,7128,5836485332,173540119,0,false,false,false],[2848,0,1,0,0,0,2847,0,2848,2060884487,83096357,0,false,false,false],[33845,0,20,0,0,0,33825,0,33845,67831559693,531191516,0,false,false,false],[6598,0,2,0,0,0,6596,0,6598,9767866469,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":46,"t":"2026-11-04T11:15:22.990226+00:00","m":8404102032726,"l":38260,"d":7645309,"rc":0,"h":"ok","id":true,"p":[[7138,0,3,0,0,0,7135,0,7138,5841434580,173540119,0,false,false,false],[2852,0,1,0,0,0,2851,0,2852,2061610157,83096357,0,false,false,false],[33889,0,20,0,0,0,33869,0,33889,67915341802,531191516,0,false,false,false],[6607,0,2,0,0,0,6605,0,6607,9777191307,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":47,"t":"2026-11-04T11:15:27.990233+00:00","m":8409102039869,"l":45403,"d":5902044,"rc":0,"h":"ok","id":true,"p":[[7141,0,3,0,0,0,7138,0,7141,5841936644,173540119,0,false,false,false],[2852,0,1,0,0,0,2851,0,2852,2061610157,83096357,0,false,false,false],[33899,0,20,0,0,0,33879,0,33899,67929323273,531191516,0,false,false,false],[6612,0,2,0,0,0,6610,0,6612,9779598000,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":48,"t":"2026-11-04T11:15:32.990281+00:00","m":8414102087424,"l":92958,"d":5659433,"rc":0,"h":"ok","id":true,"p":[[7148,0,3,0,0,0,7145,0,7148,5843647908,173540119,0,false,false,false],[2856,0,1,0,0,0,2855,0,2856,2062232263,83096357,0,false,false,false],[33933,0,20,0,0,0,33913,0,33933,67964592601,531191516,0,false,false,false],[6616,0,2,0,0,0,6614,0,6616,9782190540,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":49,"t":"2026-11-04T11:15:37.990231+00:00","m":8419102037452,"l":42986,"d":5694889,"rc":0,"h":"ok","id":true,"p":[[7158,0,3,0,0,0,7155,0,7158,5845046826,173540119,0,false,false,false],[2860,0,1,0,0,0,2859,0,2860,2062868788,83096357,0,false,false,false],[33977,0,20,0,0,0,33957,0,33977,68000675366,531191516,0,false,false,false],[6625,0,2,0,0,0,6623,0,6625,9786373372,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":50,"t":"2026-11-04T11:15:42.990241+00:00","m":8424102047311,"l":52845,"d":6252618,"rc":0,"h":"ok","id":true,"p":[[7168,0,3,0,0,0,7165,0,7168,5846826141,173540119,0,false,false,false],[2864,0,1,0,0,0,2863,0,2864,2063536371,83096357,0,false,false,false],[34021,0,20,0,0,0,34001,0,34021,68055227292,531191516,0,false,false,false],[6634,0,2,0,0,0,6632,0,6634,9791461001,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":51,"t":"2026-11-04T11:15:47.990266+00:00","m":8429102062415,"l":67949,"d":13071050,"rc":0,"h":"ok","id":true,"p":[[7175,0,3,0,0,0,7172,0,7175,5848661360,173540119,0,false,false,false],[2865,0,1,0,0,0,2864,0,2865,2063783772,83096357,0,false,false,false],[34044,0,20,0,0,0,34024,0,34044,68147667837,531191516,0,false,false,false],[6639,0,2,0,0,0,6637,0,6639,9797344871,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":52,"t":"2026-11-04T11:15:52.992746+00:00","m":8434104550207,"l":2555741,"d":20619393,"rc":0,"h":"ok","id":true,"p":[[7175,0,3,0,0,0,7172,0,7175,5848661360,173540119,0,false,false,false],[2868,0,1,0,0,0,2867,0,2868,2067609329,83096357,0,false,false,false],[34047,0,20,0,0,0,34027,0,34047,68151778674,531191516,0,false,false,false],[6643,0,2,0,0,0,6641,0,6643,9805440673,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":53,"t":"2026-11-04T11:15:57.990733+00:00","m":8439102525509,"l":531043,"d":15037355,"rc":0,"h":"ok","id":true,"p":[[7182,0,3,0,0,0,7179,0,7182,5859952888,173540119,0,false,false,false],[2870,0,1,0,0,0,2869,0,2870,2069637427,83096357,0,false,false,false],[34071,0,20,0,0,0,34051,0,34071,68312692899,531191516,0,false,false,false],[6648,0,2,0,0,0,6646,0,6648,9816349936,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":54,"t":"2026-11-04T11:16:02.991954+00:00","m":8444103733409,"l":1738943,"d":31227521,"rc":0,"h":"ok","id":true,"p":[[7182,0,3,0,0,0,7179,0,7182,5859952888,173540119,0,false,false,false],[2870,0,1,0,0,0,2869,0,2870,2069637427,83096357,0,false,false,false],[34071,0,20,0,0,0,34051,0,34071,68312692899,531191516,0,false,false,false],[6648,0,2,0,0,0,6646,0,6648,9816349936,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":55,"t":"2026-11-04T11:16:07.993110+00:00","m":8449104913256,"l":2918790,"d":21689612,"rc":0,"h":"ok","id":true,"p":[[7189,0,3,0,0,0,7186,0,7189,5892544825,173540119,0,false,false,false],[2870,0,1,0,0,0,2869,0,2870,2069637427,83096357,0,false,false,false],[34087,0,20,0,0,0,34067,0,34087,68400738101,531191516,0,false,false,false],[6653,0,2,0,0,0,6651,0,6653,9846226002,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":56,"t":"2026-11-04T11:16:12.990241+00:00","m":8454102045590,"l":51124,"d":14921692,"rc":0,"h":"ok","id":true,"p":[[7189,0,3,0,0,0,7186,0,7189,5892544825,173540119,0,false,false,false],[2870,0,1,0,0,0,2869,0,2870,2069637427,83096357,0,false,false,false],[34087,0,20,0,0,0,34067,0,34087,68400738101,531191516,0,false,false,false],[6653,0,2,0,0,0,6651,0,6653,9846226002,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":57,"t":"2026-11-04T11:16:17.990245+00:00","m":8459102049856,"l":55390,"d":10766171,"rc":0,"h":"ok","id":true,"p":[[7197,0,3,0,0,0,7194,0,7197,5895286174,173540119,0,false,false,false],[2874,0,1,0,0,0,2873,0,2874,2070677854,83096357,0,false,false,false],[34124,0,20,0,0,0,34104,0,34124,68481558745,531191516,0,false,false,false],[6662,0,2,0,0,0,6660,0,6662,9855385447,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":58,"t":"2026-11-04T11:16:22.991672+00:00","m":8464103476592,"l":1482126,"d":13095734,"rc":0,"h":"ok","id":true,"p":[[7204,0,3,0,0,0,7201,0,7204,5899049956,173540119,0,false,false,false],[2878,0,1,0,0,0,2877,0,2878,2077267515,83096357,0,false,false,false],[34150,0,20,0,0,0,34130,0,34150,68620821421,531191516,0,false,false,false],[6670,0,2,0,0,0,6668,0,6670,9867902862,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":59,"t":"2026-11-04T11:16:27.990226+00:00","m":8469102033076,"l":38610,"d":7087158,"rc":0,"h":"ok","id":true,"p":[[7205,0,3,0,0,0,7202,0,7205,5899276518,173540119,0,false,false,false],[2878,0,1,0,0,0,2877,0,2878,2077267515,83096357,0,false,false,false],[34154,0,20,0,0,0,34134,0,34154,68624672128,531191516,0,false,false,false],[6671,0,2,0,0,0,6669,0,6671,9870577285,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":60,"t":"2026-11-04T11:16:32.992885+00:00","m":8474104676508,"l":2682042,"d":22014406,"rc":0,"h":"ok","id":true,"p":[[7215,0,3,0,0,0,7212,0,7215,5902900491,173540119,0,false,false,false],[2882,0,1,0,0,0,2881,0,2882,2078447224,83096357,0,false,false,false],[34198,0,20,0,0,0,34178,0,34198,68687122760,531191516,0,false,false,false],[6680,0,2,0,0,0,6678,0,6680,9877529090,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":61,"t":"2026-11-04T11:16:37.990243+00:00","m":8479102048605,"l":54139,"d":8477680,"rc":0,"h":"ok","id":true,"p":[[7222,0,3,0,0,0,7219,0,7222,5915444056,173540119,0,false,false,false],[2886,0,1,0,0,0,2885,0,2886,2083072423,83096357,0,false,false,false],[34224,0,20,0,0,0,34204,0,34224,68767348690,531191516,0,false,false,false],[6689,0,2,0,0,0,6687,0,6689,9896133923,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":62,"t":"2026-11-04T11:16:42.990220+00:00","m":8484102026814,"l":32348,"d":5952815,"rc":0,"h":"ok","id":true,"p":[[7222,0,3,0,0,0,7219,0,7222,5915444056,173540119,0,false,false,false],[2886,0,1,0,0,0,2885,0,2886,2083072423,83096357,0,false,false,false],[34224,0,20,0,0,0,34204,0,34224,68767348690,531191516,0,false,false,false],[6689,0,2,0,0,0,6687,0,6689,9896133923,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":63,"t":"2026-11-04T11:16:47.990573+00:00","m":8489102379002,"l":384536,"d":23030855,"rc":0,"h":"ok","id":true,"p":[[7229,0,3,0,0,0,7226,0,7229,5920291519,173540119,0,false,false,false],[2890,0,1,0,0,0,2889,0,2890,2088033939,83096357,0,false,false,false],[34250,0,20,0,0,0,34230,0,34250,68838292516,531191516,0,false,false,false],[6698,0,2,0,0,0,6696,0,6698,9918403941,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":64,"t":"2026-11-04T11:16:52.992019+00:00","m":8494103787977,"l":1793511,"d":91499584,"rc":0,"h":"ok","id":true,"p":[[7232,0,3,0,0,0,7229,0,7232,5922217399,173540119,0,false,false,false],[2890,0,1,0,0,0,2889,0,2890,2088033939,83096357,0,false,false,false],[34262,0,20,0,0,0,34241,0,34261,68908085244,531191516,0,true,false,false],[6703,0,2,0,0,0,6701,0,6703,9929105157,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":65,"t":"2026-11-04T11:16:57.990384+00:00","m":8499102180415,"l":185949,"d":57678109,"rc":0,"h":"ok","id":true,"p":[[7236,0,3,0,0,0,7233,0,7236,5927337450,173540119,0,false,false,false],[2890,0,1,0,0,0,2889,0,2890,2088033939,83096357,0,false,false,false],[34267,0,20,0,0,0,34247,0,34267,68958086021,531191516,0,false,false,false],[6703,0,2,0,0,0,6701,0,6703,9929105157,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":66,"t":"2026-11-04T11:17:02.993537+00:00","m":8504105337837,"l":3343371,"d":171079914,"rc":0,"h":"ok","id":true,"p":[[7243,0,3,0,0,0,7240,0,7243,5939087895,173540119,0,false,false,false],[2890,0,1,0,0,0,2889,0,2890,2088033939,83096357,0,false,false,false],[34286,0,20,0,0,0,34266,0,34286,69125706117,531191516,0,false,false,false],[6708,0,2,0,0,0,6706,0,6708,9942639885,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":67,"t":"2026-11-04T11:17:07.991306+00:00","m":8509103110700,"l":1116234,"d":18169970,"rc":0,"h":"ok","id":true,"p":[[7243,0,3,0,0,0,7240,0,7243,5939087895,173540119,0,false,false,false],[2890,0,1,0,0,0,2889,0,2890,2088033939,83096357,0,false,false,false],[34286,0,20,0,0,0,34266,0,34286,69125706117,531191516,0,false,false,false],[6708,0,2,0,0,0,6706,0,6708,9942639885,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":68,"t":"2026-11-04T11:17:12.990254+00:00","m":8514102057176,"l":62710,"d":10098084,"rc":0,"h":"ok","id":true,"p":[[7251,0,3,0,0,0,7248,0,7251,5941220800,173540119,0,false,false,false],[2894,0,1,0,0,0,2893,0,2894,2091375406,83096357,0,false,false,false],[34321,0,20,0,0,0,34301,0,34321,69213081154,531191516,0,false,false,false],[6717,0,2,0,0,0,6715,0,6717,9957656790,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":69,"t":"2026-11-04T11:17:17.991226+00:00","m":8519103031863,"l":1037397,"d":13486561,"rc":0,"h":"ok","id":true,"p":[[7253,0,3,0,0,0,7250,0,7253,5941716946,173540119,0,false,false,false],[2894,0,1,0,0,0,2893,0,2894,2091375406,83096357,0,false,false,false],[34328,0,20,0,0,0,34308,0,34328,69232033554,531191516,0,false,false,false],[6719,0,2,0,0,0,6717,0,6719,9960598863,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":70,"t":"2026-11-04T11:17:22.990255+00:00","m":8524102059710,"l":65244,"d":21276334,"rc":0,"h":"ok","id":true,"p":[[7259,0,3,0,0,0,7256,0,7259,5945719691,173540119,0,false,false,false],[2898,0,1,0,0,0,2897,0,2898,2092937166,83096357,0,false,false,false],[34353,0,20,0,0,0,34333,0,34353,69271239490,531191516,0,false,false,false],[6726,0,2,0,0,0,6724,0,6726,9967251335,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":71,"t":"2026-11-04T11:17:27.991463+00:00","m":8529103263128,"l":1268662,"d":12773274,"rc":0,"h":"ok","id":true,"p":[[7266,0,3,0,0,0,7263,0,7266,5953462567,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34379,0,20,0,0,0,34359,0,34379,69410617682,531191516,0,false,false,false],[6735,0,2,0,0,0,6733,0,6735,9985320181,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":72,"t":"2026-11-04T11:17:32.995387+00:00","m":8534107190386,"l":5195920,"d":28267362,"rc":0,"h":"ok","id":true,"p":[[7266,0,3,0,0,0,7263,0,7266,5953462567,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34379,0,20,0,0,0,34359,0,34379,69410617682,531191516,0,false,false,false],[6735,0,2,0,0,0,6733,0,6735,9985320181,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":73,"t":"2026-11-04T11:17:37.992037+00:00","m":8539103843180,"l":1848714,"d":22999422,"rc":0,"h":"ok","id":true,"p":[[7270,0,3,0,0,0,7267,0,7270,5954892853,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34392,0,20,0,0,0,34372,0,34392,69464206616,531191516,0,false,false,false],[6740,0,2,0,0,0,6738,0,6740,9996762365,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":74,"t":"2026-11-04T11:17:42.991281+00:00","m":8544103084938,"l":1090472,"d":22113332,"rc":0,"h":"ok","id":true,"p":[[7270,0,3,0,0,0,7267,0,7270,5954892853,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34392,0,20,0,0,0,34372,0,34392,69464206616,531191516,0,false,false,false],[6740,0,2,0,0,0,6738,0,6740,9996762365,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":75,"t":"2026-11-04T11:17:47.994660+00:00","m":8549106456318,"l":4461852,"d":129817477,"rc":0,"h":"ok","id":true,"p":[[7273,0,3,0,0,0,7270,0,7273,5965075451,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34414,0,20,0,0,0,34394,0,34414,69530793468,531191516,0,false,false,false],[6742,0,2,0,0,0,6740,0,6742,10000357638,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":76,"t":"2026-11-04T11:17:52.990592+00:00","m":8554102394929,"l":400463,"d":115848297,"rc":0,"h":"ok","id":true,"p":[[7273,0,3,0,0,0,7270,0,7273,5965075451,173540119,0,false,false,false],[2902,0,1,0,0,0,2901,0,2902,2096108377,83096357,0,false,false,false],[34414,0,20,0,0,0,34394,0,34414,69530793468,531191516,0,false,false,false],[6742,0,2,0,0,0,6740,0,6742,10000357638,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":77,"t":"2026-11-04T11:17:57.991364+00:00","m":8559103167751,"l":1173285,"d":79497970,"rc":0,"h":"ok","id":true,"p":[[7273,0,3,0,0,0,7270,0,7273,5965075451,173540119,0,false,false,false],[2906,0,1,0,0,0,2905,0,2906,2102259739,83096357,0,false,false,false],[34421,0,20,0,0,0,34401,0,34421,69698634910,531191516,0,false,false,false],[6746,0,2,0,0,0,6744,0,6746,10013431387,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":78,"t":"2026-11-04T11:18:02.993031+00:00","m":8564104835184,"l":2840718,"d":66185531,"rc":0,"h":"ok","id":true,"p":[[7273,0,3,0,0,0,7270,0,7273,5965075451,173540119,0,false,false,false],[2906,0,1,0,0,0,2905,0,2906,2102259739,83096357,0,false,false,false],[34421,0,20,0,0,0,34401,0,34421,69698634910,531191516,0,false,false,false],[6746,0,2,0,0,0,6744,0,6746,10013431387,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":79,"t":"2026-11-04T11:18:07.991843+00:00","m":8569103645921,"l":1651455,"d":37173212,"rc":0,"h":"ok","id":true,"p":[[7278,0,3,0,0,0,7275,0,7278,5972069794,173540119,0,false,false,false],[2906,0,1,0,0,0,2905,0,2906,2102259739,83096357,0,false,false,false],[34439,0,20,0,0,0,34419,0,34439,69859657866,531191516,0,false,false,false],[6751,0,2,0,0,0,6749,0,6751,10025644323,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":80,"t":"2026-11-04T11:18:12.990754+00:00","m":8574102558475,"l":564009,"d":28112378,"rc":0,"h":"ok","id":true,"p":[[7278,0,3,0,0,0,7275,0,7278,5972069794,173540119,0,false,false,false],[2906,0,1,0,0,0,2905,0,2906,2102259739,83096357,0,false,false,false],[34439,0,20,0,0,0,34419,0,34439,69859657866,531191516,0,false,false,false],[6751,0,2,0,0,0,6749,0,6751,10025644323,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":81,"t":"2026-11-04T11:18:17.991237+00:00","m":8579103043198,"l":1048732,"d":21253887,"rc":0,"h":"ok","id":true,"p":[[7279,0,3,0,0,0,7276,0,7279,5972240422,173540119,0,false,false,false],[2909,0,1,0,0,0,2908,0,2909,2104503893,83096357,0,false,false,false],[34451,0,20,0,0,0,34431,0,34451,69988102104,531191516,0,false,false,false],[6755,0,2,0,0,0,6753,0,6755,10031545216,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":82,"t":"2026-11-04T11:18:22.990825+00:00","m":8584102623124,"l":628658,"d":28198049,"rc":0,"h":"ok","id":true,"p":[[7279,0,3,0,0,0,7276,0,7279,5972240422,173540119,0,false,false,false],[2909,0,1,0,0,0,2908,0,2909,2104503893,83096357,0,false,false,false],[34451,0,20,0,0,0,34431,0,34451,69988102104,531191516,0,false,false,false],[6755,0,2,0,0,0,6753,0,6755,10031545216,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":83,"t":"2026-11-04T11:18:27.990659+00:00","m":8589102466030,"l":471564,"d":30760941,"rc":0,"h":"ok","id":true,"p":[[7280,0,3,0,0,0,7277,0,7280,5973517784,173540119,0,false,false,false],[2912,0,1,0,0,0,2911,0,2912,2107880472,83096357,0,false,false,false],[34469,0,20,0,0,0,34449,0,34469,70059131429,531191516,0,false,false,false],[6759,0,2,0,0,0,6757,0,6759,10040775400,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":84,"t":"2026-11-04T11:18:32.991486+00:00","m":8594103283791,"l":1289325,"d":189395385,"rc":0,"h":"ok","id":true,"p":[[7280,0,3,0,0,0,7277,0,7280,5973517784,173540119,0,false,false,false],[2912,0,1,0,0,0,2911,0,2912,2107880472,83096357,0,false,false,false],[34469,0,20,0,0,0,34449,0,34469,70059131429,531191516,0,false,false,false],[6759,0,2,0,0,0,6757,0,6759,10040775400,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":85,"t":"2026-11-04T11:18:37.991049+00:00","m":8599102853257,"l":858791,"d":46614594,"rc":0,"h":"ok","id":true,"p":[[7283,0,3,0,0,0,7280,0,7283,5983228185,173540119,0,false,false,false],[2915,0,1,0,0,0,2914,0,2915,2119267823,83096357,0,false,false,false],[34481,0,20,0,0,0,34461,0,34481,70304189833,531191516,0,false,false,false],[6759,0,2,0,0,0,6757,0,6759,10040775400,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":86,"t":"2026-11-04T11:18:42.994599+00:00","m":8604106404429,"l":4409963,"d":80943179,"rc":0,"h":"ok","id":true,"p":[[7283,0,3,0,0,0,7280,0,7283,5983228185,173540119,0,false,false,false],[2915,0,1,0,0,0,2914,0,2915,2119267823,83096357,0,false,false,false],[34481,0,20,0,0,0,34461,0,34481,70304189833,531191516,0,false,false,false],[6759,0,2,0,0,0,6757,0,6759,10040775400,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":87,"t":"2026-11-04T11:18:47.990604+00:00","m":8609102409787,"l":415321,"d":22616916,"rc":0,"h":"ok","id":true,"p":[[7287,0,3,0,0,0,7284,0,7287,6000558000,173540119,0,false,false,false],[2915,0,1,0,0,0,2914,0,2915,2119267823,83096357,0,false,false,false],[34499,0,20,0,0,0,34479,0,34499,70384418329,531191516,0,false,false,false],[6761,0,2,0,0,0,6759,0,6761,10111308684,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":88,"t":"2026-11-04T11:18:52.994242+00:00","m":8614106038839,"l":4044373,"d":37514818,"rc":0,"h":"ok","id":true,"p":[[7287,0,3,0,0,0,7284,0,7287,6000558000,173540119,0,false,false,false],[2915,0,1,0,0,0,2914,0,2915,2119267823,83096357,0,false,false,false],[34499,0,20,0,0,0,34479,0,34499,70384418329,531191516,0,false,false,false],[6761,0,2,0,0,0,6759,0,6761,10111308684,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":89,"t":"2026-11-04T11:18:57.990242+00:00","m":8619102044995,"l":50529,"d":29596439,"rc":0,"h":"ok","id":true,"p":[[7288,0,3,0,0,0,7285,0,7288,6002764385,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34516,0,20,0,0,0,34496,0,34516,70499191905,531191516,0,false,false,false],[6765,0,2,0,0,0,6763,0,6765,10164880064,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":90,"t":"2026-11-04T11:19:02.996218+00:00","m":8624108017914,"l":6023448,"d":73723908,"rc":0,"h":"ok","id":true,"p":[[7288,0,3,0,0,0,7285,0,7288,6002764385,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34516,0,20,0,0,0,34496,0,34516,70499191905,531191516,0,false,false,false],[6765,0,2,0,0,0,6763,0,6765,10164880064,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":91,"t":"2026-11-04T11:19:07.990301+00:00","m":8629102101160,"l":106694,"d":42863922,"rc":0,"h":"ok","id":true,"p":[[7291,0,3,0,0,0,7288,0,7291,6018238822,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34520,0,20,0,0,0,34500,0,34520,70617347041,531191516,0,false,false,false],[6765,0,2,0,0,0,6763,0,6765,10164880064,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":92,"t":"2026-11-04T11:19:12.990662+00:00","m":8634102467522,"l":473056,"d":21286091,"rc":0,"h":"ok","id":true,"p":[[7291,0,3,0,0,0,7288,0,7291,6018238822,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34520,0,20,0,0,0,34500,0,34520,70617347041,531191516,0,false,false,false],[6765,0,2,0,0,0,6763,0,6765,10164880064,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":93,"t":"2026-11-04T11:19:17.993131+00:00","m":8639104930598,"l":2936132,"d":24813523,"rc":0,"h":"ok","id":true,"p":[[7296,0,3,0,0,0,7293,0,7296,6024480034,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34547,0,20,0,0,0,34527,0,34547,70720529837,531191516,0,false,false,false],[6771,0,2,0,0,0,6769,0,6771,10191601855,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":94,"t":"2026-11-04T11:19:22.991378+00:00","m":8644103182397,"l":1187931,"d":32895091,"rc":0,"h":"ok","id":true,"p":[[7296,0,3,0,0,0,7293,0,7296,6024480034,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34547,0,20,0,0,0,34527,0,34547,70720529837,531191516,0,false,false,false],[6772,0,2,0,0,0,6770,0,6772,10203160128,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":95,"t":"2026-11-04T11:19:27.991779+00:00","m":8649103581690,"l":1587224,"d":56851498,"rc":0,"h":"ok","id":true,"p":[[7297,0,3,0,0,0,7294,0,7297,6025750143,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34560,0,20,0,0,0,34540,0,34560,70814137985,531191516,0,false,false,false],[6774,0,2,0,0,0,6772,0,6774,10206588706,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":96,"t":"2026-11-04T11:19:32.991406+00:00","m":8654103208736,"l":1214270,"d":58992788,"rc":0,"h":"ok","id":true,"p":[[7297,0,3,0,0,0,7294,0,7297,6025750143,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34561,0,20,0,0,0,34541,0,34561,70817518963,531191516,0,false,false,false],[6774,0,2,0,0,0,6772,0,6774,10206588706,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":97,"t":"2026-11-04T11:19:37.990860+00:00","m":8659102663204,"l":668738,"d":54705369,"rc":0,"h":"ok","id":true,"p":[[7298,0,3,0,0,0,7295,0,7298,6027552626,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34567,0,20,0,0,0,34546,0,34566,70949473062,531191516,0,true,false,false],[6777,0,2,0,0,0,6775,0,6777,10218546437,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":98,"t":"2026-11-04T11:19:42.994268+00:00","m":8664106072370,"l":4077904,"d":78374655,"rc":0,"h":"ok","id":true,"p":[[7299,0,3,0,0,0,7296,0,7299,6027721601,173540119,0,false,false,false],[2919,0,1,0,0,0,2918,0,2919,2126926501,83096357,0,false,false,false],[34568,0,20,0,0,0,34548,0,34568,71003793535,531191516,0,false,false,false],[6777,0,2,0,0,0,6775,0,6777,10218546437,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":99,"t":"2026-11-04T11:19:47.991257+00:00","m":8669103061774,"l":1067308,"d":35043217,"rc":0,"h":"ok","id":true,"p":[[7299,0,3,0,0,0,7296,0,7299,6027721601,173540119,0,false,false,false],[2923,0,1,0,0,0,2922,0,2923,2146900742,83096357,0,false,false,false],[34573,0,20,0,0,0,34553,0,34573,71149720090,531191516,0,false,false,false],[6779,0,2,0,0,0,6777,0,6779,10227587136,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":100,"t":"2026-11-04T11:19:52.990423+00:00","m":8674102228065,"l":233599,"d":23928379,"rc":0,"h":"ok","id":true,"p":[[7299,0,3,0,0,0,7296,0,7299,6027721601,173540119,0,false,false,false],[2923,0,1,0,0,0,2922,0,2923,2146900742,83096357,0,false,false,false],[34573,0,20,0,0,0,34553,0,34573,71149720090,531191516,0,false,false,false],[6781,0,2,0,0,0,6779,0,6781,10242560224,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":101,"t":"2026-11-04T11:19:57.994565+00:00","m":8679106353833,"l":4359367,"d":38119872,"rc":0,"h":"ok","id":true,"p":[[7302,0,3,0,0,0,7299,0,7302,6038351976,173540119,0,false,false,false],[2923,0,1,0,0,0,2922,0,2923,2146900742,83096357,0,false,false,false],[34580,0,20,0,0,0,34560,0,34580,71324707605,531191516,0,false,false,false],[6786,0,2,0,0,0,6784,0,6786,10283256061,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":102,"t":"2026-11-04T11:20:02.991167+00:00","m":8684102974867,"l":980401,"d":32607338,"rc":0,"h":"ok","id":true,"p":[[7302,0,3,0,0,0,7299,0,7302,6038351976,173540119,0,false,false,false],[2923,0,1,0,0,0,2922,0,2923,2146900742,83096357,0,false,false,false],[34581,0,20,0,0,0,34561,0,34581,71403403131,531191516,0,false,false,false],[6786,0,2,0,0,0,6784,0,6786,10283256061,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":103,"t":"2026-11-04T11:20:07.993571+00:00","m":8689105369623,"l":3375157,"d":30231795,"rc":0,"h":"ok","id":true,"p":[[7302,0,3,0,0,0,7299,0,7302,6038351976,173540119,0,false,false,false],[2926,0,1,0,0,0,2924,0,2925,2156463628,83096357,0,true,false,false],[34587,0,20,0,0,0,34567,0,34587,71508580291,531191516,0,false,false,false],[6786,0,2,0,0,0,6784,0,6786,10283256061,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":104,"t":"2026-11-04T11:20:12.990547+00:00","m":8694102353659,"l":359193,"d":44317853,"rc":0,"h":"ok","id":true,"p":[[7303,0,3,0,0,0,7300,0,7303,6042052571,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34593,0,20,0,0,0,34573,0,34593,71568943026,531191516,0,false,false,false],[6790,0,2,0,0,0,6788,0,6790,10304530410,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":105,"t":"2026-11-04T11:20:17.993481+00:00","m":8699105275827,"l":3281361,"d":42684709,"rc":0,"h":"ok","id":true,"p":[[7305,0,3,0,0,0,7302,0,7305,6049298253,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34599,0,20,0,0,0,34579,0,34599,71677213702,531191516,0,false,false,false],[6792,0,2,0,0,0,6790,0,6792,10330081875,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":106,"t":"2026-11-04T11:20:22.992097+00:00","m":8704103895309,"l":1900843,"d":50942050,"rc":0,"h":"ok","id":true,"p":[[7308,0,3,0,0,0,7305,0,7308,6053747947,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34600,0,20,0,0,0,34580,0,34600,71693695980,531191516,0,false,false,false],[6792,0,2,0,0,0,6790,0,6792,10330081875,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":107,"t":"2026-11-04T11:20:27.990643+00:00","m":8709102440070,"l":445604,"d":19063206,"rc":0,"h":"ok","id":true,"p":[[7309,0,3,0,0,0,7306,0,7309,6055463489,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34603,0,20,0,0,0,34583,0,34603,71728423712,531191516,0,false,false,false],[6792,0,2,0,0,0,6790,0,6792,10330081875,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":108,"t":"2026-11-04T11:20:32.991242+00:00","m":8714103046814,"l":1052348,"d":20418297,"rc":0,"h":"ok","id":true,"p":[[7311,0,3,0,0,0,7308,0,7311,6065661371,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34621,0,20,0,0,0,34601,0,34621,71864570334,531191516,0,false,false,false],[6792,0,2,0,0,0,6790,0,6792,10330081875,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":109,"t":"2026-11-04T11:20:37.994924+00:00","m":8719106729494,"l":4735028,"d":27004972,"rc":0,"h":"ok","id":true,"p":[[7311,0,3,0,0,0,7308,0,7311,6065661371,173540119,0,false,false,false],[2927,0,1,0,0,0,2926,0,2927,2164865246,83096357,0,false,false,false],[34621,0,20,0,0,0,34601,0,34621,71864570334,531191516,0,false,false,false],[6792,0,2,0,0,0,6790,0,6792,10330081875,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":110,"t":"2026-11-04T11:20:42.991804+00:00","m":8724103602216,"l":1607750,"d":30567321,"rc":0,"h":"ok","id":true,"p":[[7312,0,3,0,0,0,7309,0,7312,6065986978,173540119,0,false,false,false],[2931,0,1,0,0,0,2930,0,2931,2173861837,83096357,0,false,false,false],[34639,0,20,0,0,0,34619,0,34639,72090570779,531191516,0,false,false,false],[6796,0,2,0,0,0,6794,0,6796,10343779413,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":111,"t":"2026-11-04T11:20:47.991681+00:00","m":8729103477843,"l":1483377,"d":37149989,"rc":0,"h":"ok","id":true,"p":[[7312,0,3,0,0,0,7309,0,7312,6065986978,173540119,0,false,false,false],[2931,0,1,0,0,0,2930,0,2931,2173861837,83096357,0,false,false,false],[34639,0,20,0,0,0,34619,0,34639,72090570779,531191516,0,false,false,false],[6796,0,2,0,0,0,6794,0,6796,10343779413,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":112,"t":"2026-11-04T11:20:52.990609+00:00","m":8734102364892,"l":370426,"d":19918438,"rc":0,"h":"ok","id":true,"p":[[7317,0,3,0,0,0,7314,0,7317,6075300251,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34652,0,21,0,0,0,34631,0,34652,72313580290,531191516,0,false,false,false],[6796,0,2,0,0,0,6794,0,6796,10343779413,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":113,"t":"2026-11-04T11:20:57.990613+00:00","m":8739102418053,"l":423587,"d":14664083,"rc":0,"h":"ok","id":true,"p":[[7317,0,3,0,0,0,7314,0,7317,6075300251,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34659,0,21,0,0,0,34638,0,34659,72333437223,531191516,0,false,false,false],[6796,0,2,0,0,0,6794,0,6796,10343779413,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":114,"t":"2026-11-04T11:21:02.991538+00:00","m":8744103344309,"l":1349843,"d":27584910,"rc":0,"h":"ok","id":true,"p":[[7325,0,3,0,0,0,7322,0,7325,6085754805,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34673,0,21,0,0,0,34652,0,34673,72392551744,531191516,0,false,false,false],[6801,0,2,0,0,0,6799,0,6801,10365574134,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":115,"t":"2026-11-04T11:21:07.990549+00:00","m":8749102356052,"l":361586,"d":25924592,"rc":0,"h":"ok","id":true,"p":[[7328,0,3,0,0,0,7325,0,7328,6091598522,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34685,0,21,0,0,0,34664,0,34685,72431484340,531191516,0,false,false,false],[6803,0,2,0,0,0,6801,0,6803,10372788249,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":116,"t":"2026-11-04T11:21:12.993105+00:00","m":8754104905277,"l":2910811,"d":52128878,"rc":0,"h":"ok","id":true,"p":[[7331,0,3,0,0,0,7328,0,7331,6093061605,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34694,0,21,0,0,0,34673,0,34694,72493376420,531191516,0,false,false,false],[6806,0,2,0,0,0,6804,0,6806,10383986041,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":117,"t":"2026-11-04T11:21:17.990390+00:00","m":8759102196345,"l":201879,"d":15036379,"rc":0,"h":"ok","id":true,"p":[[7335,0,3,0,0,0,7332,0,7335,6104183545,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34710,0,21,0,0,0,34689,0,34710,72558067866,531191516,0,false,false,false],[6809,0,2,0,0,0,6807,0,6809,10406463671,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":118,"t":"2026-11-04T11:21:22.992690+00:00","m":8764104495317,"l":2500851,"d":24688998,"rc":0,"h":"ok","id":true,"p":[[7337,0,3,0,0,0,7334,0,7337,6105380499,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34715,0,21,0,0,0,34694,0,34715,72583500161,531191516,0,false,false,false],[6811,0,2,0,0,0,6809,0,6811,10409950770,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":119,"t":"2026-11-04T11:21:27.990379+00:00","m":8769102183936,"l":189470,"d":20463971,"rc":0,"h":"ok","id":true,"p":[[7346,0,3,0,0,0,7343,0,7346,6122450642,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34743,0,21,0,0,0,34722,0,34743,72743017358,531191516,0,false,false,false],[6816,0,2,0,0,0,6814,0,6816,10420849335,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
{"i":120,"t":"2026-11-04T11:21:32.993701+00:00","m":8774105504529,"l":3510063,"d":33467488,"rc":0,"h":"ok","id":true,"p":[[7346,0,3,0,0,0,7343,0,7346,6122450642,173540119,0,false,false,false],[2932,0,1,0,0,0,2931,0,2932,2174091424,83096357,0,false,false,false],[34743,0,21,0,0,0,34722,0,34743,72743017358,531191516,0,false,false,false],[6816,0,2,0,0,0,6814,0,6816,10420849335,206261935,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false],[0,0,0,0,0,0,0,0,0,null,null,0,false,false,false]]}
```

## Verification of the documentation worktree

The tracked change is this document only. Rust source, TODO, CHANGELOG and
coverage rows are unchanged. The five mandatory commands were executed at
worktree base `bd47305c7fa901b852d076390f4ac57153482e67` with this document
added; no Rust fixes or test modifications were made.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass |
| `cargo test --locked` with managed long TMPDIR | Fail: 439 passed, 4 failed, 6 ignored |
| `cargo test --locked` with default TMPDIR | Pass: 443 passed, 6 ignored; 1 integration passed |
| `cargo build --release --locked` | Pass |
| `target/release/herdr-lineup --help` | Pass; built worktree binary only |

The first test run inherited the long managed run work directory as TMPDIR.
Three failures had Unix socket path overflow: the two shutdown/publication
fixtures (`src/publish.rs:5752`, `5991`) and the nested slow-heading child
(`5829`, propagated through its parent's `5911` status assertion).
The fourth, `envelope_expiring_after_admission_is_an_error_not_a_skip`, failed
at `src/publish.rs:8463` with `RefreshSkip { capacity_skipped: 0,
deadline_skipped: 2 }` during the later refresh.

Commander note `note_m2c_tests` explicitly authorized a rerun outside
`dev-task-temp run`, without injecting TMPDIR, using ordinary default `/tmp`
for the tests' small temporary sockets. No short root or alias was created.
The normal full suite passed, including the affected socket fixtures. Raw
rerun output is retained as `work/default-tmp-0.log` in the original checks
run. Default temporary test directories were managed by the existing tests;
no manual recursive deletion was performed.

The requested isolated repetitions used the same unchanged source, without
an injected TMPDIR, with one exact test per command and `--test-threads=1`:

```sh
cargo test --locked --lib publish::tests::envelope_expiring_after_admission_is_an_error_not_a_skip -- --exact --test-threads=1
cargo test --locked --lib publish::tests::slow_heading_does_not_consume_reader_timeout_budget -- --exact --test-threads=1
```

Each command ran five times. Envelope: 1 failed, 4 passed; its failing rerun
reported `deadline_skipped: 1` at the same unwrap. Slow heading: 5 passed.
These commands did not overlap this lane's compilation, but the shared host
was not quiet: one-minute load averages at starts were approximately
79.5–81.1. **A controlled low-load reproduction was not obtained.** The
reproducible intermittent failure was reported separately to the commander;
its root cause is unproven and was not repaired in this documentation lane.
A passing full run does not erase the failed repetition.

The complete rerun series and per-command load averages are in
`work/default-tmp-results.json`, with logs `work/default-tmp-0.log` through
`work/default-tmp-10.log`, under the original checks run. This record makes
no all-checks-always-green claim. The commander must decide how to handle the
existing intermittent test separately from the live observation record.
