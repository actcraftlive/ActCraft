//! Fuzz sweeps for the library parsers: search queries and filename format strings.

use actcraft_library::{filename, search::Query};
use actcraft_show::Song;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const QUERY_PARTS: &[&str] = &[
    "drive", "\"night", "drive\"", "-", "NOT", "AND", "OR", "(", ")", "title:", "artist:", "keyword:", "bpm:", ">",
    "<=", "120", "x", " ", "é", "\u{0}", "::", "year:", "1e999", "-NOT",
];

#[test]
fn random_queries_never_panic() {
    let songs = [Song { id: 1, title: "Night Drive".into(), bpm: 128.0, year: Some(2020), ..Song::default() }];
    let mut rng = Rng(42);
    for _ in 0..5000 {
        let n = (rng.next() % 24) as usize;
        let q: String = (0..n).map(|_| rng.pick(QUERY_PARTS)).collect();
        if let Ok(parsed) = Query::parse(&q) {
            let _ = parsed.matches(&songs[0]);
        }
    }
}

const NAME_PARTS: &[&str] =
    &["%title%", "%artist%", "%bpm%", "%year%", "%key%", "%ignore%", " - ", "[", "]", "%", "x", "_", "é", ".wav"];

#[test]
fn random_filename_formats_never_panic() {
    let mut rng = Rng(7);
    for _ in 0..5000 {
        let fmt: String = (0..(rng.next() % 8)).map(|_| rng.pick(NAME_PARTS)).collect();
        let name: String = (0..(rng.next() % 10)).map(|_| rng.pick(NAME_PARTS)).collect();
        let _ = filename::parse(&name, &fmt);
    }
}
