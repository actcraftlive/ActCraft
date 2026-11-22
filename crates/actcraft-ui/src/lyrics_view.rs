//! The Lyrics tab: lyrics with chords above the words, following the playback time, and an
//! editor with formatting buttons.

use actcraft_engine::Engine;
use actcraft_engine::lyrics::{self, Font, LineKind, Span};
use actcraft_show::{LyricsScroll, Song};
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, RichText, Stroke, TextFormat, Ui};
use serde_json::json;

use crate::flows::{FileAsk, FileKind, FileRequest};
use crate::theme::Tokens;
use crate::{ActCraftApp, tr};

/// Lyrics file types the import picker offers.
pub const LYRICS_EXTENSIONS: &[&str] = &["txt", "cho", "chopro", "chordpro", "crd", "pro", "lrc"];

/// Editing state of the lyrics editor.
#[derive(Default)]
pub(crate) struct LyricsEdit {
    pub song: u64,
    pub text: String,
    /// Last known cursor/selection as character indexes (start, end).
    pub selection: (usize, usize),
    /// Find and replace.
    pub find: String,
    pub replace_with: String,
    pub match_case: bool,
    /// A selection to put into the text box on the next frame (a found match).
    pub select: Option<(usize, usize)>,
}

/// The next match after the selection (wrapping round), as a character range.
fn next_match(edit: &LyricsEdit) -> Option<(usize, usize)> {
    let found = lyrics::find_all(&edit.text, &edit.find, edit.match_case);
    let after = edit.selection.0.max(edit.selection.1);
    found.iter().copied().find(|&(s, _)| s >= after).or_else(|| found.first().copied())
}

/// Replaces the selected match (if the selection is one) and selects the next.
fn replace_current(edit: &mut LyricsEdit) {
    let (a, b) = (edit.selection.0.min(edit.selection.1), edit.selection.0.max(edit.selection.1));
    let found = lyrics::find_all(&edit.text, &edit.find, edit.match_case);
    if found.contains(&(a, b)) {
        let (ba, bb) = (byte_at(&edit.text, a), byte_at(&edit.text, b));
        edit.text.replace_range(ba..bb, &edit.replace_with);
        let end = a + edit.replace_with.chars().count();
        edit.selection = (end, end);
    }
    edit.select = next_match(edit);
    if let Some(s) = edit.select {
        edit.selection = s;
    }
}

fn byte_at(text: &str, char_index: usize) -> usize {
    text.char_indices().nth(char_index).map_or(text.len(), |(b, _)| b)
}

/// Wraps the selection in `before`/`after` (or inserts both at the cursor).
fn wrap_selection(edit: &mut LyricsEdit, before: &str, after: &str) {
    let (a, b) = (edit.selection.0.min(edit.selection.1), edit.selection.0.max(edit.selection.1));
    let (ba, bb) = (byte_at(&edit.text, a), byte_at(&edit.text, b));
    edit.text.insert_str(bb, after);
    edit.text.insert_str(ba, before);
    let shift = before.chars().count();
    edit.selection = (a + shift, b + shift);
}

/// Puts `prefix` at the start of the cursor's line (once).
fn prefix_line(edit: &mut LyricsEdit, prefix: &str) {
    let at = byte_at(&edit.text, edit.selection.0);
    let line_start = edit.text.get(..at).and_then(|t| t.rfind('\n')).map_or(0, |i| i + 1);
    let rest = edit.text.get(line_start..).unwrap_or("");
    let stripped = rest.trim_start_matches('#').trim_start_matches(' ');
    let removed = rest.len() - stripped.len();
    edit.text.replace_range(line_start..line_start + removed, prefix);
}

/// Index of the line the cursor is on.
fn cursor_line(edit: &LyricsEdit) -> usize {
    let at = byte_at(&edit.text, edit.selection.0);
    edit.text.get(..at).map_or(0, |t| t.matches('\n').count())
}

