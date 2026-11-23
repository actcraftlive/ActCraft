//! Fuzz sweep for the MusicXML and MIDI readers and the fret placement: random scores never
//! panic, and everything read stays within the documented limits.

use actcraft_notation::{MAX_BARS, MAX_FRET, MAX_PARTS, MAX_STRINGS, read, read_midi, tab::fill_frets};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const PARTS: &[&str] = &[
    "<measure number='1'>",
    "</measure>",
    "<attributes><divisions>4</divisions></attributes>",
    "<attributes><divisions>0</divisions><time><beats>7</beats><beat-type>0</beat-type></time></attributes>",
    "<attributes><clef><sign>TAB</sign></clef></attributes>",
    "<attributes><staff-details><staff-lines>3</staff-lines><staff-tuning line='1'><tuning-step>E</tuning-step><tuning-octave>2</tuning-octave></staff-tuning><staff-tuning line='9'><tuning-step>B</tuning-step><tuning-octave>9</tuning-octave></staff-tuning></staff-details></attributes>",
    "<note><pitch><step>C</step><octave>4</octave></pitch><duration>4</duration></note>",
    "<note><chord/><pitch><step>E</step><alter>-1</alter><octave>4</octave></pitch><duration>4</duration></note>",
    "<note><chord/><pitch><step>B</step><octave>0</octave></pitch><duration>99999999</duration></note>",
    "<note><rest/><duration>2</duration></note>",
    "<note><pitch><step>A</step><octave>2</octave></pitch><duration>1</duration><notations><technical><string>7</string><fret>40</fret></technical></notations></note>",
    "<note><grace/><pitch><step>G</step><octave>3</octave></pitch></note>",
    "<note><unpitched/><duration>1</duration></note>",
    "<backup><duration>100</duration></backup>",
    "<forward><duration>3</duration></forward>",
    "<direction><sound tempo='-4'/></direction>",
    "<sound tempo='120'/>",
    "<tie type='stop'/>",
    "<",
    ">",
    "&amp;",
    "&bogus;",
    "\u{0}",
    "é",
];

fn wrap(body: &str) -> String {
    format!(
        "<score-partwise><part-list><score-part id='P1'><part-name>x</part-name></score-part></part-list><part id='P1'>{body}</part></score-partwise>"
    )
}

#[test]
fn random_scores_never_panic_and_stay_bounded() {
    let mut rng = Rng(23);
    for _ in 0..3000 {
        let n = (rng.next() % 30) as usize;
        let body: String = (0..n).map(|_| rng.pick(PARTS)).collect();
        for src in [wrap(&body), body.clone()] {
            let Ok(mut score) = read(src.as_bytes()) else { continue };
            for part in &mut score.parts {
                assert!(part.tuning.len() <= MAX_STRINGS);
                fill_frets(part);
                for note in part.bars.iter().flat_map(|b| &b.notes) {
                    assert!(note.fret.is_none_or(|f| f <= MAX_FRET));
                    assert!(note.string.is_none_or(|s| s >= 1 && usize::from(s) <= MAX_STRINGS));
                }
            }
        }
        // Random bytes, plain and behind a ZIP signature.
        let bytes: Vec<u8> = (0..(rng.next() % 64)).map(|_| rng.next() as u8).collect();
        let _ = read(&bytes);
        let mut zipped = b"PK\x03\x04".to_vec();
        zipped.extend_from_slice(&bytes);
        let _ = read(&zipped);
    }
}

#[test]
fn random_midi_files_never_panic_and_stay_bounded() {
    let mut rng = Rng(31);
    // A small valid file to start from: one track, a few notes, a tempo and a time signature.
    let base: Vec<u8> = vec![
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 1, 0, 1, 0x01, 0xE0, b'M', b'T', b'r', b'k', 0, 0, 0, 0x1C, 0x00, 0xFF,
        0x51, 0x03, 0x07, 0xA1, 0x20, 0x00, 0xFF, 0x58, 0x04, 0x03, 0x02, 0x18, 0x08, 0x00, 0x90, 0x3C, 0x64, 0x83,
        0x60, 0x80, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00,
    ];
    assert!(read_midi(&base).is_ok_and(|s| s.parts.len() == 1), "the base file reads");
    for _ in 0..3000 {
        let mut bytes = base.clone();
        for _ in 0..(rng.next() % 8) {
            let i = (rng.next() % bytes.len() as u64) as usize;
            bytes[i] = rng.next() as u8;
        }
        if rng.next().is_multiple_of(4) {
            bytes.truncate((rng.next() % bytes.len() as u64) as usize);
        }
        if let Ok(score) = read_midi(&bytes) {
            assert!(score.parts.len() <= MAX_PARTS);
            assert!(score.parts.iter().all(|p| p.bars.len() <= MAX_BARS));
        }
    }
}
