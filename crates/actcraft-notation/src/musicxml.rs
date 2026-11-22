//! The MusicXML reader (part-wise scores, plain or compressed).

use craft_xml::{El, Options};

use crate::flow::{MAX_ENDING_NUMBERS, MAX_REPEAT_TIMES};
use crate::tab::fill_frets;
use crate::{
    Bar, Clef, ClefKind, Error, Flow, MAX_BARS, MAX_FRET, MAX_INPUT, MAX_NOTES, MAX_PARTS, MAX_SLURS_PER_NOTE,
    MAX_STAVES, MAX_STRINGS, Note, NoteValue, Part, Pitch, Score, Slur, TICKS_PER_QUARTER, signature_ticks,
};

/// Entries a compressed score may hold.
const MAX_ENTRIES: u64 = 4_096;
/// Divisions per quarter note accepted (the file's own time unit).
const MAX_DIVISIONS: f64 = 1_000_000.0;
/// Longest note or move accepted, in quarter notes.
const MAX_QUARTERS: f64 = 4_096.0;

/// Reads a score, plain or compressed (told apart by the ZIP signature).
pub fn read(bytes: &[u8]) -> Result<Score, Error> {
    if bytes.starts_with(b"PK\x03\x04") { read_mxl(bytes) } else { read_xml(bytes) }
}

/// Reads a compressed score (`.mxl`): the file `META-INF/container.xml` names, else the first
/// MusicXML file in the archive.
pub fn read_mxl(bytes: &[u8]) -> Result<Score, Error> {
    if bytes.len() > MAX_INPUT {
        return Err(Error::TooLarge);
    }
    let limits = craft_zip::Limits { max_entries: MAX_ENTRIES, max_total_bytes: MAX_INPUT as u64 * 2 };
    let mut zip = craft_zip::Archive::open(bytes, limits).map_err(|e| Error::Unreadable(e.to_string()))?;
    let max = MAX_INPUT as u64;
    let named = zip
        .read("META-INF/container.xml", max)
        .ok()
        .flatten()
        .and_then(|c| Options::local().parse(&c).ok())
        .and_then(|c| c.find("rootfile").and_then(|r| r.attr("full-path")).map(str::to_owned));
    let name = named.filter(|n| zip.contains(n)).or_else(|| {
        zip.names()
            .find(|n| {
                let lower = n.to_ascii_lowercase();
                !lower.starts_with("meta-inf/") && (lower.ends_with(".xml") || lower.ends_with(".musicxml"))
            })
            .map(|n| n.into_owned())
    });
    let Some(name) = name else { return Err(Error::NoScoreInArchive) };
    let xml = zip.read(&name, max).map_err(|e| Error::Unreadable(e.to_string()))?.ok_or(Error::NoScoreInArchive)?;
    read_xml(&xml)
}

/// Reads an uncompressed MusicXML score.
pub fn read_xml(bytes: &[u8]) -> Result<Score, Error> {
    if bytes.len() > MAX_INPUT {
        return Err(Error::TooLarge);
    }
    let root = Options::local().parse(bytes).map_err(|e| Error::Unreadable(e.to_string()))?;
    if root.local() != "score-partwise" {
        return Err(Error::NotAScore);
    }
    let text = |el: Option<&El>| el.map(|e| e.deep_text().trim().to_owned()).unwrap_or_default();
    let mut title = text(root.path(&["work", "work-title"]));
    if title.is_empty() {
        title = text(root.child("movement-title"));
    }
    let composer = text(
        root.child("identification").and_then(|i| i.children("creator").find(|c| c.attr("type") == Some("composer"))),
    );
    let names: Vec<(String, String)> = root
        .child("part-list")
        .map(|list| {
            list.children("score-part")
                .map(|sp| (sp.attr("id").unwrap_or_default().to_owned(), text(sp.child("part-name"))))
                .collect()
        })
        .unwrap_or_default();
    let mut budget = MAX_NOTES;
    let mut parts = Vec::new();
    for (index, el) in root.children("part").take(MAX_PARTS).enumerate() {
        let id = el.attr("id").unwrap_or_default();
        let name = names
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, n)| n.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Part {}", index + 1));
        let mut part = read_part(el, name, &mut budget);
        if part.tablature {
            fill_frets(&mut part);
        }
        parts.push(part);
    }
    if parts.iter().all(|p| p.bars.iter().all(|b| b.notes.iter().all(|n| n.pitch.is_none()))) {
        return Err(Error::Empty);
    }
    Ok(Score { title, composer, parts })
}

