# NanoKVM USB — Rust Viewer

A native Linux NanoKVM USB viewer built with Rust, egui/eframe, and wgpu.
It provides MJPEG video, serial keyboard/mouse forwarding, device and GPU color controls, and full-video or selected-area screenshots.
Software integration checks pass; see [input verification](KEYBOARD-VERIFICATION.md#capture-and-physical-modifier-update),
[controls and screenshot verification](CONTROLS-AREA-VERIFICATION.md), and [task status](TASKS.md).
Live hardware acceptance is deferred at the user's request.

The intended environment is Ubuntu with Sway → NanoKVM USB → Arch Linux with GNOME.
Capture, Non-Capture, and a pinned/auto-hide toolbar are available.
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

The app scans for the supported NanoKVM USB layout at startup and automatically connects a single valid pair.
Scanning and connecting show progress on the main screen without a Connection dialog.
Multiple valid pairs open a selection dialog showing each pair's video and serial paths.
Select a pair to connect. A fresh scan confirms its paths and USB hub before opening either worker.
Missing devices, incomplete or ambiguous pairs, scan failures, and connection failures open an error prompt.
After acknowledging an error, select Connect on the offline screen to try again.
Cancel stops a pending connection or dismisses device selection without connecting.
The viewer requests 1920×1080 MJPG and uses the negotiated dimensions. Serial reports use 57,600 baud.

Automatic pairing supports the [recorded USB identity and topology](DEVICE-IDENTIFICATION.md).
For unsupported layouts, `--video PATH --serial PATH` connects directly in manual mode. Both paths are required.
Device paths can change after reconnection; verify them before connecting manually.

Disconnect clears local input and video, stops both workers, and attempts neutral keyboard and mouse reports before serial closes.
Either worker failing initiates the same cleanup. The app never reconnects automatically.
After both workers finish, select Connect to retry. Input capture starts disabled after reconnection.
App exit requests the same cleanup and waits against a shared two-second deadline.

A worker that ignores cancellation keeps Connect unavailable. An error prompt reports delayed shutdown after two seconds.
Failed serial writes cannot guarantee remote key release. Successful local writes do not acknowledge remote HID delivery.

## Keyboard and mouse

Once serial is ready and video is available, click the image for normal Non-Capture input.
Keyboard reports follow video focus. Clicking local controls releases held remote input and keeps local typing local.
Pointer presses belong to video only when they start there. Releasing a video drag outside releases its remote button.
Window focus loss clears input capture and held state.

Non-Capture uses normal video/local input ownership and leaves desktop shortcuts available.
Click video to forward ordinary input without inhibiting desktop shortcuts.
Physical modifiers wait for a forwarded key or mouse action before sending.
Unused modifiers never generate a remote tap, including when a local shortcut takes window focus.
Modifier changes also follow an ongoing remote key or drag, except the reserved release keys.
Standalone physical modifiers require Capture, except the reserved Right-Ctrl/Right-Alt keys, which always wait for a combo.
F12 mapping and the explicit Super toggle remain available in either mode.

Select Capture to request Wayland shortcut inhibition for the viewer window.
The app waits for compositor acknowledgement before forwarding in Capture. It reports unavailable or rejected requests.
Capture requires a native Wayland window and compositor support; it is unavailable under X11/XWayland.
The compositor can retain emergency shortcuts or reject inhibition through its policy.

Hold Right-Ctrl + Right-Alt together, in either order, to release input and return to Non-Capture.
Release both keys before capturing again. Left-side Ctrl/Alt combinations do not trigger this shortcut.
Unused Right-Ctrl and Right-Alt stay pending in both modes, even across frames.
They forward with a subsequent remote key, click, or scroll; completing the release chord discards pending input.
A modifier already used in a remote combo cannot be retracted; release still requests neutral reports.
Escape forwards as a normal remote key in both modes.

The Non-Capture button clears held input and inhibition while keeping devices connected.
Click video to resume normal forwarding; select Capture explicitly to inhibit shortcuts again.
Local controls leave Capture and release held input. Screenshot actions preserve Capture and keyboard ownership.
Focus loss, disconnect, worker failure, and exit release inhibition. Compositor revocation also returns to Non-Capture.
After a protocol failure or revocation, restart the viewer before requesting Capture again; Non-Capture remains available.

Native input preserves both sides of Ctrl, Shift, Alt, and Meta (Super), including Right Alt / AltGr.
Enable F12 → Meta to use held F12 as an additional remote Left Meta source while video has keyboard focus.
Release F12 to release its Meta hold. The Super toggle independently controls a Meta hold.
Local interactions release the HID modifier; returning to video reapplies a selected Super toggle.
The release chord, Non-Capture button, focus loss, and disconnect clear that toggle and mapped holds.

F12 mapping defaults off. Disable it to send ordinary F12; changing the mapping releases held remote input.
The mapping preference lasts for the application session, including disconnect/reconnect.
Ctrl+C/V/X, all eight physical modifiers, deferred combos, mouse combo ordering, release chords, and local isolation have software regression coverage.
Simulated Wayland protocol tests cover inhibition and cleanup. Live Sway/NanoKVM acceptance remains deferred.

The centered, title-free connection bar has a straight-edged inverted trapezoid shape and one row of icons.
Hover labels and accessibility labels name each action. Menu items retain their text labels.
Pin toggles pinning for the current application session. Unpinning enables auto-hide after 0.7 seconds outside the bar.
A narrow handle remains at the top edge. Move the pointer to the top strip to reveal the bar.
Pointer interaction, keyboard focus, and open toolbar menus keep it visible.
Showing or hiding the bar does not resize the remote image. The bar fits the minimum 640-point window width.
The mode menu provides only Capture and Non-Capture. Waiting for compositor acknowledgement is a transition status.
PNG (`📷`), WebP (`🖼`), Area PNG (`✂`), and Area WebP (`⊞`) remain directly available, including in Capture.
The more menu (`…`) contains Color, Super, and F12 → Meta. Opening local menus releases Capture.
Fullscreen/Windowed and Close (`×`) use the rightmost buttons, with hover labels.
Close exits the app through normal device cleanup instead of returning to the Connection dialog.
Hover over the keyboard icon for the input mode; hover over the information icon for FPS, capture progress, and device details.
There is no bottom bar. The reveal strip overlays the video without reserving layout space.
Video has no panel margins in windowed or fullscreen mode and fits the current window while preserving its aspect ratio.
The fullscreen button follows the system's fullscreen state, including changes made through the window manager.
Matching capture and physical screen dimensions render at 1:1 in fullscreen, underneath the toolbar overlay.
Errors show an acknowledgement prompt and release remote input before local interaction.
Normal connection and screenshot status updates do not open prompts.
Screenshot actions are disabled without a frame or while an export is running.
Area PNG and Area WebP start selection. Cancel selection stops an active selection.
Opening Color releases input capture. Resume explicitly when finished adjusting colors.

## Screenshots

Use PNG for the primary format or WebP for lossless WebP.
Both save the complete decoded remote video at its native resolution, before GPU tuning.
Saved images exclude viewer controls, borders, and the local desktop.
Device-side processing already present in the captured frame remains included.

Files go to ~/Pictures/NanoKVM with timestamp-based names. Existing files are never overwritten.
The toolbar status tooltip reports the saved path. Failures also open an error prompt. One export runs at a time without blocking the UI.
Screenshot clicks preserve existing video keyboard ownership and held keys; local keyboard ownership stays local.
Disconnect does not change an already requested snapshot.

App exit requests cancellation within the shared shutdown deadline.
Forced termination during a write can leave a partial new image; existing images remain protected.
Both formats have software checks in Capture and Non-Capture, including capture after toolbar auto-hide/reveal.
Use Area PNG or Area WebP to freeze the current frame, then drag the desired rectangle.
Selection starts inside the video. Dragging beyond its edges clamps the crop to the image.
The crop uses native image pixels, including after window resizing or letterboxing.
Selection gestures and wheel input stay local. Starting selection releases held remote mouse buttons.
Keyboard ownership remains unchanged while selecting or saving; remote keys still forward when video owns the keyboard.
Cancel selection stops without saving. Empty selections and export failures open an error prompt.
After saving, cancelling, or failing, mouse forwarding resumes only if Capture remains active.
In Non-Capture, click video to resume explicitly.
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

## Automatic color calibration

Open **Color → Auto calibration** and enter one **SSH host**, such as `user@host` or an SSH config alias.
The initial workflow targets an SDR/sRGB Arch Linux GNOME Wayland desktop.
It generates its own known RGB test chart; a remote screenshot tool is unnecessary.

On the remote machine, install the tools and configure an SSH server:

```sh
sudo pacman -S --needed openssh mpv
```

The viewer machine needs the system `ssh` client. Configure key/agent authentication and verify the remote host
in a terminal first. Calibration uses non-interactive login and strict host-key checking.
An SSH config alias can supply the user, port, identity and jump host.
The application stores no SSH password and never installs packages automatically.

For example, add this entry to `~/.ssh/config` on the viewer machine:

```sshconfig
Host nano-kvm
    HostName kvm.example.net
    User your-user
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes
```

Run `ssh nano-kvm` once and verify the host key through your usual SSH process.
Then enter `nano-kvm` in the **SSH host** field.

Log into the remote GNOME desktop as the SSH user. Select **Auto calibrate** to save the host and start.
The app detects the Wayland session and active GNOME outputs using the remote systemd tools, `systemctl` and `busctl`.
It tries those outputs in turn until NanoKVM captures the matching test chart.
No display socket, connector name, upload command or remote command is required in the UI.
The SSH session launches mpv inside that existing desktop, using `/run/user/<remote uid>`.
If several Wayland sockets exist without a session selected by systemd, discovery stops with setup guidance.

Disable HDR, Night Light and automatic device color/exposure controls before calibration.
Use a full-screen, uncropped output with the same aspect ratio as the captured video.
Close overlays and keep the remote pointer away from the chart.

Calibration performs these steps without blocking the UI:

1. Discover the remote desktop and transfer a generated PNG through SSH to full-screen mpv.
2. Try each active output, verify its fresh chart markers, and collect five stable decoded video frames.
3. Measure patch centers, fit per-channel gray curves and a linear RGB matrix, and evaluate separate validation colors.
4. Save and apply the correction only after validation and a successful atomic profile write.
5. Close only the test player started by this run and remove its temporary chart.

The status reports RGB RMSE on a 0–255 scale, before and after calibration with neutral manual GPU controls.
This is a numerical RGB metric, not a perceptual Delta E or a monitor measurement.
Validation requires at least 10% improvement and an error no higher than 8.
An existing correction with error at or below 1 is retained as already accurate.
Clipped/non-monotonic gray ramps, insufficient color information, wrong charts and unstable frames fail without replacing the correction.

Remote keyboard/mouse input and screenshot actions stay disabled during calibration.
Manual GPU and device controls also stay disabled until it finishes.
**Cancel calibration**, disconnect and app exit stop the job without applying a late result.
The local operation has a 35-second deadline. A remote 45-second watchdog, with a two-second kill grace,
bounds the test window when a broken network prevents immediate cleanup.
Remote login, missing mpv and session errors appear in the calibration panel with setup guidance.

`calibration.json`, beside `colors.json`, stores the profile, validated correction and manual GPU preferences.
After successful calibration, manual GPU controls start neutral and are saved in this profile.
Subsequent **Save preset** actions save those manual preferences and the calibration enable switch together.
Existing `colors.json` files remain readable; their values are used until a calibrated profile supplies manual preferences.
**Use saved calibration** enables/bypasses the correction. **Reset 1:1** also bypasses it until re-enabled or recalibrated.
Saved corrections require a matching SSH host, video node, resolution and last-read device controls.
Changing those values bypasses the correction. Use **Refresh device controls** after external device edits.
Remote display/color changes are not observable over this interface and require explicit recalibration.

Calibration runs before the manual GPU controls. PNG/WebP exports still contain decoded capture pixels before GPU processing.
MJPG compression and chroma subsampling limit recoverable detail; patch averaging cannot restore lost information.
This workflow corrects the source-to-capture path, including remote desktop processing, and does not calibrate the physical monitor.
See [calibration verification](CALIBRATION-VERIFICATION.md) for software checks and hardware limits.

## Troubleshooting and validation

| Symptom | Action or limitation |
| --- | --- |
| No automatic pair | Check device permissions and the supported identity profile; use verified manual paths for unsupported layouts. |
| Video or serial stops | Read the status, resolve the device problem, and reconnect manually after cleanup. |
| Video wait timeout | A frame wait exceeding 500 ms ends the stream; the viewer does not retry that stream automatically. |
| Connect stays disabled | Wait for both workers to stop. A driver that ignores timeouts may delay closure. |
| Remote input does not arrive | Confirm video focus and mode status. Non-Capture leaves desktop shortcuts available. |
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
| [calibration.rs](src/calibration.rs) | Calibration profile, SSH chart display, sampling, fitting and validation |
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
