//! Fuzz sweep for audio decoding and WAV markers: damaged files must fail cleanly, never panic,
//! and decoded clips must never hold non-finite samples.

use actcraft_audio::clip;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn wav(frames: u32) -> Vec<u8> {
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&44_100u32.to_le_bytes());
    w.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(frames * 4).to_le_bytes());
    for i in 0..frames {
        let v = ((i % 100) as i16 - 50) * 300;
        w.extend_from_slice(&v.to_le_bytes());
        w.extend_from_slice(&(-v).to_le_bytes());
    }
    w
}

#[test]
fn damaged_wav_files_never_panic() {
    let seed = wav(2000);
    let mut rng = Rng(99);
    for round in 0..1500 {
        let mut input = seed.clone();
        for _ in 0..(1 + rng.next() % 6) {
            let i = (rng.next() % input.len() as u64) as usize;
            input[i] = rng.next() as u8;
        }
        if round % 5 == 0 {
            input.truncate((rng.next() % seed.len() as u64) as usize);
        }
        if let Ok(c) = clip::decode(&input, Some("wav")) {
            assert!(c.data.iter().all(|s| s.is_finite()));
        }
        let _ = clip::wav_markers(&input);
    }
}
