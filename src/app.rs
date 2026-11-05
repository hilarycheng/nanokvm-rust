use std::{path::{Path, PathBuf}, sync::Arc, time::{Duration, Instant}};
use eframe::egui;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use crate::shortcuts::{Shortcuts, Status as ShortcutStatus};
use crate::worker::SHUTDOWN_TIMEOUT;
use crate::discovery::{self, DevicePair};
use crate::screenshot::{self, Format, Screenshot};
use crossbeam_channel::{bounded, Receiver};

use crate::{capture::{self, CameraHandle, RgbaFrame}, gpu::{ColorSettings, GpuVideo, VideoCallback}, input, protocol::{self, KeyboardReport}, serial::{self, SerialHandle, Report}};

pub struct Options {
    pub video: String,
    pub serial: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputMode { NonFocus, Pending, Focus }

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
    color_status: String,
    colors_open: bool,
    screenshot: Option<Screenshot>,
    screenshot_directory: Option<PathBuf>,
    screenshot_status: String,
    screenshot_buttons: [Option<egui::Id>; 2],
    screenshot_keeps_focus: bool,
    remote: bool,
    mode: InputMode,
    shortcuts: Option<Shortcuts>,
    mode_status: String,
    right_ctrl: bool,
    right_alt: bool,
    release_chord_held: bool,
    input_blocked: bool,
    remote_super: bool,
    map_f12_to_meta: bool,
    mapped_meta_held: bool,
    fullscreen: bool,
    toolbar_pinned: bool,
    toolbar_visible: bool,
    toolbar_rect: Option<egui::Rect>,
    toolbar_last_hover: f64,
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
    config_path_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from), std::env::var_os("HOME").map(PathBuf::from))
}

fn config_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let root = xdg.filter(|path| path.is_absolute())
        .or_else(|| home.filter(|path| path.is_absolute()).map(|path| path.join(".config")))?;
    Some(root.join("nanokvm-rust").join("colors.json"))
}

