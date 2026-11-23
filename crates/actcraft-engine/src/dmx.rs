//! DMX levels: what each channel of each universe is at now. They are live, not saved with the
//! show; lighting outputs send them, and the DMX monitor shows them.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use serde_json::{Value, json};

use crate::Engine;

/// Universes ActCraft drives, numbered from 1.
pub const UNIVERSES: usize = 64;
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
    fn set(&mut self, universe: usize, channel: usize, values: &[u8]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

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
