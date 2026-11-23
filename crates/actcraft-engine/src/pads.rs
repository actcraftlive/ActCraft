//! Pads: buttons on the screen that run a command or a macro with one press, for what a band
//! needs at hand during a show (the next song, the click, a light change).

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_PADS;
use actcraft_show::{FaderTarget, Macro, Pad, ToggleTarget, parse_colour};
use serde_json::{Value, json};

use crate::Engine;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// `pad.add {label, colour?, command? | macro? | fader?}`: a pad that runs a command that needs
/// nothing more or a macro, or a fader that moves a volume (`master`, `song`, or `bus` with
/// `bus: index`).
pub fn pad_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let label = p::str_max(prm, "label", 64)?.trim().to_owned();
    if label.is_empty() {
        return Err(invalid("a pad needs a label"));
    }
    let command = p::opt_str_max(prm, "command", 64)?.unwrap_or_default().to_owned();
    let macro_name = p::opt_str_max(prm, "macro", 32)?.unwrap_or_default().trim().to_ascii_uppercase();
    let fader = match p::opt_str_max(prm, "fader", 16)? {
        None => None,
        Some("master") => Some(FaderTarget::Master),
        Some("song") => Some(FaderTarget::Song),
        Some("bus") => {
            let bus = p::i64_in(prm, "bus", 0, actcraft_show::limits::BUSES as i64 - 1)?;
            Some(FaderTarget::Bus(u8::try_from(bus).unwrap_or(0)))
        }
        Some(_) => return Err(invalid("`fader` must be master, song or bus")),
    };
    let toggle = match p::opt_str_max(prm, "toggle", 16)? {
        None => None,
        Some("metronome") => Some(ToggleTarget::Metronome),
        Some("loop_song") => Some(ToggleTarget::LoopSong),
        Some("autoplay") => Some(ToggleTarget::Autoplay),
        Some("blackout") => Some(ToggleTarget::Blackout),
        Some("freeze") => Some(ToggleTarget::Freeze),
        Some(_) => return Err(invalid("`toggle` must be metronome, loop_song, autoplay, blackout or freeze")),
    };
    let kinds = [!command.is_empty(), !macro_name.is_empty(), fader.is_some(), toggle.is_some()];
    if kinds.iter().filter(|k| **k).count() != 1 {
        return Err(invalid("a pad runs a command or a macro, moves a volume or switches something: one of them"));
    }
    if (!command.is_empty() && !crate::remote::runnable(&command))
        || (!macro_name.is_empty() && !Macro::valid_name(&macro_name))
    {
        return Err(invalid("a pad runs a command that needs nothing more, or a macro"));
    }
    let mut pad = Pad { label, command, macro_name, fader, toggle, ..Pad::default() };
    if let Some(c) = p::opt_str_max(prm, "colour", 16)? {
        if parse_colour(c).is_none() {
            return Err(invalid("`colour` must look like #rrggbb"));
        }
        c.clone_into(&mut pad.colour);
    }
    if e.show.pads.len() >= MAX_PADS {
        return Err(failed("a show holds at most 64 pads"));
    }
    e.show.pads.push(pad);
    e.mark_changed();
    Ok(json!({"pad": e.show.pads.len() - 1}))
}

/// Whether what a toggle pad switches is on now.
pub fn toggle_state(e: &Engine, t: ToggleTarget) -> bool {
    match t {
        ToggleTarget::Metronome => e.show.metronome.enabled,
        ToggleTarget::LoopSong => e.current().is_some_and(|s| s.loop_song),
        ToggleTarget::Autoplay => e.current().is_some_and(|s| s.autoplay),
        ToggleTarget::Blackout => e.dmx.blackout,
        ToggleTarget::Freeze => e.frozen(),
    }
}

/// `pad.remove {pad}`.
pub fn pad_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    e.show.pads.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `pad.list`: every pad.
pub fn pad_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(
        e.show
            .pads
            .iter()
            .map(|p| {
                json!({"label": p.label, "colour": p.colour, "command": p.command, "macro": p.macro_name, "fader": p.fader, "toggle": p.toggle})
            })
            .collect(),
    ))
}

