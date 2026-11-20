//! MCP server over stdio. Every engine command is a tool named after its id (dots become
//! underscores, as some clients only accept `[A-Za-z0-9_-]`); tool arguments are the command's
//! params. File access is limited by the services the engine was built with (the CLI requires
//! `--automation-read-root` and `--automation-write-root`).

use std::io::{BufRead, Write};

use actcraft_engine::{Engine, registry};
use craft_jsonrpc::{Limits, RpcError};
use craft_mcp::{Policy, Session};
use serde_json::{Value, json};

/// Tool name of a command id.
pub fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

/// Command id of a tool name.
pub fn command_id(tool: &str) -> Option<&'static str> {
    registry().iter().map(|c| c.spec.id).find(|id| tool_name(id) == tool)
}

/// The tool list.
pub fn tools() -> Value {
    let list: Vec<Value> = registry()
        .iter()
        .map(|c| {
            json!({
                "name": tool_name(c.spec.id),
                "description": format!("{}. Params: {}", c.spec.label, c.spec.params),
                "inputSchema": {"type": "object", "additionalProperties": true},
            })
        })
        .collect();
    Value::Array(list)
}

fn route(
    engine: &mut Engine,
    session: &mut Session,
    req: craft_jsonrpc::Request<'_>,
    in_batch: bool,
) -> Result<Value, RpcError> {
    if let Some(answer) = session.request(req, in_batch) {
        return answer;
    }
    let empty = json!({});
    let params = req.params.unwrap_or(&empty);
    match req.method {
        "tools/list" => Ok(craft_mcp::tool_list(tools())),
        "tools/call" => {
            let (name, args) = craft_mcp::tool_call(params)?;
            let Some(id) = command_id(name) else {
                return Ok(craft_mcp::text_tool_result(
                    &format!("unknown tool {}", name.chars().take(64).collect::<String>()),
                    true,
                ));
            };
            engine.tick();
            Ok(match engine.run(id, &args) {
                Ok(v) => craft_mcp::text_tool_result(&v.to_string(), false),
                Err(e) => craft_mcp::text_tool_result(&e.to_string(), true),
            })
        }
        _ => Err(RpcError::new(-32601, "Method not found")),
    }
}

/// Serves MCP on `input`/`output` until the input ends.
pub fn serve<R: BufRead, W: Write>(engine: &mut Engine, mut input: R, mut output: W) -> Result<(), String> {
    let mut session = Session::new("actcraft", env!("CARGO_PKG_VERSION"), Policy::Compatible);
    session.set_instructions(
        "ActCraft show control. Every tool is an ActCraft command; call app_commands for the list with params."
            .to_owned(),
    );
    let limits = Limits {
        max_message_bytes: crate::MAX_LINE_BYTES,
        integer_ids_only: true,
        allow_null_ids: false,
        ..Limits::default()
    };
    loop {
        let line = match craft_jsonrpc::read_line(&mut input, crate::MAX_LINE_BYTES) {
            Ok(Some(l)) => l,
            Ok(None) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = match craft_jsonrpc::parse_json(&line, limits) {
            Err(_) => Some(craft_jsonrpc::parse_error_response()),
            Ok(value) => {
                let in_batch = value.is_array();
                let mut notes: Vec<Value> = Vec::new();
                let result = craft_jsonrpc::process(
                    &value,
                    limits,
                    |req| {
                        actcraft_engine::guard(|| route(engine, &mut session, req, in_batch))
                            .unwrap_or_else(|| Err(RpcError::new(-32603, "the request stopped unexpectedly")))
                    },
                    |req| notes.push(json!({"method": req.method})),
                );
                for n in notes {
                    if n.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
                        let fake =
                            craft_jsonrpc::Request { id: None, method: "notifications/initialized", params: None };
                        session.notification(fake);
                    }
                }
                result.unwrap_or_else(|_| {
                    Some(craft_jsonrpc::error_response(Value::Null, RpcError::new(-32600, "batch too large")))
                })
            }
        };
        if let Some(resp) = response {
            let mut text = resp.to_string();
            text.push('\n');
            output.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actcraft_engine::MemoryServices;

    #[test]
    fn lists_and_calls_tools() {
        let mut e = Engine::new(Box::new(MemoryServices::default()));
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"song_add","arguments":{"title":"Via MCP"}}}"#,
            "\n",
        );
        let mut out = Vec::new();
        assert!(serve(&mut e, input.as_bytes(), &mut out).is_ok());
        let text = String::from_utf8(out).unwrap_or_default();
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains("transport_play"));
        assert_eq!(e.show.songs.first().map(|s| s.title.as_str()), Some("Via MCP"));
    }
}
