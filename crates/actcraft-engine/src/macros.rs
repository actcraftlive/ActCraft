//! Macros: named lists of commands. `$NAME` at a timecoded line of a song's lyrics runs the
//! macro when playback reaches that line (lights up for the chorus, the next song's MIDI program
//! at the end); `macro.run` runs one at once.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::Macro;
use actcraft_show::limits::{MAX_MACRO_COMMANDS, MAX_MACROS};
use serde_json::{Value, json};

use crate::Engine;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// Playback that moves further than this between two looks (seconds) jumped; lines in between
/// don't run their macros.
const MAX_STEP: f64 = 1.0;
/// On a start or a jump, lines this close before the new place still run.
const LEAD_IN: f64 = 0.05;

/// The timecoded lines of a song's lyrics that call macros: (time, names).
pub type MacroCues = Vec<(f64, Vec<String>)>;

/// The macro calls in lyrics, by line time.
pub fn cues_of(lyrics: &str) -> MacroCues {
    if !lyrics.contains('$') {
        return Vec::new();
    }
    actcraft_lyrics::parse(lyrics)
        .lines
        .into_iter()
        .filter_map(|l| l.time.filter(|_| !l.macros.is_empty()).map(|t| (t, l.macros)))
        .collect()
}

impl Engine {
    /// Runs the macros of the lyrics lines playback passed since the last look; called from
    /// [`Engine::tick`].
    pub(crate) fn run_due_macros(&mut self) {
        let (playing, now) = (self.status.playing, self.status.time);
        let Some(song) = self.current_song.filter(|_| playing && now.is_finite()) else {
            self.macro_at = None;
            return;
        };
        // The song's calls, read again when the song or the show changed.
        let key = (song, self.edits);
        if self.macro_cues.as_ref().is_none_or(|c| c.0 != key) {
            let lyrics = self.show.song(song).map(|s| s.lyrics.as_str()).unwrap_or_default();
            self.macro_cues = Some((key, cues_of(lyrics)));
        }
        let from = match self.macro_at {
            Some(t) if now >= t && now - t <= MAX_STEP => t,
            _ => now - LEAD_IN,
        };
        self.macro_at = Some(now);
        let due: Vec<String> = self
            .macro_cues
            .as_ref()
            .map(|c| c.1.iter().filter(|(t, _)| *t > from && *t <= now).flat_map(|(_, n)| n.clone()).collect())
            .unwrap_or_default();
        for name in due {
            let _ = self.run_macro(&name);
        }
    }

    /// Runs a macro's commands in order; how many ran.
    pub fn run_macro(&mut self, name: &str) -> Result<usize, CommandError> {
        let Some(m) = self.show.macros.iter().find(|m| m.name == name) else {
            return Err(invalid(format!("no macro named {name}")));
        };
        let commands = m.commands.clone();
        for id in &commands {
            let _ = self.run(id, &Value::Null);
        }
        Ok(commands.len())
    }
}

/// `macro.set {name, commands: [id]}`: makes or changes a macro.
pub fn macro_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 32)?.trim().to_ascii_uppercase();
    if !Macro::valid_name(&name) {
        return Err(invalid("a macro's name is capitals, digits and _, starting with a capital"));
    }
    let list = prm.get("commands").and_then(Value::as_array).ok_or_else(|| invalid("`commands` must be a list"))?;
    if list.len() > MAX_MACRO_COMMANDS {
        return Err(invalid("a macro holds at most 32 commands"));
    }
    let mut commands = Vec::with_capacity(list.len());
    for c in list {
        let id = c.as_str().ok_or_else(|| invalid("`commands` must be command ids"))?;
        if !crate::remote::runnable(id) {
            return Err(invalid("a macro runs commands that need nothing more"));
        }
        commands.push(id.to_owned());
    }
    match e.show.macros.iter_mut().find(|m| m.name == name) {
        Some(m) => m.commands = commands,
        None => {
            if e.show.macros.len() >= MAX_MACROS {
                return Err(failed("a show holds at most 128 macros"));
            }
            e.show.macros.push(Macro { name: name.clone(), commands, ..Macro::default() });
        }
    }
    e.mark_changed();
    Ok(json!({"name": name}))
}

