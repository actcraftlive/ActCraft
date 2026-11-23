//! Effects: a group's attribute moved in a wave (sine, square, saw, triangle) between two
//! values, cycle by cycle, each fixture of the group a little later than the one before. A
//! cycle is seconds long, or a number of beats at the playing song's tempo. Running effects are
//! moved on by the engine's tick; they are not saved as running.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_EFFECTS;
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

pub use actcraft_show::{Effect, Shape};

use crate::Engine;
use crate::fixtures::{attribute_key, attribute_named};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

fn shape_named(key: &str) -> Option<Shape> {
    match key {
        "sine" => Some(Shape::Sine),
        "square" => Some(Shape::Square),
        "saw" => Some(Shape::Saw),
        "triangle" => Some(Shape::Triangle),
        _ => None,
    }
}

/// The shape's name in params and lists.
pub fn shape_key(s: Shape) -> &'static str {
    match s {
        Shape::Sine => "sine",
        Shape::Square => "square",
        Shape::Saw => "saw",
        Shape::Triangle => "triangle",
    }
}

impl Engine {
    /// Seconds one cycle of `fx` takes now.
    fn cycle_seconds(&self, fx: &Effect) -> f64 {
        let bpm = self.current().map_or(0.0, |s| s.bpm);
        if fx.beats > 0.0 && bpm > 0.0 { fx.beats * 60.0 / bpm } else { fx.period.max(0.05) }
    }

    /// Moves every running effect on to `now`.
    pub fn step_effects(&mut self, now: DateTime<FixedOffset>) {
        let running = self.running_effects.clone();
        for (name, started) in running {
            let Some(fx) = self.show.effects.iter().find(|f| f.name == name).cloned() else { continue };
            let Some(group) = self.show.groups.iter().find(|g| g.name == fx.group) else { continue };
            let ids = group.fixtures.clone();
            let elapsed = (now - started).num_milliseconds() as f64 / 1000.0;
            let base = elapsed / self.cycle_seconds(&fx);
            let n = ids.len().max(1) as f64;
            for (k, id) in ids.iter().enumerate() {
                let phase = base - fx.spread * k as f64 / n;
                let value = fx.low + (fx.high - fx.low) * fx.shape.at(phase);
                self.put(*id, fx.attribute, value);
            }
        }
    }

    /// The names of the effects running now.
    pub fn running_effects(&self) -> Vec<String> {
        self.running_effects.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// `effect.set {name, group, attribute, shape?, period?, beats?, low?, high?, spread?}`: an effect,
/// or the one of that name changed.
pub fn effect_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("give it a name"));
    }
    let g = p::index(prm, "group", e.show.groups.len())?;
    let group = e.show.groups.get(g).map(|g| g.name.clone()).unwrap_or_default();
    let attribute = attribute_named(p::str_max(prm, "attribute", 32)?)
        .filter(|a| !a.is_fine())
        .ok_or_else(|| invalid("`attribute` must be an attribute like intensity, red or pan"))?;
    let shape = match p::opt_str_max(prm, "shape", 16)? {
        None => Shape::Sine,
        Some(s) => shape_named(s).ok_or_else(|| invalid("`shape` must be sine, square, saw or triangle"))?,
    };
    let fx = Effect {
        name,
        group,
        attribute,
        shape,
        period: p::opt_f64_in(prm, "period", 0.05, 600.0)?.unwrap_or(2.0),
        beats: p::opt_f64_in(prm, "beats", 0.0, 64.0)?.unwrap_or(0.0),
        low: p::opt_f64_in(prm, "low", 0.0, 1.0)?.unwrap_or(0.0),
        high: p::opt_f64_in(prm, "high", 0.0, 1.0)?.unwrap_or(1.0),
        spread: p::opt_f64_in(prm, "spread", 0.0, 1.0)?.unwrap_or(0.0),
        ..Effect::default()
    };
    let i = match e.show.effects.iter().position(|f| f.name == fx.name) {
        Some(i) => {
            if let Some(slot) = e.show.effects.get_mut(i) {
                *slot = fx;
            }
            i
        }
        None => {
            if e.show.effects.len() >= MAX_EFFECTS {
                return Err(failed("a show holds at most 256 effects"));
            }
            e.show.effects.push(fx);
            e.show.effects.len() - 1
        }
    };
    e.mark_changed();
    Ok(json!({"effect": i}))
}

/// `effect.remove {effect}` (it stops if it runs).
pub fn effect_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "effect", e.show.effects.len())?;
    let gone = e.show.effects.remove(i).name;
    e.running_effects.retain(|(n, _)| *n != gone);
    e.mark_changed();
    Ok(Value::Null)
}

