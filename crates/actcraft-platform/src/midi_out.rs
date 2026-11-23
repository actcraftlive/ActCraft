//! MIDI output: the computer's MIDI ports, and a thread that sends MIDI Clock and MIDI Timecode
//! to one of them while a song plays. What to send comes from the engine
//! ([`actcraft_engine::midi_sync`]); this only follows the playback position between the audio
//! callback's updates and puts the messages on the wire.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use actcraft_audio::{Status, StatusReader};
use actcraft_engine::midi_file::{MidiFilePlayer, MidiPart};
use actcraft_engine::midi_sync::{Position, SyncConfig, SyncSender};

/// The name ActCraft gives itself to the MIDI system.
const CLIENT: &str = "ActCraft";
/// How long the thread waits between looks at the position.
const TICK: Duration = Duration::from_millis(1);
/// Longest the position is carried forward without news from the audio callback.
const MAX_AHEAD: f64 = 0.2;

/// The names of the computer's MIDI outputs; empty when MIDI is not available.
pub fn output_ports() -> Vec<String> {
    let Ok(out) = midir::MidiOutput::new(CLIENT) else { return Vec::new() };
    out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect()
}

/// The song's MIDI file the thread plays, swapped when the song changes.
type PartCell = Arc<Mutex<Option<Arc<MidiPart>>>>;

/// Sends MIDI time, and the song's MIDI file, to one output while it lives; dropping it sends
/// Stop (and releases held notes) if a song was playing and closes the port.
pub struct MidiTimeOut {
    port: String,
    config: Arc<AtomicU32>,
    part: PartCell,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MidiTimeOut {
    /// Opens the output named `port` and starts following `status`.
    pub fn open(port: &str, status: StatusReader, cfg: SyncConfig) -> Result<Self, String> {
        let out = midir::MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
        let found = out
            .ports()
            .into_iter()
            .find(|p| out.port_name(p).ok().as_deref() == Some(port))
            .ok_or_else(|| format!("{port} is not connected"))?;
        let mut conn = out.connect(&found, "ActCraft time").map_err(|e| e.to_string())?;
        let config = Arc::new(AtomicU32::new(encode(cfg)));
        let part: PartCell = Arc::new(Mutex::new(None));
        let quit = Arc::new(AtomicBool::new(false));
        let (cfg_cell, part_cell, quit_flag) = (Arc::clone(&config), Arc::clone(&part), Arc::clone(&quit));
        let thread = std::thread::Builder::new()
            .name("actcraft-midi-time".into())
            .spawn(move || {
                let mut sender = SyncSender::default();
                let mut player = MidiFilePlayer::default();
                let mut playing_part: Option<Arc<MidiPart>> = None;
                let mut follow = Follow::default();
                loop {
                    let stop = quit_flag.load(Ordering::Relaxed);
                    let now = Instant::now();
                    if let Some(s) = status.load() {
                        follow.update(&s, now);
                    }
                    let mut at = follow.at(now);
                    if stop {
                        at.playing = false;
                    }
                    for msg in sender.step(decode(cfg_cell.load(Ordering::Relaxed)), at) {
                        let _ = conn.send(&msg);
                    }
                    // Another song's file: release what the last one held and start afresh.
                    let current = part_cell.lock().ok().and_then(|p| p.clone());
                    let same = match (&current, &playing_part) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        if let Some(old) = &playing_part {
                            let off = Position { playing: false, ..at };
                            for msg in player.step(old, off) {
                                let _ = conn.send(&msg);
                            }
                        }
                        player = MidiFilePlayer::default();
                        playing_part = current;
                    }
                    if let Some(p) = &playing_part {
                        for msg in player.step(p, at) {
                            let _ = conn.send(&msg);
                        }
                    }
                    if stop {
                        break;
                    }
                    std::thread::sleep(TICK);
                }
                conn.close();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { port: port.to_owned(), config, part, quit, thread: Some(thread) })
    }

    /// The output's name.
    pub fn port(&self) -> &str {
        &self.port
    }

    /// What to send from now on.
    pub fn set(&self, cfg: SyncConfig) {
        self.config.store(encode(cfg), Ordering::Relaxed);
    }

    /// The song's MIDI file to play from now on (`None`: none).
    pub fn set_part(&self, part: Option<Arc<MidiPart>>) {
        if let Ok(mut cell) = self.part.lock() {
            *cell = part;
        }
    }
}

impl Drop for MidiTimeOut {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn encode(cfg: SyncConfig) -> u32 {
    u32::from(cfg.clock) | u32::from(cfg.timecode) << 1 | u32::from(cfg.timecode_fps) << 8
}

fn decode(bits: u32) -> SyncConfig {
    SyncConfig { clock: bits & 1 != 0, timecode: bits & 2 != 0, timecode_fps: (bits >> 8 & 0xFF) as u8 }
}

/// The playback position between the audio callback's updates: the last one, carried forward
/// by the time since, at the speed the beat went between the last two.
#[derive(Debug, Default)]
struct Follow {
    last: Option<(Status, Instant)>,
    /// Beats per second of song time, from the last two updates.
    rate: f64,
}

impl Follow {
    fn update(&mut self, s: &Status, now: Instant) {
        match &self.last {
            Some((prev, _)) if prev.frames == s.frames => return,
            Some((prev, _)) => {
                let dt = s.time - prev.time;
                let db = s.beat - prev.beat;
                if s.playing && prev.playing && dt > 0.0 && db.is_finite() && db > 0.0 {
                    self.rate = db / dt;
                }
            }
            None => {}
        }
        self.last = Some((*s, now));
    }

