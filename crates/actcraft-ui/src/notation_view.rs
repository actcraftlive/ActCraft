//! Notation in the Sheet music tab: a song's MusicXML score drawn as staves and tablature, bar
//! after bar in lines across the tab. The playing bar is marked and the view follows playback;
//! clicking a bar plays from there. Song bar 1 (the beat grid's first bar) is the score's bar 1.

use actcraft_engine::Engine;
use actcraft_engine::notation::{Bar, Clef, ClefKind, Note, NoteValue, Part, Pitch, Score, TICKS_PER_QUARTER};
use actcraft_show::Song;
use egui::{Align2, Color32, FontId, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::{ActCraftApp, tr};

/// The score file types the picker offers.
pub const NOTATION_EXTENSIONS: &[&str] = actcraft_engine::notation::EXTENSIONS;

/// An open score.
pub(crate) struct NotationDoc {
    pub path: String,
    pub score: Score,
}

/// What a part is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NotationShow {
    /// Staff with tablature under it (tab only for parts written as tab).
    #[default]
    Both,
    Notes,
    Tab,
}

/// How the notation is shown.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NotationLook {
    pub part: usize,
    pub show: NotationShow,
    pub zoom: f32,
}

impl Default for NotationLook {
    fn default() -> Self {
        Self { part: 0, show: NotationShow::Both, zoom: 1.0 }
    }
}

/// Sizes in points for one zoom.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    /// Gap between staff lines.
    space: f32,
    /// Gap between tablature lines.
    tab_gap: f32,
}

impl Metrics {
    fn new(zoom: f32) -> Self {
        let zoom = zoom.clamp(0.5, 3.0);
        Self { space: 9.0 * zoom, tab_gap: 14.0 * zoom }
    }
}

/// What one line of music holds, top to bottom.
#[derive(Debug, Clone, PartialEq)]
struct Rows {
    /// Clef of each staff drawn.
    staves: Vec<Clef>,
    /// Strings of the tablature, 0 for none.
    strings: usize,
}

impl Rows {
    fn of(part: &Part, show: NotationShow) -> Self {
        let tab = match show {
            NotationShow::Both => part.tablature,
            NotationShow::Tab => true,
            NotationShow::Notes => false,
        };
        let staves = if show == NotationShow::Tab { Vec::new() } else { part.clefs.clone() };
        let staves = if staves.is_empty() && show != NotationShow::Tab { vec![Clef::TREBLE] } else { staves };
        Self { staves, strings: if tab { part.strings().len() } else { 0 } }
    }

    /// Top of each staff's top line, the tablature's first line, and the line's whole height.
    fn layout(&self, m: Metrics) -> (Vec<f32>, f32, f32) {
        let mut y = 3.0 * m.space; // bar numbers and tempo marks
        let mut tops = Vec::new();
        for _ in &self.staves {
            y += 3.0 * m.space; // ledger lines above
            tops.push(y);
            y += 4.0 * m.space + 3.5 * m.space; // the staff, ledger lines below
        }
        let tab_top = y + m.tab_gap;
        if self.strings > 0 {
            y = tab_top + (self.strings.saturating_sub(1)) as f32 * m.tab_gap + m.tab_gap;
        }
        (tops, tab_top, y + 2.0 * m.space)
    }
}

/// A line of bars: which bars and how wide each is drawn.
#[derive(Debug, Clone, PartialEq)]
struct System {
    first: usize,
    widths: Vec<f32>,
    /// Width of the clef, key and time at the line's start.
    header: f32,
}

/// The narrowest a bar reads well: enough room for its closest notes.
fn bar_width(bar: &Bar, m: Metrics) -> f32 {
    let unit = 2.6 * m.space;
    let mut onsets: Vec<u32> = bar.notes.iter().map(|n| n.start).collect();
    onsets.sort_unstable();
    onsets.dedup();
    let mut closest = bar.length.max(1);
    let mut last = 0;
    for &o in onsets.iter().chain(std::iter::once(&bar.length)) {
        if o > last {
            closest = closest.min(o - last);
        }
        last = o;
    }
    let closest = closest.max(TICKS_PER_QUARTER / 8);
    let slots = bar.length.max(1) as f32 / closest as f32;
    (slots * unit).clamp(4.0 * unit, 40.0 * unit) + 2.0 * m.space
}

fn header_width(rows: &Rows, bar: &Bar, with_time: bool, m: Metrics) -> f32 {
    let mut w = 1.0 * m.space;
    if !rows.staves.is_empty() {
        w += 3.8 * m.space + f32::from(bar.fifths.unsigned_abs()) * 1.1 * m.space;
        if with_time {
            w += 2.6 * m.space;
        }
    } else {
        w += 2.0 * m.space; // "TAB"
    }
    w
}

