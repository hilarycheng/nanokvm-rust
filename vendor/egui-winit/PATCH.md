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

## T-15 right-side release keys

The native batch also carries physical Right-Control and Right-Alt presses/releases, before egui key translation.
Left modifiers and unrelated physical keys do not produce these events.
The existing synthetic-press filter remains in place. Window focus loss resets tracking in the viewer.
The viewer reserves the simultaneous right-side chord for local release; aggregate modifiers alone cannot trigger it.
Three backend tests cover clipboard preservation, batch publication, and side-specific release events.
Wayland inhibition lives in src/shortcuts.rs in the application, not this backend patch.
