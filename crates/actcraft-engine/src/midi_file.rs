//! A song's MIDI file played to the MIDI output in time with its audio: the file's notes and
//! controller changes as wire messages at song times, and a player that hands out the ones due
//! as playback goes on. The file starts at the song's zero; the show's MIDI latency offset moves
//! all of it earlier (negative) or later. Like [`crate::midi_sync`], this holds no port; the
//! platform layer steps it.

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_TEXT;
use craft_midi::{CtrlKind, Smf};
use serde_json::{Value, json};

use crate::midi_sync::Position;
use crate::{Engine, MessageArg};

/// Largest MIDI file read.
pub const MAX_MIDI_BYTES: usize = 16 * 1024 * 1024;
/// A move of playback further than this (seconds) is a jump: held notes are released and the
/// player picks up at the new place instead of sending everything in between.
const MAX_STEP: f64 = 0.5;

/// A MIDI file as wire messages in song time, earliest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MidiPart {
    pub events: Vec<(f64, Vec<u8>)>,
    /// Notes in the file.
    pub notes: usize,
    /// Where its last message is, in seconds.
    pub length: f64,
}

impl MidiPart {
    /// The file's messages at song times: ticks turned into seconds through its tempo map,
    /// shifted by `offset` seconds.
    pub fn from_smf(smf: &Smf, offset: f64) -> Self {
        let mut tempos: Vec<(i64, f64)> =
            smf.tempos.iter().copied().filter(|(t, bpm)| *t >= 0 && bpm.is_finite() && *bpm > 0.0).collect();
        tempos.sort_by_key(|t| t.0);
        let seconds = |tick: i64| -> f64 {
            let ppq = f64::from(craft_midi::PPQ);
            let (mut at, mut bpm, mut secs) = (0i64, 120.0, 0.0);
            for &(t, b) in &tempos {
                if t > tick {
                    break;
                }
                secs += (t - at) as f64 / ppq * 60.0 / bpm;
                at = t;
                bpm = b;
            }
            secs + (tick.max(at) - at) as f64 / ppq * 60.0 / bpm + offset
        };
        let mut events = Vec::new();
        let mut notes = 0;
        for track in &smf.tracks {
            let seq = &track.sequence;
            for n in &seq.notes {
                let ch = n.channel & 0x0F;
                events.push((seconds(n.start), vec![0x90 | ch, n.pitch & 0x7F, n.velocity.clamp(1, 127)]));
                events.push((seconds(n.start + n.length), vec![0x80 | ch, n.pitch & 0x7F, n.release_velocity & 0x7F]));
                notes += 1;
            }
            for c in &seq.ctrls {
                let ch = c.channel & 0x0F;
                let v = |x: i32| x.clamp(0, 127) as u8;
                let msg = match c.kind {
                    CtrlKind::Cc(n) => vec![0xB0 | ch, n & 0x7F, v(c.value)],
                    CtrlKind::Program => vec![0xC0 | ch, v(c.value)],
                    CtrlKind::ChannelPressure => vec![0xD0 | ch, v(c.value)],
                    CtrlKind::PolyPressure(p) => vec![0xA0 | ch, p & 0x7F, v(c.value)],
                    CtrlKind::PitchBend => {
                        let b = (c.value.clamp(-8192, 8191) + 8192) as u16;
                        vec![0xE0 | ch, (b & 0x7F) as u8, (b >> 7) as u8]
                    }
                };
                events.push((seconds(c.tick), msg));
            }
        }
        // Earliest first; at one moment a note's end goes before a new start of the same pitch.
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| (a.1[0] & 0xF0 == 0x90).cmp(&(b.1[0] & 0xF0 == 0x90))));
        let length = events.last().map_or(0.0, |e| e.0.max(0.0));
        Self { events, notes, length }
    }
}

/// Hands out the part's messages as playback moves on.
#[derive(Debug, Default)]
pub struct MidiFilePlayer {
    /// The next message to send.
    next: usize,
    /// Song time the last step reached, while playing.
    at: Option<f64>,
}

impl MidiFilePlayer {
    /// The messages due for playback now at `pos`: those between the last step and now. A stop
    /// or a jump releases every note first.
    pub fn step(&mut self, part: &MidiPart, pos: Position) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if !pos.playing || !pos.time.is_finite() {
            if self.at.take().is_some() {
                all_notes_off(&mut out);
            }
            return out;
        }
        let now = pos.time;
        let jumped = match self.at {
            None => true,
            Some(last) => now < last || now - last > MAX_STEP,
        };
        if jumped {
            if self.at.is_some() {
                all_notes_off(&mut out);
            }
            // Pick up at the new place without the messages before it (which would sound all at
            // once); anything exactly at `now` still goes.
            self.next = part.events.partition_point(|e| e.0 < now);
        }
        while let Some((t, msg)) = part.events.get(self.next) {
            if *t > now {
                break;
            }
            out.push(msg.clone());
            self.next += 1;
        }
        self.at = Some(now);
        out
    }
}

