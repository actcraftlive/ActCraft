//! Sequences: looks over time, an animation for moving heads or a chase. Each step is a preset,
//! reached over the first part of the step (its fade) and held for the rest; a sequence loops,
//! runs once, or bounces back. Step lengths are seconds or beats: in beats it follows the playing
//! song's beat grid (so its steps land on the click), or the song's tempo while nothing plays.
//! A cue can start and stop sequences (and effects), so one cue runs a whole animation.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_SEQUENCE_STEPS, MAX_SEQUENCES};
use actcraft_show::{Preset, Sequence, SequenceMode, SequenceStep};
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

use crate::Engine;

/// Longest step, seconds or beats.
const MAX_LENGTH: f64 = 3600.0;
/// Tempo for a sequence in beats when no song gives one.
const DEFAULT_BPM: f64 = 120.0;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// Where a sequence stands `pos` (seconds or beats) after its start: the step it comes from, the
/// step it goes to and how far (0-1) between them. `None` for a sequence without steps.
pub fn position(steps: &[SequenceStep], mode: SequenceMode, pos: f64) -> Option<(usize, usize, f64)> {
    let n = steps.len();
    if n == 0 {
        return None;
    }
    // The steps in the order they play in one round.
    let order: Vec<usize> = match mode {
        SequenceMode::Bounce if n > 2 => (0..n).chain((1..n - 1).rev()).collect(),
        _ => (0..n).collect(),
    };
    let len = |i: usize| steps.get(i).map_or(1.0, |s| s.length.max(0.01));
    let round: f64 = order.iter().map(|&i| len(i)).sum();
    let pos = if pos.is_finite() { pos.max(0.0) } else { 0.0 };
    if mode == SequenceMode::Once && pos >= round {
        return Some((n - 1, n - 1, 1.0));
    }
    let first_round = pos < round;
    let mut at = pos % round;
    for (k, &i) in order.iter().enumerate() {
        let l = len(i);
        if at < l || k + 1 == order.len() {
            // Coming from the step before in the round; the very first step of a run that
            // doesn't loop starts as it is.
            let from = match k {
                0 if mode == SequenceMode::Once || (first_round && mode != SequenceMode::Loop) => i,
                0 => *order.last().unwrap_or(&i),
                _ => order.get(k - 1).copied().unwrap_or(i),
            };
            let fade = steps.get(i).map_or(0.0, |s| s.fade) * l;
            let t = if fade > 0.0 && from != i { (at / fade).clamp(0.0, 1.0) } else { 1.0 };
            return Some((from, i, t));
        }
        at -= l;
    }
    None
}

/// Whether a sequence that runs once is past its end.
fn finished(steps: &[SequenceStep], mode: SequenceMode, pos: f64) -> bool {
    mode == SequenceMode::Once && pos >= steps.iter().map(|s| s.length.max(0.01)).sum::<f64>()
}

impl Engine {
    /// How far (seconds, or beats for a sequence in beats) `seq` has come since `started`. In
    /// beats while a song plays it follows the song's beat grid, so steps land on the click.
    fn sequence_pos(&self, seq: &Sequence, started: DateTime<FixedOffset>, now: DateTime<FixedOffset>) -> f64 {
        let elapsed = (now - started).num_milliseconds() as f64 / 1000.0;
        if !seq.in_beats {
            return elapsed;
        }
        let song = self.current().filter(|s| s.bpm > 0.0);
        if self.status.playing
            && self.status.time.is_finite()
            && let Some(song) = song
        {
            let clock = crate::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
            return clock.beat_at(self.status.time).max(0.0);
        }
        elapsed * song.map_or(DEFAULT_BPM, |s| s.bpm) / 60.0
    }

