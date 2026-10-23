# Codex and Kimi Reader Discovery Latency & Fail-Closed Review

| Field | Details |
|---|---|
| Date | 2026-10-23 |
| Target commit | `b5fc311` (`b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`) |
| Component | `src/reader_codex.rs`, `src/reader_kimi.rs`, `src/model.rs`, `src/publish.rs`, `src/config.rs` |
| Reviewer | agy (lane: `lane_reader_measure_agy_4`) |
| Environment | Linux 7.2.10-1-cachyos / x86_64, AMD Ryzen 9 9955HX (16 Cores), synthetic isolated `tmpfs` fixtures (`/tmp`) |
| Primary touching path | `docs/reviews/reader-discovery-latency-2026-10-23.md` |

---

## 1. Executive Summary

This review measures candidate discovery latency, scaling dynamics, and fail-closed safety behaviors of `CodexReader` and `KimiReader` in `herdr-lineup`.

During each refresh cycle, the plugin daemon iterates across configured Herdr workspace panes and queries the associated provider reader for token usage, context limits, and timestamps. When a pane lacks an exact session path or when searching for active native sessions, the reader performs local filesystem discovery within the provider's session tree.

### Key Findings
1. **Algorithmic Disparity in Candidate Discovery**:
   - **Codex (`reader_codex.rs`)**: Traverses hierarchically at depth 3 (`sessions/YYYY/MM/DD/`) and filters candidates by filename suffix (`-{native_id}.jsonl` or `-{native_id}.jsonl.zst`). It evaluates directory entry names before opening any file. For $F$ candidate files and $P$ panes, Codex performs $P$ readdir walks without opening non-matching files. On RAM-backed `tmpfs`, measured pane batch read latency scales from **0.106 ms** (1 pane, 10 files) up to **24.99 ms p50 / 26.57 ms p95** (32 panes, 3,000 files).
   - **Kimi (`reader_kimi.rs`)**: Traverses at depth 2 (`sessions/*/*/state.json`). Because the native session ID and workspace directory (`cwd`) are stored within each session's `state.json`, Kimi inspects every `state.json` file discovered. While `STATE_CACHE` eliminates JSON re-parsing on repeated accesses, each candidate check still requires `open()`, `fstat()`, and `stable_file()` metadata validation. For 32 panes and 1,500 candidate sessions on `tmpfs`, evaluating the pane batch requires 48,000 candidate evaluations ($P \times S$), resulting in **118.52 ms p50 / 309.94 ms p95** (mean 149.51 ms, peak 387.30 ms).
2. **Cycle Deadlines and Caller Wait Bounds**:
   - In `publish.rs`, all pane reads within a refresh cycle share a single deadline: `Instant::now() + Duration::from_millis(config.reader_timeout_ms)`.
   - The default `reader_timeout_ms` is **2,000 ms** (valid configurable range: 100 to 30,000 ms), distinct from the daemon's `refresh_interval_ms` (default 5,000 ms, configurable lower bound 1,000 ms).
   - Because reads execute sequentially on the publisher thread, discovery and tail parsing across all panes must fit within this shared window. At 1,500 Kimi sessions and 32 panes, discovery consumes up to ~15% of this 2,000 ms default budget under in-memory `tmpfs` conditions.
3. **Fail-Closed Safety Invariants & Background Worker Bounds**:
   - Both readers return `MissingReason::Unassociated` when duplicate candidate matches occur.
   - Symlinks are not followed during directory discovery; symlinked roots and symlinked wire files are rejected (`Invalid` / `SourceError`).
   - In-flight file truncation or replacement triggers metadata mismatch (`SourceError`), while pure appends allow valid prefix parsing without caching.
   - Traversal aborts with `MissingReason::EntryLimit` if directory entries exceed `MAX_DIRECTORY_ENTRIES` (4,096).
   - Concurrency contention on provider reader slots is bounded by `BUSY: AtomicBool`. If a worker stalls on an OS read, the caller wait ends at the deadline, while the worker retains the `BUSY` permit until termination, causing subsequent reads for that provider to return `MissingReason::Timeout` without thread leakage.

---

## 2. Experimental Setup and Methodology

Measurements were conducted using an isolated synthetic benchmark harness measuring reader execution directly. No user data, sensitive tokens, or live session paths were accessed.

- **Host Environment**:
  - OS / Kernel: Linux dev 7.2.10-1-cachyos (`#1 SMP PREEMPT_DYNAMIC`) x86_64
  - CPU: AMD Ryzen 9 9955HX 16-Core Processor (32 threads)
  - Filesystem: Dedicated temporary directories (`tempfile::tempdir()`) mounted exclusively on `tmpfs` (32 GB RAM disk). Block-backed filesystems (`ext4`, `xfs`) were not tested.
