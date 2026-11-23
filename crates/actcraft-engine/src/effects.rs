//! Effects: a group's attribute moved in a wave (sine, square, saw, triangle) between two
//! values, cycle by cycle, each fixture of the group a little later than the one before. A
//! cycle is seconds long, or a number of beats at the playing song's tempo. Running effects are
//! moved on by the engine's tick; they are not saved as running.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_EFFECT_PARTS, MAX_EFFECTS};
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

pub use actcraft_show::{Effect, EffectPart, Shape};

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
            let parts: Vec<(Attribute, f64, f64, f64)> = std::iter::once((fx.attribute, fx.low, fx.high, 0.0))
                .chain(fx.also.iter().map(|p| (p.attribute, p.low, p.high, p.offset)))
                .collect();
            for (k, id) in ids.iter().enumerate() {
                let phase = base - fx.spread * k as f64 / n;
                for (attribute, low, high, offset) in &parts {
                    let value = low + (high - low) * fx.shape.at(phase - offset);
                    self.put(*id, *attribute, value);
                }
            }
        }
    }

    /// The names of the effects running now.
    pub fn running_effects(&self) -> Vec<String> {
        self.running_effects.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// The `also` param: more attributes, each `{attribute, low?, high?, offset?}`.
fn parts(prm: &Value) -> Result<Vec<EffectPart>, CommandError> {
    let Some(list) = prm.get("also") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| invalid("`also` must be a list of attributes"))?;
    if list.len() > MAX_EFFECT_PARTS {
        return Err(invalid("an effect moves at most 8 more attributes"));
    }
    list.iter()
        .map(|part| {
            let attribute = attribute_named(p::str_max(part, "attribute", 32)?)
                .filter(|a| !a.is_fine())
                .ok_or_else(|| invalid("`attribute` must be an attribute like intensity, red or pan"))?;
            Ok(EffectPart {
                attribute,
                low: p::opt_f64_in(part, "low", 0.0, 1.0)?.unwrap_or(0.0),
                high: p::opt_f64_in(part, "high", 0.0, 1.0)?.unwrap_or(1.0),
                offset: p::opt_f64_in(part, "offset", 0.0, 1.0)?.unwrap_or(0.0),
                ..EffectPart::default()
            })
        })
        .collect()
}

/// The `also` parts as params and in lists.
fn parts_json(parts: &[EffectPart]) -> Value {
    parts
        .iter()
        .map(|p| json!({"attribute": attribute_key(p.attribute), "low": p.low, "high": p.high, "offset": p.offset}))
        .collect()
}

/// `effect.set {name, group, attribute, shape?, period?, beats?, low?, high?, spread?, also?}`: an
/// effect, or the one of that name changed.
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
        also: parts(prm)?,
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

/// A ready-made effect: its key, its name and how it moves (no group yet).
pub struct Template {
    pub key: &'static str,
    pub name: &'static str,
    pub attribute: Attribute,
    pub shape: Shape,
    pub period: f64,
    pub beats: f64,
    pub low: f64,
    pub high: f64,
    pub spread: f64,
    /// More attributes and how much later in the cycle each moves (between 0 and 1).
    pub also: &'static [(Attribute, f64)],
}

use actcraft_show::Attribute;

const fn template(
    key: &'static str,
    name: &'static str,
    attribute: Attribute,
    shape: Shape,
    period: f64,
    beats: f64,
    spread: f64,
) -> Template {
    Template { key, name, attribute, shape, period, beats, low: 0.0, high: 1.0, spread, also: &[] }
}

