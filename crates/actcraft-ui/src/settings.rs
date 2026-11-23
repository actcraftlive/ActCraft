//! The Settings window: audio device and outputs, metronome, playback and library. Everything
//! but the device choice is an engine command; the device is the host's (it owns the stream).

use actcraft_engine::Engine;
use actcraft_library::queue::DoubleTap;
use actcraft_show::{RemoteKind, RemoteTrigger};
use egui::{RichText, Ui};
use serde_json::json;

use crate::{ActCraftApp, edit_text, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Audio,
    Metronome,
    Voice,
    Playback,
    Midi,
    Macros,
    Library,
}

impl Tab {
    /// A tab by its English name in lower case.
    pub(crate) fn named(name: &str) -> Option<Tab> {
        Some(match name {
            "audio" => Tab::Audio,
            "metronome" => Tab::Metronome,
            "voice" => Tab::Voice,
            "playback" => Tab::Playback,
            "midi" => Tab::Midi,
            "macros" => Tab::Macros,
            "library" => Tab::Library,
            _ => return None,
        })
    }
}

/// A remote action being put together: its trigger (channel 0 for any) and command, and whether
/// it waits to learn the trigger from the controller.
#[derive(Debug, Clone, Default)]
pub(crate) struct RemoteDraft {
    pub kind: RemoteKind,
    pub channel: u8,
    pub number: u8,
    pub command: String,
    pub learning: bool,
}

impl RemoteDraft {
    /// Takes a learnt trigger (with its channel) and stops waiting.
    pub fn learn(&mut self, t: RemoteTrigger) {
        self.kind = t.kind;
        self.number = t.number;
        self.channel = t.channel.map_or(0, |c| c + 1);
        self.learning = false;
    }
}

fn kind_id(k: RemoteKind) -> &'static str {
    match k {
        RemoteKind::Note => "note",
        RemoteKind::Cc => "cc",
        RemoteKind::Program => "program",
    }
}

fn kind_text(k: RemoteKind) -> &'static str {
    match k {
        RemoteKind::Note => tr("Note"),
        RemoteKind::Cc => tr("Controller"),
        RemoteKind::Program => tr("Program change"),
    }
}

/// A trigger in words: "Note 36, channel 10", "Controller 64, any channel".
fn trigger_text(t: &RemoteTrigger) -> String {
    let channel = match t.channel {
        Some(c) => actcraft_engine::fill(tr("channel {channel}"), &[("channel", &(c + 1).to_string())]),
        None => tr("any channel").to_owned(),
    };
    format!("{} {}, {channel}", kind_text(t.kind), t.number)
}

/// Speech languages offered for spoken cues: (code, name in that language); empty is the
/// computer's own.
const VOICE_LANGUAGES: [(&str, &str); 6] =
    [("", ""), ("de", "Deutsch"), ("en", "English"), ("fr", "Français"), ("es", "Español"), ("it", "Italiano")];

/// Name of an output pair for a bus routed to `first` (0-based) on a device with `channels`.
fn output_label(first: u16, channels: u16) -> String {
    if channels <= 1 {
        return tr("Output 1").to_owned();
    }
    // A pair the interface doesn't have plays on 1/2, as the renderer does.
    let first = if first.saturating_add(1) < channels { first } else { 0 };
    format!("{} {}/{}", tr("Outputs"), first + 1, first + 2)
}

