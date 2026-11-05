# NanoKVM USB — Rust Viewer

A native Linux NanoKVM USB viewer built with Rust, egui/eframe, and wgpu.
It provides MJPEG video, serial keyboard/mouse forwarding, GPU color controls, and full-video screenshots.
Integration acceptance remains open; see [the acceptance record](INTEGRATION-VERIFICATION.md) and [task status](TASKS.md).

The intended environment is Ubuntu with Sway → NanoKVM USB → Arch Linux with GNOME.
Physical Meta capture, explicit input modes, the final toolbar, V4L2 image controls, and area screenshots remain pending.
Lower RAM usage than the original application has not been measured. T-13 is deferred at the user's request.

## Build and start

Install a Rust toolchain and the native development libraries required by eframe/wgpu and V4L2.
The runtime needs a working graphics backend and permission to access the video and serial devices.
[Build verification](BUILD-VERIFICATION.md) records the tested toolchain and system libraries.
Cargo.lock pins dependencies; the repository includes a patched egui-winit 0.31.1 for native keyboard events.

```sh
./build.sh
./nanokvm-rust
```

The build script copies the release executable to the project root. Build artifacts remain in target/.
Check the build exit status: a failed build can leave an older root executable in place.
Alternatively:

```sh
cargo run --release
# Manual device paths:
cargo run --release -- --video /dev/video0 --serial /dev/ttyUSB0
```

Use `./nanokvm-rust --help` for CLI usage.

## Connect and disconnect

The app starts disconnected. Automatic devices scans for the supported NanoKVM USB layout without opening a capture session.
Select Connect to rescan and start video plus serial. Rescan alone never connects.
The viewer requests 1920×1080 MJPG and uses the negotiated dimensions. Serial reports use 57,600 baud.

Automatic pairing supports the [recorded USB identity and topology](DEVICE-IDENTIFICATION.md).
For ambiguous candidates or unsupported layouts, disable Automatic devices and set both Video and Serial in the status bar.
CLI device paths also select manual mode. Device paths can change after reconnection; verify them before connecting manually.

Disconnect clears local input and video, stops both workers, and attempts neutral keyboard and mouse reports before serial closes.
Either worker failing initiates the same cleanup. The app never reconnects automatically.
After both workers finish, select Connect to retry. Input capture starts disabled after reconnection.
App exit requests the same cleanup and waits against a shared two-second deadline.

A worker that ignores cancellation keeps Connect unavailable. The status bar reports delayed shutdown after two seconds.
Failed serial writes cannot guarantee remote key release. Successful local writes do not acknowledge remote HID delivery.

## Keyboard and mouse

Once serial is ready and video is available, select Capture input or click the image.
Keyboard reports follow video focus. Clicking local controls releases held remote input and keeps local typing local.
Pointer presses belong to video only when they start there. Releasing a video drag outside releases its remote button.
Window focus loss clears input capture and held state.

With video keyboard focus, Escape releases input. Release input also ends forwarding while keeping the devices connected.
Click video or select Capture input to resume. Escape forwarding and the Right-Ctrl, Right-Alt release shortcut await T-15.
Explicit Focus Mode, Non-Focus Mode, and Sway shortcut inhibition are not implemented.
Host shortcuts can therefore intercept keys intended for the remote machine.

Enable F12 → Meta to use held F12 as remote Meta while video has keyboard focus.
Release F12 to release its Meta hold. The Super toggle independently controls a Meta hold.
Local interactions release the HID modifier; returning to video reapplies a selected Super toggle.
Release input, focus loss, and disconnect clear that toggle and mapped holds.

F12 mapping defaults off. Disable it to send ordinary F12; changing the mapping releases held remote input.
The mapping preference lasts for the application session, including disconnect/reconnect.
Ctrl+C/V/X, physical-key mapping, modifier ordering, and local input isolation have software regression coverage.
Sway and NanoKVM HID delivery remain unverified.

Fullscreen and Windowed change the viewer window. The current fixed toolbar has no pinning or auto-hide.
Opening Color releases input capture. Resume explicitly when finished adjusting colors.

## Screenshots

Use Screenshot PNG for the primary format or Screenshot WebP for lossless WebP.
Both save the complete decoded remote video at its native resolution, before GPU tuning.
Saved images exclude viewer controls, borders, and the local desktop.
Device-side processing already present in the captured frame remains included.

