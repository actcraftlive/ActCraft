use serde_json::{Value, json};

use super::*;

fn engine() -> Engine {
    Engine::new(Box::new(MemoryServices::default()))
}

/// Params a hostile or careless caller might send.
fn hostile_params() -> Vec<Value> {
    // Longer than any text cap (4096 bytes) without making the sweep slow.
    let long = "x".repeat(5_000);
    let mut deep = json!(1);
    for _ in 0..100 {
        deep = json!([deep]);
    }
    let mut out = vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!("text"),
        json!([]),
        json!({}),
        deep.clone(),
        json!(long.clone()),
    ];
    let keys = [
        "song",
        "track",
        "playlist",
        "set",
        "index",
        "indexes",
        "songs",
        "paths",
        "path",
        "name",
        "title",
        "bpm",
        "db",
        "volume_db",
        "pan",
        "bus",
        "group",
        "seconds",
        "query",
        "band",
        "freq",
        "gain_db",
        "q",
        "columns",
        "from",
        "to",
        "date",
        "mode",
        "rules",
        "end",
        "start",
        "member",
        "confirm",
        "discard",
        "colour",
        "file",
        "keywords",
        "ms",
        "output",
        "label",
        "active",
        "by",
        "format",
        "count_in",
        "year",
        "lyrics",
    ];
    let values = [
        json!(null),
        json!(-1),
        json!(0),
        json!(1),
        json!(i64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(1e308),
        json!(-1e308),
        json!(0.5),
        json!(""),
        json!(long),
        json!("((((((((((("),
        json!("%x%"),
        json!(true),
        json!([1, 2, 3]),
        json!([-1, "a", null]),
        json!({"a": 1}),
        deep,
    ];
    for k in keys {
        for v in &values {
            out.push(json!({ k: v.clone() }));
        }
    }
    // A few plausible-but-wrong combinations.
    out.push(json!({"song": 1, "track": 0, "band": 3, "gain_db": 24, "freq": 20000, "q": 18}));
    out.push(json!({"playlist": 0, "set": 0, "indexes": [0, 0, 0], "to": 0}));
    out.push(json!({"name": "x", "match_all": true, "rules": [{"field": "bpm", "op": ">", "value": "NaN"}]}));
    out
}

#[test]
fn every_command_survives_hostile_params() {
    for with_data in [false, true] {
        let mut e = engine();
        if with_data {
            let _ = e.run("song.add", &json!({"title": "A"}));
            let _ = e.run("track.add", &json!({"name": "Drums"}));
            let _ = e.run("playlist.create", &json!({"name": "P"}));
            let _ = e.run("playlist.add_songs", &json!({"playlist": 0, "songs": [1]}));
            let (ctl, _renderer) = actcraft_audio::link(48_000);
            e.attach_audio(ctl);
        }
        let ids: Vec<&str> = registry().iter().map(|c| c.spec.id).collect();
        let params = hostile_params();
        for id in ids {
            for prm in &params {
                let r = e.run(id, prm);
                assert!(!matches!(r, Err(CommandError::Internal(_))), "{id} panicked with {prm}");
            }
        }
    }
}

#[test]
fn every_command_registers_once() {
    assert_eq!(registry().len(), commands::all().len());
    for c in registry().iter() {
        assert!(!c.spec.label.is_empty(), "{} needs a label", c.spec.id);
    }
}

#[test]
fn library_playlist_queue_flow() {
    let mut e = engine();
    for (t, a, bpm) in [("Night Drive", "Ana", 128), ("Morning", "Ben", 90), ("Drive Home", "Ana", 140)] {
        assert!(e.run("song.add", &json!({"title": t, "artist": a, "bpm": bpm})).is_ok());
    }
    assert_eq!(e.run("library.search", &json!({"query": "artist:ana -home"})), Ok(json!([1])));
    assert!(e.run("playlist.create", &json!({"name": "Gig"})).is_ok());
    assert!(e.run("playlist.add_set", &json!({"playlist": 0})).is_ok());
    assert!(e.run("playlist.add_songs", &json!({"playlist": 0, "set": 0, "songs": [1, 2]})).is_ok());
    assert!(e.run("playlist.add_songs", &json!({"playlist": 0, "set": 1, "songs": [3]})).is_ok());
    assert!(e.run("playlist.move_songs", &json!({"playlist": 0, "set": 0, "indexes": [1], "to": 0})).is_ok());
    assert_eq!(e.run("playlist.songs", &json!({"playlist": 0})), Ok(json!([2, 1, 3])));
    // The sorted view leaves the set as it was.
    assert_eq!(
        e.run("playlist.sorted_view", &json!({"playlist": 0, "by": "bpm", "descending": true})),
        Ok(json!([1, 2]))
    );
    assert_eq!(e.run("playlist.songs", &json!({"playlist": 0})), Ok(json!([2, 1, 3])));
    assert!(e.run("queue.add_set", &json!({"playlist": 0, "set": 1})).is_ok());
    assert_eq!(e.run("queue.list", &Value::Null), Ok(json!([3])));
    assert!(e.run("playlist.delete", &json!({"playlist": 0})).is_err(), "delete needs confirmation");
    assert!(e.run("playlist.delete", &json!({"playlist": 0, "confirm": true})).is_ok());
    assert!(
        e.run(
            "playlist.create_smart",
            &json!({"name": "Fast", "match_all": true, "rules": [{"field": "bpm", "op": ">", "value": "120"}]})
        )
        .is_ok()
    );
    assert_eq!(e.run("playlist.songs", &json!({"playlist": 0})), Ok(json!([1, 3])));
}

