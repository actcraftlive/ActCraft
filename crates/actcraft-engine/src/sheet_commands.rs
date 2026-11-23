//! A song's further sheets, each for a musician ("Guitar", "Bass"), and the show's musicians.
//! The song's own sheet (its PDF and score) stays where it was; commands that work on a sheet
//! take `sheet` (from 0) for one of the further ones and leave it out for the song's own.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_MUSICIANS, MAX_SCORE_FIRST_BAR, MAX_SHEETS, MAX_TEXT, MIN_SCORE_FIRST_BAR};
use actcraft_show::{Sheet, Song};
use serde_json::{Value, json};

use crate::notation_commands::song_id;
use crate::{Engine, services};

/// Longest musician or sheet name.
const MAX_NAME: usize = 64;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// The sheet a command works on: `None` for the song's own sheet (no `sheet`), or a further sheet
/// the song has.
pub(crate) fn sheet_param(e: &Engine, song: u64, prm: &Value) -> Result<Option<usize>, CommandError> {
    if prm.get("sheet").is_none_or(Value::is_null) {
        return Ok(None);
    }
    let len = e.show.song(song).map_or(0, |s| s.sheets.len());
    p::index(prm, "sheet", len).map(Some)
}

/// The sheet a musician sees for a song: the first sheet made for them, else the first made for
/// anyone; `None` when there is none.
pub fn sheet_for(song: &Song, musician: &str) -> Option<usize> {
    let named = |m: &str| song.sheets.iter().position(|s| s.musician.eq_ignore_ascii_case(m));
    named(musician.trim()).or_else(|| named(""))
}

fn name_param(prm: &Value) -> Result<&str, CommandError> {
    let name = p::str_max(prm, "name", MAX_NAME)?.trim();
    if name.is_empty() {
        return Err(invalid("`name` must not be empty"));
    }
    Ok(name)
}

/// A musician named for a sheet must be one of the show's ("" is anyone); the show's spelling is
/// kept.
fn musician_param(e: &Engine, prm: &Value) -> Result<Option<String>, CommandError> {
    let Some(name) = p::opt_str_max(prm, "musician", MAX_NAME)? else { return Ok(None) };
    let name = name.trim();
    if name.is_empty() {
        return Ok(Some(String::new()));
    }
    e.show
        .musicians
        .iter()
        .find(|m| m.eq_ignore_ascii_case(name))
        .cloned()
        .map(Some)
        .ok_or_else(|| invalid(format!("no musician called `{name}`; add them first (musician.add)")))
}

/// A sheet file given as `key`: `Some(None)` takes it away (null), `Some(Some(path))` sets it.
fn file_param(
    prm: &Value,
    key: &str,
    ok: fn(&str) -> bool,
    wrong: &str,
) -> Result<Option<Option<String>>, CommandError> {
    if prm.get(key).is_some_and(Value::is_null) {
        return Ok(Some(None));
    }
    match p::opt_str_max(prm, key, MAX_TEXT)? {
        Some(v) if ok(v) => Ok(Some(Some(v.to_owned()))),
        Some(_) => Err(invalid(wrong)),
        None => Ok(None),
    }
}

/// `musician.add {name}` -> {musician: index}: a musician of the band, for their sheets.
pub fn musician_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = name_param(prm)?.to_owned();
    if e.show.musicians.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
        return Err(invalid(format!("there is already a musician called `{name}`")));
    }
    if e.show.musicians.len() >= MAX_MUSICIANS {
        return Err(failed(format!("a show holds at most {MAX_MUSICIANS} musicians")));
    }
    e.show.musicians.push(name);
    e.mark_changed();
    Ok(json!({"musician": e.show.musicians.len() - 1}))
}

