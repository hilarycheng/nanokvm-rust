# T-12 full-video screenshot verification

Date: 2026-11-05 (Asia/Hong_Kong).

## Agreed output

The user deferred screenshot color tuning and accepted these output defaults:

- Capture the complete decoded remote video at its native frame dimensions.
- Save pixels before GPU adjustments, excluding viewer controls, borders, and the local desktop.
- Device-side processing already present in the MJPEG frame remains included.
- PNG is the primary toolbar action; a second action exports lossless WebP.
- Save under HOME/Pictures/NanoKVM using timestamp-based filenames and exclusive file creation.

Window size, fullscreen state, and video letterboxing do not change screenshot dimensions.
GPU presets remain available for display but do not affect screenshots.
No GPU readback or additional color-processing implementation is introduced.

## Implementation

The toolbar snapshots the current Arc<RgbaFrame> when the action runs.
One background worker encodes and saves that snapshot while capture and the UI continue.
Further screenshot actions remain disabled until completion; direct duplicate requests also keep the existing job.
Later frame updates or removal cannot change the owned snapshot. Disconnect does not cancel an already requested export.

Both formats use the existing image 0.25.10 dependency with explicit PNG/WebP features.
Enabling WebP adds image-webp 0.2.4; its supporting dependencies already exist in Cargo.lock.
The additional crate came from the local Cargo cache. No existing package version changed.

Export validates nonzero dimensions and RGBA buffer length before encoding.
Encoding completes in memory before any output file is created.
Exclusive creation refuses existing filenames, including collisions, without changing their contents.
Write failures remove the file created by that export and report the cause.
The toolbar reports the completed path or an error without replacing video/serial status.

Screenshot pointer presses retain existing video keyboard ownership in the input hook.
The video widget restores that ownership after egui processes the outside press.
Other toolbar interactions retain their existing local-input behavior.
Screenshot clicks send neither remote pointer events nor neutral keyboard reports.
Local keyboard ownership stays local; a screenshot does not activate remote capture.

App exit requests cancellation and waits within the existing shared shutdown deadline.
Cancellation is checked before encoding and before creating output. An in-progress codec or write cannot be forcibly interrupted.
Forced process termination during a file write can leave a partial new image; it cannot overwrite an existing image.

## Verification

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

Result: **55 tests pass, zero fail**. The existing explicit GPU test remains ignored in this run.
Four screenshot regression tests were added. GPU/shader processing is unchanged and was not rerun for raw-frame export.
The offline release build passes with seven existing unused-code warnings.
The root executable matches the release output, remains executable, and passes `--help`.

| Check | Evidence |
| --- | --- |
| Formats and native pixels | PNG and WebP decode to the original 3×2 RGBA image exactly. Format detection agrees with the selected encoder. |
| Capture ownership | Real egui press/release events activate both buttons with capture disabled, video keyboard ownership, and local keyboard ownership. Held remote A/Meta remain unchanged, with no queued input or release reports. |
| Raw color stage | With Soft white active, exported white remains 255. The decoded 2×1 image matches the source frame exactly. |
| Snapshot and single job | Removing the displayed frame after requesting export leaves the output unchanged. A second direct request preserves the first job/path. |
| Missing inputs and status | Missing frames and missing HOME report errors. Worker write errors remain separate from video status and do not release input. |
| File safety | Reusing an output path fails and preserves its exact bytes. Read-only directories and file-as-directory destinations fail safely. |
| Frame validation | Empty dimensions, incorrect RGBA lengths, and overflowing dimensions fail before writing. Unsupported WebP dimensions also produce an error. |
| Cancellation and path selection | A pre-cancelled operation creates no directory or file. Only an absolute HOME supplies the agreed Pictures/NanoKVM destination. |

Fixtures use temporary directories; no screenshot of the user's live remote desktop was taken.

## Remaining acceptance

Explicit Focus Mode and Non-Focus Mode do not yet exist. Their screenshot behavior remains under T-15 integration.
T-16 must retain the screenshot actions and ownership behavior in the final toolbar.
Live NanoKVM capture and compositor acceptance were **NOT RUN**.
T-12 remains unchecked until those mode/toolbar checks pass. Area screenshots remain under T-18.