#[test]
fn track_commands_take_an_explicit_song_without_a_current_one() {
    // Regression: track commands were disabled unless a song was current, so scripts that name
    // the song (CLI, control channel) could not add tracks after opening a show.
    let mut e = engine();
    assert!(e.run("song.add", &json!({"title": "A"})).is_ok());
    e.current_song = None;
    assert!(e.run("track.add", &json!({"song": 1, "name": "Drums"})).is_ok());
    assert!(e.run("track.set", &json!({"song": 1, "track": 0, "volume_db": -3})).is_ok());
    assert!(e.run("track.add", &json!({"name": "no song chosen"})).is_err());
}

#[test]
fn show_saves_and_reopens() {
    let mut e = engine();
    assert!(e.run("song.add", &json!({"title": "Keep me", "keywords": ["Live"]})).is_ok());
    assert!(e.run("show.save", &json!({"path": "a.actcraft"})).is_ok());
    assert!(!e.dirty);
    assert!(e.run("song.add", &json!({"title": "Second"})).is_ok());
    assert!(e.run("show.open", &json!({"path": "a.actcraft"})).is_err(), "unsaved changes are protected");
    assert!(e.run("show.open", &json!({"path": "a.actcraft", "discard": true})).is_ok());
    assert_eq!(e.show.songs.len(), 1);
    assert_eq!(e.show.songs.first().map(|s| s.keywords.clone()), Some(vec!["Live".to_owned()]));
}

/// Services whose background jobs wait until the test runs them, like a slow decode.
struct DeferredServices {
    inner: MemoryServices,
    jobs: std::sync::Arc<std::sync::Mutex<Vec<services::Job>>>,
}

impl Services for DeferredServices {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.inner.read_file(path)
    }
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.inner.write_file(path, bytes)
    }
    fn now(&self) -> chrono::DateTime<chrono::FixedOffset> {
        self.inner.now()
    }
    fn file_reader(&self) -> services::FileReader {
        self.inner.file_reader()
    }
    fn spawn(&self, job: services::Job) {
        if let Ok(mut q) = self.jobs.lock() {
            q.push(job);
        }
    }
}

fn tone_wav(frames: u32) -> Vec<u8> {
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(frames * 2).to_le_bytes());
    for i in 0..frames {
        let v: i16 = if i % 20 < 10 { 8000 } else { -8000 };
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav
}

#[test]
fn import_and_play_never_wait_for_decoding() {
    // Regression: importing and loading decoded audio on the window's thread, so the window
    // stopped responding until a whole song was decoded.
    let jobs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut inner = MemoryServices::default();
    inner.files.insert("tone.wav".into(), tone_wav(8000));
    let mut e = Engine::new(Box::new(DeferredServices { inner, jobs: std::sync::Arc::clone(&jobs) }));
    let (ctl, mut renderer) = actcraft_audio::link(48_000);
    e.attach_audio(ctl);
    assert!(e.run("song.import_files", &json!({"paths": ["tone.wav"]})).is_ok());
    assert_eq!(e.loading_count(), 1, "decoding runs in the background");
    let started = e.run("transport.play", &json!({"count_in": false}));
    assert_eq!(started.ok().and_then(|v| v.get("loading").cloned()), Some(json!(true)));
    // The decode finishes later; playback then starts by itself.
    let pending: Vec<services::Job> = jobs.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
    for job in pending {
        job();
    }
    e.tick();
    let mut buf = vec![0.0f32; 2 * 4800];
    renderer.process(&mut buf, 2);
    e.tick();
    assert!(e.status.playing);
    assert_eq!(e.loading_count(), 0);
}

