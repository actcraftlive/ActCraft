//! Notation commands: a song's score (MusicXML, from notation and tablature editors) shown as notes
//! and tablature in the Sheet music tab.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_notation::{Error as NotationError, Score};
use actcraft_show::limits::MAX_TEXT;
use serde_json::{Value, json};

use crate::{Engine, MessageArg, services};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// The `song` param, or the current song.
fn song_id(e: &Engine, prm: &Value) -> Result<u64, CommandError> {
    if p::has(prm, "song") {
        let id = u64::try_from(p::i64_in(prm, "song", 1, i64::MAX)?).map_err(|_| invalid("bad song id"))?;
        if e.show.song(id).is_some() { Ok(id) } else { Err(invalid(format!("no song with id {id}"))) }
    } else {
        e.current_song.filter(|&id| e.show.song(id).is_some()).ok_or_else(|| invalid("no song chosen"))
    }
}

/// Whether a path names a score file (`.musicxml`, `.xml`, `.mxl`).
pub fn is_notation_path(path: &str) -> bool {
    services::extension(path).is_some_and(|ext| actcraft_notation::EXTENSIONS.contains(&ext.as_str()))
}

/// Reads and parses a score through the services, with the errors in fixed words the window
/// translates.
pub fn load_score(e: &Engine, path: &str) -> Result<Score, CommandError> {
    if !is_notation_path(path) {
        return Err(invalid("ActCraft reads notation from MusicXML files (.musicxml, .xml, .mxl)"));
    }
    let bytes = e.services().read_file(path).map_err(failed)?;
    actcraft_notation::read(&bytes).map_err(|err| match err {
        NotationError::TooLarge => failed("that notation file is too large"),
        NotationError::Unreadable(_) => failed("that notation file cannot be read"),
        NotationError::NotAScore => failed("that file is not a MusicXML score"),
        NotationError::NoScoreInArchive => failed("that compressed file holds no MusicXML score"),
        NotationError::Empty => failed("that score has no notes"),
    })
}

/// A short description of a score for the command line and the control channel.
fn summary(score: &Score) -> Value {
    let parts: Vec<Value> = score
        .parts
        .iter()
        .map(|part| {
            let notes = part.bars.iter().flat_map(|b| &b.notes).filter(|n| n.pitch.is_some()).count();
            json!({
                "name": part.name,
                "bars": part.bars.len(),
                "played_bars": actcraft_notation::play_order(&part.bars).len(),
                "notes": notes,
                "tablature": part.tablature,
                "strings": part.strings(),
                "capo": part.capo,
            })
        })
        .collect();
    let tempo = score.parts.iter().flat_map(|p| &p.bars).find_map(|b| b.tempo);
    json!({"title": score.title, "composer": score.composer, "tempo": tempo, "parts": parts})
}

/// `notation.attach {song?, path}`: reads the score (refusing a file that isn't one) and shows it
/// in the song's Sheet music tab. An empty or default title is taken from the score.
pub fn notation_attach(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    let score = load_score(e, &path)?;
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    let title = score.title.trim();
    if !title.is_empty() && (song.title.trim().is_empty() || song.title.starts_with("Song ")) {
        song.title = title.chars().take(MAX_TEXT).collect();
    }
    song.notation_file = Some(path);
    let bars = score.parts.iter().map(|p| p.bars.len()).max().unwrap_or(0);
    e.mark_changed();
    e.say_with(
        "Notation attached: {parts} parts, {bars} bars",
        vec![MessageArg::plain("parts", score.parts.len()), MessageArg::plain("bars", bars)],
    );
    Ok(summary(&score))
}

/// `notation.read {song?}`: the song's score, described part by part.
pub fn notation_read(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let path =
        e.show.song(id).and_then(|s| s.notation_file.clone()).ok_or_else(|| invalid("the song has no notation"))?;
    let score = load_score(e, &path)?;
    Ok(summary(&score))
}
