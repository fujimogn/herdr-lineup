# Old-binary rollback observation (2026-11-13)

This record checks actual release-profile executables against a checkpoint
written by Lineup 0.3.4. It is a Linux observation, not a guarantee for every
older release, filesystem, or crash scenario. No production source is changed.

## Versions and build boundary

| Binary | Source commit | Compiler |
| --- | --- | --- |
| 0.3.4 | `4eecd41b864296e6dd884fc2820a1fac09e7c4ad` | Rust 1.100.0, pinned |
| 0.3.3 | tag `v0.3.3`, peeled commit `7fdb1dc211640912072980949c50bed3911bbba0` | Rust 1.99.1, pinned |
| 0.1.0 | tag `v0.1.0`, commit `8ce06af940c5d5da2aff09d914747d0a4957d568` | Rust 1.99.1, approved patch-version substitution |

The 0.3.4 executable was built from the current `origin/develop` checkout.
`git diff v0.3.4 4eecd41 -- src Cargo.toml Cargo.lock rust-toolchain.toml`
is empty; the release tag peels to
`426626298b604b26e67f04b1c033a1575d653b9d`. The intervening changes are
documentation only. Each old tag has its own detached worktree and target.
All three builds use `cargo build --release --locked`, without parser
substitutes or production code changes.

The worktree setup and builds, from the repository root, were:

```sh
git fetch origin develop --tags
git worktree add .claude/worktrees/p2-rollback -b worktree-p2-rollback origin/develop
git worktree add --detach .claude/worktrees/p2-old-v033 v0.3.3
git worktree add --detach .claude/worktrees/p2-old-v010 v0.1.0
(cd .claude/worktrees/p2-rollback && cargo build --release --locked)
(cd .claude/worktrees/p2-old-v033 && cargo build --release --locked)
(cd .claude/worktrees/p2-old-v010 && RUSTUP_TOOLCHAIN=1.99.1 /home/fujimogn/.cargo/bin/cargo build --release --locked)
```

Each checkout root was verified and `worktree-init` returned
`policy / verified / config_distribution_denied`, with no config distribution.

The 0.1.1 changelog identifies the endpoint-floor change, and
`git log -S 'invalid checkpoint endpoint floor' -- src/publish.rs` identifies
`37c23ca7533763d81a617df73ebfc05bec0359c4`. That commit is absent from
`v0.1.0` and present in `v0.1.1`. Thus 0.1.0 is the pre-endpoint-row release.
Its loader accepts only `pane` and `workspace` row kinds; runtime observations
below test the actual executable rather than relying on that inspection.

The 0.1.0 tag pins Rust 1.99.0, which was not installed. The commander
approved the installed Rust 1.99.1 patch release: the target of this check is
the tag's checkpoint-reading source, not compiler-patch equivalence. The
build command is
`RUSTUP_TOOLCHAIN=1.99.1 /home/fujimogn/.cargo/bin/cargo build --release --locked`.
The absolute rustup cargo path bypasses the mise shim, which initially tried
to install 1.99.0 despite `RUSTUP_TOOLCHAIN`; that attempt was interrupted.
No 1.99.0 build result is claimed.

## Isolation and procedure

The harness and raw evidence are retained at
`/tmp/herdr-lineup-rb-5r6a4m/` for commander verification. Build artifacts
stay in each worktree's `target`, never in `/tmp`.

1. Create a private HOME and XDG directories, empty provider homes, a local
   Git workspace, and a session named `lineup-rb-<harness-pid>`. Strip every
   inherited `HERDR_*` variable plus `ENV` and `BASH_ENV`. Use a private
   `HERDR_CONFIG_PATH`, disable update checks, and start `herdr --session
   <private-session> server`, as in `scripts/e2e-isolated.sh`. Do not attach
   any client to a user session.
2. Discover only `<private-home>/.config/herdr/sessions/<private-session>/herdr.sock`.
   Assert that its resolved path is inside the private root and that the
   session is not `main`. Create one workspace with `--no-focus` and report
   one synthetic idle Codex occupant using `pane report-agent`. No provider
   credentials or real agent sessions are used.
