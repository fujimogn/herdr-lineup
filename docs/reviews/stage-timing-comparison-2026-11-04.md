# Alternating old/new synthetic stage observations, 2026-11-04

This is a new N1 observation record, separate from the historical runs in
[refresh-stage-latency-2026-10-23.md](refresh-stage-latency-2026-10-23.md).
It does not establish a speed-up, SLO, macOS result, live Herdr result, or
event-to-visible latency.

## Sources and fixture

- Old: `4afc9d42b9bab850576ffb3bd42d44accf2d21c2`, the original measurement
  candidate explicitly identified by the historical record, based on
  `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`.
- New: `bd62eb512d7708346591e8df84f05bec0bd5e645`, the assignment's local
  `origin/develop` and implementation checkout at the start of this run.
- Both use their own unchanged `publish::tests::measure_refresh_stages`,
  pinned Rust 1.99.1, locked dependencies, and debug test builds. The source
  trees and dependency versions differ; this is not an isolated test of only
  the sequence-reservation change.

The fixture creates a fresh publisher, synthetic Git repository with
`origin/main`, one workspace, a checkpoint directory, and a fake Unix socket
for each cold refresh. It uses 1, 8, 32, 128, and 256 synthetic Codex panes,
with `SampleReader` returning fixed data. Network quota is disabled by the
default configuration. No provider files, real account, sidebar rendering,
or actual Herdr server is involved.

Only the ignored library test is run for measurement. No `herdr` command,
server lifecycle command, client attachment, or user configuration edit is
part of this procedure. Every `HERDR_*` environment variable is removed from
the test subprocess environment. All socket connections target the fixture's
newly bound `fake.sock` under its own temporary directory.

## Environment and procedure

Host: `dev`, Linux `7.3.1-1-cachyos`, x86_64; AMD Ryzen 9 9955HX 16-Core
Processor, 16 visible logical CPUs, cpuset `0-15`, MemTotal 49,251,780 KiB.
Tool versions: rustc 1.99.1 (`58cf8f9c3`, 2026-10-13), cargo 1.99.1
(`5f94df478`, 2026-08-27), Git 2.56.0.

`df -T` identifies `/tmp` as tmpfs and the worktree's target directory as
ext4 on `/dev/sda2`. The short alias `/tmp/herdr-lineup-n1-m4-ext4` points to
`/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/stage-timing-n1-m4/target/n1/disk`.
Both binaries use that same alias for ext4 fixtures, avoiding the Unix socket
pathname limit. The alias is on tmpfs; its destination and fixture contents
are on ext4. Both binaries use `/tmp` directly for tmpfs fixtures.

Both builds finish before measurement. No build or other verification is
started by this lane during the measurement sequence. Other host activity
continues; its load is neither isolated nor controlled. No scheduling affinity,
priority change, cache flush, or filesystem remount is applied. Each binary
has its original lockfile and no source modifications.

## Execution and binary identity

The following builds were completed in their respective dedicated worktrees:

```sh
cargo test --locked --lib --no-run -j 2
```

The first preflight mistakenly built and invoked the main test harness
(`--bin herdr-lineup`), which ran zero tests and produced no measurements.
It was excluded. The corrected library binaries each list the exact ignored
test before execution; no successful measurement was discarded or rerun.

| Label | Library test executable | SHA256 |
| --- | --- | --- |
| old | `herdr_lineup-897e4d11a82345ed` | `fd2519dce345de9bf2209dfbd4f79e4ea53d7ea75dfcfd3ca5facf5f6a333983` |
| new | `herdr_lineup-9b68830dc803b468` | `883db649467f4e91b041f5f24cbe5d46769a08cf0b99a061c323a8f1bf046b45` |

For each filesystem, invocation order is `old, new, old, new, old, new`:
three alternating pairs, six serial invocations. Tmpfs completes before ext4.
Each invocation uses `LINEUP_BENCH_REPEATS=2`, and runs pane counts
`1, 8, 32, 128, 256` in that order, with repetitions 0 then 1 for each.
Thus each filesystem/version/pane cell contains six separate cold publisher instances,
collected in three blocks of two; there are 12 invocations and 120 rows total.
Alternation is at the whole-binary invocation level, not between individual
pane rows. Old always runs first in a pair; temporal ordering is not randomized
or counterbalanced, and the observations are not statistically independent
of changing background activity.

