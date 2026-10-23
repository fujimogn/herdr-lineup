# Reader diagnostics control and doctor design

**Status:** D implementation candidate; independent review and shipment pending.
**Implementation baseline:** `bfe2a4d3a7a4369711b49057016ec233273f9484`.
P was reviewed and integrated locally before the commander authorized D.
This candidate does not imply integration, shipment or live measurement.
This document supplements [the P plan](publisher-runtime-wiring.md); current
assignment decisions supersede its older allocation of Arc ownership to D.

## 1. Purpose and existing contracts

Expose memory-only reader worker diagnostics through existing session-local
control and read-only doctor, without running a reader or account query.
The baseline implements `ReadCycle::with_diagnostics`, `diagnostics`,
`ReaderDiagnostics::{snapshot,snapshots}`, `ReaderSnapshot`, and
`DiagnosticUnavailable::{Contended,Poisoned,Incomplete}` in `src/model.rs`.
These model types remain crate-private. The fixed source order is Claude,
Agy, Codex, Grok, Hermes, Kimi; it is independent of configured providers.
D connects publisher/control/doctor consumption. Claude/Agy/Grok observed routing
is a separate concurrent task; Codex/Kimi routing is in the baseline. Hermes
has no bounded worker: its empty slot is not a successful read measurement.
P owns one publisher-lifetime `Arc<ReaderDiagnostics>` and passes a clone to
each fresh `ReadCycle::with_diagnostics`. D reuses that Arc; it must not create
a second registry or reset counters per refresh/reconnect.

## 2. Sampling choice (commander approved)

| Approach | Benefit | Limitation |
| --- | --- | --- |
| Adopted: live best-effort sampling on each control health request | A worker released after a caller timeout becomes inactive without waiting for another refresh | Reader samples and completed-attempt metrics have different observation times; provider samples are not transactional |
| Alternative: freeze reader samples when each refresh finishes | Aligns capture with completed-attempt health | Worker active can remain true after release until another refresh; requires explicit frozen-attempt labeling and monotonic sample age, never a claim about current activity |

Commander ruling `note-reader-control-design-go-rulings-01` adopts live sampling,
the opaque handle seam and the fallback ladder below. Sample once per response,
after receiving valid `health` and before serialization. `sampling=live_best_effort`
means sequential per-provider samples during this request, not an atomic view
at accept time or at doctor print time. No wall-clock timestamp is required.
Health, total duration, stage histograms and P deadline skips retain their
existing coherent last-completed-attempt snapshot; they are not refreshed by
this request. Doctor prints this distinction even during a running refresh.
Each provider result is advisory: counters can miss updates and saturate;
worker bits and counters are not a jointly atomic accounting ledger.

## 3. Ownership and public API seam

Add a public opaque `daemon::ReaderDiagnosticsHandle` with private fields,
`Clone`, and a crate-only constructor accepting `Arc<ReaderDiagnostics>`.
Its Debug implementation, if needed by ControlState, exposes only a fixed
opaque label, never model internals or sampled data.
Add `Publisher::reader_diagnostics(&self) -> Option<ReaderDiagnosticsHandle>`
with default `None`. Neither this public signature nor public constructors
mention model types. The handle's sampling method and wire conversion remain
crate-only; do not make `model` or `ReaderSnapshot` public to silence warnings.
`MetadataPublisher` returns a handle cloning its P-owned Arc. Other publishers
retain `None`, meaning unavailable, and existing implementers remain valid.
Initialize the handle in `ControlState` before `ControlWorker::start`, including
the local state copied back after refresh/reconnect, so it is never overwritten
with `None`. ControlState holds an optional handle, not a frozen reader sample.
The daemon control worker clones health, existing observations, P deadline
snapshot and handle under the existing state mutex, then releases it.
Call `snapshots()` only after releasing that mutex. Its source uses per-slot
`try_lock`, returning promptly without retry, sleeping, or joining workers.
Except for snapshot cloning, no control mutex is held during sampling, JSON
construction, serialization or socket I/O; provider locks end before serialization.
No new nested lock ordering.
Sampling errors affect only diagnostics and cannot request shutdown/reconnect.
The handle may outlive the publisher until control teardown; it owns no worker
join, permit, file or query. Preserve existing shutdown and teardown order.

