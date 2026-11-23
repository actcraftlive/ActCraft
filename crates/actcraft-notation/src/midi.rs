//! Notes from a Standard MIDI File: each track (and each channel of a track) becomes a part,
//! its notes are placed on a sixteenth-note grid and written as notes, chords, rests and ties in
//! the file's bars, keys and tempos.
//!
//! A MIDI file says which keys sound when, not how to write them, so this guesses:
//!
//! - Onsets and ends are rounded to sixteenths; notes struck together form a chord that lasts
//!   until its shortest note ends or the next chord starts.
//! - Sharps are written in sharp keys and in C, flats in flat keys.
//! - Channel 10 is drums (the General MIDI drum map), each drum on a line of its own.
//! - A piano (General MIDI programs 1 to 8) gets two staves, split at middle C.
//! - A guitar or bass (by its program or its name) is also written as tablature.
//!
//! Files written in a notation program come out close to their score; played-in files come out
//! roughly.

use craft_midi::{CtrlKind, Smf};

use crate::tab::{self, fill_frets};
use crate::{
    Bar, Clef, ClefKind, Error, Flow, MAX_BARS, MAX_INPUT, MAX_NOTES, MAX_PARTS, Note, NoteValue, Part, Pitch, Score,
    TICKS_PER_QUARTER, signature_ticks,
};

/// Onsets and ends are rounded to this grid: a sixteenth note.
pub const GRID: u32 = TICKS_PER_QUARTER / 4;
/// The General MIDI drum channel (10, counted from 1).
const DRUMS: u8 = 9;
/// Notes starting later than this many ticks are left out: as many bars as a score keeps, of the
/// longest time signature (16/4), so every tick sum stays far from overflowing.
const MAX_TICK: u32 = MAX_BARS as u32 * 4 * 3840;

/// Reads a Standard MIDI File as a score.
pub fn read_midi(bytes: &[u8]) -> Result<Score, Error> {
    if bytes.len() > MAX_INPUT {
        return Err(Error::TooLarge);
    }
    let smf = craft_midi::read_smf(bytes).map_err(|e| Error::Unreadable(e.to_string()))?;
    from_smf(&smf)
}

/// One bar's place and what is in force there.
#[derive(Debug, Clone, Copy)]
struct Frame {
    start: u32,
    length: u32,
    beats: u8,
    beat_type: u8,
    fifths: i8,
    tempo: Option<f64>,
}

/// A run of notes to write on one staff.
struct Stream {
    name: String,
    channel: u8,
    program: Option<u8>,
    notes: Vec<(u32, u32, u8)>,
}

fn tick(t: i64) -> u32 {
    u32::try_from(t.max(0)).unwrap_or(u32::MAX)
}

fn snap(t: u32) -> u32 {
    t.saturating_add(GRID / 2) / GRID * GRID
}

/// Converts a read MIDI file.
pub fn from_smf(smf: &Smf) -> Result<Score, Error> {
    // MIDI ticks and score ticks are both 960 to the quarter note.
    let mut streams: Vec<Stream> = Vec::new();
    let mut title = String::new();
    let mut total_notes = 0usize;
    for track in &smf.tracks {
        let notes = &track.sequence.notes;
        if notes.is_empty() {
            if title.is_empty() {
                title = track.name.trim().to_owned();
            }
            continue;
        }
        let mut channels: Vec<u8> = notes.iter().map(|n| n.channel).collect();
        channels.sort_unstable();
        channels.dedup();
        for channel in channels {
            if streams.len() >= MAX_PARTS {
                break;
            }
            let program = track
                .sequence
                .ctrls
                .iter()
                .find(|c| c.channel == channel && c.kind == CtrlKind::Program)
                .and_then(|c| u8::try_from(c.value).ok());
            let mut list = Vec::new();
            for n in notes.iter().filter(|n| n.channel == channel) {
                if total_notes >= MAX_NOTES {
                    break;
                }
                let start = snap(tick(n.start));
                if start >= MAX_TICK {
                    continue;
                }
                let end = snap(tick(n.start.saturating_add(n.length))).clamp(start + GRID, MAX_TICK);
                list.push((start, end, n.pitch.min(127)));
                total_notes += 1;
            }
            let name = match track.name.trim() {
                "" if channel == DRUMS => "Drums".to_owned(),
                "" => format!("Channel {}", channel + 1),
                n => n.to_owned(),
            };
            streams.push(Stream { name, channel, program, notes: list });
        }
    }
    let end = streams.iter().flat_map(|s| s.notes.iter().map(|n| n.1)).max().unwrap_or(0);
    if end == 0 {
        return Err(Error::Empty);
    }
    let frames = frames(smf, end);
    let mut parts = Vec::new();
    for stream in streams {
        parts.push(part(stream, &frames));
    }
    Ok(Score { title, composer: String::new(), parts })
}

