# T-14 integration verification baseline

Date: 2026-11-05 (Asia/Hong_Kong).
Source baseline: `5abf129` (T-12). This revision changes documentation only.

Subsequent scope update: the user prioritized software completion and deferred live hardware checks before approving T-15.
[MODE-VERIFICATION.md](MODE-VERIFICATION.md) supersedes this baseline's missing-mode and inhibition statements.
[TASKS.md](TASKS.md) supplies current delivery status. Results below describe the T-12 source baseline only.

**Historical partial acceptance: T-14 remains open.** Existing regression checks pass, and operating documentation now matches current behavior.
T-15 through T-18 remain unimplemented. T-13 is deferred at the user's request.
Required live NanoKVM and Sway checks remain NOT RUN; software results do not replace those checks.
Physical unplug/replug exercises remain waived under the revised T-04 scope.

## Scope and environment

This pass reviews README.md, SCOPE.md, TASKS.md, relevant implementation paths, and retained verification records against DESIGN.md.
README.md now uses English and documents actual controls, screenshot choices, recovery, presets, and limitations.
SCOPE.md preserves S1–S7 and F1–F8 while replacing obsolete implementation claims and unaccepted proposals.
AGENTS.md and DESIGN.md remain unchanged. No source, test, fixture, configuration, or dependency changes were needed.

- Linux x86_64; Rust 1.97.1 and Cargo 1.97.1.
- Application dependencies: Cargo.lock; eframe/egui 0.31.1 and the pinned local egui-winit patch.
- GPU: AMD Radeon RX 6600 (RADV NAVI23), Vulkan, radv/Mesa 26.2.4.
- GPU test: host device access; offscreen pixel readback to Rgba8UnormSrgb and Rgba8Unorm targets.
- Backend tests: DISPLAY and WAYLAND_DISPLAY unset to avoid attaching to the desktop clipboard.
- Lifecycle tests: mock video/writers and Linux pseudo-terminals; no live NanoKVM capture or HID session.

## Commands and results

