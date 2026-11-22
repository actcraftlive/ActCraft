//! Region commands: named, coloured parts of a song that play, loop, skip or stop.

use actcraft_audio::Msg;
use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_REGIONS, MAX_SECONDS, MAX_TEXT};
use actcraft_show::{Region, RegionMode, parse_colour};
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

fn region_index(e: &Engine, prm: &Value, song: u64) -> Result<usize, CommandError> {
    let len = e.show.song(song).map_or(0, |s| s.regions.len());
    p::index(prm, "region", len)
}

fn mode_of(s: &str) -> Result<RegionMode, CommandError> {
    match s {
        "play" => Ok(RegionMode::Play),
        "loop" => Ok(RegionMode::Loop),
        "skip" => Ok(RegionMode::Skip),
        "stop_after" => Ok(RegionMode::StopAfter),
        other => Err(invalid(format!("`mode` must be play, loop, skip or stop_after, not `{other}`"))),
    }
}

/// Reads optional region fields into `r`; checked first so a bad field changes nothing.
fn apply(r: &mut Region, prm: &Value) -> Result<(), CommandError> {
    let mut next = r.clone();
    if let Some(v) = p::opt_str_max(prm, "name", MAX_TEXT)? {
        next.name = v.to_owned();
    }
    if let Some(v) = p::opt_str_max(prm, "colour", 16)? {
        if parse_colour(v).is_none() {
            return Err(invalid("`colour` must look like #rrggbb"));
        }
        next.colour = v.to_owned();
    }
    if let Some(v) = p::opt_f64_in(prm, "start", 0.0, MAX_SECONDS)? {
        next.start = v;
    }
    if let Some(v) = p::opt_f64_in(prm, "end", 0.0, MAX_SECONDS)? {
        next.end = v;
    }
    if let Some(v) = p::opt_str_max(prm, "mode", 16)? {
        next.mode = mode_of(v)?;
    }
    if let Some(v) = p::opt_bool(prm, "cue")? {
        next.cue = v;
    }
    if let Some(v) = p::opt_str_max(prm, "voice", MAX_TEXT)? {
        crate::voice::check_voice_text(v)?;
        next.voice = v.to_owned();
    }
    if next.end <= next.start {
        return Err(invalid("a region must end after it starts"));
    }
    *r = next;
    Ok(())
}

/// Colours given to new regions in turn.
const PALETTE: [&str; 8] = ["#8a6fd1", "#4f9cf0", "#3dc97a", "#f2b43c", "#e5484d", "#3cd2e0", "#f078b4", "#a4abb8"];

pub fn region_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let now = e.status.time.max(0.0);
    let Some(song) = e.show.song(id) else { return Err(invalid("no such song")) };
    if song.regions.len() >= MAX_REGIONS {
        return Err(failed(format!("a song holds at most {MAX_REGIONS} regions")));
    }
    // Defaults: from the playback position for two bars (or to the song end).
    let bar = 60.0 / song.bpm.max(1.0) * f64::from(song.beats_per_bar);
    let start = p::opt_f64_in(prm, "start", 0.0, MAX_SECONDS)?.unwrap_or(now);
    let song_end = song.end.unwrap_or(song.duration);
    let default_end = if song_end > start { (start + 2.0 * bar).min(song_end) } else { start + 2.0 * bar };
    let n = song.regions.len();
    let mut region = Region {
        name: format!("Part {}", n + 1),
        colour: PALETTE.get(n % PALETTE.len()).copied().unwrap_or("#8a6fd1").to_owned(),
        start,
        end: default_end.max(start + 0.1),
        ..Region::default()
    };
    apply(&mut region, prm)?;
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    song.regions.push(region);
    song.regions.sort_by(|a, b| a.start.total_cmp(&b.start));
    let index = song.regions.iter().position(|r| (r.start - start).abs() < 1e-9).unwrap_or(n);
    e.mark_changed();
    e.push_regions(id);
    Ok(json!({"region": index}))
}

pub fn region_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let i = region_index(e, prm, id)?;
    let Some(r) = e.show.song_mut(id).and_then(|s| s.regions.get_mut(i)) else { return Err(invalid("no such region")) };
    apply(r, prm)?;
    if let Some(s) = e.show.song_mut(id) {
        s.regions.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    e.mark_changed();
    e.push_regions(id);
    Ok(Value::Null)
}

pub fn region_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let i = region_index(e, prm, id)?;
    if let Some(s) = e.show.song_mut(id) {
        s.regions.remove(i);
    }
    e.mark_changed();
    e.push_regions(id);
    Ok(Value::Null)
}

/// Jumps to a region's start, live while playing.
pub fn region_jump(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let i = region_index(e, prm, id)?;
    let start = e.show.song(id).and_then(|s| s.regions.get(i)).map(|r| r.start).unwrap_or(0.0);
    if e.loaded_song_pub() != Some(id) {
        return Err(failed("load the song first"));
    }
    e.send_msg(Msg::Seek(start));
    Ok(json!({"seconds": start}))
}

/// Lets the loop that's playing finish its pass and go on.
pub fn region_leave_loop(e: &mut Engine, _: &Value) -> CommandResult {
    if !e.has_audio() {
        return Err(failed("no audio output is running"));
    }
    e.send_msg(Msg::LeaveLoop);
    Ok(Value::Null)
}

/// Markers in a track's WAV file become regions (a marker with a length is a region; points
/// run to the next marker).
pub fn region_import_markers(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let tracks = e.show.song(id).map_or(0, |s| s.tracks.len());
    let t = if p::has(prm, "track") { p::index(prm, "track", tracks)? } else { 0 };
    let Some(path) = e.show.song(id).and_then(|s| s.tracks.get(t)).map(|tr| tr.file.clone()).filter(|f| !f.is_empty())
    else {
        return Err(failed("the track has no audio file"));
    };
    let bytes = e.services().read_file(&path).map_err(failed)?;
    let mut markers = actcraft_audio::clip::wav_markers(&bytes);
    if markers.is_empty() {
        return Err(failed("the file has no markers (ActCraft reads markers from WAV files)"));
    }
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let song_end = e.show.song(id).map_or(0.0, |s| s.end.unwrap_or(s.duration));
    let mut regions = Vec::new();
    for (k, (start, dur, name)) in markers.iter().enumerate() {
        let end = if *dur > 0.0 {
            start + dur
        } else {
            markers.get(k + 1).map(|m| m.0).unwrap_or(if song_end > *start { song_end } else { start + 10.0 })
        };
        if end > *start {
            regions.push(Region {
                name: if name.trim().is_empty() { format!("Marker {}", k + 1) } else { name.clone() },
                colour: PALETTE.get(k % PALETTE.len()).copied().unwrap_or("#8a6fd1").to_owned(),
                start: *start,
                end,
                ..Region::default()
            });
        }
    }
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    let room = MAX_REGIONS.saturating_sub(song.regions.len());
    let added = regions.len().min(room);
    song.regions.extend(regions.into_iter().take(room));
    song.regions.sort_by(|a, b| a.start.total_cmp(&b.start));
    e.mark_changed();
    e.push_regions(id);
    e.say_with("Imported {count} regions from markers", vec![crate::MessageArg::plain("count", added)]);
    Ok(json!({"added": added}))
}
