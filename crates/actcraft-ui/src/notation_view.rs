//! Notation in the Sheet music tab: a song's MusicXML score drawn as staves and tablature, bar
//! after bar in lines across the tab. The playing bar is marked and the view follows playback;
//! clicking a bar plays from there. The score's bar 1 sits on the song bar set per song
//! (`score_first_bar`, 1 by default: the beat grid's first bar).

use actcraft_engine::Engine;
use actcraft_engine::notation::{Bar, Clef, ClefKind, Note, NoteValue, Part, Pitch, Score, TICKS_PER_QUARTER};
use actcraft_show::Song;
use egui::{Align2, Color32, FontId, Key, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::{ActCraftApp, tr};

/// The score file types the picker offers.
pub const NOTATION_EXTENSIONS: &[&str] = actcraft_engine::notation::EXTENSIONS;

/// An open score.
pub(crate) struct NotationDoc {
    pub path: String,
    pub score: Score,
    /// Per part, the bars in the order they are played (repeats, endings and jumps unrolled).
    pub orders: Vec<Vec<usize>>,
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
    /// The bar the keyboard has chosen (arrow keys on the score; Enter plays from it).
    pub cursor: usize,
}

impl Default for NotationLook {
    fn default() -> Self {
        Self { part: 0, show: NotationShow::Both, zoom: 1.0, cursor: 0 }
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
    /// Verses of lyrics under each staff (parallel to `staves`).
    verses: Vec<usize>,
    /// Chord symbols above the first row.
    chords: bool,
    /// Strings of the tablature, 0 for none.
    strings: usize,
}

/// Height of one line of lyrics, in staff spaces.
const LYRIC_LINE: f32 = 1.8;

impl Rows {
    fn of(part: &Part, show: NotationShow) -> Self {
        let tab = match show {
            NotationShow::Both => part.tablature,
            NotationShow::Tab => true,
            NotationShow::Notes => false,
        };
        let staves = if show == NotationShow::Tab { Vec::new() } else { part.clefs.clone() };
        let staves = if staves.is_empty() && show != NotationShow::Tab { vec![Clef::TREBLE] } else { staves };
        let notes = || part.bars.iter().flat_map(|b| &b.notes);
        let verses = (1..=staves.len())
            .map(|staff| {
                notes()
                    .filter(|n| usize::from(n.staff) == staff)
                    .flat_map(|n| n.lyrics.iter().map(|l| usize::from(l.verse)))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let chords = part.bars.iter().any(|b| !b.chords.is_empty());
        Self { staves, verses, chords, strings: if tab { part.strings().len() } else { 0 } }
    }

    /// Top of each staff's top line, the tablature's first line, and the line's whole height.
    fn layout(&self, m: Metrics) -> (Vec<f32>, f32, f32) {
        let mut y = 3.0 * m.space; // bar numbers and tempo marks
        if self.chords {
            y += 2.0 * m.space;
        }
        let mut tops = Vec::new();
        for (k, _) in self.staves.iter().enumerate() {
            y += 3.0 * m.space; // ledger lines above
            tops.push(y);
            y += 4.0 * m.space + 3.5 * m.space; // the staff, ledger lines below
            y += self.verses.get(k).copied().unwrap_or(0) as f32 * LYRIC_LINE * m.space;
        }
        let tab_top = y + m.tab_gap;
        if self.strings > 0 {
            y = tab_top + (self.strings.saturating_sub(1)) as f32 * m.tab_gap + m.tab_gap;
        }
        (tops, tab_top, y + 2.0 * m.space)
    }

    /// Where chord symbols sit, from the line's top.
    fn chord_y(m: Metrics) -> f32 {
        4.0 * m.space
    }

    /// Where verse `verse` (1-based) of the lyrics under a staff sits, from the staff's top.
    fn lyric_y(verse: u8, m: Metrics) -> f32 {
        (6.2 + f32::from(verse.saturating_sub(1)) * LYRIC_LINE) * m.space
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
    // Bends need room to the right of their fret.
    let bends = bar.notes.iter().any(|n| n.technique.bend.is_some());
    let unit = if bends { 3.8 } else { 2.6 } * m.space;
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
    let notes = (slots * unit).clamp(4.0 * unit, 40.0 * unit) + 2.0 * m.space;
    // Words under the notes need room too: neighbouring syllables may not touch. Notes sit in
    // proportion to their onset, so the closest pair of wide words sets the width.
    let word = |o: u32| {
        bar.notes
            .iter()
            .filter(|n| n.start == o)
            .flat_map(|n| &n.lyrics)
            .map(|l| l.text.chars().count() as f32 * LYRIC_CHAR * m.space)
            .fold(0.0, f32::max)
    };
    let length = bar.length.max(1);
    let mut words = 0.0f32;
    for (&a, &b) in onsets.iter().zip(onsets.iter().skip(1).chain(std::iter::once(&length))) {
        let gap = b.saturating_sub(a).max(1) as f32;
        let next = if b == length { 0.0 } else { word(b) };
        let need = (word(a) + next) / 2.0 + 0.8 * m.space;
        words = words.max(need * length as f32 / gap);
    }
    let words = words.min(40.0 * unit) + 2.4 * m.space;
    notes.max(words) + repeat_left(bar, m) + repeat_right(bar, m)
}

/// Width of one character of lyrics, in staff spaces (an average for the proportional font).
const LYRIC_CHAR: f32 = 0.7;

/// Room a repeat sign at the bar's start takes from its notes.
fn repeat_left(bar: &Bar, m: Metrics) -> f32 {
    if bar.flow.repeat_start { 1.4 * m.space } else { 0.0 }
}

/// Room a repeat sign at the bar's end takes from its notes.
fn repeat_right(bar: &Bar, m: Metrics) -> f32 {
    if bar.flow.repeat_end.is_some() { 1.4 * m.space } else { 0.0 }
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

/// Grid bars (0-based from the beat anchor) before the score's first bar; negative when the
/// score starts before the anchor (a pickup).
fn score_offset(song: &Song) -> f64 {
    f64::from(song.score_first_bar) - 1.0
}

/// Which bar of the played order (repeats unrolled) plays at `time` and how far into it (0..1),
/// on the song's beat grid; `bars` is the length of the played order.
fn playing_bar(song: &Song, time: f64, bars: usize) -> Option<(usize, f32)> {
    let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
    let beat = clock.beat_at(time);
    if !beat.is_finite() {
        return None;
    }
    let per_bar = f64::from(song.beats_per_bar.max(1));
    let grid_bar = (beat / per_bar).floor();
    let index = grid_bar - score_offset(song);
    (index >= 0.0 && index < bars as f64).then_some((index as usize, ((beat - grid_bar * per_bar) / per_bar) as f32))
}

/// The time bar `index` of the played order starts at, on the song's beat grid.
fn bar_time(song: &Song, index: usize) -> f64 {
    let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
    clock.time_of_beat((index as f64 + score_offset(song)) * f64::from(song.beats_per_bar.max(1)))
}

/// Where a click on written bar `bar` goes in the played order: of the passes through it, the one
/// nearest the playing position `near` (the first pass when nothing plays).
fn played_index(order: &[usize], bar: usize, near: Option<usize>) -> Option<usize> {
    let near = near.unwrap_or(0);
    order.iter().enumerate().filter(|&(_, &b)| b == bar).map(|(k, _)| k).min_by_key(|k| k.abs_diff(near))
}

/// A note in a line of bars: the bar's place in the line and the note's index in the bar.
type NoteAt = (usize, usize);

/// A tie or slur on one staff of a line: where it starts and ends (`None` when that end is on
/// another line), and the side a slur's file asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Arc {
    from: Option<NoteAt>,
    to: Option<NoteAt>,
    above: Option<bool>,
    /// A tie (to the next note of the same pitch) rather than a slur.
    tie: bool,
}

/// The ties and slurs of staff `staff` in a line of bars. A tie joins a note to the next one of
/// the same pitch; a slur joins its start to the stop with the same number.
fn arcs(bars: &[&Bar], staff: u8) -> Vec<Arc> {
    let mut ties: Vec<(u8, NoteAt)> = Vec::new();
    let mut slurs: Vec<(u8, NoteAt, Option<bool>)> = Vec::new();
    let mut out = Vec::new();
    for (bi, bar) in bars.iter().enumerate() {
        for (ni, n) in bar.notes.iter().enumerate().filter(|(_, n)| n.staff == staff) {
            let at = (bi, ni);
            if let Some(midi) = n.pitch.map(Pitch::midi) {
                if n.tied {
                    let from = ties.iter().position(|t| t.0 == midi).map(|k| ties.remove(k).1);
                    out.push(Arc { from, to: Some(at), above: None, tie: true });
                }
                if n.tie_start {
                    ties.push((midi, at));
                }
            }
            for s in n.slurs.iter().filter(|s| !s.start) {
                let open = slurs.iter().position(|o| o.0 == s.number).map(|k| slurs.remove(k));
                let above = open.and_then(|o| o.2).or(s.above);
                out.push(Arc { from: open.map(|o| o.1), to: Some(at), above, tie: false });
            }
            for s in n.slurs.iter().filter(|s| s.start) {
                slurs.push((s.number, at, s.above));
            }
        }
    }
    out.extend(ties.into_iter().map(|(_, at)| Arc { from: Some(at), to: None, above: None, tie: true }));
    out.extend(slurs.into_iter().map(|(_, at, above)| Arc { from: Some(at), to: None, above, tie: false }));
    out
}

/// How two frets on one string are joined in the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkKind {
    HammerOn,
    PullOff,
    Slide,
}

/// A hammer-on, pull-off or slide in a line of bars, from a note to the next on its string;
/// `to` is `None` when that note is on another line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabLink {
    from: NoteAt,
    to: Option<NoteAt>,
    kind: LinkKind,
}

/// The hammer-ons, pull-offs and slides of a line of bars: each joins its note to the next note
/// on the same string. One whose next note is on another line is drawn toward the line's edge.
fn tab_links(bars: &[&Bar]) -> Vec<TabLink> {
    let mut open: Vec<(u8, NoteAt, LinkKind)> = Vec::new();
    let mut out = Vec::new();
    for (bi, bar) in bars.iter().enumerate() {
        for (ni, n) in bar.notes.iter().enumerate() {
            let Some(string) = n.string else { continue };
            let at = (bi, ni);
            if let Some(k) = open.iter().position(|o| o.0 == string) {
                let (_, from, kind) = open.remove(k);
                out.push(TabLink { from, to: Some(at), kind });
            }
            let t = n.technique;
            let kind = if t.slide {
                LinkKind::Slide
            } else if t.pull_off {
                LinkKind::PullOff
            } else if t.hammer_on {
                LinkKind::HammerOn
            } else {
                continue;
            };
            open.push((string, at, kind));
        }
    }
    out.extend(open.into_iter().map(|(_, from, kind)| TabLink { from, to: None, kind }));
    out
}

/// A bend's size as tab writes it, in whole tones: "1/2", "full", "1 1/2", "2".
fn bend_label(semitones: f32) -> String {
    let halves = (semitones.clamp(0.0, 64.0)).round() as u32;
    let (whole, half) = (halves / 2, halves % 2 == 1);
    match (whole, half) {
        (0, _) => "1/2".to_owned(),
        (1, false) => tr("full").to_owned(),
        (w, false) => w.to_string(),
        (w, true) => format!("{w} 1/2"),
    }
}

/// Where onset `start` (ticks) sits across a bar drawn from `bx` with width `w`: a note's head,
/// a chord symbol.
fn tick_x(bar: &Bar, bx: f32, w: f32, m: Metrics, start: u32) -> f32 {
    let pad_l = 1.4 * m.space + repeat_left(bar, m);
    let inner = (w - pad_l - 1.0 * m.space - repeat_right(bar, m)).max(1.0);
    bx + pad_l + inner * (start as f32 / bar.length.max(1) as f32)
}

/// How a chord's stem is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stem {
    up: bool,
    /// Where the stem ends (a beam's height); `None` for its natural length.
    end: Option<f32>,
    /// Draw flags (notes without a beam).
    flags: bool,
}

/// Notes struck together in one voice on one staff.
#[derive(Debug, Clone, PartialEq)]
struct Event<'a> {
    start: u32,
    voice: u8,
    notes: Vec<&'a Note>,
}

impl Event<'_> {
    fn value(&self) -> NoteValue {
        self.notes.first().map_or(NoteValue::Quarter, |n| n.value)
    }

    fn pitched(&self) -> bool {
        self.notes.iter().any(|n| n.pitch.is_some())
    }
}

