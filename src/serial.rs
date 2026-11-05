//! Dedicated serial I/O with a shutdown path independent of the input queue.
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError, RecvTimeoutError};
use std::{io::{self, Write}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

use crate::{protocol, worker::Worker};

const WRITE_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone)]
pub enum Report {
    Keyboard([u8; 8]),
    AbsoluteMouse([u8; 7]),
    RelativeMouse([u8; 5]),
    Release([u8; 7]),
}

pub struct SerialHandle {
    tx: Sender<Report>,
    pending: Receiver<Report>,
    pub status_rx: Receiver<String>,
    pub worker: Worker,
}

impl SerialHandle {
    pub fn send(&mut self, report: Report) -> Result<(), TrySendError<Report>> {
        if self.worker.is_finished() { return Err(TrySendError::Disconnected(report)); }
        self.tx.try_send(report)
    }

    pub fn release(&mut self, mouse: [u8; 7]) -> Result<(), TrySendError<Report>> {
        if self.worker.is_finished() { return Err(TrySendError::Disconnected(Report::Release(mouse))); }
        // The UI is the only producer: draining guarantees space for this release.
        while self.pending.try_recv().is_ok() {}
        self.tx.try_send(Report::Release(mouse))
    }
}

fn write_packet(port: &mut impl Write, command: u8, bytes: &[u8]) -> io::Result<()> {
    let packet = protocol::encode(command, bytes).map_err(io::Error::other)?;
    let deadline = Instant::now() + WRITE_TIMEOUT;
    let mut remaining = packet.as_slice();
    while !remaining.is_empty() {
        if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
        match port.write(remaining) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => remaining = &remaining[n..],
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn release_reports(port: &mut impl Write, mouse: [u8; 7]) -> Result<(), String> {
    // Attempt both reports even when the first write fails.
    let keyboard = write_packet(port, protocol::SEND_KEYBOARD, &[0; 8]);
    let mouse = write_packet(port, protocol::SEND_MOUSE_ABSOLUTE, &mouse);
    match (keyboard, mouse) {
        (Ok(()), Ok(())) => Ok(()),
        (keyboard, mouse) => Err(format!("HID release failed: keyboard={keyboard:?}, mouse={mouse:?}")),
    }
}

fn transmit<W: Write>(port: &mut W, rx: &Receiver<Report>, stop: &AtomicBool, mut check: impl FnMut(&W) -> Result<(), String>) -> Result<(), String> {
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            check(port)?;
            let report = match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(report) => report,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if stop.load(Ordering::Acquire) { break; }
            let result = match report {
                Report::Keyboard(data) => write_packet(port, protocol::SEND_KEYBOARD, &data),
                Report::AbsoluteMouse(data) => write_packet(port, protocol::SEND_MOUSE_ABSOLUTE, &data),
                Report::RelativeMouse(data) => write_packet(port, protocol::SEND_MOUSE_RELATIVE, &data),
                Report::Release(mouse) => { release_reports(port, mouse)?; continue; }
            };
            result.map_err(|err| format!("Serial write failed: {err}"))?;
        }
        Ok(())
    })();
    let release = release_reports(port, protocol::mouse_absolute(0, 0.5, 0.5, 0));
    match (result, release) {
        (Err(err), Err(release)) => Err(format!("{err}; {release}")),
        (Err(err), _) | (_, Err(err)) => Err(err),
        _ => Ok(()),
    }
}

