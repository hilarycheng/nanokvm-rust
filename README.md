# NanoKVM USB — Rust Viewer

A native Linux NanoKVM USB viewer built with Rust, egui/eframe, and wgpu.
It provides MJPEG video, serial keyboard/mouse forwarding, device and GPU color controls, and full-video or selected-area screenshots.
Software integration checks pass; see [current verification](CONTROLS-AREA-VERIFICATION.md) and [task status](TASKS.md).
Live hardware acceptance is deferred at the user's request.

The intended environment is Ubuntu with Sway → NanoKVM USB → Arch Linux with GNOME.
Focus Mode, Non-Focus Mode, and a pinned/auto-hide toolbar are available.
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

Non-Focus Mode uses normal video/local input ownership and leaves desktop shortcuts available.
Click video or select Capture input to forward input without inhibiting desktop shortcuts.

Select Focus Mode to request Wayland shortcut inhibition for the viewer window.
The app waits for compositor acknowledgement before forwarding in Focus Mode. It reports unavailable or rejected requests.
Focus Mode requires a native Wayland window and compositor support; it is unavailable under X11/XWayland.
The compositor can retain emergency shortcuts or reject inhibition through its policy.

Hold Right-Ctrl + Right-Alt together, in either order, to release input and return to Non-Focus Mode.
Release both keys before capturing again. Left-side Ctrl/Alt combinations do not trigger this shortcut.
The first modifier can reach the remote host before the chord completes; release then requests neutral reports.
Escape forwards as a normal remote key in both modes.

Release input and the Non-Focus Mode button also clear held input while keeping devices connected.
Click video to resume normal forwarding; select Focus Mode explicitly to inhibit shortcuts again.
Local controls leave Focus Mode and release held input. Screenshot actions preserve Focus Mode and keyboard ownership.
Focus loss, disconnect, worker failure, and exit release inhibition. Compositor revocation also returns to Non-Focus Mode.
After a protocol failure or revocation, restart the viewer before requesting Focus Mode again; Non-Focus Mode remains available.

Enable F12 → Meta to use held F12 as remote Meta while video has keyboard focus.
Release F12 to release its Meta hold. The Super toggle independently controls a Meta hold.
Local interactions release the HID modifier; returning to video reapplies a selected Super toggle.
Release input, focus loss, and disconnect clear that toggle and mapped holds.

F12 mapping defaults off. Disable it to send ordinary F12; changing the mapping releases held remote input.
The mapping preference lasts for the application session, including disconnect/reconnect.
Ctrl+C/V/X, physical-key mapping, modifier ordering, and local input isolation have software regression coverage.
Simulated Wayland protocol tests cover inhibition and cleanup. Live Sway/NanoKVM acceptance remains deferred.

The centered connection bar overlays the video and starts pinned. Pin toolbar toggles pinning for the current application session.
Unpinning enables auto-hide after 0.7 seconds outside the bar. A six-point strip remains at the top edge.
Move the pointer into that strip to reveal the bar. Pointer interaction and keyboard focus keep it visible.
Showing or hiding the bar does not resize the remote image. Controls wrap within smaller windows.
Fullscreen and Windowed change the viewer window; the same reveal strip remains available.
Mode, connection, and screenshot status stay visible below the video even when the bar hides.
Screenshot actions are disabled without a frame or while an export is running.
Area PNG and Area WebP start selection. Cancel selection stops an active selection.
Opening Color releases input capture. Resume explicitly when finished adjusting colors.

## Screenshots

Use Screenshot PNG for the primary format or Screenshot WebP for lossless WebP.
Both save the complete decoded remote video at its native resolution, before GPU tuning.
Saved images exclude viewer controls, borders, and the local desktop.
Device-side processing already present in the captured frame remains included.

Files go to ~/Pictures/NanoKVM with timestamp-based names. Existing files are never overwritten.
The status bar reports the saved path or failure. One export runs at a time without blocking the UI.
Screenshot clicks preserve existing video keyboard ownership and held keys; local keyboard ownership stays local.
Disconnect does not change an already requested snapshot.