/// All Notes Off and All Sound Off on every channel.
fn all_notes_off(out: &mut Vec<Vec<u8>>) {
    for ch in 0..16u8 {
        out.push(vec![0xB0 | ch, 123, 0]);
    }
}

/// MIDI file types a song takes.
pub const MIDI_EXTENSIONS: &[&str] = &["mid", "midi", "smf"];

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// Whether a path names a MIDI file (`.mid`, `.midi`, `.smf`).
pub fn is_midi_path(path: &str) -> bool {
    crate::services::extension(path).is_some_and(|ext| MIDI_EXTENSIONS.contains(&ext.as_str()))
}

/// Reads a MIDI file through the services, with errors in fixed words the window translates.
pub fn load_smf(e: &Engine, path: &str) -> Result<Smf, CommandError> {
    if !is_midi_path(path) {
        return Err(invalid("ActCraft reads MIDI from .mid, .midi and .smf files"));
    }
    let bytes = e.services().read_file(path).map_err(failed)?;
    if bytes.len() > MAX_MIDI_BYTES {
        return Err(failed("that MIDI file is too large"));
    }
    craft_midi::read_smf(&bytes).map_err(|_| failed("that file is not a MIDI file ActCraft can read"))
}

/// The song's MIDI file as messages in song time, with the show's MIDI latency offset; `None`
/// when the song has none.
pub fn song_part(e: &Engine, song: u64) -> Result<Option<MidiPart>, CommandError> {
    let Some(path) = e.show.song(song).and_then(|s| s.midi_file.clone()) else { return Ok(None) };
    let smf = load_smf(e, &path)?;
    Ok(Some(MidiPart::from_smf(&smf, e.show.mixer.midi_latency_offset_ms / 1000.0)))
}

fn summary(smf: &Smf, part: &MidiPart) -> Value {
    let names: Vec<&str> = smf.tracks.iter().map(|t| t.name.as_str()).filter(|n| !n.is_empty()).collect();
    json!({"tracks": smf.tracks.len(), "names": names, "notes": part.notes, "seconds": part.length})
}

/// The `song` param, or the current song.
fn song_id(e: &Engine, prm: &Value) -> Result<u64, CommandError> {
    if p::has(prm, "song") {
        let id = u64::try_from(p::i64_in(prm, "song", 1, i64::MAX)?).map_err(|_| invalid("bad song id"))?;
        if e.show.song(id).is_some() { Ok(id) } else { Err(invalid(format!("no song with id {id}"))) }
    } else {
        e.current_song.filter(|&id| e.show.song(id).is_some()).ok_or_else(|| invalid("no song chosen"))
    }
}

/// `midi.attach {song?, path}`: reads the MIDI file (refusing one that isn't) and plays it to
/// the MIDI output with the song.
pub fn midi_attach(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    let smf = load_smf(e, &path)?;
    let part = MidiPart::from_smf(&smf, 0.0);
    let Some(song) = e.show.song_mut(id) else { return Err(invalid("no such song")) };
    song.midi_file = Some(path);
    e.mark_changed();
    e.say_with("MIDI file attached: {notes} notes", vec![MessageArg::plain("notes", part.notes)]);
    Ok(summary(&smf, &part))
}

