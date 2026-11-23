//! Command errors that carry a value ("no song with id 7") in the window's language.
//!
//! Commands build their error texts in English with the values filled in, and the control
//! channel and the CLI get them like that. For the window, [`template_of`] finds the English
//! template a text was made from and takes the values back out, so the window can translate the
//! template and fill the values in again. Every error text a command formats with values has
//! its template in [`TEMPLATES`]; a test checks that against the source.

use crate::MessageArg;

/// The templates of command errors with values, `{name}` for each value.
pub const TEMPLATES: &[&str] = &[
    "ActCraft reads lyrics from {types} files",
    "`click` must be off, whole_song, count_in_only or visual_only, not `{value}`",
    "`kind` must be pen, highlight or note, not `{value}`",
    "`lyrics_scroll` must be line, smooth, constant or off, not `{value}`",
    "`mode` must be play, loop, skip or stop_after, not `{value}`",
    "`{key}` has more than {max} songs",
    "`{key}` has more than {max} items",
    "`{key}` is longer than {max} bytes",
    "`{key}` is required",
    "`{key}` item longer than {max} bytes",
    "`{key}` items must be indexes below {count}",
    "`{key}` items must be text",
    "`{key}` must be a list",
    "`{key}` must be a list of song ids",
    "`{key}` must be a number",
    "`{key}` must be a song id",
    "`{key}` must be a whole number",
    "`{key}` must be between {min} and {max}",
    "`{key}` must be text",
    "`{key}` must be true or false",
    "`{key}` must hold song ids",
    "`{key}` out of range",
    "`{key}`: nothing to choose from",
    "a mark has at most {max} points",
    "a playlist holds at most {max} sets",
    "a set holds at most {max} songs",
    "a show holds at most {max} playlists",
    "a show holds at most {max} songs",
    "a show package file ends in .{extension}",
    "a song holds at most {max} regions",
    "a song holds at most {max} tracks",
    "a song's PDF holds at most {max} marks",
    "cannot sort by `{value}`",
    "no macro named {name}",
    "no song with id {id}",
    "not a show package: {why}",
    "nothing was played on {date}",
    "spoken words are at most {max} bytes",
    "the queue holds at most {max} songs",
    "unknown double-tap mode `{value}`",
    "unknown metronome mode `{value}`",
    "the package entry {entry} could not be read",
];

/// The template `text` was made from and its values, or `None` for a text with no values (or
/// one this list doesn't know, which the window shows as it is).
pub fn template_of(text: &str) -> Option<(&'static str, Vec<MessageArg>)> {
    TEMPLATES.iter().find_map(|t| unfill(t, text).map(|args| (*t, args)))
}

/// The values that turn `template` into `text`, if it does: each placeholder takes the text up
/// to the next literal part. `None` when the text doesn't fit or the template has no values.
fn unfill(template: &'static str, text: &str) -> Option<Vec<MessageArg>> {
    let mut args = Vec::new();
    let mut rest = text;
    let mut tpl = template;
    // The literal part before the first placeholder must match exactly.
    let open = tpl.find('{')?;
    rest = rest.strip_prefix(tpl.get(..open)?)?;
    tpl = tpl.get(open..)?;
    while let Some(close) = tpl.find('}') {
        let name = tpl.get(1..close)?;
        tpl = tpl.get(close + 1..)?;
        let literal_end = tpl.find('{').unwrap_or(tpl.len());
        let literal = tpl.get(..literal_end)?;
        let value_end = if literal.is_empty() { rest.len() } else { rest.find(literal)? };
        let value = rest.get(..value_end)?;
        if value.is_empty() {
            return None;
        }
        args.push(MessageArg { name, value: value.to_owned(), translate: false });
        rest = rest.get(value_end + literal.len()..)?;
        tpl = tpl.get(literal_end..)?;
        if tpl.is_empty() {
            break;
        }
    }
    (rest.is_empty() && tpl.is_empty()).then_some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Option<(&'static str, Vec<(&'static str, String)>)> {
        template_of(text).map(|(t, args)| (t, args.into_iter().map(|a| (a.name, a.value)).collect()))
    }

    #[test]
    fn texts_go_back_to_their_template_and_values() {
        assert_eq!(values("no song with id 7"), Some(("no song with id {id}", vec![("id", "7".to_owned())])));
        assert_eq!(
            values("`start` must be between 0 and 86400"),
            Some((
                "`{key}` must be between {min} and {max}",
                vec![("key", "start".to_owned()), ("min", "0".to_owned()), ("max", "86400".to_owned())]
            ))
        );
        assert_eq!(
            values("the package entry files/0001-Gig 1.wav could not be read"),
            Some(("the package entry {entry} could not be read", vec![("entry", "files/0001-Gig 1.wav".to_owned())]))
        );
        assert_eq!(values("no song chosen"), None, "no values, no template");
        assert_eq!(values("no song with id "), None, "a value is never empty");
        assert_eq!(
            values("no song with id 7 and more"),
            Some(("no song with id {id}", vec![("id", "7 and more".to_owned())]))
        );
    }

    /// Normalises `{anything}` to `{}`, so a source text and its template compare.
    fn shape(s: &str) -> String {
        let mut out = String::new();
        let mut inside = false;
        for c in s.chars() {
            match c {
                '{' => {
                    inside = true;
                    out.push_str("{}");
                }
                '}' => inside = false,
                _ if !inside => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn every_error_text_with_values_has_its_template() {
        let sources = [
            include_str!("commands.rs"),
            include_str!("region_commands.rs"),
            include_str!("package_commands.rs"),
            include_str!("notation_commands.rs"),
            include_str!("pdf_commands.rs"),
            include_str!("midi_file.rs"),
            include_str!("midi_sync.rs"),
            include_str!("remote.rs"),
            include_str!("macros.rs"),
            include_str!("pads.rs"),
            include_str!("osc.rs"),
            include_str!("dmx.rs"),
            include_str!("fixtures.rs"),
            include_str!("fixture_library.rs"),
            include_str!("looks.rs"),
            include_str!("voice.rs"),
            include_str!("lib.rs"),
            include_str!("../../actcraft-command/src/params.rs"),
            include_str!("../../actcraft-command/src/lib.rs"),
        ];
        let known: Vec<String> = TEMPLATES.iter().map(|t| shape(t)).collect();
        let mut missing = Vec::new();
        for src in sources {
            for marker in ["invalid(format!(\"", "failed(format!(\"", "InvalidParams(format!(\"", "Failed(format!(\""] {
                for (at, _) in src.match_indices(marker) {
                    let start = at + marker.len();
                    let Some(text) = src.get(start..).and_then(|s| s.split('"').next()) else { continue };
                    if !known.contains(&shape(text)) {
                        missing.push(text.to_owned());
                    }
                }
            }
        }
        assert!(missing.is_empty(), "add these to error_text::TEMPLATES: {missing:#?}");
        for t in TEMPLATES {
            assert!(t.contains('{'), "{t} has no value; it needs no template");
        }
    }
}
