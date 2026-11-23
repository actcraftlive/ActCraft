//! Everything a menu item or shortcut needs besides a plain command: native file pickers (asked
//! through the host, which owns the window), in-window dialogs for names and confirmations,
//! "save changes?" before a show is replaced or the window closes, and files dropped on the window.
//!
//! The person never types a path or JSON: every command with params gets a proper control here.

use std::path::{Path, PathBuf};

use actcraft_engine::{Engine, registry};
use actcraft_library::smart;
use egui::{Key, RichText, Ui};
use serde_json::{Value, json};

use crate::{ActCraftApp, tr};

/// Audio file types the library adds (decoded by craft-audio).
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "opus", "m4a"];
/// The show file type.
pub const SHOW_EXTENSIONS: &[&str] = &["actcraft"];
/// The show package file type (the show and its files in one file).
pub const PACKAGE_EXTENSIONS: &[&str] = &["actcraftpack"];

/// Why a file picker was opened; the answer goes back to [`ActCraftApp::file_chosen`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAsk {
    OpenShow,
    SaveShowAs {
        then: AfterSave,
    },
    AddSongs,
    AddTrack {
        song: u64,
    },
    TrackFile {
        song: u64,
        track: usize,
    },
    ImportLyrics {
        song: u64,
    },
    AttachPdf {
        song: u64,
    },
    AttachNotation {
        song: u64,
    },
    AttachMidi {
        song: u64,
    },
    /// A package of the show, or of one song (`song`) or playlist (`playlist`) of it.
    ExportPackage {
        song: Option<u64>,
        playlist: Option<usize>,
    },
    OpenPackage,
}

/// What happens once a save finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSave {
    Nothing,
    Then(AfterDiscard),
}

/// What waits behind a "save changes?" question.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum AfterDiscard {
    NewShow,
    OpenShow,
    OpenPackage,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Open,
    OpenMany,
    Save,
}

