use std::{fs, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use eframe::egui;
use egui_wgpu::{self, wgpu};

use crate::{capture::{self, CameraHandle, RgbaFrame}, gpu::{ColorSettings, GpuVideo, VideoCallback}, input, protocol::{self, KeyboardReport}, serial::{self, SerialHandle, Report}};

pub struct Options {
    pub video: String,
    pub serial: String,
}

pub struct Viewer {
    camera: CameraHandle,
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
        let camera = capture::spawn(opts.video);
        let mut app = Self {
            camera,
            serial: None,
            serial_path: opts.serial,
            serial_ready: false,
            video_status: "Connecting to V4L2…".to_owned(),
            serial_status: "Not connected".to_owned(),
            image: None,
            new_frame: None,
            colors,
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
        };
        if !app.serial_path.is_empty() {
            app.connect_serial();
        }
        app
    }

    fn connect_serial(&mut self) {
        self.release_remote();
        if let Some(old) = self.serial.take() {
            let _ = old.tx.try_send(Report::Shutdown);
        }
        if self.serial_path.trim().is_empty() { return; }
        self.serial = Some(serial::spawn(self.serial_path.trim().to_owned()));
        self.serial_ready = false;
        self.serial_status = "Connecting…".to_owned();
    }

    fn send(&mut self, report: Report) {
        if let Some(port) = self.serial.as_ref() {
            if port.tx.try_send(report).is_err() {
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
        self.remote = false;
        self.remote_super = false;
        self.keyboard.release();
        self.buttons = 0;
        // Always transmit releases even if the app/window lost focus.
        if let Some(port) = self.serial.as_ref() {
            let _ = port.tx.try_send(Report::Keyboard([0; 8]));
            let _ = port.tx.try_send(Report::AbsoluteMouse(protocol::mouse_absolute(0, self.last_mouse.0, self.last_mouse.1, 0)));
        }
    }

    fn refresh_messages(&mut self) {
        while let Ok(msg) = self.camera.status.try_recv() {
            self.video_status = msg;
        }
        if let Some(serial) = self.serial.as_ref() {
            while let Ok(msg) = serial.status_rx.try_recv() {
                self.serial_ready = msg.starts_with("Serial connected:");
                self.serial_status = msg;
            }
        }
        // Drop stale frames: use most recent frame only. This limits latency.
        while let Ok(img) = self.camera.frames.try_recv() {
            self.image = Some(img.clone());
            self.new_frame = Some(img);
            self.frames_seen += 1;
            self.fps_frames += 1;
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
                    ui.separator();
                    if self.remote {
                        if ui.button("■ Release input (Esc)").clicked() {
                            self.release_remote();
                        }
                    } else if ui.add_enabled(self.serial_ready, egui::Button::new("▶ Capture input")).clicked() {
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
                    ui.label("Serial:");
                    ui.add_sized([140.0, 18.0], egui::TextEdit::singleline(&mut self.serial_path));
                    if ui.button("Connect").clicked() { self.connect_serial(); }
                });
            });
    }

    fn render_video(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
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
        self.release_remote();
        if let Some(handle) = self.serial.take() {
            let _ = handle.tx.try_send(Report::Shutdown);
        }
    }
}