/// Breaks the bars into lines `width` wide; every line but a short last one is stretched to fill.
fn break_lines(part: &Part, rows: &Rows, width: f32, m: Metrics) -> Vec<System> {
    let mut systems: Vec<System> = Vec::new();
    let mut i = 0;
    while i < part.bars.len() {
        let Some(first) = part.bars.get(i) else { break };
        let time_changes =
            i == 0 || part.bars.get(i - 1).is_some_and(|p| (p.beats, p.beat_type) != (first.beats, first.beat_type));
        let header = header_width(rows, first, time_changes, m);
        let mut widths = Vec::new();
        let mut used = header;
        while let Some(bar) = part.bars.get(i + widths.len()) {
            let w = bar_width(bar, m);
            if !widths.is_empty() && used + w > width {
                break;
            }
            used += w;
            widths.push(w);
        }
        let natural: f32 = widths.iter().sum();
        let room = (width - header).max(natural);
        let last = i + widths.len() >= part.bars.len();
        if natural > 0.0 && !(last && natural < room * 0.6) {
            let k = room / natural;
            for w in &mut widths {
                *w *= k;
            }
        }
        let n = widths.len().max(1);
        systems.push(System { first: i, widths, header });
        i += n;
    }
    systems
}

/// Alterations the key signature gives each letter (C = 0 .. B = 6).
fn key_alters(fifths: i8) -> [i8; 7] {
    const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
    const FLATS: [usize; 7] = [6, 2, 5, 1, 4, 0, 3];
    let mut out = [0; 7];
    let n = usize::from(fifths.unsigned_abs()).min(7);
    let (order, alter) = if fifths >= 0 { (&SHARPS, 1) } else { (&FLATS, -1) };
    for &step in order.iter().take(n) {
        if let Some(slot) = out.get_mut(step) {
            *slot = alter;
        }
    }
    out
}

/// Staff positions of the key signature's sharps or flats for a clef.
fn key_positions(clef: Clef, fifths: i8) -> Vec<i32> {
    const SHARPS: [i32; 7] = [8, 5, 9, 6, 3, 7, 4];
    const FLATS: [i32; 7] = [4, 7, 3, 6, 2, 5, 1];
    let e4 = Pitch { step: 2, alter: 0, octave: 4 };
    let mut shift = (clef.position(e4)).rem_euclid(7);
    if shift > 3 {
        shift -= 7;
    }
    let order = if fifths >= 0 { &SHARPS } else { &FLATS };
    order.iter().take(usize::from(fifths.unsigned_abs()).min(7)).map(|p| p + shift).collect()
}

/// Which bar plays at `time` and how far into it (0..1), on the song's beat grid.
fn playing_bar(song: &Song, time: f64, bars: usize) -> Option<(usize, f32)> {
    let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
    let beat = clock.beat_at(time);
    if !beat.is_finite() || beat < 0.0 {
        return None;
    }
    let per_bar = f64::from(song.beats_per_bar.max(1));
    let bar = (beat / per_bar).floor();
    let index = bar as usize;
    (bar < bars as f64).then_some((index, ((beat - bar * per_bar) / per_bar) as f32))
}

/// The time bar `index` starts at, on the song's beat grid.
fn bar_time(song: &Song, index: usize) -> f64 {
    let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
    clock.time_of_beat(index as f64 * f64::from(song.beats_per_bar.max(1)))
}

/// Accidentals in force in one bar on one staff: the key's, then those written so far.
struct Accidentals {
    key: [i8; 7],
    /// Diatonic position and alteration, in the order written.
    written: Vec<(i32, i8)>,
}

/// Colours of the drawing.
#[derive(Clone, Copy)]
struct Ink {
    paper: Color32,
    ink: Color32,
    line: Color32,
    mark: Color32,
}

/// Draws one line of music with its top left at `origin`.
struct Painter<'a> {
    p: &'a egui::Painter,
    m: Metrics,
    ink: Ink,
}

