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
pub const EXTENSIONS: &[&str] = &["json", "r20"];

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
/// (`.json`) or an `.r20` file; one of the same name is replaced.
pub fn fixture_type_import(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", 4096)?;
    let ext = crate::services::extension(path).unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(invalid(
            "ActCraft reads fixture types from ActCraft and Open Fixture Library .json files and .r20 files",
        ));
    }
    let bytes = e.services().read_file(path).map_err(failed)?;
    if bytes.len() > MAX_BYTES {
        return Err(failed("that fixture file is too large"));
    }
    if ext == "r20" {
        let ty = read_r20(&String::from_utf8_lossy(&bytes))?;
        return keep(e, ty);
    }
    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|_| failed("that file is not an ActCraft or Open Fixture Library fixture"))?;
    // ActCraft's own fixture files (exported or from the library) go in as they are.
    if doc["format"].as_str() == Some(FORMAT) {
        return fixture_type_set(e, &doc);
    }
    let ty = read_ofl(&doc)?;
    keep(e, ty)
}

/// What an `.r20` attribute letter is (the letters the file format's own comments list), or by
/// the channel's name for the ones they leave open (colour mixing).
fn r20_attribute(letter: &str, name: &str) -> Attribute {
    let n = name.to_ascii_lowercase();
    match letter {
        "E" => return Attribute::Pan,
        "F" => return Attribute::Tilt,
        "H" => return Attribute::Intensity,
        "A" | "N" => return Attribute::ColourWheel,
        "I" | "J" => return Attribute::Gobo,
        "L" => return Attribute::Focus,
        "O" => return Attribute::Strobe,
        "Q" => return Attribute::Zoom,
        _ => {}
    }
    const NAMED: &[(&str, Attribute)] = &[
        ("red", Attribute::Red),
        ("green", Attribute::Green),
        ("blue", Attribute::Blue),
        ("white", Attribute::White),
        ("amber", Attribute::Amber),
        ("uv", Attribute::Uv),
        ("dimmer", Attribute::Intensity),
        ("strobe", Attribute::Strobe),
        ("shutter", Attribute::Strobe),
        ("zoom", Attribute::Zoom),
        ("focus", Attribute::Focus),
        ("gobo", Attribute::Gobo),
    ];
    NAMED.iter().find(|(word, _)| n.contains(word)).map_or(Attribute::Other, |(_, a)| *a)
}

/// Reads a fixture type from an `.r20` text file: `DEVICE` (its name), `TYPE` (the middle number
/// is how many DMX channels it uses) and the `DMX` … `END` table, one line per channel: bank,
/// fader, channel type (`s`/`S` for 16 bits), DMX offset, level, curve, attribute letter, checksum
/// and the quoted name. A 16-bit channel's next line with bank 0 is its fine channel. Value
/// ranges are not in the format; they can be added to the type afterwards.
pub fn read_r20(text: &str) -> Result<FixtureType, CommandError> {
    let not_r20 = || failed("that file is not an .r20 fixture file");
    let mut name = String::new();
    let mut count = 0usize;
    let mut table = false;
    // (offset, attribute, 16-bit, bank 0)
    let mut rows: Vec<(usize, Attribute, bool, bool)> = Vec::new();
    for line in text.lines().take(4_000) {
        let l = line.trim();
        if l.is_empty() || l.starts_with(';') {
            continue;
        }
        let mut words = l.split_whitespace();
        let first = words.next().unwrap_or_default().to_ascii_uppercase();
        if table {
            if first == "END" {
                table = false;
                continue;
            }
            let (before, quoted) = l.split_once('"').unwrap_or((l, ""));
            let label = quoted.split('"').next().unwrap_or_default();
            let cols: Vec<&str> = before.split_whitespace().collect();
            let (Some(bank), Some(kind), Some(offset), Some(letter)) =
                (cols.first(), cols.get(2), cols.get(3).and_then(|o| o.parse::<usize>().ok()), cols.get(6))
            else {
                continue;
            };
            let sixteen = kind.starts_with(['s', 'S']);
            let fine = *bank == "0";
            let attribute = match rows.last() {
                // The fine channel of the 16-bit channel just before it.
                Some((_, coarse, true, false)) if fine => coarse.fine().unwrap_or(Attribute::Other),
                _ => r20_attribute(letter, label),
            };
            if rows.len() < MAX_FIXTURE_CHANNELS {
                rows.push((offset, attribute, sixteen, fine));
            }
            continue;
        }
        match first.as_str() {
            "DEVICE" => name = words.collect::<Vec<_>>().join(" "),
            "TYPE" => count = words.nth(1).and_then(|n| n.parse().ok()).unwrap_or(0),
            "DMX" => table = true,
            _ => {}
        }
    }
    if name.is_empty() || rows.is_empty() || count == 0 || count > MAX_FIXTURE_CHANNELS {
        return Err(not_r20());
    }
    let mut channels = vec![Attribute::Other; count];
    for (offset, attribute, _, _) in rows {
        if let Some(slot) = offset.checked_sub(1).and_then(|i| channels.get_mut(i)) {
            *slot = attribute;
        }
    }
    let mode = FixtureMode { name: format!("{count} channels"), channels, ..FixtureMode::default() };
    Ok(FixtureType { name: name.chars().take(64).collect(), modes: vec![mode], ..FixtureType::default() })
}

