//! MIDI Clock and MIDI Timecode from the playback position: which messages go out as the song
//! plays, starts, stops or jumps. This holds no port and no thread; the platform layer calls
//! [`SyncSender::step`] about every millisecond and sends what it returns.
//!
//! Clock: 24 pulses per quarter note from the song's beat; a start sends the Song Position
//! Pointer and Start (at the song's beginning) or Continue; a jump (a click on the waveform, a
//! loop going round) sends Stop, the new position and Continue; stopping sends Stop.
//! Timecode: a quarter frame every quarter of a frame from the song time, eight of them carry
//! the time of the frame pair they start; a start or a jump sends a full frame first.

use craft_midi::sync::{self, CONTINUE, Rate, START, STOP};

/// Clock pulses per quarter note.
const PULSES: f64 = sync::CLOCKS_PER_QUARTER as f64;
/// More pulses than this to catch up count as a jump.
const MAX_CATCH_UP_PULSES: i64 = 48;
/// More quarter frames than this to catch up count as a jump.
const MAX_CATCH_UP_QUARTERS: i64 = 16;

/// What to send, as the song's MIDI settings say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncConfig {
    pub clock: bool,
    pub timecode: bool,
    pub timecode_fps: u8,
}

impl From<&actcraft_show::MidiSettings> for SyncConfig {
    fn from(m: &actcraft_show::MidiSettings) -> Self {
        Self { clock: m.clock, timecode: m.timecode, timecode_fps: m.timecode_fps }
    }
}

/// Where playback is: playing or not, song time in seconds, beats from the beat anchor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Position {
    pub playing: bool,
    pub time: f64,
    pub beat: f64,
}

/// What was sent so far, to send only what is new.
#[derive(Debug, Default)]
pub struct SyncSender {
    playing: bool,
    /// The last clock pulse sent (pulses from the anchor).
    pulse: Option<i64>,
    /// The last quarter frame sent (quarter frames from the song's start).
    quarter: Option<i64>,
}

impl SyncSender {
    /// The messages for playback now at `at`.
    pub fn step(&mut self, cfg: SyncConfig, at: Position) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let started = at.playing && !self.playing;
        let stopped = !at.playing && self.playing;
        self.playing = at.playing;
        if cfg.clock {
            self.clock(at, started, stopped, &mut out);
        } else {
            self.pulse = None;
        }
        if cfg.timecode && at.playing {
            self.timecode(Rate::of(cfg.timecode_fps), at, started, &mut out);
        } else {
            self.quarter = None;
        }
        out
    }

    fn clock(&mut self, at: Position, started: bool, stopped: bool, out: &mut Vec<Vec<u8>>) {
        if stopped {
            out.push(vec![STOP]);
            self.pulse = None;
            return;
        }
        if !at.playing || !at.beat.is_finite() {
            return;
        }
        // Before the anchor (a count-in) the clock waits at the start.
        let now = (at.beat.max(0.0) * PULSES).floor() as i64;
        let locate = |out: &mut Vec<Vec<u8>>, start: bool| {
            let sixteenths = (at.beat.max(0.0) * 4.0).floor().min(f64::from(sync::MAX_POSITION)) as u16;
            out.push(sync::song_position(sixteenths).to_vec());
            out.push(vec![if start && sixteenths == 0 { START } else { CONTINUE }]);
        };
        match self.pulse {
            _ if started => {
                locate(out, true);
                self.pulse = Some(now);
            }
            None => {
                locate(out, false);
                self.pulse = Some(now);
            }
            Some(last) if now < last || now - last > MAX_CATCH_UP_PULSES => {
                out.push(vec![STOP]);
                locate(out, false);
                self.pulse = Some(now);
            }
            Some(last) => {
                for _ in last..now {
                    out.push(vec![sync::CLOCK]);
                }
                self.pulse = Some(now);
            }
        }
    }

    fn timecode(&mut self, rate: Rate, at: Position, started: bool, out: &mut Vec<Vec<u8>>) {
        if !at.time.is_finite() || at.time < 0.0 {
            self.quarter = None;
            return;
        }
        let fps = f64::from(rate.fps());
        let now = (at.time * fps * 4.0 + 1e-6).floor() as i64;
        let jumped = match self.quarter {
            None => true,
            Some(last) => started || now < last || now - last > MAX_CATCH_UP_QUARTERS,
        };
        if jumped {
            out.push(sync::full_frame(at.time, rate).to_vec());
            self.quarter = Some(now);
            return;
        }
        let last = self.quarter.unwrap_or(now);
        for q in (last + 1)..=now {
            // Quarter frame `q` is piece `q % 8` of the frame pair starting at frame `q / 4`
            // rounded down to an even frame.
            let pair_start = (q.div_euclid(8) * 2) as f64 / fps;
            let piece = q.rem_euclid(8) as usize;
            if let Some(msg) = sync::quarter_frames(pair_start, rate).get(piece) {
                out.push(msg.to_vec());
            }
        }
        self.quarter = Some(now);
    }
}

