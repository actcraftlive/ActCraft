//! DMX levels: what each channel of each universe is at now. They are live, not saved with the
//! show; lighting outputs send them, and the DMX monitor shows them.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
pub use actcraft_show::{DmxOutput, DmxProtocol};
use serde_json::{Value, json};

use crate::Engine;

/// Universes ActCraft drives, numbered from 1.
pub const UNIVERSES: usize = actcraft_show::limits::DMX_UNIVERSES;
/// Channels in a universe, numbered from 1.
pub const CHANNELS: usize = 512;

/// The levels of every universe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dmx {
    universes: Vec<[u8; CHANNELS]>,
    /// Goes up with every change, for outputs to see that something changed.
    pub version: u64,
}

impl Default for Dmx {
    fn default() -> Self {
        Self { universes: vec![[0; CHANNELS]; UNIVERSES], version: 0 }
    }
}

impl Dmx {
    /// The levels of `universe` (from 1); `None` outside 1..=[`UNIVERSES`].
    pub fn universe(&self, universe: usize) -> Option<&[u8; CHANNELS]> {
        self.universes.get(universe.checked_sub(1)?)
    }

    /// Sets channels from `channel` (from 1) on; true when a level changed.
    pub(crate) fn set(&mut self, universe: usize, channel: usize, values: &[u8]) -> bool {
        let Some(u) = universe.checked_sub(1).and_then(|i| self.universes.get_mut(i)) else { return false };
        let Some(from) = channel.checked_sub(1) else { return false };
        let Some(slots) = u.get_mut(from..from.saturating_add(values.len()).min(CHANNELS)) else { return false };
        let changed = slots.iter().zip(values).any(|(a, b)| a != b);
        for (slot, v) in slots.iter_mut().zip(values) {
            *slot = *v;
        }
        if changed {
            self.version += 1;
        }
        changed
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
        _ => return Err(invalid("`protocol` must be artnet or sacn")),
    };
    let (low, high) = match protocol {
        DmxProtocol::ArtNet => (0, DmxOutput::ARTNET_MAX),
        DmxProtocol::Sacn => (DmxOutput::SACN_MIN, DmxOutput::SACN_MAX),
    };
    let network_universe = match p::opt_i64_in(prm, "network_universe", i64::from(low), i64::from(high))? {
        Some(n) => u16::try_from(n).unwrap_or(low),
        None if protocol == DmxProtocol::ArtNet => universe - 1,
        None => universe,
    };
    let address = p::opt_str_max(prm, "address", 64)?.unwrap_or_default().trim().to_owned();
    if !address.is_empty() && address.parse::<std::net::Ipv4Addr>().is_err() {
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
                };
                json!({"universe": o.universe, "protocol": protocol, "network_universe": o.network_universe, "address": o.address, "priority": o.priority})
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
