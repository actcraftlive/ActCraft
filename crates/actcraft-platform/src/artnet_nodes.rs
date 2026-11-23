//! Finding Art-Net nodes: sends an ArtPoll to everyone on the network and collects the
//! ArtPollReply answers for a moment. Nodes answer to port 6454, so it listens there while it
//! looks (and says so when another program holds that port).

use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

use artnet_protocol::{ArtCommand, Poll, PollReply};

use crate::dmx_out::ARTNET_PORT;

/// Most nodes kept from one search.
const MAX_NODES: usize = 256;

/// An Art-Net node that answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtNode {
    pub address: Ipv4Addr,
    /// Its short name, or the long one when the short is empty.
    pub name: String,
    /// The port addresses (universes) its DMX outputs take, in port order.
    pub outputs: Vec<u16>,
}

/// Asks the network for Art-Net nodes and gathers the answers for `wait`.
pub fn discover(wait: Duration) -> Result<Vec<ArtNode>, String> {
    discover_on(ARTNET_PORT, Ipv4Addr::BROADCAST, wait)
}

fn discover_on(port: u16, to: Ipv4Addr, wait: Duration) -> Result<Vec<ArtNode>, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).map_err(|e| format!("port {port}: {e}"))?;
    search(&socket, to, port, wait)
}

/// Sends the ArtPoll from `socket` to `to`:`port` and gathers the answers that reach it.
fn search(socket: &UdpSocket, to: Ipv4Addr, port: u16, wait: Duration) -> Result<Vec<ArtNode>, String> {
    socket.set_broadcast(true).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_millis(50))).map_err(|e| e.to_string())?;
    let poll = ArtCommand::Poll(Poll::default()).write_to_buffer().map_err(|e| e.to_string())?;
    socket.send_to(&poll, (to, port)).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + wait;
    let mut nodes: Vec<ArtNode> = Vec::new();
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline && nodes.len() < MAX_NODES {
        let Ok((n, _from)) = socket.recv_from(&mut buf) else { continue };
        let Some(bytes) = buf.get(..n) else { continue };
        if let Ok(ArtCommand::PollReply(reply)) = ArtCommand::from_buffer(bytes) {
            let node = node_of(&reply);
            // A node with several ports may answer once per port group (bind index).
            match nodes.iter_mut().find(|o| o.address == node.address) {
                Some(known) => {
                    for u in node.outputs {
                        if !known.outputs.contains(&u) {
                            known.outputs.push(u);
                        }
                    }
                }
                None => nodes.push(node),
            }
        }
    }
    Ok(nodes)
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(b.get(..end).unwrap_or_default()).trim().to_owned()
}

fn node_of(r: &PollReply) -> ArtNode {
    let short = text(&r.short_name);
    let name = if short.is_empty() { text(&r.long_name) } else { short };
    let ports = usize::from(r.num_ports[1]).min(4);
    // Port address: Net (7 bits), Sub-Net (4 bits), the port's universe (4 bits).
    let base = (u16::from(r.port_address[0] & 0x7F) << 8) | (u16::from(r.port_address[1] & 0x0F) << 4);
    let outputs = (0..ports)
        .filter(|i| r.port_types.get(*i).is_some_and(|t| t & 0x80 != 0))
        .filter_map(|i| r.swout.get(i).map(|s| base | u16::from(s & 0x0F)))
        .collect();
    ArtNode { address: r.address, name, outputs }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(address: Ipv4Addr, name: &str, swout: [u8; 4]) -> Vec<u8> {
        let mut short_name = [0u8; 18];
        for (d, s) in short_name.iter_mut().zip(name.bytes()) {
            *d = s;
        }
        let r = PollReply {
            address,
            port_address: [1, 2],
            short_name,
            num_ports: [0, 2],
            port_types: [0x80, 0x80, 0, 0],
            swout,
            ..PollReply::default()
        };
        ArtCommand::PollReply(Box::new(r)).write_to_buffer().unwrap_or_default()
    }

    #[test]
    fn answers_become_nodes_with_their_universes() {
        // The search keeps its own socket on a free port, and a node answers to it on 127.0.0.1.
        // (Taking a free port and binding it again later raced other tests for that port.)
        let Ok(socket) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) else { return };
        let port = socket.local_addr().map(|a| a.port()).unwrap_or(0);
        let node = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            if let Ok(s) = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)) {
                for (n, swout) in [("Stage left", [3, 4, 0, 0]), ("", [5, 4, 0, 0])] {
                    let _ = s.send_to(&reply(Ipv4Addr::new(10, 0, 0, 7), n, swout), (Ipv4Addr::LOCALHOST, port));
                }
            }
        });
        let found = search(&socket, Ipv4Addr::LOCALHOST, port, Duration::from_millis(1000));
        let _ = node.join();
        assert_eq!(
            found,
            Ok(vec![ArtNode {
                address: Ipv4Addr::new(10, 0, 0, 7),
                name: "Stage left".into(),
                outputs: vec![0x123, 0x124, 0x125]
            }])
        );
    }
}
