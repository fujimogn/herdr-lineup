# Codex reader performance trial (2026-11-04)

Status: record only; rejected implementation, no runtime change. This document
records the cancelled `asg_codex_reader_perf` trial. It does not establish a
performance improvement, SLO, production change, deployment, or acceptance.
The owner stopped the work because the measured benefit did not justify
loosening the read guarantees. The rejected source changes were restored;
the original worktree remains clean, and the measurement artifacts are retained.

## Motivation and provenance

The [ten-minute live observation](stuck-worker-frequency-2026-11-04.md)
collected diagnostics from an existing daemon, without changing it. Computing
endpoint deltas from that document's compact observations gives Codex about
7.876 seconds of accumulated reader wait over 600 seconds, 1,880 calls,
about 4.189 milliseconds per call and about 84% of the combined reader wait.
Its cumulative maximum rose to 531.192 milliseconds during that window.
Accumulated waits can overlap; the 84% share is not refresh wall time or a
user-visible delay. The live binary's source identity is only inferred in
the linked record, not proven identical to the trial source.

The commander supplied the user's rollout inventory: 272 files updated in
one day, with large files approximately 20 MB, 10 MB and 7 MB. This is supplied
context, not a new inventory performed by this lane. No real rollout contents
were used in this trial. The owner's decision was that the average live wait
of about 4.2 milliseconds per call did not justify this optimization's risks.

Trial source base: `80e11d1604a2c4bb56d0fc008c157b3b4b179d0d`.
Documentation base: `551226d0ab0800e3ea32fd008a1ff8962fa63519`, the fetched
`origin/develop` tip when the new documentation worktree was created.
The original reader module copied into the harness was checked byte-for-byte
against the trial base before this record was written; its SHA-256 is
`6b0b910dd10dfc25144ee9ba96f4c2e55deb61f1e71ed148041d1608cbfb0194`.

## Fixture, harness, and measurement boundaries

- One synthetic rollout initially exceeded 20 MiB: 58 bytes of session header,
  followed by 16,601-byte `response_item` lines. Each line contains four
  assistant text fields of 4,096 repeated ASCII `x` characters. Another 271
  rollout-shaped files each contain `{}` and a newline, for 272 names total.
- Each cycle appends a 170-byte complete `token_count` line. It reports 26,000
  used tokens in a 212,000-token window, giving the reader's expected context
  value of approximately 7%. Fixture setup and append are outside timing.
- Dependencies were built with `cargo build --release --locked`, using the
  pinned Rust 1.99.1 toolchain. The crate's release settings are `opt-level=z`,
  LTO, one codegen unit and stripped output. The external harness was compiled
  separately with `rustc --edition=2024 -C opt-level=z -C lto=thin`, against
  those release dependencies. It is not the production release binary and
  does not duplicate every release-profile setting.
- The harness directly calls production `read_session_in_cycle` with a fresh
  `ReadCycle`, after separately timing discovery, read and parse. It includes
  cache lookup/update and snapshot validation, but excludes bounded-worker
  thread startup, channel wait, scheduling by the daemon and UI publication.
  A 30-second read budget avoids imposing the live timeout on the experiment.
- Each process runs 35 cycles. Cycles 0–4 are excluded; cycles 5–34 are retained.
  The separately timed stages and full call are distinct calls; their
  medians cannot be added to explain the full-call median.
- The main comparison ran 12 blocks, each containing one old and one trial
  process: AB for even blocks, BA for odd blocks. Each process starts from
  the same fixture length (20,990,012 bytes), then appends in each cycle.
  Its cycle 0 length is 20,990,182 bytes. Existing bytes and appended records
  are the same for both variants. There are 840 raw rows and 360 retained
  rows per variant. The extra token lines before this comparison come from
  the earlier baseline and failed harness launches.
- Reads are warm: setup and prior cycles have populated filesystem caches;
  every full timed call also follows a separate read/parse of the same file.
  File growth forces the observation fingerprint to miss each cycle.
- This was a shared Linux host, not an isolated benchmark. Host load was not
  sampled numerically during the trial. Other lanes and host scheduling were
  uncontrolled; compilation preceded each measured series. Earlier baseline
  values and later attribution values differ substantially. Do not import
  load averages from the separate live-observation document into this trial.