impl ActCraftApp {
    pub(crate) fn lyrics_tab(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        let editing = self.lyrics_edit.as_ref().is_some_and(|l| l.song == song.id);
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!editing, tr("View")).clicked() && editing {
                self.finish_lyrics_edit(e);
            }
            if ui.selectable_label(editing, tr("Edit")).clicked() && !editing {
                self.lyrics_edit =
                    Some(LyricsEdit { song: song.id, text: song.lyrics.clone(), ..LyricsEdit::default() });
            }
            ui.separator();
            ui.label(tr("Transpose"));
            if ui.small_button("\u{2212}").on_hover_text(tr("Down a semitone")).clicked() && song.transpose > -12 {
                self.run(e, "song.set", json!({"song": song.id, "transpose": song.transpose - 1}));
            }
            ui.label(RichText::new(format!("{:+}", song.transpose)).monospace());
            if ui.small_button("+").on_hover_text(tr("Up a semitone")).clicked() && song.transpose < 12 {
                self.run(e, "song.set", json!({"song": song.id, "transpose": song.transpose + 1}));
            }
            ui.label(tr("Capo"));
            let mut capo = i32::from(song.capo);
            if ui
                .add(egui::DragValue::new(&mut capo).range(0..=12))
                .on_hover_text(tr("Chords are shown as the shapes to play with the capo"))
                .changed()
            {
                self.run(e, "song.set", json!({"song": song.id, "capo": capo}));
            }
            ui.separator();
            ui.checkbox(&mut self.lyrics_show_chords, tr("Chords"));
        });
        // Second row: following playback, size and import.
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.lyrics_follow, tr("Follow playback"));
            egui::ComboBox::from_id_salt(("lyrics-scroll", song.id))
                .selected_text(tr(song.lyrics_scroll.label()))
                .show_ui(ui, |ui| {
                    for m in LyricsScroll::ALL {
                        if ui.selectable_label(song.lyrics_scroll == m, tr(m.label())).clicked() {
                            self.run(e, "song.set", json!({"song": song.id, "lyrics_scroll": m.key()}));
                        }
                    }
                })
                .response
                .on_hover_text(tr("How the lyrics follow playback in this song"));
            ui.spacing_mut().slider_width = 120.0;
            ui.add(egui::Slider::new(&mut self.lyrics_size, 14.0..=64.0).show_value(false).text(tr("Size")));
            ui.separator();
            self.stage_button(ui, StageShows::Lyrics);
            if ui.button(tr("Import lyrics\u{2026}")).on_hover_text(tr("Text, ChordPro or LRC file")).clicked() {
                self.ask_file(FileRequest {
                    ask: FileAsk::ImportLyrics { song: song.id },
                    kind: FileKind::Open,
                    title: tr("Import lyrics"),
                    filters: vec![(tr("Lyrics"), LYRICS_EXTENSIONS)],
                    file_name: None,
                    directory: None,
                });
            }
        });
        ui.separator();
        if editing {
            self.lyrics_editor(ui, e);
            return;
        }
        if song.lyrics.trim().is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new(tr("No lyrics yet. Click \u{201c}Edit\u{201d} to type or paste them, or \u{201c}Import lyrics\u{2026}\u{201d} for a text, ChordPro or LRC file.")).color(t.text_dim));
            ui.label(
                RichText::new(tr("Chords go in square brackets before the syllable: The [Am]road is [F]long."))
                    .color(t.text_dim),
            );
            return;
        }
        let look = LyricsLook {
            size: self.lyrics_size,
            chords: self.lyrics_show_chords,
            follow: self.lyrics_follow,
            salt: "lyrics",
        };
        let mut follow = std::mem::take(&mut self.lyrics_follow_state);
        Self::lyrics_display(self.tokens, ui, e, song, look, &mut follow);
        self.lyrics_follow_state = follow;
    }

    pub(crate) fn finish_lyrics_edit_pub(&mut self, e: &mut Engine) {
        self.finish_lyrics_edit(e);
    }

    fn finish_lyrics_edit(&mut self, e: &mut Engine) {
        if let Some(edit) = self.lyrics_edit.take()
            && e.show.song(edit.song).is_some_and(|s| s.lyrics != edit.text)
        {
            self.run(e, "song.set", json!({"song": edit.song, "lyrics": edit.text}));
        }
    }

    fn lyrics_editor(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let time = e.status.time.max(0.0);
        let Some(edit) = self.lyrics_edit.as_mut() else { return };
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("B").strong()).on_hover_text(tr("Bold")).clicked() {
                wrap_selection(edit, "**", "**");
            }
            if ui.button(RichText::new("I").italics()).on_hover_text(tr("Italic")).clicked() {
                wrap_selection(edit, "*", "*");
            }
            if ui.button(RichText::new("U").underline()).on_hover_text(tr("Underline")).clicked() {
                wrap_selection(edit, "__", "__");
            }
            if ui.button(tr("Heading")).on_hover_text(tr("Make this line a heading")).clicked() {
                prefix_line(edit, "# ");
            }
            if ui.button(tr("Chord")).on_hover_text(tr("Insert a chord at the cursor")).clicked() {
                wrap_selection(edit, "[", "]");
            }
            egui::ComboBox::from_id_salt("lyrics-colour").selected_text(tr("Colour")).width(90.0).show_ui(ui, |ui| {
                for name in ["red", "orange", "yellow", "green", "blue", "purple", "pink", "grey"] {
                    if ui.selectable_label(false, tr(name)).clicked() {
                        wrap_selection(edit, &format!("<color={name}>"), "</color>");
                    }
                }
            });
            egui::ComboBox::from_id_salt("lyrics-font").selected_text(tr("Font")).width(80.0).show_ui(ui, |ui| {
                for name in ["sans", "serif", "mono", "script"] {
                    if ui.selectable_label(false, tr(name)).clicked() {
                        wrap_selection(edit, &format!("<font={name}>"), "</font>");
                    }
                }
            });
            ui.separator();
            if ui
                .button(format!("{} {}", tr("Time stamp"), lyrics::format_timecode(time)))
                .on_hover_text(tr("Put the current playback time at the start of the cursor's line"))
                .clicked()
            {
                let line = cursor_line(edit);
                if let Some(updated) = lyrics::stamp_line(&edit.text, line, time) {
                    edit.text = updated;
                }
            }
        });
        let id = egui::Id::unique(("lyrics-editor", edit.song));
        // Find and replace.
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Find"));
            let find = ui.add(egui::TextEdit::singleline(&mut edit.find).desired_width(160.0));
            let enter = find.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.label(tr("Replace with"));
            ui.add(egui::TextEdit::singleline(&mut edit.replace_with).desired_width(160.0));
            ui.checkbox(&mut edit.match_case, tr("Match case"));
            let count = lyrics::find_all(&edit.text, &edit.find, edit.match_case).len();
            let has = count > 0;
            if (ui.add_enabled(has, egui::Button::new(tr("Find next"))).clicked() || (enter && has))
                && let Some(s) = next_match(edit)
            {
                edit.select = Some(s);
                edit.selection = s;
            }
            if ui
                .add_enabled(has, egui::Button::new(tr("Replace")))
                .on_hover_text(tr("Replace the selected match and find the next"))
                .clicked()
            {
                replace_current(edit);
            }
            if ui.add_enabled(has, egui::Button::new(tr("Replace all"))).clicked() {
                let (text, _) = lyrics::replace_all(&edit.text, &edit.find, &edit.replace_with, edit.match_case);
                edit.text = text;
                edit.selection = (0, 0);
            }
            if !edit.find.is_empty() {
                let found = actcraft_engine::fill(tr("{count} found"), &[("count", &count.to_string())]);
                ui.label(RichText::new(found).color(if has { t.text_dim } else { t.warning }));
            }
        });
        if let Some((a, b)) = edit.select.take() {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(a),
                egui::text::CCursor::new(b),
            )));
            state.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let output = egui::TextEdit::multiline(&mut edit.text)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(18)
            .show(ui);
        if let Some(range) = output.cursor_range {
            edit.selection = (range.secondary.index.0, range.primary.index.0);
        }
        ui.horizontal(|ui| {
            if ui.button(tr("Save lyrics")).clicked() {
                self.finish_lyrics_edit(e);
            }
            if ui.button(tr("Discard changes")).clicked() {
                self.lyrics_edit = None;
            }
            ui.label(RichText::new(tr("Markup: # heading, [Am] chord, **bold**, *italic*, __underline__, {c: comment}, [01:23.45] time stamp")).small().color(t.text_dim));
        });
    }

    pub(crate) fn lyrics_display(
        t: Tokens,
        ui: &mut Ui,
        e: &Engine,
        song: &Song,
        look: LyricsLook,
        state: &mut LyricsFollow,
    ) {
        let parsed = lyrics::parse(&song.lyrics);
        let flats = lyrics::chords::prefers_flats(&song.key);
        let shift = i32::from(song.transpose) - i32::from(song.capo);
        let current = if e.status.playing || e.status.time > 0.0 { parsed.line_at(e.status.time) } else { None };
        let size = look.size;
        let heading_colours = [t.accent, t.warning, t.playing];
        let mut area = egui::ScrollArea::vertical().id_salt(look.salt).auto_shrink([false, false]);
        // Smooth and constant-speed scrolling steer the view from last frame's layout.
        let times: Vec<Option<f64>> = parsed.lines.iter().map(|l| l.time).collect();
        let span = (song.start, song.end.unwrap_or(song.duration));
        let geometry = &state.geometry;
        let target = if look.follow && e.status.playing && geometry.song == song.id {
            follow_target(
                song.lyrics_scroll,
                e.status.time,
                &times,
                &geometry.centres,
                geometry.content,
                geometry.view,
                span,
            )
        } else {
            None
        };
        if let Some(target) = target {
            let now = geometry.offset;
            // Ease towards the target; a big jump (a seek) goes straight there.
            let next = if (target - now).abs() > geometry.view { target } else { now + (target - now) * 0.2 };
            area = area.vertical_scroll_offset(next);
        }
        let mut centres = Vec::with_capacity(parsed.lines.len());
        let output = area.show(ui, |ui| {
            let top = ui.cursor().top();
            for (i, line) in parsed.lines.iter().enumerate() {
                let is_current = current == Some(i);
                let fill = if is_current { t.surface_hover } else { Color32::TRANSPARENT };
                let bar = if line.chorus { Stroke::new(3.0, t.accent) } else { Stroke::NONE };
                let r = egui::Frame::new()
                    .fill(fill)
                    .corner_radius(t.radius)
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match &line.kind {
                            LineKind::Blank => {
                                ui.add_space(size * 0.5);
                            }
                            LineKind::Directive { name, value } => {
                                let label = match name.as_str() {
                                    "title" => Some(tr("Title")),
                                    "subtitle" => Some(tr("Subtitle")),
                                    "artist" => Some(tr("Artist")),
                                    "key" => Some(tr("Key")),
                                    "capo" => Some(tr("Capo")),
                                    "tempo" => Some(tr("Tempo")),
                                    _ => None,
                                };
                                if let Some(label) = label
                                    && !value.is_empty()
                                {
                                    ui.label(
                                        RichText::new(format!("{label}: {value}")).size(size * 0.5).color(t.text_dim),
                                    );
                                }
                            }
                            LineKind::Comment => {
                                ui.label(RichText::new(line.plain_text()).italics().size(size * 0.7).color(t.text_dim));
                            }
                            LineKind::Heading(level) => {
                                let colour = heading_colours
                                    .get(usize::from(level.saturating_sub(1)))
                                    .copied()
                                    .unwrap_or(t.accent);
                                let scale = match level {
                                    1 => 1.15,
                                    2 => 1.0,
                                    _ => 0.9,
                                };
                                ui.label(layout(&line.spans, size * scale, colour, true));
                            }
                            LineKind::Text => {
                                if look.chords && line.has_chords() {
                                    chord_line(ui, &line.spans, size, t.text, t.warning, shift, flats);
                                } else {
                                    ui.label(layout(&line.spans, size, t.text, false));
                                }
                            }
                        }
                    });
                if line.chorus {
                    let rect = r.response.rect;
                    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], bar);
                }
                centres.push(r.response.rect.center().y - top);
                if is_current && look.follow && song.lyrics_scroll == LyricsScroll::Line && state.last_line != Some(i) {
                    r.response.scroll_to_me(Some(egui::Align::Center));
                    state.last_line = Some(i);
                }
            }
        });
        state.geometry = LyricsGeometry {
            song: song.id,
            centres,
            content: output.content_size.y,
            view: output.inner_rect.height(),
            offset: output.state.clamped_offset().y,
        };
        if target.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

