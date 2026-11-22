//! Notation: MusicXML scores read into parts, bars and notes, with a string and fret for every
//! pitched note so a part can be shown as tablature.
//!
//! MusicXML is the open exchange format notation and tablature editors write (`.musicxml`, `.xml`)
//! and its compressed form (`.mxl`, a ZIP whose `META-INF/container.xml` names the score). Only
//! the part-wise layout (`<score-partwise>`) is read; that is what editors write.
//!
//! Reading treats the file as hostile: input size, parts, bars and notes are capped, every number
//! is range-checked, and nothing panics.
//!
//! The shared `craft-score` library writes MusicXML but has no reader; this one depends only on
//! shared libraries (`craft-xml`, `craft-zip`) and holds no ActCraft policy, so it can move there.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod flow;
mod musicxml;
pub mod tab;

pub use flow::{Flow, play_order};
pub use musicxml::{read, read_mxl, read_xml};

/// Ticks in a quarter note; every onset and length is in ticks.
pub const TICKS_PER_QUARTER: u32 = 960;
/// Largest file read, compressed or not.
pub const MAX_INPUT: usize = 32 * 1024 * 1024;
/// Parts kept from one score.
pub const MAX_PARTS: usize = 64;
/// Bars kept per part.
pub const MAX_BARS: usize = 5_000;
/// Notes and rests kept over the whole score.
pub const MAX_NOTES: usize = 400_000;
/// Strings a tablature part may have.
pub const MAX_STRINGS: usize = 12;
/// Highest fret written.
pub const MAX_FRET: u8 = 36;

/// A score: its title and its parts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Score {
    pub title: String,
    pub composer: String,
    pub parts: Vec<Part>,
}

/// One instrument's part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Part {
    pub name: String,
    /// Open-string pitches as MIDI numbers, highest string first (string 1 is index 0). Empty when
    /// the file gives no tuning; [`Part::strings`] then falls back to a standard guitar.
    pub tuning: Vec<u8>,
    /// Capo fret, 0 for none.
    pub capo: u8,
    /// The file writes this part as tablature (a TAB clef, string tuning or fret numbers).
    pub tablature: bool,
    /// The clef of each staff (index 0 is staff 1), as first written; at least one.
    pub clefs: Vec<Clef>,
    pub bars: Vec<Bar>,
}

/// Staves one part may have.
pub const MAX_STAVES: usize = 4;

/// A clef: which note sits on which staff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clef {
    pub kind: ClefKind,
    /// Staff line the clef marks, 1 = bottom.
    pub line: u8,
    /// Octaves the music sounds above (positive) or below (negative) what is written; -1 for the
    /// guitar's treble clef with an 8 below.
    pub octave_change: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClefKind {
    /// Treble family: G4 on the line.
    G,
    /// Bass family: F3 on the line.
    F,
    /// Alto and tenor: C4 on the line.
    C,
}

impl Default for Clef {
    fn default() -> Self {
        Self::TREBLE
    }
}

impl Clef {
    pub const TREBLE: Clef = Clef { kind: ClefKind::G, line: 2, octave_change: 0 };
    pub const BASS: Clef = Clef { kind: ClefKind::F, line: 4, octave_change: 0 };
    /// Treble with an 8 below, as guitar music is written.
    pub const GUITAR: Clef = Clef { kind: ClefKind::G, line: 2, octave_change: -1 };

    /// Staff position of a pitch: 0 is the bottom line, 1 the space above it, 8 the top line.
    pub fn position(self, pitch: Pitch) -> i32 {
        let bottom = self.reference() - 2 * (i32::from(self.line.clamp(1, 5)) - 1);
        pitch.diatonic() - 7 * i32::from(self.octave_change) - bottom
    }

    /// The note the clef marks, diatonic.
    fn reference(self) -> i32 {
        match self.kind {
            ClefKind::G => 4 * 7 + 4,
            ClefKind::F => 3 * 7 + 3,
            ClefKind::C => 4 * 7,
        }
    }
}

