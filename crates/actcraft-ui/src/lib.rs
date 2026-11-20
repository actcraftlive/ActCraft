//! The ActCraft window.
//!
//! The UI is thin: it reads engine state to draw and changes the engine only by running commands
//! by id (the same commands the CLI, the control channel and MCP use). Menus and keyboard
//! shortcuts are built from the command registry; colours and sizes come from theme tokens.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::sync::{Arc, Mutex, MutexGuard};

use actcraft_engine::{Engine, registry};
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};

pub mod shortcuts;
pub mod theme;

use theme::Tokens;

/// Translation hook. Every visible string goes through it with English as the key; the catalogs
/// (craft-i18n, English and German) are wired in a later milestone, until then English shows.
pub fn tr(s: &str) -> &str {
    s
}

/// What the big clock shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMode {
    Elapsed,
    Remaining,
    QueueEnd,
    TimeOfDay,
}

impl ClockMode {
    fn next(self) -> Self {
        match self {
            Self::Elapsed => Self::Remaining,
            Self::Remaining => Self::QueueEnd,
            Self::QueueEnd => Self::TimeOfDay,
            Self::TimeOfDay => Self::Elapsed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Elapsed => "Elapsed",
            Self::Remaining => "Remaining",
            Self::QueueEnd => "Queue ends in",
            Self::TimeOfDay => "Time",
        }
    }
}

/// A command waiting for params the person types (menu items whose command takes params).
struct Pending {
    id: &'static str,
    label: &'static str,
    params_doc: &'static str,
    path: String,
    json: String,
    error: Option<String>,
}

/// Facts about the platform for the status line.
#[derive(Debug, Clone, Default)]
pub struct PlatformInfo {
    pub audio_device: Option<String>,
    pub sample_rate: u32,
    pub control_address: Option<String>,
    /// Local time as text, refreshed by the host each frame (the UI has no clock of its own).
    pub time_of_day: String,
    /// The ActCraft logo as RGBA pixels (width, height, pixels), when the host has it.
    pub logo: Option<(usize, usize, Vec<u8>)>,
}

pub struct ActCraftApp {
    engine: Arc<Mutex<Engine>>,
    tokens: Tokens,
    pub platform: PlatformInfo,
    search: String,
    focus_search: bool,
    selected_track: usize,
    clock_mode: ClockMode,
    zoom: f32,
    pending: Option<Pending>,
    last_announced: usize,
    new_playlist: String,
    logo: Option<egui::TextureHandle>,
}