The measurement driver uses this subprocess pattern with absolute paths to
the identified binaries, the new worktree as cwd, and one filesystem root
shared by both versions. No Cargo process is involved during measurement:

```python
for root in ("/tmp", "/tmp/herdr-lineup-n1-m4-ext4"):
    for label in ("old", "new") * 3:
        env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
        env.update(LINEUP_BENCH_ROOT=root, LINEUP_BENCH_REPEATS="2")
        subprocess.run([binary[label], "--exact",
                        "publish::tests::measure_refresh_stages", "--ignored",
                        "--nocapture", "--test-threads=1"],
                       env=env, cwd=new_worktree, check=True)
```

All 12 invocations exited zero and ran exactly one test each. Every CSV row
has `reports = panes + 1`; every old row has `checkpoint_rows = panes + 1`
and every new row has `checkpoint_rows = 1`. Checkpoint row count alone is
not a measurement of persist-write count. No separate persist-write counter
is present in this CSV schema.

### Execution order and background load

UTC timestamps and the 1/5/15-minute load averages sampled immediately
before and after each invocation follow. `/proc/loadavg` runnable/process
counts and last PID were saved locally but are not needed to interpret the
three load averages. Observed 1-minute load spans 72.96 to 100.64 on 16 CPUs;
this is a heavily loaded, non-dedicated host.

| FS | Invocation | Binary | Start UTC | End UTC | Load before | Load after |
| --- | ---: | --- | --- | --- | --- | --- |
| tmpfs | 1 | old | 2026-11-04T09:47:41.627745+00:00 | 2026-11-04T09:47:48.165039+00:00 | 91.77 / 105.28 / 90.01 | 93.55 / 105.42 / 90.14 |
| tmpfs | 2 | new | 2026-11-04T09:47:48.166616+00:00 | 2026-11-04T09:47:51.686579+00:00 | 93.55 / 105.42 / 90.14 | 95.11 / 105.55 / 90.26 |
| tmpfs | 3 | old | 2026-11-04T09:47:51.687207+00:00 | 2026-11-04T09:47:55.422725+00:00 | 95.11 / 105.55 / 90.26 | 94.94 / 105.34 / 90.28 |
| tmpfs | 4 | new | 2026-11-04T09:47:55.423477+00:00 | 2026-11-04T09:47:59.043321+00:00 | 94.94 / 105.34 / 90.28 | 94.94 / 105.34 / 90.28 |
| tmpfs | 5 | old | 2026-11-04T09:47:59.045562+00:00 | 2026-11-04T09:48:04.791450+00:00 | 94.94 / 105.34 / 90.28 | 89.58 / 104.05 / 89.94 |
| tmpfs | 6 | new | 2026-11-04T09:48:04.792099+00:00 | 2026-11-04T09:48:08.667671+00:00 | 89.58 / 104.05 / 89.94 | 88.33 / 103.55 / 89.86 |
| ext4 | 1 | old | 2026-11-04T09:48:08.668285+00:00 | 2026-11-04T09:48:33.845153+00:00 | 88.33 / 103.55 / 89.86 | 100.64 / 105.40 / 90.84 |
| ext4 | 2 | new | 2026-11-04T09:48:33.845829+00:00 | 2026-11-04T09:48:37.673384+00:00 | 100.64 / 105.40 / 90.84 | 98.99 / 104.97 / 90.78 |
| ext4 | 3 | old | 2026-11-04T09:48:37.674207+00:00 | 2026-11-04T09:48:54.069383+00:00 | 98.99 / 104.97 / 90.78 | 85.62 / 101.74 / 89.95 |
| ext4 | 4 | new | 2026-11-04T09:48:54.070058+00:00 | 2026-11-04T09:48:57.931637+00:00 | 85.62 / 101.74 / 89.95 | 83.16 / 100.96 / 89.76 |
| ext4 | 5 | old | 2026-11-04T09:48:57.932857+00:00 | 2026-11-04T09:49:21.072311+00:00 | 83.16 / 100.96 / 89.76 | 75.14 / 97.67 / 88.97 |
| ext4 | 6 | new | 2026-11-04T09:49:21.073072+00:00 | 2026-11-04T09:49:25.432303+00:00 | 75.14 / 97.67 / 88.97 | 72.96 / 96.84 / 88.75 |

