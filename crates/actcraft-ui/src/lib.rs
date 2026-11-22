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

mod cues_view;
pub mod flows;
pub mod i18n;
mod lyrics_view;
mod notation_view;
mod regions_view;
mod settings;
mod sheet_view;
pub mod shortcuts;
pub mod theme;

pub use flows::{FileAsk, FileKind, FileRequest};

use theme::Tokens;

/// Translation hook: every visible string goes through it with English as the key (see `i18n`).
pub use i18n::{set_language, tr};

/// An engine message in the window's language: its template translated, values filled in (a
/// value that is window text, such as a mode's name, translated too).
pub fn message_text(m: &actcraft_engine::Message) -> String {
    let values: Vec<(&str, &str)> =
        m.args.iter().map(|a| (a.name, if a.translate { tr(&a.value) } else { a.value.as_str() })).collect();
    actcraft_engine::fill(tr(&m.template), &values)
}

/// A song being dragged inside a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DragSong {
    playlist: usize,
    set: usize,
    index: usize,
}

/// Where a song dragged from `from` lands when dropped on row `row` (above it, or below when
/// `below`), as `playlist.move_songs`'s `to`; `None` when it would stay where it is.
fn drop_position(from: usize, row: usize, below: bool) -> Option<usize> {
    let to = row + usize::from(below);
    (to != from && to != from + 1).then_some(to)
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
    /// Output devices (id, name) for the Settings window.
    pub devices: Vec<(String, String)>,
    /// The chosen device, `None` for the system default.
    pub device_id: Option<String>,
    /// Output channels of the running device.
    pub output_channels: u16,
    /// The window's language setting: `en` (the default), `auto` (the system's) or a language
    /// file's code.
    pub language: String,
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
    /// Clicking the waveform jumps to the nearest beat.
    snap_to_beat: bool,
    last_announced: usize,
    new_playlist: String,
    logo: Option<egui::TextureHandle>,
    file_requests: Vec<FileRequest>,
    modal: Option<flows::Modal>,
    /// Set once the person agreed to close (changes saved or discarded).
    allow_close: bool,
    settings_open: bool,
    settings_tab: settings::Tab,
    /// A device the person picked in Settings; the host switches to it.
    device_request: Option<Option<String>>,
    /// A language the person picked in Settings; the host remembers it.
    language_request: Option<String>,
    /// Tap-tempo times (seconds of app time).
    taps: Vec<f64>,
    centre_tab: CentreTab,
    lyrics_edit: Option<lyrics_view::LyricsEdit>,
    lyrics_show_chords: bool,
    lyrics_follow: bool,
    lyrics_size: f32,
    lyrics_follow_state: lyrics_view::LyricsFollow,
    /// The second lyrics window, while open.
    stage: Option<lyrics_view::StageScreen>,
    /// The open PDF of the Sheet music tab.
    sheet: Option<sheet_view::SheetDoc>,
    sheet_view: sheet_view::SheetLook,
    /// Marking on the PDF: tool, colour, the mark being drawn.
    mark_edit: sheet_view::MarkEdit,
    /// The note being written for a spot on the PDF: song, page, spot, words.
    note_prompt: Option<(u64, u32, [f32; 2], String)>,
    /// "Remove all marks" waits for a second click.
    confirm_clear_marks: bool,
    /// The first PDF page in view (where a note added without a pointer goes).
    sheet_top_page: usize,
    /// Notation or PDF, when a song has both.
    sheet_source: sheet_view::SheetSource,
    /// The open score of the Sheet music tab.
    notation: Option<notation_view::NotationDoc>,
    notation_look: notation_view::NotationLook,
    /// Words typed for a new spoken cue in the Spoken cues tab.
    new_cue: String,
}