    /// Moves every running sequence on to `now`; one that runs once stops after its last step.
    pub fn step_sequences(&mut self, now: DateTime<FixedOffset>) {
        let running = self.running_sequences.clone();
        let mut done = Vec::new();
        for (name, started) in running {
            let Some(seq) = self.show.sequences.iter().find(|s| s.name == name).cloned() else { continue };
            let pos = self.sequence_pos(&seq, started, now);
            let Some((from, to, t)) = position(&seq.steps, seq.mode, pos) else { continue };
            let preset = |i: usize| -> Option<Preset> {
                let step = seq.steps.get(i)?;
                self.show.presets.iter().find(|p| p.name == step.preset).cloned()
            };
            let (Some(a), Some(b)) = (preset(from), preset(to)) else { continue };
            for v in &b.values {
                let start = a
                    .values
                    .iter()
                    .find(|w| w.fixture == v.fixture && w.attribute == v.attribute)
                    .map_or(v.value, |w| w.value);
                self.put(v.fixture, v.attribute, start + (v.value - start) * t);
            }
            if finished(&seq.steps, seq.mode, pos) {
                done.push(name);
            }
        }
        self.running_sequences.retain(|(n, _)| !done.contains(n));
    }

    /// The names of the sequences running now.
    pub fn running_sequences(&self) -> Vec<String> {
        self.running_sequences.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Starts a sequence or an effect by name; whether one of that name was there.
    pub(crate) fn start_by_name(&mut self, name: &str) -> bool {
        let now = self.services().now();
        if self.show.sequences.iter().any(|s| s.name == name) {
            self.running_sequences.retain(|(n, _)| n != name);
            self.running_sequences.push((name.to_owned(), now));
            return true;
        }
        if self.show.effects.iter().any(|f| f.name == name) {
            self.running_effects.retain(|(n, _)| n != name);
            self.running_effects.push((name.to_owned(), now));
            return true;
        }
        false
    }

    /// Stops a sequence or an effect by name; "*" stops every one.
    pub(crate) fn stop_by_name(&mut self, name: &str) {
        if name == "*" {
            self.running_sequences.clear();
            self.running_effects.clear();
        } else {
            self.running_sequences.retain(|(n, _)| n != name);
            self.running_effects.retain(|(n, _)| n != name);
        }
    }
}

fn index(e: &Engine, prm: &Value) -> Result<usize, CommandError> {
    p::index(prm, "sequence", e.show.sequences.len())
}

fn mode_param(prm: &Value) -> Result<Option<SequenceMode>, CommandError> {
    Ok(match p::opt_str_max(prm, "mode", 16)? {
        None => None,
        Some("loop") => Some(SequenceMode::Loop),
        Some("once") => Some(SequenceMode::Once),
        Some("bounce") => Some(SequenceMode::Bounce),
        Some(other) => return Err(invalid(format!("`mode` must be loop, once or bounce, not `{other}`"))),
    })
}

fn mode_key(m: SequenceMode) -> &'static str {
    match m {
        SequenceMode::Loop => "loop",
        SequenceMode::Once => "once",
        SequenceMode::Bounce => "bounce",
    }
}

fn preset_name(e: &Engine, prm: &Value) -> Result<Option<String>, CommandError> {
    if prm.get("preset").is_none() {
        return Ok(None);
    }
    let i = p::index(prm, "preset", e.show.presets.len())?;
    Ok(e.show.presets.get(i).map(|p| p.name.clone()))
}

/// `sequence.set {name, mode?, in_beats?}` -> {sequence}: makes a sequence, or changes the one of
/// that name.
pub fn sequence_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a sequence needs a name"));
    }
    let mode = mode_param(prm)?;
    let in_beats = p::opt_bool(prm, "in_beats")?;
    let i = match e.show.sequences.iter().position(|s| s.name == name) {
        Some(i) => i,
        None => {
            if e.show.sequences.len() >= MAX_SEQUENCES {
                return Err(failed("a show holds at most 256 sequences"));
            }
            e.show.sequences.push(Sequence { name, ..Sequence::default() });
            e.show.sequences.len() - 1
        }
    };
    if let Some(s) = e.show.sequences.get_mut(i) {
        if let Some(m) = mode {
            s.mode = m;
        }
        if let Some(b) = in_beats {
            s.in_beats = b;
        }
    }
    e.mark_changed();
    Ok(json!({"sequence": i}))
}

