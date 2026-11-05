# T-05 connection and shutdown verification

Date: 2026-11-05 (Asia/Hong_Kong).
Acceptance uses software checks, as requested. No physical unplugging, serial HID transmission, or real capture session was performed.

## Behavior

- Startup stores device paths without opening either device.
- Connect requires both paths and starts video and serial workers.
- Disconnect immediately clears local keyboard, mouse, input-capture state, and displayed frames.
- Both workers receive cancellation before the UI begins polling completion.
- The UI remains responsive and disables Connect until both workers finish and their handles are joined.
- A partial connection failure takes the same cleanup path; manual Connect can retry afterward.
- Release input discards stale queued reports and enqueues one paired neutral-report request.
- Serial shutdown checks a separate cancellation flag and writes neutral reports directly, bypassing the input queue.
- Keyboard release failure never skips the mouse release attempt. Errors are retained in completion status.
- App exit requests the same cleanup and waits for both workers against one two-second deadline.

The capture worker uses a 500 ms V4L2 wait timeout and checks cancellation before and after frame acquisition.
A timeout terminates that stream; retrying `next()` on v4l 0.14's timed-out stream could requeue an undequeued buffer.
The serial port uses a 200 ms write timeout and a 200 ms packet deadline across partial/interrupted writes.
A single in-progress write may overrun the packet deadline by its configured write timeout.
Idle serial cancellation is checked every 20 ms.

## Software checks

Rust 1.97.1 and Cargo 1.97.1; dependencies remain pinned by Cargo.lock.

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

The cache path is local and temporary. A normal Cargo installation can fetch the locked dependencies instead.

Result: 15 tests pass, comprising the existing five protocol tests and ten lifecycle checks.

| Check | Evidence |
| --- | --- |
| Disconnected startup and repeated failed Connect | No handles at startup; missing-path attempts create and then close both workers; retry succeeds in starting fresh workers. |
| Manual Disconnect | Clears held keys/buttons, Super, capture state, readiness, and frame references. |
| Repeated Disconnect and early Connect | Repeated stop requests are harmless; no replacement worker starts before completion. |
| Either endpoint fails during partial connection | Mock video failure closes serial; serial open failure stops the mock camera. |
| App exit | Mock device resources close and both neutral packets are written before Viewer drop returns. |
| Saturated input queue | All 256 slots remain occupied while shutdown writes only the two neutral packets. |
| Release input ordering | Old queued inputs are removed; release preserves mouse coordinates and precedes fresh input. |
| Failed keyboard release | Mouse release is still attempted; cleanup returns an error instead of waiting for another report. |
| Repeated interrupted writes | A packet deadline stops indefinite retries. |
| Completion and stalled worker | Resource lifetime ends before successful join; an unresponsive mock reports timeout rather than false completion. |

The optimized release build passes. Existing unused-code warnings remain.
No dependency or manifest change was required.

## Limits and remaining work

Software tests verify mock resource destruction, cancellation, ordering, and failure paths; they do not prove hardware HID delivery.
A kernel call that ignores its timeout cannot be forcibly cancelled safely by a Rust thread.
During manual disconnect, a worker still running after two seconds remains tracked; status reports the delay and Connect stays disabled.
On app exit, the two-second deadline prevents an indefinite wait and reports unfinished cleanup to stderr.
In that exceptional case, process termination reclaims resources; delivery of neutral reports is not guaranteed.

T-06 subsequently adds automatic device pairing; see [DISCOVERY-VERIFICATION.md](DISCOVERY-VERIFICATION.md).
T-07 retains full unexpected-disconnection and recovery acceptance.
T-05 introduces shared cleanup on worker termination to handle partial connections, but does not close those later tasks.
Focus Mode, shortcut inhibition, and other input fixes remain in their existing tasks.
