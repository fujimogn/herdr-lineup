# Wired-head Git heading latency and child-count matrix

This is a separate observation record for L2/N3, not a comparison with
[the historical 2026-10-23 measurement](git-heading-latency-2026-10-23.md).
It makes no speed-up, child-process reduction, or SLO claim.

## Target and environment

| Field | Value |
|---|---|
| Measurement date | 2026-11-04 (Asia/Tokyo) |
| Measured source | `2eaa4c55c169702e71617aaaf0870d94b30bb317` (fixed `origin/develop` base) |
| Host | `dev`, Linux x86_64, CachyOS |
| Kernel | `7.3.1-1-cachyos`, SMP PREEMPT_DYNAMIC |
| CPU | AMD Ryzen 9 9955HX 16-Core Processor; 16 online logical CPUs, one socket, one thread per core exposed |
| Filesystem | ext4, `/dev/sda2` mounted at `/`, `rw,relatime`; source, target, state and Git fixtures on this filesystem |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, repository-pinned toolchain |
| Git | `2.56.0`, `/home/linuxbrew/.linuxbrew/bin/git` |
| Harness build | Cargo **unoptimized test profile**, incremental disabled, dev/test debug info disabled; not a release-binary benchmark |
| Task storage | `/home/fujimogn/.cache/herdr-lineup-git-n3/runs/f720c1b8d1ddbaa16c726a858f1467b5/work`; managed by `dev-task-temp run`; approved aggregate limit 5,000,000,000 bytes |
| Source-copy integrity | Every archived regular file except `src/publish.rs` matched the fixed Git tree byte-for-byte; only the appendix test was inserted before the test module's final brace |
| Base `src/publish.rs` SHA-256 | `9075752243c46fe8f7a25a84f0d5c004cc6b3cd1208a03dd0a027218f8c3bc6c` |
| Raw CSV SHA-256 | `87f81ce6233f2b08a6384eff3860de14758228c65275292847f97ede4824680a` |

The dedicated documentation worktree started at that exact 40-character SHA.
The separate source copy came from `git archive HEAD` in that worktree, before
documentation edits. No tracked Rust code was edited.

## Method and safety boundary

