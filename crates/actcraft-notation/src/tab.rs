//! Tablature: a string and fret for every pitched note.
//!
//! A file that writes frets keeps them. Notes without one get the playable spot with the lowest
//! frets: the notes struck together go on different strings, higher notes on higher strings, and a
//! chord's frets stay close together.

use crate::{MAX_FRET, Note, Part};

/// A standard-tuned six-string guitar, highest string first (E4 B3 G3 D3 A2 E2).
pub const GUITAR: [u8; 6] = [64, 59, 55, 50, 45, 40];
/// A standard-tuned four-string bass, highest string first (G2 D2 A1 E1).
pub const BASS: [u8; 4] = [43, 38, 33, 28];

impl Part {
    /// The open strings to use: the file's tuning, else a bass when the part goes below a guitar's
    /// low E, else a guitar.
    pub fn strings(&self) -> Vec<u8> {
        if !self.tuning.is_empty() {
            return self.tuning.clone();
        }
        let lowest = self.bars.iter().flat_map(|b| &b.notes).filter_map(|n| n.pitch).map(|p| p.midi()).min();
        if lowest.is_some_and(|l| l < GUITAR[5]) { BASS.to_vec() } else { GUITAR.to_vec() }
    }
}

/// Gives every pitched note of the part a string and a fret, keeping the ones the file wrote.
pub fn fill_frets(part: &mut Part) {
    let strings = part.strings();
    let capo = part.capo;
    for bar in &mut part.bars {
        let mut i = 0;
        while i < bar.notes.len() {
            let start = bar.notes.get(i).map_or(0, |n| n.start);
            let end = bar.notes[i..].iter().position(|n| n.start != start).map_or(bar.notes.len(), |p| i + p);
            if let Some(group) = bar.notes.get_mut(i..end) {
                place(group, &strings, capo);
            }
            i = end.max(i + 1);
        }
    }
}

/// A string index and fret per note, `None` for a note placed nowhere.
type Spots = Vec<Option<(usize, u8)>>;

/// Places the notes struck together.
fn place(group: &mut [Note], strings: &[u8], capo: u8) {
    let n = strings.len();
    let valid = |s: u8, f: u8| usize::from(s) >= 1 && usize::from(s) <= n && f <= MAX_FRET;
    // Strings the file already used are taken.
    let mut taken = vec![false; n];
    for note in group.iter_mut() {
        match (note.string, note.fret) {
            (Some(s), Some(f)) if valid(s, f) => {
                if let Some(t) = taken.get_mut(usize::from(s) - 1) {
                    *t = true;
                }
            }
            _ => {
                note.string = None;
                note.fret = None;
            }
        }
    }
    // The rest, highest pitch first.
    let mut todo: Vec<(usize, u8)> = group
        .iter()
        .enumerate()
        .filter(|(_, note)| note.string.is_none())
        .filter_map(|(i, note)| note.pitch.map(|p| (i, p.midi())))
        .collect();
    todo.sort_by_key(|t| std::cmp::Reverse(t.1));
    let free: Vec<usize> = (0..n).filter(|&s| !taken.get(s).copied().unwrap_or(true)).collect();
    // More notes than free strings: the highest ones are placed, the rest stay without a spot.
    todo.truncate(free.len());
    let pitches: Vec<u8> = todo.iter().map(|t| t.1).collect();
    let mut best: Option<(u32, Spots)> = None;
    let mut current = Vec::with_capacity(pitches.len());
    search(&pitches, &free, strings, capo, 0, &mut current, &mut best);
    if let Some((_, spots)) = best {
        for ((index, _), spot) in todo.iter().zip(spots) {
            if let (Some(note), Some((s, f))) = (group.get_mut(*index), spot) {
                note.string = u8::try_from(s + 1).ok();
                note.fret = Some(f);
            }
        }
    }
}

/// Tries every way to put the notes (highest first) on increasing free strings; a note that fits
/// nowhere is left without a spot and costs most. There are no more notes than free strings and at
/// most [`crate::MAX_STRINGS`] strings, so the search stays small (at most C(12, 6) ways).
fn search(
    pitches: &[u8],
    free: &[usize],
    strings: &[u8],
    capo: u8,
    from: usize,
    current: &mut Spots,
    best: &mut Option<(u32, Spots)>,
) {
    let Some((&pitch, rest)) = pitches.split_first() else {
        let cost = cost(current);
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            *best = Some((cost, current.clone()));
        }
        return;
    };
    let mut fitted = false;
    for (k, &s) in free.iter().enumerate().skip(from) {
        let Some(&open) = strings.get(s) else { continue };
        let Some(fret) = pitch.checked_sub(open) else { continue };
        if fret > MAX_FRET || (fret != 0 && fret < capo) {
            continue;
        }
        fitted = true;
        current.push(Some((s, fret)));
        search(rest, free, strings, capo, k + 1, current, best);
        current.pop();
    }
    if !fitted {
        current.push(None);
        search(rest, free, strings, capo, from, current, best);
        current.pop();
    }
}

