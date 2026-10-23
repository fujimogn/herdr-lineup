# Git Heading Cache Invalidation Contract and Safe Implementation Design

| Field | Details |
|---|---|
| Date | 2026-10-24 |
| Target commit | `96829df` (`96829df7e6fc51af327b448f5b8b3596f0a96d3f`) |
| Touched components | `src/ship_time.rs`, `src/publish.rs`, `docs/reviews/git-cache-design.md` |
| Author | agy (`lane_git_reader`) |
| Review History | Sol-6.1 high v1–v5 NO-GO -> v6 NO-GO -> v7 (Final Simple Guard: Unconditional Non-Cache on Replacement Directory Presence) |
| Status | DESIGN / PROPOSAL v7 (Absolute Replacement Ref Invalidation Guard & Clean Truth Table) |

---

## 1. Executive Summary & Review Evolution

In `herdr-lineup`, the sidebar displays repository identity (`lineup_repo`) and the `refs/remotes/origin/main` commit timestamp (`lineup_ship_text` / `lineup_ship_unix`).
The whole-repo latency investigation (`reviews/latency-2026-10-23/consolidated.md`) identified the synchronous execution of **3 sequential Git child processes** per unique `cwd` as a P1 contributor to daemon latency:
1. `git rev-parse --is-inside-work-tree --path-format=absolute --git-common-dir` (~1.1 ms)
2. `git worktree list --porcelain -z` (~1.5 ms)
3. `git for-each-ref --format=%(refname) %(committerdate:unix) refs/remotes/origin/main` (~1.4 ms)

### Review History & Flaw Resolutions
- **Design v1–v3**: Corrected invalidation paths, replaced fragile pure-stat `cwd` traversal with mandatory per-cycle `rev-parse`, and established round-robin attempt fairness.
- **Design v4**: Formalized attempt-based fairness and 30s revalidation, but lacked an observation API separating cache verification age from display fallback age, missed packed-only replacement refs, and contained an unproven sub-200KiB claim.
- **Design v5**: Introduced `RepositoryObservation` API and packed `for-each-ref` multi-pattern detection, but removed fast-path `$COMMON_DIR/refs/replace/` directory stat.
- **Design v6**: Reintroduced `$COMMON_DIR/refs/replace/` presence check as a fingerprint equality comparison. As caught by Sol-6.1 high, if an empty `refs/replace/` directory already exists, adding a new loose replacement ref leaves presence as `Present -> Present`, masking the replacement ref behind an apparent fingerprint match!
- **Design v7 (Current & Commander Ruling)**:
  1. **Unconditional Non-Cache Guard on Replacement Directory Presence**:
     - No optimization or exception for empty replacement directories.
     - If `$COMMON_DIR/refs/replace/` is `Present` (even if empty), the repository is **unconditionally uncacheable**: it never uses the fast path and is never stored in the cache.
     - Caching is permitted **ONLY when `$COMMON_DIR/refs/replace/` is strictly `Absent`** and the slow-path multi-pattern query reports no packed replacement refs.
  2. **Comprehensive Truth Table**: Fully unified across narrative, flowcharts, data structures, and store conditions.
  3. **Typed `RepositoryObservation` API**: Explicitly carries `verified_at: Option<Instant>`. Real non-cache successes (including `replace`, `reftable`, `symref`) update `confirmed = now`, whereas cache hits inherit the original verification instant without extension.
  4. **Strict Scope Discipline**: Confined solely to an LRU capacity bound of 64 entries without unproven byte bounds.

---

## 2. Invalidation Truth Table & Replacement Ref Semantics

### 2.1 Exhaustive Invalidation Matrix

