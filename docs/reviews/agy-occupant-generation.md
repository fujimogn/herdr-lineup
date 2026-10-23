# Antigravity occupant generation contract

Status: research and Planned design; C2 remains unresolved in production.
Baseline: `894d2f25f3bc44e59a31ea48a6995c7cee1e6455`. Checked on 2026-10-24.
This change documents the boundary; it does not change readers, capture, binding,
publication, configuration, or the Herdr host.

## Primary evidence and limits

The starting official URL is the pinned wire-contract link in
`src/protocol.rs`. All upstream sources below are public files at Herdr
`v0.9.3`, the minimum in `herdr-plugin.toml`; later hosts are not assessed.
The GitHub schema-page fetch failed, so the same tagged files were read from
`raw.githubusercontent.com/herdrdev/herdr/v0.9.3/`. No installed schema command,
live socket, session, authentication, configuration, or credential API was used.

| Source | What it establishes | What it does not establish |
| --- | --- | --- |
| [Socket API](https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/socket-api.mdx), process environment section | Managed pane processes receive `HERDR_SOCKET_PATH`, `HERDR_ENV`, `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_PANE_ID`; host values override caller values | No occupant-generation or native-session environment variable is documented in that list |
| [Pane schema](https://github.com/herdrdev/herdr/blob/v0.9.3/src/api/schema/panes.rs), `PaneInfo`, lines 449–484 | `pane_id`, `terminal_id`, optional `agent`, optional `agent_session`, `revision`, cwd and presentation/state fields are exposed | No occupant-generation field exists in this record; `revision` has no occupant-generation contract here |
| [Agent schema](https://github.com/herdrdev/herdr/blob/v0.9.3/src/api/schema/agents.rs), `AgentInfo` and `AgentSessionInfo`, lines 187–237 | Agent records expose terminal/pane IDs, optional native reference, state counters and revision. Native reference contains `source`, `agent`, `kind`, `value` | Neither record supplies a per-launch generation shared with a statusLine invocation; state/completion counters are not such a contract |
| [Session schema](https://github.com/herdrdev/herdr/blob/v0.9.3/src/api/schema/session.rs), `SessionSnapshot`, lines 9–23; [response schema](https://github.com/herdrdev/herdr/blob/v0.9.3/src/api/schema/response.rs) | A snapshot contains pane and agent records, version/protocol and layout/focus data | No occupant-generation field exists in `SessionSnapshot`; version/protocol identify protocol compatibility, not an agent launch |
| Socket API, native session reporting section; pane schema `PaneReportAgentParams` / `PaneReportAgentSessionParams`, lines 362–398 | Integrations can report native ID or path; exposed native references are optional and omitted when absent | No guarantee that Agy always reports an ID, that it changes per launch, or that statusLine stdin is atomically associated with that report |
| Socket API, event subscriptions section | Events invalidate caches; after loss/reconnect reconcile with authoritative reads. Snapshot and events have no shared sequence boundary | Replaying events cannot establish a complete occupant history or prove an observation's capture generation |
| Socket API, `agent.wait` / `agent.prompt` section | Server-owned waits pin the resolved occupant; prompt plus wait can be submitted together | That internal pin is not exposed as an observation token or a conditional metadata-report parameter |

**Confirmed absence** here is limited to the listed public record shapes and
documented environment list. It is not a claim that Herdr has no internal
occupant tracking. The wait documentation explicitly describes internal pinning.
**Unconfirmed:** terminal ID uniqueness/lifetime over same-terminal agent
replacement, reconnect and cold restore; Agy native ID availability and lifecycle;
any upstream API outside these records that could export a generation to hooks.
Those properties must not be inferred from field names. No new environment name
or wire field is assigned by this design. The protocol's endpoint generation
discussion concerns client/server compatibility, not an occupant generation.

## Implemented data path and failure

1. `src/statusline.rs::run` takes `HERDR_PLUGIN_STATE_DIR` and `HERDR_PANE_ID`.
   `capture(Agent::Agy, ...)` selects the inherited pane ID, ignoring conversation
   IDs in stdin. It atomically replaces a version-1 record named by encoded pane
   key under `statusline/`. The record stores agent, key, observation time,
   context, weekly remaining and reset, but no native session or generation.
2. `src/protocol.rs::parse_snapshot` reads pane ID, provider, optional native ID
   only when `agent_session.agent` matches and `kind == "id"`, cwd and state.
   It currently discards upstream terminal ID and revision. Path references
   do not become native IDs. `SessionIdentity` in `src/model.rs` contains
   provider, native session ID and repository identity, not a launch generation.
3. `src/reader_agy.rs::AgyReader` uses its pane ID to load that record.
   `statusline::read_record` verifies shape, provider/key, bounds, timestamp and
   file constraints, and returns `UsageBinding::Pane`. The request identity
   attached to the reading is the current request's identity, not capture-time
   proof. `src/tokens.rs::pane_tokens` checks pane binding and occupant
   equality; these checks cannot reconstruct absent capture-time identity.
4. Context remains the last observation without age expiry. Weekly with a
   reset expires at reset; without one it expires by `usage_max_age_ms`.
   Publisher timeout reuse still checks its cached occupant/identity/source;
   it cannot make a pane-keyed disk record generation-bound.

Consequently Agy A can write context, leave, and Agy B can reuse the same pane
without a new statusLine observation. B can read A's record even with a changed
native session, since the disk record does not carry A's native identity.
An Agy → other provider → Agy transition between snapshots, daemon restart,
or old hook completing after replacement can also evade local transition
tracking. This is a code-derived counterexample, not a live reproduction.

## Planned host prerequisite and production seam

The safe contract needs host-owned identity covering endpoint incarnation,
pane/terminal incarnation and **each occupant launch/replacement**, including
same-provider replacement and resume of the same conversation. A native session
ID describes a conversation and may be resumed; terminal ID describes a terminal.
Neither is a proven substitute for launch identity in the inspected contract.

The host must define the generation's source of truth, update it atomically with
occupant installation/removal, forbid ABA reuse across endpoint restarts, and
deliver the captured generation to the actual launched hook lineage. A hook
must not look up the current generation by pane ID after receiving old stdin:
that would relabel A's observation as B's. Delayed hooks must retain their
original generation. Hook inheritance and resume semantics need upstream tests.

A future integration would carry that host identity through
`protocol::Pane` → publisher request → model binding, and separately through
capture → a new observation record version → reader. Equality must include
endpoint, pane/terminal, provider and launch identity; any absent or mismatched
part makes usage unavailable. Source is the host lifecycle, updated on every
replacement, not a plugin clock, PID, cwd, label, random local refresh ID,
file mtime, or an inferred environment variable. Native session may remain an
additional check when available, never the sole generation.

Even equal pre/post snapshots cannot prove no A → B → A transition. A second
snapshot narrows a race but does not close it. Strict protection at publication
also needs the host to condition metadata acceptance on the same occupant
identity atomically with replacement. The inspected metadata contract provides
per-source sequence/TTL, not that precondition. Host-owned conditional consume
or equivalent atomic association is therefore a separate requirement. A single
snapshot cannot ensure values are current when a later report is applied.
Same-user processes can forge local records or inherited values; this proposal
addresses lifecycle races, not isolation from a hostile process with the same
filesystem and socket authority. Stronger authentication is an upstream concern.

The production integration owner must cover protocol/model, capture/reader,
publisher cached fallback and conditional send as one reviewed contract. This
assignment owns only this record/spec clarification. No publisher wiring is
changed and no implementation is approved by the record.

## Planned migration, rollback and alternatives

Use a separately versioned generation-bearing observation namespace after the
host contract and capability negotiation are defined. Preserve version-1 files
byte-for-byte; do not annotate them with the current snapshot's generation,
delete them, or promote them to trusted records. Old binaries reject unknown
record fields, so overwriting their files would also break rollback. In an
explicitly approved strict mode, legacy-only or unknown-generation sources
yield unavailable usage until a new valid hook observation arrives; keep
provider/state rows and existing observations intact. Do not silently fall back
to pane-only records on the strict path.

Rollout requires a host capability gate, isolated fixture GO, independent review,
and an explicit decision on the temporary unavailable values for existing users.
Keep the current path until that decision; its C2 limitation remains documented.
Rollback uses the prior compatible binary/path with preserved v1 files and
separate new records. It restores the known pane-only limitation and must not
be described as generation-safe. Do not roll back by deleting observations or
lowering publisher sequences. No configuration is changed automatically.

| Alternative | Benefit | Remaining limitation / decision |
| --- | --- | --- |
| Keep current pane binding with explicit limitation | Preserves existing observations and behavior | C2 remains open; this is this documentation change's actual effect |
| Disable Agy usage pending host support | Prevents attribution of old usage if approved | Loses current context/quota visibility; not implemented or selected here |
| TTL, local transition cache, terminal/native comparison | Can reduce exposure in detected cases | Same-pane unseen replacement, delayed hook and restart remain; not sufficient for strict proof |
| Host launch identity plus atomic conditional consume | Covers capture and publication generation | Requires upstream design/implementation and migration approval; Planned only |

## Planned adversarial fixtures and acceptance

These are required future tests, **not tests implemented or run by this record**.
Use synthetic snapshots, hook stdin and temporary state with the existing unit
and isolated Unix socket harnesses. Never attach to a user's server/session.
The future integration owner owns end-to-end fixtures and rollback compatibility;
the host owner must supply generation lifecycle and conditional-apply fixtures.

| Fixture | Expected strict-path result |
| --- | --- |
| Same pane/provider/cwd, new occupant, no fresh hook | Reject old context and both weekly forms |
| Native ID changes, or same conversation is resumed under a new launch | Reject old launch record in both cases |
| A → other → A entirely between snapshots; terminal ID unchanged | Generation differs; reject even if all legacy identity fields match |
| A's hook finishes after B's hook | A cannot overwrite or satisfy B's generation record |
| Replacement between snapshot/read/send/server apply | Conditional host consume rejects old-generation report; no false acknowledgement of applied usage |
| Endpoint restart or pane/terminal ID reuse; daemon restart with v1 records | No ABA reuse or auto-promotion; legacy bytes preserved |
| Missing/malformed/unknown generation or unsupported host | Unavailable strict usage; provider/state remain independent |
| Event loss/reorder/reconnect during replacement | Reconcile authoritative state; never treat event history as generation proof |
| Reader timeout followed by occupant replacement | Cached fallback cannot cross generation; weekly expiry still applies |
| Valid idle generation, expired weekly, duplicate panes, forged fields | Retain same-generation context; expire weekly; reject mismatched bindings; do not claim same-user authentication |
| Upgrade then rollback; `--then` command chaining | v1 bytes and prior command output/exit remain compatible; no automatic settings edits |

Completion of this research record does not close C2. Production acceptance
requires the host guarantees, a bounded implementation through the stated seam,
the above fixture evidence, required project checks and independent review.