/// Puts a fixture type into the show, in place of one with the same name.
fn keep(e: &mut Engine, ty: FixtureType) -> CommandResult {
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

/// `fixture_type.set {name, modes: [{name, channels: [attribute, ...]}]}`: makes a fixture type by
/// hand, or replaces the one of that name.
pub fn fixture_type_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim().to_owned();
    if name.is_empty() {
        return Err(invalid("a fixture type needs a name"));
    }
    let list = prm.get("modes").and_then(Value::as_array).ok_or_else(|| invalid("`modes` must be a list of modes"))?;
    if list.is_empty() || list.len() > MAX_FIXTURE_MODES {
        return Err(invalid("a fixture type has 1 to 32 modes"));
    }
    let mut modes = Vec::new();
    for m in list {
        let mode_name = m.get("name").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let channels =
            m.get("channels").and_then(Value::as_array).ok_or_else(|| invalid("each mode needs `channels`"))?;
        if channels.is_empty() || channels.len() > MAX_FIXTURE_CHANNELS {
            return Err(invalid("a fixture has 1 to 64 channels"));
        }
        let channels = channels
            .iter()
            .map(|c| c.as_str().and_then(crate::fixtures::attribute_named))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("`channels` must be a list of attributes"))?;
        let mode_name =
            if mode_name.is_empty() { format!("{} ch", channels.len()) } else { mode_name.chars().take(64).collect() };
        let ranges = ranges_param(m, channels.len())?;
        modes.push(FixtureMode { name: mode_name, channels, ranges, ..FixtureMode::default() });
    }
    keep(e, FixtureType { name, modes, ..FixtureType::default() })
}

/// A mode's `ranges`: `[{channel (from 1), from, to, name}]`, each on one of its channels.
fn ranges_param(mode: &Value, channels: usize) -> Result<Vec<actcraft_show::ChannelRange>, CommandError> {
    let Some(list) = mode.get("ranges") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| invalid("`ranges` must be a list of channel ranges"))?;
    if list.len() > actcraft_show::limits::MAX_CHANNEL_RANGES {
        return Err(invalid("a mode has at most 256 ranges"));
    }
    list.iter()
        .map(|r| {
            let channel = p::i64_in(r, "channel", 1, i64::try_from(channels).unwrap_or(1))?;
            let from = p::i64_in(r, "from", 0, 255)?;
            let to = p::i64_in(r, "to", 0, 255)?;
            if from > to {
                return Err(invalid("a range goes from its lower level to its higher one"));
            }
            let name = p::str_max(r, "name", 64)?.trim().to_owned();
            if name.is_empty() {
                return Err(invalid("give each range a name"));
            }
            Ok(actcraft_show::ChannelRange {
                channel: u16::try_from(channel).unwrap_or(1),
                from: u8::try_from(from).unwrap_or(0),
                to: u8::try_from(to).unwrap_or(255),
                name,
                ..Default::default()
            })
        })
        .collect()
}

