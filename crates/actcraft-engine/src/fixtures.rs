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
    let (kind_name, channels, ranges) = match (prm.get("kind"), prm.get("channels"), prm.get("type")) {
        (None, None, Some(_)) => {
            let t = p::index(prm, "type", e.show.fixture_types.len())?;
            let ty = e.show.fixture_types.get(t).ok_or_else(|| invalid("no such fixture type"))?;
            let m = usize::try_from(p::opt_i64_in(prm, "mode", 0, 31)?.unwrap_or(0)).unwrap_or(0);
            let mode = ty.modes.get(m).ok_or_else(|| invalid("that fixture type has no such mode"))?;
            (format!("{} \u{b7} {}", ty.name, mode.name), mode.channels.clone(), mode.ranges.clone())
        }
        (Some(_), None, None) => {
            let key = p::str_max(prm, "kind", 32)?;
            let kind = KINDS.iter().find(|k| k.key == key).ok_or_else(|| invalid("no such kind of fixture"))?;
            (kind.name.to_owned(), kind.channels.to_vec(), Vec::new())
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
            (String::new(), channels, Vec::new())
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
            ranges: ranges.clone(),
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
                let ranges: Vec<Value> =
                    f.ranges.iter().map(|r| json!({"channel": r.channel, "from": r.from, "to": r.to, "name": r.name})).collect();
                json!({"name": f.name, "kind": f.kind, "universe": f.universe, "address": f.address, "channels": channels, "ranges": ranges})
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

/// Like [`levels_for`], for a preset's value: `channel` names which of the fixture's
/// [`Attribute::Other`] channels it is (a fixture can have several); `None` goes by the attribute.
pub fn slot_levels(f: &Fixture, attribute: Attribute, channel: Option<u16>, value: f64) -> Vec<(usize, u8)> {
    match one_channel(f, attribute, channel) {
        Some(c) => vec![(c, (value.clamp(0.0, 1.0) * 255.0).round() as u8)],
        None => levels_for(f, attribute, value),
    }
}

/// The one channel a value with a channel number sets, when the fixture has that channel and it
/// is the value's attribute.
fn one_channel(f: &Fixture, attribute: Attribute, channel: Option<u16>) -> Option<usize> {
    let c = usize::from(channel?);
    (f.channels.get(c) == Some(&attribute)).then_some(c)
}

/// An attribute's value now (0-1), from the levels being edited (blind's copy while blind is on);
/// `None` when the fixture lacks it.
pub fn value_of(e: &Engine, f: &Fixture, attribute: Attribute) -> Option<f64> {
    value_in(e.dmx.edited(usize::from(f.universe))?, f, attribute)
}

/// Like [`value_of`], for a preset's value with its channel number (see [`slot_levels`]).
pub fn slot_value_of(e: &Engine, f: &Fixture, attribute: Attribute, channel: Option<u16>) -> Option<f64> {
    slot_value_in(e.dmx.edited(usize::from(f.universe))?, f, attribute, channel)
}

/// A preset value's level at the lights now (0-1, before blackout and master), with its channel
/// number (see [`slot_levels`]).
pub(crate) fn slot_live_value_of(e: &Engine, f: &Fixture, attribute: Attribute, channel: Option<u16>) -> Option<f64> {
    slot_value_in(e.dmx.universe(usize::from(f.universe))?, f, attribute, channel)
}

fn slot_value_in(levels: &[u8; CHANNELS], f: &Fixture, attribute: Attribute, channel: Option<u16>) -> Option<f64> {
    match one_channel(f, attribute, channel) {
        Some(c) => levels.get(usize::from(f.address).saturating_sub(1) + c).map(|l| f64::from(*l) / 255.0),
        None => value_in(levels, f, attribute),
    }
}

fn value_in(levels: &[u8; CHANNELS], f: &Fixture, attribute: Attribute) -> Option<f64> {
    let at = |offset: usize| levels.get(usize::from(f.address).saturating_sub(1) + offset).copied();
    let coarse = at(f.channels.iter().position(|a| *a == attribute)?)?;
    match attribute.fine().and_then(|fine| f.channels.iter().position(|a| *a == fine)) {
        Some(fine) => Some(f64::from(u16::from_be_bytes([coarse, at(fine)?])) / 65_535.0),
        None => Some(f64::from(coarse) / 255.0),
    }
}

/// What a fixture shows now as a colour (red, green, blue, each 0-1, its intensity applied), for
/// a preview: white, amber and UV lighten the channels they look like; a fixture without colour
/// channels shows white.
pub fn shown_colour(e: &Engine, f: &Fixture) -> [f64; 3] {
    colour_from(f, |a| value_of(e, f, a))
}

/// What a fixture sends to the lights now as a colour, with blackout and the master applied
/// (for lights that follow it, like a Hue light).
pub fn output_colour(e: &Engine, f: &Fixture) -> [f64; 3] {
    let Some(levels) = e.output_universe(usize::from(f.universe)) else { return [0.0; 3] };
    colour_from(f, |a| value_in(&levels, f, a))
}

/// A fixture's colour from its attribute values.
fn colour_from(f: &Fixture, value: impl Fn(Attribute) -> Option<f64>) -> [f64; 3] {
    let v = |a| value(a).unwrap_or(0.0);
    let intensity = value(A::Intensity).unwrap_or(1.0);
    let coloured = f.channels.iter().any(|a| matches!(a, A::Red | A::Green | A::Blue | A::White | A::Amber | A::Uv));
    let [r, g, b] = if coloured {
        let (w, amber, uv) = (v(A::White), v(A::Amber), v(A::Uv));
        [v(A::Red).max(w).max(amber), v(A::Green).max(w).max(amber * 0.6), v(A::Blue).max(w).max(uv * 0.6)]
    } else {
        [1.0, 1.0, 1.0]
    };
    [r * intensity, g * intensity, b * intensity]
}

/// Pan and tilt ranges taken for a moving light, in degrees. Common values; fixture types don't
/// carry their own ranges yet, so a light with other ranges points a little off in the preview.
pub const PAN_RANGE: f64 = 540.0;
pub const TILT_RANGE: f64 = 270.0;

/// Where a moving light's beam goes, seen from above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beam {
    /// Degrees from facing the audience, turning towards the right of the stage view
    /// (-180 to 180).
    pub direction: f64,
    /// How far out the beam lands: 0 straight down, 1 level or above.
    pub reach: f64,
}

/// A light's beam if it moves (has pan or tilt): pan turns it, tilt away from straight down
/// sends it out (half way on both is straight down, facing the audience).
pub fn beam(e: &Engine, f: &Fixture) -> Option<Beam> {
    if !f.channels.iter().any(|a| matches!(a, A::Pan | A::Tilt)) {
        return None;
    }
    let pan = value_of(e, f, A::Pan).unwrap_or(0.5);
    let tilt = value_of(e, f, A::Tilt).unwrap_or(0.5);
    let from_down = (tilt - 0.5) * TILT_RANGE;
    // Tilting back points the other way.
    let mut direction = (pan - 0.5) * PAN_RANGE + if from_down < 0.0 { 180.0 } else { 0.0 };
    direction = (direction + 180.0).rem_euclid(360.0) - 180.0;
    let reach = from_down.abs().min(90.0).to_radians().sin();
    Some(Beam { direction, reach })
}

/// Sets one channel of a fixture (from 1) to `level` (0-255), in blind's copy while blind is on.
fn put_channel(e: &mut Engine, i: usize, channel: u16, level: u8) -> Result<bool, CommandError> {
    let f = e.show.fixtures.get(i).ok_or_else(|| invalid("no such fixture"))?;
    if channel == 0 || usize::from(channel) > f.channels.len() {
        return Err(invalid("the fixture has no such channel"));
    }
    let (universe, at) = (usize::from(f.universe), usize::from(f.address) + usize::from(channel) - 1);
    Ok(e.dmx.set_edited(universe, at, &[level]))
}

/// `fixture.channel {fixture, channel, level}`: one channel of a fixture (from 1) to a level
/// (0-255), whatever it does.
pub fn fixture_channel(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let channel = u16::try_from(p::i64_in(prm, "channel", 1, MAX_FIXTURE_CHANNELS as i64)?).unwrap_or(1);
    let level = u8::try_from(p::i64_in(prm, "level", 0, 255)?).unwrap_or(0);
    Ok(json!({"changed": put_channel(e, i, channel, level)?}))
}

/// `fixture.range {fixture, range, value?}`: puts the range's channel into one of its named
/// ranges ("Open", "Strobe slow to fast"), `value` (0-1, 0 without) of the way through it.
pub fn fixture_range(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let count = e.show.fixtures.get(i).map_or(0, |f| f.ranges.len());
    let r = p::index(prm, "range", count)?;
    let value = p::opt_f64_in(prm, "value", 0.0, 1.0)?.unwrap_or(0.0);
    let Some(range) = e.show.fixtures.get(i).and_then(|f| f.ranges.get(r)).cloned() else {
        return Err(invalid("no such range"));
    };
    let span = f64::from(range.to - range.from);
    let level = range.from.saturating_add((span * value).round() as u8);
    let changed = put_channel(e, i, range.channel, level)?;
    Ok(json!({"changed": changed, "level": level}))
}

/// The range a fixture's channel (from 1) stands in now (from the levels being edited), if it
/// has ranges there.
pub fn range_now(e: &Engine, f: &Fixture, channel: u16) -> Option<usize> {
    let level = e.dmx.edited(usize::from(f.universe))?.get(usize::from(f.address) + usize::from(channel) - 2)?;
    f.ranges.iter().position(|r| r.channel == channel && (r.from..=r.to).contains(level))
}

/// `fixture.hue {fixture, light}`: a Philips Hue light (its id on the paired bridge) shows what the
/// fixture shows; an empty `light` lets it go.
pub fn fixture_hue(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let light = p::str_max(prm, "light", 16)?.trim().to_owned();
    if !light.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(invalid("`light` must be a Hue light's id, like 3"));
    }
    if let Some(f) = e.show.fixtures.get_mut(i) {
        f.hue = light;
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `fixture.place {fixture, x, y}`: where it is on the stage, each 0-1 (x left to right, y back to
/// front, seen from the audience).
pub fn fixture_place(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fixture", e.show.fixtures.len())?;
    let x = p::f64_in(prm, "x", 0.0, 1.0)?;
    let y = p::f64_in(prm, "y", 0.0, 1.0)?;
    if let Some(f) = e.show.fixtures.get_mut(i) {
        f.place = Some([x, y]);
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `fixture.stage`: every fixture's place (or none) and the colour it shows now, as `#rrggbb`.
pub fn fixture_stage(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .fixtures
            .iter()
            .map(|f| {
                let [r, g, b] = shown_colour(e, f);
                let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                let colour = format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b));
                let beam = beam(e, f)
                    .map(|b| json!({"direction": b.direction.round(), "reach": (b.reach * 100.0).round() / 100.0}));
                json!({"name": f.name, "place": f.place, "colour": colour, "beam": beam})
            })
            .collect(),
    ))
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
                changed |= e.dmx.set_edited(usize::from(f.universe), usize::from(f.address) + offset, &[level]);
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
    fn a_moving_light_points_where_pan_and_tilt_send_it() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1}));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let aim = |e: &mut Engine, pan: f64, tilt: f64| {
            let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"pan": pan, "tilt": tilt}}));
            let f = e.show.fixtures[0].clone();
            beam(e, &f).map(|b| (b.direction.round(), (b.reach * 100.0).round() / 100.0))
        };
        assert_eq!(aim(&mut e, 0.5, 0.5), Some((0.0, 0.0)), "straight down");
        // Tilted 90° forward (a third of 270° past the middle): level, facing the audience.
        assert_eq!(aim(&mut e, 0.5, 0.5 + 1.0 / 3.0), Some((0.0, 1.0)));
        // Panned 90° (a sixth of 540°) and tilted 45° forward: to the right, half way out.
        assert_eq!(aim(&mut e, 0.5 + 1.0 / 6.0, 0.5 + 1.0 / 6.0), Some((90.0, 0.71)));
        // Tilted back the same amount: the other way.
        assert_eq!(aim(&mut e, 0.5 + 1.0 / 6.0, 0.5 - 1.0 / 6.0), Some((-90.0, 0.71)));
        let dimmer = e.show.fixtures[1].clone();
        assert_eq!(beam(&e, &dimmer), None, "a fixed light has no beam to turn");
        let stage = e.run("fixture.stage", &Value::Null).ok().unwrap_or_default();
        assert_eq!(stage[0]["beam"], json!({"direction": -90.0, "reach": 0.71}));
        assert_eq!(stage[1]["beam"], Value::Null);
    }

    #[test]
    fn a_mixed_channel_is_set_by_its_named_ranges() {
        let mut e = engine();
        let ty = json!({"name": "Spot", "modes": [{"name": "4 ch", "channels": ["pan", "tilt", "other", "red"],
            "ranges": [{"channel": 3, "from": 0, "to": 7, "name": "Off"},
                       {"channel": 3, "from": 8, "to": 134, "name": "Dimmer"},
                       {"channel": 3, "from": 135, "to": 239, "name": "Strobe slow to fast"},
                       {"channel": 3, "from": 240, "to": 255, "name": "Open"}]}]});
        assert!(e.run("fixture_type.set", &ty).is_ok());
        let _ = e.run("fixture.add", &json!({"type": 0, "universe": 1, "address": 10}));
        assert_eq!(e.show.fixtures[0].ranges.len(), 4, "the fixture takes its mode's ranges");
        let level = |e: &Engine| e.dmx.universe(1).map_or(0, |u| u[11]);
        assert_eq!(
            e.run("fixture.range", &json!({"fixture": 0, "range": 3})).ok(),
            Some(json!({"changed": true, "level": 240}))
        );
        assert_eq!(level(&e), 240);
        let f = e.show.fixtures[0].clone();
        assert_eq!(range_now(&e, &f, 3), Some(3), "it stands in Open");
        assert_eq!(
            e.run("fixture.range", &json!({"fixture": 0, "range": 1, "value": 1.0})).ok(),
            Some(json!({"changed": true, "level": 134}))
        );
        assert_eq!(range_now(&e, &f, 3), Some(1));
        assert_eq!(range_now(&e, &f, 1), None, "no ranges on pan");
        assert!(e.run("fixture.range", &json!({"fixture": 0, "range": 4})).is_err());
        assert!(e.run("fixture.channel", &json!({"fixture": 0, "channel": 3, "level": 200})).is_ok());
        assert_eq!(level(&e), 200);
        assert!(e.run("fixture.channel", &json!({"fixture": 0, "channel": 5, "level": 1})).is_err(), "4 channels");
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.fixtures).unwrap_or_default();
        assert_eq!(back[0].ranges, e.show.fixtures[0].ranges);
        // Bad ranges are refused.
        let bad = |r: Value| json!({"name": "X", "modes": [{"channels": ["other"], "ranges": [r]}]});
        assert!(e.run("fixture_type.set", &bad(json!({"channel": 2, "from": 0, "to": 9, "name": "A"}))).is_err());
        assert!(e.run("fixture_type.set", &bad(json!({"channel": 1, "from": 9, "to": 0, "name": "A"}))).is_err());
        assert!(e.run("fixture_type.set", &bad(json!({"channel": 1, "from": 0, "to": 9, "name": " "}))).is_err());
    }

    #[test]
    fn a_hue_light_follows_what_a_fixture_sends() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"kind": "rgb_dimmer", "universe": 1}));
        assert!(e.run("fixture.hue", &json!({"fixture": 0, "light": "3"})).is_ok());
        assert!(e.run("fixture.hue", &json!({"fixture": 0, "light": "3; rm"})).is_err());
        assert_eq!(e.show.fixtures[0].hue, "3");
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0, "red": 1.0}}));
        let f = e.show.fixtures[0].clone();
        assert_eq!(output_colour(&e, &f), [1.0, 0.0, 0.0]);
        // What goes out, not what is set: blackout takes it dark.
        let _ = e.run("dmx.blackout", &json!({"on": true}));
        assert_eq!(output_colour(&e, &f), [0.0, 0.0, 0.0]);
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.fixtures).unwrap_or_default();
        assert_eq!(back[0].hue, "3");
        assert!(e.run("fixture.hue", &json!({"fixture": 0, "light": ""})).is_ok());
        assert!(e.show.fixtures[0].hue.is_empty());
    }

    #[test]
    fn fixtures_are_placed_and_show_their_colour() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"kind": "rgb_dimmer", "universe": 1}));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        assert!(e.run("fixture.place", &json!({"fixture": 0, "x": 0.25, "y": 0.5})).is_ok());
        assert_eq!(e.show.fixtures[0].place, Some([0.25, 0.5]));
        assert!(e.run("fixture.place", &json!({"fixture": 0, "x": 1.5, "y": 0.5})).is_err());
        let _ = e.run("fixture.set", &json!({"fixtures": [0, 1], "values": {"intensity": 0.5, "red": 1.0}}));
        let [r, g, b] = shown_colour(&e, &e.show.fixtures[0].clone());
        assert!((r - 0.5).abs() < 0.01 && g == 0.0 && b == 0.0, "red at half: {r} {g} {b}");
        let dimmer = shown_colour(&e, &e.show.fixtures[1].clone());
        assert!(dimmer.iter().all(|c| (c - 0.5).abs() < 0.01), "a dimmer shows white: {dimmer:?}");
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
