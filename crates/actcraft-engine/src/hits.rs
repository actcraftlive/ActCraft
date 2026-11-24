//! Hits: a preset put on for a moment, such as a burst of fog for half a second or a flash. A hit
//! is outside the cue lists: a cue taking over leaves it alone. When its time is up, the channels
//! it set go back to what they were when it fired. A song's lighting lane can fire hits at bars.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_FADE_S, MAX_HITS};
use actcraft_show::{Hit, PresetValue};
use chrono::{DateTime, FixedOffset, TimeDelta};
use serde_json::{Value, json};

use crate::Engine;
use crate::fixtures::slot_value_of;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// Shortest hit: a DMX frame or two still sees it.
const MIN_SECONDS: f64 = 0.05;

/// A hit that is on.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningHit {
    name: String,
    until: DateTime<FixedOffset>,
    /// What its channels were before it fired: put back when it ends.
    restore: Vec<PresetValue>,
}

impl RunningHit {
    /// Moves its end later by `by` (after a freeze).
    pub(crate) fn shift(&mut self, by: TimeDelta) {
        self.until += by;
    }
}

impl Engine {
    /// Fires the hit of this name: its preset on now, until its time is up. Fired again while on,
    /// it stays on longer and still goes back to what was there before the first one.
    pub(crate) fn fire_hit(&mut self, name: &str) -> Result<(), CommandError> {
        let hit = self.show.hits.iter().find(|h| h.name == name).cloned().ok_or_else(|| invalid("no such hit"))?;
        let preset = self
            .show
            .presets
            .iter()
            .find(|p| p.name == hit.preset)
            .cloned()
            .ok_or_else(|| failed("that hit's preset is not in the show"))?;
        let millis = (hit.seconds.clamp(MIN_SECONDS, MAX_FADE_S) * 1000.0) as i64;
        let until = self.services().now() + TimeDelta::milliseconds(millis);
        if let Some(on) = self.running_hits.iter_mut().find(|r| r.name == name) {
            on.until = until;
        } else {
            let restore = preset
                .values
                .iter()
                .filter_map(|v| {
                    let f = self.show.fixtures.iter().find(|f| f.id == v.fixture)?;
                    Some(PresetValue { value: slot_value_of(self, f, v.attribute, v.channel)?, ..*v })
                })
                .collect();
            self.running_hits.push(RunningHit { name: name.to_owned(), until, restore });
        }
        for v in &preset.values {
            self.put_slot(v.fixture, v.attribute, v.channel, v.value);
        }
        Ok(())
    }

    /// Ends the hits whose time is up, putting back what their channels were.
    pub fn step_hits(&mut self, now: DateTime<FixedOffset>) {
        let (done, on): (Vec<RunningHit>, Vec<RunningHit>) =
            std::mem::take(&mut self.running_hits).into_iter().partition(|h| now >= h.until);
        self.running_hits = on;
        for h in done {
            for v in h.restore {
                self.put_slot(v.fixture, v.attribute, v.channel, v.value);
            }
        }
    }

    /// The names of the hits that are on now.
    pub fn hits_on(&self) -> Vec<String> {
        self.running_hits.iter().map(|h| h.name.clone()).collect()
    }
}

fn index(e: &Engine, prm: &Value) -> Result<usize, CommandError> {
    p::index(prm, "hit", e.show.hits.len())
}

/// `hit.set {name, preset: index, seconds?}` -> {hit}: makes a hit, or changes the one of that name.
pub fn hit_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a hit needs a name"));
    }
    let preset = p::index(prm, "preset", e.show.presets.len())?;
    let seconds = p::opt_f64_in(prm, "seconds", MIN_SECONDS, MAX_FADE_S)?.unwrap_or(0.5);
    let preset = e.show.presets.get(preset).map(|p| p.name.clone()).unwrap_or_default();
    let i = match e.show.hits.iter().position(|h| h.name == name) {
        Some(i) => i,
        None => {
            if e.show.hits.len() >= MAX_HITS {
                return Err(failed("a show holds at most 256 hits"));
            }
            e.show.hits.push(Hit { name, ..Hit::default() });
            e.show.hits.len() - 1
        }
    };
    if let Some(h) = e.show.hits.get_mut(i) {
        h.preset = preset;
        h.seconds = seconds;
    }
    e.mark_changed();
    Ok(json!({"hit": i}))
}

/// `hit.remove {hit}`.
pub fn hit_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    e.show.hits.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `hit.list`: every hit with its preset, length and whether it is on.
pub fn hit_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    let on = e.hits_on();
    Ok(Value::Array(
        e.show
            .hits
            .iter()
            .map(|h| json!({"name": h.name, "preset": h.preset, "seconds": h.seconds, "on": on.contains(&h.name)}))
            .collect(),
    ))
}