#[test]
fn playback_runs_through_the_renderer() {
    let mut e = engine();
    // A one-second mono WAV written by hand (PCM 16-bit, 8 kHz).
    let frames: u32 = 8000;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(frames * 2).to_le_bytes());
    for i in 0..frames {
        let v: i16 = if i % 20 < 10 { 8000 } else { -8000 };
        wav.extend_from_slice(&v.to_le_bytes());
    }
    let mut services = MemoryServices::default();
    services.files.insert("tone.wav".into(), wav);
    let mut e2 = Engine::new(Box::new(services));
    std::mem::swap(&mut e, &mut e2);
    assert!(e.run("song.import_files", &json!({"paths": ["tone.wav"]})).is_ok());
    let (ctl, mut renderer) = actcraft_audio::link(48_000);
    e.attach_audio(ctl);
    assert!(e.run("transport.play", &json!({"count_in": false})).is_ok());
    let mut buf = vec![0.0f32; 2 * 4800];
    renderer.process(&mut buf, 2);
    e.tick();
    assert!(e.status.playing);
    assert!(buf.iter().any(|s| s.abs() > 0.05), "audio reaches the output");
    for _ in 0..20 {
        renderer.process(&mut buf, 2);
    }
    e.tick();
    assert!(!e.status.playing, "stops at the song end");
    assert_eq!(e.show.history.len(), 1);
}

#[test]
fn message_templates_fill_once_and_keep_unknown_braces() {
    assert_eq!(crate::fill("Loaded {title}", &[("title", "Night Drive")]), "Loaded Night Drive");
    // A value is never read again, so braces in it stay as they are.
    assert_eq!(crate::fill("{a} and {b}", &[("a", "{b}"), ("b", "x")]), "{b} and x");
    assert_eq!(crate::fill("{unknown} {", &[("a", "1")]), "{unknown} {");
    let mut e = engine();
    e.say_with("Opened show with {count} songs", vec![crate::MessageArg::plain("count", 2)]);
    let m = e.messages.back().cloned();
    assert_eq!(m.as_ref().map(|m| m.text.as_str()), Some("Opened show with 2 songs"));
    assert_eq!(m.as_ref().map(|m| m.template.as_str()), Some("Opened show with {count} songs"));
}

/// A small DOCX package: two paragraphs and a one-row table, as a word processor saves them.
fn docx_with(paragraphs: &[&str]) -> Vec<u8> {
    let body: String =
        paragraphs.iter().map(|p| format!("<w:p><w:r><w:t xml:space=\"preserve\">{p}</w:t></w:r></w:p>")).collect();
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:tbl><w:tr><w:tc><w:p><w:r><w:t>left</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>right</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
    );
    let types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut zip = craft_zip::Writer::new();
    for (name, data) in
        [("[Content_Types].xml", types), ("_rels/.rels", rels), ("word/document.xml", document.as_str())]
    {
        let _ = zip.add(name, data.as_bytes(), craft_zip::Compression::Deflated);
    }
    zip.finish().unwrap_or_default()
}

#[test]
fn lyrics_import_reads_the_text_of_a_docx_document() {
    let mut files = MemoryServices::default();
    files.files.insert("/gig/song.docx".into(), docx_with(&["[Verse]", "Twinkle [C]twinkle little star"]));
    files.files.insert("/gig/broken.docx".into(), b"PK\x03\x04 not a document".to_vec());
    let mut e = Engine::new(Box::new(files));
    let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
    let r = e.run("lyrics.import", &json!({"song": id, "path": "/gig/song.docx"}));
    assert!(r.is_ok(), "{r:?}");
    let lyrics = e.show.song(id).map(|s| s.lyrics.clone()).unwrap_or_default();
    assert!(lyrics.contains("[Verse]\nTwinkle [C]twinkle little star"), "{lyrics:?}");
    assert!(lyrics.contains("left right"), "table cells read as one line: {lyrics:?}");
    // A file that only looks like one is refused and leaves the lyrics as they were.
    assert!(e.run("lyrics.import", &json!({"song": id, "path": "/gig/broken.docx"})).is_err());
    assert_eq!(e.show.song(id).map(|s| s.lyrics.clone()), Some(lyrics));
}

#[test]
fn lyrics_replace_changes_every_match_and_marks_the_show() {
    let mut e = engine();
    let id = e.run("song.add", &json!({"title": "A"})).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
    assert!(e.run("song.set", &json!({"song": id, "lyrics": "The [Am]road\nthe Road"})).is_ok());
    e.dirty = false;
    let r = e.run("lyrics.replace", &json!({"song": id, "find": "road", "replace": "way"}));
    assert_eq!(r.ok().map(|v| v["replaced"].clone()), Some(json!(2)));
    assert_eq!(e.show.song(id).map(|s| s.lyrics.as_str()), Some("The [Am]way\nthe way"));
    assert!(e.dirty);
    let r = e.run("lyrics.replace", &json!({"song": id, "find": "WAY", "replace": "x", "match_case": true}));
    assert_eq!(r.ok().map(|v| v["replaced"].clone()), Some(json!(0)));
    assert!(e.run("lyrics.replace", &json!({"song": id, "find": ""})).is_err());
}