/// How a lyrics view looks: text size, chords, whether it follows playback, and its scroll id.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LyricsLook {
    pub size: f32,
    pub chords: bool,
    pub follow: bool,
    pub salt: &'static str,
}

/// What a lyrics view remembers between frames to follow playback.
#[derive(Debug, Default, Clone)]
pub(crate) struct LyricsFollow {
    pub geometry: LyricsGeometry,
    pub last_line: Option<usize>,
}

/// What the second window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StageShows {
    #[default]
    Lyrics,
    /// The song's notation or PDF, as the Sheet music tab picks it.
    Sheet,
}

/// The second window: lyrics or sheet music large, for the stage or a second screen. It keeps
/// its own open score and PDF, so it can show another song than the main window.
pub(crate) struct StageScreen {
    pub size: f32,
    pub chords: bool,
    pub follow: LyricsFollow,
    pub shows: StageShows,
    /// Zoom of the notation.
    pub zoom: f32,
    /// White on black for the PDF.
    pub invert: bool,
    pub notation: Option<crate::notation_view::NotationDoc>,
    pub sheet: Option<crate::sheet_view::SheetDoc>,
}

impl Default for StageScreen {
    fn default() -> Self {
        Self {
            size: 56.0,
            chords: true,
            follow: LyricsFollow::default(),
            shows: StageShows::default(),
            zoom: 1.6,
            invert: false,
            notation: None,
            sheet: None,
        }
    }
}

