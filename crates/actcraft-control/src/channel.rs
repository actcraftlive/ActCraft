//! The JSON control channel: newline-delimited JSON-RPC 2.0 on a loopback TCP port.
//!
//! Only loopback addresses are accepted, so other computers can't reach it. An optional token
//! (sent as `params.token` in every request) keeps other programs on the same computer out.

use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use actcraft_engine::Engine;

/// Default port of the control channel.
pub const DEFAULT_PORT: u16 = 47_380;
/// Connections served at the same time.
pub const MAX_CLIENTS: usize = 16;

/// Starts serving on `addr` (must be a loopback address) in background threads. The engine is
/// shared with the window through the mutex.
pub fn serve(addr: SocketAddr, engine: Arc<Mutex<Engine>>, token: Option<String>) -> Result<SocketAddr, String> {
    if !addr.ip().is_loopback() {
        return Err(format!("{addr} is not a loopback address; the control channel only listens on this computer"));
    }
    let listener = TcpListener::bind(addr).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    let clients = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    thread::Builder::new()
        .name("actcraft-control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if clients.load(std::sync::atomic::Ordering::Relaxed) >= MAX_CLIENTS {
                    continue;
                }
                let engine = Arc::clone(&engine);
                let token = token.clone();
                let counter = Arc::clone(&clients);
                clients.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let spawned = thread::Builder::new().name("actcraft-control-client".into()).spawn(move || {
                    serve_client(stream, &engine, token.as_deref());
                    counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                });
                if spawned.is_err() {
                    // The thread never ran, so it can't lower the count itself.
                    clients.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        })
        .map_err(|e| format!("cannot start control channel: {e}"))?;
    Ok(local)
}

fn serve_client(stream: TcpStream, engine: &Mutex<Engine>, token: Option<&str>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(600)));
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    loop {
        let line = match craft_jsonrpc::read_line(&mut reader, crate::MAX_LINE_BYTES) {
            Ok(Some(line)) => line,
            // End of stream, an over-long line or a read error: close the connection.
            Ok(None) | Err(_) => return,
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = match engine.lock() {
            Ok(mut e) => crate::handle_line(&mut e, &line, token),
            Err(poisoned) => crate::handle_line(&mut poisoned.into_inner(), &line, token),
        };
        if let Some(resp) = response {
            let mut text = resp.to_string();
            text.push('\n');
            if writer.write_all(text.as_bytes()).is_err() {
                return;
            }
        }
    }
}

/// Sends one request to a running control channel and returns the response.
pub fn request(addr: SocketAddr, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|e| format!("cannot reach ActCraft at {addr}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let mut text = req.to_string();
    text.push('\n');
    writer.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let line = craft_jsonrpc::read_line(&mut reader, crate::MAX_LINE_BYTES)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "ActCraft closed the connection".to_owned())?;
    serde_json::from_slice(&line).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actcraft_engine::MemoryServices;

    #[test]
    fn serves_requests_over_loopback() {
        let engine = Arc::new(Mutex::new(Engine::new(Box::new(MemoryServices::default()))));
        let Ok(addr) = serve(SocketAddr::from(([127, 0, 0, 1], 0)), engine, None) else { return };
        let resp = request(addr, "command.run", serde_json::json!({"id": "app.about"}));
        assert!(resp.is_ok_and(|v| v.pointer("/result/name").is_some()));
        assert!(
            serve(
                SocketAddr::from(([0, 0, 0, 0], 0)),
                Arc::new(Mutex::new(Engine::new(Box::new(MemoryServices::default())))),
                None
            )
            .is_err()
        );
    }
}