/// A native file picker the host should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    pub ask: FileAsk,
    pub kind: FileKind,
    pub title: &'static str,
    pub filters: Vec<(&'static str, &'static [&'static str])>,
    pub file_name: Option<String>,
    pub directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAction {
    RenameShow,
    NewPlaylist,
}

/// The dialog shown on top of the window, if any.
pub(crate) enum Modal {
    SaveChanges { then: AfterDiscard },
    Text { title: &'static str, label: &'static str, value: String, action: TextAction, focus: bool },
    Confirm { title: &'static str, text: String, button: &'static str, command: &'static str, params: Value },
    Smart { name: String, match_all: bool, rules: Vec<(String, String, String)>, error: Option<String> },
    Info { title: &'static str, lines: Vec<String> },
    About,
}

/// ActCraft's website, shown in the About window.
pub const WEBSITE: &str = "https://actcraft.org";

fn show_dir(e: &Engine) -> Option<PathBuf> {
    e.show_path.as_deref().and_then(|p| Path::new(p).parent()).map(Path::to_path_buf)
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e)))
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Command ids the window drives through its own controls (file pickers, dialogs).
pub fn handled_by_window(id: &str) -> bool {
    matches!(
        id,
        "show.new"
            | "show.open"
            | "show.save"
            | "show.save_as"
            | "show.rename"
            | "song.import_files"
            | "lyrics.import"
            | "notation.attach"
            | "midi.attach"
            | "song.add"
            | "song.remove"
            | "track.add"
            | "playlist.create"
            | "playlist.create_smart"
            | "queue.add"
            | "queue.play_next"
            | "app.about"
            | "app.commands"
            | "song.duplicates"
            | "show.export_package"
            | "song.export_package"
            | "playlist.export_package"
            | "show.open_package"
    )
}

/// Whether a command can be offered in a menu: the window drives it, or it needs no params.
pub fn offered_in_menu(id: &str) -> bool {
    handled_by_window(id) || registry().get(id).is_some_and(|c| c.spec.runs_without_params())
}

impl ActCraftApp {
    /// Native file pickers waiting to be shown; the host calls this every frame.
    pub fn take_file_requests(&mut self) -> Vec<FileRequest> {
        std::mem::take(&mut self.file_requests)
    }

    /// The host reports a picker's answer.
    pub fn file_chosen(&mut self, ask: FileAsk, paths: Vec<PathBuf>) {
        let engine = std::sync::Arc::clone(&self.engine);
        let mut guard = match engine.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let e: &mut Engine = &mut guard;
        let Some(first) = paths.first().cloned() else { return };
        match ask {
            FileAsk::OpenShow => {
                let _ = e.run("show.open", &json!({"path": path_string(&first), "discard": true}));
            }
            FileAsk::SaveShowAs { then } => {
                let mut path = first;
                if !has_ext(&path, SHOW_EXTENSIONS) {
                    path.set_extension("actcraft");
                }
                if e.run("show.save", &json!({"path": path_string(&path)})).is_ok()
                    && let AfterSave::Then(next) = then
                {
                    self.continue_after_discard(e, next);
                }
            }
            FileAsk::AttachPdf { song } => {
                if e.run("song.set", &json!({"song": song, "pdf_file": path_string(&first)})).is_ok() {
                    self.centre_tab = crate::CentreTab::Sheet;
                }
            }
            FileAsk::ExportPackage { song, playlist } => {
                let mut path = first;
                if !has_ext(&path, PACKAGE_EXTENSIONS) {
                    path.set_extension("actcraftpack");
                }
                let mut prm = json!({"path": path_string(&path)});
                if let Some(id) = song {
                    prm["song"] = json!(id);
                }
                if let Some(i) = playlist {
                    prm["playlist"] = json!(i);
                }
                let _ = e.run("show.export_package", &prm);
            }
            FileAsk::OpenPackage => {
                let _ = e.run("show.open_package", &json!({"path": path_string(&first), "discard": true}));
            }
            FileAsk::AddSongs => self.add_songs(e, &paths),
            FileAsk::AddTrack { song } => {
                for p in &paths {
                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let _ = e.run("track.add", &json!({"song": song, "name": name, "file": path_string(p)}));
                }
            }
            FileAsk::TrackFile { song, track } => {
                let _ = e.run("track.set", &json!({"song": song, "track": track, "file": path_string(&first)}));
            }
            FileAsk::ImportLyrics { song } => {
                if e.run("lyrics.import", &json!({"song": song, "path": path_string(&first)})).is_ok() {
                    self.centre_tab = crate::CentreTab::Lyrics;
                }
            }
            FileAsk::AttachMidi { song } => {
                let _ = e.run("midi.attach", &json!({"song": song, "path": path_string(&first)}));
            }
            FileAsk::AttachNotation { song } => {
                if e.run("notation.attach", &json!({"song": song, "path": path_string(&first)})).is_ok() {
                    self.centre_tab = crate::CentreTab::Sheet;
                    self.sheet_source = crate::sheet_view::SheetSource::Notation;
                    // Read again, also when the same file was edited and attached once more.
                    self.notation = None;
                }
            }
        }
    }

    /// The host reports that a picker was cancelled (a pending quit is cancelled too).
    pub fn file_cancelled(&mut self, _ask: FileAsk) {}

    /// The host reports that no picker could be shown.
    pub fn file_dialog_failed(&mut self, why: String) {
        self.lock().say_error(format!("{} {why}", tr("The file window could not open:")));
    }

    pub(crate) fn ask_file(&mut self, request: FileRequest) {
        self.file_requests.push(request);
    }

    fn add_songs(&mut self, e: &mut Engine, paths: &[PathBuf]) {
        let audio: Vec<String> =
            paths.iter().filter(|p| has_ext(p, AUDIO_EXTENSIONS)).map(|p| path_string(p)).collect();
        if audio.is_empty() {
            e.say_error(tr("None of these files is an audio file ActCraft can play."));
            return;
        }
        let _ = e.run("song.import_files", &json!({"paths": audio}));
    }

    fn ask_open_show(&mut self, e: &Engine) {
        self.ask_file(FileRequest {
            ask: FileAsk::OpenShow,
            kind: FileKind::Open,
            title: tr("Open show"),
            filters: vec![(tr("ActCraft show"), SHOW_EXTENSIONS)],
            file_name: None,
            directory: show_dir(e),
        });
    }

    fn ask_open_package(&mut self, e: &Engine) {
        self.ask_file(FileRequest {
            ask: FileAsk::OpenPackage,
            kind: FileKind::Open,
            title: tr("Open show package"),
            filters: vec![(tr("ActCraft show package"), PACKAGE_EXTENSIONS)],
            file_name: None,
            directory: show_dir(e),
        });
    }

    /// Asks where to save a package of the show, or of one song or playlist; the file is named
    /// after what goes in it.
    pub(crate) fn ask_export_package(&mut self, e: &Engine, song: Option<u64>, playlist: Option<usize>) {
        let (name, title) = match (song.and_then(|id| e.show.song(id)), playlist.and_then(|i| e.show.playlists.get(i)))
        {
            (Some(s), _) => (s.title.clone(), tr("Export song package")),
            (None, Some(pl)) => (pl.name.clone(), tr("Export playlist package")),
            _ => (e.show.name.clone(), tr("Export show package")),
        };
        let name = if name.trim().is_empty() { tr("My show").to_owned() } else { name };
        // A title may hold characters no file name can.
        let name: String =
            name.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
        self.ask_file(FileRequest {
            ask: FileAsk::ExportPackage { song, playlist },
            kind: FileKind::Save,
            title,
            filters: vec![(tr("ActCraft show package"), PACKAGE_EXTENSIONS)],
            file_name: Some(format!("{name}.actcraftpack")),
            directory: show_dir(e),
        });
    }

    pub(crate) fn ask_save_as(&mut self, e: &Engine, then: AfterSave) {
        let name = if e.show.name.trim().is_empty() { tr("My show").to_owned() } else { e.show.name.clone() };
        self.ask_file(FileRequest {
            ask: FileAsk::SaveShowAs { then },
            kind: FileKind::Save,
            title: tr("Save show as"),
            filters: vec![(tr("ActCraft show"), SHOW_EXTENSIONS)],
            file_name: Some(format!("{name}.actcraft")),
            directory: show_dir(e),
        });
    }

    pub(crate) fn ask_add_songs(&mut self, e: &Engine) {
        self.ask_file(FileRequest {
            ask: FileAsk::AddSongs,
            kind: FileKind::OpenMany,
            title: tr("Add songs from audio files"),
            filters: vec![(tr("Audio files"), AUDIO_EXTENSIONS)],
            file_name: None,
            directory: show_dir(e),
        });
    }

    pub(crate) fn ask_track_file(&mut self, e: &Engine, ask: FileAsk) {
        let many = matches!(ask, FileAsk::AddTrack { .. });
        self.ask_file(FileRequest {
            ask,
            kind: if many { FileKind::OpenMany } else { FileKind::Open },
            title: if many { tr("Add tracks from audio files") } else { tr("Choose the audio file for this track") },
            filters: vec![(tr("Audio files"), AUDIO_EXTENSIONS)],
            file_name: None,
            directory: show_dir(e),
        });
    }

    /// Saves to the known file, or asks where.
    pub(crate) fn save(&mut self, e: &mut Engine, then: AfterSave) {
        if e.show_path.is_some() {
            if e.run("show.save", &Value::Null).is_ok()
                && let AfterSave::Then(next) = then
            {
                self.continue_after_discard(e, next);
            }
        } else {
            self.ask_save_as(e, then);
        }
    }

    /// Runs `next` now, or first asks whether to save unsaved changes.
    pub(crate) fn guard_unsaved(&mut self, e: &mut Engine, next: AfterDiscard) {
        if e.dirty {
            self.modal = Some(Modal::SaveChanges { then: next });
        } else {
            self.continue_after_discard(e, next);
        }
    }

    fn continue_after_discard(&mut self, e: &mut Engine, next: AfterDiscard) {
        match next {
            AfterDiscard::NewShow => {
                let _ = e.run("show.new", &json!({"discard": true}));
            }
            AfterDiscard::OpenShow => self.ask_open_show(e),
            AfterDiscard::OpenPackage => self.ask_open_package(e),
            AfterDiscard::Quit => self.allow_close = true,
        }
    }

    /// Runs a command from a menu or shortcut, with its dialog when it needs one.
    pub(crate) fn activate(&mut self, e: &mut Engine, id: &'static str) {
        let current = e.current_song;
        match id {
            "show.new" => self.guard_unsaved(e, AfterDiscard::NewShow),
            "show.open" => self.guard_unsaved(e, AfterDiscard::OpenShow),
            "show.save" => self.save(e, AfterSave::Nothing),
            "show.save_as" => self.ask_save_as(e, AfterSave::Nothing),
            "show.export_package" => self.ask_export_package(e, None, None),
            "song.export_package" => self.ask_export_package(e, e.current_song, None),
            "playlist.export_package" => self.ask_export_package(e, None, e.current_playlist),
            "show.open_package" => self.guard_unsaved(e, AfterDiscard::OpenPackage),
            "show.rename" => {
                self.modal = Some(Modal::Text {
                    title: tr("Rename show"),
                    label: tr("Show name"),
                    value: e.show.name.clone(),
                    action: TextAction::RenameShow,
                    focus: true,
                });
            }
            "song.import_files" => self.ask_add_songs(e),
            "notation.attach" => {
                if let Some(song) = current {
                    self.ask_file(FileRequest {
                        ask: FileAsk::AttachNotation { song },
                        kind: FileKind::Open,
                        title: tr("Attach notation"),
                        filters: vec![(tr("MusicXML score"), crate::notation_view::NOTATION_EXTENSIONS)],
                        file_name: None,
                        directory: show_dir(e),
                    });
                }
            }
            "midi.attach" => {
                if let Some(song) = current {
                    self.ask_file(FileRequest {
                        ask: FileAsk::AttachMidi { song },
                        kind: FileKind::Open,
                        title: tr("Attach MIDI file"),
                        filters: vec![(tr("MIDI file"), actcraft_engine::midi_file::MIDI_EXTENSIONS)],
                        file_name: None,
                        directory: show_dir(e),
                    });
                }
            }
            "lyrics.import" => {
                if let Some(song) = current {
                    self.ask_file(FileRequest {
                        ask: FileAsk::ImportLyrics { song },
                        kind: FileKind::Open,
                        title: tr("Import lyrics"),
                        filters: vec![(tr("Lyrics"), crate::lyrics_view::LYRICS_EXTENSIONS)],
                        file_name: None,
                        directory: show_dir(e),
                    });
                }
            }
            "song.add" => {
                // An empty song, shown at once so its details can be filled in.
                if let Ok(v) = e.run("song.add", &Value::Null)
                    && let Some(id) = v.get("song").and_then(Value::as_u64)
                    && !e.playing()
                {
                    let _ = e.run("song.load", &json!({"song": id}));
                }
            }
            "song.remove" => {
                if let Some(s) = current.and_then(|id| e.show.song(id)) {
                    self.modal = Some(Modal::Confirm {
                        title: tr("Delete song"),
                        text: format!(
                            "{} \u{201c}{}\u{201d}? {}",
                            tr("Delete"),
                            s.title,
                            tr("This removes it from every playlist.")
                        ),
                        button: tr("Delete"),
                        command: "song.remove",
                        params: json!({"song": s.id, "confirm": true}),
                    });
                }
            }
            "track.add" => {
                if let Some(song) = current {
                    self.ask_track_file(e, FileAsk::AddTrack { song });
                }
            }
            "playlist.create" => {
                self.modal = Some(Modal::Text {
                    title: tr("New playlist"),
                    label: tr("Playlist name"),
                    value: String::new(),
                    action: TextAction::NewPlaylist,
                    focus: true,
                });
            }
            "playlist.create_smart" => {
                self.modal = Some(Modal::Smart {
                    name: String::new(),
                    match_all: true,
                    rules: vec![("bpm".to_owned(), ">".to_owned(), "120".to_owned())],
                    error: None,
                });
            }
            "queue.add" | "queue.play_next" => {
                if let Some(song) = current {
                    let _ = e.run(id, &json!({"song": song}));
                }
            }
            "app.about" => {
                self.modal = Some(Modal::About);
            }
            "app.commands" => {
                let lines = registry()
                    .iter()
                    .map(|c| match c.spec.shortcut {
                        Some(s) => format!("{}  ({})", tr(c.spec.label), crate::shortcuts::display(s)),
                        None => tr(c.spec.label).to_owned(),
                    })
                    .collect();
                self.modal = Some(Modal::Info { title: tr("All commands"), lines });
            }
            "song.duplicates" => {
                let pairs = actcraft_library::filename::find_duplicates(&e.show.songs, 200);
                let title = |id: u64| {
                    e.show.song(id).map_or_else(String::new, |s| format!("{} \u{2013} {}", s.title, s.artist))
                };
                let lines = if pairs.is_empty() {
                    vec![tr("No duplicate songs found.").to_owned()]
                } else {
                    pairs.iter().map(|(a, b)| format!("{}  =  {}", title(*a), title(*b))).collect()
                };
                self.modal = Some(Modal::Info { title: tr("Duplicate songs"), lines });
            }
            _ => {
                if registry().get(id).is_some_and(|c| c.spec.runs_without_params()) {
                    let _ = e.run(id, &Value::Null);
                }
            }
        }
    }

    /// Files dropped on the window: a show opens, a score or PDF is attached to the song on screen,
    /// audio files become songs.
    pub(crate) fn handle_drops(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() {
            return;
        }
        if let Some(show) = dropped.iter().find(|p| has_ext(p, SHOW_EXTENSIONS) || has_ext(p, PACKAGE_EXTENSIONS)) {
            if e.dirty {
                e.say_error(tr("Save or discard the open show first (File > New Show), then drop it again."));
            } else if has_ext(show, PACKAGE_EXTENSIONS) {
                let _ = e.run("show.open_package", &json!({"path": path_string(show)}));
            } else {
                let _ = e.run("show.open", &json!({"path": path_string(show)}));
            }
            return;
        }
        // A score or a PDF goes to the song on screen, in its Sheet music tab.
        if let Some(song) = e.current_song {
            if let Some(score) = dropped.iter().find(|p| has_ext(p, crate::notation_view::NOTATION_EXTENSIONS)) {
                if e.run("notation.attach", &json!({"song": song, "path": path_string(score)})).is_ok() {
                    self.centre_tab = crate::CentreTab::Sheet;
                    self.sheet_source = crate::sheet_view::SheetSource::Notation;
                    // Read again, also when the same file was edited and attached once more.
                    self.notation = None;
                }
                return;
            }
            if let Some(pdf) = dropped.iter().find(|p| has_ext(p, crate::sheet_view::PDF_EXTENSIONS)) {
                if e.run("song.set", &json!({"song": song, "pdf_file": path_string(pdf)})).is_ok() {
                    self.centre_tab = crate::CentreTab::Sheet;
                    self.sheet_source = crate::sheet_view::SheetSource::Pdf;
                }
                return;
            }
        }
        self.add_songs(e, &dropped);
    }

    /// Hint while files are dragged over the window.
    pub(crate) fn drop_hint(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let t = self.tokens;
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-hint")));
        painter.rect_filled(rect, 0, t.bg.gamma_multiply(0.85));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("Drop audio files to add songs, a show file to open it, or a score or PDF to attach it to the song"),
            egui::FontId::proportional(24.0),
            t.text,
        );
    }

    /// Asks before the window closes with unsaved changes.
    pub(crate) fn handle_close(&mut self, ctx: &egui::Context, e: &mut Engine) {
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && e.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.modal = Some(Modal::SaveChanges { then: AfterDiscard::Quit });
        }
    }

    /// Draws the open dialog, if any.
    pub(crate) fn modal_ui(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let Some(mut modal) = self.modal.take() else { return };
        let danger = self.tokens.danger;
        if self.logo.is_none()
            && let Some((w, h, rgba)) = &self.platform.logo
        {
            let image = egui::ColorImage::from_rgba_unmultiplied([*w, *h], rgba);
            self.logo = Some(ctx.load_texture("actcraft-logo", image, egui::TextureOptions::LINEAR));
        }
        let logo = self.logo.clone();
        let mut keep = true;
        let response = egui::Modal::new(egui::Id::unique("actcraft-modal")).show(ctx, |ui| {
            ui.set_min_width(380.0);
            match &mut modal {
                Modal::SaveChanges { then } => {
                    ui.heading(tr("Save changes?"));
                    ui.label(tr("The show has changes that are not saved yet."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let save = ui.button(tr("Save"));
                        if !save.has_focus() && ui.memory(|m| m.focused().is_none()) {
                            save.request_focus();
                        }
                        if save.clicked() {
                            self.save(e, AfterSave::Then(*then));
                            keep = false;
                        }
                        if ui.button(tr("Don't save")).clicked() {
                            self.continue_after_discard(e, *then);
                            keep = false;
                        }
                        if ui.button(tr("Cancel")).clicked() {
                            keep = false;
                        }
                    });
                }
                Modal::Text { title, label, value, action, focus } => {
                    ui.heading(*title);
                    ui.label(*label);
                    let edit = ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                    if *focus {
                        edit.request_focus();
                        *focus = false;
                    }
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let ok = ui.add_enabled(!value.trim().is_empty(), egui::Button::new(tr("OK"))).clicked();
                        if (ok || enter) && !value.trim().is_empty() {
                            let name = value.trim().to_owned();
                            let _ = match action {
                                TextAction::RenameShow => e.run("show.rename", &json!({"name": name})),
                                TextAction::NewPlaylist => e.run("playlist.create", &json!({"name": name})),
                            };
                            keep = false;
                        }
                        if ui.button(tr("Cancel")).clicked() {
                            keep = false;
                        }
                    });
                }
                Modal::Confirm { title, text, button, command, params } => {
                    ui.heading(*title);
                    ui.label(text.as_str());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new(*button).color(danger)).clicked() {
                            let _ = e.run(command, params);
                            keep = false;
                        }
                        let cancel = ui.button(tr("Cancel"));
                        if ui.memory(|m| m.focused().is_none()) {
                            cancel.request_focus();
                        }
                        if cancel.clicked() {
                            keep = false;
                        }
                    });
                }
                Modal::Smart { name, match_all, rules, error } => {
                    ui.heading(tr("New smart playlist"));
                    ui.label(tr("Name"));
                    ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.label(tr("Songs must match"));
                        ui.radio_value(match_all, true, tr("all rules"));
                        ui.radio_value(match_all, false, tr("any rule"));
                    });
                    let mut remove = None;
                    for (i, (field, op, value)) in rules.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("smart-field", i))
                                .selected_text(tr(field_label(field)))
                                .show_ui(ui, |ui| {
                                    for f in smart::FIELDS {
                                        ui.selectable_value(field, f.to_owned(), tr(field_label(f)));
                                    }
                                });
                            egui::ComboBox::from_id_salt(("smart-op", i))
                                .width(90.0)
                                .selected_text(tr(op_label(op)))
                                .show_ui(ui, |ui| {
                                    for o in smart::OPS {
                                        ui.selectable_value(op, o.to_owned(), tr(op_label(o)));
                                    }
                                });
                            ui.add(egui::TextEdit::singleline(value).desired_width(120.0));
                            if ui.small_button("\u{00d7}").on_hover_text(tr("Remove rule")).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove
                        && rules.len() > 1
                    {
                        rules.remove(i);
                    }
                    if rules.len() < 32 && ui.button(tr("Add rule")).clicked() {
                        rules.push(("title".to_owned(), "contains".to_owned(), String::new()));
                    }
                    if let Some(err) = error.as_ref() {
                        ui.colored_label(danger, err.as_str());
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("Create"))).clicked() {
                            let rules_json: Vec<Value> =
                                rules.iter().map(|(f, o, v)| json!({"field": f, "op": o, "value": v})).collect();
                            match e.run(
                                "playlist.create_smart",
                                &json!({"name": name.trim(), "match_all": *match_all, "rules": rules_json}),
                            ) {
                                Ok(_) => keep = false,
                                Err(err) => *error = Some(err.to_string()),
                            }
                        }
                        if ui.button(tr("Cancel")).clicked() {
                            keep = false;
                        }
                    });
                }
                Modal::Info { title, lines } => {
                    ui.heading(*title);
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for l in lines.iter() {
                            ui.label(l.as_str());
                        }
                    });
                    ui.add_space(8.0);
                    let close = ui.button(tr("Close"));
                    if ui.memory(|m| m.focused().is_none()) {
                        close.request_focus();
                    }
                    if close.clicked() {
                        keep = false;
                    }
                }
                Modal::About => {
                    ui.set_max_width(460.0);
                    ui.horizontal(|ui| {
                        if let Some(logo) = &logo {
                            ui.add(
                                egui::Image::new((logo.id(), egui::vec2(72.0, 72.0)))
                                    .alt_text(tr("ActCraft logo: a drawn octopus")),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.heading("ActCraft");
                            ui.label(format!("{} {}", tr("Version"), env!("CARGO_PKG_VERSION")));
                            ui.hyperlink_to(WEBSITE.trim_start_matches("https://"), WEBSITE);
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(tr("ActCraft is show control for live musicians: backing tracks with lyrics, chords, sheet music, spoken cues, setlists and click, and (coming) MIDI and DMX lighting, all on one clock and in one show file."));
                    ui.add_space(4.0);
                    ui.label(tr("Free and open source for Windows, macOS and Linux."));
                    ui.add_space(8.0);
                    ui.label(RichText::new(tr("Licence: MIT OR Apache-2.0")).small());
                    ui.add_space(8.0);
                    let close = ui.button(tr("Close"));
                    if ui.memory(|m| m.focused().is_none()) {
                        close.request_focus();
                    }
                    if close.clicked() {
                        keep = false;
                    }
                }
            }
        });
        // Esc or a click outside closes the dialog, as on every platform.
        if response.should_close() {
            keep = false;
        }
        if keep && self.modal.is_none() {
            self.modal = Some(modal);
        }
    }

    /// The welcome screen's actions.
    pub(crate) fn welcome_buttons(&mut self, ui: &mut Ui, e: &mut Engine) {
        ui.horizontal(|ui| {
            let big = |text: &str| egui::Button::new(RichText::new(text).size(17.0)).min_size(egui::vec2(180.0, 44.0));
            if ui.add(big(tr("Add songs\u{2026}"))).clicked() {
                self.ask_add_songs(e);
            }
            if ui.add(big(tr("Open show\u{2026}"))).clicked() {
                self.guard_unsaved(e, AfterDiscard::OpenShow);
            }
        });
    }
}

