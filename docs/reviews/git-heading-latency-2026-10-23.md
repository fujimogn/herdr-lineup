# Git heading latency and safe change detection review

| Field | Details |
|---|---|
| Date | 2026-10-23 |
| Target commit | `b5fc311` (`b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`) |
| Component | `src/ship_time.rs`, `src/publish.rs`, `src/config.rs` |
| Reviewer | agy (lane: `lane_latency_measure_agy_3`) |
| Environment | Linux 7.2.10-1-cachyos / x86_64, isolated temporary filesystem fixtures |
| Primary touching path | `docs/reviews/git-heading-latency-2026-10-23.md` |

---

## 1. Executive Summary

This review investigates the sequential child process overhead and safe change detection conditions of the Git heading subsystem in `herdr-lineup`. The plugin inspects the local Git workspace to display repository identity and `origin/main` commit timestamp (`lineup_repo` and `lineup_ship_text`) in the Herdr sidebar.

### Key findings
1. **Subprocess multiplication (code analysis & empirically verified)**: Each repository query executes **3 sequential Git child processes**:
   - `git rev-parse --is-inside-work-tree --path-format=absolute --git-common-dir`
   - `git worktree list --porcelain -z`
   - `git for-each-ref --format=%(refname) %(committerdate:unix) refs/remotes/origin/main`
2. **Current caching boundary**: `publish.rs::repository_read_once` deduplicates queries only within a single refresh cycle based on exact workspace `cwd` (`PathBuf`).
   - If multiple workspaces share the exact same `cwd`, only 1 query (3 processes) is spawned per cycle.
   - If workspaces belong to distinct repositories or different linked worktrees of the same repository, each workspace triggers a separate query. For 32 distinct repositories or 32 distinct linked worktrees, a single refresh cycle spawns **96 sequential Git processes**.
3. **No cross-cycle caching in steady state**: Default refresh interval is **5,000 ms** (`src/config.rs:25`). Completed reads unconditionally overwrite `HeadingCache` in `publish.rs` every cycle. The cache serves solely as a fallback when a read is `Interrupted` (deadline exceeded).
4. **Empirical latency profile (Linux 7.2.10-1-cachyos, isolated fixture, 50 iterations)**:
   - 0 workspaces: 0.00 ms (0 processes)
   - 1 workspace: p50 = 3.60 ms, p95 = 3.84 ms (3 processes)
   - 8 workspaces (same cwd): p50 = 3.28 ms, p95 = 8.06 ms (3 processes)
   - 8 workspaces (distinct repos): p50 = 29.66 ms, p95 = 35.71 ms (24 processes)
   - 8 workspaces (distinct worktrees of 1 repo): p50 = 32.56 ms, p95 = 39.79 ms (24 processes)
   - 32 workspaces (same cwd): p50 = 3.46 ms, p95 = 3.86 ms (3 processes)
   - 32 workspaces (distinct repos): p50 = 121.81 ms, p95 = 133.58 ms (96 processes)
   - 32 workspaces (distinct worktrees of 1 repo): p50 = 137.23 ms, p95 = 159.02 ms (96 processes)
5. **Cascading timeout hazard**: Because queries execute synchronously in series (deduplicating only identical `cwd` paths within a cycle), if $K$ queries across distinct directories each stall up to their cooperative deadlines, deadline waiting alone can approach roughly $K \times 1.0\text{ s}$. Un-abortable synchronous I/O (such as canonicalization) and child process teardown can extend delays further without a strict upper bound, stalling the single-threaded publish daemon loop and delaying sidebar updates.

---

## 2. Experimental Setup and Methodology

To measure latency and process counts without external interference or non-deterministic environment state, an isolated test harness (`bench_heading`) was constructed using dedicated Git fixtures on a local temporary filesystem.

- **Environment**: Linux 7.2.10-1-cachyos / x86_64.
- **Isolation boundary**: TMPDIR root contains a sentinel `.git` file (`gitdir: unavailable-fixture-git-dir`) preventing upward directory discovery.
- **Fixture types**:
  1. `single-repo`: Standalone Git repository with an initial commit and `refs/remotes/origin/main` ref.
  2. `distinct-repo-0` through `distinct-repo-31`: 32 independent repositories with distinct paths and commits.
  3. `base-wt-repo` with `worktree-0` through `worktree-31`: 1 primary repository with 32 linked worktrees created via `git worktree add --detach`.
- **Counting method**:
  - Subprocess counts were measured via an atomic counter wrapping every `Command::spawn` inside the harness, and verified to match code derivation ($N_{\text{queries}} \times 3$).