#[test]
fn a_show_package_carries_its_files_and_opens_elsewhere() {
    let mut files = MemoryServices::default();
    files.files.insert("/gig/drums.wav".into(), tone_wav(800));
    files.files.insert("/gig/click.mid".into(), b"MThd".to_vec());
    let mut e = Engine::new(Box::new(files));
    assert!(e.run("song.add", &json!({"title": "A"})).is_ok());
    assert!(e.run("track.add", &json!({"song": 1, "name": "Drums", "file": "/gig/drums.wav"})).is_ok());
    // The same file twice is packed once; a missing file is reported, not fatal.
    assert!(e.run("track.add", &json!({"song": 1, "name": "Again", "file": "/gig/drums.wav"})).is_ok());
    assert!(e.run("track.add", &json!({"song": 1, "name": "Gone", "file": "/gig/gone.wav"})).is_ok());
    assert!(e.run("song.set", &json!({"song": 1, "midi_file": "/gig/click.mid"})).is_ok());
    assert!(e.run("show.export_package", &json!({"path": "/gig/show.zip"})).is_err(), "needs the package extension");
    let r = e.run("show.export_package", &json!({"path": "/gig/Friday.actcraftpack"}));
    assert!(r.is_ok(), "{r:?}");
    let r = r.unwrap_or_default();
    assert_eq!(r["files"], json!(2));
    assert_eq!(r["missing"], json!(["/gig/gone.wav"]));
    // Opened on "another computer": the files land beside the package.
    let package = e.services().read_file("/gig/Friday.actcraftpack").unwrap_or_default();
    let mut other = MemoryServices::default();
    other.files.insert("/usb/Friday.actcraftpack".into(), package);
    let mut e2 = Engine::new(Box::new(other));
    let r = e2.run("show.open_package", &json!({"path": "/usb/Friday.actcraftpack"})).unwrap_or_default();
    assert_eq!(r["folder"], json!("/usb/Friday files"));
    let song = e2.show.song(1).cloned().unwrap_or_default();
    let tracks: Vec<&str> = song.tracks.iter().map(|t| t.file.as_str()).collect();
    assert_eq!(tracks, ["/usb/Friday files/0001-drums.wav", "/usb/Friday files/0001-drums.wav", "/gig/gone.wav"]);
    assert_eq!(song.midi_file.as_deref(), Some("/usb/Friday files/0002-click.mid"));
    assert_eq!(e2.services().read_file("/usb/Friday files/0001-drums.wav").ok(), Some(tone_wav(800)));
    assert!(e2.dirty, "the opened show isn't saved anywhere yet");
    // Not a package: a clear error, nothing changes.
    assert!(
        e2.run("show.open_package", &json!({"path": "/usb/Friday files/0002-click.mid", "discard": true})).is_err()
    );
    assert_eq!(e2.show.songs.len(), 1);
}

#[test]
fn waveforms_come_from_the_cache_the_next_time() {
    let mut files = MemoryServices::default();
    files.files.insert("/gig/song.wav".into(), tone_wav(8000));
    let mut e = Engine::new(Box::new(files));
    // First time: decoded (inline with memory services), then stored on "disk".
    let _ = e.peaks_for("/gig/song.wav");
    e.tick();
    let first = e.peaks_for("/gig/song.wav");
    assert!(first.is_some(), "decoded");
    let key = e.peaks_key("/gig/song.wav").unwrap_or_default();
    let stored = e.services().cache_get(&key);
    assert!(stored.is_some(), "kept for the next time");
    // A new run with the same cache: the waveform is there at once, nothing is decoded.
    let mut services = MemoryServices::default();
    services.files.insert("/gig/song.wav".into(), tone_wav(8000));
    services.cache.insert(key.clone(), stored.clone().unwrap_or_default());
    let mut e2 = Engine::new(Box::new(services));
    assert_eq!(e2.peaks_for("/gig/song.wav"), first);
    assert_eq!(e2.loading_count(), 0, "no decoding needed");
    // A changed file has another stamp, so its old waveform isn't used.
    let mut changed = MemoryServices::default();
    changed.files.insert("/gig/song.wav".into(), tone_wav(4000));
    changed.cache.insert(key, stored.unwrap_or_default());
    let e3 = Engine::new(Box::new(changed));
    assert!(e3.stored_peaks("/gig/song.wav").is_none());
}