/// The bar's notes on one staff, as chords by onset and voice.
fn events(bar: &Bar, staff: u8) -> Vec<Event<'_>> {
    let mut out: Vec<Event<'_>> = Vec::new();
    for n in bar.notes.iter().filter(|n| n.staff == staff) {
        match out.iter_mut().find(|e| e.start == n.start && e.voice == n.voice) {
            Some(e) => e.notes.push(n),
            None => out.push(Event { start: n.start, voice: n.voice, notes: vec![n] }),
        }
    }
    out.sort_by_key(|e| (e.start, e.voice));
    out
}

/// A tuplet drawn over events `first..=last` (indexes into one staff's events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TupletGroup {
    first: usize,
    last: usize,
    actual: u8,
    bracket: bool,
}

/// The tuplets of one staff's events, per voice from the note that starts the bracket to the one
/// that stops it. Without a word from the file, a bracket is drawn when a note in it has no beam
/// (a quarter or longer, or a rest).
fn tuplet_groups(events: &[Event<'_>]) -> Vec<TupletGroup> {
    let mut out = Vec::new();
    let mut open: Vec<(u8, usize, u8, Option<bool>)> = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let Some(t) = ev.notes.iter().map(|n| n.tuplet).find(|t| t.actual > 0) else { continue };
        if t.start && !open.iter().any(|o| o.0 == ev.voice) {
            open.push((ev.voice, i, t.actual, t.bracket));
        }
        if t.stop
            && let Some(k) = open.iter().position(|o| o.0 == ev.voice)
        {
            let (voice, first, actual, bracket) = open.remove(k);
            let unbeamed = events
                .iter()
                .take(i + 1)
                .skip(first)
                .filter(|e| e.voice == voice)
                .any(|e| e.notes.iter().any(|n| n.pitch.is_none() || n.value.flags() == 0));
            out.push(TupletGroup { first, last: i, actual, bracket: bracket.unwrap_or(unbeamed) });
        }
    }
    out
}

/// The span notes are beamed within: a dotted quarter in 3/8, 6/8, 9/8 and 12/8, else one beat.
fn beam_span(bar: &Bar) -> u32 {
    if bar.beat_type == 8 && bar.beats.is_multiple_of(3) {
        TICKS_PER_QUARTER * 3 / 2
    } else {
        TICKS_PER_QUARTER * 4 / u32::from(bar.beat_type.max(1))
    }
}

