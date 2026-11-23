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
    /// Remote actions (MIDI message to command) in one show.
    pub const MAX_REMOTE_ACTIONS: usize = 256;
    /// Macros in one show, and commands in one macro.
    pub const MAX_MACROS: usize = 128;
    pub const MAX_MACRO_COMMANDS: usize = 32;
    /// Pads in one show.
    pub const MAX_PADS: usize = 64;
    /// Pad pages.
    pub const MAX_PAD_PAGES: usize = 8;
    /// DMX universes the show drives, numbered from 1.
    pub const DMX_UNIVERSES: usize = 64;
    /// DMX outputs in a show.
    pub const MAX_DMX_OUTPUTS: usize = 256;
    /// DMX triggers in a show.
    pub const MAX_DMX_TRIGGERS: usize = 256;
    /// Game controller actions in a show.
    pub const MAX_GAMEPAD_ACTIONS: usize = 128;
    /// Fixtures in a show.
    pub const MAX_FIXTURES: usize = 1_024;
    /// Channels of one fixture.
    pub const MAX_FIXTURE_CHANNELS: usize = 64;
    /// Named ranges on the channels of one fixture or mode.
    pub const MAX_CHANNEL_RANGES: usize = 256;
    /// Fixture types in a show, and modes of one.
    pub const MAX_FIXTURE_TYPES: usize = 256;
    pub const MAX_FIXTURE_MODES: usize = 32;
    /// Groups and presets in a show, and values in one preset.
    pub const MAX_GROUPS: usize = 256;
    pub const MAX_PRESETS: usize = 1_024;
    pub const MAX_PRESET_VALUES: usize = 16_384;
    /// Cue lists in a show, cues in one, and the longest fade (seconds).
    pub const MAX_CUE_LISTS: usize = 64;
    pub const MAX_CUES: usize = 1_000;
    pub const MAX_FADE_S: f64 = 600.0;
    /// Lighting cues one song fires.
    pub const MAX_LIGHT_CUES: usize = 1_000;
    /// Effects in a show.
    pub const MAX_EFFECTS: usize = 256;
    /// Sequences in a show and steps in one.
    pub const MAX_SEQUENCES: usize = 256;
    pub const MAX_SEQUENCE_STEPS: usize = 256;
    /// More attributes in one effect.
    pub const MAX_EFFECT_PARTS: usize = 8;
    /// Marks on one song's PDF.
    pub const MAX_PDF_MARKS: usize = 4_000;
    /// Further sheets on one song, and musicians in a show.
    pub const MAX_SHEETS: usize = 32;
    pub const MAX_MUSICIANS: usize = 32;
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
    /// What a MIDI controller (a foot switch, a pad) does: a message sets off a command.
    #[serde(default)]
    pub remote: Vec<RemoteAction>,
    /// Named lists of commands; `$NAME` at a timecoded lyrics line runs one when playback
    /// gets there.
    #[serde(default)]
    pub macros: Vec<Macro>,
    /// Buttons on the screen that run a command or a macro with one press.
    #[serde(default)]
    pub pads: Vec<Pad>,
    /// Where DMX levels go on the network: one Art-Net or sACN output per universe sent.
    #[serde(default)]
    pub dmx_outputs: Vec<DmxOutput>,
    /// DMX heard from other gear that sets off commands.
    #[serde(default)]
    pub dmx_triggers: Vec<DmxTrigger>,
    /// The band's logo (a PNG or JPEG file) at the top of a printed setlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band_logo: Option<String>,
    /// The band's musicians by name, for their sheets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub musicians: Vec<String>,
    /// Game controller buttons that set off commands.
    #[serde(default)]
    pub gamepad_actions: Vec<GamepadAction>,
    /// Game controller sticks and triggers that move a volume or the lighting master.
    #[serde(default)]
    pub gamepad_faders: Vec<GamepadFader>,
    /// The lights: each fixture with its channels and where it is patched.
    #[serde(default)]
    pub fixtures: Vec<Fixture>,
    /// Kinds of fixtures brought in from fixture library files.
    #[serde(default)]
    pub fixture_types: Vec<FixtureType>,
    /// Fixtures picked together by name ("Front wash").
    #[serde(default)]
    pub groups: Vec<Group>,
    /// Stored looks: attribute values of fixtures, recalled at once.
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Lists of cues gone through one by one during a show.
    #[serde(default)]
    pub cue_lists: Vec<CueList>,
    /// Effects that move fixture attributes on their own (a pulse, a chase, a sweep).
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Sequences: steps of looks over time (animations for moving heads, chases).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequences: Vec<Sequence>,
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
            remote: Vec::new(),
            macros: Vec::new(),
            pads: Vec::new(),
            dmx_outputs: Vec::new(),
            dmx_triggers: Vec::new(),
            musicians: Vec::new(),
            band_logo: None,
            gamepad_actions: Vec::new(),
            gamepad_faders: Vec::new(),
            fixtures: Vec::new(),
            fixture_types: Vec::new(),
            groups: Vec::new(),
            presets: Vec::new(),
            cue_lists: Vec::new(),
            effects: Vec::new(),
            sequences: Vec::new(),
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
    /// More sheets, each for a musician ("Guitar", "Bass"): the fields above are the song's own
    /// sheet, these come after it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<Sheet>,
    /// Lighting cues the song fires as it plays: at a time, a cue of a cue list.
    pub light_cues: Vec<LightCue>,
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
            sheets: Vec::new(),
            light_cues: Vec::new(),
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

