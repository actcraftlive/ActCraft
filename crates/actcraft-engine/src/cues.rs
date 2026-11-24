//! Fades and cue lists: a preset can fade in over some seconds instead of at once, and a cue
//! list goes through presets one by one with Go and Back. The fade moves each attribute from
//! where it is to the preset's value; the engine's tick moves it on.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_CUE_LISTS, MAX_CUES, MAX_FADE_S};
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

pub use actcraft_show::{Cue, CueList, LightCue};

use crate::Engine;
use crate::fixtures::{Attribute, levels_for, live_value_of};
use crate::looks::Preset;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// A fade under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Fade {
    started: DateTime<FixedOffset>,
    seconds: f64,
    parts: Vec<Part>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Part {
    fixture: u64,
    attribute: Attribute,
    from: f64,
    to: f64,
}

impl Fade {
    /// Moves its start later by `by` (after a freeze).
    pub(crate) fn shift(&mut self, by: chrono::TimeDelta) {
        self.started += by;
    }
}

impl Engine {
    /// Sets one fixture attribute (by fixture id) to `value`; whether a level changed.
    pub(crate) fn put(&mut self, fixture: u64, attribute: Attribute, value: f64) -> bool {
        let Some(f) = self.show.fixtures.iter().find(|f| f.id == fixture).cloned() else { return false };
        let mut changed = false;
        for (offset, level) in levels_for(&f, attribute, value) {
            changed |= self.dmx.set(usize::from(f.universe), usize::from(f.address) + offset, &[level]);
        }
        changed
    }

    /// Sets a preset's values in blind's copy (the levels being edited), at once.
    pub(crate) fn recall_edited(&mut self, preset: &Preset) -> bool {
        let mut changed = false;
        for v in &preset.values {
            let Some(f) = self.show.fixtures.iter().find(|f| f.id == v.fixture).cloned() else { continue };
            for (offset, level) in levels_for(&f, v.attribute, v.value) {
                changed |= self.dmx.set_edited(usize::from(f.universe), usize::from(f.address) + offset, &[level]);
            }
        }
        changed
    }

    /// Brings a preset's values in over `seconds` (at once for 0); whether a level changed at once.
    pub(crate) fn fade_to(&mut self, preset: &Preset, seconds: f64) -> bool {
        if seconds <= 0.0 {
            self.fade = None;
            let mut changed = false;
            for v in &preset.values {
                changed |= self.put(v.fixture, v.attribute, v.value);
            }
            return changed;
        }
        let parts = preset
            .values
            .iter()
            .filter_map(|v| {
                let f = self.show.fixtures.iter().find(|f| f.id == v.fixture)?;
                let from = live_value_of(self, f, v.attribute)?;
                Some(Part { fixture: v.fixture, attribute: v.attribute, from, to: v.value })
            })
            .collect();
        self.fade = Some(Fade { started: self.services().now(), seconds, parts });
        false
    }

    /// Moves a running fade on to `now`; it ends once its time is up.
    pub fn step_fade(&mut self, now: DateTime<FixedOffset>) {
        let Some(fade) = self.fade.clone() else { return };
        let elapsed = (now - fade.started).num_milliseconds() as f64 / 1000.0;
        let t = (elapsed / fade.seconds).clamp(0.0, 1.0);
        for part in &fade.parts {
            self.put(part.fixture, part.attribute, part.from + (part.to - part.from) * t);
        }
        if t >= 1.0 {
            self.fade = None;
        }
    }

    /// Whether a fade is under way (the window keeps drawing while it runs).
    pub fn fading(&self) -> bool {
        self.fade.is_some()
    }

    /// The cue a list stands at, if it has gone to one.
    pub fn cue_position(&self, list: usize) -> Option<usize> {
        self.cue_at.get(&list).copied()
    }

