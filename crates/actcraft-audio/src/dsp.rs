//! Small DSP building blocks: gain conversion, pan laws and biquad EQ filters.
//!
//! Filter formulas follow the public "Audio EQ Cookbook" (Robert Bristow-Johnson).

use std::f64::consts::{FRAC_PI_4, PI};

/// dB to linear gain; -96 dB and below is silence.
pub fn db_to_gain(db: f64) -> f32 {
    if !db.is_finite() || db <= -96.0 {
        return 0.0;
    }
    10f64.powf(db.min(24.0) / 20.0) as f32
}

/// Linear gain to dB (for meters); silence is -96 dB.
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 0.000_015_85 { -96.0 } else { 20.0 * g.log10() }
}

/// Constant-power pan for a mono source: (left, right) gains, centre = -3 dB each.
pub fn pan_mono(pan: f32) -> (f32, f32) {
    let p = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    let angle = (f64::from(p) + 1.0) * FRAC_PI_4;
    (angle.cos() as f32, angle.sin() as f32)
}

/// Balance for a stereo source: centre leaves both sides at unity.
pub fn pan_stereo(pan: f32) -> (f32, f32) {
    let p = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    (if p > 0.0 { 1.0 - p } else { 1.0 }, if p < 0.0 { 1.0 + p } else { 1.0 })
}

/// Normalised biquad coefficients (a0 = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
    /// True when the filter does nothing (0 dB), so it can be skipped.
    pub bypass: bool,
}

impl Default for Coeffs {
    fn default() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, bypass: true }
    }
}

/// Filter shape of an EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    LowShelf,
    Peak,
    HighShelf,
}

/// The shape of each of the four bands.
pub const BAND_SHAPES: [Shape; 4] = [Shape::LowShelf, Shape::Peak, Shape::Peak, Shape::HighShelf];

/// Computes coefficients for one band. Out-of-range input is clamped; 0 dB gives a bypass.
pub fn coeffs(shape: Shape, freq: f64, gain_db: f64, q: f64, sample_rate: f64) -> Coeffs {
    if !gain_db.is_finite() || gain_db.abs() < 0.01 || !sample_rate.is_finite() || sample_rate < 1000.0 {
        return Coeffs::default();
    }
    let gain_db = gain_db.clamp(-24.0, 24.0);
    let freq = if freq.is_finite() { freq.clamp(10.0, sample_rate * 0.49) } else { 1000.0 };
    let q = if q.is_finite() { q.clamp(0.1, 18.0) } else { 0.7 };
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * PI * freq / sample_rate;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q);
    let sa = 2.0 * a.sqrt() * alpha;
    let (b0, b1, b2, a0, a1, a2) = match shape {
        Shape::Peak => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
        Shape::LowShelf => (
            a * ((a + 1.0) - (a - 1.0) * cos + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sa),
            (a + 1.0) + (a - 1.0) * cos + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sa,
        ),
        Shape::HighShelf => (
            a * ((a + 1.0) + (a - 1.0) * cos + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sa),
            (a + 1.0) - (a - 1.0) * cos + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sa,
        ),
    };
    if a0.abs() < 1e-12 {
        return Coeffs::default();
    }
    Coeffs {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
        bypass: false,
    }
}

/// Filter memory for one channel of one band (transposed direct form II).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    pub fn process(&mut self, c: &Coeffs, x: f32) -> f32 {
        if c.bypass {
            return x;
        }
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        // A blown-up filter (hostile coefficients, denormals) resets instead of poisoning the mix.
        if !y.is_finite() {
            *self = Self::default();
            return 0.0;
        }
        y
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_boost_raises_level_at_its_frequency() {
        let sr = 48_000.0;
        let c = coeffs(Shape::Peak, 1000.0, 12.0, 1.0, sr);
        let mut st = BiquadState::default();
        let mut peak_in = 0f32;
        let mut peak_out = 0f32;
        for i in 0..48_000 {
            let x = (2.0 * PI * 1000.0 * f64::from(i) / sr).sin() as f32 * 0.1;
            let y = st.process(&c, x);
            if i > 4800 {
                peak_in = peak_in.max(x.abs());
                peak_out = peak_out.max(y.abs());
            }
        }
        assert!(peak_out > peak_in * 3.0);
        assert!(coeffs(Shape::Peak, 1000.0, 0.0, 1.0, sr).bypass);
    }

    #[test]
    fn pan_is_constant_power_and_bounded() {
        let (l, r) = pan_mono(0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        assert_eq!(pan_mono(f32::NAN), pan_mono(0.0));
    }
}