/// `midi.set {clock?, timecode?, timecode_fps?}`: what time the show sends over MIDI.
pub fn midi_set(e: &mut crate::Engine, prm: &serde_json::Value) -> actcraft_command::CommandResult {
    use actcraft_command::params as p;
    let mut next = e.show.midi.clone();
    if let Some(v) = p::opt_bool(prm, "clock")? {
        next.clock = v;
    }
    if let Some(v) = p::opt_bool(prm, "timecode")? {
        next.timecode = v;
    }
    if let Some(v) = p::opt_i64_in(prm, "timecode_fps", 24, 30)? {
        let fps = u8::try_from(v).unwrap_or(25);
        if !actcraft_show::MidiSettings::RATES.contains(&fps) {
            return Err(actcraft_command::CommandError::InvalidParams(
                "`timecode_fps` must be 24, 25 or 30".to_owned(),
            ));
        }
        next.timecode_fps = fps;
    }
    let offset = p::opt_f64_in(prm, "latency_offset_ms", -500.0, 500.0)?;
    if next != e.show.midi {
        e.show.midi = next;
        e.mark_changed();
    }
    if let Some(ms) = offset
        && ms != e.show.mixer.midi_latency_offset_ms
    {
        e.show.mixer.midi_latency_offset_ms = ms;
        e.mark_changed();
    }
    Ok(serde_json::json!({
        "clock": e.show.midi.clock,
        "timecode": e.show.midi.timecode,
        "timecode_fps": e.show.midi.timecode_fps,
        "latency_offset_ms": e.show.mixer.midi_latency_offset_ms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOCK_ONLY: SyncConfig = SyncConfig { clock: true, timecode: false, timecode_fps: 25 };
    const TIMECODE_ONLY: SyncConfig = SyncConfig { clock: false, timecode: true, timecode_fps: 25 };

    fn at(playing: bool, time: f64, beat: f64) -> Position {
        Position { playing, time, beat }
    }

    fn clocks(msgs: &[Vec<u8>]) -> usize {
        msgs.iter().filter(|m| m.as_slice() == [sync::CLOCK]).count()
    }

    #[test]
    fn the_clock_starts_ticks_and_stops_with_playback() {
        let mut s = SyncSender::default();
        assert!(s.step(CLOCK_ONLY, at(false, 0.0, 0.0)).is_empty(), "nothing while stopped");
        // Start at the song's beginning: position 0 and Start.
        assert_eq!(s.step(CLOCK_ONLY, at(true, 0.0, 0.0)), vec![vec![0xF2, 0, 0], vec![START]]);
        // Half a beat later: 12 pulses.
        assert_eq!(clocks(&s.step(CLOCK_ONLY, at(true, 0.25, 0.5))), 12);
        // Another quarter beat: 6 more, none twice.
        assert_eq!(clocks(&s.step(CLOCK_ONLY, at(true, 0.375, 0.75))), 6);
        assert_eq!(s.step(CLOCK_ONLY, at(true, 0.375, 0.75)), Vec::<Vec<u8>>::new());
        assert_eq!(s.step(CLOCK_ONLY, at(false, 0.4, 0.8)), vec![vec![STOP]]);
    }

    #[test]
    fn starting_later_in_the_song_continues_from_its_position() {
        let mut s = SyncSender::default();
        // Bar 3 of 4/4 is beat 8: 32 sixteenths.
        assert_eq!(s.step(CLOCK_ONLY, at(true, 4.0, 8.0)), vec![vec![0xF2, 32, 0], vec![CONTINUE]]);
        // During a count-in (before the anchor) the clock waits at the start.
        let mut s = SyncSender::default();
        assert_eq!(s.step(CLOCK_ONLY, at(true, -1.0, -2.0)), vec![vec![0xF2, 0, 0], vec![START]]);
        assert!(s.step(CLOCK_ONLY, at(true, -0.5, -1.0)).is_empty());
        assert_eq!(clocks(&s.step(CLOCK_ONLY, at(true, 0.5, 0.25))), 6);
    }

    #[test]
    fn a_jump_relocates_instead_of_racing() {
        let mut s = SyncSender::default();
        let _ = s.step(CLOCK_ONLY, at(true, 0.0, 0.0));
        let _ = s.step(CLOCK_ONLY, at(true, 1.0, 2.0));
        // Back to beat 0.5 (a loop going round): stop, locate, continue.
        assert_eq!(s.step(CLOCK_ONLY, at(true, 0.25, 0.5)), vec![vec![STOP], vec![0xF2, 2, 0], vec![CONTINUE]]);
        // Far ahead (a click on the waveform): no flood of pulses.
        let far = s.step(CLOCK_ONLY, at(true, 60.0, 120.0));
        assert_eq!(clocks(&far), 0);
        assert_eq!(far.first(), Some(&vec![STOP]));
    }

    #[test]
    fn timecode_sends_a_full_frame_then_quarter_frames() {
        let mut s = SyncSender::default();
        assert!(s.step(TIMECODE_ONLY, at(true, -0.5, -1.0)).is_empty(), "no time before the start");
        // At 1 s, 25 fps: a full frame 00:00:01:00.
        let first = s.step(TIMECODE_ONLY, at(true, 1.0, 2.0));
        assert_eq!(first, vec![vec![0xF0, 0x7F, 0x7F, 0x01, 0x01, 1 << 5, 0, 1, 0, 0xF7]]);
        // A frame later (40 ms): four quarter frames, the last pieces of one pair and the first of
        // the next (quarter frames 101 to 104 at 100 a second).
        let next = s.step(TIMECODE_ONLY, at(true, 1.04, 2.08));
        assert_eq!(next.len(), 4);
        let pieces: Vec<u8> = next.iter().map(|m| m.get(1).copied().unwrap_or(0) >> 4).collect();
        assert_eq!(pieces, vec![5, 6, 7, 0]);
        assert!(next.iter().all(|m| m.first() == Some(&sync::QUARTER_FRAME)));
        // A jump sends a full frame again; stopping sends nothing for timecode.
        assert_eq!(s.step(TIMECODE_ONLY, at(true, 30.0, 60.0)).len(), 1);
        assert!(s.step(TIMECODE_ONLY, at(false, 30.0, 60.0)).is_empty());
    }

    #[test]
    fn midi_set_changes_the_show_and_refuses_other_rates() {
        use serde_json::json;
        let mut e = crate::Engine::new(Box::new(crate::MemoryServices::default()));
        assert_eq!(e.show.midi, actcraft_show::MidiSettings::default());
        let r = e.run("midi.set", &json!({"clock": true, "timecode_fps": 30}));
        assert_eq!(
            r.ok(),
            Some(json!({"clock": true, "timecode": false, "timecode_fps": 30, "latency_offset_ms": 0.0}))
        );
        assert!(e.run("midi.set", &json!({"latency_offset_ms": -40})).is_ok());
        assert_eq!(e.show.mixer.midi_latency_offset_ms, -40.0);
        assert!(e.run("midi.set", &json!({"latency_offset_ms": 900})).is_err());
        assert!(e.dirty, "the show has changed");
        assert_eq!(SyncConfig::from(&e.show.midi), SyncConfig { clock: true, timecode: false, timecode_fps: 30 });
        for bad in [json!({"timecode_fps": 29}), json!({"timecode_fps": 60}), json!({"clock": "yes"})] {
            assert!(e.run("midi.set", &bad).is_err(), "{bad}");
        }
        assert_eq!(e.show.midi.timecode_fps, 30, "refused values change nothing");
        // Saved with the show, and back.
        let back = actcraft_show::Show::from_bytes(&e.show.to_bytes()).map(|s| s.midi).unwrap_or_default();
        assert_eq!(back, e.show.midi);
    }

    #[test]
    fn nothing_goes_out_when_both_are_off() {
        let mut s = SyncSender::default();
        let off = SyncConfig::default();
        assert!(s.step(off, at(true, 0.0, 0.0)).is_empty());
        assert!(s.step(off, at(true, 1.0, 2.0)).is_empty());
        assert!(s.step(off, at(false, 1.0, 2.0)).is_empty());
    }
}
