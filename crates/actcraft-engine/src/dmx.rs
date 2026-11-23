//! DMX levels: what each channel of each universe is at now. They are live, not saved with the
//! show; lighting outputs send them, and the DMX monitor shows them.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
pub use actcraft_show::{DmxOutput, DmxProtocol, DmxTrigger};
use serde_json::{Value, json};

use crate::Engine;

/// Universes ActCraft drives, numbered from 1.
pub const UNIVERSES: usize = actcraft_show::limits::DMX_UNIVERSES;
/// Channels in a universe, numbered from 1.
pub const CHANNELS: usize = 512;

/// The levels of every universe.
#[derive(Debug, Clone, PartialEq)]
pub struct Dmx {
    universes: Vec<[u8; CHANNELS]>,
    /// Goes up with every change (also of blackout and master), for outputs to see that
    /// something changed.
    pub version: u64,
    /// Every fixture dark at the output; the levels stay as they are.
    pub blackout: bool,
    /// How bright fixtures are at the output, 0-1 (1 as set).
    pub master: f64,
    /// Blind: while on, fixture and preset edits go to this copy of the levels and not to the
    /// output; the copy goes when blind goes off.
    blind: Option<Vec<[u8; CHANNELS]>>,
    /// DMX heard from other gear (Art-Net in), by Art-Net port address; live, not saved.
    input: std::collections::BTreeMap<u16, [u8; CHANNELS]>,
}

impl Default for Dmx {
    fn default() -> Self {
        Self {
            universes: vec![[0; CHANNELS]; UNIVERSES],
            version: 0,
            blackout: false,
            master: 1.0,
            blind: None,
            input: std::collections::BTreeMap::new(),
        }
    }
}

impl Dmx {
    /// The levels of `universe` (from 1); `None` outside 1..=[`UNIVERSES`].
    pub fn universe(&self, universe: usize) -> Option<&[u8; CHANNELS]> {
        self.universes.get(universe.checked_sub(1)?)
    }

    /// Sets channels from `channel` (from 1) on; true when a level changed.
    pub(crate) fn set(&mut self, universe: usize, channel: usize, values: &[u8]) -> bool {
        let changed = write(&mut self.universes, universe, channel, values);
        if changed {
            self.version += 1;
        }
        changed
    }

    /// Levels heard from other gear on an Art-Net port address, if any came.
    pub fn input(&self, port_address: u16) -> Option<&[u8; CHANNELS]> {
        self.input.get(&port_address)
    }

    /// The Art-Net port addresses DMX came in on, lowest first.
    pub fn inputs(&self) -> Vec<u16> {
        self.input.keys().copied().collect()
    }

    /// Whether blind is on.
    pub fn blind(&self) -> bool {
        self.blind.is_some()
    }

    /// Blind on (edits start from the levels sent now) or off (its edits go).
    pub(crate) fn set_blind(&mut self, on: bool) {
        match (on, self.blind.is_some()) {
            (true, false) => self.blind = Some(self.universes.clone()),
            (false, true) => self.blind = None,
            _ => {}
        }
    }

    /// The levels edits see: blind's copy while blind is on, else the levels sent.
    pub fn edited(&self, universe: usize) -> Option<&[u8; CHANNELS]> {
        let i = universe.checked_sub(1)?;
        match &self.blind {
            Some(copy) => copy.get(i),
            None => self.universes.get(i),
        }
    }

    /// Sets channels like [`Dmx::set`], in blind's copy while blind is on.
    pub(crate) fn set_edited(&mut self, universe: usize, channel: usize, values: &[u8]) -> bool {
        match &mut self.blind {
            Some(copy) => write(copy, universe, channel, values),
            None => self.set(universe, channel, values),
        }
    }

    /// Every channel of `universe`, or of every universe for `None`, to 0.
    fn clear(&mut self, universe: Option<usize>) {
        for (i, u) in self.universes.iter_mut().enumerate() {
            if universe.is_none_or(|n| n == i + 1) {
                *u = [0; CHANNELS];
            }
        }
        self.version += 1;
    }
}

