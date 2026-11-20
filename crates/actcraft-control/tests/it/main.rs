//! Fuzz sweep for the control channel: random request lines must get an answer or be ignored,
//! never crash the engine.

use actcraft_control::handle_line;
use actcraft_engine::{Engine, MemoryServices};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

const SEEDS: &[&str] = &[
    r#"{"jsonrpc":"2.0","id":1,"method":"command.run","params":{"id":"song.add","params":{"title":"A"}}}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"state.get"}"#,
    r#"[{"jsonrpc":"2.0","id":3,"method":"commands.list"},{"jsonrpc":"2.0","method":"x"}]"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"command.run","params":{"id":"playlist.move_songs","params":{"playlist":0,"indexes":[0],"to":9}}}"#,
];

#[test]
fn random_request_lines_never_panic() {
    let mut e = Engine::new(Box::new(MemoryServices::default()));
    let mut rng = Rng(2026);
    for i in 0..3000 {
        let mut line = SEEDS[i % SEEDS.len()].as_bytes().to_vec();
        for _ in 0..(rng.next() % 5) {
            let at = (rng.next() % line.len() as u64) as usize;
            line[at] = rng.next() as u8;
        }
        let _ = handle_line(&mut e, &line, None);
    }
}
