//! Remote actions: a MIDI controller (a foot switch, a pad, a keyboard) sets off commands. A
//! show keeps a list of triggers (a note, a controller or a program change, on one channel or
//! any) and the command each runs. The command may act on one thing of the show (a preset, a hit,
//! a cue list or one of its cues, a sequence, a macro), kept by name and found when the message
//! comes in, so the MIDI a show already sends keeps working after moving from another lighting
//! program. The platform layer hands incoming MIDI bytes to [`Engine::midi_in`];
//! `remote.trigger` does the same from the command line or an agent.

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

/// What a targeted command acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thing {
    Preset,
    Hit,
    CueList,
    /// A cue of a cue list: the list by name, the cue by its place.
    Cue,
    Sequence,
    Macro,
}

/// Commands a remote action can run on one thing of the show, and what that thing is.
pub const TARGETED: &[(&str, Thing)] = &[
    ("preset.recall", Thing::Preset),
    ("hit.fire", Thing::Hit),
    ("cue.go", Thing::CueList),
    ("cue.back", Thing::CueList),
    ("cue_list.stop", Thing::CueList),
    ("cue.jump", Thing::Cue),
    ("sequence.start", Thing::Sequence),
    ("sequence.toggle", Thing::Sequence),
    ("sequence.stop", Thing::Sequence),
    ("macro.run", Thing::Macro),
];

/// What a command acts on, when a remote action can aim it at one thing of the show.
pub fn thing_of(command: &str) -> Option<Thing> {
    TARGETED.iter().find(|(id, _)| *id == command).map(|(_, t)| *t)
}

/// The names the show has now for things of this kind (for a cue, the cue lists').
pub fn names(e: &Engine, thing: Thing) -> Vec<String> {
    match thing {
        Thing::Preset => e.show.presets.iter().map(|p| p.name.clone()).collect(),
        Thing::Hit => e.show.hits.iter().map(|h| h.name.clone()).collect(),
        Thing::CueList | Thing::Cue => e.show.cue_lists.iter().map(|l| l.name.clone()).collect(),
        Thing::Sequence => e.show.sequences.iter().map(|s| s.name.clone()).collect(),
        Thing::Macro => e.show.macros.iter().map(|m| m.name.clone()).collect(),
    }
}

/// The params that run `command` on the thing named `target` (and, for a cue, its place), or
/// `None` when the show has no such thing now.
fn params_for(e: &Engine, command: &str, target: &str, cue: Option<usize>) -> Option<Value> {
    let at = |thing| names(e, thing).iter().position(|n| n == target);
    Some(match thing_of(command)? {
        Thing::Preset => json!({"preset": at(Thing::Preset)?}),
        Thing::Hit => json!({"hit": at(Thing::Hit)?}),
        Thing::CueList => json!({"list": at(Thing::CueList)?}),
        Thing::Cue => {
            let list = at(Thing::Cue)?;
            let cue = cue.filter(|&c| e.show.cue_lists.get(list).is_some_and(|l| c < l.cues.len()))?;
            json!({"list": list, "cue": cue})
        }
        Thing::Sequence => json!({"sequence": at(Thing::Sequence)?}),
        Thing::Macro => {
            at(Thing::Macro)?;
            json!({"name": target})
        }
    })
}

/// What an action runs now, its command and params; `None` when it can't (its thing is gone,
/// or its command needs one).
fn what_runs(e: &Engine, a: &RemoteAction) -> Option<(String, Value)> {
    if a.target.is_empty() {
        return runnable(&a.command).then(|| (a.command.clone(), Value::Null));
    }
    params_for(e, &a.command, &a.target, a.cue).map(|prm| (a.command.clone(), prm))
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
        let runs: Vec<(String, Value)> = self
            .show
            .remote
            .iter()
            .filter(|a| answers(&a.trigger, &trigger))
            .filter_map(|a| what_runs(self, a))
            .collect();
        for (id, prm) in &runs {
            let _ = self.run(id, prm);
        }
        runs.len()
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
    let mut v = json!({
        "kind": kind,
        "channel": a.trigger.channel.map(|c| c + 1),
        "number": a.trigger.number,
        "command": a.command,
    });
    if !a.target.is_empty() {
        v["target"] = json!(a.target);
    }
    if let Some(cue) = a.cue {
        v["cue"] = json!(cue);
    }
    v
}