/// `fixture_type.remove {type}`: patched fixtures keep their channels.
/// The marker of ActCraft's own fixture type files: the type as `fixture_type.set` takes it.
pub const FORMAT: &str = "actcraft-fixture-type";

/// The fixture types that come with ActCraft, in ActCraft's own fixture format (their channel
/// layouts and ranges from the makers' manuals).
const BUILT_IN: &str = include_str!("../fixtures/library.json");

/// The library's types, each as an ActCraft fixture file holds it.
pub fn library() -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(BUILT_IN).unwrap_or_default()
}

/// A fixture type in ActCraft's own fixture format, which `fixture_type.set` and the import read.
pub fn type_json(ty: &FixtureType) -> Value {
    let modes: Vec<Value> = ty
        .modes
        .iter()
        .map(|m| {
            let channels: Vec<&str> = m.channels.iter().map(|a| crate::fixtures::attribute_key(*a)).collect();
            let ranges: Vec<Value> = m
                .ranges
                .iter()
                .map(|r| json!({"channel": r.channel, "from": r.from, "to": r.to, "name": r.name}))
                .collect();
            json!({"name": m.name, "channels": channels, "ranges": ranges})
        })
        .collect();
    json!({"format": FORMAT, "version": 1, "name": ty.name, "modes": modes})
}

/// `fixture_type.export {type, path}`: writes a fixture type to a file (`.json`) another show or
/// another computer can import.
pub fn fixture_type_export(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "type", e.show.fixture_types.len())?;
    let path = p::str_max(prm, "path", 4096)?.to_owned();
    if crate::services::extension(&path).as_deref() != Some("json") {
        return Err(invalid("a fixture type is written as a .json file"));
    }
    let Some(ty) = e.show.fixture_types.get(i) else { return Err(invalid("no such fixture type")) };
    let text = serde_json::to_string_pretty(&type_json(ty)).map_err(|err| failed(err.to_string()))?;
    e.services_mut().write_file(&path, text.as_bytes()).map_err(failed)?;
    Ok(json!({"path": path}))
}

/// `fixture_library.list`: the fixture types that come with ActCraft, with their modes.
pub fn fixture_library_list(_e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        library()
            .iter()
            .map(|t| {
                let modes: Vec<Value> = t["modes"]
                    .as_array()
                    .map(|ms| {
                        ms.iter()
                            .map(|m| json!({"name": m["name"], "channels": m["channels"].as_array().map_or(0, Vec::len)}))
                            .collect()
                    })
                    .unwrap_or_default();
                json!({"name": t["name"], "modes": modes})
            })
            .collect(),
    ))
}