/// `macro.remove {name}`.
pub fn macro_remove(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 32)?.trim().to_ascii_uppercase();
    let before = e.show.macros.len();
    e.show.macros.retain(|m| m.name != name);
    if e.show.macros.len() == before {
        return Err(invalid(format!("no macro named {name}")));
    }
    e.mark_changed();
    Ok(Value::Null)
}

/// `macro.list`: every macro with its commands.
pub fn macro_list(e: &mut Engine, _prm: &Value) -> CommandResult {
    Ok(Value::Array(e.show.macros.iter().map(|m| json!({"name": m.name, "commands": m.commands})).collect()))
}

/// `macro.run {name}`: runs a macro now.
pub fn macro_run(e: &mut Engine, prm: &Value) -> CommandResult {
    let name = p::str_max(prm, "name", 32)?.trim().to_ascii_uppercase();
    let ran = e.run_macro(&name)?;
    Ok(json!({"ran": ran}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macros_are_made_changed_listed_run_and_removed() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.add", &Value::Null);
        let first = e.current_song;
        let r = e.run("macro.set", &json!({"name": "next", "commands": ["transport.next_song"]}));
        assert_eq!(r.ok(), Some(json!({"name": "NEXT"})), "names are capitals");
        for bad in [
            json!({"name": "2FAST", "commands": []}),
            json!({"name": "OK", "commands": ["show.open"]}),
            json!({"name": "OK", "commands": "transport.stop"}),
            json!({"name": "OK", "commands": ["macro.run"]}),
        ] {
            assert!(e.run("macro.set", &bad).is_err(), "{bad}");
        }
        assert_eq!(
            e.run("macro.list", &Value::Null).ok(),
            Some(json!([{"name": "NEXT", "commands": ["transport.next_song"]}]))
        );
        assert_eq!(e.run("macro.run", &json!({"name": "NEXT"})).ok(), Some(json!({"ran": 1})));
        assert_ne!(e.current_song, first);
        assert!(e.run("macro.run", &json!({"name": "NOPE"})).is_err());
        // Changing keeps one macro of that name.
        assert!(e.run("macro.set", &json!({"name": "NEXT", "commands": []})).is_ok());
        assert_eq!(e.show.macros.len(), 1);
        assert!(e.run("macro.remove", &json!({"name": "NEXT"})).is_ok());
        assert!(e.run("macro.remove", &json!({"name": "NEXT"})).is_err());
    }

    #[test]
    fn lyrics_lines_run_their_macros_as_playback_passes_them() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
        let _ = e.run("song.add", &Value::Null);
        let _ = e.run("song.load", &json!({"song": id}));
        let lyrics = "[00:01.00]$STOP_IT Verse one\n[00:05.00]Chorus";
        assert!(e.run("song.set", &json!({"song": id, "lyrics": lyrics})).is_ok());
        assert_eq!(cues_of(lyrics), vec![(1.0, vec!["STOP_IT".to_owned()])]);
        assert!(e.run("macro.set", &json!({"name": "STOP_IT", "commands": ["transport.next_song"]})).is_ok());
        let at = |e: &mut Engine, playing: bool, time: f64| {
            e.status.playing = playing;
            e.status.time = time;
            e.run_due_macros();
            e.current_song
        };
        assert_eq!(at(&mut e, true, 0.5), Some(id), "not there yet");
        assert_ne!(at(&mut e, true, 1.02), Some(id), "the line at 1 s ran its macro");
        // Back at the song and before the line: passing it again runs it again; a jump over it
        // does not.
        assert_ne!(at(&mut e, false, 1.1), Some(id), "stopped");
        assert!(e.run("song.load", &json!({"song": id})).is_ok());
        assert_eq!(at(&mut e, true, 0.0), Some(id));
        assert_eq!(at(&mut e, true, 3.0), Some(id), "jumped over it");
    }
}