impl ActCraftApp {
    /// Opens or closes the second window (with the lyrics).
    pub fn toggle_stage_screen(&mut self) {
        self.stage = if self.stage.is_some() { None } else { Some(StageScreen::default()) };
    }

    /// The button that opens the second window with `shows`, switches an open one to it, or
    /// closes it when it already shows that.
    pub(crate) fn stage_button(&mut self, ui: &mut Ui, shows: StageShows) {
        let showing = self.stage.as_ref().is_some_and(|s| s.shows == shows);
        let label = if showing { tr("Close stage window") } else { tr("Stage window") };
        let hint = match shows {
            StageShows::Lyrics => tr("Large lyrics in a window of their own, for a second screen or the stage"),
            StageShows::Sheet => tr("Large sheet music in a window of its own, for a second screen or the stage"),
        };
        if ui.selectable_label(showing, label).on_hover_text(hint).clicked() {
            self.stage = press_stage(self.stage.take(), shows);
        }
    }

    /// Opens the second window with the song's sheet music.
    pub fn show_stage_sheet(&mut self) {
        self.stage = Some(StageScreen { shows: StageShows::Sheet, ..StageScreen::default() });
    }
}

/// The second window after its button for `shows` is pressed: opened showing it, switched to it
/// (keeping its other settings), or closed when it already shows it.
fn press_stage(stage: Option<StageScreen>, shows: StageShows) -> Option<StageScreen> {
    match stage {
        Some(s) if s.shows == shows => None,
        Some(s) => Some(StageScreen { shows, ..s }),
        None => Some(StageScreen { shows, ..StageScreen::default() }),
    }
}