- Fixture and harness scratch files were supervised by `dev-task-temp run`
  under a private ext4 root, with 500,000,000-byte / 2,000-inode reservations
  for each run (below the assigned 5 GB limit). Build products stay in the
  original worktree's `target`; no Herdr server/session or user settings were
  changed. Initial launcher failures were a duplicate libc-artifact selection
  assertion and an exact floating-point equality assertion; selection of the
  sorted first rlib and an epsilon comparison corrected the harness.

## Rejected parse trial

The trial first deserialized a type envelope. For unrelated records, a
recursive `deserialize_any` visitor validated payload values without retaining
the conversation data; relevant records used the original parser. A plain
`IgnoredAny` payload was avoided because numeric-range and Unicode validation
can differ. Envelope errors, including duplicate top-level fields, fell back
to the original parser. The observation cache and tail read were unchanged.
Two targeted tests passed, including comparisons with the old parser across
nesting limits, huge numbers, malformed Unicode, duplicate keys, and every
byte truncation with and without a final newline. This was not completed
release validation: mutant verification, the full five checks and independent
review of the discarded implementation were not performed after the gate failed.

The adoption gate, set before implementation, required at least a 10% reduction
in the median full production reader call on this fixture. The reduction was
4.491%, so the candidate was rejected without a commit. p90 was recorded for
context and did not override the median gate. Percent reduction is
`100 * (1 - trial_median / old_median)`.

| Metric (microseconds) | Old | Trial |
| --- | ---: | ---: |
| Separate discovery median | 215.332 | 201.402 |
| Separate read median | 1,046.341 | 975.784 |
| Separate parse median | 663.280 | 600.899 |
| Full reader median | 2,171.724 | 2,074.187 |
| Full reader p90 | 6,280.681 | 3,388.364 |

p90 uses sorted index `floor(0.90 * n)` (zero-based, n=360), without interpolation.
This is the convention used in the trial summary, not a confidence interval.

| Block | Order | Old median (us) | Trial median (us) | Reduction (%) |
| --- | --- | ---: | ---: | ---: |
| 0 | AB | 1,981.363 | 1,715.908 | 13.40 |
| 1 | BA | 1,731.787 | 1,820.313 | -5.11 |
| 2 | AB | 1,733.435 | 1,641.759 | 5.29 |
| 3 | BA | 2,142.735 | 1,881.266 | 12.20 |
| 4 | AB | 2,565.615 | 2,129.695 | 16.99 |
| 5 | BA | 2,146.752 | 2,272.288 | -5.85 |
| 6 | AB | 4,683.264 | 2,184.758 | 53.35 |
| 7 | BA | 2,536.383 | 2,211.162 | 12.82 |
| 8 | AB | 2,308.646 | 2,103.898 | 8.87 |
| 9 | BA | 7,124.016 | 3,148.822 | 55.80 |
| 10 | AB | 1,716.438 | 2,310.209 | -34.59 |
| 11 | BA | 2,123.228 | 2,122.924 | 0.01 |

Block reductions range from -34.59% to 55.80%. A negative reduction means the
trial median was slower in that block. Pooling the retained rows gives the
4.491% gate result; averaging the block percentages would answer a different
question. This variability prevents a general claim about performance.

## Stage attribution and the earlier unexplained difference

The first baseline series had 35 cycles, discarded the first five, and reported
median discovery 334.280 us, read 1,830.120 us, parse 2,303.104 us, and a
separately measured full call of 7,434.919 us. The apparent difference of
2,967.415 us from the sum of the three stage medians was not a measured
additional operation. These stages and the full call are separate executions;
medians are nonadditive, and shared-host delays can occur in any interval.

A later old-parser-only series instrumented the same harness's full call at
phase boundaries: 35 cycles, five discarded, 30 paired stage/total records.
The following means and medians are wall time per code interval, including
possible descheduling. They are not isolated syscall durations or CPU time.
Header read is already part of the separate read metric above.

