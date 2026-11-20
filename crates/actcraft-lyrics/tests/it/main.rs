//! Fuzz sweep for the lyrics parser and chord transposer: random markup never panics.

use actcraft_lyrics::{decode_text, parse, stamp_line, transpose_chord};

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

const PARTS: &[&str] = &[
    "[",
    "]",
    "[Am]",
    "[00:12.5]",
    "[99:99:99]",
    "{",
    "}",
    "{title: x}",
    "{soc}",
    "{c:",
    "*",
    "**",
    "_",
    "__",
    "<color=red>",
    "</color>",
    "<color=#zzzzzz>",
    "<font=serif>",
    "</font>",
    "<",
    ">",
    "# ",
    "## ",
    "\n",
    "\r",
    "\r\n",
    "é",
    "\u{0}",
    "word ",
    ":",
    "/",
    "#",
    "b",
];

#[test]
fn random_lyrics_never_panic() {
    let mut rng = Rng(11);
    for _ in 0..4000 {
        let n = (rng.next() % 40) as usize;
        let src: String = (0..n).map(|_| rng.pick(PARTS)).collect();
        let lyrics = parse(&src);
        let _ = lyrics.line_at(10.0);
        let _ = stamp_line(&src, (rng.next() % 5) as usize, 1.5);
        let bytes: Vec<u8> = src.bytes().collect();
        let _ = decode_text(&bytes);
        for l in &lyrics.lines {
            for s in &l.spans {
                if let Some(c) = &s.chord {
                    let _ = transpose_chord(c, (rng.next() % 25) as i32 - 12, rng.next().is_multiple_of(2));
                }
            }
        }
    }
}
