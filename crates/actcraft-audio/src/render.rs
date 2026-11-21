//! The renderer (audio callback side) and the controller (engine side).
//!
//! Rules inside [`Renderer::process`]: no allocation, no freeing, no locks, no blocking. Data that
//! must be freed goes back to the controller through the garbage ring.

use std::f32::consts::TAU;
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::clip::Clip;
use crate::clock::BeatClock;
use crate::dsp::{BAND_SHAPES, BiquadState, Coeffs, coeffs, db_to_gain, pan_mono, pan_stereo};
use crate::shared::StatusCell;

/// Tracks a song may have.
pub const MAX_TRACKS: usize = 64;
/// Output buses.
pub const BUSES: usize = 16;
/// Frames processed per inner block; device buffers of any size are split into these.
pub const MAX_BLOCK: usize = 512;
/// Messages the controller can queue before the renderer drains them.
const MSG_CAPACITY: usize = 4096;
/// Length of a gain ramp, frames; avoids clicks when a fader jumps.
const SMOOTH: f32 = 0.002;

/// Per-track mix settings, ready for the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackParams {
    /// Linear gain.
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    /// Bit n set: the track is in mute group n+1.
    pub mute_groups: u8,
    pub bus: u8,
    pub mono_to_stereo: bool,
    pub eq: [Coeffs; 4],
}

impl Default for TrackParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            mute: false,
            mute_groups: 0,
            bus: 0,
            mono_to_stereo: true,
            eq: [Coeffs::default(); 4],
        }
    }
}

/// A track's settings as the person sets them (dB, -1..1 pan, EQ as frequency, gain, Q).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackSettings {
    pub volume_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub mute_groups: u8,
    pub bus: u8,
    pub mono_to_stereo: bool,
    pub eq: [(f64, f64, f64); 4],
}

impl TrackParams {
    /// Builds params from user values, computing EQ coefficients for `sample_rate`.
    pub fn from_settings(s: &TrackSettings, sample_rate: f64) -> Self {
        let mut c = [Coeffs::default(); 4];
        for (i, (freq, gain, q)) in s.eq.iter().enumerate() {
            if let (Some(slot), Some(shape)) = (c.get_mut(i), BAND_SHAPES.get(i)) {
                *slot = coeffs(*shape, *freq, *gain, *q, sample_rate);
            }
        }
        Self {
            gain: db_to_gain(s.volume_db),
            pan: s.pan as f32,
            mute: s.mute,
            mute_groups: s.mute_groups,
            bus: s.bus.min((BUSES - 1) as u8),
            mono_to_stereo: s.mono_to_stereo,
            eq: c,
        }
    }
}

/// Bus settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusParams {
    pub gain: f32,
    pub mute: bool,
    /// First device channel of the bus's stereo pair.
    pub output: u16,
}

impl Default for BusParams {
    fn default() -> Self {
        Self { gain: 1.0, mute: false, output: 0 }
    }
}

/// How the click sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickMode {
    #[default]
    Audible,
    CountInOnly,
    VisualOnly,
}

/// Metronome settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetronomeParams {
    pub enabled: bool,
    pub mode: ClickMode,
    pub gain: f32,
    pub bus: u8,
    /// Seconds the click is moved earlier to make up for output latency.
    pub latency: f64,
    /// Click while stopped too (otherwise only with a playing song).
    pub free_run: bool,
}

impl Default for MetronomeParams {
    fn default() -> Self {
        Self { enabled: false, mode: ClickMode::Audible, gain: 0.5, bus: 1, latency: 0.0, free_run: false }
    }
}

/// One track inside a loaded song, with its runtime state.
#[derive(Debug, Clone)]
pub struct TrackSlot {
    pub clip: Option<Arc<Clip>>,
    pub params: TrackParams,
    cur_l: f32,
    cur_r: f32,
    eq_state: [[BiquadState; 4]; 2],
}

impl TrackSlot {
    pub fn new(clip: Option<Arc<Clip>>, params: TrackParams) -> Self {
        Self { clip, params, cur_l: 0.0, cur_r: 0.0, eq_state: [[BiquadState::default(); 4]; 2] }
    }
}