App exit requests cancellation within the shared shutdown deadline.
Forced termination during a write can leave a partial new image; existing images remain protected.
Both formats have software checks in Focus Mode and Non-Focus Mode, including capture after toolbar auto-hide/reveal.
Use Area PNG or Area WebP to freeze the current frame, then drag the desired rectangle.
Selection starts inside the video. Dragging beyond its edges clamps the crop to the image.
The crop uses native image pixels, including after window resizing or letterboxing.
Selection gestures and wheel input stay local. Starting selection releases held remote mouse buttons.
Keyboard ownership remains unchanged while selecting or saving; remote keys still forward when video owns the keyboard.
Cancel selection stops without saving. Empty selections and export failures show a status message.
After saving, cancelling, or failing, mouse forwarding resumes only if Focus Mode remains active.
In Non-Focus Mode, click video or use Capture input to resume explicitly.
Releasing input, losing window focus, or disconnecting cancels selection and never restores old capture state.
An export already started can finish after disconnect; it does not restore input.

## Device and GPU color controls

Open Color for Exposure, Gamma, Highlights, White peak, and Saturation. Hover over each control for guidance.
GPU adjustments follow V4L2 device processing; both gamma controls can be active together.
Color contains separate GPU Color Tuning and V4L2 Device Controls panels.
The device panel lists supported standard brightness, color, white-balance, gain, exposure, and anti-flicker controls.
It shows device ranges, steps, defaults, current values, and availability. Unavailable controls cannot be edited.
Refresh device controls reads current settings, including changes made outside this viewer.
Edits run on the capture worker. The panel waits for device readback before accepting another edit.
An automatic control can disable its manual counterpart; change the automatic control first.
Device processing affects captured frames and screenshots before GPU processing.
Device settings are not saved in GPU presets. Driver behavior determines whether device settings persist after reconnect.

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
Combined-panel software checks pass. Natural-color acceptance still needs NanoKVM reference images and hardware calibration.

## Troubleshooting and validation

| Symptom | Action or limitation |
| --- | --- |
| No automatic pair | Check device permissions and the supported identity profile; use verified manual paths for unsupported layouts. |
| Video or serial stops | Read the status, resolve the device problem, and reconnect manually after cleanup. |
| Video wait timeout | A frame wait exceeding 500 ms ends the stream; the viewer does not retry that stream automatically. |
| Connect stays disabled | Wait for both workers to stop. A driver that ignores timeouts may delay closure. |
| Remote input does not arrive | Confirm video focus and mode status. Non-Focus Mode leaves desktop shortcuts available. |
| Device control fails | Read its status, use Refresh, and check whether an automatic setting disables the manual control. |
| Screenshot or preset fails | Read the displayed cause and check the destination permissions and absolute HOME/XDG paths. |

Run the application and patched input-backend tests:

```sh
cargo test --release --locked --all-targets
# Requires an accessible GPU:
cargo test --release --locked --all-targets -- --include-ignored --nocapture
# Avoid connecting the backend test to the desktop clipboard:
env -u DISPLAY -u WAYLAND_DISPLAY cargo test --release --locked -p egui-winit --lib
```

The [T-14 baseline](INTEGRATION-VERIFICATION.md) maps design requirements to the earlier integration evidence.
[Mode verification](MODE-VERIFICATION.md) records T-15 behavior and software checks.
[Toolbar verification](TOOLBAR-VERIFICATION.md) records T-16 layout, input routing, and screenshot integration.
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
| [shortcuts.rs](src/shortcuts.rs) | Window-scoped Wayland shortcut inhibition and acknowledgement |
| [vendor/egui-winit](vendor/egui-winit) | Native keyboard events before local clipboard translation |

## Protocol reference and license

Protocol work references `sipeed/NanoKVM-USB@1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc`:
[proto.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/proto.ts),
[device/index.ts](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/desktop/src/main/device/index.ts),
and renderer keyboard/mouse/media handling.
Cargo.toml declares GPL-3.0-only. See the [upstream license](https://github.com/sipeed/NanoKVM-USB/blob/1d1dd5e9fd24f97e9bf4a4ed6838e04ea4e48ecc/LICENSE).
