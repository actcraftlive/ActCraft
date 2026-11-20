//! Library search.
//!
//! Grammar, loosest binding first:
//!
//! ```text
//! query   := or
//! or      := and ( "OR" and )*
//! and     := not ( ["AND"] not )*        adjacent terms mean AND
//! not     := ( "NOT" | "-" ) not | atom
//! atom    := "(" or ")" | [field ":"] ( word | "quoted phrase" )
//! field   := title | artist | key | keyword | bpm | year
//! ```
//!
//! Free terms match title, artist, key and keywords, ignoring case. `keyword:` matches a whole
//! keyword with case (keywords are case-distinct). `bpm:` and `year:` take a number, optionally
//! after `>`, `<`, `>=` or `<=`. Queries are bounded: at most [`MAX_TOKENS`] tokens and
//! [`MAX_DEPTH`] nested parentheses; anything past that is a parse error, never a stack overflow.

use actcraft_show::Song;

pub const MAX_TOKENS: usize = 128;
pub const MAX_DEPTH: usize = 16;
pub const MAX_QUERY_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Any,
    Title,
    Artist,
    Key,
    Keyword,
    Bpm,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// Matches everything (empty query).
    All,
    Text {
        field: Field,
        text: String,
    },
    Number {
        field: Field,
        cmp: Cmp,
        value: f64,
    },
    Not(Box<Query>),
    And(Vec<Query>),
    Or(Vec<Query>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Phrase(String),
    Field(Field),
    Minus,
    Not,
    And,
    Or,
    Open,
    Close,
}

fn field_name(s: &str) -> Option<Field> {
    match s.to_ascii_lowercase().as_str() {
        "title" => Some(Field::Title),
        "artist" => Some(Field::Artist),
        "key" => Some(Field::Key),
        "keyword" | "keywords" | "tag" => Some(Field::Keyword),
        "bpm" | "tempo" => Some(Field::Bpm),
        "year" => Some(Field::Year),
        _ => None,
    }
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    if src.len() > MAX_QUERY_BYTES {
        return Err(format!("query longer than {MAX_QUERY_BYTES} bytes"));
    }
    let mut toks = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        if toks.len() > MAX_TOKENS {
            return Err(format!("query has more than {MAX_TOKENS} parts"));
        }
        if c.is_whitespace() {
            chars.next();
        } else if c == '(' {
            chars.next();
            toks.push(Tok::Open);
        } else if c == ')' {
            chars.next();
            toks.push(Tok::Close);
        } else if c == '-' && toks.last().is_none_or(|t| !matches!(t, Tok::Field(_))) {
            chars.next();
            toks.push(Tok::Minus);
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            for ch in chars.by_ref() {
                if ch == '"' {
                    break;
                }
                s.push(ch);
            }
            toks.push(Tok::Phrase(s));
        } else {
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
                    break;
                }
                s.push(ch);
                chars.next();
                if ch == ':' {
                    break;
                }
            }
            if let Some(name) = s.strip_suffix(':')
                && let Some(f) = field_name(name)
            {
                toks.push(Tok::Field(f));
                continue;
            }
            match s.as_str() {
                "AND" => toks.push(Tok::And),
                "OR" => toks.push(Tok::Or),
                "NOT" => toks.push(Tok::Not),
                _ => toks.push(Tok::Word(s)),
            }
        }
    }
    Ok(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn or(&mut self, depth: usize) -> Result<Query, String> {
        let mut parts = vec![self.and(depth)?];
        while self.peek() == Some(&Tok::Or) {
            self.next();
            parts.push(self.and(depth)?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap_or(Query::All) } else { Query::Or(parts) })
    }

    fn and(&mut self, depth: usize) -> Result<Query, String> {
        let mut parts = vec![self.not(depth)?];
        loop {
            match self.peek() {
                Some(Tok::And) => {
                    self.next();
                    parts.push(self.not(depth)?);
                }
                Some(Tok::Or | Tok::Close) | None => break,
                Some(_) => parts.push(self.not(depth)?),
            }
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap_or(Query::All) } else { Query::And(parts) })
    }

    fn not(&mut self, depth: usize) -> Result<Query, String> {
        if depth > MAX_DEPTH {
            return Err("query is nested too deeply".to_owned());
        }
        match self.peek() {
            Some(Tok::Not | Tok::Minus) => {
                self.next();
                Ok(Query::Not(Box::new(self.not(depth + 1)?)))
            }
            _ => self.atom(depth),
        }
    }

    fn atom(&mut self, depth: usize) -> Result<Query, String> {
        match self.next() {
            Some(Tok::Open) => {
                if depth >= MAX_DEPTH {
                    return Err("query is nested too deeply".to_owned());
                }
                let q = self.or(depth + 1)?;
                if self.next() != Some(Tok::Close) {
                    return Err("missing )".to_owned());
                }
                Ok(q)
            }
            Some(Tok::Field(field)) => {
                let text = match self.next() {
                    Some(Tok::Word(w) | Tok::Phrase(w)) => w,
                    _ => return Err("a field needs a value after the colon".to_owned()),
                };
                if matches!(field, Field::Bpm | Field::Year) {
                    number(field, &text)
                } else {
                    Ok(Query::Text { field, text })
                }
            }
            Some(Tok::Word(text) | Tok::Phrase(text)) => Ok(Query::Text { field: Field::Any, text }),
            Some(Tok::Close) => Err("unexpected )".to_owned()),
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("query ends too early".to_owned()),
        }
    }
}

