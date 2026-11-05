//! MJPG from Linux V4L2 UVC. JPEG decode off the UI/GPU thread.
use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use jpeg_decoder::{Decoder, PixelFormat};
use std::{io::Cursor, sync::Arc, thread};
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

fn capture_loop(device_path: String, frames: Sender<Arc<RgbaFrame>>, status: Sender<String>) -> Result<()> {
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
    loop {
        let (buf, _) = stream.next().context("read MJPG video frame")?;
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
}

pub fn spawn(device: String) -> CameraHandle {
    let (frame_tx, frames) = bounded(2);
    let (status_tx, status) = bounded(8);
    thread::spawn(move || {
        if let Err(err) = capture_loop(device, frame_tx, status_tx.clone()) {
            let _ = status_tx.try_send(format!("Video stopped: {err:#}"));
        }
    });
    CameraHandle { frames, status }
}