/// Sets channels of `universe` from `channel` (both from 1) on; true when a level changed.
fn write(universes: &mut [[u8; CHANNELS]], universe: usize, channel: usize, values: &[u8]) -> bool {
    let Some(u) = universe.checked_sub(1).and_then(|i| universes.get_mut(i)) else { return false };
    let Some(from) = channel.checked_sub(1) else { return false };
    let Some(slots) = u.get_mut(from..from.saturating_add(values.len()).min(CHANNELS)) else { return false };
    let changed = slots.iter().zip(values).any(|(a, b)| a != b);
    slots.iter_mut().zip(values).for_each(|(slot, v)| *slot = *v);
    changed
}

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

fn universe(prm: &Value) -> Result<usize, CommandError> {
    let n = p::i64_in(prm, "universe", 1, UNIVERSES as i64)?;
    usize::try_from(n).map_err(|_| invalid("no such universe"))
}

/// `dmx.set {universe, channel, value | values}`: one channel to `value`, or channels from
/// `channel` on to `values` (0-255 each).
pub fn dmx_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let universe = universe(prm)?;
    let channel =
        usize::try_from(p::i64_in(prm, "channel", 1, CHANNELS as i64)?).map_err(|_| invalid("no such channel"))?;
    let values: Vec<u8> = match (prm.get("value"), prm.get("values")) {
        (Some(_), None) => vec![u8::try_from(p::i64_in(prm, "value", 0, 255)?).unwrap_or(0)],
        (None, Some(list)) => {
            let list = list.as_array().ok_or_else(|| invalid("`values` must be a list of levels 0-255"))?;
            if list.is_empty() || list.len() > CHANNELS + 1 - channel {
                return Err(invalid("`values` must fit in the universe from `channel` on"));
            }
            list.iter()
                .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect::<Option<Vec<u8>>>()
                .ok_or_else(|| invalid("`values` must be a list of levels 0-255"))?
        }
        _ => return Err(invalid("give `value` or `values`")),
    };
    let changed = e.dmx.set(universe, channel, &values);
    Ok(json!({"changed": changed}))
}

/// `dmx.clear {universe?}`: every channel to 0, in one universe or all.
pub fn dmx_clear(e: &mut Engine, prm: &Value) -> CommandResult {
    let which = if prm.get("universe").is_some() { Some(universe(prm)?) } else { None };
    e.dmx.clear(which);
    Ok(Value::Null)
}

impl Engine {
    /// What a universe sends: its levels, with every fixture's brightness scaled by the master
    /// and taken to 0 in a blackout. A fixture's brightness is its intensity channel (with its
    /// fine channel), or its colour channels when it has no intensity; pan, tilt and the rest
    /// stay as they are, so moving lights don't move.
    pub fn output_universe(&self, universe: usize) -> Option<[u8; CHANNELS]> {
        let mut out = *self.dmx.universe(universe)?;
        let scale = if self.dmx.blackout { 0.0 } else { self.dmx.master.clamp(0.0, 1.0) };
        if scale >= 1.0 {
            return Some(out);
        }
        use actcraft_show::Attribute as A;
        for f in self.show.fixtures.iter().filter(|f| usize::from(f.universe) == universe) {
            let at = |offset: usize| usize::from(f.address).saturating_sub(1) + offset;
            let has_intensity = f.channels.contains(&A::Intensity);
            let coarse = f.channels.iter().position(|a| *a == A::Intensity);
            let fine = f.channels.iter().position(|a| *a == A::IntensityFine);
            match (coarse, fine) {
                (Some(c), Some(fi)) => {
                    let (Some(&hi), Some(&lo)) = (out.get(at(c)), out.get(at(fi))) else { continue };
                    let v = (f64::from(u16::from_be_bytes([hi, lo])) * scale).round() as u16;
                    let [hi, lo] = v.to_be_bytes();
                    if let Some(slot) = out.get_mut(at(c)) {
                        *slot = hi;
                    }
                    if let Some(slot) = out.get_mut(at(fi)) {
                        *slot = lo;
                    }
                }
                _ => {
                    for (offset, a) in f.channels.iter().enumerate() {
                        let dims = if has_intensity {
                            *a == A::Intensity
                        } else {
                            matches!(a, A::Red | A::Green | A::Blue | A::White | A::Amber | A::Uv)
                        };
                        if dims && let Some(slot) = out.get_mut(at(offset)) {
                            *slot = (f64::from(*slot) * scale).round() as u8;
                        }
                    }
                }
            }
        }
        Some(out)
    }