/// `effect.list`: every effect and whether it runs.
pub fn effect_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    let running = e.running_effects();
    Ok(Value::Array(
        e.show
            .effects
            .iter()
            .map(|f| {
                json!({"name": f.name, "group": f.group, "attribute": attribute_key(f.attribute), "shape": shape_key(f.shape),
                    "period": f.period, "beats": f.beats, "low": f.low, "high": f.high, "spread": f.spread,
                    "running": running.contains(&f.name)})
            })
            .collect(),
    ))
}

/// `effect.start {effect}`: runs it from the start of a cycle.
pub fn effect_start(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "effect", e.show.effects.len())?;
    let name = e.show.effects.get(i).map(|f| f.name.clone()).unwrap_or_default();
    let now = e.services().now();
    e.running_effects.retain(|(n, _)| *n != name);
    e.running_effects.push((name, now));
    Ok(Value::Null)
}

/// `effect.stop {effect?}`: stops it, or every effect without one. Attributes stay where they
/// are.
pub fn effect_stop(e: &mut Engine, prm: &Value) -> CommandResult {
    if prm.get("effect").is_some() {
        let i = p::index(prm, "effect", e.show.effects.len())?;
        let name = e.show.effects.get(i).map(|f| f.name.clone()).unwrap_or_default();
        e.running_effects.retain(|(n, _)| *n != name);
    } else {
        e.running_effects.clear();
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{Attribute, value_of};
    use chrono::TimeDelta;

    #[test]
    fn shapes_go_round_their_cycle() {
        assert!(Shape::Sine.at(0.0).abs() < 1e-9 && (Shape::Sine.at(0.5) - 1.0).abs() < 1e-9);
        assert_eq!((Shape::Square.at(0.25), Shape::Square.at(0.75)), (1.0, 0.0));
        assert!((Shape::Saw.at(0.25) - 0.25).abs() < 1e-9);
        assert!((Shape::Triangle.at(0.5) - 1.0).abs() < 1e-9 && Shape::Triangle.at(1.0).abs() < 1e-9);
        assert!((Shape::Saw.at(-0.25) - 0.75).abs() < 1e-9, "before the start wraps round");
    }

    #[test]
    fn a_running_effect_moves_its_group_with_a_spread() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "count": 2}));
        let _ = e.run("group.set", &json!({"name": "Pair", "fixtures": [0, 1]}));
        let fx = json!({"name": "Chase", "group": 0, "attribute": "intensity", "shape": "square", "period": 2.0, "spread": 1.0});
        assert_eq!(e.run("effect.set", &fx).ok(), Some(json!({"effect": 0})));
        assert!(e.run("effect.start", &json!({"effect": 0})).is_ok());
        let start = e.services().now();
        let level = |e: &Engine, i: usize| e.show.fixtures.get(i).and_then(|f| value_of(e, f, Attribute::Intensity));
        // A quarter of the way: the first is high, the second (half a cycle behind) low.
        e.step_effects(start + TimeDelta::milliseconds(500));
        assert_eq!((level(&e, 0), level(&e, 1)), (Some(1.0), Some(0.0)));
        e.step_effects(start + TimeDelta::milliseconds(1500));
        assert_eq!((level(&e, 0), level(&e, 1)), (Some(0.0), Some(1.0)));
        assert_eq!(e.running_effects(), vec!["Chase".to_owned()]);
        assert!(e.run("effect.stop", &Value::Null).is_ok());
        assert!(e.running_effects().is_empty());
        for bad in [
            json!({"name": "X", "group": 0, "attribute": "smoke"}),
            json!({"name": "X", "group": 0, "attribute": "pan_fine"}),
            json!({"name": "X", "group": 0, "attribute": "red", "shape": "zigzag"}),
            json!({"name": "X", "group": 5, "attribute": "red"}),
            json!({"name": " ", "group": 0, "attribute": "red"}),
        ] {
            assert!(e.run("effect.set", &bad).is_err(), "{bad}");
        }
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.effects).unwrap_or_default();
        assert_eq!(back, e.show.effects);
    }

    #[test]
    fn a_beat_effect_follows_the_song_tempo() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        if let Some(id) = e.current_song {
            let _ = e.run("song.set", &json!({"song": id, "bpm": 120.0}));
        }
        let fx = Effect { beats: 4.0, period: 9.0, ..Effect::default() };
        assert!((e.cycle_seconds(&fx) - 2.0).abs() < 1e-9, "4 beats at 120 BPM");
        assert!((e.cycle_seconds(&Effect { beats: 0.0, ..fx }) - 9.0).abs() < 1e-9);
    }
}
