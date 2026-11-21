//! Tempo and beat position from audio, so the click lines up with a backing track.
//!
//! 1. An onset-strength envelope (positive energy change per 256-sample hop).
//! 2. Tempo: every candidate from 60 to 200 BPM in 0.02 BPM steps is scored by the envelope's
//!    autocorrelation at 1..=8 beat periods, read at fractional lags. Checking whole multiples
//!    of the period keeps the error per beat tiny, so the click doesn't drift over a song (an
//!    integer-lag estimate was off by up to 1 %, a full beat every 100 beats).
//! 3. Beat position: the phase whose beat grid collects the most onset strength gives the time
//!    of a beat; that becomes the song's beat anchor.
//!
//! Good for steady backing tracks; the person can always nudge tempo and first beat by hand.

use crate::Clip;

/// Samples per analysis hop.
const HOP: usize = 256;
/// Audio analysed from the start of the song.
const MAX_SECONDS: f64 = 120.0;
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
const BPM_STEP: f64 = 0.02;
/// Beat periods checked per candidate.
const MULTIPLES: usize = 8;

/// Tempo and the time of one beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    pub bpm: f64,
    /// Seconds of a beat (the first one in the song, within one beat period of zero).
    pub anchor: f64,
}

fn onset_envelope(clip: &Clip) -> Option<(Vec<f64>, f64)> {
    let sr = f64::from(clip.sample_rate);
    if sr < 1000.0 {
        return None;
    }
    let chans = usize::from(clip.channels.max(1));
    let frames = clip.frames().min((MAX_SECONDS * sr) as usize);
    let hops = frames / HOP;
    if hops < 128 {
        return None;
    }
    let mut energy = Vec::with_capacity(hops);
    for h in 0..hops {
        let mut e = 0.0f64;
        for f in h * HOP..(h + 1) * HOP {
            let mut v = 0.0f32;
            for c in 0..chans {
                v += clip.data.get(f * chans + c).copied().unwrap_or(0.0);
            }
            e += f64::from(v * v);
        }
        // Log energy makes quiet and loud passages count alike.
        energy.push((1.0 + 1000.0 * e / HOP as f64).ln());
    }
    let onset: Vec<f64> = std::iter::once(0.0).chain(energy.windows(2).map(|w| (w[1] - w[0]).max(0.0))).collect();
    let mean = onset.iter().sum::<f64>() / onset.len() as f64;
    if mean <= 1e-9 {
        return None;
    }
    Some((onset.iter().map(|v| v - mean).collect(), HOP as f64 / sr))
}

/// Tempo and beat position of a clip, or `None` when there's too little rhythmic audio.
pub fn analyse(clip: &Clip) -> Option<Beat> {
    let (onset, hop_s) = onset_envelope(clip)?;
    let n = onset.len();
    let max_lag = ((60.0 / MIN_BPM) / hop_s * MULTIPLES as f64).ceil() as usize + 2;
    if n < max_lag * 2 {
        return None;
    }
    // Autocorrelation at every integer lag up to the longest multiple we read.
    let corr: Vec<f64> =
        (0..=max_lag).map(|lag| onset.iter().zip(onset.iter().skip(lag)).map(|(a, b)| a * b).sum::<f64>()).collect();
    let at = |lag: f64| -> f64 {
        let i = lag.floor() as usize;
        let frac = lag - lag.floor();
        let a = corr.get(i).copied().unwrap_or(0.0);
        let b = corr.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * frac
    };
    let steps = ((MAX_BPM - MIN_BPM) / BPM_STEP) as usize;
    let mut best = (0.0f64, f64::MIN);
    for s in 0..=steps {
        let bpm = MIN_BPM + s as f64 * BPM_STEP;
        let period = 60.0 / bpm / hop_s;
        let score: f64 = (1..=MULTIPLES).map(|k| at(period * k as f64)).sum();
        if score > best.1 {
            best = (bpm, score);
        }
    }
    if best.1 <= 0.0 {
        return None;
    }
    let mut bpm = best.0;
    // Fold half and double time into the usual range.
    while bpm < 70.0 {
        bpm *= 2.0;
    }
    while bpm > 180.0 {
        bpm /= 2.0;
    }
    // Beat phase: the offset whose grid gathers the most onset strength.
    let period = 60.0 / bpm / hop_s;
    let phases = period.ceil() as usize;
    let mut best_phase = (0usize, f64::MIN);
    for phase in 0..phases {
        let mut sum = 0.0;
        let mut pos = phase as f64;
        while (pos as usize) < n {
            sum += onset.get(pos.round() as usize).copied().unwrap_or(0.0);
            pos += period;
        }
        if sum > best_phase.1 {
            best_phase = (phase, sum);
        }
    }
    // An onset shows in the hop after the hit starts; step back half a hop.
    let anchor = ((best_phase.0 as f64 - 0.5).max(0.0)) * hop_s;
    Some(Beat { bpm: (bpm * 100.0).round() / 100.0, anchor: (anchor * 1000.0).round() / 1000.0 })
}