- **Measurement iterations**: 50 iterations per scenario following 3 warmup cycles.
- **Environment flags**: `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, environment restricted to `PATH`, `HOME`, `TMPDIR`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, with `GIT_NO_LAZY_FETCH=1` and `GIT_ALLOW_PROTOCOL=""`.

---

## 3. Empirical Latency and Process Cost Results

### Summary Table

| Scenario | Workspaces | Repos / Worktrees | Git Procs (Measured) | Iterations | Min (ms) | p50 (ms) | p90 (ms) | p95 (ms) | p99 (ms) | Max (ms) | Mean (ms) | StdDev (ms) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **0 workspaces** | 0 | None | 0 | 50 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 |
| **1 workspace (single repo)** | 1 | 1 repo | 3 | 50 | 3.28 | 3.60 | 3.80 | 3.84 | 4.25 | 4.25 | 3.60 | 0.19 |
| **8 workspaces (same repo, same cwd)** | 8 | 1 shared cwd | 3 | 50 | 2.88 | 3.28 | 3.77 | 8.06 | 8.23 | 8.23 | 3.70 | 1.32 |
| **8 workspaces (8 distinct repos)** | 8 | 8 distinct repos | 24 | 50 | 26.60 | 29.66 | 34.78 | 35.71 | 40.79 | 40.79 | 30.70 | 3.13 |
| **8 workspaces (same repo, 8 worktrees)** | 8 | 8 worktrees | 24 | 50 | 29.03 | 32.56 | 37.60 | 39.79 | 45.62 | 45.62 | 33.71 | 3.16 |
| **32 workspaces (same repo, same cwd)** | 32 | 1 shared cwd | 3 | 50 | 3.28 | 3.46 | 3.73 | 3.86 | 8.56 | 8.56 | 3.70 | 0.99 |
| **32 workspaces (32 distinct repos)** | 32 | 32 distinct repos | 96 | 50 | 108.08 | 121.81 | 130.38 | 133.58 | 149.61 | 149.61 | 122.01 | 7.77 |
| **32 workspaces (same repo, 32 worktrees)** | 32 | 32 worktrees | 96 | 50 | 120.71 | 137.23 | 149.66 | 159.02 | 183.79 | 183.79 | 139.30 | 11.81 |

### Observations
- **Subprocess latency scaling**:
  - The measured cost per Git subprocess on this test system is approximately 1.1–1.3 ms (including process creation, kernel exec, dynamic linking, Git startup, stdout pipe read, poll/wait, and process group reap).
  - 1 workspace (3 processes) takes 3.60 ms (p50).
  - Scaling is linear with the number of distinct `cwd` entries: 8 distinct repositories take ~30 ms, and 32 distinct repositories take ~122 ms.
  - Linked worktrees incur a slight additional overhead (~139 ms vs ~122 ms for 32 worktrees) due to resolving the `.git` pointer file and common directory indirection.
- **Memoization efficacy**:
  - When 32 workspaces point to the exact same directory path, `repository_read_once` suppresses 31 redundant invocations within that cycle, holding total cycle time to 3.46 ms (p50).
  - However, because memoization keys strictly on `cwd: PathBuf`, multiple workspaces pointing to different linked worktrees of the same repository receive zero memoization benefit, executing all 96 processes.

---

## 4. Code and Fixture Cross-Verification: Change Detection, Timeout, and Reap Safety

### 4.1 Change Detection Behavior

The implementation in `src/ship_time.rs` does not use inotify, fs-events, or filesystem mtime stamps. Instead, it relies on periodic synchronous polling:

1. **Ref updates (`origin/main`)**:
   - When a commit is made or `update-ref refs/remotes/origin/main` executes, the change is picked up on the next cycle via `git for-each-ref`.
   - Delay: At most 1 refresh cycle (default 5,000 ms, configurable down to 1,000 ms).
2. **Packed references (`git pack-refs --all`)**:
   - `git for-each-ref` seamlessly handles packed references without regression. Verified in isolated fixture: after packing loose refs into `.git/packed-refs`, reads continue to succeed with identical 3-process execution.
3. **Configuration alterations (`.git/config`)**:
   - `git_command` applies explicit safeguards: `-c core.fsmonitor=false`, `GIT_NO_LAZY_FETCH=1`, and `GIT_ALLOW_PROTOCOL=""`, while preserving only essential ambient environment variables (`PATH`, `HOME`, `TMPDIR`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`). Remote lazy fetches, external transport protocols, and background fsmonitor daemons are suppressed; other local repo/system configuration directives outside these keys remain subject to standard Git behavior.
4. **Worktree addition / deletion (`git worktree add/remove/prune`)**:
   - `git worktree list --porcelain -z` identifies the root worktree (`worktree /path/to/main`) regardless of the active worktree count, ensuring consistent naming.

### 4.2 Timeout, Kill, and Reap Safety

`src/ship_time.rs:180-300` implements process lifecycle bounds:

```rust
// Process group isolation prevents orphan background child leakage
let child = git_command(cwd)
    .process_group(0)
    .spawn()?;

// Non-reaped PID protection via waitid with WNOWAIT
libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);

// Clean group kill prior to reap
libc::killpg(pid, libc::SIGKILL);
owned.child.wait();
```

