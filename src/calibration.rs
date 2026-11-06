//! One-shot SDR calibration against a generated chart shown by a remote mpv.
use std::{fs, io::{Read, Write}, net::Shutdown, os::{fd::OwnedFd, unix::{fs::DirBuilderExt, net::UnixStream}},
    path::{Path, PathBuf}, process::{Child, Command, Stdio}, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
    thread, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use anyhow::{bail, ensure, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use image::ImageEncoder;
use crate::{capture::RgbaFrame, worker::Worker};

pub const KNOTS: usize = 17;
const PATCHES: usize = 64;
const TRAIN_END: usize = 44;
const TIMEOUT: Duration = Duration::from_secs(35);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub host: String,
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.host.is_empty() && self.host.len() <= 253 && !self.host.starts_with('-') &&
            self.host.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:@[]".contains(&b)),
            "Enter an SSH config alias or user@host, without spaces or shell commands");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Correction {
    // Measured linear R/G/B and reference gray, followed by row-major RGB mixing.
    pub knots: [[f32; 4]; KNOTS],
    pub matrix: [[f32; 4]; 3],
}

impl Default for Correction {
    fn default() -> Self {
        Self { knots: std::array::from_fn(|i| [i as f32 / 16.0; 4]),
            matrix: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]] }
    }
}

impl Correction {
    pub fn validate(&self) -> Result<()> {
        for knot in self.knots {
            ensure!(knot.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "Invalid calibration curve");
        }
        for pair in self.knots.windows(2) {
            ensure!((0..4).all(|c| pair[1][c] - pair[0][c] > 0.00001), "Clipped or non-monotonic gray ramp; check device controls");
        }
        ensure!(self.knots[0][3] == 0.0 && self.knots[16][3] == 1.0, "Invalid calibration endpoints");
        ensure!(self.matrix.iter().flatten().all(|v| v.is_finite() && v.abs() <= 3.0)
            && self.matrix.iter().all(|row| row[3] == 0.0), "Unstable RGB correction");
        Ok(())
    }

    fn curve(&self, value: f32, channel: usize) -> f32 {
        for pair in self.knots.windows(2) {
            if value <= pair[1][channel] {
                let t = ((value - pair[0][channel]) / (pair[1][channel] - pair[0][channel])).clamp(0.0, 1.0);
                return pair[0][3] + t * (pair[1][3] - pair[0][3]);
            }
        }
        1.0
    }

    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let linear: [f32; 3] = std::array::from_fn(|c| self.curve(decode(rgb[c]), c));
        std::array::from_fn(|r| encode((0..3).map(|c| self.matrix[r][c] * linear[c]).sum::<f32>().clamp(0.0, 1.0)))
    }
}

fn decode(v: f32) -> f32 { if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } }
fn encode(v: f32) -> f32 { if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Saved {
    pub profile: Profile,
    pub video: String,
    pub width: u32,
    pub height: u32,
    pub controls: Vec<(u32, i64)>,
    pub correction: Correction,
    pub before: f32,
    pub after: f32,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub profile: Profile,
    pub saved: Option<Saved>,
    #[serde(default)]
    pub manual: Option<crate::gpu::ColorSettings>,
    #[serde(default)]
    pub enabled: bool,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context("Read calibration profile"),
        };
        ensure!(data.len() <= 64 * 1024, "Calibration profile is too large");
        let settings: Self = serde_json::from_slice(&data).context("Invalid calibration profile")?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if let Some(manual) = self.manual { manual.validate()?; }
        if let Some(saved) = &self.saved {
            saved.profile.validate()?;
            dimensions(saved.width, saved.height)?;
            saved.correction.validate()?;
            ensure!([saved.before, saved.after].iter().all(|v| v.is_finite() && (0.0..=255.0).contains(v)), "Invalid calibration error");
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        crate::gpu::atomic_write(path, &serde_json::to_vec_pretty(self)?)
    }
}

fn dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(width >= 320 && height >= 240 && u64::from(width) * u64::from(height) <= 16_777_216, "Calibration needs a 320×240 to 16-megapixel video frame");
    Ok(())
}

