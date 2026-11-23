//! Fixtures: the lights in a show, each with its channels (what every DMX channel controls) and
//! where it is patched. Setting an attribute of a fixture (its intensity, its red) writes the DMX
//! levels; pan, tilt and intensity with a fine channel are set to 16 bits.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::{MAX_FIXTURE_CHANNELS, MAX_FIXTURES};
use serde_json::{Map, Value, json};

pub use actcraft_show::{Attribute, Fixture};

use crate::Engine;
use crate::dmx::{CHANNELS, UNIVERSES};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// A kind of fixture ActCraft knows: its key, its name and its channels.
pub struct Kind {
    pub key: &'static str,
    pub name: &'static str,
    pub channels: &'static [Attribute],
}

use Attribute as A;

/// Generic kinds that most simple lights follow; others are set up channel by channel.
pub const KINDS: &[Kind] = &[
    Kind { key: "dimmer", name: "Dimmer", channels: &[A::Intensity] },
    Kind { key: "rgb", name: "RGB", channels: &[A::Red, A::Green, A::Blue] },
    Kind { key: "rgb_dimmer", name: "RGB with dimmer", channels: &[A::Intensity, A::Red, A::Green, A::Blue] },
    Kind { key: "rgbw", name: "RGBW", channels: &[A::Red, A::Green, A::Blue, A::White] },
    Kind {
        key: "rgbw_dimmer",
        name: "RGBW with dimmer",
        channels: &[A::Intensity, A::Red, A::Green, A::Blue, A::White],
    },
    Kind {
        key: "rgbawuv_dimmer",
        name: "RGBAW and UV with dimmer and strobe",
        channels: &[A::Intensity, A::Red, A::Green, A::Blue, A::Amber, A::White, A::Uv, A::Strobe],
    },
    Kind {
        key: "moving_head",
        name: "Moving head (16-bit pan and tilt, RGBW)",
        channels: &[
            A::Pan,
            A::PanFine,
            A::Tilt,
            A::TiltFine,
            A::Intensity,
            A::Red,
            A::Green,
            A::Blue,
            A::White,
            A::Strobe,
            A::Zoom,
        ],
    },
];

/// The attribute's name in params and lists.
pub fn attribute_key(a: Attribute) -> &'static str {
    match a {
        A::Intensity => "intensity",
        A::IntensityFine => "intensity_fine",
        A::Red => "red",
        A::Green => "green",
        A::Blue => "blue",
        A::White => "white",
        A::Amber => "amber",
        A::Uv => "uv",
        A::Pan => "pan",
        A::PanFine => "pan_fine",
        A::Tilt => "tilt",
        A::TiltFine => "tilt_fine",
        A::Strobe => "strobe",
        A::ColourWheel => "colour_wheel",
        A::Gobo => "gobo",
        A::Zoom => "zoom",
        A::Focus => "focus",
        A::Other => "other",
    }
}

/// Every attribute, in the order lists show them.
pub const ATTRIBUTES: [Attribute; 18] = [
    A::Intensity,
    A::IntensityFine,
    A::Red,
    A::Green,
    A::Blue,
    A::White,
    A::Amber,
    A::Uv,
    A::Pan,
    A::PanFine,
    A::Tilt,
    A::TiltFine,
    A::Strobe,
    A::ColourWheel,
    A::Gobo,
    A::Zoom,
    A::Focus,
    A::Other,
];

pub(crate) fn attribute_named(key: &str) -> Option<Attribute> {
    ATTRIBUTES.iter().copied().find(|a| attribute_key(*a) == key)
}

/// The lowest address in `universe` where `len` channels fit beside the fixtures there (but
/// `skip`, which is being moved).
pub fn next_free(fixtures: &[Fixture], universe: u16, len: usize, skip: Option<usize>) -> Option<u16> {
    let probe = |address: usize| Fixture {
        channels: vec![A::Other; len],
        universe,
        address: u16::try_from(address).unwrap_or(0),
        ..Fixture::default()
    };
    (1..=CHANNELS.checked_sub(len)? + 1)
        .find(|a| {
            let f = probe(*a);
            fixtures.iter().enumerate().all(|(i, o)| Some(i) == skip || !o.overlaps(&f))
        })
        .and_then(|a| u16::try_from(a).ok())
}