fn number(field: Field, text: &str) -> Result<Query, String> {
    let (cmp, rest) = if let Some(r) = text.strip_prefix(">=") {
        (Cmp::Ge, r)
    } else if let Some(r) = text.strip_prefix("<=") {
        (Cmp::Le, r)
    } else if let Some(r) = text.strip_prefix('>') {
        (Cmp::Gt, r)
    } else if let Some(r) = text.strip_prefix('<') {
        (Cmp::Lt, r)
    } else {
        (Cmp::Eq, text)
    };
    let value: f64 = rest.parse().map_err(|_| format!("`{rest}` is not a number"))?;
    if !value.is_finite() {
        return Err(format!("`{rest}` is not a number"));
    }
    Ok(Query::Number { field, cmp, value })
}

impl Query {
    /// Parses a query. An empty query matches every song.
    pub fn parse(src: &str) -> Result<Query, String> {
        let toks = tokenize(src)?;
        if toks.is_empty() {
            return Ok(Query::All);
        }
        let mut p = Parser { toks, pos: 0 };
        let q = p.or(0)?;
        if p.pos < p.toks.len() {
            return Err("unexpected text after the query".to_owned());
        }
        Ok(q)
    }

    /// Whether a song matches.
    pub fn matches(&self, song: &Song) -> bool {
        match self {
            Query::All => true,
            Query::Text { field, text } => text_matches(*field, text, song),
            Query::Number { field, cmp, value } => {
                let actual = match field {
                    Field::Bpm => song.bpm,
                    Field::Year => match song.year {
                        Some(y) => f64::from(y),
                        None => return false,
                    },
                    _ => return false,
                };
                match cmp {
                    Cmp::Eq => (actual - value).abs() < 0.5,
                    Cmp::Lt => actual < *value,
                    Cmp::Le => actual <= *value,
                    Cmp::Gt => actual > *value,
                    Cmp::Ge => actual >= *value,
                }
            }
            Query::Not(q) => !q.matches(song),
            Query::And(qs) => qs.iter().all(|q| q.matches(song)),
            Query::Or(qs) => qs.iter().any(|q| q.matches(song)),
        }
    }
}

fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    hay.to_lowercase().contains(needle_lower)
}

fn text_matches(field: Field, text: &str, song: &Song) -> bool {
    let lower = text.to_lowercase();
    match field {
        Field::Title => contains_ci(&song.title, &lower),
        Field::Artist => contains_ci(&song.artist, &lower),
        Field::Key => song.key.eq_ignore_ascii_case(text),
        Field::Keyword => song.keywords.iter().any(|k| k == text),
        Field::Any => {
            contains_ci(&song.title, &lower)
                || contains_ci(&song.artist, &lower)
                || song.key.eq_ignore_ascii_case(text)
                || song.keywords.iter().any(|k| contains_ci(k, &lower))
        }
        Field::Bpm | Field::Year => false,
    }
}

/// Ids of songs matching `query`, in library order.
pub fn search<'a>(songs: impl IntoIterator<Item = &'a Song>, query: &Query) -> Vec<u64> {
    songs.into_iter().filter(|s| query.matches(s)).map(|s| s.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u64, title: &str, artist: &str, kw: &[&str], bpm: f64) -> Song {
        Song {
            id,
            title: title.into(),
            artist: artist.into(),
            keywords: kw.iter().map(|k| (*k).to_owned()).collect(),
            bpm,
            ..Song::default()
        }
    }

    fn ids(q: &str, songs: &[Song]) -> Vec<u64> {
        Query::parse(q).map(|q| search(songs, &q)).unwrap_or_else(|_| vec![999])
    }

    #[test]
    fn boolean_fields_phrases_and_keywords() {
        let songs = vec![
            song(1, "Night Drive", "The Hollows", &["Live"], 128.0),
            song(2, "Morning Light", "Ana Ruiz", &["live"], 92.0),
            song(3, "Drive Home", "Ana Ruiz", &[], 140.0),
        ];
        assert_eq!(ids("drive", &songs), vec![1, 3]);
        assert_eq!(ids("drive -home", &songs), vec![1]);
        assert_eq!(ids("drive NOT home", &songs), vec![1]);
        assert_eq!(ids("artist:\"ana ruiz\" AND title:drive", &songs), vec![3]);
        assert_eq!(ids("night OR morning", &songs), vec![1, 2]);
        assert_eq!(ids("keyword:Live", &songs), vec![1]);
        assert_eq!(ids("keyword:live", &songs), vec![2]);
        assert_eq!(ids("bpm:>100 (ana OR hollows)", &songs), vec![1, 3]);
        assert_eq!(ids("\"night drive\"", &songs), vec![1]);
        assert_eq!(ids("", &songs), vec![1, 2, 3]);
    }

    #[test]
    fn hostile_queries_error_not_overflow() {
        let deep = "(".repeat(10_000);
        assert!(Query::parse(&deep).is_err());
        let nots = "NOT ".repeat(1000);
        assert!(Query::parse(&nots).is_err());
        assert!(Query::parse("title:").is_err());
        assert!(Query::parse("bpm:abc").is_err());
    }
}