The measured operation is the actual
`MetadataPublisher::resolve_headings_in_budget` in `src/publish.rs`, including
workspace/cwd resolution, same-cwd sharing, fair queue scheduling, the owned
`RepositoryCache::observe` calls, heading fallback handling and token formatting.
The clock starts immediately before that call and stops immediately after it.
Fixture setup, ref mutation, log reset and reading, CSV writes, compilation,
and warmup are outside the timed interval. The effective cycle Git term is
1,000 ms; the caller envelope is 7 s. `read_heading` uses its production cache
path: `publisher.hooks.git` is asserted absent. The 30 s fallback rule is
executed by this path, but this run does not establish an expiry measurement.
The current contract is described in [Git cache design §9](git-cache-design.md#9-production-implementation-status)
and [coverage L2/N3](review-followup-coverage.md).

Each of 36 cells receives a new publisher/cache/queue, **3 untimed warmup
cycles and 50 measured cycles**, run immediately in sequence without a refresh
interval sleep. There are 1,800 recorded samples and 108 excluded warmup
samples. No filesystem page-cache flush, CPU pinning, load control or randomized
ordering was used. Other host activity was not controlled.

The matrix crosses conditions `normal`, `ref-change`, `slow`, topologies
`same`, `distinct`, `linked`, and workspace counts 0/1/8/32, in that order.
Zero-workspace cells are repeated for each named topology/condition; they
all contain no cwd. `same` uses one shared cwd for every nonzero workspace
count. `distinct` uses N independent repositories/cwd. `linked` uses N
distinct detached linked-worktree cwd of one shared repository. Thus 8 or 32
same-cwd workspaces do **not** represent 8 or 32 unique cwd.

Every repository contains two empty commits A/B and a local
`refs/remotes/origin/main`, with auto-maintenance disabled. No remote is
configured or contacted. The shared repository has 32 linked worktrees even
for smaller cells. Repositories are reused between cells; in-memory publisher
state is not. Ref-change samples alternate the local remote-tracking ref
between A and B before every iteration in each participating repository
(once for the shared repository). This is a ref mutation, not working-tree
file churn or a topology/config/packed-ref mutation. A/B commit timestamps
may coincide; output timestamp correctness is not asserted by this harness.

For slow samples the PATH-only wrapper logs each Git invocation and sleeps
2 s before executing any `rev-parse`. This exceeds the 1 s cycle Git term.
The normal/changed wrapper does no deliberate sleeping. All Git children
pass through that wrapper, which then `exec`s the real Git in the same PID.
Counts are actual wrapper invocations, not `3 × N` estimates. Wrapper shell,
`dirname`, `cat`, and `sleep` descendants are not counted as additional Git
children. A slow invocation is counted even when killed before it execs Git.
Logging and wrapper overhead are inside the timed interval.

The measurement process uses isolated HOME/XDG directories and PATH; fixture
creation disables global/system Git configuration. Production `git_command`
clears its environment and preserves HOME/XDG/PATH/TMPDIR, so its explicit
configuration flags and environment sanitization remain intact. The wrapper
uses only local files. System Git configuration, if present, remains subject
to production behavior because `GIT_CONFIG_NOSYSTEM` is not preserved by
`git_command`. No user configuration is changed.

No Herdr client/server command or socket transport is used. Snapshots contain
only synthetic workspace IDs and fixture paths, and no panes. Publisher
construction performs no state I/O and does not start quota workers. This
harness calls heading resolution directly; it neither runs a daemon nor
attaches to, stops, restarts or quits a user's Herdr session/server. Metadata
delivery and event-to-visible latency are outside the measured boundary.

An initial launcher selected the crate's binary test target, which contains
zero tests. That attempt produced no matrix data and was discarded. The
recorded run explicitly selected the **library** test executable and its
log must show `running 1 test` and the exact test name. The corrected
reproduction driver below selects the library target.

The recorded library test finished in 787.52 s: **1 passed, 0 failed**, with
448 other tests filtered out. Its process exited 0 (`MEASURE_EXIT=0`).
The supervising `dev-task-temp` run
`6ba39658c97b740378a26e40bfe15c26` nevertheless exited 1 with
`state=failed`, reporting adopted descendants terminated by signal 9.
This is consistent with production Git process-group timeout kills reaching
the wrapper's helper processes (including `sleep`), which the subreaper
then observes. The supervisor treats any nonzero adopted-descendant exit
as failure. Its failed status is retained, not relabeled as a successful
run or successful descendant completion. The data and test assertions are
reported separately from that status. No claim of all descendants exiting
zero or of successful task-root reclamation follows from this record.

Source/fixtures/build were created under run
`f720c1b8d1ddbaa16c726a858f1467b5`; its initial launcher ran zero tests and
was marked complete. The subsequent measurement supervisor above reused
that source/fixture directory and explicitly set TMPDIR to it. Run
`5a016afafa0ced42a1de5a579e7fa6a7` holds the small driver and validation
scripts. The task root and all three run records are retained for commander
inspection and cleanup after ACK. Reproduction with the corrected driver
can also produce supervisor failure when the injected delays cause helper
descendants to be killed; inspect the test log and task receipt separately.

## Results

Times use integer microseconds in the raw data, rendered here as milliseconds.
Quantiles use nearest rank: sorted index `ceil(p × 50) - 1` (p50 index 24,
p95 index 47), not interpolation. Counts are per-cycle min–max ranges.
Null counts mean all three workspace heading keys are null, not `—` text;
the harness asserts that distinction for every unresolved heading.

| Condition | Topology | Workspaces / unique cwd | n | Min ms | p50 ms | p95 ms | Max ms | Mean ms | Git children / cycle | Null workspaces / cycle |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| normal | same | 0 / 0 | 50 | 0.006 | 0.008 | 0.021 | 0.040 | 0.011 | 0 | 0 |
| normal | same | 1 / 1 | 50 | 21.203 | 30.944 | 41.552 | 41.597 | 31.089 | 3 | 0 |
| normal | same | 8 / 1 | 50 | 24.057 | 37.704 | 52.106 | 56.775 | 38.708 | 3 | 0 |
| normal | same | 32 / 1 | 50 | 23.492 | 53.892 | 170.853 | 289.593 | 65.415 | 3 | 0 |
| normal | distinct | 0 / 0 | 50 | 0.009 | 0.014 | 0.030 | 0.040 | 0.017 | 0 | 0 |
| normal | distinct | 1 / 1 | 50 | 27.288 | 62.013 | 86.621 | 99.156 | 56.646 | 3 | 0 |
| normal | distinct | 8 / 8 | 50 | 232.979 | 400.834 | 592.199 | 636.991 | 382.861 | 24 | 0 |
| normal | distinct | 32 / 32 | 50 | 944.987 | 1001.236 | 1006.451 | 1333.081 | 1001.478 | 31–96 | 0–5 |
| normal | linked | 0 / 0 | 50 | 0.008 | 0.012 | 0.205 | 0.443 | 0.034 | 0 | 0 |
| normal | linked | 1 / 1 | 50 | 41.444 | 59.314 | 91.818 | 205.922 | 65.830 | 3 | 0 |
| normal | linked | 8 / 8 | 50 | 233.986 | 444.915 | 652.983 | 695.423 | 430.370 | 24 | 0 |
| normal | linked | 32 / 32 | 50 | 620.878 | 882.142 | 1002.372 | 1023.593 | 860.893 | 38–96 | 0 |
| ref-change | same | 0 / 0 | 50 | 0.007 | 0.009 | 0.023 | 0.031 | 0.012 | 0 | 0 |
| ref-change | same | 1 / 1 | 50 | 20.483 | 28.131 | 31.769 | 34.684 | 27.489 | 3 | 0 |
| ref-change | same | 8 / 1 | 50 | 19.780 | 28.778 | 33.579 | 39.096 | 28.343 | 3 | 0 |
| ref-change | same | 32 / 1 | 50 | 17.905 | 29.272 | 48.338 | 58.019 | 31.767 | 3 | 0 |
| ref-change | distinct | 0 / 0 | 50 | 0.007 | 0.008 | 0.020 | 0.053 | 0.010 | 0 | 0 |
| ref-change | distinct | 1 / 1 | 50 | 22.250 | 32.233 | 36.262 | 50.446 | 31.701 | 3 | 0 |
| ref-change | distinct | 8 / 8 | 50 | 216.377 | 245.513 | 268.688 | 273.194 | 246.025 | 24 | 0 |
| ref-change | distinct | 32 / 32 | 50 | 927.846 | 982.315 | 1001.816 | 1003.139 | 979.653 | 66–96 | 0 |
| ref-change | linked | 0 / 0 | 50 | 0.007 | 0.007 | 0.020 | 0.061 | 0.010 | 0 | 0 |
| ref-change | linked | 1 / 1 | 50 | 19.027 | 26.190 | 33.162 | 37.559 | 26.918 | 3 | 0 |
| ref-change | linked | 8 / 8 | 50 | 225.231 | 245.605 | 592.928 | 699.037 | 305.657 | 24 | 0 |
| ref-change | linked | 32 / 32 | 50 | 1000.342 | 1001.349 | 1004.679 | 1062.804 | 1002.808 | 49–92 | 0 |
| slow | same | 0 / 0 | 50 | 0.007 | 0.010 | 0.035 | 0.044 | 0.014 | 0 | 0 |
| slow | same | 1 / 1 | 50 | 1000.475 | 1001.213 | 1003.182 | 1004.509 | 1001.467 | 1 | 1 |
| slow | same | 8 / 1 | 50 | 1000.604 | 1001.466 | 1004.230 | 1007.296 | 1001.878 | 1 | 8 |
| slow | same | 32 / 1 | 50 | 1000.638 | 1001.814 | 1005.248 | 1009.681 | 1002.289 | 1 | 32 |
| slow | distinct | 0 / 0 | 50 | 0.008 | 0.026 | 0.074 | 0.087 | 0.031 | 0 | 0 |
| slow | distinct | 1 / 1 | 50 | 1000.389 | 1001.356 | 1006.619 | 1178.697 | 1007.405 | 1 | 1 |
| slow | distinct | 8 / 8 | 50 | 1000.470 | 1001.226 | 1003.363 | 1072.566 | 1003.516 | 1 | 8 |
| slow | distinct | 32 / 32 | 50 | 1000.739 | 1001.676 | 1003.690 | 1003.745 | 1001.962 | 1 | 32 |
| slow | linked | 0 / 0 | 50 | 0.008 | 0.012 | 0.037 | 0.882 | 0.032 | 0 | 0 |
| slow | linked | 1 / 1 | 50 | 1000.514 | 1002.171 | 1007.454 | 1025.314 | 1002.951 | 1 | 1 |
| slow | linked | 8 / 8 | 50 | 1000.888 | 1001.417 | 1006.377 | 1009.853 | 1002.250 | 1 | 8 |
| slow | linked | 32 / 32 | 50 | 1000.543 | 1001.257 | 1002.334 | 1003.021 | 1001.341 | 1 | 32 |

For each nonempty slow cell, all workspaces are null because no repository
has ever been confirmed by that cell's new publisher. Exactly one Git
invocation consumes the cycle budget per cycle. For `same`, its interrupted
observation is shared with every workspace; for `distinct`/`linked`, one
workspace is attempted and N−1 receive no attempt in that cycle. These are
budget-related null outcomes, with no prewarmed confirmed fallback. The
queue persists across cycles. This does not test a warmed heading surviving
an interruption, nor expiry of that heading at 30 s.

## Limits and interpretation

- In `normal/distinct/32`, null headings occurred only at measured iterations
  27–30, with 2/5/2/2 null workspaces respectively. Their timing is near the
  30 s cache/fallback age, but this harness does not instrument the cause
  and does not establish expiry as the explanation.
- This is one Linux host and local ext4 filesystem, small synthetic repos,
  an unoptimized test build, and instrumentation overhead. No macOS, remote
  filesystem, large ref namespace, real session load or release binary was measured.
- Repeated cycles do not follow the configured 5 s refresh schedule. Cache
  TTL can expire according to actual wall time; it is not forced or frozen.
- Child counts describe this fixture and this code path. Both downstream
  cache hits and misses retain native checks/ref reads and use three Git
  children for a completed repository observation; no reduction is inferred.
- Delay injection covers cancellable user-space sleep and production
  kill/reap behavior, not uninterruptible kernel I/O. The 1 s budget is
  cooperative and is not a strict latency guarantee.
- Null workspace counts do not separately instrument the internal
  `Interrupted`/`NotAttempted` enums. The slow-case attribution above follows
  the code path, the one-invocation assertion and the absence of any prior
  confirmed heading. Normal/ref-change nulls, if present, are reported as
  observations rather than inferred successful work.
- These results are separate from the historical pre-wiring record. They
  establish no comparative improvement, production capacity limit or SLO.

## Reproduction and fact checking

Create a dedicated worktree from the fixed SHA. Save the Python driver below
outside the worktree. Create a private (`0700`) task root on local disk, not
`/tmp`, `/var/tmp`, `/run` or `/dev/shm` (the tool rejects those roots). Run:

```sh
dev-task-temp run --root <private-disk-root> --task-id asg-git-heading-n3-matrix \
  --bytes 5000000000 --inodes 100000 -- /usr/bin/python3 <bootstrap.py>
```

The driver contains the complete added Rust test and Git wrapper template.
The concrete files used for this record are also included below. Replace the
driver's worktree path only when reproducing elsewhere; preserve the fixed
base. It archives the tree, initializes fixtures, builds the library test
target and runs only `publish::tests::wired_git_matrix`. No additional
production source seam is inserted. The wrapper finds its run root from
its own location because production Git commands discard arbitrary ambient
environment variables. One run needs approximately nine nonempty slow cells
× 53 cycles × 1 s, plus normal/ref-change cycles and compilation.

The raw CSV below was parsed independently of summary generation: verify
36 cells, 50 iteration IDs per cell, 1,800 rows, nearest-rank quantiles and
the SHA-256 above. Source-copy checks compared all archived regular files
against the base Git tree and confirmed exactly the appended test in
`src/publish.rs`. An independent Decimal-based script recomputed every
summary quantile, mean and count range from the embedded CSV, checked all
iteration IDs and verified the CSV/source hashes and worktree base SHA.

Candidate checks below ran in the dedicated documentation worktree after
measurement; they are not latency samples. Cargo incremental compilation
and dev/test debug info were disabled, as for the harness build. Build
artifacts remained in that worktree's `target`, not in `/tmp`.

| Command | Observed result |
|---|---|
| `cargo fmt --all --check` | Exit 0 |
| `cargo clippy --locked --all-targets -- -D warnings` | Exit 0 |
| `cargo test --locked` | Final run exit 0: 442 unit tests passed, 6 existing ignored; 1 integration test passed |
| `cargo build --release --locked` | Exit 0 |
| `target/release/herdr-lineup --help` | Exit 0 |

The first full test attempt used the long task work directory plus
`check-temp` as TMPDIR and failed (375 passed, 67 failed, 6 ignored), with
Unix socket `path must be shorter than SUN_LEN` errors. A focused rerun of
`socket::tests::deadline_api_preserves_descriptor_and_frame_boundaries`
then passed with only TMPDIR changed to the shorter disk path
`/home/fujimogn/.cache/ln3t-m2b`. The complete default-parallel test suite
passed with that same short TMPDIR; no source change was made. This fixture
path adjustment does not change the already completed, socket-free heading
measurements. Both failed and successful logs are retained in the task root.

## Appendix A: reproduction driver (complete)

```python
import os, pathlib, subprocess, tarfile, io, json, shutil
r = pathlib.Path(os.environ['DEV_TASK_TEMP_DIR'])
print('RUN_PATH=' + str(r), flush=True)
w = pathlib.Path('/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/git-heading-n3-m2b')
s = r / 'source'; s.mkdir()
tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['git', '-C', str(w), 'archive', 'HEAD']))).extractall(s, filter='data')
(r / '.git').write_text('gitdir: unavailable-fixture-git-dir\n')
for name in ['home', 'xdg', 'cache', 'bin', 'fixtures']:
    (r / name).mkdir()
realgit = shutil.which('git')
wrapper = '''#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
printf '%s\\n' "$*" >> "$root/children.log"
mode=$(cat "$root/mode")
if [ "$mode" = slow ]; then
  for arg in "$@"; do
    if [ "$arg" = rev-parse ]; then sleep 2; break; fi
  done
fi
exec REALGIT "$@"
'''.replace('REALGIT', realgit)
(r / 'bin/git').write_text(wrapper); (r / 'bin/git').chmod(0o755)
env = dict(os.environ, HOME=str(r/'home'), XDG_CONFIG_HOME=str(r/'xdg'), XDG_CACHE_HOME=str(r/'cache'), GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null', GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid', GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')
def git(p, *a):
    return subprocess.check_output([realgit, '-C', str(p), *a], env=env, stderr=subprocess.DEVNULL).decode().strip()
for i in range(33):
    p = r/'fixtures'/('shared' if i == 32 else f'repo-{i}'); p.mkdir()
    git(p, 'init', '-q', '--initial-branch=main')
    git(p, 'config', 'gc.auto', '0'); git(p, 'config', 'maintenance.auto', 'false')
    git(p, '-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-qm', 'A'); a = git(p, 'rev-parse', 'HEAD')
    git(p, '-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-qm', 'B'); b = git(p, 'rev-parse', 'HEAD')
    git(p, 'update-ref', 'refs/remotes/origin/main', a); (p/'oids').write_text(a+'\n'+b+'\n')
    if i == 32:
        for j in range(32):
            git(p, 'worktree', 'add', '--quiet', '--detach', str(r/'fixtures'/f'wt-{j}'))
harness = r'''    #[test]
    #[ignore]
    fn wired_git_matrix() {
        use std::io::Write as _;
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap());
        let mut output = std::fs::File::create(root.join("raw.csv")).unwrap();
        writeln!(output, "condition,topology,workspaces,iteration,us,children,null_workspaces").unwrap();
        for condition in ["normal", "ref-change", "slow"] {
            for topology in ["same", "distinct", "linked"] {
                for n in [0usize, 1, 8, 32] {
                    let paths: Vec<PathBuf> = (0..n).map(|i| root.join("fixtures").join(match topology {
                        "same" => "shared".to_owned(), "distinct" => format!("repo-{i}"), _ => format!("wt-{i}"),
                    })).collect();
                    let state = root.join(format!("state-{condition}-{topology}-{n}"));
                    std::fs::create_dir(&state).unwrap();
                    let mut publisher = budget_publisher(&state, budget_terms(1000, 1000, 1000));
                    assert!(publisher.hooks.git.is_none());
                    let snapshot = protocol::Snapshot {
                        version: "0.9.3".into(), protocol: 17, panes: vec![],
                        workspaces: paths.iter().enumerate().map(|(i,p)| protocol::Workspace {
                            workspace_id: format!("w{i}"), checkout_path: Some(p.to_str().unwrap().into()), stored_keys: BTreeSet::new(),
                        }).collect(),
                    };
                    let inventory = RefreshInventory { snapshot, now_unix: 0, mode: tokens::LogoMode::Text,
                        first_panes: BTreeSet::new(), codex_weekly: None, grok_weekly: None, kimi_weekly: None };
                    for iteration in 0..53 {
                        if condition == "ref-change" {
                            let repos: Vec<PathBuf> = if topology == "distinct" {
                                (0..n).map(|i| root.join(format!("fixtures/repo-{i}"))).collect()
                            } else if n > 0 { vec![root.join("fixtures/shared")] } else { vec![] };
                            for repo in repos {
                                let oids = std::fs::read_to_string(repo.join("oids")).unwrap();
                                let oid = oids.lines().nth(iteration % 2).unwrap();
                                assert!(std::process::Command::new("REALGIT").arg("-C").arg(repo)
                                    .args(["update-ref", "refs/remotes/origin/main", oid]).status().unwrap().success());
                            }
                        }
                        std::fs::write(root.join("mode"), condition).unwrap();
                        std::fs::write(root.join("children.log"), "").unwrap();
                        let started = Instant::now();
                        let headings = publisher.resolve_headings_in_budget(&inventory, started + Duration::from_secs(7), started).unwrap();
                        let us = started.elapsed().as_micros();
                        let children = std::fs::read_to_string(root.join("children.log")).unwrap().lines().count();
                        let nulls = headings.by_workspace.values().filter(|h| !h.resolved).count();
                        assert_eq!(headings.by_workspace.len(), n);
                        assert!(headings.by_workspace.values().filter(|h| !h.resolved).all(|h| h.desired.values().all(Option::is_none)));
                        if condition == "slow" { assert_eq!(nulls,n); assert_eq!(children,usize::from(n>0)); }
                        if iteration >= 3 { writeln!(output,"{condition},{topology},{n},{},{us},{children},{nulls}",iteration-3).unwrap(); output.flush().unwrap(); }
                    }
                    eprintln!("completed {condition}/{topology}/{n}");
                }
            }
        }
    }
'''.replace('REALGIT', realgit)
(r/'harness.rs').write_text(harness)
p = s/'src/publish.rs'; original = p.read_text(); pos = original.rfind('\n}')
p.write_text(original[:pos]+'\n'+harness+original[pos:])
(r/'mode').write_text('normal'); (r/'children.log').write_text('')
buildenv = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
with (r/'compile.log').open('w') as log:
    cp = subprocess.run(['cargo', 'test', '--locked', '--no-run', '--message-format=json'], cwd=s, env=buildenv, stdout=subprocess.PIPE, stderr=log, text=True)
(r/'compile.jsonl').write_text(cp.stdout)
if cp.returncode:
    print((r/'compile.log').read_text(), flush=True); raise SystemExit(cp.returncode)
exe = [json.loads(l)['executable'] for l in cp.stdout.splitlines() if json.loads(l).get('executable') and json.loads(l).get('profile',{}).get('test') and json.loads(l)['target']['kind'] == ['lib']][0]
print('TEST_BINARY='+exe, flush=True)
runenv = dict(env, PATH=str(r/'bin')+':/usr/bin:/bin', TMPDIR=str(r))
with (r/'measure.log').open('w') as log:
    cp = subprocess.run([exe, '--exact', 'publish::tests::wired_git_matrix', '--ignored', '--nocapture', '--test-threads=1'], env=runenv, cwd=s, stdout=log, stderr=subprocess.STDOUT)
print('MEASURE_EXIT='+str(cp.returncode), flush=True)
raise SystemExit(cp.returncode)
```

## Appendix B: added test (complete)

```rust
    #[test]
    #[ignore]
    fn wired_git_matrix() {
        use std::io::Write as _;
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap());
        let mut output = std::fs::File::create(root.join("raw.csv")).unwrap();
        writeln!(output, "condition,topology,workspaces,iteration,us,children,null_workspaces").unwrap();
        for condition in ["normal", "ref-change", "slow"] {
            for topology in ["same", "distinct", "linked"] {
                for n in [0usize, 1, 8, 32] {
                    let paths: Vec<PathBuf> = (0..n).map(|i| root.join("fixtures").join(match topology {
                        "same" => "shared".to_owned(), "distinct" => format!("repo-{i}"), _ => format!("wt-{i}"),
                    })).collect();
                    let state = root.join(format!("state-{condition}-{topology}-{n}"));
                    std::fs::create_dir(&state).unwrap();
                    let mut publisher = budget_publisher(&state, budget_terms(1000, 1000, 1000));
                    assert!(publisher.hooks.git.is_none());
                    let snapshot = protocol::Snapshot {
                        version: "0.9.3".into(), protocol: 17, panes: vec![],
                        workspaces: paths.iter().enumerate().map(|(i,p)| protocol::Workspace {
                            workspace_id: format!("w{i}"), checkout_path: Some(p.to_str().unwrap().into()), stored_keys: BTreeSet::new(),
                        }).collect(),
                    };
                    let inventory = RefreshInventory { snapshot, now_unix: 0, mode: tokens::LogoMode::Text,
                        first_panes: BTreeSet::new(), codex_weekly: None, grok_weekly: None, kimi_weekly: None };
                    for iteration in 0..53 {
                        if condition == "ref-change" {
                            let repos: Vec<PathBuf> = if topology == "distinct" {
                                (0..n).map(|i| root.join(format!("fixtures/repo-{i}"))).collect()
                            } else if n > 0 { vec![root.join("fixtures/shared")] } else { vec![] };
                            for repo in repos {
                                let oids = std::fs::read_to_string(repo.join("oids")).unwrap();
                                let oid = oids.lines().nth(iteration % 2).unwrap();
                                assert!(std::process::Command::new("/home/linuxbrew/.linuxbrew/bin/git").arg("-C").arg(repo)
                                    .args(["update-ref", "refs/remotes/origin/main", oid]).status().unwrap().success());
                            }
                        }
                        std::fs::write(root.join("mode"), condition).unwrap();
                        std::fs::write(root.join("children.log"), "").unwrap();
                        let started = Instant::now();
                        let headings = publisher.resolve_headings_in_budget(&inventory, started + Duration::from_secs(7), started).unwrap();
                        let us = started.elapsed().as_micros();
                        let children = std::fs::read_to_string(root.join("children.log")).unwrap().lines().count();
                        let nulls = headings.by_workspace.values().filter(|h| !h.resolved).count();
                        assert_eq!(headings.by_workspace.len(), n);
                        assert!(headings.by_workspace.values().filter(|h| !h.resolved).all(|h| h.desired.values().all(Option::is_none)));
                        if condition == "slow" { assert_eq!(nulls,n); assert_eq!(children,usize::from(n>0)); }
                        if iteration >= 3 { writeln!(output,"{condition},{topology},{n},{},{us},{children},{nulls}",iteration-3).unwrap(); output.flush().unwrap(); }
                    }
                    eprintln!("completed {condition}/{topology}/{n}");
                }
            }
        }
    }
```

## Appendix C: Git wrapper (complete)

```sh
#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
printf '%s\n' "$*" >> "$root/children.log"
mode=$(cat "$root/mode")
if [ "$mode" = slow ]; then
  for arg in "$@"; do
    if [ "$arg" = rev-parse ]; then sleep 2; break; fi
  done
fi
exec /home/linuxbrew/.linuxbrew/bin/git "$@"
```

## Appendix D: raw samples (complete CSV)

```csv
condition,topology,workspaces,iteration,us,children,null_workspaces
normal,same,0,0,6,0,0
normal,same,0,1,6,0,0
normal,same,0,2,12,0,0
normal,same,0,3,8,0,0
normal,same,0,4,8,0,0
normal,same,0,5,9,0,0
normal,same,0,6,8,0,0
normal,same,0,7,9,0,0
normal,same,0,8,8,0,0
normal,same,0,9,8,0,0
normal,same,0,10,8,0,0
normal,same,0,11,8,0,0
normal,same,0,12,8,0,0
normal,same,0,13,8,0,0
normal,same,0,14,8,0,0
normal,same,0,15,8,0,0
normal,same,0,16,8,0,0
normal,same,0,17,8,0,0
normal,same,0,18,21,0,0
normal,same,0,19,8,0,0
normal,same,0,20,8,0,0
normal,same,0,21,8,0,0
normal,same,0,22,9,0,0
normal,same,0,23,20,0,0
normal,same,0,24,21,0,0
normal,same,0,25,8,0,0
normal,same,0,26,21,0,0
normal,same,0,27,9,0,0
normal,same,0,28,11,0,0
normal,same,0,29,8,0,0
normal,same,0,30,11,0,0
normal,same,0,31,8,0,0
normal,same,0,32,8,0,0
normal,same,0,33,8,0,0
normal,same,0,34,17,0,0
normal,same,0,35,11,0,0
normal,same,0,36,35,0,0
normal,same,0,37,40,0,0
normal,same,0,38,11,0,0
normal,same,0,39,11,0,0
normal,same,0,40,8,0,0
normal,same,0,41,8,0,0
normal,same,0,42,8,0,0
normal,same,0,43,8,0,0
normal,same,0,44,8,0,0
normal,same,0,45,9,0,0
normal,same,0,46,8,0,0
normal,same,0,47,8,0,0
normal,same,0,48,8,0,0
normal,same,0,49,8,0,0
normal,same,1,0,31515,3,0
normal,same,1,1,34918,3,0
normal,same,1,2,31485,3,0
normal,same,1,3,27928,3,0
normal,same,1,4,41552,3,0
normal,same,1,5,35150,3,0
normal,same,1,6,38086,3,0
normal,same,1,7,30098,3,0
normal,same,1,8,38862,3,0
normal,same,1,9,32795,3,0
normal,same,1,10,23666,3,0
normal,same,1,11,35423,3,0
normal,same,1,12,26673,3,0
normal,same,1,13,31241,3,0
normal,same,1,14,41597,3,0
normal,same,1,15,32495,3,0
normal,same,1,16,28494,3,0
normal,same,1,17,28748,3,0
normal,same,1,18,28543,3,0
normal,same,1,19,37028,3,0
normal,same,1,20,41569,3,0
normal,same,1,21,30674,3,0
normal,same,1,22,22512,3,0
normal,same,1,23,29740,3,0
normal,same,1,24,26646,3,0
normal,same,1,25,21203,3,0
normal,same,1,26,27644,3,0
normal,same,1,27,30944,3,0
normal,same,1,28,36101,3,0
normal,same,1,29,27690,3,0
normal,same,1,30,22651,3,0
normal,same,1,31,34423,3,0
normal,same,1,32,31215,3,0
normal,same,1,33,30464,3,0
normal,same,1,34,24464,3,0
normal,same,1,35,34732,3,0
normal,same,1,36,34762,3,0
normal,same,1,37,30190,3,0
normal,same,1,38,23526,3,0
normal,same,1,39,26271,3,0
normal,same,1,40,31513,3,0
normal,same,1,41,26410,3,0
normal,same,1,42,31488,3,0
normal,same,1,43,27696,3,0
normal,same,1,44,38653,3,0
normal,same,1,45,34848,3,0
normal,same,1,46,31945,3,0
normal,same,1,47,32083,3,0
normal,same,1,48,29407,3,0
normal,same,1,49,26692,3,0
normal,same,8,0,31376,3,0
normal,same,8,1,41737,3,0
normal,same,8,2,31511,3,0
normal,same,8,3,24057,3,0
normal,same,8,4,27805,3,0
normal,same,8,5,36983,3,0
normal,same,8,6,31254,3,0
normal,same,8,7,32732,3,0
normal,same,8,8,33456,3,0
normal,same,8,9,32205,3,0
normal,same,8,10,51380,3,0
normal,same,8,11,36049,3,0
normal,same,8,12,33077,3,0
normal,same,8,13,35199,3,0
normal,same,8,14,36285,3,0
normal,same,8,15,33104,3,0
normal,same,8,16,40616,3,0
normal,same,8,17,30567,3,0
normal,same,8,18,31570,3,0
normal,same,8,19,46482,3,0
normal,same,8,20,51945,3,0
normal,same,8,21,40253,3,0
normal,same,8,22,44031,3,0
normal,same,8,23,47383,3,0
normal,same,8,24,53629,3,0
normal,same,8,25,48202,3,0
normal,same,8,26,47717,3,0
normal,same,8,27,43049,3,0
normal,same,8,28,39359,3,0
normal,same,8,29,35509,3,0
normal,same,8,30,56775,3,0
normal,same,8,31,52106,3,0
normal,same,8,32,37879,3,0
normal,same,8,33,44271,3,0
normal,same,8,34,41454,3,0
normal,same,8,35,27778,3,0
normal,same,8,36,24816,3,0
normal,same,8,37,37587,3,0
normal,same,8,38,31619,3,0
normal,same,8,39,35991,3,0
normal,same,8,40,37791,3,0
normal,same,8,41,30491,3,0
normal,same,8,42,40587,3,0
normal,same,8,43,48333,3,0
normal,same,8,44,32067,3,0
normal,same,8,45,41383,3,0
normal,same,8,46,39573,3,0
normal,same,8,47,45532,3,0
normal,same,8,48,37704,3,0
normal,same,8,49,43150,3,0
normal,same,32,0,26943,3,0
normal,same,32,1,32467,3,0
normal,same,32,2,37785,3,0
normal,same,32,3,33533,3,0
normal,same,32,4,30010,3,0
normal,same,32,5,39398,3,0
normal,same,32,6,30644,3,0
normal,same,32,7,23492,3,0
normal,same,32,8,31825,3,0
normal,same,32,9,33697,3,0
normal,same,32,10,47206,3,0
normal,same,32,11,53498,3,0
normal,same,32,12,56607,3,0
normal,same,32,13,37961,3,0
normal,same,32,14,140921,3,0
normal,same,32,15,237843,3,0
normal,same,32,16,65177,3,0
normal,same,32,17,64503,3,0
normal,same,32,18,113932,3,0
normal,same,32,19,44019,3,0
normal,same,32,20,47883,3,0
normal,same,32,21,37043,3,0
normal,same,32,22,40904,3,0
normal,same,32,23,38206,3,0
normal,same,32,24,50920,3,0
normal,same,32,25,49362,3,0
normal,same,32,26,37971,3,0
normal,same,32,27,55570,3,0
normal,same,32,28,75326,3,0
normal,same,32,29,289593,3,0
normal,same,32,30,170853,3,0
normal,same,32,31,83740,3,0
normal,same,32,32,50040,3,0
normal,same,32,33,67734,3,0
normal,same,32,34,52328,3,0
normal,same,32,35,59192,3,0
normal,same,32,36,39522,3,0
normal,same,32,37,53892,3,0
normal,same,32,38,66793,3,0
normal,same,32,39,60214,3,0
normal,same,32,40,56845,3,0
normal,same,32,41,65072,3,0
normal,same,32,42,70489,3,0
normal,same,32,43,77216,3,0
normal,same,32,44,69547,3,0
normal,same,32,45,71936,3,0
normal,same,32,46,68401,3,0
normal,same,32,47,60264,3,0
normal,same,32,48,63814,3,0
normal,same,32,49,58607,3,0
normal,distinct,0,0,9,0,0
normal,distinct,0,1,40,0,0
normal,distinct,0,2,14,0,0
normal,distinct,0,3,21,0,0
normal,distinct,0,4,12,0,0
normal,distinct,0,5,14,0,0
normal,distinct,0,6,10,0,0
normal,distinct,0,7,13,0,0
normal,distinct,0,8,14,0,0
normal,distinct,0,9,26,0,0
normal,distinct,0,10,20,0,0
normal,distinct,0,11,10,0,0
normal,distinct,0,12,10,0,0
normal,distinct,0,13,24,0,0
normal,distinct,0,14,21,0,0
normal,distinct,0,15,11,0,0
normal,distinct,0,16,10,0,0
normal,distinct,0,17,11,0,0
normal,distinct,0,18,22,0,0
normal,distinct,0,19,9,0,0
normal,distinct,0,20,9,0,0
normal,distinct,0,21,25,0,0
normal,distinct,0,22,19,0,0
normal,distinct,0,23,34,0,0
normal,distinct,0,24,9,0,0
normal,distinct,0,25,25,0,0
normal,distinct,0,26,23,0,0
normal,distinct,0,27,11,0,0
normal,distinct,0,28,20,0,0
normal,distinct,0,29,28,0,0
normal,distinct,0,30,11,0,0
normal,distinct,0,31,23,0,0
normal,distinct,0,32,10,0,0
normal,distinct,0,33,10,0,0
normal,distinct,0,34,10,0,0
normal,distinct,0,35,11,0,0
normal,distinct,0,36,28,0,0
normal,distinct,0,37,10,0,0
normal,distinct,0,38,16,0,0
normal,distinct,0,39,26,0,0
normal,distinct,0,40,10,0,0
normal,distinct,0,41,22,0,0
normal,distinct,0,42,30,0,0
normal,distinct,0,43,11,0,0
normal,distinct,0,44,10,0,0
normal,distinct,0,45,14,0,0
normal,distinct,0,46,14,0,0
normal,distinct,0,47,10,0,0
normal,distinct,0,48,14,0,0
normal,distinct,0,49,24,0,0
normal,distinct,1,0,64532,3,0
normal,distinct,1,1,69996,3,0
normal,distinct,1,2,82382,3,0
normal,distinct,1,3,63389,3,0
normal,distinct,1,4,62013,3,0
normal,distinct,1,5,63229,3,0
normal,distinct,1,6,68012,3,0
normal,distinct,1,7,54832,3,0
normal,distinct,1,8,59621,3,0
normal,distinct,1,9,63496,3,0
normal,distinct,1,10,61941,3,0
normal,distinct,1,11,71823,3,0
normal,distinct,1,12,70403,3,0
normal,distinct,1,13,70259,3,0
normal,distinct,1,14,62644,3,0
normal,distinct,1,15,50171,3,0
normal,distinct,1,16,77923,3,0
normal,distinct,1,17,78455,3,0
normal,distinct,1,18,72891,3,0
normal,distinct,1,19,77447,3,0
normal,distinct,1,20,66190,3,0
normal,distinct,1,21,71364,3,0
normal,distinct,1,22,84946,3,0
normal,distinct,1,23,86621,3,0
normal,distinct,1,24,75230,3,0
normal,distinct,1,25,69445,3,0
normal,distinct,1,26,99156,3,0
normal,distinct,1,27,69239,3,0
normal,distinct,1,28,86905,3,0
normal,distinct,1,29,75411,3,0
normal,distinct,1,30,54936,3,0
normal,distinct,1,31,56715,3,0
normal,distinct,1,32,33058,3,0
normal,distinct,1,33,34575,3,0
normal,distinct,1,34,27291,3,0
normal,distinct,1,35,29153,3,0
normal,distinct,1,36,33571,3,0
normal,distinct,1,37,34450,3,0
normal,distinct,1,38,38571,3,0
normal,distinct,1,39,27748,3,0
normal,distinct,1,40,27288,3,0
normal,distinct,1,41,32725,3,0
normal,distinct,1,42,34260,3,0
normal,distinct,1,43,27848,3,0
normal,distinct,1,44,32645,3,0
normal,distinct,1,45,30048,3,0
normal,distinct,1,46,31733,3,0
normal,distinct,1,47,51136,3,0
normal,distinct,1,48,32577,3,0
normal,distinct,1,49,31999,3,0
normal,distinct,8,0,237576,24,0
normal,distinct,8,1,262413,24,0
normal,distinct,8,2,246779,24,0
normal,distinct,8,3,243727,24,0
normal,distinct,8,4,235377,24,0
normal,distinct,8,5,232979,24,0
normal,distinct,8,6,257695,24,0
normal,distinct,8,7,267127,24,0
normal,distinct,8,8,237116,24,0
normal,distinct,8,9,294953,24,0
normal,distinct,8,10,239195,24,0
normal,distinct,8,11,244452,24,0
normal,distinct,8,12,381872,24,0
normal,distinct,8,13,312585,24,0
normal,distinct,8,14,428388,24,0
normal,distinct,8,15,254907,24,0
normal,distinct,8,16,244023,24,0
normal,distinct,8,17,240104,24,0
normal,distinct,8,18,253976,24,0
normal,distinct,8,19,254228,24,0
normal,distinct,8,20,249193,24,0
normal,distinct,8,21,269199,24,0
normal,distinct,8,22,400834,24,0
normal,distinct,8,23,636991,24,0
normal,distinct,8,24,592199,24,0
normal,distinct,8,25,503810,24,0
normal,distinct,8,26,424756,24,0
normal,distinct,8,27,446858,24,0
normal,distinct,8,28,402421,24,0
normal,distinct,8,29,417408,24,0
normal,distinct,8,30,363039,24,0
normal,distinct,8,31,365050,24,0
normal,distinct,8,32,507905,24,0
normal,distinct,8,33,474697,24,0
normal,distinct,8,34,472628,24,0
normal,distinct,8,35,418140,24,0
normal,distinct,8,36,416433,24,0
normal,distinct,8,37,394859,24,0
normal,distinct,8,38,573979,24,0
normal,distinct,8,39,436334,24,0
normal,distinct,8,40,559651,24,0
normal,distinct,8,41,446746,24,0
normal,distinct,8,42,511791,24,0
normal,distinct,8,43,622606,24,0
normal,distinct,8,44,404962,24,0
normal,distinct,8,45,506318,24,0
normal,distinct,8,46,548336,24,0
normal,distinct,8,47,456420,24,0
normal,distinct,8,48,475023,24,0
normal,distinct,8,49,475010,24,0
normal,distinct,32,0,1010983,45,0
normal,distinct,32,1,1002614,38,0
normal,distinct,32,2,1001680,52,0
normal,distinct,32,3,1003066,49,0
normal,distinct,32,4,1000303,90,0
normal,distinct,32,5,983198,96,0
normal,distinct,32,6,1001211,96,0
normal,distinct,32,7,1001286,85,0
normal,distinct,32,8,1000338,95,0
normal,distinct,32,9,1000552,96,0
normal,distinct,32,10,990405,96,0
normal,distinct,32,11,969138,96,0
normal,distinct,32,12,980627,96,0
normal,distinct,32,13,944987,96,0
normal,distinct,32,14,985176,96,0
normal,distinct,32,15,961725,96,0
normal,distinct,32,16,951694,96,0
normal,distinct,32,17,1001409,95,0
normal,distinct,32,18,946879,96,0
normal,distinct,32,19,991449,96,0
normal,distinct,32,20,1002224,90,0
normal,distinct,32,21,1000988,89,0
normal,distinct,32,22,970964,96,0
normal,distinct,32,23,1001236,88,0
normal,distinct,32,24,978066,96,0
normal,distinct,32,25,1000750,96,0
normal,distinct,32,26,1000992,87,0
normal,distinct,32,27,1001647,91,2
normal,distinct,32,28,1001357,45,5
normal,distinct,32,29,1004930,31,2
normal,distinct,32,30,1002807,36,2
normal,distinct,32,31,1006451,39,0
normal,distinct,32,32,1000590,41,0
normal,distinct,32,33,1333081,33,0
normal,distinct,32,34,1002837,39,0
normal,distinct,32,35,1001306,48,0
normal,distinct,32,36,1003273,47,0
normal,distinct,32,37,1003179,45,0
normal,distinct,32,38,1004036,47,0
normal,distinct,32,39,1002320,41,0
normal,distinct,32,40,1000448,49,0
normal,distinct,32,41,1004385,54,0
normal,distinct,32,42,1004635,45,0
normal,distinct,32,43,1001287,42,0
normal,distinct,32,44,1001104,50,0
normal,distinct,32,45,1001588,42,0
normal,distinct,32,46,1004167,45,0
normal,distinct,32,47,1001199,48,0
normal,distinct,32,48,1001113,46,0
normal,distinct,32,49,1002202,55,0
normal,linked,0,0,8,0,0
normal,linked,0,1,10,0,0
normal,linked,0,2,11,0,0
normal,linked,0,3,11,0,0
normal,linked,0,4,12,0,0
normal,linked,0,5,11,0,0
normal,linked,0,6,14,0,0
normal,linked,0,7,13,0,0
normal,linked,0,8,53,0,0
normal,linked,0,9,10,0,0
normal,linked,0,10,10,0,0
normal,linked,0,11,8,0,0
normal,linked,0,12,12,0,0
normal,linked,0,13,9,0,0
normal,linked,0,14,22,0,0
normal,linked,0,15,205,0,0
normal,linked,0,16,12,0,0
normal,linked,0,17,10,0,0
normal,linked,0,18,22,0,0
normal,linked,0,19,21,0,0
normal,linked,0,20,9,0,0
normal,linked,0,21,343,0,0
normal,linked,0,22,10,0,0
normal,linked,0,23,17,0,0
normal,linked,0,24,16,0,0
normal,linked,0,25,10,0,0
normal,linked,0,26,35,0,0
normal,linked,0,27,14,0,0
normal,linked,0,28,17,0,0
normal,linked,0,29,9,0,0
normal,linked,0,30,9,0,0
normal,linked,0,31,27,0,0
normal,linked,0,32,9,0,0
normal,linked,0,33,13,0,0
normal,linked,0,34,11,0,0
normal,linked,0,35,13,0,0
normal,linked,0,36,12,0,0
normal,linked,0,37,12,0,0
normal,linked,0,38,8,0,0
normal,linked,0,39,14,0,0
normal,linked,0,40,29,0,0
normal,linked,0,41,8,0,0
normal,linked,0,42,9,0,0
normal,linked,0,43,443,0,0
normal,linked,0,44,10,0,0
normal,linked,0,45,22,0,0
normal,linked,0,46,9,0,0
normal,linked,0,47,9,0,0
normal,linked,0,48,67,0,0
normal,linked,0,49,12,0,0
normal,linked,1,0,55541,3,0
normal,linked,1,1,53689,3,0
normal,linked,1,2,61703,3,0
normal,linked,1,3,59096,3,0
normal,linked,1,4,53679,3,0
normal,linked,1,5,64877,3,0
normal,linked,1,6,55492,3,0
normal,linked,1,7,41444,3,0
normal,linked,1,8,48706,3,0
normal,linked,1,9,56820,3,0
normal,linked,1,10,51957,3,0
normal,linked,1,11,75119,3,0
normal,linked,1,12,74136,3,0
normal,linked,1,13,57148,3,0
normal,linked,1,14,49208,3,0
normal,linked,1,15,52609,3,0
normal,linked,1,16,65018,3,0
normal,linked,1,17,48987,3,0
normal,linked,1,18,64560,3,0
normal,linked,1,19,73239,3,0
normal,linked,1,20,73863,3,0
normal,linked,1,21,62975,3,0
normal,linked,1,22,57762,3,0
normal,linked,1,23,70811,3,0
normal,linked,1,24,78967,3,0
normal,linked,1,25,55915,3,0
normal,linked,1,26,59314,3,0
normal,linked,1,27,72223,3,0
normal,linked,1,28,91818,3,0
normal,linked,1,29,42743,3,0
normal,linked,1,30,63197,3,0
normal,linked,1,31,57140,3,0
normal,linked,1,32,56534,3,0
normal,linked,1,33,42714,3,0
normal,linked,1,34,42410,3,0
normal,linked,1,35,71466,3,0
normal,linked,1,36,65638,3,0
normal,linked,1,37,49587,3,0
normal,linked,1,38,69907,3,0
normal,linked,1,39,89720,3,0
normal,linked,1,40,63804,3,0
normal,linked,1,41,63183,3,0
normal,linked,1,42,52999,3,0
normal,linked,1,43,52997,3,0
normal,linked,1,44,51807,3,0
normal,linked,1,45,205922,3,0
normal,linked,1,46,124941,3,0
normal,linked,1,47,79112,3,0
normal,linked,1,48,79998,3,0
normal,linked,1,49,79011,3,0
normal,linked,8,0,523798,24,0
normal,linked,8,1,695423,24,0
normal,linked,8,2,652983,24,0
normal,linked,8,3,670104,24,0
normal,linked,8,4,624070,24,0
normal,linked,8,5,545867,24,0
normal,linked,8,6,248371,24,0
normal,linked,8,7,268505,24,0
normal,linked,8,8,256690,24,0
normal,linked,8,9,247236,24,0
normal,linked,8,10,270255,24,0
normal,linked,8,11,541833,24,0
normal,linked,8,12,489014,24,0
normal,linked,8,13,513974,24,0
normal,linked,8,14,503691,24,0
normal,linked,8,15,480771,24,0
normal,linked,8,16,431897,24,0
normal,linked,8,17,417612,24,0
normal,linked,8,18,272937,24,0
normal,linked,8,19,237001,24,0
normal,linked,8,20,233986,24,0
normal,linked,8,21,259945,24,0
normal,linked,8,22,362249,24,0
normal,linked,8,23,503641,24,0
normal,linked,8,24,515897,24,0
normal,linked,8,25,327739,24,0
normal,linked,8,26,377047,24,0
normal,linked,8,27,473410,24,0
normal,linked,8,28,418935,24,0
normal,linked,8,29,571646,24,0
normal,linked,8,30,392623,24,0
normal,linked,8,31,507841,24,0
normal,linked,8,32,489473,24,0
normal,linked,8,33,453216,24,0
normal,linked,8,34,266026,24,0
normal,linked,8,35,278508,24,0
normal,linked,8,36,266152,24,0
normal,linked,8,37,316198,24,0
normal,linked,8,38,450923,24,0
normal,linked,8,39,476335,24,0
normal,linked,8,40,416958,24,0
normal,linked,8,41,453801,24,0
normal,linked,8,42,510278,24,0
normal,linked,8,43,495965,24,0
normal,linked,8,44,573205,24,0
normal,linked,8,45,395724,24,0
normal,linked,8,46,444915,24,0
normal,linked,8,47,424535,24,0
normal,linked,8,48,527902,24,0
normal,linked,8,49,441387,24,0
normal,linked,32,0,973254,96,0
normal,linked,32,1,962259,96,0
normal,linked,32,2,1000786,85,0
normal,linked,32,3,913159,96,0
normal,linked,32,4,1000388,91,0
normal,linked,32,5,983669,96,0
normal,linked,32,6,931210,96,0
normal,linked,32,7,938422,96,0
normal,linked,32,8,1023593,72,0
normal,linked,32,9,1001115,38,0
normal,linked,32,10,1001108,48,0
normal,linked,32,11,1001652,93,0
normal,linked,32,12,1002225,50,0
normal,linked,32,13,1002372,94,0
normal,linked,32,14,962281,96,0
normal,linked,32,15,882142,96,0
normal,linked,32,16,795686,96,0
normal,linked,32,17,750341,96,0
normal,linked,32,18,880276,96,0
normal,linked,32,19,910501,96,0
normal,linked,32,20,809390,96,0
normal,linked,32,21,720575,96,0
normal,linked,32,22,680978,96,0
normal,linked,32,23,856190,96,0
normal,linked,32,24,659667,96,0
normal,linked,32,25,620878,96,0
normal,linked,32,26,704392,96,0
normal,linked,32,27,640147,96,0
normal,linked,32,28,651996,96,0
normal,linked,32,29,667041,96,0
normal,linked,32,30,784019,96,0
normal,linked,32,31,810137,96,0
normal,linked,32,32,815240,96,0
normal,linked,32,33,673495,96,0
normal,linked,32,34,768228,96,0
normal,linked,32,35,779499,96,0
normal,linked,32,36,683064,96,0
normal,linked,32,37,672743,96,0
normal,linked,32,38,673163,96,0
normal,linked,32,39,800999,96,0
normal,linked,32,40,877342,96,0
normal,linked,32,41,1001311,88,0
normal,linked,32,42,1001180,92,0
normal,linked,32,43,958399,96,0
normal,linked,32,44,1003241,85,0
normal,linked,32,45,1000877,95,0
normal,linked,32,46,938457,96,0
normal,linked,32,47,1000603,92,0
normal,linked,32,48,915610,96,0
normal,linked,32,49,959360,96,0
ref-change,same,0,0,8,0,0
ref-change,same,0,1,8,0,0
ref-change,same,0,2,9,0,0
ref-change,same,0,3,7,0,0
ref-change,same,0,4,18,0,0
ref-change,same,0,5,31,0,0
ref-change,same,0,6,9,0,0
ref-change,same,0,7,9,0,0
ref-change,same,0,8,8,0,0
ref-change,same,0,9,21,0,0
ref-change,same,0,10,8,0,0
ref-change,same,0,11,8,0,0
ref-change,same,0,12,8,0,0
ref-change,same,0,13,8,0,0
ref-change,same,0,14,8,0,0
ref-change,same,0,15,9,0,0
ref-change,same,0,16,21,0,0
ref-change,same,0,17,9,0,0
ref-change,same,0,18,21,0,0
ref-change,same,0,19,8,0,0
ref-change,same,0,20,21,0,0
ref-change,same,0,21,9,0,0
ref-change,same,0,22,9,0,0
ref-change,same,0,23,22,0,0
ref-change,same,0,24,9,0,0
ref-change,same,0,25,9,0,0
ref-change,same,0,26,9,0,0
ref-change,same,0,27,23,0,0
ref-change,same,0,28,9,0,0
ref-change,same,0,29,20,0,0
ref-change,same,0,30,20,0,0
ref-change,same,0,31,9,0,0
ref-change,same,0,32,21,0,0
ref-change,same,0,33,9,0,0
ref-change,same,0,34,18,0,0
ref-change,same,0,35,7,0,0
ref-change,same,0,36,7,0,0
ref-change,same,0,37,7,0,0
ref-change,same,0,38,7,0,0
ref-change,same,0,39,17,0,0
ref-change,same,0,40,17,0,0
ref-change,same,0,41,25,0,0
ref-change,same,0,42,7,0,0
ref-change,same,0,43,7,0,0
ref-change,same,0,44,7,0,0
ref-change,same,0,45,7,0,0
ref-change,same,0,46,7,0,0
ref-change,same,0,47,7,0,0
ref-change,same,0,48,7,0,0
ref-change,same,0,49,7,0,0
ref-change,same,1,0,25605,3,0
ref-change,same,1,1,29391,3,0
ref-change,same,1,2,30029,3,0
ref-change,same,1,3,28334,3,0
ref-change,same,1,4,30489,3,0
ref-change,same,1,5,30716,3,0
ref-change,same,1,6,29594,3,0
ref-change,same,1,7,29378,3,0
ref-change,same,1,8,20502,3,0
ref-change,same,1,9,29552,3,0
ref-change,same,1,10,26270,3,0
ref-change,same,1,11,31769,3,0
ref-change,same,1,12,31050,3,0
ref-change,same,1,13,29159,3,0
ref-change,same,1,14,24758,3,0
ref-change,same,1,15,24394,3,0
ref-change,same,1,16,34684,3,0
ref-change,same,1,17,26297,3,0
ref-change,same,1,18,30738,3,0
ref-change,same,1,19,26511,3,0
ref-change,same,1,20,28726,3,0
ref-change,same,1,21,26518,3,0
ref-change,same,1,22,20522,3,0
ref-change,same,1,23,25059,3,0
ref-change,same,1,24,25440,3,0
ref-change,same,1,25,22079,3,0
ref-change,same,1,26,26752,3,0
ref-change,same,1,27,32980,3,0
ref-change,same,1,28,25496,3,0
ref-change,same,1,29,24589,3,0
ref-change,same,1,30,28077,3,0
ref-change,same,1,31,29376,3,0
ref-change,same,1,32,28741,3,0
ref-change,same,1,33,27714,3,0
ref-change,same,1,34,31655,3,0
ref-change,same,1,35,23218,3,0
ref-change,same,1,36,28681,3,0
ref-change,same,1,37,27974,3,0
ref-change,same,1,38,28388,3,0
ref-change,same,1,39,20483,3,0
ref-change,same,1,40,28509,3,0
ref-change,same,1,41,28567,3,0
ref-change,same,1,42,28926,3,0
ref-change,same,1,43,28747,3,0
ref-change,same,1,44,27481,3,0
ref-change,same,1,45,28131,3,0
ref-change,same,1,46,25529,3,0
ref-change,same,1,47,26018,3,0
ref-change,same,1,48,21268,3,0
ref-change,same,1,49,29584,3,0
ref-change,same,8,0,28167,3,0
ref-change,same,8,1,23419,3,0
ref-change,same,8,2,29583,3,0
ref-change,same,8,3,23600,3,0
ref-change,same,8,4,28843,3,0
ref-change,same,8,5,28801,3,0
ref-change,same,8,6,31847,3,0
ref-change,same,8,7,29817,3,0
ref-change,same,8,8,32198,3,0
ref-change,same,8,9,32377,3,0
ref-change,same,8,10,26196,3,0
ref-change,same,8,11,26146,3,0
ref-change,same,8,12,27302,3,0
ref-change,same,8,13,25149,3,0
ref-change,same,8,14,30590,3,0
ref-change,same,8,15,29215,3,0
ref-change,same,8,16,27388,3,0
ref-change,same,8,17,23258,3,0
ref-change,same,8,18,28997,3,0
ref-change,same,8,19,28778,3,0
ref-change,same,8,20,27307,3,0
ref-change,same,8,21,29424,3,0
ref-change,same,8,22,30073,3,0
ref-change,same,8,23,19780,3,0
ref-change,same,8,24,39096,3,0
ref-change,same,8,25,36614,3,0
ref-change,same,8,26,24704,3,0
ref-change,same,8,27,29682,3,0
ref-change,same,8,28,29484,3,0
ref-change,same,8,29,28221,3,0
ref-change,same,8,30,27313,3,0
ref-change,same,8,31,29297,3,0
ref-change,same,8,32,21782,3,0
ref-change,same,8,33,28289,3,0
ref-change,same,8,34,33579,3,0
ref-change,same,8,35,24149,3,0
ref-change,same,8,36,23933,3,0
ref-change,same,8,37,28786,3,0
ref-change,same,8,38,30558,3,0
ref-change,same,8,39,27972,3,0
ref-change,same,8,40,30761,3,0
ref-change,same,8,41,26141,3,0
ref-change,same,8,42,30300,3,0
ref-change,same,8,43,26858,3,0
ref-change,same,8,44,30710,3,0
ref-change,same,8,45,24387,3,0
ref-change,same,8,46,30138,3,0
ref-change,same,8,47,28768,3,0
ref-change,same,8,48,29169,3,0
ref-change,same,8,49,28210,3,0
ref-change,same,32,0,26246,3,0
ref-change,same,32,1,26746,3,0
ref-change,same,32,2,20316,3,0
ref-change,same,32,3,30126,3,0
ref-change,same,32,4,17905,3,0
ref-change,same,32,5,30536,3,0
ref-change,same,32,6,23136,3,0
ref-change,same,32,7,25184,3,0
ref-change,same,32,8,30217,3,0
ref-change,same,32,9,29272,3,0
ref-change,same,32,10,29527,3,0
ref-change,same,32,11,30794,3,0
ref-change,same,32,12,25670,3,0
ref-change,same,32,13,25212,3,0
ref-change,same,32,14,28584,3,0
ref-change,same,32,15,27327,3,0
ref-change,same,32,16,29750,3,0
ref-change,same,32,17,30350,3,0
ref-change,same,32,18,26338,3,0
ref-change,same,32,19,23015,3,0
ref-change,same,32,20,27744,3,0
ref-change,same,32,21,25550,3,0
ref-change,same,32,22,22910,3,0
ref-change,same,32,23,32408,3,0
ref-change,same,32,24,27505,3,0
ref-change,same,32,25,30239,3,0
ref-change,same,32,26,28974,3,0
ref-change,same,32,27,24412,3,0
ref-change,same,32,28,30438,3,0
ref-change,same,32,29,27715,3,0
ref-change,same,32,30,25746,3,0
ref-change,same,32,31,33584,3,0
ref-change,same,32,32,29043,3,0
ref-change,same,32,33,29180,3,0
ref-change,same,32,34,27150,3,0
ref-change,same,32,35,29712,3,0
ref-change,same,32,36,26795,3,0
ref-change,same,32,37,31143,3,0
ref-change,same,32,38,45809,3,0
ref-change,same,32,39,45520,3,0
ref-change,same,32,40,48828,3,0
ref-change,same,32,41,48035,3,0
ref-change,same,32,42,41014,3,0
ref-change,same,32,43,40888,3,0
ref-change,same,32,44,44974,3,0
ref-change,same,32,45,58019,3,0
ref-change,same,32,46,43083,3,0
ref-change,same,32,47,44367,3,0
ref-change,same,32,48,48338,3,0
ref-change,same,32,49,32986,3,0
ref-change,distinct,0,0,8,0,0
ref-change,distinct,0,1,8,0,0
ref-change,distinct,0,2,8,0,0
ref-change,distinct,0,3,8,0,0
ref-change,distinct,0,4,19,0,0
ref-change,distinct,0,5,7,0,0
ref-change,distinct,0,6,11,0,0
ref-change,distinct,0,7,8,0,0
ref-change,distinct,0,8,8,0,0
ref-change,distinct,0,9,8,0,0
ref-change,distinct,0,10,8,0,0
ref-change,distinct,0,11,20,0,0
ref-change,distinct,0,12,20,0,0
ref-change,distinct,0,13,7,0,0
ref-change,distinct,0,14,8,0,0
ref-change,distinct,0,15,8,0,0
ref-change,distinct,0,16,8,0,0
ref-change,distinct,0,17,8,0,0
ref-change,distinct,0,18,8,0,0
ref-change,distinct,0,19,8,0,0
ref-change,distinct,0,20,53,0,0
ref-change,distinct,0,21,8,0,0
ref-change,distinct,0,22,13,0,0
ref-change,distinct,0,23,11,0,0
ref-change,distinct,0,24,12,0,0
ref-change,distinct,0,25,8,0,0
ref-change,distinct,0,26,7,0,0
ref-change,distinct,0,27,8,0,0
ref-change,distinct,0,28,18,0,0
ref-change,distinct,0,29,7,0,0
ref-change,distinct,0,30,7,0,0
ref-change,distinct,0,31,19,0,0
ref-change,distinct,0,32,8,0,0
ref-change,distinct,0,33,7,0,0
ref-change,distinct,0,34,7,0,0
ref-change,distinct,0,35,19,0,0
ref-change,distinct,0,36,7,0,0
ref-change,distinct,0,37,7,0,0
ref-change,distinct,0,38,10,0,0
ref-change,distinct,0,39,7,0,0
ref-change,distinct,0,40,8,0,0
ref-change,distinct,0,41,8,0,0
ref-change,distinct,0,42,11,0,0
ref-change,distinct,0,43,8,0,0
ref-change,distinct,0,44,8,0,0
ref-change,distinct,0,45,7,0,0
ref-change,distinct,0,46,8,0,0
ref-change,distinct,0,47,8,0,0
ref-change,distinct,0,48,7,0,0
ref-change,distinct,0,49,7,0,0
ref-change,distinct,1,0,34659,3,0
ref-change,distinct,1,1,31859,3,0
ref-change,distinct,1,2,30962,3,0
ref-change,distinct,1,3,32713,3,0
ref-change,distinct,1,4,29587,3,0
ref-change,distinct,1,5,34801,3,0
ref-change,distinct,1,6,28290,3,0
ref-change,distinct,1,7,34087,3,0
ref-change,distinct,1,8,32658,3,0
ref-change,distinct,1,9,29704,3,0
ref-change,distinct,1,10,36062,3,0
ref-change,distinct,1,11,35142,3,0
ref-change,distinct,1,12,33068,3,0
ref-change,distinct,1,13,28746,3,0
ref-change,distinct,1,14,27664,3,0
ref-change,distinct,1,15,29378,3,0
ref-change,distinct,1,16,33465,3,0
ref-change,distinct,1,17,30461,3,0
ref-change,distinct,1,18,32302,3,0
ref-change,distinct,1,19,33224,3,0
ref-change,distinct,1,20,31130,3,0
ref-change,distinct,1,21,29605,3,0
ref-change,distinct,1,22,50446,3,0
ref-change,distinct,1,23,26547,3,0
ref-change,distinct,1,24,28599,3,0
ref-change,distinct,1,25,32713,3,0
ref-change,distinct,1,26,36262,3,0
ref-change,distinct,1,27,36462,3,0
ref-change,distinct,1,28,22250,3,0
ref-change,distinct,1,29,32233,3,0
ref-change,distinct,1,30,26830,3,0
ref-change,distinct,1,31,32624,3,0
ref-change,distinct,1,32,34582,3,0
ref-change,distinct,1,33,33806,3,0
ref-change,distinct,1,34,33872,3,0
ref-change,distinct,1,35,33041,3,0
ref-change,distinct,1,36,30720,3,0
ref-change,distinct,1,37,28256,3,0
ref-change,distinct,1,38,31028,3,0
ref-change,distinct,1,39,33085,3,0
ref-change,distinct,1,40,28060,3,0
ref-change,distinct,1,41,25202,3,0
ref-change,distinct,1,42,25852,3,0
ref-change,distinct,1,43,32950,3,0
ref-change,distinct,1,44,33250,3,0
ref-change,distinct,1,45,31624,3,0
ref-change,distinct,1,46,29137,3,0
ref-change,distinct,1,47,32524,3,0
ref-change,distinct,1,48,29722,3,0
ref-change,distinct,1,49,33792,3,0
ref-change,distinct,8,0,231033,24,0
ref-change,distinct,8,1,225322,24,0
ref-change,distinct,8,2,268277,24,0
ref-change,distinct,8,3,256327,24,0
ref-change,distinct,8,4,237138,24,0
ref-change,distinct,8,5,235237,24,0
ref-change,distinct,8,6,225288,24,0
ref-change,distinct,8,7,225850,24,0
ref-change,distinct,8,8,216377,24,0
ref-change,distinct,8,9,244212,24,0
ref-change,distinct,8,10,232700,24,0
ref-change,distinct,8,11,252105,24,0
ref-change,distinct,8,12,257675,24,0
ref-change,distinct,8,13,273194,24,0
ref-change,distinct,8,14,261701,24,0
ref-change,distinct,8,15,244181,24,0
ref-change,distinct,8,16,255435,24,0
ref-change,distinct,8,17,229341,24,0
ref-change,distinct,8,18,218499,24,0
ref-change,distinct,8,19,240934,24,0
ref-change,distinct,8,20,235577,24,0
ref-change,distinct,8,21,235078,24,0
ref-change,distinct,8,22,247350,24,0
ref-change,distinct,8,23,244634,24,0
ref-change,distinct,8,24,261180,24,0
ref-change,distinct,8,25,253730,24,0
ref-change,distinct,8,26,259539,24,0
ref-change,distinct,8,27,242657,24,0
ref-change,distinct,8,28,240913,24,0
ref-change,distinct,8,29,266515,24,0
ref-change,distinct,8,30,246166,24,0
ref-change,distinct,8,31,255300,24,0
ref-change,distinct,8,32,255327,24,0
ref-change,distinct,8,33,232664,24,0
ref-change,distinct,8,34,254975,24,0
ref-change,distinct,8,35,257750,24,0
ref-change,distinct,8,36,268688,24,0
ref-change,distinct,8,37,272208,24,0
ref-change,distinct,8,38,254363,24,0
ref-change,distinct,8,39,256517,24,0
ref-change,distinct,8,40,245513,24,0
ref-change,distinct,8,41,229377,24,0
ref-change,distinct,8,42,247745,24,0
ref-change,distinct,8,43,226404,24,0
ref-change,distinct,8,44,248305,24,0
ref-change,distinct,8,45,251770,24,0
ref-change,distinct,8,46,262239,24,0
ref-change,distinct,8,47,240578,24,0
ref-change,distinct,8,48,236566,24,0
ref-change,distinct,8,49,240810,24,0
ref-change,distinct,32,0,998894,96,0
ref-change,distinct,32,1,970884,96,0
ref-change,distinct,32,2,942414,96,0
ref-change,distinct,32,3,956295,96,0
ref-change,distinct,32,4,1000545,84,0
ref-change,distinct,32,5,1001816,94,0
ref-change,distinct,32,6,974582,96,0
ref-change,distinct,32,7,971571,96,0
ref-change,distinct,32,8,1000870,94,0
ref-change,distinct,32,9,990387,96,0
ref-change,distinct,32,10,967477,96,0
ref-change,distinct,32,11,983444,96,0
ref-change,distinct,32,12,963969,96,0
ref-change,distinct,32,13,1001628,95,0
ref-change,distinct,32,14,927846,96,0
ref-change,distinct,32,15,949611,96,0
ref-change,distinct,32,16,1000114,96,0
ref-change,distinct,32,17,1003139,66,0
ref-change,distinct,32,18,1000937,66,0
ref-change,distinct,32,19,1000618,90,0
ref-change,distinct,32,20,967189,96,0
ref-change,distinct,32,21,971013,96,0
ref-change,distinct,32,22,950646,96,0
ref-change,distinct,32,23,954493,96,0
ref-change,distinct,32,24,995486,96,0
ref-change,distinct,32,25,987712,96,0
ref-change,distinct,32,26,964959,96,0
ref-change,distinct,32,27,981899,96,0
ref-change,distinct,32,28,1000531,87,0
ref-change,distinct,32,29,935667,96,0
ref-change,distinct,32,30,999572,96,0
ref-change,distinct,32,31,983568,96,0
ref-change,distinct,32,32,979170,96,0
ref-change,distinct,32,33,966334,96,0
ref-change,distinct,32,34,1002499,76,0
ref-change,distinct,32,35,1000385,88,0
ref-change,distinct,32,36,982482,96,0
ref-change,distinct,32,37,1001455,91,0
ref-change,distinct,32,38,994871,96,0
ref-change,distinct,32,39,1001136,94,0
ref-change,distinct,32,40,962610,96,0
ref-change,distinct,32,41,972308,96,0
ref-change,distinct,32,42,978719,96,0
ref-change,distinct,32,43,985885,96,0
ref-change,distinct,32,44,1001222,95,0
ref-change,distinct,32,45,982315,96,0
ref-change,distinct,32,46,965943,96,0
ref-change,distinct,32,47,959953,96,0
ref-change,distinct,32,48,988792,96,0
ref-change,distinct,32,49,956770,96,0
ref-change,linked,0,0,61,0,0
ref-change,linked,0,1,10,0,0
ref-change,linked,0,2,10,0,0
ref-change,linked,0,3,7,0,0
ref-change,linked,0,4,19,0,0
ref-change,linked,0,5,7,0,0
ref-change,linked,0,6,19,0,0
ref-change,linked,0,7,8,0,0
ref-change,linked,0,8,8,0,0
ref-change,linked,0,9,7,0,0
ref-change,linked,0,10,8,0,0
ref-change,linked,0,11,20,0,0
ref-change,linked,0,12,8,0,0
ref-change,linked,0,13,7,0,0
ref-change,linked,0,14,7,0,0
ref-change,linked,0,15,19,0,0
ref-change,linked,0,16,19,0,0
ref-change,linked,0,17,9,0,0
ref-change,linked,0,18,7,0,0
ref-change,linked,0,19,7,0,0
ref-change,linked,0,20,20,0,0
ref-change,linked,0,21,7,0,0
ref-change,linked,0,22,7,0,0
ref-change,linked,0,23,7,0,0
ref-change,linked,0,24,7,0,0
ref-change,linked,0,25,7,0,0
ref-change,linked,0,26,7,0,0
ref-change,linked,0,27,7,0,0
ref-change,linked,0,28,10,0,0
ref-change,linked,0,29,7,0,0
ref-change,linked,0,30,7,0,0
ref-change,linked,0,31,8,0,0
ref-change,linked,0,32,7,0,0
ref-change,linked,0,33,8,0,0
ref-change,linked,0,34,8,0,0
ref-change,linked,0,35,7,0,0
ref-change,linked,0,36,7,0,0
ref-change,linked,0,37,7,0,0
ref-change,linked,0,38,8,0,0
ref-change,linked,0,39,8,0,0
ref-change,linked,0,40,7,0,0
ref-change,linked,0,41,8,0,0
ref-change,linked,0,42,7,0,0
ref-change,linked,0,43,7,0,0
ref-change,linked,0,44,7,0,0
ref-change,linked,0,45,7,0,0
ref-change,linked,0,46,18,0,0
ref-change,linked,0,47,7,0,0
ref-change,linked,0,48,7,0,0
ref-change,linked,0,49,9,0,0
ref-change,linked,1,0,25123,3,0
ref-change,linked,1,1,33162,3,0
ref-change,linked,1,2,28272,3,0
ref-change,linked,1,3,31399,3,0
ref-change,linked,1,4,22732,3,0
ref-change,linked,1,5,28688,3,0
ref-change,linked,1,6,21276,3,0
ref-change,linked,1,7,29043,3,0
ref-change,linked,1,8,21766,3,0
ref-change,linked,1,9,23321,3,0
ref-change,linked,1,10,29172,3,0
ref-change,linked,1,11,28786,3,0
ref-change,linked,1,12,29211,3,0
ref-change,linked,1,13,29222,3,0
ref-change,linked,1,14,25042,3,0
ref-change,linked,1,15,23526,3,0
ref-change,linked,1,16,31567,3,0
ref-change,linked,1,17,24848,3,0
ref-change,linked,1,18,29541,3,0
ref-change,linked,1,19,28710,3,0
ref-change,linked,1,20,24224,3,0
ref-change,linked,1,21,19027,3,0
ref-change,linked,1,22,26574,3,0
ref-change,linked,1,23,28698,3,0
ref-change,linked,1,24,22123,3,0
ref-change,linked,1,25,21746,3,0
ref-change,linked,1,26,28252,3,0
ref-change,linked,1,27,37559,3,0
ref-change,linked,1,28,25810,3,0
ref-change,linked,1,29,24876,3,0
ref-change,linked,1,30,26246,3,0
ref-change,linked,1,31,24831,3,0
ref-change,linked,1,32,30980,3,0
ref-change,linked,1,33,22213,3,0
ref-change,linked,1,34,29645,3,0
ref-change,linked,1,35,28045,3,0
ref-change,linked,1,36,25403,3,0
ref-change,linked,1,37,26190,3,0
ref-change,linked,1,38,26632,3,0
ref-change,linked,1,39,23845,3,0
ref-change,linked,1,40,22309,3,0
ref-change,linked,1,41,32593,3,0
ref-change,linked,1,42,25261,3,0
ref-change,linked,1,43,25257,3,0
ref-change,linked,1,44,25478,3,0
ref-change,linked,1,45,29668,3,0
ref-change,linked,1,46,30739,3,0
ref-change,linked,1,47,24984,3,0
ref-change,linked,1,48,24885,3,0
ref-change,linked,1,49,37383,3,0
ref-change,linked,8,0,253800,24,0
ref-change,linked,8,1,235657,24,0
ref-change,linked,8,2,253456,24,0
ref-change,linked,8,3,252604,24,0
ref-change,linked,8,4,247444,24,0
ref-change,linked,8,5,239522,24,0
ref-change,linked,8,6,236500,24,0
ref-change,linked,8,7,255584,24,0
ref-change,linked,8,8,253424,24,0
ref-change,linked,8,9,244111,24,0
ref-change,linked,8,10,256024,24,0
ref-change,linked,8,11,233259,24,0
ref-change,linked,8,12,225331,24,0
ref-change,linked,8,13,245413,24,0
ref-change,linked,8,14,245590,24,0
ref-change,linked,8,15,233089,24,0
ref-change,linked,8,16,225231,24,0
ref-change,linked,8,17,236321,24,0
ref-change,linked,8,18,241728,24,0
ref-change,linked,8,19,227669,24,0
ref-change,linked,8,20,230363,24,0
ref-change,linked,8,21,236919,24,0
ref-change,linked,8,22,237545,24,0
ref-change,linked,8,23,238410,24,0
ref-change,linked,8,24,236539,24,0
ref-change,linked,8,25,252483,24,0
ref-change,linked,8,26,236792,24,0
ref-change,linked,8,27,235342,24,0
ref-change,linked,8,28,242300,24,0
ref-change,linked,8,29,245143,24,0
ref-change,linked,8,30,245605,24,0
ref-change,linked,8,31,239199,24,0
ref-change,linked,8,32,226101,24,0
ref-change,linked,8,33,509336,24,0
ref-change,linked,8,34,699037,24,0
ref-change,linked,8,35,583993,24,0
ref-change,linked,8,36,433111,24,0
ref-change,linked,8,37,548222,24,0
ref-change,linked,8,38,592928,24,0
ref-change,linked,8,39,679086,24,0
ref-change,linked,8,40,448944,24,0
ref-change,linked,8,41,336321,24,0
ref-change,linked,8,42,330848,24,0
ref-change,linked,8,43,360126,24,0
ref-change,linked,8,44,322770,24,0
ref-change,linked,8,45,294076,24,0
ref-change,linked,8,46,316580,24,0
ref-change,linked,8,47,283572,24,0
ref-change,linked,8,48,267225,24,0
ref-change,linked,8,49,332172,24,0
ref-change,linked,32,0,1000773,78,0
ref-change,linked,32,1,1000930,80,0
ref-change,linked,32,2,1001608,79,0
ref-change,linked,32,3,1002078,76,0
ref-change,linked,32,4,1001363,72,0
ref-change,linked,32,5,1001327,74,0
ref-change,linked,32,6,1000342,75,0
ref-change,linked,32,7,1001708,72,0
ref-change,linked,32,8,1001208,68,0
ref-change,linked,32,9,1001394,80,0
ref-change,linked,32,10,1001305,92,0
ref-change,linked,32,11,1001756,71,0
ref-change,linked,32,12,1001703,74,0
ref-change,linked,32,13,1000914,76,0
ref-change,linked,32,14,1001349,67,0
ref-change,linked,32,15,1062804,55,0
ref-change,linked,32,16,1003221,65,0
ref-change,linked,32,17,1002271,82,0
ref-change,linked,32,18,1001094,81,0
ref-change,linked,32,19,1001271,92,0
ref-change,linked,32,20,1000649,71,0
ref-change,linked,32,21,1000398,63,0
ref-change,linked,32,22,1003447,60,0
ref-change,linked,32,23,1003245,66,0
ref-change,linked,32,24,1002030,67,0
ref-change,linked,32,25,1001847,67,0
ref-change,linked,32,26,1001445,62,0
ref-change,linked,32,27,1000879,64,0
ref-change,linked,32,28,1001704,69,0
ref-change,linked,32,29,1001224,61,0
ref-change,linked,32,30,1001712,63,0
ref-change,linked,32,31,1000990,68,0
ref-change,linked,32,32,1000880,80,0
ref-change,linked,32,33,1005422,81,0
ref-change,linked,32,34,1000782,72,0
ref-change,linked,32,35,1002532,77,0
ref-change,linked,32,36,1000556,49,0
ref-change,linked,32,37,1001394,66,0
ref-change,linked,32,38,1000439,78,0
ref-change,linked,32,39,1002994,80,0
ref-change,linked,32,40,1000453,64,0
ref-change,linked,32,41,1000435,69,0
ref-change,linked,32,42,1000876,69,0
ref-change,linked,32,43,1001450,78,0
ref-change,linked,32,44,1001046,88,0
ref-change,linked,32,45,1001812,78,0
ref-change,linked,32,46,1003288,88,0
ref-change,linked,32,47,1000600,78,0
ref-change,linked,32,48,1004679,73,0
ref-change,linked,32,49,1000753,80,0
slow,same,0,0,9,0,0
slow,same,0,1,9,0,0
slow,same,0,2,12,0,0
slow,same,0,3,11,0,0
slow,same,0,4,9,0,0
slow,same,0,5,23,0,0
slow,same,0,6,8,0,0
slow,same,0,7,8,0,0
slow,same,0,8,8,0,0
slow,same,0,9,8,0,0
slow,same,0,10,11,0,0
slow,same,0,11,19,0,0
slow,same,0,12,19,0,0
slow,same,0,13,19,0,0
slow,same,0,14,33,0,0
slow,same,0,15,10,0,0
slow,same,0,16,35,0,0
slow,same,0,17,8,0,0
slow,same,0,18,10,0,0
slow,same,0,19,10,0,0
slow,same,0,20,10,0,0
slow,same,0,21,8,0,0
slow,same,0,22,19,0,0
slow,same,0,23,7,0,0
slow,same,0,24,9,0,0
slow,same,0,25,8,0,0
slow,same,0,26,20,0,0
slow,same,0,27,20,0,0
slow,same,0,28,44,0,0
slow,same,0,29,10,0,0
slow,same,0,30,13,0,0
slow,same,0,31,11,0,0
slow,same,0,32,11,0,0
slow,same,0,33,11,0,0
slow,same,0,34,11,0,0
slow,same,0,35,13,0,0
slow,same,0,36,23,0,0
slow,same,0,37,9,0,0
slow,same,0,38,10,0,0
slow,same,0,39,9,0,0
slow,same,0,40,9,0,0
slow,same,0,41,10,0,0
slow,same,0,42,9,0,0
slow,same,0,43,13,0,0
slow,same,0,44,40,0,0
slow,same,0,45,7,0,0
slow,same,0,46,8,0,0
slow,same,0,47,8,0,0
slow,same,0,48,7,0,0
slow,same,0,49,12,0,0
slow,same,1,0,1001199,1,1
slow,same,1,1,1001012,1,1
slow,same,1,2,1001034,1,1
slow,same,1,3,1000969,1,1
slow,same,1,4,1001458,1,1
slow,same,1,5,1000475,1,1
slow,same,1,6,1001700,1,1
slow,same,1,7,1001353,1,1
slow,same,1,8,1001651,1,1
slow,same,1,9,1001430,1,1
slow,same,1,10,1000914,1,1
slow,same,1,11,1001103,1,1
slow,same,1,12,1001390,1,1
slow,same,1,13,1000836,1,1
slow,same,1,14,1001861,1,1
slow,same,1,15,1000890,1,1
slow,same,1,16,1000716,1,1
slow,same,1,17,1001479,1,1
slow,same,1,18,1001433,1,1
slow,same,1,19,1001733,1,1
slow,same,1,20,1001107,1,1
slow,same,1,21,1001962,1,1
slow,same,1,22,1000859,1,1
slow,same,1,23,1001676,1,1
slow,same,1,24,1002156,1,1
slow,same,1,25,1001141,1,1
slow,same,1,26,1000926,1,1
slow,same,1,27,1000818,1,1
slow,same,1,28,1004509,1,1
slow,same,1,29,1002212,1,1
slow,same,1,30,1000655,1,1
slow,same,1,31,1000887,1,1
slow,same,1,32,1001213,1,1
slow,same,1,33,1001335,1,1
slow,same,1,34,1000772,1,1
slow,same,1,35,1003182,1,1
slow,same,1,36,1001088,1,1
slow,same,1,37,1000830,1,1
slow,same,1,38,1000921,1,1
slow,same,1,39,1001399,1,1
slow,same,1,40,1000952,1,1
slow,same,1,41,1001133,1,1
slow,same,1,42,1001474,1,1
slow,same,1,43,1003028,1,1
slow,same,1,44,1001348,1,1
slow,same,1,45,1002020,1,1
slow,same,1,46,1003151,1,1
slow,same,1,47,1003515,1,1
slow,same,1,48,1001178,1,1
slow,same,1,49,1001258,1,1
slow,same,8,0,1002166,1,8
slow,same,8,1,1002051,1,8
slow,same,8,2,1000918,1,8
slow,same,8,3,1000896,1,8
slow,same,8,4,1000964,1,8
slow,same,8,5,1002764,1,8
slow,same,8,6,1001098,1,8
slow,same,8,7,1002137,1,8
slow,same,8,8,1003553,1,8
slow,same,8,9,1003326,1,8
slow,same,8,10,1002865,1,8
slow,same,8,11,1000725,1,8
slow,same,8,12,1002486,1,8
slow,same,8,13,1003266,1,8
slow,same,8,14,1000626,1,8
slow,same,8,15,1000871,1,8
slow,same,8,16,1001312,1,8
slow,same,8,17,1001183,1,8
slow,same,8,18,1001159,1,8
slow,same,8,19,1000986,1,8
slow,same,8,20,1000941,1,8
slow,same,8,21,1001173,1,8
slow,same,8,22,1001345,1,8
slow,same,8,23,1001221,1,8
slow,same,8,24,1000814,1,8
slow,same,8,25,1000614,1,8
slow,same,8,26,1002242,1,8
slow,same,8,27,1002299,1,8
slow,same,8,28,1007296,1,8
slow,same,8,29,1000726,1,8
slow,same,8,30,1000689,1,8
slow,same,8,31,1000604,1,8
slow,same,8,32,1001195,1,8
slow,same,8,33,1001466,1,8
slow,same,8,34,1001290,1,8
slow,same,8,35,1001281,1,8
slow,same,8,36,1004230,1,8
slow,same,8,37,1005462,1,8
slow,same,8,38,1002638,1,8
slow,same,8,39,1001775,1,8
slow,same,8,40,1001615,1,8
slow,same,8,41,1001031,1,8
slow,same,8,42,1002138,1,8
slow,same,8,43,1003660,1,8
slow,same,8,44,1001471,1,8
slow,same,8,45,1002244,1,8
slow,same,8,46,1002330,1,8
slow,same,8,47,1001507,1,8
slow,same,8,48,1001683,1,8
slow,same,8,49,1001582,1,8
slow,same,32,0,1001737,1,32
slow,same,32,1,1001193,1,32
slow,same,32,2,1004911,1,32
slow,same,32,3,1001681,1,32
slow,same,32,4,1001130,1,32
slow,same,32,5,1009681,1,32
slow,same,32,6,1000697,1,32
slow,same,32,7,1001433,1,32
slow,same,32,8,1001800,1,32
slow,same,32,9,1000704,1,32
slow,same,32,10,1000638,1,32
slow,same,32,11,1000726,1,32
slow,same,32,12,1001873,1,32
slow,same,32,13,1002218,1,32
slow,same,32,14,1001475,1,32
slow,same,32,15,1001332,1,32
slow,same,32,16,1001859,1,32
slow,same,32,17,1002864,1,32
slow,same,32,18,1005485,1,32
slow,same,32,19,1002002,1,32
slow,same,32,20,1002340,1,32
slow,same,32,21,1003102,1,32
slow,same,32,22,1004191,1,32
slow,same,32,23,1002504,1,32
slow,same,32,24,1003822,1,32
slow,same,32,25,1001664,1,32
slow,same,32,26,1005248,1,32
slow,same,32,27,1002004,1,32
slow,same,32,28,1002318,1,32
slow,same,32,29,1002420,1,32
slow,same,32,30,1001016,1,32
slow,same,32,31,1003018,1,32
slow,same,32,32,1002394,1,32
slow,same,32,33,1004431,1,32
slow,same,32,34,1001814,1,32
slow,same,32,35,1001166,1,32
slow,same,32,36,1001535,1,32
slow,same,32,37,1001904,1,32
slow,same,32,38,1002057,1,32
slow,same,32,39,1002698,1,32
slow,same,32,40,1003030,1,32
slow,same,32,41,1001622,1,32
slow,same,32,42,1001487,1,32
slow,same,32,43,1001062,1,32
slow,same,32,44,1000861,1,32
slow,same,32,45,1001152,1,32
slow,same,32,46,1001751,1,32
slow,same,32,47,1000854,1,32
slow,same,32,48,1001457,1,32
slow,same,32,49,1004101,1,32
slow,distinct,0,0,24,0,0
slow,distinct,0,1,74,0,0
slow,distinct,0,2,52,0,0
slow,distinct,0,3,12,0,0
slow,distinct,0,4,10,0,0
slow,distinct,0,5,39,0,0
slow,distinct,0,6,37,0,0
slow,distinct,0,7,36,0,0
slow,distinct,0,8,13,0,0
slow,distinct,0,9,37,0,0
slow,distinct,0,10,35,0,0
slow,distinct,0,11,13,0,0
slow,distinct,0,12,36,0,0
slow,distinct,0,13,58,0,0
slow,distinct,0,14,13,0,0
slow,distinct,0,15,9,0,0
slow,distinct,0,16,12,0,0
slow,distinct,0,17,20,0,0
slow,distinct,0,18,27,0,0
slow,distinct,0,19,30,0,0
slow,distinct,0,20,19,0,0
slow,distinct,0,21,32,0,0
slow,distinct,0,22,32,0,0
slow,distinct,0,23,26,0,0
slow,distinct,0,24,12,0,0
slow,distinct,0,25,8,0,0
slow,distinct,0,26,48,0,0
slow,distinct,0,27,87,0,0
slow,distinct,0,28,80,0,0
slow,distinct,0,29,19,0,0
slow,distinct,0,30,57,0,0
slow,distinct,0,31,25,0,0
slow,distinct,0,32,20,0,0
slow,distinct,0,33,27,0,0
slow,distinct,0,34,40,0,0
slow,distinct,0,35,13,0,0
slow,distinct,0,36,10,0,0
slow,distinct,0,37,13,0,0
slow,distinct,0,38,64,0,0
slow,distinct,0,39,44,0,0
slow,distinct,0,40,12,0,0
slow,distinct,0,41,15,0,0
slow,distinct,0,42,65,0,0
slow,distinct,0,43,19,0,0
slow,distinct,0,44,10,0,0
slow,distinct,0,45,45,0,0
slow,distinct,0,46,12,0,0
slow,distinct,0,47,29,0,0
slow,distinct,0,48,56,0,0
slow,distinct,0,49,12,0,0
slow,distinct,1,0,1001510,1,1
slow,distinct,1,1,1001052,1,1
slow,distinct,1,2,1001458,1,1
slow,distinct,1,3,1000781,1,1
slow,distinct,1,4,1001142,1,1
slow,distinct,1,5,1002630,1,1
slow,distinct,1,6,1114499,1,1
slow,distinct,1,7,1002545,1,1
slow,distinct,1,8,1001933,1,1
slow,distinct,1,9,1003635,1,1
slow,distinct,1,10,1000568,1,1
slow,distinct,1,11,1003045,1,1
slow,distinct,1,12,1003470,1,1
slow,distinct,1,13,1001929,1,1
slow,distinct,1,14,1006619,1,1
slow,distinct,1,15,1002908,1,1
slow,distinct,1,16,1001213,1,1
slow,distinct,1,17,1001835,1,1
slow,distinct,1,18,1001347,1,1
slow,distinct,1,19,1000877,1,1
slow,distinct,1,20,1002548,1,1
slow,distinct,1,21,1002016,1,1
slow,distinct,1,22,1001400,1,1
slow,distinct,1,23,1001572,1,1
slow,distinct,1,24,1000540,1,1
slow,distinct,1,25,1000660,1,1
slow,distinct,1,26,1001302,1,1
slow,distinct,1,27,1001371,1,1
slow,distinct,1,28,1001026,1,1
slow,distinct,1,29,1000686,1,1
slow,distinct,1,30,1000651,1,1
slow,distinct,1,31,1001019,1,1
slow,distinct,1,32,1002198,1,1
slow,distinct,1,33,1000687,1,1
slow,distinct,1,34,1000898,1,1
slow,distinct,1,35,1001451,1,1
slow,distinct,1,36,1178697,1,1
slow,distinct,1,37,1001056,1,1
slow,distinct,1,38,1001457,1,1
slow,distinct,1,39,1001264,1,1
slow,distinct,1,40,1001196,1,1
slow,distinct,1,41,1000542,1,1
slow,distinct,1,42,1001356,1,1
slow,distinct,1,43,1000389,1,1
slow,distinct,1,44,1001680,1,1
slow,distinct,1,45,1001266,1,1
slow,distinct,1,46,1000988,1,1
slow,distinct,1,47,1001235,1,1
slow,distinct,1,48,1001426,1,1
slow,distinct,1,49,1002662,1,1
slow,distinct,8,0,1001045,1,8
slow,distinct,8,1,1001147,1,8
slow,distinct,8,2,1001129,1,8
slow,distinct,8,3,1000904,1,8
slow,distinct,8,4,1001333,1,8
slow,distinct,8,5,1000810,1,8
slow,distinct,8,6,1000491,1,8
slow,distinct,8,7,1000553,1,8
slow,distinct,8,8,1000928,1,8
slow,distinct,8,9,1002812,1,8
slow,distinct,8,10,1001592,1,8
slow,distinct,8,11,1072566,1,8
slow,distinct,8,12,1001480,1,8
slow,distinct,8,13,1001619,1,8
slow,distinct,8,14,1001537,1,8
slow,distinct,8,15,1001137,1,8
slow,distinct,8,16,1001744,1,8
slow,distinct,8,17,1000470,1,8
slow,distinct,8,18,1001196,1,8
slow,distinct,8,19,1001350,1,8
slow,distinct,8,20,1001299,1,8
slow,distinct,8,21,1000958,1,8
slow,distinct,8,22,1001468,1,8
slow,distinct,8,23,1001226,1,8
slow,distinct,8,24,1001297,1,8
slow,distinct,8,25,1001631,1,8
slow,distinct,8,26,1001278,1,8
slow,distinct,8,27,1000740,1,8
slow,distinct,8,28,1001496,1,8
slow,distinct,8,29,1001185,1,8
slow,distinct,8,30,1001528,1,8
slow,distinct,8,31,1001158,1,8
slow,distinct,8,32,1001213,1,8
slow,distinct,8,33,1001079,1,8
slow,distinct,8,34,1000787,1,8
slow,distinct,8,35,1002862,1,8
slow,distinct,8,36,1002647,1,8
slow,distinct,8,37,1037744,1,8
slow,distinct,8,38,1000956,1,8
slow,distinct,8,39,1001116,1,8
slow,distinct,8,40,1001447,1,8
slow,distinct,8,41,1000748,1,8
slow,distinct,8,42,1000823,1,8
slow,distinct,8,43,1003363,1,8
slow,distinct,8,44,1001292,1,8
slow,distinct,8,45,1003353,1,8
slow,distinct,8,46,1001552,1,8
slow,distinct,8,47,1001218,1,8
slow,distinct,8,48,1001697,1,8
slow,distinct,8,49,1000782,1,8
slow,distinct,32,0,1003745,1,32
slow,distinct,32,1,1001985,1,32
slow,distinct,32,2,1001518,1,32
slow,distinct,32,3,1001881,1,32
slow,distinct,32,4,1003072,1,32
slow,distinct,32,5,1001486,1,32
slow,distinct,32,6,1001322,1,32
slow,distinct,32,7,1001427,1,32
slow,distinct,32,8,1002198,1,32
slow,distinct,32,9,1000956,1,32
slow,distinct,32,10,1001382,1,32
slow,distinct,32,11,1001561,1,32
slow,distinct,32,12,1002500,1,32
slow,distinct,32,13,1003712,1,32
slow,distinct,32,14,1000739,1,32
slow,distinct,32,15,1002449,1,32
slow,distinct,32,16,1001425,1,32
slow,distinct,32,17,1002441,1,32
slow,distinct,32,18,1001998,1,32
slow,distinct,32,19,1001619,1,32
slow,distinct,32,20,1001312,1,32
slow,distinct,32,21,1001676,1,32
slow,distinct,32,22,1002652,1,32
slow,distinct,32,23,1001860,1,32
slow,distinct,32,24,1001725,1,32
slow,distinct,32,25,1001464,1,32
slow,distinct,32,26,1002116,1,32
slow,distinct,32,27,1001717,1,32
slow,distinct,32,28,1003290,1,32
slow,distinct,32,29,1001429,1,32
slow,distinct,32,30,1001370,1,32
slow,distinct,32,31,1002499,1,32
slow,distinct,32,32,1001468,1,32
slow,distinct,32,33,1003079,1,32
slow,distinct,32,34,1000964,1,32
slow,distinct,32,35,1001316,1,32
slow,distinct,32,36,1001523,1,32
slow,distinct,32,37,1000762,1,32
slow,distinct,32,38,1000940,1,32
slow,distinct,32,39,1001455,1,32
slow,distinct,32,40,1001532,1,32
slow,distinct,32,41,1001638,1,32
slow,distinct,32,42,1001634,1,32
slow,distinct,32,43,1002041,1,32
slow,distinct,32,44,1002006,1,32
slow,distinct,32,45,1003522,1,32
slow,distinct,32,46,1002402,1,32
slow,distinct,32,47,1003690,1,32
slow,distinct,32,48,1003077,1,32
slow,distinct,32,49,1002524,1,32
slow,linked,0,0,15,0,0
slow,linked,0,1,13,0,0
slow,linked,0,2,29,0,0
slow,linked,0,3,36,0,0
slow,linked,0,4,8,0,0
slow,linked,0,5,8,0,0
slow,linked,0,6,12,0,0
slow,linked,0,7,9,0,0
slow,linked,0,8,13,0,0
slow,linked,0,9,8,0,0
slow,linked,0,10,11,0,0
slow,linked,0,11,11,0,0
slow,linked,0,12,13,0,0
slow,linked,0,13,18,0,0
slow,linked,0,14,13,0,0
slow,linked,0,15,23,0,0
slow,linked,0,16,15,0,0
slow,linked,0,17,15,0,0
slow,linked,0,18,25,0,0
slow,linked,0,19,12,0,0
slow,linked,0,20,12,0,0
slow,linked,0,21,37,0,0
slow,linked,0,22,30,0,0
slow,linked,0,23,42,0,0
slow,linked,0,24,8,0,0
slow,linked,0,25,13,0,0
slow,linked,0,26,12,0,0
slow,linked,0,27,12,0,0
slow,linked,0,28,9,0,0
slow,linked,0,29,9,0,0
slow,linked,0,30,12,0,0
slow,linked,0,31,12,0,0
slow,linked,0,32,11,0,0
slow,linked,0,33,8,0,0
slow,linked,0,34,9,0,0
slow,linked,0,35,32,0,0
slow,linked,0,36,9,0,0
slow,linked,0,37,8,0,0
slow,linked,0,38,11,0,0
slow,linked,0,39,11,0,0
slow,linked,0,40,12,0,0
slow,linked,0,41,882,0,0
slow,linked,0,42,10,0,0
slow,linked,0,43,12,0,0
slow,linked,0,44,9,0,0
slow,linked,0,45,28,0,0
slow,linked,0,46,8,0,0
slow,linked,0,47,11,0,0
slow,linked,0,48,9,0,0
slow,linked,0,49,28,0,0
slow,linked,1,0,1000650,1,1
slow,linked,1,1,1005658,1,1
slow,linked,1,2,1000718,1,1
slow,linked,1,3,1000987,1,1
slow,linked,1,4,1007454,1,1
slow,linked,1,5,1013465,1,1
slow,linked,1,6,1001137,1,1
slow,linked,1,7,1000727,1,1
slow,linked,1,8,1001970,1,1
slow,linked,1,9,1025314,1,1
slow,linked,1,10,1003000,1,1
slow,linked,1,11,1003049,1,1
slow,linked,1,12,1000689,1,1
slow,linked,1,13,1002279,1,1
slow,linked,1,14,1001784,1,1
slow,linked,1,15,1001969,1,1
slow,linked,1,16,1001819,1,1
slow,linked,1,17,1004417,1,1
slow,linked,1,18,1002202,1,1
slow,linked,1,19,1004362,1,1
slow,linked,1,20,1001721,1,1
slow,linked,1,21,1002171,1,1
slow,linked,1,22,1000879,1,1
slow,linked,1,23,1001006,1,1
slow,linked,1,24,1002555,1,1
slow,linked,1,25,1000743,1,1
slow,linked,1,26,1001665,1,1
slow,linked,1,27,1000864,1,1
slow,linked,1,28,1002323,1,1
slow,linked,1,29,1002960,1,1
slow,linked,1,30,1001531,1,1
slow,linked,1,31,1002709,1,1
slow,linked,1,32,1001602,1,1
slow,linked,1,33,1003270,1,1
slow,linked,1,34,1000514,1,1
slow,linked,1,35,1000802,1,1
slow,linked,1,36,1003734,1,1
slow,linked,1,37,1002562,1,1
slow,linked,1,38,1005479,1,1
slow,linked,1,39,1002484,1,1
slow,linked,1,40,1003014,1,1
slow,linked,1,41,1002710,1,1
slow,linked,1,42,1002578,1,1
slow,linked,1,43,1003037,1,1
slow,linked,1,44,1000601,1,1
slow,linked,1,45,1003274,1,1
slow,linked,1,46,1001703,1,1
slow,linked,1,47,1002053,1,1
slow,linked,1,48,1001015,1,1
slow,linked,1,49,1002351,1,1
slow,linked,8,0,1002175,1,8
slow,linked,8,1,1003014,1,8
slow,linked,8,2,1001736,1,8
slow,linked,8,3,1004663,1,8
slow,linked,8,4,1001080,1,8
slow,linked,8,5,1003915,1,8
slow,linked,8,6,1003048,1,8
slow,linked,8,7,1002816,1,8
slow,linked,8,8,1006995,1,8
slow,linked,8,9,1002995,1,8
slow,linked,8,10,1006377,1,8
slow,linked,8,11,1002500,1,8
slow,linked,8,12,1001424,1,8
slow,linked,8,13,1003055,1,8
slow,linked,8,14,1001531,1,8
slow,linked,8,15,1003485,1,8
slow,linked,8,16,1001925,1,8
slow,linked,8,17,1001415,1,8
slow,linked,8,18,1001990,1,8
slow,linked,8,19,1005459,1,8
slow,linked,8,20,1001939,1,8
slow,linked,8,21,1001955,1,8
slow,linked,8,22,1002241,1,8
slow,linked,8,23,1004521,1,8
slow,linked,8,24,1009853,1,8
slow,linked,8,25,1001417,1,8
slow,linked,8,26,1001243,1,8
slow,linked,8,27,1001302,1,8
slow,linked,8,28,1000971,1,8
slow,linked,8,29,1000888,1,8
slow,linked,8,30,1001371,1,8
slow,linked,8,31,1000981,1,8
slow,linked,8,32,1001221,1,8
slow,linked,8,33,1001378,1,8
slow,linked,8,34,1001082,1,8
slow,linked,8,35,1001304,1,8
slow,linked,8,36,1001093,1,8
slow,linked,8,37,1001335,1,8
slow,linked,8,38,1001259,1,8
slow,linked,8,39,1001182,1,8
slow,linked,8,40,1001127,1,8
slow,linked,8,41,1001305,1,8
slow,linked,8,42,1001560,1,8
slow,linked,8,43,1001161,1,8
slow,linked,8,44,1001202,1,8
slow,linked,8,45,1001153,1,8
slow,linked,8,46,1001205,1,8
slow,linked,8,47,1001484,1,8
slow,linked,8,48,1000992,1,8
slow,linked,8,49,1001168,1,8
slow,linked,32,0,1001257,1,32
slow,linked,32,1,1001541,1,32
slow,linked,32,2,1000850,1,32
slow,linked,32,3,1001302,1,32
slow,linked,32,4,1001105,1,32
slow,linked,32,5,1002447,1,32
slow,linked,32,6,1001645,1,32
slow,linked,32,7,1001323,1,32
slow,linked,32,8,1001168,1,32
slow,linked,32,9,1001316,1,32
slow,linked,32,10,1001362,1,32
slow,linked,32,11,1000856,1,32
slow,linked,32,12,1000543,1,32
slow,linked,32,13,1001227,1,32
slow,linked,32,14,1002334,1,32
slow,linked,32,15,1001552,1,32
slow,linked,32,16,1001831,1,32
slow,linked,32,17,1001410,1,32
slow,linked,32,18,1002251,1,32
slow,linked,32,19,1001274,1,32
slow,linked,32,20,1001730,1,32
slow,linked,32,21,1001545,1,32
slow,linked,32,22,1001205,1,32
slow,linked,32,23,1000776,1,32
slow,linked,32,24,1001224,1,32
slow,linked,32,25,1001545,1,32
slow,linked,32,26,1001217,1,32
slow,linked,32,27,1001259,1,32
slow,linked,32,28,1001356,1,32
slow,linked,32,29,1001000,1,32
slow,linked,32,30,1001219,1,32
slow,linked,32,31,1001704,1,32
slow,linked,32,32,1001592,1,32
slow,linked,32,33,1001358,1,32
slow,linked,32,34,1001159,1,32
slow,linked,32,35,1000920,1,32
slow,linked,32,36,1001085,1,32
slow,linked,32,37,1000970,1,32
slow,linked,32,38,1001079,1,32
slow,linked,32,39,1001168,1,32
slow,linked,32,40,1001265,1,32
slow,linked,32,41,1001364,1,32
slow,linked,32,42,1001060,1,32
slow,linked,32,43,1000937,1,32
slow,linked,32,44,1001269,1,32
slow,linked,32,45,1001251,1,32
slow,linked,32,46,1003021,1,32
slow,linked,32,47,1001031,1,32
slow,linked,32,48,1000938,1,32
slow,linked,32,49,1001233,1,32
```