3. Bind a private Unix proxy at `<root>/api.sock`. Forward JSON request and
   response frames unchanged to the isolated real Herdr socket, recording
   method, parameters, sequence, and response. Keep the proxy path, device,
   and inode unchanged across every binary run. The proxy does not implement
   the checkpoint parser, sequence allocator, or metadata application rule.
4. Set `HERDR_SOCKET_PATH` to that proxy, `HERDR_PLUGIN_STATE_DIR` to the
   current case's private state, and `HERDR_PLUGIN_CONFIG_DIR` to private
   settings. Use `refresh_interval_ms = 1000`, `reader_timeout_ms = 100`,
   `allow_network = false`, and `logo_mode = "text"`. Run the actual 0.3.4
   `daemon` command, wait for successful metadata and a snapshot containing
   `lineup_logo = "Codex"`, run `doctor`, then use Lineup `stop` and wait for
   process exit. Preserve the resulting per-session `sequences.json`,
   including its endpoint row, without editing it.
5. For each old binary, copy the latest 0.3.4-produced seed bytes into a
   separate private state's same `session-<socket-key>/sequences.json`. Run
   the old binary's `daemon`. For 0.3.3, await an actual metadata frame and visible Codex token.
   For 0.1.0, await two logged refresh failures or process exit, require zero
   metadata frames, and require no `lineup_` tokens in the pane snapshot.
   Run that binary's `doctor`; compare checkpoint bytes before and after
   Lineup `stop`. Capture exit statuses and daemon logs.
6. In that same case state, run 0.3.4 again without deleting or restoring the
   checkpoint. Require successful metadata frames with sequences strictly
   above the preceding durable bound, a visible Codex token, and successful
   `doctor`. Stop only the test Lineup process. Use its resulting checkpoint
   as the next case's seed: the real host retains its accepted sequence history,
   so restoring an older seed between cases would invalidate the experiment.
   Finally stop and delete only the explicitly named private Herdr session; assert its directory is gone.

The foreground `daemon` command is exercised here; this is not a test of
detached `ensure` spawning, lock inheritance, or readiness races.

## Observed results

The final harness exited 0 on Linux x86_64 (kernel 7.3.3-1-cachyos), with
Herdr 0.9.5 and tmpfs plugin state. Its private session was
`lineup-rb-1077498`; cleanup confirmed that session directory was removed.

| Old release | 0.3.4 seed bound | Old binary result before stop | Return to 0.3.4 in the same case state |
| --- | --- | --- | --- |
| 0.3.3 | 32 | Reads checkpoint; pane/workspace frames use seq 33/34, Codex token appears, doctor exits 0; checkpoint advances to 64. | Pane/workspace frames use seq 65/66, Codex token appears, doctor exits 0; checkpoint advances to 96. |
| 0.1.0 | 96 | Two refresh failures, zero metadata frames, no Lineup pane tokens; doctor exits 1 with `daemon refresh is unhealthy`. Checkpoint remains byte-identical. | Without deleting or rewriting the rejected checkpoint, pane/workspace frames use seq 97/98, Codex token appears, doctor exits 0; checkpoint advances to 128. |

Both seeds contain a single genuine endpoint row with empty `id`, endpoint
`03eb3a3dd4e700f3:51:209142994`, and the bound above. They were written by
0.3.4, not hand-authored. In the 0.1.0 case, both the during-run and post-stop
checkpoint equal the seed byte for byte. That daemon remains alive despite
failed refreshes until Lineup `stop`; the stop command exits 0, while the
foreground daemon exits 1 and writes `invalid checkpoint resource kind` to
stderr during shutdown clear. Both 0.3.3 stop and daemon exit 0. Thus
"rejects" here means checkpoint load/refresh failure and no publication,
not necessarily immediate process termination.

The proxy records matching real-host result envelopes for successful reports.
Snapshot assertions additionally require the Codex token to appear: an `ok`
response by itself would not prove application. The table counts only frames
observed before each old-version stop, excluding shutdown-clear frames.
The 0.1.0 observation is bounded to two completed failed refreshes, not an
unbounded assertion that it can never publish under any conditions.

README's downgrade warning now explains that current releases write endpoint
rows when reserving sequence windows, including for the first normal
publication, rather than mentioning only absent-resource pruning. It records
the observed 0.1.0 rejection, 0.3.3 compatibility, and recovery by returning to
0.3.4 with the checkpoint intact. Compatibility with every older version is
not claimed.