/// The bars up to `end`, with the time signature, key and tempo in force at each one. A change
/// takes effect at the first bar starting at or after it.
fn frames(smf: &Smf, end: u32) -> Vec<Frame> {
    let mut meters: Vec<(u32, u8, u8)> = smf.meters.iter().map(|&(t, n, d)| (tick(t), n.max(1), d.max(1))).collect();
    meters.sort_by_key(|m| m.0);
    let mut keys: Vec<(u32, i8)> = smf.key_sigs.iter().map(|&(t, f, _)| (tick(t), f.clamp(-7, 7))).collect();
    keys.sort_by_key(|k| k.0);
    let mut tempos: Vec<(u32, f64)> =
        smf.tempos.iter().filter(|t| t.1.is_finite() && t.1 > 0.0).map(|&(t, b)| (tick(t), b)).collect();
    tempos.sort_by_key(|t| t.0);
    let at = |list: &[(u32, u8, u8)], t: u32| list.iter().rev().find(|m| m.0 <= t).map_or((4, 4), |m| (m.1, m.2));
    let mut out: Vec<Frame> = Vec::new();
    let mut start = 0u32;
    let mut last_tempo: Option<f64> = None;
    while start < end && out.len() < MAX_BARS {
        let (beats, beat_type) = at(&meters, start);
        let length = signature_ticks(beats, beat_type).max(GRID);
        let fifths = keys.iter().rev().find(|k| k.0 <= start).map_or(0, |k| k.1);
        // The tempo in force at the bar, marked where it changes.
        let now = tempos.iter().rev().find(|t| t.0 < start.saturating_add(length)).map(|t| t.1);
        let tempo = now.filter(|t| last_tempo.is_none_or(|l: f64| (t - l).abs() >= 0.01));
        if tempo.is_some() {
            last_tempo = tempo;
        }
        out.push(Frame { start, length, beats, beat_type, fifths, tempo });
        start = start.saturating_add(length);
    }
    out
}

/// Where a drum of the General MIDI map is written on a five-line staff.
fn drum_line(key: u8) -> u8 {
    match key {
        35 | 36 => 65,           // bass drum: F4
        37..=40 => 72,           // snare, side stick, clap: C5
        41 | 43 => 69,           // low toms: A4
        45 | 47 => 74,           // middle toms: D5
        48 | 50 => 76,           // high toms: E5
        42 | 44 | 46 => 79,      // hi-hat: G5
        51 | 53 | 59 => 77,      // ride: F5
        49 | 52 | 55 | 57 => 81, // crashes: A5
        _ => 72,
    }
}

/// A MIDI key as a written pitch: sharps in sharp keys and C, flats in flat keys.
pub fn spell(key: u8, fifths: i8) -> Pitch {
    const SHARPS: [(u8, i8); 12] =
        [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1), (6, 0)];
    const FLATS: [(u8, i8); 12] =
        [(0, 0), (1, -1), (1, 0), (2, -1), (2, 0), (3, 0), (4, -1), (4, 0), (5, -1), (5, 0), (6, -1), (6, 0)];
    let table = if fifths < 0 { FLATS } else { SHARPS };
    let (step, alter) = table[usize::from(key % 12)];
    Pitch { step, alter, octave: i8::try_from(key / 12).unwrap_or(9) - 1 }
}

