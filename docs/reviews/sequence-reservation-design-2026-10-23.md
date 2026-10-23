# Sequence reservation design record (2026-10-23)

**Decision for this record:** an endpoint-wide reservation window is implemented
on an unshipped candidate. It separates the durable upper bound `U` from the
in-memory cursor `C` and amortizes the file and directory syncs across the
reports in one window. Only the failure-table rows listed under "Verified
failure rows" below have test evidence; the others remain design intent. No
live Herdr session, user configuration, or production checkpoint was used for
this record.

## Fixed sources and current behavior

This audit uses commit `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`, notably
`src/tokens.rs`, `src/publish.rs`, `src/protocol.rs`, `src/socket.rs`,
`src/daemon.rs`, `openspec/specs/sidebar-tokens/spec.md`,
`openspec/specs/daemon-lifecycle/spec.md`, `README.md`, and
`herdr-plugin.toml`. The pinned Herdr v0.9.3 socket API is
<https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/socket-api.mdx#agent-state-reporting>.
The earlier survey calls checkpoint rewrite PERF-01 and explicitly says its
cost was measured on tmpfs only (`docs/reviews/2026-10-18-survey.md`, item 17).

The normative plugin contract is `openspec/specs/sidebar-tokens/spec.md`: each
live-key or clearing report uses source `herdr-lineup`, `ttl_ms = 3 * refresh_ms`,
and a strictly increasing per-resource `seq` persisted before sending. The
server contract says a sequenced report with `seq` less than or equal to the
last accepted value for the same source is accepted by the API but ignored by
the resource. Thus an `ok` envelope alone does not prove token application.
Socket metadata is not restored after a Herdr server restart.

Current flow (verified from the fixed code):

1. The daemon holds an exclusive session lock keyed by socket path. It loads
   `HERDR_PLUGIN_STATE_DIR`'s per-session `sequences.json` only after taking
   that lock. A snapshot binds path, device, and inode as the endpoint identity.
2. `refresh_with_readers` validates config, takes an authoritative snapshot,
   prunes absent resource rows while retaining their maximum as an endpoint
   floor, then builds desired pane and workspace tokens. `clear` takes a fresh
   snapshot and uses the same `report` path for null-valued patches.
3. `tokens::Publisher::prepare` filters the patch and, only when nonempty,
   allocates `max(resource.high_water, endpoint_floor) + 1`. It reserves that
   report in memory. Capacity skips and absent resources allocate no number.
4. `MetadataPublisher::report` rewrites the complete checkpoint through a
   temporary file, flushes and syncs that file, renames it, and syncs the state
   directory **before** a metadata frame can be sent. Failure prevents send.
   The JSON request contains source, target ID, `seq`, TTL, and token patch.
   A successful matching `ok` invokes `acknowledge`; no second persist occurs.
   A failed or ambiguous send leaves the sequence burned and cached acceptance
   unchanged. The refresh loop stops further reports on that error.
5. On restart, the checkpoint restores only high-water rows and floors, not
   accepted token values or renewal times. The next authoritative snapshot
   causes republishing at a higher sequence. On a changed socket endpoint, the
   old daemon refuses to reconnect; the new daemon removes old endpoint rows
   after verifying its own endpoint.

`src/protocol.rs::metadata_request` is the pure wire encoder and
`response_result` checks the response ID and envelope. `src/socket.rs` pins the
socket endpoint and provides request-scoped framing. Neither owns sequence
durability. The production seam for an experiment is the
`MetadataPublisher::report` / `tokens::Publisher::prepare` boundary, with
`persist` as the durability barrier. Inputs are endpoint identity, resource
kind/ID, snapshot key set, occupant, desired tokens, clock, and refresh
interval. Outputs are zero or one patch report, checkpoint rows/floor, a
socket result, and an in-memory acceptance update only after `ok`.

## Candidate: one endpoint-wide reservation window

> **Historical (2026-10-23):** this section and "Verification and decision
> gate" record the proposal as written before implementation. The current
> implementation and its evidence are in "Implementation status (2026-10-24)".

