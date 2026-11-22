//! Marks on a song's PDF: pen strokes, highlights and notes drawn over the pages in the Sheet
//! music tab, kept in the show file with the song.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_MARK_POINTS, MAX_PDF_MARKS, MAX_PDF_PAGE, MAX_TEXT};
use actcraft_show::{PdfMark, PdfMarkKind, parse_colour};
use serde_json::{Value, json};

use crate::Engine;

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

/// The `points` param: `[[x, y], ...]`, each a fraction of the page from 0 to 1.
fn points(prm: &Value) -> Result<Vec<[f32; 2]>, CommandError> {
    let list = prm.get("points").and_then(Value::as_array).ok_or_else(|| invalid("`points` must be a list"))?;
    if list.len() > MAX_MARK_POINTS {
        return Err(invalid(format!("a mark has at most {MAX_MARK_POINTS} points")));
    }
    list.iter()
        .map(|pt| {
            let xy = pt.as_array().filter(|a| a.len() == 2).ok_or_else(|| invalid("a point is [x, y]"))?;
            let mut out = [0.0f32; 2];
            for (slot, v) in out.iter_mut().zip(xy) {
                let v = v
                    .as_f64()
                    .filter(|v| (0.0..=1.0).contains(v))
                    .ok_or_else(|| invalid("a point's x and y are fractions of the page, from 0 to 1"))?;
                *slot = v as f32;
            }
            Ok(out)
        })
        .collect()
}

/// `pdf.mark {song?, page, kind, points, colour?, text?}`: adds a mark to a page of the song's PDF.
pub fn pdf_mark(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let page = u32::try_from(p::i64_in(prm, "page", 0, i64::from(MAX_PDF_PAGE))?).unwrap_or(0);
    let kind = match p::opt_str_max(prm, "kind", 16)?.unwrap_or("pen") {
        "pen" => PdfMarkKind::Pen,
        "highlight" => PdfMarkKind::Highlight,
        "note" => PdfMarkKind::Note,
        other => return Err(invalid(format!("`kind` must be pen, highlight or note, not `{other}`"))),
    };
    let mut mark = PdfMark { page, kind, points: points(prm)?, ..PdfMark::default() };
    if let Some(c) = p::opt_str_max(prm, "colour", 16)? {
        if parse_colour(c).is_none() {
            return Err(invalid("`colour` must look like #rrggbb"));
        }
        c.clone_into(&mut mark.colour);
    }
    if let Some(t) = p::opt_str_max(prm, "text", MAX_TEXT)? {
        t.trim().clone_into(&mut mark.text);
    }
    let needed = mark.min_points();
    if mark.points.len() < needed || (kind != PdfMarkKind::Pen && mark.points.len() != needed) {
        return Err(invalid(match kind {
            PdfMarkKind::Pen => "a pen stroke needs at least two points",
            PdfMarkKind::Highlight => "a highlight needs two points, its corners",
            PdfMarkKind::Note => "a note needs one point",
        }));
    }
    if kind == PdfMarkKind::Note && mark.text.is_empty() {
        return Err(invalid("a note needs `text`"));
    }
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    if song.pdf_marks.len() >= MAX_PDF_MARKS {
        return Err(failed(format!("a song's PDF holds at most {MAX_PDF_MARKS} marks")));
    }
    song.pdf_marks.push(mark);
    let index = song.pdf_marks.len() - 1;
    e.mark_changed();
    Ok(json!({"mark": index}))
}

/// `pdf.unmark {song?, mark}`: removes one mark.
pub fn pdf_unmark(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let len = e.show.song(id).map_or(0, |s| s.pdf_marks.len());
    let i = p::index(prm, "mark", len)?;
    if let Some(s) = e.show.song_mut(id) {
        s.pdf_marks.remove(i);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `pdf.clear_marks {song?, page?, confirm: true}`: removes the marks of one page, or all.
pub fn pdf_clear_marks(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let page = p::opt_i64_in(prm, "page", 0, i64::from(MAX_PDF_PAGE))?.and_then(|v| u32::try_from(v).ok());
    if !p::opt_bool(prm, "confirm")?.unwrap_or(false) {
        return Err(failed("removing marks needs confirm: true"));
    }
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    let before = song.pdf_marks.len();
    song.pdf_marks.retain(|m| page.is_some_and(|p| m.page != p));
    let removed = before - song.pdf_marks.len();
    if removed > 0 {
        e.mark_changed();
    }
    Ok(json!({"removed": removed}))
}
