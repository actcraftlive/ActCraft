//! Remote control of the engine.
//!
//! - [`channel`]: a JSON-RPC 2.0 control channel, newline-delimited, on a loopback TCP port. Methods:
//!   `commands.list`, `command.run` `{id, params}` and `state.get`.
//! - [`mcp`]: a Model Context Protocol server over stdio; every command is a tool.
//!
//! Both only dispatch commands by id, exactly like the window and the CLI.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use actcraft_engine::{CommandError, Engine, registry};
use craft_jsonrpc::RpcError;
use serde_json::{Value, json};

pub mod channel;
pub mod mcp;

/// Largest request line accepted.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// JSON-RPC error code for a command that failed.
pub const COMMAND_FAILED: i64 = -32000;

fn rpc_error(e: &CommandError) -> RpcError {
    let mut err = RpcError::new(COMMAND_FAILED, e.to_string());
    err.data = serde_json::to_value(e).ok();
    err
}

/// The playback state, for remotes.
pub fn state(e: &Engine) -> Value {
    let s = &e.status;
    json!({
        "song": e.current_song,
        "playing": s.playing,
        "time": s.time,
        "bar": s.bar,
        "beat_in_bar": s.beat_in_bar,
        "counting_in": s.counting_in,
        "crossfading": s.crossfading,
        "queue": e.queue.len(),
        "metronome": e.show.metronome.enabled,
        "dirty": e.dirty,
    })
}

/// Routes one control-channel method.
pub fn handle(e: &mut Engine, method: &str, params: Option<&Value>) -> Result<Value, RpcError> {
    let empty = Value::Null;
    let params = params.unwrap_or(&empty);
    e.tick();
    match method {
        "commands.list" => Ok(registry().describe(e)),
        "state.get" => Ok(state(e)),
        "command.run" => {
            let id =
                params.get("id").and_then(Value::as_str).ok_or_else(|| RpcError::new(-32602, "`id` is required"))?;
            let cmd_params = params.get("params").cloned().unwrap_or(Value::Null);
            e.run(id, &cmd_params).map_err(|err| rpc_error(&err))
        }
        _ => Err(RpcError::new(-32601, format!("unknown method {}", actcraft_command_truncate(method)))),
    }
}

fn actcraft_command_truncate(s: &str) -> String {
    s.chars().take(64).collect()
}

/// Processes one raw request line and returns the response line (if any).
pub fn handle_line(e: &mut Engine, line: &[u8], token: Option<&str>) -> Option<Value> {
    let limits = craft_jsonrpc::Limits { max_message_bytes: MAX_LINE_BYTES, ..craft_jsonrpc::Limits::default() };
    let value = match craft_jsonrpc::parse_json(line, limits) {
        Ok(v) => v,
        Err(_) => return Some(craft_jsonrpc::parse_error_response()),
    };
    let result = craft_jsonrpc::process(
        &value,
        limits,
        |req| {
            if let Some(t) = token {
                let given = req.params.and_then(|p| p.get("token")).and_then(Value::as_str);
                if given != Some(t) {
                    return Err(RpcError::new(-32001, "a valid `token` is required"));
                }
            }
            actcraft_engine::guard(|| handle(e, req.method, req.params))
                .unwrap_or_else(|| Err(RpcError::new(-32603, "the request stopped unexpectedly")))
        },
        |_| {},
    );
    match result {
        Ok(resp) => resp,
        Err(_) => Some(craft_jsonrpc::error_response(Value::Null, RpcError::new(-32600, "batch too large"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actcraft_engine::MemoryServices;

    #[test]
    fn runs_commands_and_rejects_garbage() {
        let mut e = Engine::new(Box::new(MemoryServices::default()));
        let r = handle_line(
            &mut e,
            br#"{"jsonrpc":"2.0","id":1,"method":"command.run","params":{"id":"song.add","params":{"title":"X"}}}"#,
            None,
        );
        assert_eq!(r.and_then(|v| v.get("result").cloned()), Some(json!({"song": 1})));
        let bad = handle_line(&mut e, b"\xff\xfe not json", None);
        assert!(bad.is_some_and(|v| v.get("error").is_some()));
        let locked = handle_line(&mut e, br#"{"jsonrpc":"2.0","id":2,"method":"state.get"}"#, Some("secret"));
        assert!(locked.is_some_and(|v| v.get("error").is_some()));
        let fail = handle_line(
            &mut e,
            br#"{"jsonrpc":"2.0","id":3,"method":"command.run","params":{"id":"nope.nope"}}"#,
            None,
        );
        assert!(fail.is_some_and(|v| v.get("error").is_some()));
    }
}