| Invalidation Vector | Detection Mechanism | Cache Action | Boundary & Safety Contract |
|---|---|---|---|
| **Subdirectory `cwd`** | Native `git rev-parse` (every cycle). | Evaluated by native Git. No filesystem stat at `cwd`. | Native parent traversal guarantees correct root discovery. |
| **`safe.directory` / Ownership** | `git rev-parse` exit status. | Non-zero exit immediately purges cache entry for `common-dir` and returns `Complete(None)`. | **Fail-Closed**: Authorization or ownership failure immediately drops state. |
| **Git Configuration (`.git/config`)** | `git rev-parse` execution + stat on `$COMMON_DIR/config` (`ino`, `mtime`, `ctime`, `size`). | Config file mutation forces full slow path. | Evaluates native includes; stat guards downstream. |
| **Root Worktree Lifecycle** | Stat cached main worktree root (`ino`, `dev`). | Directory absence or inode shift forces `worktree list`. | Canonical repository name is invalidated immediately. |
| **Worktree Topology (`add`/`prune`)** | Stat `$COMMON_DIR/worktrees/` directory (`mtime`, `ctime`). | Directory timestamp change forces `worktree list`. | Linked worktree additions/removals force re-evaluation. |
| **`origin/main` Loose Refs** | Stat `$COMMON_DIR/refs/remotes/origin/main` (`ino`, `mtime`, `ctime`, `size`, or `Absent`). | Loose ref mutation forces `for-each-ref`. | Atomic compound tracking with `packed-refs`. |
| **`origin/main` Packed Refs** | Stat `$COMMON_DIR/packed-refs` (`ino`, `mtime`, `ctime`, `size`, or `Absent`). | Packed-refs mutation forces `for-each-ref`. | Loose-to-packed and packed-to-loose detected atomically. |
| **Replacement Directory (`refs/replace/`)** | Stat `$COMMON_DIR/refs/replace/` (`Absent` vs `Present`). | **If `Present`, unconditionally uncacheable.** | Simple guard: Never cache if directory exists, empty or not. |
| **Packed-Only Replacement Refs** | Slow-path dual query (`refs/remotes/origin/main` + `refs/replace`). | If `refs/replace/` is output, **uncacheable**. | Catches replacement refs packed in `.git/packed-refs`. |
| **Reftable Backend** | Stat `$COMMON_DIR/reftable/tables.list`. | If present, **bypass downstream cache** (always slow path). | Binary table merging cannot be safely tracked by file stat. |
| **Subprocess Group Reaping** | `process_group(0)`, `waitid(WNOWAIT)`, `killpg(SIGKILL)`, `wait()`. | Retained unmodified from `src/ship_time.rs`. | Unreaped leader pins PGID, preventing PID recycling races. |

### 2.2 Replacement Reference Uncacheability Truth Table

| `$COMMON_DIR/refs/replace/` Directory State | `for-each-ref` Output for `refs/replace` | Downstream Cache Disposition | Fast-Path Eligible? |
|---|---|---|---|
| **`Absent`** | **None** | **Cacheable** (if other conditions clean) | **YES** |
| **`Absent`** | **`refs/replace/...` found** (packed-only) | **Uncacheable** (never stored, evicts existing) | **NO** (always slow path) |
| **`Present`** (empty or non-empty) | *(Any)* | **Uncacheable** (never stored, evicts existing) | **NO** (always slow path) |
| **`Inaccessible`** (EIO / Permission) | *(Any)* | **Fail-Closed** (`Complete(None)`, evicts existing) | **NO** (immediate error) |

#### Rationale for the Simple Guard:
- Comparing `Present == Present` as a fingerprint match fails whenever an existing empty `refs/replace/` directory receives a newly created replacement ref (which does not touch `origin/main` or `packed-refs`).
- Inspecting directory entries or counting files introduces fragile directory-read I/O and TOCTOU races.
- Therefore, the rule is absolute: **`refs/replace/` must be `Absent` for a repository to be eligible for caching.** If it exists in any form, the repository runs full Git on every cycle.

---

## 3. Typed Observation API & Confirmation Semantics

### 3.1 Observation Data Structures

To prevent caller/callee ambiguity regarding whether a result represents fresh Git verification or cached data, `src/ship_time.rs` exports a typed observation:

```rust
pub struct RepositoryObservation {
    pub read: RepositoryRead,
    /// The instant at which this repository data was verified via actual Git execution.
    /// - Some(now) on successful full Git slow path (including uncacheable results).
    /// - Some(entry.downstream_verified_at) on cache hit (inheriting the original instant).
    /// - None on failure, rejection, or interruption.
    pub verified_at: Option<Instant>,
}

pub(crate) struct CachedRepository {
    pub repository: Option<Repository>,
    pub fingerprint: CompoundGitFingerprint,
    pub downstream_verified_at: Instant, // Set strictly on Git execution; never renewed on hit
    pub last_access: Instant,             // Updated on hit for LRU eviction ordering
}
```