impl ActCraftApp {
    pub fn new(ctx: &egui::Context, engine: Arc<Mutex<Engine>>, platform: PlatformInfo) -> Self {
        let tokens = theme::DARK;
        theme::apply(ctx, &tokens);
        Self {
            engine,
            tokens,
            platform,
            search: String::new(),
            focus_search: false,
            selected_track: 0,
            clock_mode: ClockMode::Elapsed,
            zoom: 1.0,
            pending: None,
            last_announced: 0,
            new_playlist: String::new(),
            logo: None,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Engine> {
        match self.engine.lock() {
            Ok(g) => g,
            // A panic elsewhere left the lock poisoned; the engine data is still usable.
            Err(p) => p.into_inner(),
        }
    }

    /// Draws one frame. Call from the host's `App::ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let engine = Arc::clone(&self.engine);
        let mut guard = match engine.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let e: &mut Engine = &mut guard;
        e.tick();
        self.handle_shortcuts(ui.ctx(), e);
        let t = self.tokens;

        egui::Panel::top("menu")
            .frame(egui::Frame::new().fill(t.bg).inner_margin(4.0))
            .show(ui, |ui| self.menu_bar(ui, e));
        egui::Panel::top("transport")
            .frame(egui::Frame::new().fill(t.panel).inner_margin(t.pad))
            .show(ui, |ui| self.transport(ui, e));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.bg).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| self.status_line(ui, e));
        // The sidebar hides during playback so the stage view gets the room.
        if !e.playing() {
            egui::Panel::left("library")
                .default_size(t.sidebar_width)
                .frame(egui::Frame::new().fill(t.panel).inner_margin(t.pad))
                .show(ui, |ui| self.sidebar(ui, e));
        }
        egui::CentralPanel::default_margins().frame(egui::Frame::new().fill(t.bg).inner_margin(t.pad)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("centre").auto_shrink([false, false]).show(ui, |ui| self.centre(ui, e))
        });
        self.params_dialog(ui.ctx(), e);
        drop(guard);
        // Playback needs steady redraws for the clock and meters.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }

    fn run(&mut self, e: &mut Engine, id: &str, params: Value) {
        // Errors are already recorded as messages by the engine.
        let _ = e.run(id, &params);
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context, e: &mut Engine) {
        if ctx.text_edit_focused() {
            return;
        }
        for (shortcut, id) in shortcuts::table() {
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                if id == "library.search" {
                    self.focus_search = true;
                } else {
                    self.activate(e, id);
                }
            }
        }
    }

    /// Runs a command from a menu or shortcut: directly when it needs no params, otherwise
    /// through the params dialog.
    fn activate(&mut self, e: &mut Engine, id: &'static str) {
        let Some(c) = registry().get(id) else { return };
        let spec = c.spec;
        if spec.params == "none" || spec.params.starts_with("{discard") || spec.params.starts_with("{path?") {
            self.run(e, spec.id, Value::Null);
        } else {
            self.pending = Some(Pending {
                id: spec.id,
                label: spec.label,
                params_doc: spec.params,
                path: e.show_path.clone().unwrap_or_default(),
                json: "{}".to_owned(),
                error: None,
            });
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui, e: &mut Engine) {
        egui::MenuBar::new().ui(ui, |ui| {
            for top in shortcuts::menu_roots() {
                ui.menu_button(tr(top), |ui| {
                    for c in registry().iter().filter(|c| c.spec.menu.split('/').next() == Some(top)) {
                        let item = c.spec.menu.split('/').nth(1).unwrap_or(c.spec.label);
                        let enabled = e.enabled(c.spec.id);
                        let mut button = egui::Button::new(tr(item));
                        if let Some(s) = c.spec.shortcut {
                            button = button.shortcut_text(shortcuts::display(s));
                        }
                        if ui.add_enabled(enabled, button).clicked() {
                            self.activate(e, c.spec.id);
                            ui.close();
                        }
                    }
                });
            }
        });
    }

    fn params_dialog(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let Some(p) = &mut self.pending else { return };
        let mut open = true;
        let mut run = false;
        let wants_path = p.params_doc.starts_with("{path") || p.params_doc.starts_with("{paths");
        egui::Window::new(tr(p.label)).collapsible(false).open(&mut open).show(ctx, |ui| {
            if wants_path {
                ui.label(if p.params_doc.starts_with("{paths") {
                    tr("Audio files (one path per line)")
                } else {
                    tr("File path")
                });
                let edit = if p.params_doc.starts_with("{paths") {
                    egui::TextEdit::multiline(&mut p.path).desired_width(420.0)
                } else {
                    egui::TextEdit::singleline(&mut p.path).desired_width(420.0)
                };
                ui.add(edit);
            } else {
                ui.label(RichText::new(p.params_doc).small());
                ui.add(egui::TextEdit::multiline(&mut p.json).code_editor().desired_width(420.0));
            }
            if let Some(err) = &p.error {
                ui.colored_label(self.tokens.danger, err);
            }
            run = ui.button(tr("Run")).clicked();
        });
        if run {
            let params = if wants_path {
                if p.params_doc.starts_with("{paths") {
                    json!({"paths": p.path.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>()})
                } else {
                    json!({"path": p.path.trim()})
                }
            } else {
                match serde_json::from_str::<Value>(&p.json) {
                    Ok(v) => v,
                    Err(err) => {
                        p.error = Some(err.to_string());
                        return;
                    }
                }
            };
            match e.run(p.id, &params) {
                Ok(_) => self.pending = None,
                Err(err) => p.error = Some(err.to_string()),
            }
        } else if !open {
            self.pending = None;
        }
    }

    fn transport(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            let playing = e.playing();
            let play_label = if playing { tr("Stop") } else { tr("Play") };
            let fill = if playing { t.danger } else { t.playing };
            let play = egui::Button::new(RichText::new(play_label).size(18.0).color(t.accent_text))
                .fill(fill)
                .min_size(vec2(96.0, 44.0));
            if ui.add_enabled(e.enabled("transport.toggle") || playing, play).clicked() {
                self.run(e, "transport.toggle", Value::Null);
            }
            if ui.add(egui::Button::new(tr("Previous")).min_size(vec2(0.0, 44.0))).clicked() {
                self.run(e, "transport.previous_song", Value::Null);
            }
            if ui.add(egui::Button::new(tr("Next")).min_size(vec2(0.0, 44.0))).clicked() {
                self.run(e, "transport.next_song", Value::Null);
            }
            ui.separator();

            // Song and clock.
            ui.vertical(|ui| {
                let title = e
                    .current()
                    .map_or_else(|| tr("No song loaded").to_owned(), |s| format!("{} — {}", s.title, s.artist));
                ui.label(RichText::new(title).size(18.0).strong());
                let clock = self.clock_text(e);
                let r = ui.add(
                    egui::Label::new(RichText::new(clock).size(t.big_text_size).monospace()).sense(Sense::click()),
                );
                if r.on_hover_text(format!("{} ({})", tr(self.clock_mode.label()), tr("click to change"))).clicked() {
                    self.clock_mode = self.clock_mode.next();
                }
            });
            ui.separator();

            // Beat display: bar.beat with a flash on each beat (text too, never colour alone).
            let s = e.status;
            let phase = s.beat - s.beat.floor();
            let flash = s.playing && phase < 0.15 || (!s.playing && e.show.metronome.enabled && phase < 0.15);
            let beat_text = format!("{}.{}", s.bar + 1, s.beat_in_bar + 1);
            let beat_fill = if flash { if s.beat_in_bar == 0 { t.warning } else { t.accent } } else { t.surface };
            egui::Frame::new().fill(beat_fill).corner_radius(t.radius).inner_margin(t.pad).show(ui, |ui| {
                ui.label(RichText::new(beat_text).size(22.0).monospace().color(if flash {
                    t.accent_text
                } else {
                    t.text
                }));
            });
            if s.counting_in {
                ui.label(RichText::new(tr("Count-in")).color(t.warning).strong());
            }
            ui.separator();
            ui.vertical(|ui| {
                let m = &e.show.metronome;
                let on = m.enabled;
                let label = format!("{}: {}", tr("Metronome"), if on { tr("On") } else { tr("Off") });
                if ui.selectable_label(on, label).clicked() {
                    self.run(e, "metronome.toggle", Value::Null);
                }
                if ui.button(format!("{}: {}", tr("Mode"), tr(e.show.metronome.mode.label()))).clicked() {
                    self.run(e, "metronome.cycle_mode", Value::Null);
                }
            });
            if let Some(song) = e.current() {
                let (bpm, id) = (song.bpm, song.id);
                ui.vertical(|ui| {
                    let mut b = bpm;
                    let r = ui.add(egui::DragValue::new(&mut b).range(20.0..=400.0).speed(0.5).suffix(" BPM"));
                    if r.changed() {
                        self.run(e, "song.set", json!({"song": id, "bpm": b}));
                    }
                    ui.label(RichText::new(format!("{} {}", tr("Queue:"), e.queue.len())).color(t.text_dim));
                });
            }
        });
    }

    fn clock_text(&self, e: &Engine) -> String {
        let s = e.status;
        let dur = e.current().map_or(0.0, |x| x.end.unwrap_or(x.duration));
        match self.clock_mode {
            ClockMode::Elapsed => fmt_time(s.time.max(0.0)),
            ClockMode::Remaining => format!("-{}", fmt_time((dur - s.time).max(0.0))),
            ClockMode::QueueEnd => {
                let queued: f64 = e.queue.iter().filter_map(|id| e.show.song(*id)).map(|x| x.duration).sum();
                fmt_time(queued + (dur - s.time).max(0.0))
            }
            ClockMode::TimeOfDay => self.platform.time_of_day.clone(),
        }
    }

    fn status_line(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            let audio = match &self.platform.audio_device {
                Some(name) => format!("{} {} · {} Hz", tr("Audio:"), name, self.platform.sample_rate),
                None => tr("Audio: no output").to_owned(),
            };
            ui.label(RichText::new(audio).color(if self.platform.audio_device.is_some() {
                t.text_dim
            } else {
                t.warning
            }));
            if let Some(addr) = &self.platform.control_address {
                ui.label(RichText::new(format!("{} {addr}", tr("Remote control:"))).color(t.text_dim));
            }
            if e.dirty {
                ui.label(RichText::new(tr("Unsaved changes")).color(t.warning));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(m) = e.messages.back() {
                    let colour = if m.error { t.danger } else { t.text };
                    let label = ui.label(RichText::new(&m.text).color(colour));
                    // New messages are announced to screen readers.
                    if e.messages.len() != self.last_announced {
                        self.last_announced = e.messages.len();
                        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, m.text.clone()));
                    }
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.heading(tr("Library"));
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(tr("Search: title:, artist:, -word, OR"))
                .desired_width(f32::INFINITY),
        );
        if self.focus_search {
            search.request_focus();
            self.focus_search = false;
        }
        if search.changed() {
            let q = self.search.clone();
            self.run(e, "library.search", json!({"query": q}));
        }
        let ids: Vec<u64> = if self.search.trim().is_empty() {
            e.show.songs.iter().map(|s| s.id).collect()
        } else {
            e.search_results.clone()
        };
        egui::ScrollArea::vertical()
            .id_salt("songs")
            .max_height(ui.available_height() * 0.45)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if ids.is_empty() {
                    ui.label(
                        RichText::new(tr("No songs yet. Use Song > Add Songs from Audio Files.")).color(t.text_dim),
                    );
                }
                for id in ids {
                    let Some(s) = e.show.song(id) else { continue };
                    let current = e.current_song == Some(id);
                    let text = format!("{}  ·  {}  ·  {:.0} BPM", s.title, s.artist, s.bpm);
                    let r = ui.selectable_label(current, text);
                    if r.double_clicked() {
                        self.run(e, "queue.double_tap", json!({"song": id}));
                    } else if r.clicked() && !e.playing() {
                        self.run(e, "song.load", json!({"song": id}));
                    }
                    r.context_menu(|ui| {
                        if ui.button(tr("Play next")).clicked() {
                            let _ = e.run("queue.play_next", &json!({"song": id}));
                            ui.close();
                        }
                        if ui.button(tr("Add to queue")).clicked() {
                            let _ = e.run("queue.add", &json!({"song": id}));
                            ui.close();
                        }
                        for (i, pl) in e.show.playlists.clone().iter().enumerate() {
                            if pl.smart.is_none() && ui.button(format!("{} {}", tr("Add to"), pl.name)).clicked() {
                                let _ = e.run("playlist.add_songs", &json!({"playlist": i, "songs": [id]}));
                                ui.close();
                            }
                        }
                    });
                }
            });
        ui.separator();
        ui.heading(tr("Playlists"));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_playlist)
                    .hint_text(tr("New playlist name"))
                    .desired_width(170.0),
            );
            if ui.add_enabled(!self.new_playlist.trim().is_empty(), egui::Button::new(tr("Create"))).clicked() {
                let name = self.new_playlist.trim().to_owned();
                self.run(e, "playlist.create", json!({"name": name}));
                self.new_playlist.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("playlists")
            .max_height(ui.available_height() * 0.6)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let playlists = e.show.playlists.clone();
                for (i, pl) in playlists.iter().enumerate() {
                    let selected = e.current_playlist == Some(i);
                    egui::CollapsingHeader::new(format!(
                        "{}{}",
                        pl.name,
                        if pl.smart.is_some() { "  (smart)" } else { "" }
                    ))
                    .id_salt(("pl", i))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(selected, tr("Play from here")).clicked() {
                                self.run(e, "playlist.select", json!({"playlist": i}));
                            }
                            if ui.button(tr("Queue all")).clicked() {
                                self.run(e, "queue.from_playlist", json!({"playlist": i}));
                            }
                        });
                        let songs = actcraft_engine::commands::playlist_songs(&e.show, pl);
                        if pl.smart.is_some() {
                            for id in songs {
                                if let Some(s) = e.show.song(id) {
                                    ui.label(&s.title);
                                }
                            }
                        } else {
                            for (si, set) in pl.sets.iter().enumerate() {
                                ui.label(RichText::new(&set.name).strong().color(t.text_dim));
                                for (k, id) in set.songs.iter().enumerate() {
                                    let title = e.show.song(*id).map_or("?", |s| s.title.as_str()).to_owned();
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{}. {title}", k + 1));
                                        // Move buttons: reordering works without dragging.
                                        if k > 0 && ui.small_button("↑").on_hover_text(tr("Move up")).clicked() {
                                            self.run(
                                                e,
                                                "playlist.move_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k], "to": k - 1}),
                                            );
                                        }
                                        if k + 1 < set.songs.len()
                                            && ui.small_button("↓").on_hover_text(tr("Move down")).clicked()
                                        {
                                            self.run(
                                                e,
                                                "playlist.move_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k], "to": k + 2}),
                                            );
                                        }
                                        if ui.small_button("✕").on_hover_text(tr("Remove from set")).clicked() {
                                            self.run(
                                                e,
                                                "playlist.remove_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k]}),
                                            );
                                        }
                                    });
                                }
                            }
                            if ui.small_button(tr("Add set")).clicked() {
                                self.run(e, "playlist.add_set", json!({"playlist": i}));
                            }
                        }
                    });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading(tr("Queue"));
            if ui.small_button(tr("Clear")).clicked() {
                self.run(e, "queue.clear", Value::Null);
            }
        });
        let queued: Vec<u64> = e.queue.iter().copied().collect();
        for (k, id) in queued.iter().enumerate() {
            let title = e.show.song(*id).map_or("?", |s| s.title.as_str()).to_owned();
            ui.horizontal(|ui| {
                ui.label(format!("{}. {title}", k + 1));
                if ui.small_button("✕").on_hover_text(tr("Remove from queue")).clicked() {
                    self.run(e, "queue.remove", json!({"index": k}));
                }
            });
        }
    }

    fn centre(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let Some(song) = e.current().cloned() else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                if self.logo.is_none()
                    && let Some((w, h, rgba)) = &self.platform.logo
                {
                    let image = egui::ColorImage::from_rgba_unmultiplied([*w, *h], rgba);
                    self.logo = Some(ui.ctx().load_texture("actcraft-logo", image, egui::TextureOptions::LINEAR));
                }
                if let Some(tex) = &self.logo {
                    ui.add(
                        egui::Image::new((tex.id(), vec2(220.0, 220.0))).alt_text(tr("ActCraft logo: a drawn octopus")),
                    );
                }
                ui.heading(tr("Welcome to ActCraft"));
                ui.label(
                    RichText::new(tr(
                        "Add songs from audio files (Song menu), build a playlist, then press Play or Space.",
                    ))
                    .color(t.text_dim),
                );
                if ui.button(tr("Add a song")).clicked() {
                    self.run(e, "song.add", Value::Null);
                }
            });
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(tr("Mixer"));
            if ui.button(tr("Add track")).clicked() {
                self.run(e, "track.add", json!({"song": song.id}));
            }
            ui.label(RichText::new(format!("{} {}", song.tracks.len(), tr("tracks"))).color(t.text_dim));
        });
        egui::ScrollArea::horizontal().id_salt("strips").max_height(330.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, tr_) in song.tracks.iter().enumerate() {
                    self.track_strip(ui, e, song.id, i, tr_);
                }
                ui.separator();
                self.master_strip(ui, e, &song);
            });
        });
        ui.separator();
        // Mute groups.
        ui.horizontal(|ui| {
            ui.label(tr("Mute groups"));
            for (g, group) in e.show.mute_groups.clone().iter().enumerate() {
                let text = format!("{}{}", group.label, if group.active { " (muted)" } else { "" });
                let b = egui::Button::new(text).selected(group.active);
                if ui.add(b).clicked() {
                    self.run(e, "mute_group.toggle", json!({"group": g + 1}));
                }
            }
        });
        ui.separator();
        self.waveform(ui, e, &song);
    }

    fn track_strip(&mut self, ui: &mut Ui, e: &mut Engine, song: u64, i: usize, track: &actcraft_show::Track) {
        let t = self.tokens;
        let selected = self.selected_track == i;
        let stroke = if selected { Stroke::new(2.0, t.focus) } else { Stroke::new(1.0, t.stroke) };
        egui::Frame::new().fill(t.surface).stroke(stroke).corner_radius(t.radius).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(t.strip_width);
            ui.vertical(|ui| {
                let colour = theme::colour(&track.colour, t.accent);
                let (rect, _) = ui.allocate_exact_size(vec2(t.strip_width, 4.0), Sense::hover());
                ui.painter().rect_filled(rect, 2, colour);
                let mut name = track.name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(t.strip_width));
                if r.lost_focus() && name != track.name {
                    self.run(e, "track.set", json!({"song": song, "track": i, "name": name}));
                }
                if r.clicked() || r.gained_focus() {
                    self.selected_track = i;
                }
                let mut vol = track.volume_db;
                let v = ui.add(egui::Slider::new(&mut vol, -60.0..=12.0).vertical().suffix(" dB").text(tr("Volume")));
                if v.changed() {
                    self.run(e, "track.set", json!({"song": song, "track": i, "volume_db": vol}));
                }
                let mut pan = track.pan;
                ui.spacing_mut().slider_width = t.strip_width - 36.0;
                if ui.add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false).text(tr("Pan"))).changed() {
                    self.run(e, "track.set", json!({"song": song, "track": i, "pan": pan}));
                }
                ui.horizontal(|ui| {
                    let mute = egui::Button::new(if track.mute { "M ●" } else { "M" }).selected(track.mute);
                    if ui.add(mute).on_hover_text(tr("Mute")).clicked() {
                        self.run(e, "track.set", json!({"song": song, "track": i, "mute": !track.mute}));
                    }
                    if ui.small_button(tr("Select")).clicked() {
                        self.selected_track = i;
                    }
                });
                let bus_name = e
                    .show
                    .buses
                    .get(usize::from(track.bus))
                    .map_or_else(|| format!("Bus {}", track.bus + 1), |b| b.name.clone());
                egui::ComboBox::from_id_salt(("bus", song, i))
                    .width(t.strip_width - 8.0)
                    .selected_text(bus_name)
                    .show_ui(ui, |ui| {
                        for (b, bus) in e.show.buses.clone().iter().enumerate() {
                            if ui.selectable_label(usize::from(track.bus) == b, &bus.name).clicked() {
                                self.run(e, "track.set", json!({"song": song, "track": i, "bus": b}));
                            }
                        }
                    });
                ui.horizontal_wrapped(|ui| {
                    for g in 0..6u8 {
                        let member = track.mute_groups & (1 << g) != 0;
                        if ui.selectable_label(member, format!("{}", g + 1)).on_hover_text(tr("Mute group")).clicked() {
                            self.run(
                                e,
                                "track.mute_group",
                                json!({"song": song, "track": i, "group": g + 1, "member": !member}),
                            );
                        }
                    }
                });
            });
        });
    }

    fn master_strip(&mut self, ui: &mut Ui, e: &mut Engine, song: &actcraft_show::Song) {
        let t = self.tokens;
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.stroke))
            .corner_radius(t.radius)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.set_width(t.strip_width * 1.4);
                ui.vertical(|ui| {
                    ui.label(RichText::new(tr("Song")).strong());
                    let mut sv = song.volume_db;
                    if ui
                        .add(egui::Slider::new(&mut sv, -60.0..=12.0).vertical().suffix(" dB").text(tr("Song volume")))
                        .changed()
                    {
                        self.run(e, "mixer.song_volume", json!({"song": song.id, "db": sv}));
                    }
                    ui.label(RichText::new(tr("Master")).strong());
                    let mut mv = e.show.mixer.master_volume_db;
                    if ui
                        .add(
                            egui::Slider::new(&mut mv, -60.0..=12.0).vertical().suffix(" dB").text(tr("Master volume")),
                        )
                        .changed()
                    {
                        self.run(e, "mixer.master_volume", json!({"db": mv}));
                    }
                    // Peak meter of the main bus.
                    let peak = e.status.bus_peaks.first().copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(t.strip_width, 8.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2, t.bg);
                    let mut fill = rect;
                    fill.set_width(rect.width() * peak);
                    ui.painter().rect_filled(fill, 2, if peak > 0.95 { t.danger } else { t.playing });
                });
            });
    }

    fn waveform(&mut self, ui: &mut Ui, e: &mut Engine, song: &actcraft_show::Song) {
        let t = self.tokens;
        let Some(track) = song.tracks.get(self.selected_track).or_else(|| song.tracks.first()) else {
            ui.label(RichText::new(tr("This song has no tracks yet.")).color(t.text_dim));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", tr("Waveform:"), track.name));
            ui.add(egui::Slider::new(&mut self.zoom, 1.0..=64.0).logarithmic(true).text(tr("Zoom")));
        });
        if track.file.is_empty() {
            ui.label(RichText::new(tr("No audio file on this track.")).color(t.text_dim));
            return;
        }
        let peaks = match e.cached_peaks(&track.file) {
            Some(p) => p,
            None => match e.peaks(&track.file) {
                Ok(p) => p,
                Err(err) => {
                    ui.colored_label(t.danger, err);
                    return;
                }
            },
        };
        let width = ui.available_width().max(100.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, 120.0), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, t.radius, t.surface);
        let total = peaks.pairs.len() as f64 * actcraft_audio_bucket() / f64::from(peaks.sample_rate.max(1));
        let span = (total / f64::from(self.zoom)).max(0.1);
        let now = e.status.time.max(0.0);
        let from = (now - span * 0.25).clamp(0.0, (total - span).max(0.0));
        let cols = width as usize;
        let mid = rect.center().y;
        let h = rect.height() * 0.48;
        let colour = theme::colour(&track.colour, t.accent);
        for (c, (lo, hi)) in peaks.window(from, from + span, cols).iter().enumerate() {
            let x = rect.left() + c as f32;
            painter.line_segment([egui::pos2(x, mid - hi * h), egui::pos2(x, mid - lo * h)], Stroke::new(1.0, colour));
        }
        let px = rect.left() + ((now - from) / span * f64::from(width)) as f32;
        painter.line_segment([egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())], Stroke::new(2.0, t.warning));
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let secs = from + f64::from((pos.x - rect.left()) / width) * span;
            self.run(e, "transport.seek", json!({"seconds": secs}));
        }
        response.on_hover_text(tr("Click to jump to that point"));
        let _ = Color32::TRANSPARENT;
    }

    /// The engine, for hosts that need it (screenshots, tests).
    pub fn engine(&self) -> MutexGuard<'_, Engine> {
        self.lock()
    }
}

fn actcraft_audio_bucket() -> f64 {
    256.0
}

/// `m:ss` (or `h:mm:ss`).
pub fn fmt_time(secs: f64) -> String {
    let s = if secs.is_finite() { secs.max(0.0) as u64 } else { 0 };
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn time_format() {
        assert_eq!(super::fmt_time(65.4), "1:05");
        assert_eq!(super::fmt_time(f64::NAN), "0:00");
        assert_eq!(super::fmt_time(3725.0), "1:02:05");
    }
}
