//! Philips Hue: finding a bridge on the local network (mDNS, no online service), pairing with it
//! (its link button pressed) and talking to its local API over plain HTTP. The key a pairing
//! gives is kept in this computer's settings by the window, never in a show file.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The mDNS group and port.
const MDNS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);
/// The service Hue bridges announce.
const SERVICE: &[&str] = &["_hue", "_tcp", "local"];
/// The most of an answer read (a bridge's light list is a few kilobytes).
const MAX_BODY: usize = 1 << 20;
/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(3);

/// An mDNS question for the Hue service (PTR, asking for a unicast answer).
pub fn mdns_query() -> Vec<u8> {
    // Header: id 0, flags 0, one question.
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in SERVICE {
        q.push(u8::try_from(label.len()).unwrap_or(0));
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    // Type PTR (12), class IN with the unicast-response bit.
    q.extend_from_slice(&[0, 12, 0x80, 1]);
    q
}

/// Whether an mDNS packet is an answer that names the Hue service.
pub fn names_hue(packet: &[u8]) -> bool {
    // A response (QR bit) with at least one answer, carrying the service's first label.
    let response = packet.get(2).is_some_and(|f| f & 0x80 != 0);
    let answers = packet.get(6..8).is_some_and(|n| n != [0, 0]);
    response && answers && packet.windows(5).any(|w| w == b"\x04_hue")
}

/// Looks for Hue bridges on the local network for `wait`; their addresses.
pub fn discover(wait: Duration) -> Result<Vec<Ipv4Addr>, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
    socket.send_to(&mdns_query(), MDNS).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let mut buf = [0u8; 2048];
    let end = Instant::now() + wait;
    while Instant::now() < end {
        let Ok((n, from)) = socket.recv_from(&mut buf) else { continue };
        if let (IpAddr::V4(ip), Some(packet)) = (from.ip(), buf.get(..n))
            && names_hue(packet)
            && !found.contains(&ip)
        {
            found.push(ip);
        }
    }
    Ok(found)
}

/// One plain HTTP/1.1 request; the body of a 200 answer.
fn request(to: SocketAddr, method: &str, path: &str, body: Option<&Value>) -> Result<String, String> {
    let mut stream = TcpStream::connect_timeout(&to, TIMEOUT).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        to.ip(),
        body.len()
    );
    // In one piece, so the bridge reads the whole request before it answers.
    stream.write_all(format!("{head}{body}").as_bytes()).map_err(|e| e.to_string())?;
    let mut answer = Vec::new();
    stream.take(u64::try_from(MAX_BODY).unwrap_or(u64::MAX)).read_to_end(&mut answer).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&answer);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("the bridge's answer had no body")?;
    let status = head.split_whitespace().nth(1).unwrap_or_default();
    if status != "200" {
        return Err(format!("the bridge answered {status}"));
    }
    Ok(body.to_owned())
}

/// The bridge's error description in an answer, if it is one.
fn error_of(answer: &Value) -> Option<String> {
    let e = answer.get(0)?.get("error")?;
    Some(e.get("description").and_then(Value::as_str).unwrap_or("error").to_owned())
}

/// Pairs with the bridge at `bridge` (port 80, or another for tests); its link button must have
/// been pressed in the last 30 seconds. The key to use from now on.
pub fn pair(bridge: SocketAddr) -> Result<String, String> {
    let body = request(bridge, "POST", "/api", Some(&json!({"devicetype": "actcraft#computer"})))?;
    let answer: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if let Some(e) = error_of(&answer) {
        return Err(e);
    }
    answer
        .get(0)
        .and_then(|a| a.get("success"))
        .and_then(|s| s.get("username"))
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty() && k.len() <= 128 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned)
        .ok_or_else(|| "the bridge gave no key".to_owned())
}

/// A light the bridge knows: its id and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HueLight {
    pub id: String,
    pub name: String,
}

