# Kimi wiring macOS failure investigation

## Result and limits

A concrete macOS-specific fixture defect is identified and fixed: accepted
TCP streams retained the nonblocking mode inherited from the fixture listener.
The request reader treated `WouldBlock` as the end of the headers. It could
therefore reply before receiving the Authorization header, producing an
unintended 401, or attempt a response with an incomplete request.

This is the leading explanation for the historical failure, **not proof of
which assertion failed in that particular child**. The old parent discarded
the child's stdout and did not print its exit status. The missing diagnostic
cannot be recovered from the stored CI log. The historical failure remains
open pending a macOS observation with the improved diagnostics. No macOS run
was performed or workflow dispatched in this investigation.

Investigation base: `fe0e5aef9bbc7a569378824e2ce346dc3033c0b5`
(`origin/develop` when the dedicated worktree was created). Failed CI source:
`9de810b82c0c1a12da147369633ba9058d1bd9b7`, using Rust 1.99.1; the current
worktree uses its pinned Rust 1.100.0.

## Evidence read

### CI logs

The logs were fetched read-only with the following commands. Line numbers
below count the complete output of each individual `gh run view --log`
command, including the job, step, and timestamp prefixes.

```sh
gh run view 41847126534 --attempt 1 --json headSha,jobs,url,conclusion,createdAt
gh run view 41847126534 --attempt 1 --job 126633528332 --log
gh run view 41847126534 --attempt 2 --job 126636169441 --log
gh run view 41847126534 --attempt 1 --job 126633528136 --log
```

