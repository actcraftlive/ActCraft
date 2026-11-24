//! Groups and presets: fixtures picked together by name, and stored looks (each fixture's
//! attribute values) recalled at once. Both refer to fixtures by id, so they survive fixtures
//! being added, moved or removed.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_FIXTURES, MAX_GROUPS, MAX_PRESET_VALUES, MAX_PRESETS};
use serde_json::{Value, json};

pub use actcraft_show::{Group, Preset, PresetValue};

use crate::Engine;
use crate::fixtures::{Attribute, attribute_key, slot_value_of, targets, value_of};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

fn name(prm: &Value) -> Result<String, CommandError> {
    let n = p::str_max(prm, "name", 64)?.trim().to_owned();
    if n.is_empty() {
        return Err(invalid("give it a name"));
    }
    Ok(n)
}

/// The fixtures' current indexes for these ids (those still there).
fn indexes(e: &Engine, ids: &[u64]) -> Vec<usize> {
    ids.iter().filter_map(|id| e.show.fixtures.iter().position(|f| f.id == *id)).collect()
}

/// `group.set {name, fixtures: [index]}`: a group, or the one of that name changed.
pub fn group_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = name(prm)?;
    let picked = p::index_list(prm, "fixtures", e.show.fixtures.len(), MAX_FIXTURES)?;
    let mut ids: Vec<u64> = Vec::new();
    for i in picked {
        if let Some(f) = e.show.fixtures.get(i)
            && !ids.contains(&f.id)
        {
            ids.push(f.id);
        }
    }
    let group = Group { name, fixtures: ids, ..Group::default() };
    let i = match e.show.groups.iter().position(|g| g.name == group.name) {
        Some(i) => {
            if let Some(slot) = e.show.groups.get_mut(i) {
                *slot = group;
            }
            i
        }
        None => {
            if e.show.groups.len() >= MAX_GROUPS {
                return Err(failed("a show holds at most 256 groups"));
            }
            e.show.groups.push(group);
            e.show.groups.len() - 1
        }
    };
    e.mark_changed();
    Ok(json!({"group": i}))
}

/// `group.remove {group}`.
pub fn group_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "group", e.show.groups.len())?;
    e.show.groups.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `group.list`: every group with its fixtures (indexes).
pub fn group_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show.groups.iter().map(|g| json!({"name": g.name, "fixtures": indexes(e, &g.fixtures)})).collect(),
    ))
}

/// `preset.store {name, fixture | fixtures | group?}`: stores the current values of the fixtures'
/// attributes (every fixture without a choice) as a preset; one of that name is replaced.
pub fn preset_store(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = name(prm)?;
    let picked = if prm.get("fixture").is_some() || prm.get("fixtures").is_some() || prm.get("group").is_some() {
        targets(e, prm)?
    } else {
        (0..e.show.fixtures.len()).collect()
    };
    let mut values = Vec::new();
    for i in picked {
        let Some(f) = e.show.fixtures.get(i) else { continue };
        let mut seen = Vec::new();
        for (c, a) in f.channels.iter().copied().enumerate().filter(|(_, a)| !a.is_fine()) {
            // A channel without a named attribute (a dimmer/strobe mode, a colour macro, a speed):
            // a fixture can have several, so each is kept by its number.
            if a == Attribute::Other {
                let channel = u16::try_from(c).ok();
                if let Some(value) = slot_value_of(e, f, a, channel) {
                    values.push(PresetValue { fixture: f.id, attribute: a, value, channel });
                }
                continue;
            }
            if seen.contains(&a) {
                continue;
            }
            seen.push(a);
            if let Some(value) = value_of(e, f, a) {
                values.push(PresetValue { fixture: f.id, attribute: a, value, channel: None });
            }
        }
    }
    if values.len() > MAX_PRESET_VALUES {
        return Err(failed("that preset would hold too many values"));
    }
    let count = values.len();
    let preset = Preset { name, values, ..Preset::default() };
    let i = match e.show.presets.iter().position(|p| p.name == preset.name) {
        Some(i) => {
            if let Some(slot) = e.show.presets.get_mut(i) {
                *slot = preset;
            }
            i
        }
        None => {
            if e.show.presets.len() >= MAX_PRESETS {
                return Err(failed("a show holds at most 1024 presets"));
            }
            e.show.presets.push(preset);
            e.show.presets.len() - 1
        }
    };
    e.mark_changed();
    Ok(json!({"preset": i, "values": count}))
}

