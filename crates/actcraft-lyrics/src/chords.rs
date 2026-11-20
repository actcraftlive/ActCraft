//! Chord names: transposing and capo.

const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLATS: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// Splits a note name off the front: (pitch class 0-11, rest of the text).
fn note(s: &str) -> Option<(i32, &str)> {
    let mut chars = s.chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' | 'H' => 11,
        _ => return None,
    };
    let rest = s.get(1..).unwrap_or("");
    if let Some(r) = rest.strip_prefix('#') {
        Some(((base + 1) % 12, r))
    } else if let Some(r) = rest.strip_prefix('b') {
        Some(((base + 11) % 12, r))
    } else {
        Some((base, rest))
    }
}

/// Transposes a chord name by `semitones` (e.g. `Am7/G` + 2 = `Bm7/A`). Anything that isn't a
/// chord (`N.C.`, `x2`) comes back unchanged. `flats` picks Db over C#.
pub fn transpose_chord(chord: &str, semitones: i32, flats: bool) -> String {
    if semitones.rem_euclid(12) == 0 {
        return chord.to_owned();
    }
    let names = if flats { &FLATS } else { &SHARPS };
    let shift = |pc: i32| names.get((pc + semitones).rem_euclid(12) as usize).copied().unwrap_or("C");
    let Some((root, rest)) = note(chord) else { return chord.to_owned() };
    match rest.rsplit_once('/') {
        Some((quality, bass)) => match note(bass) {
            Some((b, "")) => format!("{}{quality}/{}", shift(root), shift(b)),
            _ => format!("{}{rest}", shift(root)),
        },
        None => format!("{}{rest}", shift(root)),
    }
}

/// Whether a key or chord reads better with flats (F, Bb, Eb, … and their minors).
pub fn prefers_flats(key: &str) -> bool {
    let Some((pc, rest)) = note(key.trim()) else { return false };
    let minor = rest.starts_with('m') && !rest.starts_with("maj");
    let major_pc = if minor { (pc + 3) % 12 } else { pc };
    matches!(major_pc, 5 | 10 | 3 | 8 | 1) || key.contains('b')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transposes_roots_and_bass_notes() {
        assert_eq!(transpose_chord("Am7/G", 2, false), "Bm7/A");
        assert_eq!(transpose_chord("C", -1, true), "B");
        assert_eq!(transpose_chord("F#m", 1, false), "Gm");
        assert_eq!(transpose_chord("Bb", 2, true), "C");
        assert_eq!(transpose_chord("N.C.", 3, false), "N.C.");
        assert_eq!(transpose_chord("E", 12, false), "E");
        assert!(prefers_flats("Dm") && !prefers_flats("A"));
    }
}
