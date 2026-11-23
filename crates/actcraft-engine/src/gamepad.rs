//! Game controllers: a button pressed on any connected controller (a gamepad, an arcade stick, a
//! wireless one on a mic stand) sets off a command, like a MIDI remote action. The platform layer
//! reads the controllers; this only decides.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_GAMEPAD_ACTIONS;
use serde_json::{Value, json};

pub use actcraft_show::GamepadAction;

use crate::Engine;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

fn button(prm: &Value) -> Result<String, CommandError> {
    let b = p::str_max(prm, "button", 32)?;
    if GamepadAction::BUTTONS.contains(&b) {
        Ok(b.to_owned())
    } else {
        Err(invalid("`button` must be a controller button like south, start or dpad_up"))
    }
}

impl Engine {
    /// Runs the commands a controller button sets off; returns how many ran.
    pub fn gamepad_in(&mut self, button: &str) -> usize {
        let due: Vec<String> =
            self.show.gamepad_actions.iter().filter(|a| a.button == button).map(|a| a.command.clone()).collect();
        let mut ran = 0;
        for command in due {
            if crate::remote::runnable(&command) && self.run(&command, &Value::Null).is_ok() {
                ran += 1;
            }
        }
        ran
    }
}

/// `gamepad.add {button, command}`: a controller button runs a command that needs nothing more.
pub fn gamepad_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let button = button(prm)?;
    let command = p::str_max(prm, "command", 64)?.to_owned();
    if !crate::remote::runnable(&command) {
        return Err(invalid("a button runs a command that needs nothing more"));
    }
    if e.show.gamepad_actions.len() >= MAX_GAMEPAD_ACTIONS {
        return Err(failed("a show holds at most 128 controller buttons"));
    }
    e.show.gamepad_actions.push(GamepadAction { button, command, ..GamepadAction::default() });
    e.mark_changed();
    Ok(json!({"action": e.show.gamepad_actions.len() - 1}))
}

/// `gamepad.remove {action}`.
pub fn gamepad_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "action", e.show.gamepad_actions.len())?;
    e.show.gamepad_actions.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `gamepad.list`: every controller button and what it runs.
pub fn gamepad_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(e.show.gamepad_actions.iter().map(|a| json!({"button": a.button, "command": a.command})).collect()))
}

/// `gamepad.press {button}`: acts as if the button was pressed.
pub fn gamepad_press(e: &mut Engine, prm: &Value) -> CommandResult {
    let button = button(prm)?;
    Ok(json!({"ran": e.gamepad_in(&button)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_button_runs_its_commands() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let start = e.show.metronome.enabled;
        assert_eq!(
            e.run("gamepad.add", &json!({"button": "south", "command": "metronome.toggle"})).ok(),
            Some(json!({"action": 0}))
        );
        assert!(e.run("gamepad.add", &json!({"button": "turbo", "command": "metronome.toggle"})).is_err());
        assert!(e.run("gamepad.add", &json!({"button": "north", "command": "dmx.set"})).is_err(), "needs params");
        assert_eq!(e.gamepad_in("south"), 1);
        assert_eq!(e.show.metronome.enabled, !start);
        assert_eq!(e.gamepad_in("east"), 0, "nothing on it");
        assert_eq!(e.run("gamepad.press", &json!({"button": "south"})).ok(), Some(json!({"ran": 1})));
        assert_eq!(e.show.metronome.enabled, start);
        assert_eq!(
            e.run("gamepad.list", &Value::Null).ok(),
            Some(json!([{"button": "south", "command": "metronome.toggle"}]))
        );
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.gamepad_actions).unwrap_or_default();
        assert_eq!(back, e.show.gamepad_actions);
        let file = br#"{"format":"actcraft-show","version":1,"name":"G",
            "gamepad_actions":[{"button":"turbo","command":"metronome.toggle"}]}"#;
        assert_eq!(actcraft_show::Show::from_bytes(file).map(|s| s.gamepad_actions.len()).ok(), Some(0));
        assert!(e.run("gamepad.remove", &json!({"action": 0})).is_ok());
        assert_eq!(e.gamepad_in("south"), 0);
    }
}