| Phase | Mean (us) | Median (us) |
| --- | ---: | ---: |
| setup | 5.216 | 5.029 |
| discovery_filter | 1,915.335 | 325.499 |
| open_fingerprint_cache | 11.697 | 11.877 |
| header_read | 1,791.219 | 164.002 |
| header_validate | 11.786 | 10.224 |
| tail_read | 7,747.107 | 6,665.310 |
| tail_trim | 2,357.432 | 158.553 |
| decode | 889.514 | 179.951 |
| parse | 4,641.071 | 893.375 |
| tail_text_drop | 374.340 | 0.586 |
| snapshot_validation | 16.208 | 15.434 |
| cache_insert | 5.367 | 4.338 |
| Full call | 19,787.716 | 21,446.762 |
| Per-call total minus sum of phase intervals | 21.424 | 19.302 |

The median of each call's summed phases is
21,416.892 us. The paired residual
median of 19.302 us leaves no observed fixed ~2.97 ms omitted phase in this
series. It does not retrospectively identify where time went in the first
series. The larger later totals and large phase means are further evidence
of variation, not a justification for treating one phase as a production
bottleneck. Thread startup is absent from these direct-call measurements.

## Incremental alternatives considered, not implemented

A: retain per-(path, session) file identity, size/mtime, header binding,
complete-line offset and the last observation plus its absolute line offset.
Read only appended bytes after rechecking the first header line. Keep fresh
discovery, `O_NOFOLLOW`, and post-read path/file snapshot validation. Restrict
the fast path to large uncompressed files ending at a complete newline;
fall back for partial JSON/UTF-8, truncation, replacement, same-size rewrite,
small/large-file boundary changes, large appends, missing/error observations,
and uncertain moving-tail boundaries. Keep at most 128 FIFO entries with
fixed derived information and no raw conversation data. In particular, an
old observation that falls outside the current 2 MiB tail must become
`NotFound`, rather than being kept indefinitely.

A cannot distinguish a genuine append from an old-region rewrite followed by
an append using dev/inode/size/mtime alone. Validating new bytes only can miss
corruption of old completed lines or changed historical token counts that the
current full tail would detect. Header checking alone does not close that gap.
The proposal therefore required an explicit append-only writer assumption
and a narrower corruption-detection guarantee, conflicting with the assignment's
unchanged values and `MissingReason` requirement.

B: reread the overlapping old tail and compare fixed-block digests before
parsing only new bytes; reread fully on mismatch. Store derived digests, not
raw text. This could retain practical detection of historical changes, but
has collision risk and still rereads the tail, so it does not accomplish pure
incremental I/O. Any benefit would have to come from smaller header reads and
avoiding repeat trim/decode/parse. Exact equivalence without digest assumptions
would require the existing full validation or retaining/comparing raw bytes;
the proposal did not authorize retaining raw conversation data.

Neither proposal was implemented or benchmarked. In commander note
`note_codexperf_stop`, the owner declined further optimization: the benefit
was insufficient to relax guarantees, and the live average wait was not a
perceptible delay. The implementation assignment was cancelled; only this
record was requested. No implementation, change in read semantics, or new
runtime guarantee resulted.

## Fact check, retained evidence, and limits

The document's comparison tables were recomputed from all 840 rows, retaining
cycles 5–34. Counts, common starting sizes, AB/BA orders, pooled medians,
zero-based p90, reductions and the twelve block ranges were checked from the
raw data. Attribution was recomputed by pairing each of the 30 retained full
calls with its phase list; mean/median and per-call residuals were checked.
The live endpoint deltas were recomputed from the compact observations in
the linked record; rollout inventory and owner decisions remain attributed
to the commander's supplied information rather than new live verification.

Private root: `/home/fujimogn/.cache/herdr-lineup-codexperf`.
Run IDs and files under its `runs/<id>/work/` directories:

- `b0b9fa844b6b1283612ffbe8ac9ce3bb`: synthetic fixture, `baseline.jsonl`,
  `reader_codex-before.rs`, `reader_codex.rs`, `model.rs`, `reader_cache.rs`,
  `bench.rs`, `bench-instrumented.rs`, `compile.json`.
