//! The ActCraft desktop app: the window, the audio output and the JSON control channel.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use actcraft_control::channel;
use actcraft_engine::Engine;
use actcraft_platform::{AudioOutput, NativeServices};
use actcraft_ui::{ActCraftApp, PlatformInfo};
use serde_json::json;

/// `--screenshot FILE`: save a PNG of the window after it settles, then close (for docs and
/// visual checks).
struct Screenshot {
    path: std::path::PathBuf,
    started: std::time::Instant,
    requested: bool,
    done: Arc<std::sync::atomic::AtomicBool>,
}

struct Host {
    app: ActCraftApp,
    // Kept alive for the lifetime of the window: dropping it stops the sound.
    _audio: Option<AudioOutput>,
    screenshot: Option<Screenshot>,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.platform.time_of_day = chrono_now();
        self.app.ui(ui);
        if let Some(shot) = &mut self.screenshot {
            let ctx = ui.ctx().clone();
            if !shot.requested && shot.started.elapsed() > std::time::Duration::from_secs(2) {
                shot.requested = true;
                let path = shot.path.clone();
                let done = Arc::clone(&shot.done);
                ctx.request_screenshot(move |image| {
                    let [w, h] = image.size;
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    let saved = u32::try_from(w)
                        .ok()
                        .zip(u32::try_from(h).ok())
                        .map(|(w, h)| image::save_buffer(&path, &bytes, w, h, image::ExtendedColorType::Rgba8));
                    if !matches!(saved, Some(Ok(()))) {
                        eprintln!("could not save the screenshot to {}", path.display());
                    }
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
            if shot.done.load(std::sync::atomic::Ordering::Relaxed) {
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }
}

fn chrono_now() -> String {
    use actcraft_engine::Services;
    NativeServices.now().format("%H:%M:%S").to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).take(16).collect();
    let mut engine = Engine::new(Box::new(NativeServices));
    let mut platform = PlatformInfo::default();

    let audio = match AudioOutput::open(None) {
        Ok((out, controller)) => {
            platform.audio_device = Some(out.device_name.clone());
            platform.sample_rate = out.sample_rate;
            engine.attach_audio(controller);
            Some(out)
        }
        Err(e) => {
            engine.say_error(format!("No audio output: {e}"));
            None
        }
    };

    // A show file given on the command line opens at start.
    if let Some(path) = args.iter().find(|a| a.ends_with(".actcraft")) {
        let _ = engine.run("show.open", &json!({"path": path}));
    }

    let engine = Arc::new(Mutex::new(engine));
    let addr = SocketAddr::from(([127, 0, 0, 1], channel::DEFAULT_PORT));
    match channel::serve(addr, Arc::clone(&engine), None) {
        Ok(a) => platform.control_address = Some(a.to_string()),
        Err(e) => {
            if let Ok(mut en) = engine.lock() {
                en.say_error(format!("Remote control is off: {e}"));
            }
        }
    }

    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(|p| Screenshot {
        path: std::path::PathBuf::from(p),
        started: std::time::Instant::now(),
        requested: false,
        done: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    // The logo (see ATTRIBUTION.md) is the window icon and greets on the empty screen.
    let logo = image::load_from_memory(include_bytes!("../../../assets/logo/actcraft-icon-256.png"))
        .ok()
        .map(|i| i.to_rgba8());
    let mut viewport = eframe::egui::ViewportBuilder::default();
    if let Some(img) = &logo {
        viewport = viewport.with_icon(eframe::egui::IconData {
            rgba: img.as_raw().clone(),
            width: img.width(),
            height: img.height(),
        });
        platform.logo = Some((img.width() as usize, img.height() as usize, img.as_raw().clone()));
    }
    let options = eframe::NativeOptions {
        viewport: viewport.with_title("ActCraft").with_inner_size([1280.0, 800.0]).with_min_inner_size([800.0, 520.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let result = eframe::run_native(
        "ActCraft",
        options,
        Box::new(move |cc| {
            Ok(Box::new(Host { app: ActCraftApp::new(&cc.egui_ctx, engine, platform), _audio: audio, screenshot }))
        }),
    );
    if let Err(e) = result {
        eprintln!("ActCraft could not open its window: {e}");
        std::process::exit(1);
    }
}
