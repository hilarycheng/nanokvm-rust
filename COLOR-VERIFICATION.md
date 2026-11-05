# T-11 GPU controls and preset verification

Date: 2026-11-05 (Asia/Hong_Kong).

## Changes

Gamma, Highlights, Saturation, Exposure, White peak, Reset 1:1, and Soft white remain available.
Each slider explains its effect. The panel states that V4L2 processing precedes GPU processing and both gamma controls can remain active.
GPU presets affect the video only; they do not modify device controls or desktop colors.
The panel scrolls to keep guidance and errors accessible in small windows.
Reset applies neutral GPU values. Soft white resets the other controls and sets white peak to 0.84.
Neither action writes a preset automatically.

Preset load/save validates finite values against the same ranges used by the sliders:

| Control | Range | Neutral |
| --- | --- | --- |
| Exposure | -2 to +2 EV | 0 |
| Gamma | 0.60 to 1.60 | 1 |
| Highlights | 0 to 1 | 0 |
| White peak | 0.55 to 1 | 1 |
| Saturation | 0 to 1.5 | 1 |

Missing presets use defaults. Invalid JSON, missing fields, out-of-range values, and read failures use defaults with a visible explanation.
Failed loads leave the saved file untouched. GPU preset status remains separate from video connection status.
Saving validates and serializes first, creates a new sibling temporary file, writes and syncs it, then renames it over the destination.
Failed writes/replacements remove only the temporary file created by that save. Existing temporary files are never overwritten or removed.
Concurrent saves from separate processes may replace one another; the last successful rename wins.
This is atomic file replacement, not a guarantee against filesystem or power-loss failure.

An absolute XDG_CONFIG_HOME takes precedence over an absolute HOME/.config.
Empty or relative environment paths are ignored. Without a usable root, the panel explains why presets cannot be saved.
The filename and JSON schema remain compatible with existing valid presets.
The dependency graph and production shader remain unchanged.

## Commands and results

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets -- --include-ignored --nocapture
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

The regular run passes 51 tests and leaves the GPU test explicitly ignored.
The second command ran with host GPU access: **52 passed, zero failed, zero ignored**.
Rust and cached dependencies match BUILD-VERIFICATION.md.
After adding panel scrolling, the focused UI preset-button regression check passed again.
The final offline release build passes with seven existing unused-code warnings.
The root executable matches the release output, remains executable, and passes `--help`.

| Check | Result |
| --- | --- |
| Preset round trip | Defaults, Soft white, and all slider boundary values survive save/load exactly. |
| Invalid presets | Malformed/incomplete JSON, both sides of every range, NaN, and infinities are rejected. Failed saves preserve existing bytes. |
| File failures | Read-only directories, unreadable files, a file used as a parent, a directory used as a destination, and an existing temporary file produce errors without destroying the old preset. Permission tests ran as UID 1000. |
| Status and paths | Defaults, restored settings, save success/failure, missing environment paths, and XDG/HOME precedence behave as documented. Video status is preserved. |
| UI preset buttons | Real egui pointer events apply Soft white and Reset across all five settings and report that Save preset is needed. |
| GPU processing | The production texture upload, uniforms, pipeline, WGSL, and draw call render an eight-patch image to both output formats. Pixel readback checks each adjustment and reset. |

## Actual GPU rendering

- Adapter: AMD Radeon RX 6600 (RADV NAVI23), discrete GPU.
- Backend: Vulkan; driver: radv, Mesa 26.2.4.
- Targets: Rgba8UnormSrgb and Rgba8Unorm.
- Inputs: black, gray, white, a mixed color, primary colors, and a second mixed color across two rows.
- Neutral settings preserve input pixels within two 8-bit levels, including row orientation and alpha.
- Exposure +1 EV maps gray 128 to approximately 176. Gamma 1.6 maps it to 83.
- Highlights 1 maps gray/white to 117/188. White peak 0.55 maps them to 67/140.
- Soft white maps gray/white to 106/214. Saturation 0 makes all patches grayscale and maps the mixed color to 118.
- Controls update the retained texture without another captured frame.
- Resizing to a one-pixel texture and back succeeds. Reset returns the original neutral image.

Creating the production pipeline compiles and validates the WGSL through wgpu.
No shader change was needed. This replaces the earlier unavailable-GPU result for this offscreen test only.

## Remaining acceptance

The separate V4L2 control panel does not exist yet; T-17 must verify independent device/GPU adjustment and combined gamma behavior.
Live NanoKVM capture, reference-image natural-color acceptance, monitor calibration, and compositor presentation were **NOT RUN**.
Synthetic pixel checks verify numerical processing, not subjective color quality or capture-device calibration.
T-11 remains unchecked until combined-panel and natural-color acceptance are complete.