- **Fixture Matrix Dimensions**:
  - **Codex candidate file counts ($F$)**: 10, 100, 500, 1,500, and 3,000 files in `sessions/YYYY/MM/DD/` depth-3 trees.
  - **Kimi candidate session counts ($S$)**: 10, 50, 100, 500, and 1,500 sessions in `sessions/wd_synthetic/<session_id>/state.json` depth-2 trees.
  - **Pane counts ($P$)**: 1, 4, 8, 16, and 32 panes per batch.
- **Resource Constraints**:
  - Fixture disk usage remained under 25 MB on `tmpfs` per run, well within the 1 GB ceiling.
  - All fixture directories were dropped and unlinked upon completion.
- **Measurement Protocol**:
  - 5 warmup iterations per $(F, P)$ and $(S, P)$ coordinate.
  - 50 standalone read iterations per coordinate across the pane batch via `std::time::Instant` and `std::hint::black_box` (measuring reader discovery and tail parsing in isolation, without executing the broader `MetadataPublisher` cycle which includes Git subprocesses and socket transport).
  - Metrics: $p_{50}$ (median), $p_{95}$ (95th percentile), arithmetic mean, minimum, and maximum in milliseconds.

---

## 3. Quantitative Discovery Latency Results

### 3.1 Codex Reader Discovery Matrix (on `tmpfs`)

Codex matches candidates by filename suffix (`-{session_id}.jsonl`). Files are not opened until a filename match occurs.

| Candidate Files ($F$) | Panes ($P$) | $p_{50}$ (ms) | $p_{95}$ (ms) | Mean (ms) | Min (ms) | Max (ms) |
|---|---|---|---|---|---|---|
| 10 | 1 | 0.106 | 0.130 | 0.112 | 0.092 | 0.359 |
| 10 | 4 | 0.407 | 0.527 | 0.440 | 0.316 | 1.491 |
| 10 | 8 | 0.767 | 0.929 | 0.779 | 0.602 | 0.970 |
| 10 | 16 | 1.442 | 1.762 | 1.474 | 1.287 | 2.285 |
| 10 | 32 | 3.722 | 4.506 | 3.780 | 2.790 | 5.086 |
| 100 | 1 | 0.130 | 0.158 | 0.133 | 0.111 | 0.166 |
| 100 | 4 | 0.525 | 0.631 | 0.535 | 0.467 | 0.670 |
| 100 | 8 | 1.032 | 1.139 | 1.044 | 0.954 | 1.536 |
| 100 | 16 | 1.985 | 2.184 | 1.991 | 1.747 | 2.296 |
| 100 | 32 | 3.718 | 4.031 | 3.682 | 3.189 | 4.183 |
| 500 | 1 | 0.255 | 0.333 | 0.255 | 0.176 | 0.587 |
| 500 | 4 | 0.951 | 1.074 | 0.938 | 0.760 | 1.187 |
| 500 | 8 | 1.917 | 2.111 | 1.912 | 1.635 | 2.173 |
| 500 | 16 | 3.507 | 4.005 | 3.559 | 3.154 | 4.072 |
| 500 | 32 | 7.577 | 8.254 | 7.608 | 6.469 | 8.737 |
| 1,500 | 1 | 0.440 | 0.590 | 0.459 | 0.374 | 0.600 |
| 1,500 | 4 | 1.777 | 2.250 | 1.846 | 1.638 | 2.350 |
| 1,500 | 8 | 4.105 | 4.511 | 4.099 | 3.525 | 4.776 |
| 1,500 | 16 | 8.287 | 8.843 | 8.193 | 6.713 | 9.296 |
| 1,500 | 32 | 14.981 | 16.812 | 15.072 | 12.882 | 16.863 |
| 3,000 | 1 | 1.032 | 1.238 | 0.976 | 0.742 | 1.312 |
| 3,000 | 4 | 3.755 | 4.393 | 3.729 | 3.115 | 4.434 |
| 3,000 | 8 | 7.302 | 8.087 | 7.229 | 6.113 | 8.420 |
| 3,000 | 16 | 13.051 | 15.634 | 13.433 | 12.495 | 16.286 |
| 3,000 | 32 | 24.991 | 26.572 | 25.135 | 24.169 | 26.908 |

### 3.2 Kimi Reader Discovery Matrix (on `tmpfs`)

Kimi traverses depth-2 directories for `state.json` and evaluates session bindings.

