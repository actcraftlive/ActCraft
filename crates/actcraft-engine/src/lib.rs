//! The ActCraft engine: the open show, playback state, the queue and every command.
//!
//! Front ends (the window, the CLI, the JSON control channel, the MCP server) only read engine
//! state and run commands by id through [`Engine::run`]. Nothing else changes the engine.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

use actcraft_audio::peaks::Peaks;
use actcraft_audio::render::TrackSlot;
use actcraft_audio::{BusParams, ClickMode, Clip, Controller, MetronomeParams, Msg, SongLoad, Status, TrackParams};
use actcraft_command::{CommandResult, Registry};
use actcraft_library::Queue;
use actcraft_library::queue::DoubleTap;
use actcraft_show::{MetronomeMode, Show, Song, SongClick, Track};
use serde_json::Value;

pub mod commands;
pub mod error_text;
pub mod macros;
pub mod midi_file;
pub mod midi_sync;
pub mod notation_commands;
pub mod package_commands;
pub mod pads;
pub mod pdf_commands;
pub mod region_commands;
pub mod remote;
pub mod services;
pub mod voice;

pub use actcraft_audio::tempo;
pub use actcraft_audio::{BeatClock, GridLine};
pub use actcraft_command::{CommandError, CommandSpec, guard};
pub use actcraft_lyrics as lyrics;
pub use actcraft_notation as notation;
pub use services::{MemoryServices, Services};

/// Messages kept for the status line and screen readers.
pub const MAX_MESSAGES: usize = 50;
/// Decoded files kept in memory.
pub const MAX_CACHED_CLIPS: usize = 256;

/// One message for the person (shown and announced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The message in English, values filled in (for the CLI, logs and the control channel).
    pub text: String,
    /// The English template with `{name}` placeholders; the window translates it and fills in
    /// `args`. Equal to `text` for a message without values.
    pub template: String,
    pub args: Vec<MessageArg>,
    pub error: bool,
}

/// A value put into a message's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageArg {
    pub name: &'static str,
    pub value: String,
    /// The value is English window text (a mode's name) the window translates too; a song title
    /// or a file name is shown as it is.
    pub translate: bool,
}

impl MessageArg {
    /// A value shown as it is (a title, a file name, a number).
    pub fn plain(name: &'static str, value: impl ToString) -> Self {
        Self { name, value: value.to_string(), translate: false }
    }

    /// A value that is English window text, translated with the message.
    pub fn word(name: &'static str, value: &str) -> Self {
        Self { name, value: value.to_owned(), translate: true }
    }
}

/// `template` with each `{name}` replaced by its value (one pass; values are never read again).
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').map(|close| (&after[..close], &after[close + 1..])) {
            Some((name, tail)) if args.iter().any(|(n, _)| *n == name) => {
                if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                    out.push_str(v);
                }
                rest = tail;
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A file decoded in the background.
struct LoadResult {
    path: String,
    result: Result<(Arc<Clip>, Arc<Peaks>), String>,
}

/// The engine.
pub struct Engine {
    pub show: Show,
    /// Where the show was opened from or last saved to.
    pub show_path: Option<String>,
    /// Unsaved changes.
    pub dirty: bool,
    /// The song selected for playback.
    pub current_song: Option<u64>,
    /// The playlist playback follows for next/previous.
    pub current_playlist: Option<usize>,
    /// Last value of each MIDI controller (channel, number) seen, so a foot switch sets off its
    /// remote action once per press.
    remote_cc: std::collections::HashMap<(u8, u8), u8>,
    /// Changes made to the show so far (lyrics read again for macro calls after one).
    edits: usize,
    /// Song time macros were last looked at, while playing; and the playing song's macro calls.
    macro_at: Option<f64>,
    macro_cues: Option<((u64, usize), macros::MacroCues)>,
    pub queue: Queue,
    pub double_tap: DoubleTap,
    /// Last search and its results.
    pub search_query: String,
    pub search_results: Vec<u64>,
    /// Format string for song details from file names.
    pub filename_format: String,
    /// Ignore a second play/stop within this many milliseconds (double-trigger protection).
    pub play_guard_ms: u32,
    /// Newest audio status.
    pub status: Status,
    /// Messages for the person, newest last.
    pub messages: VecDeque<Message>,
    audio: Option<Controller>,
    loaded_song: Option<u64>,
    last_ended: u32,
    last_transport_ms: Option<i64>,
    clips: BTreeMap<String, Arc<Clip>>,
    peaks: BTreeMap<String, Arc<Peaks>>,
    /// Spoken cues, ready and in the making.
    voices: voice::Voices,
    /// A loaded song to load again (when stopped) because its spoken cues changed.
    voice_reload: Option<u64>,
    services: Box<dyn Services>,
    // Background decoding: files in flight, files that failed, and what waits for them.
    load_tx: Sender<LoadResult>,
    load_rx: Receiver<LoadResult>,
    loading: BTreeSet<String>,
    failed: BTreeSet<String>,
    pending_load: Option<u64>,
    /// Start playback (with or without count-in) once the pending song has loaded.
    play_after_load: Option<bool>,
    /// Songs whose tempo is estimated once their first track has decoded.
    detect_bpm: BTreeSet<u64>,
}

