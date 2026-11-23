//! Game controllers: a button pressed on any connected controller (a gamepad, an arcade stick, a
//! wireless one on a mic stand) sets off a command, like a MIDI remote action. The platform layer
//! reads the controllers; this only decides.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_GAMEPAD_ACTIONS;
use serde_json::{Value, json};

use actcraft_show::FaderTarget;
pub use actcraft_show::{GamepadAction, GamepadFader};

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

/// Lowest and highest volume a stick or trigger moves between, in dB (as the fader pads).
const LOW_DB: f64 = -60.0;
const HIGH_DB: f64 = 12.0;

impl Engine {
    /// A stick or trigger moved to `value` (0-1, a stick's middle is 0.5): the volumes and the
    /// lighting master on it follow. Returns how many it moved.
    pub fn gamepad_axis(&mut self, axis: &str, value: f64) -> usize {
        if !value.is_finite() {
            return 0;
        }
        let v = value.clamp(0.0, 1.0);
        let db = LOW_DB + (HIGH_DB - LOW_DB) * v;
        let targets: Vec<Option<FaderTarget>> =
            self.show.gamepad_faders.iter().filter(|f| f.axis == axis).map(|f| f.volume).collect();
        let mut moved = 0;
        for target in targets {
            let done = match target {
                None => self.run("dmx.master", &json!({"level": v})),
                Some(FaderTarget::Master) => self.run("mixer.master_volume", &json!({"db": db})),
                Some(FaderTarget::Song) => match self.current_song {
                    Some(id) => self.run("song.set", &json!({"song": id, "volume_db": db})),
                    None => continue,
                },
                Some(FaderTarget::Bus(b)) => self.run("bus.set", &json!({"bus": b, "volume_db": db})),
            };
            moved += usize::from(done.is_ok());
        }
        moved
    }
}

fn axis(prm: &Value) -> Result<String, CommandError> {
    let a = p::str_max(prm, "axis", 32)?;
    if GamepadFader::AXES.contains(&a) {
        Ok(a.to_owned())
    } else {
        Err(invalid("`axis` must be left_trigger, right_trigger, left_x, left_y, right_x or right_y"))
    }
}

/// `gamepad.fader_add {axis, fader: master | song | bus | lighting, bus?}`: a stick or trigger
/// moves a volume or the lighting master.
pub fn gamepad_fader_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let axis = axis(prm)?;
    let volume = match p::str_max(prm, "fader", 16)? {
        "master" => Some(FaderTarget::Master),
        "song" => Some(FaderTarget::Song),
        "bus" => {
            let b = p::i64_in(prm, "bus", 0, actcraft_show::limits::BUSES as i64 - 1)?;
            Some(FaderTarget::Bus(u8::try_from(b).unwrap_or(0)))
        }
        "lighting" => None,
        _ => return Err(invalid("`fader` must be master, song, bus or lighting")),
    };
    if e.show.gamepad_faders.len() >= MAX_GAMEPAD_ACTIONS {
        return Err(failed("a show holds at most 128 controller faders"));
    }
    e.show.gamepad_faders.push(GamepadFader { axis, volume, ..GamepadFader::default() });
    e.mark_changed();
    Ok(json!({"fader": e.show.gamepad_faders.len() - 1}))
}

/// `gamepad.fader_remove {fader}`.
pub fn gamepad_fader_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "fader", e.show.gamepad_faders.len())?;
    e.show.gamepad_faders.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `gamepad.fader_list`: every stick and trigger and what it moves.
pub fn gamepad_fader_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .gamepad_faders
            .iter()
            .map(|f| {
                let (fader, bus) = match f.volume {
                    None => ("lighting", None),
                    Some(FaderTarget::Master) => ("master", None),
                    Some(FaderTarget::Song) => ("song", None),
                    Some(FaderTarget::Bus(b)) => ("bus", Some(b)),
                };
                json!({"axis": f.axis, "fader": fader, "bus": bus})
            })
            .collect(),
    ))
}

/// `gamepad.move {axis, value}`: acts as if the stick or trigger moved to `value` (0-1).
pub fn gamepad_move(e: &mut Engine, prm: &Value) -> CommandResult {
    let axis = axis(prm)?;
    let value = p::f64_in(prm, "value", 0.0, 1.0)?;
    Ok(json!({"moved": e.gamepad_axis(&axis, value)}))
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
    fn sticks_and_triggers_move_volumes_and_the_lighting_master() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let add = |e: &mut Engine, prm: Value| e.run("gamepad.fader_add", &prm);
        assert_eq!(add(&mut e, json!({"axis": "right_trigger", "fader": "lighting"})).ok(), Some(json!({"fader": 0})));
        assert!(add(&mut e, json!({"axis": "left_y", "fader": "master"})).is_ok());
        assert!(add(&mut e, json!({"axis": "left_z", "fader": "master"})).is_err());
        assert!(add(&mut e, json!({"axis": "left_x", "fader": "bus", "bus": 99})).is_err());
        assert!(add(&mut e, json!({"axis": "left_x", "fader": "laser"})).is_err());
        assert_eq!(e.gamepad_axis("right_trigger", 0.25), 1);
        assert!((e.dmx.master - 0.25).abs() < 1e-9);
        assert_eq!(e.gamepad_axis("left_y", 1.0), 1);
        assert!((e.show.mixer.master_volume_db - 12.0).abs() < 1e-9, "top of a stick: +12 dB");
        assert_eq!(e.run("gamepad.move", &json!({"axis": "left_y", "value": 0.0})).ok(), Some(json!({"moved": 1})));
        assert!((e.show.mixer.master_volume_db + 60.0).abs() < 1e-9);
        assert_eq!(e.gamepad_axis("left_x", 0.5), 0, "nothing on it");
        assert_eq!(e.gamepad_axis("right_trigger", f64::NAN), 0);
        assert_eq!(
            e.run("gamepad.fader_list", &Value::Null).ok(),
            Some(json!([{"axis": "right_trigger", "fader": "lighting", "bus": null},
                        {"axis": "left_y", "fader": "master", "bus": null}]))
        );
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.gamepad_faders).unwrap_or_default();
        assert_eq!(back, e.show.gamepad_faders);
        assert!(e.run("gamepad.fader_remove", &json!({"fader": 0})).is_ok());
        assert_eq!(e.gamepad_axis("right_trigger", 1.0), 0);
    }

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
