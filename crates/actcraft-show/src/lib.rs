//! The show model and the `.actcraft` file format.
//!
//! One show file holds the whole performance: songs (with their audio tracks, regions, lyrics and
//! lighting lane), playlists, mixer and metronome settings and the play history. The format is
//! JSON through serde, carries a format tag and a version, loads files written by older versions
//! and keeps fields it doesn't know (so a newer file opened and saved by an older ActCraft loses
//! nothing). Everything read from a file is clamped by [`Show::normalise`] before use.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub mod limits {
    //! Hard limits. Input beyond them is cut, never trusted.
    /// Audio tracks per song (the product promises at least 32).
    pub const MAX_TRACKS: usize = 64;
    /// Output buses (at least 16).
    pub const BUSES: usize = 16;
    /// Mute groups.
    pub const MUTE_GROUPS: usize = 6;
    /// Named regions per song.
    pub const MAX_REGIONS: usize = 64;
    /// Songs in one show.
    pub const MAX_SONGS: usize = 20_000;
    /// Playlists in one show.
    pub const MAX_PLAYLISTS: usize = 2_000;
    /// Sets per playlist.
    pub const MAX_SETS: usize = 64;
    /// Songs per set.
    pub const MAX_SET_SONGS: usize = 1_000;
    /// Smart playlist rules.
    pub const MAX_RULES: usize = 32;
    /// Played-song history entries kept.
    pub const MAX_HISTORY: usize = 500;
    /// Keywords per song.
    pub const MAX_KEYWORDS: usize = 64;
    /// Bytes of a show file we are willing to parse.
    pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
    /// Bytes of one text field (titles, names, paths).
    pub const MAX_TEXT: usize = 1024;
    /// Longest spoken cue (a few words; speech takes a moment per word).
    pub const MAX_VOICE_TEXT: usize = 200;
    /// Bytes of lyrics per song.
    pub const MAX_LYRICS: usize = 512 * 1024;
    /// Longest song, seconds (24 h).
    pub const MAX_SECONDS: f64 = 86_400.0;
    /// Marks on one song's PDF.
    pub const MAX_PDF_MARKS: usize = 4_000;
    /// Points in one pen stroke.
    pub const MAX_MARK_POINTS: usize = 4_096;
    /// Highest PDF page a mark may sit on (0-based).
    pub const MAX_PDF_PAGE: u32 = 9_999;
    /// Earliest song bar a score's first bar may sit on (a long lead-in before the anchor).
    pub const MIN_SCORE_FIRST_BAR: i32 = -999;
    /// Latest song bar a score's first bar may sit on.
    pub const MAX_SCORE_FIRST_BAR: i32 = 9_999;
}

/// The format tag written into every show file.
pub const FORMAT_TAG: &str = "actcraft-show";
/// The version this build writes.
pub const FORMAT_VERSION: u32 = 1;

/// Why a show file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowError {
    TooLarge(usize),
    NotJson(String),
    NotAShow,
}

impl fmt::Display for ShowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(n) => {
                write!(f, "show file is {n} bytes, more than the {} byte limit", limits::MAX_FILE_BYTES)
            }
            Self::NotJson(e) => write!(f, "show file is not valid: {e}"),
            Self::NotAShow => write!(f, "this file is not an ActCraft show"),
        }
    }
}

impl std::error::Error for ShowError {}

fn default_tag() -> String {
    FORMAT_TAG.to_owned()
}