| Log | Evidence |
| --- | --- |
| [macOS attempt 1](https://github.com/fujimogn/herdr-lineup/actions/runs/41847126534/job/126633528332), lines 11–20 | macOS 26.6.2, build 25G83, `macos-26-arm64` runner image. |
| Same log, lines 160–190 | TMPDIR was resolved to `/private/var/folders/.../T` before the Rust commands. |
| Same log, lines 886–903 | The wiring test failed after 5.824 s. The outer harness reported a panic at `src/publish.rs:3932:13`, with `isolated Kimi wiring child failed:` and no child stderr. No child stdout or exit status appears in that message. |
| Same log, lines 1134–1137 | 454 tests ran: 453 passed and one failed, with five skipped. The nextest step exited 100. |
| [macOS attempt 2](https://github.com/fujimogn/herdr-lineup/actions/runs/41847126534/job/126636169441), lines 675, 844 | The same wiring test passed in 8.831 s; all 454 selected tests passed, with five skipped. |
| [Linux attempt 1](https://github.com/fujimogn/herdr-lineup/actions/runs/41847126534/job/126633528136), lines 1098–1100 | The wiring test passed in 8.784 s; all 456 selected tests passed, with six skipped. |

These results establish one observed macOS-only failure and successful
executions. They do not establish a failure rate or a macOS-only limitation of
the production quota client. The second attempt is historical evidence, not
a retry introduced by this change.

### Repository source

Both the failed SHA and the investigation base were read. Historical line
numbers in this table belong to the failed SHA; base line numbers belong to
the investigation base, before this patch.

| File / function | Evidence |
| --- | --- |
| `src/publish.rs`, `serve_kimi_fixture`, historical lines 2756–2821; base lines 2809–2874 | The listener is nonblocking. Accepted streams receive a two-second read timeout but no blocking-mode reset. Every read error breaks the header loop. A missing matching bearer token selects 401. Response write errors are ignored. |
| `src/publish.rs`, `kimi_production_wiring_covers_opt_in_success_rotation_expiry_and_reset`, historical lines 3874–4160; base lines 3927–4264 | The parent binds loopback before spawning one exact child test. It supplies an isolated HOME/Kimi home, a loopback endpoint and 100 ms polling; removes provider/base-URL/Herdr overrides; and uses no real Herdr session or account. |
| Same test, historical lines 3894–3936 | Both child output streams are piped, but only stderr is read after exit. The parent has a distinct 60-second timeout message. Libtest captures a failing test's panic output in its stdout report, so empty stderr is compatible with an ordinary assertion failure. It is not evidence of a signal or silent crash. |
| Same test, historical lines 3996–4009, 4032 onward | Individual phase waits are five seconds; clearing/expiry waits are eight seconds. An initial opt-out check sleeps 400 ms. The 5.824 s failure is compatible with a five-second phase wait, but does not identify which phase. |
| Same test, historical lines 4046–4060, 4114–4158 | Reply scripts use rename replacement; credential writes truncate in place. Reset and credential expiry are set roughly four seconds ahead. The test checks success, rotation, transient retention, reset-less clearing, reset clearing and credential expiry. |
| `src/publish.rs`, `kimi_worker_settings`, `MetadataPublisher::start_weekly`, `current_kimi_weekly_with_state`; `src/model.rs`, `WeeklyWorker::spawn` | The fixture uses a 100 ms worker cadence and two-second client timeout. Source changes invalidate samples. A publication/diagnostic race or worker delay can make a phase wait fail; there is no fixture-specific retry added here. |
| `src/quota_kimi.rs`, `Client::{from_test_endpoint,with_endpoint,poll_at,fetch}`, `source_state`, `read_credentials` | The test endpoint is restricted to loopback. Non-200 replies become fixed failure categories; 401 is unauthorized. Credentials and source metadata are rechecked, including size, timestamps and inode identity. |
| `openspec/specs/provider-readers/spec.md`, Kimi opt-in, bounded request and quota expiry requirements (lines 439–570); `README.md`; `herdr-plugin.toml` | Network quota requires both opt-ins; local context stays independent. Authentication/source failures clear quota, while transient retention requires a known future reset. The fix changes only the test fixture. |

`git diff 9de810b82c0c1a12da147369633ba9058d1bd9b7
fe0e5aef9bbc7a569378824e2ce346dc3033c0b5 -- src/publish.rs` confirms that
the listener/header handling defect still exists at the base. Commit
`9fdac8d` already added concurrent stdout/stderr draining and both streams to
the failure report; `bf789f1` added both streams and status to the timeout
report. Those diagnostic improvements are retained, not reimplemented.

### Platform mechanism

Apple's [accept manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/accept.2.html)
describes the accepted socket inheriting properties. More specifically,
[XNU accept implementation](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_syscalls.c#L587-L619)
copies the listener's file flags and applies `FNONBLOCK` to the accepted
socket's `SS_NBIO` state. This source was inspected on 2026-11-18; it is not a
measurement of the failed runner's kernel.

The [Rust 1.99.1 accept implementation](https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/net/connection/socket/unix.rs#L222-L252)
uses `accept` plus close-on-exec on Apple, without clearing nonblocking mode.
Its [TcpListener wrapper](https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/net/connection/socket/mod.rs#L561-L570)
does not normalize that mode either. Rust 1.100.0's Unix accept implementation
was also checked and has the same relevant behavior. Linux's
[accept manual](https://man7.org/linux/man-pages/man2/accept.2.html)
explicitly describes noninheritance of `O_NONBLOCK`, in contrast to BSD, and
advises explicitly setting required accepted-socket flags.

Thus the fixture's intended two-second blocking read is not obtained merely
by setting a read timeout. On macOS an early or fragmented request can hit
`WouldBlock`; accepting a TCP connection does not mean its HTTP headers have
arrived. The 10 ms listener poll and file writes before reading affect the
window, which explains why the defect can be intermittent. This is a causal
explanation from code and platform sources, not a recovered packet trace.

## Candidate causes, ranked

1. **Inherited nonblocking stream: confirmed defect, leading historical
   explanation.** It directly explains the OS asymmetry and premature
   unauthorized/transient replies. A deterministic Linux regression fails
   before normalization and passes after it. The old CI log cannot tie the
   particular child failure to one request or phase.
2. **Credential replacement/source observation race: possible, weaker.**
   Truncating credential writes can expose an empty/partial file or changed
   stamp to a concurrent poll. The source guards intentionally clear mixed
   observations. Later polls can recover after the write completes; no
   recorded credential diagnostic or macOS-specific evidence points here.
   This investigation does not change credential writing or source guards.
3. **Scheduling, filesystem or network delay exceeding a phase budget:
   possible, unproven.** A five-second phase failure fits the duration, but
   there is no child phase diagnostic or scheduler trace. The observed pass
   durations cannot distinguish ordinary load from the fixture defect. No
   timeout is enlarged to accommodate this hypothesis.
4. **Reset/expiry timing: possible but less supported.** The four-second
   windows can be missed under a long stall, and reset formatting truncates
   to seconds. No surviving log identifies those later phases. Passing
   executions take about nine seconds, but elapsed time alone cannot locate
   the failed phase. No clock or expiry contract is changed.
5. **Parent's 60-second deadline, stdout backpressure, temporary Unix socket
   length or real credentials: unsupported as this failure's explanation.**
   The parent reported child failure after 5.824 s, not its timeout branch.
   No large child output is emitted during normal execution to support pipe
   saturation. The fixture uses TCP, not a Unix socket, and its HOME,
   credential path and endpoint are synthetic. The log also confirms physical
   TMPDIR resolution. These observations do not rule out every OS failure or
   signal; the historical exit status was not printed.

## Fix and deterministic regression

`configure_kimi_fixture_stream` explicitly clears nonblocking mode before
using the existing two-second read timeout. `serve_kimi_fixture` uses this
setup for every accepted stream. Listener polling remains nonblocking.
All changes to Rust code are inside `#[cfg(test)]`; the production request
client and quota behavior are unchanged.

`kimi_fixture_clears_inherited_nonblocking_mode` uses a real loopback socket,
explicitly forces its accepted stream into nonblocking mode on every OS,
then invokes the same setup as the fixture. It checks the kernel's `F_GETFL`
result to prove `O_NONBLOCK` is cleared and the two-second timeout is retained.
There is no sleep, request-arrival race, retry, skip or external server. It
tests the normalization boundary; the existing wiring test supplies the
end-to-end quota assertions. Removing the blocking-mode reset makes the new
test fail on Linux even though Linux itself does not inherit this flag.

Red/green command:

```sh
cargo test --locked kimi_fixture_clears_inherited_nonblocking_mode -- --exact publish::tests::kimi_fixture_clears_inherited_nonblocking_mode
```

- Before the fix: exit 101, one failed test, `O_NONBLOCK` mask 2048 instead
  of 0, completed in 0.00 s after compilation.
- After the fix: exit 0, one passed test, completed in 0.00 s after compilation.
- The original wiring test's assertions and all its deadlines remain intact.

Linux validation on the candidate code (all commands exited 0):

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed. |
| `cargo test --locked` | 466 library tests and one integration test passed; six library tests ignored; zero failures. The original wiring test passed in this full run. |
| `cargo build --release --locked` | Passed. |
| `target/release/herdr-lineup --help` | Passed; printed the CLI usage. |

The existing ignored tests were not changed or used to qualify the fix.
No Herdr server was stopped, restarted, quit or attached to a user session.

## Remaining macOS verification

Run the unchanged wiring test and the new normalization regression on an
authorized macOS host or the next authorized platform check. The new test
must also pass there; a Linux forced-mode test does not prove a macOS run.
If wiring still fails, retain the current parent's child status, stdout and
stderr, then identify the failed phase and fixed diagnostic before selecting
another fix. The historical child output/status and the runner's packet,
scheduler and filesystem state remain unavailable. No claim is made that
the original CI failure has been conclusively reproduced or eliminated.
