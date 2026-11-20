//! Waveform peaks for drawing, at any zoom.

use crate::Clip;

/// Frames summarised by one base peak.
pub const BASE_BUCKET: usize = 256;

/// Min/max pairs of a clip (channels mixed), one pair per [`BASE_BUCKET`] frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Peaks {
    pub sample_rate: u32,
    pub pairs: Vec<(f32, f32)>,
}

impl Peaks {
    pub fn of(clip: &Clip) -> Peaks {
        let chans = usize::from(clip.channels.max(1));
        let frames = clip.frames();
        let mut pairs = Vec::with_capacity(frames / BASE_BUCKET + 1);
        let mut start = 0;
        while start < frames {
            let end = (start + BASE_BUCKET).min(frames);
            let (mut lo, mut hi) = (0f32, 0f32);
            for f in start..end {
                let mut v = 0.0;
                for c in 0..chans {
                    v += clip.data.get(f * chans + c).copied().unwrap_or(0.0);
                }
                v /= chans as f32;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            pairs.push((lo, hi));
            start = end;
        }
        Peaks { sample_rate: clip.sample_rate, pairs }
    }

    /// `columns` min/max pairs covering seconds `from..to` (the visible window at the current
    /// zoom). Columns outside the audio are silent.
    pub fn window(&self, from: f64, to: f64, columns: usize) -> Vec<(f32, f32)> {
        let columns = columns.min(16_384);
        // NaN bounds or an empty window draw silence.
        if columns == 0 || to.is_nan() || from.is_nan() || to <= from || self.sample_rate == 0 {
            return vec![(0.0, 0.0); columns];
        }
        let per_sec = f64::from(self.sample_rate) / BASE_BUCKET as f64;
        let span = (to - from) / columns as f64;
        (0..columns)
            .map(|c| {
                let a = ((from + span * c as f64) * per_sec).floor();
                let b = ((from + span * (c as f64 + 1.0)) * per_sec).ceil().max(a + 1.0);
                if b <= 0.0 {
                    return (0.0, 0.0);
                }
                let a = a.max(0.0) as usize;
                let b = (b as usize).min(self.pairs.len());
                self.pairs
                    .get(a..b)
                    .map_or((0.0, 0.0), |s| s.iter().fold((0f32, 0f32), |(lo, hi), &(l, h)| (lo.min(l), hi.max(h))))
            })
            .collect()
    }
}