/// Lower is easier to play: low frets, a narrow hand span, every note placed.
fn cost(spots: &[Option<(usize, u8)>]) -> u32 {
    let mut sum = 0u32;
    let (mut lo, mut hi) = (u8::MAX, 0u8);
    for spot in spots {
        match spot {
            Some((_, f)) => {
                sum += u32::from(*f);
                if *f > 0 {
                    lo = lo.min(*f);
                    hi = hi.max(*f);
                }
            }
            None => sum += 1000,
        }
    }
    let span = if hi >= lo { u32::from(hi - lo) } else { 0 };
    sum + span * 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bar, Pitch};

    fn note(start: u32, midi: u8) -> Note {
        // Spell the MIDI number with sharps.
        const STEPS: [(u8, i8); 12] =
            [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1), (6, 0)];
        let (step, alter) = STEPS[usize::from(midi % 12)];
        let pitch = Pitch { step, alter, octave: (midi / 12) as i8 - 1 };
        assert_eq!(pitch.midi(), midi);
        Note { start, length: 960, pitch: Some(pitch), ..Note::default() }
    }

    fn part(notes: Vec<Note>) -> Part {
        let bar = Bar {
            number: "1".into(),
            beats: 4,
            beat_type: 4,
            fifths: 0,
            tempo: None,
            length: 3840,
            notes,
            flow: Default::default(),
        };
        Part { name: "Guitar".into(), bars: vec![bar], ..Part::default() }
    }

    fn spots(p: &Part) -> Vec<(Option<u8>, Option<u8>)> {
        p.bars[0].notes.iter().map(|n| (n.string, n.fret)).collect()
    }

    #[test]
    fn single_notes_take_the_lowest_fret() {
        let mut p = part(vec![note(0, 40), note(960, 64), note(1920, 59), note(2880, 69)]);
        fill_frets(&mut p);
        assert_eq!(spots(&p), vec![(Some(6), Some(0)), (Some(1), Some(0)), (Some(2), Some(0)), (Some(1), Some(5))]);
    }

    #[test]
    fn a_chord_goes_on_separate_strings() {
        // C major in first position: C3 E3 G3 C4 E4.
        let mut p = part(vec![note(0, 48), note(0, 52), note(0, 55), note(0, 60), note(0, 64)]);
        fill_frets(&mut p);
        let got = spots(&p);
        let used: Vec<u8> = got.iter().filter_map(|s| s.0).collect();
        let mut unique = used.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 5, "{got:?}");
        assert_eq!(
            got,
            vec![(Some(5), Some(3)), (Some(4), Some(2)), (Some(3), Some(0)), (Some(2), Some(1)), (Some(1), Some(0))]
        );
    }

    #[test]
    fn written_frets_stay_and_impossible_notes_get_none() {
        let mut written = note(0, 64);
        written.string = Some(2);
        written.fret = Some(5);
        let mut bad = note(960, 60);
        bad.string = Some(9);
        bad.fret = Some(1);
        let mut p = part(vec![written, note(0, 59), bad, note(1920, 20), note(2880, 127)]);
        fill_frets(&mut p);
        let got = spots(&p);
        assert_eq!(got[0], (Some(2), Some(5)), "kept as written");
        assert_ne!(got[1].0, Some(2), "string 2 is taken: {got:?}");
        assert!(got[2].0.is_some_and(|s| s <= 6), "a bad string is placed again");
        assert_eq!(got[3], (None, None), "below the lowest string");
        assert_eq!(got[4], (None, None), "above the highest fret");
    }

    #[test]
    fn more_notes_than_strings_leaves_the_rest_unplaced() {
        let mut p = part((0..9).map(|i| note(0, 50 + i * 2)).collect());
        p.tuning = vec![64, 59, 55, 50];
        fill_frets(&mut p);
        let placed = spots(&p).iter().filter(|s| s.0.is_some()).count();
        assert_eq!(placed, 4);
    }

    #[test]
    fn low_parts_read_as_bass() {
        let p = part(vec![note(0, 28)]);
        assert_eq!(p.strings(), BASS.to_vec());
        assert_eq!(part(vec![note(0, 40)]).strings(), GUITAR.to_vec());
    }
}
