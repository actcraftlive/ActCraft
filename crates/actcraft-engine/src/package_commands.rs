//! Show packages: the show and every file it uses in one `.actcraftpack` file (a ZIP), to move a
//! show to another computer or keep a backup.
//!
//! Layout: `show.actcraft` (the show, its file references pointing inside the package) and
//! `files/<nnnn>-<name>` for each audio, MIDI or PDF file. Opening a package writes the files into a
//! folder beside it and opens the show with its references pointing there.

use std::collections::BTreeMap;

use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::Show;
use actcraft_show::limits::MAX_TEXT;
use serde_json::{Value, json};

use crate::services::{self, MAX_READ_BYTES, Services};
use crate::{Engine, MessageArg};

/// File extension of a show package.
pub const PACKAGE_EXTENSION: &str = "actcraftpack";
/// The show inside a package.
const SHOW_ENTRY: &str = "show.actcraft";
/// Where packed files live inside a package.
const FILES_PREFIX: &str = "files/";
/// Most entries a package may declare (a show holds at most a few thousand files).
const MAX_ENTRIES: u64 = 20_000;
/// Largest show entry read from a package.
const MAX_SHOW_BYTES: u64 = 64 * 1024 * 1024;
/// Where waveforms of packed files live: `peaks/<the file's name>`.
const PEAKS_PREFIX: &str = "peaks/";
/// Largest waveform entry read from a package.
const MAX_PEAKS_BYTES: u64 = 256 * 1024 * 1024;

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

fn failed(msg: impl Into<String>) -> CommandError {
    CommandError::Failed(msg.into())
}

/// A file name safe inside a package and in any file system: letters, digits, space, `-`, `_`,
/// `.`, `(`, `)`; anything else becomes `_`. Never empty, never only dots, at most 120 chars.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(120)
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').to_owned();
    if trimmed.is_empty() { "file".to_owned() } else { trimmed }
}

/// A packed entry name as the package writer makes it: `files/` and one safe file name.
fn packed_file_name(entry: &str) -> Option<&str> {
    let name = entry.strip_prefix(FILES_PREFIX)?;
    (!name.is_empty() && safe_name(name) == name).then_some(name)
}

/// Whether a path names a show package (`services::extension` stops at 8 letters).
pub fn is_package_path(path: &str) -> bool {
    services::file_name(path)
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case(PACKAGE_EXTENSION))
}

/// Packs files while the show's references are rewritten.
struct Packer<'a> {
    services: &'a dyn Services,
    writer: craft_zip::Writer,
    /// Original path → entry name, so a file used twice is packed once.
    packed: BTreeMap<String, String>,
    total: u64,
    missing: Vec<String>,
    /// Waveforms already worked out, by original path; packed beside their files.
    peaks: BTreeMap<String, Vec<u8>>,
}

impl Packer<'_> {
    /// Packs the file at `path` (once) and points `path` at it; a file that can't be read is
    /// listed as missing and its reference kept.
    fn relink(&mut self, path: &mut String) -> Result<(), CommandError> {
        if path.is_empty() {
            return Ok(());
        }
        if let Some(entry) = self.packed.get(path.as_str()) {
            *path = entry.clone();
            return Ok(());
        }
        let bytes = match self.services.read_file(path) {
            Ok(b) => b,
            Err(_) => {
                self.missing.push(path.clone());
                return Ok(());
            }
        };
        self.total = self.total.saturating_add(bytes.len() as u64);
        if self.total > MAX_READ_BYTES {
            return Err(failed("the show's files are larger than a package can hold (2 GB)"));
        }
        let entry = format!("{FILES_PREFIX}{:04}-{}", self.packed.len() + 1, safe_name(services::file_name(path)));
        // Audio is compressed already; storing keeps packing fast.
        self.writer.add(&entry, &bytes, craft_zip::Compression::Stored).map_err(|e| failed(e.to_string()))?;
        if let Some(peaks) = self.peaks.get(path.as_str())
            && let Some(name) = entry.strip_prefix(FILES_PREFIX)
        {
            let peaks_entry = format!("{PEAKS_PREFIX}{name}");
            self.writer
                .add(&peaks_entry, peaks, craft_zip::Compression::Deflated)
                .map_err(|e| failed(e.to_string()))?;
        }
        self.packed.insert(path.clone(), entry.clone());
        *path = entry;
        Ok(())
    }
}

/// Writes the show and every file it uses into one package file.
pub fn show_export_package(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    if !is_package_path(&path) {
        return Err(invalid(format!("a show package file ends in .{PACKAGE_EXTENSION}")));
    }
    let mut show = e.show.clone();
    // Waveforms known now (in memory or on disk) travel with their files, so the package opens
    // without decoding everything again.
    let mut peaks = BTreeMap::new();
    for song in &e.show.songs {
        for file in song.tracks.iter().map(|t| t.file.as_str()) {
            if file.is_empty() || peaks.contains_key(file) {
                continue;
            }
            if let Some(p) = e.cached_peaks(file).or_else(|| e.stored_peaks(file)) {
                peaks.insert(file.to_owned(), p.to_bytes());
            }
        }
    }
    let mut packer = Packer {
        services: e.services(),
        writer: craft_zip::Writer::new(),
        packed: BTreeMap::new(),
        total: 0,
        missing: Vec::new(),
        peaks,
    };
    for song in &mut show.songs {
        for track in &mut song.tracks {
            packer.relink(&mut track.file)?;
        }
        if let Some(midi) = &mut song.midi_file {
            packer.relink(midi)?;
        }
        if let Some(pdf) = &mut song.pdf_file {
            packer.relink(pdf)?;
        }
    }
    let Packer { mut writer, packed, missing, total, .. } = packer;
    writer.add(SHOW_ENTRY, &show.to_bytes(), craft_zip::Compression::Deflated).map_err(|e| failed(e.to_string()))?;
    let bytes = writer.finish().map_err(|e| failed(e.to_string()))?;
    e.services_mut().write_file(&path, &bytes).map_err(failed)?;
    let file = services::file_name(&path).to_owned();
    if missing.is_empty() {
        e.say_with(
            "Packed the show and {count} files into {file}",
            vec![MessageArg::plain("count", packed.len()), MessageArg::plain("file", file)],
        );
    } else {
        e.say_error_with(
            "Packed the show into {file}; {missing} files could not be read and are not in it",
            vec![MessageArg::plain("file", file), MessageArg::plain("missing", missing.len())],
        );
    }
    Ok(json!({"path": path, "files": packed.len(), "bytes": total, "missing": missing}))
}