| Candidate Sessions ($S$) | Panes ($P$) | $p_{50}$ (ms) | $p_{95}$ (ms) | Mean (ms) | Min (ms) | Max (ms) |
|---|---|---|---|---|---|---|
| 10 | 1 | 0.131 | 0.188 | 0.142 | 0.107 | 0.402 |
| 10 | 4 | 0.527 | 0.716 | 0.550 | 0.435 | 0.745 |
| 10 | 8 | 1.104 | 1.216 | 1.094 | 0.886 | 1.239 |
| 10 | 16 | 2.656 | 2.883 | 2.670 | 2.456 | 3.131 |
| 10 | 32 | 7.381 | 7.786 | 7.392 | 6.760 | 8.192 |
| 50 | 1 | 0.307 | 0.343 | 0.305 | 0.250 | 0.361 |
| 50 | 4 | 1.301 | 1.490 | 1.287 | 1.101 | 1.560 |
| 50 | 8 | 2.463 | 2.707 | 2.487 | 2.220 | 2.836 |
| 50 | 16 | 5.197 | 5.598 | 5.200 | 4.525 | 5.681 |
| 50 | 32 | 10.161 | 10.900 | 10.056 | 8.913 | 11.213 |
| 100 | 1 | 0.508 | 0.722 | 0.523 | 0.424 | 0.751 |
| 100 | 4 | 1.933 | 2.240 | 1.984 | 1.723 | 4.430 |
| 100 | 8 | 3.990 | 4.592 | 4.031 | 3.537 | 4.756 |
| 100 | 16 | 7.513 | 8.112 | 7.560 | 7.162 | 8.920 |
| 100 | 32 | 15.955 | 17.386 | 16.076 | 15.084 | 18.889 |
| 500 | 1 | 1.977 | 2.370 | 2.021 | 1.826 | 2.620 |
| 500 | 4 | 8.046 | 8.581 | 8.081 | 7.684 | 8.746 |
| 500 | 8 | 16.724 | 66.325 | 23.867 | 15.358 | 138.576 |
| 500 | 16 | 56.044 | 162.299 | 75.367 | 38.084 | 390.855 |
| 500 | 32 | 72.469 | 83.244 | 73.806 | 66.034 | 91.629 |
| 1,500 | 1 | 3.210 | 4.763 | 3.456 | 2.779 | 10.263 |
| 1,500 | 4 | 13.512 | 18.343 | 13.764 | 10.731 | 23.494 |
| 1,500 | 8 | 23.655 | 27.166 | 23.764 | 21.190 | 28.036 |
| 1,500 | 16 | 49.532 | 57.412 | 50.631 | 44.729 | 62.578 |
| 1,500 | 32 | 118.517 | 309.943 | 149.513 | 96.354 | 387.302 |

---

## 4. Architectural Analysis & Cost Scaling

### 4.1 Cost Models
Let $P$ be the number of panes in the batch, and $N$ be the number of candidate files/sessions in the provider home:

- **Codex Cost Model**:
  $$\text{Cost}_{\text{Codex}} \approx P \times \left( C_{\text{readdir}} \times N \right) + C_{\text{open\_tail}}$$
  `matching_rollout` filters candidates on `path.file_name()` without opening file descriptors for non-matching sessions. Each pane executes directory readdir calls, scaling linearly with $P \times N$. At 3,000 candidate files and 32 panes, total batch discovery takes ~25 ms on `tmpfs`.

- **Kimi Cost Model**:
  $$\text{Cost}_{\text{Kimi}} \approx P \times \left( C_{\text{readdir}} \times N + \sum_{i=1}^N \left( C_{\text{open}} + C_{\text{fstat}} + C_{\text{cache\_check}} \right) \right)$$
  Because Kimi sessions do not encode native session ID or repository binding in the directory path, every `state.json` is opened and inspected via `state_binding()`. For 32 panes and 1,500 candidate sessions, this executes 48,000 candidate evaluations ($P \times S$) across the batch of pane reads. Even with `STATE_CACHE` hits avoiding repeated JSON parsing, metadata inspection scales total latency to 118.52 ms p50 and 309.94 ms p95 on `tmpfs`.

---

## 5. Fail-Closed Verification & Boundary Conditions

The table below delineates behaviors validated directly via synthetic fixture injection versus those verified through static code audit and unit test inspection.

