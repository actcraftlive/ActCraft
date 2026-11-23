//! Pads: buttons on the screen that run a command or a macro with one press, for what a band
//! needs at hand during a show (the next song, the click, a light change).

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_PADS;
use actcraft_show::{Macro, Pad, parse_colour};
use serde_json::{Value, json};

use crate::Engine;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// `pad.add {label, colour?, command? | macro?}`: a pad that runs a command that needs nothing
/// more, or a macro.
pub fn pad_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let label = p::str_max(prm, "label", 64)?.trim().to_owned();
    if label.is_empty() {
        return Err(invalid("a pad needs a label"));
    }
    let command = p::opt_str_max(prm, "command", 64)?.unwrap_or_default().to_owned();
    let macro_name = p::opt_str_max(prm, "macro", 32)?.unwrap_or_default().trim().to_ascii_uppercase();
    match (command.is_empty(), macro_name.is_empty()) {
        (false, true) if crate::remote::runnable(&command) => {}
        (true, false) if Macro::valid_name(&macro_name) => {}
        (false, false) | (true, true) => return Err(invalid("a pad runs a command or a macro, one of them")),
        _ => return Err(invalid("a pad runs a command that needs nothing more, or a macro")),
    }
    let mut pad = Pad { label, command, macro_name, ..Pad::default() };
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
            .map(|p| json!({"label": p.label, "colour": p.colour, "command": p.command, "macro": p.macro_name}))
            .collect(),
    ))
}

/// `pad.press {pad}`: runs what the pad runs.
pub fn pad_press(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "pad", e.show.pads.len())?;
    let Some(pad) = e.show.pads.get(i).cloned() else { return Err(invalid("no such pad")) };
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
        assert_eq!(e.run("pad.list", &Value::Null).ok().map(|v| v[1]["macro"].clone()), Some(json!("BACK")));
        assert!(e.run("pad.remove", &json!({"pad": 0})).is_ok());
        assert_eq!(e.show.pads.len(), 1);
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.pads).unwrap_or_default();
        assert_eq!(back, e.show.pads);
    }
}