impl ActCraftApp {
    /// Draws the second lyrics window while it's open (its own viewport, so it can be moved to
    /// another screen and made full screen).
    pub(crate) fn stage_window(&mut self, ctx: &egui::Context, e: &Engine) {
        let Some(mut stage) = self.stage.take() else { return };
        let t = self.tokens;
        // With both a PDF and a score, the window shows the one the Sheet music tab shows.
        let source = self.sheet_source;
        let song = e.current().cloned();
        let mut open = true;
        let title = match &song {
            Some(s) if !s.title.is_empty() => format!("ActCraft \u{2014} {}", s.title),
            _ => "ActCraft".to_owned(),
        };
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("actcraft-stage"),
            egui::ViewportBuilder::default().with_title(title).with_inner_size([1024.0, 640.0]),
            |vui, _class| {
                let ctx = vui.ctx().clone();
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bg).inner_margin(t.pad)).show(
                    vui,
                    |ui| {
                        ui.horizontal(|ui| {
                            if let Some(s) = &song {
                                ui.label(RichText::new(&s.title).size(22.0).strong().color(t.text));
                            }
                            ui.separator();
                            ui.selectable_value(&mut stage.shows, StageShows::Lyrics, tr("Lyrics"));
                            ui.selectable_value(&mut stage.shows, StageShows::Sheet, tr("Sheet music"));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(tr("Close")).clicked() {
                                    open = false;
                                }
                                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                                let label = if full { tr("Leave full screen") } else { tr("Full screen") };
                                if ui.button(label).on_hover_text("F11").clicked()
                                    || ctx.input(|i| i.key_pressed(egui::Key::F11))
                                {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                                }
                                ui.spacing_mut().slider_width = 120.0;
                                if stage.shows == StageShows::Lyrics {
                                    ui.add(
                                        egui::Slider::new(&mut stage.size, 24.0..=120.0)
                                            .show_value(false)
                                            .text(tr("Size")),
                                    );
                                    ui.checkbox(&mut stage.chords, tr("Chords"));
                                } else {
                                    // Zoom for notes, inverted colours for a PDF.
                                    match song.as_ref().and_then(|s| sheet_choice(s, source)) {
                                        Some((_, true)) => {
                                            ui.add(
                                                egui::Slider::new(&mut stage.zoom, 0.6..=3.0)
                                                    .show_value(false)
                                                    .text(tr("Zoom")),
                                            );
                                        }
                                        Some((_, false)) => {
                                            ui.checkbox(&mut stage.invert, tr("Invert colours"))
                                                .on_hover_text(tr("White on black, for a dark stage"));
                                        }
                                        None => {}
                                    }
                                }
                            });
                        });
                        ui.separator();
                        match &song {
                            Some(s) if stage.shows == StageShows::Sheet => {
                                stage_sheet(ui, t, e, s, &mut stage, source);
                            }
                            Some(s) if !s.lyrics.trim().is_empty() => {
                                let look = LyricsLook {
                                    size: stage.size,
                                    chords: stage.chords,
                                    follow: true,
                                    salt: "stage-lyrics",
                                };
                                Self::lyrics_display(t, ui, e, s, look, &mut stage.follow);
                            }
                            Some(_) => {
                                ui.label(RichText::new(tr("This song has no lyrics.")).size(28.0).color(t.text_dim));
                            }
                            None => {
                                ui.label(RichText::new(tr("No song loaded")).size(28.0).color(t.text_dim));
                            }
                        }
                    },
                );
                if ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    && !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
                {
                    open = false;
                }
            },
        );
        if open {
            self.stage = Some(stage);
        }
    }
}

