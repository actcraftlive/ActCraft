//! A lock-free "latest value" cell for the renderer's status.
//!
//! A ring buffer would hand the engine old statuses when it falls behind (a minimised window
//! polls rarely). This cell always holds the newest one. The writer (audio thread) never waits;
//! the reader retries while a write is in progress (a sequence lock built on atomics).

use std::sync::atomic::{AtomicU64, Ordering};

use crate::render::{BUSES, Status};

const FIELDS: usize = 8 + BUSES;

pub struct StatusCell {
    seq: AtomicU64,
    fields: [AtomicU64; FIELDS],
}

impl Default for StatusCell {
    fn default() -> Self {
        Self { seq: AtomicU64::new(0), fields: std::array::from_fn(|_| AtomicU64::new(0)) }
    }
}

fn encode(s: &Status) -> [u64; FIELDS] {
    let mut f = [0u64; FIELDS];
    let flags = u64::from(s.loaded)
        | u64::from(s.playing) << 1
        | u64::from(s.counting_in) << 2
        | u64::from(s.crossfading) << 3
        | u64::from(s.in_loop) << 4;
    f[0] = flags | u64::from(s.beat_in_bar) << 8;
    f[1] = s.time.to_bits();
    f[2] = s.beat.to_bits();
    f[3] = s.bar as u64;
    f[4] = u64::from(s.ended_count);
    f[5] = s.frames;
    for (i, p) in s.bus_peaks.iter().enumerate() {
        if let Some(slot) = f.get_mut(8 + i) {
            *slot = u64::from(p.to_bits());
        }
    }
    f
}

fn decode(f: &[u64; FIELDS]) -> Status {
    let mut s = Status {
        loaded: f[0] & 1 != 0,
        playing: f[0] & 2 != 0,
        counting_in: f[0] & 4 != 0,
        crossfading: f[0] & 8 != 0,
        in_loop: f[0] & 16 != 0,
        beat_in_bar: ((f[0] >> 8) & 0xff) as u8,
        time: f64::from_bits(f[1]),
        beat: f64::from_bits(f[2]),
        bar: f[3] as i64,
        ended_count: (f[4] & 0xffff_ffff) as u32,
        frames: f[5],
        ..Status::default()
    };
    for (i, p) in s.bus_peaks.iter_mut().enumerate() {
        *p = f32::from_bits((f.get(8 + i).copied().unwrap_or(0) & 0xffff_ffff) as u32);
    }
    s
}

impl StatusCell {
    /// Audio thread: publish a new status. Never blocks.
    pub fn store(&self, s: &Status) {
        let words = encode(s);
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        std::sync::atomic::fence(Ordering::Release);
        for (slot, w) in self.fields.iter().zip(words) {
            slot.store(w, Ordering::Relaxed);
        }
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// Engine thread: read the newest status. `None` only if every attempt raced a write.
    pub fn load(&self) -> Option<Status> {
        for _ in 0..64 {
            let before = self.seq.load(Ordering::Acquire);
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let mut words = [0u64; FIELDS];
            for (w, slot) in words.iter_mut().zip(self.fields.iter()) {
                *w = slot.load(Ordering::Relaxed);
            }
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == before {
                return Some(decode(&words));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_status() {
        let cell = StatusCell::default();
        let mut s = Status { playing: true, time: -1.5, bar: -2, beat_in_bar: 3, ended_count: 9, ..Status::default() };
        s.bus_peaks[5] = 0.25;
        cell.store(&s);
        assert_eq!(cell.load(), Some(s));
    }
}
