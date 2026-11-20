//! `actcraft-cli`: run ActCraft commands without the window.
//!
//! ```text
//! actcraft-cli commands                          list every command (JSON)
//! actcraft-cli run <id> [params] [--show F] [--save]
//!                                                run a command on a show file
//! actcraft-cli send <id> [params] [--port N] [--token T]
//!                                                run a command in the running app
//! actcraft-cli state [--port N] [--token T]      playback state of the running app
//! actcraft-cli serve [--port N] [--token T] [--show F]
//!                                                headless: audio and control channel, no window
//! actcraft-cli mcp --automation-read-root DIR --automation-write-root DIR [--show F]
//!                                                MCP server on stdin/stdout
//! ```
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use actcraft_control::{channel, mcp};
use actcraft_engine::{Engine, registry};
use actcraft_platform::{AudioOutput, NativeServices, RootedServices};
use serde_json::{Value, json};

const USAGE: &str = "usage:
  actcraft-cli commands
  actcraft-cli run <id> [params-json] [--show FILE] [--save]
  actcraft-cli send <id> [params-json] [--port N] [--token T]
  actcraft-cli state [--port N] [--token T]
  actcraft-cli serve [--port N] [--token T] [--show FILE]
  actcraft-cli mcp --automation-read-root DIR --automation-write-root DIR [--show FILE]";

/// Arguments split into positional values and `--flag value` options.
struct Args {
    positional: Vec<String>,
    options: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Self {
        let mut positional = Vec::new();
        let mut options = Vec::new();
        let mut it = raw.into_iter().take(64).peekable();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let takes_value = !matches!(name, "save" | "help");
                let value = if takes_value { it.next() } else { None };
                options.push((name.to_owned(), value));
            } else {
                positional.push(a);
            }
        }
        Self { positional, options }
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.options.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }

    fn flag(&self, name: &str) -> bool {
        self.options.iter().any(|(n, _)| n == name)
    }
}

fn params_of(s: Option<&String>) -> Result<Value, String> {
    match s {
        None => Ok(Value::Null),
        Some(text) if text.trim().is_empty() => Ok(Value::Null),
        Some(text) => serde_json::from_str(text).map_err(|e| format!("params are not valid JSON: {e}")),
    }
}

fn addr(args: &Args) -> Result<SocketAddr, String> {
    let port = match args.opt("port") {
        Some(p) => p.parse::<u16>().map_err(|_| format!("`{p}` is not a port"))?,
        None => channel::DEFAULT_PORT,
    };
    Ok(SocketAddr::from(([127, 0, 0, 1], port)))
}

fn with_token(mut params: Value, token: Option<&str>) -> Value {
    if let (Some(t), Some(obj)) = (token, params.as_object_mut()) {
        obj.insert("token".into(), json!(t));
    }
    params
}

fn open_show(engine: &mut Engine, args: &Args) -> Result<(), String> {
    if let Some(show) = args.opt("show") {
        engine.run("show.open", &json!({"path": show})).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()));
}

fn real_main() -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1).collect());
    let Some(cmd) = args.positional.first().cloned() else {
        println!("{USAGE}");
        return Ok(());
    };
    if args.flag("help") {
        println!("{USAGE}");
        return Ok(());
    }
    match cmd.as_str() {
        "commands" => {
            let engine = Engine::new(Box::new(NativeServices));
            print(&registry().describe(&engine));
            Ok(())
        }
        "run" => {
            let id = args.positional.get(1).ok_or("run needs a command id")?;
            let params = params_of(args.positional.get(2))?;
            let mut engine = Engine::new(Box::new(NativeServices));
            open_show(&mut engine, &args)?;
            let result = engine.run(id, &params).map_err(|e| e.to_string())?;
            if args.flag("save") {
                engine.run("show.save", &Value::Null).map_err(|e| e.to_string())?;
            }
            print(&result);
            Ok(())
        }
        "send" => {
            let id = args.positional.get(1).ok_or("send needs a command id")?;
            let params = params_of(args.positional.get(2))?;
            let resp = channel::request(
                addr(&args)?,
                "command.run",
                with_token(json!({"id": id, "params": params}), args.opt("token")),
            )?;
            print(&resp);
            if resp.get("error").is_some() { Err("the command failed".to_owned()) } else { Ok(()) }
        }
        "state" => {
            let resp = channel::request(addr(&args)?, "state.get", with_token(json!({}), args.opt("token")))?;
            print(&resp);
            Ok(())
        }
        "serve" => {
            let mut engine = Engine::new(Box::new(NativeServices));
            let audio = match AudioOutput::open(None) {
                Ok((out, ctl)) => {
                    engine.attach_audio(ctl);
                    eprintln!("audio: {} at {} Hz", out.device_name, out.sample_rate);
                    Some(out)
                }
                Err(e) => {
                    eprintln!("no audio output: {e}");
                    None
                }
            };
            open_show(&mut engine, &args)?;
            let engine = Arc::new(Mutex::new(engine));
            let bound = channel::serve(addr(&args)?, Arc::clone(&engine), args.opt("token").map(str::to_owned))?;
            eprintln!("control channel on {bound}; stop with Ctrl+C");
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(mut e) = engine.lock() {
                    e.tick();
                }
                let _keep = &audio;
            }
        }
        "mcp" => {
            // Agent entry points must be fenced to two folders (Craft rule for agent entry points).
            let read = args.opt("automation-read-root").ok_or("mcp needs --automation-read-root DIR")?;
            let write = args.opt("automation-write-root").ok_or("mcp needs --automation-write-root DIR")?;
            let services = RootedServices::new(Path::new(read), Path::new(write))?;
            let mut engine = Engine::new(Box::new(services));
            open_show(&mut engine, &args)?;
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            mcp::serve(&mut engine, stdin.lock(), stdout.lock())
        }
        other => Err(format!("unknown command `{other}`\n{USAGE}")),
    }
}

fn main() {
    let outcome = actcraft_engine::guard(real_main).unwrap_or_else(|| Err("stopped unexpectedly".to_owned()));
    if let Err(e) = outcome {
        eprintln!("actcraft-cli: {e}");
        std::process::exit(1);
    }
}