/// What kind of instrument a stream is, from its channel, program and name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Drums,
    Piano,
    Guitar,
    Bass,
    Other,
}

fn kind(stream: &Stream) -> Kind {
    let name = stream.name.to_ascii_lowercase();
    if stream.channel == DRUMS {
        Kind::Drums
    } else if matches!(stream.program, Some(32..=39)) || name.contains("bass") {
        Kind::Bass
    } else if matches!(stream.program, Some(24..=31)) || name.contains("guitar") || name.contains("gitarre") {
        Kind::Guitar
    } else if matches!(stream.program, Some(0..=7)) || name.contains("piano") || name.contains("klavier") {
        Kind::Piano
    } else {
        Kind::Other
    }
}

fn part(stream: Stream, frames: &[Frame]) -> Part {
    let kind = kind(&stream);
    let lowest = stream.notes.iter().map(|n| n.2).min().unwrap_or(60);
    let highest = stream.notes.iter().map(|n| n.2).max().unwrap_or(60);
    let (clefs, tablature, tuning) = match kind {
        Kind::Drums => (vec![Clef::TREBLE], false, Vec::new()),
        Kind::Piano if lowest < 60 && highest >= 60 => (vec![Clef::TREBLE, Clef::BASS], false, Vec::new()),
        Kind::Guitar => (vec![Clef::GUITAR], true, tab::GUITAR.to_vec()),
        Kind::Bass => (vec![Clef { kind: ClefKind::F, line: 4, octave_change: -1 }], true, tab::BASS.to_vec()),
        _ if highest < 60 => (vec![Clef::BASS], false, Vec::new()),
        _ => (vec![Clef::TREBLE], false, Vec::new()),
    };
    let staves = clefs.len();
    let mut bars: Vec<Bar> = frames
        .iter()
        .enumerate()
        .map(|(i, f)| Bar {
            number: (i + 1).to_string(),
            beats: f.beats,
            beat_type: f.beat_type,
            fifths: f.fifths,
            tempo: f.tempo,
            length: f.length,
            notes: Vec::new(),
            flow: Flow::default(),
            chords: Vec::new(),
        })
        .collect();
    for staff in 1..=staves {
        let mut notes: Vec<(u32, u32, u8)> = stream
            .notes
            .iter()
            .filter(|n| staves == 1 || (staff == 1) == (n.2 >= 60))
            .map(|&(s, e, k)| (s, e, if kind == Kind::Drums { drum_line(k) } else { k }))
            .collect();
        notes.sort_unstable();
        notes.dedup_by(|a, b| a.0 == b.0 && a.2 == b.2);
        write_staff(&mut bars, frames, &notes, u8::try_from(staff).unwrap_or(1));
    }
    for bar in &mut bars {
        bar.notes.sort_by_key(|n| (n.start, n.staff, n.pitch.map_or(0, Pitch::midi)));
    }
    let mut part = Part { name: stream.name, tuning, capo: 0, tablature, clefs, bars };
    if tablature {
        fill_frets(&mut part);
    }
    part
}

/// Writes one staff's notes into the bars: chords and the rests between them, split at bar lines
/// and into written values, tied where a sound goes on.
fn write_staff(bars: &mut [Bar], frames: &[Frame], notes: &[(u32, u32, u8)], staff: u8) {
    let end = frames.last().map_or(0, |f| f.start + f.length);
    let mut at = 0u32;
    let mut i = 0;
    while at < end {
        // The chord starting next, or a rest up to it.
        let next = notes.get(i).map_or(end, |n| n.0.max(at));
        if next > at {
            put(bars, frames, at, next, &[], staff);
            at = next;
            continue;
        }
        let start = notes.get(i).map_or(at, |n| n.0);
        let mut j = i;
        while notes.get(j).is_some_and(|n| n.0 == start) {
            j += 1;
        }
        let chord: Vec<u8> = notes.get(i..j).unwrap_or_default().iter().map(|n| n.2).collect();
        let shortest = notes.get(i..j).unwrap_or_default().iter().map(|n| n.1).min().unwrap_or(start + GRID);
        let following = notes.get(j).map_or(end, |n| n.0);
        let stop = shortest.min(following).max(start + GRID).min(end);
        put(bars, frames, at, stop, &chord, staff);
        at = stop;
        i = j;
    }
}