/// Where a fixture goes: universe (from 1) and address (from 1).
fn placement(prm: &Value) -> Result<(u16, Option<u16>), CommandError> {
    let universe = u16::try_from(p::i64_in(prm, "universe", 1, UNIVERSES as i64)?).unwrap_or(1);
    let address = p::opt_i64_in(prm, "address", 1, CHANNELS as i64)?.and_then(|a| u16::try_from(a).ok());
    Ok((universe, address))
}

/// `fixture.add {kind | type, mode? | channels, name?, universe, address?, count?}`: patches one
/// fixture (a built-in kind, an imported type in one of its modes, or channel by channel), or
/// `count` of the same in a row; without `address` each goes to the first free one.
pub fn fixture_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let (kind_name, channels) = match (prm.get("kind"), prm.get("channels"), prm.get("type")) {
        (None, None, Some(_)) => {
            let t = p::index(prm, "type", e.show.fixture_types.len())?;
            let ty = e.show.fixture_types.get(t).ok_or_else(|| invalid("no such fixture type"))?;
            let m = usize::try_from(p::opt_i64_in(prm, "mode", 0, 31)?.unwrap_or(0)).unwrap_or(0);
            let mode = ty.modes.get(m).ok_or_else(|| invalid("that fixture type has no such mode"))?;
            (format!("{} \u{b7} {}", ty.name, mode.name), mode.channels.clone())
        }
        (Some(_), None, None) => {
            let key = p::str_max(prm, "kind", 32)?;
            let kind = KINDS.iter().find(|k| k.key == key).ok_or_else(|| invalid("no such kind of fixture"))?;
            (kind.name.to_owned(), kind.channels.to_vec())
        }
        (None, Some(list), None) => {
            let list = list.as_array().ok_or_else(|| invalid("`channels` must be a list of attributes"))?;
            if list.is_empty() || list.len() > MAX_FIXTURE_CHANNELS {
                return Err(invalid("a fixture has 1 to 64 channels"));
            }
            let channels = list
                .iter()
                .map(|v| v.as_str().and_then(attribute_named))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("`channels` must be a list of attributes"))?;
            (String::new(), channels)
        }
        _ => return Err(invalid("give `kind`, `type` or `channels`")),
    };
    let (universe, address) = placement(prm)?;
    let count = usize::try_from(p::opt_i64_in(prm, "count", 1, 512)?.unwrap_or(1)).unwrap_or(1);
    let fallback = if kind_name.is_empty() { "Fixture" } else { kind_name.as_str() };
    let name = p::opt_str_max(prm, "name", 60)?.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(fallback).to_owned();
    if e.show.fixtures.len() + count > MAX_FIXTURES {
        return Err(failed("a show holds at most 1024 fixtures"));
    }
    let mut added = Vec::new();
    let mut fixtures = e.show.fixtures.clone();
    let first_id = fixtures.iter().map(|f| f.id).max().unwrap_or(0) + 1;
    let mut at = address;
    for n in 0..count {
        let start = match at {
            Some(a) => a,
            None => next_free(&fixtures, universe, channels.len(), None)
                .ok_or_else(|| failed("there is no room left in that universe"))?,
        };
        let fixture = Fixture {
            id: first_id + u64::try_from(n).unwrap_or(0),
            name: if count > 1 { format!("{name} {}", n + 1) } else { name.clone() },
            kind: kind_name.clone(),
            channels: channels.clone(),
            universe,
            address: start,
            ..Fixture::default()
        };
        if *fixture.footprint().end() > CHANNELS {
            return Err(invalid("the fixture does not fit in the universe from that address"));
        }
        if fixtures.iter().any(|o| o.overlaps(&fixture)) {
            return Err(failed("that address is taken by another fixture"));
        }
        // The next one of a row goes right after this one.
        at = address.map(|_| start.saturating_add(u16::try_from(channels.len()).unwrap_or(u16::MAX)));
        fixtures.push(fixture);
        added.push(fixtures.len() - 1);
    }
    e.show.fixtures = fixtures;
    e.mark_changed();
    Ok(json!({"fixtures": added}))
}