/// The lights of a paired bridge, by id.
pub fn lights(bridge: SocketAddr, key: &str) -> Result<Vec<HueLight>, String> {
    let body = request(bridge, "GET", &format!("/api/{key}/lights"), None)?;
    let answer: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if let Some(e) = error_of(&answer) {
        return Err(e);
    }
    let map = answer.as_object().ok_or("the bridge's light list is not a list")?;
    let mut out: Vec<HueLight> = map
        .iter()
        .take(256)
        .map(|(id, l)| HueLight {
            id: id.chars().take(16).collect(),
            name: l.get("name").and_then(Value::as_str).unwrap_or_default().chars().take(64).collect(),
        })
        .collect();
    out.sort_by(|a, b| a.id.len().cmp(&b.id.len()).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// The bridge on its normal port.
pub fn bridge_at(ip: Ipv4Addr) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(ip), 80)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A bridge on this computer that answers each request with the next of `answers`.
    fn fake_bridge(answers: Vec<&'static str>) -> Option<(SocketAddr, std::thread::JoinHandle<Vec<String>>)> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        let addr = listener.local_addr().ok()?;
        let thread = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in answers {
                let Ok((mut s, _)) = listener.accept() else { break };
                // Everything the client sends, so closing doesn't cut it off.
                let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(buf.get(..n).unwrap_or_default());
                    let text = String::from_utf8_lossy(&got);
                    let want = text
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|n| n.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if text.split_once("\r\n\r\n").is_some_and(|(_, body)| body.len() >= want) {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&got).into_owned());
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{answer}", answer.len());
                let _ = s.write_all(reply.as_bytes());
            }
            seen
        });
        Some((addr, thread))
    }

    #[test]
    fn pairing_waits_for_the_link_button_then_gives_a_key() {
        let Some((addr, bridge)) = fake_bridge(vec![
            r#"[{"error":{"type":101,"address":"","description":"link button not pressed"}}]"#,
            r#"[{"success":{"username":"abcDEF123-xyz"}}]"#,
        ]) else {
            return;
        };
        assert_eq!(pair(addr), Err("link button not pressed".to_owned()));
        assert_eq!(pair(addr), Ok("abcDEF123-xyz".to_owned()));
        let seen = bridge.join().unwrap_or_default();
        assert!(seen.iter().all(|r| r.starts_with("POST /api HTTP/1.1")), "{seen:?}");
        assert!(seen[0].contains(r#""devicetype":"actcraft#computer""#));
    }

    #[test]
    fn a_paired_bridge_lists_its_lights() {
        let Some((addr, bridge)) = fake_bridge(vec![
            r#"{"10":{"name":"Back"},"2":{"name":"Front left"},"1":{"name":"Front right"}}"#,
            r#"[{"error":{"type":1,"description":"unauthorized user"}}]"#,
        ]) else {
            return;
        };
        let got = lights(addr, "key1").unwrap_or_default();
        assert_eq!(
            got.iter().map(|l| (l.id.as_str(), l.name.as_str())).collect::<Vec<_>>(),
            vec![("1", "Front right"), ("2", "Front left"), ("10", "Back")]
        );
        assert_eq!(lights(addr, "nope"), Err("unauthorized user".to_owned()));
        let seen = bridge.join().unwrap_or_default();
        assert!(seen[0].starts_with("GET /api/key1/lights HTTP/1.1"));
    }

    #[test]
    fn the_mdns_question_asks_for_hue_and_answers_are_told_apart() {
        let q = mdns_query();
        assert!(q.windows(5).any(|w| w == b"\x04_hue"));
        assert_eq!(q[q.len() - 4..], [0, 12, 0x80, 1]);
        assert!(!names_hue(&q), "our own question is no answer");
        let mut answer = q.clone();
        answer[2] = 0x84;
        answer[7] = 1;
        assert!(names_hue(&answer));
        assert!(!names_hue(&[0x00, 0x00, 0x84]), "too short");
    }
}
