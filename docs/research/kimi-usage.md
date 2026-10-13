# Kimi usage source follow-up (2026-10-13)

## Decision

**Lineup reads Kimi context from a matching local session's completed turn. Weekly remaining is unavailable because it requires account usage, which the Kimi reader does not query.** The installed bundle persists `token_counting.turn_recorded.tokens`, computed by the same `statusSize` method as live status; neither `token_counting.measured.tokens` alone nor `usage.record` is a current context percentage. The sampled files contain no `agent.status.updated`, `context_tokens`, `max_context_tokens`, `context_usage`, or account quota snapshot. A verified idle Kimi pane displayed 11% after the reader landed; this runtime observation was reported by the integration owner, not reproduced in this research lane.

## Local evidence

`kimi --version` still reports **2.1.1**, the version recorded in [usage-sources.md](usage-sources.md). The installed bundle's SHA-256 also still matches the value there. The package is `~/.bun/install/global/node_modules/@moonshot-ai/kimi-code`; its `dist/main.mjs` contains the server and SDK implementation. The package has no separately installed OpenAPI or AsyncAPI file: this build registers `/openapi.json` through `app.swagger()` and creates `/asyncapi.json` at server runtime. Neither endpoint was called.

Under the default `~/.kimi-code/sessions/`, there were 715 `state.json` files and 530 `agents/*/wire.jsonl` files at inspection time. A top-level key inventory of `state.json` found session metadata including `id`, `cwd`, `agents`, `createdAt`, and `updatedAt`, but no context or quota field. Across the wire files, event-type counts included `token_counting.measured` (9,753), `token_counting.turn_recorded` (834), `token_counting.rebased` (10), `context.apply_compaction` (10), `usage.record` (9,910), and **zero** `agent.status.updated`. The measured event has numeric `length` and `tokens` (observed `tokens` range 3,973–239,780), plus `agentId` and `time`. `usage.record` has `agentId`, `model`, `usage`, `usageScope`, and `time`; `usage` contains `inputCacheCreation`, `inputCacheRead`, `inputOther`, and `output` keys. No event directly stores weekly remaining.

