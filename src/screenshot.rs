//! Export a snapshot of the decoded remote video without GPU adjustments or UI.
use std::{fs, io::Write, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{SystemTime, UNIX_EPOCH}};
use anyhow::{Context, Result};
use image::ImageEncoder;
use crate::{capture::RgbaFrame, worker::Worker};

#[derive(Clone, Copy, Debug)]
pub enum Format { Png, WebP }

impl Format {
    pub fn extension(self) -> &'static str {
        match self { Self::Png => "png", Self::WebP => "webp" }
    }
}

pub struct Screenshot {
    pub path: PathBuf,
    pub worker: Worker,
}

pub fn directory(home: Option<PathBuf>) -> Option<PathBuf> {
    home.filter(|path| path.is_absolute()).map(|path| path.join("Pictures/NanoKVM"))
}

pub fn spawn(frame: Arc<RgbaFrame>, format: Format, directory: &Path) -> Result<Screenshot> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let path = directory.join(format!("nanokvm-{}-{:09}-{}.{}",
        now.as_secs(), now.subsec_nanos(), std::process::id(), format.extension()));
    let destination = path.clone();
    let worker = Worker::spawn(move |stop| {
        save(&frame, format, &destination, &stop).map_err(|error| format!("{error:#}"))
    });
    Ok(Screenshot { path, worker })
}

fn save(frame: &RgbaFrame, format: Format, path: &Path, stop: &AtomicBool) -> Result<()> {
    anyhow::ensure!(!stop.load(Ordering::Acquire), "Screenshot cancelled");
    anyhow::ensure!(frame.width > 0 && frame.height > 0, "No video pixels to save");
    let expected = (frame.width as usize).checked_mul(frame.height as usize).and_then(|pixels| pixels.checked_mul(4));
    anyhow::ensure!(expected == Some(frame.rgba.len()), "Invalid RGBA frame length");
    let mut encoded = Vec::new();
    match format {
        Format::Png => image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&frame.rgba, frame.width, frame.height, image::ExtendedColorType::Rgba8)?,
        Format::WebP => image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .write_image(&frame.rgba, frame.width, frame.height, image::ExtendedColorType::Rgba8)?,
    }
    anyhow::ensure!(!stop.load(Ordering::Acquire), "Screenshot cancelled");
    fs::create_dir_all(path.parent().context("Screenshot has no destination directory")?)
        .context("Create screenshot directory")?;
    // Exclusive creation protects existing images even if two filenames collide.
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)
        .context("Create screenshot without overwriting")?;
    let result = file.write_all(&encoded).and_then(|_| file.sync_all());
    if result.is_err() { let _ = fs::remove_file(path); }
    result.context("Write screenshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_formats_preserve_native_dimensions_and_rgba_without_overwriting() {
        let root = std::env::temp_dir().join(format!("nanokvm-screenshots-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let frame = RgbaFrame { width: 3, height: 2, rgba: vec![
            0,0,0,255, 255,255,255,255, 100,150,200,255,
            255,0,0,255, 0,255,0,255, 0,0,255,255,
        ] };
        let stop = AtomicBool::new(false);
        for (format, decoder) in [(Format::Png, image::ImageFormat::Png), (Format::WebP, image::ImageFormat::WebP)] {
            let path = root.join(format!("native.{}", format.extension()));
            save(&frame, format, &path, &stop).unwrap();
            let encoded = fs::read(&path).unwrap();
            assert_eq!(image::guess_format(&encoded).unwrap(), decoder);
            let decoded = image::load_from_memory_with_format(&encoded, decoder).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (frame.width, frame.height));
            assert_eq!(decoded.as_raw(), &frame.rgba);
            assert!(save(&frame, format, &path, &stop).is_err());
            assert_eq!(fs::read(&path).unwrap(), encoded);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_frames_cancelled_jobs_and_write_failures_create_no_image() {
        let root = std::env::temp_dir().join(format!("nanokvm-screenshot-errors-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let path = root.join("failure.png");
        for frame in [
            RgbaFrame { width: 0, height: 1, rgba: vec![] },
            RgbaFrame { width: 2, height: 2, rgba: vec![0; 4] },
            RgbaFrame { width: u32::MAX, height: u32::MAX, rgba: vec![] },
        ] {
            for format in [Format::Png, Format::WebP] {
                assert!(save(&frame, format, &path, &AtomicBool::new(false)).is_err());
                assert!(!path.exists());
            }
        }
        let frame = RgbaFrame { width: 1, height: 1, rgba: vec![128,128,128,255] };
        assert!(save(&frame, Format::Png, &path, &AtomicBool::new(true)).is_err());
        assert!(!root.exists());
        let too_wide = RgbaFrame { width: 16385, height: 1, rgba: vec![255; 16385 * 4] };
        assert!(save(&too_wide, Format::WebP, &path, &AtomicBool::new(false)).is_err());
        assert!(!path.exists());
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, b"existing data").unwrap();
        assert!(save(&frame, Format::Png, &path.join("child.png"), &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing data");
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if fs::metadata(&root).unwrap().uid() != 0 {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
            let result = save(&frame, Format::Png, &root.join("denied.png"), &AtomicBool::new(false));
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(result.is_err());
            assert!(!root.join("denied.png").exists());
        }
        assert_eq!(directory(Some(PathBuf::from("/home/test"))), Some(PathBuf::from("/home/test/Pictures/NanoKVM")));
        assert_eq!(directory(Some(PathBuf::from("relative"))), None);
        assert_eq!(directory(Some(PathBuf::new())), None);
        assert_eq!(directory(None), None);
        fs::remove_dir_all(root).unwrap();
    }
}
