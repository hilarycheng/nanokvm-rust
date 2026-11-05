# T-15 input modes and shortcut inhibition verification

Historical T-15 record. The approved Capture/Non-Capture naming and physical-modifier update supersede its input behavior.
See [current input verification](KEYBOARD-VERIFICATION.md#capture-and-physical-modifier-update).

Date: 2026-11-05 (Asia/Hong_Kong).

The user approved T-15 and selected simultaneous Right-Ctrl + Right-Alt for local release.
The user also prioritized software completion and deferred live hardware acceptance.
This record closes T-15 software delivery and the dependent T-07/T-10 software integration gates.
It does not claim Sway, NanoKVM HID, or live capture acceptance.

## Behavior

- Startup and reconnection use Non-Focus Mode with remote forwarding initially disabled.
- Non-Focus Mode uses video/local keyboard ownership and video-owned pointer gestures. Desktop shortcuts remain available.
- The Focus Mode button requests Wayland shortcut inhibition on the existing viewer surface.
- Forwarding stays disabled while activation is pending. The compositor must acknowledge activation before Focus Mode becomes active.
- The pending request times out after two seconds. Unsupported, failed, or revoked inhibition returns to Non-Focus Mode with status.
- Hold Right-Ctrl + Right-Alt together, in either order, to release input locally. A non-overlapping sequence does not trigger release.
- Left modifiers and aggregate Ctrl/Alt state cannot trigger the chord. Escape forwards normally in both modes.
- Completing the chord clears held keys, buttons, mapped F12/Meta, and the Super toggle, then destroys inhibition.
- Both chord keys must be released before capture can start again. A release batch cannot immediately recapture video input.
- Release input and Non-Focus Mode also clear held state and inhibition. A fresh video click resumes normal Non-Focus forwarding.
- Local controls leave Focus Mode. Screenshot PNG/WebP retain Focus Mode, keyboard ownership, and held keys.
- Window focus loss, either device worker ending, disconnect, and application exit destroy inhibition through shared cleanup.
- Connect remains manual. No mode automatically restores inhibition after cleanup or revocation.

The first chord modifier can reach the remote host before the second key arrives.
Chord completion requests neutral HID reports; failed serial writes still cannot guarantee remote release.
F12 mapping remains the keyboard path to remote Meta. This change does not add physical Meta forwarding.

## Implementation and dependencies

src/shortcuts.rs uses the standard keyboard-shortcuts-inhibit protocol on eframe's existing Wayland connection and wl_surface.
It creates one inhibitor per advertised seat, waits for activation, and never creates duplicate inhibitors for an existing request.
Seat/manager removal, seat additions during inhibition, and compositor revocation fail closed.
Late inhibitor events cannot reactivate a released request.

Queue dispatch and flush do not perform blocking round trips. Winit remains responsible for reading the shared display connection.
The controller borrows eframe's display/surface. It never closes that display or destroys that surface.
The unsafe boundary requires those handles to outlive the controller.
The pinned Linux eframe implementation keeps the root window alive through Viewer drop; on_exit explicitly drops the controller earlier.
Recheck this lifetime and the native input patch before upgrading eframe/winit.

The vendored egui-winit patch now publishes physical Right-Control/Right-Alt events in the native keyboard batch.
Its synthetic-press filtering and local clipboard translation remain unchanged.
The app tracks the release gesture before forwarding a batch and resets physical tracking on window focus loss.

Direct dependencies now include raw-window-handle, wayland-client with its system backend, and wayland-protocols with client/unstable support.
All three already existed in Cargo.lock. No package versions changed and no new package was introduced.
AGENTS.md and DESIGN.md remain unchanged.

Protocol reference: the [Wayland protocol definition](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/unstable/keyboard-shortcuts-inhibit/keyboard-shortcuts-inhibit-unstable-v1.xml).
The compositor can retain emergency shortcuts and can refuse or revoke inhibition.
Focus Mode therefore requires native Wayland support and reports actual acknowledgement, not merely a successful request write.

## Commands and results

Environment: Linux x86_64, Rust 1.97.1, Cargo 1.97.1, locked cached dependencies.
Commands run from the repository root:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline -p egui-winit --lib
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
cmp target/release/nanokvm-rust nanokvm-rust
test -x nanokvm-rust
./nanokvm-rust --help
./nanokvm-rust -h
git diff --check
```

Application result: **62 passed, zero failed, one ignored GPU test**. Backend result: **three passed, zero failed**.
Seven new app/protocol tests and one new backend test supplement the existing suite.
Existing screenshot tests now exercise both modes with acknowledged synthetic inhibition.
Old Escape-release assertions now check Escape forwarding or the agreed right-side chord.

The initial sandboxed run could not write to the simulated Wayland peer socket: operation not permitted.
The successful app run used host execution with DISPLAY and WAYLAND_DISPLAY unset.
The fixture uses a private Unix socket pair and minimal protocol messages; it never connects to Sway or any real device.
The tests assert actual inhibit/destroy wire requests and dispatch actual protocol activation/revocation events.

The release build succeeds. The root executable matches target/release, remains executable, and passes both help options.
Three existing unused-code warnings remain in the test build; seven remain in the release build.
The unchanged GPU rendering check is not rerun; T-11/T-14 retain its earlier successful RX 6600 evidence.

| Check | Result |
| --- | --- |
| Surface and guest connection | Inhibit request names the fixture's original surface. Repeated requests do not duplicate inhibitors; dropping the controller preserves the host connection. |
| Activation and failure | No Focus forwarding before acknowledgement. Unsupported protocol, timeout, revocation, and seat/manager changes never claim active capture. |
| Reserved release gesture | Both press orders release held input. Non-overlapping sequences and aggregate/left modifiers do not. Both keys must come up before recapture. |
| Toolbar modes | Real egui button events enter pending Focus Mode, accept activation, and return to Non-Focus Mode with held state cleared. |
| Pending cancellation | Local controls and disconnect destroy pending inhibition before activation. Unavailable native support preserves Non-Focus Mode. |
| Escape and Meta | Escape reaches HID reports while mapped Meta remains held. Mode exits clear mapped Meta and the independent Super toggle. |
| Cleanup | Local interaction, release chord, focus loss, disconnect, either worker ending, revocation, on_exit, and Drop destroy inhibition. |
| Screenshots | Both formats preserve Focus/Non-Focus ownership and held keyboard state. Saved dimensions/pixels remain correct before GPU tuning. |
| Existing input/lifecycle checks | Clipboard shortcuts, modifier ordering, local text, pointer ownership, queue saturation, failed release writes, and manual recovery still pass. |

## Limits and next work

Native Wayland is required for Focus Mode; X11/XWayland retains Non-Focus Mode only.
After protocol failure or compositor revocation, this implementation requires restarting the viewer before requesting Focus Mode again.
It does not override compositor policy or automatically reactivate a revoked inhibitor.

Live Sway shortcut routing, compositor presentation, NanoKVM HID delivery, and capture-device behavior were NOT RUN, as requested.
The protocol fixture verifies requests and client state, not compositor policy or real input devices.
T-16 owns the final pinned/auto-hide toolbar. T-17 owns V4L2 controls; T-18 owns area selection and guarded mouse resumption.
T-13 remains deferred. T-14 final software integration remains open for those remaining features.