/// One bar of a part.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// The bar number as the file writes it ("1", "12a"); empty when it has none.
    pub number: String,
    /// Time signature in force: beats and the beat's note value (4 = quarter).
    pub beats: u8,
    pub beat_type: u8,
    /// Key signature in force: sharps (positive) or flats (negative), -7..=7.
    pub fifths: i8,
    /// Tempo marked at this bar, quarter notes per minute.
    pub tempo: Option<f64>,
    /// Length in ticks: the furthest a voice reaches, or the time signature for an empty bar.
    pub length: u32,
    /// Notes and rests by onset, then by pitch.
    pub notes: Vec<Note>,
    /// Repeat signs, endings and jumps at this bar.
    pub flow: Flow,
    /// Chord symbols written above the bar, by onset.
    pub chords: Vec<ChordSymbol>,
}

/// A chord symbol ("Am7", "F#/C#") and where in the bar it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordSymbol {
    /// Onset from the bar's start, ticks.
    pub start: u32,
    pub name: String,
}

/// Chord symbols kept per bar.
pub const MAX_CHORDS_PER_BAR: usize = 32;

impl Bar {
    /// The length the time signature gives, in ticks.
    pub fn signature_length(&self) -> u32 {
        signature_ticks(self.beats, self.beat_type)
    }
}

/// The length of a bar of `beats`/`beat_type` in ticks.
pub fn signature_ticks(beats: u8, beat_type: u8) -> u32 {
    let beat_type = u32::from(beat_type.max(1));
    u32::from(beats.max(1)).saturating_mul(TICKS_PER_QUARTER).saturating_mul(4) / beat_type
}

/// A note or a rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Onset from the bar's start, ticks.
    pub start: u32,
    /// Length, ticks.
    pub length: u32,
    /// `None` for a rest.
    pub pitch: Option<Pitch>,
    /// How the note is written (whole, half, quarter...); taken from its length when the file
    /// doesn't say.
    pub value: NoteValue,
    pub dots: u8,
    /// String (1 = highest) and fret; filled for every pitched note of a tablature part.
    pub string: Option<u8>,
    pub fret: Option<u8>,
    /// Sounds on from the previous note (the fret is not struck again).
    pub tied: bool,
    /// Tied to the next note of the same pitch.
    pub tie_start: bool,
    /// Slurs that start or end at this note.
    pub slurs: Vec<Slur>,
    /// Part of a triplet or another tuplet.
    pub tuplet: Tuplet,
    /// How a string player plays it: bends, slides, legato, mutes, harmonics.
    pub technique: Technique,
    /// The words sung on this note, one per verse.
    pub lyrics: Vec<Lyric>,
    pub voice: u8,
    /// Staff of a part written on several staves (piano), 1-based.
    pub staff: u8,
}

/// A syllable or word sung on a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lyric {
    /// Verse, 1-based.
    pub verse: u8,
    pub text: String,
    /// Where the syllable sits in its word; a hyphen joins it to the next one.
    pub syllabic: Syllabic,
}

/// A syllable's place in its word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Syllabic {
    /// A whole word.
    #[default]
    Single,
    Begin,
    Middle,
    End,
}

impl Syllabic {
    /// The word goes on in the next syllable (a hyphen follows).
    pub fn continues(self) -> bool {
        matches!(self, Syllabic::Begin | Syllabic::Middle)
    }
}

/// Verses kept per note.
pub const MAX_VERSES: u8 = 4;
/// Characters kept per syllable or chord symbol.
pub const MAX_WORD: usize = 48;

/// Playing techniques of a fretted note, as tablature shows them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Technique {
    /// Bent up by this many semitones (1 = half, 2 = full); `None` for no bend.
    pub bend: Option<f32>,
    /// The bend is let down again.
    pub release: bool,
    /// A hammer-on or pull-off from this note to the next on its string.
    pub hammer_on: bool,
    pub pull_off: bool,
    /// A slide from this note to the next on its string.
    pub slide: bool,
    /// Played as a harmonic.
    pub harmonic: bool,
    /// Muted with the picking hand's palm.
    pub palm_mute: bool,
    pub vibrato: bool,
    /// Tapped with the picking hand.
    pub tap: bool,
}

