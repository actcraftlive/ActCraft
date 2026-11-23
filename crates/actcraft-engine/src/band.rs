//! Band link, the engine's side: what a host tells its followers (the song, where it is, whether
//! it plays), and a follower going along with it. Each computer plays its own copy of the
//! songs; a follower finds the host's song by its title. If the host goes, a follower keeps
//! playing (the backup machine).

use serde_json::{Value, json};

use crate::Engine;

/// How far a follower may drift before it jumps to the host's place, in seconds.
const MAX_DRIFT: f64 = 0.5;

impl Engine {
    /// What followers see: the current song's title, the playing position and whether it plays.
    pub fn band_state(&self) -> Value {
        json!({
            "song": self.current().map(|s| s.title.clone()),
            "time": if self.status.time.is_finite() { self.status.time } else { 0.0 },
            "playing": self.status.playing,
        })
    }

    /// What musicians see in the browser: the current song, how far it has played (0 to 1),
    /// whether it plays, and each musician with the sheet they see (their own, else one for
    /// anyone, else none).
    pub fn sheets_state(&self) -> Value {
        let song = self.current();
        let time = if self.status.time.is_finite() { self.status.time.max(0.0) } else { 0.0 };
        let progress = song.filter(|s| s.duration > 0.0).map_or(0.0, |s| (time / s.duration).clamp(0.0, 1.0));
        let musicians: Vec<Value> = self
            .show
            .musicians
            .iter()
            .map(|name| {
                let sheet = song.and_then(|s| {
                    let i = crate::sheet_commands::sheet_for(s, name)?;
                    let sheet = s.sheets.get(i)?;
                    Some(json!({"song": s.id, "index": i, "name": sheet.name, "pdf_file": sheet.pdf_file,
                        "notation_file": sheet.notation_file}))
                });
                json!({"name": name, "sheet": sheet})
            })
            .collect();
        json!({
            "song": song.map(|s| s.title.clone()),
            "progress": progress,
            "playing": self.status.playing,
            "musicians": musicians,
        })
    }

    /// Goes along with a host's state: the song of the same title, then playing or paused like
    /// the host, then the host's place when more than half a second off. Returns the title it
    /// could not find, if any.
    pub fn follow_band(&mut self, state: &Value) -> Option<String> {
        if let Some(title) = state.get("song").and_then(Value::as_str)
            && self.current().is_none_or(|s| s.title != title)
        {
            let Some(id) = self.show.songs.iter().find(|s| s.title == title).map(|s| s.id) else {
                return Some(title.to_owned());
            };
            let _ = self.run("song.load", &json!({"song": id}));
        }
        let playing = state.get("playing").and_then(Value::as_bool).unwrap_or(false);
        let time = state.get("time").and_then(Value::as_f64).filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(0.0);
        if playing && !self.status.playing {
            let _ = self.run("transport.play", &json!({"count_in": false, "from": time}));
        } else if !playing && self.status.playing {
            let _ = self.run("transport.pause", &Value::Null);
        } else if playing && (self.status.time - time).abs() > MAX_DRIFT {
            let _ = self.run("transport.seek", &json!({"seconds": time}));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_follower_takes_the_hosts_song_by_its_title() {
        let mut host = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = host.run("song.add", &json!({"title": "Opener"}));
        let state = host.band_state();
        assert_eq!(state["song"], json!("Opener"));
        assert_eq!(state["playing"], json!(false));

        let mut follower = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = follower.run("song.add", &json!({"title": "Encore"}));
        let _ = follower.run("song.add", &json!({"title": "Opener"}));
        let encore = follower.show.songs.iter().find(|s| s.title == "Encore").map_or(0, |s| s.id);
        let _ = follower.run("song.load", &json!({"song": encore}));
        assert_eq!(follower.current().map(|s| s.title.clone()), Some("Encore".into()));
        assert_eq!(follower.follow_band(&state), None);
        assert_eq!(follower.current().map(|s| s.title.clone()), Some("Opener".into()), "the host's song");
        assert_eq!(
            follower.follow_band(&json!({"song": "Not here", "time": 1.0, "playing": false})),
            Some("Not here".into())
        );
        // Odd values don't upset it.
        assert_eq!(follower.follow_band(&json!({"song": 5, "time": "x", "playing": "yes"})), None);
        assert_eq!(follower.follow_band(&Value::Null), None);
    }
}