**Inference, not implemented at the time:** use the already supported `kind: "endpoint"`
checkpoint row as a durable upper bound `U` for all resources on one pinned
endpoint. Choose a small fixed block size `B` for an experiment; do not add a
user setting. On loading a checkpoint, calculate `base = max(endpoint floor,
all resource rows for this endpoint)`. Discard any previously unused portion
of a window. Before the first send needing a new window, check `base + B` for
overflow, write `U = base + B` durably with the existing file-and-directory
sync protocol, then hand out `base + 1` through `U` sequentially in memory.
Every allocation, including clear and retry, consumes a fresh number. On
exhaustion, persist the next upper bound before handing out another number.
Never return a number to the pool after a send failure or unknown response.

The planner must distinguish **durable upper bound** from **next unsent
number**. Its current `prepare` reads the endpoint floor as an allocation
lower bound on every call, so merely prewriting a larger floor would skip the
entire window. A minimal candidate would add an endpoint-local cursor and a
checked sequence-supply method at the planner boundary, while leaving patch
filtering, `acknowledge`, protocol encoding, transport, and the checkpoint JSON
row shape intact. The cursor must start beyond every loaded resource row and
must not survive restart. The persisted upper bound must never decrease during
resource pruning or endpoint reconciliation. If reservation persistence fails,
the publisher must not use the tentative window or send a report; it should
fail closed until checkpoint state is safely reloaded or the daemon restarts.

This global sequence is stronger than the plugin's per-resource order: for any
one resource, later reports get larger numbers. It also permits one sync per
`B` reports after the first reservation. Reserving `B` numbers separately for
each resource would still cause one sync per resource during the first cycle
and is not the proposed latency experiment. A window is **not** a transaction
across reports: Herdr may apply an arbitrary prefix or subset after failures.
Recovery must re-read the snapshot and recompute desired values; it must never
replay an old patch merely because its number was reserved.

### State and failure table

| Point or event | Durable checkpoint / in-memory state | Required action and invariant |
| --- | --- | --- |
| Crash before the window's rename and directory sync | Old upper bound is the last proven durable bound; no new frame may have started | Restart from the readable checkpoint. A failed or malformed read stops publication. |
| Crash after rename but before directory sync | The new file may or may not survive; no new frame may have started | Either readable old or new upper bound is safe because no number from the new window was sent. Treat an unreadable checkpoint as fatal. |
| Crash after durable reservation, before first send | `U` is durable; all window slots may be unused | Restart above `U`; gaps are allowed. Never reclaim a slot. |
| Crash between two sends, or after a successful send before `acknowledge` | `U` is durable; server may have accepted any sent prefix | Restart above `U`, discard cached acceptance, snapshot and republish current desired tokens. |
| Send fails before completion | The attempted slot was consumed; server effect may be unknown | Stop the current refresh/clear path, reconnect to the pinned endpoint, obtain a new snapshot, and use a new slot. |
| Response is lost, malformed, or an error | Acceptance is unknown even if the frame was written | Do not acknowledge or reuse the slot. Retry from current authoritative state with a higher number. |
| Process restarts while the window has unused slots | Disk has `U`, memory cursor is lost | Start the next reservation strictly above `U`; wasted numbers are intentional. |
| Endpoint path/device/inode changes | Old endpoint rows and upper bound belong to the old server | Old daemon stops. New daemon verifies its endpoint, then may discard old rows/floor and start a separate sequence history. Do not transfer an unsent window to another endpoint. |
| Resource disappears and its ID later reappears on the same endpoint | Durable endpoint upper bound covers even deleted rows | Pruning must retain at least `U`; reappearing ID receives a number above `U` after restart, or above the cursor during the same run. |
| Concurrent ensure, refresh, or shutdown clear | Current daemon/session lock permits one publisher; refresh and clear run on that mutable publisher | Keep allocation and send serialized through that owner. Clear consumes slots and observes its existing three-second deadline after reservation. Reject a second allocator for the same state directory. |
| Config changes during or between refreshes | Sequence state is independent of desired tokens and TTL | Complete or stop the in-flight report under existing rules; recompute values from validated config on the next snapshot. Disabling a provider must still clear its keys with a new number. |
| Window near `u64::MAX` or checkpoint persistence error | Next upper bound cannot be proved durable | Fail closed before any send; do not wrap, reduce the floor, or silently fall back to an older number. |
| Rollback to the current binary | Current code can read the existing endpoint row and allocates above its `seq` | This is sequence-safe **if** the current binary remains the rollback target and owns the session lock. An older binary that rejects endpoint rows can fail to publish; preserve the checkpoint and return to a compatible binary. |