/// Which sheet music of a song shows: its score or its PDF (the file and whether it is the
/// score); with both, the one `source` names, as in the Sheet music tab.
fn sheet_choice(song: &Song, source: crate::sheet_view::SheetSource) -> Option<(&str, bool)> {
    use crate::sheet_view::SheetSource;
    match (song.notation_file.as_deref(), song.pdf_file.as_deref()) {
        (Some(score), Some(_)) if source == SheetSource::Notation => Some((score, true)),
        (Some(score), None) => Some((score, true)),
        (_, Some(pdf)) => Some((pdf, false)),
        (None, None) => None,
    }
}

/// The second window's sheet music: the song's notation (whole width, following playback) or
/// its PDF (pages at the window's width, scrolling over the song).
fn stage_sheet(
    ui: &mut Ui,
    t: Tokens,
    e: &Engine,
    song: &Song,
    stage: &mut StageScreen,
    source: crate::sheet_view::SheetSource,
) {
    let status = (e.status.time, e.status.playing);
    let notice = |ui: &mut Ui, text: String| {
        ui.label(RichText::new(text).size(28.0).color(t.text_dim));
    };
    let Some((path, notation)) = sheet_choice(song, source) else {
        notice(ui, tr("This song has no sheet music.").to_owned());
        return;
    };
    if notation {
        if let Err(why) = crate::notation_view::load_notation_into(&mut stage.notation, e, path) {
            notice(ui, format!("{} {why}", tr("The notation could not be opened:")));
            return;
        }
        let Some(doc) = stage.notation.as_ref() else { return };
        let look = crate::notation_view::NotationLook { zoom: stage.zoom, ..Default::default() };
        let view = crate::notation_view::ScoreView { look, follow: true, keyboard: false, salt: "stage-notation" };
        crate::notation_view::score_canvas(ui, t, doc, song, status, view);
    } else {
        if let Err(err) = crate::sheet_view::load_pdf_into(&mut stage.sheet, ui.ctx(), e, Some(path)) {
            notice(ui, format!("{} {err}", tr("The PDF could not be opened:")));
            return;
        }
        let Some(doc) = stage.sheet.as_mut() else { return };
        let look = crate::sheet_view::SheetLook { zoom: 1.0, invert: stage.invert, follow: true };
        crate::sheet_view::pdf_pages(ui, doc, look, song, status, "stage-sheet", None);
    }
}

/// Last frame's lyrics layout, for smooth and constant-speed scrolling.
#[derive(Debug, Default, Clone)]
pub(crate) struct LyricsGeometry {
    pub song: u64,
    /// Each line's vertical centre from the top of the lyrics.
    pub centres: Vec<f32>,
    pub content: f32,
    pub view: f32,
    pub offset: f32,
}

