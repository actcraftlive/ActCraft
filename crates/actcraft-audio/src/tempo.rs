//! Tempo estimate from audio: an onset-strength envelope and its autocorrelation.
//!
//! Good enough to suggest a BPM for a backing track (drums, steady rhythm); the person can always
//! correct it or tap it in. Half and double tempo are folded into 70..=180 BPM.

use crate::Clip;

/// Samples per analysis hop at the analysis rate.
const HOP: usize = 512;
/// Most audio analysed: the first 90 seconds is plenty and keeps it fast.
const MAX_SECONDS: f64 = 90.0;

/// Estimated tempo in BPM, or `None` when there's too little or no rhythmic audio.
pub fn estimate(clip: &Clip) -> Option<f64> {
    let sr = f64::from(clip.sample_rate);
    if sr < 1000.0 {
        return None;
    }
    let chans = usize::from(clip.channels.max(1));
    let frames = clip.frames().min((MAX_SECONDS * sr) as usize);
    let hops = frames / HOP;
    if hops < 64 {
        return None;
    }
    // Energy per hop, then the positive change (onset strength).
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
        energy.push((e / HOP as f64).sqrt());
    }
    let mut onset: Vec<f64> = energy.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    let mean = onset.iter().sum::<f64>() / onset.len().max(1) as f64;
    if mean <= 1e-9 {
        return None;
    }
    for v in &mut onset {
        *v -= mean;
    }
    let hop_seconds = HOP as f64 / sr;
    let lag_for = |bpm: f64| (60.0 / bpm / hop_seconds).round() as usize;
    let (min_lag, max_lag) = (lag_for(220.0).max(1), lag_for(55.0).min(onset.len() / 2));
    if min_lag >= max_lag {
        return None;
    }
    let corr = |lag: usize| -> f64 { onset.iter().zip(onset.iter().skip(lag)).map(|(a, b)| a * b).sum() };
    let mut best = (0usize, f64::MIN);
    for lag in min_lag..=max_lag {
        // Reward lags whose double also correlates (the beat, not an off-beat).
        let score = corr(lag) + 0.5 * if lag * 2 < onset.len() { corr(lag * 2) } else { 0.0 };
        if score > best.1 {
            best = (lag, score);
        }
    }
    if best.0 == 0 || best.1 <= 0.0 {
        return None;
    }
    let mut bpm = 60.0 / (best.0 as f64 * hop_seconds);
    while bpm < 70.0 {
        bpm *= 2.0;
    }
    while bpm > 180.0 {
        bpm /= 2.0;
    }
    Some((bpm * 10.0).round() / 10.0)
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

    fn clicks(bpm: f64, seconds: f64) -> Clip {
        let sr = 22_050u32;
        let n = (f64::from(sr) * seconds) as usize;
        let period = (60.0 / bpm * f64::from(sr)) as usize;
        let data = (0..n).map(|i| if i % period < 400 { 0.8 * ((i % 37) as f32 / 37.0 - 0.5) } else { 0.0 }).collect();
        Clip { sample_rate: sr, channels: 1, data }
    }

    #[test]
    fn finds_the_tempo_of_a_click_track() {
        for bpm in [92.0, 120.0, 140.0] {
            let est = estimate(&clicks(bpm, 30.0)).unwrap_or(0.0);
            assert!((est - bpm).abs() < 3.0, "{bpm} estimated as {est}");
        }
        assert_eq!(estimate(&Clip { sample_rate: 22_050, channels: 1, data: vec![0.0; 22_050 * 10] }), None);
    }

    #[test]
    fn taps_give_the_median_tempo() {
        let taps = [0.0, 0.5, 1.0, 1.52, 2.0, 2.5];
        assert!(from_taps(&taps).is_some_and(|b| (b - 120.0).abs() < 2.0));
        assert_eq!(from_taps(&[1.0]), None);
    }
}