/// A whole show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Show {
    #[serde(default = "default_tag")]
    pub format: String,
    /// Version of the writer; files from older versions are migrated on load.
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub songs: Vec<Song>,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub mute_groups: Vec<MuteGroup>,
    #[serde(default)]
    pub metronome: MetronomeSettings,
    #[serde(default)]
    pub voice: VoiceSettings,
    /// Time sent to other gear over MIDI while a song plays.
    #[serde(default)]
    pub midi: MidiSettings,
    #[serde(default)]
    pub mixer: MixerSettings,
    #[serde(default)]
    pub transitions: TransitionSettings,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Fields this version doesn't know, kept and written back unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Show {
    fn default() -> Self {
        let mut show = Self {
            format: default_tag(),
            version: FORMAT_VERSION,
            name: String::new(),
            songs: Vec::new(),
            playlists: Vec::new(),
            buses: Vec::new(),
            mute_groups: Vec::new(),
            metronome: MetronomeSettings::default(),
            voice: VoiceSettings::default(),
            midi: MidiSettings::default(),
            mixer: MixerSettings::default(),
            transitions: TransitionSettings::default(),
            history: Vec::new(),
            extra: Map::new(),
        };
        show.normalise();
        show
    }
}

/// One song: its metadata, audio tracks, regions, lyrics and lighting lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    /// Stable id inside the show; playlists and history refer to it.
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub key: String,
    pub bpm: f64,
    /// Beats per bar.
    pub beats_per_bar: u8,
    pub year: Option<i32>,
    /// Length in seconds (from the longest track, or set by hand).
    pub duration: f64,
    /// Keywords; case matters (`Live` and `live` are different keywords).
    pub keywords: Vec<String>,
    pub capo: i8,
    pub transpose: i8,
    /// Where playback starts, seconds.
    pub start: f64,
    /// Where playback ends, seconds; `None` plays to the end.
    pub end: Option<f64>,
    /// Bars of count-in before the song starts.
    pub count_in_bars: u8,
    /// When this song's click sounds.
    pub click: SongClick,
    /// Time of beat one of bar one, seconds, independent of where playback starts.
    pub beat_anchor: f64,
    /// Song volume in dB.
    pub volume_db: f64,
    pub tracks: Vec<Track>,
    pub regions: Vec<Region>,
    /// Lyrics source text (markup and ChordPro chords).
    pub lyrics: String,
    /// How the lyrics follow playback.
    pub lyrics_scroll: LyricsScroll,
    /// A MIDI file played in sync with the audio.
    pub midi_file: Option<String>,
    /// A PDF shown in the Sheet music tab (sheet music, lyrics, notes).
    pub pdf_file: Option<String>,
    /// Marks drawn on the PDF's pages: pen strokes, highlights, notes.
    pub pdf_marks: Vec<PdfMark>,
    /// A score (MusicXML) shown as notes and tablature in the Sheet music tab.
    pub notation_file: Option<String>,
    /// The song bar (beat grid numbering: 1 at the beat anchor, 0 and below before it) where
    /// the score's first bar plays; 0 or less for a score that starts with a pickup bar.
    pub score_first_bar: i32,
    /// Start the next song when this one ends.
    pub autoplay: bool,
    /// Loop the whole song.
    pub loop_song: bool,
    /// Speak the title when the song starts (see the show's voice settings).
    pub announce_title: bool,
    /// Fade-out length at the song end, seconds; 0 for none.
    pub fade_out: f64,
    /// Lighting cues on the song timeline (filled from milestone M7).
    pub lighting_lane: Vec<LaneCue>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Song {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            artist: String::new(),
            key: String::new(),
            bpm: 120.0,
            beats_per_bar: 4,
            year: None,
            duration: 0.0,
            keywords: Vec::new(),
            capo: 0,
            transpose: 0,
            start: 0.0,
            end: None,
            count_in_bars: 0,
            click: SongClick::default(),
            beat_anchor: 0.0,
            volume_db: 0.0,
            tracks: Vec::new(),
            regions: Vec::new(),
            lyrics: String::new(),
            lyrics_scroll: LyricsScroll::default(),
            midi_file: None,
            pdf_file: None,
            pdf_marks: Vec::new(),
            notation_file: None,
            score_first_bar: 1,
            autoplay: false,
            loop_song: false,
            announce_title: false,
            fade_out: 0.0,
            lighting_lane: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// One audio track of a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Audio file, relative to the show file or absolute.
    pub file: String,
    pub volume_db: f64,
    /// -1 left .. 1 right.
    pub pan: f64,
    pub eq: [EqBand; 4],
    pub mute: bool,
    /// Output bus, 0-based.
    pub bus: u8,
    /// Spread a mono file to both sides of the bus.
    pub mono_to_stereo: bool,
    /// Mute groups this track belongs to (bit n = group n+1).
    pub mute_groups: u8,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            name: String::new(),
            colour: "#4f8cc9".to_owned(),
            file: String::new(),
            volume_db: 0.0,
            pan: 0.0,
            eq: EqBand::defaults(),
            mute: false,
            bus: 0,
            mono_to_stereo: true,
            mute_groups: 0,
            extra: Map::new(),
        }
    }
}