### Evidence retained for commander verification

These are local raw artifacts, not committed source or a claim of permanent
artifact availability. Independent repository review checks the document
change; the commander verifies observations against these originals.

| Artifact under `/tmp/herdr-lineup-rb-5r6a4m/` | SHA-256 |
| --- | --- |
| `run.py` (complete harness) | `1efa42897087a7f2036c031310f4037863bf502dd9927d2dc61729ac1e81b8c0` |
| `results.json` (checkpoint, doctor, sequence, and token assertions) | `82132623b815a357db60315f4fbcd96e8f70e888be2860850e711272455cad70` |
| `wire.json` (forwarded request/response frames) | `e9e3acd76520147185edbf3d9356fff8015400c828456be003b7df10adf079e5` |
| `seed-033.json` | `0488712407eb0defdecc71d5419be80756956b4b09d50a6cafacc50ee6397c9d` |
| `seed-010.json` | `32647a6b800820be6e6bc68dceaee1c4d11451657a2b53919d95a949051b8a94` |

| Executable | SHA-256 |
| --- | --- |
| 0.3.4 | `88499a5ada7d0032b901869ec389d22bfda0382b9ce38d106d73acb7d2409d9f` |
| 0.3.3 | `3c2893deaa9428452e79d7edd28538c00dcde3d309d1bbec92d634f7420e6ae2` |
| 0.1.0 | `0d76cd94f12ed9abe09f5297c619e8dcdd93002509a82fab10d45d50bb22b890` |

Earlier harness attempts are retained separately and are not passing evidence:
`/tmp/herdr-lineup-rb-uFU6kI` timed out because it expected the newer structured
log format from 0.1.0; `/tmp/herdr-lineup-rb-xoMrmO` reused the original seed
on a host with later accepted sequences, so `ok` responses did not restore
tokens; `/tmp/herdr-lineup-rb-UuQojg` observed publication but called doctor
before the first refresh finished. The final harness recognizes the old log,
carries forward the latest 0.3.4 checkpoint, and waits for healthy doctor
completion within a 12-second bound. These are harness corrections, not
production fixes. The stale-seed attempt also demonstrates that the token
assertion catches response-success without application.

Build and required-check logs are under `/tmp/herdr-lineup-rb-uFU6kI/`.


## Unverified limits

Only these two old versions, this seed checkpoint, and a clean sequential
stop/start on Linux are exercised. This does not cover every historical
release, malformed checkpoints, arbitrary resource sets, concurrent writers,
power loss or kernel crash, macOS storage, or compiler-patch equivalence.
It does not establish rendered sidebar latency or authenticated usage data.
The checkpoint is retained throughout rollback and recovery; deleting it is
not a tested or safe recovery procedure.

## Candidate checks

The initial candidate's unchanged Rust source passed the required checks on Linux:

- `cargo fmt --all --check`: exit 0.
- `cargo clippy --locked --all-targets -- -D warnings`: exit 0.
- `cargo test --locked -- --test-threads=8`: exit 0; 462 unit tests passed,
  six existing tests ignored, and one integration test passed. Self-spawned
  unit-test child runs are additional output, not additional parent tests.
- `cargo build --release --locked`: exit 0.
- `target/release/herdr-lineup --help`: exit 0.

This change adds observation documentation, not an automated test. The old
binary rejection and compatible binary recovery are the runtime negative
and positive controls; no production mutation is left in any checkout.

After restacking the documentation onto integrated base
`eb696425d73037a216c9e2293f183917100635e3`, all five commands above were
rerun successfully. The new base adds the detached config-restart fixture:
463 unit tests and one integration test passed, with six existing tests
ignored. The source tree is identical to that base; both its detached-restart
coverage and this record's rollback coverage remain in the shared documents.
The original runtime observation still uses the fixed 0.3.4 source listed
above. Its measured executable was kept as
`target/release/herdr-lineup-observed-v034` in the candidate worktree until
that worktree was removed after shipping; its SHA-256 is listed in this record.
Restacked-check logs were written as `restacked-*.log` under
`/tmp/herdr-lineup-rb-5r6a4m/`, which is not durable storage.