    /// Holds fades and effects where they are (`true`) or lets them go on from there.
    pub(crate) fn set_frozen(&mut self, on: bool) {
        let now = self.services().now();
        match (self.frozen_at, on) {
            (None, true) => self.frozen_at = Some(now),
            (Some(since), false) => {
                // Pick up where they were held: their clocks move on by the time held.
                let held = now - since;
                if let Some(fade) = &mut self.fade {
                    fade.shift(held);
                }
                for (_, started) in &mut self.running_effects {
                    *started += held;
                }
                for went in self.cue_went.values_mut() {
                    *went += held;
                }
                self.frozen_at = None;
            }
            _ => {}
        }
    }

    /// Takes levels heard from other gear on Art-Net port address `port_address` (at most 512;
    /// missing ones are 0). At most 256 universes are kept.
    /// A channel that goes up past half sets off its DMX triggers (once, not again while it stays
    /// up); returns how many commands ran.
    pub fn dmx_in(&mut self, port_address: u16, levels: &[u8]) -> usize {
        if self.dmx.input.len() >= 256 && !self.dmx.input.contains_key(&port_address) {
            return 0;
        }
        let mut u = [0; CHANNELS];
        for (slot, v) in u.iter_mut().zip(levels) {
            *slot = *v;
        }
        let before = self.dmx.input.insert(port_address, u).unwrap_or([0; CHANNELS]);
        let up = |levels: &[u8; CHANNELS], channel: u16| {
            levels.get(usize::from(channel).saturating_sub(1)).is_some_and(|v| *v >= HALF)
        };
        let due: Vec<String> = self
            .show
            .dmx_triggers
            .iter()
            .filter(|t| t.universe == port_address && up(&u, t.channel) && !up(&before, t.channel))
            .map(|t| t.command.clone())
            .collect();
        let mut ran = 0;
        for command in due {
            if crate::remote::runnable(&command) && self.run(&command, &Value::Null).is_ok() {
                ran += 1;
            }
        }
        ran
    }

    /// Whether fades and effects are held.
    pub fn frozen(&self) -> bool {
        self.frozen_at.is_some()
    }
}

fn opt_on(prm: &Value, now: bool) -> Result<bool, CommandError> {
    Ok(p::opt_bool(prm, "on")?.unwrap_or(!now))
}

/// `dmx.blackout {on?}`: every fixture dark at the output (switches without `on`).
pub fn dmx_blackout(e: &mut Engine, prm: &Value) -> CommandResult {
    let on = opt_on(prm, e.dmx.blackout)?;
    e.dmx.blackout = on;
    e.dmx.version += 1;
    Ok(json!({"on": on}))
}

/// `dmx.master {level}`: how bright fixtures are at the output, 0-1.
pub fn dmx_master(e: &mut Engine, prm: &Value) -> CommandResult {
    e.dmx.master = p::f64_in(prm, "level", 0.0, 1.0)?;
    e.dmx.version += 1;
    Ok(Value::Null)
}

/// `lighting.freeze {on?}`: holds fades and effects (switches without `on`).
pub fn lighting_freeze(e: &mut Engine, prm: &Value) -> CommandResult {
    let on = opt_on(prm, e.frozen())?;
    e.set_frozen(on);
    Ok(json!({"on": on}))
}

