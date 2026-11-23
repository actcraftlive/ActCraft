//! DMX out: sends the show's DMX levels over the network, Art-Net or sACN (ANSI E1.31), or to a
//! USB interface speaking the DMX USB Pro serial protocol; one universe per output. A thread
//! sends a universe when its levels change and again every
//! [`REFRESH`] so receivers keep their levels; the window hands it the outputs and levels.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use actcraft_engine::dmx::{DmxOutput, DmxProtocol};
use artnet_protocol::{ArtCommand, Output, PortAddress};

/// Channels in a universe.
pub const CHANNELS: usize = 512;
/// The Art-Net UDP port.
pub const ARTNET_PORT: u16 = 6454;
/// The sACN UDP port.
pub const SACN_PORT: u16 = 5568;
/// A universe is sent again this often when nothing changes (Art-Net and sACN both ask for a
/// refresh within a second or so).
pub const REFRESH: Duration = Duration::from_millis(800);
/// How often the thread looks for changes (about 40 frames a second, DMX's own pace).
const TICK: Duration = Duration::from_millis(25);
/// The name receivers see for sACN.
const SOURCE_NAME: &str = "ActCraft";

/// What to send: the outputs and the levels of the universes they send.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DmxFrame {
    pub outputs: Vec<DmxOutput>,
    /// Levels by show universe (from 1).
    pub levels: HashMap<u16, [u8; CHANNELS]>,
    /// Changes with the levels, so unchanged universes wait for the refresh.
    pub version: u64,
}

