# Implementation tasks

- [ ] Replace the capacity-only skip error with a typed refresh skip carrying
      capacity and deadline counts, and classify it in daemon health without
      changing the `Health` struct.
- [ ] Add the cooperative refresh envelope and pass the caller deadline to
      every snapshot and metadata exchange, including the implicit reconnect.
- [ ] Add the 2 s admission threshold after reservation persist; refused
      admission is a deadline skip, a failed admitted exchange is an ambiguous
      error with a burned sequence and no replay.
- [ ] Add fair queues for Git headings and metadata sends with sync, single
      pass, rotation on attempt start and per-ID deadline skip counting.
- [ ] Share one Git budget per cycle through a publisher-owned repository cache
      and implement the Confirmed, Fallback and Unknown heading states.
- [ ] Create one reader cycle per refresh, skip the reader call when its budget
      is spent, and keep the publisher-owned reader diagnostics for later use.
- [ ] Report production refresh stage timings after every attempt.
- [ ] Keep clear on its own three-second deadline.
- [ ] Cover the plan's boundary fixtures in the default test suite.
