//! Smart playlists: songs chosen by rules, matching all or any of them.

use actcraft_show::{SmartRule, SmartRules, Song};

/// Fields a rule may test.
pub const FIELDS: [&str; 7] = ["title", "artist", "key", "keyword", "bpm", "year", "duration"];
/// Operators a rule may use.
pub const OPS: [&str; 7] = ["contains", "is", "not", ">", "<", ">=", "<="];

/// Checks a rule's field and operator. Values are checked when the rule is evaluated.
pub fn validate(rule: &SmartRule) -> Result<(), String> {
    if !FIELDS.contains(&rule.field.as_str()) {
        return Err(format!("unknown field `{}`", rule.field));
    }
    if !OPS.contains(&rule.op.as_str()) {
        return Err(format!("unknown operator `{}`", rule.op));
    }
    Ok(())
}

fn number_of(song: &Song, field: &str) -> Option<f64> {
    match field {
        "bpm" => Some(song.bpm),
        "year" => song.year.map(f64::from),
        "duration" => Some(song.duration),
        _ => None,
    }
}

fn text_of<'a>(song: &'a Song, field: &str) -> Option<&'a str> {
    match field {
        "title" => Some(&song.title),
        "artist" => Some(&song.artist),
        "key" => Some(&song.key),
        _ => None,
    }
}

/// Whether one rule matches a song. An unknown field or operator never matches.
pub fn rule_matches(rule: &SmartRule, song: &Song) -> bool {
    if rule.field == "keyword" {
        // Keywords are case-distinct.
        let has = song.keywords.iter().any(|k| k == &rule.value);
        let contains = song.keywords.iter().any(|k| k.contains(rule.value.as_str()));
        return match rule.op.as_str() {
            "is" => has,
            "not" => !has,
            "contains" => contains,
            _ => false,
        };
    }
    if let Some(actual) = number_of(song, &rule.field) {
        let Ok(want) = rule.value.trim().parse::<f64>() else { return false };
        if !want.is_finite() {
            return false;
        }
        return match rule.op.as_str() {
            "is" => (actual - want).abs() < 0.5,
            "not" => (actual - want).abs() >= 0.5,
            ">" => actual > want,
            "<" => actual < want,
            ">=" => actual >= want,
            "<=" => actual <= want,
            _ => false,
        };
    }
    let Some(text) = text_of(song, &rule.field) else { return false };
    let (t, v) = (text.to_lowercase(), rule.value.to_lowercase());
    match rule.op.as_str() {
        "contains" => t.contains(&v),
        "is" => t == v,
        "not" => t != v,
        _ => false,
    }
}

/// Ids of songs the rules select, in library order. No rules selects nothing.
pub fn evaluate<'a>(rules: &SmartRules, songs: impl IntoIterator<Item = &'a Song>) -> Vec<u64> {
    if rules.rules.is_empty() {
        return Vec::new();
    }
    songs
        .into_iter()
        .filter(|s| {
            if rules.match_all {
                rules.rules.iter().all(|r| rule_matches(r, s))
            } else {
                rules.rules.iter().any(|r| rule_matches(r, s))
            }
        })
        .map(|s| s.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(f: &str, o: &str, v: &str) -> SmartRule {
        SmartRule { field: f.into(), op: o.into(), value: v.into() }
    }

    #[test]
    fn match_all_versus_any() {
        let songs = [
            Song { id: 1, bpm: 130.0, keywords: vec!["Opener".into()], ..Song::default() },
            Song { id: 2, bpm: 90.0, keywords: vec!["Opener".into()], ..Song::default() },
            Song { id: 3, bpm: 140.0, ..Song::default() },
        ];
        let rules = vec![rule("bpm", ">", "120"), rule("keyword", "is", "Opener")];
        assert_eq!(evaluate(&SmartRules { match_all: true, rules: rules.clone() }, &songs), vec![1]);
        assert_eq!(evaluate(&SmartRules { match_all: false, rules }, &songs), vec![1, 2, 3]);
    }
}
