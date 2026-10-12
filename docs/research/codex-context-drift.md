# Codex context one-point drift

Research snapshot: 2026-10-13. Lineup revision `f390fd0bb1be7fc2970522284a7a33a2528d5997`. Retired plugin `fujimogn/herdr-agent-quota` at `d428889cd6d0dc675e0b4058dc5c5f172d979e5b` (the commit recorded in `docs/research/usage-sources.md`). Codex UI formula from `openai/codex` `main`, `TokenUsage::percent_of_context_window_remaining`, retrieved the same day.

No `src/` change. The two idle panes that disagreed by one displayed point were not captured, so this note does not pick a single cause. It records which inputs reproduce a one-point gap, and which do not.

## Shared occupancy formula

Both implementations, given the same finite numbers, compute used occupancy as:

```text
max(total_tokens - 12000, 0) / (model_context_window - 12000) * 100
```

Lineup: `src/reader_codex.rs` reads `payload.info.last_token_usage.total_tokens` and `payload.info.model_context_window` as `f64`, requires `capacity > 12000`, and keeps the last parsed `token_count` (`observation = Some(...)` inside the forward scan). The 12,000-token baseline is subtracted from both sides. `src/model.rs` `UsageReading::sample` keeps that percent for an idle pane when it is finite and in `0.0..=100.0`. A timestamp in the future, or a percent outside that range, becomes unavailable. Lineup does not clamp an over-100 result to 100.

Retired quota: `src/providers/codex.rs` `parse_rollout_observation` reads the same two fields, plus camelCase aliases, through `as_u64` (`token_count`). `CODEX_CONTEXT_BASELINE_TOKENS` is 12,000. `total == 0` or `window <= 12000` skips that line and keeps an earlier one. The ratio is clamped to `0.0..=100.0`.

Codex's own UI uses the same baseline on `total_tokens`, then rounds the **remaining** ratio with `f64::round` (`half away from zero`). It is the complement of the shared used ratio, not a different occupancy formula.

`total_token_usage` is the session cumulative counter. The retired parser uses it for cache totals only. Lineup ignores it. Substituting it for `last_token_usage.total_tokens` saturates the window (often 100%), which is not a one-point gap.

## Where the displayed integer is decided

Lineup sidebar text is `src/tokens.rs`: `format!("{:02}%", value.round() as u8)`. `f64::round` is half away from zero. `0.0` prints as `00%`. The numeric token `lineup_context_pct` keeps the unrounded decimal (`7.2`), so a one-point comparison is about `lineup_context_text`, not that decimal. README states that values at or above 99.5% display `100%`. The test `invalid_percentages_clear_and_rounding_is_half_up` locks `7.5 → 08%` and `99.5 → 100%`.

Retired sidebar text is `format_percent` in `src/model.rs`: Rust `{:.0}` (half to even) and, when the value is below 100 but that format yields `"100"`, the string `"99"`. `printed_percent` parses that string. Default `PercentStyle` is `Remaining` (`src/cli.rs`). `context_percent` in `src/presentation.rs` then shows `100 - used_percent` under that default, and `used_percent` only when the style is `used`.

Padding (`07%` versus `7%`) does not change the integer.

## Reproducible examples

Each row is one JSON object in the rollout shape both readers parse. Only the numeric fields that enter the formula are shown. Percents below were computed with Rust `f64` on these integers.

### Same latest integers: no one-point gap

```json
{"type":"event_msg","timestamp":"2026-10-12T00:00:00Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":199826,"output_tokens":281,"total_tokens":200107},"total_token_usage":{"total_tokens":6951013},"model_context_window":258400}}}
```

| Quantity | Value | Lineup text | Retired `format_percent` |
| --- | --- | --- | --- |
| Used occupancy | 76.3421266234% | `76%` | `76` in `used` style |
| Remaining | 23.6578733766% | (not shown; lineup context is use) | `24` in default `remaining` style |
| Codex UI remaining | same ratio, `f64::round` | | `24` |
| `total_tokens / window` with no baseline | 77.4407894737% | would print `77%` | would print `77` |

The live shape on this machine uses `model_context_window` 258400 and integer `total_tokens`. Cumulative `total_token_usage.total_tokens` was about 6.9e6 on that same record and is not what either context percent uses. Default remaining (`24`) versus lineup use (`76`) is a large gap, not one point. A report of "one point" is a same-side comparison, or a complement of the printed remaining integer.