/// `preset.recall {preset, fade?}`: sets every stored value again, at once or fading in over
/// `fade` seconds. In blind it goes to blind's copy at once, to be changed and stored again.
pub fn preset_recall(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "preset", e.show.presets.len())?;
    let fade = p::opt_f64_in(prm, "fade", 0.0, actcraft_show::limits::MAX_FADE_S)?.unwrap_or(0.0);
    let Some(preset) = e.show.presets.get(i).cloned() else { return Err(invalid("no such preset")) };
    if e.dmx.blind() {
        let changed = e.recall_edited(&preset);
        return Ok(json!({"changed": changed, "fade": 0.0}));
    }
    let changed = e.fade_to(&preset, fade);
    Ok(json!({"changed": changed, "fade": fade}))
}

/// `preset.remove {preset}`.
pub fn preset_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "preset", e.show.presets.len())?;
    e.show.presets.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `preset.list`: every preset with the fixtures it holds (indexes) and its values.
pub fn preset_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .presets
            .iter()
            .map(|p| {
                let values: Vec<Value> = p
                    .values
                    .iter()
                    .filter_map(|v| {
                        let i = e.show.fixtures.iter().position(|f| f.id == v.fixture)?;
                        let mut value =
                            json!({"fixture": i, "attribute": attribute_key(v.attribute), "value": v.value});
                        if let (Some(c), Some(o)) = (v.channel, value.as_object_mut()) {
                            o.insert("channel".to_owned(), json!(c));
                        }
                        Some(value)
                    })
                    .collect();
                json!({"name": p.name, "values": values})
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "rgb_dimmer", "universe": 1, "count": 3}));
        e
    }

    /// The owner's case: the Betopper LM70SR (14 channels) has six channels without a named
    /// attribute, among them channel 6, which switches the light on (dimmer / strobe / open). A
    /// preset keeps every one of them, and recalling it sets them all again.
    #[test]
    fn a_preset_keeps_every_channel_of_a_moving_head() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let kind = e.run("fixture_type.from_library", &json!({"name": "Betopper LM70SR"})).ok();
        let kind = kind.and_then(|k| k.get("type").cloned()).unwrap_or(json!(0));
        assert!(e.run("fixture.add", &json!({"type": kind, "mode": 0, "universe": 1, "address": 1})).is_ok());
        // Every channel to a level of its own: 1, 2, ... 14.
        let levels: Vec<u8> = (1..=14).collect();
        assert!(e.run("dmx.set", &json!({"universe": 1, "channel": 1, "values": levels})).is_ok());
        let stored = e.run("preset.store", &json!({"name": "On"})).ok();
        let count = stored.as_ref().and_then(|s| s.get("values")).and_then(Value::as_u64);
        // 14 channels less the two fine ones (pan and tilt keep 16 bits in one value each).
        assert_eq!(count, Some(12), "{stored:?}");
        assert!(e.run("dmx.clear", &Value::Null).is_ok());
        assert!(e.run("preset.recall", &json!({"preset": 0})).is_ok());
        let back = e.dmx.universe(1).map(|u| u[..14].to_vec());
        assert_eq!(back, Some(levels), "every channel back, channel 6 (light on) too");
    }

    /// A preset stored before channel numbers were kept still loads and recalls.
    #[test]
    fn an_older_preset_without_channel_numbers_still_loads() {
        let v: Result<PresetValue, _> = serde_json::from_value(json!({"fixture": 1, "attribute": "red", "value": 0.5}));
        assert_eq!(v.ok().map(|v| v.channel), Some(None));
        let back =
            serde_json::to_value(PresetValue { fixture: 1, attribute: Attribute::Red, value: 0.5, channel: None });
        assert_eq!(back.ok(), Some(json!({"fixture": 1, "attribute": "red", "value": 0.5})), "saved as before");
    }

    #[test]
    fn groups_pick_fixtures_and_follow_them() {
        let mut e = engine();
        assert_eq!(e.run("group.set", &json!({"name": "Sides", "fixtures": [0, 2]})).ok(), Some(json!({"group": 0})));
        assert!(e.run("fixture.set", &json!({"group": 0, "values": {"red": 1.0}})).is_ok());
        let red = |e: &Engine, i: usize| e.show.fixtures.get(i).and_then(|f| value_of(e, f, Attribute::Red));
        assert_eq!((red(&e, 0), red(&e, 1), red(&e, 2)), (Some(1.0), Some(0.0), Some(1.0)));
        // Removing the first fixture: the group still holds the last one, now at index 1.
        assert!(e.run("fixture.remove", &json!({"fixture": 0})).is_ok());
        assert_eq!(e.run("group.list", &Value::Null).ok(), Some(json!([{"name": "Sides", "fixtures": [1]}])));
        assert!(e.run("group.set", &json!({"name": " ", "fixtures": [0]})).is_err());
        assert!(e.run("group.set", &json!({"name": "X", "fixtures": [7]})).is_err());
        assert!(e.run("group.remove", &json!({"group": 0})).is_ok());
    }

    #[test]
    fn fixtures_from_older_files_get_ids_and_dangling_references_go() {
        let file = br#"{"format":"actcraft-show","version":1,"name":"Old",
            "fixtures":[{"name":"A","channels":["intensity"],"universe":1,"address":1},
                        {"name":"B","channels":["intensity"],"universe":1,"address":2,"id":7},
                        {"name":"C","channels":["intensity"],"universe":1,"address":3,"id":7}],
            "groups":[{"name":"G","fixtures":[7,99]}]}"#;
        let show = actcraft_show::Show::from_bytes(file);
        assert!(show.is_ok());
        let Ok(show) = show else { return };
        let ids: Vec<u64> = show.fixtures.iter().map(|f| f.id).collect();
        assert!(ids.iter().all(|id| *id != 0), "{ids:?}");
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "unique: {ids:?}");
        assert_eq!(show.groups[0].fixtures, vec![7], "99 is not there");
    }

    #[test]
    fn presets_store_and_recall_a_look() {
        let mut e = engine();
        let _ = e.run("fixture.set", &json!({"fixtures": [0, 1, 2], "values": {"intensity": 1.0, "blue": 0.5}}));
        let r = e.run("preset.store", &json!({"name": "Blue", "fixtures": [0, 1]}));
        assert_eq!(r.ok(), Some(json!({"preset": 0, "values": 8})), "four attributes each");
        let _ = e.run("dmx.clear", &Value::Null);
        assert_eq!(e.run("preset.recall", &json!({"preset": 0})).ok(), Some(json!({"changed": true, "fade": 0.0})));
        let blue = |e: &Engine, i: usize| e.show.fixtures.get(i).and_then(|f| value_of(e, f, Attribute::Blue));
        assert!(blue(&e, 0).is_some_and(|v| (v - 0.5).abs() < 0.01));
        assert_eq!(blue(&e, 2), Some(0.0), "not in the preset");
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.presets).unwrap_or_default();
        assert_eq!(back, e.show.presets);
        assert!(e.run("preset.store", &json!({"name": "Blue"})).is_ok());
        assert_eq!(e.show.presets.len(), 1, "same name: replaced");
        assert_eq!(e.show.presets[0].values.len(), 12, "every fixture without a choice");
        assert!(e.run("preset.recall", &json!({"preset": 3})).is_err());
        assert!(e.run("preset.remove", &json!({"preset": 0})).is_ok());
    }
}