    /// Goes to cue `cue` of list `list`: fades its preset in.
    fn go_to(&mut self, list: usize, cue: usize) -> CommandResult {
        let c = self
            .show
            .cue_lists
            .get(list)
            .and_then(|l| l.cues.get(cue))
            .cloned()
            .ok_or_else(|| invalid("no such cue"))?;
        // A cue may have no preset of its own and only start or stop sequences and effects.
        if !c.preset.is_empty() {
            let preset = self.show.presets.iter().find(|p| p.name == c.preset).cloned();
            let Some(preset) = preset else { return Err(failed("that cue's preset is not in the show")) };
            self.fade_to(&preset, c.fade);
        }
        for name in &c.stops {
            self.stop_by_name(name);
        }
        for name in &c.starts {
            self.start_by_name(name);
        }
        self.cue_at.insert(list, cue);
        let now = self.services().now();
        self.cue_went.insert(list, now);
        Ok(json!({"cue": cue, "name": c.name}))
    }
}

impl Engine {
    /// Moves on every cue list whose cue goes on by itself and whose time is up: to the next cue,
    /// or after the last back to the first (a chase).
    pub fn step_follow(&mut self, now: DateTime<FixedOffset>) {
        let due: Vec<(usize, usize)> = self
            .cue_at
            .iter()
            .filter_map(|(&list, &cue)| {
                let l = self.show.cue_lists.get(list)?;
                let follow = l.cues.get(cue)?.follow;
                let went = self.cue_went.get(&list)?;
                let waited = (now - *went).num_milliseconds() as f64 / 1000.0;
                (follow > 0.0 && waited >= follow).then_some((list, (cue + 1) % l.cues.len().max(1)))
            })
            .collect();
        for (list, next) in due {
            if self.go_to(list, next).is_err() {
                // A cue whose preset is gone: stop going on rather than trying every frame.
                self.cue_went.remove(&list);
            }
        }
    }

    /// Fires the playing song's lighting cues whose time playback has just passed (as the
    /// song's macros: a jump does not fire what it skipped).
    pub(crate) fn run_due_light_cues(&mut self) {
        let (playing, now) = (self.status.playing, self.status.time);
        let Some(song) = self.current_song.filter(|_| playing && now.is_finite()) else {
            self.light_at = None;
            return;
        };
        let from = match self.light_at {
            Some(t) if now >= t && now - t <= crate::macros::MAX_STEP => t,
            _ => now - crate::macros::LEAD_IN,
        };
        self.light_at = Some(now);
        let due: Vec<LightCue> = self
            .show
            .song(song)
            .map(|s| s.light_cues.iter().filter(|c| c.time > from && c.time <= now).cloned().collect())
            .unwrap_or_default();
        for c in due {
            if let Some(list) = self.show.cue_lists.iter().position(|l| l.name == c.list) {
                let _ = self.go_to(list, c.cue);
            }
        }
    }
}

/// The `song` param, or the current song.
fn song_id(e: &Engine, prm: &Value) -> Result<u64, CommandError> {
    if p::has(prm, "song") {
        let id = u64::try_from(p::i64_in(prm, "song", 1, i64::MAX)?).map_err(|_| invalid("no such song"))?;
        if e.show.song(id).is_some() { Ok(id) } else { Err(invalid("no such song")) }
    } else {
        e.current_song.filter(|&id| e.show.song(id).is_some()).ok_or_else(|| invalid("no song chosen"))
    }
}

/// Where a song lighting cue goes: `time` in seconds, or `bar` (from 1, as the grid numbers them)
/// and `beat` (from 1; 1 without) on the song's beat grid.
fn cue_time(e: &Engine, id: u64, prm: &Value) -> Result<f64, CommandError> {
    let Some(song) = e.show.song(id) else { return Err(invalid("no such song")) };
    let time = match (p::has(prm, "time"), p::has(prm, "bar")) {
        (true, false) => p::f64_in(prm, "time", 0.0, 86_400.0)?,
        (false, true) => {
            let clock = crate::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
            let bar = p::i64_in(prm, "bar", -9_999, 99_999)?;
            let beat = p::opt_i64_in(prm, "beat", 1, i64::from(clock.beats_per_bar))?.unwrap_or(1);
            clock.time_of_beat(((bar - 1) * i64::from(clock.beats_per_bar) + beat - 1) as f64)
        }
        _ => return Err(invalid("give `time`, or `bar` and maybe `beat`")),
    };
    if !(0.0..=86_400.0).contains(&time) {
        return Err(invalid("that bar is before the song starts"));
    }
    Ok(time)
}