### Rounding, same integers: one point

`total_tokens` 49000, `model_context_window` 212000 (the capacity in `src/reader_codex.rs` tests):

```text
(49000 - 12000) / (212000 - 12000) * 100 = 18.5 exactly
```

| Display | Result |
| --- | --- |
| Lineup `f64::round` | `19%` |
| Retired `{:.0}` half-to-even, `used` style | `18` |
| Retired default remaining | `82` (`100 - 18.5 = 81.5` → `82`) |
| Complement of that remaining | `18` |

Lineup `19` versus retired used `18`, or versus the complement of default remaining, is one point. The same split happens for every exact `n.5` where half-to-even and half-away disagree (`0.5`, `2.5`, `4.5`, …, `98.5`). Odd ties such as `7.5` and `1.5` agree (`08` / `8`, `02` / `2`).

`total_tokens` 211200, window 212000:

```text
(211200 - 12000) / 200000 * 100 = 99.6
```

Lineup prints `100%` (specified, including the 99.5 boundary). Retired `used` style prints `99` because of the cap. That is one point, and it is not only a `.5` tie: every used percent in `[99.5, 100)` does this. Retired default remaining of 99.6 is `0` (`0.4` rounds to `0`), so the raw sidebar strings are `100%` versus `0`, which is not a one-point report.

`total_tokens` 12985, window 258400:

```text
(12985 - 12000) / 246400 * 100 = 0.3997564935
```

Both `used` displays are 0. Retired default remaining is `99`, not `100`, because 99.6002435065 is below 100 and `{:.0}` would have been `100`. The complement of the printed remaining is 1. Lineup `00%` versus that complement is one point. The raw strings are `00%` versus `99`.

### Different accepted line: one point, same formula and same rounding

Two complete events, same window 258400:

| Event | `total_tokens` | Used percent | Both displays |
| --- | --- | --- | --- |
| Previous | 200107 | 76.3421266234 | 76 |
| Latest | 200743 | 76.6002435065 | 77 |

If one reader keeps the previous line and the other keeps the latest, the sidebars differ by one point. Rounding is not involved.

The retired parser skips a line when `as_u64` fails or `total == 0`, then keeps the previous line. Lineup's `as_f64` accepts a JSON number that is not an integer. A latest record whose `total_tokens` is the JSON number `200743.0` (a float, so `as_u64` is `None`) makes the retired total 0, skips the line, and stays on 76. Lineup uses 200743 and prints `77%`. Local tails that were checked stored these fields as JSON integers, so this path is demonstrated from the parsers, not from a captured pane.

Lineup does the opposite on a bad latest line: a non-finite value, a negative total, or a missing `total_tokens` returns `MissingReason::Invalid` for the whole file (`src/reader_codex.rs`), so the sidebar becomes `—`, not the previous percent. A completed malformed JSON line does the same. The retired parser `continue`s on a JSON error. That disagreement is a number versus `—`, not one point.

`total_tokens == 0` on the latest line: retired keeps the previous percent; lineup reports 0%. That is a large gap.

### Other inputs that can move the integer by one

`input_tokens` instead of `total_tokens`, window 258400, baseline still applied, lineup rounding:

| Field | Tokens above 12000 | Percent | Display |
| --- | --- | --- | --- |
| `total_tokens` | 144145 | 58.5004058442 | 59 |
| `input_tokens` | 143845 | 58.3786525974 | 58 |

Neither reader uses `input_tokens` for the percent. The output-token delta is enough to cross a rounding boundary. This is a one-point gap only if a third formula used input.

No baseline, on the first example: 77 versus the shared formula's 76. Both readers apply the baseline, so this does not separate them. It does show that a one-point gap can be a formula change whose raw percents are 76.34 and 77.44, not a `.5` tie.

Catalog window 272000 versus the recorded 258400 (272000 × 0.95 = 258400, a split described in Codex issue comments) moves the display by more than one point on the first example: `(200107 - 12000) / (272000 - 12000) * 100 = 72.3488461538`, which prints 72 against 76. Neither reader substitutes 272000.

## What the local rollouts did

