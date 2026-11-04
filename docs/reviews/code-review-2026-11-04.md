# Source review — 2026-11-04

CR-01 resolved by `a9b4b84`: unreadable same-ID state candidates close association; unrelated unreadable candidates preserve a readable match.
CR-02 resolved by `51be817`: lifecycle lock, control handshake, and process-exit wait share one five-second absolute deadline; no shutdown request is sent after expiry.

The findings and reproductions below describe the historical reviewed source.

## Fixed scope and result

Reviewed source: `80e11d1604a2c4bb56d0fc008c157b3b4b179d0d` (`origin/develop` when this lane started).
Formation assignment: `asg_code_review`, lane `lane_review`.
This is a record-only candidate. It neither fixes the findings nor accepts,
integrates, pushes, or deploys code. Line numbers below refer to that source SHA.

Two P2 findings were reproduced on Linux with synthetic local fixtures. No P0
or P1 finding is asserted. P2 means a bounded correctness/operational defect
that should receive a separate fix, rather than a demonstrated critical exploit.

| ID | Category | Severity | Source | Finding |
| --- | --- | --- | --- | --- |
| CR-01 | Correctness / fail-closed association | P2 | `src/reader_kimi.rs:88–109` | An unreadable same-ID candidate is ignored unless its error is PermissionDenied. |
| CR-02 | Lifecycle / deadline correctness | P2 | `src/daemon.rs:1338–1369` | Stop can send its shutdown request after the five-second operation deadline. |

## CR-01 — Unreadable same-ID Kimi candidates do not consistently close association

**Evidence.** `read_session_in_cycle` marks a possibly duplicate session directory
unproven only for `MissingReason::PermissionDenied` (`src/reader_kimi.rs:100–103`).
The catch-all at line 105 ignores other failures. `state_binding` reads the whole
state file through `Budget::read_range` (`src/reader_kimi.rs:126–151`), which
returns `SourceError` above 16 MiB (`src/model.rs:1135–1137`). Consequently, an
oversized candidate in a directory named exactly like the requested native ID
is silently excluded from the uniqueness decision.

The provider contract explicitly requires an unreadable `state.json` in a
matching session directory to make context unavailable, because it could be a
duplicate (`openspec/specs/provider-readers/spec.md:207–211`). The code comment at
`src/reader_kimi.rs:96–99` states the same rationale without restricting it to
permission errors.

**Reproduction.** Invoke the public `KimiReader.read` on a synthetic Kimi home,
using provider `Kimi`, native ID `native-1`, repository `/synthetic/repo`, current
wall clock and a three-second reader budget:

1. Create `config.toml` with `[models.fixture]` and `max_input_size = 1000`.
2. Create `sessions/a/native-1/state.json` containing
   `{"id":"native-1","cwd":"/synthetic/repo"}`.
3. In that session's `agents/main/wire.jsonl`, write these two newline-terminated
   records, with `time` set to the current epoch milliseconds minus 1,000:

   ```json
   {"type":"profile.bind","agentId":"main","modelAlias":"fixture"}
   {"type":"token_counting.turn_recorded","agentId":"main","tokens":75,"time":1793790000000}
   ```

   The time above is illustrative; use the sampled fixture time, not this
   literal, when recreating the observation.
4. Read once: `Available(7.5)`.
5. Add `sessions/b/native-1/state.json` as a sparse file of `16 * 1024 * 1024 + 1`
   bytes and read again in a fresh process: still `Available(7.5)`.

**Impact.** A directory that could contain a second binding is not examined
successfully, yet context is presented as associated. The fixture establishes
failure to prove uniqueness; it does not establish an actual wrong-account
value or any access to real session data. Other I/O failures mapped to
`SourceError` follow the same branch by inspection, but were not fault-injected.

**Improvement.** Preserve the immediate timeout/entry-limit return, but treat
errors that prevent proving a candidate's binding as unproven when its session
directory could match the native ID. Decide explicitly how a vanished file and
a successfully parsed, unrelated state differ. Keep unrelated-directory failures
from hiding a valid match.

**Test gap.** `unreadable_state_hides_context_only_when_it_could_be_the_same_session`
(`src/reader_kimi.rs:1081`) covers chmod-based PermissionDenied. It does not cover
the size-limit/SourceError path. Add the matching-ID oversized case and an
unrelated-ID negative control; keep duplicate readable-match coverage.

## CR-02 — Stop's control handshake resets the remaining budget

**Evidence.** `stop_with_lock` creates an absolute five-second deadline and passes
it to `LifecycleLock::acquire` (`src/daemon.rs:1340–1341`). After acquiring that
lock, it calls `control_stream`, `read_identity`, `write_frame`, and `read_frame`
without the operation deadline (`src/daemon.rs:1343–1357`). These helpers create
fresh local budgets: connect gets one second (`src/socket.rs:208–216`), and each
frame helper gets 500 ms (`src/socket.rs:378–379`, `src/socket.rs:427–428`).
The stop deadline is next checked only in the process-exit loop at line 1366.

**Reproduction.** Use a synthetic Unix control peer, not an actual Herdr server:

1. Bind a private `herdr.sock` only to establish the expected device/inode and
   compute `RuntimePaths::new(state, socket)`'s session paths/key.
2. Hold the session's `daemon.lock` with an independent flock. Bind its
   `control.sock`. Return a valid `Health`: plugin/socket/key/endpoint match,
   nonce is 64 hexadecimal bytes, and PID is the fixture peer's actual PID.