/// The folder a package unpacks into by default: beside it, named after it.
pub fn default_unpack_folder(package: &str) -> String {
    let name = services::file_name(package);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    services::join(services::parent(package), &format!("{} files", safe_name(stem)))
}

/// Opens a package: writes its files into a folder (beside the package unless `folder` is given)
/// and opens its show with the references pointing there.
pub fn show_open_package(e: &mut Engine, prm: &Value) -> CommandResult {
    let path = p::str_max(prm, "path", MAX_TEXT)?.to_owned();
    crate::commands::check_discard(e, prm)?;
    let folder = match p::opt_str_max(prm, "folder", MAX_TEXT)? {
        Some(f) => f.to_owned(),
        None => default_unpack_folder(&path),
    };
    let bytes = e.services().read_file(&path).map_err(failed)?;
    let limits = craft_zip::Limits { max_entries: MAX_ENTRIES, max_total_bytes: MAX_READ_BYTES };
    let mut archive =
        craft_zip::Archive::open(&bytes, limits).map_err(|err| failed(format!("not a show package: {err}")))?;
    if archive.has_duplicate_names() {
        return Err(failed("the package has two files with the same name"));
    }
    let show_bytes = archive
        .read(SHOW_ENTRY, MAX_SHOW_BYTES)
        .map_err(|err| failed(err.to_string()))?
        .ok_or_else(|| failed("the package has no show in it"))?;
    let mut show = Show::from_bytes(&show_bytes).map_err(|err| failed(err.to_string()))?;
    let names: Vec<String> = archive.names().map(|n| n.into_owned()).collect();
    e.services_mut().create_folder(&folder).map_err(failed)?;
    let mut written: BTreeMap<String, String> = BTreeMap::new();
    for entry in &names {
        // Only files the writer would have made; anything else (other folders, `..`) is skipped.
        let Some(name) = packed_file_name(entry) else { continue };
        let data = archive
            .read(entry, MAX_READ_BYTES)
            .map_err(|err| failed(err.to_string()))?
            .ok_or_else(|| failed(format!("{entry} could not be read")))?;
        let target = services::join(&folder, name);
        e.services_mut().write_file(&target, &data).map_err(failed)?;
        // Its waveform, when the package carries one, goes into the cache for the new file.
        let peaks_entry = format!("{PEAKS_PREFIX}{name}");
        if let Ok(Some(stored)) = archive.read(&peaks_entry, MAX_PEAKS_BYTES)
            && let Some(p) = actcraft_audio::peaks::Peaks::from_bytes(&stored)
        {
            e.store_peaks(&target, &p);
        }
        written.insert(entry.clone(), target);
    }
    let relink = |file: &mut String| {
        if let Some(target) = written.get(file.as_str()) {
            *file = target.clone();
        }
    };
    for song in &mut show.songs {
        for track in &mut song.tracks {
            relink(&mut track.file);
        }
        if let Some(midi) = &mut song.midi_file {
            relink(midi);
        }
        if let Some(pdf) = &mut song.pdf_file {
            relink(pdf);
        }
    }
    let songs = show.songs.len();
    let first = show.songs.first().map(|s| s.id);
    // The show isn't saved anywhere yet: Save asks where (next to the unpacked files is natural).
    crate::commands::replace_show(e, show, None);
    e.current_song = first;
    e.mark_changed();
    e.say_with(
        "Opened the package: {songs} songs, files in {folder}",
        vec![MessageArg::plain("songs", songs), MessageArg::plain("folder", folder.clone())],
    );
    Ok(json!({"songs": songs, "files": written.len(), "folder": folder}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_inside_packages_are_safe() {
        assert_eq!(safe_name("Drums (take 2).wav"), "Drums (take 2).wav");
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(packed_file_name("files/0001-drums.wav"), Some("0001-drums.wav"));
        assert_eq!(packed_file_name("files/../x.wav"), None);
        assert_eq!(packed_file_name("files/sub/x.wav"), None);
        assert_eq!(packed_file_name("other/x.wav"), None);
        assert_eq!(packed_file_name("files/"), None);
        assert_eq!(default_unpack_folder("C:\\Gigs\\Friday.actcraftpack"), "C:\\Gigs\\Friday files");
        assert_eq!(default_unpack_folder("/home/me/Friday.actcraftpack"), "/home/me/Friday files");
        assert_eq!(default_unpack_folder("Friday.actcraftpack"), "Friday files");
    }
}
