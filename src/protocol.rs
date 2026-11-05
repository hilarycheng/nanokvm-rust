//! Host-side framing and HID data for Sipeed NanoKVM USB.
//! Reference: sipeed/NanoKVM-USB@1d1dd5e, desktop/src/main/device/{proto,index}.ts.
//! Serial baudrate: 57_600. No firmware modification is needed.

use std::fmt;

pub const BAUD: u32 = 57_600;
pub const HEADER: [u8; 2] = [0x57, 0xAB];
pub const ADDRESS: u8 = 0x00;
pub const SEND_KEYBOARD: u8 = 0x02;
pub const SEND_MOUSE_ABSOLUTE: u8 = 0x04;
pub const SEND_MOUSE_RELATIVE: u8 = 0x05;
pub const GET_INFO: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketError {
    TooLong,
    Header,
    Incomplete,
    Checksum,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PacketError {}

/// Wire format: 57 AB ADDR CMD LEN DATA[LEN] CHECKSUM.
/// Checksum is the low 8 bits of the sum of *all* preceding bytes,
/// including HEADER, ADDR, CMD and LEN. Do not copy upstream decode()'s
/// apparent off-by-one bounds bug.
pub fn encode(command: u8, data: &[u8]) -> Result<Vec<u8>, PacketError> {
    let len = u8::try_from(data.len()).map_err(|_| PacketError::TooLong)?;
    let mut packet = Vec::with_capacity(6 + data.len());
    packet.extend_from_slice(&HEADER);
    packet.extend_from_slice(&[ADDRESS, command, len]);
    packet.extend_from_slice(data);
    let sum = packet.iter().fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    packet.push(sum);
    Ok(packet)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decoded<'a> {
    pub address: u8,
    pub command: u8,
    pub payload: &'a [u8],
}

pub fn decode(packet: &[u8]) -> Result<Decoded<'_>, PacketError> {
    if packet.len() < 6 {
        return Err(PacketError::Incomplete);
    }
    if packet[..2] != HEADER {
        return Err(PacketError::Header);
    }
    let len = packet[4] as usize;
    if packet.len() != len + 6 {
        return Err(PacketError::Incomplete);
    }
    let expected = packet[..packet.len() - 1]
        .iter()
        .fold(0u8, |acc, b| acc.wrapping_add(*b));
    if expected != packet[packet.len() - 1] {
        return Err(PacketError::Checksum);
    }
    Ok(Decoded { address: packet[2], command: packet[3], payload: &packet[5..5+len] })
}

/// Standard boot keyboard report: modifier, reserved, up to 6 USB usages.
#[derive(Debug, Default, Clone)]
pub struct KeyboardReport {
    pub modifiers: u8,
    usages: Vec<u8>,
}

impl KeyboardReport {
    pub fn set_key(&mut self, usage: u8, down: bool) {
        if down && !self.usages.contains(&usage) && self.usages.len() < 6 {
            self.usages.push(usage);
        } else if !down {
            self.usages.retain(|v| *v != usage);
        }
    }

    pub fn release(&mut self) {
        self.modifiers = 0;
        self.usages.clear();
    }

    pub fn bytes(&self) -> [u8; 8] {
        let mut report = [0u8; 8];
        report[0] = self.modifiers;
        report[2..2 + self.usages.len()].copy_from_slice(&self.usages);
        report
    }
}

pub mod modifiers {
    pub const LEFT_CTRL: u8 = 1 << 0;
    pub const LEFT_SHIFT: u8 = 1 << 1;
    pub const LEFT_ALT: u8 = 1 << 2;
    pub const LEFT_SUPER: u8 = 1 << 3;
    pub const RIGHT_SUPER: u8 = 1 << 7;
}

/// Upstream absolute mouse coordinate range is 0..4096 (NOT 32767).
/// Upstream command payload includes a report mode prefix byte 0x02,
/// followed by 6-byte HID report: buttons, x_lo, x_hi, y_lo, y_hi, wheel.
pub fn mouse_absolute(buttons: u8, x: f32, y: f32, wheel: i8) -> [u8; 7] {
    let to_abs = |v: f32| -> u16 { (v.clamp(0.0, 1.0) * 4096.0).floor() as u16 };
    let x = to_abs(x).to_le_bytes();
    let y = to_abs(y).to_le_bytes();
    [0x02, buttons, x[0], x[1], y[0], y[1], wheel as u8]
}

/// Upstream relative mouse payload: 0x01 + buttons, delta_x, delta_y, wheel.
pub fn mouse_relative(buttons: u8, dx: i8, dy: i8, wheel: i8) -> [u8; 5] {
    [0x01, buttons, dx as u8, dy as u8, wheel as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_frame_checksum() {
        let report = [0x08, 0, 0x04, 0, 0, 0, 0, 0];
        let pkt = encode(SEND_KEYBOARD, &report).unwrap();
        assert_eq!(pkt.as_slice(), &[0x57, 0xab, 0, 0x02, 8, 8, 0, 4, 0, 0, 0, 0, 0, 0x18]);
        let decoded = decode(&pkt).unwrap();
        assert_eq!(decoded.command, SEND_KEYBOARD);
        assert_eq!(decoded.payload, report);
    }

    #[test]
    fn checksum_error() {
        let mut pkt = encode(GET_INFO, &[]).unwrap();
        *pkt.last_mut().unwrap() ^= 1;
        assert_eq!(decode(&pkt).unwrap_err(), PacketError::Checksum);
    }

    #[test]
    fn too_large_payload() {
        assert_eq!(encode(GET_INFO, &[1; 256]).unwrap_err(), PacketError::TooLong);
    }

    #[test]
    fn mouse_coordinates() {
        let m = mouse_absolute(1, 1.0, 0.5, -1);
        assert_eq!(m, [2, 1, 0, 16, 0, 8, 255]);
    }

    #[test]
    fn key_rollover_release() {
        let mut k = KeyboardReport::default();
        for n in 4..12 {
            k.set_key(n, true);
        }
        assert_eq!(&k.bytes()[2..], &[4, 5, 6, 7, 8, 9]);
        k.set_key(5, false);
        assert!(!k.bytes().contains(&5));
        k.release();
        assert_eq!(k.bytes(), [0; 8]);
    }
}