/// A song ready to play, built off the audio thread.
#[derive(Debug, Clone)]
pub struct SongLoad {
    pub tracks: Vec<TrackSlot>,
    pub start: f64,
    pub end: Option<f64>,
    pub loop_song: bool,
    pub fade_out: f64,
    pub song_gain: f32,
    pub clock: BeatClock,
    pub count_in_bars: u8,
    // Runtime: song time = origin + elapsed / sample_rate.
    origin: f64,
    elapsed: u64,
    /// Tracks are silent before this song time (during a count-in).
    audible_from: f64,
}

impl SongLoad {
    pub fn new(mut tracks: Vec<TrackSlot>, start: f64, end: Option<f64>, clock: BeatClock) -> Self {
        tracks.truncate(MAX_TRACKS);
        let start = if start.is_finite() { start.max(0.0) } else { 0.0 };
        Self {
            tracks,
            start,
            end: end.filter(|e| e.is_finite() && *e > start),
            loop_song: false,
            fade_out: 0.0,
            song_gain: 1.0,
            clock,
            count_in_bars: 0,
            origin: start,
            elapsed: 0,
            audible_from: f64::NEG_INFINITY,
        }
    }

    /// Length of the longest track, seconds.
    pub fn length(&self) -> f64 {
        self.tracks.iter().filter_map(|t| t.clip.as_ref()).map(|c| c.seconds()).fold(0.0, f64::max)
    }

    fn end_time(&self) -> f64 {
        self.end.unwrap_or_else(|| self.length())
    }

    fn time(&self, sr: f64) -> f64 {
        self.origin + self.elapsed as f64 / sr
    }

    fn set_time(&mut self, t: f64) {
        self.origin = if t.is_finite() { t } else { self.start };
        self.elapsed = 0;
    }
}

/// Engine → renderer messages.
#[derive(Debug)]
pub enum Msg {
    /// Replace the loaded song (stops playback). The old one comes back through the garbage ring.
    Load(Box<SongLoad>),
    /// Unload the song.
    Unload,
    /// Start. `count_in`: play the song's count-in first. `quantized`: wait for the next beat of a
    /// running metronome.
    Play {
        count_in: bool,
        quantized: bool,
    },
    Stop,
    Seek(f64),
    Track {
        index: u8,
        params: TrackParams,
    },
    Bus {
        index: u8,
        params: BusParams,
    },
    MasterGain(f32),
    SongGain(f32),
    MuteGroups(u8),
    Metronome(MetronomeParams),
    /// Tempo, meter and anchor of the loaded song.
    Clock(BeatClock),
    Loop(bool),
    /// Bars of count-in of the loaded song.
    CountIn(u8),
    /// Crossfade into another song over `seconds`. Ignored while a crossfade runs.
    Crossfade {
        next: Box<SongLoad>,
        seconds: f64,
    },
}

/// Renderer → engine: what to free off the audio thread.
#[derive(Debug)]
pub enum Garbage {
    Song(Box<SongLoad>),
}

/// Renderer → engine: playback state, sent after each device buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    pub loaded: bool,
    pub playing: bool,
    /// Song time, seconds (negative during a count-in before zero).
    pub time: f64,
    /// Beat position from the anchor.
    pub beat: f64,
    pub bar: i64,
    pub beat_in_bar: u8,
    pub counting_in: bool,
    pub crossfading: bool,
    /// Increments each time a song reaches its end without looping.
    pub ended_count: u32,
    /// Peak level per bus since the last status, linear.
    pub bus_peaks: [f32; BUSES],
    /// Frames rendered since start; for tests and diagnostics.
    pub frames: u64,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            loaded: false,
            playing: false,
            time: 0.0,
            beat: 0.0,
            bar: 0,
            beat_in_bar: 0,
            counting_in: false,
            crossfading: false,
            ended_count: 0,
            bus_peaks: [0.0; BUSES],
            frames: 0,
        }
    }
}

/// The engine's end of the link.
pub struct Controller {
    sample_rate: f64,
    tx: Producer<Msg>,
    status: Arc<StatusCell>,
    garbage_rx: Consumer<Garbage>,
    last: Status,
}