Files go to ~/Pictures/NanoKVM with timestamp-based names. Existing files are never overwritten.
The toolbar reports the saved path or failure. One export runs at a time without blocking the UI.
Screenshot clicks preserve existing video keyboard ownership and held keys; local keyboard ownership stays local.
Disconnect does not change an already requested snapshot.

App exit requests cancellation within the shared shutdown deadline.
Forced termination during a write can leave a partial new image; existing images remain protected.
Area capture is not implemented. Explicit mode and final toolbar acceptance await T-15, T-16, and T-18.

## GPU color controls

Open Color for Exposure, Gamma, Highlights, White peak, and Saturation. Hover over each control for guidance.
GPU adjustments follow V4L2 device processing; both gamma controls can be active together.
The separate V4L2 image-control panel is not implemented.

Reset 1:1 restores neutral GPU settings only. Soft white resets other controls and sets white peak to 0.84.
Neither action changes the saved preset until Save preset is selected.
GPU presets affect the displayed video, not device settings, desktop colors, or exported screenshots.

Save preset writes to:

- `$XDG_CONFIG_HOME/nanokvm-rust/colors.json` when XDG_CONFIG_HOME is absolute.
- `~/.config/nanokvm-rust/colors.json` otherwise, when HOME is absolute.

Startup restores valid preset values. Missing, invalid, or unreadable presets use defaults with a status message.
Saving writes and syncs a new temporary file before replacing the preset. Reported failures preserve the previous preset.
Unavailable configuration paths produce an error. Concurrent saves use the last successful replacement.
[Color verification](COLOR-VERIFICATION.md) records ranges, persistence checks, and RX 6600 offscreen rendering results.
Natural-color acceptance still needs NanoKVM reference images and combined V4L2/GPU checks.

## Troubleshooting and validation

| Symptom | Action or limitation |
| --- | --- |
| No automatic pair | Check device permissions and the supported identity profile; use verified manual paths for unsupported layouts. |
| Video or serial stops | Read the status, resolve the device problem, and reconnect manually after cleanup. |
| Video wait timeout | A frame wait exceeding 500 ms ends the stream; the viewer does not retry that stream automatically. |
| Connect stays disabled | Wait for both workers to stop. A driver that ignores timeouts may delay closure. |
| Remote input does not arrive | Confirm video focus and capture state; Sway may intercept shortcuts until T-15. |
| Screenshot or preset fails | Read the displayed cause and check the destination permissions and absolute HOME/XDG paths. |

Run the application and patched input-backend tests:

```sh
cargo test --release --locked --all-targets
# Requires an accessible GPU:
cargo test --release --locked --all-targets -- --include-ignored --nocapture
# Avoid connecting the backend test to the desktop clipboard:
env -u DISPLAY -u WAYLAND_DISPLAY cargo test --release --locked -p egui-winit --lib
```

The [integration record](INTEGRATION-VERIFICATION.md) maps every design requirement to evidence and remaining acceptance.
It distinguishes automated checks from NanoKVM and Sway hardware checks.
Recheck the pinned egui-winit patch before upgrading egui/eframe.
[Scope](SCOPE.md) preserves the original requirements and audit findings with their current status.
[AGENTS.md](AGENTS.md) defines project rules and the explicit approval requirement for code changes.

## Source map

| File | Responsibility |
| --- | --- |
| [main.rs](src/main.rs) | CLI and application startup |
| [app.rs](src/app.rs) | GUI, input ownership, connection lifecycle, and settings |
| [discovery.rs](src/discovery.rs) | Supported USB identity and topology matching |
| [capture.rs](src/capture.rs) | V4L2 MJPEG capture and decoding |
| [screenshot.rs](src/screenshot.rs) | Native-frame PNG/WebP export and exclusive file creation |
| [gpu.rs](src/gpu.rs), [color.wgsl](src/color.wgsl) | Texture upload, rendering, and GPU color processing |
| [input.rs](src/input.rs) | Keyboard mapping, modifiers, and image coordinates |
| [protocol.rs](src/protocol.rs) | Serial framing and HID reports |
| [serial.rs](src/serial.rs) | Serial worker and report queue |
| [worker.rs](src/worker.rs) | Shared cancellation and completion tracking |
| [vendor/egui-winit](vendor/egui-winit) | Native keyboard events before local clipboard translation |

## Protocol reference and license

Protocol work references `sipeed/NanoKVM-USB@1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc`:
[proto.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/proto.ts),
[device/index.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/index.ts),
and renderer keyboard/mouse/media handling.
Cargo.toml declares GPL-3.0-only. See the [upstream license](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/LICENSE).
