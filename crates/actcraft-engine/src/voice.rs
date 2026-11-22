//! Spoken cues: a song's title when it starts and a region's words shortly before it begins
//! ("Chorus", "End 3 2 1"), made with the computer's own speech.
//!
//! Each phrase is spoken once in the background and kept in ActCraft's cache; a song's phrases
//! are then laid out on one extra track at their times, which plays on the voice bus (usually the
//! in-ear mix) like any other track.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use actcraft_audio::{BeatClock, Clip};
use actcraft_command::params as p;
use actcraft_command::{CommandError, CommandResult};
use actcraft_show::limits::MAX_VOICE_TEXT;
use actcraft_show::{Song, Track};
use serde_json::{Value, json};

use crate::{Engine, MessageArg, guard};

/// Longest voice track (a whole long song).
const MAX_VOICE_SECONDS: f64 = 3.0 * 3600.0;

/// A phrase spoken in the background: its cache key and the result.
struct Spoken {
    key: String,
    result: Result<(Arc<Clip>, Vec<u8>), String>,
}

/// Spoken phrases by cache key, and those still being spoken.
pub(crate) struct Voices {
    clips: BTreeMap<String, Arc<Clip>>,
    pending: BTreeSet<String>,
    failed: BTreeSet<String>,
    tx: Sender<Spoken>,
    rx: Receiver<Spoken>,
}

impl Default for Voices {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { clips: BTreeMap::new(), pending: BTreeSet::new(), failed: BTreeSet::new(), tx, rx }
    }
}

/// The cache key of a phrase in a language.
fn voice_key(language: &str, text: &str) -> String {
    format!("voice1|{language}|{text}")
}

/// The title to say when the song is started (with its count-in), if it's announced.
pub fn announced_title(song: &Song) -> Option<String> {
    let title = song.title.trim();
    (song.announce_title && !title.is_empty()).then(|| title.to_owned())
}

/// Every phrase a song needs: its announced title and its regions' words.
pub fn voice_texts(song: &Song, lead_beats: u8) -> Vec<String> {
    let mut out: Vec<String> = announced_title(song).into_iter().collect();
    out.extend(voice_cues(song, lead_beats).into_iter().map(|(_, t)| t));
    out
}