### Independent medians

Values are microseconds, independently calculated across the six rows in
each cell. They describe this experiment only. Stage values overlap and must
not be summed. No ratio or causal improvement is inferred.

| FS | Binary | Panes | Total | Pipeline | Snapshot | Git | Reader | Persist | Socket | External |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| tmpfs | old | 1 | 41256 | 41056 | 1338 | 29394.5 | 10 | 300 | 5424.5 | 41275.5 |
| tmpfs | old | 8 | 64053 | 63938.5 | 2311 | 34222.5 | 70.5 | 2444 | 24776 | 64070 |
| tmpfs | old | 32 | 127091 | 126923.5 | 6406 | 29846 | 241 | 10236 | 74549.5 | 127115.5 |
| tmpfs | old | 128 | 509206.5 | 508823 | 24689 | 26486 | 1135.5 | 150981 | 266481 | 509231 |
| tmpfs | old | 256 | 1705885.5 | 1705756 | 34236 | 35066 | 2179.5 | 758077 | 704118 | 1705903.5 |
| tmpfs | new | 1 | 25925.5 | 25728.5 | 1340.5 | 19482 | 9 | 128.5 | 4819 | 25967.5 |
| tmpfs | new | 8 | 52050.5 | 51882.5 | 1842.5 | 26192.5 | 57 | 122 | 20251 | 52066.5 |
| tmpfs | new | 32 | 135000 | 134814.5 | 5713.5 | 31684.5 | 260.5 | 140.5 | 85847.5 | 135029.5 |
| tmpfs | new | 128 | 465923.5 | 465768.5 | 23207.5 | 37253 | 1090 | 116 | 346780.5 | 465949 |
| tmpfs | new | 256 | 881957 | 881754.5 | 37795.5 | 30268 | 2075 | 119 | 723516.5 | 881979 |
| ext4 | old | 1 | 74502.5 | 74267.5 | 814.5 | 28224 | 10 | 43791.5 | 4259.5 | 74522.5 |
| ext4 | old | 8 | 184841.5 | 184633 | 3405 | 27106.5 | 73.5 | 136010.5 | 20503.5 | 184856.5 |
| ext4 | old | 32 | 687954 | 687731.5 | 4827 | 26702 | 300.5 | 582589 | 66166 | 687969.5 |
| ext4 | old | 128 | 3041076.5 | 3040835.5 | 23492.5 | 22725 | 1380 | 2660630.5 | 259919 | 3041095.5 |
| ext4 | old | 256 | 7020522.5 | 7020197 | 44446 | 41229 | 2711.5 | 6176487 | 623439 | 7020537.5 |
| ext4 | new | 1 | 54515.5 | 54277 | 1369.5 | 29826.5 | 8.5 | 18748 | 4484 | 54539 |
| ext4 | new | 8 | 132952.5 | 132752.5 | 2453 | 30441.5 | 73 | 75129.5 | 24273.5 | 132969.5 |
| ext4 | new | 32 | 137360 | 137171 | 6357.5 | 22868 | 292.5 | 16341 | 83682.5 | 137382.5 |
| ext4 | new | 128 | 434380 | 434204.5 | 20871.5 | 27426.5 | 984.5 | 32194.5 | 316118.5 | 434395.5 |
| ext4 | new | 256 | 928747.5 | 928564 | 35845.5 | 28577.5 | 1917.5 | 75650 | 658676 | 928762 |

### Raw rows

Integer microseconds as emitted by the fixture, with filesystem, invocation
and binary label prepended. `run` restarts at zero for every pane count in
each invocation; it is not a global sample index.

