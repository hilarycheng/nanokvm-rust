# NanoKVM keyboard input patch

Based on the crates.io `egui-winit` 0.31.1 source pinned in the original Cargo.lock.
Upstream: https://github.com/emilk/egui/tree/0.31.1/crates/egui-winit
This copy uses the upstream MIT license, reproduced in LICENSE-MIT.

The upstream backend replaces Ctrl+C/X/V key presses with clipboard commands.
Empty or unavailable clipboard contents can remove Ctrl+V entirely.
An application input hook cannot recover those presses or their modifier order.

This patch retains mapped key events before clipboard translation and records modifier changes in order.
`State::take_egui_input` publishes one keyboard batch per viewport in the egui context.
`take_keyboard_input` drains it; the next frame replaces any unread batch.
The viewer drains every frame, including frames with local focus or disabled forwarding.
Existing egui key, text, and clipboard translation remains unchanged.

Only src/lib.rs differs from upstream source. The manifest selects MIT and includes this license locally.
The new direct dependency uses the existing resolved crate, without additional third-party packages.
Keep this patch pinned until upstream exposes pre-translation keyboard input through eframe.
Recheck the patch and its tests before upgrading egui/eframe.

## Physical modifiers and pointer ordering

The native batch carries all eight physical Ctrl/Shift/Alt/Super presses and releases before egui translation.
It retains KeyCode identity; HID mapping stays in the application.
Aggregate egui modifiers remain unchanged for local widgets and never replace the native remote modifier state.
Mouse button, motion, wheel, and pointer-leave events join the same batch in native order.
This lets the viewer send modifier-click and modifier-scroll reports before a later modifier release in the same frame.

`take_keyboard_input` returns `Some(batch)` even for an empty published frame, and `None` without a published batch.
The distinction prevents an empty native frame from falling back to egui's lossy Linux modifier snapshot.
The synthetic-press filter remains unchanged. Window focus loss resets tracking in the viewer.
The viewer defers unused Right-Ctrl/Right-Alt until a remote combo, so its local release chord cannot leak its prefix.
Backend tests cover clipboard preservation, publication, all physical modifier sides, and pointer ordering.
Wayland inhibition lives in src/shortcuts.rs in the application.