/// A song's regions as the renderer's table.
pub(crate) fn region_table(song: &Song) -> actcraft_audio::regions::RegionTable {
    use actcraft_audio::regions::{RegionAction, RegionSpan, RegionTable};
    use actcraft_show::RegionMode;
    RegionTable::new(song.regions.iter().map(|r| RegionSpan {
        start: r.start,
        end: r.end,
        action: match r.mode {
            RegionMode::Play => RegionAction::Play,
            RegionMode::Loop => RegionAction::Loop,
            RegionMode::Skip => RegionAction::Skip,
            RegionMode::StopAfter => RegionAction::StopAfter,
        },
    }))
}

/// The registry with every engine command, built once.
pub fn registry() -> &'static Registry<Engine> {
    static REGISTRY: OnceLock<Registry<Engine>> = OnceLock::new();
    REGISTRY.get_or_init(commands::build)
}

impl Engine {
    /// An engine with an empty show and no audio device.
    pub fn new(services: Box<dyn Services>) -> Self {
        let (load_tx, load_rx) = channel();
        Self {
            load_tx,
            load_rx,
            loading: BTreeSet::new(),
            failed: BTreeSet::new(),
            pending_load: None,
            play_after_load: None,
            detect_bpm: BTreeSet::new(),
            show: Show::default(),
            show_path: None,
            dirty: false,
            current_song: None,
            current_playlist: None,
            remote_cc: std::collections::HashMap::new(),
            edits: 0,
            macro_at: None,
            macro_cues: None,
            queue: Queue::new(),
            double_tap: DoubleTap::default(),
            search_query: String::new(),
            search_results: Vec::new(),
            filename_format: "%artist% - %title%".to_owned(),
            play_guard_ms: 0,
            status: Status::default(),
            messages: VecDeque::new(),
            audio: None,
            loaded_song: None,
            last_ended: 0,
            last_transport_ms: None,
            clips: BTreeMap::new(),
            peaks: BTreeMap::new(),
            voices: voice::Voices::default(),
            voice_reload: None,
            services,
        }
    }

    /// Connects an audio renderer's controller (from the platform's audio output).
    pub fn attach_audio(&mut self, controller: Controller) {
        self.audio = Some(controller);
        self.loaded_song = None;
        self.push_all_mixer();
        if let Some(id) = self.current_song {
            self.load_into_renderer(id);
        }
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Runs a command by id; the one entry point for every front end.
    pub fn run(&mut self, id: &str, params: &Value) -> CommandResult {
        let result = registry().dispatch(self, id, params);
        if let Err(e) = &result {
            // The window shows (and translates) the message alone, a text with values through
            // its template; the full text stays for the CLI, logs and the control channel.
            let (template, args) = match error_text::template_of(e.message()) {
                Some((template, args)) => (template.to_owned(), args),
                None => (e.message().to_owned(), Vec::new()),
            };
            self.push_message(template, args, e.to_string(), true);
        }
        result
    }

    /// Whether a command exists and is enabled now.
    pub fn enabled(&self, id: &str) -> bool {
        registry().is_enabled(self, id)
    }

    /// Call regularly (each UI frame, each control-channel poll): reads the audio status and
    /// handles song ends (history, autoplay).
    pub fn tick(&mut self) {
        self.drain_loads();
        self.drain_voices();
        let Some(audio) = &mut self.audio else { return };
        self.status = audio.poll();
        if self.status.ended_count != self.last_ended {
            self.last_ended = self.status.ended_count;
            self.on_song_end();
        }
        self.run_due_macros();
    }

    fn on_song_end(&mut self) {
        let Some(id) = self.current_song else { return };
        let autoplay = self.show.song(id).is_some_and(|s| s.autoplay);
        if autoplay && let Some(next) = self.next_song_id() {
            self.select_song(next);
            self.send(Msg::Play { count_in: true, quantized: false });
        }
    }

    /// A message without values (English; the window translates it).
    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.push_message(text.clone(), Vec::new(), text, false);
    }

