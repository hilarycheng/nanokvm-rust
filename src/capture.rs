//! MJPG from Linux V4L2 UVC. JPEG decode off the UI/GPU thread.
use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use jpeg_decoder::{Decoder, PixelFormat};
use std::{io::Cursor, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use crate::worker::Worker;
use v4l::{buffer::Type, io::traits::CaptureStream, prelude::MmapStream, video::Capture, Device, FourCC};

#[derive(Debug)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct CameraHandle {
    pub frames: Receiver<Arc<RgbaFrame>>,
    pub status: Receiver<String>,
    pub worker: Worker,
}

fn decode_mjpeg(data: &[u8]) -> Result<RgbaFrame> {
    let mut decoder = Decoder::new(Cursor::new(data));
    let pixels = decoder.decode().context("JPEG decode failed")?;
    let info = decoder.info().context("JPEG metadata missing")?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let count = (width as usize) * (height as usize);
    let mut rgba = Vec::with_capacity(count * 4);
    match info.pixel_format {
        PixelFormat::RGB24 => {
            for px in pixels.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        PixelFormat::L8 => {
            for gray in pixels {
                rgba.extend_from_slice(&[gray, gray, gray, 255]);
            }
        }
        _ => bail!("Unsupported MJPEG pixel layout"),
    }
    if rgba.len() != count * 4 {
        bail!("Unexpected MJPEG frame buffer length");
    }
    Ok(RgbaFrame { width, height, rgba })
}

fn capture_loop(device_path: String, frames: Sender<Arc<RgbaFrame>>, status: Sender<String>, stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) { return Ok(()); }
    let dev = Device::with_path(&device_path).context("open video device")?;
    let mut fmt = dev.format().context("query capture format")?;
    fmt.width = 1920;
    fmt.height = 1080;
    fmt.fourcc = FourCC::new(b"MJPG");
    let actual = dev.set_format(&fmt).context("set MJPG capture format")?;
    if actual.fourcc != FourCC::new(b"MJPG") {
        bail!("Device did not negotiate MJPG (reported {})", actual.fourcc);
    }
    let _ = status.try_send(format!("Video: {}×{} MJPG", actual.width, actual.height));

    let mut stream = MmapStream::with_buffers(&dev, Type::VideoCapture, 4).context("start capture")?;
    stream.set_timeout(Duration::from_millis(500));
    while !stop.load(Ordering::Acquire) {
        // A timeout exits this stream; retrying next() would requeue an undequeued buffer.
        let next = stream.next();
        if stop.load(Ordering::Acquire) { break; }
        let (buf, _) = next.context("read MJPG video frame (500 ms timeout)")?;
        // Fast reject when UI is not consuming; next decoder loop still uses latest frame.
        if frames.is_full() { continue; }
        match decode_mjpeg(buf) {
            Ok(img) => {
                // UI drains channel and retains only the latest available frame.
                let _ = frames.try_send(Arc::new(img));
            }
            Err(err) => {
                let _ = status.try_send(format!("Video decode: {err:#}"));
            }
        }
    }
    Ok(())
}

pub fn spawn(device: String) -> CameraHandle {
    let (frame_tx, frames) = bounded(2);
    let (status_tx, status) = bounded(8);
    let worker = Worker::spawn(move |stop| {
        capture_loop(device, frame_tx, status_tx, &stop).map_err(|err| format!("Video stopped: {err:#}"))
    });
    CameraHandle { frames, status, worker }
}