/// Memory services with a "voice" that says every phrase as a short tone, counting the calls.
struct SpeakingServices {
    inner: MemoryServices,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Services for SpeakingServices {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        self.inner.read_file(path)
    }
    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.inner.write_file(path, bytes)
    }
    fn now(&self) -> chrono::DateTime<chrono::FixedOffset> {
        self.inner.now()
    }
    fn file_reader(&self) -> services::FileReader {
        self.inner.file_reader()
    }
    fn cache_get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.cache_get(key)
    }
    fn cache_put(&mut self, key: &str, bytes: &[u8]) {
        self.inner.cache_put(key, bytes);
    }
    fn speaker(&self) -> Option<services::Speaker> {
        let calls = std::sync::Arc::clone(&self.calls);
        Some(std::sync::Arc::new(move |_text: &str, _lang: &str| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(tone_wav(800))
        }))
    }
}

#[test]
fn spoken_cues_become_a_voice_track_and_are_spoken_once() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let services = SpeakingServices { inner: MemoryServices::default(), calls: std::sync::Arc::clone(&calls) };
    let mut e = Engine::new(Box::new(services));
    let (ctl, _renderer) = actcraft_audio::link(48_000);
    e.attach_audio(ctl);
    assert!(e.run("song.add", &json!({"title": "Asgard"})).is_ok());
    assert!(e.run("song.set", &json!({"song": 1, "announce_title": true})).is_ok());
    assert!(e.run("region.add", &json!({"song": 1, "start": 4.0, "end": 8.0, "voice": "Refrain"})).is_ok());
    assert!(e.run("region.add", &json!({"song": 1, "start": 9.0, "end": 12.0, "voice": "Refrain"})).is_ok());
    assert!(e.run("region.set", &json!({"song": 1, "region": 0, "voice": "x".repeat(300)})).is_err(), "too long");
    let r = e.run("voice.prepare", &json!({"song": 1}));
    assert_eq!(r.ok().map(|v| v["pending"].clone()), Some(json!(0)), "spoken inline here");
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2, "title and Refrain, each once");
    let song = e.show.song(1).cloned().unwrap_or_default();
    let voice = e.voice_track(&song);
    assert!(voice.is_some());
    assert_eq!(voice.map(|(_, t)| t.bus), Some(e.show.voice.bus));
    // Asking again speaks nothing new; switching voices off drops the track.
    let _ = e.run("voice.prepare", &json!({"song": 1}));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert!(e.run("voice.set", &json!({"enabled": false})).is_ok());
    assert!(e.voice_track(&song).is_none());
    assert!(e.run("voice.set", &json!({"language": "de; rm -rf"})).is_err());
}

#[test]
fn edited_cue_words_are_spoken_and_used_at_once() {
    // Regression: changing a part's words to "Thema" still said the old word, because nothing
    // spoke the new words or rebuilt the loaded song's voice track.
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let services = SpeakingServices { inner: MemoryServices::default(), calls: std::sync::Arc::clone(&calls) };
    let mut e = Engine::new(Box::new(services));
    let (ctl, _renderer) = actcraft_audio::link(48_000);
    e.attach_audio(ctl);
    assert!(e.run("song.add", &json!({"title": "Asgard"})).is_ok());
    assert!(e.run("region.add", &json!({"song": 1, "start": 4.0, "end": 8.0, "voice": "Zwischenspiel"})).is_ok());
    e.load_into_renderer(1);
    e.tick();
    assert_eq!(e.loaded_song_pub(), Some(1));
    let before = calls.load(std::sync::atomic::Ordering::Relaxed);
    assert!(e.run("region.set", &json!({"song": 1, "region": 0, "voice": "Thema"})).is_ok());
    e.tick();
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), before + 1, "the new words are spoken");
    let song = e.show.song(1).cloned().unwrap_or_default();
    assert_eq!(crate::voice::voice_cues(&song, e.show.voice.lead_beats).first().map(|c| c.1.as_str()), Some("Thema"));
    assert!(e.voice_track(&song).is_some(), "the voice track has the new words");
    assert_eq!(e.voice_reload, None, "the stopped song was loaded again");
    // The title is spoken as soon as it's switched on, for the start of the song.
    assert!(e.run("song.set", &json!({"song": 1, "announce_title": true})).is_ok());
    e.tick();
    assert!(e.announce_track(&song).is_none(), "uses the song as it was");
    let song = e.show.song(1).cloned().unwrap_or_default();
    assert!(e.announce_track(&song).is_some());
}