/// A song's sheet fields to change: the song's own sheet or one of its further sheets.
pub struct SheetFields<'a> {
    pub pdf_file: &'a mut Option<String>,
    pub pdf_marks: &'a mut Vec<PdfMark>,
    pub notation_file: &'a mut Option<String>,
    pub score_first_bar: &'a mut i32,
}

/// One more sheet of a song, for a musician: a PDF or a score (MusicXML, one of its parts), with
/// marks of its own; it scrolls with playback like the song's own sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sheet {
    /// What it is called ("Guitar", "Bass chords").
    pub name: String,
    /// The musician it is for (a name from the show's musicians); empty for anyone.
    pub musician: String,
    pub pdf_file: Option<String>,
    pub pdf_marks: Vec<PdfMark>,
    pub notation_file: Option<String>,
    /// The score's part shown (from 0), when the score has several.
    pub part: usize,
    /// As [`Song::score_first_bar`].
    pub score_first_bar: i32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            name: String::new(),
            musician: String::new(),
            pdf_file: None,
            pdf_marks: Vec::new(),
            notation_file: None,
            part: 0,
            score_first_bar: 1,
            extra: Map::new(),
        }
    }
}

impl Sheet {
    /// Keeps a sheet from a file within bounds.
    fn tidy(&mut self) {
        use limits::*;
        cut(&mut self.name, 64);
        cut(&mut self.musician, 64);
        for file in [&mut self.pdf_file, &mut self.notation_file].into_iter().flatten() {
            cut(file, MAX_TEXT);
        }
        self.pdf_marks.truncate(MAX_PDF_MARKS);
        for m in &mut self.pdf_marks {
            m.normalise();
        }
        self.pdf_marks.retain(|m| m.points.len() >= m.min_points());
        self.part = self.part.min(255);
        self.score_first_bar = self.score_first_bar.clamp(MIN_SCORE_FIRST_BAR, MAX_SCORE_FIRST_BAR);
    }
}

impl Song {
    /// The song as one of its sheets shows it: `None` is the song's own sheet, `Some(i)` puts
    /// sheet `i`'s file, marks and first bar in the song's own sheet fields (for views that show
    /// one sheet). A sheet that isn't there gives the song as it is.
    pub fn with_sheet(&self, sheet: Option<usize>) -> Song {
        let mut s = self.clone();
        if let Some(sh) = sheet.and_then(|i| self.sheets.get(i)) {
            s.pdf_file.clone_from(&sh.pdf_file);
            s.pdf_marks.clone_from(&sh.pdf_marks);
            s.notation_file.clone_from(&sh.notation_file);
            s.score_first_bar = sh.score_first_bar;
        }
        s
    }