- `6de8cafe9ed54a5902aa264d76c52e61`: `alternating.jsonl` (840 rows).
- `b2eea0781a2aa70eb6e13a3214eff8f8`: `stages.jsonl` and `stdout.jsonl`
  (35 paired rows each).
- Failed/launcher-only supervised runs also remain in the root; they are not
  included as comparison observations.

The original worktree is `.claude/worktrees/codex-reader-perf-fm-20261104`.
It retains the old, trial and instrumented executables under `target`.
The scratch module `reader_codex.rs` was overwritten by the attribution copy;
the trial module snapshot itself was not separately archived, and the uncommitted
trial source was subsequently restored as directed. The retained binary and
summaries do not prove a byte-for-byte rebuild of that discarded variant.
The appendix preserves the retained harness source in full and gives the
orchestration needed to interpret the experiment. It is not a new benchmark
run, and no commands in the appendix were run as part of writing this document.

All results are from one synthetic text-heavy fixture, warm reads, sequential
processes and an uncontrolled shared host. The trial did not sample the user's
actual rollout distribution or establish user-visible latency, confidence
bounds, service guarantees, or an acceptable tradeoff for weaker validation.

## Documentation worktree verification

Only this Markdown record is changed against documentation base
`551226d0ab0800e3ea32fd008a1ff8962fa63519`; Rust code, manifests, TODO and
CHANGELOG are unchanged. Fresh checks on this worktree completed successfully:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass |
| `cargo test --locked` | Pass: 446 library tests, 6 ignored, 1 integration test |
| `cargo build --release --locked` | Pass |
| `target/release/herdr-lineup --help` | Pass; help only, no daemon operation |

An additional fact check validated the 840 comparison rows, 24 process groups,
cycle sequences, equal starting sizes, retained counts, table medians and p90,
and the evidence hashes. Python and JSON appendix blocks parsed successfully.
The shared `worktree-init` tool returned a stale-binary freshness error before
initialization; this was reported to the commander. No tool installation or
configuration distribution was performed. The documentation work required no
secrets or additional dependency setup, and the normal crate checks passed.
These checks validate the documentation worktree's unchanged software, not
the discarded optimization or any production latency claim.

## Appendix: harness source

The repository modules are external inputs pinned to the trial base, not
new production implementations. `reader_codex-before.rs` is that base module
with the following measurement function appended. Its main program and the
instrumented main program are included verbatim below. The orchestration
listing is reconstructed from the commands used; scratch paths are explicit
so readers can distinguish retained inputs from reconstruction.

### Measurement function (verbatim retained source)

```rust
pub fn measure(home: &Path, cycle_index: usize) {
    use std::time::{Duration, Instant};
    use std::io::Write;
    let id = SessionIdentity { provider: Provider::Codex, native_session_id: "synthetic-perf".into(), repository_identity: "/synthetic".into() };
    let request = ReadRequest { identity: &id, home, now: SystemTime::now(), max_age: Duration::from_secs(60), timeout: Duration::from_secs(30), shutdown: None };
    let target = home.join("sessions/2026/11/04/rollout-synthetic-perf.jsonl");
    let sample = "{\"type\":\"event_msg\",\"timestamp\":\"2026-11-04T00:00:00Z\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"total_tokens\":26000},\"model_context_window\":212000}}}\n";
    let mut out = fs::OpenOptions::new().append(true).open(&target).unwrap();
    out.write_all(sample.as_bytes()).unwrap(); drop(out);
    let mut budget = Budget::new(&request);
    let cycle = ReadCycle::new();
    let begin = Instant::now();
    let paths = cycle.files(&mut budget, home, DiscoveryKind::Codex).unwrap();
    std::hint::black_box(paths);
    let discovery_ns = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let mut file = budget.open(&target).unwrap();
    let size = file.metadata().unwrap().len();
    let text = read_jsonl_tail(&budget, &mut file, size, |header| {
        let v: Value = serde_json::from_slice(header).unwrap();
        assert_eq!(v["payload"]["id"], "synthetic-perf"); Ok(())
    }).unwrap();
    let read_ns = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let result = parse_records(&text, "synthetic-perf", &budget, true).unwrap();
    assert!((result.0 - 7.0).abs() < 1e-9);
    let parse_ns = begin.elapsed().as_nanos();
    let begin = Instant::now();
    assert_eq!(read_session_in_cycle(&request, &ReadCycle::new()).unwrap(), result);
    let total_ns = begin.elapsed().as_nanos();
    println!("{{\"cycle\":{cycle_index},\"size\":{size},\"tail_bytes\":{},\"discovery_ns\":{discovery_ns},\"read_ns\":{read_ns},\"parse_ns\":{parse_ns},\"total_ns\":{total_ns}}}", text.len());
}
```

