//! Decoded audio held in memory for playback.

use std::sync::Arc;

/// Most samples (frames × channels) one decoded file may hold: 4 hours of 48 kHz stereo.
pub const MAX_CLIP_SAMPLES: u64 = 4 * 3600 * 48_000 * 2;

/// Decoded audio: interleaved `f32`, one or two channels.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    pub data: Vec<f32>,
}

impl Clip {
    /// Builds a clip from planar channels. More than two channels fold to stereo (centre and
    /// surrounds mixed into left and right at -3 dB). Non-finite samples become silence.
    pub fn from_planar(sample_rate: u32, planar: &[Vec<f32>]) -> Option<Clip> {
        if sample_rate == 0 || planar.is_empty() {
            return None;
        }
        let frames = planar.iter().map(Vec::len).min().unwrap_or(0);
        let clean = |x: f32| if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        let at = |ch: usize, i: usize| planar.get(ch).and_then(|c| c.get(i)).copied().map_or(0.0, clean);
        if planar.len() == 1 {
            let data = (0..frames).map(|i| at(0, i)).collect();
            return Some(Clip { sample_rate, channels: 1, data });
        }
        let mut data = Vec::with_capacity(frames.saturating_mul(2));
        let extra = planar.len().saturating_sub(2);
        for i in 0..frames {
            let mut l = at(0, i);
            let mut r = at(1, i);
            for ch in 0..extra {
                let v = at(ch + 2, i) * std::f32::consts::FRAC_1_SQRT_2;
                l += v;
                r += v;
            }
            data.push(l);
            data.push(r);
        }
        Some(Clip { sample_rate, channels: 2, data })
    }

    pub fn frames(&self) -> usize {
        self.data.len() / usize::from(self.channels.max(1))
    }

    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / f64::from(self.sample_rate) }
    }

    /// Sample of channel `ch` at fractional frame `pos`, linearly interpolated; silence outside.
    #[inline]
    pub fn sample(&self, ch: usize, pos: f64) -> f32 {
        // NaN and negative positions are silence.
        if pos.is_nan() || pos < 0.0 {
            return 0.0;
        }
        let chans = usize::from(self.channels.max(1));
        let ch = ch.min(chans - 1);
        let i = pos as usize;
        let frac = (pos - i as f64) as f32;
        let a = self.data.get(i * chans + ch).copied().unwrap_or(0.0);
        let b = self.data.get((i + 1) * chans + ch).copied().unwrap_or(0.0);
        a + (b - a) * frac
    }
}

/// Decodes an audio file (WAV, AIFF, FLAC, MP3 and the other formats craft-audio reads natively).
pub fn decode(bytes: &[u8], ext_hint: Option<&str>) -> Result<Arc<Clip>, String> {
    let opts = craft_audio::DecodeOptions::default().with_max_samples(MAX_CLIP_SAMPLES);
    let (_, buf) = craft_audio::decode_with(bytes, ext_hint, &opts).map_err(|e| e.to_string())?;
    Clip::from_planar(buf.sample_rate, &buf.channels).map(Arc::new).ok_or_else(|| "the file has no audio".to_owned())
}

/// Marker positions (seconds) and names embedded in a WAV file, for importing regions.
pub fn wav_markers(bytes: &[u8]) -> Vec<(f64, f64, String)> {
    let m = craft_audio::wav_markers::read(bytes);
    m.cue
        .iter()
        .chain(m.xmp.iter())
        .filter(|mk| mk.rate.is_finite() && mk.rate > 0.0 && mk.start.is_finite() && mk.start >= 0.0)
        .take(64)
        .map(|mk| {
            let dur = if mk.duration.is_finite() && mk.duration > 0.0 { mk.duration / mk.rate } else { 0.0 };
            (mk.start / mk.rate, dur, mk.name.clone())
        })
        .collect()
}
