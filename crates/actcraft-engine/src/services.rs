//! Platform services the engine needs: files and the clock. The engine itself does no I/O, so it
//! builds for wasm32; the desktop app passes native services, tests pass [`MemoryServices`].

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset};

/// Largest file the engine asks for (audio files can be big).
pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub trait Services: Send {
    /// Reads a whole file.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String>;
    /// Writes a whole file, replacing it safely.
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String>;
    /// Local time now.
    fn now(&self) -> DateTime<FixedOffset>;
}

/// Files kept in memory; the clock is fixed unless set.
#[derive(Debug, Clone, Default)]
pub struct MemoryServices {
    pub files: BTreeMap<String, Vec<u8>>,
    pub now: Option<DateTime<FixedOffset>>,
}

impl Services for MemoryServices {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.files.get(path).cloned().ok_or_else(|| format!("no file {path}"))
    }

    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        if self.files.len() > 10_000 {
            return Err("memory file store is full".to_owned());
        }
        self.files.insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn now(&self) -> DateTime<FixedOffset> {
        self.now.unwrap_or_default()
    }
}

/// The file name part of a path (after the last `/` or `\`).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The extension of a path, lowercase, without the dot.
pub fn extension(path: &str) -> Option<String> {
    let name = file_name(path);
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 8 { None } else { Some(ext.to_ascii_lowercase()) }
}
