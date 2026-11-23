//! OSC in: listens for Open Sound Control messages on a UDP port and queues their address and
//! first value for the window to hand to the engine ([`actcraft_engine::Engine::osc_in`]). It
//! listens on this computer only unless asked to take messages from the network too.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rosc::{OscPacket, OscType};

/// Messages kept until the window takes them; the oldest go first.
const MAX_QUEUED: usize = 256;
/// Bundles nested deeper than this are not opened.
const MAX_DEPTH: usize = 8;
/// Longest address kept.
const MAX_ADDRESS: usize = 256;

/// OSC messages that came in: (address, first value as a number).
pub type OscQueue = Arc<Mutex<VecDeque<(String, Option<f64>)>>>;

/// Listening on one UDP port while it lives.
pub struct OscIn {
    addr: SocketAddr,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OscIn {
    /// Listens on `port`, on this computer only or (`network`) on every network the computer is
    /// on; each message goes into `queue` and `wake` is called.
    pub fn open(port: u16, network: bool, queue: OscQueue, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let host = if network { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        let socket = UdpSocket::bind((host, port)).map_err(|e| e.to_string())?;
        // The thread looks at the quit flag between waits.
        socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
        let addr = socket.local_addr().map_err(|e| e.to_string())?;
        let quit = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&quit);
        let thread = std::thread::Builder::new()
            .name("actcraft-osc-in".into())
            .spawn(move || {
                let mut buf = vec![0u8; rosc::decoder::MTU];
                while !stop.load(Ordering::Relaxed) {
                    let Ok((n, _from)) = socket.recv_from(&mut buf) else { continue };
                    let Some(bytes) = buf.get(..n) else { continue };
                    if let Ok((_, packet)) = rosc::decoder::decode_udp(bytes) {
                        let mut found = Vec::new();
                        messages(&packet, 0, &mut found);
                        if !found.is_empty() {
                            push(&queue, found);
                            wake();
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { addr, quit, thread: Some(thread) })
    }

    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for OscIn {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The messages in a packet, bundles opened (to a bounded depth): address and first value.
fn messages(packet: &OscPacket, depth: usize, out: &mut Vec<(String, Option<f64>)>) {
    match packet {
        OscPacket::Message(m) => {
            if m.addr.len() <= MAX_ADDRESS {
                out.push((m.addr.clone(), m.args.first().and_then(number)));
            }
        }
        OscPacket::Bundle(b) if depth < MAX_DEPTH => {
            for p in &b.content {
                messages(p, depth + 1, out);
            }
        }
        OscPacket::Bundle(_) => {}
    }
}

/// An OSC value as a number, where it is one.
fn number(v: &OscType) -> Option<f64> {
    match v {
        OscType::Float(f) => Some(f64::from(*f)),
        OscType::Double(d) => Some(*d),
        OscType::Int(i) => Some(f64::from(*i)),
        OscType::Long(l) => Some(*l as f64),
        OscType::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn push(queue: &OscQueue, found: Vec<(String, Option<f64>)>) {
    if let Ok(mut q) = queue.lock() {
        for m in found {
            if q.len() >= MAX_QUEUED {
                q.pop_front();
            }
            q.push_back(m);
        }
    }
}

/// Takes every queued message, oldest first.
pub fn drain(queue: &OscQueue) -> Vec<(String, Option<f64>)> {
    queue.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscBundle, OscMessage, OscTime};

    #[test]
    fn a_message_sent_to_the_port_is_queued_with_its_value() {
        let queue: OscQueue = Arc::default();
        let input = OscIn::open(0, false, Arc::clone(&queue), Arc::new(|| {}));
        assert!(input.is_ok(), "{:?}", input.err());
        let Ok(input) = input else { return };
        let msg = OscPacket::Message(OscMessage { addr: "/actcraft/pad/1".into(), args: vec![OscType::Float(1.0)] });
        let inner = OscPacket::Message(OscMessage { addr: "/actcraft/macro/END".into(), args: vec![] });
        let bundle =
            OscPacket::Bundle(OscBundle { timetag: OscTime { seconds: 0, fractional: 1 }, content: vec![inner] });
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).ok();
        for packet in [&msg, &bundle] {
            let bytes = rosc::encoder::encode(packet).unwrap_or_default();
            let sent = sender.as_ref().map(|s| s.send_to(&bytes, input.addr()));
            assert!(sent.is_some_and(|r| r.is_ok()));
        }
        let mut got = Vec::new();
        for _ in 0..50 {
            got.extend(drain(&queue));
            if got.len() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(got, vec![("/actcraft/pad/1".to_owned(), Some(1.0)), ("/actcraft/macro/END".to_owned(), None)]);
        drop(input);
    }

    #[test]
    fn values_become_numbers_and_deep_bundles_stay_shut() {
        assert_eq!(number(&OscType::Int(3)), Some(3.0));
        assert_eq!(number(&OscType::Bool(true)), Some(1.0));
        assert_eq!(number(&OscType::String("x".into())), None);
        let mut packet = OscPacket::Message(OscMessage { addr: "/deep".into(), args: vec![] });
        for _ in 0..(MAX_DEPTH + 2) {
            packet =
                OscPacket::Bundle(OscBundle { timetag: OscTime { seconds: 0, fractional: 1 }, content: vec![packet] });
        }
        let mut out = Vec::new();
        messages(&packet, 0, &mut out);
        assert!(out.is_empty());
    }
}