    /// The fields of one of the song's sheets to change: `None` is the song's own sheet;
    /// `None` back for a sheet that isn't there.
    pub fn sheet_mut(&mut self, sheet: Option<usize>) -> Option<SheetFields<'_>> {
        match sheet {
            None => Some(SheetFields {
                pdf_file: &mut self.pdf_file,
                pdf_marks: &mut self.pdf_marks,
                notation_file: &mut self.notation_file,
                score_first_bar: &mut self.score_first_bar,
            }),
            Some(i) => self.sheets.get_mut(i).map(|s| SheetFields {
                pdf_file: &mut s.pdf_file,
                pdf_marks: &mut s.pdf_marks,
                notation_file: &mut s.notation_file,
                score_first_bar: &mut s.score_first_bar,
            }),
        }
    }

    /// Every sheet file the song names (PDFs and scores, its own and its further sheets'), to
    /// relink them when a show is packed or unpacked.
    pub fn sheet_files_mut(&mut self) -> impl Iterator<Item = &mut String> {
        let own = [&mut self.pdf_file, &mut self.notation_file];
        let more = self.sheets.iter_mut().flat_map(|s| [&mut s.pdf_file, &mut s.notation_file]);
        own.into_iter().chain(more).flatten()
    }
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

/// What a toggle pad switches on and off (and shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleTarget {
    /// The click.
    Metronome,
    /// The playing song loops.
    LoopSong,
    /// The next song starts when the playing one ends.
    Autoplay,
    /// Every fixture dark at the output.
    Blackout,
    /// Fades and effects held.
    Freeze,
    /// Fixture changes and recalled presets kept off the lights.
    Blind,
}

/// The volume a fader pad moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaderTarget {
    Master,
    /// The song that is playing (or chosen).
    Song,
    /// An output bus, 0-based.
    Bus(u8),
}

/// A button on the screen: one press runs its command or its macro; or a fader that moves a
/// volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pad {
    pub label: String,
    /// `#rrggbb`.
    pub colour: String,
    /// A command id that needs nothing more; empty when the pad runs a macro.
    pub command: String,
    /// A macro's name; empty when the pad runs a command.
    #[serde(rename = "macro")]
    pub macro_name: String,
    /// A fader instead of a button: the volume it moves.
    pub fader: Option<FaderTarget>,
    /// A toggle instead of a button: what it switches, showing whether it is on.
    pub toggle: Option<ToggleTarget>,
    /// The pad page it is on (from 0); the bar shows one page at a time.
    pub page: u8,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            label: String::new(),
            colour: "#4f9cf0".to_owned(),
            command: String::new(),
            macro_name: String::new(),
            fader: None,
            toggle: None,
            page: 0,
            extra: Map::new(),
        }
    }
}

/// A named list of commands, run in order. Lyrics call it with `$NAME` at a timecoded line.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Macro {
    /// Capitals, digits and `_`, starting with a capital: `INTRO`, `LIGHTS_UP`.
    pub name: String,
    /// Command ids, run in order.
    pub commands: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Macro {
    /// Whether `name` can be a macro's name (and so be called as `$NAME` in lyrics).
    pub fn valid_name(name: &str) -> bool {
        name.len() <= 32
            && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    }
}

/// The kind of MIDI message that sets off a remote action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKind {
    /// A key pressed (Note On with a velocity above zero).
    #[default]
    Note,
    /// A controller going up past half way (a foot switch pressed).
    Cc,
    /// A program change.
    Program,
}

/// A MIDI message that sets off a remote action: its kind, channel (`None` for any) and note,
/// controller or program number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteTrigger {
    pub kind: RemoteKind,
    /// 0-based; `None` answers on every channel.
    pub channel: Option<u8>,
    pub number: u8,
}

/// A command a MIDI controller sets off.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteAction {
    pub trigger: RemoteTrigger,
    /// A command id (`transport.toggle`, `song.next`...).
    pub command: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What one DMX channel of a fixture controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribute {
    Intensity,
    IntensityFine,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Pan,
    PanFine,
    Tilt,
    TiltFine,
    Strobe,
    ColourWheel,
    Gobo,
    Zoom,
    Focus,
    /// Anything else (a macro, a speed, a reset): set by its own channel.
    #[serde(other)]
    Other,
}

impl Attribute {
    /// The channel that holds the low byte of a 16-bit value with this one, if there is one.
    pub fn fine(self) -> Option<Attribute> {
        match self {
            Attribute::Intensity => Some(Attribute::IntensityFine),
            Attribute::Pan => Some(Attribute::PanFine),
            Attribute::Tilt => Some(Attribute::TiltFine),
            _ => None,
        }
    }