/// One EQ band: frequency, gain and width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqBand {
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

impl Default for EqBand {
    fn default() -> Self {
        Self { freq: 1000.0, gain_db: 0.0, q: 0.7 }
    }
}

impl EqBand {
    /// Low shelf, two bells, high shelf, all flat.
    pub fn defaults() -> [EqBand; 4] {
        [
            EqBand { freq: 100.0, gain_db: 0.0, q: 0.7 },
            EqBand { freq: 500.0, gain_db: 0.0, q: 1.0 },
            EqBand { freq: 2500.0, gain_db: 0.0, q: 1.0 },
            EqBand { freq: 8000.0, gain_db: 0.0, q: 0.7 },
        ]
    }
}

/// What a mark on a PDF page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfMarkKind {
    /// A free-hand stroke through its points.
    #[default]
    Pen,
    /// A see-through box between its two corner points.
    Highlight,
    /// A text note at its point.
    Note,
}

/// A mark on a page of a song's PDF. Points are fractions of the page (0..1 from the top left),
/// so a mark stays on its spot at any zoom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PdfMark {
    /// Page, 0-based.
    pub page: u32,
    pub kind: PdfMarkKind,
    /// `#rrggbb`.
    pub colour: String,
    pub points: Vec<[f32; 2]>,
    /// The note's words (notes only).
    pub text: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for PdfMark {
    fn default() -> Self {
        Self {
            page: 0,
            kind: PdfMarkKind::Pen,
            colour: "#e5484d".to_owned(),
            points: Vec::new(),
            text: String::new(),
            extra: Map::new(),
        }
    }
}

impl PdfMark {
    /// Points it needs at least: two for a stroke or a box, one for a note.
    pub fn min_points(&self) -> usize {
        if self.kind == PdfMarkKind::Note { 1 } else { 2 }
    }

    fn normalise(&mut self) {
        use limits::*;
        self.page = self.page.min(MAX_PDF_PAGE);
        fix_colour(&mut self.colour, "#e5484d");
        self.points.retain(|p| p.iter().all(|v| v.is_finite()));
        self.points.truncate(if self.kind == PdfMarkKind::Pen { MAX_MARK_POINTS } else { self.min_points() });
        for p in &mut self.points {
            for v in p.iter_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        cut(&mut self.text, MAX_TEXT);
    }
}

/// What playback does when it reaches a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionMode {
    #[default]
    Play,
    Loop,
    Skip,
    StopAfter,
}

/// A named, coloured part of a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Region {
    pub name: String,
    pub colour: String,
    pub start: f64,
    pub end: f64,
    pub mode: RegionMode,
    /// Shown in the cue list for live jumps.
    pub cue: bool,
    /// Spoken shortly before the region starts ("Chorus", "End 3 2 1"); empty for nothing.
    #[serde(default)]
    pub voice: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            name: String::new(),
            colour: "#8a6fd1".to_owned(),
            start: 0.0,
            end: 0.0,
            mode: RegionMode::Play,
            cue: false,
            voice: String::new(),
            extra: Map::new(),
        }
    }
}

/// Spoken cues: a song's title when it starts and a region's words before it begins, made by the
/// computer's own speech and played on a bus of their own choosing (usually the in-ear mix).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub enabled: bool,
    /// Bus the voice goes to, 0-based (the click bus by default).
    pub bus: u8,
    pub volume_db: f64,
    /// How many beats before a region its words are spoken.
    pub lead_beats: u8,
    /// Speech language (`de`, `en`...); empty for the computer's own.
    pub language: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self { enabled: true, bus: 1, volume_db: 0.0, lead_beats: 4, language: String::new() }
    }
}

