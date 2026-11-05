# T-16 remote desktop toolbar verification

Date: 2026-11-05 (Asia/Hong_Kong).
Current update: [CONTROLS-AREA-VERIFICATION.md](CONTROLS-AREA-VERIFICATION.md) supersedes the implementation gaps below.
T-16/T-17/T-18 and combined-panel software checks are complete. The following results remain historical baseline evidence.
T-13 and live hardware acceptance remain deferred.


The user approved T-16 and requested a commit after implementation.
Software checks remain the acceptance scope; live hardware/compositor checks are deferred.
The connection bar, pinning, auto-hide, reveal strip, existing actions, and full-video screenshot integration are implemented.
T-16 remains open only for T-18's area-selection action. Capture area is visibly disabled until that workflow exists.
T-12 software acceptance is complete with the new toolbar in both input modes.

## Behavior

The toolbar is a centered blue connection bar with a title, frame rate, pin toggle, and wrapping actions.
It overlays the video and defaults to pinned. Pin state lasts for the application session, including disconnect/reconnect.
Unpinning enables auto-hide after 0.7 seconds outside the toolbar.
Hover, a held pointer gesture, or keyboard focus inside the bar keeps it visible.
Mouse actions surrender toolbar-button focus so accidental retained focus does not prevent auto-hide.

A six-point strip remains along the top edge in windowed and fullscreen layouts.
Entering the strip reveals the bar, including entry and further movement within one input batch.
Only that strip reserves layout space. Showing/hiding the bar does not resize the remote image after layout settles.
Controls wrap within the available window width; the bar has a maximum width of 1000 points.

Connect, Disconnect, Focus Mode, Non-Focus Mode, release, Super, F12 mapping, Color, Fullscreen/Windowed, and both screenshot actions remain available.
Actions retain their existing availability rules. Screenshot actions additionally require a frame and no export already running.
Capture area has an unavailable tooltip and cannot start an export.
Mode, connection, discovery, and screenshot results remain in the bottom status area while the toolbar is hidden.

## Input integration

The overlay and reveal strip are excluded explicitly from video press, movement, and wheel hit testing.
This also protects the first frame of a newly revealed area, before egui's retained layer state catches up.
A video-owned mouse release over the bar still releases its button at the last remote coordinates.
Unowned toolbar presses/releases do not move the remote pointer.
Hidden screenshot button IDs are cleared so old geometry cannot preserve keyboard ownership for unrelated clicks.

Existing local-control behavior remains: local actions leave Focus Mode and clear held remote input.
PNG/WebP screenshot actions preserve video keyboard ownership, held keys, and Focus Mode.
Reveal hover alone neither activates capture nor releases held keyboard state.
Both formats still save native decoded video before GPU tuning, excluding viewer controls and the local desktop.

## Commands and results

Environment: Linux x86_64, Rust 1.97.1, Cargo 1.97.1, existing locked dependencies.
Commands run from the repository root:

```sh
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline toolbar_pin_hide_reveal_and_resize_preserve_video_geometry
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
cmp target/release/nanokvm-rust nanokvm-rust
test -x nanokvm-rust
./nanokvm-rust --help
./nanokvm-rust -h
git diff --check
```

Result: **65 passed, zero failed, one ignored GPU test**.
Three new toolbar tests cover layout/visibility, pointer routing, and unavailable/fullscreen actions.
The focused layout test also passes after adding explicit keyboard-focus visibility coverage.
Existing screenshot checks now include auto-hide/reveal in Focus Mode and Non-Focus Mode for PNG and WebP.
Existing floating-panel fixtures allow egui's initial sizing pass before interacting with controls.

The complete suite ran with host execution for private simulated Wayland socket traffic, with display environment variables unset.
It did not connect to a desktop compositor or physical NanoKVM device.
The native input backend is unchanged; its three passing T-15 tests remain applicable.
The unchanged GPU test is not rerun; earlier GPU evidence remains in COLOR-VERIFICATION.md and the T-14 baseline.

| Check | Evidence |
| --- | --- |
| Pin and auto-hide | Real pin-button clicks toggle state. The bar stays visible during the delay and hides after it expires. |
| Reveal and sizing | Pointer entry reveals the bar. At 640×480 and 1920×1080, settled video rectangles remain identical across hide/reveal. |
| Resize and fullscreen | Bar bounds remain within the resized viewport. Fullscreen/Windowed buttons emit the expected viewport commands; the strip remains available. |
| Keyboard access | A focused toolbar action keeps the unpinned bar visible without a pointer. Removing that focus permits hiding. |
| Pointer isolation | Same-batch strip entry, toolbar press/release, and wheel input queue no remote pointer reports. Owned drag release sends neutral buttons at preserved coordinates. |
| Unavailable actions | Disconnected mode/disconnect actions and missing-frame screenshots cannot activate. The area placeholder does not export. |
| Input modes | Existing button-driven activation and release tests pass with the new floating bar. |
| Screenshot ownership | Both formats retain held A/Meta and keyboard ownership in both modes, including after auto-hide/reveal. Decoded pixels and dimensions remain correct. |
| Existing regression suite | Keyboard mapping, lifecycle cleanup, Wayland inhibition, presets, discovery, file safety, and screenshot errors still pass. |

The release build succeeds with seven existing unused-code warnings; the test build reports three.
The copied root executable matches target/release, is executable, and passes both help options.
No dependencies, configuration schema, AGENTS.md, or DESIGN.md changed.

## Remaining work

T-18 must enable Capture area and verify selection, export, cancellation, and conditional mouse resumption through this toolbar.
The disabled placeholder does not constitute completed area capture, so T-16 remains unchecked.
Live compositor presentation, physical mouse/keyboard delivery, and real NanoKVM screenshots were NOT RUN, as requested.
Software UI checks verify egui layout/events and viewport commands, not actual compositor fullscreen behavior.