```csv
filesystem,invocation,binary,panes,run,total_us,pipeline_us,snapshot_us,git_us,reader_us,persist_us,socket_us,reports,checkpoint_rows,external_us
tmpfs,1,old,1,0,43767,43587,1295,37089,10,227,5398,2,2,43784
tmpfs,1,old,1,1,31979,31837,1979,25429,9,252,5315,2,2,32000
tmpfs,1,old,8,0,44300,44093,2205,13061,59,4495,22068,9,9,44315
tmpfs,1,old,8,1,73278,73158,2417,34029,71,1585,33806,9,9,73292
tmpfs,1,old,32,0,148350,148161,6788,38084,205,8640,89695,33,33,148368
tmpfs,1,old,32,1,142040,141914,4104,34554,220,8402,85319,33,33,142054
tmpfs,1,old,128,0,832089,831958,24624,34871,2838,158210,485579,129,129,832107
tmpfs,1,old,128,1,717878,717737,33580,24360,1053,154187,405088,129,129,717891
tmpfs,1,old,256,0,2401296,2401167,66001,42495,2547,1183995,936334,257,257,2401312
tmpfs,1,old,256,1,1664543,1664424,70647,37112,2598,684772,740715,257,257,1664559
tmpfs,2,new,1,0,24504,24263,950,18980,8,120,3971,2,1,24550
tmpfs,2,new,1,1,38007,37849,574,32021,11,102,4710,2,1,38024
tmpfs,2,new,8,0,43010,42884,1759,16023,83,157,20277,9,1,43028
tmpfs,2,new,8,1,92276,92152,1072,69095,73,95,18956,9,1,92293
tmpfs,2,new,32,0,122174,121865,6009,28217,262,118,76398,33,1,122191
tmpfs,2,new,32,1,123872,123691,7632,33217,240,193,76285,33,1,123891
tmpfs,2,new,128,0,467976,467812,18359,32601,1101,200,372564,129,1,468005
tmpfs,2,new,128,1,415740,415542,13186,25670,1027,102,336031,129,1,415757
tmpfs,2,new,256,0,883519,883251,39379,32256,2107,110,725398,257,1,883542
tmpfs,2,new,256,1,863974,863759,35344,23759,1987,240,721635,257,1,863993
tmpfs,3,old,1,0,39298,39036,1002,23819,21,715,10186,2,2,39324
tmpfs,3,old,1,1,43214,43076,1859,36600,10,352,4906,2,2,43227
tmpfs,3,old,8,0,73610,73476,3675,34661,70,3303,27484,9,9,73629
tmpfs,3,old,8,1,59833,59723,1790,34715,85,1409,20212,9,9,59846
tmpfs,3,old,32,0,108346,108205,11299,12466,235,10768,72370,33,33,108364
tmpfs,3,old,32,1,129912,129770,5794,27967,262,11809,76729,33,33,129931
tmpfs,3,old,128,0,471214,470958,22143,29700,799,120170,264744,129,129,471239
tmpfs,3,old,128,1,433913,433800,20580,20736,771,114715,249031,129,129,433927
tmpfs,3,old,256,0,1025353,1025236,26150,33020,1842,394659,492314,257,257,1025370
tmpfs,3,old,256,1,895478,895336,27987,9076,1694,369721,422380,257,257,895491
tmpfs,4,new,1,0,16270,16008,2660,8660,8,110,6089,2,1,16302
tmpfs,4,new,1,1,35063,34927,787,28703,10,149,4928,2,1,35081
tmpfs,4,new,8,0,36715,36560,1788,12817,45,129,20225,9,1,36729
tmpfs,4,new,8,1,56898,56729,1897,33304,59,123,19192,9,1,56915
tmpfs,4,new,32,0,105497,105358,4714,17146,236,97,71774,33,1,105514
tmpfs,4,new,32,1,146128,145938,3090,30152,377,140,95297,33,1,146168
tmpfs,4,new,128,0,463871,463725,47141,33430,1079,116,366604,129,1,463893
tmpfs,4,new,128,1,533081,532765,35926,178555,865,116,302292,129,1,533104
tmpfs,4,new,256,0,957878,957698,32931,11424,2472,151,784164,257,1,957897
tmpfs,4,new,256,1,880395,880258,39293,40350,2844,105,655057,257,1,880416
tmpfs,5,old,1,0,209243,209034,1352,33360,10,257,174427,2,2,209264
tmpfs,5,old,1,1,22756,22601,1324,15955,9,343,5451,2,2,22772
tmpfs,5,old,8,0,44254,44124,1233,15890,99,3748,19678,9,9,44270
tmpfs,5,old,8,1,68273,68154,10603,34416,65,1570,28201,9,9,68294
tmpfs,5,old,32,0,124270,124077,8028,31725,269,10505,67073,33,33,124300
tmpfs,5,old,32,1,114903,114775,6024,19682,247,9967,70953,33,33,114918
tmpfs,5,old,128,0,505766,505332,24754,28612,1218,157448,244286,129,129,505789
tmpfs,5,old,128,1,512647,512314,29562,22325,1238,147775,268218,129,129,512673
tmpfs,5,old,256,0,1747228,1747088,29352,31486,2333,831382,689940,257,257,1747248
tmpfs,5,old,256,1,1833950,1833800,39120,45079,2026,863749,718296,257,257,1833968
tmpfs,6,new,1,0,22868,22620,1731,17004,10,143,4101,2,1,22899
tmpfs,6,new,1,1,27347,27194,1867,19984,6,137,6048,2,1,27385
tmpfs,6,new,8,0,71309,71175,5166,31007,55,121,34174,9,1,71329
tmpfs,6,new,8,1,47203,47036,3967,21378,45,114,22396,9,1,47218
tmpfs,6,new,32,0,218772,218563,9145,50558,306,141,151512,33,1,218790
tmpfs,6,new,32,1,190440,190320,5418,77109,259,144,96761,33,1,190454
tmpfs,6,new,128,0,509696,509489,15571,46896,1196,110,357530,129,1,509730
tmpfs,6,new,128,1,453465,453290,28056,41076,1408,172,330108,129,1,453481
tmpfs,6,new,256,0,788593,788440,36298,34659,1923,128,635776,257,1,788618
tmpfs,6,new,256,1,1082334,1080156,49680,28280,2043,101,906969,257,1,1082350
ext4,1,old,1,0,82796,82577,739,28679,9,49102,3493,2,2,82816
ext4,1,old,1,1,77603,77431,1491,22193,59,48809,5026,2,2,77618
ext4,1,old,8,0,254753,254531,2895,27351,65,204536,17801,9,9,254768
ext4,1,old,8,1,187123,186986,1514,26470,67,143550,12132,9,9,187137
ext4,1,old,32,0,874210,874045,4814,33853,302,722703,99053,33,33,874225
ext4,1,old,32,1,981436,981259,3976,25899,296,835050,83933,33,33,981451
ext4,1,old,128,0,3621424,3621280,25797,22561,1326,3200146,323641,129,129,3621437
ext4,1,old,128,1,3915670,3915375,30359,22889,1238,3382920,427328,129,129,3915685
ext4,1,old,256,0,8032717,8032484,93607,48863,2689,7011634,802310,257,257,8032732
ext4,1,old,256,1,6703639,6703206,46102,39852,2732,5850845,620996,257,257,6703654
ext4,2,new,1,0,55940,55695,1921,37157,8,11839,4789,2,1,55959
ext4,2,new,1,1,49717,49554,3345,20760,9,19526,8094,2,1,49736
ext4,2,new,8,0,362454,362195,7477,30183,361,294209,27749,9,1,362477
ext4,2,new,8,1,256279,256091,2031,23900,67,208865,19283,9,1,256295
ext4,2,new,32,0,132371,132162,8638,26082,226,15947,76381,33,1,132387
ext4,2,new,32,1,124500,124342,3824,16649,297,14814,77177,33,1,124518
ext4,2,new,128,0,386505,386217,21598,24652,989,14397,291314,129,1,386523
ext4,2,new,128,1,519593,519249,33598,40151,1257,95502,326963,129,1,519612
ext4,2,new,256,0,813686,813472,38921,31399,1923,25696,633430,257,1,813703
ext4,2,new,256,1,795232,795075,29584,26269,1903,44810,607368,257,1,795249
ext4,3,old,1,0,89900,89681,890,26857,9,56383,5368,2,2,89920
ext4,3,old,1,1,71402,71104,503,28920,13,38774,2263,2,2,71427
ext4,3,old,8,0,180229,179964,3675,33663,91,115798,24638,9,9,180249
ext4,3,old,8,1,162027,161371,3135,26862,86,110488,18901,9,9,162040
ext4,3,old,32,0,686026,685727,11432,27505,299,571263,67507,33,33,686041
ext4,3,old,32,1,553819,553460,5329,24709,336,456713,53628,33,33,553834
ext4,3,old,128,0,2184051,2183689,25287,19904,1201,1840389,250026,129,129,2184069
ext4,3,old,128,1,2188079,2187928,19283,22254,1697,1832297,250511,129,129,2188093
ext4,3,old,256,0,4911920,4910978,33279,19909,2691,4171312,564774,257,257,4911936
ext4,3,old,256,1,5078950,5078745,42790,21319,2661,4277343,625882,257,257,5078966
ext4,4,new,1,0,53091,52859,818,30036,8,17970,3649,2,1,53119
ext4,4,new,1,1,44123,43963,489,24672,8,13719,4179,2,1,44139
ext4,4,new,8,0,78641,78480,3229,31508,84,16364,25365,9,1,78657
ext4,4,new,8,1,71497,71248,2875,31284,73,11995,23182,9,1,71523
ext4,4,new,32,0,142349,142180,14335,20750,268,17085,89691,33,1,142378
ext4,4,new,32,1,129984,129747,4077,24333,327,13489,77674,33,1,130000
ext4,4,new,128,0,443101,442933,17600,25717,991,11363,355539,129,1,443115
ext4,4,new,128,1,541687,541487,22123,33074,837,123007,337513,129,1,541702
ext4,4,new,256,0,918452,918246,59492,28249,1805,19188,767046,257,1,918466
ext4,4,new,256,1,995082,994869,35057,57081,1912,124774,703605,257,1,995098
ext4,5,old,1,0,57445,57215,584,27769,10,24948,3141,2,2,57462
ext4,5,old,1,1,61765,61567,1548,29550,10,24954,5834,2,2,61793
ext4,5,old,8,0,258346,258045,3793,30580,75,198080,24187,9,9,258362
ext4,5,old,8,1,182560,182280,4378,26760,72,128471,22106,9,9,182576
ext4,5,old,32,0,477399,477238,3642,30615,296,364345,64825,33,33,477414
ext4,5,old,32,1,689882,689736,4840,19476,437,593915,57058,33,33,689898
ext4,5,old,128,0,2656189,2655997,17232,29626,1434,2295891,248545,129,129,2656207
ext4,5,old,128,1,3425964,3425674,21698,35509,1436,3025370,269327,129,129,3425984
ext4,5,old,256,0,7572041,7571653,114304,46824,2839,6545296,800584,257,257,7572064
ext4,5,old,256,1,7337406,7337188,37057,42606,3311,6502129,618458,257,257,7337421
ext4,6,new,1,0,60457,60085,2350,29617,11,22238,6815,2,1,60497
ext4,6,new,1,1,342594,342323,773,33269,9,304466,2750,2,1,342610
ext4,6,new,8,0,88260,88067,1134,17352,66,43734,21881,9,1,88278
ext4,6,new,8,1,177645,177438,1680,30700,73,106525,33637,9,1,177661
ext4,6,new,32,0,158834,158675,3533,35118,288,16735,91146,33,1,158850
ext4,6,new,32,1,368719,367060,13893,21403,469,131640,181453,33,1,368739
ext4,6,new,128,0,409384,409188,20145,23188,980,23662,305274,129,1,409403
ext4,6,new,128,1,425659,425476,17451,29136,945,40727,296385,129,1,425676
ext4,6,new,256,0,939043,938882,32776,26700,4548,108872,648268,257,1,939058
ext4,6,new,256,1,972641,971932,36634,28906,2622,106490,669084,257,1,972660
```

