//! HID mapping for keyboard reports and absolute pointing in the video viewport.
//! Native modifier events preserve physical sides, including Linux Super/Meta.

use eframe::egui;
use crate::protocol::modifiers;

pub fn map_key(key: egui::Key) -> Option<u8> {
    use egui::Key::*;
    Some(match key {
        A => 0x04, B => 0x05, C => 0x06, D => 0x07, E => 0x08, F => 0x09,
        G => 0x0a, H => 0x0b, I => 0x0c, J => 0x0d, K => 0x0e, L => 0x0f,
        M => 0x10, N => 0x11, O => 0x12, P => 0x13, Q => 0x14, R => 0x15,
        S => 0x16, T => 0x17, U => 0x18, V => 0x19, W => 0x1a, X => 0x1b,
        Y => 0x1c, Z => 0x1d,
        Num1 => 0x1e, Num2 => 0x1f, Num3 => 0x20, Num4 => 0x21,
        Num5 => 0x22, Num6 => 0x23, Num7 => 0x24, Num8 => 0x25,
        Num9 => 0x26, Num0 => 0x27,
        Enter => 0x28, Escape => 0x29, Backspace => 0x2a,
        Tab => 0x2b, Space => 0x2c, Minus => 0x2d, Equals => 0x2e,
        OpenBracket => 0x2f, CloseBracket => 0x30, Backslash => 0x31,
        Semicolon => 0x33, Quote => 0x34, Backtick => 0x35,
        Comma => 0x36, Period => 0x37, Slash => 0x38,
        F1 => 0x3a, F2 => 0x3b, F3 => 0x3c, F4 => 0x3d, F5 => 0x3e,
        F6 => 0x3f, F7 => 0x40, F8 => 0x41, F9 => 0x42,
        F10 => 0x43, F11 => 0x44, F12 => 0x45,
        Insert => 0x49, Home => 0x4a, PageUp => 0x4b, Delete => 0x4c,
        End => 0x4d, PageDown => 0x4e,
        ArrowRight => 0x4f, ArrowLeft => 0x50, ArrowDown => 0x51, ArrowUp => 0x52,
        _ => return None,
    })
}

pub fn modifier_bit(key: egui_winit::winit::keyboard::KeyCode) -> u8 {
    use egui_winit::winit::keyboard::KeyCode::*;
    match key {
        ControlLeft => modifiers::LEFT_CTRL, ControlRight => modifiers::RIGHT_CTRL,
        ShiftLeft => modifiers::LEFT_SHIFT, ShiftRight => modifiers::RIGHT_SHIFT,
        AltLeft => modifiers::LEFT_ALT, AltRight => modifiers::RIGHT_ALT,
        SuperLeft => modifiers::LEFT_SUPER, SuperRight => modifiers::RIGHT_SUPER,
        _ => 0,
    }
}

pub fn fallback_modifiers(mod_state: egui::Modifiers) -> u8 {
    (if mod_state.ctrl { modifiers::LEFT_CTRL } else { 0 })
        | (if mod_state.shift { modifiers::LEFT_SHIFT } else { 0 })
        | (if mod_state.alt { modifiers::LEFT_ALT } else { 0 })
        | (if mod_state.mac_cmd { modifiers::LEFT_SUPER } else { 0 })
}

/// Full video aspect fits inside a window; pointer math MUST use rendered image
/// rectangle, not the egui window/client rectangle (prevents mouse drift).
pub fn fit_rect(outer: egui::Rect, width: u32, height: u32) -> egui::Rect {
    if width == 0 || height == 0 { return outer; }
    let scale = (outer.width() / width as f32).min(outer.height() / height as f32);
    let size = egui::vec2(width as f32 * scale, height as f32 * scale);
    egui::Rect::from_center_size(outer.center(), size)
}

pub fn normalized_point(rect: egui::Rect, point: egui::Pos2) -> (f32, f32) {
    let x = (point.x - rect.left()) / rect.width().max(1.0);
    let y = (point.y - rect.top()) / rect.height().max(1.0);
    (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}
