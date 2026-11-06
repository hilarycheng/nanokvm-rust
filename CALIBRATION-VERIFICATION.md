# Automatic color calibration verification

Date: 2026-11-06 (Asia/Hong_Kong).

The user approved one-click calibration and then simplified setup to a single SSH host or config alias.
The selected reference is the application's generated PNG, covering the remote display-to-capture path.
No remote screenshot tool or screenshot permission is required.

## Implementation

- One profile records only the SSH host/config alias. SSH uses the existing user, port, identity and jump-host configuration.
- Remote systemd session data and GNOME DisplayConfig discover the Wayland socket and active output connectors.
- The app tries active outputs until captured chart markers confirm the output connected to NanoKVM.
- The existing system SSH client transfers the PNG and runs mpv in the remote user's logged-in desktop.
- A private remote temporary directory contains the chart. A control pipe closes only this run's player.
- Non-interactive authentication and strict host-key checking preserve normal SSH trust checks.
- A remote watchdog bounds cleanup after a lost network connection. Local writes have cancellation-aware timeouts.
- Fresh markers for each output attempt reject stale, cropped, obscured and wrong-output frames.
- Five stable frames supply center samples for 17 gray patches, 27 fitting colors and 20 held-out validation colors.
- Per-channel monotonic curves linearize the gray response. A 3×3 matrix corrects linear RGB.
- Validation uses held-out RGB RMSE in 8-bit units, including the previous active correction as the baseline.
- The GPU applies calibration before manual color controls; the neutral path bypasses the calibration loops.
- Successful results atomically save the correction and neutral manual preferences before applying them.
- Cancellation, changed context and save failure retain previous correction and manual settings.
- Profile, video node, dimensions and last-read V4L2 values gate reuse. Remote color changes require recalibration.
- Old `colors.json` files remain readable. Calibrated profiles store subsequent manual GPU preferences in `calibration.json`.
- Screenshots retain the existing decoded, pre-GPU pixels.

No dependency or native input-backend change was needed. AGENTS.md and DESIGN.md were not changed.

## Software validation

```sh
cargo test --release --locked --offline --all-targets -- --include-ignored --skip live_single_device_discovery
CARGO_NET_OFFLINE=true ./build.sh
cmp target/release/nanokvm-rust nanokvm-rust
./nanokvm-rust --help
git diff --check
```

Result: **99 passed, zero failed, zero ignored, one hardware test filtered out**.
This includes the production WGSL rendering check on an available Vulkan adapter.
The release build, copied executable comparison, CLI help and diff whitespace check pass.
The test build retains two existing dead-code warnings; the release build retains six.

| Check | Evidence |
| --- | --- |
| Numerical correction | Synthetic channel gains, offsets and gamma errors are corrected on held-out colors. Neutral input remains neutral. |
| Invalid measurements | Wrong nonce, flat/wrong chart, clipped ramps and unimproved held-out colors are rejected. |
| SSH orchestration | A fake SSH executable receives the alias unchanged and the actual encoded PNG. The first output fails chart detection; the next completes sampling/fitting. Remote failures retain stderr context. |
| Desktop discovery | Parsed GNOME replies select active connectors and reject unsafe or missing values. The production discovery shell detects a selected or unique Wayland socket and rejects ambiguity. |
| Remote cleanup | The production remote shell runs with a stub player and a private socket; EOF stops that player and removes its chart directory. |
| Cancellation/deadline | A blocked transfer exits on deadline or cancellation without indefinite writes. UI cancellation suppresses completed results. |
| Persistence | Invalid host inputs cannot replace a saved file. Old profile fields are ignored; legacy GPU presets round-trip independently. |
| UI lifecycle | Success saves before applying; cancel, disconnect, changed dimensions/controls and save failure preserve prior settings. |
| Input isolation | Calibration blocks remote keyboard/mouse ownership and screenshot actions. |
| Shader | CPU and production GPU correction agree within two 8-bit levels on sRGB and non-sRGB targets. Existing GPU controls and reset checks pass. |

The local mpv option inventory confirms the full-screen output, image duration, cursor and terminal flags.
The local busctl option inventory and a private D-Bus call confirm its JSON reply envelope.
The shell/player and SSH fixtures do not connect to a desktop compositor or remote host.

## Limits

Live SSH authentication, GNOME output selection, actual MJPG chart capture and natural-color improvement were **NOT RUN**.
No remote host credentials were provided. Software fixture results do not establish live calibration quality.
The existing NanoKVM hardware discovery test was deliberately excluded.
T-13 RAM comparison and other deferred hardware acceptance remain deferred.

The first version supports SDR/sRGB on a logged-in GNOME desktop with an uncropped, matching-aspect-ratio output.
Automatic discovery needs the remote systemd tools and the SSH user's desktop session.
Ambiguous Wayland sessions fail with guidance instead of adding more configuration fields.
RGB RMSE is not Delta E, and it does not measure a physical display.
MJPG quantization, subsampling and clipping cannot be reversed by this calibration.
The thresholds are conservative software acceptance rules, not measured device accuracy guarantees.
Saved device matching uses the configured node and values, not a universal hardware identity.
Use Refresh after external V4L2 changes; recalibrate after remote display changes or replacing hardware.
