//! Platform services the engine needs: files and the clock. The engine itself does no I/O, so it
//! builds for wasm32; the desktop app passes native services, tests pass [`MemoryServices`].

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, FixedOffset};

/// Largest file the engine asks for (audio files can be big).
pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Reads a whole file; handed to background jobs.
pub type FileReader = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// A background job.
pub type Job = Box<dyn FnOnce() + Send>;

/// Speaks text with the computer's own voice into an audio file's bytes (WAV or AIFF): text and
/// a language code (`de`, `en`; empty for the computer's own). Runs on background jobs.
pub type Speaker = Arc<dyn Fn(&str, &str) -> Result<Vec<u8>, String> + Send + Sync>;

pub trait Services: Send {
    /// Reads a whole file.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String>;
    /// Writes a whole file, replacing it safely.
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String>;
    /// Local time now.
    fn now(&self) -> DateTime<FixedOffset>;
    /// A reader background jobs can use, with the same access rules as `read_file`.
    fn file_reader(&self) -> FileReader;
    /// Makes a folder (its parent must exist); an existing folder is fine. Platforms without
    /// folders (memory) have nothing to do.
    fn create_folder(&mut self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    /// Size and modification time of a file as one text, so a cache entry made for it can tell
    /// when the file changed. `None` when unknown (then nothing is cached for it).
    fn file_stamp(&self, _path: &str) -> Option<String> {
        None
    }
    /// A file from ActCraft's own cache (work done before, such as waveforms), by key.
    fn cache_get(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }
    /// Keeps bytes in ActCraft's own cache under a key; failures are ignored (it's only a cache).
    fn cache_put(&mut self, _key: &str, _bytes: &[u8]) {}
    /// The computer's speech, for spoken cues; `None` where there is none.
    fn speaker(&self) -> Option<Speaker> {
        None
    }
    /// Runs a job off the caller's thread where the platform has threads; inline otherwise.
    fn spawn(&self, job: Job) {
        job();
    }
}

/// Files kept in memory; the clock is fixed unless set.
#[derive(Debug, Clone, Default)]
pub struct MemoryServices {
    pub files: BTreeMap<String, Vec<u8>>,
    pub now: Option<DateTime<FixedOffset>>,
    pub cache: BTreeMap<String, Vec<u8>>,
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

    fn file_stamp(&self, path: &str) -> Option<String> {
        // Memory files have no clock: the length and a checksum stand in for the time.
        self.files.get(path).map(|b| format!("{}-{:016x}", b.len(), fnv1a(b)))
    }

    fn cache_get(&self, key: &str) -> Option<Vec<u8>> {
        self.cache.get(key).cloned()
    }

    fn cache_put(&mut self, key: &str, bytes: &[u8]) {
        if self.cache.len() < 10_000 {
            self.cache.insert(key.to_owned(), bytes.to_vec());
        }
    }

    fn file_reader(&self) -> FileReader {
        let files = self.files.clone();
        Arc::new(move |path: &str| files.get(path).cloned().ok_or_else(|| format!("no file {path}")))
    }
}

/// FNV-1a, 64 bit: a small stable hash for cache file names (not for security).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The folder part of a path (before the last `/` or `\`), empty for a bare file name.
pub fn parent(path: &str) -> &str {
    path.rfind(['/', '\\']).map_or("", |i| &path[..i])
}

/// `name` inside `folder`, joined with the separator `folder` already uses.
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        return name.to_owned();
    }
    let sep = if folder.contains('\\') && !folder.contains('/') { '\\' } else { '/' };
    format!("{}{sep}{name}", folder.trim_end_matches(['/', '\\']))
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
