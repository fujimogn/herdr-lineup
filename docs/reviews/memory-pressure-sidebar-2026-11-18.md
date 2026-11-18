# Sidebar heading loss under memory pressure

## Finding

A Git stage that repeatedly misses its deadline can remove all group headings
while usage values, vendor logos, and the indentation of later panes continue
to update. The isolated fixture reproduced this through explicit null patches
for heading keys that were still live in the metadata store. The group headings
returned on the first successful Git refresh. This mechanism matches the
second owner report. The incident has no per-workspace Git outcomes or accepted
metadata frames from the affected interval, so the live trigger remains
unconfirmed.

The two reported symptoms have different evidence. The reported Claude row's
missing indentation is consistent with it being the first snapshot pane of a
workspace whose heading disappeared. Loss of the whole pane's metadata or
a change in snapshot order could also affect its appearance. The first report
does not identify which case occurred. The Codex row's two dashes can come from
a successful Lineup report carrying unavailable usage. The fixture observed
these literal dash tokens with an available logo and healthy Git. In the
no-publication control, TTL expiry removed the tokens altogether. The cause of
the original Codex usage gap remains unconfirmed.

The commander reported 35 GiB of used swap and memory PSI full avg10 of 3.5.
The reported doctor aggregates included a 16.8 s maximum refresh, 392 refreshes
of at least 5 s, 380 deadline skips, approximately 1.2 s mean socket stage time,
and a 15.7 s maximum persistence stage. These aggregates establish pressure and
publication delays across the observed window. They do not identify how long any
individual key went unacknowledged or when a particular repository was last
verified. A deadline skip is counted when at least one metadata resource misses
admission. Other resources in the same pass can still be published.
[Refresh accounting](../../src/daemon.rs#L1075-L1114) ·
[Per-resource admission and skip accounting](../../src/publish.rs#L1639-L1666) ·
[Refresh send pass](../../src/publish.rs#L2401-L2463)

## Why the paths differ

- The Git stage gets a shared 1 s budget, limited by the remaining refresh
  envelope, with at most four concurrent observations. One query also has a
  1 s cap. A deadline produces `Interrupted`, and work declined before starting
  can produce `NotAttempted`. Reaching a cooperative deadline can still take
  longer in wall-clock time when the process is descheduled, blocked in the
  kernel, or reaping a child.
  [Cycle terms](../../src/publish.rs#L67-L92) ·
  [Git admission](../../src/publish.rs#L1851-L1857) ·
  [Git batches](../../src/publish.rs#L1925-L2040) ·
  [Observation deadline](../../src/ship_time.rs#L214-L259)
- Both outcomes can reuse a heading with the same cwd and ordered pane
  occupants for less than 30 s from the original verification instant. A Git
  repository cache hit carries the original instant. At the boundary, the
  fallback yields an unresolved heading and drops its cache entry. An explicit
  `Unknown` can take the same output path immediately for a completed read
  without a repository, including a refusal. A changed cwd or occupant list
  invalidates the old fallback.
  [Heading fallback](../../src/publish.rs#L1061-L1113) ·
  [Git cache hit](../../src/ship_time.rs#L306-L324) ·
  [Heading state contract](../../openspec/specs/sidebar-tokens/spec.md#L147-L174)
- An unresolved heading sets the three workspace keys to null and leaves
  `lineup_group` and `lineup_group_ship_text` null on the first pane. Usage and
  the logo are computed separately. With positive
  `non_first_pane_logo_indent`, only later panes get U+2800 prefixes, based on
  snapshot order. This accounts for the first pane appearing at the left
  margin of each former group.
  [Workspace keys](../../src/publish.rs#L2043-L2091) ·
  [Pane keys and logo](../../src/publish.rs#L2286-L2326) ·
  [First-pane order](../../src/publish.rs#L2587-L2597) ·
  [Pane token defaults and usage formatting](../../src/tokens.rs#L146-L200)
- Metadata TTL is a separate per-key lease of three refresh intervals, which
  is 30 s at the default interval. A successful report renews the populated
  keys in its patch. An omitted key can still expire if its own accepted
  reports fall too far apart. Losing all metadata to expiry would remove
  the logo and value keys as well. Codex's literal dashes are populated
  `lineup_context_text` and `lineup_weekly_text` values for unavailable usage.
  For example, a missing native-session binding can leave context unavailable,
  and the daemon requires a proven local binding to apply account quota.
  [TTL and dirty keys](../../src/tokens.rs#L476-L555) ·
  [Codex account binding](../../src/publish.rs#L2221-L2234) ·
  [Degraded expiry contract](../../openspec/specs/sidebar-tokens/spec.md#L429-L433)

## Isolated evidence

Source: `92e3e63d6860b9efffc599c7d5cc08c915c06ce6`, Linux, pinned Rust
1.100.0. The fixture used the foreground publisher, a private Unix socket
with a synthetic Herdr 0.9.3/protocol 22 peer, private plugin state and config,
an empty home, and local Git repositories. It used no Herdr server, user data,
or network. The peer applied increasing sequences and per-key TTLs and
returned live token maps in each snapshot. The check ends at stored metadata.
The owner reports supply the sidebar appearance.

The fixture uses two workspaces with a synthetic local `origin/main` each,
two Claude panes per workspace, and one Codex pane without a native-session
binding. The first pane stays fixed in each workspace. The fixture rewrites
synthetic Claude statusLine records before each snapshot, increments both
percentages, and supplies a future weekly reset. Settings are a 1 s refresh
interval, 100 ms reader timeout, text logos, and one cell of indentation.
The resulting metadata TTL is 3 s and the Git fallback is still 30 s.

1. Let native Git resolve both workspaces. Verify both group headings and ship
   times, four current Claude readings, first and later logo indentation, and
   the Codex literal dash tokens before injecting any fault.
2. Keep the same snapshot shape, cwd, and occupants. Put an executable `git`
   shim first on the daemon's private `PATH`. Once the baseline is live, have
   the shim run a 4 s sleep in place of Git. With the stall disabled, the shim
   executes local Git. Keep acknowledging and recording all metadata patches.
3. Observe both first panes and both workspaces receive heading nulls. Check
   each targeted key was still live immediately before its null patch.
   Check the same first-pane patches carry current usage and the normal logo,
   and that the other Claude panes retain their indent and receive current
   values. Remove the stall and check heading recovery on the next refresh.
4. After recovery, close the connection on each subsequent snapshot request
   before sending a response. Verify no further metadata frames are sent and
   the peer drops all previously stored keys when their TTLs expire.

The release fixture passed one run with 17 completed snapshots (1 healthy,
15 stalled, 1 recovered) and 30 deliberately stalled Git invocations. The
four null patches arrived in cycle 16, 30.297–30.307 s after the initial
complete publication. Each first-pane patch contained `lineup_group: null`,
`lineup_group_ship_text: null`, `lineup_logo: "Claude"`, `lineup_context_text:
"36%"`, and `lineup_weekly_text: "7d 76% 1d0h"`. The workspace patches cleared
all three workspace heading keys. Every cleared key was live at deletion.
Both later Claude panes retained their indent and both usage values. Both
headings returned in cycle 17. After snapshot transport was interrupted, the
fixture observed zero live resources and zero literal dash tokens 3.035 s
after the last accepted report.

The release executable SHA-256 was
`9270efb3f25debb6ec5d7ef5ff879ab7a7b70de1d3f582b8f8581a2df43f7326`.
The ephemeral driver SHA-256 was
`b51f31033ff509dbc80b360b4a52a4157b1e535e348e4af9a01a8b581d79d2bc`.
The earlier debug attempt observed the same clear and recovery, then
failed the harness's assertion that deliberately terminating the fixture
child would return exit 0. The release run expected the termination signal.
These are fixture timings under injected Git stalls. The current production
failure frequency and the host's sustained memory-pressure threshold remain
unmeasured.

The repository's existing isolated tests also cover
`heading_fallback_expires_thirty_seconds_after_the_original_verification`
(including 29 s and 30 s), `group_heading_moves_with_snapshot_order_and_clears_without_repository`,
`logo_indent_tracks_snapshot_order_and_changes_only_logos`,
`git_queue_rotates_a_stalled_repository_and_unread_headings_are_null`, and
`deadline_skip_counts_refused_frames_and_rotates_only_started_items`.
They ran in the pinned `cargo test --locked` suite.

## Options for a follow-up

| Option | User-visible effect and staleness | Sequence and durability | Verification |
| --- | --- | --- | --- |
| A. Give same-identity interrupted headings a bounded stale display | A brief, visibly marked grace period after the current 30 s limit would reduce heading flicker. Repo and ship text can be outdated until the selected hard cap. Clear immediately for explicit `Unknown`, changed cwd or occupants, or a missing workspace. Anchor both cutoffs to the original verification. | Reuse the acknowledged pane and workspace patch path, with an explicit stale marker within current capacity or a separately specified token. Keep the fallback in memory. Do not persist stale values, change the checkpoint, or extend key TTL. A transport outage can still let the keys expire. | Inject 29 s, 30 s, and hard-cap boundaries. Check both heading locations, stale indication, immediate removal or refusal, identity changes, recovery, restart, and missing acknowledgements. |
| B. Schedule Git work by verification age and reduce redundant reads | Favor headings nearest their cutoff, within a bounded slot for newly seen workspaces. Keep sharing reads per cwd. Measure whether retry placement or distinct Git work causes most misses before choosing additional work. Fresh readings should survive more pressure, while a sustained failure still clears at the existing cutoff. If Git is moved off the refresh path, bind every result to its original cwd, ordered occupants, and verification time. | Git reads update only in-memory observation state. The existing publisher remains the sole metadata writer and only it reserves sequence windows. Discard obsolete worker results and keep process reaping and bounded concurrency. Accepted frames and acknowledgement semantics stay the same. | Replay the stalled-Git fixture and mixed fast and slow repositories under a bounded workload. Compare heading age, timeout and starvation counts, accepted-frame gaps, child count, and stage latency. Run the identity, shutdown, Git refusal, and exact-expiry cases. |
| C. Give necessary layout renewals earlier metadata admission | This can protect layouts when evidence shows accepted-frame gaps. Combine an imminent valid layout renewal with the normal pane patch when possible. A dedicated layout pass may delay usage or leave a stale layout visible for its existing lease if subsequent reconciliation is delayed. Apply current heading freshness and provider-specific usage rules before renewal. | Use the same sender, capacity check, sequence reservation and acknowledgement path for a renewal. Each extra frame spends a sequence and may require a persisted window. Avoid extra frames where coalescing works. Keep explicit nulls and stop on an ambiguous socket response. | Reproduce repeated admission skips with a fake clock independently of Git. Record per-key accepted-send gaps, ordering, usage starvation, and additional frames and checkpoint writes. Verify repair, restart, sequence monotonicity and lost acknowledgements. |

Start with B and measure per-workspace verification ages and the counts of
`Interrupted`, `NotAttempted`, and explicit `Unknown`. Pair those counts
with per-resource acceptance gaps and heading-clear frames in a synthetic
pressure replay before choosing a scheduling change. A is an owner decision
about the acceptable age of displayed Git information. C addresses a separate
publication failure mode supported by the no-frame control. Raising token TTL
or changing the refresh interval would leave the explicit null patches in
place and would change the degraded-expiry contract. The recorded options
await owner selection.
