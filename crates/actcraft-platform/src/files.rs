//! File services.

use std::io::Read;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use actcraft_engine::Services;
use actcraft_engine::services::{FileReader, Job, MAX_READ_BYTES};
use chrono::{DateTime, FixedOffset, Local};

/// Real files. Writes go through a temporary file and a rename, so a crash never leaves a half
/// written show. Every file operation is wrapped in a last-resort guard.
#[derive(Debug, Clone, Default)]
pub struct NativeServices;

fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len > MAX_READ_BYTES {
        return Err(format!("{} is larger than {} bytes", path.display(), MAX_READ_BYTES));
    }
    let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(MAX_READ_BYTES).read_to_end(&mut out).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(out)
}

fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    actcraft_engine::guard(f).unwrap_or_else(|| Err(format!("{what} stopped unexpectedly")))
}

impl Services for NativeServices {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        guarded("reading a file", || read_capped(Path::new(path)))
    }

    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        guarded("writing a file", || {
            craft_atomic::atomic_write(Path::new(path), bytes).map_err(|e| format!("cannot write {path}: {e}"))
        })
    }

    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset()
    }

    fn file_reader(&self) -> FileReader {
        Arc::new(|path: &str| guarded("reading a file", || read_capped(Path::new(path))))
    }

    fn file_stamp(&self, path: &str) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(format!("{}-{}", meta.len(), modified.as_nanos()))
    }

    fn cache_get(&self, key: &str) -> Option<Vec<u8>> {
        let file = cache_dir()?.join(cache_file_name(key));
        guarded("reading the cache", || read_capped(&file)).ok()
    }

    fn cache_put(&mut self, key: &str, bytes: &[u8]) {
        let Some(dir) = cache_dir() else { return };
        let _ = guarded("writing the cache", || {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            craft_atomic::atomic_write(&dir.join(cache_file_name(key)), bytes).map_err(|e| e.to_string())
        });
    }

    fn create_folder(&mut self, path: &str) -> Result<(), String> {
        guarded("making a folder", || match std::fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && Path::new(path).is_dir() => Ok(()),
            Err(e) => Err(format!("cannot make the folder {path}: {e}")),
        })
    }

    fn spawn(&self, job: Job) {
        spawn_job(job);
    }
}

/// ActCraft's cache folder: work that can be redone (waveforms), kept between runs.
/// Windows `%LOCALAPPDATA%\ActCraft\cache`, macOS `~/Library/Caches/ActCraft`, elsewhere
/// `$XDG_CACHE_HOME/actcraft` or `~/.cache/actcraft`. `ACTCRAFT_CACHE_DIR` overrides it.
pub fn cache_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("ACTCRAFT_CACHE_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        return var("LOCALAPPDATA").map(|d| d.join("ActCraft").join("cache"));
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|h| h.join("Library").join("Caches").join("ActCraft"));
    }
    var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache"))).map(|d| d.join("actcraft"))
}

/// A cache key as a file name: its hash in hex, so any key is a safe name.
fn cache_file_name(key: &str) -> String {
    format!("{:016x}.bin", actcraft_engine::services::fnv1a(key.as_bytes()))
}

/// Runs a background job on its own thread; if no thread can start, runs it here so the work
/// still happens.
fn spawn_job(job: Job) {
    let slot = Arc::new(std::sync::Mutex::new(Some(job)));
    let worker = Arc::clone(&slot);
    let started = std::thread::Builder::new().name("actcraft-decode".into()).spawn(move || {
        if let Some(job) = worker.lock().ok().and_then(|mut j| j.take()) {
            job();
        }
    });
    if started.is_err()
        && let Some(job) = slot.lock().ok().and_then(|mut j| j.take())
    {
        job();
    }
}

/// Files limited to two folders: one that may be read and one that may be written (the
/// automation roots `actcraft-cli mcp` requires). Paths are resolved before the check, so `..`
/// and links can't escape.
#[derive(Debug, Clone)]
pub struct RootedServices {
    read_root: PathBuf,
    write_root: PathBuf,
    inner: NativeServices,
}

impl RootedServices {
    /// Both roots must exist.
    pub fn new(read_root: &Path, write_root: &Path) -> Result<Self, String> {
        let read_root =
            std::fs::canonicalize(read_root).map_err(|e| format!("read root {}: {e}", read_root.display()))?;
        let write_root =
            std::fs::canonicalize(write_root).map_err(|e| format!("write root {}: {e}", write_root.display()))?;
        Ok(Self { read_root, write_root, inner: NativeServices })
    }

    fn resolve(&self, path: &str, root: &Path) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let p = if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
        // A file that doesn't exist yet (a save) is checked through its folder.
        let resolved = match std::fs::canonicalize(&p) {
            Ok(r) => r,
            Err(_) => {
                let parent = p.parent().ok_or_else(|| format!("{path} has no folder"))?;
                let name = p.file_name().ok_or_else(|| format!("{path} has no file name"))?;
                std::fs::canonicalize(parent).map_err(|e| format!("{path}: {e}"))?.join(name)
            }
        };
        if resolved.starts_with(root) { Ok(resolved) } else { Err(format!("{path} is outside the allowed folder")) }
    }

    /// Whether reading `path` is allowed (inside the read or the write root).
    pub fn may_read(&self, path: &str) -> bool {
        self.resolve(path, &self.read_root).is_ok() || self.resolve(path, &self.write_root).is_ok()
    }
}

impl Services for RootedServices {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        // A relative path is looked up in the read root first, then in the write root (where
        // automation saved it).
        let in_read = self.resolve(path, &self.read_root).ok().filter(|p| p.exists());
        let p = match in_read {
            Some(p) => p,
            None => self.resolve(path, &self.write_root).or_else(|_| self.resolve(path, &self.read_root))?,
        };
        self.inner.read_file(&p.to_string_lossy())
    }

    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let p = self.resolve(path, &self.write_root)?;
        self.inner.write_file(&p.to_string_lossy(), bytes)
    }

    fn now(&self) -> DateTime<FixedOffset> {
        self.inner.now()
    }

    fn create_folder(&mut self, path: &str) -> Result<(), String> {
        // Checked like a file written there: the parent must be inside the write root.
        let p = self.resolve(path, &self.write_root)?;
        self.inner.create_folder(&p.to_string_lossy())
    }

    fn file_reader(&self) -> FileReader {
        // The same folder rules as `read_file`, on any thread.
        let me = self.clone();
        Arc::new(move |path: &str| me.read_file(path))
    }

    fn spawn(&self, job: Job) {
        spawn_job(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_services_refuse_paths_outside() {
        let base = std::env::temp_dir().join(format!("actcraft-root-test-{}", std::process::id()));
        let read = base.join("in");
        let write = base.join("out");
        assert!(std::fs::create_dir_all(&read).is_ok() && std::fs::create_dir_all(&write).is_ok());
        let Ok(mut s) = RootedServices::new(&read, &write) else { return };
        let written = s.write_file("show.actcraft", b"{}");
        assert!(written.is_ok(), "{written:?}");
        assert!(s.read_file("show.actcraft").is_ok(), "the write root is readable too");
        assert!(s.write_file("../escape.txt", b"x").is_err());
        assert!(s.read_file("../../../../../../etc/hosts").is_err());
        let outside = std::env::temp_dir().join("actcraft-outside.txt");
        assert!(s.write_file(&outside.to_string_lossy(), b"x").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