The table proves only the intended ordering **conditional** on a correct
durability barrier, single allocator, correctly pinned endpoint, and Herdr's
documented per-source `seq` rule. It does not prove those conditions hold under
every crash or filesystem. In particular, an `ok` for a stale number may be a
no-op on the server; the plugin's source must remain exclusive to this
publisher for its resource. A second writer using `herdr-lineup` is outside
the current single-daemon guarantee and needs an explicit conflict test or
separate ownership rule before enabling the optimization.

## Verification and decision gate

> **Historical (2026-10-23):** the gate below, including "keep the current
> per-report persist", described the code before the window was implemented.

The implementation owner would be the publisher/planner module owner. The
integration-test owner would be the same change lane, using the existing
synthetic Unix socket harness in `src/publish.rs` and pure planner tests in
`src/tokens.rs`; independent review must cover the complete candidate diff.
The tests must model Herdr's `seq <= last accepted` no-op, not merely return
`{"type":"ok"}` for every report. Inject termination/failure at each row
above, including checkpoint write, file sync, rename, directory sync, frame
write, server apply, and response read. Restart a fresh publisher from the
resulting checkpoint and assert strictly increasing per-resource applied
sequences and that the latest desired patch wins. Include resource reuse,
endpoint replacement, capacity skips, same-source competing writer, config
change, simultaneous ensure attempts, and shutdown clear/deadline cases.

Compare the unchanged baseline and a candidate using identical synthetic
snapshot/token workloads, pane and workspace counts, refresh intervals,
resource churn, response delays, and storage type. Record exact checkpoint
writes, file/directory sync counts, checkpoint bytes, cycle and per-report
p50/p95, missed TTL renewals, and skipped sequence count. Run on Linux disk
and macOS disk as well as tmpfs; report each filesystem and sample size.
`cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`, `cargo build --release --locked`, and release binary
`--help` remain required. No server or user session is needed for these tests.

Only enable the new path after those tests, disk measurements, and independent
review establish a material benefit without a sequence regression. Until
then, keep the current per-report persist and its explicit pre-send durability
guarantee. Reverting an experimental code change should retain the checkpoint
file and use the current compatible binary; deleting the file or lowering its
high-water mark is not a safe rollback.

## Implementation status (2026-10-24)

The endpoint-wide reservation window is implemented in `src/tokens.rs` and
`src/publish.rs`. It is not shipped.

1. **State separation**: `endpoint_floors` holds the durable upper bound `U`
   per endpoint and `endpoint_cursors` holds the in-memory cursor `C`. Loading
   a checkpoint sets `C = U = max(endpoint row, resource rows)`, which discards
   any unused window.
2. **Window sizing**: `compute_reservation_window(panes, workspaces)` sizes the
   window from the authoritative snapshot counts, clamped to `32..=4096`. No
   user setting is added.
3. **Barrier**: the reservation is written lazily, before the first send that
   needs a sequence above `U`. The file is flushed and synced, renamed, and the
   directory is synced; only then is `U` raised in memory.
4. **Uncertain persistence is fatal**: any failed checkpoint write (file sync,
   rename, or directory sync, for reservation or pruning) poisons the
   publisher. After a rename has succeeded, the directory sync can still fail,
   leaving a higher `U` on disk than in memory. A poisoned publisher therefore
   refuses every later reservation, checkpoint write, snapshot, report,
   refresh, and clear, and `reconnected` keeps the poison. Publication resumes
   only in a new instance that reloads the checkpoint (daemon restart). There
   is no in-process reload path.
5. **Empty patches and capacity skips**: `Publisher::needs_report` filters them
   before reservation, so they allocate no sequence and write no checkpoint.
6. **Ambiguous sends**: a number is consumed by `prepare` before the send and is
   never returned, including after an error response.