/// `musician.remove {musician: index}`: the musician's sheets stay, for anyone.
pub fn musician_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "musician", e.show.musicians.len())?;
    let name = e.show.musicians.remove(i);
    for song in &mut e.show.songs {
        for sheet in &mut song.sheets {
            if sheet.musician.eq_ignore_ascii_case(&name) {
                sheet.musician.clear();
            }
        }
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `musician.list` -> [name].
pub fn musician_list(e: &mut Engine, _: &Value) -> CommandResult {
    Ok(json!(e.show.musicians))
}

/// `sheet.add {song?, name, musician?}` -> {sheet: index}: a further, empty sheet.
pub fn sheet_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let name = name_param(prm)?.to_owned();
    let musician = musician_param(e, prm)?.unwrap_or_default();
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    if song.sheets.len() >= MAX_SHEETS {
        return Err(failed(format!("a song holds at most {MAX_SHEETS} further sheets")));
    }
    song.sheets.push(Sheet { name, musician, ..Sheet::default() });
    let index = song.sheets.len() - 1;
    e.mark_changed();
    Ok(json!({"sheet": index}))
}

/// `sheet.set {song?, sheet?, name?, musician?, part?, pdf_file?, notation_file?,
/// score_first_bar?}`: changes a sheet; without `sheet` the song's own (which has no name,
/// musician or part).
pub fn sheet_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let sheet = sheet_param(e, id, prm)?;
    let name = if p::has(prm, "name") { Some(name_param(prm)?.to_owned()) } else { None };
    let musician = musician_param(e, prm)?;
    let part = p::opt_i64_in(prm, "part", 0, 255)?.and_then(|v| usize::try_from(v).ok());
    if sheet.is_none() && (name.is_some() || musician.is_some() || part.is_some()) {
        return Err(invalid("the song's own sheet has no name, musician or part; give `sheet`"));
    }
    let pdf = file_param(
        prm,
        "pdf_file",
        |v| services::extension(v).as_deref() == Some("pdf"),
        "`pdf_file` must be a .pdf file",
    )?;
    let notation = file_param(
        prm,
        "notation_file",
        crate::notation_commands::is_notation_path,
        "`notation_file` must be a .musicxml, .xml, .mxl or .mid file",
    )?;
    let first_bar = p::opt_i64_in(prm, "score_first_bar", MIN_SCORE_FIRST_BAR.into(), MAX_SCORE_FIRST_BAR.into())?
        .and_then(|v| i32::try_from(v).ok());
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    if let Some(s) = sheet.and_then(|i| song.sheets.get_mut(i)) {
        if let Some(v) = name {
            s.name = v;
        }
        if let Some(v) = musician {
            s.musician = v;
        }
        if let Some(v) = part {
            s.part = v;
        }
    }
    let Some(fields) = song.sheet_mut(sheet) else { return Err(invalid("no such sheet")) };
    if let Some(v) = pdf {
        *fields.pdf_file = v;
    }
    if let Some(v) = notation {
        *fields.notation_file = v;
    }
    if let Some(v) = first_bar {
        *fields.score_first_bar = v;
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `sheet.remove {song?, sheet}`.
pub fn sheet_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let len = e.show.song(id).map_or(0, |s| s.sheets.len());
    let i = p::index(prm, "sheet", len)?;
    if let Some(song) = e.show.song_mut(id) {
        song.sheets.remove(i);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `sheet.list {song?}` -> [{sheet, name, musician, pdf_file, notation_file, part}].
pub fn sheet_list(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let Some(song) = e.show.song(id) else { return Err(invalid("no such song")) };
    Ok(Value::Array(
        song.sheets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                json!({"sheet": i, "name": s.name, "musician": s.musician, "pdf_file": s.pdf_file,
                    "notation_file": s.notation_file, "part": s.part})
            })
            .collect(),
    ))
}

/// `sheet.for {song?, musician}` -> {sheet: index|null}: the sheet that musician sees.
pub fn sheet_for_musician(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let musician = p::str_max(prm, "musician", MAX_NAME)?;
    let Some(song) = e.show.song(id) else { return Err(invalid("no such song")) };
    Ok(json!({"sheet": sheet_for(song, musician)}))
}