impl Viewer {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let renderer = GpuVideo::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(renderer);
        }
        let mut app = Self::disconnected(opts);
        let shortcuts = (|| {
            // The Linux root window outlives Viewer; on_exit drops this before eframe destroys it.
            unsafe { Shortcuts::new(cc.display_handle()?.as_raw(), cc.window_handle()?.as_raw()) }
        })();
        match shortcuts {
            Ok(shortcuts) => app.shortcuts = Some(shortcuts),
            Err(error) => app.mode_status = error.to_string(),
        }
        app.restore_colors(config_path().as_deref());
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
            color_status: "Using GPU defaults".into(),
            colors_open: false,
            screenshot: None,
            screenshot_directory: screenshot::directory(std::env::var_os("HOME").map(PathBuf::from)),
            screenshot_status: String::new(),
            screenshot_buttons: [None; 2],
            screenshot_keeps_focus: false,
            remote: false,
            mode: InputMode::NonFocus,
            shortcuts: None,
            mode_status: "Focus Mode unavailable without Wayland shortcut inhibition".into(),
            right_ctrl: false,
            right_alt: false,
            release_chord_held: false,
            input_blocked: false,
            remote_super: false,
            map_f12_to_meta: false,
            mapped_meta_held: false,
            fullscreen: false,
            toolbar_pinned: true,
            toolbar_visible: true,
            toolbar_rect: None,
            toolbar_last_hover: 0.0,
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
        if let Some(shortcuts) = &mut self.shortcuts { shortcuts.release(); }
        if self.mode != InputMode::NonFocus {
            self.mode_status = "Non-Focus Mode: desktop shortcuts remain available".into();
        }
        self.mode = InputMode::NonFocus;
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
        self.refresh_screenshot();
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

    fn take_screenshot(&mut self, format: Format) {
        if self.screenshot.is_some() { return; }
        let Some(frame) = self.image.clone() else {
            self.screenshot_status = "Screenshot unavailable: no video frame".into();
            return;
        };
        let Some(directory) = &self.screenshot_directory else {
            self.screenshot_status = "Cannot save screenshot: no absolute HOME directory".into();
            return;
        };
        match screenshot::spawn(frame, format, directory) {
            Ok(job) => {
                self.screenshot = Some(job);
                self.screenshot_status = format!("Saving {} screenshot…", format.extension().to_uppercase());
            }
            Err(error) => self.screenshot_status = format!("Screenshot failed: {error:#}"),
        }
    }

    fn refresh_screenshot(&mut self) {
        let Some(job) = self.screenshot.as_mut() else { return; };
        let Some(result) = job.worker.finish() else { return; };
        self.screenshot_status = match result {
            Ok(()) => format!("Screenshot saved: {}", job.path.display()),
            Err(error) => format!("Screenshot failed: {error}"),
        };
        self.screenshot = None;
    }

    fn over_screenshot_button(&self, ctx: &egui::Context, pos: egui::Pos2) -> bool {
        self.screenshot_buttons.iter().flatten().any(|id| {
            ctx.read_response(*id).is_some_and(|button|
                button.rect.contains(pos) && ctx.layer_id_at(pos) == Some(button.layer_id))
        })
    }

    fn restore_colors(&mut self, path: Option<&Path>) {
        self.colors = ColorSettings::default();
        self.color_status = match path {
            None => "Using GPU defaults: no absolute HOME or XDG_CONFIG_HOME".into(),
            Some(path) => match ColorSettings::load(path) {
                Ok(Some(colors)) => {
                    self.colors = colors;
                    format!("GPU preset loaded: {}", path.display())
                }
                Ok(None) => "No saved GPU preset; using defaults".into(),
                Err(error) => format!("Using GPU defaults: {error:#}"),
            },
        };
    }

    fn save_colors(&mut self, path: Option<&Path>) {
        let Some(path) = path else {
            self.color_status = "Cannot save: no absolute HOME or XDG_CONFIG_HOME".into();
            return;
        };
        self.color_status = match self.colors.save(path) {
            Ok(()) => format!("GPU preset saved: {}", path.display()),
            Err(error) => format!("Could not save GPU preset: {error:#}"),
        };
    }

    fn color_panel(&mut self, ctx: &egui::Context) {
        if !self.colors_open { return; }
        egui::SidePanel::right("color_tuning")
            .resizable(false)
            .default_width(270.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("GPU Color Tuning");
                    ui.label("Only the remote video is affected.");
                    ui.add_space(8.0);
                    ui.add(egui::Slider::new(&mut self.colors.white, ColorSettings::WHITE)
                        .text("White peak").fixed_decimals(2))
                        .on_hover_text("Lower values dim the video, including peak white. 1.00 leaves white unchanged.");
                    ui.add(egui::Slider::new(&mut self.colors.highlights, ColorSettings::HIGHLIGHTS)
                        .text("Highlights").fixed_decimals(2))
                        .on_hover_text("Higher values compress bright tones. 0.00 disables compression; clipped capture detail cannot be recovered.");
                    ui.add(egui::Slider::new(&mut self.colors.gamma, ColorSettings::GAMMA)
                        .text("Gamma").fixed_decimals(2))
                        .on_hover_text("Below 1.00 brightens midtones; above 1.00 darkens them. This applies after device gamma.");
                    ui.add(egui::Slider::new(&mut self.colors.exposure, ColorSettings::EXPOSURE)
                        .text("Exposure (EV)").fixed_decimals(2))
                        .on_hover_text("Adjust linear-light brightness. +1 EV doubles it; -1 EV halves it. Bright values can clip.");
                    ui.add(egui::Slider::new(&mut self.colors.saturation, ColorSettings::SATURATION)
                        .text("Saturation").fixed_decimals(2))
                        .on_hover_text("0.00 gives grayscale, 1.00 preserves color, and higher values boost color intensity.");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Reset 1:1").on_hover_text("Reset GPU adjustments only. Device controls and the saved preset stay unchanged.").clicked() {
                            self.colors = ColorSettings::default();
                            self.color_status = "GPU defaults applied; use Save preset to keep them".into();
                        }
                        if ui.button("Soft white").clicked() {
                            self.colors = ColorSettings::SOFT_WHITE;
                            self.color_status = "Soft white applied; use Save preset to keep it".into();
                        }
                    });
                    if ui.button("Save preset").clicked() { self.save_colors(config_path().as_deref()); }
                    ui.small(&self.color_status);
                    ui.add_space(10.0);
                    ui.label("GPU adjustments apply after V4L2 device controls. Both gamma controls can be used together.");
                    ui.small("Device controls are not available in this panel. GPU presets do not change device settings or desktop colors.");
                });
            });
    }

    fn toolbar_strip(ctx: &egui::Context) -> egui::Rect {
        let screen = ctx.screen_rect();
        egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), 6.0))
    }

    fn over_toolbar(&self, ctx: &egui::Context, pos: egui::Pos2) -> bool {
        Self::toolbar_strip(ctx).contains(pos) || self.toolbar_rect.is_some_and(|rect| rect.contains(pos))
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let strip = Self::toolbar_strip(ctx);
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("rdp_toolbar"));
        let hovered = ctx.input(|input| input.pointer.hover_pos()).is_some_and(|pos| self.over_toolbar(ctx, pos))
            || ctx.input(|input| input.events.iter().any(|event|
                matches!(event, egui::Event::PointerMoved(pos) if strip.contains(*pos))));
        let keyboard_focus = ctx.memory(|memory| memory.focused())
            .and_then(|id| ctx.read_response(id)).is_some_and(|response| response.layer_id == layer);
        if self.toolbar_pinned || hovered || keyboard_focus || (self.toolbar_visible && ctx.input(|input| input.pointer.any_down())) {
            self.toolbar_visible = true;
            self.toolbar_last_hover = now;
        } else if now - self.toolbar_last_hover >= 0.7 {
            self.toolbar_visible = false;
        }
        self.screenshot_buttons = [None; 2];
        let blue = egui::Color32::from_rgb(32, 77, 112);
        // Reserve only the reveal strip so showing the bar never resizes the remote image.
        egui::TopBottomPanel::top("toolbar_reveal").exact_height(6.0)
            .frame(egui::Frame::NONE.fill(blue)).show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.painter().hline(rect.center().x - 20.0..=rect.center().x + 20.0, rect.center().y,
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(175, 210, 235)));
                ui.interact(rect, ui.id().with("reveal"), egui::Sense::hover()).on_hover_text("Show remote desktop toolbar");
            });
        self.toolbar_rect = None;
        if self.toolbar_visible {
            let width = (ctx.screen_rect().width() - 32.0).min(1000.0).max(280.0);
            let response = egui::Area::new(layer.id).order(layer.order).movable(false)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 6.0)).default_width(width)
                .show(ctx, |ui| {
                    *ui.visuals_mut() = egui::Visuals::dark();
                    egui::Frame::NONE.fill(blue).inner_margin(8)
                        .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 })
                        .show(ui, |ui| {
                            ui.set_width(width - 16.0);
                            ui.horizontal(|ui| {
                                ui.strong("NanoKVM USB");
                                ui.separator();
                                ui.label(format!("{:.0} FPS", self.fps));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.selectable_label(self.toolbar_pinned, "Pin toolbar")
                                        .on_hover_text("Pinned stays visible. Unpin to hide after leaving the toolbar.").clicked() {
                                        self.toolbar_pinned = !self.toolbar_pinned;
                                        self.toolbar_last_hover = now;
                                    }
                                });
                            });
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                let active = self.camera.is_some() || self.serial.is_some();
                                if ui.add_enabled(!active && self.disconnecting.is_none() && self.discovery_rx.is_none(), egui::Button::new("Connect")).clicked() {
                                    self.connect();
                                }
                                if ui.add_enabled((active || self.discovery_rx.is_some()) && self.disconnecting.is_none(), egui::Button::new("Disconnect")).clicked() {
                                    self.disconnect();
                                }
                                ui.separator();
                                let focus_available = self.serial_ready && self.image.is_some() && !self.release_chord_held && !(self.right_ctrl && self.right_alt) && !self.input_blocked
                                    && self.shortcuts.as_ref().is_some_and(|s| s.status() == ShortcutStatus::Ready);
                                if ui.add_enabled(focus_available, egui::Button::new("Focus Mode")).clicked() {
                                    self.request_focus_mode(ctx);
                                }
                                if ui.selectable_label(self.mode == InputMode::NonFocus, "Non-Focus Mode")
                                    .on_hover_text("Use normal video interaction with desktop shortcuts available.").clicked() {
                                    self.release_remote();
                                }
                                if self.remote || self.mode == InputMode::Pending {
                                    if ui.button("Release input (Right-Ctrl + Right-Alt)").clicked() {
                                        self.release_remote();
                                    }
                                } else if ui.add_enabled(self.serial_ready && self.image.is_some() && !self.release_chord_held && !(self.right_ctrl && self.right_alt) && !self.input_blocked, egui::Button::new("▶ Capture input")).clicked() {
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
                                for (index, format, label) in [(0, Format::Png, "Screenshot PNG"), (1, Format::WebP, "Screenshot WebP")] {
                                    let response = ui.add_enabled(self.image.is_some() && self.screenshot.is_none(), egui::Button::new(label))
                                        .on_hover_text("Save the complete remote video at native resolution, before GPU tuning, without toolbar or borders. WebP is lossless.");
                                    self.screenshot_buttons[index] = Some(response.id);
                                    if response.clicked() { self.take_screenshot(format); }
                                }
                                ui.add_enabled(false, egui::Button::new("Capture area"))
                                    .on_disabled_hover_text("Area capture is not available yet.");
                            });
                        });
                });
            self.toolbar_rect = Some(response.response.rect);
            // Mouse actions should not leave hidden keyboard focus on toolbar buttons.
            if ctx.input(|input| input.pointer.any_released()) {
                if let Some(id) = ctx.memory(|memory| memory.focused()) {
                    if ctx.read_response(id).is_some_and(|response| response.layer_id == layer) {
                        ctx.memory_mut(|memory| memory.surrender_focus(id));
                    }
                }
            }
        }
        egui::TopBottomPanel::bottom("status")
            .show(ctx, |ui| {
                ui.small(&self.mode_status);
                if !self.screenshot_status.is_empty() { ui.small(&self.screenshot_status); }
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
            // egui clears video focus on an outside press before this response is created.
            if self.remote && self.screenshot_keeps_focus { response.request_focus(); }
            let toolbar_rect = self.toolbar_rect;
            let strip = Self::toolbar_strip(ctx);
            let pressed = ctx.input(|i| i.events.iter().any(|event|
                matches!(event, egui::Event::PointerButton { pressed: true, pos, .. }
                    if rect.contains(*pos) && !strip.contains(*pos) && !toolbar_rect.is_some_and(|bar| bar.contains(*pos)))));
            if self.serial_ready && self.mode != InputMode::Pending && !self.release_chord_held && !(self.right_ctrl && self.right_alt) && !self.input_blocked && ctx.input(|i| i.focused) && pressed && (response.clicked() || response.is_pointer_button_down_on()) {
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
        let toolbar_rect = self.toolbar_rect;
        let strip = Self::toolbar_strip(ctx);
        let over_video = |pos| video_rect.contains(pos) && !strip.contains(pos)
            && !toolbar_rect.is_some_and(|rect| rect.contains(pos)) && ctx.layer_id_at(pos) == Some(video.layer_id);
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

    fn request_focus_mode(&mut self, ctx: &egui::Context) {
        if !self.serial_ready || self.image.is_none() || self.release_chord_held || (self.right_ctrl && self.right_alt) { return; }
        self.release_remote();
        let result = self.shortcuts.as_mut().ok_or_else(|| anyhow::anyhow!(self.mode_status.clone()))
            .and_then(Shortcuts::request);
        match result {
            Ok(()) => {
                self.mode = InputMode::Pending;
                self.mode_status = "Waiting for compositor to activate Focus Mode…".into();
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            }
            Err(error) => {
                self.release_remote();
                self.mode_status = error.to_string();
            }
        }
    }

    fn refresh_shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(shortcuts) = &mut self.shortcuts {
            let status = shortcuts.poll();
            self.apply_shortcut_status(ctx, status);
        }
    }

    fn apply_shortcut_status(&mut self, ctx: &egui::Context, status: ShortcutStatus) {
        match status {
            ShortcutStatus::Active if self.mode == InputMode::Pending => {
                self.mode = InputMode::Focus;
                self.remote = true;
                self.mode_status = "Focus Mode: Right-Ctrl + Right-Alt releases input".into();
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            }
            ShortcutStatus::Unavailable(error) => {
                if self.mode != InputMode::NonFocus { self.release_remote(); }
                self.mode_status = error;
            }
            ShortcutStatus::Discovering => self.mode_status = "Discovering Wayland shortcut support…".into(),
            ShortcutStatus::Ready => self.mode_status = "Non-Focus Mode: desktop shortcuts remain available".into(),
            _ => {}
        }
    }

    fn consume_keyboard(input: &mut egui::RawInput) {
        input.events.retain(|event| !matches!(event,
            egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy |
            egui::Event::Cut | egui::Event::Paste(_) | egui::Event::Ime(_)));
        input.modifiers = egui::Modifiers::default();
    }

    fn forward_key(&mut self, event: &egui::Event) {
        let egui::Event::Key { key, physical_key, pressed, modifiers, .. } = event else { return; };
        let key = physical_key.unwrap_or(*key);
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
        self.screenshot_keeps_focus = false;
        self.input_blocked = false;
        let native = egui_winit::take_keyboard_input(ctx, input.viewport_id);
        let video_id = egui::Id::new("video");
        if !input.focused || input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))) {
            self.right_ctrl = false;
            self.right_alt = false;
            self.release_chord_held = false;
            if self.remote || self.mode != InputMode::NonFocus { self.release_remote(); }
            ctx.memory_mut(|memory| memory.surrender_focus(video_id));
            return;
        }
        self.refresh_shortcuts(ctx);
        let mut released = false;
        for event in &native {
            match event {
                egui_winit::KeyboardInput::RightControl(pressed) => self.right_ctrl = *pressed,
                egui_winit::KeyboardInput::RightAlt(pressed) => self.right_alt = *pressed,
                _ => continue,
            }
            if self.right_ctrl && self.right_alt && (self.remote || self.mode != InputMode::NonFocus) {
                self.release_chord_held = true;
                self.release_remote();
                released = true;
                ctx.memory_mut(|memory| memory.surrender_focus(video_id));
            }
            if !self.right_ctrl && !self.right_alt { self.release_chord_held = false; }
        }
        let local_click = input.events.iter().any(|event| {
            matches!(event, egui::Event::PointerButton { pos, pressed: true, .. }
                if !self.over_screenshot_button(ctx, *pos) && (self.over_toolbar(ctx, *pos) || !ctx.read_response(video_id).is_some_and(|video|
                    video.rect.contains(*pos) && ctx.layer_id_at(*pos) == Some(video.layer_id))))
        });
        if local_click {
            ctx.memory_mut(|memory| memory.surrender_focus(video_id));
            if self.mode != InputMode::NonFocus { self.release_remote(); }
        }
        if released || self.release_chord_held || self.mode == InputMode::Pending {
            self.input_blocked = true;
            Self::consume_keyboard(input);
            return;
        }
        if !self.remote { return; }
        if !ctx.memory(|memory| memory.has_focus(video_id)) {
            if self.mode == InputMode::Focus { self.release_remote(); }
            if self.keyboard.bytes() != [0; 8] || self.buttons != 0 {
                self.release_held_input();
            }
            return;
        }
        self.screenshot_keeps_focus = input.events.iter().any(|event|
            matches!(event, egui::Event::PointerButton { pos, .. } if self.over_screenshot_button(ctx, *pos)));
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
                    egui_winit::KeyboardInput::RightControl(_) | egui_winit::KeyboardInput::RightAlt(_) => {}
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
        Self::consume_keyboard(input);
    }

    fn on_exit(&mut self) {
        self.release_remote();
        self.shortcuts = None;
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_messages();
        if (self.remote || self.mode != InputMode::NonFocus) && !ctx.input(|i| i.focused) {
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
        if let Some(job) = &self.screenshot { job.worker.stop(); }
        self.disconnect();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        if let Some(camera) = &mut self.camera {
            if let Err(err) = camera.worker.wait_until(deadline) { eprintln!("{err}"); }
        }
        if let Some(serial) = &mut self.serial {
            if let Err(err) = serial.worker.wait_until(deadline) { eprintln!("{err}"); }
        }
        if let Some(job) = &mut self.screenshot {
            if let Err(err) = job.worker.wait_until(deadline) { eprintln!("Screenshot: {err}"); }
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

    #[test]
    fn preset_paths_ignore_empty_and_relative_environment_values() {
        let home = Some(PathBuf::from("/home/test"));
        let fallback = Some(PathBuf::from("/home/test/.config/nanokvm-rust/colors.json"));
        assert_eq!(config_path_from(Some(PathBuf::from("/config")), home.clone()), Some(PathBuf::from("/config/nanokvm-rust/colors.json")));
        for xdg in [None, Some(PathBuf::new()), Some(PathBuf::from("relative"))] {
            assert_eq!(config_path_from(xdg, home.clone()), fallback);
        }
        assert_eq!(config_path_from(None, Some(PathBuf::from("relative"))), None);
        assert_eq!(config_path_from(None, None), None);
    }

    #[test]
    fn preset_status_reports_load_save_and_failures_without_hiding_video_status() {
        let root = std::env::temp_dir().join(format!("nanokvm-color-status-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = root.join("colors.json");
        let mut app = viewer();
        app.video_status = "Video connected".into();
        app.restore_colors(None);
        assert!(app.color_status.contains("no absolute HOME"));
        app.restore_colors(Some(&path));
        assert_eq!(app.colors, ColorSettings::default());
        assert!(app.color_status.contains("No saved GPU preset"));
        app.colors = ColorSettings::SOFT_WHITE;
        app.save_colors(Some(&path));
        assert!(app.color_status.contains("GPU preset saved"));
        app.colors = ColorSettings::default();
        app.restore_colors(Some(&path));
        assert_eq!(app.colors, ColorSettings::SOFT_WHITE);
        assert!(app.color_status.contains("GPU preset loaded"));
        app.save_colors(None);
        assert!(app.color_status.contains("Cannot save"));
        assert_eq!(app.colors, ColorSettings::SOFT_WHITE);
        std::fs::write(&path, b"invalid preset").unwrap();
        app.restore_colors(Some(&path));
        assert_eq!(app.colors, ColorSettings::default());
        assert!(app.color_status.contains("Using GPU defaults: Invalid GPU preset JSON"));
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid preset");
        app.save_colors(Some(&path.join("child.json")));
        assert!(app.color_status.contains("Could not save GPU preset"));
        app.restore_colors(Some(&root));
        assert!(app.color_status.contains("Read GPU preset"));
        assert_eq!(app.video_status, "Video connected");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn color_buttons_apply_soft_white_and_reset_gpu_controls() {
        let mut app = viewer();
        app.colors_open = true;
        let ctx = egui::Context::default();
        for (label, expected) in [("Soft white", ColorSettings::SOFT_WHITE), ("Reset 1:1", ColorSettings::default())] {
            app.colors = ColorSettings { exposure: 1.0, gamma: 1.3, highlights: 0.5, white: 0.7, saturation: 0.3 };
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 800.0))),
                ..Default::default()
            };
            let output = ctx.run(raw.clone(), |ctx| app.color_panel(ctx));
            let pos = output.shapes.iter().find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    if text.galley.text() == label { return Some(text.pos + egui::vec2(3.0, 3.0)); }
                }
                None
            }).expect("color preset button");
            let raw = egui::RawInput { events: vec![
                egui::Event::PointerMoved(pos), pointer_button(pos, true), pointer_button(pos, false),
            ], ..raw };
            let _ = ctx.run(raw, |ctx| app.color_panel(ctx));
            assert_eq!(app.colors, expected);
            assert!(app.color_status.contains("use Save preset"));
        }
    }

    fn wait_screenshot(app: &mut Viewer) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.screenshot.is_some() {
            app.refresh_screenshot();
            assert!(Instant::now() < deadline, "screenshot did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn screenshot_buttons_keep_remote_keyboard_ownership_for_both_formats() {
        let root = std::env::temp_dir().join(format!("nanokvm-screenshot-ui-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        for (remote, video_focus, focus_mode, auto_hide) in [
            (false, false, false, false), (true, false, false, false),
            (true, true, false, false), (true, true, true, false),
            (true, true, false, true), (true, true, true, true),
        ] {
            for (index, extension) in [(0, "png"), (1, "webp")] {
                let mut peer = crate::shortcuts::tests::Peer::new();
                let (mut app, ctx, reports) = input_viewer();
                if focus_mode {
                    app.shortcuts = Some(peer.controller(true));
                    app.request_focus_mode(&ctx);
                    peer.activate(app.shortcuts.as_mut().unwrap());
                    app.refresh_shortcuts(&ctx);
                    reports.try_iter().for_each(drop);
                }
                app.remote = remote;
                app.screenshot_directory = Some(root.clone());
                app.colors = ColorSettings::SOFT_WHITE;
                let image = Arc::new(RgbaFrame { width: 2, height: 1, rgba: vec![255,255,255,255, 200,80,40,255] });
                app.image = Some(image.clone());
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 600.0))),
                    ..Default::default()
                };
                for _ in 0..2 { let _ = ctx.run(raw.clone(), |ctx| { app.toolbar(ctx); app.render_video(ctx); }); }
                if video_focus {
                    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
                    app.keyboard.set_key(4, true);
                    app.remote_super = true;
                    app.keyboard.modifiers = protocol::modifiers::LEFT_SUPER;
                }
                if auto_hide {
                    app.toolbar_pinned = false;
                    toolbar_frame(&mut app, &ctx, egui::vec2(1400.0, 600.0), 1.0, vec![egui::Event::PointerGone]);
                    assert!(!app.toolbar_visible);
                    toolbar_frame(&mut app, &ctx, egui::vec2(1400.0, 600.0), 1.1,
                        vec![egui::Event::PointerMoved(Viewer::toolbar_strip(&ctx).center())]);
                    toolbar_frame(&mut app, &ctx, egui::vec2(1400.0, 600.0), 1.2, vec![]);
                    assert!(app.toolbar_visible);
                }
                let pos = ctx.read_response(app.screenshot_buttons[index].unwrap()).unwrap().rect.center();
                for pressed in [true, false] {
                    let mut input = egui::RawInput { events: vec![egui::Event::PointerMoved(pos), pointer_button(pos, pressed)], ..raw.clone() };
                    app.raw_input_hook(&ctx, &mut input);
                    if video_focus { assert!(app.screenshot_keeps_focus, "lost ownership before egui: {index}, {pressed}"); }
                    let _ = ctx.run(input, |ctx| { app.toolbar(ctx); app.render_video(ctx); });
                    assert_eq!(app.remote, remote);
                    assert_eq!(app.mode, if focus_mode { InputMode::Focus } else { InputMode::NonFocus });
                    if video_focus {
                        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("video"))),
                            "index={index}, pressed={pressed}, focus={:?}, button={:?}",
                            ctx.memory(|memory| memory.focused()),
                            ctx.read_response(app.screenshot_buttons[index].unwrap()).map(|response|
                                (response.rect, response.is_pointer_button_down_on(), response.clicked())));
                        assert_eq!(app.keyboard.bytes(), [8, 0, 4, 0, 0, 0, 0, 0]);
                    }
                    if !video_focus { assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new("video")))); }
                    assert!(reports.is_empty(), "screenshot sent a remote input or release report");
                }
                let path = app.screenshot.as_ref().expect("button starts export").path.clone();
                assert_eq!(path.extension().unwrap(), extension);
                app.take_screenshot(Format::Png);
                assert_eq!(app.screenshot.as_ref().unwrap().path, path);
                // The job owns the clicked frame even when live capture changes or disconnects.
                app.image = None;
                wait_screenshot(&mut app);
                assert!(app.screenshot_status.starts_with("Screenshot saved:"));
                assert!(app.screenshot_status.contains(path.to_str().unwrap()));
                let decoded = image::open(&path).unwrap().to_rgba8();
                assert_eq!(decoded.dimensions(), (2, 1));
                assert_eq!(decoded.as_raw(), &image.rgba);
                assert_eq!(app.colors, ColorSettings::SOFT_WHITE);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn screenshot_errors_do_not_release_input_or_hide_connection_status() {
        let (mut app, ctx, reports) = input_viewer();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        app.keyboard.set_key(4, true);
        app.video_status = "Video connected".into();
        app.image = None;
        app.take_screenshot(Format::Png);
        assert!(app.screenshot_status.contains("no video frame"));
        assert!(app.screenshot.is_none());
        app.image = Some(Arc::new(RgbaFrame { width: 1, height: 1, rgba: vec![0,0,0,255] }));
        app.screenshot_directory = None;
        app.take_screenshot(Format::WebP);
        assert!(app.screenshot_status.contains("no absolute HOME"));
        assert!(app.screenshot.is_none());
        let root = std::env::temp_dir().join(format!("nanokvm-screenshot-error-ui-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&root, b"not a directory").unwrap();
        app.screenshot_directory = Some(root.clone());
        app.take_screenshot(Format::Png);
        wait_screenshot(&mut app);
        assert!(app.screenshot_status.contains("Screenshot failed: Create screenshot directory"));
        assert_eq!(std::fs::read(&root).unwrap(), b"not a directory");
        assert!(app.remote && app.keyboard.bytes()[2] == 4);
        assert!(reports.is_empty());
        assert_eq!(app.video_status, "Video connected");
        std::fs::remove_file(root).unwrap();
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
    fn right_modifier_chord_releases_both_orders_and_requires_both_keys_up() {
        use egui_winit::KeyboardInput::{RightControl, RightAlt, Modifiers};
        for reverse in [false, true] {
            let (mut app, ctx, reports) = input_viewer();
            app.mode = InputMode::Focus;
            hold_input(&mut app);
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
            native_keyboard_frame(&mut app, &ctx, vec![Modifiers(egui::Modifiers::CTRL | egui::Modifiers::ALT)],
                egui::RawInput { modifiers: egui::Modifiers::CTRL | egui::Modifiers::ALT, ..Default::default() });
            assert_eq!(app.mode, InputMode::Focus, "aggregate/left modifiers cannot release");
            reports.try_iter().for_each(drop);
            native_keyboard_frame(&mut app, &ctx, vec![RightControl(true), RightControl(false), RightAlt(true), RightAlt(false)],
                egui::RawInput::default());
            assert_eq!(app.mode, InputMode::Focus, "a non-overlapping sequence cannot release");
            reports.try_iter().for_each(drop);
            native_keyboard_frame(&mut app, &ctx, vec![if reverse { RightAlt(true) } else { RightControl(true) }],
                egui::RawInput::default());
            assert_eq!(app.mode, InputMode::Focus);
            reports.try_iter().for_each(drop);
            native_keyboard_frame(&mut app, &ctx, vec![if reverse { RightControl(true) } else { RightAlt(true) }],
                egui::RawInput { events: vec![key(egui::Key::A, true), egui::Event::Text("must not leak".into())], ..Default::default() });
            assert_eq!(app.mode, InputMode::NonFocus);
            assert!(!app.remote && app.release_chord_held && app.input_blocked);
            assert_eq!(app.keyboard.bytes(), [0; 8]);
            assert_eq!(app.buttons, 0);
            assert!(!app.mapped_meta_held && !app.remote_super);
            assert!(matches!(reports.try_recv().unwrap(), Report::Release(_)));
            assert!(reports.is_empty());
            native_keyboard_frame(&mut app, &ctx, vec![RightControl(false)], egui::RawInput::default());
            assert!(app.release_chord_held);
            native_keyboard_frame(&mut app, &ctx, vec![RightAlt(false)], egui::RawInput::default());
            assert!(!app.release_chord_held && !app.remote);
            assert!(reports.is_empty());
        }
    }

    #[test]
    fn focus_mode_waits_for_ack_and_all_exit_paths_destroy_inhibition() {
        use crate::shortcuts::tests::Peer;
        for exit in ["release", "local", "focus_loss", "disconnect", "video_failure", "serial_failure", "revoked", "exit", "drop"] {
            let mut peer = Peer::new();
            let (mut app, ctx, reports) = input_viewer();
            app.shortcuts = Some(peer.controller(true));
            app.request_focus_mode(&ctx);
            assert_eq!(app.mode, InputMode::Pending);
            assert!(!app.remote);
            reports.try_iter().for_each(drop);
            native_keyboard_frame(&mut app, &ctx, vec![egui_winit::KeyboardInput::Key(key(egui::Key::A, true))],
                egui::RawInput::default());
            assert!(reports.is_empty() && !app.remote);
            let inhibitor = peer.activate(app.shortcuts.as_mut().unwrap());
            app.raw_input_hook(&ctx, &mut egui::RawInput::default());
            assert_eq!(app.mode, InputMode::Focus);
            assert!(app.remote);
            app.map_f12_to_meta = true;
            input_frame(&mut app, &ctx, vec![key(egui::Key::F12, true), key(egui::Key::Escape, true)], &mut String::new());
            assert!(app.remote && app.mapped_meta_held);
            assert_eq!(app.keyboard.bytes(), [8, 0, 0x29, 0, 0, 0, 0, 0]);
            reports.try_iter().for_each(drop);
            match exit {
                "release" => native_keyboard_frame(&mut app, &ctx, vec![
                    egui_winit::KeyboardInput::RightControl(true), egui_winit::KeyboardInput::RightAlt(true),
                ], egui::RawInput::default()),
                "local" => input_frame(&mut app, &ctx, vec![pointer_button(egui::pos2(40.0, 10.0), true)], &mut String::new()),
                "focus_loss" => input_frame(&mut app, &ctx, vec![egui::Event::WindowFocused(false)], &mut String::new()),
                "disconnect" => app.disconnect(),
                "video_failure" => { app.camera = Some(camera(true).0); wait_disconnected(&mut app); }
                "serial_failure" => {
                    app.serial.as_ref().unwrap().worker.stop();
                    wait_disconnected(&mut app);
                }
                "revoked" => {
                    peer.revoke(inhibitor, app.shortcuts.as_mut().unwrap());
                    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
                }
                "exit" => app.on_exit(),
                "drop" => {
                    drop(app);
                    assert!(peer.requests().contains(&(inhibitor, 0, vec![])));
                    continue;
                }
                _ => unreachable!(),
            }
            assert_eq!(app.mode, InputMode::NonFocus, "{exit}");
            assert!(!app.remote && !app.mapped_meta_held && !app.remote_super, "{exit}");
            assert_eq!(app.keyboard.bytes(), [0; 8], "{exit}");
            assert!(peer.requests().contains(&(inhibitor, 0, vec![])), "{exit}");
        }
    }

    fn toolbar_frame(app: &mut Viewer, ctx: &egui::Context, size: egui::Vec2, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time), events, ..Default::default() };
        app.raw_input_hook(ctx, &mut raw);
        ctx.run(raw, |ctx| { app.toolbar(ctx); app.render_video(ctx); })
    }

    fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.text() == label { return Some(text.pos + egui::vec2(3.0, 3.0)); }
            }
            None
        }).unwrap_or_else(|| panic!("missing toolbar label: {label}"))
    }

    #[test]
    fn toolbar_pin_hide_reveal_and_resize_preserve_video_geometry() {
        for (size, fullscreen) in [(egui::vec2(640.0, 480.0), false), (egui::vec2(1920.0, 1080.0), true)] {
            let (mut app, ctx, reports) = input_viewer();
            app.fullscreen = fullscreen;
            for time in [0.0, 0.02, 0.04, 0.06] { toolbar_frame(&mut app, &ctx, size, time, vec![]); }
            let output = toolbar_frame(&mut app, &ctx, size, 0.1, vec![]);
            let rect = app.toolbar_rect.unwrap();
            assert!(rect.left() >= 0.0 && rect.right() <= size.x && rect.top() >= 6.0 && rect.bottom() < size.y);
            let video = ctx.read_response(egui::Id::new("video")).unwrap().rect;
            let pin = label_position(&output, "Pin toolbar");
            toolbar_frame(&mut app, &ctx, size, 0.2, vec![egui::Event::PointerMoved(pin), pointer_button(pin, true), pointer_button(pin, false)]);
            assert!(!app.toolbar_pinned && app.toolbar_visible);
            reports.try_iter().for_each(drop);
            toolbar_frame(&mut app, &ctx, size, 0.4, vec![egui::Event::PointerGone]);
            assert!(app.toolbar_visible, "hide delay keeps the bar reachable");
            toolbar_frame(&mut app, &ctx, size, 1.1, vec![]);
            assert!(!app.toolbar_visible && app.toolbar_rect.is_none());
            assert!(app.screenshot_buttons.iter().all(Option::is_none));
            assert_eq!(ctx.read_response(egui::Id::new("video")).unwrap().rect, video);
            let strip = Viewer::toolbar_strip(&ctx);
            assert_eq!(strip.height(), 6.0);
            toolbar_frame(&mut app, &ctx, size, 1.2, vec![egui::Event::PointerMoved(strip.center())]);
            assert!(app.toolbar_visible);
            let output = toolbar_frame(&mut app, &ctx, size, 1.3, vec![]);
            assert_eq!(ctx.read_response(egui::Id::new("video")).unwrap().rect, video);
            assert!(reports.is_empty(), "reveal motion must stay local");
            let pin = label_position(&output, "Pin toolbar");
            toolbar_frame(&mut app, &ctx, size, 1.4, vec![egui::Event::PointerMoved(pin), pointer_button(pin, true), pointer_button(pin, false)]);
            assert!(app.toolbar_pinned);
            toolbar_frame(&mut app, &ctx, size, 3.0, vec![egui::Event::PointerGone]);
            assert!(app.toolbar_visible);
            let resized = egui::vec2(800.0, 600.0);
            toolbar_frame(&mut app, &ctx, resized, 3.1, vec![]);
            toolbar_frame(&mut app, &ctx, resized, 3.2, vec![]);
            let rect = app.toolbar_rect.unwrap();
            assert!(rect.left() >= 0.0 && rect.right() <= resized.x && rect.top() >= 6.0);
            app.toolbar_pinned = false;
            let action = app.screenshot_buttons[0].unwrap();
            ctx.memory_mut(|memory| memory.request_focus(action));
            toolbar_frame(&mut app, &ctx, resized, 4.2, vec![egui::Event::PointerGone]);
            assert!(app.toolbar_visible, "keyboard-focused controls must remain reachable");
            ctx.memory_mut(|memory| memory.surrender_focus(action));
            toolbar_frame(&mut app, &ctx, resized, 5.0, vec![]);
            assert!(!app.toolbar_visible);
        }
    }

    #[test]
    fn toolbar_overlay_blocks_pointer_and_wheel_on_reveal_and_owned_release() {
        let (mut app, ctx, reports) = input_viewer();
        let size = egui::vec2(1000.0, 700.0);
        toolbar_frame(&mut app, &ctx, size, 0.0, vec![]);
        let output = toolbar_frame(&mut app, &ctx, size, 0.1, vec![]);
        let area = label_position(&output, "Capture area");
        app.toolbar_pinned = false;
        toolbar_frame(&mut app, &ctx, size, 1.0, vec![egui::Event::PointerGone]);
        assert!(!app.toolbar_visible);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("video")));
        app.remote = true;
        app.last_mouse = (0.25, 0.75);
        toolbar_frame(&mut app, &ctx, size, 1.1, vec![
            egui::Event::PointerMoved(Viewer::toolbar_strip(&ctx).center()),
            egui::Event::PointerMoved(area), pointer_button(area, true), pointer_button(area, false),
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 20.0), modifiers: egui::Modifiers::default() },
        ]);
        assert!(app.toolbar_visible);
        assert_eq!(app.last_mouse, (0.25, 0.75));
        assert!(reports.is_empty(), "toolbar gestures reached remote video");
        assert!(app.screenshot.is_none(), "unavailable area action started an export");
        app.buttons = 1;
        toolbar_frame(&mut app, &ctx, size, 1.2, vec![pointer_button(area, false)]);
        assert!(matches!(reports.try_recv().unwrap(), Report::AbsoluteMouse(bytes)
            if bytes == protocol::mouse_absolute(0, 0.25, 0.75, 0)));
        assert_eq!(app.buttons, 0);
        assert!(reports.is_empty());
    }

    #[test]
    fn toolbar_unavailable_actions_and_fullscreen_commands() {
        let mut app = viewer();
        let ctx = egui::Context::default();
        let size = egui::vec2(800.0, 600.0);
        toolbar_frame(&mut app, &ctx, size, 0.0, vec![]);
        let output = toolbar_frame(&mut app, &ctx, size, 0.1, vec![]);
        for id in app.screenshot_buttons.iter().flatten() { assert!(!ctx.read_response(*id).unwrap().enabled()); }
        for label in ["Focus Mode", "Disconnect", "Capture area", "Screenshot PNG", "Screenshot WebP"] {
            let pos = label_position(&output, label);
            toolbar_frame(&mut app, &ctx, size, 0.2, vec![egui::Event::PointerMoved(pos), pointer_button(pos, true), pointer_button(pos, false)]);
            assert_eq!(app.mode, InputMode::NonFocus);
            assert!(app.screenshot.is_none() && app.camera.is_none() && app.serial.is_none());
        }
        for (label, expected) in [("Fullscreen", true), ("Windowed", false)] {
            let output = toolbar_frame(&mut app, &ctx, size, 0.3, vec![]);
            let pos = label_position(&output, label);
            let output = toolbar_frame(&mut app, &ctx, size, 0.4, vec![egui::Event::PointerMoved(pos), pointer_button(pos, true), pointer_button(pos, false)]);
            assert_eq!(app.fullscreen, expected);
            assert!(output.viewport_output[&egui::ViewportId::ROOT].commands.iter().any(|command|
                matches!(command, egui::ViewportCommand::Fullscreen(value) if *value == expected)));
            assert_eq!(Viewer::toolbar_strip(&ctx).height(), 6.0);
        }
    }

    #[test]
    fn toolbar_switches_modes_without_reusing_held_input() {
        let mut peer = crate::shortcuts::tests::Peer::new();
        let (mut app, ctx, reports) = input_viewer();
        app.shortcuts = Some(peer.controller(true));
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 700.0))),
            ..Default::default()
        };
        for label in ["Focus Mode", "Non-Focus Mode"] {
            let _ = ctx.run(raw.clone(), |ctx| { app.toolbar(ctx); app.render_video(ctx); });
            let output = ctx.run(raw.clone(), |ctx| { app.toolbar(ctx); app.render_video(ctx); });
            let pos = output.shapes.iter().find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    if text.galley.text() == label { return Some(text.pos + egui::vec2(3.0, 3.0)); }
                }
                None
            }).expect("mode button label");
            let mut input = egui::RawInput { events: vec![egui::Event::PointerMoved(pos),
                pointer_button(pos, true), pointer_button(pos, false)], ..raw.clone() };
            app.raw_input_hook(&ctx, &mut input);
            let _ = ctx.run(input, |ctx| { app.toolbar(ctx); app.render_video(ctx); });
            if label == "Focus Mode" {
                assert_eq!(app.mode, InputMode::Pending);
                peer.activate(app.shortcuts.as_mut().unwrap());
                app.raw_input_hook(&ctx, &mut raw.clone());
                assert_eq!(app.mode, InputMode::Focus);
                hold_input(&mut app);
                reports.try_iter().for_each(drop);
            } else {
                assert_eq!(app.mode, InputMode::NonFocus);
                assert!(!app.remote && !app.remote_super && !app.mapped_meta_held);
                assert_eq!(app.keyboard.bytes(), [0; 8]);
                assert_eq!(app.buttons, 0);
                assert!(peer.requests().iter().any(|(_, op, body)| *op == 0 && body.is_empty()));
                assert!(matches!(reports.try_recv().unwrap(), Report::Release(_)));
            }
        }
    }

    #[test]
    fn pending_focus_can_be_cancelled_by_local_controls_or_disconnect() {
        use crate::shortcuts::tests::Peer;
        for local in [true, false] {
            let mut peer = Peer::new();
            let (mut app, ctx, reports) = input_viewer();
            app.shortcuts = Some(peer.controller(true));
            app.request_focus_mode(&ctx);
            let requests = peer.requests();
            let inhibitor = requests.iter().find(|(_, op, body)| *op == 1 && body.len() == 3).unwrap().2[0];
            reports.try_iter().for_each(drop);
            if local { input_frame(&mut app, &ctx, vec![pointer_button(egui::pos2(40.0, 10.0), true)], &mut String::new()); }
            else { app.disconnect(); }
            assert_eq!(app.mode, InputMode::NonFocus);
            assert!(!app.remote);
            assert!(peer.requests().contains(&(inhibitor, 0, vec![])));
        }
        let (mut app, ctx, _) = input_viewer();
        app.request_focus_mode(&ctx);
        assert_eq!(app.mode, InputMode::NonFocus);
        assert!(!app.remote);
        assert!(app.mode_status.contains("unavailable"));
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
    fn mapped_meta_releases_on_local_focus_and_window_focus_loss() {
        for events in [
            vec![pointer_button(egui::pos2(40.0, 10.0), true)],
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
            let _ = ctx.run(raw.clone(), |ctx| app.toolbar(ctx));
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
        assert!(app.remote && local == "a");
        assert!(matches!(reports.try_recv().unwrap(), Report::Keyboard(bytes) if bytes[3] == 0x29));
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
        native_keyboard_frame(&mut app, &ctx, vec![
            egui_winit::KeyboardInput::RightControl(true), egui_winit::KeyboardInput::RightAlt(true),
        ], egui::RawInput::default());
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
