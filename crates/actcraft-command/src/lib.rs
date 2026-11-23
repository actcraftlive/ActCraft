//! The command registry.
//!
//! Every action in ActCraft is a command. The UI, the CLI, the JSON control channel and the MCP
//! server all look commands up by id and dispatch them here, so each front end can do exactly what
//! the others can. A command has an id, an English label (the translation key), a menu path, an
//! optional shortcut, a params doc, an `enabled` check and a `run` function.
//!
//! Dispatch is wrapped in a last-resort guard: a panic inside a command becomes an error instead of
//! taking the program down (house rule "never crash").
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Serialize;
use serde_json::{Map, Value};

pub mod params;

/// Longest command id we accept, so a hostile id can't make lookups or logs expensive.
pub const MAX_ID_LEN: usize = 64;

/// What went wrong running a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum CommandError {
    /// No command has this id.
    UnknownCommand(String),
    /// The command exists but can't run in the current state.
    Disabled(String),
    /// The params were missing, of the wrong type or out of range.
    InvalidParams(String),
    /// The command ran and failed.
    Failed(String),
    /// The command isn't available in this build or on this platform yet.
    Unsupported(String),
    /// The command panicked; the guard caught it.
    Internal(String),
}

impl CommandError {
    /// The message without its kind (what a person reads in the window).
    pub fn message(&self) -> &str {
        match self {
            Self::UnknownCommand(m)
            | Self::Disabled(m)
            | Self::InvalidParams(m)
            | Self::Failed(m)
            | Self::Unsupported(m)
            | Self::Internal(m) => m,
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(m) => write!(f, "unknown command: {m}"),
            Self::Disabled(m) => write!(f, "command disabled: {m}"),
            Self::InvalidParams(m) => write!(f, "invalid params: {m}"),
            Self::Failed(m) => write!(f, "failed: {m}"),
            Self::Unsupported(m) => write!(f, "not supported yet: {m}"),
            Self::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// Result of running a command: a JSON value for every front end.
pub type CommandResult = Result<Value, CommandError>;

/// The static description of a command, shared by menus, help, the CLI and MCP tool lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CommandSpec {
    /// Stable id, `area.action`, lowercase ASCII, digits, `.` and `_`.
    pub id: &'static str,
    /// English label; also the translation key.
    pub label: &'static str,
    /// Menu path, segments separated by `/` (e.g. `Transport/Play`). Empty: not in a menu.
    pub menu: &'static str,
    /// Default shortcut in a platform-neutral form (`Space`, `Ctrl+S`), or none.
    pub shortcut: Option<&'static str>,
    /// What params the command takes, in plain words or a short JSON outline.
    pub params: &'static str,
}

impl CommandSpec {
    /// Whether the command asks for nothing that's required (params `none`, or only `name?:`
    /// fields), so a menu or a remote action can run it as it is.
    pub fn runs_without_params(&self) -> bool {
        if self.params == "none" {
            return true;
        }
        let Some(fields) = self.params.strip_prefix('{').and_then(|d| d.split('}').next()) else { return false };
        fields.match_indices(':').all(|(i, _)| fields.get(..i).is_some_and(|before| before.ends_with('?')))
    }
}

/// A command bound to a context type `C` (the engine).
pub struct Command<C> {
    pub spec: CommandSpec,
    pub enabled: fn(&C) -> bool,
    pub run: fn(&mut C, &Value) -> CommandResult,
}

impl<C> Clone for Command<C> {
    fn clone(&self) -> Self {
        Self { spec: self.spec, enabled: self.enabled, run: self.run }
    }
}

/// Always enabled; the default `enabled` check.
pub fn always<C>(_: &C) -> bool {
    true
}

/// Why a command could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    DuplicateId(&'static str),
    BadId(&'static str),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "duplicate command id {id}"),
            Self::BadId(id) => write!(f, "bad command id {id:?}"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Checks the id shape: `area.action`, lowercase ASCII letters, digits, `_` and `.`, no empty
/// segment, at most [`MAX_ID_LEN`] bytes.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.contains('.')
        && id.split('.').all(|seg| {
            !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// All commands of one context type, by id.
pub struct Registry<C> {
    commands: Vec<Command<C>>,
    index: BTreeMap<&'static str, usize>,
}

impl<C> Default for Registry<C> {
    fn default() -> Self {
        Self { commands: Vec::new(), index: BTreeMap::new() }
    }
}

impl<C> Registry<C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a command. Ids must be unique and well formed.
    pub fn register(&mut self, command: Command<C>) -> Result<(), RegisterError> {
        let id = command.spec.id;
        if !is_valid_id(id) {
            return Err(RegisterError::BadId(id));
        }
        if self.index.contains_key(id) {
            return Err(RegisterError::DuplicateId(id));
        }
        self.index.insert(id, self.commands.len());
        self.commands.push(command);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Command<C>> {
        if id.len() > MAX_ID_LEN {
            return None;
        }
        self.index.get(id).and_then(|&i| self.commands.get(i))
    }

    /// Commands in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &Command<C>> {
        self.commands.iter()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Specs of every command, for help, menus and tool lists.
    pub fn specs(&self) -> Vec<CommandSpec> {
        self.commands.iter().map(|c| c.spec).collect()
    }

    /// Whether the command exists and is enabled in `ctx`.
    pub fn is_enabled(&self, ctx: &C, id: &str) -> bool {
        self.get(id).is_some_and(|c| guard(|| (c.enabled)(ctx)).unwrap_or(false))
    }

    /// Runs a command by id. `params` may be `null` for commands without params. A panic inside
    /// the command is caught and reported as [`CommandError::Internal`]; the context may then be
    /// partly updated, but the program keeps running.
    pub fn dispatch(&self, ctx: &mut C, id: &str, params: &Value) -> CommandResult {
        let Some(command) = self.get(id) else {
            return Err(CommandError::UnknownCommand(truncate(id, MAX_ID_LEN)));
        };
        let enabled = guard(|| (command.enabled)(ctx)).unwrap_or(false);
        if !enabled {
            return Err(CommandError::Disabled(command.spec.id.to_owned()));
        }
        match catch_unwind(AssertUnwindSafe(|| (command.run)(ctx, params))) {
            Ok(result) => result,
            Err(payload) => Err(CommandError::Internal(format!(
                "{} stopped unexpectedly: {}",
                command.spec.id,
                panic_message(payload.as_ref())
            ))),
        }
    }

    /// A JSON list of all commands with their enabled state, for the CLI, control channel and MCP.
    pub fn describe(&self, ctx: &C) -> Value {
        let list = self
            .commands
            .iter()
            .map(|c| {
                let mut m = Map::new();
                m.insert("id".into(), c.spec.id.into());
                m.insert("label".into(), c.spec.label.into());
                m.insert("menu".into(), c.spec.menu.into());
                m.insert("shortcut".into(), c.spec.shortcut.map_or(Value::Null, |s| Value::String(s.to_owned())));
                m.insert("params".into(), c.spec.params.into());
                m.insert("enabled".into(), guard(|| (c.enabled)(ctx)).unwrap_or(false).into());
                Value::Object(m)
            })
            .collect();
        Value::Array(list)
    }
}

/// Runs `f` and turns a panic into `None`.
pub fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        truncate(s, 200)
    } else {
        "no message".to_owned()
    }
}

/// Cuts a string to at most `max` bytes on a character boundary.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx(i64);

    fn add(ctx: &mut Ctx, p: &Value) -> CommandResult {
        let n = params::i64_in(p, "n", -100, 100)?;
        ctx.0 += n;
        Ok(ctx.0.into())
    }

    #[allow(clippy::panic)]
    fn boom(_: &mut Ctx, _: &Value) -> CommandResult {
        panic!("boom")
    }

    fn spec(id: &'static str) -> CommandSpec {
        CommandSpec { id, label: "x", menu: "", shortcut: None, params: "" }
    }

    #[test]
    fn dispatch_runs_and_guards_panics() {
        let mut r = Registry::new();
        assert!(r.register(Command { spec: spec("t.add"), enabled: always, run: add }).is_ok());
        assert!(r.register(Command { spec: spec("t.boom"), enabled: always, run: boom }).is_ok());
        let mut c = Ctx(0);
        assert_eq!(r.dispatch(&mut c, "t.add", &serde_json::json!({"n": 5})), Ok(5.into()));
        assert!(matches!(r.dispatch(&mut c, "t.boom", &Value::Null), Err(CommandError::Internal(_))));
        assert!(matches!(
            r.dispatch(&mut c, "t.add", &serde_json::json!({"n": 1e300})),
            Err(CommandError::InvalidParams(_))
        ));
        assert!(matches!(r.dispatch(&mut c, "nope.x", &Value::Null), Err(CommandError::UnknownCommand(_))));
    }

    #[test]
    fn rejects_duplicate_and_malformed_ids() {
        let mut r = Registry::<Ctx>::new();
        assert!(r.register(Command { spec: spec("a.b"), enabled: always, run: add }).is_ok());
        assert_eq!(
            r.register(Command { spec: spec("a.b"), enabled: always, run: add }),
            Err(RegisterError::DuplicateId("a.b"))
        );
        for bad in ["", "nodot", "A.b", "a..b", "a.b-c"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }
}
