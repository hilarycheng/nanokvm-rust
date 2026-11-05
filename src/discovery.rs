//! Discovery for the USB layout recorded in DEVICE-IDENTIFICATION.md.
use std::{fs, io, path::{Path, PathBuf}};
use v4l::{capability::Flags, Device};

#[derive(Debug, PartialEq, Eq)]
pub struct DevicePair {
    pub video: String,
    pub serial: String,
    pub hub: PathBuf,
}

struct Endpoint {
    node: String,
    hub: PathBuf,
}

fn usb_id(path: &Path) -> io::Result<Option<(u16, u16)>> {
    let vendor = match fs::read_to_string(path.join("idVendor")) {
        Ok(value) => value,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let product = fs::read_to_string(path.join("idProduct"))?;
    let parse = |value: &str| u16::from_str_radix(value.trim(), 16).map_err(io::Error::other);
    Ok(Some((parse(&vendor)?, parse(&product)?)))
}

fn usb_parent(path: &Path) -> io::Result<Option<(PathBuf, (u16, u16))>> {
    for ancestor in path.ancestors() {
        if let Some(id) = usb_id(ancestor)? { return Ok(Some((ancestor.to_owned(), id))); }
    }
    Ok(None)
}

pub fn detect() -> Result<Vec<DevicePair>, String> {
    detect_at(Path::new("/sys/class"), Path::new("/dev"), |node| {
        let caps = Device::with_path(node)?.query_caps()?;
        Ok(caps.capabilities.contains(Flags::VIDEO_CAPTURE | Flags::STREAMING))
    })
}

fn detect_at(
    class_root: &Path,
    dev_root: &Path,
    capture: impl Fn(&Path) -> io::Result<bool>,
) -> Result<Vec<DevicePair>, String> {
    let mut videos = Vec::new();
    let mut serials = Vec::new();
    for (class, id, port, endpoints) in [
        ("video4linux", (0x345f, 0x2131), "2", &mut videos),
        ("tty", (0x1a86, 0x7523), "4", &mut serials),
    ] {
        let directory = class_root.join(class);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(format!("Cannot scan {}: {err}", directory.display())),
        };
        for entry in entries {
            let entry = entry.map_err(|err| format!("Device enumeration failed: {err}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let prefixes: &[&str] = if class == "tty" { &["ttyUSB", "ttyACM"] } else { &["video"] };
            if !prefixes.iter().any(|prefix| name.strip_prefix(prefix).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))) {
                continue;
            }
            let inspect = || -> io::Result<Option<Endpoint>> {
                let sys_path = fs::canonicalize(entry.path())?;
                let Some((usb, found)) = usb_parent(&sys_path)? else { return Ok(None); };
                if found != id { return Ok(None); }
                let node = dev_root.join(name.as_ref());
                fs::metadata(&node)?;
                if class == "video4linux" && !capture(&node)? { return Ok(None); }
                let hub = usb.parent().ok_or_else(|| io::Error::other("USB hub path missing"))?;
                // shortcut: only the observed hub ports are supported; add profiles after collecting device evidence.
                let child_port = usb.file_name().and_then(|n| n.to_str()).and_then(|n| n.rsplit_once('.')).map(|(_, n)| n);
                if usb_id(hub)? != Some((0x1a40, 0x0101)) || child_port != Some(port) {
                    return Err(io::Error::other("Unsupported USB topology; use manual Video and Serial paths"));
                }
                // Reject an endpoint that disappeared or moved while capabilities were queried.
                if fs::canonicalize(entry.path())? != sys_path {
                    return Err(io::Error::other("Device changed during discovery; rescan"));
                }
                Ok(Some(Endpoint { node: node.to_string_lossy().into_owned(), hub: hub.to_owned() }))
            };
            if let Some(endpoint) = inspect().map_err(|err| format!("Cannot identify {name}: {err}"))? {
                endpoints.push(endpoint);
            }
        }
    }
    match (videos.as_slice(), serials.as_slice()) {
        ([], []) => return Err("No supported NanoKVM video or serial device found; check the USB connection".into()),
        ([], _) => return Err("NanoKVM video capture device missing".into()),
        (_, []) => return Err("NanoKVM serial device missing".into()),
        _ => {}
    }
    let mut pairs = Vec::new();
    for video in &videos {
        if videos.iter().filter(|other| other.hub == video.hub).count() != 1 {
            return Err("Multiple video capture nodes on one USB hub; specify both paths manually".into());
        }
        let mut matches = serials.iter().filter(|serial| serial.hub == video.hub);
        let serial = matches.next().ok_or("Video and serial are on different USB hubs or a serial device is missing")?;
        if matches.next().is_some() {
            return Err("Multiple serial devices on one USB hub; specify both paths manually".into());
        }
        pairs.push(DevicePair { video: video.node.clone(), serial: serial.node.clone(), hub: video.hub.clone() });
    }
    if pairs.len() != serials.len() {
        return Err("NanoKVM video capture device missing on a USB hub".into());
    }
    pairs.sort_by(|a, b| a.hub.cmp(&b.hub));
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::symlink, sync::atomic::{AtomicUsize, Ordering}};

    #[test]
    #[ignore = "requires one connected NanoKVM; queries capabilities without streaming or serial I/O"]
    fn live_single_device_discovery() {
        let pairs = detect().expect("detect the connected NanoKVM");
        assert_eq!(pairs.len(), 1);
        let pair = &pairs[0];
        assert!(Path::new(&pair.video).exists() && Path::new(&pair.serial).exists());
        println!("Detected video={} serial={}", pair.video, pair.serial);
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("nanokvm-discovery-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(root.join("class/video4linux")).unwrap();
            fs::create_dir_all(root.join("class/tty")).unwrap();
            fs::create_dir_all(root.join("dev")).unwrap();
            Self(root)
        }
        fn endpoint(&self, name: &str, hub: &str, port: &str, id: (u16, u16)) {
            let parent = self.0.join("devices").join(hub);
            let usb = parent.join(format!("{hub}.{port}"));
            let endpoint = usb.join("interface").join(name);
            fs::create_dir_all(&endpoint).unwrap();
            for (path, ids) in [(&parent, (0x1a40, 0x0101)), (&usb, id)] {
                fs::write(path.join("idVendor"), format!("{:04x}\n", ids.0)).unwrap();
                fs::write(path.join("idProduct"), format!("{:04x}\n", ids.1)).unwrap();
            }
            let class = if name.starts_with("video") { "video4linux" } else { "tty" };
            symlink(endpoint, self.0.join("class").join(class).join(name)).unwrap();
            fs::write(self.0.join("dev").join(name), "").unwrap();
        }
        fn pair(&self, video: &str, serial: &str, hub: &str) {
            self.endpoint(video, hub, "2", (0x345f, 0x2131));
            self.endpoint(serial, hub, "4", (0x1a86, 0x7523));
        }
        fn detect(&self) -> Result<Vec<DevicePair>, String> {
            detect_at(&self.0.join("class"), &self.0.join("dev"), |path| Ok(path.file_name().unwrap() != "video1"))
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }

    #[test]
    fn pairs_shared_hub_and_excludes_metadata_and_unrelated_devices() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        f.endpoint("video1", "2-11", "2", (0x345f, 0x2131));
        f.endpoint("video5", "3-7", "2", (0x1111, 0x2222));
        f.endpoint("ttyUSB3", "3-7", "4", (0x1111, 0x2222));
        assert_eq!(f.detect().unwrap(), vec![DevicePair {
            video: f.0.join("dev/video0").to_string_lossy().into_owned(),
            serial: f.0.join("dev/ttyUSB0").to_string_lossy().into_owned(),
            hub: f.0.join("devices/2-11"),
        }]);
    }

    #[test]
    fn absent_and_partial_devices_are_not_paired() {
        let f = Fixture::new();
        assert!(f.detect().unwrap_err().contains("No supported"));
        f.endpoint("video0", "2-11", "2", (0x345f, 0x2131));
        assert!(f.detect().unwrap_err().contains("serial device missing"));
        fs::remove_file(f.0.join("class/video4linux/video0")).unwrap();
        f.endpoint("ttyUSB0", "2-11", "4", (0x1a86, 0x7523));
        assert!(f.detect().unwrap_err().contains("video capture device missing"));
    }

    #[test]
    fn multiple_pairs_keep_hub_identity_and_reject_cross_hub_or_ambiguous_endpoints() {
        let f = Fixture::new();
        f.endpoint("video0", "2-11", "2", (0x345f, 0x2131));
        f.endpoint("ttyUSB0", "2-12", "4", (0x1a86, 0x7523));
        assert!(f.detect().unwrap_err().contains("different USB hubs"));
        f.endpoint("ttyUSB2", "2-11", "4", (0x1a86, 0x7523));
        f.endpoint("video2", "2-12", "2", (0x345f, 0x2131));
        let pairs = f.detect().unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs[0].video.ends_with("video0") && pairs[0].serial.ends_with("ttyUSB2"));
        assert!(pairs[1].video.ends_with("video2") && pairs[1].serial.ends_with("ttyUSB0"));
        assert_eq!(pairs[0].hub, f.0.join("devices/2-11"));
        assert_eq!(pairs[1].hub, f.0.join("devices/2-12"));
        fs::remove_file(f.0.join("class/tty/ttyUSB0")).unwrap();
        fs::remove_file(f.0.join("class/video4linux/video2")).unwrap();
        f.endpoint("video4", "2-11", "2", (0x345f, 0x2131));
        assert!(f.detect().unwrap_err().contains("Multiple"));
        fs::remove_file(f.0.join("class/video4linux/video4")).unwrap();
        f.endpoint("ttyUSB4", "2-11", "4", (0x1a86, 0x7523));
        assert!(f.detect().unwrap_err().contains("Multiple serial"));
    }

    #[test]
    fn complete_pair_does_not_hide_an_incomplete_device() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        f.endpoint("ttyUSB2", "2-12", "4", (0x1a86, 0x7523));
        assert!(f.detect().unwrap_err().contains("video capture device missing"));
        fs::remove_file(f.0.join("class/tty/ttyUSB2")).unwrap();
        f.endpoint("video2", "2-12", "2", (0x345f, 0x2131));
        assert!(f.detect().unwrap_err().contains("serial device is missing"));
    }

    #[test]
    fn rescan_uses_new_node_numbers_and_hub_location() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        let old = f.detect().unwrap();
        fs::remove_file(f.0.join("class/video4linux/video0")).unwrap();
        fs::remove_file(f.0.join("class/tty/ttyUSB0")).unwrap();
        assert!(f.detect().is_err());
        f.pair("video6", "ttyUSB4", "3-7");
        let new = f.detect().unwrap();
        assert_ne!(new, old);
        assert!(new[0].video.ends_with("video6") && new[0].serial.ends_with("ttyUSB4"));
    }

    #[test]
    fn unknown_topology_and_unreadable_candidates_fail_closed() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        fs::write(f.0.join("devices/2-11/idProduct"), "9999").unwrap();
        assert!(f.detect().unwrap_err().contains("Unsupported USB topology"));
        fs::write(f.0.join("devices/2-11/idProduct"), "0101").unwrap();
        let error = detect_at(&f.0.join("class"), &f.0.join("dev"), |_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
        assert!(error.contains("Cannot identify video0"));
        fs::remove_file(f.0.join("dev/ttyUSB0")).unwrap();
        assert!(f.detect().unwrap_err().contains("Cannot identify ttyUSB0"));
        fs::write(f.0.join("devices/2-11/2-11.2/idVendor"), "invalid").unwrap();
        assert!(f.detect().unwrap_err().contains("Cannot identify video0"));
    }

    #[test]
    fn wrong_child_port_is_not_treated_as_the_recorded_pair() {
        let f = Fixture::new();
        f.endpoint("video0", "2-11", "3", (0x345f, 0x2131));
        f.endpoint("ttyUSB0", "2-11", "4", (0x1a86, 0x7523));
        assert!(f.detect().unwrap_err().contains("Unsupported USB topology"));
    }

    #[test]
    fn removal_during_capability_query_aborts_discovery() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        let result = detect_at(&f.0.join("class"), &f.0.join("dev"), |_| {
            fs::remove_file(f.0.join("class/video4linux/video0"))?;
            Ok(true)
        });
        assert!(result.unwrap_err().contains("Cannot identify video0"));
    }

    #[test]
    fn vanished_sysfs_entry_is_not_silently_ignored() {
        let f = Fixture::new();
        f.pair("video0", "ttyUSB0", "2-11");
        symlink(f.0.join("gone"), f.0.join("class/video4linux/video9")).unwrap();
        assert!(f.detect().unwrap_err().contains("Cannot identify video9"));
    }
}
