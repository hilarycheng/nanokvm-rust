use std::{fs, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use eframe::egui;
use crate::worker::SHUTDOWN_TIMEOUT;
use crate::discovery::{self, DevicePair};
use crossbeam_channel::{bounded, Receiver};

use crate::{capture::{self, CameraHandle, RgbaFrame}, gpu::{ColorSettings, GpuVideo, VideoCallback}, input, protocol::{self, KeyboardReport}, serial::{self, SerialHandle, Report}};

pub struct Options {
    pub video: String,
    pub serial: String,
}

pub struct Viewer {
    camera: Option<CameraHandle>,
    video_path: String,
    automatic: bool,
    discovery_status: String,
    discovery_rx: Option<Receiver<Result<DevicePair, String>>>,
    connect_after_discovery: bool,
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
    map_f12_to_meta: bool,
    mapped_meta_held: bool,
    fullscreen: bool,
    keyboard: KeyboardReport,
    buttons: u8,
    last_mouse: (f32, f32),
    pointer_pos: Option<egui::Pos2>,
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
        if app.automatic { app.discover(false); }
        app
    }

    fn disconnected(opts: Options) -> Self {
        Self {
            camera: None,
            automatic: opts.video.trim().is_empty() && opts.serial.trim().is_empty(),
            discovery_status: "Disconnected".into(),
            discovery_rx: None,
            connect_after_discovery: false,
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
            map_f12_to_meta: false,
            mapped_meta_held: false,
            fullscreen: false,
            keyboard: KeyboardReport::default(),
            buttons: 0,
            last_mouse: (0.5, 0.5),
            pointer_pos: None,
            next_mouse_update: Instant::now(),
            frames_seen: 0,
            fps: 0.0,
            fps_clock: Instant::now(),
            fps_frames: 0,
        }
    }

    fn discover(&mut self, connect: bool) {
        if self.camera.is_some() || self.serial.is_some() || self.disconnecting.is_some() || self.discovery_rx.is_some() { return; }
        self.video_path.clear();
        self.serial_path.clear();
        self.discovery_status = "Looking for NanoKVM…".into();
        self.connect_after_discovery = connect;
        let (tx, rx) = bounded(1);
        self.discovery_rx = Some(rx);
        std::thread::spawn(move || { let _ = tx.send(discovery::detect()); });
    }

    fn refresh_discovery(&mut self) {
        let Some(rx) = &self.discovery_rx else { return; };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Err("Device discovery stopped unexpectedly; rescan".into()),
        };
        self.discovery_rx = None;
        let connect = std::mem::take(&mut self.connect_after_discovery);
        match result {
            Ok(pair) => {
                self.video_path = pair.video;
                self.serial_path = pair.serial;
                self.discovery_status = "NanoKVM found".into();
                if connect { self.connect_paths(); }
            }
            Err(error) => {
                self.video_path.clear();
                self.serial_path.clear();
                self.discovery_status = error;
            }
        }
    }

    fn connect(&mut self) {
        if self.camera.is_some() || self.serial.is_some() || self.disconnecting.is_some() || self.discovery_rx.is_some() { return; }
        if self.automatic { self.discover(true); } else { self.connect_paths(); }
    }

    fn connect_paths(&mut self) {
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
        self.mapped_meta_held = false;
        self.keyboard.release();
        self.buttons = 0;
        self.last_mouse = (0.5, 0.5);
        self.pointer_pos = None;
        self.next_mouse_update = Instant::now();
    }

    fn disconnect(&mut self) {
        self.discovery_rx = None;
        self.connect_after_discovery = false;
        self.discovery_status = "Disconnected".into();
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
        self.release_held_input();
        self.clear_input();
    }

    fn release_held_input(&mut self) {
        let mouse = protocol::mouse_absolute(0, self.last_mouse.0, self.last_mouse.1, 0);
        self.mapped_meta_held = false;
        self.keyboard.release();
        self.buttons = 0;
        if self.disconnecting.is_none() {
            if let Some(port) = self.serial.as_mut() {
                if port.release(mouse).is_err() {
                    self.serial_status = "Could not queue HID release: serial worker closed".into();
                }
            }
        }
    }

    fn refresh_messages(&mut self) {
        self.refresh_discovery();
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
        // Either worker ending closes both devices and clears held input before manual retry.
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
                    if ui.add_enabled(!active && self.disconnecting.is_none() && self.discovery_rx.is_none(), egui::Button::new("Connect")).clicked() {
                        self.connect();
                    }
                    if ui.add_enabled((active || self.discovery_rx.is_some()) && self.disconnecting.is_none(), egui::Button::new("Disconnect")).clicked() {
                        self.disconnect();
                    }
                    ui.separator();
                    if self.remote {
                        if ui.button("■ Release input (Esc)").clicked() {
                            self.release_remote();
                        }
                    } else if ui.add_enabled(self.serial_ready && self.image.is_some(), egui::Button::new("▶ Capture input")).clicked() {
                        self.remote = true;
                        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
                    }
                    if ui.add_enabled(self.remote, egui::Button::new(
                        if self.remote_super { "Super: ON" } else { "Super: OFF" }
                    )).clicked() {
                        self.remote_super = !self.remote_super;
                        self.keyboard.modifiers = if self.remote_super || self.mapped_meta_held {
                            self.keyboard.modifiers | protocol::modifiers::LEFT_SUPER
                        } else {
                            self.keyboard.modifiers & !protocol::modifiers::LEFT_SUPER
                        };
                        self.send_keyboard();
                        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
                    }
                    if ui.checkbox(&mut self.map_f12_to_meta, "F12 → Meta")
                        .on_hover_text("Hold F12 for remote Meta while video has keyboard focus. Turn off to send F12 normally. The Super toggle can also hold Meta.")
                        .changed() {
                        self.release_held_input();
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
                    ui.add_enabled_ui(self.camera.is_none() && self.serial.is_none() && self.discovery_rx.is_none(), |ui| {
                        if ui.checkbox(&mut self.automatic, "Automatic devices").changed() {
                            if self.automatic { self.discover(false); }
                            else { self.discovery_status = "Set both device paths manually".into(); }
                        }
                        if self.automatic && ui.button("Rescan").clicked() { self.discover(false); }
                        ui.add_enabled_ui(!self.automatic, |ui| {
                            ui.label("Video:");
                            ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut self.video_path));
                            ui.label("Serial:");
                            ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut self.serial_path));
                        });
                    });
                    ui.small(&self.discovery_status);
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
            let response = ui.interact(rect, egui::Id::new("video"), egui::Sense::click_and_drag());
            let pressed = ctx.input(|i| i.events.iter().any(|event|
                matches!(event, egui::Event::PointerButton { pressed: true, pos, .. } if rect.contains(*pos))));
            if self.serial_ready && ctx.input(|i| i.focused) && pressed && (response.clicked() || response.is_pointer_button_down_on()) {
                self.remote = true;
                response.request_focus();
            }
            if self.remote && ctx.input(|i| i.focused) {
                self.handle_mouse(ctx, &response);
            }
        });
    }

    fn handle_mouse(&mut self, ctx: &egui::Context, video: &egui::Response) {
        let video_rect = video.rect;
        let now = Instant::now();
        let events = ctx.input(|i| i.events.clone());
        let over_video = |pos| video_rect.contains(pos) && ctx.layer_id_at(pos) == Some(video.layer_id);
        let local_drag = ctx.dragged_id().or(ctx.drag_stopped_id()).is_some_and(|id| id != video.id);
        let mut pointer_pos = self.pointer_pos;
        for event in events {
            if !self.remote { break; }
            if let egui::Event::PointerButton { pos, button, pressed, .. } = event {
                pointer_pos = Some(pos);
                let bit = match button {
                    egui::PointerButton::Primary => 1,
                    egui::PointerButton::Secondary => 2,
                    egui::PointerButton::Middle => 4,
                    egui::PointerButton::Extra1 => 8,
                    egui::PointerButton::Extra2 => 16,
                };
                // Only video-owned presses can produce remote releases.
                if pressed && (!over_video(pos) || local_drag) { continue; }
                if !pressed && self.buttons & bit == 0 { continue; }
                if pressed { self.buttons |= bit; }
                else { self.buttons &= !bit; }
                if over_video(pos) { self.last_mouse = input::normalized_point(video_rect, pos); }
                self.send_pointer(0);
            } else if let egui::Event::PointerMoved(point) = event {
                pointer_pos = Some(point);
                if over_video(point) && !local_drag {
                    self.move_pointer(input::normalized_point(video_rect, point), now);
                }
            } else if let egui::Event::MouseWheel { delta, .. } = event {
                if delta.y != 0.0 && pointer_pos.is_some_and(over_video) && !local_drag {
                    self.send_pointer(if delta.y > 0.0 { 1 } else { -1 });
                }
            } else if matches!(event, egui::Event::PointerGone) {
                pointer_pos = None;
            }
        }
        if self.remote {
            self.pointer_pos = pointer_pos;
            // Retry the final throttled motion even when the next frame has no mouse events.
            if let Some(point) = pointer_pos.filter(|point| over_video(*point) && !local_drag) {
                self.move_pointer(input::normalized_point(video_rect, point), now);
            }
        }
    }

    fn move_pointer(&mut self, point: (f32, f32), now: Instant) {
        if now < self.next_mouse_update { return; }
        if (self.last_mouse.0 - point.0).abs() > 0.0002 ||
           (self.last_mouse.1 - point.1).abs() > 0.0002 {
            self.last_mouse = point;
            self.send_pointer(0);
        }
        self.next_mouse_update = now + Duration::from_millis(12);
    }

    fn forward_key(&mut self, event: &egui::Event) {
        let egui::Event::Key { key, physical_key, pressed, modifiers, .. } = event else { return; };
        let key = physical_key.unwrap_or(*key);
        if key == egui::Key::Escape && *pressed {
            self.release_remote();
            return;
        }
        let before = self.keyboard.bytes();
        if self.map_f12_to_meta && key == egui::Key::F12 {
            self.mapped_meta_held = *pressed;
        } else if let Some(usage) = input::map_key(key) {
            self.keyboard.set_key(usage, *pressed);
        }
        input::sync_modifiers(&mut self.keyboard, *modifiers, self.remote_super || self.mapped_meta_held);
        if self.keyboard.bytes() != before { self.send_keyboard(); }
    }
}