/// Runs of eighths and shorter in one voice and one beat, which share a beam (indexes into
/// `events`, two or more each). A rest or a longer note ends a run.
fn beam_groups(bar: &Bar, events: &[Event<'_>]) -> Vec<Vec<usize>> {
    let span = beam_span(bar).max(1);
    let mut voices: Vec<u8> = events.iter().map(|e| e.voice).collect();
    voices.sort_unstable();
    voices.dedup();
    let mut groups = Vec::new();
    for v in voices {
        let mut run: Vec<usize> = Vec::new();
        let mut beat = u32::MAX;
        for (i, e) in events.iter().enumerate().filter(|(_, e)| e.voice == v) {
            let beamable = e.pitched() && e.value().flags() > 0;
            if !beamable || e.start / span != beat {
                if run.len() > 1 {
                    groups.push(std::mem::take(&mut run));
                }
                run.clear();
            }
            if beamable {
                beat = e.start / span;
                run.push(i);
            }
        }
        if run.len() > 1 {
            groups.push(run);
        }
    }
    groups
}

/// A beam's height at each stem `(x, natural end)`: a straight line sloped like the notes (at most
/// a quarter), moved so no stem gets shorter than its natural length.
fn beam_line(stems: &[(f32, f32)], up: bool) -> Vec<f32> {
    let (Some(&(x0, y0)), Some(&(x1, y1))) = (stems.first(), stems.last()) else { return Vec::new() };
    let slope = if x1 > x0 { ((y1 - y0) / (x1 - x0)).clamp(-0.25, 0.25) } else { 0.0 };
    let offsets = stems.iter().map(|&(x, y)| y - slope * (x - x0));
    let base = if up { offsets.fold(f32::INFINITY, f32::min) } else { offsets.fold(f32::NEG_INFINITY, f32::max) };
    stems.iter().map(|&(x, _)| base + slope * (x - x0)).collect()
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

    /// A repeat sign at bar line `x`: a thick and a thin line, and two dots on the side of the
    /// repeated bars (right of `x` for a start, left for an end).
    fn repeat_sign(&self, x: f32, top: f32, bottom: f32, dots: [f32; 2], forward: bool) {
        let s = self.m.space;
        let dir = if forward { 1.0 } else { -1.0 };
        let thick = x + dir * 0.25 * s;
        self.p.line_segment([pos2(thick, top), pos2(thick, bottom)], Stroke::new(0.5 * s, self.ink.ink));
        let thin = x + dir * 0.75 * s;
        self.p.line_segment([pos2(thin, top), pos2(thin, bottom)], Stroke::new(1.2, self.ink.ink));
        for y in dots {
            self.p.circle_filled(pos2(x + dir * 1.15 * s, y), 0.22 * s, self.ink.ink);
        }
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

    /// Where the stem of notes at `x` stands.
    fn stem_x(&self, x: f32, up: bool) -> f32 {
        let rx = 0.62 * self.m.space;
        if up { x + rx * 0.95 } else { x - rx * 0.95 }
    }

    /// Where a stem ends with no beam: three and a half spaces past the furthest head.
    fn natural_end(&self, top: f32, clef: Clef, notes: &[&Note], up: bool) -> f32 {
        let positions = notes.iter().filter_map(|n| n.pitch).map(|p| clef.position(p));
        if up {
            self.y(top, positions.max().unwrap_or(4)) - 3.4 * self.m.space
        } else {
            self.y(top, positions.min().unwrap_or(4)) + 3.4 * self.m.space
        }
    }

    /// Notes struck together on one staff in one voice, with their stem as planned.
    fn chord(&self, x: f32, top: f32, clef: Clef, notes: &[&Note], stem: Stem, acc: &mut Accidentals) {
        let s = self.m.space;
        let up = stem.up;
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
        let sx = self.stem_x(x, up);
        let from = if up { self.y(top, low.0) } else { self.y(top, high.0) };
        let to = stem.end.unwrap_or_else(|| self.natural_end(top, clef, notes, up));
        self.p.line_segment([pos2(sx, from), pos2(sx, to)], stroke);
        let dir = if up { 1.0 } else { -1.0 };
        let flags = if stem.flags { value.flags() } else { 0 };
        for k in 0..flags {
            let y = to + dir * f32::from(k) * 0.8 * s;
            self.p.line_segment([pos2(sx, y), pos2(sx + 0.9 * s, y + dir * 1.3 * s)], Stroke::new(1.8, self.ink.ink));
        }
    }
    /// A bend from a fret at `from`: a curve up to `top` with an arrow and its size above it,
    /// and an arrow back down when it is released.
    fn bend(&self, from: egui::Pos2, top: f32, label: &str, release: bool) {
        let s = self.m.space;
        let stroke = Stroke::new(1.3, self.ink.ink);
        let peak = pos2(from.x + 1.4 * s, top);
        let ctrl = pos2(peak.x, from.y);
        let pts: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                let u = 1.0 - t;
                pos2(
                    u * u * from.x + 2.0 * u * t * ctrl.x + t * t * peak.x,
                    u * u * from.y + 2.0 * u * t * ctrl.y + t * t * peak.y,
                )
            })
            .collect();
        self.p.add(Shape::line(pts, stroke));
        self.arrow_head(peak, true);
        self.p.text(
            pos2(peak.x, top - 0.9 * s),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(1.0 * s),
            self.ink.ink,
        );
        if release {
            let down = pos2(peak.x + 1.2 * s, from.y - 0.6 * s);
            self.p.line_segment([peak, down], stroke);
            self.arrow_head(down, false);
        }
    }

    /// A small filled arrowhead pointing up or down at `tip`.
    fn arrow_head(&self, tip: egui::Pos2, up: bool) {
        let s = self.m.space;
        let d = if up { 1.0 } else { -1.0 } * 0.6 * s;
        let pts = vec![tip, pos2(tip.x - 0.35 * s, tip.y + d), pos2(tip.x + 0.35 * s, tip.y + d)];
        self.p.add(Shape::convex_polygon(pts, self.ink.ink, Stroke::NONE));
    }

    /// A vibrato's wavy line from `x0` to `x1` at height `y`.
    fn vibrato(&self, x0: f32, x1: f32, y: f32) {
        let s = self.m.space;
        let step = 0.4 * s;
        let count = (((x1 - x0) / step).max(1.0) as usize).min(64);
        let pts: Vec<egui::Pos2> =
            (0..=count).map(|i| pos2(x0 + i as f32 * step, y + if i % 2 == 0 { -0.25 } else { 0.25 } * s)).collect();
        self.p.add(Shape::line(pts, Stroke::new(1.3, self.ink.ink)));
    }

    /// A tuplet's number over `x0..x1` at height `y`, in a bracket open toward the notes.
    fn tuplet(&self, x0: f32, x1: f32, y: f32, actual: u8, bracket: bool) {
        let s = self.m.space;
        let mid = (x0 + x1) / 2.0;
        let text = actual.to_string();
        let galley = self.p.layout_no_wrap(text, FontId::proportional(1.3 * s), self.ink.ink);
        let half = galley.size().x / 2.0 + 0.3 * s;
        if bracket && x1 - x0 > 2.0 * half {
            let stroke = Stroke::new(1.2, self.ink.ink);
            let hook = 0.6 * s;
            self.p.line_segment([pos2(x0, y + hook), pos2(x0, y)], stroke);
            self.p.line_segment([pos2(x0, y), pos2(mid - half, y)], stroke);
            self.p.line_segment([pos2(mid + half, y), pos2(x1, y)], stroke);
            self.p.line_segment([pos2(x1, y), pos2(x1, y + hook)], stroke);
        }
        self.p.galley(pos2(mid - galley.size().x / 2.0, y - galley.size().y / 2.0), galley, self.ink.ink);
    }

    /// A tie or slur from `a` to `b`, bowed up or down; two close curves make it thicker in the
    /// middle than at its ends.
    fn arc(&self, a: egui::Pos2, b: egui::Pos2, above: bool) {
        let s = self.m.space;
        let dir = if above { -1.0 } else { 1.0 };
        let rise = (0.12 * (b.x - a.x).abs()).clamp(0.5 * s, 1.6 * s);
        let curve = |rise: f32| -> Vec<egui::Pos2> {
            let c = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0 + dir * rise);
            (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    pos2(u * u * a.x + 2.0 * u * t * c.x + t * t * b.x, u * u * a.y + 2.0 * u * t * c.y + t * t * b.y)
                })
                .collect()
        };
        for r in [rise, rise - 0.15 * s] {
            self.p.add(Shape::line(curve(r), Stroke::new(1.3, self.ink.ink)));
        }
    }

    /// Beams over stems `(x, end, flags)`: one per flag level, a short one for a note alone on
    /// its level.
    fn beam(&self, stems: &[(f32, f32, u8)], up: bool) {
        let s = self.m.space;
        let thick = 0.45 * s;
        let dir = if up { 1.0 } else { -1.0 };
        let at = |k: usize| stems.get(k).copied();
        let levels = stems.iter().map(|t| t.2).max().unwrap_or(0);
        for level in 1..=levels {
            let off = dir * f32::from(level - 1) * 0.75 * s;
            for k in 0..stems.len() {
                let Some((x, y, f)) = at(k) else { continue };
                if f < level {
                    continue;
                }
                let segment = match at(k + 1) {
                    Some((nx, ny, nf)) if nf >= level => Some((nx, ny)),
                    _ => None,
                };
                let (to_x, to_y) = match segment {
                    Some(p) => p,
                    None => {
                        // Already joined to the previous stem, or a stub toward a neighbour.
                        if k > 0 && at(k - 1).is_some_and(|p| p.2 >= level) {
                            continue;
                        }
                        let toward = if k + 1 < stems.len() { 1.2 * s } else { -1.2 * s };
                        let slope = match (at(0), at(stems.len() - 1)) {
                            (Some(a), Some(b)) if b.0 > a.0 => (b.1 - a.1) / (b.0 - a.0),
                            _ => 0.0,
                        };
                        (x + toward, y + slope * toward)
                    }
                };
                let a = pos2(x, y + off);
                let b = pos2(to_x, to_y + off);
                let pts = vec![a, b, pos2(b.x, b.y + dir * thick), pos2(a.x, a.y + dir * thick)];
                self.p.add(Shape::convex_polygon(pts, self.ink.ink, Stroke::NONE));
            }
        }
    }
}