fn reference(index: usize) -> [f32; 3] {
    let bytes = if index < KNOTS { [(index * 255 / 16) as u8; 3] }
    else if index < TRAIN_END {
        let n = index - KNOTS;
        let levels = [38, 128, 217];
        [levels[n / 9], levels[n / 3 % 3], levels[n % 3]]
    } else {
        let n = index - TRAIN_END;
        [32 + ((n * 47 + 17) % 190) as u8, 32 + ((n * 71 + 39) % 190) as u8, 32 + ((n * 31 + 83) % 190) as u8]
    };
    bytes.map(|v| v as f32 / 255.0)
}

fn chart(width: u32, height: u32, nonce: u32) -> Result<RgbaFrame> {
    dimensions(width, height)?;
    let mut rgba = vec![255; width as usize * height as usize * 4];
    for y in 0..height {
        let row = (u64::from(y) * 10 / u64::from(height)) as usize;
        for x in 0..width {
            let rgb = if row == 0 || row == 9 {
                let bit = u64::from(x) * 32 / u64::from(width);
                let on = ((nonce >> bit) & 1 != 0) ^ (row == 9);
                [if on { 232 } else { 24 }; 3]
            } else {
                reference((row - 1) * 8 + (u64::from(x) * 8 / u64::from(width)) as usize).map(|v| (v * 255.0).round() as u8)
            };
            let start = (y as usize * width as usize + x as usize) * 4;
            rgba[start..start + 3].copy_from_slice(&rgb);
        }
    }
    Ok(RgbaFrame { width, height, rgba })
}

fn sample(frame: &RgbaFrame, nonce: u32) -> Result<[[f32; 3]; PATCHES]> {
    dimensions(frame.width, frame.height)?;
    ensure!(frame.rgba.len() == frame.width as usize * frame.height as usize * 4, "Invalid captured frame");
    let patch = |column: usize, columns: usize, row: usize| -> Result<[f32; 3]> {
        let mut sums = [0.0f64; 3];
        let mut squares = [0.0f64; 3];
        let mut count = 0.0;
        let x0 = ((column as f64 + 0.3) * frame.width as f64 / columns as f64) as u32;
        let x1 = ((column as f64 + 0.7) * frame.width as f64 / columns as f64) as u32;
        let y0 = ((row as f64 + 0.3) * frame.height as f64 / 10.0) as u32;
        let y1 = ((row as f64 + 0.7) * frame.height as f64 / 10.0) as u32;
        for y in y0..y1 { for x in x0..x1 {
            let p = (y as usize * frame.width as usize + x as usize) * 4;
            for c in 0..3 { let v = frame.rgba[p + c] as f64; sums[c] += v; squares[c] += v * v; }
            count += 1.0;
        } }
        ensure!(count > 0.0 && (0..3).all(|c| squares[c] / count - (sums[c] / count).powi(2) < 100.0),
            "Test chart is obscured or not flat; move the pointer and close overlays");
        Ok(sums.map(|v| (v / count / 255.0) as f32))
    };
    for row in [0, 9] {
        let mut levels = [0.0; 32];
        for (bit, level) in levels.iter_mut().enumerate() { *level = patch(bit, 32, row)?.iter().sum::<f32>() / 3.0; }
        let low = levels.iter().copied().fold(1.0, f32::min);
        let high = levels.iter().copied().fold(0.0, f32::max);
        ensure!(high - low > 0.3, "Waiting for the current test chart on the selected output");
        for (bit, level) in levels.iter().enumerate() {
            ensure!((*level > (low + high) / 2.0) == (((nonce >> bit) & 1 != 0) ^ (row == 9)),
                "Waiting for the current full-screen chart; check output and aspect ratio");
        }
    }
    let mut samples = [[0.0; 3]; PATCHES];
    for (i, sample) in samples.iter_mut().enumerate() { *sample = patch(i % 8, 8, 1 + i / 8)?; }
    Ok(samples)
}

fn error(samples: &[[f32; 3]; PATCHES], correction: Option<&Correction>) -> f32 {
    let mut square = 0.0;
    for (i, sample) in samples.iter().enumerate().skip(TRAIN_END) {
        let actual = correction.map(|c| c.apply(*sample)).unwrap_or(*sample);
        let target = reference(i);
        for c in 0..3 { square += (actual[c] - target[c]).powi(2); }
    }
    (square / ((PATCHES - TRAIN_END) * 3) as f32).sqrt() * 255.0
}

