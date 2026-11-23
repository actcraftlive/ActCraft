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
use actcraft_engine::{Engine, MessageArg};

use actcraft_engine::midi_sync::SyncConfig;
use actcraft_platform::{AudioOutput, MidiTimeOut, NativeServices, StatusReader};
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
    /// Where MIDI time goes while it plays, and the playback position it follows.
    midi: Option<MidiTimeOut>,
    status: Option<StatusReader>,
    /// The song, MIDI file and latency offset the MIDI output plays now (loaded again when one
    /// changes).
    midi_part: Option<(u64, Option<String>, u64)>,
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
        if let Some(port) = self.app.take_midi_request() {
            self.switch_midi(port);
        }
        if self.app.take_midi_refresh() {
            self.app.platform.midi_ports = actcraft_platform::midi_output_ports();
        }
        // What the show sends over MIDI can change any moment (Settings, a remote command).
        if let Some(midi) = &self.midi {
            midi.set(SyncConfig::from(&self.app.engine().show.midi));
        }
        self.follow_midi_file();
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
    /// Hands the current song's MIDI file to the MIDI output when the song, its file or the
    /// MIDI latency offset changed since the last look.
    fn follow_midi_file(&mut self) {
        let Some(midi) = &self.midi else { return };
        let (key, part) = {
            let e = self.app.engine();
            let key = e.current_song.map(|id| {
                let file = e.show.song(id).and_then(|s| s.midi_file.clone());
                (id, file, e.show.mixer.midi_latency_offset_ms.to_bits())
            });
            if key == self.midi_part {
                return;
            }
            let part = match key.as_ref().map(|k| actcraft_engine::midi_file::song_part(&e, k.0)) {
                Some(Ok(part)) => part,
                Some(Err(err)) => {
                    drop(e);
                    self.app.engine().say_error(err.message());
                    None
                }
                None => None,
            };
            (key, part)
        };
        midi.set_part(part.map(Arc::new));
        self.midi_part = key;
    }

    /// Closes the MIDI time output and opens `port` (none for `None`), following the playback
    /// position of the running audio output. Remembered on this computer.
    fn switch_midi(&mut self, port: Option<String>) {
        self.midi = None;
        self.midi_part = None;
        self.app.platform.midi_port = None;
        let name = port.clone().unwrap_or_default();
        self.prefs.update(|pr| pr.audio.midi_output = name);
        let (Some(port), Some(status)) = (port, self.status.clone()) else { return };
        let cfg = SyncConfig::from(&self.app.engine().show.midi);
        match MidiTimeOut::open(&port, status, cfg) {
            Ok(out) => {
                self.app.platform.midi_port = Some(out.port().to_owned());
                self.app.engine().say_with("MIDI: {port}", vec![MessageArg::plain("port", &port)]);
                self.midi = Some(out);
            }
            Err(e) => {
                self.app
                    .engine()
                    .say_error_with("Could not open that MIDI output: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

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
                self.status = Some(controller.status_reader());
                self.app.engine().attach_audio(controller);
                self.app.engine().say_with("Audio: {device}", vec![MessageArg::plain("device", &out.device_name)]);
                self._audio = Some(out);
                self.prefs.update(|pr| pr.audio.device = device.unwrap_or_default());
                // MIDI time follows the new output's position.
                if let Some(port) = self.app.platform.midi_port.clone() {
                    self.switch_midi(Some(port));
                }
            }
            Err(e) => {
                self.app.platform.audio_device = None;
                self.app.engine().say_error_with(
                    "Could not open that audio interface: {error}",
                    vec![MessageArg::plain("error", e)],
                );
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
    /// Empty or `en` for English, `auto` for the system language, or a language file's code.
    language: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AudioPrefs {
    /// Output device id; empty for the system default.
    device: String,
    /// MIDI output that MIDI time goes to; empty for none.
    midi_output: String,
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
    // English unless the person picked another language.
    platform.language = if language.is_empty() { "en".to_owned() } else { language };
    actcraft_ui::set_language(&platform.language);

    // The interface chosen last time, or the system default when it's gone.
    let saved = Some(prefs.get().audio.device.clone()).filter(|d| !d.is_empty());
    let opened = match AudioOutput::open(saved.as_deref()) {
        Ok(ok) => Ok((ok, saved)),
        Err(_) if saved.is_some() => AudioOutput::open(None).map(|ok| (ok, None)),
        Err(e) => Err(e),
    };
    let mut status = None;
    let audio = match opened {
        Ok(((out, controller), device_id)) => {
            platform.audio_device = Some(out.device_name.clone());
            platform.sample_rate = out.sample_rate;
            platform.output_channels = out.channels;
            platform.device_id = device_id;
            status = Some(controller.status_reader());
            engine.attach_audio(controller);
            Some(out)
        }
        Err(e) => {
            engine.say_error_with("No audio output: {error}", vec![MessageArg::plain("error", e)]);
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
                en.say_error_with("Remote control is off: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    // `--open-settings [TAB]`: the Settings window, on a tab by name (`midi`, `audio`...).
    let open_settings = args.iter().position(|a| a == "--open-settings").map(|i| {
        args.get(i + 1).filter(|t| !t.starts_with("--") && !t.ends_with(".actcraft")).cloned().unwrap_or_default()
    });
    let show_lyrics = args.iter().any(|a| a == "--lyrics");
    let edit_lyrics = args.iter().any(|a| a == "--edit-lyrics");
    let stage = args.iter().any(|a| a == "--stage");
    let cues = args.iter().any(|a| a == "--cues");
    let sheet = args.iter().any(|a| a == "--sheet");
    let about = args.iter().any(|a| a == "--about");
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
            if let Some(tab) = &open_settings {
                app.open_settings_at(tab);
            }
            if show_lyrics {
                app.show_lyrics();
            }
            if sheet {
                app.show_sheet();
            }
            if edit_lyrics {
                app.edit_lyrics();
            }
            // `--stage` opens the second window; with `--sheet` it shows the sheet music.
            if stage && sheet {
                app.show_stage_sheet();
            } else if stage {
                app.toggle_stage_screen();
            }
            if cues {
                app.show_cues();
            }
            if about {
                app.show_about();
            }
            let mut host = Host { app, _audio: audio, screenshot, dialogs, prefs, midi: None, status, midi_part: None };
            // The MIDI output chosen last time, if it is still there.
            host.app.platform.midi_ports = actcraft_platform::midi_output_ports();
            let saved = host.prefs.get().audio.midi_output.clone();
            if !saved.is_empty() && host.app.platform.midi_ports.contains(&saved) {
                host.switch_midi(Some(saved));
            }
            Ok(Box::new(host))
        }),
    );
    if let Err(e) = result {
        eprintln!("ActCraft could not open its window: {e}");
        std::process::exit(1);
    }
}