The installed bundle marks `TokenCountingMeasured` as durable, while `AgentStatusUpdated` is observable; the latter is passed through `withStatusSnapshot` to the SDK event receiver. This is consistent with the file inventory. [Kimi's session guide](https://www.kimi.com/code/docs/en/kimi-code-cli/guides/sessions.html) documents `state.json` as metadata and `wire.jsonl` as a recovery stream that can also contain request traces. Session-file inspection selected **only key names, event types, model aliases, and numeric aggregate ranges**; no message body, tool input/output, credential, or quota value was selected.

## Token arithmetic and model window

In the installed bundle, `TokenCountingAgentModel.measured` emits `token_counting.measured` only when the request input matches the current context. Its `tokens` is `inputCacheRead + inputCacheCreation + inputOther + output` for **one full-context request**, not a delta; the output component and later context edits prevent treating the latest measured record as the exact current context size. The default `tokenCounting.strategy` is `measured+estimated`: `statusSize` takes the greater of the measured anchor and an estimate of subsequent context. `withStatusSnapshot` uses `statusSize` for live `contextTokens`, and the server's `assembleStatus` uses it for `context_tokens`. Thus the live numerator and `measured.tokens` share a counting service but are **not guaranteed equal**.

On `TurnEnded`, that service emits durable `token_counting.turn_recorded` with `tokens: statusSize(strategy)`. This is the same numerator as status **at that turn boundary**, without reading conversation content or reproducing the estimator. A later prompt, append, undo, or model binding makes the old record unsuitable until another completed turn. Compaction emits `token_counting.rebased` with `tokensAfter` and `measured: false`; a resumed session replays durable events, so the last measured anchor alone can predate a rebase or later context. Lineup rejects a turn record when a later context- or counting-changing event appears or the file tail cannot establish the required binding.

For the denominator, the bundle resolves model capabilities as `max_input_tokens ?? max_context_tokens`; local `models.<alias>.max_input_size` takes precedence over `max_context_size`. The inspected `~/.kimi-code/config.toml` defines `kimi-code/k3` and `kimi-code/kimi-for-coding` at `max_context_size = 1,048,576`, and both aliases appeared in `profile.bind.modelAlias`. The file's `state.json` has no model/window key. Lineup resolves the alias bound before the selected turn against its configured Kimi home's current `config.toml`. It rejects missing, invalid, or contradictory window sizes. A changed model catalog or config can still make a historical turn's denominator uncertain; this is a source limitation, not evidence of live API accuracy. The research script parsed the whole config file but printed only model aliases and window sizes; it did not open an account or credential store.

## Server API and binding

The [official server API reference](https://www.kimi.com/en/help/kimi-code/cli-server-api) documents `GET /api/v1/sessions/{session_id}/status` with `context_tokens`, optional `max_context_tokens`, and optional `context_usage` (0–1). **Reading the status resumes a cold session**, so this is not a passive observation of an unrelated running CLI. `GET /api/v1/oauth/usage` fetches live managed-account quota; an `ok` response may contain `quota.usages.limit7d.usedRatio` and `resetAt`. Weekly remaining would be `100 × (1 - usedRatio)` only for that returned window. Other account types need not have a managed weekly quota. Both routes require the server Bearer token; the usage route also relies on the managed OAuth account and an external account-service request. The API is explicitly experimental. The installed bundle's runtime schema symbols and the reference agree on these field names, but no live schema or endpoint was requested.

Lineup's Herdr snapshot parses an exact `agent_session` ID (agent `kimi`, kind `id`) and pane `cwd`/`foreground_cwd`; its `Pane` does not expose a PID. Kimi's `state.json` provides `id` and `cwd`, and `session_index.jsonl` has `sessionId` and `workDir`. The reader selects one regular, nonsymlinked state file under `KIMI_CODE_HOME` (default `~/.kimi-code`) whose `id` equals the pane's nonempty native ID and whose lexically normalized `cwd` equals the pane's `cwd`. Duplicates, mismatches, unknown homes, and symlinks below the home are unavailable. Cwd or PID alone cannot choose a session. The existing Codex and Grok readers likewise require an exact native ID and reject ambiguous local matches. A matching file does not establish server or account identity for weekly quota.

## Implemented reader and remaining quota gap

`src/reader_kimi.rs` is dispatched from `src/publish.rs` for a Kimi pane with an exact native session ID. It reads that session's regular `agents/main/wire.jsonl` without following symlinks, selects the last main-agent `token_counting.turn_recorded.tokens` and its preceding `profile.bind.modelAlias` or `config.update.modelAlias`, and rejects later events that invalidate the completed turn. It parses `config.toml` and retains only that alias's `max_input_size` and `max_context_size` integers, requires positive consistent values, and computes `100 × tokens / window` within 0–100%. Credential values in that file are not retained or logged. An unchanged wire file uses a fingerprint cache; a changed file is checked again. Missing or ambiguous bindings, an invalid tail, an unknown model, and future timestamps remain unavailable.

The completed-turn context observation **does not expire by age while the same pane/session binding remains valid**, including while idle. Timeout retention uses the same rule as Codex and Grok. Kimi weekly always remains unavailable, even when context is numeric. The reader makes no network request and does not retain credential values. See the [README usage source](../../README.md) and [provider reader specification](../../openspec/specs/provider-readers/spec.md) for the shipped contract.

The documented `GET /api/v1/oauth/usage` could provide a managed account's `quota.usages.limit7d.usedRatio` and optional `resetAt`, but no Kimi weekly worker exists. Any future quota integration would need an explicitly enabled credential route, a proved account binding, a bounded refresh, and separate freshness/error handling. A separately started server is not proven to own a pane's CLI session; the status API can resume a cold session. No Kimi API or credential endpoint was called in this research.

## Hermes check

`hermes --version` still reports v0.21.6 (2026.9.24). The [CLI reference](https://hermes-agent.nousresearch.com/docs/reference/cli-commands) lists `hermes usage --json` for the configured provider's account windows; this is not a Hermes-owned universal weekly allowance. The [storage guide](https://hermes-agent.nousresearch.com/docs/developer-guide/session-storage) describes cumulative token accounting, not a session's current context occupancy. The existing live `context_usage_fields` helper remains an in-process source with no confirmed durable session-bound record. Hermes therefore remains unavailable under the same exact-session and freshness rules. No usage CLI was invoked.

## Unverified points

- The research lane did not reproduce the integration owner's 11% idle-pane observation or test accuracy against a live Kimi status response.
- The server's generated OpenAPI/AsyncAPI output and real account quota response were not requested. A pane-to-account binding for weekly quota is unproven.
- A historical turn's denominator after a model catalog or config change is not independently recoverable from the session files. No durable Hermes context record was confirmed.

## Commands and limits

```sh
kimi --version
sha256sum ~/.bun/install/global/node_modules/@moonshot-ai/kimi-code/dist/main.mjs
hermes --version
fd -t f -g state.json ~/.kimi-code/sessions | wc -l
fd -t f -g wire.jsonl ~/.kimi-code/sessions | wc -l
fd -t f -g state.json ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'keys[]' | sort | uniq -c
fd -t f -g state.json ~/.kimi-code/sessions -0 | xargs -0 -r jq -r '.agents.main | if type=="object" then keys[] else type end' | sort | uniq -c
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'if type == "object" then (.type // .event // "NO_TYPE") else "NON_OBJECT" end' | sort | uniq -c
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'select(.type=="token_counting.measured" or .type=="usage.record") | .type as $t | keys[] | "\($t) \(.)"' | sort | uniq -c
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'select(.type=="token_counting.measured") | [(.tokens|type), (.length|type)] | @tsv' | sort | uniq -c
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'select(.type=="token_counting.measured") | .tokens' | awk 'NR==1{min=$1;max=$1} {if($1<min)min=$1;if($1>max)max=$1} END{print "count=" NR, "min=" min, "max=" max}'
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'select(.type=="profile.bind" or .type=="token_counting.turn_recorded" or .type=="token_counting.rebased") | .type' | sort | uniq -c
fd -t f -g wire.jsonl ~/.kimi-code/sessions -0 | xargs -0 -r jq -r 'select(.type=="profile.bind" and (.modelAlias|type)=="string") | .modelAlias' | sort | uniq -c
jq -r 'keys[]' ~/.kimi-code/session_index.jsonl | sort | uniq -c
python3 - <<'PY'
from pathlib import Path
import tomllib
config = tomllib.loads((Path.home() / '.kimi-code/config.toml').read_text())
for alias, model in config.get('models', {}).items():
    print(alias, model.get('max_input_size'), model.get('max_context_size'))
PY
```

The installed `dist/main.mjs` was inspected around `TokenCountingAgentModel`, `SessionTokenCountingService`, `withStatusSnapshot`, `SessionLegacyService.assembleStatus`, and `resolveModelCapabilities` using a local Python substring search. No server, session, account, authentication, or provider endpoint was called. The counts describe this installation at inspection time, not a format guarantee. The bundled API schemas are generated by the installed server rather than shipped as static files, so their full runtime output and real response behavior remain unverified.