/// Puts a lighting cue into a song's list in time order; its place.
fn place(song: &mut actcraft_show::Song, cue: LightCue) -> usize {
    let at = song.light_cues.partition_point(|c| c.time <= cue.time);
    song.light_cues.insert(at, cue);
    at
}

/// `song.light_cue_add {song?, time | bar + beat?, list, cue}`: the song (the current one
/// without) fires cue `cue` of list `list` when playback passes that point.
pub fn light_cue_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let time = cue_time(e, id, prm)?;
    let list = p::index(prm, "list", e.show.cue_lists.len())?;
    let len = e.show.cue_lists.get(list).map_or(0, |l| l.cues.len());
    let cue = p::index(prm, "cue", len)?;
    let list_name = e.show.cue_lists.get(list).map(|l| l.name.clone()).unwrap_or_default();
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    if song.light_cues.len() >= actcraft_show::limits::MAX_LIGHT_CUES {
        return Err(failed("a song fires at most 1000 lighting cues"));
    }
    let at = place(song, LightCue { time, list: list_name, cue, ..LightCue::default() });
    e.mark_changed();
    Ok(json!({"light_cue": at}))
}

/// `song.light_cue_move {song?, light_cue, time | bar + beat?}`: the cue fires at another point.
pub fn light_cue_move(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let len = e.show.song(id).map(|s| s.light_cues.len()).ok_or_else(|| invalid("no such song"))?;
    let i = p::index(prm, "light_cue", len)?;
    let time = cue_time(e, id, prm)?;
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    let mut cue = song.light_cues.remove(i);
    cue.time = time;
    let at = place(song, cue);
    e.mark_changed();
    Ok(json!({"light_cue": at}))
}

/// `song.light_cue_remove {song?, light_cue}`.
pub fn light_cue_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let len = e.show.song(id).map(|s| s.light_cues.len()).ok_or_else(|| invalid("no such song"))?;
    let i = p::index(prm, "light_cue", len)?;
    if let Some(song) = e.show.song_mut(id) {
        song.light_cues.remove(i);
    }
    e.mark_changed();
    Ok(Value::Null)
}

fn list_index(e: &Engine, prm: &Value) -> Result<usize, CommandError> {
    if prm.get("list").is_none() {
        // The first list when none is named, so Go works from a pad, a key or a foot switch.
        return if e.show.cue_lists.is_empty() { Err(failed("the show has no cue list")) } else { Ok(0) };
    }
    p::index(prm, "list", e.show.cue_lists.len())
}

/// `cue_list.add {name}`.
pub fn cue_list_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("give it a name"));
    }
    if e.show.cue_lists.len() >= MAX_CUE_LISTS {
        return Err(failed("a show holds at most 64 cue lists"));
    }
    e.show.cue_lists.push(CueList { name, ..CueList::default() });
    e.mark_changed();
    Ok(json!({"list": e.show.cue_lists.len() - 1}))
}

/// `cue_list.remove {list}`.
pub fn cue_list_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "list", e.show.cue_lists.len())?;
    e.show.cue_lists.remove(i);
    // Positions belong to list indexes, which have moved.
    e.cue_at.clear();
    e.cue_went.clear();
    e.mark_changed();
    Ok(Value::Null)
}

/// `cue_list.list`: every list with its cues and where it stands.
pub fn cue_list_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .cue_lists
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let cues: Vec<Value> =
                    l.cues.iter().map(|c| json!({"name": c.name, "preset": c.preset, "fade": c.fade})).collect();
                json!({"name": l.name, "cues": cues, "at": e.cue_position(i)})
            })
            .collect(),
    ))
}

/// `cue.add {list, preset, fade?, name?}`: a cue at the end of the list.
pub fn cue_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = p::index(prm, "list", e.show.cue_lists.len())?;
    let preset = p::index(prm, "preset", e.show.presets.len())?;
    let fade = p::opt_f64_in(prm, "fade", 0.0, MAX_FADE_S)?.unwrap_or(0.0);
    let follow = p::opt_f64_in(prm, "follow", 0.0, MAX_FADE_S)?.unwrap_or(0.0);
    let preset_name = e.show.presets.get(preset).map(|p| p.name.clone()).unwrap_or_default();
    let name =
        p::opt_str_max(prm, "name", 64)?.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&preset_name).to_owned();
    let Some(l) = e.show.cue_lists.get_mut(list) else { return Err(invalid("no such cue list")) };
    if l.cues.len() >= MAX_CUES {
        return Err(failed("a cue list holds at most 1000 cues"));
    }
    l.cues.push(Cue { name, preset: preset_name, fade, follow, ..Cue::default() });
    let cue = l.cues.len() - 1;
    e.mark_changed();
    Ok(json!({"cue": cue}))
}