/// `pad.press {pad}`: runs what the pad runs.
pub fn pad_press(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    let Some(pad) = e.show.pads.get(i).cloned() else { return Err(invalid("no such pad")) };
    if pad.fader.is_some() {
        return Err(invalid("a fader is moved, not pressed"));
    }
    if let Some(t) = pad.toggle {
        let on = toggle_state(e, t);
        match t {
            ToggleTarget::Metronome => {
                e.run("metronome.toggle", &Value::Null)?;
            }
            ToggleTarget::Blackout => {
                e.run("dmx.blackout", &Value::Null)?;
            }
            ToggleTarget::Freeze => {
                e.run("lighting.freeze", &Value::Null)?;
            }
            ToggleTarget::LoopSong | ToggleTarget::Autoplay => {
                let id = e.current_song.ok_or_else(|| invalid("no song chosen"))?;
                let field = if t == ToggleTarget::LoopSong { "loop_song" } else { "autoplay" };
                e.run("song.set", &json!({"song": id, field: !on}))?;
            }
        }
        return Ok(json!({"on": toggle_state(e, t)}));
    }
    if !pad.macro_name.is_empty() {
        let ran = e.run_macro(&pad.macro_name)?;
        return Ok(json!({"ran": ran}));
    }
    if !crate::remote::runnable(&pad.command) {
        return Err(failed("this pad's command can't run without more"));
    }
    e.run(&pad.command, &Value::Null)?;
    Ok(json!({"ran": 1}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_switch_and_show_their_state() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        assert!(e.run("pad.add", &json!({"label": "Click", "toggle": "metronome"})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "Loop", "toggle": "loop_song"})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "x", "toggle": "lights"})).is_err());
        assert!(e.run("pad.add", &json!({"label": "x", "toggle": "autoplay", "command": "transport.stop"})).is_err());
        let click = toggle_state(&e, ToggleTarget::Metronome);
        assert_eq!(e.run("pad.press", &json!({"pad": 0})).ok(), Some(json!({"on": !click})));
        assert_eq!(toggle_state(&e, ToggleTarget::Metronome), !click);
        assert!(!toggle_state(&e, ToggleTarget::LoopSong));
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"on": true})));
        assert!(e.current().is_some_and(|s| s.loop_song), "the song loops now");
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"on": false})));
    }

    #[test]
    fn pads_are_added_pressed_and_removed() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.add", &Value::Null);
        let first = e.current_song;
        let r = e.run("pad.add", &json!({"label": "Next", "command": "transport.next_song", "colour": "#3dc97a"}));
        assert_eq!(r.ok(), Some(json!({"pad": 0})));
        assert!(e.run("macro.set", &json!({"name": "BACK", "commands": ["transport.previous_song"]})).is_ok());
        assert!(e.run("pad.add", &json!({"label": "Back", "macro": "back"})).is_ok());
        for bad in [
            json!({"label": "", "command": "transport.stop"}),
            json!({"label": "x", "command": "show.open"}),
            json!({"label": "x", "command": "transport.stop", "macro": "A"}),
            json!({"label": "x", "command": "transport.stop", "fader": "master"}),
            json!({"label": "x", "fader": "bus", "bus": 99}),
            json!({"label": "x", "fader": "lights"}),
            json!({"label": "x"}),
            json!({"label": "x", "macro": "2BAD"}),
            json!({"label": "x", "command": "transport.stop", "colour": "green"}),
        ] {
            assert!(e.run("pad.add", &bad).is_err(), "{bad}");
        }
        assert_eq!(e.run("pad.press", &json!({"pad": 0})).ok(), Some(json!({"ran": 1})));
        assert_ne!(e.current_song, first);
        assert_eq!(e.run("pad.press", &json!({"pad": 1})).ok(), Some(json!({"ran": 1})), "the macro");
        assert_eq!(e.current_song, first);
        assert!(e.run("pad.press", &json!({"pad": 9})).is_err());
        let r = e.run("pad.add", &json!({"label": "Monitors", "fader": "bus", "bus": 2}));
        assert_eq!(r.ok(), Some(json!({"pad": 2})));
        assert_eq!(e.show.pads[2].fader, Some(FaderTarget::Bus(2)));
        assert!(e.run("pad.press", &json!({"pad": 2})).is_err(), "a fader is moved, not pressed");
        assert_eq!(e.run("pad.list", &Value::Null).ok().map(|v| v[1]["macro"].clone()), Some(json!("BACK")));
        assert!(e.run("pad.remove", &json!({"pad": 0})).is_ok());
        assert_eq!(e.show.pads.len(), 2);
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.pads).unwrap_or_default();
        assert_eq!(back, e.show.pads);
    }
}