### Artifact checksums and fact checks

Local driver, environment capture, binary identity JSON, execution JSON and
raw stdout/stderr are retained under the new worktree's `target/n1/`.
The raw rows above are the portable measurement data; local artifacts are
not required to recalculate this table. Stdout includes the test harness
output as well as the CSV. Stderr records retained ext4 fixture paths.

| FS | Invocation | Binary | stdout SHA256 | stderr SHA256 |
| --- | ---: | --- | --- | --- |
| tmpfs | 1 | old | `5b7cf7cd8acbd620e25303c2fd5322d2390d4240eaf74bbb0dbeebbf7f7c22ce` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| tmpfs | 2 | new | `b121ed9008d0dc049d024b9b6520f5ead18dd4d64029112b9ab2a65f6d258420` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| tmpfs | 3 | old | `bd75e3ebf107d6ad418b27256bbc79b5cf9cba7629809acb38684f6c05fe2306` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| tmpfs | 4 | new | `ffd24b7b6ad459af110ff8af70e490dd19976161fdf198791ff113e0a3527de6` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| tmpfs | 5 | old | `3cc1f684ce7625ec636e821a5e37ba18d90a324b2d562157820645c2b4193fac` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| tmpfs | 6 | new | `1b6b58cccfef21f61ebe0f8eab1055fa218b138e85ef5f7535a70f345a7f1c41` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| ext4 | 1 | old | `0739d5908a99e79d8832a744afe58f31b47b69aa3975ad8e95e4b9a4bf2ddf72` | `6395bb8d2140ba63f4ccff0e139a3e91bc3c29edbae5d42842400b8c3d33fa2b` |
| ext4 | 2 | new | `cbc31df3028b3c09e838cc7508caed455c1c48482bb04d58910a87fa24024ff1` | `7fc571e629045cc62a4d09aee08604683753a8e7b85256f2f52f7d75aa886f8f` |
| ext4 | 3 | old | `e01593d49e8e80787651927d31b0bad403ad6d75fb0db02f64888c10f2e171e1` | `069b44c4d6968d210b907ae8954038495f4a67d86b1ec25ca26f0dcdc5e6056b` |
| ext4 | 4 | new | `d5b4fe1bb3cdb18dc20f41a0c510880ed9e0217c03af0e726d875c7513b3c3e5` | `20953c582a07490ef8b20453accbf4278583e09ad712ab46e858c405b3b7b2e6` |
| ext4 | 5 | old | `5d3922bf10296d62649e1233957ccc5f718e268930afe10f1af4f1015d14472a` | `2f4f22854236bc3aa15b259cc3cad905b9381d71cfdccb3c167eb93d4d142965` |
| ext4 | 6 | new | `60c54885edd176b410125b41cfca61afeb847ca29e5fdfe909989d51067ab21e` | `69b864b1957bb8f14af615ca08e9d79c56d2bd129c27c5ff5e9e8e7de000e5ce` |

