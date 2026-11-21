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
use actcraft_ui::{ActCraftApp, FileAsk, FileKind, FileRequest, PlatformInfo};
use craft_dialogs::{Answer, Dialogs, FileDialog};
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
    /// Native file pickers, shown without blocking the event loop.
    dialogs: Dialogs<FileAsk>,
    /// Settings kept on this computer (the audio interface).
    prefs: craft_settings::SettingsStore<Prefs>,
}

/// Turns the window's request into a native file picker.
fn to_dialog(req: &FileRequest) -> FileDialog {
    let mut d = match req.kind {
        FileKind::Open => FileDialog::open(),
        FileKind::OpenMany => FileDialog::open_many(),
        FileKind::Save => FileDialog::save(),
    }
    .title(req.title);
    for (name, exts) in &req.filters {
        d = d.filter(*name, exts);
    }
    if let Some(dir) = &req.directory {
        d = d.directory(dir.clone());
    }
    if let Some(name) = &req.file_name {
        d = d.file_name(name.clone());
    }
    d
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.app.platform.time_of_day = chrono_now();
        // Answers first, then draw, then show what the window asked for.
        for (ask, answer) in self.dialogs.take_answers() {
            match answer {
                Answer::Paths(paths) => self.app.file_chosen(ask, paths),
                Answer::Unavailable(why) => self.app.file_dialog_failed(why),
                Answer::Cancelled | Answer::Message(_) => self.app.file_cancelled(ask),
            }
        }
        self.app.ui(ui);
        if let Some(device) = self.app.take_device_request() {
            self.switch_device(device);
        }
        if let Some(language) = self.app.take_language_request() {
            self.prefs.update(|pr| pr.window.language = language);
        }
        let _ = self.prefs.tick(std::time::Instant::now());
        for req in self.app.take_file_requests() {
            let dialog = to_dialog(&req);
            self.dialogs.ask(dialog, req.ask);
        }
        self.dialogs.show_pending(Some(&*frame));
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

impl Host {
    /// Stops the current output and opens another device; the engine gets the new stream.
    fn switch_device(&mut self, device: Option<String>) {
        self._audio = None;
        match AudioOutput::open(device.as_deref()) {
            Ok((out, controller)) => {
                let p = &mut self.app.platform;
                p.audio_device = Some(out.device_name.clone());
                p.sample_rate = out.sample_rate;
                p.output_channels = out.channels;
                p.device_id = device.clone();
                self.app.engine().attach_audio(controller);
                self.app.engine().say(format!("Audio: {}", out.device_name));
                self._audio = Some(out);
                self.prefs.update(|pr| pr.audio.device = device.unwrap_or_default());
            }
            Err(e) => {
                self.app.platform.audio_device = None;
                self.app.engine().say_error(format!("Could not open that audio interface: {e}"));
            }
        }
    }
}

/// What ActCraft remembers on this computer (not in the show file).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Prefs {
    audio: AudioPrefs,
    window: WindowPrefs,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct WindowPrefs {
    /// `auto` or empty for the system language, `en` or `de`.
    language: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AudioPrefs {
    /// Output device id; empty for the system default.
    device: String,
}

impl craft_settings::Settings for Prefs {
    const APP_ID: &'static str = "actcraft";
    const SCHEMA_VERSION: u32 = 1;
}

fn chrono_now() -> String {
    use actcraft_engine::Services;
    NativeServices.now().format("%H:%M:%S").to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).take(16).collect();
    let mut engine = Engine::new(Box::new(NativeServices));
    let mut platform = PlatformInfo::default();
    let (prefs, _report) = craft_settings::SettingsStore::<Prefs>::open(craft_settings::Location::Auto);
    platform.devices = actcraft_platform::output_devices().into_iter().map(|d| (d.id, d.name)).collect();
    // The language chosen last time (`--lang CODE` overrides it, for screenshots).
    let language = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| prefs.get().window.language.clone());
    platform.language = if language.is_empty() { "auto".to_owned() } else { language };
    actcraft_ui::set_language(&platform.language);

    // The interface chosen last time, or the system default when it's gone.
    let saved = Some(prefs.get().audio.device.clone()).filter(|d| !d.is_empty());
    let opened = match AudioOutput::open(saved.as_deref()) {
        Ok(ok) => Ok((ok, saved)),
        Err(_) if saved.is_some() => AudioOutput::open(None).map(|ok| (ok, None)),
        Err(e) => Err(e),
    };
    let audio = match opened {
        Ok(((out, controller), device_id)) => {
            platform.audio_device = Some(out.device_name.clone());
            platform.sample_rate = out.sample_rate;
            platform.output_channels = out.channels;
            platform.device_id = device_id;
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

    let open_settings = args.iter().any(|a| a == "--open-settings");
    let show_lyrics = args.iter().any(|a| a == "--lyrics");
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
            let ctx = cc.egui_ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || ctx.request_repaint()));
            let mut app = ActCraftApp::new(&cc.egui_ctx, engine, platform);
            if open_settings {
                app.open_settings();
            }
            if show_lyrics {
                app.show_lyrics();
            }
            Ok(Box::new(Host { app, _audio: audio, screenshot, dialogs, prefs }))
        }),
    );
    if let Err(e) = result {
        eprintln!("ActCraft could not open its window: {e}");
        std::process::exit(1);
    }
}