/// Time sent to other gear (lights, effects, a second computer) over MIDI while a song plays.
/// Which MIDI output it goes to is chosen per computer, in Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiSettings {
    /// MIDI Clock (24 per quarter note) with start, stop and song position.
    pub clock: bool,
    /// MIDI Timecode (quarter frames, a full frame on a jump).
    pub timecode: bool,
    /// Frames per second of the timecode: 24, 25 or 30.
    pub timecode_fps: u8,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl MidiSettings {
    /// Frame rates the timecode can run at.
    pub const RATES: [u8; 3] = [24, 25, 30];
}

impl Default for MidiSettings {
    fn default() -> Self {
        Self { clock: false, timecode: false, timecode_fps: 25, extra: Map::new() }
    }
}

/// A lighting cue placed on the song timeline (kept opaque until milestone M7).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LaneCue {
    pub at: f64,
    pub cue: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An output bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bus {
    pub name: String,
    pub volume_db: f64,
    pub mute: bool,
    /// First output channel of the stereo pair on the audio interface (0-based).
    pub output: u16,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Bus {
    fn default() -> Self {
        Self { name: String::new(), volume_db: 0.0, mute: false, output: 0, extra: Map::new() }
    }
}

/// A mute group with its label.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MuteGroup {
    pub label: String,
    pub active: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// How the metronome sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetronomeMode {
    /// Clicks on every beat.
    #[default]
    Audible,
    /// Clicks only during the count-in.
    CountInOnly,
    /// No sound; the beat is only shown.
    VisualOnly,
}

impl MetronomeMode {
    pub const ALL: [MetronomeMode; 3] = [Self::Audible, Self::CountInOnly, Self::VisualOnly];

    pub fn next(self) -> Self {
        match self {
            Self::Audible => Self::CountInOnly,
            Self::CountInOnly => Self::VisualOnly,
            Self::VisualOnly => Self::Audible,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Audible => "Audible",
            Self::CountInOnly => "Count-in only",
            Self::VisualOnly => "Visual only",
        }
    }
}

/// When a song's click sounds (the toolbar's Metronome switch turns every click on or off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongClick {
    /// No click for this song.
    Off,
    /// Click through the whole song (and its count-in).
    #[default]
    WholeSong,
    /// Click only while counting in.
    CountInOnly,
    /// No sound; the beat is only shown.
    VisualOnly,
}

impl SongClick {
    pub const ALL: [SongClick; 4] = [Self::WholeSong, Self::CountInOnly, Self::VisualOnly, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::WholeSong => "Whole song",
            Self::CountInOnly => "Count-in only",
            Self::VisualOnly => "Visual only",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::WholeSong => "whole_song",
            Self::CountInOnly => "count_in_only",
            Self::VisualOnly => "visual_only",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }

    pub fn next(self) -> Self {
        match self {
            Self::WholeSong => Self::CountInOnly,
            Self::CountInOnly => Self::VisualOnly,
            Self::VisualOnly => Self::Off,
            Self::Off => Self::WholeSong,
        }
    }
}

/// How the lyrics follow playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsScroll {
    /// Jump to the playing line (timecoded lyrics).
    #[default]
    Line,
    /// Glide between timecoded lines so the playing words stay in the middle.
    Smooth,
    /// Scroll at a constant speed from the song's start to its end (lyrics without timecodes).
    Constant,
    /// Don't scroll.
    Off,
}

impl LyricsScroll {
    pub const ALL: [LyricsScroll; 4] = [Self::Line, Self::Smooth, Self::Constant, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Jump to the line",
            Self::Smooth => "Smooth",
            Self::Constant => "Constant speed",
            Self::Off => "Don't scroll",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Smooth => "smooth",
            Self::Constant => "constant",
            Self::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }
}