fn fit(samples: &[[f32; 3]; PATCHES], previous: Correction) -> Result<(Correction, f32, f32)> {
    let before = error(samples, Some(&previous));
    let mut correction = Correction::default();
    for (i, knot) in correction.knots.iter_mut().enumerate() {
        *knot = [decode(samples[i][0]), decode(samples[i][1]), decode(samples[i][2]), decode(reference(i)[0])];
    }
    correction.validate()?;
    let mut system = [[0.0f64; 6]; 3];
    for (i, sample) in samples.iter().enumerate().take(TRAIN_END).skip(KNOTS) {
        let input: [f64; 3] = std::array::from_fn(|c| correction.curve(decode(sample[c]), c) as f64);
        let target = reference(i).map(|v| decode(v) as f64);
        for r in 0..3 { for c in 0..3 {
            system[r][c] += input[r] * input[c];
            system[r][c + 3] += input[r] * target[c];
        } }
    }
    for pivot in 0..3 {
        let row = (pivot..3).max_by(|&a, &b| system[a][pivot].abs().total_cmp(&system[b][pivot].abs())).unwrap();
        system.swap(pivot, row);
        let divisor = system[pivot][pivot];
        ensure!(divisor.abs() > 0.000001, "Insufficient color information for calibration");
        for v in &mut system[pivot] { *v /= divisor; }
        for r in 0..3 { if r != pivot {
            let factor = system[r][pivot];
            for c in 0..6 { system[r][c] -= factor * system[pivot][c]; }
        } }
    }
    for r in 0..3 { for (c, row) in system.iter().enumerate() { correction.matrix[r][c] = row[r + 3] as f32; } }
    correction.validate()?;
    let after = error(samples, Some(&correction));
    // shortcut: use held-out RGB RMSE for SDR; add perceptual metrics if hardware evidence needs them.
    if before <= 1.0 { return Ok((previous, before, before)); }
    ensure!(after <= 8.0 && after < before * 0.9, "Validation did not improve enough ({before:.2} → {after:.2} RGB RMSE); original settings kept");
    Ok((correction, before, after))
}

pub struct Job {
    pub frames: Sender<Arc<RgbaFrame>>,
    pub messages: Receiver<String>,
    pub result: Receiver<Saved>,
    pub worker: Worker,
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("nanokvm-calibration-{}-{stamp}-{}", std::process::id(), NEXT_ID.fetch_add(1, Ordering::Relaxed)));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temporary { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

struct Remote { child: Child, input: UnixStream }
impl Drop for Remote {
    fn drop(&mut self) {
        let _ = self.input.write_all(b"stop\n");
        let _ = self.input.shutdown(Shutdown::Write);
        let end = Instant::now() + Duration::from_millis(700);
        while Instant::now() < end {
            if self.child.try_wait().ok().flatten().is_some() { return; }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

// shortcut: discover one GNOME session for the SSH user; add other desktops when required.
const DISCOVER_SCRIPT: &str = r#"set -eu
command -v mpv >/dev/null || { echo 'Install remote tools: sudo pacman -S --needed openssh mpv' >&2; exit 1; }
command -v busctl >/dev/null || { echo 'Remote GNOME discovery needs busctl from systemd.' >&2; exit 1; }
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
display=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^WAYLAND_DISPLAY=//p')
case "$display" in ''|*[!a-zA-Z0-9_.-]*) display= ;; esac
if [ -z "$display" ] || [ ! -S "$XDG_RUNTIME_DIR/$display" ]; then
  display=
  for socket in "$XDG_RUNTIME_DIR"/wayland-*; do
    [ -S "$socket" ] || continue
    [ -z "$display" ] || { echo 'Multiple remote Wayland sessions; use one logged-in desktop for the SSH user.' >&2; exit 1; }
    display=${socket##*/}
  done
fi
[ -n "$display" ] || { echo 'No remote Wayland session. Log into GNOME as the SSH user.' >&2; exit 1; }
printf '%s\n' "$display"
busctl --address="unix:path=$XDG_RUNTIME_DIR/bus" --timeout=5 --auto-start=no --allow-interactive-authorization=no --json=short call org.gnome.Mutter.DisplayConfig /org/gnome/Mutter/DisplayConfig org.gnome.Mutter.DisplayConfig GetCurrentState
"#;

fn desktop(reply: &str) -> Result<(String, Vec<String>)> {
    let (display, data) = reply.split_once('\n').context("Missing remote desktop discovery reply")?;
    ensure!(display.starts_with("wayland-") && display.len() <= 128 &&
        display.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)), "Invalid remote Wayland socket");
    let state: serde_json::Value = serde_json::from_str(data).context("Invalid remote GNOME display reply")?;
    let monitors = state.pointer("/data/2").and_then(|v| v.as_array()).context("Missing active GNOME outputs")?;
    let mut outputs = Vec::new();
    for monitor in monitors {
        let specs = monitor.get(5).and_then(|v| v.as_array()).context("Invalid GNOME monitor configuration")?;
        for spec in specs {
            let output = spec.get(0).and_then(|v| v.as_str()).context("Missing GNOME output connector")?;
            ensure!(!output.is_empty() && output.len() <= 128 && !output.starts_with('-') &&
                output.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)), "Invalid remote output connector");
            if !outputs.iter().any(|s| s == output) { outputs.push(output.to_owned()); }
        }
    }
    ensure!(!outputs.is_empty() && outputs.len() <= 16, "No active GNOME outputs or too many outputs");
    Ok((display.to_owned(), outputs))
}

