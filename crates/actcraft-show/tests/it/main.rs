//! Fuzz sweep for the show file parser: mutated show files must load or fail, never panic.

use actcraft_show::{Show, Song, Track};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: deterministic, so a failure reproduces.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut out = seed.to_vec();
    for _ in 0..=rng.below(8) {
        match rng.below(4) {
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] = rng.next() as u8;
            }
            1 if !out.is_empty() => {
                let i = rng.below(out.len());
                out.truncate(i);
            }
            2 => {
                let i = rng.below(out.len() + 1);
                let junk: &[u8] =
                    [b"{".as_slice(), b"[", b"\"", b"1e999", b"-", b"null", b"\\u0000", b"}"][rng.below(8)];
                out.splice(i..i, junk.iter().copied());
            }
            _ if !out.is_empty() => {
                let a = rng.below(out.len());
                let b = rng.below(out.len());
                out.swap(a, b);
            }
            _ => {}
        }
    }
    out
}

#[test]
fn mutated_show_files_never_panic() {
    let mut show = Show::default();
    let mut song = Song { id: 1, title: "Seed".into(), keywords: vec!["Live".into()], ..Song::default() };
    song.tracks.push(Track { name: "Drums".into(), file: "drums.wav".into(), ..Track::default() });
    show.songs.push(song);
    let seed = show.to_bytes();
    let mut rng = Rng(0x00C0_FFEE_1234_5678);
    for _ in 0..3000 {
        let input = mutate(&mut rng, &seed);
        if let Ok(s) = Show::from_bytes(&input) {
            // Whatever loads must save and load again.
            assert!(Show::from_bytes(&s.to_bytes()).is_ok());
        }
    }
}
