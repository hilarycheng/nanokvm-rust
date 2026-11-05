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
T-07 software evidence follows below; Focus Mode inhibition integration remains open under T-15.
T-05 introduces shared cleanup on worker termination to handle partial connections, but does not close those later tasks.
Focus Mode, shortcut inhibition, and other input fixes remain in their existing tasks.

## T-07 unexpected disconnection and manual recovery

Date: 2026-11-05 (Asia/Hong_Kong).

### Detection and cleanup

- Video open, configuration, stream, and frame-read errors terminate the capture worker.
- A V4L2 frame wait exceeding 500 ms terminates the stream. The error reports this configured limit.
  The stream is not retried because v4l 0.14 may requeue an undequeued buffer after a timeout.
- Serial open and report-write errors terminate the serial worker. Writes retain the 200 ms timeout and packet deadline.
- The serial loop now checks `bytes_to_read()` before each queue wait or report, including while idle.
  On Linux, serialport 4.10.1 uses FIONREAD. A hung-up tty returns an error without a probe HID packet.
  Idle queue waits last 20 ms; scheduling and ongoing writes can delay the next check.
- Worker errors remain in the completion result, independently of bounded status queues.
- Either worker finishing triggers shared cleanup. Local held keys, modifiers, buttons, capture state, and frame references clear immediately.
- Both workers receive cancellation. Serial cleanup attempts keyboard and mouse neutral reports, even after a probe or write failure.
  A failed keyboard release does not skip mouse release. Status retains the original error and release errors.
- Completion is polled without blocking the UI. A worker still running after two seconds remains tracked and prevents Connect.
- Cleanup never reconnects automatically. Manual Connect starts both workers again; automatic device mode performs fresh discovery first.
  Input capture stays off after reconnect until the user enables it.

### Software evidence

The same locked offline test command above passes **31 tests**, including four new checks:

| Check | Evidence |
| --- | --- |
| Video loss or timeout after held input | A mock video worker fails after keyboard and mouse reports reach a recording writer. Cleanup clears state, closes both resources, writes both neutral packets, preserves the video cause, and permits a subsequent manual Connect. Video read errors are injected; the V4L2 driver timeout itself is not exercised. |
| Idle serial hangup and recovery | Linux pseudo-terminals use production serial open, probe, and write paths. Closing the master while local inputs remain held triggers cleanup without queued input. Failed release writes are reported. A second manual Connect opens a fresh tty successfully; video is mocked. |
| Input write failure | A failing writer records the input attempt and both neutral-report attempts. Completion retains the write and release failures. |
| Unresponsive peer | Serial failure initiates cleanup while a mock video worker ignores cancellation. UI polling reports delayed shutdown, retains the serial cause, and blocks reconnect until the mock is released. |

Existing checks cover full queues, interrupted writes, failed keyboard release, partial opens, resource destruction,
discovery after changed device numbers, and application exit. No dependency or configuration change was required.
The offline release build also passes and updates the root executable. Existing unused-code warnings remain.

### Acceptance limits

FIONREAD checks local tty availability, not remote HID delivery or remote host responsiveness.
A successful write only means the local driver accepted bytes. The application does not claim remote release acknowledgement.
Video frames containing an unchanged image are not considered a stall. MJPEG decode errors remain recoverable status messages.
Kernel calls that ignore their timeout cannot be forcibly cancelled; the T-05 shutdown limitation still applies.
The reusable GPU renderer remains allocated while the window exists; cleanup closes device workers and drops CPU frame references.

Physical unplugging remains waived. Real capture, GPU rendering, and hardware HID delivery were **NOT RUN**.
Focus Mode inhibition does not exist yet. T-07 remains unchecked until T-15 implements and verifies its release on this cleanup path.

## T-09 input ownership and focus release

Date: 2026-11-05 (Asia/Hong_Kong).

### Behavior

- A fresh video press or Capture input enables forwarding and gives keyboard focus to the video widget.
- Keyboard forwarding requires video focus. A local pointer press surrenders that focus before the batch reaches local widgets.
- Local widget focus releases held keyboard and mouse reports at the last remote pointer position.
  The on-screen Super toggle retains its selected state during local interaction, but its HID modifier is released.
  Returning keyboard focus to video reapplies that toggle. Release input, window focus loss, and disconnect clear it completely.
- Forwarded batches remove Key, Text, clipboard, and IME events from egui input and clear local modifiers.
  Local text input remains available when a local widget owns focus. T-08 still owns clipboard-to-HID and key-mapping fixes.
- Pointer presses require an unobscured video hit. Unowned releases are ignored; a video-owned release outside the image uses the last remote position.
- Local widget drags do not become remote drags when they cross the video. Wheel routing follows pointer-event order.
- Throttled movement retains its final pointer location and sends it on a later frame, even without another pointer event.
- Window focus loss clears input before keyboard forwarding. Releasing capture during a drag requires a fresh video press to capture again.

### Software evidence

The same locked offline test command above passes **39 tests**, including eight new egui regression checks:

| Check | Evidence |
| --- | --- |
| Local and remote keyboard ownership | A real egui TextEdit receives local text while no HID report is queued. With video focus, a key report is queued and text, clipboard, and IME events do not edit the local field. Escape release does not leak the remaining text batch. |
| Local clicks and held input | Outside-image press/release queues no pointer movement. With held input, it queues a paired neutral release at the last remote coordinates. Super remains selected without repeated release reports. |
| Video press and outside release | A single press enables forwarding and takes video focus. An outside release clears only the owned button and preserves the last remote position. |
| Local drag across video | Dragging a local TextEdit selection across video queues no remote input. |
| Explicit release during drag | Escape ends capture while a button remains physically held. Subsequent motion and release cannot recapture input. |
| Wheel event ordering | A local wheel event followed by movement into video is not forwarded. A video wheel event followed by movement outside is forwarded. |
| Window focus loss | A batch containing a key and focus loss queues only neutral release, clearing keys, buttons, capture, and Super. |
| Final throttled motion | After throttling expires, a frame without pointer events sends the last pending position. |

Tests run the production input hook and video interaction through `egui::Context::run` with a local TextEdit fixture.
A deterministic serial queue records reports; no device or GPU render pass is needed.
No dependency, configuration, or key-mapping change was required.
The offline release build passes with existing unused-code warnings. The root executable matches the release output and remains executable.

### Limits and later integration

These checks verify routing and queued releases, not hardware delivery. Sway, GPU rendering, and NanoKVM input were **NOT RUN**.
The T-08 physical-key, clipboard shortcut, and modifier-order defects remain open.
T-15 owns explicit Focus Mode, Non-Focus Mode, shortcut inhibition, and replacing Escape release.
T-18 owns screenshot selection and must suspend forwarding while its overlay is active.
