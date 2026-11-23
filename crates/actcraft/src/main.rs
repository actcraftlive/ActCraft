//! The ActCraft desktop app: the window, the audio output and the JSON control channel.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use actcraft_control::channel;
use actcraft_engine::{Engine, MessageArg};

use actcraft_engine::midi_sync::SyncConfig;
use actcraft_platform::{
    AudioOutput, DmxFrame, DmxIn, DmxInLevels, DmxOut, Gamepads, MidiIn, MidiQueue, MidiTimeOut, NativeServices, OscIn,
    OscQueue, StatusReader,
};
use actcraft_ui::{ActCraftApp, FileAsk, FileKind, FileRequest, PlatformInfo};
use craft_dialogs::{Answer, Dialogs, FileDialog};
use serde_json::json;

/// `--screenshot FILE`: save a PNG of the window after it settles, then close (for docs and
/// visual checks).
struct Screenshot {
    path: std::path::PathBuf,
    started: std::time::Instant,
    requested: bool,
    done: Arc<std::sync::atomic::AtomicBool>,
}

struct Host {
    app: ActCraftApp,
    // Kept alive for the lifetime of the window: dropping it stops the sound.
    _audio: Option<AudioOutput>,
    screenshot: Option<Screenshot>,
    /// Native file pickers, shown without blocking the event loop.
    dialogs: Dialogs<FileAsk>,
    /// Settings kept on this computer (the audio interface).
    prefs: craft_settings::SettingsStore<Prefs>,
    /// Where MIDI time goes while it plays, and the playback position it follows.
    midi: Option<MidiTimeOut>,
    status: Option<StatusReader>,
    /// The song, MIDI file and latency offset the MIDI output plays now (loaded again when one
    /// changes).
    midi_part: Option<(u64, Option<String>, u64)>,
    /// The MIDI input remote actions listen to, and what came in from it.
    midi_in: Option<MidiIn>,
    midi_queue: MidiQueue,
    /// Where OSC messages come in, and what came.
    osc_in: Option<OscIn>,
    osc_queue: OscQueue,
    /// Listening for Art-Net from other gear, and the newest levels it heard.
    dmx_in: Option<DmxIn>,
    dmx_in_levels: DmxInLevels,
    /// Game controllers (`None` when they could not be watched).
    gamepads: Option<Gamepads>,
    /// A Hue request running off the window's thread.
    hue_job: Option<std::sync::mpsc::Receiver<HueAnswer>>,
    /// Sends fixture colours to the paired bridge's lights (`None` when not paired).
    hue_out: Option<actcraft_platform::hue::HueOut>,
    /// Band link: hosting, following, a connection being made, and when to try again.
    band_host: Option<actcraft_platform::band::BandHost>,
    /// Musicians' sheets in the browser, while on.
    sheets_web: Option<actcraft_platform::sheet_web::SheetWeb>,
    /// Scores read for the browser pages, by file, and when their pictures were last drawn.
    web_scores: std::collections::HashMap<String, actcraft_engine::notation::Score>,
    web_score_tick: std::time::Instant,
    band_follow: Option<actcraft_platform::band::BandFollow>,
    band_connecting: Option<std::sync::mpsc::Receiver<Result<BandConnected, String>>>,
    band_retry: std::time::Instant,
    /// Sends DMX levels to the show.s outputs while it has any; what it sends now (the levels.
    /// version and the outputs) and the problems already reported.
    dmx_out: Option<DmxOut>,
    dmx_sent: Option<(u64, Vec<actcraft_engine::dmx::DmxOutput>)>,
    dmx_reported: Vec<String>,
    /// An Art-Net node search running in the background.
    artnet_search: Option<std::sync::mpsc::Receiver<Result<Vec<actcraft_platform::ArtNode>, String>>>,
    /// Wakes the window when a MIDI message comes in.
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// Turns the window's request into a native file picker.
fn to_dialog(req: &FileRequest) -> FileDialog {
    let mut d = match req.kind {
        FileKind::Open => FileDialog::open(),
        FileKind::OpenMany => FileDialog::open_many(),
        FileKind::Save => FileDialog::save(),
    }
    .title(req.title);
    for (name, exts) in &req.filters {
        d = d.filter(*name, exts);
    }
    if let Some(dir) = &req.directory {
        d = d.directory(dir.clone());
    }
    if let Some(name) = &req.file_name {
        d = d.file_name(name.clone());
    }
    d
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.app.platform.time_of_day = chrono_now();
        // Answers first, then draw, then show what the window asked for.
        for (ask, answer) in self.dialogs.take_answers() {
            match answer {
                Answer::Paths(paths) => self.app.file_chosen(ask, paths),
                Answer::Unavailable(why) => self.app.file_dialog_failed(why),
                Answer::Cancelled | Answer::Message(_) => self.app.file_cancelled(ask),
            }
        }
        self.app.ui(ui);
        if let Some(device) = self.app.take_device_request() {
            self.switch_device(device);
        }
        if let Some(port) = self.app.take_midi_request() {
            self.switch_midi(port);
        }
        if let Some(port) = self.app.take_midi_input_request() {
            self.switch_midi_input(port);
        }
        if let Some(listen) = self.app.take_osc_request() {
            self.switch_osc(listen);
        }
        if let Some(listen) = self.app.take_dmx_in_request() {
            self.switch_dmx_in(listen);
        }
        for (universe, levels) in actcraft_platform::dmx_in::take(&self.dmx_in_levels) {
            self.app.engine().dmx_in(universe, &levels);
        }
        self.follow_gamepads();
        self.follow_hue();
        self.follow_hue_out();
        self.follow_band();
        for (address, value) in actcraft_platform::osc_in::drain(&self.osc_queue) {
            self.app.engine().osc_in(&address, value);
        }
        if self.app.take_midi_refresh() {
            self.app.platform.midi_ports = actcraft_platform::midi_output_ports();
            self.app.platform.midi_inputs = actcraft_platform::midi_input_ports();
        }
        // What a controller sent: a trigger to learn, or remote actions to set off.
        for msg in actcraft_platform::midi_in::drain(&self.midi_queue) {
            if self.app.wants_midi_learn() {
                self.app.midi_learned(&msg);
            } else {
                self.app.engine().midi_in(&msg);
            }
        }
        // What the show sends over MIDI can change any moment (Settings, a remote command).
        if let Some(midi) = &self.midi {
            midi.set(SyncConfig::from(&self.app.engine().show.midi));
        }
        self.follow_midi_file();
        self.follow_dmx();
        self.follow_artnet_search();
        // Listing serial ports is quick (no waiting on the network), so it runs right here.
        if self.app.take_usb_find_request() {
            self.app.platform.serial_ports = Some(actcraft_platform::serial_ports());
        }
        if let Some(language) = self.app.take_language_request() {
            self.prefs.update(|pr| pr.window.language = language);
        }
        let _ = self.prefs.tick(std::time::Instant::now());
        for req in self.app.take_file_requests() {
            let dialog = to_dialog(&req);
            self.dialogs.ask(dialog, req.ask);
        }
        self.dialogs.show_pending(Some(&*frame));
        if let Some(shot) = &mut self.screenshot {
            let ctx = ui.ctx().clone();
            if !shot.requested && shot.started.elapsed() > std::time::Duration::from_secs(2) {
                shot.requested = true;
                let path = shot.path.clone();
                let done = Arc::clone(&shot.done);
                ctx.request_screenshot(move |image| {
                    let [w, h] = image.size;
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    let saved = u32::try_from(w)
                        .ok()
                        .zip(u32::try_from(h).ok())
                        .map(|(w, h)| image::save_buffer(&path, &bytes, w, h, image::ExtendedColorType::Rgba8));
                    if !matches!(saved, Some(Ok(()))) {
                        eprintln!("could not save the screenshot to {}", path.display());
                    }
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
            if shot.done.load(std::sync::atomic::Ordering::Relaxed) {
                self.app.close_without_asking();
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }
}

impl Host {
    /// Starts what Settings asked of the Hue bridge (one thing at a time) and hands the answer to
    /// the window when it comes.
    fn follow_hue(&mut self) {
        if self.hue_job.is_none()
            && let Some(request) = self.app.take_hue_request()
        {
            self.start_hue(request);
        }
        let Some(rx) = &self.hue_job else { return };
        let Ok(answer) = rx.try_recv() else { return };
        self.hue_job = None;
        self.app.platform.hue_busy = false;
        match answer {
            HueAnswer::Found(found) => {
                self.app.platform.hue_bridges = Some(found.map(|ips| ips.iter().map(ToString::to_string).collect()));
            }
            HueAnswer::Paired(ip, Ok(key)) => {
                // The key stays in this computer's settings, never in a show file.
                self.prefs.update(|pr| {
                    pr.audio.hue_bridge.clone_from(&ip);
                    pr.audio.hue_key = key;
                });
                self.app.platform.hue_paired = Some(ip);
                self.app.platform.hue_note = None;
                self.start_hue(actcraft_ui::HueRequest::Lights);
            }
            HueAnswer::Paired(_, Err(e)) => {
                self.app.platform.hue_note = Some(if e.contains("link button") {
                    actcraft_ui::tr("Press the round button on the bridge, then pair again within 30 seconds.")
                        .to_owned()
                } else {
                    e
                });
            }
            HueAnswer::Lights(lights) => {
                self.app.platform.hue_lights = Some(lights.map(|ls| ls.into_iter().map(|l| (l.id, l.name)).collect()));
            }
        }
    }

    /// Hands the paired bridge what each Hue light should show: the colour its fixture sends now.
    /// Starts sending when a bridge is paired and stops when it is forgotten.
    fn follow_hue_out(&mut self) {
        use actcraft_platform::hue;
        let pr = self.prefs.get().audio.clone();
        let paired = self.app.platform.hue_paired.is_some() && !pr.hue_key.is_empty();
        if !paired {
            self.hue_out = None;
            return;
        }
        if self.hue_out.is_none() {
            let Ok(ip) = pr.hue_bridge.parse::<std::net::Ipv4Addr>() else { return };
            match hue::HueOut::start(hue::bridge_at(ip), pr.hue_key.clone()) {
                Ok(out) => self.hue_out = Some(out),
                Err(e) => {
                    self.app.platform.hue_note = Some(e);
                    return;
                }
            }
        }
        let Some(out) = &self.hue_out else { return };
        let engine = self.app.engine();
        let states = engine
            .show
            .fixtures
            .iter()
            .filter(|f| !f.hue.is_empty())
            .map(|f| (f.hue.clone(), hue::state_of(actcraft_engine::fixtures::output_colour(&engine, f))))
            .collect();
        drop(engine);
        out.set(states);
        for e in out.take_errors() {
            self.app.engine().say_error_with("Hue: {error}", vec![MessageArg::plain("error", e)]);
        }
    }

    /// Band link each frame: what Settings asked, the state for followers while hosting, and
    /// going along with the host while following (connecting again when it went away).
    fn follow_band(&mut self) {
        use actcraft_ui::BandRequest;
        match self.app.take_band_request() {
            Some(BandRequest::Host(on)) => self.switch_band_host(on),
            Some(BandRequest::Sheets(on)) => self.switch_sheets_web(on),
            Some(BandRequest::Follow(addr, code)) => {
                let token = self.prefs.get().audio.band_token.clone();
                let same_host = self.prefs.get().audio.band_follow == addr;
                let auth = if code.is_empty() && same_host && !token.is_empty() {
                    actcraft_platform::band::BandAuth::Token(token)
                } else {
                    actcraft_platform::band::BandAuth::Code(code)
                };
                self.app.platform.band_note = None;
                self.start_band_connect(addr, auth);
            }
            Some(BandRequest::Unfollow) => {
                self.band_follow = None;
                self.band_connecting = None;
                self.app.platform.band_following = None;
                self.prefs.update(|pr| {
                    pr.audio.band_follow.clear();
                    pr.audio.band_token.clear();
                });
            }
            None => {}
        }
        if let Some(host) = &self.band_host {
            host.publish(self.app.engine().band_state());
            let new = host.take_new_tokens();
            if !new.is_empty() {
                self.prefs.update(|pr| {
                    pr.audio.band_tokens.extend(new);
                    // The newest few dozen are enough; old followers pair again.
                    let extra = pr.audio.band_tokens.len().saturating_sub(64);
                    pr.audio.band_tokens.drain(..extra);
                });
            }
            self.app.platform.band_hosting = Some((host.addr().to_string(), host.code(), host.followers()));
        }
        if let Some(web) = &self.sheets_web {
            web.publish(self.app.engine().sheets_state());
            let new = web.take_new_tokens();
            if !new.is_empty() {
                self.prefs.update(|pr| {
                    pr.audio.sheets_tokens.extend(new);
                    let extra = pr.audio.sheets_tokens.len().saturating_sub(64);
                    pr.audio.sheets_tokens.drain(..extra);
                });
            }
            let ip = actcraft_platform::sheet_web::local_address()
                .map_or_else(|| actcraft_ui::tr("this computer's address").to_owned(), |ip| ip.to_string());
            let address = format!("http://{ip}:{}", web.port());
            self.app.platform.sheets_web = Some((address, web.code(), web.connections()));
            // Score sheets as pictures of the lines playing now, while pages are open.
            if web.connections() > 0 && self.web_score_tick.elapsed() >= std::time::Duration::from_millis(700) {
                self.web_score_tick = std::time::Instant::now();
                let e = self.app.engine();
                let count = e.show.musicians.len();
                for i in 0..count {
                    let picture = score_picture(&e, &mut self.web_scores, i);
                    web.publish_score(i, picture);
                }
            }
        }
        // A connection being made comes back from its thread.
        if let Some(rx) = &self.band_connecting
            && let Ok(result) = rx.try_recv()
        {
            self.band_connecting = None;
            match result {
                Ok(BandConnected { follow, addr, token }) => {
                    self.prefs.update(|pr| {
                        pr.audio.band_follow.clone_from(&addr);
                        if let Some(t) = token {
                            pr.audio.band_token = t;
                        }
                    });
                    self.band_follow = Some(follow);
                    self.app.platform.band_following = Some((addr, true));
                }
                Err(e) => {
                    if self.app.platform.band_following.is_none() {
                        self.app.platform.band_note = Some(e);
                    }
                }
            }
        }
        if let Some(follow) = &self.band_follow {
            let missing = follow.take_state().and_then(|state| self.app.engine().follow_band(&state));
            if let Some(missing) = missing {
                self.app.platform.band_note = Some(
                    actcraft_ui::tr("The host plays \u{201c}{song}\u{201d}, which this computer does not have.")
                        .replace("{song}", &missing),
                );
            }
            if !follow.connected() {
                self.band_follow = None;
                if let Some(f) = &mut self.app.platform.band_following {
                    f.1 = false;
                }
            }
        }
        // Gone or not yet there: try again with the token every few seconds (playing on meanwhile).
        if self.band_follow.is_none()
            && self.band_connecting.is_none()
            && let Some((addr, false)) = self.app.platform.band_following.clone()
            && self.band_retry.elapsed() >= std::time::Duration::from_secs(3)
        {
            let token = self.prefs.get().audio.band_token.clone();
            if !token.is_empty() {
                self.start_band_connect(addr, actcraft_platform::band::BandAuth::Token(token));
            }
        }
    }

    /// Connects to a band host on a thread of its own, so the window never waits for it.
    fn start_band_connect(&mut self, addr: String, auth: actcraft_platform::band::BandAuth) {
        use actcraft_platform::band::{BAND_PORT, BandFollow};
        self.band_retry = std::time::Instant::now();
        let target = addr
            .parse::<std::net::SocketAddr>()
            .or_else(|_| addr.parse::<std::net::IpAddr>().map(|ip| std::net::SocketAddr::new(ip, BAND_PORT)));
        let Ok(target) = target else {
            self.app.platform.band_note = Some(actcraft_ui::tr("That is not an address like 192.168.1.20.").to_owned());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let job = move || {
            let result =
                BandFollow::connect(target, &auth).map(|(follow, token)| BandConnected { follow, addr, token });
            let _ = tx.send(result);
        };
        if std::thread::Builder::new().name("actcraft-band-connect".into()).spawn(job).is_ok() {
            self.band_connecting = Some(rx);
        }
    }

    /// Lets band members follow this computer, or stops. Remembered on this computer.
    fn switch_sheets_web(&mut self, on: bool) {
        use actcraft_platform::sheet_web::{SHEETS_PORT, SheetWeb};
        self.sheets_web = None;
        self.app.platform.sheets_web = None;
        self.prefs.update(|pr| pr.audio.sheets_web = on);
        if !on {
            return;
        }
        let tokens = self.prefs.get().audio.sheets_tokens.clone();
        let lang = actcraft_ui::i18n::current_code();
        let tr: actcraft_platform::sheet_web::Translate = std::sync::Arc::new(actcraft_ui::i18n::translator());
        match SheetWeb::open(SHEETS_PORT, tokens, &lang, tr) {
            Ok(web) => self.sheets_web = Some(web),
            Err(e) => self.app.platform.band_note = Some(e),
        }
    }

    fn switch_band_host(&mut self, on: bool) {
        self.band_host = None;
        self.app.platform.band_hosting = None;
        self.prefs.update(|pr| pr.audio.band_host = on);
        if !on {
            return;
        }
        let tokens = self.prefs.get().audio.band_tokens.clone();
        match actcraft_platform::band::BandHost::open(actcraft_platform::band::BAND_PORT, tokens) {
            Ok(host) => self.band_host = Some(host),
            Err(e) => self.app.platform.band_note = Some(e),
        }
    }

    /// Runs a Hue request on a thread of its own (the network may take seconds).
    fn start_hue(&mut self, request: actcraft_ui::HueRequest) {
        use actcraft_platform::hue;
        use actcraft_ui::HueRequest;
        if let HueRequest::Forget = request {
            self.prefs.update(|pr| {
                pr.audio.hue_bridge.clear();
                pr.audio.hue_key.clear();
            });
            self.app.platform.hue_paired = None;
            self.app.platform.hue_lights = None;
            return;
        }
        let pr = self.prefs.get().audio.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let job = move || {
            let answer = match request {
                HueRequest::Find => HueAnswer::Found(hue::discover(std::time::Duration::from_secs(2))),
                HueRequest::Pair(ip) => {
                    let key = ip
                        .parse::<std::net::Ipv4Addr>()
                        .map_err(|e| e.to_string())
                        .and_then(|addr| hue::pair(hue::bridge_at(addr)));
                    HueAnswer::Paired(ip, key)
                }
                HueRequest::Lights => HueAnswer::Lights(
                    pr.hue_bridge
                        .parse::<std::net::Ipv4Addr>()
                        .map_err(|e| e.to_string())
                        .and_then(|addr| hue::lights(hue::bridge_at(addr), &pr.hue_key)),
                ),
                HueRequest::Forget => return,
            };
            let _ = tx.send(answer);
        };
        match std::thread::Builder::new().name("actcraft-hue".into()).spawn(job) {
            Ok(_) => {
                self.hue_job = Some(rx);
                self.app.platform.hue_busy = true;
            }
            Err(e) => self.app.platform.hue_note = Some(e.to_string()),
        }
    }

    /// Hands controller buttons pressed since the last frame to the engine (or to Settings while
    /// it learns one), and tells the window which controllers are connected.
    fn follow_gamepads(&mut self) {
        let Some(pads) = &mut self.gamepads else { return };
        let input = pads.poll();
        self.app.platform.gamepads = pads.connected();
        for button in input.pressed {
            if self.app.gamepad_learning() {
                self.app.gamepad_learned(button);
            } else {
                self.app.engine().gamepad_in(button);
            }
        }
        for (axis, value) in input.moved {
            self.app.engine().gamepad_axis(axis, value);
        }
    }

    /// Stops listening for Art-Net DMX and listens again as asked (`Some(network)`), or not
    /// (`None`). Remembered on this computer.
    fn switch_dmx_in(&mut self, listen: Option<bool>) {
        self.dmx_in = None;
        self.app.platform.dmx_in_listening = None;
        self.prefs.update(|pr| {
            pr.audio.dmx_in = listen.is_some();
            if let Some(network) = listen {
                pr.audio.dmx_in_network = network;
            }
        });
        let Some(network) = listen else { return };
        match DmxIn::open(network, Arc::clone(&self.dmx_in_levels), Arc::clone(&self.wake)) {
            Ok(input) => {
                // Art-Net's address, and sACN's port when it could be had.
                let address = if input.hears_sacn() {
                    format!("{} \u{b7} sACN {}", input.addr(), actcraft_platform::dmx_in::SACN_PORT)
                } else {
                    input.addr().to_string()
                };
                self.app
                    .engine()
                    .say_with("DMX in: listening on {address}", vec![MessageArg::plain("address", &address)]);
                self.app.platform.dmx_in_listening = Some(address);
                self.dmx_in = Some(input);
            }
            Err(e) => {
                self.app
                    .engine()
                    .say_error_with("Could not listen for DMX: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    /// Stops listening for OSC and listens again as asked (none for `None`). Remembered on this
    /// computer.
    fn switch_osc(&mut self, listen: Option<(u16, bool)>) {
        self.osc_in = None;
        self.app.platform.osc_listening = None;
        self.prefs.update(|pr| {
            pr.audio.osc = listen.is_some();
            if let Some((port, network)) = listen {
                pr.audio.osc_port = port;
                pr.audio.osc_network = network;
            }
        });
        let Some((port, network)) = listen else { return };
        match OscIn::open(port, network, Arc::clone(&self.osc_queue), Arc::clone(&self.wake)) {
            Ok(input) => {
                let address = input.addr().to_string();
                self.app.engine().say_with("OSC: listening on {address}", vec![MessageArg::plain("address", &address)]);
                self.app.platform.osc_listening = Some(address);
                self.osc_in = Some(input);
            }
            Err(e) => {
                self.app
                    .engine()
                    .say_error_with("Could not listen for OSC: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    /// Stops listening to the MIDI input and listens to `port` (none for `None`). Remembered on
    /// this computer.
    fn switch_midi_input(&mut self, port: Option<String>) {
        self.midi_in = None;
        self.app.platform.midi_input = None;
        let name = port.clone().unwrap_or_default();
        self.prefs.update(|pr| pr.audio.midi_input = name);
        let Some(port) = port else { return };
        match MidiIn::open(&port, Arc::clone(&self.midi_queue), Arc::clone(&self.wake)) {
            Ok(input) => {
                self.app.platform.midi_input = Some(input.port().to_owned());
                self.app.engine().say_with("MIDI input: {port}", vec![MessageArg::plain("port", &port)]);
                self.midi_in = Some(input);
            }
            Err(e) => {
                self.app
                    .engine()
                    .say_error_with("Could not open that MIDI input: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    /// Hands the DMX levels and outputs to the sender when they changed; starts it when the show
    /// has outputs and stops it when it has none. Sending problems become messages, each once
    /// until the outputs change.
    fn follow_dmx(&mut self) {
        let frame = {
            let e = self.app.engine();
            let outputs = &e.show.dmx_outputs;
            let same = self.dmx_sent.as_ref().is_some_and(|(v, o)| *v == e.dmx.version && o == outputs);
            if outputs.is_empty() || same {
                None
            } else {
                let mut levels = std::collections::HashMap::new();
                for o in outputs {
                    if let Some(u) = e.output_universe(usize::from(o.universe)) {
                        levels.insert(o.universe, u);
                    }
                }
                Some(DmxFrame { outputs: outputs.clone(), levels, version: e.dmx.version })
            }
        };
        if self.app.engine().show.dmx_outputs.is_empty() {
            self.dmx_out = None;
            self.dmx_sent = None;
            self.dmx_reported.clear();
            return;
        }
        if let Some(frame) = frame {
            if self.dmx_sent.as_ref().is_none_or(|(_, o)| *o != frame.outputs) {
                self.dmx_reported.clear();
            }
            if self.dmx_out.is_none() {
                match DmxOut::open() {
                    Ok(out) => self.dmx_out = Some(out),
                    Err(e) => self.report_dmx(e),
                }
            }
            if let Some(out) = &self.dmx_out {
                self.dmx_sent = Some((frame.version, frame.outputs.clone()));
                out.set(frame);
            }
        }
        let errors = self.dmx_out.as_ref().map(DmxOut::take_errors).unwrap_or_default();
        for e in errors {
            self.report_dmx(e);
        }
    }

    /// Starts an Art-Net node search when asked (on a thread: it listens for two seconds) and
    /// hands its answer to the window when it ends.
    fn follow_artnet_search(&mut self) {
        if self.app.take_artnet_find_request() && self.artnet_search.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = Arc::clone(&self.wake);
            let started = std::thread::Builder::new().name("actcraft-artnet-search".into()).spawn(move || {
                let _ = tx.send(actcraft_platform::discover_artnet_nodes(std::time::Duration::from_secs(2)));
                wake();
            });
            match started {
                Ok(_) => {
                    self.artnet_search = Some(rx);
                    self.app.platform.artnet_searching = true;
                }
                Err(e) => self.app.platform.artnet_nodes = Some(Err(e.to_string())),
            }
        }
        let Some(rx) = &self.artnet_search else { return };
        let answer = match rx.try_recv() {
            Ok(answer) => answer,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the search stopped".to_owned()),
        };
        self.artnet_search = None;
        self.app.platform.artnet_searching = false;
        self.app.platform.artnet_nodes = Some(answer.map(|nodes| {
            nodes
                .into_iter()
                .map(|n| actcraft_ui::ArtNetNode { address: n.address.to_string(), name: n.name, universes: n.outputs })
                .collect()
        }));
    }

    fn report_dmx(&mut self, error: String) {
        if self.dmx_reported.contains(&error) || self.dmx_reported.len() >= 16 {
            return;
        }
        self.app.engine().say_error_with("Could not send DMX: {error}", vec![MessageArg::plain("error", &error)]);
        self.dmx_reported.push(error);
    }

    /// Hands the current song's MIDI file to the MIDI output when the song, its file or the
    /// MIDI latency offset changed since the last look.
    fn follow_midi_file(&mut self) {
        let Some(midi) = &self.midi else { return };
        let (key, part) = {
            let e = self.app.engine();
            let key = e.current_song.map(|id| {
                let file = e.show.song(id).and_then(|s| s.midi_file.clone());
                (id, file, e.show.mixer.midi_latency_offset_ms.to_bits())
            });
            if key == self.midi_part {
                return;
            }
            let part = match key.as_ref().map(|k| actcraft_engine::midi_file::song_part(&e, k.0)) {
                Some(Ok(part)) => part,
                Some(Err(err)) => {
                    drop(e);
                    self.app.engine().say_error(err.message());
                    None
                }
                None => None,
            };
            (key, part)
        };
        midi.set_part(part.map(Arc::new));
        self.midi_part = key;
    }

    /// Closes the MIDI time output and opens `port` (none for `None`), following the playback
    /// position of the running audio output. Remembered on this computer.
    fn switch_midi(&mut self, port: Option<String>) {
        self.midi = None;
        self.midi_part = None;
        self.app.platform.midi_port = None;
        let name = port.clone().unwrap_or_default();
        self.prefs.update(|pr| pr.audio.midi_output = name);
        let (Some(port), Some(status)) = (port, self.status.clone()) else { return };
        let cfg = SyncConfig::from(&self.app.engine().show.midi);
        match MidiTimeOut::open(&port, status, cfg) {
            Ok(out) => {
                self.app.platform.midi_port = Some(out.port().to_owned());
                self.app.engine().say_with("MIDI: {port}", vec![MessageArg::plain("port", &port)]);
                self.midi = Some(out);
            }
            Err(e) => {
                self.app
                    .engine()
                    .say_error_with("Could not open that MIDI output: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    /// Stops the current output and opens another device; the engine gets the new stream.
    fn switch_device(&mut self, device: Option<String>) {
        self._audio = None;
        match AudioOutput::open(device.as_deref()) {
            Ok((out, controller)) => {
                let p = &mut self.app.platform;
                p.audio_device = Some(out.device_name.clone());
                p.sample_rate = out.sample_rate;
                p.output_channels = out.channels;
                p.device_id = device.clone();
                self.status = Some(controller.status_reader());
                self.app.engine().attach_audio(controller);
                self.app.engine().say_with("Audio: {device}", vec![MessageArg::plain("device", &out.device_name)]);
                self._audio = Some(out);
                self.prefs.update(|pr| pr.audio.device = device.unwrap_or_default());
                // MIDI time follows the new output's position.
                if let Some(port) = self.app.platform.midi_port.clone() {
                    self.switch_midi(Some(port));
                }
            }
            Err(e) => {
                self.app.platform.audio_device = None;
                self.app.engine().say_error_with(
                    "Could not open that audio interface: {error}",
                    vec![MessageArg::plain("error", e)],
                );
            }
        }
    }
}

/// The UDP port OSC is listened for on until another is chosen.
const DEFAULT_OSC_PORT: u16 = 8000;

/// What ActCraft remembers on this computer (not in the show file).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Prefs {
    audio: AudioPrefs,
    window: WindowPrefs,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct WindowPrefs {
    /// Empty or `en` for English, `auto` for the system language, or a language file's code.
    language: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AudioPrefs {
    /// Output device id; empty for the system default.
    device: String,
    /// MIDI output that MIDI time goes to; empty for none.
    midi_output: String,
    /// MIDI input remote actions listen to; empty for none.
    midi_input: String,
    /// Whether ActCraft listens for OSC, on which UDP port (0: the default), and whether other
    /// computers may send.
    osc: bool,
    osc_port: u16,
    osc_network: bool,
    /// Whether ActCraft listens for Art-Net DMX, and whether other computers may send.
    dmx_in: bool,
    dmx_in_network: bool,
    /// The Hue bridge paired with (its address) and the key it gave; both empty when none.
    hue_bridge: String,
    hue_key: String,
    /// Band link: whether this computer lets others follow it, the tokens of followers it
    /// paired with, and the host it follows with the token that host gave (empty: none).
    band_host: bool,
    band_tokens: Vec<String>,
    /// Sheets in the browser: on or off, and the tokens of browsers signed in.
    sheets_web: bool,
    sheets_tokens: Vec<String>,
    band_follow: String,
    band_token: String,
}

impl craft_settings::Settings for Prefs {
    const APP_ID: &'static str = "actcraft";
    const SCHEMA_VERSION: u32 = 1;
}

fn chrono_now() -> String {
    use actcraft_engine::Services;
    NativeServices.now().format("%H:%M:%S").to_string()
}

/// The score picture musician `index` sees in the browser now: the lines playing of the score
/// their sheet names, as SVG with a key that changes with the lines; `None` for a PDF sheet, no
/// sheet or a score that can't be read. Scores are read once and kept (a few).
fn score_picture(
    e: &actcraft_engine::Engine,
    kept: &mut std::collections::HashMap<String, actcraft_engine::notation::Score>,
    index: usize,
) -> Option<(String, String)> {
    use std::hash::{Hash, Hasher};
    let song = e.current()?;
    let name = e.show.musicians.get(index)?;
    let at = actcraft_engine::sheet_commands::sheet_for(song, name)?;
    let sheet = song.sheets.get(at)?;
    if sheet.pdf_file.is_some() {
        return None;
    }
    let path = sheet.notation_file.as_ref()?;
    if !kept.contains_key(path) {
        let score = actcraft_engine::notation_commands::load_score(e, path).ok()?;
        if kept.len() >= 8 {
            kept.clear();
        }
        kept.insert(path.clone(), score);
    }
    let score = kept.get(path)?;
    let (line, svg) = actcraft_ui::score_svg(score, sheet.part, &song.with_sheet(Some(at)), e.status.time, 820.0, 3)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (path, sheet.part, song.id, at, line, sheet.score_first_bar).hash(&mut hasher);
    Some((format!("{:016x}", hasher.finish()), svg))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).take(16).collect();
    let mut engine = Engine::new(Box::new(NativeServices));
    let mut platform = PlatformInfo::default();
    let (prefs, _report) = craft_settings::SettingsStore::<Prefs>::open(craft_settings::Location::Auto);
    platform.devices = actcraft_platform::output_devices().into_iter().map(|d| (d.id, d.name)).collect();
    // The language chosen last time (`--lang CODE` overrides it, for screenshots).
    let language = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| prefs.get().window.language.clone());
    // English unless the person picked another language.
    platform.language = if language.is_empty() { "en".to_owned() } else { language };
    actcraft_ui::set_language(&platform.language);

    // The interface chosen last time, or the system default when it's gone.
    let saved = Some(prefs.get().audio.device.clone()).filter(|d| !d.is_empty());
    let opened = match AudioOutput::open(saved.as_deref()) {
        Ok(ok) => Ok((ok, saved)),
        Err(_) if saved.is_some() => AudioOutput::open(None).map(|ok| (ok, None)),
        Err(e) => Err(e),
    };
    let mut status = None;
    let audio = match opened {
        Ok(((out, controller), device_id)) => {
            platform.audio_device = Some(out.device_name.clone());
            platform.sample_rate = out.sample_rate;
            platform.output_channels = out.channels;
            platform.device_id = device_id;
            status = Some(controller.status_reader());
            engine.attach_audio(controller);
            Some(out)
        }
        Err(e) => {
            engine.say_error_with("No audio output: {error}", vec![MessageArg::plain("error", e)]);
            None
        }
    };

    // A show file given on the command line opens at start.
    if let Some(path) = args.iter().find(|a| a.ends_with(".actcraft")) {
        let _ = engine.run("show.open", &json!({"path": path}));
    }

    let engine = Arc::new(Mutex::new(engine));
    let addr = SocketAddr::from(([127, 0, 0, 1], channel::DEFAULT_PORT));
    match channel::serve(addr, Arc::clone(&engine), None) {
        Ok(a) => platform.control_address = Some(a.to_string()),
        Err(e) => {
            if let Ok(mut en) = engine.lock() {
                en.say_error_with("Remote control is off: {error}", vec![MessageArg::plain("error", e)]);
            }
        }
    }

    // `--open-settings [TAB]`: the Settings window, on a tab by name (`midi`, `audio`...).
    let open_settings = args.iter().position(|a| a == "--open-settings").map(|i| {
        args.get(i + 1).filter(|t| !t.starts_with("--") && !t.ends_with(".actcraft")).cloned().unwrap_or_default()
    });
    let show_lyrics = args.iter().any(|a| a == "--lyrics");
    let edit_lyrics = args.iter().any(|a| a == "--edit-lyrics");
    let stage = args.iter().any(|a| a == "--stage");
    let cues = args.iter().any(|a| a == "--cues");
    let sheet = args.iter().any(|a| a == "--sheet");
    let about = args.iter().any(|a| a == "--about");
    let dmx = args.iter().any(|a| a == "--dmx");
    let fixtures = args.iter().any(|a| a == "--fixtures");
    let new_fixture_type = args.iter().any(|a| a == "--new-fixture-type");
    let presets = args.iter().any(|a| a == "--presets");
    let light_cues = args.iter().any(|a| a == "--light-cues");
    let stage_view = args.iter().any(|a| a == "--stage-view");
    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(|p| Screenshot {
        path: std::path::PathBuf::from(p),
        started: std::time::Instant::now(),
        requested: false,
        done: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    // The logo (see ATTRIBUTION.md) is the window icon and greets on the empty screen.
    let logo = image::load_from_memory(include_bytes!("../../../assets/logo/actcraft-icon-256.png"))
        .ok()
        .map(|i| i.to_rgba8());
    let mut viewport = eframe::egui::ViewportBuilder::default();
    if let Some(img) = &logo {
        viewport = viewport.with_icon(eframe::egui::IconData {
            rgba: img.as_raw().clone(),
            width: img.width(),
            height: img.height(),
        });
        platform.logo = Some((img.width() as usize, img.height() as usize, img.as_raw().clone()));
    }
    let options = eframe::NativeOptions {
        viewport: viewport.with_title("ActCraft").with_inner_size([1280.0, 800.0]).with_min_inner_size([800.0, 520.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let result = eframe::run_native(
        "ActCraft",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || ctx.request_repaint()));
            let mut app = ActCraftApp::new(&cc.egui_ctx, engine, platform);
            if let Some(tab) = &open_settings {
                app.open_settings_at(tab);
            }
            if show_lyrics {
                app.show_lyrics();
            }
            if sheet {
                app.show_sheet();
            }
            if edit_lyrics {
                app.edit_lyrics();
            }
            // `--stage` opens the second window; with `--sheet` it shows the sheet music.
            if stage && sheet {
                app.show_stage_sheet();
            } else if stage {
                app.toggle_stage_screen();
            }
            if cues {
                app.show_cues();
            }
            if about {
                app.show_about();
            }
            if dmx {
                app.show_dmx_monitor();
            }
            if fixtures {
                app.show_fixtures();
            }
            if new_fixture_type {
                app.show_type_editor();
            }
            if presets {
                app.show_presets();
            }
            if light_cues {
                app.show_light_cues();
            }
            if stage_view {
                app.show_stage_view();
            }
            let repaint = cc.egui_ctx.clone();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || repaint.request_repaint());
            let mut host = Host {
                app,
                _audio: audio,
                screenshot,
                dialogs,
                prefs,
                midi: None,
                status,
                midi_part: None,
                midi_in: None,
                midi_queue: MidiQueue::default(),
                osc_in: None,
                osc_queue: OscQueue::default(),
                dmx_in: None,
                dmx_in_levels: DmxInLevels::default(),
                gamepads: Gamepads::open().ok(),
                hue_job: None,
                hue_out: None,
                band_host: None,
                sheets_web: None,
                web_scores: std::collections::HashMap::new(),
                web_score_tick: std::time::Instant::now(),
                band_follow: None,
                band_connecting: None,
                band_retry: std::time::Instant::now(),
                dmx_out: None,
                dmx_sent: None,
                dmx_reported: Vec::new(),
                artnet_search: None,
                wake,
            };
            // The MIDI output and input chosen last time, if they are still there.
            host.app.platform.midi_ports = actcraft_platform::midi_output_ports();
            host.app.platform.midi_inputs = actcraft_platform::midi_input_ports();
            let saved = host.prefs.get().audio.midi_output.clone();
            if !saved.is_empty() && host.app.platform.midi_ports.contains(&saved) {
                host.switch_midi(Some(saved));
            }
            let saved = host.prefs.get().audio.midi_input.clone();
            if !saved.is_empty() && host.app.platform.midi_inputs.contains(&saved) {
                host.switch_midi_input(Some(saved));
            }
            // OSC as it was set last time.
            let pr = host.prefs.get().audio.clone();
            host.app.platform.osc_port = if pr.osc_port == 0 { DEFAULT_OSC_PORT } else { pr.osc_port };
            host.app.platform.osc_network = pr.osc_network;
            if pr.osc {
                host.switch_osc(Some((host.app.platform.osc_port, pr.osc_network)));
            }
            host.app.platform.dmx_in_network = pr.dmx_in_network;
            if pr.dmx_in {
                host.switch_dmx_in(Some(pr.dmx_in_network));
            }
            // A Hue bridge paired on this computer before: its lights are read again.
            // Band link as it was: hosting again, or following the same host again.
            if pr.band_host {
                host.switch_band_host(true);
            }
            if pr.sheets_web {
                host.switch_sheets_web(true);
            }
            if !pr.band_follow.is_empty() && !pr.band_token.is_empty() {
                host.app.platform.band_following = Some((pr.band_follow.clone(), false));
            }
            if !pr.hue_key.is_empty() && !pr.hue_bridge.is_empty() {
                host.app.platform.hue_paired = Some(pr.hue_bridge.clone());
                host.start_hue(actcraft_ui::HueRequest::Lights);
            }
            Ok(Box::new(host))
        }),
    );
    if let Err(e) = result {
        eprintln!("ActCraft could not open its window: {e}");
        std::process::exit(1);
    }
}

/// What a Hue request running on its own thread came back with.
enum HueAnswer {
    Found(Result<Vec<std::net::Ipv4Addr>, String>),
    Paired(String, Result<String, String>),
    Lights(Result<Vec<actcraft_platform::hue::HueLight>, String>),
}

/// A band host connected to: the follower, the address as typed, and the token a pairing gave.
struct BandConnected {
    follow: actcraft_platform::band::BandFollow,
    addr: String,
    token: Option<String>,
}
