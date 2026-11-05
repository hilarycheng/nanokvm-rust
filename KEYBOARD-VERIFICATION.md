# T-08 keyboard forwarding verification

The T-08/T-10 records below are historical. See the [current update](#capture-and-physical-modifier-update) for approved behavior.

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
T-10 selects the permitted alternate-key mapping described below. Serial failure still cannot guarantee remote release.
Revalidate the local backend patch when upgrading egui/eframe.


## T-10 optional F12-to-Meta mapping

Date: 2026-11-05 (Asia/Hong_Kong).

The user approved T-10 before T-15. This implementation selects DESIGN.md's alternate-key mapping option.
The toolbar's F12 → Meta checkbox defaults off. It lasts for the current application session, including reconnects.
When enabled, F12 down/up controls the remote Left Meta modifier while video owns keyboard input.
Ordinary F12 is suppressed only in that mapping path; disabling the setting restores normal F12 reports.
The physical key identity from T-08 determines whether a key is F12.

Mapped Meta combines with Ctrl/Alt/Shift and survives empty input frames.
Repeated F12 presses do not add reports. Releasing F12 removes its hold without clearing an independent Super toggle hold.
Local focus, Escape, window focus loss, mapping changes, manual disconnect, and unexpected worker failure clear mapped state.
Existing release handling still attempts neutral reports; failed serial writes cannot guarantee remote release.
The mapping does not reinterpret Ctrl or Alt. Right-side release detection remains under T-15.

### Software evidence

```sh
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home CARGO_NET_OFFLINE=true ./build.sh
```

**47 app tests pass**, including four new checks.
The offline release build passes with seven existing unused-code warnings.
The root executable matches the release output, remains executable, and passes `--help`.

| Check | Evidence |
| --- | --- |
| Default F12 and mapped combinations | Default F12 sends usage 0x45. An ordered native batch sends Meta, Ctrl+Alt+Meta, and A reports, then releases Meta while A remains held. No F12 usage leaks while mapped. |
| Hold and toggle interaction | Empty frames preserve mapped Meta. Releasing F12 leaves an independent Super hold active; clearing the toggle releases the final hold. |
| Ownership and release | Local clicks, Escape, and window focus loss queue neutral release and clear mapped state. A later key-up cannot restore Meta. Local F12 presses remain local. |
| Toolbar setting changes | Real egui pointer events toggle the checkbox in both directions while F12 is held. Held input clears and ordinary F12 returns after disabling the mapping. |

Existing disconnect and failure fixtures now include mapped Meta and verify that shared cleanup clears it.
The dependency graph, backend patch, protocol mapping, and saved color configuration are unchanged.
No new dependencies, arbitrary key selector, or mapping persistence were added.

### Remaining acceptance

A read-only IPC query reached Sway 1.10.1-fde480b. This establishes compositor availability, not input acceptance.
Live Sway key delivery, NanoKVM HID output, and desktop recovery tests were **NOT RUN**.
Sway bindings can intercept keys before the viewer receives them; shortcut inhibition is still absent.
Explicit Focus Mode and Non-Focus Mode do not yet exist. T-15 must verify mapped Meta cleanup on those transitions.
The reserved Right-Ctrl/Right-Alt gesture also awaits T-15 implementation and verification.
T-10 remains unchecked until that integration and hardware acceptance are complete.


## Capture and physical modifier update

Date: 2026-11-05 (Asia/Hong_Kong).

The user explicitly approved Capture/Non-Capture naming instead of the names in DESIGN.md.
DESIGN.md and AGENTS.md remain unchanged. Non-Capture retains ordinary remote input with local desktop shortcuts available.

### Behavior

- The mode menu has only Capture and Non-Capture. Pending compositor acknowledgement remains a transition status.
- Capture waits for confirmed Wayland shortcut inhibition. Existing failure, revocation, and cleanup behavior remains.
- Native KeyCode events preserve left/right Ctrl, Shift, Alt, and Super as the eight HID modifier bits.
- Non-Capture defers unused physical modifiers until a forwarded key, click, or scroll. An unused release never synthesizes a tap.
- Modifier changes follow an ongoing remote key or drag. Right-Ctrl/Right-Alt always wait for a subsequent remote action.
- The simultaneous right-side release chord discards its unused prefix, including presses in separate frames and either order.
- Both reserved keys must come up before input can resume. Escape remains a remote key.
- F12 mapping, the Super toggle, and physical Meta maintain independent holds.
- Empty native batches preserve physical state. Aggregate egui snapshots cannot overwrite physical sides or Meta.
- Native pointer and keyboard events share ordering, preserving same-frame modifier-click and modifier-scroll releases.
- Local interactions, focus loss, disconnect, and worker failures clear deferred and forwarded modifier state.

### Scope and limits

The existing backend patch, input mapping, HID modifier constants, app forwarding, mode labels, and regression tests changed.
README.md and the backend patch notes describe the new behavior. No dependencies or persistent configuration were added.

Non-Capture cannot classify every compositor-owned shortcut; the compositor may hide part of its event sequence.
A modifier already used by a remote key or drag cannot be retracted. Neutral reports release input rather than undo remote actions.
Standalone physical modifiers require Capture; reserved Right-Ctrl/Right-Alt still require a subsequent remote action in either mode.
Explicit F12 mapping and the Super toggle retain their existing standalone behavior.

Live Sway routing and NanoKVM-to-GNOME HID behavior remain unverified. Tests use queued reports and a private Wayland protocol peer.

### Software verification

```sh
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline --all-targets
env -u DISPLAY -u WAYLAND_DISPLAY CARGO_HOME=/tmp/nanokvm-rust-audit.wHsm4I/cargo-home cargo test --release --locked --offline -p egui-winit --lib
```

Results: **88 app tests passed, zero failed, two hardware/GPU tests ignored; three backend tests passed.**
The app suite ran outside the filesystem sandbox because its private Wayland socket fixture requires socket I/O.
DISPLAY and WAYLAND_DISPLAY were unset; tests did not connect to the running compositor or real NanoKVM devices.
The initial restricted run failed at the fixture socket, not at shortcut activation assertions.

The tests cover all eight modifier bits, both-side overlaps, unused modifier suppression, native empty frames,
F12/physical Meta/toggle independence, same-frame click/scroll ordering, both release-chord orders across frames,
local shortcut prefixes followed by focus loss, clipboard forwarding, mode labels, and existing lifecycle/screenshot behavior.
The mouse fixture initializes the exact normalized position to avoid a floating-point coordinate-rounding mismatch.

The offline release build passed through `./build.sh` with six existing unused-code warnings.
The root `nanokvm-rust` executable matches `target/release/nanokvm-rust`, remains executable, and passes `--help`.
`git diff --check` passed. AGENTS.md and DESIGN.md hashes match the pre-change snapshot.