Binary SHA256s were taken before measurement. Source identities were read
with `git rev-parse HEAD` in both worktrees and checked against the specified
full SHAs. The measurement builds used unmodified source trees. Each raw log
hash was recomputed after execution. Parsing found 12 successful invocations,
120 numeric rows, and exactly six rows per filesystem/binary/pane cell; all
report and checkpoint invariants above passed. Median columns are calculated
from the integer rows, without rounding to milliseconds.

Tmpfs fixture directories are dropped by the existing test. Ext4 fixture
directories are retained by the test beneath the worktree's `target/n1/disk`;
no recursive cleanup is performed. The short alias and both build targets
are left for commander-managed cleanup. They are measurement/build residue,
not plugin state or user settings.

## Limitations

Stage intervals overlap. Snapshot includes socket work; persistence runs
within report processing. The reader column times injected reader calls;
the Git column includes heading and repository work. Never sum stage columns
to reconstruct total time. `external_us` surrounds the fixture refresh call;
its gap from `total_us` does not estimate instrumentation overhead.

Alternating binaries controls host identity, filesystem, fixture parameters,
build profile, and temporal ordering. It does not control background load,
cache state, storage scheduling, or the differences between the two source
trees and their instrumentation. Fresh fixture directories do not imply cold
kernel caches. No uninstrumented control or stable tail estimate is provided.
The fake socket excludes Herdr scheduling, storage, and rendering. Real reader
discovery, provider I/O, macOS, crash recovery, and visible latency remain
outside this measurement.

## Local verification

After measurement, the new worktree passed `cargo fmt --all --check`,
`cargo clippy --locked --all-targets -j 2 -- -D warnings`,
`cargo test --locked` (442 unit tests and one integration test passed;
six existing ignored tests), `cargo build --release --locked -j 2`, and
`target/release/herdr-lineup --help`. These checks were not concurrent with
the measurements. The old worktree was used only to build and run the
historical ignored measurement test; its full suite was not rerun.

A second parser read the CSV embedded in this Markdown, compared all 120
rows with the captured logs, checked the 20 cells each contain six rows, and
recomputed all 160 displayed median values by sorting and averaging the two
middle integers. All values matched exactly, including half-microseconds.
Both executable SHA256s and source HEAD SHAs matched after measurement.
Local links, UTF-8 without BOM, LF line endings and `git diff --check` passed.
This is evidence for the record's facts and local checks, not an integration,
publication, acceptance, or live-performance claim.
