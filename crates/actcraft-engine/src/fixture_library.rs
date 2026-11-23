//! Fixture types from Open Fixture Library files (the open, MIT-licensed fixture format: a JSON
//! file per fixture with its channels and modes). Each channel's capability becomes the
//! attribute ActCraft drives it by; fine channels follow the channel they refine. JSON is read
//! by `serde_json`; this only maps what it finds, bounded, onto the show's fixture types.

use std::collections::HashMap;

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_FIXTURE_CHANNELS, MAX_FIXTURE_MODES, MAX_FIXTURE_TYPES};
use serde_json::{Value, json};

pub use actcraft_show::{FixtureMode, FixtureType};

use crate::Engine;
use crate::fixtures::Attribute;

/// Largest fixture file read.
const MAX_BYTES: usize = 2 << 20;
/// Most channels of a file looked at.
const MAX_CHANNELS_LOOKED_AT: usize = 1_024;
/// Extensions of fixture files.
pub const EXTENSIONS: &[&str] = &["json"];

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// The attribute a colour of `ColorIntensity` is.
fn colour(name: &str) -> Attribute {
    match name.to_ascii_lowercase().as_str() {
        "red" => Attribute::Red,
        "green" => Attribute::Green,
        "blue" => Attribute::Blue,
        "white" | "warm white" | "cold white" => Attribute::White,
        "amber" => Attribute::Amber,
        "uv" => Attribute::Uv,
        _ => Attribute::Other,
    }
}

/// What a channel controls, from its capability (or the first of its capabilities that does
/// something).
fn attribute_of(channel_name: &str, def: &Value) -> Attribute {
    let caps: Vec<&Value> = match (def.get("capability"), def.get("capabilities").and_then(Value::as_array)) {
        (Some(c), _) => vec![c],
        (None, Some(list)) => list.iter().take(256).collect(),
        _ => Vec::new(),
    };
    let Some(cap) = caps.into_iter().find(|c| c.get("type").and_then(Value::as_str).is_some_and(|t| t != "NoFunction"))
    else {
        return Attribute::Other;
    };
    let kind = cap.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "Intensity" => Attribute::Intensity,
        "ColorIntensity" => colour(cap.get("color").and_then(Value::as_str).unwrap_or_default()),
        "Pan" => Attribute::Pan,
        "Tilt" => Attribute::Tilt,
        "ShutterStrobe" | "StrobeSpeed" | "StrobeDuration" => Attribute::Strobe,
        "ColorPreset" => Attribute::ColourWheel,
        "WheelSlot" | "WheelShake" | "WheelSlotRotation" | "WheelRotation" => {
            let wheel = cap.get("wheel").and_then(Value::as_str).unwrap_or(channel_name).to_ascii_lowercase();
            if wheel.contains("gobo") {
                Attribute::Gobo
            } else if wheel.contains("colo") {
                Attribute::ColourWheel
            } else {
                Attribute::Other
            }
        }
        "Zoom" => Attribute::Zoom,
        "Focus" => Attribute::Focus,
        _ => Attribute::Other,
    }
}