| Boundary / Vulnerability Condition | Codex Behavior | Kimi Behavior | Verification Status | Source Reference |
|---|---|---|---|---|
| **Duplicate Native Session ID** | Returns `Unavailable(Unassociated)` when multiple files match | Returns `Unavailable(Unassociated)` when multiple state files match | **PASS (Fixture Injected)** | `reader_codex.rs:72`, `reader_kimi.rs:90` |
| **Permission Denied on Potential Duplicate** | Returns `PermissionDenied` | Marks `unproven_duplicate = true` if directory name could match, returning `Unassociated` | **PASS (Code Audit)** | `reader_kimi.rs:85-88` |
| **Symlinked Session Root** | `symlink_metadata(root)` rejects root as `Invalid` | `symlink_metadata(root)` rejects root as `Invalid` | **PASS (Fixture Injected)** | `model.rs:513` |
| **Symlinked Child Entry** | `entry.file_type()` does not follow directory/file symlinks | `open_nofollow` enforces `O_NOFOLLOW`; components validated with `symlink_metadata` | **PASS (Code Audit)** | `model.rs:528-530`, `reader_kimi.rs:162-205` |
| **Corrupted Payload / Non-Object Record** | Returns `Invalid` or `SourceError` without panic | Returns `Invalid` on corrupt wire; ignores corrupt unrelated state files | **PASS (Fixture Injected)** | `reader_codex.rs:135`, `reader_kimi.rs:78, 245` |
| **In-Flight Append** | `snapshot_change` returns un-cached valid prefix observation | Allowed if complete turn was parsed prior to append | **PASS (Code Audit)** | `reader_codex.rs:114`, `reader_kimi.rs:150` |
| **In-Flight Shrink / Move / Rewrite** | `snapshot_change` detects size reduction / inode change -> `SourceError` | `stable_file` detects state/config change; wire cache miss checks `snapshot_change` -> `SourceError`. However, wire cache hit (`reader_kimi.rs:222-228`) skips pathname/metadata revalidation after fingerprinting, leaving a residual race. | **PARTIAL (Code Audit)** | `reader_codex.rs:115`, `reader_kimi.rs:156, 222-228, 240-244` |
| **Directory Cap Exceeded (> 4,096 Entries)** | Visited entries $>4,096$ returns `EntryLimit`, invalidating cached state | Visited entries $>4,096$ returns `EntryLimit`, invalidating cached state | **PASS (Fixture Injected)** | `model.rs:520-524` |
| **Reader Slot Contention (`BUSY` Flag)** | `BUSY.compare_exchange` fails -> returns `Timeout` immediately | `BUSY.compare_exchange` fails -> returns `Timeout` immediately | **PASS (Code Audit)** | `model.rs:314-320` |
| **Cycle Deadline Expiry** | `Budget::check()` aborts traversal with `Timeout` | `Budget::check()` aborts traversal with `Timeout` | **PASS (Fixture Injected)** | `model.rs:495-497` |

### 5.1 In-Flight Mutation and Cache Revalidation Nuance
While `stable_file()` protects `state.json` discovery and configuration loading, and wire cache-miss paths execute `snapshot_change` comparing pre- and post-read metadata, Kimi's wire `CACHE` hit path (`reader_kimi.rs:222-228`) returns cached turn records immediately following `budget.check()`. It does not perform a secondary pathname `stat` or inode verification. If a wire file is substituted or rewritten concurrently after the initial fingerprint check during a cache hit cycle, the cache return path does not detect the pathname mutation.

### 5.2 Unmeasured Conditions
The following conditions were not measured or fall outside the synthetic test harness:
1. **Live User Sessions**: Real production sessions and personal credentials were not accessed.
2. **Block Storage & Network Filesystems**: All tests executed on RAM-backed `tmpfs`. Latency profiles on block storage (`ext4`, `btrfs`, `xfs`) or high-latency network storage (NFS, SMB, SSHFS) were unmeasured. On storage with non-negligible seek or open overhead, Kimi's 48,000 evaluation pattern would compound latency significantly.
3. **Directory Scales Beyond 4,096 Entries**: Traversal beyond 4,096 files was not timed because `Budget::files` fails-closed with `MissingReason::EntryLimit` at entry 4,097.

---

## 6. Recommendations & Conclusions

1. **Safety Boundary Adherence**: Both readers consistently fail-closed when encountering ambiguous duplicates, symlink redirection, malformed inputs, or budget exhaustion.
2. **Kimi Discovery Scaling**: In multi-pane setups (e.g. 32 panes) with many historical sessions (1,500+), Kimi's candidate evaluation cost ($P \times S$) is a substantial overhead within the reader discovery stage itself, reaching ~310 ms p95 on `tmpfs`. If necessary in future revisions, indexing sessions or caching the directory scan across panes within the same cycle would reduce repeated evaluations.
3. **Hard Bounds & Caller Wait Isolation**: The `MAX_DIRECTORY_ENTRIES = 4096` cap and configurable `reader_timeout_ms` (default 2,000 ms) successfully bound caller wait times against runaway filesystem traversals. Note that if an OS read stalls in the background worker thread, that worker continues to hold the provider's `BUSY` permit until termination, causing subsequent reads for that provider to return `Timeout` without leaking un-bounded threads.