/// `midi.read {song?}`: the song's MIDI file, described.
pub fn midi_read(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = song_id(e, prm)?;
    let path = e.show.song(id).and_then(|s| s.midi_file.clone()).ok_or_else(|| invalid("the song has no MIDI file"))?;
    let smf = load_smf(e, &path)?;
    Ok(summary(&smf, &MidiPart::from_smf(&smf, 0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_midi::{CtrlEvent, Note, Sequence, SmfTrack};

    fn smf(notes: Vec<Note>, ctrl: Vec<CtrlEvent>, tempos: Vec<(i64, f64)>) -> Smf {
        let sequence = Sequence { notes, ctrls: ctrl, ..Sequence::default() };
        Smf {
            format: 1,
            tracks: vec![SmfTrack { name: "Keys".into(), sequence }],
            tempos,
            meters: Vec::new(),
            markers: Vec::new(),
            key_sigs: Vec::new(),
        }
    }

    fn note(pitch: u8, start: i64, length: i64) -> Note {
        Note { pitch, velocity: 100, release_velocity: 64, channel: 2, start, length }
    }

    fn at(playing: bool, time: f64) -> Position {
        Position { playing, time, beat: 0.0 }
    }

    #[test]
    fn ticks_become_song_seconds_through_the_tempo_map() {
        // 960 ticks a quarter: a quarter at 120 BPM is half a second; from tick 1920 at 60 BPM a
        // quarter is a second.
        let file = smf(vec![note(60, 0, 960), note(62, 2880, 960)], vec![], vec![(0, 120.0), (1920, 60.0)]);
        let part = MidiPart::from_smf(&file, 0.0);
        let times: Vec<f64> = part.events.iter().map(|e| e.0).collect();
        assert_eq!(times, vec![0.0, 0.5, 2.0, 3.0]);
        assert_eq!(part.events[0].1, vec![0x92, 60, 100], "note on, channel 3");
        assert_eq!(part.events[1].1, vec![0x82, 60, 64]);
        assert_eq!((part.notes, part.length), (2, 3.0));
        // The latency offset moves everything.
        let early = MidiPart::from_smf(&file, -0.1);
        assert!((early.events[1].0 - 0.4).abs() < 1e-9);
    }

    #[test]
    fn controller_changes_become_their_messages() {
        let ctrl = vec![
            CtrlEvent { tick: 0, channel: 0, kind: CtrlKind::Program, value: 5 },
            CtrlEvent { tick: 0, channel: 1, kind: CtrlKind::Cc(7), value: 300 },
            CtrlEvent { tick: 0, channel: 0, kind: CtrlKind::PitchBend, value: 0 },
        ];
        let part = MidiPart::from_smf(&smf(vec![], ctrl, vec![]), 0.0);
        let msgs: Vec<Vec<u8>> = part.events.iter().map(|e| e.1.clone()).collect();
        assert!(msgs.contains(&vec![0xC0, 5]));
        assert!(msgs.contains(&vec![0xB1, 7, 127]), "values are bounded");
        assert!(msgs.contains(&vec![0xE0, 0, 64]), "a centred bend");
    }

    #[test]
    fn a_midi_file_attaches_reads_and_plays_with_the_latency_offset() {
        let file = smf(vec![note(60, 960, 480)], vec![], vec![(0, 120.0)]);
        let bytes = craft_midi::write_smf(&file).unwrap_or_default();
        let mut services = crate::MemoryServices::default();
        services.files.insert("/gig/keys.mid".into(), bytes);
        services.files.insert("/gig/fake.mid".into(), b"not midi".to_vec());
        let mut e = Engine::new(Box::new(services));
        let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
        assert!(e.run("midi.read", &json!({"song": id})).is_err(), "nothing attached yet");
        assert!(matches!(song_part(&e, id), Ok(None)));
        for bad in ["/gig/fake.mid", "/gig/missing.mid", "/gig/keys.wav"] {
            assert!(e.run("midi.attach", &json!({"song": id, "path": bad})).is_err(), "{bad}");
        }
        let r = e.run("midi.attach", &json!({"song": id, "path": "/gig/keys.mid"}));
        assert_eq!(r.ok().map(|v| (v["notes"].clone(), v["names"].clone())), Some((json!(1), json!(["Keys"]))));
        assert_eq!(e.show.song(id).and_then(|s| s.midi_file.clone()).as_deref(), Some("/gig/keys.mid"));
        // song.set checks the type, too.
        assert!(e.run("song.set", &json!({"song": id, "midi_file": "/gig/keys.txt"})).is_err());
        // The part is in song time, moved by the show's MIDI latency offset.
        let part = song_part(&e, id).ok().flatten().unwrap_or_default();
        assert_eq!(part.events.first().map(|ev| ev.0), Some(0.5));
        e.show.mixer.midi_latency_offset_ms = -50.0;
        let part = song_part(&e, id).ok().flatten().unwrap_or_default();
        assert!(part.events.first().is_some_and(|ev| (ev.0 - 0.45).abs() < 1e-9));
    }

    #[test]
    fn the_player_sends_what_is_due_and_releases_on_stop_and_jump() {
        let part = MidiPart::from_smf(&smf(vec![note(60, 0, 960), note(64, 960, 960)], vec![], vec![]), 0.0);
        let mut p = MidiFilePlayer::default();
        assert!(p.step(&part, at(false, 0.0)).is_empty());
        assert_eq!(p.step(&part, at(true, 0.0)), vec![vec![0x92, 60, 100]]);
        assert!(p.step(&part, at(true, 0.2)).is_empty());
        // 0.5 s: the first note ends and the second starts, end first.
        assert_eq!(p.step(&part, at(true, 0.5)), vec![vec![0x82, 60, 64], vec![0x92, 64, 100]]);
        // Stopping releases every note on every channel.
        let stop = p.step(&part, at(false, 0.6));
        assert_eq!(stop.len(), 16);
        assert!(stop.iter().all(|m| m[1] == 123));
        // Starting in the middle of the second note sends nothing until its end.
        assert!(p.step(&part, at(true, 0.7)).is_empty());
        assert_eq!(p.step(&part, at(true, 1.0)), vec![vec![0x82, 64, 64]]);
        // A jump back releases and picks up at the new place.
        let back = p.step(&part, at(true, 0.0));
        assert_eq!(back.len(), 17, "notes off, then the note at zero");
        assert_eq!(back.last(), Some(&vec![0x92, 60, 100]));
    }
}