/// `fixture.remove {fixture}`.
pub fn fixture_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let gone = e.show.fixtures.remove(i).id;
    // Groups and presets forget it too.
    for g in &mut e.show.groups {
        g.fixtures.retain(|id| *id != gone);
    }
    for p in &mut e.show.presets {
        p.values.retain(|v| v.fixture != gone);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `fixture.move {fixture, universe, address?}`: patches it elsewhere (the first free address
/// without one).
pub fn fixture_move(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let (universe, address) = placement(prm)?;
    let len = e.show.fixtures.get(i).map_or(0, |f| f.channels.len());
    let address = match address {
        Some(a) => a,
        None => next_free(&e.show.fixtures, universe, len, Some(i))
            .ok_or_else(|| failed("there is no room left in that universe"))?,
    };
    let Some(mut moved) = e.show.fixtures.get(i).cloned() else { return Err(invalid("no such fixture")) };
    moved.universe = universe;
    moved.address = address;
    if *moved.footprint().end() > CHANNELS {
        return Err(invalid("the fixture does not fit in the universe from that address"));
    }
    if e.show.fixtures.iter().enumerate().any(|(j, o)| j != i && o.overlaps(&moved)) {
        return Err(failed("that address is taken by another fixture"));
    }
    if let Some(slot) = e.show.fixtures.get_mut(i) {
        *slot = moved;
    }
    e.mark_changed();
    Ok(json!({"universe": universe, "address": address}))
}

/// `fixture.list`: every fixture with its channels.
pub fn fixture_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .fixtures
            .iter()
            .map(|f| {
                let channels: Vec<&str> = f.channels.iter().map(|a| attribute_key(*a)).collect();
                json!({"name": f.name, "kind": f.kind, "universe": f.universe, "address": f.address, "channels": channels})
            })
            .collect(),
    ))
}

/// `fixture.kinds`: the kinds `fixture.add` knows.
pub fn fixture_kinds(_e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        KINDS
            .iter()
            .map(|k| {
                let channels: Vec<&str> = k.channels.iter().map(|a| attribute_key(*a)).collect();
                json!({"kind": k.key, "name": k.name, "channels": channels})
            })
            .collect(),
    ))
}

/// The DMX levels for `value` (0-1) on a fixture's attribute: (channel offset, level) pairs, two
/// when it has a fine channel. Nothing when it lacks the attribute.
pub fn levels_for(f: &Fixture, attribute: Attribute, value: f64) -> Vec<(usize, u8)> {
    let Some(coarse) = f.channels.iter().position(|a| *a == attribute) else { return Vec::new() };
    let v = value.clamp(0.0, 1.0);
    match attribute.fine().and_then(|fine| f.channels.iter().position(|a| *a == fine)) {
        Some(fine) => {
            let n = (v * 65_535.0).round() as u16;
            let [hi, lo] = n.to_be_bytes();
            vec![(coarse, hi), (fine, lo)]
        }
        None => vec![(coarse, (v * 255.0).round() as u8)],
    }
}

/// An attribute's value now (0-1), from the DMX levels; `None` when the fixture lacks it.
pub fn value_of(e: &Engine, f: &Fixture, attribute: Attribute) -> Option<f64> {
    let levels = e.dmx.universe(usize::from(f.universe))?;
    let at = |offset: usize| levels.get(usize::from(f.address).saturating_sub(1) + offset).copied();
    let coarse = at(f.channels.iter().position(|a| *a == attribute)?)?;
    match attribute.fine().and_then(|fine| f.channels.iter().position(|a| *a == fine)) {
        Some(fine) => Some(f64::from(u16::from_be_bytes([coarse, at(fine)?])) / 65_535.0),
        None => Some(f64::from(coarse) / 255.0),
    }
}

/// The fixtures a command acts on, as indexes: `fixture`, `fixtures` or a `group`.
pub(crate) fn targets(e: &Engine, prm: &Value) -> Result<Vec<usize>, CommandError> {
    let len = e.show.fixtures.len();
    match (prm.get("fixture"), prm.get("fixtures"), prm.get("group")) {
        (Some(_), None, None) => Ok(vec![p::index(prm, "fixture", len)?]),
        (None, Some(_), None) => p::index_list(prm, "fixtures", len, MAX_FIXTURES),
        (None, None, Some(_)) => {
            let g = p::index(prm, "group", e.show.groups.len())?;
            let ids = e.show.groups.get(g).map(|g| g.fixtures.clone()).unwrap_or_default();
            Ok(ids.iter().filter_map(|id| e.show.fixtures.iter().position(|f| f.id == *id)).collect())
        }
        _ => Err(invalid("give `fixture`, `fixtures` or `group`")),
    }
}