impl Painter<'_> {
    fn y(&self, top: f32, pos: i32) -> f32 {
        // Position 8 is the top line, 0 the bottom one.
        top + (8 - pos) as f32 * self.m.space / 2.0
    }

    fn staff_lines(&self, x0: f32, x1: f32, top: f32) {
        for k in 0..5 {
            let y = top + k as f32 * self.m.space;
            self.p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0, self.ink.line));
        }
    }

    fn clef(&self, x: f32, top: f32, clef: Clef) {
        let s = self.m.space;
        let stroke = Stroke::new(1.6, self.ink.ink);
        match clef.kind {
            ClefKind::G => {
                let line = i32::from(clef.line.clamp(1, 5)) * 2 - 2;
                let c = pos2(x + 0.9 * s, self.y(top, line));
                self.p.circle_stroke(c, 0.85 * s, stroke);
                self.p.line_segment(
                    [pos2(c.x + 0.3 * s, self.y(top, line - 3)), pos2(c.x - 0.1 * s, self.y(top, 11))],
                    stroke,
                );
                self.p.circle_filled(pos2(c.x - 0.2 * s, self.y(top, line - 4)), 0.3 * s, self.ink.ink);
            }
            ClefKind::F => {
                let line = i32::from(clef.line.clamp(1, 5)) * 2 - 2;
                let c = pos2(x + 0.6 * s, self.y(top, line));
                self.p.circle_filled(c, 0.35 * s, self.ink.ink);
                let points = vec![
                    c,
                    pos2(c.x + 1.6 * s, c.y - 0.6 * s),
                    pos2(c.x + 1.6 * s, c.y + 1.4 * s),
                    pos2(c.x - 0.2 * s, c.y + 2.8 * s),
                ];
                self.p.add(Shape::line(points, stroke));
                for d in [-0.5, 0.5] {
                    self.p.circle_filled(pos2(c.x + 2.2 * s, c.y + d * s), 0.18 * s, self.ink.ink);
                }
            }
            ClefKind::C => {
                let line = i32::from(clef.line.clamp(1, 5)) * 2 - 2;
                let (y0, y1) = (top, top + 4.0 * s);
                self.p.line_segment([pos2(x + 0.3 * s, y0), pos2(x + 0.3 * s, y1)], Stroke::new(2.5, self.ink.ink));
                self.p.line_segment([pos2(x + 0.8 * s, y0), pos2(x + 0.8 * s, y1)], stroke);
                let mid = self.y(top, line);
                for dir in [-1.0, 1.0] {
                    let pts = vec![
                        pos2(x + 0.8 * s, mid),
                        pos2(x + 1.8 * s, mid + dir * 0.6 * s),
                        pos2(x + 1.6 * s, mid + dir * 1.8 * s),
                    ];
                    self.p.add(Shape::line(pts, stroke));
                }
            }
        }
        if clef.octave_change != 0 {
            let y = if clef.octave_change < 0 { top + 5.2 * s } else { top - 1.4 * s };
            self.p.text(pos2(x + 0.9 * s, y), Align2::CENTER_CENTER, "8", FontId::proportional(1.1 * s), self.ink.ink);
        }
    }

    fn accidental(&self, x: f32, y: f32, alter: i8) {
        let s = self.m.space;
        let font = FontId::proportional(1.9 * s);
        match alter {
            0 => {
                // A natural sign, drawn.
                let stroke = Stroke::new(1.2, self.ink.ink);
                self.p.line_segment([pos2(x - 0.25 * s, y - 1.1 * s), pos2(x - 0.25 * s, y + 0.5 * s)], stroke);
                self.p.line_segment([pos2(x + 0.25 * s, y - 0.5 * s), pos2(x + 0.25 * s, y + 1.1 * s)], stroke);
                self.p.line_segment(
                    [pos2(x - 0.25 * s, y - 0.2 * s), pos2(x + 0.25 * s, y - 0.4 * s)],
                    Stroke::new(2.0, self.ink.ink),
                );
                self.p.line_segment(
                    [pos2(x - 0.25 * s, y + 0.4 * s), pos2(x + 0.25 * s, y + 0.2 * s)],
                    Stroke::new(2.0, self.ink.ink),
                );
            }
            a if a > 0 => {
                let text = if a > 1 { "x" } else { "#" };
                self.p.text(pos2(x, y), Align2::CENTER_CENTER, text, font, self.ink.ink);
            }
            a => {
                let text = if a < -1 { "bb" } else { "b" };
                self.p.text(pos2(x, y - 0.3 * s), Align2::CENTER_CENTER, text, font, self.ink.ink);
            }
        }
    }

    fn time_signature(&self, x: f32, top: f32, bar: &Bar) {
        let s = self.m.space;
        let font = FontId::proportional(2.4 * s);
        self.p.text(pos2(x, top + 1.0 * s), Align2::CENTER_CENTER, bar.beats.to_string(), font.clone(), self.ink.ink);
        self.p.text(pos2(x, top + 3.0 * s), Align2::CENTER_CENTER, bar.beat_type.to_string(), font, self.ink.ink);
    }

    fn rest(&self, x: f32, top: f32, value: NoteValue) {
        let s = self.m.space;
        let ink = self.ink.ink;
        match value {
            NoteValue::Whole => {
                self.p.rect_filled(Rect::from_min_size(pos2(x - 0.6 * s, top + s), vec2(1.2 * s, 0.5 * s)), 0.0, ink);
            }
            NoteValue::Half => {
                self.p.rect_filled(
                    Rect::from_min_size(pos2(x - 0.6 * s, top + 1.5 * s), vec2(1.2 * s, 0.5 * s)),
                    0.0,
                    ink,
                );
            }
            NoteValue::Quarter => {
                let pts = vec![
                    pos2(x - 0.3 * s, top + 0.6 * s),
                    pos2(x + 0.4 * s, top + 1.5 * s),
                    pos2(x - 0.3 * s, top + 2.3 * s),
                    pos2(x + 0.4 * s, top + 3.0 * s),
                    pos2(x - 0.2 * s, top + 3.4 * s),
                ];
                self.p.add(Shape::line(pts, Stroke::new(2.0, ink)));
            }
            v => {
                let flags = v.flags();
                let stroke = Stroke::new(1.5, ink);
                let bottom = top + 3.2 * s;
                let top_y = top + 1.2 * s - f32::from(flags.saturating_sub(2)) * 0.8 * s;
                self.p.line_segment([pos2(x + 0.5 * s, top_y), pos2(x - 0.2 * s, bottom)], stroke);
                for k in 0..flags {
                    let y = top_y + f32::from(k) * 0.8 * s;
                    self.p.circle_filled(pos2(x - 0.3 * s, y + 0.2 * s), 0.25 * s, ink);
                    self.p.line_segment([pos2(x - 0.3 * s, y + 0.3 * s), pos2(x + 0.5 * s, y)], stroke);
                }
            }
        }
    }

    /// Notes struck together on one staff in one voice; `up` points the stem up.
    fn chord(&self, x: f32, top: f32, clef: Clef, notes: &[&Note], up: bool, acc: &mut Accidentals) {
        let s = self.m.space;
        let (rx, ry) = (0.62 * s, 0.45 * s);
        let mut heads: Vec<(i32, &Note, Pitch)> =
            notes.iter().filter_map(|n| n.pitch.map(|p| (clef.position(p), *n, p))).collect();
        if heads.is_empty() {
            if let Some(n) = notes.first() {
                self.rest(x, top, n.value);
            }
            return;
        }
        heads.sort_by_key(|h| h.0);
        let value = heads.first().map_or(NoteValue::Quarter, |h| h.1.value);
        let dots = heads.first().map_or(0, |h| h.1.dots);
        let mut previous: Option<(i32, bool)> = None;
        for &(pos, note, pitch) in &heads {
            // A second above the last head goes to the other side of the stem.
            let shifted = previous.is_some_and(|(p, sh)| pos - p == 1 && !sh);
            previous = Some((pos, shifted));
            let side = if up { 1.0 } else { -1.0 };
            let hx = if shifted { x + side * 2.0 * rx * 0.95 } else { x };
            let y = self.y(top, pos);
            // Ledger lines.
            let ledger = Stroke::new(1.0, self.ink.ink);
            let mut lp = -2;
            while lp >= pos {
                let ly = self.y(top, lp);
                self.p.line_segment([pos2(hx - 1.6 * rx, ly), pos2(hx + 1.6 * rx, ly)], ledger);
                lp -= 2;
            }
            let mut lp = 10;
            while lp <= pos {
                let ly = self.y(top, lp);
                self.p.line_segment([pos2(hx - 1.6 * rx, ly), pos2(hx + 1.6 * rx, ly)], ledger);
                lp += 2;
            }
            // Accidental when the note differs from the key or an earlier note in the bar.
            let diatonic = pitch.diatonic();
            let current = acc
                .written
                .iter()
                .rev()
                .find(|(d, _)| *d == diatonic)
                .map_or_else(|| acc.key.get(usize::from(pitch.step)).copied().unwrap_or(0), |(_, a)| *a);
            if pitch.alter != current && !note.tied {
                self.accidental(x - 2.6 * rx, y, pitch.alter);
                acc.written.push((diatonic, pitch.alter));
            }
            let open = matches!(value, NoteValue::Whole | NoteValue::Half);
            let shape = if open {
                Shape::ellipse_stroke(pos2(hx, y), vec2(rx, ry), Stroke::new(1.6, self.ink.ink))
            } else {
                Shape::ellipse_filled(pos2(hx, y), vec2(rx, ry), self.ink.ink)
            };
            self.p.add(shape);
            for d in 0..dots {
                let dy = if pos.rem_euclid(2) == 0 { -0.5 * s } else { 0.0 };
                self.p.circle_filled(pos2(x + 1.8 * rx + f32::from(d) * 0.6 * s, y + dy), 0.17 * s, self.ink.ink);
            }
        }
        if value == NoteValue::Whole {
            return;
        }
        let (Some(low), Some(high)) = (heads.first(), heads.last()) else { return };
        let stroke = Stroke::new(1.3, self.ink.ink);
        let (sx, from, to) = if up {
            (x + rx * 0.95, self.y(top, low.0), self.y(top, high.0) - 3.4 * s)
        } else {
            (x - rx * 0.95, self.y(top, high.0), self.y(top, low.0) + 3.4 * s)
        };
        self.p.line_segment([pos2(sx, from), pos2(sx, to)], stroke);
        let dir = if up { 1.0 } else { -1.0 };
        for k in 0..value.flags() {
            let y = to + dir * f32::from(k) * 0.8 * s;
            self.p.line_segment([pos2(sx, y), pos2(sx + 0.9 * s, y + dir * 1.3 * s)], Stroke::new(1.8, self.ink.ink));
        }
    }
}