/// A fixture type from an Open Fixture Library fixture; modes that use matrix blocks (pixel
/// templates) or have more than 64 channels are left out.
pub fn read_ofl(doc: &Value) -> Result<FixtureType, CommandError> {
    let not_ofl = || failed("that file is not an Open Fixture Library fixture");
    let name = doc.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or_else(not_ofl)?;
    let available = doc.get("availableChannels").and_then(Value::as_object).ok_or_else(not_ofl)?;
    let mut by_name: HashMap<&str, Attribute> = HashMap::new();
    for (channel, def) in available.iter().take(MAX_CHANNELS_LOOKED_AT) {
        let attribute = attribute_of(channel, def);
        by_name.insert(channel.as_str(), attribute);
        let fines = def.get("fineChannelAliases").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        for (k, alias) in fines.iter().take(4).enumerate() {
            if let Some(alias) = alias.as_str() {
                // 16 bits is what ActCraft drives; a third or fourth byte is left alone.
                let fine = if k == 0 { attribute.fine().unwrap_or(Attribute::Other) } else { Attribute::Other };
                by_name.insert(alias, fine);
            }
        }
    }
    let mut modes = Vec::new();
    for mode in doc.get("modes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        if modes.len() >= MAX_FIXTURE_MODES {
            break;
        }
        let Some(list) = mode.get("channels").and_then(Value::as_array) else { continue };
        if list.is_empty() || list.len() > MAX_FIXTURE_CHANNELS {
            continue;
        }
        let channels: Option<Vec<Attribute>> = list
            .iter()
            .map(|c| match c {
                Value::String(s) => Some(by_name.get(s.as_str()).copied().unwrap_or(Attribute::Other)),
                Value::Null => Some(Attribute::Other),
                _ => None,
            })
            .collect();
        let Some(channels) = channels else { continue };
        let mode_name = mode.get("name").and_then(Value::as_str).unwrap_or_default().trim();
        let mode_name =
            if mode_name.is_empty() { format!("{} ch", channels.len()) } else { mode_name.chars().take(64).collect() };
        modes.push(FixtureMode { name: mode_name, channels, ..FixtureMode::default() });
    }
    if modes.is_empty() {
        return Err(failed("that fixture has no mode ActCraft can patch"));
    }
    Ok(FixtureType { name: name.chars().take(64).collect(), modes, ..FixtureType::default() })
}

/// `fixture_type.import {path}`: brings a fixture type in from an Open Fixture Library file
/// (`.json`); one of the same name is replaced.
pub fn fixture_type_import(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", 4096)?;
    if !crate::services::extension(path).is_some_and(|ext| EXTENSIONS.contains(&ext.as_str())) {
        return Err(invalid("ActCraft reads fixture types from Open Fixture Library .json files"));
    }
    let bytes = e.services().read_file(path).map_err(failed)?;
    if bytes.len() > MAX_BYTES {
        return Err(failed("that fixture file is too large"));
    }
    let doc: Value =
        serde_json::from_slice(&bytes).map_err(|_| failed("that file is not an Open Fixture Library fixture"))?;
    let ty = read_ofl(&doc)?;
    let modes: Vec<String> = ty.modes.iter().map(|m| m.name.clone()).collect();
    let name = ty.name.clone();
    let index = match e.show.fixture_types.iter().position(|t| t.name == ty.name) {
        Some(i) => {
            if let Some(slot) = e.show.fixture_types.get_mut(i) {
                *slot = ty;
            }
            i
        }
        None => {
            if e.show.fixture_types.len() >= MAX_FIXTURE_TYPES {
                return Err(failed("a show holds at most 256 fixture types"));
            }
            e.show.fixture_types.push(ty);
            e.show.fixture_types.len() - 1
        }
    };
    e.mark_changed();
    Ok(json!({"type": index, "name": name, "modes": modes}))
}

/// `fixture_type.remove {type}`: patched fixtures keep their channels.
pub fn fixture_type_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "type", e.show.fixture_types.len())?;
    e.show.fixture_types.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `fixture_type.list`: every imported type with its modes.