Commands run from the repository root:

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets -- --include-ignored --nocapture
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline -p egui-winit --lib
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
cmp target/release/nanokvm-rust nanokvm-rust
test -x nanokvm-rust
./nanokvm-rust --help
./nanokvm-rust -h
sha256sum nanokvm-rust Cargo.lock
git diff --check
```

The temporary Cargo cache supplied the locked dependencies without registry access. Other machines need their own cache or registry access.

| Check | Result |
| --- | --- |
| Application regression suite, including explicit GPU test | PASS: 56 passed, zero failed, zero ignored. |
| Patched egui-winit backend | PASS: two passed, zero failed. |
| Release build and copied executable | PASS: build succeeds; root executable matches target/release and is executable. |
| Both CLI help options | PASS: optional paired paths are documented; startup remains disconnected. |
| Documentation consistency | PASS: local Markdown targets exist; task counts remain eight delivered and ten open. |
| Patch whitespace and protected files | PASS: diff check passes; AGENTS.md and DESIGN.md remain unchanged. |

The test build reports three existing unused-code warnings; the release build reports seven.
No warning cleanup or dependency upgrade belongs to this documentation pass.
Root executable SHA-256: `f1b17b2703a3f2475ec8c0d56f41b9755ffa05cda2e2be8d2d9dde9e3228e281`.
Cargo.lock SHA-256: `3c6d1e56327d6071bc5d250575436d6f05ae0a43503c32962894b78ea834d458`.

## DESIGN.md requirement coverage

PASS below applies only to the named check, not complete hardware acceptance.
PENDING means implementation or integration remains incomplete. NOT RUN means the named acceptance check has no result.

| Design requirement | Evidence available | Remaining acceptance |
| --- | --- | --- |
| Rewrite the TypeScript application in Rust | Native Rust executable builds successfully. | Full feature acceptance remains open. |
| Use egui/eframe instead of HTML | Native UI source and egui event regression checks pass. | Live desktop presentation: NOT RUN. |
| Full-screen or selected-area PNG/WebP | Full native-frame files decode to exact test pixels in both formats. [T-12 evidence](SCREENSHOT-VERIFICATION.md). | Selected-area workflow: PENDING T-18. Live capture: NOT RUN. |
| Full capture from toolbar during Focus Mode | Both screenshot buttons retain current keyboard ownership and held keys in egui tests. | Explicit Focus Mode and final toolbar: PENDING T-15/T-16. |
| Partial capture pauses mouse and resumes only in Focus Mode | No selection workflow exists. | PENDING T-18; selection, cancellation, failures, and mode transitions all require checks. |
| Separate V4L2/GPU panels, required GPU controls, concurrent gamma | GPU controls pass actual render/readback checks. [T-11 evidence](COLOR-VERIFICATION.md). | V4L2 panel and combined gamma: PENDING T-17; device effects: NOT RUN. |
| Clear guidance for both panels | GPU labels/tooltips and processing-order guidance exist. | V4L2 guidance: PENDING T-17. |
| Capture/release keyboard and mouse | Ownership, local widgets, drag release, focus loss, and neutral reports pass. [Lifecycle evidence](LIFECYCLE-VERIFICATION.md). | Physical HID delivery: NOT RUN. |
| Focus Mode captures Sway keys; Right-Ctrl, Right-Alt releases locally | Current Escape release is covered; required inhibition and right-side tracking are absent. | PENDING T-15. Gesture meaning requires confirmation; Sway recovery: NOT RUN. |
| Remote Desktop-style top toolbar with pinning/auto-hide | Fixed top toolbar exists. | PENDING T-16; styling, pinning, and auto-hide. |
| Narrow reveal strip and pointer reveal | No reveal strip exists. | PENDING T-16; windowed/fullscreen reveal checks. |
| Toolbar Focus/Non-Focus switch; VNC-style Non-Focus input | Video/local ownership is tested; explicit mode actions are absent. | PENDING T-15/T-16; mode transitions and desktop shortcuts. |
| Disconnect and Fullscreen toolbar actions | Both actions exist. Disconnect cleanup passes software tests. | Final toolbar and compositor fullscreen checks: NOT RUN. |
| GPU presets and wgpu/WGSL processing | Validation, file safety, round trips, all controls, texture resize, and reset pass. | Combined controls and natural-color reference acceptance: NOT RUN. |
| Manual disconnect stops I/O, clears state, and attempts neutral reports | Full-queue bypass, resource closure, partial opens, failed writes, and exit deadlines pass. | Live capture/HID session: NOT RUN. Inhibition cleanup awaits T-15. |
| Unexpected device loss cleans up despite release failures | Injected video failure, idle tty hangup, failed writes, and manual serial recovery pass. | Actual V4L2 stream behavior: NOT RUN. Inhibition cleanup awaits T-15. |
| Physical Meta or mapped key | F12-to-Meta combinations, ordering, independent Super state, and release paths pass. [Keyboard evidence](KEYBOARD-VERIFICATION.md). | T-15 mode/shortcut integration; Sway/NanoKVM delivery: NOT RUN. |

Full-screen capture here means the complete remote video at native dimensions, before GPU tuning, as recorded for T-12.
Neither the local desktop nor viewer chrome is included. This choice supersedes the earlier unaccepted screenshot proposal.

## Supplementary scope

| Scope | Evidence and status |
| --- | --- |
| Build delivery | Current release build, executable comparison, permissions, and help pass. T-03's isolated build-failure checks remain applicable to the unchanged build script. |
| Start disconnected and reconnect manually | Current regression checks pass. Discovery alone does not open device workers; reconnect waits for cleanup. |
| Automatic USB pairing | Matching fixtures cover absent, ambiguous, removed, and renumbered devices. [T-04 inventory](DEVICE-IDENTIFICATION.md) and [T-06 verification](DISCOVERY-VERIFICATION.md) remain applicable. |
| RAM comparison | NOT RUN; T-13 deferred by the user on 2026-11-05. No workload duration or memory reduction is claimed. |

## Completion gates

T-14 cannot close while the implementation gaps and required NOT RUN results above remain.
Complete T-15 modes/inhibition, T-16 toolbar, T-17 V4L2 controls, and T-18 area capture before final integration acceptance.
Then verify mode changes, release shortcuts, Meta, both color panels, screenshots, disconnects, shutdown, and manual recovery together.
Resolve the remaining T-07/T-10/T-11/T-12 acceptance with those features and live evidence.
T-13 stays deferred until the user resumes it or explicitly revises final acceptance scope.

Software tests prove local reports and cleanup paths, not remote HID acknowledgement or kernel timeout compliance.
Synthetic GPU pixels prove processing, not monitor calibration or subjective natural-color quality.
Historical records retain their original test counts and limits; this record supplies the current aggregate results.