/// Puts a chord (or a rest, for no keys) from `from` to `to` into the bars.
fn put(bars: &mut [Bar], frames: &[Frame], from: u32, to: u32, keys: &[u8], staff: u8) {
    let mut at = from;
    let mut first = true;
    while at < to {
        let Some(index) = frames.iter().position(|f| at >= f.start && at < f.start + f.length) else { return };
        let (Some(frame), Some(bar)) = (frames.get(index), bars.get_mut(index)) else { return };
        let piece_end = to.min(frame.start + frame.length);
        for (length, offset) in pieces(piece_end - at, at - frame.start) {
            let start = offset;
            let (value, dots) = NoteValue::from_ticks(length);
            let more = start + length < piece_end - frame.start || piece_end < to;
            if keys.is_empty() {
                bar.notes.push(note(start, length, None, value, dots, false, false, staff));
            } else {
                for &k in keys {
                    let pitch = Some(spell(k, frame.fifths));
                    bar.notes.push(note(start, length, pitch, value, dots, !first, more, staff));
                }
            }
            first = false;
        }
        at = piece_end;
    }
}

/// A length split into written values (dotted ones too), each with its offset in the bar.
fn pieces(length: u32, offset: u32) -> Vec<(u32, u32)> {
    const VALUES: [u32; 5] = [3840, 1920, 960, 480, 240];
    let mut out = Vec::new();
    let mut left = length;
    let mut at = offset;
    while left >= GRID {
        let take = VALUES.iter().flat_map(|&v| [v + v / 2, v]).find(|&t| t <= left).unwrap_or(GRID);
        out.push((take, at));
        at += take;
        left -= take;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn note(
    start: u32,
    length: u32,
    pitch: Option<Pitch>,
    value: NoteValue,
    dots: u8,
    tied: bool,
    tie_start: bool,
    staff: u8,
) -> Note {
    Note {
        start,
        length,
        pitch,
        value,
        dots,
        string: None,
        fret: None,
        tied,
        tie_start,
        slurs: Vec::new(),
        tuplet: Default::default(),
        technique: Default::default(),
        lyrics: Vec::new(),
        voice: staff,
        staff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_midi::{CtrlEvent, Note as MidiNote, Sequence, SmfTrack};

    fn n(pitch: u8, start: i64, length: i64, channel: u8) -> MidiNote {
        MidiNote { pitch, velocity: 100, release_velocity: 64, channel, start, length }
    }

    fn smf(tracks: Vec<(&str, Vec<MidiNote>, Option<i32>)>) -> Smf {
        Smf {
            format: 1,
            tracks: tracks
                .into_iter()
                .map(|(name, notes, program)| SmfTrack {
                    name: name.to_owned(),
                    sequence: Sequence {
                        notes,
                        ctrls: program
                            .map(|p| vec![CtrlEvent { tick: 0, channel: 0, kind: CtrlKind::Program, value: p }])
                            .unwrap_or_default(),
                        sysex: Vec::new(),
                    },
                })
                .collect(),
            tempos: vec![(0, 100.0)],
            meters: vec![(0, 4, 4)],
            markers: Vec::new(),
            key_sigs: vec![(0, -1, false)],
        }
    }

    #[test]
    fn a_played_line_becomes_notes_rests_and_ties() {
        // A quarter slightly late, a half note over the bar line, nothing after it.
        let file = smf(vec![("Song", Vec::new(), None), ("Lead", vec![n(70, 30, 950, 0), n(60, 2880, 1920, 0)], None)]);
        let score = from_smf(&file).unwrap_or_default();
        assert_eq!(score.title, "Song");
        assert_eq!(score.parts.len(), 1);
        let part = &score.parts[0];
        assert_eq!(part.bars.len(), 2);
        assert_eq!(part.bars[0].tempo, Some(100.0));
        let first: Vec<_> =
            part.bars[0].notes.iter().map(|n| (n.start, n.length, n.pitch.is_some(), n.tie_start)).collect();
        // B flat quarter (in F major), a half rest, then the half note's first quarter, tied.
        assert_eq!(first, vec![(0, 960, true, false), (960, 1920, false, false), (2880, 960, true, true)]);
        assert_eq!(part.bars[0].notes[0].pitch, Some(Pitch { step: 6, alter: -1, octave: 4 }));
        let second = &part.bars[1].notes;
        assert_eq!((second[0].start, second[0].length, second[0].tied), (0, 960, true));
        assert!(second.iter().skip(1).all(|n| n.pitch.is_none()), "the rest of the bar is rests");
        assert_eq!(second.iter().map(|n| n.length).sum::<u32>(), 3840);
    }

    #[test]
    fn instruments_get_their_staves_and_tablature() {
        let file = smf(vec![
            ("", vec![n(36, 0, 240, DRUMS), n(42, 0, 240, DRUMS)], None),
            ("Keys", vec![n(48, 0, 960, 0), n(64, 0, 960, 0)], Some(0)),
            ("Rhythm Guitar", vec![n(52, 0, 960, 0), n(59, 0, 960, 0)], None),
            ("", vec![n(40, 0, 960, 0)], Some(33)),
        ]);
        let score = from_smf(&file).unwrap_or_default();
        let names: Vec<_> = score.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Drums", "Keys", "Rhythm Guitar", "Channel 1"]);
        let drums = &score.parts[0].bars[0].notes;
        let lines: Vec<u8> = drums.iter().filter_map(|n| n.pitch).map(Pitch::midi).collect();
        assert_eq!(lines, vec![65, 79], "kick on F4, hi-hat on G5");
        assert_eq!(score.parts[1].clefs, vec![Clef::TREBLE, Clef::BASS]);
        let staves: Vec<_> =
            score.parts[1].bars[0].notes.iter().filter(|n| n.pitch.is_some()).map(|n| n.staff).collect();
        assert_eq!(staves, vec![1, 2]);
        assert!(score.parts[2].tablature && score.parts[3].tablature);
        assert!(score.parts[2].bars[0].notes.iter().filter(|n| n.pitch.is_some()).all(|n| n.fret.is_some()));
        assert_eq!(score.parts[3].tuning, tab::BASS.to_vec());
    }

    #[test]
    fn odd_lengths_are_written_as_tied_values() {
        assert_eq!(pieces(5 * GRID, 0), vec![(960, 0), (240, 960)]);
        assert_eq!(pieces(1440, 480), vec![(1440, 480)]);
        assert_eq!(snap(130), 240);
        assert_eq!(spell(61, 2), Pitch { step: 0, alter: 1, octave: 4 });
        assert_eq!(spell(61, -3), Pitch { step: 1, alter: -1, octave: 4 });
    }

    #[test]
    fn hostile_files_are_refused_or_bounded() {
        assert!(read_midi(b"not midi").is_err());
        assert_eq!(from_smf(&smf(vec![("", Vec::new(), None)])).err(), Some(Error::Empty));
        // A note far away does not make millions of bars.
        let file = smf(vec![("x", vec![n(60, i64::from(u32::MAX), 960, 0)], None)]);
        let score = from_smf(&file).unwrap_or_default();
        assert!(score.parts.iter().all(|p| p.bars.len() <= MAX_BARS));
    }
}