### Verified failure rows

The synthetic socket fixture `sequence_server` models Herdr's per-resource,
per-source rule: a report whose `seq` is not newer than the last accepted one
is answered `ok` but not applied, and the accepted `seq` outlives the resource
leaving the snapshot. Failures are injected as returned errors inside the
process; a real power loss or kernel crash was not simulated.

| Failure-table row | Evidence |
| --- | --- |
| Failure before the window's rename (file sync, rename) | `uncertain_checkpoint_write_poisons_publication_until_new_instance`: disk keeps the old bound, nothing is sent, the poison survives retry, `reconnected`, refresh, and clear; a new instance resumes and the report applies. |
| Rename done, directory sync fails | Same test: disk keeps the attempted `U = 4096`; a retry with a 32-slot window is refused instead of writing a lower bound; a new instance sends `4097`. |
| Durable reservation, then restart with unused slots | `stale_sequence_server_applies_reports_across_window_restart_and_reuse`: the first seq after restart is above the old `U`, and the server applies it. |
| Window boundary in one run | Same test: 40 reports cross the 32-slot boundary with exactly two checkpoint writes; every report applies with a strictly increasing seq. |
| Resource disappears and the ID reappears in the same run | Same test: after pruning, the reused ID gets a seq above the cursor and applies; the final snapshot holds the latest patch. |
| Server applies, then the response is lost | `response_lost_after_apply_is_never_replayed`: the unacknowledged seq is not resent; after `reconnected` and after a restart, the current desired value goes out above the old window and wins. |
| Process exits with unused slots | `process_exit_after_reservation_discards_unused_window`: a self-spawned test child reserves a 32-slot window, sends seq 1, and calls `process::exit` without `Drop`; the parent's new instance sends seq 33. |
| Shutdown-clear deadline after reservation | `clear_deadline_after_reservation_burns_the_unsent_seq`: `report` with an expired deadline reserves and allocates seq 1, then sends nothing; the next report uses seq 2 and a restart uses seq 33. |
| Pruning write fails | `failed_pruning_persist_poisons_refresh_clear_and_reconnect`: the poison survives refresh, `reconnected`, and clear; only a new instance sends again, above the old bound. |
| Lost checkpoint (negative control) | `stale_sequence_server_applies_reports_across_window_restart_and_reuse`: a publisher without the checkpoint sends `seq = 1`, which is answered `ok` but not applied. |
| Error response | `rejected_report_burns_sequence_and_clear_preserves_other_keys`: the next report uses a fresh number. |
| Endpoint replaced | `checkpoint_pruning_persists_floor_and_drops_replaced_endpoint`. |
| Overflow | `checked_overflow_fails_closed_in_reservation_and_allocation`, `reservation_checked_overflow_fails_closed_before_send`. |
| Legacy checkpoint without endpoint rows | `legacy_checkpoint_without_endpoint_rows_migrates_safely`, `legacy_checkpoint_migration_in_metadata_publisher`. |
| Empty patch and capacity skip | `empty_patch_and_capacity_skip_preserve_reservation_cursor`. |

The process-exit row uses a normal `exit`, not `SIGKILL` or power loss. The
deadline row passes an already expired deadline instead of racing a timer.

Not verified: kernel crash or power loss, concurrent ensure or a second
allocator, a same-source competing writer, config change during refresh, and
rollback to an older binary. macOS disks were not available and are
unmeasured; the Linux results are not extrapolated to them.

### Measurement

`refresh-stage-latency-2026-10-23.md` records the window run. The deterministic
result is the checkpoint write count of a cold cycle: `panes + 1` before, one
after. The elapsed times were not taken under the same conditions as the
historical baseline and do not establish a speed-up.

### Out of scope (C2)

Per the owner ruling
(`note_all_e9d850faa907e0c7fb2d987a73fb3fde95a83c84fb8b296092dc6b254fef4654`),
Herdr host generation, conditional apply, and retirement on replacement (C2)
belong to a separate Herdr task. The existing behavior (endpoint pinned by
path, device, and inode, without generations) is kept, and C2 remains an
unresolved TODO.