### 3.2 Dual-Layer 30-Second Freshness Contract
The 30-second staleness bound operates across two distinct architectural layers:
1. **Downstream Cache Revalidation Limit (`ship_time.rs`)**:
   - On every query, if a cache entry matches the compound fingerprint but `now - entry.downstream_verified_at >= Duration::from_secs(30)`, the fast path **falls through to the slow path**.
   - Downstream cache hits update `last_access` (for LRU ordering), but **never renew** `downstream_verified_at`.
   - When a newly discovered workspace hits an existing cache entry, it receives the entry's original `downstream_verified_at`, inheriting its remaining TTL without renewal.
2. **Display Fallback Age Limit (`publish.rs`)**:
   - `HeadingCache` stores `confirmed: Instant`, updated whenever `RepositoryObservation.verified_at` is `Some(instant)`.
   - Healthy non-cache operations (e.g. repositories with active replacement refs running full Git every 5s) successfully update `confirmed = now` on every cycle.
   - If an interrupted read occurs (`read == RepositoryRead::Interrupted`), `publish.rs` retains the previous heading **only if** `now - confirmed < Duration::from_secs(30)`.
   - If interruptions persist for $> 30\text{ s}$, the heading is cleared to `"—"`.

---

## 4. Comprehensive Outcome & State Transition Matrix

The table below specifies the exact outcome, cache mutation, timestamp behavior, and sidebar display fallback for all possible conditions:

| Condition / Event | Observation `read` | Observation `verified_at` | Downstream Cache Entry Mutation | `HeadingCache.confirmed` Action in `publish.rs` | Sidebar Heading Display |
|---|---|---|---|---|---|
| **`rev-parse` fails (non-Git, `safe.directory` reject, broken `.git`)** | `Complete(None)` | `None` | **Purged immediately** (`cache.remove(common_dir)`) | Not updated | Clears sidebar heading to `"—"` immediately. |
| **Fast Path Hit ($F_{\text{now}} == F_{\text{entry}}$, `replace` is `Absent`, age $< 30\text{ s}$)** | `Complete(Some(cached))` | `Some(entry.downstream_verified_at)` | Retained. `last_access` updated to `now`. `downstream_verified_at` **unchanged**. | Updated to `obs.verified_at` (inherits original instant) | Renders cached repository name and timestamp. |
| **Hard Age Expiry ($F_{\text{now}} == F_{\text{entry}}$ BUT age $\ge 30\text{ s}$)** | *Falls through to slow path* | *Follows slow path rules* | Evaluated via slow path. | Follows slow path rules | Refreshed from live Git read. |
| **Slow Path Success ($F_{\text{pre}} == F_{\text{post}}$, cacheable, `replace` is `Absent`)** | `Complete(Some(repo))` | `Some(now)` | Inserted/Overwritten with new fingerprint, `verified_at = now`, `last_access = now` | **Updated to `now`** | Renders updated repository name and timestamp. |
| **TOCTOU Race ($F_{\text{pre}} \neq F_{\text{post}}$ during query)** | `Complete(None)` | `None` | **Purged / Not cached** (unverified intermediate data discarded) | Not updated | Clears heading (fail-closed); will re-attempt on next cycle. |
| **Uncacheable Read (`replace` Present, `ship_unix == None`, `reftable`, `symref`)** | `Complete(Some(repo))` or `Complete(None)` | `Some(now)` | **Never cached** (evicts existing entry if present) | **Updated to `now`** (validates live health) | Renders live query result; slow path repeats next cycle. |
| **I/O Error / Permission Denied (`Inaccessible`)** | `Complete(None)` | `None` | **Purged immediately** | Not updated | Clears sidebar heading (fail-closed). |
| **Normal `NotFound` (absent `packed-refs`, `worktrees/`, or `refs/replace/`)** | *Normal flow* | *Normal flow* | Recorded as `FileFingerprint::Absent` | *Normal flow* | Normal fast-path evaluation allowed. |
| **Query Deadline / Cycle Budget Exceeded** | `RepositoryRead::Interrupted` | `None` | **No change to cache** | Not updated | Retains previous heading IF `now - confirmed < 30\text{ s}`; otherwise clears to `"—"`. |