/// Where the top of the view should be for `mode` at `time`, or `None` when this mode doesn't
/// steer the view (jump to the line, off, or nothing to follow yet). `times` are the lines'
/// timecodes and `centres` their vertical centres; `span` is the song's (start, end) in seconds.
pub(crate) fn follow_target(
    mode: LyricsScroll,
    time: f64,
    times: &[Option<f64>],
    centres: &[f32],
    content: f32,
    view: f32,
    span: (f64, f64),
) -> Option<f32> {
    let max = (content - view).max(0.0);
    match mode {
        LyricsScroll::Line | LyricsScroll::Off => None,
        LyricsScroll::Constant => {
            let (start, end) = span;
            if end <= start {
                return None;
            }
            let frac = ((time - start) / (end - start)).clamp(0.0, 1.0) as f32;
            Some(frac * max)
        }
        LyricsScroll::Smooth => {
            let timed: Vec<(f64, f32)> = times.iter().zip(centres).filter_map(|(t, &c)| t.map(|t| (t, c))).collect();
            let i = timed.iter().rposition(|&(t, _)| t <= time)?;
            let (t0, y0) = *timed.get(i)?;
            let y = match timed.iter().skip(i + 1).find(|&&(t, _)| t > t0) {
                Some(&(t1, y1)) => y0 + (y1 - y0) * ((time - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32,
                None => y0,
            };
            Some((y - view / 2.0).clamp(0.0, max))
        }
    }
}

fn family(font: Font) -> FontFamily {
    // Serif and script need font files, which this repository doesn't ship; they show in the
    // standard face until fonts arrive from the shared font store.
    match font {
        Font::Mono => FontFamily::Monospace,
        Font::Sans | Font::Serif | Font::Script => FontFamily::Proportional,
    }
}

fn format_for(span: &Span, size: f32, colour: Color32, strong: bool) -> TextFormat {
    let c = span.style.colour.map_or(colour, |[r, g, b]| Color32::from_rgb(r, g, b));
    let mut f = TextFormat {
        font_id: FontId::new(if span.style.bold || strong { size * 1.04 } else { size }, family(span.style.font)),
        color: if span.style.bold || strong { c } else { c.gamma_multiply(0.95) },
        italics: span.style.italic || span.style.font == Font::Script,
        ..TextFormat::default()
    };
    if span.style.underline {
        f.underline = Stroke::new(1.5, c);
    }
    f
}

/// One line of styled text without chords.
fn layout(spans: &[Span], size: f32, colour: Color32, strong: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for s in spans {
        job.append(&s.text, 0.0, format_for(s, size, colour, strong));
    }
    job
}

/// A line with each chord above the syllable it starts on.
fn chord_line(ui: &mut Ui, spans: &[Span], size: f32, colour: Color32, chord_colour: Color32, shift: i32, flats: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for s in spans {
            ui.vertical(|ui| {
                let chord = s.chord.as_deref().map(|c| lyrics::transpose_chord(c, shift, flats)).unwrap_or_default();
                ui.label(
                    RichText::new(if chord.is_empty() { " ".to_owned() } else { format!("{chord} ") })
                        .size(size * 0.75)
                        .strong()
                        .color(chord_colour),
                );
                let text = if s.text.is_empty() { " ".to_owned() } else { s.text.clone() };
                let one = Span { text, ..s.clone() };
                ui.label(layout(std::slice::from_ref(&one), size, colour, false));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_wraps_the_selection() {
        let mut e = LyricsEdit { song: 1, text: "The road is long".into(), selection: (4, 8), ..LyricsEdit::default() };
        wrap_selection(&mut e, "**", "**");
        assert_eq!(e.text, "The **road** is long");
        let mut h = LyricsEdit { song: 1, text: "one\n## two".into(), selection: (6, 6), ..LyricsEdit::default() };
        prefix_line(&mut h, "# ");
        assert_eq!(h.text, "one\n# two");
        assert_eq!(cursor_line(&h), 1);
    }

    #[test]
    fn find_next_wraps_and_replace_moves_on() {
        let mut e = LyricsEdit {
            song: 1,
            text: "road, Road, road".into(),
            find: "road".into(),
            replace_with: "way".into(),
            ..LyricsEdit::default()
        };
        assert_eq!(next_match(&e), Some((0, 4)));
        e.selection = (0, 4);
        assert_eq!(next_match(&e), Some((6, 10)));
        e.selection = (16, 16);
        assert_eq!(next_match(&e), Some((0, 4)), "wraps round to the first");
        // The selection isn't a match: replace only finds the next one.
        e.selection = (1, 2);
        replace_current(&mut e);
        assert_eq!(e.text, "road, Road, road");
        assert_eq!(e.selection, (6, 10));
        replace_current(&mut e);
        assert_eq!(e.text, "road, way, road");
        assert_eq!(e.selection, (11, 15));
        e.match_case = true;
        e.find = "Road".into();
        assert_eq!(next_match(&e), None);
    }

    #[test]
    fn smooth_and_constant_scrolling_follow_the_time() {
        // Lines at 0, 10 and 20 s, 100 points apart; the view is 200 points of 1000.
        let times = [None, Some(0.0), None, Some(10.0), Some(20.0)];
        let centres = [10.0, 100.0, 150.0, 200.0, 300.0];
        let smooth = |t| follow_target(LyricsScroll::Smooth, t, &times, &centres, 1000.0, 200.0, (0.0, 100.0));
        // Halfway between the lines at 0 s (y 100) and 10 s (y 200): y 150, view top 50.
        assert_eq!(smooth(5.0), Some(50.0));
        assert_eq!(smooth(10.0), Some(100.0));
        // After the last timecode it stays on that line; before the first there's nothing to follow.
        assert_eq!(smooth(99.0), Some(200.0));
        assert_eq!(smooth(-1.0), None);
        let constant = |t| follow_target(LyricsScroll::Constant, t, &times, &centres, 1000.0, 200.0, (10.0, 90.0));
        assert_eq!(constant(10.0), Some(0.0));
        assert_eq!(constant(50.0), Some(400.0));
        assert_eq!(constant(500.0), Some(800.0));
        assert_eq!(follow_target(LyricsScroll::Constant, 5.0, &times, &centres, 1000.0, 200.0, (5.0, 5.0)), None);
        assert_eq!(follow_target(LyricsScroll::Line, 5.0, &times, &centres, 1000.0, 200.0, (0.0, 9.0)), None);
        assert_eq!(follow_target(LyricsScroll::Off, 5.0, &times, &centres, 1000.0, 200.0, (0.0, 9.0)), None);
    }

    #[test]
    fn the_second_window_shows_the_sheet_music_the_tab_shows() {
        use crate::sheet_view::SheetSource;
        let mut song = Song::default();
        assert_eq!(sheet_choice(&song, SheetSource::Notation), None);
        song.pdf_file = Some("/a.pdf".into());
        assert_eq!(sheet_choice(&song, SheetSource::Notation), Some(("/a.pdf", false)), "only a PDF");
        song.notation_file = Some("/a.mxl".into());
        assert_eq!(sheet_choice(&song, SheetSource::Notation), Some(("/a.mxl", true)));
        assert_eq!(sheet_choice(&song, SheetSource::Pdf), Some(("/a.pdf", false)), "both: as the tab picks");
        song.pdf_file = None;
        assert_eq!(sheet_choice(&song, SheetSource::Pdf), Some(("/a.mxl", true)), "only a score");
    }

    #[test]
    fn the_stage_buttons_open_switch_and_close_the_second_window() {
        let shows = |s: &Option<StageScreen>| s.as_ref().map(|s| s.shows);
        let opened = press_stage(None, StageShows::Sheet);
        assert_eq!(shows(&opened), Some(StageShows::Sheet));
        // The lyrics button switches the open window and keeps its settings.
        let mut big = opened;
        if let Some(s) = big.as_mut() {
            s.size = 99.0;
        }
        let switched = press_stage(big, StageShows::Lyrics);
        assert_eq!(shows(&switched), Some(StageShows::Lyrics));
        assert_eq!(switched.as_ref().map(|s| s.size), Some(99.0));
        // Pressed again it closes.
        assert!(press_stage(switched, StageShows::Lyrics).is_none());
    }

    #[test]
    fn the_lyrics_window_opens_and_closes_with_its_own_settings() {
        let ctx = egui::Context::default();
        let engine = Engine::new(Box::new(actcraft_engine::MemoryServices::default()));
        let mut app = ActCraftApp::new(&ctx, std::sync::Arc::new(std::sync::Mutex::new(engine)), Default::default());
        assert!(app.stage.is_none());
        app.toggle_stage_screen();
        assert!(app.stage.as_ref().is_some_and(|s| s.chords && s.size > app.lyrics_size), "larger than the tab's text");
        app.toggle_stage_screen();
        assert!(app.stage.is_none());
    }
}