/// `cue.remove {list, cue}`.
pub fn cue_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = p::index(prm, "list", e.show.cue_lists.len())?;
    let len = e.show.cue_lists.get(list).map_or(0, |l| l.cues.len());
    let cue = p::index(prm, "cue", len)?;
    if let Some(l) = e.show.cue_lists.get_mut(list) {
        l.cues.remove(cue);
    }
    e.cue_at.remove(&list);
    e.cue_went.remove(&list);
    e.mark_changed();
    Ok(Value::Null)
}

/// `cue.go {list?}`: the next cue (the first one at the start).
pub fn cue_go(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = list_index(e, prm)?;
    let len = e.show.cue_lists.get(list).map_or(0, |l| l.cues.len());
    let next = e.cue_position(list).map_or(0, |c| c + 1);
    if next >= len {
        return Err(failed("the cue list is at its last cue"));
    }
    e.go_to(list, next)
}

/// `cue.back {list?}`: the cue before.
pub fn cue_back(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = list_index(e, prm)?;
    let Some(previous) = e.cue_position(list).and_then(|c| c.checked_sub(1)) else {
        return Err(failed("the cue list is at its first cue"));
    };
    e.go_to(list, previous)
}

/// `cue.jump {list, cue}`: straight to a cue.
pub fn cue_jump(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = p::index(prm, "list", e.show.cue_lists.len())?;
    let len = e.show.cue_lists.get(list).map_or(0, |l| l.cues.len());
    let cue = p::index(prm, "cue", len)?;
    e.go_to(list, cue)
}

/// The attributes that make a fixture give light; Stop sets these to zero and leaves the rest
/// (position, gobo, zoom) where they are, so a moving head goes dark without swinging round.
const LIGHT: [Attribute; 9] = [
    Attribute::Intensity,
    Attribute::IntensityFine,
    Attribute::Red,
    Attribute::Green,
    Attribute::Blue,
    Attribute::White,
    Attribute::Amber,
    Attribute::Uv,
    Attribute::Strobe,
];

/// `cue_list.stop {list?, fade?}`: the list back to "not started", so it no longer goes on by
/// itself; the sequences and effects its cues started stop; the fixtures its cues' presets light
/// go dark (at once, or over `fade` seconds) without moving.
pub fn cue_list_stop(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = list_index(e, prm)?;
    let fade = p::opt_f64_in(prm, "fade", 0.0, MAX_FADE_S)?.unwrap_or(0.0);
    let Some(l) = e.show.cue_lists.get(list).cloned() else { return Err(invalid("no such cue list")) };
    e.cue_at.remove(&list);
    e.cue_went.remove(&list);
    for name in l.cues.iter().flat_map(|c| &c.starts) {
        e.stop_by_name(name);
    }
    // Every light-giving value the list's presets set, once each, to zero.
    let mut values: Vec<actcraft_show::PresetValue> = Vec::new();
    for preset in l.cues.iter().filter_map(|c| e.show.presets.iter().find(|p| p.name == c.preset)) {
        for v in preset.values.iter().filter(|v| LIGHT.contains(&v.attribute)) {
            if !values.iter().any(|d| d.fixture == v.fixture && d.attribute == v.attribute) {
                values.push(actcraft_show::PresetValue { value: 0.0, ..*v });
            }
        }
    }
    e.fade_to(&Preset { values, ..Preset::default() }, fade);
    Ok(Value::Null)
}