    /// Whether this is the low byte of another attribute.
    pub fn is_fine(self) -> bool {
        matches!(self, Attribute::IntensityFine | Attribute::PanFine | Attribute::TiltFine)
    }
}

/// Fixtures picked together by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub name: String,
    /// Fixture ids, in the order they were picked.
    pub fixtures: Vec<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A stored look: attribute values of fixtures.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub values: Vec<PresetValue>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The wave an effect follows over one cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Smooth up and down.
    #[default]
    Sine,
    /// High for the first half, low for the second.
    Square,
    /// Up evenly, then back to low at once.
    Saw,
    /// Up evenly and down evenly.
    Triangle,
}

/// An effect: a group's attribute moved between two values in a wave, each fixture a little
/// later than the one before (spread).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effect {
    pub name: String,
    /// The group's name.
    pub group: String,
    pub attribute: Attribute,
    pub shape: Shape,
    /// Seconds per cycle when `beats` is 0.
    pub period: f64,
    /// Beats per cycle at the playing song's tempo; 0 to use `period`.
    pub beats: f64,
    /// The values it moves between, 0-1.
    pub low: f64,
    pub high: f64,
    /// How far apart in the cycle the group's fixtures are, 0 (together) to 1 (spread over it).
    pub spread: f64,
    /// More attributes moved by the same wave and timing (a circle: pan and tilt; a rainbow:
    /// red, green and blue), each later in the cycle by its offset.
    pub also: Vec<EffectPart>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Another attribute an effect moves, between its own two values, `offset` (0-1) of a cycle
/// later than the effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectPart {
    pub attribute: Attribute,
    pub low: f64,
    pub high: f64,
    pub offset: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for EffectPart {
    fn default() -> Self {
        Self { attribute: Attribute::Intensity, low: 0.0, high: 1.0, offset: 0.0, extra: Map::new() }
    }
}