impl eframe::App for Viewer {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let native = egui_winit::take_keyboard_input(ctx, input.viewport_id);
        if !self.remote { return; }
        let video_id = egui::Id::new("video");
        if !input.focused || input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))) {
            self.release_remote();
            ctx.memory_mut(|memory| memory.surrender_focus(video_id));
            return;
        }
        let local_click = input.events.iter().any(|event| {
            matches!(event, egui::Event::PointerButton { pos, pressed: true, .. }
                if !ctx.read_response(video_id).is_some_and(|video|
                    video.rect.contains(*pos) && ctx.layer_id_at(*pos) == Some(video.layer_id)))
        });
        if local_click { ctx.memory_mut(|memory| memory.surrender_focus(video_id)); }
        if !ctx.memory(|memory| memory.has_focus(video_id)) {
            if self.keyboard.bytes() != [0; 8] || self.buttons != 0 {
                self.release_held_input();
            }
            return;
        }
        if native.is_empty() {
            for event in &input.events {
                if !self.remote { break; }
                self.forward_key(event);
            }
        } else {
            for event in native {
                if !self.remote { break; }
                match event {
                    egui_winit::KeyboardInput::Key(event) => self.forward_key(&event),
                    egui_winit::KeyboardInput::Modifiers(modifiers) => {
                        if input::sync_modifiers(&mut self.keyboard, modifiers, self.remote_super || self.mapped_meta_held) {
                            self.send_keyboard();
                        }
                    }
                }
            }
        }
        // The batch snapshot applies after its ordered events, including modifier-only frames.
        if self.remote && input::sync_modifiers(&mut self.keyboard, input.modifiers, self.remote_super || self.mapped_meta_held) {
            self.send_keyboard();
        }
        // Remote keyboard events must not also activate or edit local widgets.
        input.events.retain(|event| !matches!(event,
            egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy |
            egui::Event::Cut | egui::Event::Paste(_) | egui::Event::Ime(_)));
        input.modifiers = egui::Modifiers::default();
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
    use eframe::App;
    use crate::worker::Worker;
    use crossbeam_channel::Sender;
    use std::{io::{self, Write}, sync::{Mutex, atomic::Ordering}, thread};

    fn viewer() -> Viewer {
        Viewer::disconnected(Options {
            video: "/nonexistent-nanokvm-t05/video".into(),
            serial: "/nonexistent-nanokvm-t05/serial".into(),
        })
    }

    fn input_frame(app: &mut Viewer, ctx: &egui::Context, events: Vec<egui::Event>, local_text: &mut String) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        app.raw_input_hook(ctx, &mut raw);
        let _ = ctx.run(raw, |ctx| {
            egui::TopBottomPanel::top("test_local_controls").show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(local_text).id(egui::Id::new("local_text")));
            });
            app.render_video(ctx);
        });
    }

    fn input_viewer() -> (Viewer, egui::Context, Receiver<Report>) {
        let mut app = viewer();
        let (port, reports) = serial::tests::queued_port();
        app.serial = Some(port);
        app.serial_ready = true;
        app.remote = true;
        app.image = Some(Arc::new(RgbaFrame { width: 4, height: 3, rgba: vec![0; 48] }));
        let ctx = egui::Context::default();
        input_frame(&mut app, &ctx, vec![], &mut String::new());
        (app, ctx, reports)
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() }
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::default() }
    }

    fn native_keyboard_frame(app: &mut Viewer, ctx: &egui::Context, native: Vec<egui_winit::KeyboardInput>, mut raw: egui::RawInput) {
        ctx.data_mut(|data| data.insert_temp(
            egui::Id::new("egui-winit-keyboard-input").with(raw.viewport_id), native,
        ));
        app.raw_input_hook(ctx, &mut raw);
    }

    fn keyboard_reports(reports: &Receiver<Report>) -> Vec<[u8; 8]> {
        reports.try_iter().map(|report| {
            let Report::Keyboard(bytes) = report else { panic!("expected keyboard report"); };
            bytes
        }).collect()
    }

    #[test]
    fn f12_mapping_preserves_native_combo_order_and_normal_f12() {
        use egui_winit::KeyboardInput::{Key, Modifiers};
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        assert!(!app.map_f12_to_meta);
        input_frame(&mut app, &ctx, vec![key(egui::Key::F12, true), key(egui::Key::F12, false)], &mut String::new());
        assert_eq!(keyboard_reports(&reports), vec![[0, 0, 0x45, 0, 0, 0, 0, 0], [0; 8]]);
        app.map_f12_to_meta = true;
        native_keyboard_frame(&mut app, &ctx, vec![
            Key(key(egui::Key::F12, true)), Key(key(egui::Key::F12, true)),
            Modifiers(egui::Modifiers::CTRL | egui::Modifiers::ALT),
            Key(egui::Event::Key { key: egui::Key::A, physical_key: Some(egui::Key::A),
                pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL | egui::Modifiers::ALT }),
            Modifiers(egui::Modifiers::default()),
            Key(key(egui::Key::F12, false)), Key(key(egui::Key::A, false)),
        ], egui::RawInput::default());
        assert_eq!(keyboard_reports(&reports), vec![
            [8, 0, 0, 0, 0, 0, 0, 0], [13, 0, 0, 0, 0, 0, 0, 0],
            [13, 0, 4, 0, 0, 0, 0, 0], [8, 0, 4, 0, 0, 0, 0, 0],
            [0, 0, 4, 0, 0, 0, 0, 0], [0; 8],
        ]);
        assert!(!app.mapped_meta_held);
    }

    #[test]
    fn mapped_meta_survives_frames_and_combines_with_super_toggle() {
        let (mut app, ctx, reports) = input_viewer();
        app.map_f12_to_meta = true;
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        input_frame(&mut app, &ctx, vec![key(egui::Key::F12, true)], &mut String::new());
        assert_eq!(keyboard_reports(&reports), vec![[8, 0, 0, 0, 0, 0, 0, 0]]);
        input_frame(&mut app, &ctx, vec![], &mut String::new());
        assert!(reports.is_empty() && app.mapped_meta_held);
        app.remote_super = true;
        input_frame(&mut app, &ctx, vec![key(egui::Key::F12, false)], &mut String::new());
        assert!(!app.mapped_meta_held && reports.is_empty());
        assert_eq!(app.keyboard.modifiers, protocol::modifiers::LEFT_SUPER);
        app.remote_super = false;
        input_frame(&mut app, &ctx, vec![], &mut String::new());
        assert_eq!(keyboard_reports(&reports), vec![[0; 8]]);
    }

    #[test]
    fn mapped_meta_releases_on_local_focus_escape_and_window_focus_loss() {
        for events in [
            vec![pointer_button(egui::pos2(40.0, 10.0), true)],
            vec![key(egui::Key::Escape, true)],
            vec![egui::Event::WindowFocused(false)],
        ] {
            let (mut app, ctx, reports) = input_viewer();
            app.map_f12_to_meta = true;
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            input_frame(&mut app, &ctx, vec![key(egui::Key::F12, true)], &mut String::new());
            reports.try_iter().for_each(drop);
            input_frame(&mut app, &ctx, events, &mut String::new());
            assert!(!app.mapped_meta_held);
            assert_eq!(app.keyboard.bytes(), [0; 8]);
            assert!(matches!(reports.try_recv().unwrap(), Report::Release(_)));
            assert!(reports.is_empty());
            assert!(app.map_f12_to_meta);
            app.remote = true;
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            input_frame(&mut app, &ctx, vec![key(egui::Key::F12, false)], &mut String::new());
            assert!(reports.is_empty());
        }
        let (mut app, ctx, reports) = input_viewer();
        app.map_f12_to_meta = true;
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("local_text")));
        input_frame(&mut app, &ctx, vec![key(egui::Key::F12, true)], &mut String::new());
        assert!(!app.mapped_meta_held && reports.is_empty());
    }

    #[test]
    fn mapping_checkbox_releases_held_input_before_switching_behavior() {
        let (mut app, ctx, reports) = input_viewer();
        for enabled in [true, false] {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 600.0))),
                ..Default::default()
            };
            let output = ctx.run(raw.clone(), |ctx| app.toolbar(ctx));
            let pos = output.shapes.iter().find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    if text.galley.text() == "F12 → Meta" { return Some(text.pos + egui::vec2(3.0, 3.0)); }
                }
                None
            }).expect("mapping checkbox label");
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            app.forward_key(&key(egui::Key::F12, true));
            reports.try_iter().for_each(drop);
            let mut raw = egui::RawInput { events: vec![
                egui::Event::PointerMoved(pos), pointer_button(pos, true), pointer_button(pos, false),
            ], ..raw };
            app.raw_input_hook(&ctx, &mut raw);
            let _ = ctx.run(raw, |ctx| app.toolbar(ctx));
            assert_eq!(app.map_f12_to_meta, enabled);
            assert!(!app.mapped_meta_held);
            assert_eq!(app.keyboard.bytes(), [0; 8]);
            assert!(matches!(reports.try_recv().unwrap(), Report::Release(_)));
            assert!(reports.is_empty());
        }
        app.forward_key(&key(egui::Key::F12, true));
        assert_eq!(keyboard_reports(&reports), vec![[0, 0, 0x45, 0, 0, 0, 0, 0]]);
    }

    #[test]
    fn physical_keys_survive_shift_symbols_layout_changes_and_repeats() {
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        for (logical, physical, usage) in [
            (egui::Key::Plus, egui::Key::Equals, 0x2e),
            (egui::Key::Questionmark, egui::Key::Slash, 0x38),
            (egui::Key::Colon, egui::Key::Semicolon, 0x33),
            (egui::Key::Exclamationmark, egui::Key::Num1, 0x1e),
            (egui::Key::OpenCurlyBracket, egui::Key::OpenBracket, 0x2f),
            (egui::Key::CloseCurlyBracket, egui::Key::CloseBracket, 0x30),
            (egui::Key::Pipe, egui::Key::Backslash, 0x31),
            (egui::Key::Z, egui::Key::Y, 0x1c),
        ] {
            let down = egui::Event::Key { key: logical, physical_key: Some(physical),
                pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT };
            let repeat = egui::Event::Key { key: logical, physical_key: Some(physical),
                pressed: true, repeat: true, modifiers: egui::Modifiers::SHIFT };
            let up = key(physical, false);
            let mut raw = egui::RawInput { events: vec![down, repeat, up], ..Default::default() };
            app.raw_input_hook(&ctx, &mut raw);
            let sent = keyboard_reports(&reports);
            assert_eq!(sent, vec![[2, 0, usage, 0, 0, 0, 0, 0], [0; 8]]);
            assert!(raw.events.is_empty());
        }
        let mut raw = egui::RawInput { events: vec![egui::Event::Key {
            key: egui::Key::A, physical_key: None, pressed: true, repeat: false,
            modifiers: egui::Modifiers::default(),
        }, key(egui::Key::A, false)], ..Default::default() };
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(keyboard_reports(&reports), vec![[0, 0, 4, 0, 0, 0, 0, 0], [0; 8]]);
    }

    #[test]
    fn key_modifiers_precede_the_final_batch_snapshot() {
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        let mut raw = egui::RawInput { events: vec![
            egui::Event::Key { key: egui::Key::A, physical_key: Some(egui::Key::A),
                pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL },
            egui::Event::Key { key: egui::Key::A, physical_key: Some(egui::Key::A),
                pressed: false, repeat: false, modifiers: egui::Modifiers::CTRL },
            egui::Event::Key { key: egui::Key::B, physical_key: Some(egui::Key::B),
                pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT },
        ], ..Default::default() };
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(keyboard_reports(&reports), vec![
            [1, 0, 4, 0, 0, 0, 0, 0], [1, 0, 0, 0, 0, 0, 0, 0],
            [2, 0, 5, 0, 0, 0, 0, 0], [0, 0, 5, 0, 0, 0, 0, 0],
        ]);
    }

    #[test]
    fn native_clipboard_shortcuts_and_modifier_only_changes_keep_order() {
        use egui_winit::KeyboardInput::{Key, Modifiers};
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        for (keycode, usage, translated) in [
            (egui::Key::C, 6, vec![egui::Event::Copy]),
            (egui::Key::X, 27, vec![egui::Event::Cut]),
            (egui::Key::V, 25, vec![]),
            (egui::Key::V, 25, vec![egui::Event::Paste("local clipboard".into())]),
        ] {
            native_keyboard_frame(&mut app, &ctx, vec![
                Modifiers(egui::Modifiers::CTRL),
                Key(egui::Event::Key { key: keycode, physical_key: Some(keycode),
                    pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL }),
                Modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT),
                Modifiers(egui::Modifiers::CTRL),
                Key(egui::Event::Key { key: keycode, physical_key: Some(keycode),
                    pressed: false, repeat: false, modifiers: egui::Modifiers::CTRL }),
                Modifiers(egui::Modifiers::default()),
            ], egui::RawInput { events: translated, ..Default::default() });
            assert_eq!(keyboard_reports(&reports), vec![
                [1, 0, 0, 0, 0, 0, 0, 0], [1, 0, usage, 0, 0, 0, 0, 0],
                [3, 0, usage, 0, 0, 0, 0, 0], [1, 0, usage, 0, 0, 0, 0, 0],
                [1, 0, 0, 0, 0, 0, 0, 0], [0; 8],
            ]);
        }
    }

    #[test]
    fn native_input_is_drained_when_local_or_released() {
        for remote in [false, true] {
            let (mut app, ctx, reports) = input_viewer();
            app.remote = remote;
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("local_text")));
            native_keyboard_frame(&mut app, &ctx, vec![egui_winit::KeyboardInput::Key(key(egui::Key::A, true))],
                egui::RawInput::default());
            assert!(egui_winit::take_keyboard_input(&ctx, egui::ViewportId::ROOT).is_empty());
            assert!(reports.is_empty());
            app.remote = true;
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            app.raw_input_hook(&ctx, &mut egui::RawInput::default());
            assert!(reports.is_empty());
        }
    }

    #[test]
    fn keyboard_focus_separates_local_text_and_remote_reports() {
        let (mut app, ctx, reports) = input_viewer();
        let mut local = String::new();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("local_text")));
        input_frame(&mut app, &ctx, vec![key(egui::Key::A, true), egui::Event::Text("a".into())], &mut local);
        assert_eq!(local, "a");
        assert!(reports.is_empty());
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        input_frame(&mut app, &ctx, vec![
            key(egui::Key::B, true), egui::Event::Text("b".into()),
            egui::Event::Paste("pasted".into()), egui::Event::Copy, egui::Event::Cut,
            egui::Event::Ime(egui::ImeEvent::Commit("ime".into())),
        ], &mut local);
        assert_eq!(local, "a");
        assert!(matches!(reports.try_recv().unwrap(), Report::Keyboard(bytes) if bytes[2] == 5));
        assert!(reports.is_empty());
        input_frame(&mut app, &ctx, vec![key(egui::Key::Escape, true), egui::Event::Text("leak".into())], &mut local);
        assert!(!app.remote && local == "a");
        assert!(matches!(reports.try_recv().unwrap(), Report::Release(_)));
    }

    #[test]
    fn local_clicks_do_not_move_remote_pointer_and_release_held_input() {
        let (mut app, ctx, reports) = input_viewer();
        let outside = egui::pos2(40.0, 10.0);
        app.last_mouse = (0.25, 0.75);
        input_frame(&mut app, &ctx, vec![pointer_button(outside, true), pointer_button(outside, false)], &mut String::new());
        assert!(reports.is_empty());
        assert_eq!(app.last_mouse, (0.25, 0.75));
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        app.keyboard.set_key(4, true);
        app.keyboard.modifiers = protocol::modifiers::LEFT_SUPER;
        app.remote_super = true;
        app.buttons = 1;
        input_frame(&mut app, &ctx, vec![pointer_button(outside, true), pointer_button(outside, false)], &mut String::new());
        assert_eq!(app.keyboard.bytes(), [0; 8]);
        assert_eq!(app.buttons, 0);
        assert!(matches!(reports.try_recv().unwrap(), Report::Release(bytes)
            if bytes == protocol::mouse_absolute(0, 0.25, 0.75, 0)));
        assert!(reports.is_empty());
        assert!(app.remote_super);
        input_frame(&mut app, &ctx, vec![], &mut String::new());
        assert!(reports.is_empty());
    }

    #[test]
    fn video_press_acquires_input_and_drag_release_keeps_last_remote_position() {
        let (mut app, ctx, reports) = input_viewer();
        app.remote = false;
        let rect = ctx.read_response(egui::Id::new("video")).unwrap().rect;
        let inside = rect.center();
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(inside), pointer_button(inside, true)], &mut String::new());
        assert!(app.remote && app.buttons == 1);
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("video"))));
        let last = app.last_mouse;
        reports.try_iter().for_each(drop);
        let outside = egui::pos2(40.0, 10.0);
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(outside), pointer_button(outside, false)], &mut String::new());
        assert_eq!(app.buttons, 0);
        assert_eq!(app.last_mouse, last);
        assert!(matches!(reports.try_recv().unwrap(), Report::AbsoluteMouse(bytes)
            if bytes == protocol::mouse_absolute(0, last.0, last.1, 0)));
        assert!(reports.is_empty());
    }

    #[test]
    fn local_drag_into_video_does_not_become_remote_input() {
        let (mut app, ctx, reports) = input_viewer();
        let inside = ctx.read_response(egui::Id::new("video")).unwrap().rect.center();
        let mut text = "local selection".to_owned();
        let outside = egui::pos2(40.0, 10.0);
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(outside), pointer_button(outside, true)], &mut text);
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(inside)], &mut text);
        input_frame(&mut app, &ctx, vec![pointer_button(inside, false)], &mut text);
        assert!(reports.is_empty());
        assert_eq!(app.buttons, 0);
    }

    #[test]
    fn release_during_a_drag_does_not_recapture_on_the_next_frame() {
        let (mut app, ctx, reports) = input_viewer();
        let inside = ctx.read_response(egui::Id::new("video")).unwrap().rect.center();
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(inside), pointer_button(inside, true)], &mut String::new());
        input_frame(&mut app, &ctx, vec![key(egui::Key::Escape, true)], &mut String::new());
        assert!(!app.remote);
        reports.try_iter().for_each(drop);
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(inside), pointer_button(inside, false)], &mut String::new());
        assert!(!app.remote && app.buttons == 0);
        assert!(reports.is_empty());
    }

    #[test]
    fn scroll_uses_pointer_location_at_the_event() {
        let (mut app, ctx, reports) = input_viewer();
        let inside = ctx.read_response(egui::Id::new("video")).unwrap().rect.center();
        let outside = egui::pos2(40.0, 10.0);
        let wheel = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 20.0), modifiers: egui::Modifiers::default() };
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(outside), wheel.clone(), egui::Event::PointerMoved(inside)], &mut String::new());
        for report in reports.try_iter() {
            assert!(matches!(report, Report::AbsoluteMouse(bytes) if bytes == protocol::mouse_absolute(0, 0.5, 0.5, 0)));
        }
        input_frame(&mut app, &ctx, vec![wheel, egui::Event::PointerMoved(outside)], &mut String::new());
        assert!(matches!(reports.try_recv().unwrap(), Report::AbsoluteMouse(bytes)
            if bytes == protocol::mouse_absolute(0, 0.5, 0.5, 1)));
        assert!(reports.is_empty());
    }

    #[test]
    fn focus_loss_releases_input_before_forwarding_the_batch() {
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        app.keyboard.set_key(4, true);
        app.remote_super = true;
        app.buttons = 7;
        input_frame(&mut app, &ctx, vec![key(egui::Key::B, true), egui::Event::WindowFocused(false)], &mut String::new());
        assert!(!app.remote && !app.remote_super);
        assert_eq!(app.keyboard.bytes(), [0; 8]);
        assert_eq!(app.buttons, 0);
        assert!(matches!(reports.try_recv().unwrap(), Report::Release(bytes)
            if bytes == protocol::mouse_absolute(0, 0.5, 0.5, 0)));
        assert!(reports.is_empty());
    }

    #[test]
    fn final_throttled_motion_is_sent_without_another_pointer_event() {
        let (mut app, ctx, reports) = input_viewer();
        let rect = ctx.read_response(egui::Id::new("video")).unwrap().rect;
        let point = rect.center();
        app.last_mouse = (0.0, 0.0);
        app.next_mouse_update = Instant::now() + Duration::from_secs(1);
        input_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(point)], &mut String::new());
        assert!(reports.is_empty());
        app.next_mouse_update = Instant::now();
        input_frame(&mut app, &ctx, vec![], &mut String::new());
        let Report::AbsoluteMouse(bytes) = reports.try_recv().unwrap() else { panic!("expected mouse motion"); };
        assert_eq!(bytes, protocol::mouse_absolute(0, 0.5, 0.5, 0));
        assert!(reports.is_empty());
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

    fn hold_input(app: &mut Viewer) {
        app.remote = true;
        app.remote_super = true;
        app.map_f12_to_meta = true;
        app.mapped_meta_held = true;
        app.keyboard.set_key(4, true);
        app.keyboard.modifiers = 8;
        app.buttons = 7;
        app.image = Some(Arc::new(RgbaFrame { width: 1, height: 1, rgba: vec![0; 4] }));
        app.new_frame = app.image.clone();
    }

    fn assert_input_cleared(app: &Viewer) {
        assert!(!app.remote && !app.remote_super && !app.mapped_meta_held && !app.serial_ready);
        assert_eq!(app.keyboard.bytes(), [0; 8]);
        assert_eq!(app.buttons, 0);
        assert!(app.image.is_none() && app.new_frame.is_none());
    }

    #[test]
    fn video_loss_with_held_input_releases_serial_and_requires_manual_retry() {
        for cause in ["Video stopped: device removed", "Video stopped: read MJPG video frame (500 ms timeout)"] {
            let mut app = viewer();
            let (fail, failure) = bounded(1);
            let (frame_tx, frames) = bounded(2);
            let (status_tx, status) = bounded(8);
            let (resource, closed) = bounded::<()>(1);
            let worker = Worker::spawn(move |_| {
                let _owned = (resource, frame_tx, status_tx);
                failure.recv().unwrap();
                Err(cause.into())
            });
            let (serial, serial_closed, bytes) = serial();
            app.camera = Some(CameraHandle { frames, status, worker });
            app.serial = Some(serial);
            hold_input(&mut app);
            app.send_keyboard();
            app.send_pointer(0);
            let mut held = protocol::encode(protocol::SEND_KEYBOARD, &app.keyboard.bytes()).unwrap();
            held.extend(protocol::encode(protocol::SEND_MOUSE_ABSOLUTE, &protocol::mouse_absolute(7, 0.5, 0.5, 0)).unwrap());
            let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
            while bytes.lock().unwrap().len() < held.len() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            fail.send(()).unwrap();
            wait_disconnected(&mut app);
            assert_input_cleared(&app);
            assert_eq!(app.video_status, cause);
            assert!(closed.recv().is_err() && serial_closed.recv().is_err());
            let written = bytes.lock().unwrap();
            assert_eq!(&written[..held.len()], held.as_slice());
            assert_neutral(&written[held.len()..]);
            app.refresh_messages();
            assert!(app.camera.is_none() && app.serial.is_none() && app.discovery_rx.is_none());
            app.connect();
            assert!(app.camera.is_some() && app.serial.is_some());
            wait_disconnected(&mut app);
        }
    }

    #[test]
    fn idle_serial_hangup_cleans_held_input_and_manual_recovery_reopens_serial() {
        use serialport::SerialPort;
        let mut app = viewer();
        for _ in 0..2 {
            let (master, slave) = serialport::TTYPort::pair().unwrap();
            app.serial_path = slave.name().unwrap();
            drop(slave);
            app.connect();
            let (camera, video_closed) = camera(false);
            // Replace unavailable V4L2 hardware; serial uses the production open and I/O path.
            let mut unopened_camera = app.camera.replace(camera).unwrap();
            unopened_camera.worker.stop();
            let _ = unopened_camera.worker.wait_until(Instant::now() + SHUTDOWN_TIMEOUT);
            let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
            while !app.serial_ready {
                app.refresh_messages();
                assert!(Instant::now() < deadline, "serial did not open: {}", app.serial_status);
                thread::sleep(Duration::from_millis(1));
            }
            assert!(!app.remote);
            hold_input(&mut app);
            drop(master);
            wait_disconnected(&mut app);
            assert_input_cleared(&app);
            assert!(video_closed.recv().is_err());
            assert!(app.serial_status.contains("Serial connection lost"), "{}", app.serial_status);
            assert!(app.serial_status.contains("HID release failed"));
            app.refresh_messages();
            assert!(app.camera.is_none() && app.serial.is_none() && app.discovery_rx.is_none());
        }
    }

    #[test]
    fn stalled_peer_keeps_cleanup_nonblocking_and_reconnect_disabled() {
        let mut app = viewer();
        let (resume, stalled) = bounded::<()>(1);
        let (frame_tx, frames) = bounded(2);
        let (status_tx, status) = bounded(8);
        let worker = Worker::spawn(move |_| {
            let _owned = (frame_tx, status_tx);
            let _ = stalled.recv();
            Ok(())
        });
        app.camera = Some(CameraHandle { frames, status, worker });
        app.serial = Some(serial::spawn(app.serial_path.clone()));
        hold_input(&mut app);
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while app.disconnecting.is_none() {
            app.refresh_messages();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert_input_cleared(&app);
        app.disconnecting = Some(Instant::now() - SHUTDOWN_TIMEOUT);
        app.refresh_messages();
        assert!(app.video_status.contains("Video still stopping"));
        assert!(app.serial_status.contains("Serial open failed"));
        app.connect();
        assert!(app.serial.is_none() && app.camera.is_some() && app.disconnecting.is_some());
        resume.send(()).unwrap();
        wait_disconnected(&mut app);
        assert!(app.serial_status.contains("Serial open failed"));
    }

    fn discovery_result(app: &mut Viewer, connect: bool, result: Result<DevicePair, String>) {
        let (tx, rx) = bounded(1);
        app.discovery_rx = Some(rx);
        app.connect_after_discovery = connect;
        tx.send(result).unwrap();
    }

    fn detected_pair() -> DevicePair {
        DevicePair {
            video: "/nonexistent-nanokvm-t06/video6".into(),
            serial: "/nonexistent-nanokvm-t06/ttyUSB4".into(),
        }
    }

    #[test]
    fn discovery_alone_updates_paths_without_starting_workers() {
        let mut app = Viewer::disconnected(Options { video: String::new(), serial: String::new() });
        assert!(app.automatic);
        discovery_result(&mut app, false, Ok(detected_pair()));
        app.refresh_discovery();
        assert!(app.camera.is_none() && app.serial.is_none());
        assert!(app.video_path.ends_with("video6") && app.serial_path.ends_with("ttyUSB4"));
        assert!(app.discovery_rx.is_none() && !app.connect_after_discovery);
    }

    #[test]
    fn explicit_connect_uses_fresh_discovery_paths() {
        let mut app = viewer();
        app.automatic = true;
        discovery_result(&mut app, true, Ok(detected_pair()));
        app.refresh_discovery();
        assert!(app.camera.is_some() && app.serial.is_some());
        assert!(app.video_path.ends_with("video6") && app.serial_path.ends_with("ttyUSB4"));
        wait_disconnected(&mut app);
    }

    #[test]
    fn failed_discovery_clears_old_paths_and_keeps_devices_closed() {
        let mut app = viewer();
        assert!(!app.automatic);
        discovery_result(&mut app, true, Err("Multiple NanoKVM candidates".into()));
        app.refresh_discovery();
        assert!(app.video_path.is_empty() && app.serial_path.is_empty());
        assert!(app.camera.is_none() && app.serial.is_none());
        assert!(app.discovery_status.contains("Multiple"));
    }

    #[test]
    fn disconnect_cancels_a_pending_discovery_connection() {
        let mut app = viewer();
        discovery_result(&mut app, true, Ok(detected_pair()));
        app.disconnect();
        app.refresh_discovery();
        assert!(app.discovery_rx.is_none() && !app.connect_after_discovery);
        assert!(app.camera.is_none() && app.serial.is_none());
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
        app.map_f12_to_meta = true;
        app.mapped_meta_held = true;
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
        assert!(!app.remote && !app.remote_super && !app.mapped_meta_held && !app.serial_ready);
        assert!(app.map_f12_to_meta);
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