/// `lighting.stop_all {fade?}`: every cue list back to "not started", every sequence and effect
/// stopped, every fixture dark (at once, or over `fade` seconds) without moving.
pub fn lighting_stop_all(e: &mut Engine, prm: &Value) -> CommandResult {
    let fade = p::opt_f64_in(prm, "fade", 0.0, MAX_FADE_S)?.unwrap_or(0.0);
    e.cue_at.clear();
    e.cue_went.clear();
    e.stop_by_name("*");
    let values: Vec<actcraft_show::PresetValue> = e
        .show
        .fixtures
        .iter()
        .flat_map(|f| LIGHT.map(|attribute| actcraft_show::PresetValue { fixture: f.id, attribute, value: 0.0 }))
        .collect();
    e.fade_to(&Preset { values, ..Preset::default() }, fade);
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    /// A chase (a cue that goes on by itself) that starts a sequence stops with its list: no more
    /// going on, the sequence stops, the light goes dark.
    #[test]
    fn stop_ends_a_running_cue_list_and_darkens_its_lights() {
        let mut e = engine();
        let _ = e.run("sequence.set", &json!({"name": "Sweep"}));
        let _ = e.run("sequence.step_add", &json!({"sequence": 0, "preset": 0, "length": 1.0}));
        let _ = e.run("cue_list.add", &json!({"name": "Chase"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0, "follow": 1.0}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0, "follow": 1.0}));
        let _ = e.run("cue.set", &json!({"list": 0, "cue": 0, "starts": ["Sweep"]}));
        assert!(e.run("cue.go", &json!({"list": 0})).is_ok());
        assert_eq!(level(&e), 255);
        assert_eq!(e.running_sequences.len(), 1);

        assert!(e.run("cue_list.stop", &json!({"list": 0})).is_ok());
        assert_eq!(e.cue_position(0), None, "back to not started");
        assert!(e.running_sequences.is_empty(), "the sequence its cue started stops");
        assert_eq!(level(&e), 0, "dark");
        // Time passes: nothing goes on by itself any more.
        e.step_follow(e.services().now() + TimeDelta::seconds(5));
        assert_eq!(e.cue_position(0), None);
        assert_eq!(level(&e), 0);
        // Go starts it again from the first cue.
        assert!(e.run("cue.go", &json!({"list": 0})).is_ok());
        assert_eq!(e.cue_position(0), Some(0));
    }

    /// All stop ends every list, sequence and effect and darkens every fixture, also one no cue
    /// lit, without moving any.
    #[test]
    fn all_stop_stops_everything_and_darkens_every_fixture() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"channels": ["pan", "red"], "universe": 1, "address": 10}));
        let _ = e.run("fixture.set", &json!({"fixture": 1, "values": {"pan": 0.5, "red": 1.0}}));
        let _ = e.run("sequence.set", &json!({"name": "Sweep"}));
        let _ = e.run("sequence.step_add", &json!({"sequence": 0, "preset": 0, "length": 1.0}));
        let _ = e.run("sequence.start", &json!({"sequence": 0}));
        let _ = e.run("cue_list.add", &json!({"name": "A"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0, "follow": 1.0}));
        let _ = e.run("cue_list.add", &json!({"name": "B"}));
        let _ = e.run("cue.add", &json!({"list": 1, "preset": 0}));
        let _ = e.run("cue.go", &json!({"list": 0}));
        let _ = e.run("cue.go", &json!({"list": 1}));
        assert_eq!(level(&e), 255);

        assert!(e.run("lighting.stop_all", &Value::Null).is_ok());
        assert_eq!((e.cue_position(0), e.cue_position(1)), (None, None));
        assert!(e.running_sequences.is_empty());
        assert_eq!(level(&e), 0);
        assert_eq!(e.dmx.universe(1).map(|u| (u[9], u[10])), Some((128, 0)), "pan kept, red off");
        e.step_follow(e.services().now() + TimeDelta::seconds(5));
        assert_eq!(e.cue_position(0), None, "nothing goes on by itself");
    }

    /// Stop darkens without moving: a moving head's position stays where the cue put it.
    #[test]
    fn stop_leaves_position_alone() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"channels": ["pan", "intensity"], "universe": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"pan": 0.5, "intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Up"}));
        let _ = e.run("cue_list.add", &json!({"name": "L"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        let _ = e.run("cue.go", &Value::Null);
        assert!(e.run("cue_list.stop", &Value::Null).is_ok(), "the first list without a list");
        let u = e.dmx.universe(1).map(|u| (u[0], u[1]));
        assert_eq!(u, Some((128, 0)), "pan kept, intensity off");
        assert!(e.run("cue_list.stop", &json!({"list": 5})).is_err());
        assert!(e.run("cue_list.stop", &json!({"fade": -1})).is_err());
    }

    fn engine() -> Engine {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Full"}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let _ = e.run("preset.store", &json!({"name": "Dark"}));
        e
    }

    fn level(e: &Engine) -> u8 {
        e.dmx.universe(1).map_or(0, |u| u[0])
    }

    #[test]
    fn a_preset_fades_in_over_its_seconds() {
        let mut e = engine();
        let start = e.services().now();
        assert!(e.run("preset.recall", &json!({"preset": 0, "fade": 2.0})).is_ok());
        assert!(e.fading());
        e.step_fade(start + TimeDelta::milliseconds(1000));
        assert!((120..=135).contains(&level(&e)), "half way: {}", level(&e));
        e.step_fade(start + TimeDelta::milliseconds(2500));
        assert_eq!(level(&e), 255);
        assert!(!e.fading(), "the fade is over");
        assert!(e.run("preset.recall", &json!({"preset": 0, "fade": 601.0})).is_err());
    }

    #[test]
    fn a_song_fires_its_lighting_cues_as_playback_passes_them() {
        let mut e = engine();
        let _ = e.run("song.add", &Value::Null);
        let song = e.current_song.unwrap_or(0);
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        assert!(e.run("song.light_cue_add", &json!({"time": 10.0, "list": 0, "cue": 1})).is_ok());
        assert_eq!(
            e.run("song.light_cue_add", &json!({"time": 2.0, "list": 0, "cue": 0})).ok(),
            Some(json!({"light_cue": 0})),
            "kept in time order"
        );
        assert!(e.run("song.light_cue_add", &json!({"time": 3.0, "list": 0, "cue": 7})).is_err());
        let at = |e: &mut Engine, t: f64| {
            e.status.playing = true;
            e.status.time = t;
            e.run_due_light_cues();
        };
        at(&mut e, 1.9);
        assert_eq!(e.cue_position(0), None);
        at(&mut e, 2.1);
        assert_eq!((e.cue_position(0), level(&e)), (Some(0), 255), "cue 1 at 2 s");
        // A jump over 10 s does not fire what it skipped.
        at(&mut e, 30.0);
        assert_eq!(e.cue_position(0), Some(0));
        // Playing through it does.
        at(&mut e, 9.9);
        at(&mut e, 10.2);
        assert_eq!(e.cue_position(0), Some(1));
        assert!(e.run("song.light_cue_remove", &json!({"song": song, "light_cue": 0})).is_ok());
        assert_eq!(e.show.song(song).map(|s| s.light_cues.len()), Some(1));
    }

    #[test]
    fn lighting_cues_are_placed_and_moved_by_bar_and_beat() {
        let mut e = engine();
        let _ = e.run("song.add", &Value::Null);
        let song = e.current_song.unwrap_or(0);
        // 120 BPM in 4/4, beat one of bar one at 1 s: a beat is 0.5 s, a bar 2 s.
        let _ = e.run("song.set", &json!({"song": song, "bpm": 120.0, "beats_per_bar": 4, "beat_anchor": 1.0}));
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        let times = |e: &Engine| e.show.song(song).map(|s| s.light_cues.iter().map(|c| c.time).collect::<Vec<_>>());
        assert!(e.run("song.light_cue_add", &json!({"bar": 3, "list": 0, "cue": 0})).is_ok());
        assert!(e.run("song.light_cue_add", &json!({"bar": 2, "beat": 3, "list": 0, "cue": 1})).is_ok());
        assert_eq!(times(&e), Some(vec![4.0, 5.0]), "bar 2 beat 3 = 1 + 2 + 1; bar 3 = 1 + 4");
        assert!(e.run("song.light_cue_add", &json!({"bar": 2, "beat": 5, "list": 0, "cue": 0})).is_err(), "4/4");
        assert!(e.run("song.light_cue_add", &json!({"bar": -3, "list": 0, "cue": 0})).is_err(), "before 0 s");
        assert!(e.run("song.light_cue_add", &json!({"list": 0, "cue": 0})).is_err(), "no point");
        assert!(e.run("song.light_cue_add", &json!({"time": 1, "bar": 1, "list": 0, "cue": 0})).is_err(), "two points");
        // Moving keeps the list in time order and says where the cue went.
        assert_eq!(
            e.run("song.light_cue_move", &json!({"light_cue": 0, "bar": 5})).ok(),
            Some(json!({"light_cue": 1}))
        );
        assert_eq!(times(&e), Some(vec![5.0, 9.0]));
        assert_eq!(e.show.song(song).map(|s| s.light_cues[1].cue), Some(1), "the cue moved, not another");
        assert_eq!(
            e.run("song.light_cue_move", &json!({"light_cue": 1, "time": 0.5})).ok(),
            Some(json!({"light_cue": 0}))
        );
        assert!(e.run("song.light_cue_move", &json!({"light_cue": 2, "time": 1.0})).is_err());
    }

    #[test]
    fn cues_that_follow_go_on_by_themselves_and_round_again() {
        let mut e = engine();
        let _ = e.run("cue_list.add", &json!({"name": "Chase"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0, "follow": 0.5}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1, "follow": 0.5}));
        assert!(e.run("cue.add", &json!({"list": 0, "preset": 0, "follow": 601.0})).is_err());
        let start = e.services().now();
        assert!(e.run("cue.go", &Value::Null).is_ok());
        assert_eq!((e.cue_position(0), level(&e)), (Some(0), 255));
        e.step_follow(start + TimeDelta::milliseconds(400));
        assert_eq!(e.cue_position(0), Some(0), "not yet");
        // The clock in tests stands still, so "when it went" is `start`; half a second on, it goes.
        e.step_follow(start + TimeDelta::milliseconds(600));
        assert_eq!((e.cue_position(0), level(&e)), (Some(1), 0), "the next cue by itself");
        e.step_follow(start + TimeDelta::milliseconds(1200));
        assert_eq!(e.cue_position(0), Some(0), "after the last, back to the first");
        // A cue without follow waits for Go.
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        assert!(e.run("cue.jump", &json!({"list": 0, "cue": 2})).is_ok());
        e.step_follow(start + TimeDelta::milliseconds(9000));
        assert_eq!(e.cue_position(0), Some(2));
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.cue_lists).unwrap_or_default();
        assert_eq!(back, e.show.cue_lists);
    }

    #[test]
    fn a_cue_list_goes_forward_and_back() {
        let mut e = engine();
        assert!(e.run("cue.go", &Value::Null).is_err(), "no list yet");
        assert_eq!(e.run("cue_list.add", &json!({"name": "Main"})).ok(), Some(json!({"list": 0})));
        assert!(e.run("cue.add", &json!({"list": 0, "preset": 0})).is_ok());
        assert!(e.run("cue.add", &json!({"list": 0, "preset": 1, "fade": 3.0, "name": "Blackout"})).is_ok());
        assert_eq!(e.run("cue.go", &Value::Null).ok(), Some(json!({"cue": 0, "name": "Full"})));
        assert_eq!(level(&e), 255, "no fade: at once");
        assert_eq!(e.run("cue.go", &Value::Null).ok(), Some(json!({"cue": 1, "name": "Blackout"})));
        assert!(e.fading());
        assert!(e.run("cue.go", &Value::Null).is_err(), "at the last cue");
        assert_eq!(e.run("cue.back", &json!({"list": 0})).ok(), Some(json!({"cue": 0, "name": "Full"})));
        assert!(e.run("cue.back", &Value::Null).is_err(), "at the first cue");
        assert_eq!(e.run("cue.jump", &json!({"list": 0, "cue": 1})).ok().map(|v| v["cue"].clone()), Some(json!(1)));
        // A cue whose preset was removed says so.
        assert!(e.run("preset.remove", &json!({"preset": 0})).is_ok());
        assert!(e.run("cue.jump", &json!({"list": 0, "cue": 0})).is_err());
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.cue_lists).unwrap_or_default();
        assert_eq!(back, e.show.cue_lists);
    }
}
