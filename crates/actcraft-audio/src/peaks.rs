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

/// Start of a stored peaks file (format version 1).
const MAGIC: &[u8; 8] = b"ACPEAKS1";
/// Most pairs a stored peaks file may hold (about 7 hours at 192 kHz).
pub const MAX_STORED_PAIRS: usize = 20_000_000;

impl Peaks {
    /// The peaks as bytes for a cache file: magic, sample rate, count, then min/max pairs, all
    /// little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = self.pairs.len().min(MAX_STORED_PAIRS);
        let mut out = Vec::with_capacity(16 + n * 8);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&u32::try_from(n).unwrap_or(0).to_le_bytes());
        for &(lo, hi) in self.pairs.iter().take(n) {
            out.extend_from_slice(&lo.to_le_bytes());
            out.extend_from_slice(&hi.to_le_bytes());
        }
        out
    }

    /// Reads [`Peaks::to_bytes`]; `None` for anything else (wrong magic, length or values).
    pub fn from_bytes(bytes: &[u8]) -> Option<Peaks> {
        let rest = bytes.strip_prefix(MAGIC)?;
        let (rate, rest) = rest.split_first_chunk::<4>()?;
        let (count, rest) = rest.split_first_chunk::<4>()?;
        let sample_rate = u32::from_le_bytes(*rate);
        let count = usize::try_from(u32::from_le_bytes(*count)).ok()?;
        if sample_rate == 0 || count > MAX_STORED_PAIRS || rest.len() != count.checked_mul(8)? {
            return None;
        }
        let mut pairs = Vec::with_capacity(count);
        for pair in rest.as_chunks::<8>().0 {
            let lo = f32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
            let hi = f32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
            if !(lo.is_finite() && hi.is_finite()) {
                return None;
            }
            pairs.push((lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0)));
        }
        Some(Peaks { sample_rate, pairs })
    }
}

#[cfg(test)]
mod stored_tests {
    use super::Peaks;

    #[test]
    fn peaks_survive_a_round_trip_and_bad_bytes_are_refused() {
        let p = Peaks { sample_rate: 48_000, pairs: vec![(-0.5, 0.25), (0.0, 1.0)] };
        assert_eq!(Peaks::from_bytes(&p.to_bytes()), Some(p.clone()));
        let mut short = p.to_bytes();
        short.pop();
        assert_eq!(Peaks::from_bytes(&short), None);
        assert_eq!(Peaks::from_bytes(b"ACPEAKS1"), None);
        assert_eq!(Peaks::from_bytes(b"something else entirely"), None);
        let mut nan = p.to_bytes();
        nan[16..20].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Peaks::from_bytes(&nan), None);
        // A count far beyond the data is refused before anything is allocated.
        let mut huge = p.to_bytes();
        huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Peaks::from_bytes(&huge), None);
    }
}
