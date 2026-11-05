use std::{fs, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use eframe::egui;
use crate::worker::SHUTDOWN_TIMEOUT;

use crate::{capture::{self, CameraHandle, RgbaFrame}, gpu::{ColorSettings, GpuVideo, VideoCallback}, input, protocol::{self, KeyboardReport}, serial::{self, SerialHandle, Report}};

pub struct Options {
    pub video: String,
    pub serial: String,
}

pub struct Viewer {
    camera: Option<CameraHandle>,
    video_path: String,
    disconnecting: Option<Instant>,
    serial: Option<SerialHandle>,
    serial_path: String,
    serial_ready: bool,
    video_status: String,
    serial_status: String,
    image: Option<Arc<RgbaFrame>>,
    new_frame: Option<Arc<RgbaFrame>>,
    colors: ColorSettings,
    colors_open: bool,
    remote: bool,
    remote_super: bool,
    fullscreen: bool,
    keyboard: KeyboardReport,
    buttons: u8,
    last_mouse: (f32, f32),
    next_mouse_update: Instant,
    frames_seen: u64,
    fps: f32,
    fps_clock: Instant,
    fps_frames: u32,
}

fn config_path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))?;
    Some(root.join("nanokvm-rust").join("colors.json"))
}

impl Viewer {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        let colors = config_path()
            .and_then(|p| fs::read(p).ok())
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let renderer = GpuVideo::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(renderer);
        }
        let mut app = Self::disconnected(opts);
        app.colors = colors;
        app
    }

    fn disconnected(opts: Options) -> Self {
        Self {
            camera: None,
            video_path: opts.video,
            disconnecting: None,
            serial: None,
            serial_path: opts.serial,
            serial_ready: false,
            video_status: "Disconnected".to_owned(),
            serial_status: "Disconnected".to_owned(),
            image: None,
            new_frame: None,
            colors: ColorSettings::default(),
            colors_open: false,
            remote: false,
            remote_super: false,
            fullscreen: false,
            keyboard: KeyboardReport::default(),
            buttons: 0,
            last_mouse: (0.5, 0.5),
            next_mouse_update: Instant::now(),
            frames_seen: 0,
            fps: 0.0,
            fps_clock: Instant::now(),
            fps_frames: 0,
        }
    }

    fn connect(&mut self) {
        if self.camera.is_some() || self.serial.is_some() || self.disconnecting.is_some() { return; }
        if self.video_path.trim().is_empty() || self.serial_path.trim().is_empty() {
            self.video_status = "Set both Video and Serial paths before connecting".into();
            return;
        }
        self.camera = Some(capture::spawn(self.video_path.trim().to_owned()));
        self.serial = Some(serial::spawn(self.serial_path.trim().to_owned()));
        self.video_status = "Connecting to V4L2…".into();
        self.serial_status = "Connecting…".into();
        self.serial_ready = false;
    }

    fn clear_input(&mut self) {
        self.remote = false;
        self.remote_super = false;
        self.keyboard.release();
        self.buttons = 0;
        self.last_mouse = (0.5, 0.5);
        self.next_mouse_update = Instant::now();
    }

    fn disconnect(&mut self) {
        self.clear_input();
        self.serial_ready = false;
        self.image = None;
        self.new_frame = None;
        self.frames_seen = 0;
        self.fps = 0.0;
        self.fps_frames = 0;
        self.fps_clock = Instant::now();
        if self.disconnecting.is_some() { return; }
        if self.camera.is_none() && self.serial.is_none() { return; }
        self.disconnecting = Some(Instant::now());
        self.video_status = "Disconnecting…".into();
        self.serial_status = "Disconnecting…".into();
        if let Some(camera) = &self.camera { camera.worker.stop(); }
        if let Some(serial) = &self.serial { serial.worker.stop(); }
    }

    fn poll_disconnect(&mut self) {
        let Some(started) = self.disconnecting else { return; };
        if let Some(result) = self.camera.as_mut().and_then(|camera| camera.worker.finish()) {
            self.video_status = result.err().unwrap_or_else(|| "Disconnected".into());
            self.camera = None;
        }
        if let Some(result) = self.serial.as_mut().and_then(|serial| serial.worker.finish()) {
            self.serial_status = result.err().unwrap_or_else(|| "Disconnected".into());
            self.serial = None;
        }
        if self.camera.is_none() && self.serial.is_none() {
            self.disconnecting = None;
        } else if started.elapsed() >= SHUTDOWN_TIMEOUT {
            if self.camera.is_some() { self.video_status = "Video still stopping; reconnect unavailable".into(); }
            if self.serial.is_some() { self.serial_status = "Serial still stopping; reconnect unavailable".into(); }
        }
    }

    fn send(&mut self, report: Report) {
        if self.disconnecting.is_some() { return; }
        if let Some(port) = self.serial.as_mut() {
            if port.send(report).is_err() {
                self.serial_status = "USB Serial TX queue full or closed".to_owned();
                self.serial_ready = false;
                self.release_remote();
            }
        }
    }

    fn send_keyboard(&mut self) {
        self.send(Report::Keyboard(self.keyboard.bytes()));
    }

    fn send_pointer(&mut self, wheel: i8) {
        self.send(Report::AbsoluteMouse(protocol::mouse_absolute(
            self.buttons, self.last_mouse.0, self.last_mouse.1, wheel,
        )));
    }

    fn release_remote(&mut self) {
        let mouse = protocol::mouse_absolute(0, self.last_mouse.0, self.last_mouse.1, 0);
        self.clear_input();
        if self.disconnecting.is_none() {
            if let Some(port) = self.serial.as_mut() {
                if port.release(mouse).is_err() {
                    self.serial_status = "Could not queue HID release: serial worker closed".into();
                }
            }
        }
    }

    fn refresh_messages(&mut self) {
        if self.disconnecting.is_some() { self.poll_disconnect(); return; }
        if let Some(camera) = &self.camera {
            while let Ok(msg) = camera.status.try_recv() { self.video_status = msg; }
            while let Ok(img) = camera.frames.try_recv() {
                self.image = Some(img.clone());
                self.new_frame = Some(img);
                self.frames_seen += 1;
                self.fps_frames += 1;
            }
        }
        if let Some(serial) = &self.serial {
            while let Ok(msg) = serial.status_rx.try_recv() {
                self.serial_ready = msg.starts_with("Serial connected:");
                self.serial_status = msg;
            }
        }
        // A failed partial connection must close the other worker before retrying.
        if self.camera.as_ref().is_some_and(|c| c.worker.is_finished()) ||
            self.serial.as_ref().is_some_and(|s| s.worker.is_finished()) {
            self.disconnect();
            self.poll_disconnect();
            return;
        }
        let elapsed = self.fps_clock.elapsed();
        if elapsed >= Duration::from_millis(1000) {
            self.fps = self.fps_frames as f32 / elapsed.as_secs_f32();
            self.fps_frames = 0;
            self.fps_clock = Instant::now();
        }
    }

    fn save_colors(&mut self) {
        let Some(path) = config_path() else {
            self.video_status = "HOME/XDG_CONFIG_HOME unavailable".to_owned();
            return;
        };
        let success = path.parent()
            .map(|p| fs::create_dir_all(p))
            .and_then(|r| r.ok())
            .and_then(|_| serde_json::to_vec_pretty(&self.colors).ok())
            .and_then(|data| fs::write(&path, data).ok());
        self.video_status = (if success.is_some() { "Color preset saved" } else { "Could not save preset" }).to_owned();
    }

    fn color_panel(&mut self, ctx: &egui::Context) {
        if !self.colors_open { return; }
        egui::SidePanel::right("color_tuning")
            .resizable(false)
            .default_width(270.0)
            .show(ctx, |ui| {
                ui.heading("GPU Color Tuning");
                ui.label("Only the remote video is affected.");
                ui.add_space(8.0);
                ui.add(egui::Slider::new(&mut self.colors.white, 0.55..=1.0)
                    .text("White peak").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut self.colors.highlights, 0.0..=1.0)
                    .text("Highlights").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut self.colors.gamma, 0.60..=1.60)
                    .text("Gamma").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut self.colors.exposure, -2.0..=2.0)
                    .text("Exposure (EV)").fixed_decimals(2));
                ui.add(egui::Slider::new(&mut self.colors.saturation, 0.0..=1.5)
                    .text("Saturation").fixed_decimals(2));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reset 1:1").clicked() { self.colors = ColorSettings::default(); }
                    if ui.button("Soft white").clicked() {
                        self.colors = ColorSettings { white: 0.84, ..Default::default() };
                    }
                });
                if ui.button("Save preset").clicked() { self.save_colors(); }
                ui.add_space(10.0);
                ui.label("Capture V4L2 at accurate baseline; tone-map only in GPU.");
                ui.small("Sway physical Super key interception is not yet implemented.");
            });
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("rdp_toolbar")
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("NanoKVM Rust");
                    let active = self.camera.is_some() || self.serial.is_some();
                    if ui.add_enabled(!active && self.disconnecting.is_none(), egui::Button::new("Connect")).clicked() {
                        self.connect();
                    }
                    if ui.add_enabled(active && self.disconnecting.is_none(), egui::Button::new("Disconnect")).clicked() {
                        self.disconnect();
                    }
                    ui.separator();
                    if self.remote {
                        if ui.button("■ Release input (Esc)").clicked() {
                            self.release_remote();
                        }
                    } else if ui.add_enabled(self.serial_ready && self.image.is_some(), egui::Button::new("▶ Capture input")).clicked() {
                        self.remote = true;
                    }
                    if ui.add_enabled(self.remote, egui::Button::new(
                        if self.remote_super { "Super: ON" } else { "Super: OFF" }
                    )).clicked() {
                        self.remote_super = !self.remote_super;
                        self.keyboard.modifiers = if self.remote_super {
                            self.keyboard.modifiers | protocol::modifiers::LEFT_SUPER
                        } else {
                            self.keyboard.modifiers & !protocol::modifiers::LEFT_SUPER
                        };
                        self.send_keyboard();
                    }
                    if ui.button("Color").clicked() {
                        self.release_remote();
                        self.colors_open = !self.colors_open;
                    }
                    if ui.button(if self.fullscreen { "Windowed" } else { "Fullscreen" }).clicked() {
                        self.fullscreen = !self.fullscreen;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                    }
                    ui.separator();
                    ui.label(format!("{:.0} FPS", self.fps));
                });
            });
        egui::TopBottomPanel::bottom("status")
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.small(&self.video_status);
                    ui.separator();
                    ui.small(&self.serial_status);
                    ui.separator();
                    ui.add_enabled_ui(self.camera.is_none() && self.serial.is_none(), |ui| {
                        ui.label("Video:");
                        ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut self.video_path));
                        ui.label("Serial:");
                        ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut self.serial_path));
                    });
                });
            });
    }

    fn render_video(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.image.is_none() {
                ui.centered_and_justified(|ui| { ui.label("No video frame — connect devices to start"); });
                return;
            }
            let outer = ui.available_rect_before_wrap();
            let (width, height) = self.image.as_ref()
                .map(|f| (f.width, f.height)).unwrap_or((1920, 1080));
            let rect = input::fit_rect(outer, width, height);
            // Real GPU shader draws the video inside this viewport.
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                VideoCallback {
                    frame: self.new_frame.take(),
                    settings: self.colors,
                },
            ));
            let response = ui.interact(rect, ui.id().with("video"), egui::Sense::click_and_drag());
            if !self.remote && response.double_clicked() && self.serial_ready {
                self.remote = true;
            }
            if self.remote {
                self.handle_mouse(ctx, rect);
            }
        });
    }

    fn handle_mouse(&mut self, ctx: &egui::Context, video_rect: egui::Rect) {
        let now = Instant::now();
        let (pos, scroll, events) = ctx.input(|i| (
            i.pointer.hover_pos(),
            i.raw_scroll_delta.y,
            i.events.clone(),
        ));
        if let Some(pos) = pos {
            if video_rect.contains(pos) && now >= self.next_mouse_update {
                let point = input::normalized_point(video_rect, pos);
                if (self.last_mouse.0 - point.0).abs() > 0.0002 ||
                   (self.last_mouse.1 - point.1).abs() > 0.0002 {
                    self.last_mouse = point;
                    self.send_pointer(0);
                }
                self.next_mouse_update = now + Duration::from_millis(12);
            }
        }
        for event in events {
            if let egui::Event::PointerButton { pos, button, pressed, .. } = event {
                let bit = match button {
                    egui::PointerButton::Primary => 1,
                    egui::PointerButton::Secondary => 2,
                    egui::PointerButton::Middle => 4,
                    egui::PointerButton::Extra1 => 8,
                    egui::PointerButton::Extra2 => 16,
                };
                // Release any held button even if mouse went outside video.
                if pressed && !video_rect.contains(pos) { continue; }
                if pressed { self.buttons |= bit; }
                else { self.buttons &= !bit; }
                self.last_mouse = input::normalized_point(video_rect, pos);
                self.send_pointer(0);
            }
        }
        if scroll != 0.0 && pos.is_some_and(|p| video_rect.contains(p)) {
            // HID wheel is discrete; scale / clamp UI pixel deltas.
            let wheel = if scroll > 0.0 { 1 } else { -1 };
            self.send_pointer(wheel);
        }
    }
}

