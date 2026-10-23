# Design: publisher runtime wiring

The full design, invariants and fixture list are in
`docs/plans/publisher-runtime-wiring.md` (design GO on `86ea771`). This file
records the decisions that the spec deltas depend on.

## Decisions

1. **Cooperative envelope.** At cycle start `t0`, `E = t0 + 2 s (snapshot) +
   1 s (Git) + reader_timeout_ms + 2 s (one metadata request)`. These are the
   existing local caps, not measurements. `E` is not an SLO, not a strict
   wall-clock bound and not a bound on fsync, `stat`, Git reap or scheduler
   delay; synchronous work is checked before and after and can overrun it.
2. **Admission.** A metadata request starts only while `E − now ≥ 2 s`,
   checked after any reservation persist and beside the shutdown check. The
   check only reads the remaining budget, so an admitted exchange can still be
   cut by `E` in flight. That is an ambiguous error: the sequence is burned,
   the request is never resent, the cycle ends with an error, and the daemon
   reconnects. A refused admission writes no frame and is a deadline skip.
3. **Fair queues.** One private `FairQueue<K>` for Git (workspace ID) and sends
   (resource ID). Sync keeps survivors in order and appends new IDs in
   snapshot order. Each cycle walks a copy of the order once with a visited
   set. An item rotates to the tail when its first budgeted I/O starts, even if
   that I/O then fails. Rotation is never undone and is independent of the
   `deadline_skipped` count (once per resource ID per cycle).
4. **Send pass.** Panes keep observe → plan → send per pane. A pane calls its
   reader only while `min(reader deadline, E)` is positive; otherwise it gets
   a `Timeout` observation through the normal timeout cache rules, and the pass
   continues under admission. A planner that needs no frame does no I/O, does
   not rotate and is not counted.
5. **Git.** `G = min(t_git + 1 s, E)`, each query `min(1 s, G − now)`. One
   `RepositoryCache` lives for the publisher; one result per cwd is shared
   within a cycle and only the workspace that started the read rotates.
   Workspaces not reached are `NotAttempted`.
6. **Heading states.** Confirmed needs `Complete(Some)` with `verified_at`.
   Fallback is allowed only for `Interrupted` or `NotAttempted`, with the same
   cwd and occupants, for 30 s from the original `verified_at`; it never
   renews that instant. Everything else, including `Complete(None)`, is
   Unknown and publishes null. Only a confirmed repository without
   `origin/main` shows the `—` ship text.
7. **Outcome.** A refresh returns one typed skip payload with capacity and
   deadline counts. Shutdown is classified first, then the payload, then
   errors. A deadline skip does not set `refresh_failed` and does not
   reconnect. The `Health` struct is unchanged; deadline skips are reported in
   a separate optional control field so older peers keep parsing the frame.
8. **Clear** keeps its own 3 s deadline, independent of any refresh envelope,
   and passes it to the socket calls it makes.

## Rollback

Revert the implementation commits. No on-disk format changes; a checkpoint
written by the new code is valid for the old code.