fn remote_script(display: &str, output: &str, length: usize) -> String {
    let script = format!(r#"set -eu
command -v mpv >/dev/null || {{ echo 'Install remote tools: sudo pacman -S --needed openssh mpv' >&2; exit 1; }}
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export WAYLAND_DISPLAY={display}
test -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" || {{ echo 'Remote Wayland session ended. Log into GNOME as the SSH user.' >&2; exit 1; }}
dir=$(mktemp -d /tmp/nanokvm-chart.XXXXXXXX)
player= reader=
cleanup() {{
  [ -z "$reader" ] || kill "$reader" 2>/dev/null || :
  [ -z "$player" ] || kill "$player" 2>/dev/null || :
  wait 2>/dev/null || :
  rm -rf -- "$dir"
}}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
exec 3<&0
head -c {length} <&3 > "$dir/chart.png"
test "$(wc -c < "$dir/chart.png")" -eq {length} || exit 1
mpv --no-config --vo=wlshm --fs --fs-screen-name={output} --image-display-duration=inf --no-audio --no-osc --osd-level=0 --cursor-autohide=always --input-terminal=no -- "$dir/chart.png" < /dev/null &
player=$!
(read -r stop <&3 || :; kill "$player" 2>/dev/null || :) &
reader=$!
wait "$player"
"#, display = quote(display), output = quote(output));
    // The remote deadline also cleans up when the network disappears without EOF.
    format!("timeout -k 2s 45s sh -c {}", quote(&script))
}

fn start_remote(ssh: &Path, host: &str, command: &str, stdout: fs::File, stderr: fs::File) -> Result<Remote> {
    let (input, child_input) = UnixStream::pair()?;
    input.set_write_timeout(Some(Duration::from_millis(100)))?;
    let child = Command::new(ssh).args(["-T", "-oBatchMode=yes", "-oStrictHostKeyChecking=yes", "-oConnectTimeout=5",
        "-oConnectionAttempts=1", "-oServerAliveInterval=2", "-oServerAliveCountMax=2", "-oClearAllForwardings=yes",
        "-oForwardAgent=no", "-oRemoteCommand=none", "--", host, command])
        .stdin(Stdio::from(OwnedFd::from(child_input))).stdout(stdout).stderr(stderr).spawn()
        .context("Start ssh; install openssh-client locally and configure key/agent login first")?;
    Ok(Remote { child, input })
}

fn discover_desktop(profile: &Profile, ssh: &Path, directory: &Path, log: &fs::File, stop: &AtomicBool, deadline: Instant) -> Result<(String, Vec<String>)> {
    let path = directory.join("desktop.json");
    let mut remote = start_remote(ssh, &profile.host, &format!("timeout -k 2s 10s sh -c {}", quote(DISCOVER_SCRIPT)),
        fs::File::create(&path)?, log.try_clone()?)?;
    loop {
        check(stop, deadline)?;
        if let Some(status) = remote.child.try_wait()? {
            ensure!(status.success(), "Could not discover the remote GNOME desktop ({status})");
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    ensure!(fs::metadata(&path)?.len() <= 1024 * 1024, "Remote display reply is too large");
    desktop(&fs::read_to_string(path)?)
}

fn check(stop: &AtomicBool, deadline: Instant) -> Result<()> {
    ensure!(!stop.load(Ordering::Acquire), "Calibration cancelled");
    ensure!(Instant::now() < deadline, "Calibration timed out");
    Ok(())
}

fn write_chart(remote: &mut Remote, data: &[u8], stop: &AtomicBool, deadline: Instant) -> Result<()> {
    let mut remaining = data;
    while !remaining.is_empty() {
        check(stop, deadline)?;
        match remote.input.write(remaining) {
            Ok(0) => bail!("SSH closed while sending the chart"),
            Ok(n) => remaining = &remaining[n..],
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {},
            Err(e) => return Err(e).context("Send chart over SSH"),
        }
    }
    Ok(())
}

fn sample_output(remote: &mut Remote, frames: &Receiver<Arc<RgbaFrame>>, width: u32, height: u32,
    nonce: u32, stop: &AtomicBool, deadline: Instant) -> Result<Option<[[f32; 3]; PATCHES]>> {
    let end = deadline.min(Instant::now() + Duration::from_secs(5));
    let mut previous_sample: Option<[[f32; 3]; PATCHES]> = None;
    let mut sum = [[0.0; 3]; PATCHES];
    let mut count = 0;
    loop {
        check(stop, deadline)?;
        if Instant::now() >= end { return Ok(None); }
        if let Some(status) = remote.child.try_wait()? { bail!("Remote display ended ({status})"); }
        let frame = match frames.recv_timeout(Duration::from_millis(50)) {
            Ok(frame) => frame,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(_) => bail!("Video connection ended"),
        };
        ensure!(frame.width == width && frame.height == height, "Video resolution changed during calibration");
        let measured = match sample(&frame, nonce) {
            Ok(value) => value,
            Err(_) => { previous_sample = None; count = 0; sum = [[0.0; 3]; PATCHES]; continue; }
        };
        if previous_sample.as_ref().is_some_and(|first| first.iter().flatten().zip(measured.iter().flatten()).any(|(a, b)| (a - b).abs() > 2.0 / 255.0)) {
            previous_sample = None; count = 0; sum = [[0.0; 3]; PATCHES];
        }
        for i in 0..PATCHES { for c in 0..3 { sum[i][c] += measured[i][c]; } }
        if previous_sample.is_none() { previous_sample = Some(measured); }
        count += 1;
        if count == 5 {
            for value in sum.iter_mut().flatten() { *value /= 5.0; }
            return Ok(Some(sum));
        }
    }
}

fn run(profile: &Profile, width: u32, height: u32, frames: &Receiver<Arc<RgbaFrame>>, messages: &Sender<String>, stop: &AtomicBool, ssh: &Path, previous: Correction) -> Result<(Correction, f32, f32)> {
    profile.validate()?;
    let directory = Temporary::new()?;
    let log_path = directory.0.join("ssh.log");
    let log = fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    let deadline = Instant::now() + TIMEOUT;
    let result = (|| {
        let _ = messages.try_send("Connecting over SSH and finding the remote desktop…".into());
        let (display, outputs) = discover_desktop(profile, ssh, &directory.0, &log, stop, deadline)?;
        for output in outputs {
            check(stop, deadline)?;
            let _ = messages.try_send(format!("Finding NanoKVM: displaying the chart on {output}…"));
            let nonce = 0xd300_0000 | ((SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u32 ^ NEXT_ID.fetch_add(1, Ordering::Relaxed) as u32) & 0x00ff_ffff);
            let frame = chart(width, height, nonce)?;
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png).write_image(&frame.rgba, width, height, image::ExtendedColorType::Rgba8)?;
            let mut remote = start_remote(ssh, &profile.host, &remote_script(&display, &output, png.len()), log.try_clone()?, log.try_clone()?)?;
            write_chart(&mut remote, &png, stop, deadline)?;
            let samples = sample_output(&mut remote, frames, width, height, nonce, stop, deadline)?;
            drop(remote);
            if let Some(samples) = samples {
                check(stop, deadline)?;
                let _ = messages.try_send(format!("NanoKVM chart confirmed on {output}; validating color correction…"));
                return fit(&samples, previous);
            }
        }
        bail!("No stable full-screen chart reached NanoKVM. Close remote overlays and check the capture connection and aspect ratio")
    })();
    ensure!(!stop.load(Ordering::Acquire), "Calibration cancelled");
    result.map_err(|error| {
        let mut log = String::new();
        if let Ok(file) = fs::File::open(log_path) { let _ = file.take(8192).read_to_string(&mut log); }
        error.context(format!("Calibration failed. Check SSH key/known-host setup and a logged-in remote GNOME desktop with mpv. {}", log.trim()))
    })
}

pub fn spawn(profile: Profile, video: String, width: u32, height: u32, controls: Vec<(u32, i64)>, previous: Correction) -> Result<Job> {
    profile.validate()?;
    dimensions(width, height)?;
    let (frames, rx) = bounded(1);
    let (tx, messages) = bounded(8);
    let (done, result) = bounded(1);
    let worker = Worker::spawn(move |stop| {
        let (correction, before, after) = run(&profile, width, height, &rx, &tx, &stop, Path::new("ssh"), previous).map_err(|e| format!("{e:#}"))?;
        if stop.load(Ordering::Acquire) { return Err("Calibration cancelled".into()); }
        done.send(Saved { profile, video, width, height, controls, correction, before, after }).map_err(|e| e.to_string())
    });
    Ok(Job { frames, messages, result, worker })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile { Profile { host: "desktop".into() } }

    #[test]
    fn chart_checks_nonce_geometry_and_recovers_gray_and_channel_bias() {
        let nonce = 0xd3ab_cdef;
        let mut frame = chart(640, 480, nonce).unwrap();
        let samples = sample(&frame, nonce).unwrap();
        let (neutral, before, after) = fit(&samples, Correction::default()).unwrap();
        assert!(before < 0.01 && after < 0.01);
        assert_eq!(neutral, Correction::default());
        assert!(sample(&frame, nonce ^ 1).is_err());
        for pixel in frame.rgba.chunks_exact_mut(4) {
            for c in 0..3 {
                let value = pixel[c] as f32 / 255.0;
                pixel[c] = ((0.035 + [0.86, 0.90, 0.83][c] * value.powf([1.12, 0.94, 1.04][c])) * 255.0).round() as u8;
            }
        }
        let measured = sample(&frame, nonce).unwrap();
        let (correction, before, after) = fit(&measured, Correction::default()).unwrap();
        assert!(before > 8.0 && after < 2.0, "{before} → {after}");
        for (i, input) in measured.iter().enumerate().skip(TRAIN_END) {
            let output = correction.apply(*input);
            assert!((0..3).all(|c| (output[c] - reference(i)[c]).abs() < 0.02));
        }
        frame.rgba.fill(128);
        assert!(sample(&frame, nonce).is_err());
    }

    #[test]
    fn clipped_ramps_and_unimproved_validation_are_rejected() {
        let mut samples = std::array::from_fn(reference);
        samples[1] = samples[0];
        assert!(fit(&samples, Correction::default()).is_err());
        let mut samples = std::array::from_fn(reference);
        for sample in samples.iter_mut().skip(TRAIN_END) { sample.swap(0, 2); }
        assert!(fit(&samples, Correction::default()).is_err());
        let mut bad = Correction::default();
        bad.matrix[0][0] = f32::NAN;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn profile_validation_atomic_save_and_old_color_preset_are_independent() {
        let dir = Temporary::new().unwrap();
        let path = dir.0.join("calibration.json");
        let settings = Settings { profile: profile(), ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().profile, profile());
        for host in ["desktop", "lab-pc", "user@host", "user@192.168.1.20", "user@[::1]"] {
            Profile { host: host.into() }.validate().unwrap();
        }
        let old: Profile = serde_json::from_str(r#"{"host":"desktop","name":"Old profile","display":"wayland-1","output":"HDMI-A-1"}"#).unwrap();
        assert_eq!(old, profile());
        assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"host":"desktop"}"#);
        let original = fs::read(&path).unwrap();
        for invalid in ["-oProxyCommand=bad", "host;id", "$(id)", "host\nother", "", "a b"] {
            let mut changed = settings.clone();
            changed.profile.host = invalid.into();
            assert!(changed.save(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        let legacy = dir.0.join("colors.json");
        crate::gpu::ColorSettings::SOFT_WHITE.save(&legacy).unwrap();
        settings.save(&path).unwrap();
        assert_eq!(crate::gpu::ColorSettings::load(&legacy).unwrap(), Some(crate::gpu::ColorSettings::SOFT_WHITE));
    }

    #[test]
    fn blocked_transfer_obeys_cancellation_and_deadline() {
        let (input, peer) = UnixStream::pair().unwrap();
        input.set_write_timeout(Some(Duration::from_millis(10))).unwrap();
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let mut remote = Remote { child, input };
        let start = Instant::now();
        assert!(write_chart(&mut remote, &vec![0; 4 * 1024 * 1024], &AtomicBool::new(false), start + Duration::from_millis(60)).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(write_chart(&mut remote, &[0], &AtomicBool::new(true), start + TIMEOUT).is_err());
        drop(peer);
    }

    fn executable(path: &Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn display_reply() -> String {
        serde_json::json!({"type": "ua((ssss)a(siiddada{sv})a{sv})a(iiduba(ssss)a{sv})a{sv}",
            "data": [1, [[["inactive", "", "", ""], [], {}]], [
                [0, 0, 1.0, 0, true, [["eDP-1", "", "", ""]], {}],
                [1920, 0, 1.0, 0, false, [["HDMI-A-1", "", "", ""], ["HDMI-A-1", "", "", ""]], {}]
            ], {}]}).to_string()
    }

    #[test]
    fn desktop_reply_uses_active_connectors_and_rejects_invalid_values() {
        let reply = format!("wayland-1\n{}", display_reply());
        assert_eq!(desktop(&reply).unwrap(), ("wayland-1".into(), vec!["eDP-1".to_owned(), "HDMI-A-1".to_owned()]));
        for invalid in [reply.replace("wayland-1", "../wayland-1"), reply.replace("HDMI-A-1", "$(id)"),
            "wayland-0\n{}".into(), "wayland-0\n{\"data\":[1,[],[],{}]}".into(), "garbage".into()] {
            assert!(desktop(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn discovery_script_finds_session_and_rejects_ambiguous_sockets() {
        use std::os::unix::net::UnixListener;
        let dir = Temporary::new().unwrap();
        let environment = dir.0.join("environment");
        fs::write(&environment, "").unwrap();
        executable(&dir.0.join("mpv"), "#!/bin/sh\nexit 0\n");
        executable(&dir.0.join("systemctl"), &format!("#!/bin/sh\ncat {}\n", quote(environment.to_str().unwrap())));
        executable(&dir.0.join("busctl"), &format!("#!/bin/sh\nprintf '%s\\n' {}\n", quote(&display_reply())));
        let script = DISCOVER_SCRIPT.replace("/run/user/$(id -u)", dir.0.to_str().unwrap());
        let run = || Command::new("sh").args(["-c", &script])
            .env("PATH", format!("{}:{}", dir.0.display(), std::env::var("PATH").unwrap()))
            .env_remove("WAYLAND_DISPLAY").output().unwrap();
        let missing = run();
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("No remote Wayland session"));
        let _first = UnixListener::bind(dir.0.join("wayland-1")).unwrap();
        let found = run();
        assert!(found.status.success());
        assert_eq!(desktop(&String::from_utf8(found.stdout).unwrap()).unwrap().0, "wayland-1");
        let _second = UnixListener::bind(dir.0.join("wayland-2")).unwrap();
        let ambiguous = run();
        assert!(!ambiguous.status.success());
        assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("Multiple remote Wayland sessions"));
        fs::write(&environment, "WAYLAND_DISPLAY=wayland-2\n").unwrap();
        let selected = run();
        assert!(selected.status.success());
        assert_eq!(desktop(&String::from_utf8(selected.stdout).unwrap()).unwrap().0, "wayland-2");
        fs::write(&environment, "WAYLAND_DISPLAY=$(false)\n").unwrap();
        assert!(!run().status.success());
    }

    #[test]
    fn ssh_fixture_transfers_chart_validates_frames_and_reports_remote_failure() {
        let dir = Temporary::new().unwrap();
        let upload = dir.0.join("uploaded.png");
        let ignored = dir.0.join("ignored.png");
        let closed = dir.0.join("closed");
        let args = dir.0.join("arguments");
        let ssh = dir.0.join("ssh");
        executable(&ssh, &format!(r#"#!/bin/sh
printf '%s\n' "$@" > {args}
for arg do last=$arg; done
case "$last" in
  *GetCurrentState*) printf '%s\n' wayland-1 {reply}; exit 0 ;;
  *eDP-1*) cat > {ignored}; exit 0 ;;
esac
cat > {upload}
touch {closed}
"#, args = quote(args.to_str().unwrap()), reply = quote(&display_reply()),
            ignored = quote(ignored.to_str().unwrap()), upload = quote(upload.to_str().unwrap()), closed = quote(closed.to_str().unwrap())));
        let (tx, rx) = bounded(1);
        let sender = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(12);
            loop {
                let correct = upload.exists();
                if let Ok(data) = fs::read(if correct { &upload } else { &ignored }) {
                    if let Ok(image) = image::load_from_memory(&data) {
                        let rgba = image.to_rgba8();
                        let (width, height) = rgba.dimensions();
                        let mut rgba = rgba.into_raw();
                        if !correct { rgba.fill(128); }
                        let frame = Arc::new(RgbaFrame { width, height, rgba });
                        if correct {
                            for _ in 0..6 {
                                if tx.send_timeout(frame.clone(), Duration::from_secs(1)).is_err() { break; }
                                thread::sleep(Duration::from_millis(10));
                            }
                            return;
                        }
                        if tx.send_timeout(frame, Duration::from_secs(1)).is_err() { return; }
                    }
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(5));
            }
        });
        let (messages, _) = bounded(8);
        let (_, before, after) = run(&profile(), 640, 480, &rx, &messages, &AtomicBool::new(false), &ssh, Correction::default()).unwrap();
        sender.join().unwrap();
        assert!(before < 0.01 && after < 0.01);
        assert!(closed.exists(), "SSH stdin must close on success");
        let args = fs::read_to_string(args).unwrap();
        assert!(args.contains("\n--\ndesktop\n"), "SSH config alias must be passed unchanged");
        assert!(args.contains("-oBatchMode=yes\n") && args.contains("-oStrictHostKeyChecking=yes\n"));
        assert!(args.contains("HDMI-A-1"), "The next output must be tried when no chart reaches capture");
        executable(&ssh, "#!/bin/sh\necho 'fixture: mpv missing' >&2\nexit 3\n");
        let (_tx, rx) = bounded(1);
        let result = run(&profile(), 640, 480, &rx, &messages, &AtomicBool::new(false), &ssh, Correction::default()).unwrap_err();
        assert!(format!("{result:#}").contains("fixture: mpv missing"));
    }

    #[test]
    fn remote_script_closes_only_its_player_and_cleans_files_on_eof() {
        use std::os::unix::net::UnixListener;
        let dir = Temporary::new().unwrap();
        let _socket = UnixListener::bind(dir.0.join("wayland-0")).unwrap();
        let marker = dir.0.join("started");
        let ended = dir.0.join("ended");
        let mpv = dir.0.join("mpv");
        executable(&mpv, &format!("#!/bin/sh\nfor arg do last=$arg; done\nprintf '%s' \"$last\" > {}\ntrap 'touch {}; exit 0' TERM\nwhile :; do sleep 0.1; done\n", quote(marker.to_str().unwrap()), quote(ended.to_str().unwrap())));
        let script = remote_script("wayland-0", "HDMI-A-1", 4).replace("/run/user/$(id -u)", dir.0.to_str().unwrap());
        let mut child = Command::new("sh").args(["-c", &script])
            .env("PATH", format!("{}:{}", dir.0.display(), std::env::var("PATH").unwrap()))
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"test").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(10)); }
        let chart = PathBuf::from(fs::read_to_string(marker).unwrap());
        assert_eq!(fs::read(&chart).unwrap(), b"test");
        drop(input);
        while child.try_wait().unwrap().is_none() { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(10)); }
        assert!(ended.exists());
        assert!(!chart.parent().unwrap().exists());
    }
}