impl ActCraftApp {
    /// The notation of the Sheet music tab: options, then the score.
    pub(crate) fn notation_view(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        let Some(path) = song.notation_file.clone() else { return };
        if let Err(why) = self.load_notation(e, &path) {
            ui.label(egui::RichText::new(format!("{} {why}", tr("The notation could not be opened:"))).color(t.danger));
            return;
        }
        let Some(doc) = self.notation.as_ref() else { return };
        let look = &mut self.notation_look;
        look.part = look.part.min(doc.score.parts.len().saturating_sub(1));
        let mut first_bar: Option<i32> = None;
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
            // Where the score sits in the audio, so the playing bar lines up after an intro.
            let label = ui.label(tr("Score bar 1 is song bar"));
            let mut first = song.score_first_bar;
            let range = actcraft_show::limits::MIN_SCORE_FIRST_BAR..=actcraft_show::limits::MAX_SCORE_FIRST_BAR;
            if ui
                .add(egui::DragValue::new(&mut first).range(range).speed(0.1))
                .labelled_by(label.id)
                .on_hover_text(tr(
                    "The bar of the song's beat grid where the score's first bar plays; 0 or less for a pickup bar",
                ))
                .changed()
            {
                first_bar = Some(first);
            }
            ui.spacing_mut().slider_width = 120.0;
            ui.add(egui::Slider::new(&mut look.zoom, 0.6..=2.5).show_value(false).text(tr("Zoom")));
            ui.checkbox(&mut self.sheet_view.follow, tr("Follow playback"))
                .on_hover_text(tr("Keep the playing bar in view while the song plays"));
        });
        if let Some(v) = first_bar {
            self.run(e, "song.set", json!({"song": song.id, "score_first_bar": v}));
        }
        ui.separator();
        let Some(doc) = self.notation.as_ref() else { return };
        let view =
            ScoreView { look: self.notation_look, follow: self.sheet_view.follow, keyboard: true, salt: "notation" };
        let out = score_canvas(ui, t, doc, song, (e.status.time, e.status.playing), view);
        self.notation_look.cursor = out.cursor;
        if let Some(at) = out.seek {
            self.run(e, "transport.seek", json!({"seconds": at}));
        }
    }

    /// Opens the song's score into the cache unless it is already there; the error says why not.
    pub(crate) fn load_notation(&mut self, e: &Engine, path: &str) -> Result<(), String> {
        if load_notation_into(&mut self.notation, e, path)? {
            self.notation_look.part = 0;
        }
        Ok(())
    }
}

/// Opens the score at `path` into `slot` unless it is already there; true when it was opened
/// now. The error says why it could not be; a failed file is tried again on the next call.
pub(crate) fn load_notation_into(slot: &mut Option<NotationDoc>, e: &Engine, path: &str) -> Result<bool, String> {
    if slot.as_ref().is_some_and(|d| d.path == path) {
        return Ok(false);
    }
    *slot = None;
    let score = actcraft_engine::notation_commands::load_score(e, path).map_err(|err| tr(err.message()).to_owned())?;
    let orders = score.parts.iter().map(|p| actcraft_engine::notation::play_order(&p.bars)).collect();
    *slot = Some(NotationDoc { path: path.to_owned(), score, orders });
    Ok(true)
}

/// How a score is shown: its part, display and zoom, whether the view follows playback, and
/// whether it takes clicks and keys (the main window) or only shows (the second window).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScoreView {
    pub look: NotationLook,
    pub follow: bool,
    pub keyboard: bool,
    /// Tells this view's scroll position apart from another one's.
    pub salt: &'static str,
}

/// What a score view did: where the keyboard's bar is now, and a time to play from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScoreOutput {
    pub cursor: usize,
    pub seek: Option<f64>,
}

