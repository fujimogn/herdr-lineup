# Refresh stage latency, 2026-10-23

## Scope and reproduction

The original measurement candidate, `4afc9d42b9bab850576ffb3bd42d44accf2d21c2`,
was based directly on `b5fc3114565f63d6fc5765a36d69dcf3ba47f3a0`.
Its test instrumentation and this report were subsequently carried forward
onto the local consolidation candidate
`a6cddcca01873c9f30f0632b20d1b28108562129`. The results below remain the
original measurements on the earlier base, not measurements of the consolidated
code. Local integration does not establish independent review or shipment;
shipment remains on hold.

This is a synthetic Linux measurement of one cold publisher refresh, not a
measurement of a running Herdr session. The ignored Rust test
`publish::tests::measure_refresh_stages` reuses the existing one-request-per-
connection fake socket and snapshot fixture. It creates one temporary local Git
repository with `origin/main`, one workspace, and 1, 8, 32, 128, or 256 Codex
panes. Every pane has a synthetic native session; `SampleReader` returns a fixed
sample without touching user agent files. Network quota is disabled. Each run
uses a new publisher, checkpoint, fake socket, and repository, with three runs
per pane count and filesystem. The test does not contact a Herdr server or a
user session, or change user configuration.

The test uses `Instant` for the entire refresh cycle, the snapshot, workspace
heading/Git work, injected reader calls, sequence checkpoint persistence, and
socket exchanges. It counts successful metadata responses and the final number
of checkpoint rows. All counters and timing branches are compiled only for
Rust tests (`cfg(test)`); the normal runtime does not emit metrics or change
metadata. Run it with:

```sh
LINEUP_BENCH_ROOT=/tmp LINEUP_BENCH_REPEATS=3 cargo test --locked measure_refresh_stages -- --ignored --nocapture
LINEUP_BENCH_ROOT=/var/tmp LINEUP_BENCH_REPEATS=3 cargo test --locked measure_refresh_stages -- --ignored --nocapture
```

On this Linux host, `df -T` identified `/tmp` as tmpfs and `/var/tmp` as ext4.
The test drops only its `/tmp/herdr-lineup-refresh-*` fixture directories. The
ext4 fixtures from this run were retained under
`/var/tmp/herdr-lineup-refresh-measure-20261023-pV`; this path is measurement
residue, not plugin state. Results below are debug test builds with the pinned
toolchain, on the same host and test binary. The two filesystem runs were
sequential, so cache and host load were not controlled experimentally.

## Results

Times are milliseconds. The three cycle totals show all runs in order; stage
columns are independent medians of the same three runs. The cycle includes
configuration validation, weekly diagnostic initialization, and the publication
pipeline. `pipeline` excludes the first two steps. The test's external wall
clock was within 0.04 ms of the recorded cycle on every run. That small gap
checks the timer boundary; it **does not** isolate the cost of instrumentation,
which is included in the cycle time.

| Filesystem | Panes | Cycle runs | Cycle median | Snapshot | Heading/Git | Reader | Persist | Socket | Reports | Checkpoint rows |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| tmpfs | 1 | 9.608, 11.413, 9.199 | 9.608 | 0.462 | 5.516 | 0.007 | 0.177 | 3.111 | 2 | 2 |
| tmpfs | 8 | 37.554, 25.744, 25.891 | 25.891 | 0.748 | 6.644 | 0.038 | 0.936 | 17.351 | 9 | 9 |
| tmpfs | 32 | 83.694, 87.250, 90.303 | 87.250 | 2.838 | 11.279 | 0.149 | 6.700 | 61.586 | 33 | 33 |
| tmpfs | 128 | 318.325, 308.164, 299.461 | 308.164 | 9.849 | 11.751 | 0.454 | 66.706 | 203.987 | 129 | 129 |
| tmpfs | 256 | 597.520, 609.941, 603.923 | 603.923 | 18.829 | 5.295 | 0.810 | 230.642 | 317.930 | 257 | 257 |
| ext4 | 1 | 39.294, 36.237, 34.308 | 36.237 | 0.436 | 9.391 | 0.008 | 22.422 | 3.098 | 2 | 2 |
| ext4 | 8 | 119.965, 108.573, 132.621 | 119.965 | 1.101 | 5.568 | 0.053 | 100.521 | 10.431 | 9 | 9 |
| ext4 | 32 | 368.603, 356.700, 303.551 | 356.700 | 3.430 | 5.786 | 0.208 | 273.117 | 43.809 | 33 | 33 |
| ext4 | 128 | 1281.125, 1439.873, 1495.477 | 1439.873 | 12.309 | 11.171 | 0.927 | 1193.179 | 192.339 | 129 | 129 |
| ext4 | 256 | 2881.471, 2808.358, 2877.134 | 2877.134 | 17.794 | 4.594 | 1.690 | 2414.420 | 360.949 | 257 | 257 |