/// Largest bend kept, in semitones.
pub const MAX_BEND: f32 = 12.0;

/// A note's place in a tuplet: `actual` notes in the time of `normal` ones (3 in 2 for a triplet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tuplet {
    /// Notes played (the number shown); 0 when the note is in no tuplet.
    pub actual: u8,
    /// Notes whose time they take.
    pub normal: u8,
    /// The tuplet's bracket starts or ends at this note.
    pub start: bool,
    pub stop: bool,
    /// Draw a bracket (`Some(true)`), only the number (`Some(false)`), or decide by the notes.
    pub bracket: Option<bool>,
}

impl Default for Note {
    /// A quarter rest at the bar's start, voice 1 on staff 1.
    fn default() -> Self {
        Self {
            start: 0,
            length: TICKS_PER_QUARTER,
            pitch: None,
            value: NoteValue::Quarter,
            dots: 0,
            string: None,
            fret: None,
            tied: false,
            tie_start: false,
            slurs: Vec::new(),
            tuplet: Tuplet::default(),
            technique: Technique::default(),
            lyrics: Vec::new(),
            voice: 1,
            staff: 1,
        }
    }
}

/// Slurs one note may start or end.
pub const MAX_SLURS_PER_NOTE: usize = 4;

/// One end of a slur (a phrase arc over notes of different pitch) at a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slur {
    /// Tells overlapping slurs apart; the start and the stop of one slur share it.
    pub number: u8,
    /// The slur starts here (else it ends here).
    pub start: bool,
    /// Drawn above the notes (`Some(true)`), below, or where the notes leave room (`None`).
    pub above: Option<bool>,
}

/// A written pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pitch {
    /// Letter: 0 = C .. 6 = B.
    pub step: u8,
    /// Semitones up (sharp) or down (flat), -2..=2.
    pub alter: i8,
    /// Octave, 4 holds middle C.
    pub octave: i8,
}

impl Pitch {
    /// The MIDI note number, clamped to 0..=127.
    pub fn midi(self) -> u8 {
        const SEMITONES: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
        let base = SEMITONES.get(usize::from(self.step)).copied().unwrap_or(0);
        let n = (i32::from(self.octave) + 1) * 12 + base + i32::from(self.alter);
        n.clamp(0, 127) as u8
    }

    /// Diatonic position: seven per octave, C0 = 0. A staff places notes by this.
    pub fn diatonic(self) -> i32 {
        i32::from(self.octave) * 7 + i32::from(self.step)
    }
}

/// A note's written value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteValue {
    Whole,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
}

impl NoteValue {
    /// Its plain length (no dots) in ticks.
    pub fn ticks(self) -> u32 {
        match self {
            Self::Whole => TICKS_PER_QUARTER * 4,
            Self::Half => TICKS_PER_QUARTER * 2,
            Self::Quarter => TICKS_PER_QUARTER,
            Self::Eighth => TICKS_PER_QUARTER / 2,
            Self::Sixteenth => TICKS_PER_QUARTER / 4,
            Self::ThirtySecond => TICKS_PER_QUARTER / 8,
            Self::SixtyFourth => TICKS_PER_QUARTER / 16,
        }
    }

    /// Flags (or beams) the value carries: none up to a quarter.
    pub fn flags(self) -> u8 {
        match self {
            Self::Whole | Self::Half | Self::Quarter => 0,
            Self::Eighth => 1,
            Self::Sixteenth => 2,
            Self::ThirtySecond => 3,
            Self::SixtyFourth => 4,
        }
    }