### Uninstrumented main (verbatim)

```rust
#![allow(dead_code)]
mod model; mod reader_cache; mod reader_codex;
fn main() { let home=std::env::args().nth(1).unwrap(); for i in 0..35 { reader_codex::measure(std::path::Path::new(&home), i); } }
```

### Instrumented main and clock recorder (verbatim)

```rust
#![allow(dead_code)]
mod model; mod reader_cache; mod reader_codex;
fn main() { let home=std::env::args().nth(1).unwrap(); for i in 0..35 { reader_codex::measure(std::path::Path::new(&home), i); } }

thread_local! { static PERF: std::cell::RefCell<(Option<std::time::Instant>, Vec<(&'static str, u128)>)> = const { std::cell::RefCell::new((None, Vec::new())) }; }
fn perf_start() { PERF.with(|p| { let mut p=p.borrow_mut(); p.1.clear(); p.0=Some(std::time::Instant::now()); }); }
fn perf_mark(label: &'static str) { PERF.with(|p| { let mut p=p.borrow_mut(); if let Some(t)=p.0 { let now=std::time::Instant::now(); p.1.push((label,now.duration_since(t).as_nanos())); p.0=Some(now); } }); }
fn perf_end() { PERF.with(|p| { let mut p=p.borrow_mut(); p.0=None; eprintln!("{}",serde_json::to_string(&p.1).unwrap()); }); }
```

### Original compile arguments (verbatim retained JSON)

```json
[
  "rustc",
  "--edition=2024",
  "-C",
  "opt-level=z",
  "-C",
  "lto=thin",
  "-L",
  "dependency=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps",
  "/home/fujimogn/.cache/herdr-lineup-codexperf/runs/b0b9fa844b6b1283612ffbe8ac9ce3bb/work/bench.rs",
  "-o",
  "/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/codexperf-before",
  "--extern",
  "libc=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps/liblibc-4c01c88e941cc779.rlib",
  "--extern",
  "serde=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps/libserde-05523243022fe4c0.rlib",
  "--extern",
  "serde_json=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps/libserde_json-da76b398a112461d.rlib",
  "--extern",
  "chrono=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps/libchrono-adffcaeb12d179d3.rlib",
  "--extern",
  "zstd=/home/fujimogn/Develop/herdr-lineup/.claude/worktrees/codex-reader-perf-fm-20261104/target/release/deps/libzstd-4e1ad84ed3869527.rlib"
]
```

### Attribution instrumentation (complete diffs)

These diffs were computed from retained source copies. The reader diff includes
all measurement-wrapper changes as well as the production-copy phase markers.

