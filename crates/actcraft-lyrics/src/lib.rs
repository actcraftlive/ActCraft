//! Lyrics: plain text with light markup, chords in ChordPro syntax and timecodes.
//!
//! ```text
//! {title: Night Drive}            ChordPro directives (shown or hidden)
//! # Verse 1                       headings, levels 1-3 (#, ##, ###)
//! [00:12.50]The [Am]road is [F]long   a timecode (LRC style) and chords before the syllable
//! **bold** *italic* __underline__ styles, stackable
//! <color=#e0a030>gold</color>     colours by name or hex; may span lines
//! <font=serif>…</font>            fonts: sans, serif, mono, script
//! {c: Slowly}                     a comment line
//! ```
//!
//! Parsing never fails and is bounded: lines, line length and open tags are capped.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod chords;

pub use chords::transpose_chord;

/// Lines parsed from one song's lyrics.
pub const MAX_LINES: usize = 20_000;
/// Characters of one line that are parsed (the rest is cut).
pub const MAX_LINE_CHARS: usize = 4_000;
/// Nested colour or font tags kept open.
pub const MAX_OPEN_TAGS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Font {
    #[default]
    Sans,
    Serif,
    Mono,
    Script,
}

/// How a piece of text looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub font: Font,
    pub colour: Option<[u8; 3]>,
}

/// Text with the chord sung on its first syllable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub chord: Option<String>,
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Text,
    Blank,
    /// Heading level 1-3.
    Heading(u8),
    /// `{c: …}` / `{comment: …}`.
    Comment,
    /// Any other directive, `{name: value}`.
    Directive {
        name: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    /// Timecode in seconds from a leading `[mm:ss.xx]`.
    pub time: Option<f64>,
    pub spans: Vec<Span>,
    /// Inside `{start_of_chorus}` … `{end_of_chorus}`.
    pub chorus: bool,
}

impl Line {
    /// The line's words without chords or markup.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn has_chords(&self) -> bool {
        self.spans.iter().any(|s| s.chord.is_some())
    }
}

/// Parsed lyrics.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lyrics {
    pub lines: Vec<Line>,
}

impl Lyrics {
    /// Index of the timed line playing at `seconds`: the last line whose timecode has passed.
    pub fn line_at(&self, seconds: f64) -> Option<usize> {
        let mut best = None;
        for (i, l) in self.lines.iter().enumerate() {
            if let Some(t) = l.time
                && t <= seconds
            {
                best = Some(i);
            }
        }
        best
    }

