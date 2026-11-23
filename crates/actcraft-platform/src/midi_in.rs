//! MIDI input: the computer's MIDI inputs, and a connection to one that queues what comes in
//! for the window to hand to the engine ([`actcraft_engine::Engine::midi_in`]) and wakes it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The name ActCraft gives itself to the MIDI system.
const CLIENT: &str = "ActCraft";
/// Messages kept until the window takes them; the oldest go first.
const MAX_QUEUED: usize = 256;

/// Messages that came in and wait for the window.
pub type MidiQueue = Arc<Mutex<VecDeque<Vec<u8>>>>;

/// The names of the computer's MIDI inputs; empty when MIDI is not available.
pub fn input_ports() -> Vec<String> {
    let Ok(input) = midir::MidiInput::new(CLIENT) else { return Vec::new() };
    input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
}

/// A connection to one MIDI input while it lives.
pub struct MidiIn {
    port: String,
    _conn: midir::MidiInputConnection<()>,
}

impl MidiIn {
    /// Listens to the input named `port`: each message (not clock, active sensing or SysEx)
    /// goes into `queue`, and `wake` is called so the window looks at it.
    pub fn open(port: &str, queue: MidiQueue, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let mut input = midir::MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
        input.ignore(midir::Ignore::All);
        let found = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).ok().as_deref() == Some(port))
            .ok_or_else(|| format!("{port} is not connected"))?;
        let conn = input
            .connect(
                &found,
                "ActCraft remote",
                move |_stamp, bytes, _| {
                    push(&queue, bytes);
                    wake();
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { port: port.to_owned(), _conn: conn })
    }

    /// The input's name.
    pub fn port(&self) -> &str {
        &self.port
    }
}

/// Queues one message, dropping the oldest when the window hasn't kept up.
fn push(queue: &MidiQueue, bytes: &[u8]) {
    if bytes.is_empty() || bytes.len() > 3 {
        return;
    }
    if let Ok(mut q) = queue.lock() {
        if q.len() >= MAX_QUEUED {
            q.pop_front();
        }
        q.push_back(bytes.to_vec());
    }
}

/// Takes every queued message, oldest first.
pub fn drain(queue: &MidiQueue) -> Vec<Vec<u8>> {
    queue.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_keeps_the_newest_and_drops_long_messages() {
        let q: MidiQueue = Arc::default();
        for i in 0..(MAX_QUEUED + 10) {
            push(&q, &[0x90, (i % 128) as u8, 100]);
        }
        push(&q, &[0xF0, 1, 2, 3, 0xF7]);
        push(&q, &[]);
        let all = drain(&q);
        assert_eq!(all.len(), MAX_QUEUED);
        assert_eq!(all.first(), Some(&vec![0x90, 10, 100]), "the oldest went first");
        assert!(drain(&q).is_empty());
    }
}