pub fn fixture_type_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .fixture_types
            .iter()
            .map(|t| {
                let modes: Vec<Value> = t
                    .modes
                    .iter()
                    .map(|m| {
                        let channels: Vec<&str> =
                            m.channels.iter().map(|a| crate::fixtures::attribute_key(*a)).collect();
                        json!({"name": m.name, "channels": channels})
                    })
                    .collect();
                json!({"name": t.name, "modes": modes})
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up fixture in the Open Fixture Library layout (our own test data).
    fn sample() -> Value {
        json!({
            "name": "Test Spot 250",
            "availableChannels": {
                "Pan": {"fineChannelAliases": ["Pan fine"], "capability": {"type": "Pan", "angleStart": "0deg", "angleEnd": "540deg"}},
                "Tilt": {"fineChannelAliases": ["Tilt fine"], "capability": {"type": "Tilt"}},
                "Dimmer": {"fineChannelAliases": ["Dimmer fine", "Dimmer extra fine"], "capability": {"type": "Intensity"}},
                "Red": {"capability": {"type": "ColorIntensity", "color": "Red"}},
                "Warm": {"capability": {"type": "ColorIntensity", "color": "Warm White"}},
                "Shutter": {"capabilities": [
                    {"dmxRange": [0, 9], "type": "NoFunction"},
                    {"dmxRange": [10, 255], "type": "ShutterStrobe", "shutterEffect": "Strobe"}
                ]},
                "Gobo Wheel": {"capabilities": [{"dmxRange": [0, 255], "type": "WheelSlot", "slotNumber": 1}]},
                "Macro": {"capability": {"type": "Effect", "effectName": "Auto"}}
            },
            "modes": [
                {"name": "Basic", "channels": ["Pan", "Tilt", "Dimmer", "Red", "Warm", "Shutter", "Gobo Wheel"]},
                {"name": "Extended", "channels": ["Pan", "Pan fine", "Tilt", "Tilt fine", "Dimmer", "Dimmer fine", "Dimmer extra fine", null, "Macro"]},
                {"name": "Pixels", "channels": ["Dimmer", {"insert": "matrixChannels", "repeatFor": "eachPixelABC"}]}
            ]
        })
    }

    #[test]
    fn capabilities_become_attributes_and_fine_channels_follow() {
        use Attribute as A;
        let t = read_ofl(&sample());
        assert!(t.is_ok(), "{t:?}");
        let Ok(t) = t else { return };
        assert_eq!(t.name, "Test Spot 250");
        assert_eq!(t.modes.len(), 2, "the matrix mode is left out");
        assert_eq!(t.modes[0].channels, vec![A::Pan, A::Tilt, A::Intensity, A::Red, A::White, A::Strobe, A::Gobo]);
        assert_eq!(
            t.modes[1].channels,
            vec![
                A::Pan,
                A::PanFine,
                A::Tilt,
                A::TiltFine,
                A::Intensity,
                A::IntensityFine,
                A::Other,
                A::Other,
                A::Other
            ]
        );
        for bad in
            [json!({}), json!({"name": "x"}), json!({"name": "x", "availableChannels": {}, "modes": []}), json!([1, 2])]
        {
            assert!(read_ofl(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_imported_type_is_patched_by_mode() {
        let mut services = crate::MemoryServices::default();
        let bytes = serde_json::to_vec(&sample()).unwrap_or_default();
        services.files.insert("/fixtures/spot.json".into(), bytes);
        services.files.insert("/fixtures/notes.txt".into(), b"hello".to_vec());
        services.files.insert("/fixtures/broken.json".into(), b"{".to_vec());
        let mut e = Engine::new(Box::new(services));
        let r = e.run("fixture_type.import", &json!({"path": "/fixtures/spot.json"}));
        assert_eq!(r.ok(), Some(json!({"type": 0, "name": "Test Spot 250", "modes": ["Basic", "Extended"]})));
        assert!(e.run("fixture_type.import", &json!({"path": "/fixtures/spot.json"})).is_ok());
        assert_eq!(e.show.fixture_types.len(), 1, "imported again: replaced");
        for path in ["/fixtures/notes.txt", "/fixtures/broken.json", "/fixtures/missing.json"] {
            assert!(e.run("fixture_type.import", &json!({"path": path})).is_err(), "{path}");
        }
        let r = e.run("fixture.add", &json!({"type": 0, "mode": 1, "universe": 1, "count": 2}));
        assert_eq!(r.ok(), Some(json!({"fixtures": [0, 1]})));
        assert_eq!(e.show.fixtures[1].address, 10, "nine channels each");
        assert_eq!(e.show.fixtures[0].kind, "Test Spot 250 \u{b7} Extended");
        assert!(e.run("fixture.add", &json!({"type": 0, "mode": 5, "universe": 1})).is_err());
        assert!(e.run("fixture_type.remove", &json!({"type": 0})).is_ok());
        assert_eq!(e.show.fixtures.len(), 2, "patched fixtures stay");
    }
}