/// `remote.add {kind, channel?, number, command, target?, cue?}`: a MIDI message runs a command;
/// one that acts on a preset, hit, cue list, sequence or macro runs on the one named `target`
/// (for `cue.jump`, on the cue at place `cue` of that list).
pub fn remote_add(e: &mut Engine, prm: &Value) -> CommandResult {
    let trigger = trigger_param(prm)?;
    let command = p::str_max(prm, "command", 64)?;
    let target = p::opt_str_max(prm, "target", 64)?.map(str::trim).filter(|t| !t.is_empty());
    let cue = p::opt_i64_in(prm, "cue", 0, 9_999)?.and_then(|c| usize::try_from(c).ok());
    match (thing_of(command), target) {
        (Some(_), Some(t)) => {
            if params_for(e, command, t, cue).is_none() {
                return Err(invalid("the show has no such preset, hit, cue list, cue, sequence or macro"));
            }
        }
        (None, Some(_)) => {
            return Err(invalid("that command does not act on a preset, hit, cue list, sequence or macro"));
        }
        _ => {
            if !runnable(command) {
                return Err(invalid(
                    "a remote action runs a command that needs nothing more, or names what it acts on",
                ));
            }
        }
    }
    let cue = if thing_of(command) == Some(Thing::Cue) { cue } else { None };
    if e.show.remote.len() >= MAX_REMOTE_ACTIONS {
        return Err(failed("a show holds at most 256 remote actions"));
    }
    e.show.remote.push(RemoteAction {
        trigger,
        command: command.to_owned(),
        target: target.unwrap_or_default().to_owned(),
        cue,
        ..RemoteAction::default()
    });
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

    /// The owner's case: someone moving from another lighting program keeps the notes their
    /// backing-track program sends per song: each note recalls a look, fires a burst, jumps to a
    /// cue or starts an animation, found by name.
    #[test]
    fn a_note_acts_on_the_preset_hit_cue_or_sequence_it_names() {
        let mut e = engine();
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "address": 1}));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "address": 2}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.2}}));
        let _ = e.run("preset.store", &json!({"name": "Dim", "fixture": 0}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Warm", "fixture": 0}));
        let _ = e.run("fixture.set", &json!({"fixture": 1, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Fog on", "fixture": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 1, "values": {"intensity": 0.0}}));
        let _ = e.run("hit.set", &json!({"name": "Fog", "preset": 2, "seconds": 0.5}));
        let _ = e.run("cue_list.add", &json!({"name": "Song 2"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 1}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let level = |e: &Engine, ch: usize| e.dmx.universe(1).and_then(|u| u.get(ch).copied()).unwrap_or(0);
        let add = |e: &mut Engine, number: u8, command: &str, target: &str, cue: Option<usize>| {
            let mut prm = json!({"kind": "note", "channel": 1, "number": number, "command": command, "target": target});
            if let Some(c) = cue {
                prm["cue"] = json!(c);
            }
            e.run("remote.add", &prm)
        };
        assert!(add(&mut e, 60, "preset.recall", "Warm", None).is_ok());
        assert!(add(&mut e, 61, "hit.fire", "Fog", None).is_ok());
        assert!(add(&mut e, 62, "cue.jump", "Song 2", Some(1)).is_ok());
        assert_eq!(
            e.run("remote.list", &Value::Null).ok().map(|l| l[2].clone()),
            Some(
                json!({"kind": "note", "channel": 1, "number": 62, "command": "cue.jump", "target": "Song 2", "cue": 1})
            )
        );
        // Note 60: the preset.
        assert_eq!(e.midi_in(&[0x90, 60, 100]), 1);
        assert_eq!(level(&e, 0), 255);
        // Note 61: the hit, on for its time.
        assert_eq!(e.midi_in(&[0x90, 61, 100]), 1);
        assert_eq!((level(&e, 1), e.hits_on()), (255, vec!["Fog".to_owned()]));
        // Note 62: straight to the second cue of the list.
        assert_eq!(e.midi_in(&[0x90, 62, 100]), 1);
        assert_eq!(e.cue_position(0), Some(1));
        // Found by name: with the preset before it removed, "Warm" moves up a place and the note
        // still recalls it; once "Warm" is gone the note does nothing.
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 0.0}}));
        let _ = e.run("preset.remove", &json!({"preset": 0}));
        assert_eq!(e.show.presets.first().map(|p| p.name.as_str()), Some("Warm"));
        assert_eq!(e.midi_in(&[0x90, 60, 100]), 1);
        assert_eq!(level(&e, 0), 255);
        let warm = e.show.presets.iter().position(|p| p.name == "Warm").unwrap_or(0);
        let _ = e.run("preset.remove", &json!({"preset": warm}));
        assert_eq!(e.midi_in(&[0x90, 60, 100]), 0, "its preset is gone");
        // Saved with the show.
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.remote).unwrap_or_default();
        assert_eq!(back, e.show.remote);
    }

    #[test]
    fn a_target_is_checked() {
        let mut e = engine();
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("macro.set", &json!({"name": "INTRO", "commands": ["transport.stop"]}));
        for bad in [
            json!({"kind": "note", "number": 1, "command": "preset.recall"}),
            json!({"kind": "note", "number": 1, "command": "preset.recall", "target": "Nope"}),
            json!({"kind": "note", "number": 1, "command": "cue.jump", "target": "Main", "cue": 0}),
            json!({"kind": "note", "number": 1, "command": "cue.jump", "target": "Main"}),
            json!({"kind": "note", "number": 1, "command": "transport.stop", "target": "Main"}),
            json!({"kind": "note", "number": 1, "command": "macro.run", "target": "OUTRO"}),
            json!({"kind": "note", "number": 1, "command": "cue.go", "target": 5}),
        ] {
            assert!(e.run("remote.add", &bad).is_err(), "{bad}");
        }
        // A command that runs without a thing may still name one; the macro by its name.
        assert!(e.run("remote.add", &json!({"kind": "note", "number": 1, "command": "cue.go"})).is_ok());
        assert!(
            e.run("remote.add", &json!({"kind": "note", "number": 2, "command": "cue.go", "target": "Main"})).is_ok()
        );
        assert!(
            e.run("remote.add", &json!({"kind": "note", "number": 3, "command": "macro.run", "target": "INTRO"}))
                .is_ok()
        );
        assert_eq!(e.midi_in(&[0x90, 3, 100]), 1);
    }
}