```diff
--- reader_codex-before.rs
+++ reader_codex.rs
@@ -101,6 +101,7 @@
                 .and_then(|s| s.to_str())
                 .is_some_and(|name| name.ends_with(&jsonl_suffix) || name.ends_with(&zst_suffix))
     };
+    crate::perf_mark("setup");
     let paths: Vec<_> = cycle
         .files(&mut budget, request.home, DiscoveryKind::Codex)?
         .into_iter()
@@ -110,6 +111,7 @@
         return Err(MissingReason::Unassociated);
     }
     let path = paths.first().ok_or(MissingReason::NotFound)?;
+    crate::perf_mark("discovery_filter");
     let mut file = budget.open(path)?;
     let fingerprint = Fingerprint::of(&file.metadata().map_err(|e| io_reason(&e))?);
     let key = (path.to_path_buf(), id.clone());
@@ -121,6 +123,7 @@
         budget.check()?;
         return cached;
     }
+    crate::perf_mark("open_fingerprint_cache");
     let result = if path.extension().is_some_and(|e| e == "zst") {
         // Keep the existing 16 MiB decompressed limit (SourceError on overflow).
         budget
@@ -129,6 +132,7 @@
     } else {
         read_tail(&mut file, fingerprint.size, id, &budget)
     };
+    crate::perf_mark("tail_text_drop");
     budget.check()?;
     after_read(path);
     // Never cache a mixed snapshot while a writer appends, truncates or replaces it.
@@ -146,6 +150,7 @@
         SnapshotChange::Rejected => return Err(MissingReason::SourceError),
         SnapshotChange::Stable => {}
     }
+    crate::perf_mark("snapshot_validation");
     if !matches!(
         result,
         Err(MissingReason::Timeout | MissingReason::SourceError | MissingReason::PermissionDenied)
@@ -155,6 +160,7 @@
             .map_err(|_| MissingReason::SourceError)?
             .insert(key, fingerprint, result);
     }
+    crate::perf_mark("cache_insert");
     result
 }
 
@@ -166,7 +172,9 @@
         }
         Ok(())
     })?;
-    parse_records(&text, id, budget, size > JSONL_TAIL_BYTES as u64)
+    let result = parse_records(&text, id, budget, size > JSONL_TAIL_BYTES as u64);
+    crate::perf_mark("parse");
+    result
 }
 
 #[derive(Deserialize)]
@@ -1152,7 +1160,9 @@
     assert!((result.0 - 7.0).abs() < 1e-9);
     let parse_ns = begin.elapsed().as_nanos();
     let begin = Instant::now();
+    crate::perf_start();
     assert_eq!(read_session_in_cycle(&request, &ReadCycle::new()).unwrap(), result);
     let total_ns = begin.elapsed().as_nanos();
+    crate::perf_end();
     println!("{{\"cycle\":{cycle_index},\"size\":{size},\"tail_bytes\":{},\"discovery_ns\":{discovery_ns},\"read_ns\":{read_ns},\"parse_ns\":{parse_ns},\"total_ns\":{total_ns}}}", text.len());
 }
--- base/src/model.rs
+++ model.rs
@@ -1235,20 +1235,26 @@
         return Budget::decode_jsonl(budget.read_range(file, 0, size as usize)?);
     }
     let header = budget.read_range(file, 0, JSONL_HEADER_BYTES)?;
+    crate::perf_mark("header_read");
     let end = header
         .iter()
         .position(|byte| *byte == b'\n')
         .ok_or(MissingReason::SourceError)?;
     bind_header(&header[..end])?;
+    crate::perf_mark("header_validate");
     let start = size - JSONL_TAIL_BYTES as u64 - 1;
     let bytes = budget.read_range(file, start, JSONL_TAIL_BYTES + 1)?;
+    crate::perf_mark("tail_read");
     let complete = bytes
         .iter()
         .position(|byte| *byte == b'\n')
         .map_or(bytes.len(), |index| index + 1);
     let mut bytes = bytes;
     bytes.drain(..complete);
-    Budget::decode_jsonl(bytes)
+    crate::perf_mark("tail_trim");
+    let text = Budget::decode_jsonl(bytes);
+    crate::perf_mark("decode");
+    text
 }
 
 pub(crate) fn timestamp(value: &serde_json::Value) -> Option<SystemTime> {
```

### Fixture, compilation, comparison and attribution orchestration

This complete orchestration listing restates the historical commands without
rerunning them. For the rejected variant it requires an already prepared
trial module input; that discarded input is not supplied here. The initial
fixture setup is separate from the comparison, which used the later retained
starting length stated above. Compilation artifacts belong in the worktree's
`target`; setup and these drivers ran inside `dev-task-temp run` with the
reservation described above.

