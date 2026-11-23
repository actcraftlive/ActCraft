//! DMX in: listens for Art-Net (ArtDmx packets on UDP port 6454) and keeps the newest levels of
//! each universe it hears, by Art-Net port address, for the window to hand to the engine. It
//! listens on this computer only unless asked to take DMX from the network too.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use artnet_protocol::ArtCommand;

/// The Art-Net UDP port.
pub const ARTNET_PORT: u16 = 6454;
/// Universes kept at once; more are dropped until the window takes them.
const MAX_UNIVERSES: usize = 256;
/// Levels in a universe.
const CHANNELS: usize = 512;

/// The newest levels heard, by Art-Net port address, until the window takes them.
pub type DmxInLevels = Arc<Mutex<HashMap<u16, Vec<u8>>>>;

/// Listening for Art-Net while it lives.
pub struct DmxIn {
    addr: SocketAddr,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DmxIn {
    /// Listens on the Art-Net port, on this computer only or (`network`) on every network the
    /// computer is on; each universe heard goes into `levels` and `wake` is called.
    pub fn open(network: bool, levels: DmxInLevels, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let host = if network { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        Self::open_on(SocketAddr::from((host, ARTNET_PORT)), levels, wake)
    }

    fn open_on(at: SocketAddr, levels: DmxInLevels, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let socket = UdpSocket::bind(at).map_err(|e| e.to_string())?;
        // The thread looks at the quit flag between waits.
        socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
        let addr = socket.local_addr().map_err(|e| e.to_string())?;
        let quit = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&quit);
        let thread = std::thread::Builder::new()
            .name("actcraft-dmx-in".into())
            .spawn(move || {
                let mut buf = vec![0u8; 1024];
                while !stop.load(Ordering::Relaxed) {
                    let Ok((n, _from)) = socket.recv_from(&mut buf) else { continue };
                    let Some(bytes) = buf.get(..n) else { continue };
                    if let Some((universe, data)) = art_dmx(bytes)
                        && keep(&levels, universe, data)
                    {
                        wake();
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

impl Drop for DmxIn {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// An ArtDmx packet's port address and levels (at most 512); `None` for anything else.
pub fn art_dmx(bytes: &[u8]) -> Option<(u16, Vec<u8>)> {
    match ArtCommand::from_buffer(bytes).ok()? {
        ArtCommand::Output(o) => {
            let mut data = o.data.as_ref().clone();
            data.truncate(CHANNELS);
            Some((u16::from(o.port_address), data))
        }
        _ => None,
    }
}

/// Keeps the newest levels of a universe; false when it was dropped (too many universes).
fn keep(levels: &DmxInLevels, universe: u16, data: Vec<u8>) -> bool {
    let Ok(mut map) = levels.lock() else { return false };
    if map.len() >= MAX_UNIVERSES && !map.contains_key(&universe) {
        return false;
    }
    map.insert(universe, data);
    true
}

/// Takes the levels heard since the last time, by port address.
pub fn take(levels: &DmxInLevels) -> Vec<(u16, Vec<u8>)> {
    levels.lock().map(|mut map| map.drain().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_dmx_packets_from_the_network_land_by_universe() {
        let levels = DmxInLevels::default();
        let Ok(input) =
            DmxIn::open_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Arc::clone(&levels), Arc::new(|| {}))
        else {
            return;
        };
        let Ok(sender) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) else { return };
        let mut l = [0u8; CHANNELS];
        l[0] = 200;
        l[511] = 7;
        let packet = crate::dmx_out::artnet_packet(3, 1, &l).unwrap_or_default();
        let _ = sender.send_to(&packet, input.addr());
        let _ = sender.send_to(b"not art-net", input.addr());
        let mut got = Vec::new();
        for _ in 0..50 {
            got = take(&levels);
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(got.len(), 1, "one universe, the junk ignored");
        let (universe, data) = &got[0];
        assert_eq!((*universe, data.len(), data[0], data[511]), (3, CHANNELS, 200, 7));
        assert!(take(&levels).is_empty(), "taken");
    }
}
