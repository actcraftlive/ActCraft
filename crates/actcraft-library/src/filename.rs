//! Song details from file names, with a user format string such as `%artist% - %title%`, and
//! duplicate detection.

use actcraft_show::Song;

/// Placeholders a format string may use.
pub const PLACEHOLDERS: [&str; 6] = ["title", "artist", "key", "bpm", "year", "ignore"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Lit(String),
    Field(String),
}

fn compile(format: &str) -> Result<Vec<Part>, String> {
    if format.len() > 256 {
        return Err("format longer than 256 bytes".to_owned());
    }
    let mut parts = Vec::new();
    let mut rest = format;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('%') {
            let Some(end) = after.find('%') else { return Err("a % has no closing %".to_owned()) };
            let name = after.get(..end).unwrap_or_default().to_ascii_lowercase();
            if !PLACEHOLDERS.contains(&name.as_str()) {
                return Err(format!("unknown placeholder %{name}%"));
            }
            if matches!(parts.last(), Some(Part::Field(_))) {
                return Err("two placeholders need text between them".to_owned());
            }
            parts.push(Part::Field(name));
            rest = after.get(end + 1..).unwrap_or_default();
        } else {
            let end = rest.find('%').unwrap_or(rest.len());
            parts.push(Part::Lit(rest.get(..end).unwrap_or_default().to_owned()));
            rest = rest.get(end..).unwrap_or_default();
        }
    }
    Ok(parts)
}

/// Fills a song's details from a file name (without folder; the extension is dropped). Fields that
/// don't fit the format stay empty; when nothing fits, the whole name becomes the title.
pub fn parse(file_name: &str, format: &str) -> Result<Song, String> {
    let parts = compile(format)?;
    let stem = match file_name.rfind('.') {
        Some(i) if i > 0 => file_name.get(..i).unwrap_or(file_name),
        _ => file_name,
    };
    let mut song = Song::default();
    let mut rest = stem;
    let mut ok = true;
    let mut i = 0;
    while i < parts.len() {
        match parts.get(i) {
            Some(Part::Lit(l)) => {
                if let Some(r) = rest.strip_prefix(l.as_str()) {
                    rest = r;
                } else {
                    ok = false;
                    break;
                }
            }
            Some(Part::Field(name)) => {
                // A field runs to the next literal (first occurrence) or to the end.
                let value = match parts.get(i + 1) {
                    Some(Part::Lit(next)) if !next.is_empty() => match rest.find(next.as_str()) {
                        Some(at) => {
                            let v = rest.get(..at).unwrap_or_default();
                            rest = rest.get(at..).unwrap_or_default();
                            v
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    },
                    _ => {
                        let v = rest;
                        rest = "";
                        v
                    }
                };
                apply(&mut song, name, value.trim());
            }
            None => break,
        }
        i += 1;
    }
    if !ok || song.title.is_empty() {
        song.title = stem.trim().to_owned();
    }
    Ok(song)
}

fn apply(song: &mut Song, field: &str, value: &str) {
    match field {
        "title" => song.title = value.to_owned(),
        "artist" => song.artist = value.to_owned(),
        "key" => song.key = value.to_owned(),
        "bpm" => {
            if let Ok(b) = value.parse::<f64>()
                && b.is_finite()
                && (20.0..=400.0).contains(&b)
            {
                song.bpm = b;
            }
        }
        "year" => song.year = value.parse::<i32>().ok().filter(|y| (0..=9999).contains(y)),
        _ => {}
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether two songs look like the same song: same title and artist ignoring case and spacing, or
/// the same first audio file.
pub fn is_duplicate(a: &Song, b: &Song) -> bool {
    let same_names =
        !a.title.trim().is_empty() && norm(&a.title) == norm(&b.title) && norm(&a.artist) == norm(&b.artist);
    let same_file = match (a.tracks.first(), b.tracks.first()) {
        (Some(x), Some(y)) => !x.file.is_empty() && x.file == y.file,
        _ => false,
    };
    same_names || same_file
}

/// Keys that make two songs duplicates: their normalised title and artist, and their first audio
/// file. Matching keys means [`is_duplicate`] is true.
pub fn duplicate_keys(s: &Song) -> (Option<String>, Option<String>) {
    let names =
        if s.title.trim().is_empty() { None } else { Some(format!("{}\u{1f}{}", norm(&s.title), norm(&s.artist))) };
    let file = s.tracks.first().map(|t| t.file.clone()).filter(|f| !f.is_empty());
    (names, file)
}

/// Every pair of duplicate songs (first id, later id), found by key lookup in O(n log n), at
/// most `max_pairs` pairs.
pub fn find_duplicates(songs: &[Song], max_pairs: usize) -> Vec<(u64, u64)> {
    use std::collections::BTreeMap;
    let mut by_names: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut by_file: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for s in songs {
        let (names, file) = duplicate_keys(s);
        if let Some(k) = names {
            by_names.entry(k).or_default().push(s.id);
        }
        if let Some(k) = file {
            by_file.entry(k).or_default().push(s.id);
        }
    }
    let mut pairs = std::collections::BTreeSet::new();
    for group in by_names.values().chain(by_file.values()) {
        for (i, a) in group.iter().enumerate() {
            for b in group.iter().skip(i + 1) {
                if pairs.len() >= max_pairs {
                    return pairs.into_iter().collect();
                }
                pairs.insert((*a.min(b), *a.max(b)));
            }
        }
    }
    pairs.into_iter().collect()
}

/// Sorts file names the way a person reads them: case-insensitive, then exact.
pub fn sort_alphabetical(names: &mut [String]) {
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_format() {
        let s = parse("Ana Ruiz - Night Drive [Am] 128.wav", "%artist% - %title% [%key%] %bpm%").unwrap_or_default();
        assert_eq!((s.artist.as_str(), s.title.as_str(), s.key.as_str()), ("Ana Ruiz", "Night Drive", "Am"));
        assert!((s.bpm - 128.0).abs() < f64::EPSILON);
        let fallback = parse("whatever.mp3", "%artist% - %title%").unwrap_or_default();
        assert_eq!(fallback.title, "whatever");
        assert!(parse("x", "%nope%").is_err());
        assert!(parse("x", "%title%%artist%").is_err());
    }

    #[test]
    fn detects_duplicates() {
        let a = Song { title: "Night  Drive".into(), artist: "ANA".into(), ..Song::default() };
        let b = Song { title: "night drive".into(), artist: "ana".into(), ..Song::default() };
        assert!(is_duplicate(&a, &b));
        let songs =
            vec![Song { id: 1, ..a }, Song { id: 2, title: "Other".into(), ..Song::default() }, Song { id: 3, ..b }];
        assert_eq!(find_duplicates(&songs, 10), vec![(1, 3)]);
    }
}
