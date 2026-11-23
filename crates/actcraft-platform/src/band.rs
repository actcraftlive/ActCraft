//! Band link: one ActCraft (the host) lets others on the local network follow it. It is off until
//! switched on; a follower pairs once with a six-digit code the host shows and gets a token it
//! uses from then on. Only addresses on the local network are let in. Messages are JSON lines
//! over TCP: the follower's first line is `{"code": "123456"}` or `{"token": "…"}`, the host
//! answers `{"token": "…"}`, `{"ok": true}` or `{"error": "…"}`, then sends its state as a line
//! a few times a second.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

/// The port a host listens on.
pub const BAND_PORT: u16 = 47390;
/// Time between two state lines.
const PACE: Duration = Duration::from_millis(200);
/// Wrong codes before the code changes.
const MAX_WRONG: usize = 5;
/// Longest line read from the other side.
const MAX_LINE: u64 = 4096;
/// Followers at once.
const MAX_FOLLOWERS: usize = 16;

/// Whether an address is on the local network (or this computer): the only ones let in.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// `n` random bytes as hex, from the system's random source.
pub(crate) fn random_hex(n: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A new six-digit pairing code.
pub(crate) fn new_code() -> Result<String, String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000))
}

/// Reads one line (bounded).
fn read_line(reader: &mut impl BufRead) -> Option<String> {
    let mut line = String::new();
    let n = reader.take(MAX_LINE).read_line(&mut line).ok()?;
    (n > 0).then(|| line.trim().to_owned())
}

fn send(stream: &mut TcpStream, v: &Value) -> std::io::Result<()> {
    stream.write_all(format!("{v}\n").as_bytes())
}

/// What the host keeps between its threads.
#[derive(Default)]
struct Shared {
    code: String,
    wrong: usize,
    tokens: Vec<String>,
    new_tokens: Vec<String>,
    state: Value,
}

/// A host: listening for followers while it lives.
pub struct BandHost {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
    followers: Arc<AtomicUsize>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BandHost {
    /// Listens on `port` (on every network the computer is on; only local addresses are let in),
    /// knowing the tokens of followers paired before.
    pub fn open(port: u16, tokens: Vec<String>) -> Result<Self, String> {
        Self::open_on(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)), tokens)
    }

    fn open_on(at: SocketAddr, tokens: Vec<String>) -> Result<Self, String> {
        let listener = TcpListener::bind(at).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let shared =
            Arc::new(Mutex::new(Shared { code: new_code()?, tokens, state: Value::Null, ..Shared::default() }));
        let followers = Arc::new(AtomicUsize::new(0));
        let quit = Arc::new(AtomicBool::new(false));
        let (s, f, q) = (Arc::clone(&shared), Arc::clone(&followers), Arc::clone(&quit));
        let thread = std::thread::Builder::new()
            .name("actcraft-band-host".into())
            .spawn(move || {
                while !q.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, from)) if is_local(from.ip()) && f.load(Ordering::Relaxed) < MAX_FOLLOWERS => {
                            let (s, f, q) = (Arc::clone(&s), Arc::clone(&f), Arc::clone(&q));
                            let _ = std::thread::Builder::new()
                                .name("actcraft-band-follower".into())
                                .spawn(move || serve(stream, &s, &f, &q));
                        }
                        // Not from the local network, or too many: closed at once.
                        Ok(_) => {}
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { addr, shared, followers, quit, thread: Some(thread) })
    }

    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The code a new follower enters.
    pub fn code(&self) -> String {
        self.shared.lock().map(|s| s.code.clone()).unwrap_or_default()
    }

    /// What followers see from now on.
    pub fn publish(&self, state: Value) {
        if let Ok(mut s) = self.shared.lock() {
            s.state = state;
        }
    }

    /// Tokens of followers paired since the last call (to keep on this computer).
    pub fn take_new_tokens(&self) -> Vec<String> {
        self.shared.lock().map(|mut s| std::mem::take(&mut s.new_tokens)).unwrap_or_default()
    }

    /// How many followers are connected now.
    pub fn followers(&self) -> usize {
        self.followers.load(Ordering::Relaxed)
    }
}

impl Drop for BandHost {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// One follower: its pairing or token, then the state a few times a second until it goes.
fn serve(mut stream: TcpStream, shared: &Mutex<Shared>, followers: &AtomicUsize, quit: &AtomicBool) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let Some(hello) = read_line(&mut reader).and_then(|l| serde_json::from_str::<Value>(&l).ok()) else { return };
    let answer = {
        let Ok(mut s) = shared.lock() else { return };
        if let Some(code) = hello.get("code").and_then(Value::as_str) {
            if !s.code.is_empty() && code == s.code {
                match random_hex(16) {
                    Ok(token) => {
                        s.tokens.push(token.clone());
                        s.new_tokens.push(token.clone());
                        // A code pairs once.
                        s.code = new_code().unwrap_or_default();
                        s.wrong = 0;
                        json!({"token": token})
                    }
                    Err(e) => json!({"error": e}),
                }
            } else {
                s.wrong += 1;
                if s.wrong >= MAX_WRONG {
                    s.code = new_code().unwrap_or_default();
                    s.wrong = 0;
                }
                json!({"error": "wrong code"})
            }
        } else if hello.get("token").and_then(Value::as_str).is_some_and(|t| s.tokens.iter().any(|k| k == t)) {
            json!({"ok": true})
        } else {
            json!({"error": "not paired"})
        }
    };
    if send(&mut stream, &answer).is_err() || answer.get("error").is_some() {
        return;
    }
    followers.fetch_add(1, Ordering::Relaxed);
    while !quit.load(Ordering::Relaxed) {
        let state = shared.lock().map(|s| s.state.clone()).unwrap_or_default();
        if send(&mut stream, &state).is_err() {
            break;
        }
        std::thread::sleep(PACE);
    }
    followers.fetch_sub(1, Ordering::Relaxed);
}