    fn at(&self, now: Instant) -> Position {
        let Some((s, when)) = &self.last else { return Position::default() };
        if !s.playing {
            return Position { playing: false, time: s.time, beat: s.beat };
        }
        let ahead = now.saturating_duration_since(*when).as_secs_f64().min(MAX_AHEAD);
        Position { playing: true, time: s.time + ahead, beat: s.beat + ahead * self.rate }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(playing: bool, time: f64, beat: f64, frames: u64) -> Status {
        Status { playing, time, beat, frames, ..Status::default() }
    }

    /// Opens the first MIDI output of this computer, follows a renderer for a moment and closes.
    /// Run by hand where a MIDI output exists: `cargo test -p actcraft-platform -- --ignored`.
    #[test]
    #[ignore = "needs a MIDI output on this computer"]
    fn a_real_port_opens_follows_and_closes() {
        let ports = output_ports();
        let Some(port) = ports.first() else { return };
        let (controller, _renderer) = actcraft_audio::link(48_000);
        let cfg = SyncConfig { clock: true, timecode: true, timecode_fps: 25 };
        let out = MidiTimeOut::open(port, controller.status_reader(), cfg);
        assert!(out.is_ok(), "{port}: {:?}", out.err());
        std::thread::sleep(Duration::from_millis(50));
        drop(out);
    }

    #[test]
    fn settings_travel_through_one_number() {
        for cfg in [
            SyncConfig { clock: true, timecode: false, timecode_fps: 25 },
            SyncConfig { clock: false, timecode: true, timecode_fps: 30 },
            SyncConfig { clock: true, timecode: true, timecode_fps: 24 },
        ] {
            assert_eq!(decode(encode(cfg)), cfg);
        }
    }

    #[test]
    fn the_position_moves_on_between_updates_and_holds_when_stopped() {
        let t0 = Instant::now();
        let mut f = Follow::default();
        assert_eq!(f.at(t0), Position::default(), "nothing known yet");
        // 120 BPM: two beats a second.
        f.update(&status(true, 1.0, 2.0, 100), t0);
        f.update(&status(true, 1.01, 2.02, 200), t0);
        let later = f.at(t0 + Duration::from_millis(50));
        assert!((later.time - 1.06).abs() < 1e-9 && (later.beat - 2.12).abs() < 1e-9, "{later:?}");
        // Without news it goes no further than a moment.
        let stalled = f.at(t0 + Duration::from_secs(5));
        assert!((stalled.time - (1.01 + MAX_AHEAD)).abs() < 1e-9, "{stalled:?}");
        // The same update again changes nothing; a stop holds the place.
        f.update(&status(true, 1.01, 2.02, 200), t0 + Duration::from_secs(1));
        f.update(&status(false, 1.5, 3.0, 300), t0);
        assert_eq!(f.at(t0 + Duration::from_secs(1)), Position { playing: false, time: 1.5, beat: 3.0 });
    }
}