/// What the main area shows for the current song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CentreTab {
    Mixer,
    Lyrics,
    Sheet,
    Cues,
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
            snap_to_beat: false,
            last_announced: 0,
            new_playlist: String::new(),
            logo: None,
            file_requests: Vec::new(),
            modal: None,
            allow_close: false,
            settings_open: false,
            settings_tab: settings::Tab::default(),
            device_request: None,
            language_request: None,
            taps: Vec::new(),
            centre_tab: CentreTab::Mixer,
            lyrics_edit: None,
            lyrics_show_chords: true,
            lyrics_follow: true,
            lyrics_size: 28.0,
            lyrics_follow_state: lyrics_view::LyricsFollow::default(),
            stage: None,
            sheet: None,
            sheet_view: sheet_view::SheetLook::default(),
            mark_edit: sheet_view::MarkEdit::default(),
            note_prompt: None,
            confirm_clear_marks: false,
            sheet_top_page: 0,
            sheet_source: sheet_view::SheetSource::default(),
            notation: None,
            notation_look: notation_view::NotationLook::default(),
            new_cue: String::new(),
        }
    }

    /// Shows the Sheet music tab (also used by `--sheet` for screenshots).
    pub fn show_sheet(&mut self) {
        self.centre_tab = CentreTab::Sheet;
    }

    /// Shows the Lyrics tab (also used by `--lyrics` for screenshots).
    pub fn show_lyrics(&mut self) {
        self.centre_tab = CentreTab::Lyrics;
    }

    /// Opens the lyrics editor for the current song (`--edit-lyrics`, for screenshots).
    pub fn edit_lyrics(&mut self) {
        self.centre_tab = CentreTab::Lyrics;
        let e = self.lock();
        let current = e.current().map(|s| (s.id, s.lyrics.clone()));
        drop(e);
        if let Some((song, text)) = current {
            self.lyrics_edit = Some(lyrics_view::LyricsEdit { song, text, ..Default::default() });
        }
    }

    /// Opens the Settings window (also used by `--open-settings` for screenshots).
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    /// Shows the About window (`--about`, for screenshots).
    pub fn show_about(&mut self) {
        self.modal = Some(flows::Modal::About);
    }

    /// Shows the Spoken cues tab (`--cues`, for screenshots).
    pub fn show_cues(&mut self) {
        self.centre_tab = CentreTab::Cues;
    }

    /// A device switch the person asked for (`Some(None)`: the system default); the host calls
    /// this every frame.
    pub fn take_device_request(&mut self) -> Option<Option<String>> {
        self.device_request.take()
    }

    /// A language the person picked (already applied to the window); the host saves it.
    pub fn take_language_request(&mut self) -> Option<String> {
        self.language_request.take()
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
        let ctx = ui.ctx().clone();
        self.handle_close(&ctx, e);
        self.handle_drops(&ctx, e);
        if self.modal.is_none() {
            self.handle_shortcuts(&ctx, e);
            if !ctx.text_edit_focused()
                && ctx.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma))
                })
            {
                self.settings_open = true;
            }
        }
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
        self.settings_window(&ctx, e);
        self.stage_window(&ctx, e);
        self.modal_ui(&ctx, e);
        self.drop_hint(&ctx);
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

    fn menu_bar(&mut self, ui: &mut Ui, e: &mut Engine) {
        egui::MenuBar::new().ui(ui, |ui| {
            for top in shortcuts::menu_roots() {
                ui.menu_button(tr(top), |ui| {
                    for c in registry()
                        .iter()
                        .filter(|c| c.spec.menu.split('/').next() == Some(top) && flows::offered_in_menu(c.spec.id))
                    {
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
                    if top == "File" {
                        ui.separator();
                        let b = egui::Button::new(tr("Settings\u{2026}")).shortcut_text(shortcuts::display("Ctrl+,"));
                        if ui.add(b).clicked() {
                            self.settings_open = true;
                            ui.close();
                        }
                    }
                });
            }
        });
    }

    fn transport(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            let playing = e.playing();
            // Play / Pause in one button (Space); Stop also goes back to the song's start (Esc).
            let play_label = if playing { tr("Pause") } else { tr("Play") };
            let fill = if playing { t.warning } else { t.playing };
            let play = egui::Button::new(RichText::new(play_label).size(18.0).color(t.accent_text))
                .fill(fill)
                .min_size(vec2(96.0, 44.0));
            let hint =
                if playing { tr("Pause here; Play goes on from this point (Space)") } else { tr("Play (Space)") };
            if ui.add_enabled(e.enabled("transport.toggle") || playing, play).on_hover_text(hint).clicked() {
                self.run(e, "transport.toggle", Value::Null);
            }
            let at_start = e.status.time <= e.current().map_or(0.0, |s| s.start.max(0.0)) + 0.01;
            let stop = egui::Button::new(RichText::new(tr("Stop")).size(18.0).color(t.accent_text))
                .fill(t.danger)
                .min_size(vec2(80.0, 44.0));
            if ui
                .add_enabled(e.has_audio() && (playing || !at_start), stop)
                .on_hover_text(tr("Stop and go back to the start of the song (Esc)"))
                .clicked()
            {
                self.run(e, "transport.stop", Value::Null);
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
            let beat_text = if s.playing || e.show.metronome.enabled {
                format!("{}.{}", s.bar + 1, s.beat_in_bar + 1)
            } else {
                "\u{2013}.\u{2013}".to_owned()
            };
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
                let mode = e.current().map_or_else(|| tr(e.show.metronome.mode.label()), |s| tr(s.click.label()));
                if ui
                    .button(format!("{}: {}", tr("Click"), mode))
                    .on_hover_text(tr("When this song's click sounds; click to change"))
                    .clicked()
                {
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(tr("Settings")).min_size(vec2(0.0, 44.0)))
                    .on_hover_text(tr("Audio interface, outputs, metronome, playback"))
                    .clicked()
                {
                    self.settings_open = true;
                }
                if e.loading_count() > 0 {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("{} {}", tr("Loading audio:"), e.loading_count())).color(t.text_dim),
                    );
                }
            });
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
                    let text = message_text(m);
                    let label = ui.label(RichText::new(&text).color(colour));
                    // New messages are announced to screen readers.
                    if e.messages.len() != self.last_announced {
                        self.last_announced = e.messages.len();
                        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text.clone()));
                    }
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            ui.heading(tr("Library"));
            if ui.button(tr("Add songs\u{2026}")).on_hover_text(tr("Choose audio files; each becomes a song")).clicked()
            {
                self.ask_add_songs(e);
            }
        });
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
                        RichText::new(tr("No songs yet. Click \u{201c}Add songs\u{2026}\u{201d} or drag audio files onto the window."))
                            .color(t.text_dim),
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
                        ui.separator();
                        if ui.button(tr("Delete song\u{2026}")).clicked() {
                            let title = e.show.song(id).map_or_else(String::new, |s| s.title.clone());
                            self.modal = Some(flows::Modal::Confirm {
                                title: tr("Delete song"),
                                text: format!("{} \u{201c}{title}\u{201d}? {}", tr("Delete"), tr("This removes it from every playlist.")),
                                button: tr("Delete"),
                                command: "song.remove",
                                params: json!({"song": id, "confirm": true}),
                            });
                            ui.close();
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
                        if pl.smart.is_some() { format!("  ({})", tr("smart")) } else { String::new() }
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
                                    let row = ui.horizontal(|ui| {
                                        // Drag the title to reorder; the buttons do the same without dragging.
                                        let payload = DragSong { playlist: i, set: si, index: k };
                                        ui.dnd_drag_source(egui::Id::unique(("set-song", i, si, k)), payload, |ui| {
                                            ui.label(format!("{}. {title}", k + 1))
                                        })
                                        .response
                                        .on_hover_text(tr("Drag to move it in the set"));
                                        // Move buttons: reordering works without dragging.
                                        if k > 0 && ui.small_button(tr("Up")).on_hover_text(tr("Move up")).clicked() {
                                            self.run(
                                                e,
                                                "playlist.move_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k], "to": k - 1}),
                                            );
                                        }
                                        if k + 1 < set.songs.len()
                                            && ui.small_button(tr("Down")).on_hover_text(tr("Move down")).clicked()
                                        {
                                            self.run(
                                                e,
                                                "playlist.move_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k], "to": k + 2}),
                                            );
                                        }
                                        if ui.small_button(tr("Remove")).on_hover_text(tr("Remove from set")).clicked()
                                        {
                                            self.run(
                                                e,
                                                "playlist.remove_songs",
                                                json!({"playlist": i, "set": si, "indexes": [k]}),
                                            );
                                        }
                                    });
                                    // A song dragged over this row goes above or below it.
                                    let rect = row.response.rect;
                                    let below = ui.ctx().pointer_hover_pos().is_some_and(|p| p.y > rect.center().y);
                                    if let Some(drag) = row.response.dnd_hover_payload::<DragSong>()
                                        && drag.playlist == i
                                        && drag.set == si
                                    {
                                        let y = if below { rect.bottom() } else { rect.top() };
                                        ui.painter().hline(rect.x_range(), y, Stroke::new(2.0, t.accent));
                                    }
                                    if let Some(drag) = row.response.dnd_release_payload::<DragSong>()
                                        && drag.playlist == i
                                        && drag.set == si
                                        && let Some(to) = drop_position(drag.index, k, below)
                                    {
                                        self.run(
                                            e,
                                            "playlist.move_songs",
                                            json!({"playlist": i, "set": si, "indexes": [drag.index], "to": to}),
                                        );
                                    }
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
                if ui.small_button(tr("Remove")).on_hover_text(tr("Remove from queue")).clicked() {
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
                        "Add your backing tracks, or drag audio files onto this window. Then press Play or Space.",
                    ))
                    .color(t.text_dim),
                );
                ui.add_space(12.0);
                ui.scope(|ui| {
                    ui.set_max_width(380.0);
                    self.welcome_buttons(ui, e);
                });
            });
            return;
        };
        self.song_details(ui, e, &song);
        ui.separator();
        ui.horizontal(|ui| {
            let big = |text: &str| RichText::new(text).size(17.0);
            ui.selectable_value(&mut self.centre_tab, CentreTab::Mixer, big(tr("Mixer")));
            let lyrics_label = if song.lyrics.trim().is_empty() { tr("Lyrics") } else { tr("Lyrics \u{2022}") };
            ui.selectable_value(&mut self.centre_tab, CentreTab::Lyrics, big(lyrics_label));
            let sheet_label = if song.pdf_file.is_some() || song.notation_file.is_some() {
                tr("Sheet music \u{2022}")
            } else {
                tr("Sheet music")
            };
            ui.selectable_value(&mut self.centre_tab, CentreTab::Sheet, big(sheet_label));
            let spoken = song.announce_title || song.regions.iter().any(|r| !r.voice.trim().is_empty());
            let cues_label = if spoken { tr("Spoken cues \u{2022}") } else { tr("Spoken cues") };
            ui.selectable_value(&mut self.centre_tab, CentreTab::Cues, big(cues_label));
        });
        self.region_bar(ui, e, &song);
        if self.centre_tab == CentreTab::Lyrics {
            self.lyrics_tab(ui, e, &song);
            return;
        }
        if self.centre_tab == CentreTab::Cues {
            if self.lyrics_edit.is_some() {
                self.finish_lyrics_edit_pub(e);
            }
            self.cues_tab(ui, e, &song);
            return;
        }
        if self.centre_tab == CentreTab::Sheet {
            if self.lyrics_edit.is_some() {
                self.finish_lyrics_edit_pub(e);
            }
            self.sheet_tab(ui, e, &song);
            return;
        }
        // Leaving the Lyrics tab keeps what was typed.
        if self.lyrics_edit.is_some() {
            self.finish_lyrics_edit_pub(e);
        }
        ui.horizontal(|ui| {
            ui.heading(tr("Mixer"));
            if ui
                .button(tr("Add tracks\u{2026}"))
                .on_hover_text(tr("Choose audio files; each becomes a track"))
                .clicked()
            {
                self.ask_track_file(e, FileAsk::AddTrack { song: song.id });
            }
            ui.label(RichText::new(format!("{} {}", song.tracks.len(), tr("tracks"))).color(t.text_dim));
        });
        if song.tracks.is_empty() {
            ui.label(
                RichText::new(tr(
                    "This song has no tracks yet. Use \u{201c}Add tracks\u{2026}\u{201d} or drop audio files here.",
                ))
                .color(t.text_dim),
            );
        }
        egui::ScrollArea::horizontal().id_salt("strips").max_height(460.0).show(ui, |ui| {
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
                let text = format!(
                    "{}{}",
                    group.label,
                    if group.active { format!(" ({})", tr("muted")) } else { String::new() }
                );
                let b = egui::Button::new(text).selected(group.active);
                if ui.add(b).clicked() {
                    self.run(e, "mute_group.toggle", json!({"group": g + 1}));
                }
            }
        });
        ui.separator();
        self.waveform(ui, e, &song);
        ui.separator();
        self.region_editor(ui, e, &song);
    }

    /// Song details, edited in place: name fields save when you leave them, numbers and switches
    /// at once.
    fn song_details(&mut self, ui: &mut Ui, e: &mut Engine, song: &actcraft_show::Song) {
        let id = song.id;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Title"));
            if let Some(v) = edit_text(ui, egui::Id::unique(("song-title", id)), &song.title, 220.0) {
                self.run(e, "song.set", json!({"song": id, "title": v}));
            }
            ui.label(tr("Artist"));
            if let Some(v) = edit_text(ui, egui::Id::unique(("song-artist", id)), &song.artist, 180.0) {
                self.run(e, "song.set", json!({"song": id, "artist": v}));
            }
            ui.label(tr("Key"));
            if let Some(v) = edit_text(ui, egui::Id::unique(("song-key", id)), &song.key, 60.0) {
                self.run(e, "song.set", json!({"song": id, "key": v}));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Tempo"));
            let mut bpm = song.bpm;
            if ui.add(egui::DragValue::new(&mut bpm).range(20.0..=400.0).speed(0.5).suffix(" BPM")).changed() {
                self.run(e, "song.set", json!({"song": id, "bpm": bpm}));
            }
            if ui.button(tr("Tap")).on_hover_text(tr("Click in time with the music to set the tempo")).clicked() {
                let now = ui.input(|i| i.time);
                if self.taps.last().is_some_and(|last| now - last > 3.0) {
                    self.taps.clear();
                }
                self.taps.push(now);
                if let Some(b) = actcraft_engine::tempo::from_taps(&self.taps) {
                    self.run(e, "song.set", json!({"song": id, "bpm": b}));
                }
            }
            if ui.button(tr("Detect")).on_hover_text(tr("Find tempo and first beat from the first track")).clicked() {
                self.run(e, "song.detect_bpm", json!({"song": id}));
            }
        });
        // Where the beat sits in the audio: nudge until the click lands on the music.
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("First beat at"));
            let mut anchor = song.beat_anchor;
            if ui
                .add(egui::DragValue::new(&mut anchor).range(-60.0..=600.0).speed(0.001).fixed_decimals(3).suffix(" s"))
                .on_hover_text(tr("Time of a beat in the audio; the click grid starts from here"))
                .changed()
            {
                self.run(e, "song.set", json!({"song": id, "beat_anchor": anchor}));
            }
            for (label, delta, hint) in
                [("\u{2212}10 ms", -0.010, tr("Click 10 ms earlier")), ("+10 ms", 0.010, tr("Click 10 ms later"))]
            {
                if ui.small_button(label).on_hover_text(hint).clicked() {
                    self.run(e, "song.set", json!({"song": id, "beat_anchor": song.beat_anchor + delta}));
                }
            }
            let beat_len = 60.0 / song.bpm.max(1.0);
            if ui
                .small_button(tr("Downbeat earlier"))
                .on_hover_text(tr("Move the accented click one beat earlier"))
                .clicked()
            {
                self.run(e, "song.set", json!({"song": id, "beat_anchor": song.beat_anchor - beat_len}));
            }
            if ui
                .small_button(tr("Downbeat later"))
                .on_hover_text(tr("Move the accented click one beat later"))
                .clicked()
            {
                self.run(e, "song.set", json!({"song": id, "beat_anchor": song.beat_anchor + beat_len}));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Beats per bar"));
            let mut bpb = u32::from(song.beats_per_bar);
            if ui.add(egui::DragValue::new(&mut bpb).range(1..=16)).changed() {
                self.run(e, "song.set", json!({"song": id, "beats_per_bar": bpb}));
            }
            ui.label(tr("Count-in"));
            let mut ci = u32::from(song.count_in_bars);
            let unit = format!(" {}", if ci == 1 { tr("bar") } else { tr("bars") });
            if ui.add(egui::DragValue::new(&mut ci).range(0..=8).suffix(unit)).changed() {
                self.run(e, "song.set", json!({"song": id, "count_in_bars": ci}));
            }
            ui.label(tr("Click"));
            egui::ComboBox::from_id_salt(("song-click", id)).selected_text(tr(song.click.label())).show_ui(ui, |ui| {
                for c in actcraft_show::SongClick::ALL {
                    if ui.selectable_label(song.click == c, tr(c.label())).clicked() {
                        self.run(e, "song.set", json!({"song": id, "click": c.key()}));
                    }
                }
            });
            ui.separator();
            ui.label(tr("Fade-out"));
            let mut fade = song.fade_out;
            if ui.add(egui::DragValue::new(&mut fade).range(0.0..=60.0).speed(0.1).suffix(" s")).changed() {
                self.run(e, "song.set", json!({"song": id, "fade_out": fade}));
            }
            let mut autoplay = song.autoplay;
            if ui.checkbox(&mut autoplay, tr("Play next song automatically")).changed() {
                self.run(e, "song.set", json!({"song": id, "autoplay": autoplay}));
            }
            let mut looped = song.loop_song;
            if ui.checkbox(&mut looped, tr("Loop this song")).changed() {
                self.run(e, "song.set", json!({"song": id, "loop_song": looped}));
            }
            let mut announce = song.announce_title;
            if ui
                .checkbox(&mut announce, tr("Announce the title"))
                .on_hover_text(tr("The computer's voice says the title when the song starts (Settings > Spoken cues)"))
                .changed()
            {
                self.run(e, "song.set", json!({"song": id, "announce_title": announce}));
            }
        });
    }

    fn track_strip(&mut self, ui: &mut Ui, e: &mut Engine, song: u64, i: usize, track: &actcraft_show::Track) {
        let t = self.tokens;
        let selected = self.selected_track == i;
        let stroke = if selected { Stroke::new(2.0, t.focus) } else { Stroke::new(1.0, t.stroke) };
        egui::Frame::new().fill(t.surface).stroke(stroke).corner_radius(t.radius).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(t.strip_width);
            ui.vertical(|ui| {
                let colour = theme::colour(&track.colour, t.accent);
                ui.horizontal(|ui| {
                    let mut rgb = colour.to_array();
                    let mut c3 = [rgb[0], rgb[1], rgb[2]];
                    if ui.color_edit_button_srgb(&mut c3).on_hover_text(tr("Track colour")).changed() {
                        rgb = [c3[0], c3[1], c3[2], 255];
                        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                        self.run(e, "track.set", json!({"song": song, "track": i, "colour": hex}));
                    }
                    if ui.small_button("\u{00d7}").on_hover_text(tr("Remove track")).clicked() {
                        self.modal = Some(flows::Modal::Confirm {
                            title: tr("Remove track"),
                            text: format!("{} \u{201c}{}\u{201d}?", tr("Remove the track"), track.name),
                            button: tr("Remove"),
                            command: "track.remove",
                            params: json!({"song": song, "track": i}),
                        });
                    }
                });
                if let Some(name) = edit_text(ui, egui::Id::unique(("track-name", song, i)), &track.name, t.strip_width)
                {
                    self.run(e, "track.set", json!({"song": song, "track": i, "name": name}));
                }
                let file_label = if track.file.is_empty() {
                    tr("Choose file\u{2026}").to_owned()
                } else {
                    std::path::Path::new(&track.file)
                        .file_name()
                        .map_or_else(String::new, |f| f.to_string_lossy().into_owned())
                };
                let file_button = egui::Button::new(RichText::new(file_label).small()).truncate();
                if ui
                    .add_sized([t.strip_width, 22.0], file_button)
                    .on_hover_text(if track.file.is_empty() {
                        tr("Choose the audio file for this track")
                    } else {
                        track.file.as_str()
                    })
                    .clicked()
                {
                    self.selected_track = i;
                    self.ask_track_file(e, FileAsk::TrackFile { song, track: i });
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
                    let mute = egui::Button::new(if track.mute { tr("Muted") } else { "M" }).selected(track.mute);
                    if ui.add(mute).on_hover_text(tr("Mute")).clicked() {
                        self.run(e, "track.set", json!({"song": song, "track": i, "mute": !track.mute}));
                    }
                    ui.menu_button(tr("EQ"), |ui| {
                        ui.label(RichText::new(format!("{} \u{2013} {}", tr("EQ"), track.name)).strong());
                        for (band, label) in [tr("Low"), tr("Low mid"), tr("High mid"), tr("High")].iter().enumerate() {
                            let Some(b) = track.eq.get(band) else { continue };
                            let mut gain = b.gain_db;
                            let r = ui.add(
                                egui::Slider::new(&mut gain, -24.0..=24.0)
                                    .suffix(" dB")
                                    .text(format!("{label} ({:.0} Hz)", b.freq)),
                            );
                            if r.changed() {
                                self.run(
                                    e,
                                    "track.set_eq",
                                    json!({"song": song, "track": i, "band": band, "gain_db": gain}),
                                );
                            }
                        }
                        if ui.button(tr("Reset EQ")).clicked() {
                            for band in 0..4 {
                                self.run(
                                    e,
                                    "track.set_eq",
                                    json!({"song": song, "track": i, "band": band, "gain_db": 0.0}),
                                );
                            }
                        }
                    });
                    if ui.small_button(tr("Wave")).on_hover_text(tr("Show this track's waveform")).clicked() {
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
            ui.checkbox(&mut self.snap_to_beat, tr("Snap to beat"))
                .on_hover_text(tr("Clicking the waveform jumps to the nearest beat of the grid"));
        });
        if track.file.is_empty() {
            ui.label(RichText::new(tr("No audio file on this track.")).color(t.text_dim));
            return;
        }
        // From memory or the cache on disk; otherwise decoded in the background, so the window
        // never waits for it.
        let Some(peaks) = e.peaks_for(&track.file) else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("Loading waveform\u{2026}")).color(t.text_dim));
            });
            return;
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
        // Regions behind the waveform, with their names at the top.
        for r in &song.regions {
            if r.end < from || r.start > from + span {
                continue;
            }
            let x0 = rect.left() + ((r.start - from).max(0.0) / span * f64::from(width)) as f32;
            let x1 = rect.left() + ((r.end - from).min(span) / span * f64::from(width)) as f32;
            let rc = theme::colour(&r.colour, t.accent);
            let area =
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1.max(x0 + 1.0), rect.bottom()));
            painter.rect_filled(area, 0, rc.gamma_multiply(0.18));
            painter.line_segment([area.left_top(), area.left_bottom()], Stroke::new(1.5, rc));
            let label = format!("{} \u{00b7} {}", r.name, tr(regions_view::mode_label(r.mode)));
            painter.text(
                area.left_top() + vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(12.0),
                t.text,
            );
        }
        // The beat grid from the song's tempo and first beat: bars stronger, with numbers.
        let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
        let px_beat = (clock.beat_len() / span * f64::from(width)) as f32;
        let px_bar = px_beat * f32::from(clock.beats_per_bar);
        if px_bar >= 4.0 {
            let every = (40.0 / px_bar).ceil().max(1.0) as i64;
            for line in clock.grid(from, from + span, 8_000) {
                let x = rect.left() + ((line.time - from) / span * f64::from(width)) as f32;
                if line.bar_start && line.bar.rem_euclid(every) == 1 % every {
                    painter.vline(x, rect.y_range(), Stroke::new(1.0, t.text_dim.gamma_multiply(0.7)));
                    painter.text(
                        egui::pos2(x + 3.0, rect.bottom() - 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        line.bar.to_string(),
                        egui::FontId::monospace(11.0),
                        t.text_dim,
                    );
                } else if !line.bar_start && px_beat >= 8.0 {
                    painter.vline(x, rect.y_range(), Stroke::new(1.0, t.text_dim.gamma_multiply(0.25)));
                }
            }
        }
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
            let mut secs = from + f64::from((pos.x - rect.left()) / width) * span;
            if self.snap_to_beat {
                secs = clock.snap(secs, 1).max(0.0);
            }
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

/// A one-line text field that keeps what the person types while it has focus and hands the new
/// text back once they leave it (Enter, Tab or a click elsewhere). Returns `Some` only when the
/// text changed.
fn edit_text(ui: &mut Ui, id: egui::Id, current: &str, width: f32) -> Option<String> {
    let mut buffer: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned());
    // A fixed size, so grids and wrapping rows can't squeeze the field to a sliver.
    let height = ui.spacing().interact_size.y;
    let r = ui.add_sized([width, height], egui::TextEdit::singleline(&mut buffer).id(id));
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        return (buffer != current).then_some(buffer);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buffer));
    } else {
        // Not being edited: show the stored value, also after an undo or a remote change.
        ui.data_mut(|d| d.remove::<String>(id));
    }
    None
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

    #[test]
    fn dropping_a_song_on_a_row_moves_it_above_or_below() {
        use super::drop_position;
        // Set A B C D; drag B (1).
        assert_eq!(drop_position(1, 3, true), Some(4), "below D: to the end");
        assert_eq!(drop_position(1, 0, false), Some(0), "above A: to the start");
        assert_eq!(drop_position(1, 1, false), None, "on itself");
        assert_eq!(drop_position(1, 1, true), None);
        assert_eq!(drop_position(1, 2, false), None, "above C is where it is");
        assert_eq!(drop_position(1, 2, true), Some(3));
        let set = ["A", "B", "C", "D"];
        let moved = actcraft_library::order::move_items(&set, &[1], 3);
        assert_eq!(moved, ["A", "C", "B", "D"], "below C");
    }
}