```python
import json
import os
from pathlib import Path
import statistics
import subprocess

repo = Path.cwd()  # original dedicated trial worktree
scratch = Path(os.environ["DEV_TASK_TEMP_DIR"])
home = scratch / "fixture"
folder = home / "sessions/2026/11/04"
folder.mkdir(parents=True)
for index in range(271):
    (folder / f"rollout-other-{index}.jsonl").write_text("{}\n")
response = json.dumps({
    "type": "response_item",
    "payload": {"type": "message", "role": "assistant", "content": [
        {"type": "output_text", "text": "x" * 4096} for _ in range(4)
    ]},
}, separators=(",", ":")).encode() + b"\n"
target = folder / "rollout-synthetic-perf.jsonl"
with target.open("wb") as stream:
    stream.write(b'{"type":"session_meta","payload":{"id":"synthetic-perf"}}\n')
    while stream.tell() < 20 * 1024 * 1024:
        stream.write(response)

# Stage the pinned model.rs and reader_cache.rs, and the chosen reader module
# plus the measurement function, as siblings of the retained bench.rs.
def compile_harness(source, output):
    arguments = ["rustc", "--edition=2024", "-C", "opt-level=z",
                 "-C", "lto=thin", "-L", f"dependency={repo}/target/release/deps",
                 str(source), "-o", str(output)]
    for dependency in ["libc", "serde", "serde_json", "chrono", "zstd"]:
        files = sorted((repo / "target/release/deps").glob(
            f"lib{dependency}-*.rlib"))
        arguments += ["--extern", f"{dependency}={files[0]}"]
    subprocess.run(arguments, check=True)
    return arguments

# In the historical run these were compiled in succession from old/trial
# module copies using the same uninstrumented main and flags.
# compile_harness(scratch / "bench.rs", repo / "target/codexperf-before")
# compile_harness(scratch / "bench.rs", repo / "target/codexperf-after")

def run_comparison(fixture, output):
    target = fixture / "sessions/2026/11/04/rollout-synthetic-perf.jsonl"
    initial = target.stat().st_size
    assert initial == 20990012  # retained comparison input, not fresh setup
    rows = []
    for block in range(12):
        order = ["before", "after"] if block % 2 == 0 else ["after", "before"]
        for variant in order:
            with target.open("r+b") as stream:
                stream.truncate(initial)
            result = subprocess.run([
                str(repo / f"target/codexperf-{variant}"), str(fixture)
            ], capture_output=True, text=True, check=True)
            rows.extend(dict(json.loads(line), variant=variant, block=block)
                        for line in result.stdout.splitlines())
    output.write_text("".join(json.dumps(row) + "\n" for row in rows))
    for variant in ["before", "after"]:
        retained = [row for row in rows
                    if row["variant"] == variant and row["cycle"] >= 5]
        print(variant, {key: statistics.median(row[key] for row in retained) / 1000
                        for key in ["discovery_ns", "read_ns", "parse_ns", "total_ns"]})

# Apply the complete attribution diffs above to the old module/model copies,
# then compile bench-instrumented.rs with the same compiler flags.
def run_attribution(fixture, output):
    result = subprocess.run([
        str(repo / "target/codexperf-attribution"), str(fixture)
    ], capture_output=True, text=True, check=True)
    (output / "stdout.jsonl").write_text(result.stdout)
    (output / "stages.jsonl").write_text(result.stderr)
    phases = [dict(json.loads(line)) for line in result.stderr.splitlines()][5:]
    totals = [json.loads(line) for line in result.stdout.splitlines()][5:]
    assert len(phases) == len(totals) == 30
    residual = [total["total_ns"] - sum(phase.values())
                for total, phase in zip(totals, phases)]
    print("stage_mean_us", {key: statistics.mean(p[key] for p in phases) / 1000
                            for key in phases[0]})
    print("residual_median_us", statistics.median(residual) / 1000)
```

### Retained evidence SHA-256

| File | SHA-256 |
| --- | --- |
| alternating.jsonl | `1a0f955b0400a7e50a22e50cbf0541eea3359e0467872ede538f85dd62f87d88` |
| stages.jsonl | `0df9a3f2f9a209e11af666320a62a5179b0c6f6c8a23b92f9ec89d3132469552` |
| stdout.jsonl | `44128f993bb533133030f88d6973a95e7a3588d21ad39edcad1f05f434b43268` |
| baseline.jsonl | `2d64feab0252d8ab76690442ad39e96fe6ca8bb47683c34ff893c10835dec227` |