impl Default for Effect {
    fn default() -> Self {
        Self {
            name: String::new(),
            group: String::new(),
            attribute: Attribute::Intensity,
            shape: Shape::Sine,
            period: 2.0,
            beats: 0.0,
            low: 0.0,
            high: 1.0,
            spread: 0.0,
            also: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl Shape {
    /// The wave's value (0-1) at `phase` (0-1) of the cycle.
    pub fn at(self, phase: f64) -> f64 {
        let p = phase.rem_euclid(1.0);
        match self {
            Shape::Sine => 0.5 - 0.5 * (p * std::f64::consts::TAU).cos(),
            Shape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Shape::Saw => p,
            Shape::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
        }
    }
}

/// A lighting cue a song fires when playback passes its time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightCue {
    /// Seconds into the song.
    pub time: f64,
    /// The cue list's name.
    pub list: String,
    /// The cue's place in that list, from 0.
    pub cue: usize,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Cues gone through one by one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CueList {
    pub name: String,
    pub cues: Vec<Cue>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One step of a cue list: a preset faded in over some seconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cue {
    pub name: String,
    /// The preset's name.
    pub preset: String,
    /// Seconds to fade in; 0 for at once.
    pub fade: f64,
    /// Seconds after this cue before the list goes on to the next by itself (after the last, back
    /// to the first); 0 waits for Go.
    pub follow: f64,
    /// Sequences and effects (by name) this cue starts, and ones it stops ("*": all running).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub starts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A run of looks over time, an animation (a moving head's figure, a colour chase): steps, each a
/// preset reached over part of the step and held for the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sequence {
    pub name: String,
    pub steps: Vec<SequenceStep>,
    pub mode: SequenceMode,
    /// Step lengths are beats (following the playing song's tempo and bar, or the tapped tempo)
    /// instead of seconds.
    pub in_beats: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One step of a sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SequenceStep {
    /// The preset's name.
    pub preset: String,
    /// Seconds, or beats for a sequence in beats.
    pub length: f64,
    /// The part of the step (0-1) spent moving from the step before to this one; the rest holds.
    pub fade: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for SequenceStep {
    fn default() -> Self {
        Self { preset: String::new(), length: 1.0, fade: 0.0, extra: Map::new() }
    }
}

/// How a sequence goes on after its last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequenceMode {
    /// From the first step again.
    #[default]
    Loop,
    /// Stops on the last step.
    Once,
    /// Back through the steps to the first, and so on.
    Bounce,
}

/// One fixture's attribute in a preset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PresetValue {
    /// The fixture's id.
    pub fixture: u64,
    pub attribute: Attribute,
    /// 0-1.
    pub value: f64,
}

/// A kind of fixture brought into the show (from a fixture library file), with its modes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixtureType {
    pub name: String,
    pub modes: Vec<FixtureMode>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One way a fixture type can be set up: its channels in DMX order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixtureMode {
    pub name: String,
    pub channels: Vec<Attribute>,
    /// Named parts of channels that do several things (off, dimmer, strobe, open on one channel).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<ChannelRange>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A named part of one channel's levels: `from` to `to` (inclusive) on channel `channel` (from 1)
/// does `name` ("Strobe slow to fast", "Open").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelRange {
    pub channel: u16,
    pub from: u8,
    pub to: u8,
    pub name: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ChannelRange {
    /// Whether it fits a fixture with `channels` channels and is in order.
    pub fn fits(&self, channels: usize) -> bool {
        self.channel >= 1 && usize::from(self.channel) <= channels && self.from <= self.to
    }

    /// Keeps only ranges that fit `channels` channels, at most [`limits::MAX_CHANNEL_RANGES`],
    /// with short names.
    pub fn tidy(ranges: &mut Vec<ChannelRange>, channels: usize) {
        ranges.retain(|r| r.fits(channels));
        ranges.truncate(limits::MAX_CHANNEL_RANGES);
        for r in ranges.iter_mut() {
            cut(&mut r.name, 64);
        }
    }
}

/// A light (or anything else DMX drives) patched into a universe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fixture {
    /// Stays the same while the fixture is in the show (groups and presets refer to it); 0 in an
    /// older file, given one on load.
    pub id: u64,
    /// What the person calls it ("Front left").
    pub name: String,
    /// The kind of fixture it is, in words ("RGB wash"); for the person, not used to drive it.
    pub kind: String,
    /// What each of its channels controls, in DMX order.
    pub channels: Vec<Attribute>,
    /// The show universe it is patched into, from 1.
    pub universe: u16,
    /// Its first DMX channel, from 1.
    pub address: u16,
    /// Where it is on the stage seen from the audience: x left to right, y back to front, each
    /// 0-1; `None` until placed.
    pub place: Option<[f64; 2]>,
    /// The Philips Hue light that shows what it shows (its id on the paired bridge); empty for
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hue: String,
    /// Named parts of its channels that do several things, from its fixture type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<ChannelRange>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            kind: String::new(),
            channels: Vec::new(),
            universe: 1,
            address: 1,
            place: None,
            hue: String::new(),
            ranges: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl Fixture {
    /// The DMX channels it takes, from its address on (from 1).
    pub fn footprint(&self) -> std::ops::RangeInclusive<usize> {
        let first = usize::from(self.address);
        first..=first + self.channels.len().saturating_sub(1)
    }

    /// Whether it shares a channel with `other`.
    pub fn overlaps(&self, other: &Fixture) -> bool {
        self.universe == other.universe
            && !self.channels.is_empty()
            && !other.channels.is_empty()
            && self.footprint().start() <= other.footprint().end()
            && other.footprint().start() <= self.footprint().end()
    }
}

/// The network protocol a DMX output speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmxProtocol {
    /// Art-Net (UDP port 6454).
    #[default]
    #[serde(rename = "artnet")]
    ArtNet,
    /// sACN, ANSI E1.31 (UDP port 5568).
    Sacn,
    /// A USB interface speaking the DMX USB Pro serial protocol (its published API), on a serial
    /// port; `address` names the port.
    UsbPro,
    /// A plain USB-to-RS-485 cable (an FTDI chip and a line driver) on a serial port; the
    /// computer makes the DMX signal itself. `address` names the port.
    UsbCable,
}

impl DmxProtocol {
    /// Whether it goes to a USB device on a serial port (its `address` names the port).
    pub fn is_usb(self) -> bool {
        matches!(self, DmxProtocol::UsbPro | DmxProtocol::UsbCable)
    }
}

/// A game controller button that sets off a command when pressed (any controller).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadAction {
    /// One of [`GamepadAction::BUTTONS`].
    pub button: String,
    /// A command that needs nothing more.
    pub command: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A game controller stick or trigger that moves a volume, or the lighting master when
/// `volume` is `None`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadFader {
    /// One of [`GamepadFader::AXES`].
    pub axis: String,
    pub volume: Option<FaderTarget>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl GamepadFader {
    /// The sticks (each way) and the analogue triggers.
    pub const AXES: &[&str] = &["left_trigger", "right_trigger", "left_x", "left_y", "right_x", "right_y"];
}

impl GamepadAction {
    /// The buttons, by where they sit on a standard controller (south is the lowest face button).
    pub const BUTTONS: &[&str] = &[
        "south",
        "east",
        "north",
        "west",
        "left_bumper",
        "left_trigger",
        "right_bumper",
        "right_trigger",
        "select",
        "start",
        "mode",
        "left_stick",
        "right_stick",
        "dpad_up",
        "dpad_down",
        "dpad_left",
        "dpad_right",
    ];
}

/// A channel of DMX heard from other gear that sets off a command when it goes up past half
/// (a fader pushed up, a button on a desk pressed).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DmxTrigger {
    /// The Art-Net port address it comes in on.
    pub universe: u16,
    /// From 1.
    pub channel: u16,
    /// A command that needs nothing more.
    pub command: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One universe of DMX levels sent over the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DmxOutput {
    /// The show's universe that is sent, from 1.
    pub universe: u16,
    pub protocol: DmxProtocol,
    /// The universe on the network: an Art-Net port address (0-32767) or an sACN universe
    /// (1-63999).
    pub network_universe: u16,
    /// The IPv4 address it goes to; empty for everyone on the network (Art-Net broadcast, sACN
    /// multicast).
    pub address: String,
    /// sACN priority, 0-200 (100 normally); receivers take the highest.
    pub priority: u8,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DmxOutput {
    /// Highest Art-Net port address.
    pub const ARTNET_MAX: u16 = 32_767;
    /// Lowest and highest sACN universe.
    pub const SACN_MIN: u16 = 1;
    pub const SACN_MAX: u16 = 63_999;
    /// Highest sACN priority.
    pub const PRIORITY_MAX: u8 = 200;

    /// The network universe this output's protocol allows nearest to `n`.
    pub fn clamp_network_universe(protocol: DmxProtocol, n: u16) -> u16 {
        match protocol {
            DmxProtocol::ArtNet => n.min(Self::ARTNET_MAX),
            DmxProtocol::Sacn => n.clamp(Self::SACN_MIN, Self::SACN_MAX),
            DmxProtocol::UsbPro | DmxProtocol::UsbCable => 0,
        }
    }

    /// The address it goes to, `None` for everyone on the network.
    pub fn target(&self) -> Option<std::net::Ipv4Addr> {
        self.address.trim().parse().ok()
    }

    /// Whether `name` can be a serial port name (`COM3`, `/dev/ttyUSB0`): short, and only
    /// letters, digits and `/ . _ - \ :`.
    pub fn valid_port_name(name: &str) -> bool {
        !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "/._-\\:".contains(c))
    }
}

impl Default for DmxOutput {
    fn default() -> Self {
        Self {
            universe: 1,
            protocol: DmxProtocol::ArtNet,
            network_universe: 0,
            address: String::new(),
            priority: 100,
            extra: Map::new(),
        }
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
    /// Lines printed on the setlist before a song: a break, an instrument change.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<SetNote>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A line on the setlist before a song of a set ("Break, 20 minutes", "Acoustic guitar").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SetNote {
    /// The song it comes before.
    pub song: u64,
    pub text: String,
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
        self.sheets.truncate(MAX_SHEETS);
        for s in &mut self.sheets {
            s.tidy();
        }
        self.light_cues.retain(|c| c.time.is_finite() && c.time >= 0.0);
        self.light_cues.truncate(MAX_LIGHT_CUES);
        for c in &mut self.light_cues {
            cut(&mut c.list, 64);
        }
        self.light_cues.sort_by(|a, b| a.time.total_cmp(&b.time));
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
        self.pads.truncate(MAX_PADS);
        for pad in &mut self.pads {
            cut(&mut pad.label, 64);
            fix_colour(&mut pad.colour, "#4f9cf0");
            cut(&mut pad.command, 64);
            cut(&mut pad.macro_name, 32);
            pad.page = pad.page.min(u8::try_from(MAX_PAD_PAGES - 1).unwrap_or(0));
            if let Some(FaderTarget::Bus(b)) = pad.fader
                && usize::from(b) >= BUSES
            {
                pad.fader = None;
            }
        }
        self.fixture_types.truncate(MAX_FIXTURE_TYPES);
        for t in &mut self.fixture_types {
            cut(&mut t.name, 64);
            t.modes.truncate(MAX_FIXTURE_MODES);
            for m in &mut t.modes {
                cut(&mut m.name, 64);
                m.channels.truncate(MAX_FIXTURE_CHANNELS);
                ChannelRange::tidy(&mut m.ranges, m.channels.len());
            }
            t.modes.retain(|m| !m.channels.is_empty());
        }
        self.fixture_types.retain(|t| !t.modes.is_empty());
        self.fixtures.truncate(MAX_FIXTURES);
        for f in &mut self.fixtures {
            cut(&mut f.name, 64);
            cut(&mut f.kind, 64);
            cut(&mut f.hue, 16);
            if !f.hue.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                f.hue.clear();
            }
            f.channels.truncate(MAX_FIXTURE_CHANNELS);
            ChannelRange::tidy(&mut f.ranges, f.channels.len());
            if let Some([x, y]) = f.place {
                f.place = (x.is_finite() && y.is_finite()).then(|| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)]);
            }
        }
        // A fixture must fit in its universe.
        self.fixtures.retain(|f| {
            (1..=DMX_UNIVERSES).contains(&usize::from(f.universe))
                && f.address >= 1
                && *f.footprint().end() <= 512
                && !f.channels.is_empty()
        });
        // Every fixture its own id; groups and presets only name fixtures that are there.
        let mut seen = std::collections::HashSet::new();
        let mut next = self.fixtures.iter().map(|f| f.id).max().unwrap_or(0);
        for f in &mut self.fixtures {
            if f.id == 0 || !seen.insert(f.id) {
                next += 1;
                f.id = next;
                seen.insert(next);
            }
        }
        self.groups.truncate(MAX_GROUPS);
        for g in &mut self.groups {
            cut(&mut g.name, 64);
            g.fixtures.retain(|id| seen.contains(id));
            g.fixtures.truncate(MAX_FIXTURES);
        }
        self.presets.truncate(MAX_PRESETS);
        for p in &mut self.presets {
            cut(&mut p.name, 64);
            p.values.retain(|v| seen.contains(&v.fixture) && v.value.is_finite());
            p.values.truncate(MAX_PRESET_VALUES);
            for v in &mut p.values {
                v.value = v.value.clamp(0.0, 1.0);
            }
        }
        self.sequences.truncate(MAX_SEQUENCES);
        for s in &mut self.sequences {
            cut(&mut s.name, 64);
            s.steps.truncate(MAX_SEQUENCE_STEPS);
            for step in &mut s.steps {
                cut(&mut step.preset, 64);
                step.length = clamp_f(step.length, 0.01, 3600.0, 1.0);
                step.fade = clamp_f(step.fade, 0.0, 1.0, 0.0);
            }
        }
        self.effects.truncate(MAX_EFFECTS);
        for fx in &mut self.effects {
            cut(&mut fx.name, 64);
            cut(&mut fx.group, 64);
            fx.period = clamp_f(fx.period, 0.05, 600.0, 2.0);
            fx.beats = clamp_f(fx.beats, 0.0, 64.0, 0.0);
            fx.low = clamp_f(fx.low, 0.0, 1.0, 0.0);
            fx.high = clamp_f(fx.high, 0.0, 1.0, 1.0);
            fx.spread = clamp_f(fx.spread, 0.0, 1.0, 0.0);
            fx.also.truncate(MAX_EFFECT_PARTS);
            for part in &mut fx.also {
                part.low = clamp_f(part.low, 0.0, 1.0, 0.0);
                part.high = clamp_f(part.high, 0.0, 1.0, 1.0);
                part.offset = clamp_f(part.offset, 0.0, 1.0, 0.0);
            }
        }
        self.cue_lists.truncate(MAX_CUE_LISTS);
        for l in &mut self.cue_lists {
            cut(&mut l.name, 64);
            l.cues.truncate(MAX_CUES);
            for c in &mut l.cues {
                cut(&mut c.name, 64);
                cut(&mut c.preset, 64);
                c.fade = clamp_f(c.fade, 0.0, MAX_FADE_S, 0.0);
                c.follow = clamp_f(c.follow, 0.0, MAX_FADE_S, 0.0);
                for list in [&mut c.starts, &mut c.stops] {
                    list.truncate(64);
                    for n in list.iter_mut() {
                        cut(n, 64);
                    }
                }
            }
        }
        self.gamepad_faders.truncate(MAX_GAMEPAD_ACTIONS);
        self.gamepad_faders.retain(|f| GamepadFader::AXES.contains(&f.axis.as_str()));
        for f in &mut self.gamepad_faders {
            if let Some(FaderTarget::Bus(b)) = f.volume
                && usize::from(b) >= BUSES
            {
                f.volume = None;
            }
        }
        self.gamepad_actions.truncate(MAX_GAMEPAD_ACTIONS);
        self.gamepad_actions.retain(|a| GamepadAction::BUTTONS.contains(&a.button.as_str()));
        for a in &mut self.gamepad_actions {
            cut(&mut a.command, 64);
        }
        self.musicians.truncate(MAX_MUSICIANS);
        for m in &mut self.musicians {
            cut(m, 64);
        }
        self.musicians.retain(|m| !m.trim().is_empty());
        self.dmx_triggers.truncate(MAX_DMX_TRIGGERS);
        self.dmx_triggers.retain(|t| (1..=512).contains(&t.channel) && t.universe <= DmxOutput::ARTNET_MAX);
        for t in &mut self.dmx_triggers {
            cut(&mut t.command, 64);
        }
        self.dmx_outputs.truncate(MAX_DMX_OUTPUTS);
        self.dmx_outputs.retain(|o| (1..=DMX_UNIVERSES).contains(&usize::from(o.universe)));
        for o in &mut self.dmx_outputs {
            o.network_universe = DmxOutput::clamp_network_universe(o.protocol, o.network_universe);
            o.priority = o.priority.min(DmxOutput::PRIORITY_MAX);
            cut(&mut o.address, 64);
            let fits = if o.protocol.is_usb() { DmxOutput::valid_port_name(&o.address) } else { o.target().is_some() };
            if !fits {
                o.address.clear();
            }
        }
        self.macros.truncate(MAX_MACROS);
        self.macros.retain(|m| Macro::valid_name(&m.name));
        for m in &mut self.macros {
            m.commands.truncate(MAX_MACRO_COMMANDS);
            for c in &mut m.commands {
                cut(c, 64);
            }
        }
        self.remote.truncate(MAX_REMOTE_ACTIONS);
        for r in &mut self.remote {
            r.trigger.channel = r.trigger.channel.filter(|c| *c < 16);
            r.trigger.number = r.trigger.number.min(127);
            cut(&mut r.command, 64);
        }
        if !MidiSettings::RATES.contains(&self.midi.timecode_fps) {
            self.midi.timecode_fps = 25;
        }
        self.transitions.crossfade = clamp_f(self.transitions.crossfade, 0.0, 30.0, 0.0);
        self.playlists.truncate(MAX_PLAYLISTS);
        if let Some(logo) = &mut self.band_logo {
            cut(logo, MAX_TEXT);
        }
        for p in &mut self.playlists {
            cut(&mut p.name, MAX_TEXT);
            p.sets.truncate(MAX_SETS);
            if p.sets.is_empty() && p.smart.is_none() {
                p.sets.push(Set { name: "Set 1".to_owned(), ..Set::default() });
            }
            for s in &mut p.sets {
                cut(&mut s.name, MAX_TEXT);
                s.songs.truncate(MAX_SET_SONGS);
                s.notes.truncate(MAX_SET_SONGS);
                for n in &mut s.notes {
                    cut(&mut n.text, 200);
                }
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