    pub fn say_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.push_message(text.clone(), Vec::new(), text, true);
    }

    /// A message from an English template with `{name}` placeholders and their values.
    pub fn say_with(&mut self, template: &'static str, args: Vec<MessageArg>) {
        let text = Self::english(template, &args);
        self.push_message(template.to_owned(), args, text, false);
    }

    pub fn say_error_with(&mut self, template: &'static str, args: Vec<MessageArg>) {
        let text = Self::english(template, &args);
        self.push_message(template.to_owned(), args, text, true);
    }

    fn english(template: &str, args: &[MessageArg]) -> String {
        let pairs: Vec<(&str, &str)> = args.iter().map(|a| (a.name, a.value.as_str())).collect();
        fill(template, &pairs)
    }

    fn push_message(&mut self, template: String, args: Vec<MessageArg>, text: String, error: bool) {
        if self.messages.len() >= MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(Message { text, template, args, error });
    }

    pub fn services(&self) -> &dyn Services {
        self.services.as_ref()
    }

    pub fn services_mut(&mut self) -> &mut dyn Services {
        self.services.as_mut()
    }

    pub fn current(&self) -> Option<&Song> {
        self.current_song.and_then(|id| self.show.song(id))
    }

    /// Whether audio is playing.
    pub fn playing(&self) -> bool {
        self.status.playing
    }

    pub(crate) fn send(&mut self, msg: Msg) -> bool {
        match &mut self.audio {
            Some(a) => a.send(msg),
            None => false,
        }
    }

    pub(crate) fn sample_rate(&self) -> f64 {
        self.audio.as_ref().map_or(48_000.0, Controller::sample_rate)
    }

    /// Double-trigger protection: true when a transport command came too soon after the last.
    pub(crate) fn transport_guard(&mut self) -> bool {
        if self.play_guard_ms == 0 {
            return false;
        }
        let now = self.services.now().timestamp_millis();
        let too_soon = self.last_transport_ms.is_some_and(|t| now.saturating_sub(t) < i64::from(self.play_guard_ms));
        if !too_soon {
            self.last_transport_ms = Some(now);
        }
        too_soon
    }

    /// The song after the current one: the queue first, then the current playlist.
    pub fn next_song_id(&mut self) -> Option<u64> {
        if let Some(q) = self.queue.pop() {
            return Some(q);
        }
        self.neighbour(1)
    }

    /// The song `delta` places from the current one in the current playlist (all songs when no
    /// playlist is chosen).
    pub fn neighbour(&self, delta: i64) -> Option<u64> {
        let order: Vec<u64> = match self.current_playlist.and_then(|i| self.show.playlists.get(i)) {
            Some(p) => commands::playlist_songs(&self.show, p),
            None => self.show.songs.iter().map(|s| s.id).collect(),
        };
        let cur = self.current_song?;
        let pos = order.iter().position(|&s| s == cur)?;
        let target = i64::try_from(pos).ok()?.checked_add(delta)?;
        order.get(usize::try_from(target).ok()?).copied()
    }

    /// Makes a song current and loads its audio.
    pub fn select_song(&mut self, id: u64) {
        self.current_song = Some(id);
        self.load_into_renderer(id);
        if let Some(s) = self.show.song(id) {
            if s.title.is_empty() {
                self.say("Loaded an untitled song");
            } else {
                let title = s.title.clone();
                self.say_with("Loaded {title}", vec![MessageArg::plain("title", title)]);
            }
        }
    }

    /// Decoded audio of a file, cached.
    pub fn clip(&mut self, path: &str) -> Result<Arc<Clip>, String> {
        if let Some(c) = self.clips.get(path) {
            return Ok(Arc::clone(c));
        }
        let bytes = self.services.read_file(path)?;
        let ext = services::extension(path);
        let clip = actcraft_audio::clip::decode(&bytes, ext.as_deref())?;
        let peaks = Arc::new(Peaks::of(&clip));
        self.cache_clip(path.to_owned(), Arc::clone(&clip), peaks);
        Ok(clip)
    }

    /// Waveform peaks of a file, cached.
    pub fn peaks(&mut self, path: &str) -> Result<Arc<Peaks>, String> {
        if let Some(p) = self.peaks.get(path) {
            return Ok(Arc::clone(p));
        }
        let clip = self.clip(path)?;
        let p = Arc::new(Peaks::of(&clip));
        self.peaks.insert(path.to_owned(), Arc::clone(&p));
        Ok(p)
    }

    /// Cached peaks without loading anything (for drawing).
    pub fn cached_peaks(&self, path: &str) -> Option<Arc<Peaks>> {
        self.peaks.get(path).cloned()
    }

    /// The waveform of a file: from memory, else from the cache on disk (made the last time the
    /// file was decoded, as long as the file hasn't changed), else decoded in the background
    /// (`None` until then).
    pub fn peaks_for(&mut self, path: &str) -> Option<Arc<Peaks>> {
        if let Some(p) = self.peaks.get(path) {
            return Some(Arc::clone(p));
        }
        if let Some(p) = self.stored_peaks(path) {
            self.keep_peaks(path.to_owned(), Arc::clone(&p));
            return Some(p);
        }
        self.request_clips([path.to_owned()]);
        None
    }

    /// The cache key of a file's waveform: its path and stamp (size and time), so an edited file
    /// is decoded again.
    pub(crate) fn peaks_key(&self, path: &str) -> Option<String> {
        self.services.file_stamp(path).map(|stamp| format!("peaks1|{path}|{stamp}"))
    }

    /// A waveform stored on disk for this file as it is now.
    pub(crate) fn stored_peaks(&self, path: &str) -> Option<Arc<Peaks>> {
        let key = self.peaks_key(path)?;
        let bytes = self.services.cache_get(&key)?;
        // The entry starts with its key, so a hash collision in the file name can't mix files up.
        let rest = bytes.strip_prefix(key.as_bytes())?.strip_prefix(&[0])?;
        Peaks::from_bytes(rest).map(Arc::new)
    }

    /// Stores a file's waveform on disk for the next time.
    pub(crate) fn store_peaks(&mut self, path: &str, peaks: &Peaks) {
        if let Some(key) = self.peaks_key(path) {
            let mut entry = key.clone().into_bytes();
            entry.push(0);
            entry.extend_from_slice(&peaks.to_bytes());
            self.services.cache_put(&key, &entry);
        }
    }

    fn keep_peaks(&mut self, path: String, peaks: Arc<Peaks>) {
        // Waveforms without their audio are small, but still bounded.
        if self.peaks.len() >= MAX_CACHED_CLIPS * 4
            && let Some(k) = self.peaks.keys().find(|k| !self.clips.contains_key(*k)).cloned()
        {
            self.peaks.remove(&k);
        }
        self.peaks.insert(path, peaks);
    }

    pub(crate) fn track_params(&self, t: &Track) -> TrackParams {
        let eq = [0, 1, 2, 3].map(|i| t.eq.get(i).map_or((1000.0, 0.0, 0.7), |b| (b.freq, b.gain_db, b.q)));
        let settings = actcraft_audio::render::TrackSettings {
            volume_db: t.volume_db,
            pan: t.pan,
            mute: t.mute,
            mute_groups: t.mute_groups,
            bus: t.bus,
            mono_to_stereo: t.mono_to_stereo,
            eq,
        };
        TrackParams::from_settings(&settings, self.sample_rate())
    }

    pub(crate) fn song_load(&mut self, id: u64) -> Option<SongLoad> {
        let song = self.show.song(id)?.clone();
        let mut slots = Vec::with_capacity(song.tracks.len());
        let mut problems = Vec::new();
        for t in &song.tracks {
            // Only decoded audio: files are decoded in the background before a song loads.
            let clip = if t.file.is_empty() { None } else { self.clips.get(&t.file).cloned() };
            if clip.is_none() && self.failed.contains(&t.file) {
                problems.push(t.name.clone());
            }
            slots.push(TrackSlot::new(clip, self.track_params(t)));
        }
        // Spoken cues play as one more track on the voice bus.
        if let Some((clip, track)) = self.voice_track(&song) {
            slots.push(TrackSlot::new(Some(clip), self.track_params(&track)));
        }
        for p in problems {
            self.say_error_with(
                "Track not loaded (its file could not be read): {file}",
                vec![MessageArg::plain("file", p)],
            );
        }
        let mut load =
            SongLoad::new(slots, song.start, song.end, BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor));
        load.loop_song = song.loop_song;
        // The spoken title plays when the song starts, with its count-in.
        load.announce =
            self.announce_track(&song).map(|(clip, track)| TrackSlot::new(Some(clip), self.track_params(&track)));
        load.fade_out = song.fade_out;
        load.song_gain = actcraft_audio::dsp::db_to_gain(song.volume_db);
        load.count_in_bars = song.count_in_bars;
        load.regions = region_table(&song);
        // Record the real length once audio is known.
        let len = load.length();
        if len > 0.0
            && let Some(s) = self.show.song_mut(id)
            && (s.duration - len).abs() > 0.01
        {
            s.duration = len;
        }
        Some(load)
    }

    /// Loads a song into the renderer, first decoding its files in the background. The window
    /// keeps responding meanwhile; the song loads (and plays, if asked) once its files are ready.
    pub(crate) fn load_into_renderer(&mut self, id: u64) {
        if self.audio.is_none() {
            return;
        }
        // Spoken cues are made in the background; the song reloads when they're ready.
        self.request_voices(id);
        let missing = self.missing_files(id);
        if missing.is_empty() {
            self.load_now(id);
        } else {
            self.pending_load = Some(id);
            self.play_after_load = None;
            self.request_clips(missing);
        }
    }

    fn load_now(&mut self, id: u64) {
        if self.pending_load == Some(id) {
            self.pending_load = None;
        }
        if let Some(load) = self.song_load(id)
            && self.send(Msg::Load(Box::new(load)))
        {
            self.loaded_song = Some(id);
            self.push_metronome();
        }
    }

    /// Files of a song not decoded yet (files that failed don't hold it up).
    fn missing_files(&self, id: u64) -> Vec<String> {
        let Some(song) = self.show.song(id) else { return Vec::new() };
        let mut out: Vec<String> = song
            .tracks
            .iter()
            .map(|t| t.file.clone())
            .filter(|f| !f.is_empty() && !self.clips.contains_key(f) && !self.failed.contains(f))
            .collect();
        out.dedup();
        out
    }

    /// Whether some of a song's files still need decoding.
    pub(crate) fn missing_files_pub(&self, id: u64) -> bool {
        !self.missing_files(id).is_empty()
    }

    /// Whether a song is waiting for its files to decode.
    pub fn is_loading(&self, id: u64) -> bool {
        self.pending_load == Some(id)
    }

    /// How many files are decoding right now.
    pub fn loading_count(&self) -> usize {
        self.loading.len()
    }

    /// Decodes files in the background (inline where there are no threads).
    pub fn request_clips(&mut self, paths: impl IntoIterator<Item = String>) {
        for path in paths {
            if path.is_empty() || self.clips.contains_key(&path) || self.loading.contains(&path) {
                continue;
            }
            self.failed.remove(&path);
            self.loading.insert(path.clone());
            let reader = self.services.file_reader();
            let tx = self.load_tx.clone();
            self.services.spawn(Box::new(move || {
                let result = guard(|| {
                    let bytes = reader(&path)?;
                    let ext = services::extension(&path);
                    let clip = actcraft_audio::clip::decode(&bytes, ext.as_deref())?;
                    let peaks = Arc::new(Peaks::of(&clip));
                    Ok((clip, peaks))
                })
                .unwrap_or_else(|| Err("decoding stopped unexpectedly".to_owned()));
                // The engine may be gone (app closing); then nobody needs the result.
                let _ = tx.send(LoadResult { path, result });
            }));
        }
        self.drain_loads();
    }

    /// Estimates the tempo of these songs once their first track has decoded.
    pub(crate) fn queue_tempo_detection(&mut self, songs: impl IntoIterator<Item = u64>) {
        let mut files = Vec::new();
        for id in songs {
            if let Some(f) = self.show.song(id).and_then(|s| s.tracks.first()).map(|t| t.file.clone()) {
                self.detect_bpm.insert(id);
                files.push(f);
            }
        }
        self.request_clips(files);
    }

    fn cache_clip(&mut self, path: String, clip: Arc<Clip>, peaks: Arc<Peaks>) {
        if self.clips.len() >= MAX_CACHED_CLIPS {
            let first = self.clips.keys().next().cloned();
            if let Some(k) = first {
                self.clips.remove(&k);
                self.peaks.remove(&k);
            }
        }
        self.clips.insert(path.clone(), clip);
        self.peaks.insert(path, peaks);
    }

    /// Takes finished background work: caches files, estimates tempos, loads a waiting song.
    fn drain_loads(&mut self) {
        while let Ok(done) = self.load_rx.try_recv() {
            self.loading.remove(&done.path);
            match done.result {
                Ok((clip, peaks)) => {
                    if self.stored_peaks(&done.path).is_none() {
                        self.store_peaks(&done.path, &peaks);
                    }
                    self.cache_clip(done.path, clip, peaks);
                }
                Err(e) => {
                    self.say_error(format!("{}: {e}", services::file_name(&done.path)));
                    self.failed.insert(done.path);
                }
            }
        }
        let waiting: Vec<u64> = self.detect_bpm.iter().copied().collect();
        for id in waiting {
            let file = self.show.song(id).and_then(|s| s.tracks.first()).map(|t| t.file.clone()).unwrap_or_default();
            if let Some(clip) = self.clips.get(&file).cloned() {
                self.detect_bpm.remove(&id);
                if let Some(beat) = actcraft_audio::tempo::analyse(&clip)
                    && let Some(s) = self.show.song_mut(id)
                {
                    // Tempo and the first beat together, so the click lands on the music.
                    s.bpm = beat.bpm;
                    s.beat_anchor = beat.anchor;
                    let title = s.title.clone();
                    self.mark_changed();
                    self.push_song_settings(id);
                    self.say_with(
                        "{title}: {bpm} BPM, first beat at {seconds} s (detected; adjust under the song details)",
                        vec![
                            MessageArg::plain("title", title),
                            MessageArg::plain("bpm", format!("{:.2}", beat.bpm)),
                            MessageArg::plain("seconds", format!("{:.3}", beat.anchor)),
                        ],
                    );
                }
            } else if file.is_empty() || self.failed.contains(&file) {
                self.detect_bpm.remove(&id);
            }
        }
        if let Some(id) = self.pending_load
            && self.missing_files(id).is_empty()
        {
            self.load_now(id);
            if let Some(count_in) = self.play_after_load.take() {
                let quantized = self.show.metronome.quantized_start;
                self.send(Msg::Play { count_in, quantized });
            }
        }
    }

    /// Plays the pending song as soon as it has loaded.
    pub(crate) fn play_when_loaded(&mut self, count_in: bool) {
        self.play_after_load = Some(count_in);
    }

    /// Re-sends one track's mix settings when its song is loaded.
    pub(crate) fn push_track(&mut self, song: u64, index: usize) {
        if self.loaded_song != Some(song) {
            return;
        }
        let Some(t) = self.show.song(song).and_then(|s| s.tracks.get(index)).cloned() else { return };
        let params = self.track_params(&t);
        if let Ok(i) = u8::try_from(index) {
            self.send(Msg::Track { index: i, params });
        }
    }

    pub(crate) fn push_bus(&mut self, index: usize) {
        let Some(b) = self.show.buses.get(index).cloned() else { return };
        let params = BusParams { gain: actcraft_audio::dsp::db_to_gain(b.volume_db), mute: b.mute, output: b.output };
        if let Ok(i) = u8::try_from(index) {
            self.send(Msg::Bus { index: i, params });
        }
    }

    pub(crate) fn push_metronome(&mut self) {
        let m = &self.show.metronome;
        // The playing (or chosen) song decides when its click sounds; without a song the show's
        // own mode applies. The toolbar switch (`enabled`) turns every click on or off.
        let song_click = self.loaded_song.or(self.current_song).and_then(|id| self.show.song(id)).map(|s| s.click);
        let (enabled, mode) = match song_click {
            Some(SongClick::Off) => (false, ClickMode::Audible),
            Some(SongClick::WholeSong) => (m.enabled, ClickMode::Audible),
            Some(SongClick::CountInOnly) => (m.enabled, ClickMode::CountInOnly),
            Some(SongClick::VisualOnly) => (m.enabled, ClickMode::VisualOnly),
            None => (
                m.enabled,
                match m.mode {
                    MetronomeMode::Audible => ClickMode::Audible,
                    MetronomeMode::CountInOnly => ClickMode::CountInOnly,
                    MetronomeMode::VisualOnly => ClickMode::VisualOnly,
                },
            ),
        };
        let params = MetronomeParams {
            enabled,
            mode,
            gain: actcraft_audio::dsp::db_to_gain(m.volume_db),
            bus: m.bus,
            latency: m.latency_offset_ms / 1000.0,
            free_run: m.run_while_stopped,
        };
        self.send(Msg::Metronome(params));
    }

    pub(crate) fn push_mute_groups(&mut self) {
        let mask = self
            .show
            .mute_groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.active)
            .fold(0u8, |m, (i, _)| m | (1u8 << i.min(7)));
        self.send(Msg::MuteGroups(mask));
    }

    pub(crate) fn push_all_mixer(&mut self) {
        for b in 0..self.show.buses.len() {
            self.push_bus(b);
        }
        let master = actcraft_audio::dsp::db_to_gain(self.show.mixer.master_volume_db);
        self.send(Msg::MasterGain(master));
        self.push_metronome();
        self.push_mute_groups();
    }

    /// Sends the current song's tempo, meter, anchor and loop flag after they change.
    pub(crate) fn push_song_settings(&mut self, id: u64) {
        if self.loaded_song != Some(id) {
            return;
        }
        let Some(s) = self.show.song(id) else { return };
        let clock = BeatClock::new(s.bpm, s.beats_per_bar, s.beat_anchor);
        let (loop_song, gain, count_in) = (s.loop_song, actcraft_audio::dsp::db_to_gain(s.volume_db), s.count_in_bars);
        self.send(Msg::Clock(clock));
        self.send(Msg::Loop(loop_song));
        self.send(Msg::SongGain(gain));
        self.send(Msg::CountIn(count_in));
        self.push_metronome();
    }

    /// Sends a song's regions to the renderer when it's loaded.
    pub(crate) fn push_regions(&mut self, id: u64) {
        if self.loaded_song != Some(id) {
            return;
        }
        if let Some(s) = self.show.song(id) {
            let table = Box::new(region_table(s));
            self.send(Msg::Regions(table));
        }
        // Region words or times may have changed: the voice track follows.
        self.voices_changed(id);
    }

    pub(crate) fn send_msg(&mut self, msg: Msg) -> bool {
        self.send(msg)
    }

    pub(crate) fn loaded_song_pub(&self) -> Option<u64> {
        self.loaded_song
    }

    /// After a structural change (tracks added or removed, files changed), reload the song.
    pub(crate) fn reload_if_loaded(&mut self, id: u64) {
        if self.loaded_song == Some(id) && !self.status.playing {
            self.load_into_renderer(id);
        }
    }

    pub(crate) fn mark_changed(&mut self) {
        self.dirty = true;
        self.edits = self.edits.wrapping_add(1);
    }

    pub(crate) fn loaded_song(&self) -> Option<u64> {
        self.loaded_song
    }

    pub(crate) fn mark_loaded(&mut self, id: u64) {
        self.loaded_song = Some(id);
    }

    pub(crate) fn forget_audio_state(&mut self) {
        self.loaded_song = None;
        self.pending_load = None;
        self.play_after_load = None;
        self.detect_bpm.clear();
        self.send(Msg::Unload);
    }
}

#[cfg(test)]
mod tests;