3. Hold `lifecycle.lock` before invoking public `daemon::stop`; release it 4.8 s
   after invocation. The peer remains alive for the duration of the call.
4. After receiving `health`, delay its response by 0.4 s. After receiving the
   valid stop JSON, delay the `stopping` response by another 0.4 s.
5. Observe that the stop JSON arrives after the original five-second deadline.

Observed on this Linux run:

| Observation | Seconds from stop invocation |
| --- | ---: |
| Stop request received by the synthetic peer | 5.228 |
| Stop returned an error | 5.630 |

Returned diagnostic: `daemon did not stop within five seconds; no signal sent`.
No process signal was sent. The observation is one synthetic reproduction,
not a latency distribution or a strict scheduling bound.

**Impact.** A valid concurrent lifecycle operation can consume most of the stop
budget; stop then starts additional I/O and can request shutdown after its own
deadline. The five-second shutdown contract in
`openspec/specs/daemon-lifecycle/spec.md:65–73` is not enforced across the whole
handshake. Process identity validation remains intact in this reproduction.

**Improvement.** Thread the original absolute deadline through control connect,
health request/response, stop request/ACK, and process-exit observation. Reuse
`connect_stream_before`, `write_frame_before`, and `read_frame_before`, with
existing local caps. Check expiration before beginning the stop request. Keep
stop free of process signals.

**Test gap.** `stop_deadline_does_not_signal_a_process_still_tearing_down`
(`src/daemon.rs:2146`) checks delayed teardown with an otherwise quick handshake.
It does not combine lifecycle-lock contention with a slow valid control reply.
Add that composition and assert that no shutdown request begins after the shared
deadline. A clock/handshake seam can avoid relying on a fragile sleep boundary.

## Read coverage and limits

Production entry points and boundary logic were inspected across every source
module, with deeper reads in the association, persistence, transport and lifecycle
paths. This is not a line-by-line certification of every test or every possible
execution path.

| Area | Files inspected |
| --- | --- |
| CLI, config and explicit user mutations | `main.rs`, `lib.rs`, `config.rs`, `configure.rs`, `font.rs` |
| Session ownership, daemon control and diagnostics | `lock.rs`, `daemon.rs`, `doctor.rs`, `quota_diagnostic.rs` |
| Snapshot, publication, sequence reservation and display | `protocol.rs`, `publish.rs`, `tokens.rs` |
| Local reader association, caches and budgets | `model.rs`, `reader_cache.rs`, `reader_claude.rs`, `reader_agy.rs`, `reader_codex.rs`, `reader_grok.rs`, `reader_kimi.rs`, `reader_hermes.rs` |
| Account clients and local Git headings | `quota_codex.rs`, `quota_grok.rs`, `quota_kimi.rs`, `ship_time.rs` |

The production sections were read; tests were sampled around the relevant
association, timeout, stop and mutation boundaries and run as the existing suite.
`src/test_support.rs` was not reviewed in detail. Integration-test internals,
release workflows, dependency internals, and upstream Herdr implementation were
not audited. README, SECURITY, the manifest, and local lifecycle/provider
specifications were consulted. Existing Agy generation limitations, font
check/rename/unlink races, unkillable OS-stalled readers and conditional child
timeouts are already tracked; they are not presented here as new findings.
No claim about macOS execution, power-loss durability or production frequency
is made. The doctor old-success experiment did not establish a new contract
violation and is excluded from the findings.

## Verification and retained evidence

The fixed source passed all required checks on this Linux host:

- `cargo fmt --all --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked` (446 unit tests passed, 6 ignored; 1 integration test passed)
- `cargo build --release --locked`
- `target/release/herdr-lineup --help`

The repro driver linked the unchanged crate's debug rlib through a small Rust
probe calling only public APIs. Python's standard library created the synthetic
files, locks and Unix listeners. No tracked Rust source, TODO, configuration or
manifest was edited. No real Herdr socket was contacted, no client was attached,
and no user server or daemon was stopped/restarted. The only stop request was to
the synthetic control peer created by the fixture.

Local evidence, relative to this dedicated worktree:
`target/review-evidence/{probe.rs,probe,reproduce.py,reproductions.json,checks.json,check-1.log,...,check-5.log}`.
These are untracked build artifacts, retained for commander inspection, not
portable evidence committed to the repository. The reproduction procedures above
and the fixed source support review without access to those artifacts.

| Local artifact | SHA-256 |
| --- | --- |
| `target/review-evidence/probe.rs` | `13c09486175154a1c09daf90a11203fc113e7aad706a74d8ac062a3c30be60dc` |
| `target/review-evidence/reproduce.py` | `8fcc08fe5e46556f9b412369b77b9cd108990206981537e3bcfbd9475777d868` |
| `target/review-evidence/reproductions.json` | `03556ee7e703714dc71d2152c5686152bc3d25271b69b0b8349373ba1cfa59e4` |

The temporary fixture root `/tmp/herdr-lineup-review-12dqnggj` was created and
recorded by this run and removed by its owning TemporaryDirectory after all
fixture threads joined and sockets/files closed. No fixture subprocess remains.
Independent review of this record is a separate evidence step; its result and
fixed candidate SHA are reported through Formation, not inferred from these
checks. Source findings remain unresolved until separately assigned fixes land.