/// `sequence.step_add {sequence, preset?, length?, fade?}` -> {step, preset}: adds a step; without
/// a preset it stores the look on stage now as one ("<sequence> <n>").
pub fn sequence_step_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let length = p::opt_f64_in(prm, "length", 0.01, MAX_LENGTH)?.unwrap_or(1.0);
    let fade = p::opt_f64_in(prm, "fade", 0.0, 1.0)?.unwrap_or(0.5);
    if e.show.sequences.get(i).is_some_and(|s| s.steps.len() >= MAX_SEQUENCE_STEPS) {
        return Err(failed("a sequence holds at most 256 steps"));
    }
    let preset = match preset_name(e, prm)? {
        Some(p) => p,
        None => {
            let seq = e.show.sequences.get(i).map(|s| s.name.clone()).unwrap_or_default();
            let count = e.show.sequences.get(i).map_or(0, |s| s.steps.len());
            // A name not taken yet, so recording never overwrites another preset.
            let mut k = count + 1;
            let mut name = format!("{seq} {k}");
            while e.show.presets.iter().any(|p| p.name == name) {
                k += 1;
                name = format!("{seq} {k}");
            }
            crate::looks::preset_store(e, &json!({"name": name}))?;
            name
        }
    };
    let Some(s) = e.show.sequences.get_mut(i) else { return Err(invalid("no such sequence")) };
    s.steps.push(SequenceStep { preset: preset.clone(), length, fade, ..SequenceStep::default() });
    let step = s.steps.len() - 1;
    e.mark_changed();
    Ok(json!({"step": step, "preset": preset}))
}

