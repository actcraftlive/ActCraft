//! OSC remote control: a tablet or another program sends Open Sound Control messages, and
//! these addresses run things:
//!
//! - `/actcraft/<command id>` runs a command that needs nothing more (`/actcraft/transport.toggle`);
//! - `/actcraft/macro/<NAME>` runs a macro;
//! - `/actcraft/pad/<n>` presses pad n (from 1).
//!
//! A message whose first value is 0 or less (a button let go) does nothing, so buttons that send
//! on press and release act once. The platform layer receives the messages; this only decides.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}
use serde_json::{Value, json};

use crate::Engine;

/// Every address starts with this.
pub const PREFIX: &str = "/actcraft/";

impl Engine {
    /// Runs what an OSC message at `address` with first value `value` asks for; how many
    /// commands ran.
    pub fn osc_in(&mut self, address: &str, value: Option<f64>) -> usize {
        if value.is_some_and(|v| v.is_nan() || v <= 0.0) {
            return 0;
        }
        let Some(rest) = address.strip_prefix(PREFIX) else { return 0 };
        if let Some(name) = rest.strip_prefix("macro/") {
            return self.run_macro(&name.to_ascii_uppercase()).unwrap_or(0);
        }
        if let Some(n) = rest.strip_prefix("pad/") {
            let Some(i) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else { return 0 };
            return match self.run("pad.press", &json!({"pad": i})) {
                Ok(v) => v["ran"].as_u64().map_or(1, |n| n as usize),
                Err(_) => 0,
            };
        }
        if crate::remote::runnable(rest) && rest != "osc.trigger" {
            return usize::from(self.run(rest, &Value::Null).is_ok());
        }
        0
    }
}

/// `osc.trigger {address, value?}`: acts as if the OSC message came in.
pub fn osc_trigger(e: &mut Engine, prm: &Value) -> CommandResult {
    let address = p::str_max(prm, "address", 256)?;
    if !address.starts_with(PREFIX) {
        return Err(invalid("an ActCraft OSC address starts with /actcraft/"));
    }
    let value = p::opt_f64_in(prm, "value", -1e9, 1e9)?;
    Ok(json!({"ran": e.osc_in(address, value)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_run_commands_macros_and_pads() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.add", &Value::Null);
        let first = e.current_song;
        assert_eq!(e.osc_in("/actcraft/transport.next_song", Some(1.0)), 1);
        let second = e.current_song;
        assert_ne!(second, first);
        assert_eq!(e.osc_in("/actcraft/transport.next_song", Some(0.0)), 0, "a button let go");
        assert_eq!(e.current_song, second);
        assert!(e.run("macro.set", &json!({"name": "BACK", "commands": ["transport.previous_song"]})).is_ok());
        assert_eq!(e.osc_in("/actcraft/macro/back", None), 1);
        assert_eq!(e.current_song, first);
        assert!(e.run("pad.add", &json!({"label": "Next", "command": "transport.next_song"})).is_ok());
        assert_eq!(e.osc_in("/actcraft/pad/1", Some(1.0)), 1);
        assert_eq!(e.current_song, second);
        // Nothing runs for other addresses, commands needing more, or this one itself.
        for address in
            ["/other/x", "/actcraft/show.open", "/actcraft/pad/0", "/actcraft/pad/9", "/actcraft/osc.trigger"]
        {
            assert_eq!(e.osc_in(address, Some(1.0)), 0, "{address}");
        }
        assert_eq!(
            e.run("osc.trigger", &json!({"address": "/actcraft/transport.previous_song"})).ok(),
            Some(json!({"ran": 1}))
        );
        assert!(e.run("osc.trigger", &json!({"address": "/else"})).is_err());
    }
}
