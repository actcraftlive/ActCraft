//! DMX in: listens for Art-Net (ArtDmx packets on UDP port 6454) and sACN (E1.31 on UDP port
//! 5568) and keeps the newest levels of each universe it hears, by Art-Net port address (sACN
//! universe 1 is port address 0), for the window to hand to the engine. It listens on this
//! computer only unless asked to take DMX from the network too (sACN then also joins the
//! multicast groups of the first universes).

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use artnet_protocol::ArtCommand;

/// The Art-Net UDP port.
pub const ARTNET_PORT: u16 = 6454;
/// The sACN UDP port.
pub const SACN_PORT: u16 = 5568;
/// sACN universes listened to on the network (their multicast groups are joined): as many as
/// the engine keeps.
const SACN_UNIVERSES: u16 = 64;
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
    /// The sACN listener, unless its port could not be had (another program holds it).
    sacn: Option<JoinHandle<()>>,
}

impl DmxIn {
    /// Listens on the Art-Net port, on this computer only or (`network`) on every network the
    /// computer is on; each universe heard goes into `levels` and `wake` is called.
    pub fn open(network: bool, levels: DmxInLevels, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let host = if network { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        let sacn = SocketAddr::from((host, SACN_PORT));
        Self::open_on(SocketAddr::from((host, ARTNET_PORT)), Some((sacn, network)), levels, wake)
    }

    fn open_on(
        at: SocketAddr,
        sacn_at: Option<(SocketAddr, bool)>,
        levels: DmxInLevels,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let socket = UdpSocket::bind(at).map_err(|e| e.to_string())?;
        // The thread looks at the quit flag between waits.
        socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
        let addr = socket.local_addr().map_err(|e| e.to_string())?;
        let quit = Arc::new(AtomicBool::new(false));
        let sacn = sacn_at.and_then(|(addr, network)| {
            listen_sacn(addr, network, Arc::clone(&levels), Arc::clone(&wake), Arc::clone(&quit)).ok()
        });
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
        Ok(Self { addr, quit, thread: Some(thread), sacn })
    }

    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Whether sACN is heard too (its port was free).
    pub fn hears_sacn(&self) -> bool {
        self.sacn.is_some()
    }
}

impl Drop for DmxIn {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.sacn.take() {
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

/// Listens for sACN at `at` on a thread of its own until `quit`; on the network (`network`) it also
/// joins the multicast groups of the first universes (239.255.0.1 and on). Only levels (start code
/// 0) are kept; preview data is left out.
fn listen_sacn(
    at: SocketAddr,
    network: bool,
    levels: DmxInLevels,
    wake: Arc<dyn Fn() + Send + Sync>,
    quit: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let socket = UdpSocket::bind(at).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
    if network {
        for u in 1..=SACN_UNIVERSES {
            let [hi, lo] = u.to_be_bytes();
            // A group that can't be joined (no network yet) leaves unicast working.
            let _ = socket.join_multicast_v4(&Ipv4Addr::new(239, 255, hi, lo), &Ipv4Addr::UNSPECIFIED);
        }
    }
    std::thread::Builder::new()
        .name("actcraft-sacn-in".into())
        .spawn(move || {
            let mut buf = vec![0u8; 1144];
            while !quit.load(Ordering::Relaxed) {
                let Ok((n, _from)) = socket.recv_from(&mut buf) else { continue };
                if let Some((universe, data)) = buf.get(..n).and_then(sacn_dmx)
                    && keep(&levels, universe.saturating_sub(1), data)
                {
                    wake();
                }
            }
        })
        .map_err(|e| e.to_string())
}

/// An E1.31 data packet's universe and levels (at most 512); `None` for anything else (another
/// protocol, synchronisation or discovery, preview data, another start code, a short packet).
pub fn sacn_dmx(bytes: &[u8]) -> Option<(u16, Vec<u8>)> {
    const ACN_ID: &[u8] = b"ASC-E1.17\0\0\0";
    let at = |range: std::ops::Range<usize>| bytes.get(range);
    if at(4..16)? != ACN_ID || at(18..22)? != [0, 0, 0, 4] || at(40..44)? != [0, 0, 0, 2] || *bytes.get(117)? != 2 {
        return None;
    }
    // Options: preview data is for visualisers, not for the stage.
    if bytes.get(112)? & 0x80 != 0 {
        return None;
    }
    let universe = u16::from_be_bytes([*bytes.get(113)?, *bytes.get(114)?]);
    let count = usize::from(u16::from_be_bytes([*bytes.get(123)?, *bytes.get(124)?]));
    if universe == 0 || count == 0 || *bytes.get(125)? != 0 {
        return None;
    }
    let end = (125 + count).min(bytes.len()).min(126 + CHANNELS);
    Some((universe, at(126..end)?.to_vec()))
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
            DmxIn::open_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), None, Arc::clone(&levels), Arc::new(|| {}))
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

    #[test]
    fn only_sacn_level_data_is_read() {
        // A well-formed packet: universe 7, start code 0, three levels.
        let mut p = vec![0u8; 129];
        p[4..16].copy_from_slice(b"ASC-E1.17\0\0\0");
        p[18..22].copy_from_slice(&[0, 0, 0, 4]);
        p[40..44].copy_from_slice(&[0, 0, 0, 2]);
        p[113..115].copy_from_slice(&7u16.to_be_bytes());
        p[117] = 2;
        p[123..125].copy_from_slice(&4u16.to_be_bytes());
        p[126..129].copy_from_slice(&[10, 20, 30]);
        assert_eq!(sacn_dmx(&p), Some((7, vec![10, 20, 30])));
        let changed = |at: usize, v: u8| {
            let mut q = p.clone();
            q[at] = v;
            sacn_dmx(&q)
        };
        assert_eq!(changed(112, 0x80), None, "preview data");
        assert_eq!(changed(125, 0xdd), None, "another start code");
        assert_eq!(changed(43, 8), None, "synchronisation or discovery");
        assert_eq!(changed(4, b'X'), None, "not E1.31");
        assert_eq!(sacn_dmx(&p[..100]), None, "short");
        // A count beyond the packet reads what is there.
        let mut long = p.clone();
        long[123..125].copy_from_slice(&600u16.to_be_bytes());
        assert_eq!(sacn_dmx(&long), Some((7, vec![10, 20, 30])));
    }

    #[test]
    fn sacn_packets_land_by_universe_one_below() {
        let levels = DmxInLevels::default();
        // A free port for the sACN listener (given back at once).
        let port =
            UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).ok().and_then(|s| s.local_addr().ok()).map_or(0, |a| a.port());
        let sacn_at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let artnet_at = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let input = DmxIn::open_on(artnet_at, Some((sacn_at, false)), Arc::clone(&levels), Arc::new(|| {}));
        let Ok(input) = input else { return };
        assert!(input.hears_sacn());
        let Ok(mut source) = sacn::source::SacnSource::with_ip("test", SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        else {
            return;
        };
        let _ = source.register_universe(2);
        // The start code (0: levels) first, as the sender takes it.
        let mut l = vec![0u8; CHANNELS + 1];
        l[1] = 99;
        l[512] = 5;
        let mut got = Vec::new();
        for _ in 0..50 {
            let _ = source.send(&[2], &l, Some(100), Some(sacn_at), None);
            got = take(&levels);
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(got.len(), 1);
        let (universe, data) = &got[0];
        assert_eq!((*universe, data.len(), data[0], data[511]), (1, CHANNELS, 99, 5), "universe 2 is port address 1");
    }
}