#[test]
fn pause_keeps_the_place_and_stop_goes_back_to_the_start() {
    // Regression: the only button was "Stop", which paused and never went back to the start.
    let mut files = MemoryServices::default();
    files.files.insert("song.wav".into(), tone_wav(96_000));
    let mut e = Engine::new(Box::new(files));
    let (ctl, mut renderer) = actcraft_audio::link(48_000);
    e.attach_audio(ctl);
    let _ = e.run("transport.set_play_guard", &json!({"ms": 0}));
    assert!(e.run("song.import_files", &json!({"paths": ["song.wav"]})).is_ok());
    e.tick();
    let mut buf = vec![0.0f32; 2 * 4800];
    let mut run = |e: &mut Engine, blocks: usize| {
        for _ in 0..blocks {
            renderer.process(&mut buf, 2);
        }
        e.tick();
    };
    assert!(e.run("transport.play", &json!({"count_in": false})).is_ok());
    run(&mut e, 5);
    assert!(e.status.playing);
    // Space: pause, the place stays.
    assert!(e.run("transport.toggle", &Value::Null).is_ok());
    run(&mut e, 1);
    let paused_at = e.status.time;
    assert!(!e.status.playing && paused_at > 0.3, "paused at {paused_at}");
    run(&mut e, 3);
    assert!((e.status.time - paused_at).abs() < 1e-6, "pause doesn't move");
    // Esc: stop, back to the start.
    assert!(e.run("transport.stop", &Value::Null).is_ok());
    run(&mut e, 1);
    assert!(!e.status.playing);
    assert!(e.status.time.abs() < 1e-6, "back at the start, not {}", e.status.time);
    // Listening to a cue: play from a place, also while already playing.
    assert!(e.run("transport.play", &json!({"count_in": false, "from": 1.0})).is_ok());
    run(&mut e, 1);
    assert!(e.status.playing && (e.status.time - 1.1).abs() < 0.05, "plays from 1 s: {}", e.status.time);
    assert!(e.run("transport.play", &json!({"count_in": false, "from": 0.5})).is_ok());
    run(&mut e, 1);
    assert!(e.status.playing && (e.status.time - 0.6).abs() < 0.05, "jumps to 0.5 s: {}", e.status.time);
}

/// A one-bar guitar score with fret numbers.
const SCORE: &str = r#"<?xml version="1.0"?>
<score-partwise><work><work-title>Night Drive</work-title></work>
<part-list><score-part id="P1"><part-name>Guitar</part-name></score-part></part-list>
<part id="P1"><measure number="1"><attributes><divisions>1</divisions><clef><sign>TAB</sign></clef></attributes>
<direction><sound tempo="100"/></direction>
<note><pitch><step>E</step><octave>2</octave></pitch><duration>4</duration><type>whole</type>
<notations><technical><string>6</string><fret>0</fret></technical></notations></note>
</measure></part></score-partwise>"#;

#[test]
fn a_package_can_hold_one_song_or_one_playlist() {
    let mut files = MemoryServices::default();
    for name in ["a", "b", "c"] {
        files.files.insert(format!("/gig/{name}.pdf"), b"%PDF-1.4".to_vec());
    }
    let mut e = Engine::new(Box::new(files));
    let mut ids = Vec::new();
    for name in ["a", "b", "c"] {
        let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
        assert!(e.run("song.set", &json!({"song": id, "title": name, "pdf_file": format!("/gig/{name}.pdf")})).is_ok());
        ids.push(id);
    }
    assert!(e.run("playlist.create", &json!({"name": "Encore"})).is_ok());
    assert!(e.run("playlist.add_songs", &json!({"playlist": 0, "songs": [ids[2], ids[0]]})).is_ok());
    let open = |e: &Engine, path: &str| {
        let bytes = e.services().read_file(path).unwrap_or_default();
        let mut other = MemoryServices::default();
        other.files.insert("/usb/p.actcraftpack".into(), bytes);
        let mut e2 = Engine::new(Box::new(other));
        assert!(e2.run("show.open_package", &json!({"path": "/usb/p.actcraftpack"})).is_ok());
        e2
    };
    // One song: only it and its file, no playlists.
    let r = e.run("show.export_package", &json!({"path": "/gig/one.actcraftpack", "song": ids[1]}));
    assert_eq!(r.ok().map(|v| v["files"].clone()), Some(json!(1)));
    let one = open(&e, "/gig/one.actcraftpack");
    assert_eq!(one.show.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), vec!["b"]);
    assert!(one.show.playlists.is_empty());
    // One playlist: it and its songs.
    let r = e.run("show.export_package", &json!({"path": "/gig/encore.actcraftpack", "playlist": 0}));
    assert_eq!(r.ok().map(|v| v["files"].clone()), Some(json!(2)));
    let encore = open(&e, "/gig/encore.actcraftpack");
    let mut titles: Vec<&str> = encore.show.songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, vec!["a", "c"]);
    assert_eq!(encore.show.playlists.len(), 1);
    // Both at once, or ones that don't exist, are refused.
    for bad in [
        json!({"path": "/gig/x.actcraftpack", "song": ids[0], "playlist": 0}),
        json!({"path": "/gig/x.actcraftpack", "song": 999}),
        json!({"path": "/gig/x.actcraftpack", "playlist": 5}),
    ] {
        assert!(e.run("show.export_package", &bad).is_err(), "{bad}");
    }
    assert!(e.services().read_file("/gig/x.actcraftpack").is_err(), "nothing written");
    // The menu's commands take the chosen song and the open playlist.
    assert!(e.run("song.load", &json!({"song": ids[2]})).is_ok());
    assert!(e.enabled("song.export_package"));
    let r = e.run("song.export_package", &json!({"path": "/gig/c.actcraftpack"}));
    assert_eq!(r.ok().map(|v| v["files"].clone()), Some(json!(1)));
    assert_eq!(
        open(&e, "/gig/c.actcraftpack").show.songs.iter().map(|s| s.title.clone()).collect::<Vec<_>>(),
        vec!["c"]
    );
    assert!(!e.enabled("playlist.export_package"), "no playlist open yet");
    assert!(e.run("playlist.export_package", &json!({"path": "/gig/p.actcraftpack"})).is_err());
    assert!(e.run("playlist.select", &json!({"playlist": 0})).is_ok());
    assert!(e.enabled("playlist.export_package"));
    assert!(e.run("playlist.export_package", &json!({"path": "/gig/p.actcraftpack"})).is_ok());
    // The show in the window is untouched.
    assert_eq!(e.show.songs.len(), 3);
}