pub fn spawn(port_path: String) -> SerialHandle {
    let (tx, rx) = bounded::<Report>(256);
    let pending = rx.clone();
    let (status_tx, status_rx) = bounded::<String>(8);
    let worker = Worker::spawn(move |stop| {
        if stop.load(Ordering::Acquire) { return Ok(()); }
        let mut port = serialport::new(&port_path, protocol::BAUD)
            .timeout(WRITE_TIMEOUT)
            .open()
            .map_err(|err| format!("Serial open failed ({port_path}): {err}"))?;
        let _ = status_tx.try_send(format!("Serial connected: {port_path}"));
        transmit(&mut port, &rx, &stop, |port| {
            // FIONREAD detects a hung-up Linux tty without sending remote input.
            port.bytes_to_read().map(|_| ()).map_err(|err| format!("Serial connection lost: {err}"))
        })
    });
    SerialHandle { tx, pending, status_rx, worker }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn queued_port() -> (SerialHandle, Receiver<Report>) {
        let (tx, pending) = bounded(256);
        let (_, status_rx) = bounded(1);
        let worker = Worker::spawn(|stop| {
            while !stop.load(Ordering::Acquire) { std::thread::park_timeout(Duration::from_millis(1)); }
            Ok(())
        });
        (SerialHandle { tx, pending: pending.clone(), status_rx, worker }, pending)
    }

    pub fn spawn_port(mut port: impl Write + Send + 'static) -> SerialHandle {
        let (tx, rx) = bounded(256);
        let pending = rx.clone();
        let (status_tx, status_rx) = bounded(8);
        let worker = Worker::spawn(move |stop| {
            let _ = status_tx.try_send("Serial connected: mock".into());
            transmit(&mut port, &rx, &stop, |_| Ok(()))
        });
        SerialHandle { tx, pending, status_rx, worker }
    }

    #[test]
    fn shutdown_bypasses_full_queue() {
        let (tx, rx) = bounded(256);
        for _ in 0..256 { tx.send(Report::Keyboard([1; 8])).unwrap(); }
        let mut written = Vec::new();
        transmit(&mut written, &rx, &AtomicBool::new(true), |_| Ok(())).unwrap();
        let mut expected = protocol::encode(protocol::SEND_KEYBOARD, &[0; 8]).unwrap();
        expected.extend(protocol::encode(protocol::SEND_MOUSE_ABSOLUTE, &protocol::mouse_absolute(0, 0.5, 0.5, 0)).unwrap());
        assert_eq!(written, expected);
        assert_eq!(rx.len(), 256);
    }

    #[test]
    fn release_discards_stale_inputs_but_keeps_new_inputs_in_order() {
        let (tx, rx) = bounded(256);
        let (_, status_rx) = bounded(1);
        let mut handle = SerialHandle { tx, pending: rx.clone(), status_rx, worker: Worker::spawn(|stop| { while !stop.load(Ordering::Acquire) { std::thread::yield_now(); } Ok(()) }) };
        for _ in 0..256 { handle.send(Report::Keyboard([1; 8])).unwrap(); }
        let mouse = protocol::mouse_absolute(0, 0.25, 0.75, 0);
        handle.release(mouse).unwrap();
        handle.send(Report::Keyboard([2; 8])).unwrap();
        assert!(matches!(rx.recv().unwrap(), Report::Release(data) if data == mouse));
        assert!(matches!(rx.recv().unwrap(), Report::Keyboard([2, 2, 2, 2, 2, 2, 2, 2])));
        assert!(rx.is_empty());
    }

    #[test]
    fn repeated_interruptions_do_not_extend_the_packet_deadline() {
        struct InterruptedPort;
        impl Write for InterruptedPort {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> { Err(io::ErrorKind::Interrupted.into()) }
            fn flush(&mut self) -> io::Result<()> { Ok(()) }
        }
        assert_eq!(write_packet(&mut InterruptedPort, protocol::SEND_KEYBOARD, &[0; 8]).unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn failed_keyboard_release_still_attempts_mouse() {
        struct FailedPort(Vec<Vec<u8>>);
        impl Write for FailedPort {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                self.0.push(data.to_vec());
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> { Ok(()) }
        }
        let mut port = FailedPort(Vec::new());
        let (_, rx) = bounded(1);
        assert!(transmit(&mut port, &rx, &AtomicBool::new(true), |_| Ok(())).is_err());
        assert_eq!(port.0.len(), 2);
        assert_eq!(port.0[0][3], protocol::SEND_KEYBOARD);
        assert_eq!(port.0[1][3], protocol::SEND_MOUSE_ABSOLUTE);
    }

    #[test]
    fn input_write_failure_still_attempts_both_releases() {
        struct FailedPort(Vec<u8>);
        impl Write for FailedPort {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                self.0.push(data[3]);
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> { Ok(()) }
        }
        let (tx, rx) = bounded(1);
        tx.send(Report::Keyboard([1; 8])).unwrap();
        let mut port = FailedPort(Vec::new());
        let error = transmit(&mut port, &rx, &AtomicBool::new(false), |_| Ok(())).unwrap_err();
        assert!(error.contains("Serial write failed") && error.contains("HID release failed"));
        assert_eq!(port.0, [protocol::SEND_KEYBOARD, protocol::SEND_KEYBOARD, protocol::SEND_MOUSE_ABSOLUTE]);
    }
}