## 4. Optional wire object and tolerant client decoding

Add top-level optional `reader_diagnostics`, sibling to `refresh_observation`
and P's approved `refresh_deadline_skips`. Keep `Health` unchanged.
Version 1 schema (all names here are new wire names):
`{version:1,sampling:"live_best_effort",providers:{claude:entry,...}}`.
Provider keys are exactly the six lowercase source labels, never arbitrary IDs.
An entry is `{status:"available",sample:{...}}` or
`{status:"contended"|"poisoned"|"incomplete"}`. The sample copies these source
fields with their original units; all integers are unsigned 64-bit:

| Fields | Meaning / doctor unit |
| --- | --- |
| `attempts`, `busy_rejections`, `deadline_ends`, `shutdown_ends` | Observed calls and caller outcomes; count |
| `spawn_failures`, `worker_failures`, `completed`, `invalid` | Fixed source outcome categories; count |
| `caller_wait_count`, `caller_wait_total_ns`, `caller_wait_max_ns` | Caller wait samples, total and maximum; count / ns / ns |
| `sampling_skips` | Source updates omitted because recording could not lock; count, not number of unavailable control requests |
| `worker_active`, `timed_out_worker_active`, `shutdown_worker_active` | Source worker bits observed now; boolean |

`Contended` and `Poisoned` do not return a partial/default sample. `Incomplete`
means the source cannot account for worker activity; never print inactive or
zero for it. Available with `sampling_skips > 0` stays available but is explicitly
best-effort; do not infer exact outcome partitions or compare counters as equal.
Do not infer active from attempts minus completions, or fabricate missing zeros.

Parse `Health` as today, then decode each optional extension independently from
the raw JSON value into crate-private typed DTOs. Never add the object inside
`RefreshObservation` (its strict schema would break version skew).
Missing/null object, unsupported version/sampling, wrong envelope shape or
unknown envelope fields => reader diagnostics unavailable, valid Health intact.
Missing provider or malformed/unknown entry => that provider unavailable;
known valid sibling providers survive. Ignore unknown provider keys without
printing them. Reject unknown sample fields, missing fields, negatives, floats,
overflow, string counters, invalid booleans, or inconsistent active subflags.
Do not enforce exact accounting equalities on best-effort/saturated counters.
An old client ignores the new top-level field as Health does today; a new
client reads old Health normally and prints reader diagnostics unavailable.
Malformed overall JSON or invalid Health/identity remains a real control error.

## 5. Frame budget and preservation of control identity

`daemon::read_control` and `serve_control` call `read_frame(...,8192)`.
`socket::read_frame_before` counts the newline in that limit; JSON must be at
most 8191 bytes. `write_frame` appends the newline. Do not raise this limit.
Fixed provider cardinality alone does not prove the combined frame fits:
stage histograms also contain many maximum-width counters and Health includes
a variable encoded socket path. Never truncate JSON or shorten identity fields.
A compact synthetic JSON calculation for the schema above, `/fixture.sock`,
64-byte synthetic key/nonce, maximum integer widths and false booleans is
6536 bytes including newline (reader object alone: 3524). This is a design
budget estimate, not a measurement of the future Rust serializer or a bound
for arbitrary paths; the exact production serializer fixture is mandatory.
Before writing, serialize and check exact bytes plus newline. Reuse the one
reader sample through all fallback attempts; never resample. If oversized,
omit the entire reader extension and retry; if still oversized, omit optional
`refresh_observation` and `refresh_deadline_skips` together and retry. Preserve
all Health fields, including capacity count. Missing diagnostics then decode
as unavailable. If even Health alone exceeds the bound, return the existing
control failure; never produce an oversized frame or claim healthy identity.
The implementation fixture measures six available providers with u64::MAX
for every counter, false booleans (longer JSON), maximum stage histograms,
deadline counters and an escaped long synthetic socket path. Assert fallback
keeps a valid Health and makes each removed extension unavailable; also cover
normal payloads, near-limit Health, exactly 8192 bytes including newline and
8193 bytes including newline (rejected), with a single sampling call per response.