#[test]
fn pdf_marks_are_added_checked_removed_and_saved() {
    let mut e = Engine::new(Box::new(MemoryServices::default()));
    let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
    let stroke = json!({"song": id, "page": 1, "points": [[0.1, 0.2], [0.3, 0.4], [0.5, 0.4]]});
    assert_eq!(e.run("pdf.mark", &stroke).ok(), Some(json!({"mark": 0})));
    let r = e.run(
        "pdf.mark",
        &json!({"song": id, "page": 0, "kind": "highlight", "points": [[0.1, 0.1], [0.9, 0.2]], "colour": "#f2b43c"}),
    );
    assert_eq!(r.ok(), Some(json!({"mark": 1})));
    let note = json!({"song": id, "page": 1, "kind": "note", "points": [[0.5, 0.5]], "text": " Capo 2 "});
    assert_eq!(e.run("pdf.mark", &note).ok(), Some(json!({"mark": 2})));
    // Refused, changing nothing: outside the page, too few points, no words, a bad colour or kind.
    for bad in [
        json!({"song": id, "page": 0, "points": [[1.5, 0.2], [0.3, 0.4]]}),
        json!({"song": id, "page": 0, "points": [[0.1, 0.2]]}),
        json!({"song": id, "page": 0, "kind": "highlight", "points": [[0.1, 0.2], [0.2, 0.3], [0.3, 0.4]]}),
        json!({"song": id, "page": 0, "kind": "note", "points": [[0.1, 0.2]]}),
        json!({"song": id, "page": 0, "points": [[0.1, 0.2], [0.3, 0.4]], "colour": "red"}),
        json!({"song": id, "page": 0, "kind": "circle", "points": [[0.1, 0.2], [0.3, 0.4]]}),
        json!({"song": id, "page": -1, "points": [[0.1, 0.2], [0.3, 0.4]]}),
    ] {
        assert!(e.run("pdf.mark", &bad).is_err(), "{bad}");
    }
    let song = e.show.song(id).cloned().unwrap_or_default();
    assert_eq!(song.pdf_marks.len(), 3);
    assert_eq!(song.pdf_marks[2].text, "Capo 2");
    assert_eq!(song.pdf_marks[1].colour, "#f2b43c");
    // The marks go into the show file and come back.
    let saved = e.show.to_bytes();
    let back = actcraft_show::Show::from_bytes(&saved).ok().and_then(|s| s.song(id).cloned()).unwrap_or_default();
    assert_eq!(back.pdf_marks, song.pdf_marks);
    // Removing one, then a page's marks, then all; clearing asks for confirmation.
    assert!(e.run("pdf.unmark", &json!({"song": id, "mark": 0})).is_ok());
    assert!(e.run("pdf.unmark", &json!({"song": id, "mark": 9})).is_err());
    assert!(e.run("pdf.clear_marks", &json!({"song": id, "page": 1})).is_err(), "needs confirm");
    assert_eq!(
        e.run("pdf.clear_marks", &json!({"song": id, "page": 1, "confirm": true})).ok(),
        Some(json!({"removed": 1}))
    );
    assert_eq!(e.show.song(id).map(|s| s.pdf_marks.len()), Some(1));
    assert_eq!(e.run("pdf.clear_marks", &json!({"song": id, "confirm": true})).ok(), Some(json!({"removed": 1})));
    assert_eq!(e.show.song(id).map(|s| s.pdf_marks.len()), Some(0));
}