- **Reap safety**: By using `libc::waitid` with `libc::WNOWAIT`, the process remains unreaped until `killpg` has been sent to its process group. This eliminates the race condition where a quickly recycled PID could result in `SIGKILL` being delivered to an innocent process.
- **Per-workspace composite deadline**: `read_repository_controlled` establishes a cooperative 1-second deadline (`Instant::now() + Duration::from_secs(1)`) shared across all 3 sequential Git commands for that workspace query, rather than an independent per-command timeout. However, this is a cooperative deadline check between child process executions and polling loops, not a preemptive wall-clock upper bound: `read_repository_before` executes synchronous `fs::canonicalize`, and `GitChild::drop` waits synchronously on termination without an absolute deadline. Stalled I/O operations inside `canonicalize` or unresponsive child teardown cannot be aborted by the deadline.
- **The cascading delay risk**:
  - The 1-second deadline provides a cooperative upper target for the subprocess polling loop of a single workspace.
  - Because `publish.rs` iterates sequentially over workspaces in a single thread (deduplicating only identical `cwd` values), if $K$ queries across distinct working directories each experience stalls up to their individual deadlines, the cumulative delay from deadline waits alone can approach roughly $K \times 1.0\text{ s}$. Furthermore, un-abortable synchronous I/O (such as hung network filesystem canonicalization) and child process teardown can extend stalls further, meaning there is no strict wall-clock upper bound. Workspaces sharing the same `cwd` reuse cached results within the cycle, and queries completing before their deadline incur proportionally less delay.
  - During prolonged query stalls, socket heartbeats, quota token updates, and Herdr UI notifications are entirely blocked.

---

## 5. Safe Minimal Improvement Proposals

To reduce subprocess overhead and protect daemon responsiveness without introducing architectural complexity, the following minimal improvements are evaluated:

### Proposal A: Refresh Cycle Global Git Budget (Highest Safety)
- **Mechanism**: Complement the per-workspace 1-second deadline with a **shared cycle budget** (e.g. 150–200 ms total across all workspaces in one refresh pass).
- **Behavior**: If the cumulative time for Git reads reaches the budget during a cycle, remaining workspaces yield `RepositoryRead::Interrupted`, falling back to their cached heading in `self.headings`.
- **Benefit**: Bounds total publish loop delay under multi-workspace stalls while preserving existing semantics.
- **Tradeoff & Capacity Consideration**: A fixed budget such as 150–200 ms directly competes with the empirical p95 of 32 distinct worktrees (159.02 ms, max 183.79 ms). Under large workspace configurations, a 150 ms threshold would frequently trigger partial-cycle exhaustion, forcing trailing workspaces into persistent fallback mode. Adopting a fixed threshold therefore requires an explicit decision on workspace capacity bounds versus freshness requirements.

### Proposal B: In-Memory TTL for Stable Headings
- **Mechanism**: Introduce an in-memory TTL on `HeadingCache` in `publish.rs` (e.g. 15–30 seconds, spanning 3–6 default cycles).
- **Behavior**: If a workspace's `cwd` and occupants remain unchanged, skip child process execution until TTL expiry, returning the cached `Repository`.
- **Benefit**: Reduces steady-state subprocess frequency under stable workloads while keeping logic entirely in memory without brittle filesystem stat assumptions.
- **Freshness Tradeoff & Invalidation**: Relying solely on unchanged `cwd` and occupants cannot detect underlying Git mutations (`origin/main` ref updates, `packed-refs` consolidation, or config edits) during the TTL window. The heading remains stale throughout the TTL duration. Adopting this proposal requires either explicitly accepting a defined freshness tolerance (e.g. up to 15–30 s staleness) or incorporating an auxiliary invalidation trigger (such as stat-checking `.git/refs/remotes/origin/main` and `.git/packed-refs`).

### Proposal C: Worktree Discovery and Shared Ref Disambiguation
- **Mechanism**: Recognize that multiple worktrees share the same common Git directory (`.git`).
- **Nuance**: While repository root discovery (`worktree list`) is needed to resolve the canonical main worktree path, ref resolution (`origin/main`) reads from the common ref store. Caching common-dir mappings can avoid redundant ref lookups across linked worktrees of the same repository.

---

## 6. Unmeasured Bounds and Operational Hypotheses

1. **macOS process creation overhead (Hypothesis / Unmeasured)**:
   - The empirical benchmarks were performed exclusively on Linux (`x86_64`). Process creation on macOS via `posix_spawnp` may incur different scheduling and dyld costs; exact latency on macOS remains unmeasured.
2. **Network / Virtualized filesystems (Hypothesis / Unmeasured)**:
   - Remote filesystems (NFS, SMB, 9P, SSHFS) were not tested; metadata latency under network round-trips could substantially increase child process duration.
3. **Repository scale (Unmeasured)**:
   - Repositories with very large packed-refs files, thousands of worktrees, or complex ref namespaces were not benchmarked.
