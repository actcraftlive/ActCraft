//! Remote actions: a MIDI controller (a foot switch, a pad, a keyboard) sets off commands. A
//! show keeps a list of triggers (a note, a controller or a program change, on one channel or
//! any) and the command each runs. The platform layer hands incoming MIDI bytes to
//! [`Engine::midi_in`]; `remote.trigger` does the same from the command line or an agent.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_REMOTE_ACTIONS;
use actcraft_show::{RemoteAction, RemoteKind, RemoteTrigger};
use serde_json::{Value, json};

use crate::{Engine, registry};

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// A controller counts as pressed from this value up.
const PRESSED: u8 = 64;

/// What a MIDI message is as a trigger, with its value (velocity, controller value; 127 for a
/// program change). `None` for anything a remote action can't answer (a Note Off, a Note On at
/// zero velocity, clock, SysEx...).
pub fn trigger_of(bytes: &[u8]) -> Option<(RemoteTrigger, u8)> {
    let &status = bytes.first()?;
    let channel = Some(status & 0x0F);
    let data = |i: usize| bytes.get(i).copied().filter(|b| *b < 0x80);
    match status & 0xF0 {
        0x90 => {
            let (note, velocity) = (data(1)?, data(2)?);
            (velocity > 0).then_some((RemoteTrigger { kind: RemoteKind::Note, channel, number: note }, velocity))
        }
        0xB0 => Some((RemoteTrigger { kind: RemoteKind::Cc, channel, number: data(1)? }, data(2)?)),
        0xC0 => Some((RemoteTrigger { kind: RemoteKind::Program, channel, number: data(1)? }, 127)),
        _ => None,
    }
}

/// Whether an action's trigger answers an incoming one.
fn answers(action: &RemoteTrigger, incoming: &RemoteTrigger) -> bool {
    action.kind == incoming.kind
        && action.number == incoming.number
        && action.channel.is_none_or(|c| Some(c) == incoming.channel)
}

/// A command a remote action may run: one that exists and needs no params.
pub fn runnable(id: &str) -> bool {
    id != "remote.trigger" && registry().get(id).is_some_and(|c| c.spec.runs_without_params())
}

impl Engine {
    /// Runs the remote actions an incoming MIDI message sets off; returns how many ran. A
    /// controller sets them off once on its way up past half (a foot switch pressed), not again
    /// while it stays up.
    pub fn midi_in(&mut self, bytes: &[u8]) -> usize {
        let Some((trigger, value)) = trigger_of(bytes) else { return 0 };
        if trigger.kind == RemoteKind::Cc {
            let key = (trigger.channel.unwrap_or(0), trigger.number);
            let was = self.remote_cc.insert(key, value).unwrap_or(0);
            if value < PRESSED || was >= PRESSED {
                return 0;
            }
        }
        let commands: Vec<String> = self
            .show
            .remote
            .iter()
            .filter(|a| answers(&a.trigger, &trigger) && runnable(&a.command))
            .map(|a| a.command.clone())
            .collect();
        for id in &commands {
            let _ = self.run(id, &Value::Null);
        }
        commands.len()
    }
}

/// The trigger in params: `kind` (note, cc, program), `channel` (1-16, or left out for any) and
/// `number` (0-127).
fn trigger_param(prm: &Value) -> Result<RemoteTrigger, CommandError> {
    let kind = match p::str_max(prm, "kind", 16)? {
        "note" => RemoteKind::Note,
        "cc" => RemoteKind::Cc,
        "program" => RemoteKind::Program,
        _ => return Err(invalid("`kind` must be note, cc or program")),
    };
    let channel = p::opt_i64_in(prm, "channel", 1, 16)?.map(|c| (c - 1) as u8);
    let number = p::i64_in(prm, "number", 0, 127)? as u8;
    Ok(RemoteTrigger { kind, channel, number })
}

fn action_json(a: &RemoteAction) -> Value {
    let kind = match a.trigger.kind {
        RemoteKind::Note => "note",
        RemoteKind::Cc => "cc",
        RemoteKind::Program => "program",
    };
    json!({
        "kind": kind,
        "channel": a.trigger.channel.map(|c| c + 1),
        "number": a.trigger.number,
        "command": a.command,
    })
}

/// `remote.add {kind, channel?, number, command}`: a MIDI message runs a command.
pub fn remote_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let trigger = trigger_param(prm)?;
    let command = p::str_max(prm, "command", 64)?;
    if !runnable(command) {
        return Err(invalid("a remote action runs a command that needs nothing more"));
    }
    if e.show.remote.len() >= MAX_REMOTE_ACTIONS {
        return Err(failed("a show holds at most 256 remote actions"));
    }
    e.show.remote.push(RemoteAction { trigger, command: command.to_owned(), ..RemoteAction::default() });
    e.mark_changed();
    Ok(json!({"action": e.show.remote.len() - 1}))
}