/// What carries over from bar to bar.
struct State {
    divisions: f64,
    beats: u8,
    beat_type: u8,
    fifths: i8,
    /// The first clef written for each staff.
    clefs: [Option<Clef>; MAX_STAVES],
    /// Staves the part declares (`<staves>`).
    staves: usize,
    /// The numbers of the ending whose bracket is open, carried until its stop.
    ending: Option<Vec<u8>>,
}

fn read_part(el: &El, name: String, budget: &mut usize) -> Part {
    let mut part = Part { name, ..Part::default() };
    let mut state =
        State { divisions: 1.0, beats: 4, beat_type: 4, fifths: 0, clefs: [None; MAX_STAVES], staves: 1, ending: None };
    for measure in el.children("measure").take(MAX_BARS) {
        let bar = read_bar(measure, &mut state, &mut part, budget);
        part.bars.push(bar);
    }
    if part.bars.iter().any(|b| b.notes.iter().any(|n| n.fret.is_some())) {
        part.tablature = true;
    }
    let used = part.bars.iter().flat_map(|b| &b.notes).map(|n| usize::from(n.staff)).max().unwrap_or(1);
    let staves = used.max(state.staves).clamp(1, MAX_STAVES);
    part.clefs = (0..staves)
        .map(|i| {
            let fallback = if i > 0 {
                Clef::BASS
            } else if part.tablature {
                Clef::GUITAR
            } else {
                Clef::TREBLE
            };
            state.clefs.get(i).copied().flatten().unwrap_or(fallback)
        })
        .collect();
    part
}

fn read_bar(measure: &El, state: &mut State, part: &mut Part, budget: &mut usize) -> Bar {
    let mut notes = Vec::new();
    let mut tempo = None;
    let mut cursor: u32 = 0;
    let mut last_start: u32 = 0;
    let mut reach: u32 = 0;
    let mut flow = Flow::default();
    let mut ending_ends = false;
    for child in measure.elements() {
        match child.local() {
            "attributes" => read_attributes(child, state, part),
            "direction" => {
                if let Some(sound) = child.find("sound") {
                    if let Some(t) = sound_tempo(sound) {
                        tempo = Some(t);
                    }
                    sound_flow(sound, &mut flow);
                }
                // The segno sign itself marks where D.S. goes back to.
                if child.find("segno").is_some() {
                    flow.segno = true;
                }
            }
            "sound" => {
                if let Some(t) = sound_tempo(child) {
                    tempo = Some(t);
                }
                sound_flow(child, &mut flow);
            }
            "barline" => ending_ends |= read_barline(child, state, &mut flow),
            "backup" => cursor = cursor.saturating_sub(ticks(child.child_text("duration").as_deref(), state)),
            "forward" => {
                cursor = cursor.saturating_add(ticks(child.child_text("duration").as_deref(), state));
                reach = reach.max(cursor);
            }
            "note" => {
                if child.child("grace").is_some() || child.child("cue").is_some() {
                    continue;
                }
                let length = ticks(child.child_text("duration").as_deref(), state);
                let start = if child.child("chord").is_some() { last_start } else { cursor };
                if child.child("chord").is_none() {
                    last_start = cursor;
                    cursor = cursor.saturating_add(length);
                }
                reach = reach.max(start.saturating_add(length));
                // Drum notes have no pitch to show; a pitch that cannot be read is left out
                // rather than shown as a rest. Both still take their time.
                if child.child("unpitched").is_some() || *budget == 0 {
                    continue;
                }
                if let Some(note) = read_note(child, start, length) {
                    *budget -= 1;
                    notes.push(note);
                }
            }
            _ => {}
        }
    }
    notes.sort_by(|a: &Note, b: &Note| {
        a.start.cmp(&b.start).then_with(|| b.pitch.map(Pitch::midi).cmp(&a.pitch.map(Pitch::midi)))
    });
    let signature = signature_ticks(state.beats, state.beat_type);
    flow.endings = state.ending.clone().unwrap_or_default();
    if ending_ends {
        state.ending = None;
    }
    Bar {
        number: measure.attr("number").unwrap_or_default().chars().take(16).collect(),
        beats: state.beats,
        beat_type: state.beat_type,
        fifths: state.fifths,
        tempo,
        length: if reach == 0 { signature } else { reach },
        notes,
        flow,
    }
}