/// Metronome settings shared by all songs (BPM and count-in are per song).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetronomeSettings {
    pub enabled: bool,
    pub mode: MetronomeMode,
    pub volume_db: f64,
    /// Bus the click goes to, 0-based.
    pub bus: u8,
    /// Start playback on the next beat of a running metronome.
    pub quantized_start: bool,
    /// Keep clicking while stopped (off: the click only sounds with a playing song).
    pub run_while_stopped: bool,
    /// Moves the click earlier (positive) to make up for output latency, milliseconds.
    pub latency_offset_ms: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: MetronomeMode::Audible,
            volume_db: -6.0,
            bus: 1,
            quantized_start: false,
            run_while_stopped: false,
            latency_offset_ms: 0.0,
            extra: Map::new(),
        }
    }
}

/// Master section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerSettings {
    pub master_volume_db: f64,
    /// Moves the MIDI file earlier to make up for output latency, milliseconds.
    pub midi_latency_offset_ms: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self { master_volume_db: 0.0, midi_latency_offset_ms: 0.0, extra: Map::new() }
    }
}

/// Song-to-song transitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransitionSettings {
    /// Crossfade length when changing songs during playback, seconds; 0 cuts.
    pub crossfade: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for TransitionSettings {
    fn default() -> Self {
        Self { crossfade: 0.0, extra: Map::new() }
    }
}

/// A playlist, split into sets, or a smart playlist built from rules.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub name: String,
    pub sets: Vec<Set>,
    /// When present, the songs come from these rules instead of the sets.
    pub smart: Option<SmartRules>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One set of a playlist.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Set {
    pub name: String,
    pub songs: Vec<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Rules of a smart playlist.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartRules {
    /// `true`: a song must match every rule; `false`: any rule.
    pub match_all: bool,
    pub rules: Vec<SmartRule>,
}

/// One smart playlist rule, e.g. `bpm` `>` `120`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartRule {
    pub field: String,
    pub op: String,
    pub value: String,
}

/// One played song.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    pub song: u64,
    /// RFC 3339 time it started.
    pub played_at: String,
    /// Marked as done by the performer.
    pub done: bool,
}

/// Clamps a number into a range; NaN and infinities become `fallback`.
pub fn clamp_f(v: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if v.is_finite() { v.clamp(min, max) } else { fallback }
}