/// Ready-made effects. None flashes faster than a few times a second (people sensitive to
/// flashing light are in audiences too).
pub const TEMPLATES: &[Template] = &[
    template("breathe", "Breathe", Attribute::Intensity, Shape::Sine, 4.0, 0.0, 0.0),
    template("beat_pulse", "Pulse on the beat", Attribute::Intensity, Shape::Sine, 1.0, 1.0, 0.0),
    template("chase", "Chase", Attribute::Intensity, Shape::Square, 1.0, 2.0, 1.0),
    template("wave", "Wave", Attribute::Intensity, Shape::Sine, 2.0, 0.0, 1.0),
    template("build", "Build over a bar", Attribute::Intensity, Shape::Saw, 4.0, 4.0, 0.0),
    template("bounce", "Bounce", Attribute::Intensity, Shape::Triangle, 2.0, 2.0, 0.5),
    template("red_pulse", "Red pulse", Attribute::Red, Shape::Sine, 2.0, 0.0, 0.0),
    template("blue_wave", "Blue wave", Attribute::Blue, Shape::Sine, 3.0, 0.0, 1.0),
    template("warm_drift", "Warm drift", Attribute::Amber, Shape::Sine, 6.0, 0.0, 0.5),
    template("sweep", "Sweep", Attribute::Pan, Shape::Triangle, 4.0, 0.0, 0.0),
    template("fan", "Fan out", Attribute::Pan, Shape::Sine, 4.0, 0.0, 1.0),
    template("nod", "Nod", Attribute::Tilt, Shape::Sine, 2.0, 0.0, 0.25),
    template("tilt_wave", "Tilt wave", Attribute::Tilt, Shape::Triangle, 4.0, 0.0, 1.0),
    template("zoom_breathe", "Zoom in and out", Attribute::Zoom, Shape::Sine, 4.0, 0.0, 0.0),
    Template {
        also: &[(Attribute::Tilt, 0.25)],
        ..template("circle", "Circle", Attribute::Pan, Shape::Sine, 4.0, 0.0, 0.0)
    },
    Template {
        also: &[(Attribute::Tilt, 0.25)],
        ..template("circle_wave", "Circles one after another", Attribute::Pan, Shape::Sine, 4.0, 0.0, 1.0)
    },
    Template {
        also: &[(Attribute::Green, 1.0 / 3.0), (Attribute::Blue, 2.0 / 3.0)],
        ..template("rainbow", "Rainbow", Attribute::Red, Shape::Sine, 6.0, 0.0, 0.0)
    },
    Template {
        also: &[(Attribute::Green, 1.0 / 3.0), (Attribute::Blue, 2.0 / 3.0)],
        ..template("rainbow_wave", "Rainbow wave", Attribute::Red, Shape::Sine, 6.0, 0.0, 1.0)
    },
];

/// A template's more attributes as `also` params (each between 0 and 1).
fn template_parts(t: &Template) -> Value {
    t.also
        .iter()
        .map(|(a, offset)| json!({"attribute": attribute_key(*a), "low": 0.0, "high": 1.0, "offset": offset}))
        .collect()
}

/// `effect.templates`: the ready-made effects.
pub fn effect_templates(_e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        TEMPLATES
            .iter()
            .map(|t| {
                json!({"template": t.key, "name": t.name, "attribute": attribute_key(t.attribute), "shape": shape_key(t.shape),
                    "period": t.period, "beats": t.beats, "spread": t.spread, "also": template_parts(t)})
            })
            .collect(),
    ))
}