impl ActCraftApp {
    /// The notation of the Sheet music tab: options, then the score.
    pub(crate) fn notation_view(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        let Some(path) = song.notation_file.clone() else { return };
        if self.notation.as_ref().map(|d| d.path.as_str()) != Some(path.as_str()) {
            self.notation = None;
            match actcraft_engine::notation_commands::load_score(e, &path) {
                Ok(score) => {
                    self.notation = Some(NotationDoc { path: path.clone(), score });
                    self.notation_look.part = 0;
                }
                Err(err) => {
                    let why = tr(err.message()).to_owned();
                    ui.label(
                        egui::RichText::new(format!("{} {why}", tr("The notation could not be opened:")))
                            .color(t.danger),
                    );
                    return;
                }
            }
        }
        let Some(doc) = self.notation.as_ref() else { return };
        let look = &mut self.notation_look;
        look.part = look.part.min(doc.score.parts.len().saturating_sub(1));
        ui.horizontal_wrapped(|ui| {
            if !doc.score.title.is_empty() {
                ui.label(egui::RichText::new(&doc.score.title).strong());
                ui.separator();
            }
            let current = doc.score.parts.get(look.part).map(|p| p.name.as_str()).unwrap_or_default();
            // Labels go before their lists, and name them for screen readers.
            let label = ui.label(tr("Instrument"));
            egui::ComboBox::from_id_salt("notation-part")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, p) in doc.score.parts.iter().enumerate() {
                        ui.selectable_value(&mut look.part, i, &p.name);
                    }
                })
                .response
                .labelled_by(label.id);
            let label = ui.label(tr("Display"));
            egui::ComboBox::from_id_salt("notation-show")
                .selected_text(match look.show {
                    NotationShow::Both => tr("Notes and tab"),
                    NotationShow::Notes => tr("Notes"),
                    NotationShow::Tab => tr("Tab"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut look.show, NotationShow::Both, tr("Notes and tab"));
                    ui.selectable_value(&mut look.show, NotationShow::Notes, tr("Notes"));
                    ui.selectable_value(&mut look.show, NotationShow::Tab, tr("Tab"));
                })
                .response
                .labelled_by(label.id);
            ui.spacing_mut().slider_width = 120.0;
            ui.add(egui::Slider::new(&mut look.zoom, 0.6..=2.5).show_value(false).text(tr("Zoom")));
            ui.checkbox(&mut self.sheet_view.follow, tr("Follow playback"))
                .on_hover_text(tr("Keep the playing bar in view while the song plays"));
        });
        ui.separator();
        let Some(part) = doc.score.parts.get(look.part) else { return };
        if part.bars.is_empty() {
            return;
        }
        let look = *look;
        let m = Metrics::new(look.zoom);
        let rows = Rows::of(part, look.show);
        let (tops, tab_top, line_h) = rows.layout(m);
        let margin = 8.0;
        let width = (ui.available_width() - 2.0 * margin).max(200.0);
        let systems = break_lines(part, &rows, width, m);
        let content = systems.len() as f32 * line_h + 2.0 * margin;
        let playing = playing_bar(song, e.status.time, part.bars.len());
        let ink = Ink { paper: t.surface, ink: t.text, line: t.text_dim, mark: t.accent };
        let mut area = egui::ScrollArea::vertical().id_salt(("notation", song.id)).auto_shrink([false, false]);
        if self.sheet_view.follow && e.status.playing {
            if let Some((bar, _)) = playing
                && let Some(line) = systems.iter().position(|s| bar >= s.first && bar < s.first + s.widths.len())
            {
                area = area.vertical_scroll_offset((line as f32 * line_h - line_h * 0.3).max(0.0));
            }
            ui.ctx().request_repaint();
        }
        let mut seek: Option<usize> = None;
        let bar_word = tr("bar");
        let name = format!(
            "{} {}, {}",
            tr("Notation:"),
            part.name,
            match playing {
                Some((b, _)) => format!("{bar_word} {} / {}", b + 1, part.bars.len()),
                None => format!("{} {}", part.bars.len(), tr("bars")),
            }
        );
        area.show_viewport(ui, |ui, viewport| {
            let (rect, response) = ui.allocate_exact_size(vec2(width + 2.0 * margin, content), Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, &name));
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, ink.paper);
            let pa = Painter { p: &painter, m, ink };
            for (li, sys) in systems.iter().enumerate() {
                let y0 = rect.top() + margin + li as f32 * line_h;
                let local = Rect::from_min_size(pos2(0.0, y0 - rect.top()), vec2(width, line_h));
                if !local.intersects(viewport) {
                    continue;
                }
                let x0 = rect.left() + margin;
                let x_end = x0 + sys.header + sys.widths.iter().sum::<f32>();
                // Bar positions.
                let mut xs = Vec::with_capacity(sys.widths.len() + 1);
                let mut x = x0 + sys.header;
                for w in &sys.widths {
                    xs.push(x);
                    x += w;
                }
                xs.push(x);
                // The playing bar, under everything else.
                if let Some((pb, frac)) = playing
                    && pb >= sys.first
                    && let (Some(&bx), Some(&bx1)) = (xs.get(pb - sys.first), xs.get(pb - sys.first + 1))
                {
                    let r = Rect::from_min_max(pos2(bx, y0 + 2.0 * m.space), pos2(bx1, y0 + line_h - m.space));
                    painter.rect_filled(r, 2.0, ink.mark.gamma_multiply(0.18));
                    let cx = bx + (bx1 - bx) * frac.clamp(0.0, 1.0);
                    painter.line_segment([pos2(cx, r.top()), pos2(cx, r.bottom())], Stroke::new(2.0, ink.mark));
                }
                let first_bar = part.bars.get(sys.first);
                // Staves: lines, clef, key, time.
                for (si, clef) in rows.staves.iter().enumerate() {
                    let Some(&top) = tops.get(si) else { continue };
                    let top = y0 + top;
                    pa.staff_lines(x0, x_end, top);
                    pa.clef(x0 + 0.2 * m.space, top, *clef);
                    if let Some(bar) = first_bar {
                        let mut kx = x0 + 4.0 * m.space;
                        for pos in key_positions(*clef, bar.fifths) {
                            pa.accidental(kx, pa.y(top, pos), if bar.fifths > 0 { 1 } else { -1 });
                            kx += 1.1 * m.space;
                        }
                        let time_changes = sys.first == 0
                            || part
                                .bars
                                .get(sys.first - 1)
                                .is_some_and(|p| (p.beats, p.beat_type) != (bar.beats, bar.beat_type));
                        if time_changes {
                            pa.time_signature(kx + 0.9 * m.space, top, bar);
                        }
                    }
                }
                // Tablature lines.
                let tab_y = y0 + tab_top;
                for k in 0..rows.strings {
                    let y = tab_y + k as f32 * m.tab_gap;
                    painter.line_segment([pos2(x0, y), pos2(x_end, y)], Stroke::new(1.0, ink.line));
                }
                if rows.strings > 0 {
                    let mid = tab_y + (rows.strings - 1) as f32 * m.tab_gap / 2.0;
                    for (k, letter) in ["T", "A", "B"].iter().enumerate() {
                        let y = mid + (k as f32 - 1.0) * 1.1 * m.tab_gap.min(1.6 * m.space);
                        painter.text(
                            pos2(x0 + 1.0 * m.space, y),
                            Align2::CENTER_CENTER,
                            *letter,
                            FontId::proportional(1.3 * m.space),
                            ink.ink,
                        );
                    }
                }
                // Bars.
                for (k, w) in sys.widths.iter().enumerate() {
                    let index = sys.first + k;
                    let Some(bar) = part.bars.get(index) else { continue };
                    let (Some(&bx), Some(&bx1)) = (xs.get(k), xs.get(k + 1)) else { continue };
                    let pad_l = 1.4 * m.space;
                    let inner = (w - pad_l - 1.0 * m.space).max(1.0);
                    let x_of = |n: &Note| bx + pad_l + inner * (n.start as f32 / bar.length.max(1) as f32);
                    // Bar number and tempo above the line.
                    let label_y = y0 + 1.6 * m.space;
                    if !bar.number.is_empty() {
                        painter.text(
                            pos2(bx + 2.0, label_y),
                            Align2::LEFT_CENTER,
                            &bar.number,
                            FontId::proportional(1.1 * m.space),
                            ink.line,
                        );
                    }
                    if let Some(tempo) = bar.tempo {
                        let text = actcraft_engine::fill(tr("{bpm} BPM"), &[("bpm", &format!("{tempo:.0}"))]);
                        painter.text(
                            pos2(bx + 3.0 * m.space, label_y),
                            Align2::LEFT_CENTER,
                            text,
                            FontId::proportional(1.3 * m.space),
                            ink.ink,
                        );
                    }
                    // Bar lines.
                    let end_line = index + 1 == part.bars.len();
                    for (si, _) in rows.staves.iter().enumerate() {
                        let Some(&top) = tops.get(si) else { continue };
                        let top = y0 + top;
                        painter.line_segment(
                            [pos2(bx1, top), pos2(bx1, top + 4.0 * m.space)],
                            Stroke::new(if end_line { 3.0 } else { 1.2 }, ink.ink),
                        );
                    }
                    if rows.strings > 0 {
                        let bottom = tab_y + (rows.strings - 1) as f32 * m.tab_gap;
                        painter.line_segment(
                            [pos2(bx1, tab_y), pos2(bx1, bottom)],
                            Stroke::new(if end_line { 3.0 } else { 1.2 }, ink.ink),
                        );
                    }
                    // Notes on each staff, grouped by onset and voice.
                    for (si, clef) in rows.staves.iter().enumerate() {
                        let Some(&top) = tops.get(si) else { continue };
                        let top = y0 + top;
                        let staff = (si + 1) as u8;
                        let mut voices: Vec<u8> =
                            bar.notes.iter().filter(|n| n.staff == staff).map(|n| n.voice).collect();
                        voices.sort_unstable();
                        voices.dedup();
                        let several = voices.len() > 1;
                        let mut acc = Accidentals { key: key_alters(bar.fifths), written: Vec::new() };
                        let on_staff: Vec<&Note> = bar.notes.iter().filter(|n| n.staff == staff).collect();
                        let mut i = 0;
                        while i < on_staff.len() {
                            let Some(first) = on_staff.get(i) else { break };
                            let group: Vec<&Note> = on_staff[i..]
                                .iter()
                                .take_while(|n| n.start == first.start)
                                .filter(|n| n.voice == first.voice)
                                .copied()
                                .collect();
                            let taken = on_staff[i..].iter().take_while(|n| n.start == first.start).count().max(1);
                            // Other voices at the same onset are drawn on their own pass.
                            let rest_voices: Vec<&Note> = on_staff[i..]
                                .iter()
                                .take_while(|n| n.start == first.start)
                                .filter(|n| n.voice != first.voice)
                                .copied()
                                .collect();
                            for g in [group, rest_voices] {
                                let mut by_voice = g;
                                while let Some(v) = by_voice.first().map(|n| n.voice) {
                                    let chord: Vec<&Note> = by_voice.iter().filter(|n| n.voice == v).copied().collect();
                                    by_voice.retain(|n| n.voice != v);
                                    let x = chord.first().map_or(bx, |n| x_of(n));
                                    let up = if several {
                                        v % 2 == 1
                                    } else {
                                        let avg =
                                            chord.iter().filter_map(|n| n.pitch).map(|p| clef.position(p)).sum::<i32>()
                                                / chord.len().max(1) as i32;
                                        avg < 4
                                    };
                                    pa.chord(x, top, *clef, &chord, up, &mut acc);
                                }
                            }
                            i += taken;
                        }
                    }
                    // Frets.
                    if rows.strings > 0 {
                        let font = FontId::proportional(1.25 * m.space);
                        for n in &bar.notes {
                            let (Some(s), Some(f)) = (n.string, n.fret) else { continue };
                            let s = usize::from(s);
                            if s == 0 || s > rows.strings {
                                continue;
                            }
                            let y = tab_y + (s - 1) as f32 * m.tab_gap;
                            let text = if n.tied { format!("({f})") } else { f.to_string() };
                            let x = x_of(n);
                            let galley = painter.layout_no_wrap(text, font.clone(), ink.ink);
                            let r = Rect::from_center_size(pos2(x, y), galley.size() + vec2(2.0, 0.0));
                            painter.rect_filled(r, 0.0, ink.paper);
                            painter.galley(r.min + vec2(1.0, 0.0), galley, ink.ink);
                        }
                    }
                }
                // Clicking a bar plays from its start.
                if response.clicked()
                    && let Some(at) = response.interact_pointer_pos()
                    && at.y >= y0
                    && at.y < y0 + line_h
                    && let Some(k) = xs.windows(2).position(|w| at.x >= w[0] && at.x < w[1])
                {
                    seek = Some(sys.first + k);
                }
            }
            response.on_hover_text(tr("Click a bar to play from there"));
        });
        if let Some(bar) = seek {
            let at = bar_time(song, bar).max(0.0);
            self.run(e, "transport.seek", json!({"seconds": at}));
        }
    }
}