/// `hit.fire {hit}`: puts the hit on now.
pub fn hit_fire(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = index(e, prm)?;
    let name = e.show.hits.get(i).map(|h| h.name.clone()).unwrap_or_default();
    e.fire_hit(&name)?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fog machine on channel 1 and a dimmer on 2: the dimmer is lit by a cue; the hit fires the
    /// fog for half a second.
    fn engine() -> Engine {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "address": 1, "name": "Fog"}));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "address": 2}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Fog on", "fixture": 0}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let _ = e.run("fixture.set", &json!({"fixture": 1, "values": {"intensity": 0.6}}));
        e
    }

    fn levels(e: &Engine) -> (u8, u8) {
        e.dmx.universe(1).map_or((0, 0), |u| (u[0], u[1]))
    }

    #[test]
    fn a_hit_is_on_for_its_time_then_its_channels_go_back() {
        let mut e = engine();
        assert_eq!(
            e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 0.5})).ok(),
            Some(json!({"hit": 0}))
        );
        assert!(e.run("hit.fire", &json!({"hit": 0})).is_ok());
        assert_eq!(levels(&e), (255, 153), "fog on, the other light untouched");
        let now = e.services().now();
        e.step_hits(now + TimeDelta::milliseconds(400));
        assert_eq!(levels(&e).0, 255, "still on at 0.4 s");
        e.step_hits(now + TimeDelta::milliseconds(600));
        assert_eq!(levels(&e), (0, 153), "back to what it was");
        assert!(e.hits_on().is_empty());
    }

    #[test]
    fn firing_again_while_on_keeps_it_on_longer_and_still_goes_back() {
        let mut e = engine();
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 1.0}));
        let _ = e.run("hit.fire", &json!({"hit": 0}));
        let _ = e.run("hit.fire", &json!({"hit": 0}));
        assert_eq!(e.hits_on().len(), 1);
        e.step_hits(e.services().now() + TimeDelta::seconds(2));
        assert_eq!(levels(&e).0, 0, "back to off, not to the hit's own level");
    }

    /// A cue taking over leaves a hit alone: it ends on its own time.
    #[test]
    fn a_cue_taking_over_leaves_a_hit_alone() {
        let mut e = engine();
        let _ = e.run("preset.store", &json!({"name": "Dim", "fixture": 1}));
        let _ = e.run("cue_list.add", &json!({"name": "L"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 1.0}));
        let _ = e.run("hit.fire", &json!({"hit": 0}));
        let _ = e.run("cue.go", &Value::Null);
        assert_eq!((levels(&e).0, e.hits_on().len()), (255, 1));
    }

    /// The owner's case: fog for half a second at a point of the song, from its lighting lane.
    #[test]
    fn a_song_fires_a_hit_from_its_lighting_lane() {
        let mut e = engine();
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 0.5}));
        assert_eq!(e.run("song.light_cue_add", &json!({"time": 4.0, "hit": 0})).ok(), Some(json!({"light_cue": 0})));
        assert_eq!(e.current().and_then(|s| s.light_cues.first()).map(|c| c.hit.clone()), Some("Fog".to_owned()));
        assert!(e.run("song.light_cue_add", &json!({"time": 4.0, "hit": 3})).is_err());
        let at = |e: &mut Engine, t: f64| {
            e.status.playing = true;
            e.status.time = t;
            e.run_due_light_cues();
        };
        at(&mut e, 3.9);
        assert_eq!(levels(&e).0, 0);
        at(&mut e, 4.1);
        assert_eq!(levels(&e).0, 255, "fog at 4 s");
        e.step_hits(e.services().now() + TimeDelta::seconds(1));
        assert_eq!(levels(&e).0, 0, "and off again");
    }

    #[test]
    fn hits_are_checked() {
        let mut e = engine();
        assert!(e.run("hit.set", &json!({"name": " ", "preset": 0})).is_err());
        assert!(e.run("hit.set", &json!({"name": "Fog", "preset": 9})).is_err());
        assert!(e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 0.0})).is_err());
        assert!(e.run("hit.fire", &json!({"hit": 0})).is_err(), "no hit yet");
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 0}));
        assert_eq!(e.show.hits.first().map(|h| h.seconds), Some(0.5), "half a second by default");
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 0, "seconds": 2.0}));
        assert_eq!(e.show.hits.len(), 1, "the same name changes the hit");
        // Its preset gone: firing says so instead of doing nothing.
        let _ = e.run("preset.remove", &json!({"preset": 0}));
        assert!(e.run("hit.fire", &json!({"hit": 0})).is_err());
        assert!(e.run("hit.remove", &json!({"hit": 0})).is_ok());
        assert_eq!(e.run("hit.list", &Value::Null).ok(), Some(json!([])));
    }
}