Keep plugin, socket key/path, live SessionLock, nonce format and endpoint checks
in `read_control`; keep stop nonce/plugin/key/endpoint and peer PID verification
on their existing paths. `probe`, `probe_view`, `stop`, `ensure` keep their
existing identity decisions. Optional diagnostics never authorize any action.
No new display includes secrets, identities, paths, usage, nonce, error text,
session contents or unknown JSON labels. Existing identity wire data is retained.

## 6. Doctor output and refresh classification

Print a fixed header: `Reader diagnostics: live best-effort; sampled during
control request; refresh metrics describe completed attempts` (one line).
Print all six fixed provider labels with the available counters/booleans and
explicit ns/count units, or `unavailable` with a fixed reason category.
No registry => `Reader diagnostics: unavailable`; no fabricated provider zeros.
Build reader and refresh/deadline text before `refresh_diagnostic` returns an
unhealthy/first-refresh error, so available diagnostic text still prints and
the pre-existing nonzero status is preserved. Optional absence alone succeeds.
Consume P's `refresh_deadline_skips{attempt_count,last_deadline_skipped,last_capacity_skipped}` as
separate cumulative deadline-skip attempt count and deadline/capacity resource
counts from the latest deadline-skip attempt, not reader deadlines or cumulative
skipped resources. These resource counts remain across later successful, error
or capacity-only attempts; they are not counts of the latest refresh.
Missing/malformed means unavailable, including before the first deadline-skip
attempt. A measured 0 is 0.
Deadline skips belong in total attempt/duration/stage accounting; capacity and
deadline may coexist in one attempt, so category counts are not a partition.
Use P's integrated classification: real errors, poison and shutdown cannot be
relabeled as skips. Keep `Health.last_refresh_ms` startup-relative semantics.

## 7. Implementation fixtures, spec proposal and rollback

| Fixture owner | Required isolated regression |
| --- | --- |
| daemon | Default Publisher None; opaque seam compiles under clippy -D warnings without expanding model visibility; reconnect/refresh retains the same registry |
| daemon/model API consumer | Channel-held worker reaches caller deadline, control reports active; release worker, request again without refresh, active false; no timing-threshold assertion |
| daemon | Hold a provider lock: contended promptly, other providers survive; poison/incomplete map to unavailable and never false/0; control mutex available throughout sampling and blocked client I/O |
| daemon | All maximum counters round-trip without wrap; sampling_skips preserved; contradictory active subflags rejected; no exact sum validation at saturation |
| daemon | Old/new clients and producers; missing/null/unknown/malformed object, one broken provider, unknown provider, invalid Health; valid sibling metrics survive independently |
| daemon | Exact frame sizes and fallback ladder above; unchanged valid/invalid nonce, replaced endpoint, peer PID, live lock, stop/ensure/probe fixtures |
| doctor | Fixed labels/units only; None versus zero; live versus completed header; escaped arbitrary input never printed; metrics visible on first-refresh/error path with unchanged exit status |
| daemon/doctor | P mixed capacity+deadline, success, real error, poison, shutdown; total attempts/durations remain consistent and deadline counter saturation remains accepted |

D touches only `src/publish.rs`, `src/daemon.rs`, `src/doctor.rs` plus cfg(test) model helpers approved by touching amendment 382, this plan and
the approved daemon-lifecycle delta in `openspec/changes/wire-publisher-runtime/specs/daemon-lifecycle/spec.md`
for live sampling/release, optional-data version skew, frame fallback, fixed
redacted output and unhealthy-refresh diagnostics. Scope approval and strict
OpenSpec validation preceded source implementation.
No disk/config format changes. Rollback D's commit independently of P restores
unavailable reader control output; registry counters reset only on daemon lifetime
replacement. No restart is performed for verification. D fixtures use synthetic
sockets, channels and samples; real Herdr/session/config/API contact stays forbidden.

## 8. Candidate verification mapping

Normal daemon tests named `reader_control_*`, `reader_wire_*`, `reader_frame_*`,
`reader_maximum_frame_*`, `reader_sampling_*` and `reader_handle_survives_*`
exercise live release, source lock/error states, decoding, saturated counters,
exact serializer fallback and the daemon completed-state publication seam.
Doctor's `reader_*` tests cover unavailable versus measured values, fixed labels,
units, retained deadline-skip meaning, nontransactional observation times and
existing unhealthy/startup errors. Existing identity and refresh-classification
fixtures remain in the normal suite. No new ignored tests or live verification.