/// Draws one part of a score as staves and tablature, in lines across the space, with the bar
/// playing at `status` (time, playing) marked.
pub(crate) fn score_canvas(
    ui: &mut Ui,
    t: crate::theme::Tokens,
    doc: &NotationDoc,
    song: &Song,
    status: (f64, bool),
    view: ScoreView,
) -> ScoreOutput {
    let look = view.look;
    let unchanged = ScoreOutput { cursor: look.cursor, seek: None };
    let Some(part) = doc.score.parts.get(look.part) else { return unchanged };
    if part.bars.is_empty() {
        return unchanged;
    }
    let (time, playing_now) = status;
    let m = Metrics::new(look.zoom);
    let rows = Rows::of(part, look.show);
    let (tops, tab_top, line_h) = rows.layout(m);
    let margin = 8.0;
    let width = (ui.available_width() - 2.0 * margin).max(200.0);
    let systems = break_lines(part, &rows, width, m);
    let content = systems.len() as f32 * line_h + 2.0 * margin;
    let order = doc.orders.get(look.part).map(Vec::as_slice).unwrap_or_default();
    let played = playing_bar(song, time, order.len());
    // The written bar that plays.
    let playing = played.and_then(|(k, frac)| order.get(k).map(|&b| (b, frac)));
    let ink = Ink { paper: t.surface, ink: t.text, line: t.text_dim, mark: t.accent };
    let mut area = egui::ScrollArea::vertical().id_salt((view.salt, song.id)).auto_shrink([false, false]);
    if view.follow && playing_now {
        if let Some((bar, _)) = playing
            && let Some(line) = systems.iter().position(|s| bar >= s.first && bar < s.first + s.widths.len())
        {
            area = area.vertical_scroll_offset((line as f32 * line_h - line_h * 0.3).max(0.0));
        }
        ui.ctx().request_repaint();
    }
    let mut seek: Option<usize> = None;
    let mut cursor = look.cursor.min(part.bars.len() - 1);
    let bar_word = tr("bar");
    area.show_viewport(ui, |ui, viewport| {
        let sense = if view.keyboard { Sense::click() } else { Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(vec2(width + 2.0 * margin, content), sense);
        // From the keyboard: arrow keys, Home and End choose a bar, Enter plays from it.
        let focused = response.has_focus();
        if focused {
            let filter = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
            ui.memory_mut(|mem| mem.set_focus_lock_filter(response.id, filter));
            let keys = [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Home, Key::End];
            let mut moved = false;
            for key in keys {
                if ui.input(|i| i.key_pressed(key)) {
                    cursor = move_cursor(cursor, key, &systems, part.bars.len());
                    moved = true;
                }
            }
            if moved && let Some(line) = line_of(&systems, cursor) {
                let top = rect.top() + margin + line as f32 * line_h;
                ui.scroll_to_rect(Rect::from_min_size(pos2(rect.left(), top), vec2(width, line_h)), None);
            }
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                seek = Some(cursor);
            }
        }
        let mut name = format!(
            "{} {}, {}",
            tr("Notation:"),
            part.name,
            match playing {
                Some((b, _)) => format!("{bar_word} {} / {}", b + 1, part.bars.len()),
                None => format!("{} {}", part.bars.len(), tr("bars")),
            }
        );
        if focused {
            let chosen = actcraft_engine::fill(tr("selected: bar {bar}"), &[("bar", &(cursor + 1).to_string())]);
            name = format!("{name}, {chosen}");
        }
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
            // The keyboard's bar, outlined while the score has focus.
            if focused
                && cursor >= sys.first
                && let (Some(&bx), Some(&bx1)) = (xs.get(cursor - sys.first), xs.get(cursor - sys.first + 1))
            {
                let r = Rect::from_min_max(pos2(bx, y0 + 2.0 * m.space), pos2(bx1, y0 + line_h - m.space));
                painter.rect_stroke(r, 2.0, Stroke::new(2.0, ink.mark), egui::StrokeKind::Inside);
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
                let x_of = |n: &Note| tick_x(bar, bx, *w, m, n.start);
                // Bar number, the signs a jump goes to, and tempo above the line, one after
                // the other.
                let label_y = y0 + 1.6 * m.space;
                let mut label_x = bx + 2.0;
                if !bar.number.is_empty() {
                    let r = painter.text(
                        pos2(label_x, label_y),
                        Align2::LEFT_CENTER,
                        &bar.number,
                        FontId::proportional(1.1 * m.space),
                        ink.line,
                    );
                    label_x = r.right() + 0.6 * m.space;
                }
                for (on, word) in [(bar.flow.segno, tr("Segno")), (bar.flow.coda, tr("Coda"))] {
                    if on {
                        let r = painter.text(
                            pos2(label_x, label_y),
                            Align2::LEFT_CENTER,
                            word,
                            FontId::proportional(1.2 * m.space),
                            ink.ink,
                        );
                        label_x = r.right() + 0.6 * m.space;
                    }
                }
                if let Some(tempo) = bar.tempo {
                    let text = actcraft_engine::fill(tr("{bpm} BPM"), &[("bpm", &format!("{tempo:.0}"))]);
                    painter.text(
                        pos2(label_x.max(bx + 3.0 * m.space), label_y),
                        Align2::LEFT_CENTER,
                        text,
                        FontId::proportional(1.3 * m.space),
                        ink.ink,
                    );
                }
                // Chord symbols above the first row, where each chord changes.
                for chord in &bar.chords {
                    painter.text(
                        pos2(tick_x(bar, bx, *w, m, chord.start) - 0.6 * m.space, y0 + Rows::chord_y(m)),
                        Align2::LEFT_CENTER,
                        &chord.name,
                        FontId::proportional(1.5 * m.space),
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
                // Repeat signs on every staff and the tablature.
                let flow = &bar.flow;
                let mut spans: Vec<(f32, f32, [f32; 2])> = rows
                    .staves
                    .iter()
                    .enumerate()
                    .filter_map(|(si, _)| tops.get(si))
                    .map(|&top| {
                        let top = y0 + top;
                        (top, top + 4.0 * m.space, [pa.y(top, 5), pa.y(top, 3)])
                    })
                    .collect();
                if rows.strings > 1 {
                    let bottom = tab_y + (rows.strings - 1) as f32 * m.tab_gap;
                    let mid = (tab_y + bottom) / 2.0;
                    spans.push((tab_y, bottom, [mid - 0.5 * m.tab_gap, mid + 0.5 * m.tab_gap]));
                }
                for &(top, bottom, dots) in &spans {
                    if flow.repeat_start {
                        pa.repeat_sign(bx, top, bottom, dots, true);
                    }
                    if flow.repeat_end.is_some() {
                        pa.repeat_sign(bx1, top, bottom, dots, false);
                    }
                }
                // An ending's bracket over its bars, numbered where it starts.
                if !flow.endings.is_empty() {
                    let y = y0 + 2.6 * m.space;
                    let stroke = Stroke::new(1.2, ink.ink);
                    painter.line_segment([pos2(bx + 1.0, y), pos2(bx1 - 0.4 * m.space, y)], stroke);
                    if flow.ending_start || k == 0 {
                        painter.line_segment([pos2(bx + 1.0, y), pos2(bx + 1.0, y + 1.4 * m.space)], stroke);
                        let numbers: Vec<String> = flow.endings.iter().map(|n| format!("{n}.")).collect();
                        painter.text(
                            pos2(bx + 0.5 * m.space, y + 0.2 * m.space),
                            Align2::LEFT_TOP,
                            numbers.join(" "),
                            FontId::proportional(1.2 * m.space),
                            ink.ink,
                        );
                    }
                    if flow.repeat_end.is_some() {
                        let x = bx1 - 0.4 * m.space;
                        painter.line_segment([pos2(x, y), pos2(x, y + 1.4 * m.space)], stroke);
                    }
                }
                // Jumps and repeat counts, at the bar's end above the staff.
                let mut words: Vec<String> = Vec::new();
                for (on, word) in [
                    (flow.to_coda, tr("To Coda")),
                    (flow.dal_segno, tr("D.S.")),
                    (flow.da_capo, tr("D.C.")),
                    (flow.fine, tr("Fine")),
                ] {
                    if on {
                        words.push(word.to_owned());
                    }
                }
                if let Some(times) = flow.repeat_end.filter(|t| *t > 2) {
                    words.push(actcraft_engine::fill(tr("{times}\u{d7}"), &[("times", &times.to_string())]));
                }
                if !words.is_empty() {
                    painter.text(
                        pos2(bx1 - 2.0, label_y),
                        Align2::RIGHT_CENTER,
                        words.join("  "),
                        FontId::proportional(1.2 * m.space),
                        ink.ink,
                    );
                }
                // Notes on each staff: chords by onset and voice, short notes beamed by beat.
                for (si, clef) in rows.staves.iter().enumerate() {
                    let Some(&top) = tops.get(si) else { continue };
                    let top = y0 + top;
                    let evs = events(bar, (si + 1) as u8);
                    let mut voices: Vec<u8> = evs.iter().map(|e| e.voice).collect();
                    voices.sort_unstable();
                    voices.dedup();
                    let several = voices.len() > 1;
                    let leans_up = |notes: &mut dyn Iterator<Item = &Note>, voice: u8| {
                        if several {
                            return voice % 2 == 1;
                        }
                        let (sum, count) =
                            notes.filter_map(|n| n.pitch).fold((0i32, 0i32), |(s, c), p| (s + clef.position(p), c + 1));
                        count == 0 || sum < 4 * count
                    };
                    let first_x = |ev: &Event<'_>| ev.notes.first().map_or(bx, |n| x_of(n));
                    let mut plan: Vec<Stem> = evs
                        .iter()
                        .map(|ev| Stem {
                            up: leans_up(&mut ev.notes.iter().copied(), ev.voice),
                            end: None,
                            flags: true,
                        })
                        .collect();
                    let mut beams = Vec::new();
                    for group in beam_groups(bar, &evs) {
                        let members: Vec<&Event<'_>> = group.iter().filter_map(|&i| evs.get(i)).collect();
                        let voice = members.first().map_or(1, |e| e.voice);
                        let up = leans_up(&mut members.iter().flat_map(|e| e.notes.iter().copied()), voice);
                        let stems: Vec<(f32, f32)> = members
                            .iter()
                            .map(|ev| (pa.stem_x(first_x(ev), up), pa.natural_end(top, *clef, &ev.notes, up)))
                            .collect();
                        let ys = beam_line(&stems, up);
                        let mut beam = Vec::with_capacity(group.len());
                        for ((&i, &(sx, _)), &y) in group.iter().zip(&stems).zip(&ys) {
                            if let (Some(p), Some(ev)) = (plan.get_mut(i), evs.get(i)) {
                                *p = Stem { up, end: Some(y), flags: false };
                                beam.push((sx, y, ev.value().flags()));
                            }
                        }
                        beams.push((beam, up));
                    }
                    let mut acc = Accidentals { key: key_alters(bar.fifths), written: Vec::new() };
                    for (ev, stem) in evs.iter().zip(&plan) {
                        pa.chord(first_x(ev), top, *clef, &ev.notes, *stem, &mut acc);
                    }
                    for (beam, up) in &beams {
                        pa.beam(beam, *up);
                    }
                    // Tuplets: their number, with a bracket, above the notes and stems.
                    for g in tuplet_groups(&evs) {
                        let mut y = top - 0.8 * m.space;
                        for (ev, stem) in evs.iter().zip(&plan).take(g.last + 1).skip(g.first) {
                            let head_top = ev
                                .notes
                                .iter()
                                .filter_map(|n| n.pitch)
                                .map(|p| pa.y(top, clef.position(p)))
                                .fold(f32::INFINITY, f32::min);
                            let reach = if stem.up {
                                stem.end.unwrap_or_else(|| pa.natural_end(top, *clef, &ev.notes, true))
                            } else {
                                head_top - 0.6 * m.space
                            };
                            y = y.min(reach - 0.9 * m.space);
                        }
                        let (Some(a), Some(b)) = (evs.get(g.first), evs.get(g.last)) else { continue };
                        let x0 = first_x(a) - 0.7 * m.space;
                        let x1 = first_x(b) + 0.7 * m.space;
                        pa.tuplet(x0, x1, y, g.actual, g.bracket);
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
                        let tech = n.technique;
                        // A harmonic's fret in angle brackets, a tied one in parentheses.
                        let text = match (tech.harmonic, n.tied) {
                            (true, _) => format!("<{f}>"),
                            (false, true) => format!("({f})"),
                            (false, false) => f.to_string(),
                        };
                        let x = x_of(n);
                        let galley = painter.layout_no_wrap(text, font.clone(), ink.ink);
                        let r = Rect::from_center_size(pos2(x, y), galley.size() + vec2(2.0, 0.0));
                        painter.rect_filled(r, 0.0, ink.paper);
                        painter.galley(r.min + vec2(1.0, 0.0), galley, ink.ink);
                        let above_tab = tab_y - 1.2 * m.tab_gap;
                        let bottom = tab_y + (rows.strings - 1) as f32 * m.tab_gap;
                        if let Some(bend) = tech.bend {
                            pa.bend(pos2(r.right(), y), above_tab, &bend_label(bend), tech.release);
                        }
                        if tech.vibrato {
                            pa.vibrato(x, x + 2.4 * m.space, above_tab + 0.4 * m.tab_gap);
                        }
                        for (on, word, at_y) in
                            [(tech.palm_mute, tr("P.M."), bottom + 1.1 * m.tab_gap), (tech.tap, tr("T"), above_tab)]
                        {
                            if on {
                                painter.text(
                                    pos2(x, at_y),
                                    Align2::CENTER_CENTER,
                                    word,
                                    FontId::proportional(1.1 * m.space),
                                    ink.ink,
                                );
                            }
                        }
                    }
                }
            }
            // Ties and slurs over the line's bars; one whose other end is on another line runs
            // to the line's edge.
            let line_bars: Vec<&Bar> = (0..sys.widths.len()).filter_map(|k| part.bars.get(sys.first + k)).collect();
            let line_start = xs.first().copied().unwrap_or(x0);
            for (si, clef) in rows.staves.iter().enumerate() {
                let Some(&top) = tops.get(si) else { continue };
                let top = y0 + top;
                let head = |(k, ni): NoteAt| {
                    let bar = line_bars.get(k)?;
                    let n = bar.notes.get(ni)?;
                    let pos = clef.position(n.pitch?);
                    let x = tick_x(bar, *xs.get(k)?, *sys.widths.get(k)?, m, n.start);
                    Some((x, pa.y(top, pos), pos))
                };
                for arc in arcs(&line_bars, (si + 1) as u8) {
                    let (from, to) = (arc.from.and_then(head), arc.to.and_then(head));
                    let Some((_, y_ref, pos)) = from.or(to) else { continue };
                    // Away from the stems: above for high notes, below for low ones.
                    let above = arc.above.unwrap_or(pos >= 4);
                    let off = if above { -0.8 } else { 0.8 } * m.space;
                    // A tie whose other note is on another line is a short half tie toward the
                    // line's edge; a slur runs to the edge.
                    let a = from.map(|(x, y, _)| pos2(x + 0.5 * m.space, y));
                    let b = to.map(|(x, y, _)| pos2(x - 0.5 * m.space, y));
                    let stub = 3.0 * m.space;
                    let a = a.unwrap_or_else(|| {
                        let edge = b.map_or(line_start, |b| if arc.tie { b.x - stub } else { line_start });
                        pos2(edge.max(line_start), y_ref)
                    });
                    let b = b.unwrap_or_else(|| {
                        let edge = if arc.tie { a.x + stub } else { x_end };
                        pos2(edge.min(x_end), y_ref)
                    });
                    if b.x > a.x {
                        pa.arc(a + vec2(0.0, off), b + vec2(0.0, off), above);
                    }
                }
            }
            // Lyrics under each staff, verse by verse; a hyphen joins the syllables of a word.
            for (si, _) in rows.staves.iter().enumerate() {
                let (Some(&top), Some(&verses)) = (tops.get(si), rows.verses.get(si)) else { continue };
                if verses == 0 {
                    continue;
                }
                let top = y0 + top;
                let font = FontId::proportional(1.25 * m.space);
                // Per verse, the right edge of the last syllable and whether its word goes on.
                let mut last: Vec<Option<(f32, bool)>> = vec![None; verses];
                for (k, bar) in line_bars.iter().enumerate() {
                    let (Some(&bx), Some(&w)) = (xs.get(k), sys.widths.get(k)) else { continue };
                    for n in bar.notes.iter().filter(|n| usize::from(n.staff) == si + 1) {
                        for l in &n.lyrics {
                            let Some(slot) = last.get_mut(usize::from(l.verse).saturating_sub(1)) else {
                                continue;
                            };
                            let y = top + Rows::lyric_y(l.verse, m);
                            let x = tick_x(bar, bx, w, m, n.start);
                            let galley = painter.layout_no_wrap(l.text.clone(), font.clone(), ink.ink);
                            let left = x - galley.size().x / 2.0;
                            if let Some((right, true)) = *slot {
                                painter.text(
                                    pos2((right + left) / 2.0, y),
                                    Align2::CENTER_CENTER,
                                    "-",
                                    font.clone(),
                                    ink.ink,
                                );
                            }
                            *slot = Some((left + galley.size().x, l.syllabic.continues()));
                            painter.galley(pos2(left, y - galley.size().y / 2.0), galley, ink.ink);
                        }
                    }
                }
                // A word that goes on in the next line ends this one with a hyphen.
                for (v, slot) in last.iter().enumerate() {
                    if let Some((right, true)) = *slot {
                        let y = top + Rows::lyric_y((v + 1) as u8, m);
                        painter.text(pos2(right + 0.8 * m.space, y), Align2::CENTER_CENTER, "-", font.clone(), ink.ink);
                    }
                }
            }
            // Hammer-ons, pull-offs and slides between frets on one string.
            if rows.strings > 0 {
                let fret = |(k, ni): NoteAt| {
                    let bar = line_bars.get(k)?;
                    let n = bar.notes.get(ni)?;
                    let s = f32::from(n.string?.saturating_sub(1));
                    let x = tick_x(bar, *xs.get(k)?, *sys.widths.get(k)?, m, n.start);
                    Some((x, tab_y + s * m.tab_gap, n.fret.unwrap_or(0)))
                };
                for link in tab_links(&line_bars) {
                    let Some((x0, y, f0)) = fret(link.from) else { continue };
                    let to = link.to.and_then(fret);
                    let a = pos2(x0 + 0.7 * m.space, y);
                    let b =
                        to.map_or(pos2((a.x + 3.0 * m.space).min(x_end), y), |(x, y, _)| pos2(x - 0.7 * m.space, y));
                    if b.x <= a.x {
                        continue;
                    }
                    match link.kind {
                        LinkKind::Slide => {
                            let rising = to.is_none_or(|(_, _, f1)| f1 >= f0);
                            let d = if rising { 0.3 } else { -0.3 } * m.tab_gap;
                            painter.line_segment([a + vec2(0.0, d), b - vec2(0.0, d)], Stroke::new(1.3, ink.ink));
                        }
                        LinkKind::HammerOn | LinkKind::PullOff => {
                            let lift = vec2(0.0, -0.45 * m.tab_gap);
                            pa.arc(a + lift, b + lift, true);
                            let word = if link.kind == LinkKind::HammerOn { tr("H") } else { tr("P") };
                            painter.text(
                                pos2((a.x + b.x) / 2.0, y - 1.5 * m.tab_gap),
                                Align2::CENTER_CENTER,
                                word,
                                FontId::proportional(1.0 * m.space),
                                ink.ink,
                            );
                        }
                    }
                }
            }
            // Clicking a bar plays from its start and makes it the keyboard's bar. Only a
            // pointer click: Enter on the focused score also counts as a click.
            if response.clicked()
                && ui.input(|i| i.pointer.any_click())
                && let Some(at) = response.interact_pointer_pos()
                && at.y >= y0
                && at.y < y0 + line_h
                && let Some(k) = xs.windows(2).position(|w| at.x >= w[0] && at.x < w[1])
            {
                seek = Some(sys.first + k);
                cursor = sys.first + k;
            }
        }
        response.on_hover_text(tr("Click a bar to play from there, or choose one with the arrow keys and press Enter"));
    });
    let seek = seek.and_then(|bar| played_index(order, bar, played.map(|p| p.0))).map(|k| bar_time(song, k).max(0.0));
    ScoreOutput { cursor, seek }
}

/// The line a bar is drawn on.
fn line_of(systems: &[System], bar: usize) -> Option<usize> {
    systems.iter().position(|s| bar >= s.first && bar < s.first + s.widths.len())
}

/// Where the keyboard's bar goes for a key: one bar left or right, the same place one line up or
/// down, the first or the last bar. Other keys leave it.
fn move_cursor(cursor: usize, key: Key, systems: &[System], bars: usize) -> usize {
    let last = bars.saturating_sub(1);
    let cursor = cursor.min(last);
    let line_step = |down: bool| {
        let Some(line) = line_of(systems, cursor) else { return cursor };
        let target = if down { line + 1 } else { line.wrapping_sub(1) };
        let (Some(from), Some(to)) = (systems.get(line), systems.get(target)) else { return cursor };
        to.first + (cursor - from.first).min(to.widths.len().saturating_sub(1))
    };
    match key {
        Key::ArrowLeft => cursor.saturating_sub(1),
        Key::ArrowRight => (cursor + 1).min(last),
        Key::ArrowUp => line_step(false),
        Key::ArrowDown => line_step(true),
        Key::Home => 0,
        Key::End => last,
        _ => cursor,
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
        Bar {
            number: "1".into(),
            beats: 4,
            beat_type: 4,
            fifths: 0,
            tempo: None,
            length: 3840,
            notes,
            flow: Default::default(),
            chords: Vec::new(),
        }
    }

    fn quarter(start: u32) -> Note {
        Note { start, length: 960, pitch: Some(Pitch { step: 0, alter: 0, octave: 5 }), ..Note::default() }
    }

    fn eighth(start: u32, step: u8) -> Note {
        Note {
            start,
            length: 480,
            value: NoteValue::Eighth,
            pitch: Some(Pitch { step, alter: 0, octave: 4 }),
            ..Note::default()
        }
    }

    #[test]
    fn short_notes_are_beamed_by_beat_and_runs_break_at_rests() {
        // 4/4: eighths on beat 1, an eighth and a rest on beat 2, a quarter, a 16th pair.
        let mut notes = vec![eighth(0, 0), eighth(480, 2), eighth(960, 4)];
        notes.push(Note { start: 1440, length: 480, value: NoteValue::Eighth, ..Note::default() });
        notes.push(quarter(1920));
        let mut s1 = eighth(2880, 0);
        s1.value = NoteValue::Sixteenth;
        let mut s2 = eighth(3120, 1);
        s2.value = NoteValue::Sixteenth;
        notes.extend([s1, s2, eighth(3360, 2)]);
        let b = bar(notes);
        let evs = events(&b, 1);
        assert_eq!(evs.len(), 8);
        assert_eq!(beam_groups(&b, &evs), vec![vec![0, 1], vec![5, 6, 7]]);
        // 6/8: two groups of three eighths.
        let six = Bar { beats: 6, beat_type: 8, length: 2880, ..bar((0..6).map(|i| eighth(i * 480, 0)).collect()) };
        assert_eq!(beam_groups(&six, &events(&six, 1)), vec![vec![0, 1, 2], vec![3, 4, 5]]);
        // Chord notes are one event; another voice is beamed on its own.
        let mut chord = vec![eighth(0, 0), eighth(0, 2), eighth(480, 0)];
        let mut v2 = eighth(0, 0);
        v2.voice = 2;
        let mut v2b = eighth(480, 0);
        v2b.voice = 2;
        chord.extend([v2, v2b]);
        let c = bar(chord);
        let evs = events(&c, 1);
        assert_eq!(
            evs.iter().map(|e| (e.start, e.voice, e.notes.len())).collect::<Vec<_>>(),
            vec![(0, 1, 2), (0, 2, 1), (480, 1, 1), (480, 2, 1)]
        );
        assert_eq!(beam_groups(&c, &evs), vec![vec![0, 2], vec![1, 3]]);
    }

    #[test]
    fn beams_slope_gently_and_keep_every_stem_long_enough() {
        // Rising notes, stems up: the line rises at most a quarter and stays above every end.
        let stems = [(0.0, 100.0), (20.0, 90.0), (40.0, 40.0)];
        let ys = beam_line(&stems, true);
        assert_eq!(ys.len(), 3);
        assert!((ys[2] - ys[0] - (-10.0)).abs() < 1e-3, "slope capped at 0.25: {ys:?}");
        for (y, (_, natural)) in ys.iter().zip(stems) {
            assert!(*y <= natural + 1e-3, "{ys:?}");
        }
        let down = beam_line(&stems, false);
        for (y, (_, natural)) in down.iter().zip(stems) {
            assert!(*y >= natural - 1e-3, "{down:?}");
        }
        assert!(beam_line(&[], true).is_empty());
        assert_eq!(beam_line(&[(5.0, 7.0)], true), vec![7.0]);
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
        let rows = |staves: Vec<Clef>, strings| Rows { verses: vec![0; staves.len()], staves, chords: false, strings };
        assert_eq!(Rows::of(&part, NotationShow::Both), rows(vec![Clef::GUITAR], 6));
        assert_eq!(Rows::of(&part, NotationShow::Tab), rows(vec![], 6));
        assert_eq!(Rows::of(&part, NotationShow::Notes), rows(vec![Clef::GUITAR], 0));
        let m = Metrics::new(1.0);
        let (tops, tab_top, h) = Rows::of(&part, NotationShow::Both).layout(m);
        assert_eq!(tops.len(), 1);
        assert!(tab_top > tops[0] + 4.0 * m.space && h > tab_top + 5.0 * m.tab_gap);
    }

    #[test]
    fn lyrics_and_chords_get_room_of_their_own() {
        use actcraft_engine::notation::{ChordSymbol, Lyric, Syllabic};
        let m = Metrics::new(1.0);
        let mut part = Part { clefs: vec![Clef::TREBLE], ..Part::default() };
        part.bars.push(bar(vec![quarter(0)]));
        let plain = Rows::of(&part, NotationShow::Notes);
        let lyric = |verse| Lyric { verse, text: "la".into(), syllabic: Syllabic::Single };
        part.bars[0].notes[0].lyrics = vec![lyric(1), lyric(2)];
        part.bars[0].chords = vec![ChordSymbol { start: 0, name: "C".into() }];
        let sung = Rows::of(&part, NotationShow::Notes);
        assert_eq!((sung.verses.clone(), sung.chords), (vec![2], true));
        let (plain_tops, _, plain_h) = plain.layout(m);
        let (tops, _, h) = sung.layout(m);
        assert!(tops[0] > plain_tops[0], "chords push the staff down");
        assert!(Rows::chord_y(m) < tops[0] - 3.0 * m.space, "chords sit above the ledger lines");
        // Both verses fit inside the line, below the staff.
        let last_verse = tops[0] + Rows::lyric_y(2, m);
        assert!(last_verse > tops[0] + 4.0 * m.space && last_verse + m.space < h);
        assert!(h > plain_h + 2.0 * LYRIC_LINE * m.space);
        // Long words make the bar wider than its notes alone need.
        let short = bar_width(&part.bars[0], m);
        part.bars[0].notes[0].lyrics = vec![Lyric { verse: 1, text: "wonderfully".into(), syllabic: Syllabic::Single }];
        part.bars[0].notes.push(Note { lyrics: vec![lyric(1)], ..quarter(960) });
        let mut no_words = part.bars[0].clone();
        for n in &mut no_words.notes {
            n.lyrics.clear();
        }
        assert!(bar_width(&part.bars[0], m) > bar_width(&no_words, m), "{short}");
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

    #[test]
    fn hammer_ons_pull_offs_and_slides_join_the_next_fret_on_their_string() {
        let on = |string, fret, start| Note {
            start,
            string: Some(string),
            fret: Some(fret),
            pitch: Some(Pitch { step: 0, alter: 0, octave: 4 }),
            ..Note::default()
        };
        // String 3: 5 h 7 p 5, then a slide that goes on to the next line. String 2 apart.
        let mut a = on(3, 5, 0);
        a.technique.hammer_on = true;
        let mut b = on(3, 7, 960);
        b.technique.pull_off = true;
        let c = on(2, 8, 1440);
        let d = on(3, 5, 1920);
        let mut e = on(3, 3, 2880);
        e.technique.slide = true;
        let b1 = bar(vec![a, b, c, d, e]);
        assert_eq!(
            tab_links(&[&b1]),
            vec![
                TabLink { from: (0, 0), to: Some((0, 1)), kind: LinkKind::HammerOn },
                TabLink { from: (0, 1), to: Some((0, 3)), kind: LinkKind::PullOff },
                TabLink { from: (0, 4), to: None, kind: LinkKind::Slide },
            ]
        );
    }

    #[test]
    fn the_keyboard_moves_through_bars_and_lines() {
        // Two lines: bars 0-3 and 4-5.
        let systems = vec![
            System { first: 0, widths: vec![10.0; 4], header: 0.0 },
            System { first: 4, widths: vec![10.0; 2], header: 0.0 },
        ];
        let go = |cursor, key| move_cursor(cursor, key, &systems, 6);
        assert_eq!(go(0, Key::ArrowLeft), 0, "stays at the first bar");
        assert_eq!(go(0, Key::ArrowRight), 1);
        assert_eq!(go(3, Key::ArrowRight), 4, "on to the next line");
        assert_eq!(go(5, Key::ArrowRight), 5, "stays at the last bar");
        assert_eq!(go(1, Key::ArrowDown), 5, "same place on the next line");
        assert_eq!(go(3, Key::ArrowDown), 5, "or its last bar when the line is shorter");
        assert_eq!(go(5, Key::ArrowUp), 1);
        assert_eq!(go(2, Key::ArrowUp), 2, "no line above");
        assert_eq!(go(4, Key::ArrowDown), 4, "no line below");
        assert_eq!((go(3, Key::Home), go(1, Key::End)), (0, 5));
        assert_eq!(go(99, Key::A), 5, "a cursor past the end comes back");
        assert_eq!((line_of(&systems, 4), line_of(&systems, 9)), (Some(1), None));
    }

    #[test]
    fn bars_with_bends_are_wider() {
        let m = Metrics::new(1.0);
        let plain = bar((0..4).map(|i| quarter(i * 960)).collect());
        let mut bent = plain.clone();
        bent.notes[0].technique.bend = Some(2.0);
        assert!(bar_width(&bent, m) > bar_width(&plain, m));
    }

    #[test]
    fn bends_are_labelled_in_whole_tones() {
        assert_eq!(bend_label(1.0), "1/2");
        assert_eq!(bend_label(2.0), "full");
        assert_eq!(bend_label(3.0), "1 1/2");
        assert_eq!(bend_label(4.0), "2");
        assert_eq!(bend_label(f32::NAN), "1/2", "nonsense stays small");
    }

    #[test]
    fn tuplets_group_from_start_to_stop_with_a_bracket_when_unbeamed() {
        use actcraft_engine::notation::Tuplet;
        let three = |start, stop| Tuplet { actual: 3, normal: 2, start, stop, bracket: None };
        // An eighth triplet (beamed: number only), then a quarter triplet (bracket).
        let mut notes: Vec<Note> = (0..3).map(|i| eighth(i * 320, 2)).collect();
        notes.extend((0..3).map(|i| Note { start: 960 + i * 640, ..quarter(0) }));
        for (k, n) in notes.iter_mut().enumerate() {
            n.tuplet = three(k % 3 == 0, k % 3 == 2);
        }
        let b = bar(notes);
        let evs = events(&b, 1);
        assert_eq!(
            tuplet_groups(&evs),
            vec![
                TupletGroup { first: 0, last: 2, actual: 3, bracket: false },
                TupletGroup { first: 3, last: 5, actual: 3, bracket: true },
            ]
        );
        // The file's word wins; a start without a stop draws nothing.
        let mut b2 = b.clone();
        b2.notes[0].tuplet.bracket = Some(true);
        b2.notes[5].tuplet.stop = false;
        assert_eq!(tuplet_groups(&events(&b2, 1)), vec![TupletGroup { first: 0, last: 2, actual: 3, bracket: true }]);
    }

    #[test]
    fn ties_and_slurs_join_their_notes_across_bars_and_lines() {
        use actcraft_engine::notation::Slur;
        let slur = |number, start| Slur { number, start, above: None };
        // Bar 1: C tied over the bar line; a slur from E (bar 1) to D (bar 2).
        let mut b1 = bar(vec![quarter(0), eighth(960, 2)]);
        b1.notes[0].tie_start = true;
        b1.notes[1].slurs = vec![slur(1, true)];
        let mut b2 = bar(vec![quarter(0), eighth(960, 1)]);
        b2.notes[0].tied = true;
        b2.notes[1].slurs = vec![slur(1, false), slur(1, true)];
        let got = arcs(&[&b1, &b2], 1);
        assert_eq!(
            got,
            vec![
                Arc { from: Some((0, 0)), to: Some((1, 0)), above: None, tie: true },
                Arc { from: Some((0, 1)), to: Some((1, 1)), above: None, tie: false },
                // The new slur from D ends on the next line.
                Arc { from: Some((1, 1)), to: None, above: None, tie: false },
            ]
        );
        // On the next line the tie's and slur's ends come from its start; other staves are apart.
        assert_eq!(arcs(&[&b2], 1)[0], Arc { from: None, to: Some((0, 0)), above: None, tie: true });
        assert!(arcs(&[&b1, &b2], 2).is_empty());
        // A tie joins notes of the same pitch only.
        let mut c = bar(vec![eighth(0, 2)]);
        c.notes[0].tied = true;
        assert_eq!(arcs(&[&b1, &c], 1).first().map(|a| a.from), Some(None));
    }

    #[test]
    fn a_click_goes_to_the_pass_nearest_the_playing_one() {
        // |: 1 2 :| 3, played 1 2 1 2 3.
        let order = [0, 1, 0, 1, 2];
        assert_eq!(played_index(&order, 0, None), Some(0), "nothing plays: the first pass");
        assert_eq!(played_index(&order, 0, Some(3)), Some(2), "during the second pass");
        assert_eq!(played_index(&order, 2, Some(0)), Some(4));
        assert_eq!(played_index(&order, 7, Some(0)), None, "a bar never played");
        // The playing bar on the second pass is written bar 1 again.
        let song = Song { bpm: 120.0, beats_per_bar: 4, beat_anchor: 0.0, ..Song::default() };
        let played = playing_bar(&song, 4.0, order.len());
        assert_eq!(played.and_then(|(k, _)| order.get(k).copied()), Some(0));
        assert!((bar_time(&song, 2) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn repeat_signs_take_room_from_the_notes() {
        let m = Metrics::new(1.0);
        let plain = bar(vec![quarter(0)]);
        let mut repeated = plain.clone();
        repeated.flow.repeat_start = true;
        repeated.flow.repeat_end = Some(2);
        assert!(bar_width(&repeated, m) > bar_width(&plain, m) + 2.0 * m.space);
    }

    #[test]
    fn the_score_can_start_on_another_song_bar() {
        // Two bars of intro in the audio before the score starts: score bar 1 is song bar 3.
        let song = Song { bpm: 120.0, beats_per_bar: 4, beat_anchor: 1.0, score_first_bar: 3, ..Song::default() };
        assert_eq!(playing_bar(&song, 1.0, 10), None, "the intro is not in the score");
        assert_eq!(playing_bar(&song, 5.0, 10), Some((0, 0.0)));
        assert_eq!(playing_bar(&song, 8.0, 10), Some((1, 0.5)));
        assert!((bar_time(&song, 0) - 5.0).abs() < 1e-9, "a click on bar 1 plays from song bar 3");
        assert!((bar_time(&song, 2) - 9.0).abs() < 1e-9);
        // A pickup bar: score bar 1 is song bar 0, the bar before the beat anchor.
        let song = Song { score_first_bar: 0, beat_anchor: 3.0, ..song };
        assert_eq!(playing_bar(&song, 0.5, 10), None, "before the pickup");
        assert_eq!(playing_bar(&song, 1.0, 10), Some((0, 0.0)), "the pickup bar");
        assert_eq!(playing_bar(&song, 3.0, 10), Some((1, 0.0)), "score bar 2 starts on beat one");
        assert!((bar_time(&song, 0) - 1.0).abs() < 1e-9);
    }
}
