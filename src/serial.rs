//! Dedicated serial I/O thread: never block egui/GPU on a 57_600-baud port.
use crossbeam_channel::{bounded, Receiver, Sender};
use std::{io::Write, thread, time::Duration};

use crate::protocol;

#[derive(Debug, Clone)]
pub enum Report {
    Keyboard([u8; 8]),
    AbsoluteMouse([u8; 7]),
    RelativeMouse([u8; 5]),
    Shutdown,
}

pub struct SerialHandle {
    pub tx: Sender<Report>,
    pub status_rx: Receiver<String>,
}

pub fn spawn(port_path: String) -> SerialHandle {
    let (tx, rx) = bounded::<Report>(256);
    let (status_tx, status_rx) = bounded::<String>(8);
    thread::spawn(move || {
        let opened = serialport::new(&port_path, protocol::BAUD)
            .timeout(Duration::from_millis(200))
            .open();
        let mut port = match opened {
            Ok(port) => port,
            Err(err) => {
                let _ = status_tx.send(format!("Serial open failed ({port_path}): {err}"));
                return;
            }
        };
        let _ = status_tx.send(format!("Serial connected: {port_path}"));
        while let Ok(report) = rx.recv() {
            let (cmd, bytes): (u8, &[u8]) = match &report {
                Report::Keyboard(data) => (protocol::SEND_KEYBOARD, data),
                Report::AbsoluteMouse(data) => (protocol::SEND_MOUSE_ABSOLUTE, data),
                Report::RelativeMouse(data) => (protocol::SEND_MOUSE_RELATIVE, data),
                Report::Shutdown => break,
            };
            match protocol::encode(cmd, bytes)
                .map_err(|e| e.to_string())
                .and_then(|packet| port.write_all(&packet).map_err(|e| e.to_string()))
            {
                Ok(()) => (),
                Err(err) => {
                    let _ = status_tx.try_send(format!("Serial write failed: {err}"));
                    break;
                }
            }
        }
        // Release all remote keys and mouse buttons on ordinary exit.
        if let Ok(frame) = protocol::encode(protocol::SEND_KEYBOARD, &[0u8; 8]) {
            let _ = port.write_all(&frame);
        }
        if let Ok(frame) = protocol::encode(
            protocol::SEND_MOUSE_ABSOLUTE,
            &protocol::mouse_absolute(0, 0.5, 0.5, 0),
        ) {
            let _ = port.write_all(&frame);
        }
    });
    SerialHandle { tx, status_rx }
}