#[test]
fn notation_attaches_reads_and_travels_in_packages() {
    let mut files = MemoryServices::default();
    files.files.insert("/gig/night.musicxml".into(), SCORE.as_bytes().to_vec());
    files.files.insert("/gig/notes.xml".into(), b"<html/>".to_vec());
    files.files.insert("/gig/notes.txt".into(), SCORE.as_bytes().to_vec());
    let mut e = Engine::new(Box::new(files));
    let id = e.run("song.add", &Value::Null).ok().and_then(|v| v["song"].as_u64()).unwrap_or(0);
    assert!(e.run("notation.read", &json!({"song": id})).is_err(), "nothing attached yet");
    // Files that aren't scores are refused and change nothing.
    assert!(e.run("notation.attach", &json!({"song": id, "path": "/gig/notes.txt"})).is_err());
    assert!(e.run("notation.attach", &json!({"song": id, "path": "/gig/notes.xml"})).is_err());
    assert!(e.run("notation.attach", &json!({"song": id, "path": "/gig/missing.mxl"})).is_err());
    assert_eq!(e.show.song(id).and_then(|s| s.notation_file.clone()), None);
    e.dirty = false;
    let r = e.run("notation.attach", &json!({"song": id, "path": "/gig/night.musicxml"}));
    assert!(r.is_ok(), "{r:?}");
    let r = r.unwrap_or_default();
    assert_eq!(r["title"], json!("Night Drive"));
    assert_eq!(r["tempo"], json!(100.0));
    assert_eq!(
        r["parts"][0],
        json!({"name": "Guitar", "bars": 1, "played_bars": 1, "notes": 1, "tablature": true, "strings": [64, 59, 55, 50, 45, 40], "capo": 0})
    );
    assert!(e.dirty);
    let song = e.show.song(id).cloned().unwrap_or_default();
    assert_eq!(song.notation_file.as_deref(), Some("/gig/night.musicxml"));
    assert_eq!(song.title, "Night Drive", "a new song takes the score's title");
    assert_eq!(e.run("notation.read", &Value::Null).ok().map(|v| v["parts"][0]["bars"].clone()), Some(json!(1)));
    // song.set checks the type, and null removes it.
    assert!(e.run("song.set", &json!({"song": id, "notation_file": "/gig/a.pdf"})).is_err());
    assert!(e.run("song.set", &json!({"song": id, "notation_file": "/gig/b.mxl"})).is_ok());
    assert!(e.run("song.set", &json!({"song": id, "notation_file": null})).is_ok());
    assert_eq!(e.show.song(id).and_then(|s| s.notation_file.clone()), None);
    // Where the score's bar 1 sits on the song's bars: 1 by default, 0 or less for a pickup.
    assert_eq!(e.show.song(id).map(|s| s.score_first_bar), Some(1));
    assert!(e.run("song.set", &json!({"song": id, "score_first_bar": 3})).is_ok());
    assert!(e.run("song.set", &json!({"song": id, "score_first_bar": -1})).is_ok());
    assert_eq!(e.show.song(id).map(|s| s.score_first_bar), Some(-1));
    assert!(e.run("song.set", &json!({"song": id, "score_first_bar": 100_000})).is_err());
    assert!(e.run("song.set", &json!({"song": id, "score_first_bar": "two"})).is_err());
    assert_eq!(e.show.song(id).map(|s| s.score_first_bar), Some(-1), "refused values change nothing");
    // In a package the score goes along and is found again after unpacking.
    assert!(e.run("song.set", &json!({"song": id, "notation_file": "/gig/night.musicxml"})).is_ok());
    assert!(e.run("show.export_package", &json!({"path": "/gig/Night.actcraftpack"})).is_ok());
    let package = e.services().read_file("/gig/Night.actcraftpack").unwrap_or_default();
    let mut other = MemoryServices::default();
    other.files.insert("/usb/Night.actcraftpack".into(), package);
    let mut e2 = Engine::new(Box::new(other));
    assert!(e2.run("show.open_package", &json!({"path": "/usb/Night.actcraftpack"})).is_ok());
    let moved = e2.show.song(id).and_then(|s| s.notation_file.clone()).unwrap_or_default();
    assert!(moved.starts_with("/usb/Night files/"), "{moved}");
    assert_eq!(
        e2.run("notation.read", &json!({"song": id})).ok().map(|v| v["title"].clone()),
        Some(json!("Night Drive"))
    );
}