pub(crate) fn field_label(f: &str) -> &'static str {
    match f {
        "title" => "Title",
        "artist" => "Artist",
        "key" => "Key",
        "keyword" => "Keyword",
        "bpm" => "BPM",
        "year" => "Year",
        "duration" => "Length (s)",
        _ => "?",
    }
}

pub(crate) fn op_label(o: &str) -> &'static str {
    match o {
        "contains" => "contains",
        "is" => "is",
        "not" => "is not",
        ">" => "more than",
        "<" => "less than",
        ">=" => "at least",
        "<=" => "at most",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_command_has_a_control() {
        // A menu item whose command needs params must be driven by a window dialog, never by
        // typed JSON.
        for c in registry().iter().filter(|c| !c.spec.menu.is_empty()) {
            assert!(offered_in_menu(c.spec.id), "{} is in a menu but has no window control", c.spec.id);
        }
    }

    #[test]
    fn audio_and_show_files_are_told_apart() {
        assert!(has_ext(Path::new("C:/a/Song.WAV"), AUDIO_EXTENSIONS));
        assert!(has_ext(Path::new("gig.actcraft"), SHOW_EXTENSIONS));
        assert!(!has_ext(Path::new("notes.txt"), AUDIO_EXTENSIONS));
    }

    #[test]
    fn about_opens_its_own_window_with_the_website() {
        let ctx = egui::Context::default();
        let engine = Engine::new(Box::new(actcraft_engine::MemoryServices::default()));
        let mut app = ActCraftApp::new(&ctx, std::sync::Arc::new(std::sync::Mutex::new(engine)), Default::default());
        let mut e = Engine::new(Box::new(actcraft_engine::MemoryServices::default()));
        app.activate(&mut e, "app.about");
        assert!(matches!(app.modal, Some(Modal::About)));
        assert_eq!(WEBSITE, "https://actcraft.org");
    }
}
