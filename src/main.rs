mod app;
mod discovery;
mod capture;
mod gpu;
mod input;
mod protocol;
mod serial;
mod screenshot;
mod worker;
mod shortcuts;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut video = String::new();
    let mut serial = String::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--video" => video = args.next().unwrap_or_else(|| video.clone()),
            "--serial" => serial = args.next().unwrap_or_default(),
            "--help" | "-h" => {
                println!("Usage: nanokvm-rust [--video PATH --serial PATH]\nWithout paths, discover NanoKVM automatically; Connect starts the devices.");
                return Ok(());
            }
            _ => eprintln!("Ignoring unknown argument: {arg}"),
        }
    }
    let options = app::Options { video, serial };
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NanoKVM USB — Rust Viewer")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "NanoKVM USB — Rust Viewer",
        native,
        Box::new(move |cc| Ok(Box::new(app::Viewer::new(cc, options)))),
    )
}