impl ActCraftApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Settings"))
            .open(&mut open)
            .default_width(680.0)
            .default_height(600.0)
            .min_height(420.0)
            .collapsible(false)
            .show(ctx, |ui| {
                self.language_row(ui);
                ui.separator();
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (Tab::Audio, tr("Audio")),
                        (Tab::Metronome, tr("Metronome")),
                        (Tab::Voice, tr("Spoken cues")),
                        (Tab::Playback, tr("Playback")),
                        (Tab::Midi, tr("MIDI")),
                        (Tab::Macros, tr("Macros")),
                        (Tab::Library, tr("Library")),
                    ] {
                        ui.selectable_value(&mut self.settings_tab, tab, label);
                    }
                });
                ui.separator();
                match self.settings_tab {
                    Tab::Audio => self.audio_settings(ui, e),
                    Tab::Metronome => self.metronome_settings(ui, e),
                    Tab::Voice => self.voice_settings(ui, e),
                    Tab::Playback => self.playback_settings(ui, e),
                    Tab::Midi => self.midi_settings(ui, e),
                    Tab::Macros => self.macro_settings(ui, e),
                    Tab::Library => self.library_settings(ui, e),
                }
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.settings_open = false;
        }
    }

    /// Spoken cues: the computer's voice says a song's title and each part's words.
    fn voice_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let v = e.show.voice.clone();
        ui.label(
            RichText::new(tr("The computer's own voice says a song's title when it starts (tick “Announce the title” in the song details) and a part's words shortly before it begins (the “Spoken” column of the regions, such as “Chorus” or “End 3 2 1”). Each phrase is spoken once and kept."))
                .color(t.text_dim),
        );
        ui.add_space(6.0);
        let mut enabled = v.enabled;
        if ui.checkbox(&mut enabled, tr("Spoken cues on")).changed() {
            self.run(e, "voice.set", json!({"enabled": enabled}));
        }
        egui::Grid::new("voice-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(tr("Voice goes to bus"));
            let current = e.show.buses.get(usize::from(v.bus)).map_or_else(String::new, |b| b.name.clone());
            egui::ComboBox::from_id_salt("voice-bus").selected_text(current).show_ui(ui, |ui| {
                for (b, bus) in e.show.buses.clone().iter().enumerate() {
                    if ui.selectable_label(usize::from(v.bus) == b, &bus.name).clicked() {
                        self.run(e, "voice.set", json!({"bus": b}));
                    }
                }
            });
            ui.end_row();
            ui.label(tr("Voice volume"));
            let mut vol = v.volume_db;
            if ui.add(egui::Slider::new(&mut vol, -40.0..=12.0).suffix(" dB")).changed() {
                self.run(e, "voice.set", json!({"volume_db": vol}));
            }
            ui.end_row();
            ui.label(tr("Speak before a part"));
            let mut lead = u32::from(v.lead_beats);
            if ui.add(egui::DragValue::new(&mut lead).range(0..=32).suffix(format!(" {}", tr("beats")))).changed() {
                self.run(e, "voice.set", json!({"lead_beats": lead}));
            }
            ui.end_row();
            ui.label(tr("Speech language"));
            let name =
                |code: &str, n: &str| if code.is_empty() { tr("The computer's own").to_owned() } else { n.to_owned() };
            let shown = VOICE_LANGUAGES
                .iter()
                .find(|(c, _)| *c == v.language)
                .map_or_else(|| v.language.clone(), |(c, n)| name(c, n));
            egui::ComboBox::from_id_salt("voice-language").selected_text(shown).show_ui(ui, |ui| {
                for (code, n) in VOICE_LANGUAGES {
                    if ui.selectable_label(v.language == code, name(code, n)).clicked() {
                        self.run(e, "voice.set", json!({"language": code}));
                    }
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(e.current_song.is_some(), egui::Button::new(tr("Speak this song's cues now")))
                .on_hover_text(tr("They are also made when the song loads"))
                .clicked()
            {
                self.run(e, "voice.prepare", serde_json::Value::Null);
            }
            let pending = e.voices_pending();
            if pending > 0 {
                ui.spinner();
                let n = pending.to_string();
                ui.label(actcraft_engine::fill(tr("{count} phrases being spoken…"), &[("count", &n)]));
            }
        });
    }

    /// The window's language; the host keeps the choice on this computer.
    fn language_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Language")).strong());
            // Language names are in their own language; only "System language" is translated.
            let shown = |value: &str, name: &str| {
                if value == crate::i18n::SYSTEM { tr("System language").to_owned() } else { name.to_owned() }
            };
            let choices = crate::i18n::choices();
            let current = choices
                .iter()
                .find(|(v, _)| *v == self.platform.language)
                .map_or_else(|| "English".to_owned(), |(v, n)| shown(v, n));
            egui::ComboBox::from_id_salt("language").width(200.0).selected_text(current).show_ui(ui, |ui| {
                for (value, name) in &choices {
                    if ui.selectable_label(self.platform.language == *value, shown(value, name)).clicked() {
                        self.platform.language.clone_from(value);
                        crate::i18n::set_language(value);
                        self.language_request = Some(value.clone());
                    }
                }
            });
            ui.label(
                RichText::new(tr("More languages: add a language file (see docs/translating.md)"))
                    .color(self.tokens.text_dim),
            );
        });
    }

    fn audio_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Audio interface")).strong());
        let current = self.platform.audio_device.clone().unwrap_or_else(|| tr("No output").to_owned());
        egui::ComboBox::from_id_salt("audio-device").width(360.0).selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(self.platform.device_id.is_none(), tr("System default")).clicked() {
                self.device_request = Some(None);
            }
            for (id, name) in self.platform.devices.clone() {
                let selected = self.platform.device_id.as_deref() == Some(id.as_str());
                if ui.selectable_label(selected, &name).clicked() {
                    self.device_request = Some(Some(id));
                }
            }
        });
        let channels = self.platform.output_channels;
        ui.label(
            RichText::new(format!(
                "{} {} \u{00b7} {} Hz",
                channels,
                if channels == 1 { tr("output channel") } else { tr("output channels") },
                self.platform.sample_rate
            ))
            .color(t.text_dim),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Output buses")).strong());
        ui.label(RichText::new(tr("Each track goes to a bus (chosen on its mixer strip); each bus goes to a pair of outputs on the interface.")).color(t.text_dim));
        egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
            egui::Grid::new("buses").num_columns(4).spacing([12.0, 6.0]).striped(true).show(ui, |ui| {
                ui.label(RichText::new(tr("Bus")).strong());
                ui.label(RichText::new(tr("Goes to")).strong());
                ui.label(RichText::new(tr("Volume")).strong());
                ui.label(RichText::new(tr("Mute")).strong());
                ui.end_row();
                for (b, bus) in e.show.buses.clone().iter().enumerate() {
                    if let Some(name) = edit_text(ui, egui::Id::unique(("bus-name", b)), &bus.name, 140.0) {
                        let _ = e.run("bus.set", &json!({"bus": b, "name": name}));
                    }
                    let pairs = (channels.max(1) / 2).max(1);
                    egui::ComboBox::from_id_salt(("bus-out", b))
                        .width(140.0)
                        .selected_text(output_label(bus.output, channels))
                        .show_ui(ui, |ui| {
                            for pair in 0..pairs {
                                let first = pair * 2;
                                if ui.selectable_label(bus.output == first, output_label(first, channels)).clicked() {
                                    let _ = e.run("bus.set", &json!({"bus": b, "output": first}));
                                }
                            }
                        });
                    let mut vol = bus.volume_db;
                    if ui.add(egui::Slider::new(&mut vol, -60.0..=12.0).suffix(" dB")).changed() {
                        let _ = e.run("bus.set", &json!({"bus": b, "volume_db": vol}));
                    }
                    let mut mute = bus.mute;
                    if ui.checkbox(&mut mute, "").on_hover_text(tr("Mute this bus")).changed() {
                        let _ = e.run("bus.set", &json!({"bus": b, "mute": mute}));
                    }
                    ui.end_row();
                }
            });
        });
    }

    fn metronome_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let m = e.show.metronome.clone();
        let mut enabled = m.enabled;
        if ui.checkbox(&mut enabled, tr("Metronome on")).changed() {
            let _ = e.run("metronome.set", &json!({"enabled": enabled}));
        }
        ui.label(
            RichText::new(tr("Each song sets when its click sounds (whole song, count-in only, visual only or off) and how many bars it counts in: see the song details above the mixer."))
                .color(self.tokens.text_dim),
        );
        let mut free = m.run_while_stopped;
        if ui.checkbox(&mut free, tr("Keep clicking while stopped")).changed() {
            let _ = e.run("metronome.set", &json!({"run_while_stopped": free}));
        }
        let mut vol = m.volume_db;
        if ui.add(egui::Slider::new(&mut vol, -60.0..=6.0).suffix(" dB").text(tr("Click volume"))).changed() {
            let _ = e.run("metronome.set", &json!({"volume_db": vol}));
        }
        ui.horizontal(|ui| {
            ui.label(tr("Click goes to bus"));
            let current = e.show.buses.get(usize::from(m.bus)).map_or_else(String::new, |b| b.name.clone());
            egui::ComboBox::from_id_salt("click-bus").selected_text(current).show_ui(ui, |ui| {
                for (b, bus) in e.show.buses.clone().iter().enumerate() {
                    if ui.selectable_label(usize::from(m.bus) == b, &bus.name).clicked() {
                        let _ = e.run("metronome.set", &json!({"bus": b}));
                    }
                }
            });
            let out = e.show.buses.get(usize::from(m.bus)).map_or(0, |b| b.output);
            ui.label(
                RichText::new(format!("({})", output_label(out, self.platform.output_channels)))
                    .color(self.tokens.text_dim),
            );
        });
        // Starting on the beat needs a click that's already running.
        let mut quant = m.quantized_start;
        let check = egui::Checkbox::new(&mut quant, tr("Start playback on the next beat of the running click"));
        if ui.add_enabled(m.run_while_stopped, check).changed() {
            let _ = e.run("metronome.set", &json!({"quantized_start": quant}));
        }
        ui.horizontal(|ui| {
            ui.label(tr("Click timing offset"));
            let mut lat = m.latency_offset_ms;
            if ui
                .add(egui::DragValue::new(&mut lat).range(-500.0..=500.0).speed(0.5).suffix(" ms"))
                .on_hover_text(tr("Positive moves every click earlier (to make up for output latency)"))
                .changed()
            {
                let _ = e.run("metronome.set", &json!({"latency_offset_ms": lat}));
            }
        });
    }

    /// MIDI: the output on this computer, and what time the show sends to it while it plays.
    fn midi_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("MIDI output")).strong());
        ui.horizontal(|ui| {
            let none = tr("None");
            let current = self.platform.midi_port.clone().unwrap_or_else(|| none.to_owned());
            let label = ui.label(tr("Send to"));
            egui::ComboBox::from_id_salt("midi-port")
                .width(300.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.platform.midi_port.is_none(), none).clicked() {
                        self.midi_request = Some(None);
                    }
                    for name in self.platform.midi_ports.clone() {
                        let selected = self.platform.midi_port.as_deref() == Some(name.as_str());
                        if ui.selectable_label(selected, &name).clicked() {
                            self.midi_request = Some(Some(name));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if ui.button(tr("Look again")).on_hover_text(tr("Find MIDI outputs connected since")).clicked() {
                self.midi_refresh = true;
            }
        });
        if self.platform.midi_ports.is_empty() {
            ui.label(RichText::new(tr("No MIDI outputs found on this computer.")).color(t.text_dim));
        }
        ui.label(
            RichText::new(tr("The output is kept on this computer; what is sent is kept with the show."))
                .color(t.text_dim),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Send while a song plays")).strong());
        let midi = e.show.midi.clone();
        let mut clock = midi.clock;
        if ui
            .checkbox(&mut clock, tr("MIDI Clock"))
            .on_hover_text(tr("24 pulses per beat with start, stop and the song position, so other gear plays in time"))
            .changed()
        {
            self.run(e, "midi.set", serde_json::json!({"clock": clock}));
        }
        ui.horizontal(|ui| {
            let mut timecode = midi.timecode;
            if ui
                .checkbox(&mut timecode, tr("MIDI Timecode"))
                .on_hover_text(tr("The song time as hours, minutes, seconds and frames"))
                .changed()
            {
                self.run(e, "midi.set", serde_json::json!({"timecode": timecode}));
            }
            let label = ui.label(tr("Frames per second"));
            let mut fps = midi.timecode_fps;
            egui::ComboBox::from_id_salt("midi-fps")
                .selected_text(fps.to_string())
                .show_ui(ui, |ui| {
                    for rate in actcraft_show::MidiSettings::RATES {
                        ui.selectable_value(&mut fps, rate, rate.to_string());
                    }
                })
                .response
                .labelled_by(label.id);
            if fps != midi.timecode_fps {
                self.run(e, "midi.set", serde_json::json!({"timecode_fps": fps}));
            }
        });
        ui.horizontal(|ui| {
            let label = ui.label(tr("MIDI file offset"));
            let mut ms = e.show.mixer.midi_latency_offset_ms;
            if ui
                .add(egui::DragValue::new(&mut ms).range(-500.0..=500.0).speed(1.0).suffix(" ms"))
                .labelled_by(label.id)
                .on_hover_text(tr(
                    "Plays the songs' MIDI files earlier (minus) or later, to meet the sound of the gear they drive",
                ))
                .changed()
            {
                self.run(e, "midi.set", serde_json::json!({"latency_offset_ms": ms}));
            }
        });
        ui.add_space(12.0);
        self.remote_settings(ui, e);
    }

    /// Macros: named lists of commands, called with `$NAME` at a timecoded lyrics line.
    fn macro_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(
            RichText::new(tr(
                "Write $NAME at a timecoded line of a song's lyrics, and the macro NAME runs when playback gets there.",
            ))
            .color(t.text_dim),
        );
        let label_of = |id: &str| tr(actcraft_engine::registry().get(id).map_or(id, |c| c.spec.label)).to_owned();
        let macros = e.show.macros.clone();
        let (mut run, mut remove, mut edit) = (None, None, None);
        egui::Grid::new("macros").num_columns(4).striped(true).show(ui, |ui| {
            for m in &macros {
                ui.label(RichText::new(format!("${}", m.name)).strong());
                let steps: Vec<String> = m.commands.iter().map(|c| label_of(c)).collect();
                ui.label(steps.join(" \u{b7} "));
                ui.horizontal(|ui| {
                    if ui.button(tr("Run")).clicked() {
                        run = Some(m.name.clone());
                    }
                    if ui.button(tr("Change")).clicked() {
                        edit = Some((m.name.clone(), m.commands.clone()));
                    }
                    if ui.button(tr("Remove")).clicked() {
                        remove = Some(m.name.clone());
                    }
                });
                ui.end_row();
            }
        });
        if let Some(name) = run {
            self.run(e, "macro.run", json!({"name": name}));
        }
        if let Some(name) = remove {
            self.run(e, "macro.remove", json!({"name": name}));
        }
        if let Some(draft) = edit {
            self.macro_draft = draft;
        }
        ui.add_space(8.0);
        ui.label(RichText::new(tr("New or changed macro")).strong());
        let (name, commands) = &mut self.macro_draft;
        ui.horizontal(|ui| {
            let label = ui.label(tr("Name"));
            ui.add(egui::TextEdit::singleline(name).desired_width(160.0).hint_text("LIGHTS_UP")).labelled_by(label.id);
            *name =
                name.to_ascii_uppercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(32).collect();
        });
        let mut drop_step = None;
        for (i, c) in commands.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}. {}", i + 1, label_of(c)));
                if ui.small_button(tr("Remove")).clicked() {
                    drop_step = Some(i);
                }
            });
        }
        if let Some(i) = drop_step {
            commands.remove(i);
        }
        ui.horizontal(|ui| {
            let label = ui.label(tr("Add a command"));
            let mut choices: Vec<(&'static str, &'static str)> = actcraft_engine::registry()
                .iter()
                .filter(|c| actcraft_engine::remote::runnable(c.spec.id))
                .map(|c| (c.spec.id, tr(c.spec.label)))
                .collect();
            choices.sort_by(|a, b| a.1.cmp(b.1));
            egui::ComboBox::from_id_salt("macro-command")
                .width(260.0)
                .selected_text(tr("Choose a command"))
                .show_ui(ui, |ui| {
                    for (id, label) in &choices {
                        if ui.selectable_label(false, *label).clicked() && commands.len() < 32 {
                            commands.push((*id).to_owned());
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let ready = actcraft_show::Macro::valid_name(&self.macro_draft.0);
        if ui.add_enabled(ready, egui::Button::new(tr("Save macro"))).clicked() {
            let (name, commands) = std::mem::take(&mut self.macro_draft);
            self.run(e, "macro.set", json!({"name": name, "commands": commands}));
        }
    }

    /// Remote actions: the MIDI input a controller is on, and what its messages set off.
    fn remote_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Remote actions")).strong());
        ui.label(
            RichText::new(tr("A foot switch, a pad or a key on a MIDI controller sets off a command."))
                .color(t.text_dim),
        );
        ui.horizontal(|ui| {
            let none = tr("None");
            let current = self.platform.midi_input.clone().unwrap_or_else(|| none.to_owned());
            let label = ui.label(tr("Listen to"));
            egui::ComboBox::from_id_salt("midi-input")
                .width(300.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.platform.midi_input.is_none(), none).clicked() {
                        self.midi_input_request = Some(None);
                    }
                    for name in self.platform.midi_inputs.clone() {
                        let selected = self.platform.midi_input.as_deref() == Some(name.as_str());
                        if ui.selectable_label(selected, &name).clicked() {
                            self.midi_input_request = Some(Some(name));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        // The actions there are.
        let actions = e.show.remote.clone();
        let mut remove = None;
        egui::Grid::new("remote-actions").num_columns(3).striped(true).show(ui, |ui| {
            for (i, a) in actions.iter().enumerate() {
                ui.label(trigger_text(&a.trigger));
                let label = actcraft_engine::registry().get(&a.command).map_or(a.command.as_str(), |c| c.spec.label);
                ui.label(tr(label));
                if ui.button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.run(e, "remote.remove", json!({"action": i}));
        }
        // A new one: its trigger, learnt from the controller or set by hand, and its command.
        ui.add_space(6.0);
        let d = &mut self.remote_draft;
        ui.horizontal_wrapped(|ui| {
            let learn = if d.learning { tr("Waiting for the controller\u{2026}") } else { tr("Learn") };
            if ui
                .selectable_label(d.learning, learn)
                .on_hover_text(tr("Press the switch, pad or key to use; its message fills in the fields"))
                .clicked()
            {
                d.learning = !d.learning;
            }
            let label = ui.label(tr("Message"));
            egui::ComboBox::from_id_salt("remote-kind")
                .selected_text(kind_text(d.kind))
                .show_ui(ui, |ui| {
                    for k in [RemoteKind::Note, RemoteKind::Cc, RemoteKind::Program] {
                        ui.selectable_value(&mut d.kind, k, kind_text(k));
                    }
                })
                .response
                .labelled_by(label.id);
            let label = ui.label(tr("Number"));
            ui.add(egui::DragValue::new(&mut d.number).range(0..=127)).labelled_by(label.id);
            let label = ui.label(tr("Channel"));
            let any = tr("any");
            ui.add(
                egui::DragValue::new(&mut d.channel)
                    .range(0..=16)
                    .custom_formatter(move |v, _| if v < 0.5 { any.to_owned() } else { format!("{v:.0}") }),
            )
            .labelled_by(label.id)
            .on_hover_text(tr("0 answers on every channel"));
        });
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Runs"));
            let mut commands: Vec<(&'static str, &'static str)> = actcraft_engine::registry()
                .iter()
                .filter(|c| actcraft_engine::remote::runnable(c.spec.id))
                .map(|c| (c.spec.id, tr(c.spec.label)))
                .collect();
            commands.sort_by(|a, b| a.1.cmp(b.1));
            let current = commands.iter().find(|c| c.0 == d.command).map_or(tr("Choose a command"), |c| c.1);
            egui::ComboBox::from_id_salt("remote-command")
                .width(260.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (id, name) in &commands {
                        if ui.selectable_label(d.command == *id, *name).clicked() {
                            d.command = (*id).to_owned();
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let ready = !self.remote_draft.command.is_empty();
        if ui.add_enabled(ready, egui::Button::new(tr("Add remote action"))).clicked() {
            let d = &self.remote_draft;
            let mut prm = json!({"kind": kind_id(d.kind), "number": d.number, "command": d.command});
            if d.channel > 0 {
                prm["channel"] = json!(d.channel);
            }
            self.remote_draft.learning = false;
            self.run(e, "remote.add", prm);
        }
    }

    fn playback_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let mut xf = e.show.transitions.crossfade;
        ui.horizontal(|ui| {
            ui.label(tr("Crossfade when changing songs during playback"));
            if ui.add(egui::DragValue::new(&mut xf).range(0.0..=30.0).speed(0.1).suffix(" s")).changed() {
                let _ = e.run("transport.set_crossfade", &json!({"seconds": xf}));
            }
        });
        let mut guard = e.play_guard_ms;
        ui.horizontal(|ui| {
            ui.label(tr("Ignore a second Play/Stop within"));
            if ui.add(egui::DragValue::new(&mut guard).range(0..=5000).speed(10.0).suffix(" ms")).changed() {
                let _ = e.run("transport.set_play_guard", &json!({"ms": guard}));
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("Double-click on a song"));
            let options = [
                (DoubleTap::Load, "load", tr("loads it")),
                (DoubleTap::PlayNext, "play_next", tr("plays it next")),
                (DoubleTap::AddToQueue, "add_to_queue", tr("adds it to the queue")),
                (DoubleTap::Ignore, "ignore", tr("does nothing")),
            ];
            let current = options.iter().find(|o| o.0 == e.double_tap).map_or("", |o| o.2);
            egui::ComboBox::from_id_salt("double-tap").selected_text(current).show_ui(ui, |ui| {
                for (mode, key, label) in options {
                    if ui.selectable_label(e.double_tap == mode, label).clicked() {
                        let _ = e.run("queue.set_double_tap", &json!({"mode": key}));
                    }
                }
            });
        });
    }

    fn library_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        ui.label(RichText::new(tr("Song details from file names")).strong());
        ui.label(RichText::new(tr("When you add audio files, their names are read with this pattern. Use %title%, %artist%, %key%, %bpm%, %year% and %ignore%.")).color(self.tokens.text_dim));
        let current = e.filename_format.clone();
        if let Some(v) = edit_text(ui, egui::Id::unique("filename-format"), &current, 320.0) {
            let _ = e.run("library.set_filename_format", &json!({"format": v}));
        }
        ui.label(
            RichText::new(format!("{} \u{201c}Ana Ruiz - Night Drive.wav\u{201d}", tr("Example:")))
                .color(self.tokens.text_dim),
        );
    }
}