---

## 5. Attempt-Based Fair Queue Scheduling & Anti-Starvation Contract

### 5.1 Queue Data Structure & Lifecycle
Instead of index arithmetic over snapshot slices, `publish.rs` maintains an explicit persistent queue:
```rust
pub struct WorkspaceFairQueue {
    pending: std::collections::VecDeque<String>, // Workspace IDs
}
```

#### Lifecycle Rules per Refresh Cycle:
1. **Reconciliation**:
   - Compare `pending` against current `snapshot.workspaces`.
   - Remove workspace IDs that no longer exist in the snapshot.
   - Append newly discovered workspace IDs to the **back** of `pending`.
2. **Attempt-Driven Rotation**:
   - At the start of query processing for a workspace, pop `id` from the **front** of `pending`.
   - Once a query attempt is initiated (i.e. child process spawned or fast path evaluated), push `id` to the **back** of `pending`.
   - **Crucial Rule**: Even if the query hits its deadline and times out (`RepositoryRead::Interrupted`), it has consumed an attempt and is moved to the back. A failing or hanging workspace can **never** starve subsequent workspaces in future cycles.
3. **Budget Pre-Emption (No Attempt)**:
   - If `Instant::now() >= cycle_budget_deadline` *before* a workspace query is initiated, that workspace is **not spawned** and remains at the **front** of `pending`.
   - It will be the first workspace evaluated in the subsequent refresh cycle.
4. **Fairness Invariant**:
   - Under a static set of $N$ workspaces where the cycle budget permits at least 1 attempt per cycle, **every workspace is guaranteed an evaluation attempt within at most $N$ refresh cycles**.

### 5.2 Cooperative Cycle Budget & Dynamic Deadline Clamping
- **Initial Cycle Budget**: **1,000 ms (1.0 s)** wall-clock elapsed time (aligned with the single-query deadline limit).
- **Workspace Query Clamped Deadline**:
  $$\text{effective\_deadline} = \min(\text{now} + \text{Duration::from\_secs}(1), \text{cycle\_budget\_deadline})$$
- **Non-Guarantee of Strict Wall Bounds**:
  - The cycle budget is cooperative. It checks `Instant::now()` before spawning a child.
  - If a child process blocks inside an un-interruptible kernel I/O call (D-state sleep on NFS/CIFS), the child reap cannot complete until the kernel driver unblocks. The daemon guarantees it will send `SIGKILL` to the process group, but cannot bypass kernel hardware wait states.

---

## 6. Architectural Seams & Capacity Bounds

### 6.1 Seam Division of Responsibilities

| Subsystem | File | Concrete Responsibilities |
|---|---|---|
| **Daemon Scheduler** | `src/publish.rs` | 1. Owns `WorkspaceFairQueue` and drives attempt-based rotation.<br>2. Computes `cycle_budget_deadline = Instant::now() + Duration::from_millis(1000)`.<br>3. Clamps query deadlines and skips spawning when budget is exhausted.<br>4. Tracks `HeadingCache.confirmed` and enforces 30s staleness fallback. |
| **Downstream Cache & Identity** | `src/ship_time.rs` | 1. Executes mandatory `rev-parse` on every cycle.<br>2. Owns `BoundedRepositoryCache` (LRU max 64 entries).<br>3. Enforces that `$COMMON_DIR/refs/replace/` must be `Absent` for caching.<br>4. Enforces the 30s hard revalidation limit on cache hits.<br>5. Enforces pre/post query fingerprint matching ($F_{\text{pre}} == F_{\text{post}}$).<br>6. Returns typed `RepositoryObservation`. |
| **Process Lifecycle** | `src/ship_time.rs` | Unchanged: `git_output`, `GitChild`, `process_group(0)`, `waitid(WNOWAIT)`, `killpg(SIGKILL)`. |

### 6.2 In-Memory Capacity Bounds
- Cache capacity is strictly capped at **$N = 64$ entries** using an LRU eviction policy.
- All unproven sub-200KiB or sub-100KiB byte guarantees are explicitly retracted; arbitrary path lengths under POSIX `PATH_MAX` cannot be bounded without truncating paths.
- The 64-entry LRU cap guarantees $O(1)$ lookup time and prevents unbounded growth from ephemeral workspace creation.