/// `fixture_type.from_library {name}`: puts a type from ActCraft's library into the show (in place
/// of one of the same name).
pub fn fixture_type_from_library(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 64)?.trim();
    let Some(doc) = library().into_iter().find(|t| t["name"].as_str() == Some(name)) else {
        return Err(invalid(format!("the library has no fixture type called `{name}`")));
    };
    fixture_type_set(e, &doc)
}

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
                        let ranges: Vec<Value> = m
                            .ranges
                            .iter()
                            .map(|r| json!({"channel": r.channel, "from": r.from, "to": r.to, "name": r.name}))
                            .collect();
                        json!({"name": m.name, "channels": channels, "ranges": ranges})
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
    fn a_type_made_by_hand_replaces_one_of_its_name() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let par = json!({"name": "Par", "modes": [{"name": "3 ch", "channels": ["red", "green", "blue"]}, {"channels": ["intensity"]}]});
        assert_eq!(e.run("fixture_type.set", &par).ok().map(|v| v["modes"].clone()), Some(json!(["3 ch", "1 ch"])));
        let again = json!({"name": "Par", "modes": [{"name": "Dim", "channels": ["intensity", "strobe"]}]});
        assert!(e.run("fixture_type.set", &again).is_ok());
        assert_eq!(e.show.fixture_types.len(), 1);
        assert_eq!(e.show.fixture_types[0].modes[0].channels, vec![Attribute::Intensity, Attribute::Strobe]);
        for bad in [
            json!({"name": " ", "modes": [{"channels": ["red"]}]}),
            json!({"name": "X", "modes": []}),
            json!({"name": "X", "modes": [{"channels": []}]}),
            json!({"name": "X", "modes": [{"channels": ["smoke"]}]}),
            json!({"name": "X"}),
        ] {
            assert!(e.run("fixture_type.set", &bad).is_err(), "{bad}");
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

    #[test]
    fn an_r20_file_becomes_a_fixture_type() {
        // Written for this test in the format the files describe in their own comments.
        let text = "; a comment\r\nDEVICE  Spot_6ch\r\nTYPE 1   6   M\r\nMIRROR  2   540   270\r\n\
            DMX\r\n\
            1    1   H   6     100   1N   H   1   \"Dimmer\"   255  0  0\r\n\
            3    1   s   3     100   1N   F   1   \"Tilt\"     128  0  0\r\n\
            0    0   L   4     100   1N   F   1   \"\"         128  0  0\r\n\
            3   61   s   1     100   1N   E   1   \"Pan\"      128  0  0\r\n\
            0    0   L   2     100   1N   E   1   \"\"         128  0  0\r\n\
            4    1   L   5     100   1N   T   1   \"Red\"      0    0  0\r\n\
            END\r\n";
        let ty = read_r20(text);
        assert!(ty.is_ok(), "{:?}", ty.err());
        let Ok(ty) = ty else { return };
        assert_eq!(ty.name, "Spot_6ch");
        assert_eq!(
            ty.modes[0].channels,
            vec![
                Attribute::Pan,
                Attribute::PanFine,
                Attribute::Tilt,
                Attribute::TiltFine,
                Attribute::Red,
                Attribute::Intensity
            ]
        );
        assert!(read_r20("just some text").is_err());
        assert!(read_r20("DEVICE X\nTYPE 1 999 M\nDMX\n1 1 L 1 100 1N H 1 \"D\" 0 0 0\nEND").is_err(), "too many");
        // Through the command, by its extension in either case.
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.services_mut().write_file("/f/Spot.R20", text.as_bytes());
        assert_eq!(
            e.run("fixture_type.import", &json!({"path": "/f/Spot.R20"})).ok().map(|v| v["name"].clone()),
            Some(json!("Spot_6ch"))
        );
    }

    #[test]
    fn the_library_type_goes_into_a_show_and_out_and_back_with_its_ranges() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let names = e.run("fixture_library.list", &Value::Null).ok().unwrap_or_default();
        assert!(names.as_array().is_some_and(|l| l.iter().any(|t| t["name"] == "Betopper LM70SR")));
        assert!(e.run("fixture_type.from_library", &json!({"name": "Nothing Like It"})).is_err());
        let r = e.run("fixture_type.from_library", &json!({"name": "Betopper LM70SR"}));
        assert_eq!(r.ok().map(|v| v["modes"].clone()), Some(json!(["14 channels", "9 channels"])));
        let lm = e.show.fixture_types[0].modes[0].clone();
        assert_eq!(lm.channels.len(), 14);
        assert_eq!(lm.channels[0], Attribute::Pan);
        assert_eq!(lm.channels[1], Attribute::PanFine);
        assert_eq!(lm.channels[6], Attribute::Red);
        let six: Vec<_> = lm.ranges.iter().filter(|r| r.channel == 6).map(|r| (r.from, r.to)).collect();
        assert_eq!(six, vec![(0, 7), (8, 134), (135, 239), (240, 255)], "channel 6: off, dimmer, strobe, open");
        // Out to a file and into another show, ranges and all.
        assert!(e.run("fixture_type.export", &json!({"type": 0, "path": "/f/lm.txt"})).is_err());
        assert!(e.run("fixture_type.export", &json!({"type": 0, "path": "/f/lm.json"})).is_ok());
        let bytes = e.services().read_file("/f/lm.json").unwrap_or_default();
        let mut other = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = other.services_mut().write_file("/g/lm.json", &bytes);
        assert!(other.run("fixture_type.import", &json!({"path": "/g/lm.json"})).is_ok());
        assert_eq!(other.show.fixture_types, e.show.fixture_types);
        // Every library type is a valid one.
        for doc in library() {
            let mut fresh = Engine::new(Box::new(crate::MemoryServices::default()));
            assert!(fixture_type_set(&mut fresh, &doc).is_ok(), "{}", doc["name"]);
        }
    }
}