/// Repeat signs, ending brackets, segno and coda at a bar line. Returns whether an open ending
/// stops at this bar.
fn read_barline(el: &El, state: &mut State, flow: &mut Flow) -> bool {
    if let Some(repeat) = el.child("repeat") {
        match repeat.attr("direction") {
            Some("forward") => flow.repeat_start = true,
            Some("backward") => {
                let times = number(repeat.attr("times")).map_or(2.0, |t| t.clamp(2.0, f64::from(MAX_REPEAT_TIMES)));
                flow.repeat_end = Some(times as u8);
            }
            _ => {}
        }
    }
    if el.child("segno").is_some() {
        flow.segno = true;
    }
    if el.child("coda").is_some() {
        flow.coda = true;
    }
    let mut ends = false;
    if let Some(ending) = el.child("ending") {
        match ending.attr("type") {
            Some("start") => {
                let numbers = ending_numbers(ending.attr("number").unwrap_or_default());
                if !numbers.is_empty() {
                    state.ending = Some(numbers);
                    flow.ending_start = true;
                }
            }
            Some("stop" | "discontinue") => ends = true,
            _ => {}
        }
    }
    ends
}

/// Ending numbers as written: "1", "1, 2", "1 2"; numbers out of range are left out.
fn ending_numbers(text: &str) -> Vec<u8> {
    let mut numbers: Vec<u8> = text
        .split([',', ' '])
        .filter_map(|x| x.trim().parse::<u8>().ok())
        .filter(|n| (1..=MAX_REPEAT_TIMES).contains(n))
        .take(MAX_ENDING_NUMBERS)
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// Playback jumps a `<sound>` element carries (D.C., D.S., Fine, To Coda and their targets).
fn sound_flow(el: &El, flow: &mut Flow) {
    let yes = |name: &str| el.attr(name).is_some_and(|v| !v.trim().is_empty() && v.trim() != "no");
    flow.segno |= yes("segno");
    flow.coda |= yes("coda");
    flow.to_coda |= yes("tocoda");
    flow.da_capo |= yes("dacapo");
    flow.dal_segno |= yes("dalsegno");
    flow.fine |= yes("fine");
}

fn read_attributes(el: &El, state: &mut State, part: &mut Part) {
    if let Some(d) = number(el.child_text("divisions").as_deref()).filter(|d| *d > 0.0 && *d <= MAX_DIVISIONS) {
        state.divisions = d;
    }
    if let Some(f) = el.child("key").and_then(|k| number(k.child_text("fifths").as_deref())) {
        state.fifths = f.round().clamp(-7.0, 7.0) as i8;
    }
    if let Some(time) = el.child("time") {
        // "3+2" is a compound meter: its beats add up.
        let beats = time.child_text("beats").map(|b| b.split('+').filter_map(|x| number(Some(x))).sum::<f64>());
        let beat_type = number(time.child_text("beat-type").as_deref());
        if let (Some(b), Some(t)) = (beats, beat_type)
            && (1.0..=64.0).contains(&b)
            && [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0].contains(&t)
        {
            state.beats = b as u8;
            state.beat_type = t as u8;
        }
    }
    if let Some(n) = number(el.child_text("staves").as_deref()) {
        state.staves = n.clamp(1.0, MAX_STAVES as f64) as usize;
    }
    for c in el.children("clef") {
        let staff = c.attr_u32("number").unwrap_or(1).clamp(1, MAX_STAVES as u32) as usize - 1;
        if let (Some(clef), Some(slot)) = (read_clef(c), state.clefs.get_mut(staff))
            && slot.is_none()
        {
            *slot = Some(clef);
        }
    }
    if el.children("clef").any(|c| c.child_text("sign").is_some_and(|s| s.trim().eq_ignore_ascii_case("tab"))) {
        part.tablature = true;
    }
    if let Some(details) = el.child("staff-details") {
        let lines = number(details.child_text("staff-lines").as_deref()).map_or(0, |l| l.clamp(0.0, 64.0) as usize);
        // Line 1 is the lowest string; ActCraft keeps the highest first.
        let mut tuning: Vec<(u32, u8)> = details
            .children("staff-tuning")
            .filter_map(|t| {
                let line = t.attr_u32("line")?;
                let pitch = Pitch {
                    step: step(t.child_text("tuning-step").as_deref())?,
                    alter: alter(t.child_text("tuning-alter").as_deref()),
                    octave: octave(t.child_text("tuning-octave").as_deref())?,
                };
                Some((line, pitch.midi()))
            })
            .collect();
        tuning.sort_by_key(|t| std::cmp::Reverse(t.0));
        tuning.dedup_by_key(|t| t.0);
        if !tuning.is_empty() && tuning.len() <= MAX_STRINGS && (lines == 0 || lines == tuning.len()) {
            part.tuning = tuning.into_iter().map(|t| t.1).collect();
            part.tablature = true;
        }
        if let Some(c) = number(details.child_text("capo").as_deref()) {
            part.capo = c.clamp(0.0, f64::from(MAX_FRET)) as u8;
        }
    }
}

/// A note or rest; `None` for a note whose pitch cannot be read.
fn read_note(el: &El, start: u32, length: u32) -> Option<Note> {
    let pitch = match el.child("pitch") {
        Some(p) => Some(Pitch {
            step: step(p.child_text("step").as_deref())?,
            alter: alter(p.child_text("alter").as_deref()),
            octave: octave(p.child_text("octave").as_deref())?,
        }),
        None => None,
    };
    let written = el.child_text("type").and_then(|t| value(t.trim()));
    let (value, dots) = match written {
        Some(v) => (v, el.children("dot").count().min(3) as u8),
        None => NoteValue::from_ticks(length),
    };
    let technical = el.child("notations").and_then(|n| n.find("technical"));
    let small = |name: &str, max: f64| {
        technical
            .and_then(|t| number(t.child_text(name).as_deref()))
            .filter(|v| *v >= 0.0 && *v <= max)
            .map(|v| v as u8)
    };
    // A tie is written as a sound (`<tie>`) and as a sign (`<notations><tied>`); either counts.
    let notations = el.child("notations");
    let tie = |kind: &str| {
        el.children("tie").any(|t| t.attr("type") == Some(kind))
            || notations.is_some_and(|n| n.children("tied").any(|t| t.attr("type") == Some(kind)))
    };
    let tied = tie("stop");
    let tie_start = tie("start");
    let slurs = notations.map(read_slurs).unwrap_or_default();
    Some(Note {
        start,
        length,
        pitch,
        value,
        dots,
        string: small("string", MAX_STRINGS as f64).filter(|s| *s >= 1),
        fret: small("fret", f64::from(MAX_FRET)),
        tied,
        tie_start,
        slurs,
        voice: number(el.child_text("voice").as_deref()).map_or(1, |v| v.clamp(1.0, 64.0) as u8),
        staff: number(el.child_text("staff").as_deref()).map_or(1, |v| v.clamp(1.0, 8.0) as u8),
    })
}

/// The slur starts and stops a note's `<notations>` hold; a "continue" only passes through.
fn read_slurs(notations: &El) -> Vec<Slur> {
    notations
        .children("slur")
        .filter_map(|s| {
            let start = match s.attr("type") {
                Some("start") => true,
                Some("stop") => false,
                _ => return None,
            };
            let number = number(s.attr("number")).map_or(1, |n| n.clamp(1.0, 16.0) as u8);
            let above = match (s.attr("placement"), s.attr("orientation")) {
                (Some("above"), _) | (_, Some("over")) => Some(true),
                (Some("below"), _) | (_, Some("under")) => Some(false),
                _ => None,
            };
            Some(Slur { number, start, above })
        })
        .take(MAX_SLURS_PER_NOTE)
        .collect()
}

/// A G, F or C clef; `None` for tablature, percussion and anything else.
fn read_clef(el: &El) -> Option<Clef> {
    let (kind, line) = match el.child_text("sign")?.trim() {
        "G" => (ClefKind::G, 2),
        "F" => (ClefKind::F, 4),
        "C" => (ClefKind::C, 3),
        _ => return None,
    };
    let line = number(el.child_text("line").as_deref()).filter(|l| (1.0..=5.0).contains(l)).map_or(line, |l| l as u8);
    let octave_change = number(el.child_text("clef-octave-change").as_deref()).map_or(0, |o| o.clamp(-2.0, 2.0) as i8);
    Some(Clef { kind, line, octave_change })
}

fn sound_tempo(el: &El) -> Option<f64> {
    el.attr("tempo").and_then(|t| number(Some(t))).filter(|t| *t >= 1.0 && *t <= 1_000.0)
}

/// A length in the file's divisions, as ticks.
fn ticks(text: Option<&str>, state: &State) -> u32 {
    let Some(d) = number(text).filter(|d| *d > 0.0) else { return 0 };
    let quarters = (d / state.divisions).min(MAX_QUARTERS);
    (quarters * f64::from(TICKS_PER_QUARTER)).round() as u32
}

fn number(text: Option<&str>) -> Option<f64> {
    text?.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn step(text: Option<&str>) -> Option<u8> {
    match text?.trim() {
        "C" | "c" => Some(0),
        "D" | "d" => Some(1),
        "E" | "e" => Some(2),
        "F" | "f" => Some(3),
        "G" | "g" => Some(4),
        "A" | "a" => Some(5),
        "B" | "b" => Some(6),
        _ => None,
    }
}

fn alter(text: Option<&str>) -> i8 {
    number(text).map_or(0, |a| a.round().clamp(-2.0, 2.0) as i8)
}

fn octave(text: Option<&str>) -> Option<i8> {
    number(text).filter(|o| (-1.0..=9.0).contains(o)).map(|o| o as i8)
}

fn value(name: &str) -> Option<NoteValue> {
    Some(match name {
        "maxima" | "long" | "breve" | "whole" => NoteValue::Whole,
        "half" => NoteValue::Half,
        "quarter" => NoteValue::Quarter,
        "eighth" => NoteValue::Eighth,
        "16th" => NoteValue::Sixteenth,
        "32nd" => NoteValue::ThirtySecond,
        "64th" | "128th" | "256th" | "512th" | "1024th" => NoteValue::SixtyFourth,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-bar guitar part as tablature editors write it: tuning, a TAB clef, string and fret on
    /// each note, a chord, a tie and a tempo.
    pub(crate) const TAB_SCORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="4.0">
  <work><work-title>Night Drive</work-title></work>
  <identification><creator type="composer">The Band</creator></identification>
  <part-list><score-part id="P1"><part-name>Lead Guitar</part-name></score-part></part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>2</divisions>
        <key><fifths>1</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>TAB</sign><line>5</line></clef>
        <staff-details>
          <staff-lines>6</staff-lines>
          <staff-tuning line="1"><tuning-step>D</tuning-step><tuning-octave>2</tuning-octave></staff-tuning>
          <staff-tuning line="2"><tuning-step>A</tuning-step><tuning-octave>2</tuning-octave></staff-tuning>
          <staff-tuning line="3"><tuning-step>D</tuning-step><tuning-octave>3</tuning-octave></staff-tuning>
          <staff-tuning line="4"><tuning-step>G</tuning-step><tuning-octave>3</tuning-octave></staff-tuning>
          <staff-tuning line="5"><tuning-step>B</tuning-step><tuning-octave>3</tuning-octave></staff-tuning>
          <staff-tuning line="6"><tuning-step>E</tuning-step><tuning-octave>4</tuning-octave></staff-tuning>
        </staff-details>
      </attributes>
      <direction><sound tempo="96"/></direction>
      <note><pitch><step>D</step><octave>2</octave></pitch><duration>2</duration><voice>1</voice><type>quarter</type>
        <notations><technical><string>6</string><fret>0</fret></technical></notations></note>
      <note><pitch><step>F</step><alter>1</alter><octave>3</octave></pitch><duration>1</duration><voice>1</voice><type>eighth</type>
        <notations><technical><string>4</string><fret>4</fret></technical></notations></note>
      <note><chord/><pitch><step>A</step><octave>3</octave></pitch><duration>1</duration><voice>1</voice><type>eighth</type>
        <notations><technical><string>3</string><fret>2</fret></technical></notations></note>
      <note><rest/><duration>1</duration><voice>1</voice><type>eighth</type></note>
      <note><grace/><pitch><step>B</step><octave>3</octave></pitch><voice>1</voice><type>eighth</type></note>
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>4</duration><voice>1</voice><type>half</type>
        <tie type="start"/><notations><technical><string>1</string><fret>0</fret></technical></notations></note>
    </measure>
    <measure number="2">
      <note><pitch><step>E</step><octave>4</octave></pitch><duration>6</duration><voice>1</voice><type>half</type><dot/>
        <tie type="stop"/><notations><technical><string>1</string><fret>0</fret></technical></notations></note>
      <note><pitch><step>G</step><octave>4</octave></pitch><duration>2</duration><voice>1</voice></note>
    </measure>
  </part>
</score-partwise>"#;

    #[test]
    fn a_tab_score_reads_with_tuning_frets_chords_and_ties() {
        let score = read(TAB_SCORE.as_bytes());
        assert!(score.is_ok(), "{score:?}");
        let Ok(score) = score else { return };
        assert_eq!(score.title, "Night Drive");
        assert_eq!(score.composer, "The Band");
        let part = &score.parts[0];
        assert_eq!(part.name, "Lead Guitar");
        assert!(part.tablature);
        assert_eq!(part.tuning, vec![64, 59, 55, 50, 45, 38], "drop D, highest first");
        assert_eq!(part.clefs, vec![Clef::GUITAR], "tab only: the staff above reads as guitar music");
        assert_eq!(part.bars.len(), 2);
        let bar = &part.bars[0];
        assert_eq!((bar.beats, bar.beat_type, bar.fifths, bar.tempo), (4, 4, 1, Some(96.0)));
        assert_eq!(bar.length, 3840);
        type Row = (u32, u32, Option<u8>, Option<u8>, Option<u8>);
        let got: Vec<Row> =
            bar.notes.iter().map(|n| (n.start, n.length, n.pitch.map(Pitch::midi), n.string, n.fret)).collect();
        assert_eq!(
            got,
            vec![
                (0, 960, Some(38), Some(6), Some(0)),
                (960, 480, Some(57), Some(3), Some(2)),
                (960, 480, Some(54), Some(4), Some(4)),
                (1440, 480, None, None, None),
                (1920, 1920, Some(64), Some(1), Some(0)),
            ]
        );
        let second = &part.bars[1];
        assert_eq!((second.number.as_str(), second.beats, second.fifths), ("2", 4, 1), "carried over");
        assert!(second.notes[0].tied);
        assert!(part.bars[0].notes.iter().any(|n| n.tie_start), "the tie starts in bar 1");
        assert_eq!((second.notes[0].value, second.notes[0].dots), (NoteValue::Half, 1));
        // No type written: taken from the length. No fret written: placed on the tuning.
        assert_eq!(second.notes[1].value, NoteValue::Quarter);
        assert_eq!((second.notes[1].string, second.notes[1].fret), (Some(1), Some(3)));
    }

    /// A one-entry ZIP with the entry stored (no compression).
    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let mut w = craft_zip::Writer::new();
        assert!(w.add(name, data, craft_zip::Compression::Stored).is_ok());
        w.finish().unwrap_or_default()
    }

    #[test]
    fn compressed_scores_read_through_the_container_or_the_first_score() {
        let plain = stored_zip("song/score.musicxml", TAB_SCORE.as_bytes());
        assert_eq!(read(&plain).map(|s| s.title), Ok("Night Drive".to_owned()));
        let mut w = craft_zip::Writer::new();
        let container = br#"<container><rootfiles><rootfile full-path="b.xml"/></rootfiles></container>"#;
        assert!(w.add("META-INF/container.xml", container, craft_zip::Compression::Deflated).is_ok());
        assert!(w.add("a.xml", b"<score-partwise/>", craft_zip::Compression::Deflated).is_ok());
        assert!(w.add("b.xml", TAB_SCORE.as_bytes(), craft_zip::Compression::Deflated).is_ok());
        let named = w.finish().unwrap_or_default();
        assert_eq!(read(&named).map(|s| s.parts.len()), Ok(1), "the container's file, not the first");
        assert_eq!(read(&stored_zip("notes.txt", b"hi")), Err(Error::NoScoreInArchive));
    }

    #[test]
    fn plain_scores_get_tab_only_when_written_as_tab() {
        let xml = r#"<score-partwise><part-list><score-part id="P1"/></part-list><part id="P1">
            <measure number="1"><attributes><divisions>1</divisions><clef number="1"><sign>G</sign><line>2</line></clef><clef number="1"><sign>F</sign></clef><time><beats>3+2</beats><beat-type>8</beat-type></time></attributes>
            <note><pitch><step>C</step><octave>5</octave></pitch><duration>1</duration></note>
            <backup><duration>5</duration></backup>
            <note><pitch><step>C</step><octave>3</octave></pitch><duration>1</duration><staff>2</staff></note>
            <forward><duration>9</duration></forward>
            </measure></part></score-partwise>"#;
        let score = read(xml.as_bytes());
        assert!(score.is_ok(), "{score:?}");
        let Ok(score) = score else { return };
        let part = &score.parts[0];
        assert_eq!(part.name, "Part 1");
        assert!(!part.tablature);
        assert_eq!(part.clefs, vec![Clef::TREBLE, Clef::BASS], "a second staff in use");
        let bar = &part.bars[0];
        assert_eq!((bar.beats, bar.beat_type), (5, 8));
        assert_eq!(
            bar.notes.iter().map(|n| (n.start, n.staff, n.fret)).collect::<Vec<_>>(),
            vec![(0, 1, None), (0, 2, None)]
        );
        assert_eq!(bar.length, 960 * 10, "the forward reaches furthest");
    }

    #[test]
    fn repeats_endings_and_jumps_are_read_and_unrolled() {
        let bar = |n: u32, inner: &str| {
            format!(
                r#"<measure number="{n}">{inner}<note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration></note></measure>"#
            )
        };
        let body = [
            bar(1, r#"<attributes><divisions>1</divisions></attributes>
                <barline location="left"><repeat direction="forward"/></barline>"#),
            bar(2, r#"<direction><direction-type><segno/></direction-type><sound segno="s1"/></direction>"#),
            bar(3, r#"<barline location="left"><ending number="1, 2" type="start"/></barline>
                <barline location="right"><ending number="1, 2" type="stop"/><repeat direction="backward" times="3"/></barline>"#),
            bar(4, r#"<barline location="left"><ending number="3" type="start"/></barline>
                <barline location="right"><ending number="3" type="discontinue"/></barline>
                <direction><sound tocoda="c1"/></direction>"#),
            bar(5, r#"<sound dalsegno="s1"/>"#),
            bar(6, r#"<direction><sound coda="c1"/></direction>"#),
        ]
        .concat();
        let xml = format!(
            r#"<score-partwise><part-list><score-part id="P1"><part-name>Keys</part-name></score-part></part-list>
            <part id="P1">{body}</part></score-partwise>"#
        );
        let score = read(xml.as_bytes());
        assert!(score.is_ok(), "{score:?}");
        let score = score.unwrap_or_default();
        let bars = &score.parts[0].bars;
        assert_eq!(bars.len(), 6);
        assert!(bars[0].flow.repeat_start);
        assert!(bars[1].flow.segno);
        assert_eq!(bars[2].flow.endings, vec![1, 2]);
        assert!(bars[2].flow.ending_start);
        assert_eq!(bars[2].flow.repeat_end, Some(3));
        assert_eq!(bars[3].flow.endings, vec![3]);
        assert!(bars[3].flow.to_coda);
        assert!(bars[4].flow.dal_segno && bars[4].flow.endings.is_empty(), "the ending closed");
        assert!(bars[5].flow.coda);
        // Three passes with endings, then D.S. to bar 2, to the coda after the last ending.
        assert_eq!(crate::play_order(bars), vec![0, 1, 2, 0, 1, 2, 0, 1, 3, 4, 1, 3, 5]);
        assert_eq!(ending_numbers("1 2, 2, 0, 99, x"), vec![1, 2]);
    }

    #[test]
    fn slurs_and_tie_signs_are_read() {
        let note = |step: &str, notations: &str| {
            format!(
                r#"<note><pitch><step>{step}</step><octave>4</octave></pitch><duration>1</duration><notations>{notations}</notations></note>"#
            )
        };
        let notes = [
            note("C", r#"<slur type="start" number="1" placement="below"/><tied type="start"/>"#),
            note("C", r#"<tied type="stop"/><slur type="continue" number="1"/>"#),
            note("E", r#"<slur type="stop" number="1"/><slur type="start" number="2" orientation="over"/>"#),
            note("G", r#"<slur type="stop" number="99"/>"#),
        ]
        .concat();
        let xml = format!(
            r#"<score-partwise><part-list><score-part id="P1"/></part-list><part id="P1">
            <measure number="1"><attributes><divisions>1</divisions></attributes>{notes}</measure></part></score-partwise>"#
        );
        let score = read(xml.as_bytes());
        assert!(score.is_ok(), "{score:?}");
        let Ok(score) = score else { return };
        let n = &score.parts[0].bars[0].notes;
        assert!(n[0].tie_start && !n[0].tied);
        assert!(n[1].tied && !n[1].tie_start, "the tie sign counts like the tie sound");
        assert_eq!(n[0].slurs, vec![Slur { number: 1, start: true, above: Some(false) }]);
        assert!(n[1].slurs.is_empty(), "a continue only passes through");
        assert_eq!(
            n[2].slurs,
            vec![Slur { number: 1, start: false, above: None }, Slur { number: 2, start: true, above: Some(true) }]
        );
        assert_eq!(n[3].slurs, vec![Slur { number: 16, start: false, above: None }], "numbers are bounded");
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(read(b"<score-timewise/>"), Err(Error::NotAScore));
        assert_eq!(read(b"<html><body/></html>"), Err(Error::NotAScore));
        assert!(matches!(read(b"\x00\x01 not xml <"), Err(Error::Unreadable(_)) | Err(Error::NotAScore)));
        assert_eq!(read(b"<score-partwise><part id='P1'><measure/></part></score-partwise>"), Err(Error::Empty));
        assert!(matches!(read(b"PK\x03\x04garbage"), Err(Error::Unreadable(_))));
        assert_eq!(read(&vec![b' '; MAX_INPUT + 1]), Err(Error::TooLarge));
    }

    #[test]
    fn hostile_numbers_are_bounded() {
        let xml = r#"<score-partwise><part id="P1"><measure>
            <attributes><divisions>0.0000001</divisions><key><fifths>99</fifths></key>
            <time><beats>NaN</beats><beat-type>3</beat-type></time>
            <staff-details><staff-tuning line="1"><tuning-step>X</tuning-step></staff-tuning><capo>999</capo></staff-details></attributes>
            <note><pitch><step>C</step><alter>-50</alter><octave>4</octave></pitch><duration>1e300</duration>
              <notations><technical><string>-3</string><fret>9999</fret></technical></notations><voice>300</voice></note>
            <backup><duration>1e300</duration></backup>
            <note><chord/><pitch><step>G</step><octave>99</octave></pitch><duration>-5</duration></note>
            </measure></part></score-partwise>"#;
        let score = read(xml.as_bytes());
        assert!(score.is_ok(), "{score:?}");
        let Ok(score) = score else { return };
        let part = &score.parts[0];
        let bar = &part.bars[0];
        assert_eq!((bar.fifths, bar.beats, bar.beat_type, part.capo), (7, 4, 4, MAX_FRET));
        let n = &bar.notes[0];
        assert_eq!(n.length, (MAX_QUARTERS as u32) * TICKS_PER_QUARTER, "capped");
        assert_eq!(n.pitch.map(|p| p.alter), Some(-2));
        assert_eq!((n.string, n.fret, n.voice), (None, None, 64));
        assert_eq!(bar.notes.len(), 1, "a pitch with a bad octave is left out, not made a rest");
    }
}