/// `lighting.blind {on?}`: fixture changes and recalled presets go to a copy of the levels, not
/// to the lights, so presets can be stored without the audience seeing them; off drops the copy
/// (switches without `on`). Cues, fades and effects keep playing live.
pub fn lighting_blind(e: &mut Engine, prm: &Value) -> CommandResult {
    let on = opt_on(prm, e.dmx.blind())?;
    e.dmx.set_blind(on);
    Ok(json!({"on": on}))
}

/// A DMX channel counts as up from this level (half way).
const HALF: u8 = 128;

/// `dmx.trigger_add {universe, channel, command}`: a channel heard on an Art-Net port address
/// runs a command when it goes up past half.
pub fn dmx_trigger_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let universe = p::i64_in(prm, "universe", 0, i64::from(DmxOutput::ARTNET_MAX))?;
    let channel = p::i64_in(prm, "channel", 1, CHANNELS as i64)?;
    let command = p::str_max(prm, "command", 64)?.to_owned();
    if !crate::remote::runnable(&command) {
        return Err(invalid("a trigger runs a command that needs nothing more"));
    }
    if e.show.dmx_triggers.len() >= actcraft_show::limits::MAX_DMX_TRIGGERS {
        return Err(failed("a show holds at most 256 DMX triggers"));
    }
    e.show.dmx_triggers.push(DmxTrigger {
        universe: u16::try_from(universe).unwrap_or(0),
        channel: u16::try_from(channel).unwrap_or(1),
        command,
        ..DmxTrigger::default()
    });
    e.mark_changed();
    Ok(json!({"trigger": e.show.dmx_triggers.len() - 1}))
}

/// `dmx.trigger_remove {trigger}`.
pub fn dmx_trigger_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "trigger", e.show.dmx_triggers.len())?;
    e.show.dmx_triggers.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `dmx.trigger_list`: every DMX trigger.
pub fn dmx_trigger_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .dmx_triggers
            .iter()
            .map(|t| json!({"universe": t.universe, "channel": t.channel, "command": t.command}))
            .collect(),
    ))
}

/// `dmx.input_list`: the Art-Net port addresses DMX came in on.
pub fn dmx_input_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(json!(e.dmx.inputs()))
}

/// `dmx.input_get {network_universe}`: the levels heard on an Art-Net port address, channel 1
/// first.
pub fn dmx_input_get(e: &mut Engine, prm: &Value) -> CommandResult {
    let n = p::i64_in(prm, "network_universe", 0, i64::from(DmxOutput::ARTNET_MAX))?;
    let n = u16::try_from(n).map_err(|_| invalid("no such universe"))?;
    let levels = e.dmx.input(n).ok_or_else(|| failed("no DMX came in on that universe"))?;
    Ok(json!(levels.to_vec()))
}

/// `dmx.get {universe}`: the levels of one universe, channel 1 first.
pub fn dmx_get(e: &mut Engine, prm: &Value) -> CommandResult {
    let n = universe(prm)?;
    let levels = e.dmx.universe(n).map(|u| u.to_vec()).unwrap_or_default();
    Ok(json!({"universe": n, "levels": levels}))
}

