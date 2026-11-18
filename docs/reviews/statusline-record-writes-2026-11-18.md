# Direct statusLine record writes, 2026-11-18

The grouped writer reduced each measured invocation from 47 `write`
calls to 1 and from 122 total syscalls to 76. The median paired drop in
userspace instructions was 511 for AB and 184.5 for BA, both within the
1,553-instruction A/A spread. The candidate did not meet the adoption
criterion and the production writer was left at A.

## Measurement setup

This follows candidate 6 of the [CPU and memory baseline](performance-cpu-memory-baseline-2026-11-18.md).
A is `origin/develop` at `5eb6493a997b02828045dab45704af2658ee298e`.
B serializes the same record with `serde_json::to_vec`, writes it with
`write_all`, and flushes and persists the same kind of private temporary
file. Both binaries were built in the same worktree with Rust 1.100.0 and
`cargo build --release --locked`.

| Input | SHA-256 |
| --- | --- |
| `Cargo.lock` | `1c45b9615e8fa56316555bd4cbb399844838360dad92d453da8ef3d1ae8d82ea` |
| A release binary | `995ff0fc38b7047ba949a265c33a4a4a47031cc4d157a1722a0474d0763320cb` |
| B release binary | `975fc493f063a184b2e2097db4b6b40be46b29235a760ab54c83f3fbe28957d5` |
| 113-byte stdin | `c46fc31a7f1b65df42c38f561937171cd0c79f7eb8ca553d393de8795806477a` |

Each sample invoked `herdr-lineup statusline claude` directly with the
baseline's one-line fixture, including the trailing newline:

```json
{"session_id":"lineup-fixture","context_window":{"used_percentage":23.5},"model":{"id":"claude-sonnet-fixture"}}
```

The controller reused one private ext4 state directory and replaced the
record on every timed invocation. Both binaries ran with the same
`HERDR_PLUGIN_STATE_DIR`, `HOME`, `LC_ALL=C`, and `PATH=/usr/bin:/bin`
in an otherwise empty environment. No Herdr session was involved. The
controller and its children inherited affinity to CPU 22 on the shared
24-logical-CPU Ryzen 9 9955HX host (Linux 7.3.3-1-cachyos). Affinity did
not reserve the CPU. One-minute system load at the start and end was
103.46 / 103.46 for A/A and 74.00 / 71.28 for the interleaved pass.

Each pass warmed both slots once, then recorded 12 pairs. A/A used the
same A binary in both slots. The A/B pass interleaved six AB and six BA
pairs. Syscalls came from `strace -qq -c` (7.3), userspace instructions
from `perf stat --no-big-num -x, -e instructions:u` (7.3.1-1), and CPU
from `wait4` user plus system time for the directly launched child.
These were separate passes. All perf samples reported 100% running time.
The A/A spread is the max minus min across the 24 control samples for each
metric. The gate requires lower write and total syscall counts and fewer
userspace instructions, with the median paired drop in each order exceeding
the A/A spread.

## Results

Each entry in the first table is min / median / max per direct invocation.

| Metric | A/A (n=24) | A/A spread | A in AB/BA (n=12) | B in AB/BA (n=12) |
| --- | ---: | ---: | ---: | ---: |
| `write` calls | 47 / 47 / 47 | 0 | 47 / 47 / 47 | 1 / 1 / 1 |
| Total syscalls | 122 / 122 / 122 | 0 | 122 / 122 / 122 | 76 / 76 / 76 |
| Userspace instructions | 466,692 / 467,335 / 468,245 | 1,553 | 466,789 / 467,526.5 / 468,166 | 466,487 / 467,252 / 468,016 |
| CPU (µs) | 968 / 1,145.5 / 1,583 | 615 | 1,144 / 1,335.5 / 1,488 | 1,214 / 1,424 / 1,633 |

The paired differences below are A minus B, reported as min / median /
max. Positive values indicate a reduction.

| Metric | AB (n=6) | BA (n=6) |
| --- | ---: | ---: |
| `write` calls | 46 / 46 / 46 | 46 / 46 / 46 |
| Total syscalls | 46 / 46 / 46 | 46 / 46 / 46 |
| Userspace instructions | −1,227 / 511 / 1,092 | −184 / 184.5 / 616 |
| CPU (µs) | −246 / −169.5 / −76 | −105 / −64 / 37 |

Both orders beat the syscall spread. The instruction and CPU measurements
give no demonstrated improvement beyond their respective A/A spreads.
The measurements cover a single synthetic direct Claude callback on Linux.
Production frequency, relayed commands, Antigravity performance, and macOS
runtime performance were outside this measurement.

## Behavior checks

- Every timed invocation exited 0 with empty stdout and stderr. Each
  replaced the old inode with a single mode-0600 record. Substituting
  only the timestamp digits for byte comparison gave identical records.
- A separate direct CLI check fixed `CLOCK_REALTIME` with a preload shim.
  Both executables wrote identical 143-byte records with SHA-256
  `f1b0fe4ed3d02c8e84a4edcbb1cb960863857dddad1f4b28f9ee8ca784ed20ec`.
  The shim was absent from the timed passes.
- A focused trace showed the 47 A writes and the single 143-byte B write
  targeting temporary files before `renameat`. Injecting `ENOSPC` at
  the first write preserved the previous inode and bytes and left no
  temporary file. Direct mode stayed silent and exited 0. With
  `--then`, the child received the fixture unchanged and its stdout,
  stderr, and exit 7 were preserved for both versions.
- Temporary Rust tests checked an already-open reader across replacement,
  exact record bytes at a fixed time, mode 0600, valid JSON at the 1 MiB
  input limit and one byte above it, and a real partial-write failure under
  a subprocess file-size limit. The atomic replacement test failed at
  the inode assertion when the writer was temporarily changed to
  truncate and overwrite the published file. All 15 statusLine tests
  passed with the grouped writer. The prototype and its temporary tests
  were removed from the final diff.