---

## 7. Concrete Fixture Specifications for Integration Verification

The following 20 isolated fixture scenarios specify verification requirements in tests without external dependencies:

| Fixture ID | Test Scenario | Expected Outcome & Verified Invariant |
|---|---|---|
| **F-01** | Subdirectory `cwd` (`repo/src/deep`) | Cycle 1: 3 processes. Cycle 2: 1 process (`rev-parse`). Correct repository name resolved. |
| **F-02** | Linked Worktree Sharing | 3 linked worktrees of 1 repo. Worktree 1 runs slow path; Worktrees 2 & 3 run 1 process (`rev-parse`) and hit downstream cache on identical `common_dir`. |
| **F-03** | Worktree Topology Addition | `git worktree add`. `$COMMON_DIR/worktrees/` mtime shifts -> triggers slow path `worktree list`. |
| **F-04** | Loose Ref Update | `git update-ref refs/remotes/origin/main`. Loose ref mtime shifts -> triggers slow path `for-each-ref`. |
| **F-05** | Packed-Refs Transition | `git pack-refs --all`. Loose ref deleted, `packed-refs` created. Both transitions detected atomically. |
| **F-06A** | Warm Cache then Empty Replace Dir Created | Cache warmed without replacement (`Absent`). An empty `refs/replace/` dir is created. Transitions to `Present` -> cache immediately invalidated -> slow path runs. Subsequent loose replace added -> correctly read without stale hit. |
| **F-06B** | Initial Empty Replace Dir | Repo initialized with empty `refs/replace/` dir. Cycle 1 & 2 both run full Git (never cached). Loose replace added -> immediately reflected in next cycle. |
| **F-06C** | Packed-Only Replacement Ref | `git replace` active and packed via `git pack-refs --all --prune` with loose dir removed via `std::fs::remove_dir`. Dual-pattern query detects replace -> unconditionally non-cacheable. |
| **F-07** | Reftable Format Detection | `git init --ref-format=reftable`. Detected via `tables.list` -> non-cacheable slow path. |
| **F-08** | Symbolic Ref Handling | `refs/remotes/origin/main` is symbolic ref -> non-cacheable slow path. |
| **F-09** | Missing Ref (`ship_unix == None`) | Initial commit without `origin/main` -> non-cacheable; `for-each-ref` runs every cycle until ref exists. |
| **F-10** | TOCTOU Mutation | Mock wrapper mutates ref during `for-each-ref`. $F_{\text{pre}} \neq F_{\text{post}}$ -> returns `Complete(None)`, entry purged, slow path retried next cycle. |
| **F-11** | Hard Age Expiry (30s) | Cache hit occurs continuously. At $T=30\text{s}$, fast path falls through to slow path for re-verification. |
| **F-12** | Cache Hits Never Renew Age | Cache hit returns original `verified_at`. A new workspace hitting the existing entry also inherits the original timestamp. |
| **F-13** | Healthy Non-Cache Reads > 30s | Repository with active replacement refs runs full Git every 5s for 40s. `confirmed` advances on every cycle. An interruption at $T=45\text{s}$ successfully falls back to previous heading. |
| **F-14** | All Queries Timeout | All workspaces stall in Git. Every workspace consumes its 1.0s deadline, returns `Interrupted`, and rotates to queue back. All $N$ workspaces are attempted over $N$ cycles. |
| **F-15** | Workspace Churn | Workspaces added and removed dynamically during execution. Queue properly purges absent IDs and appends new IDs without deadlocks. |
| **F-16** | Cycle Budget Cutoff | Tight budget. Workspaces beyond budget cutoff return `Interrupted` **without spawning** `rev-parse`, and remain at the front of `pending`. |
| **F-17** | Stalled Process Kill & Reap | Hung child process terminated with `SIGKILL` at deadline; no zombie leaks. |
| **F-18** | Finite Staleness Expiry (30s) | Continuous `Interrupted` state for $> 30\text{ s}$ clears sidebar display from stale heading to `"—"`. |
| **F-19** | Normal NotFound Distinguishment | Repository lacking `packed-refs` or `worktrees/` records `Absent` and successfully utilizes fast path. |

---

## 8. Current Code Baseline vs. Pending Implementation