/// `dmx.output_add {universe, protocol: artnet|sacn, network_universe?, address?, priority?}`:
/// sends a universe over the network. Without `network_universe`, Art-Net counts from 0 (show
/// universe 1 is port address 0) and sACN from 1; without `address` it goes to everyone on the
/// network.
pub fn dmx_output_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let universe = u16::try_from(universe(prm)?).map_err(|_| invalid("no such universe"))?;
    let protocol = match p::str_max(prm, "protocol", 16)? {
        "artnet" => DmxProtocol::ArtNet,
        "sacn" => DmxProtocol::Sacn,
        "usb_pro" => DmxProtocol::UsbPro,
        _ => return Err(invalid("`protocol` must be artnet, sacn or usb_pro")),
    };
    let (low, high) = match protocol {
        DmxProtocol::ArtNet => (0, DmxOutput::ARTNET_MAX),
        DmxProtocol::Sacn => (DmxOutput::SACN_MIN, DmxOutput::SACN_MAX),
        DmxProtocol::UsbPro => (0, 0),
    };
    let network_universe = match p::opt_i64_in(prm, "network_universe", i64::from(low), i64::from(high))? {
        Some(n) => u16::try_from(n).unwrap_or(low),
        None => match protocol {
            DmxProtocol::ArtNet => universe - 1,
            DmxProtocol::Sacn => universe,
            // A USB interface sends the one universe it is given.
            DmxProtocol::UsbPro => 0,
        },
    };
    let address = p::opt_str_max(prm, "address", 64)?.unwrap_or_default().trim().to_owned();
    if protocol == DmxProtocol::UsbPro {
        if !DmxOutput::valid_port_name(&address) {
            return Err(invalid("`address` must name the serial port, like COM3 or /dev/ttyUSB0"));
        }
    } else if !address.is_empty() && address.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(invalid("`address` must be an IPv4 address like 192.168.1.20, or empty"));
    }
    let priority = p::opt_i64_in(prm, "priority", 0, i64::from(DmxOutput::PRIORITY_MAX))?.unwrap_or(100);
    if e.show.dmx_outputs.len() >= actcraft_show::limits::MAX_DMX_OUTPUTS {
        return Err(failed("a show holds at most 256 DMX outputs"));
    }
    e.show.dmx_outputs.push(DmxOutput {
        universe,
        protocol,
        network_universe,
        address,
        priority: u8::try_from(priority).unwrap_or(100),
        ..DmxOutput::default()
    });
    e.mark_changed();
    Ok(json!({"output": e.show.dmx_outputs.len() - 1}))
}

/// `dmx.output_remove {output}`.
pub fn dmx_output_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "output", e.show.dmx_outputs.len())?;
    e.show.dmx_outputs.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `dmx.output_list`: every output.