impl Controller {
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Queues a message. Returns false when the renderer is not keeping up (the message is
    /// dropped and the caller may retry).
    pub fn send(&mut self, msg: Msg) -> bool {
        self.tx.push(msg).is_ok()
    }

    /// Reads the newest status and frees what the renderer returned.
    pub fn poll(&mut self) -> Status {
        if let Some(s) = self.status.load() {
            self.last = s;
        }
        while let Ok(g) = self.garbage_rx.pop() {
            drop(g);
        }
        self.last
    }

    pub fn last_status(&self) -> Status {
        self.last
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Click {
    phase: f32,
    freq: f32,
    env: f32,
}

struct Fade {
    song: Box<SongLoad>,
    total: u64,
    done: u64,
}

/// The audio callback's end of the link.
pub struct Renderer {
    sr: f64,
    rx: Consumer<Msg>,
    status_cell: Arc<StatusCell>,
    garbage_tx: Producer<Garbage>,
    song: Option<Box<SongLoad>>,
    fade: Option<Fade>,
    playing: bool,
    buses: [BusParams; BUSES],
    bus_cur: [f32; BUSES],
    master: f32,
    master_cur: f32,
    mute_mask: u8,
    metronome: MetronomeParams,
    click: Click,
    last_click_beat: i64,
    /// Free-running clock for the metronome while stopped, seconds.
    free_time: f64,
    /// Frames until a quantized start fires.
    pending_start: Option<u64>,
    pending_count_in: bool,
    ended_count: u32,
    peaks: [f32; BUSES],
    frames: u64,
    bus_buf: Vec<f32>,
}

/// Makes a connected controller and renderer for a device running at `sample_rate`.
pub fn link(sample_rate: u32) -> (Controller, Renderer) {
    let sr = f64::from(sample_rate.clamp(8_000, 384_000));
    let (tx, rx) = RingBuffer::new(MSG_CAPACITY);
    let status_cell = Arc::new(StatusCell::default());
    let (garbage_tx, garbage_rx) = RingBuffer::new(MSG_CAPACITY);
    let controller =
        Controller { sample_rate: sr, tx, status: Arc::clone(&status_cell), garbage_rx, last: Status::default() };
    let renderer = Renderer {
        sr,
        rx,
        status_cell,
        garbage_tx,
        song: None,
        fade: None,
        playing: false,
        buses: [BusParams::default(); BUSES],
        bus_cur: [1.0; BUSES],
        master: 1.0,
        master_cur: 1.0,
        mute_mask: 0,
        metronome: MetronomeParams::default(),
        click: Click::default(),
        last_click_beat: i64::MIN,
        free_time: 0.0,
        pending_start: None,
        pending_count_in: false,
        ended_count: 0,
        peaks: [0.0; BUSES],
        frames: 0,
        bus_buf: vec![0.0; BUSES * MAX_BLOCK * 2],
    };
    (controller, renderer)
}

impl Renderer {
    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    /// Hands a song back for freeing. If the ring is full the box is leaked instead of freed here:
    /// a small leak is better than freeing memory on the audio thread.
    fn discard(&mut self, song: Box<SongLoad>) {
        if let Err(rtrb::PushError::Full(Garbage::Song(s))) = self.garbage_tx.push(Garbage::Song(song)) {
            std::mem::forget(s);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Load(song) => {
                self.playing = false;
                self.pending_start = None;
                if let Some(old) = self.song.replace(song) {
                    self.discard(old);
                }
                if let Some(f) = self.fade.take() {
                    self.discard(f.song);
                }
            }
            Msg::Unload => {
                self.playing = false;
                if let Some(old) = self.song.take() {
                    self.discard(old);
                }
            }
            Msg::Play { count_in, quantized } => {
                if self.song.is_none() || self.playing {
                    return;
                }
                let free_running = self.metronome.enabled && quantized;
                if free_running {
                    let clock = self.song.as_ref().map(|s| s.clock).unwrap_or_default();
                    let wait = clock.next_beat_time(self.free_time) - self.free_time;
                    self.pending_start = Some((wait.max(0.0) * self.sr) as u64);
                    self.pending_count_in = count_in;
                } else {
                    self.start(count_in);
                }
            }
            Msg::Stop => {
                self.playing = false;
                self.pending_start = None;
                if let Some(s) = &mut self.song {
                    s.audible_from = f64::NEG_INFINITY;
                    let t = s.time(self.sr).max(s.start);
                    s.set_time(t);
                }
            }
            Msg::Seek(t) => {
                if let Some(s) = &mut self.song {
                    let max = s.end_time().max(s.start);
                    s.set_time(if t.is_finite() { t.clamp(0.0, max) } else { s.start });
                    s.audible_from = f64::NEG_INFINITY;
                    self.last_click_beat = i64::MIN;
                }
            }
            Msg::Track { index, params } => {
                if let Some(t) = self.song.as_mut().and_then(|s| s.tracks.get_mut(usize::from(index))) {
                    t.params = params;
                }
            }
            Msg::Bus { index, params } => {
                if let Some(b) = self.buses.get_mut(usize::from(index)) {
                    *b = params;
                }
            }
            Msg::MasterGain(g) => self.master = sane_gain(g),
            Msg::SongGain(g) => {
                if let Some(s) = &mut self.song {
                    s.song_gain = sane_gain(g);
                }
            }
            Msg::MuteGroups(m) => self.mute_mask = m,
            Msg::Metronome(m) => self.metronome = m,
            Msg::Clock(c) => {
                if let Some(s) = &mut self.song {
                    s.clock = c;
                }
            }
            Msg::Loop(l) => {
                if let Some(s) = &mut self.song {
                    s.loop_song = l;
                }
            }
            Msg::CountIn(bars) => {
                if let Some(s) = &mut self.song {
                    s.count_in_bars = bars.min(8);
                }
            }
            Msg::Crossfade { next, seconds } => {
                if self.fade.is_some() || !self.playing {
                    // A crossfade is already running (or nothing plays): ignore the request.
                    self.discard(next);
                    return;
                }
                let total = (seconds.clamp(0.0, 30.0) * self.sr) as u64;
                let mut next = next;
                next.audible_from = f64::NEG_INFINITY;
                if let Some(old) = self.song.replace(next) {
                    if total == 0 {
                        self.discard(old);
                    } else {
                        self.fade = Some(Fade { song: old, total, done: 0 });
                    }
                }
            }
        }
    }

    fn start(&mut self, count_in: bool) {
        let sr = self.sr;
        if let Some(s) = &mut self.song {
            let mut from = s.time(sr);
            if from >= s.end_time() - 0.001 {
                from = s.start;
            }
            let latency = self.metronome.latency;
            let start_at = if count_in && s.count_in_bars > 0 {
                // The count-in ends on the song's next beat, so its last click leads straight
                // into the music's beat (the audio from `from` plays as a pickup).
                let first_beat = s.clock.next_beat_time(from);
                let t = first_beat - s.clock.bars_len(s.count_in_bars);
                s.audible_from = from;
                t
            } else {
                s.audible_from = f64::NEG_INFINITY;
                from
            };
            s.set_time(start_at);
            // A start exactly on a beat clicks that beat (it used to be skipped).
            let beat = s.clock.beat_at(start_at + latency);
            let nearest = beat.round();
            self.last_click_beat = if (beat - nearest).abs() < 1e-6 { nearest as i64 - 1 } else { beat.floor() as i64 };
            self.playing = true;
        }
    }

    /// Renders into an interleaved device buffer with `channels` channels.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        while let Ok(msg) = self.rx.pop() {
            self.handle(msg);
        }
        for s in out.iter_mut() {
            *s = 0.0;
        }
        if channels == 0 {
            return;
        }
        let total_frames = out.len() / channels;
        let mut done = 0;
        while done < total_frames {
            let n = (total_frames - done).min(MAX_BLOCK);
            let start = done * channels;
            let end = (done + n) * channels;
            if let Some(chunk) = out.get_mut(start..end) {
                self.block(chunk, channels, n);
            }
            done += n;
        }
        self.frames = self.frames.wrapping_add(total_frames as u64);
        let status = self.status();
        self.status_cell.store(&status);
        self.peaks = [0.0; BUSES];
    }

    fn status(&self) -> Status {
        let (time, clock, counting_in) = match &self.song {
            Some(s) => {
                let t = s.time(self.sr);
                (t, s.clock, self.playing && t < s.audible_from)
            }
            None => (0.0, BeatClock::default(), false),
        };
        let shown = if self.playing || self.song.is_some() { time } else { self.free_time };
        let (bar, beat_in_bar) = clock.bar_beat(shown);
        Status {
            loaded: self.song.is_some(),
            playing: self.playing,
            time,
            beat: clock.beat_at(shown),
            bar,
            beat_in_bar,
            counting_in,
            crossfading: self.fade.is_some(),
            ended_count: self.ended_count,
            bus_peaks: self.peaks,
            frames: self.frames,
        }
    }

    fn block(&mut self, out: &mut [f32], channels: usize, n: usize) {
        let sr = self.sr;
        for v in self.bus_buf.iter_mut().take(BUSES * n * 2) {
            *v = 0.0;
        }
        // Quantized start: count down to the beat.
        let mut start_at: Option<usize> = None;
        if let Some(wait) = self.pending_start {
            if wait < n as u64 {
                start_at = Some(wait as usize);
                self.pending_start = None;
            } else {
                self.pending_start = Some(wait - n as u64);
            }
        }

        let mute_mask = self.mute_mask;
        // Taken out for the block so `self` stays free; `take` leaves an empty Vec, which
        // doesn't allocate, and the buffer goes back at the end.
        let mut bus_buf = std::mem::take(&mut self.bus_buf);
        let bus_buf = &mut bus_buf;
        for f in 0..n {
            if start_at == Some(f) {
                let ci = self.pending_count_in;
                self.start(ci);
            }

            // Main song.
            let mut song_ended = false;
            if self.playing
                && let Some(s) = &mut self.song
            {
                let t = s.time(sr);
                let end = s.end_time();
                if t >= end && end > 0.0 {
                    if s.loop_song {
                        s.set_time(s.start);
                    } else {
                        song_ended = true;
                    }
                } else {
                    let fade = if s.fade_out > 0.0 && t > end - s.fade_out {
                        ((end - t) / s.fade_out).clamp(0.0, 1.0) as f32
                    } else {
                        1.0
                    };
                    if t >= s.audible_from {
                        mix_song(s, t, fade, mute_mask, bus_buf, n, f);
                    }
                    s.elapsed += 1;
                }
            }
            if song_ended {
                self.playing = false;
                self.ended_count = self.ended_count.wrapping_add(1);
                if let Some(s) = &mut self.song {
                    let st = s.start;
                    s.set_time(st);
                }
            }

            // Fading-out song of a crossfade.
            let mut fade_finished = false;
            if let Some(fd) = &mut self.fade {
                let t = fd.song.time(sr);
                let g = 1.0 - (fd.done as f64 / fd.total.max(1) as f64) as f32;
                if t < fd.song.end_time() {
                    mix_song(&mut fd.song, t, g, mute_mask, bus_buf, n, f);
                }
                fd.song.elapsed += 1;
                fd.done += 1;
                if fd.done >= fd.total {
                    fade_finished = true;
                }
            }
            if fade_finished && let Some(fd) = self.fade.take() {
                self.discard(fd.song);
            }

            // Metronome.
            self.metronome_frame(bus_buf, n, f);
        }

        // Buses to device channels.
        for b in 0..BUSES {
            let params = self.buses.get(b).copied().unwrap_or_default();
            let target = if params.mute { 0.0 } else { sane_gain(params.gain) };
            let mut cur = self.bus_cur.get(b).copied().unwrap_or(1.0);
            let mut peak = self.peaks.get(b).copied().unwrap_or(0.0);
            let o = usize::from(params.output);
            let (ol, or) = if o + 1 < channels {
                (o, o + 1)
            } else if channels >= 2 {
                (0, 1)
            } else {
                (0, 0)
            };
            for f in 0..n {
                cur += (target - cur) * SMOOTH * 8.0;
                self.master_cur += (self.master - self.master_cur) * SMOOTH;
                let base = (b * MAX_BLOCK + f) * 2;
                let l = bus_buf.get(base).copied().unwrap_or(0.0) * cur;
                let r = bus_buf.get(base + 1).copied().unwrap_or(0.0) * cur;
                peak = peak.max(l.abs()).max(r.abs());
                let m = self.master_cur;
                if let Some(s) = out.get_mut(f * channels + ol) {
                    *s += l * m;
                }
                if ol != or {
                    if let Some(s) = out.get_mut(f * channels + or) {
                        *s += r * m;
                    }
                } else if let Some(s) = out.get_mut(f * channels + ol) {
                    *s += r * m;
                }
            }
            if let Some(c) = self.bus_cur.get_mut(b) {
                *c = cur;
            }
            if let Some(p) = self.peaks.get_mut(b) {
                *p = peak;
            }
        }
        let taken = std::mem::take(bus_buf);
        self.bus_buf = taken;
        for s in out.iter_mut() {
            *s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        }
        if !self.playing {
            self.free_time += n as f64 / sr;
        }
    }

    fn metronome_frame(&mut self, bus_buf: &mut [f32], _n: usize, f: usize) {
        let m = self.metronome;
        let (t, clock, counting_in) = match &self.song {
            Some(s) if self.playing => {
                let t = s.time(self.sr);
                (t, s.clock, t < s.audible_from)
            }
            Some(s) => (self.free_time, s.clock, false),
            None => (self.free_time, BeatClock::default(), false),
        };
        // Stopped: silent unless the person wants the click to keep running.
        let sounding = m.enabled
            && (self.playing || m.free_run)
            && match m.mode {
                ClickMode::Audible => true,
                ClickMode::CountInOnly => counting_in,
                ClickMode::VisualOnly => false,
            };
        let beat = clock.beat_at(t + m.latency).floor();
        let beat_i = if beat.is_finite() { beat as i64 } else { 0 };
        if beat_i != self.last_click_beat {
            if self.last_click_beat != i64::MIN && sounding {
                let bpb = i64::from(clock.beats_per_bar.max(1));
                self.click =
                    Click { phase: 0.0, freq: if beat_i.rem_euclid(bpb) == 0 { 1760.0 } else { 1175.0 }, env: 1.0 };
            }
            self.last_click_beat = beat_i;
        }
        if self.click.env > 0.0005 {
            let v = self.click.phase.sin() * self.click.env * sane_gain(m.gain);
            self.click.phase = (self.click.phase + TAU * self.click.freq / self.sr as f32) % TAU;
            self.click.env *= 1.0 - 180.0 / self.sr as f32;
            let base = (usize::from(m.bus).min(BUSES - 1) * MAX_BLOCK + f) * 2;
            if let Some(x) = bus_buf.get_mut(base) {
                *x += v;
            }
            if let Some(x) = bus_buf.get_mut(base + 1) {
                *x += v;
            }
        }
    }

    /// Song time of a track's read head, in seconds of its own file: for drift tests.
    pub fn read_head_seconds(&self, track: usize) -> Option<f64> {
        let s = self.song.as_ref()?;
        let clip = s.tracks.get(track)?.clip.as_ref()?;
        let pos = s.time(self.sr) * f64::from(clip.sample_rate);
        Some(pos / f64::from(clip.sample_rate))
    }

    /// Current song time.
    pub fn time(&self) -> Option<f64> {
        self.song.as_ref().map(|s| s.time(self.sr))
    }
}

fn sane_gain(g: f32) -> f32 {
    if g.is_finite() { g.clamp(0.0, 16.0) } else { 0.0 }
}

/// Mixes one frame of every track of `s` at song time `t` into the bus buffer.
#[inline]
fn mix_song(s: &mut SongLoad, t: f64, fade: f32, mute_mask: u8, bus_buf: &mut [f32], _n: usize, f: usize) {
    let song_gain = s.song_gain * fade;
    for slot in s.tracks.iter_mut() {
        let Some(clip) = &slot.clip else { continue };
        let p = slot.params;
        let muted = p.mute || (p.mute_groups & mute_mask) != 0;
        let (gl, gr) = if clip.channels >= 2 {
            pan_stereo(p.pan)
        } else if p.mono_to_stereo {
            pan_mono(p.pan)
        } else {
            (1.0, 0.0)
        };
        let g = if muted { 0.0 } else { sane_gain(p.gain) * song_gain };
        slot.cur_l += (g * gl - slot.cur_l) * SMOOTH * 8.0;
        slot.cur_r += (g * gr - slot.cur_r) * SMOOTH * 8.0;
        if slot.cur_l.abs() < 1e-6 && slot.cur_r.abs() < 1e-6 && muted {
            continue;
        }
        // The read head comes from the shared clock every frame: no accumulated drift.
        let pos = t * f64::from(clip.sample_rate);
        let mut l = clip.sample(0, pos);
        let mut r = if clip.channels >= 2 { clip.sample(1, pos) } else { l };
        let [st_l, st_r] = &mut slot.eq_state;
        for (band, c) in p.eq.iter().enumerate() {
            if let Some(stl) = st_l.get_mut(band) {
                l = stl.process(c, l);
            }
            if clip.channels >= 2
                && let Some(str_) = st_r.get_mut(band)
            {
                r = str_.process(c, r);
            }
        }
        if clip.channels < 2 {
            r = l;
        }
        let base = (usize::from(p.bus).min(BUSES - 1) * MAX_BLOCK + f) * 2;
        if let Some(x) = bus_buf.get_mut(base) {
            *x += l * slot.cur_l;
        }
        if let Some(x) = bus_buf.get_mut(base + 1) {
            *x += r * slot.cur_r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(sr: u32, secs: f64) -> Arc<Clip> {
        let frames = (f64::from(sr) * secs) as usize;
        let data = (0..frames).map(|i| (i as f64 / f64::from(sr) / secs) as f32 * 0.5).collect();
        Arc::new(Clip { sample_rate: sr, channels: 1, data })
    }

    #[test]
    fn tracks_with_different_rates_stay_in_sync() {
        // Timing test: 60 s of playback in odd-sized device buffers.
        let (mut ctl, mut r) = link(48_000);
        let tracks = vec![
            TrackSlot::new(Some(ramp(44_100, 120.0)), TrackParams::default()),
            TrackSlot::new(Some(ramp(48_000, 120.0)), TrackParams::default()),
            TrackSlot::new(Some(ramp(96_000, 120.0)), TrackParams::default()),
        ];
        assert!(ctl.send(Msg::Load(Box::new(SongLoad::new(tracks, 0.0, None, BeatClock::default())))));
        assert!(ctl.send(Msg::Play { count_in: false, quantized: false }));
        let mut buf = vec![0.0; 2 * 997];
        let mut rendered = 0usize;
        while rendered < 48_000 * 60 {
            r.process(&mut buf, 2);
            rendered += 997;
        }
        let heads: Vec<f64> = (0..3).filter_map(|i| r.read_head_seconds(i)).collect();
        assert_eq!(heads.len(), 3);
        let expected = rendered as f64 / 48_000.0;
        for h in &heads {
            // Within one output sample of the master clock.
            assert!((h - expected).abs() < 1.0 / 48_000.0, "{h} vs {expected}");
        }
        let status = ctl.poll();
        assert!(status.playing);
    }

    #[test]
    fn song_end_stops_or_loops() {
        let (mut ctl, mut r) = link(48_000);
        let tracks = vec![TrackSlot::new(Some(ramp(48_000, 0.5)), TrackParams::default())];
        assert!(ctl.send(Msg::Load(Box::new(SongLoad::new(tracks.clone(), 0.0, None, BeatClock::default())))));
        assert!(ctl.send(Msg::Play { count_in: false, quantized: false }));
        let mut buf = vec![0.0; 2 * 4800];
        for _ in 0..10 {
            r.process(&mut buf, 2);
        }
        let s = ctl.poll();
        assert!(!s.playing && s.ended_count == 1);

        assert!(ctl.send(Msg::Loop(true)));
        assert!(ctl.send(Msg::Play { count_in: false, quantized: false }));
        for _ in 0..10 {
            r.process(&mut buf, 2);
        }
        assert!(ctl.poll().playing);
    }

    #[test]
    fn count_in_delays_tracks_and_mute_groups_silence() {
        let (mut ctl, mut r) = link(48_000);
        let loud = Arc::new(Clip { sample_rate: 48_000, channels: 1, data: vec![0.5; 48_000 * 10] });
        let params = TrackParams { mute_groups: 0b1, ..TrackParams::default() };
        let mut song =
            SongLoad::new(vec![TrackSlot::new(Some(loud), params)], 0.0, None, BeatClock::new(120.0, 4, 0.0));
        song.count_in_bars = 1;
        assert!(ctl.send(Msg::Load(Box::new(song))));
        assert!(ctl.send(Msg::Play { count_in: true, quantized: false }));
        let mut buf = vec![0.0; 2 * 4800];
        r.process(&mut buf, 2);
        assert!(ctl.poll().counting_in);
        assert!(buf.iter().all(|s| s.abs() < 1e-6), "tracks silent during count-in");
        for _ in 0..30 {
            r.process(&mut buf, 2);
        }
        assert!(buf.iter().any(|s| s.abs() > 0.1));
        assert!(ctl.send(Msg::MuteGroups(0b1)));
        for _ in 0..3 {
            r.process(&mut buf, 2);
        }
        assert!(buf.iter().all(|s| s.abs() < 1e-3), "mute group silences its tracks");
    }

    /// Sample indexes where a click starts (signal rises above the threshold after silence).
    fn click_starts(buf: &[f32]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut quiet = 1000;
        for (i, frame) in buf.chunks(2).enumerate() {
            let v = frame.first().copied().unwrap_or(0.0).abs();
            if v > 0.05 && quiet > 500 {
                out.push(i);
            }
            quiet = if v > 0.01 { 0 } else { quiet + 1 };
        }
        out
    }

    #[test]
    fn count_in_clicks_land_on_the_beat_grid() {
        // Regression: the first click after Play was skipped, and the count-in wasn't aligned to
        // the song's beat anchor, so the click sat between the music's beats.
        let (mut ctl, mut r) = link(48_000);
        let silent = Arc::new(Clip { sample_rate: 48_000, channels: 1, data: vec![0.0; 48_000 * 10] });
        let mut song = SongLoad::new(
            vec![TrackSlot::new(Some(silent), TrackParams::default())],
            0.0,
            None,
            BeatClock::new(120.0, 4, 0.25),
        );
        song.count_in_bars = 1;
        assert!(ctl.send(Msg::Load(Box::new(song))));
        let metro = MetronomeParams { enabled: true, bus: 0, gain: 1.0, ..MetronomeParams::default() };
        assert!(ctl.send(Msg::Metronome(metro)));
        // Stopped: no click (the metronome doesn't run on its own).
        let mut idle = vec![0.0f32; 2 * 48_000];
        r.process(&mut idle, 2);
        assert!(click_starts(&idle).is_empty(), "silent while stopped");
        assert!(ctl.send(Msg::Play { count_in: true, quantized: false }));
        let mut buf = vec![0.0f32; 2 * 48_000 * 3];
        r.process(&mut buf, 2);
        let starts = click_starts(&buf);
        // Count-in starts at 0.25 - 2.0 s: the first click is at once, then every 0.5 s, and the
        // fifth click is the song's first beat at 0.25 s (2.0 s after the start).
        assert!(starts.first().is_some_and(|&s| s < 10), "first click at once: {starts:?}");
        for (k, s) in starts.iter().take(5).enumerate() {
            let expected = k * 24_000;
            assert!(s.abs_diff(expected) < 10, "click {k} at {s}, expected {expected}");
        }
    }

    #[test]
    fn second_crossfade_is_ignored_while_one_runs() {
        let (mut ctl, mut r) = link(48_000);
        let mk = || {
            Box::new(SongLoad::new(
                vec![TrackSlot::new(Some(ramp(48_000, 30.0)), TrackParams::default())],
                0.0,
                None,
                BeatClock::default(),
            ))
        };
        assert!(ctl.send(Msg::Load(mk())));
        assert!(ctl.send(Msg::Play { count_in: false, quantized: false }));
        assert!(ctl.send(Msg::Crossfade { next: mk(), seconds: 2.0 }));
        assert!(ctl.send(Msg::Crossfade { next: mk(), seconds: 2.0 }));
        let mut buf = vec![0.0; 2 * 480];
        r.process(&mut buf, 2);
        assert!(ctl.poll().crossfading);
        for _ in 0..250 {
            r.process(&mut buf, 2);
        }
        assert!(!ctl.poll().crossfading);
    }
}