impl eframe::App for Viewer {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if !self.remote { return; }
        // EgUI 0.31 provides Ctrl/Alt/Shift but NOT native Super state on Linux.
        if input::sync_modifiers(&mut self.keyboard, input.modifiers, self.remote_super) {
            self.send_keyboard();
        }
        for event in &input.events {
            if let egui::Event::Key { key, pressed, repeat, .. } = event {
                if *key == egui::Key::Escape && *pressed {
                    self.release_remote();
                    break;
                }
                if *repeat && *pressed { continue; }
                if let Some(usage) = input::map_key(*key) {
                    self.keyboard.set_key(usage, *pressed);
                    self.send_keyboard();
                }
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_messages();
        if self.remote && !ctx.input(|i| i.focused) {
            self.release_remote();
        }
        self.toolbar(ctx);
        self.color_panel(ctx);
        self.render_video(ctx);
        // Do not busy-spin; native V4L2 decode is on a dedicated thread.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.disconnect();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        if let Some(camera) = &mut self.camera {
            if let Err(err) = camera.worker.wait_until(deadline) { eprintln!("{err}"); }
        }
        if let Some(serial) = &mut self.serial {
            if let Err(err) = serial.worker.wait_until(deadline) { eprintln!("{err}"); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::Worker;
    use crossbeam_channel::{bounded, Receiver, Sender};
    use std::{io::{self, Write}, sync::{Mutex, atomic::Ordering}, thread};

    fn viewer() -> Viewer {
        Viewer::disconnected(Options {
            video: "/nonexistent-nanokvm-t05/video".into(),
            serial: "/nonexistent-nanokvm-t05/serial".into(),
        })
    }

    struct Port {
        bytes: Arc<Mutex<Vec<u8>>>,
        _resource: Sender<()>,
    }
    impl Write for Port {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    fn camera(fail: bool) -> (CameraHandle, Receiver<()>) {
        let (frame_tx, frames) = bounded(2);
        let (status_tx, status) = bounded(8);
        let (resource, closed) = bounded(1);
        let worker = Worker::spawn(move |stop| {
            let _resources = (resource, frame_tx, status_tx);
            if fail { return Err("Mock video open failed".into()); }
            while !stop.load(Ordering::Acquire) { thread::yield_now(); }
            Ok(())
        });
        (CameraHandle { frames, status, worker }, closed)
    }

    fn serial() -> (SerialHandle, Receiver<()>, Arc<Mutex<Vec<u8>>>) {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (resource, closed) = bounded(1);
        let port = Port { bytes: bytes.clone(), _resource: resource };
        (serial::tests::spawn_port(port), closed, bytes)
    }

    fn wait_disconnected(app: &mut Viewer) {
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while app.camera.is_some() || app.serial.is_some() {
            app.refresh_messages();
            assert!(Instant::now() < deadline, "workers did not close");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(app.disconnecting.is_none());
    }

    fn assert_neutral(bytes: &[u8]) {
        let mut expected = protocol::encode(protocol::SEND_KEYBOARD, &[0; 8]).unwrap();
        expected.extend(protocol::encode(protocol::SEND_MOUSE_ABSOLUTE, &protocol::mouse_absolute(0, 0.5, 0.5, 0)).unwrap());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn starts_disconnected_and_failed_connections_can_retry() {
        let mut app = viewer();
        assert!(app.camera.is_none() && app.serial.is_none());
        app.disconnect();
        for _ in 0..2 {
            app.connect();
            assert!(app.camera.is_some() && app.serial.is_some());
            wait_disconnected(&mut app);
            assert!(app.video_status.contains("open video device") || app.serial_status.contains("Serial open failed"));
        }
    }

    #[test]
    fn disconnect_clears_state_joins_resources_and_blocks_early_reconnect() {
        let mut app = viewer();
        let (camera, video_closed) = camera(false);
        let (serial, serial_closed, bytes) = serial();
        app.camera = Some(camera);
        app.serial = Some(serial);
        app.remote = true;
        app.remote_super = true;
        app.serial_ready = true;
        app.keyboard.set_key(4, true);
        app.keyboard.modifiers = 8;
        app.buttons = 7;
        app.image = Some(Arc::new(RgbaFrame { width: 1, height: 1, rgba: vec![0; 4] }));
        app.new_frame = app.image.clone();
        app.disconnect();
        app.disconnect();
        app.connect();
        assert!(app.disconnecting.is_some());
        assert!(!app.remote && !app.remote_super && !app.serial_ready);
        assert_eq!(app.keyboard.bytes(), [0; 8]);
        assert_eq!(app.buttons, 0);
        assert!(app.image.is_none() && app.new_frame.is_none());
        wait_disconnected(&mut app);
        assert!(video_closed.recv().is_err());
        assert!(serial_closed.recv().is_err());
        assert_neutral(&bytes.lock().unwrap());
        app.connect();
        assert!(app.camera.is_some() && app.serial.is_some());
        wait_disconnected(&mut app);
    }

    #[test]
    fn either_partial_open_failure_closes_the_other_worker() {
        for video_fails in [true, false] {
            let mut app = viewer();
            let (camera, video_closed) = camera(video_fails);
            let (working_serial, serial_closed, _) = serial();
            app.camera = Some(camera);
            app.serial = Some(if video_fails { working_serial } else {
                drop(working_serial);
                serial::spawn(app.serial_path.clone())
            });
            wait_disconnected(&mut app);
            assert!(video_closed.recv().is_err());
            assert_eq!(serial_closed.recv_timeout(SHUTDOWN_TIMEOUT), Err(crossbeam_channel::RecvTimeoutError::Disconnected));
        }
    }

    #[test]
    fn app_drop_waits_for_both_resources_and_neutral_reports() {
        let mut app = viewer();
        let (camera, video_closed) = camera(false);
        let (serial, serial_closed, bytes) = serial();
        app.camera = Some(camera);
        app.serial = Some(serial);
        drop(app);
        assert_eq!(video_closed.try_recv(), Err(crossbeam_channel::TryRecvError::Disconnected));
        assert_eq!(serial_closed.try_recv(), Err(crossbeam_channel::TryRecvError::Disconnected));
        assert_neutral(&bytes.lock().unwrap());
    }
}
