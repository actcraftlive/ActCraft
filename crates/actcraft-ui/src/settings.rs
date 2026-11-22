//! The Settings window: audio device and outputs, metronome, playback and library. Everything
//! but the device choice is an engine command; the device is the host's (it owns the stream).

use actcraft_engine::Engine;
use actcraft_library::queue::DoubleTap;
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
    Library,
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