Each cold cycle sent one report per pane plus one workspace report. The durable
checkpoint therefore grew to `panes + 1` rows. At 256 panes, persistence was
about 38% of the tmpfs cycle median and 84% of the ext4 cycle median; the
socket stage was about 53% and 13%, respectively. These are ratios of
independent medians and are approximate. The total grows with report count on
both filesystems. The ext4 result points to repeated durable sequence writes as
the first optimization candidate **for this fixture**; it is not evidence for
changing reservation order or durability. A batched reservation design would
need a separate safety review before implementation.

## Boundaries and missing evidence

- Stage times are nested. Snapshot includes its socket exchange and checkpoint
  load; socket includes the snapshot and metadata exchanges. Persistence occurs
  inside reports. Adding stage columns double-counts time and cannot reproduce
  the cycle total. Heading/Git also includes heading formatting and repository
  cache work, not only Git subprocesses. The reader column times injected
  `SampleReader::read` calls, not complete pane processing.
- The test records successful report responses. It does not measure retries,
  ambiguous responses, reconnects, shutdown, resource pruning, or periodic
  renewals against an existing checkpoint. The fake socket does not reproduce
  Herdr's own scheduling, storage, or sidebar rendering cost.
- Real Codex/Grok/Kimi file discovery, slow or stalled readers, statusLine
  inputs, account workers, and network latency were not measured. The test
  does not establish a bound on their contribution to a real refresh.
- An earlier latency investigation reported about 200 ms for 256 panes. Its
  fixture, build mode, and disk conditions differ from this experiment, so the
  two values are not directly comparable.
- Timing counters and mutex updates add test-only work to the measured cycle.
  There is no uninstrumented control run, so their independent cost cannot be
  estimated from the external-versus-cycle gap. Three runs per condition expose
  variation but do not provide a stable tail-latency estimate.
- macOS and non-ext4 disks were not measured. Linux results should not be
  generalized to macOS durability or socket behavior.

No runtime optimization is proposed from these measurements alone. The next
decision is whether the ext4 cold-cycle latency and actual pane/report counts
justify evaluating a safe sequence-reservation batch, while separately timing
real reader work in an isolated, authorized fixture.

## Local carry-forward verification

On 2026-10-23, the instrumentation was applied to local consolidation base
`a6cddcca01873c9f30f0632b20d1b28108562129`. Formatting, Clippy with warnings
denied, locked tests (320 passed, 6 ignored, plus one integration test), the
locked release build, and release binary `--help` passed. Encoding and diff
checks passed as well.

The ignored measurement test also passed with `LINEUP_BENCH_ROOT=/tmp` and
`LINEUP_BENCH_REPEATS=2`. All ten cases reported `panes + 1` successful reports
and checkpoint rows. This was an integration smoke check, concurrent with a
release build, not a replacement for the historical latency results above.
The ext4 experiment was not repeated. Independent review of the carry-forward
diff and shipment remain outstanding.