/// How a follower proves it may follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BandAuth {
    /// The six-digit code the host shows (first time).
    Code(String),
    /// The token a pairing gave.
    Token(String),
}

/// A follower: connected to a host while it lives, keeping the newest state it sent.
pub struct BandFollow {
    latest: Arc<Mutex<Option<Value>>>,
    connected: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BandFollow {
    /// Connects to the host at `host` and proves itself; the follower and, after pairing with a
    /// code, the token to keep.
    pub fn connect(host: SocketAddr, auth: &BandAuth) -> Result<(Self, Option<String>), String> {
        if !is_local(host.ip()) {
            return Err("the host must be on the local network".to_owned());
        }
        let mut stream = TcpStream::connect_timeout(&host, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
        let hello = match auth {
            BandAuth::Code(c) => json!({"code": c}),
            BandAuth::Token(t) => json!({"token": t}),
        };
        send(&mut stream, &hello).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let answer: Value =
            read_line(&mut reader).and_then(|l| serde_json::from_str(&l).ok()).ok_or("the host did not answer")?;
        if let Some(e) = answer.get("error").and_then(Value::as_str) {
            return Err(e.to_owned());
        }
        let token = answer
            .get("token")
            .and_then(Value::as_str)
            .filter(|t| t.len() <= 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_owned);
        let latest: Arc<Mutex<Option<Value>>> = Arc::default();
        let connected = Arc::new(AtomicBool::new(true));
        let quit = Arc::new(AtomicBool::new(false));
        let (l, c, q) = (Arc::clone(&latest), Arc::clone(&connected), Arc::clone(&quit));
        let thread = std::thread::Builder::new()
            .name("actcraft-band-follow".into())
            .spawn(move || {
                // A host that says nothing for this long is taken as gone.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                while !q.load(Ordering::Relaxed) {
                    let Some(line) = read_line(&mut reader) else { break };
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && let Ok(mut slot) = l.lock()
                    {
                        *slot = Some(v);
                    }
                }
                c.store(false, Ordering::Relaxed);
            })
            .map_err(|e| e.to_string())?;
        Ok((Self { latest, connected, quit, thread: Some(thread) }, token))
    }

    /// The newest state the host sent, once.
    pub fn take_state(&self) -> Option<Value> {
        self.latest.lock().ok().and_then(|mut s| s.take())
    }

    /// Whether the host is still there.
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

impl Drop for BandFollow {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Option<BandHost> {
        BandHost::open_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Vec::new()).ok()
    }

    #[test]
    fn a_follower_pairs_with_the_code_then_comes_back_with_its_token() {
        let Some(h) = host() else { return };
        let code = h.code();
        assert_eq!(code.len(), 6);
        assert_eq!(BandFollow::connect(h.addr(), &BandAuth::Code("000000x".into())).err(), Some("wrong code".into()));
        let paired = BandFollow::connect(h.addr(), &BandAuth::Code(code.clone()));
        let Ok((follower, Some(token))) = paired else { panic!("paired: {:?}", paired.err()) };
        assert_eq!(h.take_new_tokens(), vec![token.clone()], "the host keeps it");
        assert_ne!(h.code(), code, "a code pairs once");
        h.publish(json!({"song": "Opener", "time": 12.5, "playing": true}));
        let mut got = None;
        for _ in 0..30 {
            got = follower.take_state();
            if got.as_ref().is_some_and(|v| v["song"] == "Opener") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(got.map(|v| v["time"].clone()), Some(json!(12.5)));
        assert_eq!(h.followers(), 1);
        drop(follower);
        let again = BandFollow::connect(h.addr(), &BandAuth::Token(token));
        assert!(again.as_ref().is_ok_and(|(_, t)| t.is_none()), "{:?}", again.err());
        assert_eq!(BandFollow::connect(h.addr(), &BandAuth::Token("feed".into())).err(), Some("not paired".into()));
    }

    #[test]
    fn too_many_wrong_codes_change_the_code() {
        let Some(h) = host() else { return };
        let code = h.code();
        for _ in 0..MAX_WRONG {
            let _ = BandFollow::connect(h.addr(), &BandAuth::Code("wrong".into()));
        }
        assert_ne!(h.code(), code, "guessing does not get far");
    }

    #[test]
    fn only_the_local_network_is_let_in() {
        for ip in ["127.0.0.1", "10.1.2.3", "172.16.0.9", "192.168.1.20", "169.254.3.4", "::1", "fd00::1", "fe80::1"] {
            assert!(ip.parse().is_ok_and(is_local), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "2001:db8::1"] {
            assert!(ip.parse().is_ok_and(|a| !is_local(a)), "{ip}");
        }
        let far = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), BAND_PORT));
        assert!(BandFollow::connect(far, &BandAuth::Code("123456".into())).is_err());
    }
}