/// Sends DMX while it lives.
pub struct DmxOut {
    frame: Arc<Mutex<DmxFrame>>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl DmxOut {
    /// Starts sending (nothing until [`Self::set`] gives it outputs).
    pub fn open() -> Result<Self, String> {
        Self::start(ARTNET_PORT, SACN_PORT)
    }

    fn start(artnet_port: u16, sacn_port: u16) -> Result<Self, String> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
        socket.set_broadcast(true).map_err(|e| e.to_string())?;
        let frame = Arc::new(Mutex::new(DmxFrame::default()));
        let quit = Arc::new(AtomicBool::new(false));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let (shared, stop, errs) = (Arc::clone(&frame), Arc::clone(&quit), Arc::clone(&errors));
        let thread = std::thread::Builder::new()
            .name("actcraft-dmx-out".into())
            .spawn(move || {
                let mut sender = Sender {
                    socket,
                    sacn: None,
                    artnet_port,
                    sacn_port,
                    sent: HashMap::new(),
                    seq: 1,
                    ports: HashMap::new(),
                };
                while !stop.load(Ordering::Relaxed) {
                    let frame = shared.lock().map(|f| f.clone()).unwrap_or_default();
                    for e in sender.step(&frame, Instant::now()) {
                        if let Ok(mut list) = errs.lock()
                            && !list.contains(&e)
                            && list.len() < 16
                        {
                            list.push(e);
                        }
                    }
                    std::thread::sleep(TICK);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { frame, quit, thread: Some(thread), errors })
    }

    /// What to send from now on.
    pub fn set(&self, frame: DmxFrame) {
        if let Ok(mut f) = self.frame.lock() {
            *f = frame;
        }
    }

    /// Problems sending since the last look (each once).
    pub fn take_errors(&self) -> Vec<String> {
        self.errors.lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default()
    }
}

impl Drop for DmxOut {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Sender {
    socket: UdpSocket,
    sacn: Option<sacn::source::SacnSource>,
    artnet_port: u16,
    sacn_port: u16,
    /// Per output (its settings): the version last sent and when.
    sent: HashMap<OutputKey, (u64, Instant)>,
    /// Art-Net sequence, 1-255 (0 would switch sequencing off).
    seq: u8,
    /// USB interfaces open now, by port name.
    ports: HashMap<String, crate::serial::SerialPort>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OutputKey {
    universe: u16,
    protocol: DmxProtocol,
    network_universe: u16,
    address: String,
    priority: u8,
}

impl From<&DmxOutput> for OutputKey {
    fn from(o: &DmxOutput) -> Self {
        Self {
            universe: o.universe,
            protocol: o.protocol,
            network_universe: o.network_universe,
            address: o.address.clone(),
            priority: o.priority,
        }
    }
}

impl Sender {
    /// Sends what is due; problems as text.
    fn step(&mut self, frame: &DmxFrame, now: Instant) -> Vec<String> {
        let mut errors = Vec::new();
        let keys: Vec<OutputKey> = frame.outputs.iter().map(OutputKey::from).collect();
        self.sent.retain(|k, _| keys.contains(k));
        // A port no output uses any more is let go, so another program can open it.
        self.ports.retain(|name, _| frame.outputs.iter().any(|o| o.protocol.is_usb() && o.address.trim() == name));
        let any_artnet = frame.outputs.iter().any(|o| o.protocol == DmxProtocol::ArtNet);
        for (output, key) in frame.outputs.iter().zip(keys) {
            let due =
                // A plain USB cable needs its frames all the time; the others when the levels
                // change and on the refresh.
                output.protocol == DmxProtocol::UsbCable || self.sent.get(&key).is_none_or(|(v, at)| *v != frame.version || now.duration_since(*at) >= REFRESH);
            if !due {
                continue;
            }
            let levels = frame.levels.get(&output.universe).copied().unwrap_or([0; CHANNELS]);
            let result = match output.protocol {
                DmxProtocol::ArtNet => self.artnet(output, &levels),
                DmxProtocol::Sacn => self.sacn(output, &levels),
                DmxProtocol::UsbPro => self.usb_pro(output, &levels),
                DmxProtocol::UsbCable => self.usb_cable(output, &levels),
            };
            if let Err(e) = result {
                errors.push(e);
            }
            self.sent.insert(key, (frame.version, now));
        }
        if any_artnet {
            self.seq = if self.seq == 255 { 1 } else { self.seq + 1 };
        }
        errors
    }

    fn artnet(&self, o: &DmxOutput, levels: &[u8; CHANNELS]) -> Result<(), String> {
        let packet = artnet_packet(o.network_universe, self.seq, levels)?;
        let ip = o.target().unwrap_or(Ipv4Addr::BROADCAST);
        self.socket.send_to(&packet, (ip, self.artnet_port)).map(|_| ()).map_err(|e| format!("Art-Net to {ip}: {e}"))
    }

    fn sacn(&mut self, o: &DmxOutput, levels: &[u8; CHANNELS]) -> Result<(), String> {
        if self.sacn.is_none() {
            // A free port of its own, so another sACN program on this computer keeps 5568.
            let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
            self.sacn = Some(sacn::source::SacnSource::with_ip(SOURCE_NAME, bind).map_err(|e| format!("sACN: {e}"))?);
        }
        let Some(src) = self.sacn.as_mut() else { return Ok(()) };
        let universe = o.network_universe;
        // Registering a universe again is harmless; it must be known before sending.
        let _ = src.register_universe(universe);
        let mut data = Vec::with_capacity(CHANNELS + 1);
        data.push(0); // the DMX start code
        data.extend_from_slice(levels);
        let to = o.target().map(|ip| SocketAddr::new(IpAddr::V4(ip), self.sacn_port));
        src.send(&[universe], &data, Some(o.priority), to, None).map_err(|e| format!("sACN universe {universe}: {e}"))
    }
}

impl Sender {
    /// Sends a universe to the USB interface on the output's port, opening the port the first
    /// time; a port that fails is closed and opened again on the next send.
    fn usb_pro(&mut self, o: &DmxOutput, levels: &[u8; CHANNELS]) -> Result<(), String> {
        let name = o.address.trim();
        if name.is_empty() {
            return Err("USB DMX: no port chosen".to_owned());
        }
        if !self.ports.contains_key(name) {
            let port = crate::serial::SerialPort::open(name, crate::serial::Line::Interface)
                .map_err(|e| format!("USB DMX on {name}: {e}"))?;
            self.ports.insert(name.to_owned(), port);
        }
        let Some(port) = self.ports.get_mut(name) else { return Ok(()) };
        if let Err(e) = port.write_all(&usb_pro_packet(levels)) {
            self.ports.remove(name);
            return Err(format!("USB DMX on {name}: {e}"));
        }
        Ok(())
    }
}

impl Sender {
    /// Sends one DMX frame down a plain USB cable on the output's port (the computer times the
    /// signal), opening the port the first time; a port that fails is opened again next time.
    fn usb_cable(&mut self, o: &DmxOutput, levels: &[u8; CHANNELS]) -> Result<(), String> {
        let name = o.address.trim();
        if name.is_empty() {
            return Err("USB DMX: no port chosen".to_owned());
        }
        if !self.ports.contains_key(name) {
            let port = crate::serial::SerialPort::open(name, crate::serial::Line::Cable)
                .map_err(|e| format!("USB DMX on {name}: {e}"))?;
            self.ports.insert(name.to_owned(), port);
        }
        let Some(port) = self.ports.get_mut(name) else { return Ok(()) };
        if let Err(e) = port.send_frame(levels) {
            self.ports.remove(name);
            return Err(format!("USB DMX on {name}: {e}"));
        }
        Ok(())
    }
}

/// A DMX USB Pro "output only send DMX packet" message (label 6): the DMX start code 0, then the
/// 512 levels, framed by the start and end delimiters and the data length (low byte first).
pub fn usb_pro_packet(levels: &[u8; CHANNELS]) -> Vec<u8> {
    const START: u8 = 0x7E;
    const SEND_DMX: u8 = 6;
    const END: u8 = 0xE7;
    let [lo, hi] = u16::try_from(CHANNELS + 1).unwrap_or(513).to_le_bytes();
    let mut packet = Vec::with_capacity(CHANNELS + 6);
    packet.extend_from_slice(&[START, SEND_DMX, lo, hi, 0]);
    packet.extend_from_slice(levels);
    packet.push(END);
    packet
}

/// An ArtDmx packet for port address `port_address` with sequence `seq`.
pub fn artnet_packet(port_address: u16, seq: u8, levels: &[u8; CHANNELS]) -> Result<Vec<u8>, String> {
    let port_address = PortAddress::try_from(port_address).map_err(|e| e.to_string())?;
    let output = Output { sequence: seq, port_address, data: levels.to_vec().into(), ..Output::default() };
    ArtCommand::Output(output).write_to_buffer().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels() -> [u8; CHANNELS] {
        let mut l = [0; CHANNELS];
        l[0] = 255;
        l[511] = 7;
        l
    }

    #[test]
    fn a_usb_pro_message_frames_the_start_code_and_every_level() {
        let l = levels();
        let packet = usb_pro_packet(&l);
        assert_eq!(packet.len(), 5 + CHANNELS + 1);
        // Start, label 6 (send DMX), length 513 low byte first, then the start code 0.
        assert_eq!(packet[..5], [0x7E, 6, 0x01, 0x02, 0]);
        assert_eq!(packet[5..5 + CHANNELS], l[..]);
        assert_eq!(packet.last(), Some(&0xE7));
    }

    #[test]
    fn a_usb_output_without_its_interface_reports_and_keeps_going() {
        let Ok(socket) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) else { return };
        let mut sender = Sender {
            socket,
            sacn: None,
            artnet_port: 9,
            sacn_port: 9,
            sent: HashMap::new(),
            seq: 1,
            ports: HashMap::new(),
        };
        let output = DmxOutput {
            protocol: DmxProtocol::UsbPro,
            address: "ActCraftNoSuchPort9".to_owned(),
            ..DmxOutput::default()
        };
        let frame = DmxFrame { outputs: vec![output], levels: HashMap::new(), version: 1 };
        let errors = sender.step(&frame, Instant::now());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("USB DMX on ActCraftNoSuchPort9"), "{errors:?}");
        assert!(sender.ports.is_empty());
    }

    #[test]
    fn an_artnet_packet_carries_the_port_address_and_every_level() {
        let p = artnet_packet(0x0123, 9, &levels()).unwrap_or_default();
        assert_eq!(p.get(..8), Some(&b"Art-Net\0"[..]));
        assert_eq!(p.get(8..10), Some(&[0x00, 0x50][..]), "OpDmx, little endian");
        assert_eq!(p.get(12), Some(&9), "sequence");
        assert_eq!(p.get(14..16), Some(&[0x23, 0x01][..]), "SubUni then Net");
        assert_eq!(p.get(16..18), Some(&[0x02, 0x00][..]), "512 channels, big endian");
        assert_eq!(p.get(18..), Some(&levels()[..]));
        assert!(artnet_packet(40_000, 1, &levels()).is_err());
    }

    fn receiver() -> Option<UdpSocket> {
        let r = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        r.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        Some(r)
    }

    #[test]
    fn outputs_send_their_universe_now_and_again_later() {
        let (artnet, sacn_rx) = (receiver(), receiver());
        assert!(artnet.is_some() && sacn_rx.is_some());
        let (Some(artnet), Some(sacn_rx)) = (artnet, sacn_rx) else { return };
        let (ap, sp) =
            (artnet.local_addr().map(|a| a.port()).unwrap_or(0), sacn_rx.local_addr().map(|a| a.port()).unwrap_or(0));
        let out = DmxOut::start(ap, sp);
        assert!(out.is_ok(), "{:?}", out.as_ref().err());
        let Ok(out) = out else { return };
        let mut levels_by_universe = HashMap::new();
        levels_by_universe.insert(3, levels());
        out.set(DmxFrame {
            outputs: vec![
                DmxOutput { universe: 3, network_universe: 2, address: "127.0.0.1".into(), ..DmxOutput::default() },
                DmxOutput {
                    universe: 3,
                    protocol: DmxProtocol::Sacn,
                    network_universe: 7,
                    address: "127.0.0.1".into(),
                    priority: 150,
                    ..DmxOutput::default()
                },
            ],
            levels: levels_by_universe,
            version: 1,
        });
        let mut buf = [0u8; 1024];
        let n = artnet.recv(&mut buf).unwrap_or(0);
        assert_eq!(buf.get(14..16), Some(&[2, 0][..]), "port address 2");
        assert_eq!(buf.get(18..n), Some(&levels()[..]));
        // E1.31: priority at 108, universe at 113, start code at 125, levels after it.
        let n = loop {
            let n = sacn_rx.recv(&mut buf).unwrap_or(0);
            if n == 0 || n >= 126 + CHANNELS {
                break n;
            }
        };
        assert_eq!(buf.get(4..13), Some(&b"ASC-E1.17"[..]));
        assert_eq!(buf.get(108), Some(&150));
        assert_eq!(buf.get(113..115), Some(&[0, 7][..]));
        assert_eq!(buf.get(125), Some(&0));
        assert_eq!(buf.get(126..n), Some(&levels()[..]));
        // Unchanged levels go out again within the refresh time.
        let again = artnet.recv(&mut buf).unwrap_or(0);
        assert!(again > 18, "sent again");
        assert!(out.take_errors().is_empty());
    }
}