/// Cuts text to `max` bytes on a character boundary.
pub fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// Parses `#rrggbb` into RGB.
pub fn parse_colour(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn fix_colour(s: &mut String, fallback: &str) {
    if parse_colour(s).is_none() {
        *s = fallback.to_owned();
    }
}

impl Song {
    fn normalise(&mut self) {
        use limits::*;
        for s in [&mut self.title, &mut self.artist, &mut self.key] {
            cut(s, MAX_TEXT);
        }
        self.bpm = clamp_f(self.bpm, 20.0, 400.0, 120.0);
        self.beats_per_bar = self.beats_per_bar.clamp(1, 16);
        self.year = self.year.map(|y| y.clamp(0, 9999));
        self.duration = clamp_f(self.duration, 0.0, MAX_SECONDS, 0.0);
        self.keywords.truncate(MAX_KEYWORDS);
        for k in &mut self.keywords {
            cut(k, MAX_TEXT);
        }
        self.capo = self.capo.clamp(0, 12);
        self.transpose = self.transpose.clamp(-12, 12);
        self.start = clamp_f(self.start, 0.0, MAX_SECONDS, 0.0);
        self.end = self.end.map(|e| clamp_f(e, 0.0, MAX_SECONDS, MAX_SECONDS)).filter(|&e| e > self.start);
        self.count_in_bars = self.count_in_bars.min(8);
        self.beat_anchor = clamp_f(self.beat_anchor, -MAX_SECONDS, MAX_SECONDS, 0.0);
        self.volume_db = clamp_f(self.volume_db, -96.0, 12.0, 0.0);
        self.tracks.truncate(MAX_TRACKS);
        for t in &mut self.tracks {
            t.normalise();
        }
        self.regions.truncate(MAX_REGIONS);
        for r in &mut self.regions {
            cut(&mut r.name, MAX_TEXT);
            cut(&mut r.voice, MAX_VOICE_TEXT);
            fix_colour(&mut r.colour, "#8a6fd1");
            r.start = clamp_f(r.start, 0.0, MAX_SECONDS, 0.0);
            r.end = clamp_f(r.end, r.start, MAX_SECONDS, r.start);
        }
        cut(&mut self.lyrics, MAX_LYRICS);
        if let Some(m) = &mut self.pdf_file {
            cut(m, MAX_TEXT);
        }
        self.pdf_marks.truncate(MAX_PDF_MARKS);
        for m in &mut self.pdf_marks {
            m.normalise();
        }
        self.pdf_marks.retain(|m| m.points.len() >= m.min_points());
        if let Some(m) = &mut self.notation_file {
            cut(m, MAX_TEXT);
        }
        self.score_first_bar = self.score_first_bar.clamp(MIN_SCORE_FIRST_BAR, MAX_SCORE_FIRST_BAR);
        if let Some(m) = &mut self.midi_file {
            cut(m, MAX_TEXT);
        }
        self.fade_out = clamp_f(self.fade_out, 0.0, 60.0, 0.0);
        for c in &mut self.lighting_lane {
            c.at = clamp_f(c.at, 0.0, MAX_SECONDS, 0.0);
            cut(&mut c.cue, MAX_TEXT);
        }
    }
}

impl Track {
    fn normalise(&mut self) {
        use limits::*;
        cut(&mut self.name, MAX_TEXT);
        cut(&mut self.file, MAX_TEXT);
        fix_colour(&mut self.colour, "#4f8cc9");
        self.volume_db = clamp_f(self.volume_db, -96.0, 12.0, 0.0);
        self.pan = clamp_f(self.pan, -1.0, 1.0, 0.0);
        for b in &mut self.eq {
            b.freq = clamp_f(b.freq, 20.0, 20_000.0, 1000.0);
            b.gain_db = clamp_f(b.gain_db, -24.0, 24.0, 0.0);
            b.q = clamp_f(b.q, 0.1, 18.0, 0.7);
        }
        if usize::from(self.bus) >= BUSES {
            self.bus = 0;
        }
        self.mute_groups &= (1u8 << MUTE_GROUPS) - 1;
    }
}

impl Show {
    /// Brings every value into its allowed range and fills fixed-size lists. Safe to call twice.
    pub fn normalise(&mut self) {
        use limits::*;
        cut(&mut self.name, MAX_TEXT);
        self.songs.truncate(MAX_SONGS);
        for s in &mut self.songs {
            s.normalise();
        }
        // Ids must be unique and non-zero: give clashing songs fresh ids.
        let mut seen = std::collections::BTreeSet::new();
        let mut next = self.songs.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1);
        for s in &mut self.songs {
            if s.id == 0 || !seen.insert(s.id) {
                s.id = next;
                next = next.saturating_add(1);
                seen.insert(s.id);
            }
        }
        self.buses.truncate(BUSES);
        while self.buses.len() < BUSES {
            let n = self.buses.len();
            self.buses.push(Bus {
                name: if n == 0 {
                    "Main".to_owned()
                } else if n == 1 {
                    "Click".to_owned()
                } else {
                    format!("Bus {}", n + 1)
                },
                // Every new bus starts on outputs 1/2, so nothing is silent until the person
                // routes it elsewhere (a click on outputs 3/4 is inaudible on most setups).
                output: 0,
                ..Bus::default()
            });
        }
        for b in &mut self.buses {
            cut(&mut b.name, MAX_TEXT);
            b.volume_db = clamp_f(b.volume_db, -96.0, 12.0, 0.0);
            b.output = b.output.min(255);
        }
        self.mute_groups.truncate(MUTE_GROUPS);
        while self.mute_groups.len() < MUTE_GROUPS {
            let n = self.mute_groups.len();
            self.mute_groups.push(MuteGroup { label: format!("Group {}", n + 1), ..MuteGroup::default() });
        }
        for g in &mut self.mute_groups {
            cut(&mut g.label, MAX_TEXT);
        }
        let m = &mut self.metronome;
        m.volume_db = clamp_f(m.volume_db, -96.0, 12.0, -6.0);
        if usize::from(m.bus) >= BUSES {
            m.bus = 0;
        }
        m.latency_offset_ms = clamp_f(m.latency_offset_ms, -500.0, 500.0, 0.0);
        let v = &mut self.voice;
        v.volume_db = clamp_f(v.volume_db, -96.0, 12.0, 0.0);
        if usize::from(v.bus) >= BUSES {
            v.bus = 0;
        }
        v.lead_beats = v.lead_beats.min(32);
        cut(&mut v.language, 16);
        self.mixer.master_volume_db = clamp_f(self.mixer.master_volume_db, -96.0, 12.0, 0.0);
        self.mixer.midi_latency_offset_ms = clamp_f(self.mixer.midi_latency_offset_ms, -500.0, 500.0, 0.0);
        if !MidiSettings::RATES.contains(&self.midi.timecode_fps) {
            self.midi.timecode_fps = 25;
        }
        self.transitions.crossfade = clamp_f(self.transitions.crossfade, 0.0, 30.0, 0.0);
        self.playlists.truncate(MAX_PLAYLISTS);
        for p in &mut self.playlists {
            cut(&mut p.name, MAX_TEXT);
            p.sets.truncate(MAX_SETS);
            if p.sets.is_empty() && p.smart.is_none() {
                p.sets.push(Set { name: "Set 1".to_owned(), ..Set::default() });
            }
            for s in &mut p.sets {
                cut(&mut s.name, MAX_TEXT);
                s.songs.truncate(MAX_SET_SONGS);
            }
            if let Some(smart) = &mut p.smart {
                smart.rules.truncate(MAX_RULES);
                for r in &mut smart.rules {
                    cut(&mut r.field, MAX_TEXT);
                    cut(&mut r.op, 8);
                    cut(&mut r.value, MAX_TEXT);
                }
            }
        }
        if self.history.len() > MAX_HISTORY {
            let drop = self.history.len() - MAX_HISTORY;
            self.history.drain(..drop);
        }
        for h in &mut self.history {
            cut(&mut h.played_at, 64);
        }
    }

    /// Reads a show file. Older versions are migrated; unknown fields are kept.
    pub fn from_bytes(bytes: &[u8]) -> Result<Show, ShowError> {
        if bytes.len() > limits::MAX_FILE_BYTES {
            return Err(ShowError::TooLarge(bytes.len()));
        }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let value: Value = serde_json::from_slice(bytes).map_err(|e| ShowError::NotJson(e.to_string()))?;
        let Value::Object(obj) = &value else { return Err(ShowError::NotAShow) };
        match obj.get("format") {
            Some(Value::String(tag)) if tag == FORMAT_TAG => {}
            _ => return Err(ShowError::NotAShow),
        }
        let mut show: Show = serde_json::from_value(value).map_err(|e| ShowError::NotJson(e.to_string()))?;
        show.migrate();
        show.normalise();
        Ok(show)
    }

    /// Writes the show as pretty JSON with the current version.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.clone();
        out.format = default_tag();
        // A newer file keeps its version so an older writer doesn't claim to understand it.
        out.version = out.version.max(FORMAT_VERSION);
        serde_json::to_vec_pretty(&out).unwrap_or_default()
    }

    fn migrate(&mut self) {
        // Version 0 files predate the version field; their fields are a subset of version 1, so
        // only the number moves. Later migrations go here, oldest first.
        if self.version == 0 {
            self.version = 1;
        }
    }

    /// A fresh song id.
    pub fn next_song_id(&self) -> u64 {
        self.songs.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1)
    }

    pub fn song(&self, id: u64) -> Option<&Song> {
        self.songs.iter().find(|s| s.id == id)
    }

    pub fn song_mut(&mut self, id: u64) -> Option<&mut Song> {
        self.songs.iter_mut().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_unknown_fields() {
        let src = br##"{"format":"actcraft-show","version":7,"future":{"a":1},
            "songs":[{"id":3,"title":"A","tracks":[{"name":"Drums","new_field":true}],"zz":2}]}"##;
        let show = Show::from_bytes(src).unwrap_or_default();
        assert_eq!(show.songs.len(), 1);
        let out = String::from_utf8(show.to_bytes()).unwrap_or_default();
        assert!(out.contains("\"future\""));
        assert!(out.contains("\"new_field\""));
        assert!(out.contains("\"zz\""));
        assert!(out.contains("\"version\": 7"));
    }

    #[test]
    fn old_file_without_version_loads() {
        let show = Show::from_bytes(br#"{"format":"actcraft-show","songs":[{"title":"x"}]}"#);
        assert!(show.is_ok_and(|s| s.version == 1 && s.songs.first().is_some_and(|x| x.id != 0)));
    }

    #[test]
    fn hostile_pdf_marks_are_bounded_or_dropped() {
        let src = br##"{"format":"actcraft-show","version":1,"songs":[{"id":1,"pdf_marks":[
            {"page":4000000000,"kind":"pen","colour":"red","points":[[-1,0.5],[0.5,9],[1e40,0.2]]},
            {"page":1,"kind":"highlight","points":[[0.1,0.1],[0.2,0.2],[0.3,0.3]]},
            {"page":1,"kind":"note","points":[],"text":"lost"},
            {"page":1,"kind":"pen","points":[[0.1,0.1]]}]}]}"##;
        let show = Show::from_bytes(src);
        assert!(show.is_ok(), "{show:?}");
        let marks = show.ok().and_then(|s| s.songs.first().map(|x| x.pdf_marks.clone())).unwrap_or_default();
        assert_eq!(marks.len(), 2, "marks without enough points are dropped: {marks:?}");
        assert_eq!(marks[0].page, limits::MAX_PDF_PAGE);
        assert_eq!(marks[0].colour, "#e5484d");
        assert_eq!(marks[0].points, vec![[0.0, 0.5], [0.5, 1.0]], "kept inside the page; the infinite point left out");
        assert_eq!(marks[1].points.len(), 2, "a box keeps two corners");
    }

    #[test]
    fn score_first_bar_defaults_to_bar_one_and_is_clamped() {
        let show = Show::from_bytes(br#"{"format":"actcraft-show","version":1,"songs":[{"title":"x"}]}"#);
        assert_eq!(show.ok().and_then(|s| s.songs.first().map(|x| x.score_first_bar)), Some(1), "older files");
        let show = Show::from_bytes(br#"{"format":"actcraft-show","version":1,"songs":[{"score_first_bar":-5000}]}"#);
        assert_eq!(
            show.ok().and_then(|s| s.songs.first().map(|x| x.score_first_bar)),
            Some(limits::MIN_SCORE_FIRST_BAR)
        );
    }

    #[test]
    fn hostile_values_are_clamped() {
        let src = br#"{"format":"actcraft-show","songs":[{"id":1,"bpm":1e308,"tracks":[{"pan":99,"bus":200,"colour":"red"}]},{"id":1}]}"#;
        let show = Show::from_bytes(src).unwrap_or_default();
        let ids: Vec<u64> = show.songs.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids.first(), ids.get(1));
        let t = show.songs.first().and_then(|s| s.tracks.first()).cloned().unwrap_or_default();
        assert!(t.pan <= 1.0 && usize::from(t.bus) < limits::BUSES && parse_colour(&t.colour).is_some());
        assert_eq!(show.buses.len(), limits::BUSES);
    }

    #[test]
    fn rejects_other_json() {
        assert_eq!(Show::from_bytes(b"{}"), Err(ShowError::NotAShow));
        assert!(matches!(Show::from_bytes(b"\xff"), Err(ShowError::NotJson(_))));
    }
}