    /// Value of the first directive with this name (`title`, `artist`, `key`, `capo`, `tempo`…).
    pub fn directive(&self, name: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match &l.kind {
            LineKind::Directive { name: n, value } if n == name => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn has_timecodes(&self) -> bool {
        self.lines.iter().any(|l| l.time.is_some())
    }
}

/// Turns foreign line endings (`\r\n`, `\r`) into `\n` and drops a byte-order mark.
pub fn normalise_line_endings(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n")
}

/// Decodes a lyrics file: UTF-8 (lossy), or UTF-16 with a byte-order mark; line endings normalised.
pub fn decode_text(bytes: &[u8]) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&pair| u16::from_be_bytes(pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    normalise_line_endings(&text)
}

/// `[mm:ss]`, `[mm:ss.xx]` or `[h:mm:ss.xx]` to seconds.
pub fn parse_timecode(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || p.len() > 6) {
        return None;
    }
    let num = |p: &str| -> Option<f64> {
        if p.chars().all(|c| c.is_ascii_digit() || c == '.') { p.parse::<f64>().ok() } else { None }
    };
    let mut total = 0.0;
    for p in &parts {
        total = total * 60.0 + num(p)?;
    }
    (total.is_finite() && total < 86_400.0).then_some(total)
}

/// `seconds` as `[mm:ss.xx]`.
pub fn format_timecode(seconds: f64) -> String {
    let s = if seconds.is_finite() { seconds.clamp(0.0, 86_399.0) } else { 0.0 };
    let centis = (s * 100.0).round() as u64;
    format!("[{:02}:{:02}.{:02}]", centis / 6000, (centis / 100) % 60, centis % 100)
}

/// Sets (or replaces) the timecode at the start of line `index` in the source text. Returns the
/// new text, or `None` when there is no such line.
pub fn stamp_line(source: &str, index: usize, seconds: f64) -> Option<String> {
    let mut lines: Vec<String> = source.split('\n').map(str::to_owned).collect();
    let line = lines.get_mut(index)?;
    let rest = strip_timecode(line).to_owned();
    *line = format!("{}{rest}", format_timecode(seconds));
    Some(lines.join("\n"))
}

/// Most matches [`find_all`] reports (and [`replace_all`] replaces) in one call.
pub const MAX_MATCHES: usize = 10_000;

fn same_char(a: char, b: char, match_case: bool) -> bool {
    a == b || (!match_case && a.to_lowercase().eq(b.to_lowercase()))
}

/// Every place `needle` occurs in `text`, as character ranges `(start, end)`, left to right and
/// not overlapping; at most [`MAX_MATCHES`]. An empty needle finds nothing.
pub fn find_all(text: &str, needle: &str, match_case: bool) -> Vec<(usize, usize)> {
    let hay: Vec<char> = text.chars().collect();
    let pin: Vec<char> = needle.chars().collect();
    let mut out = Vec::new();
    if pin.is_empty() || pin.len() > hay.len() {
        return out;
    }
    let mut i = 0;
    while i + pin.len() <= hay.len() && out.len() < MAX_MATCHES {
        let hit =
            hay.get(i..i + pin.len()).is_some_and(|w| w.iter().zip(&pin).all(|(&a, &b)| same_char(a, b, match_case)));
        if hit {
            out.push((i, i + pin.len()));
            i += pin.len();
        } else {
            i += 1;
        }
    }
    out
}

/// `text` with every occurrence of `needle` replaced by `replacement`, and how many were replaced
/// (at most [`MAX_MATCHES`]; later ones stay).
pub fn replace_all(text: &str, needle: &str, replacement: &str, match_case: bool) -> (String, usize) {
    let found = find_all(text, needle, match_case);
    if found.is_empty() {
        return (text.to_owned(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for &(start, end) in &found {
        out.extend(chars.get(at..start).unwrap_or(&[]));
        out.push_str(replacement);
        at = end;
    }
    out.extend(chars.get(at..).unwrap_or(&[]));
    (out, found.len())
}

/// Removes a leading timecode from a line, if any.
fn strip_timecode(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(end) = rest.find(']')
        && rest.get(..end).and_then(parse_timecode).is_some()
    {
        return rest.get(end + 1..).unwrap_or("");
    }
    line
}

fn colour_by_name(name: &str) -> Option<[u8; 3]> {
    let n = name.trim().to_ascii_lowercase();
    if let Some(hex) = n.strip_prefix('#') {
        if hex.len() == 6 && hex.is_ascii() {
            let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
            return Some([byte(0)?, byte(2)?, byte(4)?]);
        }
        return None;
    }
    Some(match n.as_str() {
        "red" => [0xe5, 0x48, 0x48],
        "orange" => [0xf0, 0x8c, 0x30],
        "yellow" => [0xf2, 0xd0, 0x3c],
        "green" => [0x3d, 0xc9, 0x7a],
        "blue" => [0x4f, 0x9c, 0xf0],
        "purple" | "violet" => [0xa7, 0x7b, 0xe8],
        "pink" => [0xf0, 0x78, 0xb4],
        "grey" | "gray" => [0xa4, 0xab, 0xb8],
        "white" => [0xff, 0xff, 0xff],
        "cyan" => [0x3c, 0xd2, 0xe0],
        _ => return None,
    })
}

fn font_by_name(name: &str) -> Option<Font> {
    match name.trim().to_ascii_lowercase().as_str() {
        "sans" => Some(Font::Sans),
        "serif" => Some(Font::Serif),
        "mono" | "monospace" => Some(Font::Mono),
        "script" => Some(Font::Script),
        _ => None,
    }
}

/// Open colour and font tags carried from line to line.
#[derive(Debug, Default)]
struct OpenTags {
    colours: Vec<[u8; 3]>,
    fonts: Vec<Font>,
}

fn directive_of(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (name, value) = match inner.split_once(':') {
        Some((n, v)) => (n.trim(), v.trim()),
        None => (inner.trim(), ""),
    };
    if name.is_empty() || name.len() > 40 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    Some((name.to_ascii_lowercase(), value.to_owned()))
}

/// Parses lyrics source text. Never fails.
pub fn parse(source: &str) -> Lyrics {
    let text = normalise_line_endings(source);
    let mut out = Lyrics::default();
    let mut tags = OpenTags::default();
    let mut chorus = false;
    let mut inline = Style::default();
    for raw in text.split('\n').take(MAX_LINES) {
        let raw: String = raw.chars().take(MAX_LINE_CHARS).collect();
        // Bold/italic/underline don't carry across lines (a stray marker can't ruin the rest).
        inline.bold = false;
        inline.italic = false;
        inline.underline = false;
        if let Some((name, value)) = directive_of(&raw) {
            let kind = match name.as_str() {
                "c" | "comment" | "ci" | "comment_italic" | "cb" | "comment_box" => LineKind::Comment,
                "soc" | "start_of_chorus" => {
                    chorus = true;
                    continue;
                }
                "eoc" | "end_of_chorus" => {
                    chorus = false;
                    continue;
                }
                "t" => LineKind::Directive { name: "title".into(), value: value.clone() },
                "st" => LineKind::Directive { name: "subtitle".into(), value: value.clone() },
                _ => LineKind::Directive { name, value: value.clone() },
            };
            let spans =
                if kind == LineKind::Comment { vec![Span { text: value, ..Span::default() }] } else { Vec::new() };
            out.lines.push(Line { kind, time: None, spans, chorus });
            continue;
        }
        let (time, body) = match raw.strip_prefix('[').and_then(|r| r.find(']').map(|e| (r, e))) {
            Some((r, end)) => match r.get(..end).and_then(parse_timecode) {
                Some(t) => (Some(t), r.get(end + 1..).unwrap_or("").to_owned()),
                None => (None, raw.clone()),
            },
            None => (None, raw.clone()),
        };
        let trimmed = body.trim_start();
        let (kind, content) = if let Some(rest) = trimmed.strip_prefix("### ") {
            (LineKind::Heading(3), rest.to_owned())
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            (LineKind::Heading(2), rest.to_owned())
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            (LineKind::Heading(1), rest.to_owned())
        } else if trimmed.is_empty() {
            (LineKind::Blank, String::new())
        } else {
            (LineKind::Text, body.clone())
        };
        let spans = parse_spans(&content, &mut tags, &mut inline);
        out.lines.push(Line { kind, time, spans, chorus });
    }
    out
}

/// Splits one line into spans at chords and style changes.
fn parse_spans(content: &str, tags: &mut OpenTags, inline: &mut Style) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut current = Span { style: current_style(tags, inline), ..Span::default() };
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let flush = |spans: &mut Vec<Span>, current: &mut Span, style: Style| {
        if !current.text.is_empty() || current.chord.is_some() {
            spans.push(std::mem::take(current));
        }
        current.style = style;
    };
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or(' ');
        let next = chars.get(i + 1).copied();
        // Chord: [Am7]
        if c == '['
            && let Some(len) = chars.get(i + 1..).and_then(|rest| rest.iter().position(|&ch| ch == ']'))
            && len > 0
            && len <= 24
        {
            let chord: String = chars.get(i + 1..i + 1 + len).map(|s| s.iter().collect()).unwrap_or_default();
            flush(&mut spans, &mut current, current_style(tags, inline));
            current.chord = Some(chord.trim().to_owned());
            i += len + 2;
            continue;
        }
        // Styles: ** bold, __ underline, * italic.
        if c == '*' && next == Some('*') {
            inline.bold = !inline.bold;
            flush(&mut spans, &mut current, current_style(tags, inline));
            i += 2;
            continue;
        }
        if c == '_' && next == Some('_') {
            inline.underline = !inline.underline;
            flush(&mut spans, &mut current, current_style(tags, inline));
            i += 2;
            continue;
        }
        if c == '*' {
            inline.italic = !inline.italic;
            flush(&mut spans, &mut current, current_style(tags, inline));
            i += 1;
            continue;
        }
        // Tags: <color=…>, </color>, <font=…>, </font>
        if c == '<'
            && let Some(len) = chars.get(i + 1..).and_then(|rest| rest.iter().take(40).position(|&ch| ch == '>'))
        {
            let tag: String = chars.get(i + 1..i + 1 + len).map(|s| s.iter().collect()).unwrap_or_default();
            let lower = tag.to_ascii_lowercase();
            let mut handled = true;
            if let Some(v) = lower.strip_prefix("color=").or_else(|| lower.strip_prefix("colour=")) {
                match colour_by_name(v) {
                    Some(col) if tags.colours.len() < MAX_OPEN_TAGS => tags.colours.push(col),
                    _ => handled = false,
                }
            } else if lower == "/color" || lower == "/colour" {
                tags.colours.pop();
            } else if let Some(v) = lower.strip_prefix("font=") {
                match font_by_name(v) {
                    Some(f) if tags.fonts.len() < MAX_OPEN_TAGS => tags.fonts.push(f),
                    _ => handled = false,
                }
            } else if lower == "/font" {
                tags.fonts.pop();
            } else {
                handled = false;
            }
            if handled {
                flush(&mut spans, &mut current, current_style(tags, inline));
                i += len + 2;
                continue;
            }
        }
        current.text.push(c);
        i += 1;
    }
    if !current.text.is_empty() || current.chord.is_some() {
        spans.push(current);
    }
    spans
}

fn current_style(tags: &OpenTags, inline: &Style) -> Style {
    Style {
        bold: inline.bold,
        italic: inline.italic,
        underline: inline.underline,
        font: tags.fonts.last().copied().unwrap_or_default(),
        colour: tags.colours.last().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_timecodes_headings_and_styles() {
        let src = "{title: Night Drive}\r\n# Verse\r\n[00:12.50]The [Am]road is [F]long\n**loud** *soft* __under__\n{c: slowly}\n{soc}\nLa la\n{eoc}\n";
        let l = parse(src);
        assert_eq!(l.directive("title"), Some("Night Drive"));
        assert_eq!(l.lines.get(1).map(|x| x.kind.clone()), Some(LineKind::Heading(1)));
        let line = l.lines.get(2).cloned().unwrap_or_else(|| Line {
            kind: LineKind::Blank,
            time: None,
            spans: vec![],
            chorus: false,
        });
        assert_eq!(line.time, Some(12.5));
        assert_eq!(line.plain_text(), "The road is long");
        let chords: Vec<_> = line.spans.iter().filter_map(|s| s.chord.clone()).collect();
        assert_eq!(chords, vec!["Am", "F"]);
        let styles = l
            .lines
            .get(3)
            .map(|x| x.spans.iter().map(|s| (s.style.bold, s.style.italic, s.style.underline)).collect::<Vec<_>>());
        assert!(styles.is_some_and(|s| s.contains(&(true, false, false))
            && s.contains(&(false, true, false))
            && s.contains(&(false, false, true))));
        assert_eq!(l.lines.get(4).map(|x| x.kind.clone()), Some(LineKind::Comment));
        assert!(l.lines.get(5).is_some_and(|x| x.chorus && x.plain_text() == "La la"));
        assert_eq!(l.line_at(13.0), Some(2));
    }

    #[test]
    fn colours_span_lines_and_unknown_tags_stay_text() {
        let l = parse("<color=gold>x</color>\n<color=#e0a030>one\ntwo</color> three\n<b>");
        assert!(l.lines.first().is_some_and(|x| x.plain_text() == "<color=gold>x"));
        assert!(
            l.lines.get(2).and_then(|x| x.spans.first()).is_some_and(|s| s.style.colour == Some([0xe0, 0xa0, 0x30]))
        );
        assert!(l.lines.get(2).and_then(|x| x.spans.last()).is_some_and(|s| s.style.colour.is_none()));
        assert!(l.lines.get(3).is_some_and(|x| x.plain_text() == "<b>"));
    }

    #[test]
    fn stamping_replaces_or_adds_timecodes() {
        let src = "one\n[00:01.00]two";
        assert_eq!(stamp_line(src, 0, 3.25).as_deref(), Some("[00:03.25]one\n[00:01.00]two"));
        assert_eq!(stamp_line(src, 1, 61.0).as_deref(), Some("one\n[01:01.00]two"));
        assert_eq!(stamp_line(src, 5, 1.0), None);
        assert_eq!(decode_text(b"\xFF\xFEa\0\r\0\n\0b\0"), "a\nb");
    }

    #[test]
    fn find_and_replace_by_characters_with_or_without_case() {
        let text = "Über die Straße, über den Fluss";
        assert_eq!(find_all(text, "über", false), vec![(0, 4), (17, 21)]);
        assert_eq!(find_all(text, "über", true), vec![(17, 21)]);
        assert!(find_all(text, "", false).is_empty());
        assert_eq!(find_all("aaaa", "aa", true), vec![(0, 2), (2, 4)]);
        let (out, n) = replace_all("The [Am]road, the road", "road", "way", false);
        assert_eq!((out.as_str(), n), ("The [Am]way, the way", 2));
        let (out, n) = replace_all("abc", "x", "y", false);
        assert_eq!((out.as_str(), n), ("abc", 0));
        // The replacement is never searched again.
        let (out, n) = replace_all("a a", "a", "aa", true);
        assert_eq!((out.as_str(), n), ("aa aa", 2));
    }
}
