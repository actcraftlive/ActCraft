//! The Lyrics tab: lyrics with chords above the words, following the playback time, and an
//! editor with formatting buttons.

use actcraft_engine::Engine;
use actcraft_engine::lyrics::{self, Font, LineKind, Span};
use actcraft_show::Song;
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, RichText, Stroke, TextFormat, Ui};
use serde_json::json;

use crate::flows::{FileAsk, FileKind, FileRequest};
use crate::{ActCraftApp, tr};

/// Lyrics file types the import picker offers.
pub const LYRICS_EXTENSIONS: &[&str] = &["txt", "cho", "chopro", "chordpro", "crd", "pro", "lrc"];

/// Editing state of the lyrics editor.
pub(crate) struct LyricsEdit {
    pub song: u64,
    pub text: String,
    /// Last known cursor/selection as character indexes (start, end).
    pub selection: (usize, usize),
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
            if ui.selectable_label(!editing, tr("Show")).clicked() && editing {
                self.finish_lyrics_edit(e);
            }
            if ui.selectable_label(editing, tr("Edit")).clicked() && !editing {
                self.lyrics_edit = Some(LyricsEdit { song: song.id, text: song.lyrics.clone(), selection: (0, 0) });
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
            ui.checkbox(&mut self.lyrics_follow, tr("Follow playback"));
            ui.add(egui::Slider::new(&mut self.lyrics_size, 14.0..=64.0).show_value(false).text(tr("Size")));
            ui.separator();
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
        self.lyrics_display(ui, e, song);
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

    fn lyrics_display(&mut self, ui: &mut Ui, e: &Engine, song: &Song) {
        let t = self.tokens;
        let parsed = lyrics::parse(&song.lyrics);
        let flats = lyrics::chords::prefers_flats(&song.key);
        let shift = i32::from(song.transpose) - i32::from(song.capo);
        let current = if e.status.playing || e.status.time > 0.0 { parsed.line_at(e.status.time) } else { None };
        let size = self.lyrics_size;
        let heading_colours = [t.accent, t.warning, t.playing];
        egui::ScrollArea::vertical().id_salt("lyrics").auto_shrink([false, false]).show(ui, |ui| {
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
                                if self.lyrics_show_chords && line.has_chords() {
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
                if is_current && self.lyrics_follow && self.lyrics_last_line != Some(i) {
                    r.response.scroll_to_me(Some(egui::Align::Center));
                    self.lyrics_last_line = Some(i);
                }
            }
        });
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
        let mut e = LyricsEdit { song: 1, text: "The road is long".into(), selection: (4, 8) };
        wrap_selection(&mut e, "**", "**");
        assert_eq!(e.text, "The **road** is long");
        let mut h = LyricsEdit { song: 1, text: "one\n## two".into(), selection: (6, 6) };
        prefix_line(&mut h, "# ");
        assert_eq!(h.text, "one\n# two");
        assert_eq!(cursor_line(&h), 1);
    }
}