/// `effect.from_template {template, group, name?}`: an effect from a ready-made one, on a group.
pub fn effect_from_template(e: &mut Engine, prm: &Value) -> CommandResult {
    let key = p::str_max(prm, "template", 32)?;
    let t = TEMPLATES.iter().find(|t| t.key == key).ok_or_else(|| invalid("no such template"))?;
    let g = p::index(prm, "group", e.show.groups.len())?;
    let group = e.show.groups.get(g).map(|g| g.name.clone()).unwrap_or_default();
    let name = p::opt_str_max(prm, "name", 64)?
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| format!("{} \u{b7} {group}", t.name), str::to_owned);
    let fx = json!({"name": name, "group": g, "attribute": attribute_key(t.attribute), "shape": shape_key(t.shape),
        "period": t.period, "beats": t.beats, "low": t.low, "high": t.high, "spread": t.spread, "also": template_parts(t)});
    effect_set(e, &fx)
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
                    "period": f.period, "beats": f.beats, "low": f.low, "high": f.high, "spread": f.spread, "also": parts_json(&f.also),
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
    fn templates_make_effects_on_a_group() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1, "count": 2}));
        let _ = e.run("group.set", &json!({"name": "Heads", "fixtures": [0, 1]}));
        assert_eq!(
            e.run("effect.from_template", &json!({"template": "fan", "group": 0})).ok(),
            Some(json!({"effect": 0}))
        );
        let fx = e.show.effects.first().cloned().unwrap_or_default();
        assert_eq!(
            (fx.name.as_str(), fx.group.as_str(), fx.attribute),
            ("Fan out \u{b7} Heads", "Heads", Attribute::Pan)
        );
        assert!(e.run("effect.from_template", &json!({"template": "lasers", "group": 0})).is_err());
        let list = e.run("effect.templates", &Value::Null).ok().unwrap_or_default();
        assert_eq!(list.as_array().map(Vec::len), Some(TEMPLATES.len()));
        // Every template is a valid effect on the group, and none flashes faster than 3 times a second.
        for t in TEMPLATES {
            assert!(e.run("effect.from_template", &json!({"template": t.key, "group": 0})).is_ok(), "{}", t.key);
            let shortest = if t.beats > 0.0 { t.beats * 60.0 / 200.0 } else { t.period };
            assert!(t.shape != Shape::Square || shortest >= 1.0 / 3.0, "{} flashes too fast", t.key);
        }
    }

    #[test]
    fn an_effect_moves_more_attributes_each_later_by_its_offset() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1}));
        let _ = e.run("group.set", &json!({"name": "Head", "fixtures": [0]}));
        assert!(e.run("effect.from_template", &json!({"template": "circle", "group": 0})).is_ok());
        let fx = e.show.effects.first().cloned().unwrap_or_default();
        assert_eq!(fx.also.iter().map(|p| (p.attribute, p.offset)).collect::<Vec<_>>(), vec![(Attribute::Tilt, 0.25)]);
        let start = e.services().now();
        assert!(e.run("effect.start", &json!({"effect": 0})).is_ok());
        // Half the 4 s cycle in: pan is at its top, tilt (a quarter of a cycle behind) half way up.
        e.step_effects(start + TimeDelta::milliseconds(2000));
        let f = e.show.fixtures[0].clone();
        let (pan, tilt) = (value_of(&e, &f, Attribute::Pan), value_of(&e, &f, Attribute::Tilt));
        assert!(pan.is_some_and(|v| v > 0.99), "{pan:?}");
        assert!(tilt.is_some_and(|v| (v - 0.5).abs() < 0.01), "{tilt:?}");
        // Saved and loaded with its parts; refused when wrong.
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.effects).unwrap_or_default();
        assert_eq!(back, e.show.effects);
        let set = |e: &mut Engine, also: Value| {
            e.run("effect.set", &json!({"name": "X", "group": 0, "attribute": "red", "also": also}))
        };
        assert!(set(&mut e, json!([{"attribute": "green", "offset": 0.5, "low": 0.2}])).is_ok());
        assert!(set(&mut e, json!([{"attribute": "lasers"}])).is_err());
        assert!(set(&mut e, json!([{"attribute": "green", "offset": 2.0}])).is_err());
        assert!(set(&mut e, json!("green")).is_err());
        assert!(set(&mut e, Value::Array(vec![json!({"attribute": "green"}); 9])).is_err());
        let listed = e.run("effect.list", &Value::Null).ok().unwrap_or_default();
        assert_eq!(listed[1]["also"], json!([{"attribute": "green", "low": 0.2, "high": 1.0, "offset": 0.5}]));
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
