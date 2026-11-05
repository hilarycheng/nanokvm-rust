# T-08 keyboard forwarding verification

Date: 2026-11-05 (Asia/Hong_Kong).

## Behavior and scope

- Remote reports prefer the event's physical key; logical keys are a fallback only when physical identity is absent.
- Modifier changes retain native event order. Each key uses its own modifiers; the final batch snapshot applies afterward.
- Ctrl+C/X/V presses and releases survive local clipboard translation, including when the local clipboard is empty.
- Keyboard state suppresses duplicate reports from repeated key-down events. Release uses the same physical identity after Shift changes.
- The native batch drains every frame, including local-focus and released-input frames. Local keys cannot replay on later capture.
- Existing local text and clipboard commands, focus release, pointer isolation, serial handling, and HID mapping remain unchanged.

`raw_input_hook` cannot recover presses discarded by upstream egui-winit.
The pinned [backend patch](vendor/egui-winit/PATCH.md) preserves mapped keys before clipboard translation and records modifier changes.
It publishes one batch per viewport when eframe takes input; the viewer drains it before checking ownership.
Frames without native keyboard events use the existing egui event path.
The direct egui-winit dependency exposes this batch; it adds no new third-party packages or version upgrades.
Cargo.lock changes only the patched package source and the root dependency entry.

## Software evidence

The existing Rust toolchain and temporary dependency cache from BUILD-VERIFICATION.md were used.

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --offline --all-targets
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline -p egui-winit --lib
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

The first command updates Cargo.lock to the local patched package. Subsequent test runs can use `--locked`.
Backend tests run without display variables to use a fallback clipboard and avoid changing the desktop clipboard.

Results: **43 app tests and two backend tests pass**. Four app tests and both backend tests are new.
The offline release build passes with seven existing unused-code warnings.
The root executable matches the release output, remains executable, and passes `--help`.
`cargo tree --locked --offline -i egui-winit` confirms eframe and the viewer use the same patched crate.

| Check | Evidence |
| --- | --- |
| Physical identity | Seven Shift punctuation cases and logical Z / physical Y produce the expected usages. Releases after Shift changes clear those usages. Logical fallback works when physical identity is absent. |
| Repeated presses | Repeated key-down events produce no duplicate unchanged report and do not interfere with release. |
| Event-specific modifiers | Ctrl+A and Shift+B in one batch retain their own modifiers before the final neutral modifier snapshot. |
| Clipboard shortcuts | The backend preserves C/X/V down and up while normal egui still receives Copy, Cut, or Paste. An empty clipboard produces no egui Paste but preserves both native V events. |
| Ordered native modifiers | The viewer queues Ctrl down, shortcut down, Shift on/off while held, shortcut up, and Ctrl up in order. Populated Paste content is not converted into remote text. |
| Batch ownership | Local-focus and disabled-forwarding frames drain native input without forwarding. Later capture does not replay it. |
| Publication | Native batches preserve physical identity, drain once, remain scoped to their viewport, and replace unread previous batches. |
| Existing behavior | All 39 earlier app tests still pass, including T-09 local isolation and connection cleanup. |

Tests use queued HID reports and the production translation helper without a native window.
They do not exercise a live compositor's key event delivery.

## Limits

Real NanoKVM HID delivery, Sway, and GPU rendering were **NOT RUN**.
Existing egui key coverage and aggregate Ctrl/Alt/Shift mapping remain; this task does not add a full native HID keymap.
The target's keyboard layout determines characters from forwarded physical usages.
Escape still releases capture. Explicit modes, right-side modifier detection, and shortcut inhibition remain under T-15.
Physical Meta forwarding remains under T-10. Serial failure still cannot guarantee remote release.
Revalidate the local backend patch when upgrading egui/eframe.