/// Estimated tempo in BPM.
pub fn estimate(clip: &Clip) -> Option<f64> {
    analyse(clip).map(|b| b.bpm)
}

/// Tempo from tap times in seconds (newest last): the median gap of the last taps.
pub fn from_taps(times: &[f64]) -> Option<f64> {
    let recent = times.get(times.len().saturating_sub(9)..)?;
    let mut gaps: Vec<f64> = recent.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > 0.15 && *g < 3.0).collect();
    if gaps.len() < 2 {
        return None;
    }
    gaps.sort_by(f64::total_cmp);
    let median = gaps.get(gaps.len() / 2).copied()?;
    Some(((60.0 / median) * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click track at `bpm` whose first click is at `offset` seconds.
    fn clicks(bpm: f64, seconds: f64, offset: f64) -> Clip {
        let sr = 44_100u32;
        let n = (f64::from(sr) * seconds) as usize;
        let period = 60.0 / bpm * f64::from(sr);
        let start = offset * f64::from(sr);
        let data = (0..n)
            .map(|i| {
                let pos = i as f64 - start;
                if pos < 0.0 {
                    return 0.0;
                }
                let into = pos % period;
                if into < 300.0 { 0.8 * ((i % 37) as f32 / 37.0 - 0.5) } else { 0.0 }
            })
            .collect();
        Clip { sample_rate: sr, channels: 1, data }
    }

    #[test]
    fn tempo_is_precise_enough_not_to_drift() {
        // Regression: integer-lag estimates were up to 1 % off (128 BPM read as 129.2), so the
        // click drifted a beat every 100 beats. Now the error must stay under 0.1 BPM.
        for bpm in [92.0, 120.0, 128.0, 140.0, 174.0] {
            let est = estimate(&clicks(bpm, 60.0, 0.0)).unwrap_or(0.0);
            assert!((est - bpm).abs() < 0.1, "{bpm} estimated as {est}");
        }
        assert_eq!(estimate(&Clip { sample_rate: 44_100, channels: 1, data: vec![0.0; 44_100 * 10] }), None);
    }

    #[test]
    fn finds_where_the_beat_falls() {
        // A track that starts its beat 0.37 s in: the anchor lands on a beat.
        let beat = analyse(&clicks(120.0, 40.0, 0.37)).unwrap_or(Beat { bpm: 0.0, anchor: -1.0 });
        let period = 60.0 / beat.bpm;
        let off = (beat.anchor - 0.37).rem_euclid(period);
        let err = off.min(period - off);
        assert!(err < 0.012, "anchor {} vs 0.37 (error {err})", beat.anchor);
    }

    #[test]
    fn taps_give_the_median_tempo() {
        let taps = [0.0, 0.5, 1.0, 1.52, 2.0, 2.5];
        assert!(from_taps(&taps).is_some_and(|b| (b - 120.0).abs() < 2.0));
        assert_eq!(from_taps(&[1.0]), None);
    }
}