/// `remote.remove {action}`.
pub fn remote_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let i = p::index(prm, "action", e.show.remote.len())?;
    e.show.remote.remove(i);
    e.mark_changed();
    Ok(Value::Null)
}

/// `remote.list`: every remote action.
pub fn remote_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(e.show.remote.iter().map(action_json).collect()))
}

/// `remote.trigger {kind, channel?, number, value?}`: acts as if the MIDI message came in.
pub fn remote_trigger(e: &mut Engine, prm: &Value) -> CommandResult {
    let t = trigger_param(prm)?;
    let value = p::opt_i64_in(prm, "value", 0, 127)?.unwrap_or(127) as u8;
    let ch = t.channel.unwrap_or(0);
    let bytes = match t.kind {
        RemoteKind::Note => vec![0x90 | ch, t.number, value],
        RemoteKind::Cc => vec![0xB0 | ch, t.number, value],
        RemoteKind::Program => vec![0xC0 | ch, t.number],
    };
    Ok(json!({"ran": e.midi_in(&bytes)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(Box::new(crate::MemoryServices::default()))
    }

    #[test]
    fn midi_messages_become_triggers() {
        let note = |ch| Some(RemoteTrigger { kind: RemoteKind::Note, channel: Some(ch), number: 36 });
        assert_eq!(trigger_of(&[0x92, 36, 100]).map(|t| t.0), note(2));
        assert_eq!(trigger_of(&[0x90, 36, 0]), None, "a Note On at zero is a release");
        assert_eq!(trigger_of(&[0x80, 36, 64]), None, "releases set nothing off");
        assert_eq!(
            trigger_of(&[0xB0, 64, 127]),
            Some((RemoteTrigger { kind: RemoteKind::Cc, channel: Some(0), number: 64 }, 127))
        );
        assert_eq!(trigger_of(&[0xC5, 3]).map(|t| t.0.kind), Some(RemoteKind::Program));
        assert_eq!(trigger_of(&[0xF8]), None, "clock");
        assert_eq!(trigger_of(&[0x90, 200, 1]), None, "bad data");
    }

    #[test]
    fn actions_are_added_listed_set_off_and_removed() {
        let mut e = engine();
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.add", &Value::Null);
        let first = e.current_song;
        // Foot switch CC 64 on any channel: next song.
        let r = e.run("remote.add", &json!({"kind": "cc", "number": 64, "command": "transport.next_song"}));
        assert_eq!(r.ok(), Some(json!({"action": 0})));
        // Refused: a command needing more, an unknown one, bad numbers or kinds.
        for bad in [
            json!({"kind": "cc", "number": 64, "command": "show.open"}),
            json!({"kind": "cc", "number": 64, "command": "nothing.here"}),
            json!({"kind": "cc", "number": 300, "command": "transport.next_song"}),
            json!({"kind": "cc", "channel": 17, "number": 1, "command": "transport.next_song"}),
            json!({"kind": "sysex", "number": 1, "command": "transport.next_song"}),
            json!({"kind": "note", "number": 1, "command": "remote.trigger"}),
        ] {
            assert!(e.run("remote.add", &bad).is_err(), "{bad}");
        }
        assert_eq!(
            e.run("remote.list", &Value::Null).ok(),
            Some(json!([{"kind": "cc", "channel": null, "number": 64, "command": "transport.next_song"}]))
        );
        // Pressed once: one step; held: nothing more; released and pressed again: another.
        assert_eq!(e.midi_in(&[0xB3, 64, 127]), 1);
        let second = e.current_song;
        assert_ne!(second, first);
        assert_eq!(e.midi_in(&[0xB3, 64, 120]), 0, "still held");
        assert_eq!(e.midi_in(&[0xB3, 64, 0]), 0, "released");
        assert_eq!(e.midi_in(&[0xB3, 64, 127]), 1);
        // Another controller or a note does nothing.
        assert_eq!(e.midi_in(&[0xB0, 65, 127]), 0);
        assert_eq!(e.midi_in(&[0x90, 64, 100]), 0);
        // A note on channel 10 only, set off through the command too.
        assert!(
            e.run("remote.add", &json!({"kind": "note", "channel": 10, "number": 36, "command": "transport.stop"}))
                .is_ok()
        );
        assert_eq!(
            e.run("remote.trigger", &json!({"kind": "note", "channel": 10, "number": 36})).ok(),
            Some(json!({"ran": 1}))
        );
        assert_eq!(
            e.run("remote.trigger", &json!({"kind": "note", "channel": 9, "number": 36})).ok(),
            Some(json!({"ran": 0}))
        );
        // Removed, and saved with the show.
        assert!(e.run("remote.remove", &json!({"action": 0})).is_ok());
        assert_eq!(e.show.remote.len(), 1);
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.remote).unwrap_or_default();
        assert_eq!(back, e.show.remote);
    }
}