/// `fixture.set {fixture | fixtures | group, values: {attribute: 0-1, ...}}`: sets attributes of one or
/// more fixtures; an attribute a fixture lacks is left alone.
pub fn fixture_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let targets = targets(e, prm)?;
    let values: &Map<String, Value> =
        prm.get("values").and_then(Value::as_object).ok_or_else(|| invalid("`values` must map attributes to 0-1"))?;
    let mut wanted = Vec::new();
    for (key, v) in values.iter().take(ATTRIBUTES.len()) {
        let a = attribute_named(key)
            .filter(|a| !a.is_fine())
            .ok_or_else(|| invalid("`values` must map attributes to 0-1"))?;
        let v = v
            .as_f64()
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or_else(|| invalid("`values` must map attributes to 0-1"))?;
        wanted.push((a, v));
    }
    let mut changed = false;
    for i in targets {
        let Some(f) = e.show.fixtures.get(i).cloned() else { continue };
        for (a, v) in &wanted {
            for (offset, level) in levels_for(&f, *a, *v) {
                changed |= e.dmx.set(usize::from(f.universe), usize::from(f.address) + offset, &[level]);
            }
        }
    }
    Ok(json!({"changed": changed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(Box::new(crate::MemoryServices::default()))
    }

    #[test]
    fn fixtures_are_patched_without_overlap() {
        let mut e = engine();
        let r = e.run("fixture.add", &json!({"kind": "rgbw_dimmer", "universe": 1, "count": 3, "name": "Wash"}));
        assert_eq!(r.ok(), Some(json!({"fixtures": [0, 1, 2]})));
        let addresses: Vec<u16> = e.show.fixtures.iter().map(|f| f.address).collect();
        assert_eq!(addresses, vec![1, 6, 11], "one after another");
        assert_eq!(e.show.fixtures[2].name, "Wash 3");
        // Taken, does not fit, unknown kind or attribute.
        for bad in [
            json!({"kind": "dimmer", "universe": 1, "address": 8}),
            json!({"kind": "moving_head", "universe": 1, "address": 505}),
            json!({"kind": "laser", "universe": 1}),
            json!({"channels": ["red", "smoke"], "universe": 1}),
            json!({"channels": [], "universe": 1}),
            json!({"universe": 1}),
        ] {
            assert!(e.run("fixture.add", &bad).is_err(), "{bad}");
        }
        let r = e.run("fixture.add", &json!({"channels": ["pan", "tilt", "intensity"], "universe": 1}));
        assert_eq!(r.ok(), Some(json!({"fixtures": [3]})));
        assert_eq!(e.show.fixtures[3].address, 16, "the first free address");
        assert!(e.run("fixture.remove", &json!({"fixture": 1})).is_ok());
        assert_eq!(
            e.run("fixture.move", &json!({"fixture": 2, "universe": 1})).ok(),
            Some(json!({"universe": 1, "address": 6}))
        );
        assert!(e.run("fixture.move", &json!({"fixture": 2, "universe": 1, "address": 1})).is_err(), "taken");
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.fixtures).unwrap_or_default();
        assert_eq!(back, e.show.fixtures);
    }

    #[test]
    fn setting_attributes_writes_dmx_levels() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 2, "address": 100}));
        let _ = e.run("fixture.add", &json!({"kind": "rgb", "universe": 2}));
        let r =
            e.run("fixture.set", &json!({"fixtures": [0, 1], "values": {"pan": 0.5, "red": 1.0, "intensity": 0.25}}));
        assert_eq!(r.ok(), Some(json!({"changed": true})));
        let levels = e.dmx.universe(2).copied().unwrap_or([0; CHANNELS]);
        // Pan at 100/101 in 16 bits; red at 105 (moving head) and at 1 (the RGB light).
        assert_eq!((levels[99], levels[100]), (0x80, 0x00));
        assert_eq!(levels[104], 255);
        assert_eq!(levels[0], 255);
        let f = e.show.fixtures[0].clone();
        assert!(value_of(&e, &f, A::Pan).is_some_and(|v| (v - 0.5).abs() < 1e-4));
        assert_eq!(value_of(&e, &e.show.fixtures[1].clone(), A::Intensity), None, "the RGB light has no dimmer");
        for bad in [
            json!({"fixture": 0, "values": {"pan": 2.0}}),
            json!({"fixture": 0, "values": {"pan_fine": 0.5}}),
            json!({"fixture": 0, "values": {"smoke": 0.5}}),
            json!({"fixture": 9, "values": {"pan": 0.5}}),
            json!({"fixture": 0}),
        ] {
            assert!(e.run("fixture.set", &bad).is_err(), "{bad}");
        }
    }
}
