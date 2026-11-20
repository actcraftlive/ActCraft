//! The beat clock: song time to beats and bars, and the beat grid with snapping.
//!
//! The same clock drives the metronome and (from milestone M7) the lighting effects.

/// Tempo, meter and where beat one of bar one sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatClock {
    pub bpm: f64,
    pub beats_per_bar: u8,
    /// Song time of beat one, bar one, in seconds; independent of where playback starts.
    pub anchor: f64,
}

impl Default for BeatClock {
    fn default() -> Self {
        Self { bpm: 120.0, beats_per_bar: 4, anchor: 0.0 }
    }
}

impl BeatClock {
    /// A clock with values forced into a usable range.
    pub fn new(bpm: f64, beats_per_bar: u8, anchor: f64) -> Self {
        let bpm = if bpm.is_finite() { bpm.clamp(20.0, 400.0) } else { 120.0 };
        let anchor = if anchor.is_finite() { anchor } else { 0.0 };
        Self { bpm, beats_per_bar: beats_per_bar.max(1), anchor }
    }

    /// Seconds per beat.
    pub fn beat_len(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Beats since the anchor at song time `t` (negative before it).
    pub fn beat_at(&self, t: f64) -> f64 {
        (t - self.anchor) / self.beat_len()
    }

    /// Song time of a beat number.
    pub fn time_of_beat(&self, beat: f64) -> f64 {
        self.anchor + beat * self.beat_len()
    }

    /// Bar (0-based, may be negative) and beat in bar (0-based) at `t`.
    pub fn bar_beat(&self, t: f64) -> (i64, u8) {
        let beat = self.beat_at(t).floor();
        let bpb = f64::from(self.beats_per_bar);
        let bar = (beat / bpb).floor();
        let in_bar = beat - bar * bpb;
        (bar as i64, in_bar.clamp(0.0, 255.0) as u8)
    }

    /// The next beat at or after `t`.
    pub fn next_beat_time(&self, t: f64) -> f64 {
        self.time_of_beat(self.beat_at(t).ceil())
    }

    /// Snaps `t` to the nearest grid line, `division` lines per beat (1 = beats, 4 = sixteenths).
    pub fn snap(&self, t: f64, division: u32) -> f64 {
        let div = f64::from(division.clamp(1, 64));
        self.time_of_beat((self.beat_at(t) * div).round() / div)
    }

    /// Length of `bars` bars in seconds.
    pub fn bars_len(&self, bars: u8) -> f64 {
        f64::from(bars) * f64::from(self.beats_per_bar) * self.beat_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_relative_to_anchor() {
        let c = BeatClock::new(120.0, 4, 0.25);
        assert!((c.snap(0.5, 1) - 0.25).abs() < 1e-9 || (c.snap(0.5, 1) - 0.75).abs() < 1e-9);
        assert!((c.next_beat_time(0.3) - 0.75).abs() < 1e-9);
        assert_eq!(c.bar_beat(0.25 + 2.0), (1, 0));
        assert_eq!(c.bar_beat(0.0).0, -1);
    }
}