/// `sequence.step_set {sequence, step, preset?, length?, fade?}`.
pub fn sequence_step_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let steps = e.show.sequences.get(i).map_or(0, |s| s.steps.len());
    let k = p::index(prm, "step", steps)?;
    let preset = preset_name(e, prm)?;
    let length = p::opt_f64_in(prm, "length", 0.01, MAX_LENGTH)?;
    let fade = p::opt_f64_in(prm, "fade", 0.0, 1.0)?;
    let Some(step) = e.show.sequences.get_mut(i).and_then(|s| s.steps.get_mut(k)) else {
        return Err(invalid("no such step"));
    };
    if let Some(p) = preset {
        step.preset = p;
    }
    if let Some(l) = length {
        step.length = l;
    }
    if let Some(f) = fade {
        step.fade = f;
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `sequence.step_remove {sequence, step}`.
pub fn sequence_step_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let steps = e.show.sequences.get(i).map_or(0, |s| s.steps.len());
    let k = p::index(prm, "step", steps)?;
    if let Some(s) = e.show.sequences.get_mut(i) {
        s.steps.remove(k);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `sequence.step_move {sequence, step, to}`: puts a step at another place.
pub fn sequence_step_move(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let steps = e.show.sequences.get(i).map_or(0, |s| s.steps.len());
    let k = p::index(prm, "step", steps)?;
    let to = p::index(prm, "to", steps)?;
    if let Some(s) = e.show.sequences.get_mut(i) {
        let step = s.steps.remove(k);
        s.steps.insert(to, step);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `sequence.remove {sequence}`: it stops if it runs.
pub fn sequence_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let gone = e.show.sequences.remove(i);
    e.running_sequences.retain(|(n, _)| *n != gone.name);
    e.mark_changed();
    Ok(Value::Null)
}

/// `sequence.list` -> [{name, mode, in_beats, running, steps: [{preset, length, fade}]}].
pub fn sequence_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    let running = e.running_sequences();
    Ok(Value::Array(
        e.show
            .sequences
            .iter()
            .map(|s| {
                let steps: Vec<Value> = s
                    .steps
                    .iter()
                    .map(|st| json!({"preset": st.preset, "length": st.length, "fade": st.fade}))
                    .collect();
                json!({"name": s.name, "mode": mode_key(s.mode), "in_beats": s.in_beats,
                    "running": running.contains(&s.name), "steps": steps})
            })
            .collect(),
    ))
}

/// `sequence.start {sequence}`: runs it from its first step.
pub fn sequence_start(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let name = e.show.sequences.get(i).map(|s| s.name.clone()).unwrap_or_default();
    e.start_by_name(&name);
    Ok(Value::Null)
}

/// `sequence.stop {sequence?}`: stops it, or every sequence without one. Lights stay as they are.
pub fn sequence_stop(e: &mut Engine, prm: &Value) -> CommandResult {
    if prm.get("sequence").is_some() {
        let i = index(e, prm)?;
        let name = e.show.sequences.get(i).map(|s| s.name.clone()).unwrap_or_default();
        e.running_sequences.retain(|(n, _)| *n != name);
    } else {
        e.running_sequences.clear();
    }
    Ok(Value::Null)
}

/// `sequence.toggle {sequence}` -> {running}: starts it, or stops it while it runs (for pads and
/// foot switches).
pub fn sequence_toggle(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let name = e.show.sequences.get(i).map(|s| s.name.clone()).unwrap_or_default();
    let on = !e.running_sequences.iter().any(|(n, _)| *n == name);
    if on {
        e.start_by_name(&name);
    } else {
        e.running_sequences.retain(|(n, _)| *n != name);
    }
    Ok(json!({"running": on}))
}

/// `cue.set {list, cue, name?, preset?, fade?, follow?, starts?, stops?}`: changes a cue; `starts`
/// and `stops` name sequences and effects it starts and stops ("*" stops all), `preset` may be
/// null for a cue that only starts or stops them.
pub fn cue_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let list = p::index(prm, "list", e.show.cue_lists.len())?;
    let cues = e.show.cue_lists.get(list).map_or(0, |l| l.cues.len());
    let k = p::index(prm, "cue", cues)?;
    let names = |key: &str| -> Result<Option<Vec<String>>, CommandError> {
        match prm.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) if items.len() <= 64 => items
                .iter()
                .map(|v| v.as_str().map(|s| s.trim().chars().take(64).collect::<String>()))
                .collect::<Option<Vec<_>>>()
                .map(|v| Some(v.into_iter().filter(|s| !s.is_empty()).collect()))
                .ok_or_else(|| invalid(format!("`{key}` must be a list of names"))),
            Some(_) => Err(invalid(format!("`{key}` must be a list of names"))),
        }
    };
    let starts = names("starts")?;
    let stops = names("stops")?;
    for n in starts.iter().flatten() {
        if !e.show.sequences.iter().any(|s| s.name == *n) && !e.show.effects.iter().any(|f| f.name == *n) {
            return Err(invalid(format!("no sequence or effect called `{n}`")));
        }
    }
    let preset = if prm.get("preset").is_some_and(Value::is_null) { Some(String::new()) } else { preset_name(e, prm)? };
    let name = p::opt_str_max(prm, "name", 64)?.map(|n| n.trim().to_owned());
    let fade = p::opt_f64_in(prm, "fade", 0.0, actcraft_show::limits::MAX_FADE_S)?;
    let follow = p::opt_f64_in(prm, "follow", 0.0, actcraft_show::limits::MAX_FADE_S)?;
    let Some(c) = e.show.cue_lists.get_mut(list).and_then(|l| l.cues.get_mut(k)) else {
        return Err(invalid("no such cue"));
    };
    if let Some(v) = starts {
        c.starts = v;
    }
    if let Some(v) = stops {
        c.stops = v;
    }
    if let Some(v) = preset {
        c.preset = v;
    }
    if let Some(v) = name.filter(|n| !n.is_empty()) {
        c.name = v;
    }
    if let Some(v) = fade {
        c.fade = v;
    }
    if let Some(v) = follow {
        c.follow = v;
    }
    e.mark_changed();
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn steps(lengths: &[f64], fade: f64) -> Vec<SequenceStep> {
        lengths.iter().map(|&length| SequenceStep { length, fade, ..SequenceStep::default() }).collect()
    }

    #[test]
    fn a_position_names_the_steps_and_how_far_between_them() {
        let s = steps(&[1.0, 2.0, 1.0], 0.5);
        // Halfway through step 0's fade, on the first round of a loop: from the last step.
        assert_eq!(position(&s, SequenceMode::Loop, 0.25), Some((2, 0, 0.5)));
        // Step 1 holds after its fade (1 s of its 2).
        assert_eq!(position(&s, SequenceMode::Loop, 2.5), Some((0, 1, 1.0)));
        // Round two of a loop.
        assert_eq!(position(&s, SequenceMode::Loop, 4.0), Some((2, 0, 0.0)));
        // Once: stays on the last step at the end.
        assert_eq!(position(&s, SequenceMode::Once, 9.0), Some((2, 2, 1.0)));
        assert_eq!(position(&s, SequenceMode::Once, 0.25), Some((0, 0, 1.0)), "starts as it is");
        // Bounce: 0 1 2 1 0 1 ...
        assert_eq!(position(&s, SequenceMode::Bounce, 4.5).map(|p| p.1), Some(1));
        assert_eq!(position(&s, SequenceMode::Bounce, 6.5).map(|p| p.1), Some(0));
        assert_eq!(position(&[], SequenceMode::Loop, 1.0), None);
    }

    fn rig() -> Engine {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let _ = e.run("preset.store", &json!({"name": "Dark"}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Full"}));
        e
    }

    #[test]
    fn a_sequence_moves_the_lights_step_by_step_and_a_cue_starts_it() {
        let mut e = rig();
        assert_eq!(e.run("sequence.set", &json!({"name": "Pulse", "mode": "once"})).ok(), Some(json!({"sequence": 0})));
        assert!(e.run("sequence.set", &json!({"name": "X", "mode": "sideways"})).is_err());
        assert!(e.run("sequence.step_add", &json!({"sequence": 0, "preset": 0, "length": 1.0, "fade": 0.0})).is_ok());
        assert!(e.run("sequence.step_add", &json!({"sequence": 0, "preset": 1, "length": 2.0, "fade": 0.5})).is_ok());
        let level = |e: &Engine| e.dmx.universe(1).map_or(0, |u| u[0]);
        let start = e.services().now();
        e.running_sequences.push(("Pulse".into(), start));
        e.step_sequences(start + TimeDelta::milliseconds(500));
        assert_eq!(level(&e), 0, "step 1: dark");
        e.step_sequences(start + TimeDelta::milliseconds(1500));
        assert!((120..=135).contains(&level(&e)), "halfway into full: {}", level(&e));
        e.step_sequences(start + TimeDelta::milliseconds(2500));
        assert_eq!(level(&e), 255);
        e.step_sequences(start + TimeDelta::milliseconds(4000));
        assert!(e.running_sequences().is_empty(), "a sequence that runs once ends");
        // A cue without a preset that starts it, and one that stops everything.
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        assert!(e.run("cue.set", &json!({"list": 0, "cue": 0, "preset": null, "starts": ["Pulse"]})).is_ok());
        assert!(e.run("cue.set", &json!({"list": 0, "cue": 0, "starts": ["Nope"]})).is_err());
        assert!(e.run("cue.go", &json!({"list": 0})).is_ok());
        assert_eq!(e.running_sequences(), vec!["Pulse".to_owned()]);
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        let _ = e.run("cue.set", &json!({"list": 0, "cue": 1, "stops": ["*"]}));
        assert!(e.run("cue.go", &json!({"list": 0})).is_ok());
        assert!(e.running_sequences().is_empty());
        // Recording a step stores the look on stage under a fresh name.
        let r = e.run("sequence.step_add", &json!({"sequence": 0}));
        assert_eq!(r.ok().map(|v| v["preset"].clone()), Some(json!("Pulse 3")));
        assert_eq!(e.run("sequence.toggle", &json!({"sequence": 0})).ok(), Some(json!({"running": true})));
        assert_eq!(e.run("sequence.toggle", &json!({"sequence": 0})).ok(), Some(json!({"running": false})));
    }
}