| Capability | Current Code (`96829df`) | Proposed Implementation (Pending) |
|---|---|---|
| `cwd` Identity & Security | Native Git in all 3 processes | Native Git preserved in mandatory `rev-parse` |
| Subprocess Execution | 3 processes unconditionally per `cwd` | 1 process (`rev-parse`) per `cwd`; 2 elided on downstream hit |
| Replacement Object Support | Verified via baseline unit test | Simple guard: `refs/replace` present -> uncacheable; dual query catches packed refs |
| Downstream `common_dir` Cache | None | LRU max 64 entries in `ship_time.rs` with 30s hard revalidation limit |
| Observation API | `RepositoryRead` enum only | `RepositoryObservation` struct with explicit `verified_at` timestamp |
| Starvation Prevention | Fixed array index traversal | `WorkspaceFairQueue` attempt-based rotation in `publish.rs` |
| Query Deadline Clamping | Static 1.0s deadline per workspace | $\min(\text{now} + 1.0\text{s}, \text{cycle\_budget\_deadline})$ in `publish.rs` |
| Finite Fallback Age | Indefinite fallback on `Interrupted` | 30s staleness timeout clearing heading to `"—"` |

---

## 9. Production Implementation Status

The downstream cache in `src/ship_time.rs` implements this contract. **Publication is not wired yet**: `src/publish.rs` still calls `read_repository_controlled`, which observes through a fresh `RepositoryCache` on every call and therefore always performs the full Git read. The `WorkspaceFairQueue`, cycle budget, and `HeadingCache.confirmed` fallback (§5, F-14 to F-18) belong to the follow-up wiring task, which also removes the `dead_code` allowance on `RepositoryObservation::verified_at`.

### 9.1 API

- `RepositoryCache::observe(cwd, shutdown, deadline) -> RepositoryObservation`. The effective deadline is `min(now + 1 s, deadline)` for the caller's absolute deadline. Git children are killed and reaped at that deadline exactly as before; synchronous `stat` and config reads are bounded only cooperatively.
- Cache hits update only `last_access` and return the entry's original `verified_at`. Full reads, including uncacheable successes, return `verified_at = now`. Rejections, discards, and interruptions return `None`.
- Capacity is 64 entries with least-recently-accessed eviction; the verified TTL is 30 s.

### 9.2 Refinements Made During Implementation

| Point | Implemented rule | Reason |
|---|---|---|
| `rev-parse` refusal (`safe.directory`, broken `.git`, unsupported format) | Clear **every** entry and return `Complete(None)` | A refused cwd reveals no common directory to key the purge on. Commander ruling: clear all (at most 64 entries) rather than keep old entries; every repository is then verified again by a full read. |
| Ref backend | `rev-parse --show-ref-format` must print `files`; otherwise always full read | Covers reftable and unknown backends. Git without the option echoes it back, which is treated as an unknown backend. The `reftable/` stat remains as a second guard. |
| Included configuration | A common `config` mentioning `include` (case-insensitive, also `includeIf`) is never cached | The fingerprint cannot follow included files. |
| Object alternates | `objects/info/alternates` is part of the fingerprint | Object availability can change through alternates. |
| Loose `origin/main` symlink | Never cached | A symlinked loose ref changes with an untracked target. |
| `origin/main` symref | Detected with `%(symref)` in the slow query; never cached | The symref file does not change when its target moves. |
| Main root stat | Kept as defense in depth | Git derives the main root from the common directory, so the common fingerprint already covers it (a `--separate-git-dir` main root is the common directory itself). |
| Pre/post comparison | The whole compound fingerprint (including `refs/replace` and `reftable`) must match; the main root must still exist | Mixed-state reads are discarded with the old entry. |

### 9.3 Fixture Coverage

`src/ship_time.rs` tests cover F-01 to F-13 and F-19 with full-query counts and verification instants, plus configuration change and include, refusal clearing all entries, unsupported repository format, re-initialization at the same path (ABA), missing objects, inaccessible dependencies, interruption by the caller deadline and after a completed full query, main root rename, and LRU eviction. Each guard was mutated once to confirm that at least one test fails; the `files` format check and the `reftable/` stat fail only when both are removed, and the main root stat cannot be isolated by a fixture.
