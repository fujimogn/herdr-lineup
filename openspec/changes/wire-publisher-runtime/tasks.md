# Implementation tasks

- [x] Replace the capacity-only skip error with a typed refresh skip carrying
      capacity and deadline counts, and classify it in daemon health without
      changing the `Health` struct.
- [x] Add the cooperative refresh envelope and pass the caller deadline to
      every snapshot and metadata exchange, including the implicit reconnect.
- [x] Add the 2 s admission threshold after reservation persist; refused
      admission is a deadline skip, a failed admitted exchange is an ambiguous
      error with a burned sequence and no replay.
- [x] Add fair queues for Git headings and metadata sends with sync, single
      pass, rotation on attempt start and per-ID deadline skip counting.
- [x] Share one Git budget per cycle through a publisher-owned repository cache
      and implement the Confirmed, Fallback and Unknown heading states.
- [x] Create one reader cycle per refresh, skip the reader call when its budget
      is spent, and keep the publisher-owned reader diagnostics for later use.
- [x] Report production refresh stage timings after every attempt.
- [x] Keep clear on its own three-second deadline.
- [x] Cover the plan's boundary fixtures in the default test suite.

- [x] Expose the publisher registry via an opaque handle and typed optional
      live reader diagnostics, keeping private model types private.
- [x] Enforce the exact control frame budget with one-sample fallback.
- [x] Print fixed reader and retained deadline-skip diagnostics before errors.
- [x] Verify live release, contention, skew, saturation and frame boundaries
      in normal isolated tests and run the mandatory checks.