Method, 2026-10-13, this machine's Codex home, files at most 16 MiB, no session text retained: take the last `token_count` whose `last_token_usage.total_tokens` and `model_context_window` are JSON integers and whose window is above 12000; run both display functions in Rust.

- 400 of 400 latest samples: lineup integer, retired `used` integer, and `100 -` retired remaining integer all matched.
- 1392 files between 256 KiB and 16 MiB: the last `token_count` line started inside the final 256 KiB, so the retired tail read (`ROLLOUT_TAIL_BYTES` in `src/providers/codex.rs` `read_rollout_observation`) and lineup's full read selected the same line.
- Of 200 recent files, the displayed integer changed by exactly 1 between the last two `token_count` events on 42 files. A one-line selection miss is a common one-point neighbor. It did not occur for tail-versus-full on this corpus.
- Of those 200, `input_tokens` versus `total_tokens` changed the displayed integer by 1 on 10 files. Dropping the baseline changed it by 1 on 26 files and by 2–4 on most of the rest. Using 272000 instead of a recorded 258400 changed it by 1 on 42 of 199 such windows.

So "rounding of the same latest integer sample" does not explain a one-point gap on ordinary current rollouts. The fractions sit near a half (for example 58.5057 and 60.4988) often enough to look like a rounding bug, and both rounders still agree.

## Behaviors that do not produce one point

- File larger than 16 MiB: lineup `Budget::bytes` reads at most `16 * 1024 * 1024 + 1` bytes and returns `SourceError` (`src/model.rs`). The retired reader still uses the last 256 KiB. The sidebar is `—` versus a percent.
- Two rollout paths for one session id: lineup returns `Unassociated` when more than one `sessions` or `archived_sessions` name ends with `-{id}.jsonl` or `-{id}.jsonl.zst` (`src/reader_codex.rs`). The retired finder keeps the newest mtime whose name contains the id, and skips a `.jsonl.zst` that has a plain sibling. Again `—` versus a percent, unless the two files' percents are what get compared across different choices.
- Future `timestamp`: lineup drops the sample (`src/model.rs`). The retired timestamp is only the cache-TTL anchor. `—` versus a percent.
- Idle age: lineup does not expire exact-session context. The retired context value is also the last accepted rollout observation, not a heartbeat. Age alone does not move the integer while the file is unchanged.

## Conclusion

The cause of the two idle panes is **not determined**.

On one shared latest integer sample, lineup and the retired plugin compute the same used percent and, away from the ties below, print the same integer. That includes the real `200107 / 258400` shape. Rounding explains a one-point gap only for:

- an exact half where half-away and half-to-even disagree (example: 49000 tokens in a 212000 window → lineup `19%`, retired used `18`), or
- used percent in `[99.5, 100)` under `used` style (lineup `100%`, retired `99`), which lineup documents on purpose, or
- used percent in `(0, 0.5)` when the retired default remaining print is complemented (`00%` versus complement `1`).

None of those ties appeared in 400 local latest samples. A one-point gap on those panes therefore needs a different accepted input: another `token_count` line (commonly one displayed point away), a non-integer `total_tokens` that only lineup accepts, or a different window or token field. Which of those happened was not captured.

## Observations still required

For each of the two panes, at the moment the integers differ:

1. Both sidebar strings and the retired percent style (`remaining` or `used`, including `HERDR_AGENT_QUOTA_PERCENT` or the stored preference).
2. The rollout path each reader opened, the file size, and whether a second `-{session}.jsonl` existed.
3. The accepted line's `last_token_usage.total_tokens`, `input_tokens`, `output_tokens`, `model_context_window`, JSON number type (integer or float), and `timestamp`, plus the previous `token_count` with those same fields.
4. Wall clock versus that timestamp (future skew hides lineup only).

No source change until that capture matches one row above. If it matches the exact-half or `[99.5, 100)` row, leave `src/tokens.rs` as specified (`99.5 → 100%`) and treat the retired `99` cap as the other side of the gap. If it matches a skipped latest line, the reader change would be in `src/reader_codex.rs`: decide whether a non-`u64` or zero `total_tokens` should skip the line, as `parse_rollout_observation` does, instead of failing the file or accepting `as_f64`. Add a fixture with a previous integer sample of 200107 / 258400 (76) and a latest `total_tokens` of `200743.0`, and assert the chosen display. Do not make that change on this evidence.
