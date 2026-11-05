# T-16/T-17/T-18 software integration verification

Date: 2026-11-05 (Asia/Hong_Kong).
Source baseline: `144a994`, followed by the changes accompanying this record.
The user approved T-16 through T-21 and requested a commit after completion.
TASKS.md defines only T-01 through T-18. No T-19/T-20/T-21 requirements were supplied or invented.
Live hardware acceptance and T-13 benchmarking remain deferred at the user's request.

## Delivered behavior

T-16 now includes Area PNG, Area WebP, and Cancel selection alongside full-video screenshots.
T-17 provides separate framed GPU and V4L2 panels within Color.
T-18 freezes the selected video frame, isolates selection mouse gestures, and exports native cropped pixels.
T-11's combined-panel software checks are complete. T-14's functional software integration passes.
Final supplementary acceptance remains dependent on paused T-13; no RAM improvement is claimed.

### Device controls

The capture worker owns all V4L2 enumeration, reads, validation, writes, and readback.
The UI sends one bounded pending request and displays the confirmed reply or failure.
Connect reads controls; Refresh retrieves current values and availability, including external changes.
The panel lists standard brightness, contrast, saturation, hue, white balance, gamma, gain, exposure, and anti-flicker controls when exposed.
Integer, Boolean, and sparse Menu controls have suitable editors. Numeric edits align with the reported step.
Range, step, default, and unavailable status remain visible. Unsupported types and unsafe flags disable editing.
The worker rechecks current descriptors before writing, because automatic controls can change manual availability.
Readback updates related controls after successful or failed writes. Errors do not become optimistic UI values.
Disconnect clears panel values and pending state; late replies cannot restore them.
A driver call already in progress remains subject to driver behavior and existing shutdown limits.

GPU settings, Reset, and saved presets remain independent of device controls.
Both gamma values can remain active; device processing precedes GPU processing.
Screenshots include processing already present in captured frames and exclude GPU adjustments, as previously agreed.
No dependency, shader, preset format, or native keyboard-backend change was required.

Control validation follows the Linux [query-control interface](https://www.kernel.org/doc/html/latest/userspace-api/media/v4l/vidioc-queryctrl.html).
Actual device support and resulting colors remain hardware acceptance items.

### Area capture

An area action retains the current decoded frame and releases held remote mouse buttons.
Live frame reception continues; the displayed selection frame and export snapshot stay fixed.
Selection starts inside the fitted video rectangle. Drag direction can be reversed; coordinates clamp to image edges.
Normalized endpoints preserve the selected image coordinates across display scaling and window resizing.
Cropping rounds outward to native pixel boundaries, validates buffer lengths and bounds, and runs in the export worker.
PNG and lossless WebP share the existing unique-file and no-overwrite export path.

Selection pointer movements, buttons, and wheel events do not reach the remote host.
Keyboard ownership remains unchanged during selection/export, including held remote keys.
Saving, Cancel selection, an empty selection, or an export error resumes mouse forwarding only in active Focus Mode.
Non-Focus Mode requires explicit reacquisition through a fresh video click or Capture input.
Mode exit, window focus loss, and disconnect clear selection without restoring old input ownership.
An export already running can finish after release/disconnect, but its completion cannot recapture input.
Full-video actions remain disabled during selection or another export.

## Commands and results

Environment: Linux x86_64, Rust 1.97.1, Cargo 1.97.1, existing locked dependencies.
Commands run from the repository root:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
cmp target/release/nanokvm-rust nanokvm-rust
test -x nanokvm-rust
./nanokvm-rust --help
./nanokvm-rust -h
git diff --check
```

Application result: **71 passed, zero failed, one ignored GPU test**.
Release build succeeds. The root executable matches target/release/nanokvm-rust; both CLI help flags pass.
Diff whitespace, documentation links, and the 18 unique task IDs with 16 delivered tasks pass validation.
The build retains seven existing unused-code warnings; the test build retains three.
Six added tests cover control validation/readback, combined panels, crop safety, toolbar area export, and selection lifecycle.
The suite uses fake devices, queued reports, pseudo-terminals, and private simulated Wayland sockets.
Display environment variables are unset; tests do not connect to the desktop compositor or NanoKVM.

| Check | Evidence |
| --- | --- |
| V4L2 validation | Minimum-relative steps, bounds, sparse menus, Boolean values, unsupported types, and unavailable flags reject invalid writes. |
| Fresh device state | Changed availability, missing controls, write errors, and read errors return confirmed values or clear errors. |
| Independent panels | Both headings render. Device replies preserve GPU settings. A real GPU Reset click changes no device value or request queue. |
| Edit lifecycle | A second request cannot bypass a pending edit. Disconnect clears state and ignores late replies. |
| Crop correctness | Reverse/fractional/clamped coordinates, empty selections, invalid buffers, overflow, and crop bounds have direct checks. |
| Actual exports | Both area toolbar actions save files in both modes. Decoded 4×2 pixel crops match the frozen 8×4 source after the live image changes. |
| Resize and letterboxing | The WebP case resizes from 1400×800 to 700×1000 during selection before dragging. The crop remains correct. |
| Input isolation | Held mouse buttons become neutral; selection movement/wheel queues no remote reports. Held keyboard state survives active Focus Mode. |
| Cancel and errors | Real Cancel button clicks, empty selection, missing HOME, and asynchronous write failure obey conditional resumption. |
| Exit during work | Release, focus loss, and disconnect cancel selection. Release/disconnect during export never restore capture on completion. |
| Toolbar regression | Pin, auto-hide, reveal, resizing, fullscreen commands, unavailable actions, and full-video screenshots still pass. |
| Existing integration | Discovery, connection cleanup, manual recovery, input mapping, inhibition, preset validation, and file safety still pass. |

The patched native input backend is unchanged; its three passing tests in MODE-VERIFICATION.md remain applicable.
The shader and GPU processing are unchanged; earlier RX 6600 evidence in COLOR-VERIFICATION.md remains applicable.
The GPU test is deliberately not rerun in this software-only pass.

## Deferred acceptance

Live Sway shortcut capture, NanoKVM HID delivery, actual V4L2 control changes, natural-color calibration, and live screenshot comparison: **NOT RUN**.
T-13 process-tree RAM comparison: **NOT RUN**, paused by the user.
Software tests establish local routing and state transitions, not remote acknowledgement or kernel-driver timeout compliance.
AGENTS.md and DESIGN.md remain unchanged. README.md, SCOPE.md, and TASKS.md reflect the delivered software and remaining acceptance.