pub fn dmx_output_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .dmx_outputs
            .iter()
            .map(|o| {
                let protocol = match o.protocol {
                    DmxProtocol::ArtNet => "artnet",
                    DmxProtocol::Sacn => "sacn",
                    DmxProtocol::UsbPro => "usb_pro",
                };
                json!({"universe": o.universe, "protocol": protocol, "network_universe": o.network_universe, "address": o.address, "priority": o.priority})
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{Attribute, value_of};

    #[test]
    fn outputs_are_added_listed_and_removed() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        assert_eq!(
            e.run("dmx.output_add", &json!({"universe": 1, "protocol": "artnet"})).ok(),
            Some(json!({"output": 0}))
        );
        let r = e
            .run("dmx.output_add", &json!({"universe": 2, "protocol": "sacn", "address": "10.0.0.9", "priority": 150}));
        assert!(r.is_ok());
        let list = e.run("dmx.output_list", &Value::Null).ok().unwrap_or_default();
        assert_eq!(list[0]["network_universe"], json!(0), "Art-Net counts from 0");
        assert_eq!(list[1]["network_universe"], json!(2), "sACN from 1");
        assert_eq!(e.show.dmx_outputs[1].target(), Some(std::net::Ipv4Addr::new(10, 0, 0, 9)));
        for bad in [
            json!({"universe": 1, "protocol": "dmx"}),
            json!({"universe": 0, "protocol": "artnet"}),
            json!({"universe": 1, "protocol": "sacn", "network_universe": 0}),
            json!({"universe": 1, "protocol": "artnet", "network_universe": 40000}),
            json!({"universe": 1, "protocol": "sacn", "address": "lights.local"}),
            json!({"universe": 1, "protocol": "sacn", "priority": 201}),
        ] {
            assert!(e.run("dmx.output_add", &bad).is_err(), "{bad}");
        }
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.dmx_outputs).unwrap_or_default();
        assert_eq!(back, e.show.dmx_outputs);
        assert!(e.run("dmx.output_remove", &json!({"output": 0})).is_ok());
        assert_eq!(e.show.dmx_outputs.len(), 1);
    }

    #[test]
    fn dmx_from_other_gear_is_kept_by_universe_apart_from_ours() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        assert_eq!(e.run("dmx.input_list", &Value::Null).ok(), Some(json!([])));
        e.dmx_in(5, &[10, 20, 30]);
        e.dmx_in(1, &[255; 600]);
        assert_eq!(e.run("dmx.input_list", &Value::Null).ok(), Some(json!([1, 5])));
        let got = e.run("dmx.input_get", &json!({"network_universe": 5})).ok().unwrap_or_default();
        assert_eq!((got[0].clone(), got[2].clone(), got[3].clone()), (json!(10), json!(30), json!(0)));
        assert_eq!(got.as_array().map(Vec::len), Some(CHANNELS), "padded and cut to 512");
        assert!(e.run("dmx.input_get", &json!({"network_universe": 9})).is_err(), "nothing came there");
        assert_eq!(e.dmx.universe(1).map(|u| u[0]), Some(0), "the show's own levels stay");
        for n in 100..400 {
            e.dmx_in(n, &[1]);
        }
        assert_eq!(e.dmx.inputs().len(), 256, "bounded");
    }

    #[test]
    fn a_channel_from_a_desk_going_up_past_half_runs_its_command_once() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let on = |e: &Engine| e.show.metronome.enabled;
        let start = on(&e);
        let add = |e: &mut Engine, prm: Value| e.run("dmx.trigger_add", &prm);
        assert_eq!(
            add(&mut e, json!({"universe": 2, "channel": 3, "command": "metronome.toggle"})).ok(),
            Some(json!({"trigger": 0}))
        );
        assert!(add(&mut e, json!({"universe": 2, "channel": 513, "command": "metronome.toggle"})).is_err());
        assert!(add(&mut e, json!({"universe": 2, "channel": 3, "command": "dmx.set"})).is_err(), "needs params");
        let level = |v: u8| [0, 0, v];
        assert_eq!(e.dmx_in(2, &level(100)), 0, "below half");
        assert_eq!(e.dmx_in(2, &level(200)), 1, "up past half");
        assert_eq!(on(&e), !start);
        assert_eq!(e.dmx_in(2, &level(255)), 0, "still up: not again");
        assert_eq!(e.dmx_in(3, &level(0)), 0);
        assert_eq!(e.dmx_in(3, &level(255)), 0, "another universe");
        assert_eq!(e.dmx_in(2, &level(0)), 0);
        assert_eq!(e.dmx_in(2, &level(128)), 1, "down and up again");
        assert_eq!(on(&e), start);
        let list = e.run("dmx.trigger_list", &Value::Null).ok().unwrap_or_default();
        assert_eq!(list, json!([{"universe": 2, "channel": 3, "command": "metronome.toggle"}]));
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.dmx_triggers).unwrap_or_default();
        assert_eq!(back, e.show.dmx_triggers);
        assert!(e.run("dmx.trigger_remove", &json!({"trigger": 0})).is_ok());
        assert_eq!(e.dmx_in(2, &level(0)) + e.dmx_in(2, &level(255)), 0, "gone");
    }

    #[test]
    fn a_usb_output_names_its_serial_port() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let add = |e: &mut Engine, address: &str| {
            e.run("dmx.output_add", &json!({"universe": 2, "protocol": "usb_pro", "address": address}))
        };
        assert_eq!(add(&mut e, "COM3").ok(), Some(json!({"output": 0})));
        assert!(add(&mut e, "/dev/ttyUSB0").is_ok());
        assert!(add(&mut e, "").is_err(), "a USB output needs its port");
        assert!(add(&mut e, "COM3; rm").is_err());
        assert!(add(&mut e, &"A".repeat(65)).is_err());
        let list = e.run("dmx.output_list", &Value::Null).ok().unwrap_or_default();
        assert_eq!(list[0]["protocol"], json!("usb_pro"));
        assert_eq!(list[1]["address"], json!("/dev/ttyUSB0"));
        assert_eq!(list[0]["network_universe"], json!(0));
        // Saved and loaded; a file with a bad port name loses it.
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.dmx_outputs).unwrap_or_default();
        assert_eq!(back, e.show.dmx_outputs);
        let file = br#"{"format":"actcraft-show","version":1,"name":"U",
            "dmx_outputs":[{"universe":1,"protocol":"usb_pro","address":"COM1\n"}]}"#;
        let loaded = actcraft_show::Show::from_bytes(file).map(|s| s.dmx_outputs).unwrap_or_default();
        assert_eq!(loaded.first().map(|o| o.address.as_str()), Some(""));
    }

    #[test]
    fn blind_keeps_fixture_changes_and_recalls_off_the_lights() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "count": 2}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.2}}));
        let _ = e.run("preset.store", &json!({"name": "Low"}));
        let live = |e: &Engine| e.dmx.universe(1).map(|u| [u[0], u[1]]);
        assert_eq!(live(&e), Some([51, 0]));
        assert_eq!(e.run("lighting.blind", &Value::Null).ok(), Some(json!({"on": true})));
        let version = e.dmx.version;
        // A new look is built and stored without the lights moving.
        let _ = e.run("fixture.set", &json!({"fixtures": [0, 1], "values": {"intensity": 1.0}}));
        assert_eq!(live(&e), Some([51, 0]), "the lights stay");
        assert_eq!(e.dmx.version, version, "nothing new to send");
        let f = e.show.fixtures[1].clone();
        assert_eq!(value_of(&e, &f, Attribute::Intensity), Some(1.0), "edits see the blind levels");
        assert!(e.run("preset.store", &json!({"name": "Full"})).is_ok());
        assert_eq!(e.show.presets[1].values.iter().map(|v| v.value).collect::<Vec<_>>(), vec![1.0, 1.0]);
        // A recall in blind goes to the copy at once, even with a fade.
        assert!(e.run("preset.recall", &json!({"preset": 0, "fade": 3.0})).is_ok());
        assert!(!e.fading());
        assert_eq!(e.dmx.edited(1).map(|u| [u[0], u[1]]), Some([51, 0]), "the copy holds the recalled look");
        assert_eq!(live(&e), Some([51, 0]));
        // Cues play live in blind.
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        assert!(e.run("cue.go", &Value::Null).is_ok());
        assert_eq!(live(&e), Some([255, 255]), "the cue reached the lights");
        // Off: blind's copy goes; edits see the lights again.
        assert_eq!(e.run("lighting.blind", &json!({"on": false})).ok(), Some(json!({"on": false})));
        assert!(!e.dmx.blind());
        assert_eq!(e.dmx.edited(1).map(|u| [u[0], u[1]]), Some([255, 255]));
        assert_eq!(e.run("lighting.blind", &json!({"on": false})).ok(), Some(json!({"on": false})), "stays off");
    }

    #[test]
    fn blackout_and_master_dim_fixtures_at_the_output_only() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        // A moving head (16-bit intensity: no, 8-bit with pan) and an RGB light without a dimmer.
        let _ = e.run("fixture.add", &json!({"channels": ["pan", "intensity", "intensity_fine"], "universe": 1}));
        let _ = e.run("fixture.add", &json!({"kind": "rgb", "universe": 1}));
        let _ =
            e.run("fixture.set", &json!({"fixtures": [0, 1], "values": {"pan": 0.5, "intensity": 1.0, "red": 1.0}}));
        let out = |e: &Engine| e.output_universe(1).unwrap_or([0; CHANNELS]);
        assert_eq!(out(&e)[..6], [128, 255, 255, 255, 0, 0], "as set");
        let before = e.dmx.version;
        assert_eq!(e.run("dmx.blackout", &Value::Null).ok(), Some(json!({"on": true})));
        assert!(e.dmx.version > before, "the output sends again");
        assert_eq!(out(&e)[..6], [128, 0, 0, 0, 0, 0], "dark, but the head keeps its pan");
        assert_eq!(e.dmx.universe(1).map(|u| u[1]), Some(255), "the levels stay");
        assert_eq!(e.run("dmx.blackout", &Value::Null).ok(), Some(json!({"on": false})));
        assert!(e.run("dmx.master", &json!({"level": 0.5})).is_ok());
        let half = out(&e);
        assert_eq!(half[0], 128, "pan kept");
        assert!((127..=128).contains(&half[1]) && (127..=128).contains(&half[3]), "intensity and red halved: {half:?}");
        assert!(e.run("dmx.master", &json!({"level": 1.5})).is_err());
    }

    #[test]
    fn freeze_holds_a_fade_and_goes_on_from_there() {
        use chrono::TimeDelta;
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Full"}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let start = e.services().now();
        let _ = e.run("preset.recall", &json!({"preset": 0, "fade": 2.0}));
        e.step_fade(start + TimeDelta::milliseconds(1000));
        let held = e.dmx.universe(1).map_or(0, |u| u[0]);
        assert_eq!(e.run("lighting.freeze", &Value::Null).ok(), Some(json!({"on": true})));
        assert!(e.frozen());
        // While frozen the tick leaves the fade alone.
        e.tick();
        assert_eq!(e.dmx.universe(1).map_or(0, |u| u[0]), held);
        assert!(e.run("lighting.freeze", &json!({"on": false})).is_ok());
        assert!(!e.frozen() && e.fading(), "the fade goes on");
    }

    #[test]
    fn levels_are_set_read_and_cleared() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let before = e.dmx.version;
        assert_eq!(
            e.run("dmx.set", &json!({"universe": 2, "channel": 512, "value": 255})).ok(),
            Some(json!({"changed": true}))
        );
        assert!(e.dmx.version > before, "an output sees the change");
        assert_eq!(
            e.run("dmx.set", &json!({"universe": 2, "channel": 512, "value": 255})).ok(),
            Some(json!({"changed": false}))
        );
        assert!(e.run("dmx.set", &json!({"universe": 1, "channel": 10, "values": [1, 2, 3]})).is_ok());
        let one = e.run("dmx.get", &json!({"universe": 1})).ok().unwrap_or_default();
        assert_eq!(one["levels"].as_array().and_then(|l| l.get(9..12)), Some(&[json!(1), json!(2), json!(3)][..]));
        assert_eq!(e.dmx.universe(2).map(|u| u[511]), Some(255));
        for bad in [
            json!({"universe": 0, "channel": 1, "value": 1}),
            json!({"universe": 65, "channel": 1, "value": 1}),
            json!({"universe": 1, "channel": 513, "value": 1}),
            json!({"universe": 1, "channel": 1, "value": 256}),
            json!({"universe": 1, "channel": 511, "values": [1, 2, 3]}),
            json!({"universe": 1, "channel": 1, "values": [1, -1]}),
            json!({"universe": 1, "channel": 1, "values": []}),
            json!({"universe": 1, "channel": 1}),
        ] {
            assert!(e.run("dmx.set", &bad).is_err(), "{bad}");
        }
        assert!(e.run("dmx.clear", &json!({"universe": 1})).is_ok());
        assert_eq!(e.dmx.universe(1).map(|u| u.iter().all(|v| *v == 0)), Some(true));
        assert_eq!(e.dmx.universe(2).map(|u| u[511]), Some(255), "only the one universe");
        assert!(e.run("dmx.clear", &Value::Null).is_ok());
        assert_eq!(e.dmx.universe(2).map(|u| u[511]), Some(0));
    }
}