/// Where a song's region words are spoken: (song time, text), in time order, each
/// `lead_beats` before its region (never before the song's start). The title isn't here: it
/// plays when the song is started, with the count-in ([`announced_title`]).
pub fn voice_cues(song: &Song, lead_beats: u8) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let start = song.start.max(0.0);
    let clock = BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
    let lead = f64::from(lead_beats) * clock.beat_len();
    for r in &song.regions {
        let text = r.voice.trim();
        if !text.is_empty() {
            out.push(((r.start - lead).max(start), text.to_owned()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// One mono clip with each phrase at its time (phrases at another rate are resampled), or
/// `None` when there is nothing to lay out.
pub fn lay_out(cues: &[(f64, Arc<Clip>)]) -> Option<Arc<Clip>> {
    let rate = cues.first().map(|(_, c)| c.sample_rate.max(1))?;
    let rate_f = f64::from(rate);
    let end = cues
        .iter()
        .map(|(t, c)| t + c.frames() as f64 / f64::from(c.sample_rate.max(1)))
        .fold(0.0_f64, f64::max)
        .min(MAX_VOICE_SECONDS);
    let len = (end * rate_f).ceil() as usize;
    let mut data = vec![0.0_f32; len];
    for (t, clip) in cues {
        let chans = usize::from(clip.channels.max(1));
        let step = f64::from(clip.sample_rate.max(1)) / rate_f;
        let frames = clip.frames();
        let first = (t * rate_f).round().max(0.0) as usize;
        let mut src = 0.0_f64;
        let mut i = first;
        while (src as usize) < frames && i < len {
            let f = src as usize;
            let mono: f32 =
                (0..chans).map(|ch| clip.data.get(f * chans + ch).copied().unwrap_or(0.0)).sum::<f32>() / chans as f32;
            if let Some(slot) = data.get_mut(i) {
                *slot = (*slot + mono).clamp(-1.0, 1.0);
            }
            src += step;
            i += 1;
        }
    }
    Clip::from_planar(rate, &[data]).map(Arc::new)
}

impl Engine {
    /// Starts speaking a song's phrases that aren't ready yet (from the cache when they were
    /// spoken before). Returns how many are still being spoken.
    pub(crate) fn request_voices(&mut self, id: u64) -> usize {
        let Some(song) = self.show.song(id) else { return 0 };
        if !self.show.voice.enabled {
            return 0;
        }
        let language = self.show.voice.language.clone();
        let texts = voice_texts(song, self.show.voice.lead_beats);
        for text in texts {
            let key = voice_key(&language, &text);
            if self.voices.clips.contains_key(&key)
                || self.voices.pending.contains(&key)
                || self.voices.failed.contains(&key)
            {
                continue;
            }
            if let Some(bytes) = self.services.cache_get(&key)
                && let Ok(clip) = actcraft_audio::clip::decode(&bytes, None)
            {
                self.voices.clips.insert(key, clip);
                continue;
            }
            let Some(speaker) = self.services.speaker() else {
                self.voices.failed.insert(key);
                self.say_error("This computer has no speech for spoken cues");
                continue;
            };
            self.voices.pending.insert(key.clone());
            let tx = self.voices.tx.clone();
            let lang = language.clone();
            self.services.spawn(Box::new(move || {
                let result = guard(|| {
                    let bytes = speaker(&text, &lang)?;
                    let clip = actcraft_audio::clip::decode(&bytes, None)?;
                    Ok((clip, bytes))
                })
                .unwrap_or_else(|| Err("speaking stopped unexpectedly".to_owned()));
                let _ = tx.send(Spoken { key, result });
            }));
        }
        self.drain_voices();
        self.voices.pending.len()
    }

    /// Takes phrases spoken in the background; a stopped song that uses them is loaded again so
    /// its voice track has them.
    pub(crate) fn drain_voices(&mut self) {
        let mut got = false;
        while let Ok(done) = self.voices.rx.try_recv() {
            self.voices.pending.remove(&done.key);
            match done.result {
                Ok((clip, bytes)) => {
                    self.services.cache_put(&done.key, &bytes);
                    self.voices.clips.insert(done.key, clip);
                    got = true;
                }
                Err(e) => {
                    let text = done.key.splitn(3, '|').nth(2).unwrap_or("").to_owned();
                    self.voices.failed.insert(done.key);
                    self.say_error_with(
                        "Could not speak “{text}”: {error}",
                        vec![MessageArg::plain("text", text), MessageArg::plain("error", e)],
                    );
                }
            }
        }
        if got && let Some(id) = self.loaded_song_pub() {
            self.voice_reload = Some(id);
        }
        // A loaded song whose cues changed is loaded again once it's stopped and its phrases are
        // ready, so what plays is what the cues say.
        if let Some(id) = self.voice_reload
            && self.voices.pending.is_empty()
            && !self.status.playing
        {
            self.voice_reload = None;
            if self.loaded_song_pub() == Some(id) {
                self.load_into_renderer(id);
            }
        }
    }

    /// A song's spoken cues changed (words, title, timing): speak what's new and load the song
    /// again when it's stopped.
    pub(crate) fn voices_changed(&mut self, id: u64) {
        if self.loaded_song_pub() != Some(id) {
            return;
        }
        self.voice_reload = Some(id);
        self.request_voices(id);
    }

    /// The spoken title, played when the song is started (with its count-in).
    pub(crate) fn announce_track(&self, song: &Song) -> Option<(Arc<Clip>, Track)> {
        let v = &self.show.voice;
        if !v.enabled {
            return None;
        }
        let title = announced_title(song)?;
        let clip = self.voices.clips.get(&voice_key(&v.language, &title))?;
        Some((Arc::clone(clip), self.voice_bus_track()))
    }

    fn voice_bus_track(&self) -> Track {
        let v = &self.show.voice;
        Track { name: "Voice".to_owned(), bus: v.bus, volume_db: v.volume_db, mono_to_stereo: true, ..Track::default() }
    }

    /// The song's voice track, from the phrases ready now.
    pub(crate) fn voice_track(&self, song: &Song) -> Option<(Arc<Clip>, Track)> {
        let v = &self.show.voice;
        if !v.enabled {
            return None;
        }
        let cues: Vec<(f64, Arc<Clip>)> = voice_cues(song, v.lead_beats)
            .into_iter()
            .filter_map(|(t, text)| self.voices.clips.get(&voice_key(&v.language, &text)).map(|c| (t, Arc::clone(c))))
            .collect();
        let clip = lay_out(&cues)?;
        Some((clip, self.voice_bus_track()))
    }

    /// Phrases still being spoken.
    pub fn voices_pending(&self) -> usize {
        self.voices.pending.len()
    }
}

fn invalid(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidParams(msg.into())
}

/// Voice settings for the whole show.
pub fn voice_set(e: &mut Engine, prm: &Value) -> CommandResult {
    let mut next = e.show.voice.clone();
    if let Some(v) = p::opt_bool(prm, "enabled")? {
        next.enabled = v;
    }
    if let Some(v) = p::opt_i64_in(prm, "bus", 0, actcraft_show::limits::BUSES as i64 - 1)? {
        next.bus = u8::try_from(v).unwrap_or(0);
    }
    if let Some(v) = p::opt_f64_in(prm, "volume_db", -96.0, 12.0)? {
        next.volume_db = v;
    }
    if let Some(v) = p::opt_i64_in(prm, "lead_beats", 0, 32)? {
        next.lead_beats = u8::try_from(v).unwrap_or(4);
    }
    if let Some(v) = p::opt_str_max(prm, "language", 16)? {
        if !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(invalid("`language` must be a code such as de or en"));
        }
        next.language = v.to_owned();
    }
    e.show.voice = next;
    e.mark_changed();
    if let Some(id) = e.current_song {
        e.request_voices(id);
        e.voices_changed(id);
        e.drain_voices();
    }
    Ok(json!({"enabled": e.show.voice.enabled}))
}

/// Speaks a song's phrases now (they are also spoken when the song loads).
pub fn voice_prepare(e: &mut Engine, prm: &Value) -> CommandResult {
    let id = match p::opt_i64_in(prm, "song", 1, i64::MAX)? {
        Some(v) => u64::try_from(v).unwrap_or(0),
        None => e.current_song.ok_or_else(|| invalid("no song chosen"))?,
    };
    if e.show.song(id).is_none() {
        return Err(invalid("no such song"));
    }
    let pending = e.request_voices(id);
    Ok(json!({"pending": pending}))
}

/// Checks a region's spoken words.
pub fn check_voice_text(text: &str) -> Result<(), CommandError> {
    if text.len() > MAX_VOICE_TEXT {
        return Err(invalid(format!("spoken words are at most {MAX_VOICE_TEXT} bytes")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f64) -> Arc<Clip> {
        let n = (f64::from(rate) * secs) as usize;
        Arc::new(Clip { sample_rate: rate, channels: 1, data: vec![0.5; n] })
    }

    #[test]
    fn cues_come_before_their_regions_and_never_before_the_start() {
        let mut song = Song { title: "Asgard".into(), announce_title: true, bpm: 120.0, ..Song::default() };
        for (name, start, voice) in [("Verse", 1.0, "Strophe"), ("Chorus", 10.0, "Refrain"), ("Bridge", 20.0, "")] {
            song.regions.push(actcraft_show::Region {
                name: name.into(),
                start,
                end: start + 5.0,
                voice: voice.into(),
                ..Default::default()
            });
        }
        // Four beats at 120 BPM: two seconds early.
        let cues = voice_cues(&song, 4);
        assert_eq!(cues, vec![(0.0, "Strophe".to_owned()), (8.0, "Refrain".to_owned())]);
        // The title isn't placed in the song: it plays when the song is started.
        assert_eq!(announced_title(&song).as_deref(), Some("Asgard"));
        assert_eq!(voice_texts(&song, 4), ["Asgard", "Strophe", "Refrain"]);
        song.announce_title = false;
        assert_eq!(announced_title(&song), None);
        assert_eq!(voice_cues(&song, 0).first().map(|c| c.0), Some(1.0));
    }

    #[test]
    fn phrases_are_laid_out_at_their_times() {
        let track = lay_out(&[(0.5, tone(1000, 0.1)), (1.0, tone(2000, 0.1))]);
        assert!(track.is_some());
        let Some(track) = track else { return };
        assert_eq!(track.sample_rate, 1000);
        assert_eq!(track.frames(), 1100);
        let at = |i: usize| track.data.get(i).copied().unwrap_or(-1.0);
        assert_eq!(at(400), 0.0, "silence before the first phrase");
        assert!((at(550) - 0.5).abs() < 1e-6);
        assert_eq!(at(700), 0.0);
        assert!((at(1050) - 0.5).abs() < 1e-6, "the 2 kHz phrase is resampled into place");
        assert!(lay_out(&[]).is_none());
    }
}