    /// The value and dots nearest to a length in ticks.
    pub fn from_ticks(ticks: u32) -> (Self, u8) {
        const ALL: [NoteValue; 7] = [
            NoteValue::Whole,
            NoteValue::Half,
            NoteValue::Quarter,
            NoteValue::Eighth,
            NoteValue::Sixteenth,
            NoteValue::ThirtySecond,
            NoteValue::SixtyFourth,
        ];
        for v in ALL {
            let t = v.ticks();
            if ticks >= t {
                let dots = if ticks >= t + t / 2 + t / 4 {
                    2
                } else if ticks >= t + t / 2 {
                    1
                } else {
                    0
                };
                return (v, dots);
            }
        }
        (NoteValue::SixtyFourth, 0)
    }
}

/// Why a score could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Larger than [`MAX_INPUT`].
    TooLarge,
    /// Not XML, or not a ZIP when compressed.
    Unreadable(String),
    /// XML, but not a part-wise MusicXML score.
    NotAScore,
    /// A compressed score whose archive names no score file.
    NoScoreInArchive,
    /// A score with no notes in any part.
    Empty,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "the file is larger than {} MB", MAX_INPUT / (1024 * 1024)),
            Self::Unreadable(why) => write!(f, "the file cannot be read: {why}"),
            Self::NotAScore => f.write_str("the file is not a MusicXML score"),
            Self::NoScoreInArchive => f.write_str("the compressed file holds no MusicXML score"),
            Self::Empty => f.write_str("the score has no notes"),
        }
    }
}

impl std::error::Error for Error {}

/// File extensions read as notation.
pub const EXTENSIONS: &[&str] = &["musicxml", "xml", "mxl"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_map_to_midi_and_staff_positions() {
        let c4 = Pitch { step: 0, alter: 0, octave: 4 };
        assert_eq!(c4.midi(), 60);
        assert_eq!(Pitch { step: 2, alter: 0, octave: 2 }.midi(), 40, "low E string");
        assert_eq!(Pitch { step: 3, alter: 1, octave: 4 }.midi(), 66, "F#4");
        assert_eq!(Pitch { step: 6, alter: -1, octave: 3 }.midi(), 58, "Bb3");
        assert_eq!(Pitch { step: 6, alter: 2, octave: 120 }.midi(), 127, "clamped");
        assert_eq!(Pitch { step: 0, alter: -2, octave: -5 }.midi(), 0, "clamped");
        assert_eq!(c4.diatonic() - Pitch { step: 6, alter: 0, octave: 3 }.diatonic(), 1);
    }

    #[test]
    fn clefs_place_notes_on_the_staff() {
        let e4 = Pitch { step: 2, alter: 0, octave: 4 };
        let f5 = Pitch { step: 3, alter: 0, octave: 5 };
        let g2 = Pitch { step: 4, alter: 0, octave: 2 };
        let low_e = Pitch { step: 2, alter: 0, octave: 2 };
        assert_eq!(Clef::TREBLE.position(e4), 0, "bottom line");
        assert_eq!(Clef::TREBLE.position(f5), 8, "top line");
        assert_eq!(Clef::BASS.position(g2), 0);
        assert_eq!(Clef::GUITAR.position(low_e), -7, "written E3: under the third ledger line");
        let alto = Clef { kind: ClefKind::C, line: 3, octave_change: 0 };
        assert_eq!(alto.position(Pitch { step: 0, alter: 0, octave: 4 }), 4, "middle line");
    }

    #[test]
    fn values_come_from_lengths() {
        assert_eq!(NoteValue::from_ticks(960), (NoteValue::Quarter, 0));
        assert_eq!(NoteValue::from_ticks(1440), (NoteValue::Quarter, 1));
        assert_eq!(NoteValue::from_ticks(1680), (NoteValue::Quarter, 2));
        assert_eq!(NoteValue::from_ticks(3840), (NoteValue::Whole, 0));
        assert_eq!(NoteValue::from_ticks(320), (NoteValue::Sixteenth, 0), "a triplet eighth");
        assert_eq!(NoteValue::from_ticks(0), (NoteValue::SixtyFourth, 0));
        assert_eq!(signature_ticks(4, 4), 3840);
        assert_eq!(signature_ticks(6, 8), 2880);
        assert_eq!(signature_ticks(0, 0), 3840, "never zero");
    }
}