/// Where a point lands: the bar index under it, for tests of the layout.
#[cfg(test)]
fn bar_at(systems: &[System], line_h: f32, header_x: f32, p: egui::Pos2) -> Option<usize> {
    let line = (p.y / line_h).floor() as usize;
    let sys = systems.get(line)?;
    let mut x = header_x + sys.header;
    for (k, w) in sys.widths.iter().enumerate() {
        if p.x >= x && p.x < x + w {
            return Some(sys.first + k);
        }
        x += w;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(notes: Vec<Note>) -> Bar {
        Bar { number: "1".into(), beats: 4, beat_type: 4, fifths: 0, tempo: None, length: 3840, notes }
    }

    fn quarter(start: u32) -> Note {
        Note { start, length: 960, pitch: Some(Pitch { step: 0, alter: 0, octave: 5 }), ..Note::default() }
    }

    #[test]
    fn keys_alter_the_right_letters() {
        assert_eq!(key_alters(0), [0; 7]);
        assert_eq!(key_alters(2), [1, 0, 0, 1, 0, 0, 0], "D major: F# C#");
        assert_eq!(key_alters(-3), [0, 0, -1, 0, 0, -1, -1], "Eb major: Bb Eb Ab");
        assert_eq!(key_alters(99), [1; 7]);
        assert_eq!(key_positions(Clef::TREBLE, 1), vec![8], "F# on the top line");
        assert_eq!(key_positions(Clef::GUITAR, 1), vec![8], "the 8 below changes nothing");
        assert_eq!(key_positions(Clef::BASS, 1), vec![6], "F# on the bass clef's fourth line");
        assert_eq!(key_positions(Clef::TREBLE, -2), vec![4, 7]);
    }

    #[test]
    fn busy_bars_are_wider_and_lines_fill_the_width() {
        let m = Metrics::new(1.0);
        let sparse = bar(vec![quarter(0)]);
        let busy = bar((0..16).map(|i| quarter(i * 240)).collect());
        assert!(bar_width(&busy, m) > bar_width(&sparse, m) * 2.0);
        let part = Part { bars: vec![sparse.clone(); 30], clefs: vec![Clef::TREBLE], ..Part::default() };
        let rows = Rows::of(&part, NotationShow::Both);
        assert_eq!(rows.strings, 0, "not written as tab");
        let systems = break_lines(&part, &rows, 800.0, m);
        assert!(systems.len() > 1);
        assert_eq!(systems.iter().map(|s| s.widths.len()).sum::<usize>(), 30, "every bar once");
        for s in &systems[..systems.len() - 1] {
            let used = s.header + s.widths.iter().sum::<f32>();
            assert!((used - 800.0).abs() < 0.5, "{used}");
        }
        // A bar wider than the line still gets its own line.
        let huge = Part { bars: vec![busy.clone(), busy], clefs: vec![Clef::TREBLE], ..Part::default() };
        let narrow = break_lines(&huge, &rows, 50.0, m);
        assert_eq!(narrow.len(), 2);
        let (_, _, line_h) = rows.layout(m);
        assert_eq!(bar_at(&systems, line_h, 0.0, pos2(systems[0].header + 1.0, 5.0)), Some(0));
        assert_eq!(bar_at(&systems, line_h, 0.0, pos2(1.0, line_h + 5.0)), None, "the header is no bar");
    }

    #[test]
    fn tab_rows_follow_the_choice() {
        let mut part = Part { clefs: vec![Clef::GUITAR], tablature: true, ..Part::default() };
        part.bars.push(bar(vec![quarter(0)]));
        assert_eq!(Rows::of(&part, NotationShow::Both), Rows { staves: vec![Clef::GUITAR], strings: 6 });
        assert_eq!(Rows::of(&part, NotationShow::Tab), Rows { staves: vec![], strings: 6 });
        assert_eq!(Rows::of(&part, NotationShow::Notes), Rows { staves: vec![Clef::GUITAR], strings: 0 });
        let m = Metrics::new(1.0);
        let (tops, tab_top, h) = Rows::of(&part, NotationShow::Both).layout(m);
        assert_eq!(tops.len(), 1);
        assert!(tab_top > tops[0] + 4.0 * m.space && h > tab_top + 5.0 * m.tab_gap);
    }

    #[test]
    fn the_playing_bar_comes_from_the_beat_grid() {
        let song = Song { bpm: 120.0, beats_per_bar: 4, beat_anchor: 1.0, ..Song::default() };
        assert_eq!(playing_bar(&song, 0.5, 10), None, "before bar 1");
        assert_eq!(playing_bar(&song, 1.0, 10), Some((0, 0.0)));
        assert_eq!(playing_bar(&song, 4.0, 10), Some((1, 0.5)), "two seconds per bar");
        assert_eq!(playing_bar(&song, 100.0, 10), None, "after the last bar");
        assert!((bar_time(&song, 3) - 7.0).abs() < 1e-9);
    }
}
