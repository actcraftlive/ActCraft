//! The Spoken cues tab: everything about a song's spoken cues in one place. The title
//! announcement, quick buttons that add a cue ("Chorus", "End 3 2 1") at the playback position,
//! and every part of the song with the words said before it and a button to hear it.

use actcraft_engine::Engine;
use actcraft_show::Song;
use egui::{RichText, Ui};
use serde_json::json;

use crate::{ActCraftApp, edit_text, fmt_time, tr};

/// Ready-made cue words, in English; shown and spoken in the window's language.
const PRESETS: [&str; 8] = ["Intro", "Verse", "Chorus", "Bridge", "Solo", "Interlude", "Outro", "End 3 2 1"];

/// Seconds of music played before a cue when listening to it.
const PRE_ROLL: f64 = 1.5;

impl ActCraftApp {
    pub(crate) fn cues_tab(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        let id = song.id;
        let voice = e.show.voice.clone();
        ui.label(
            RichText::new(tr("The computer's voice says these words on the voice bus: the title when you start the song (as the count-in begins), and each part's words a few beats before the part begins."))
                .color(t.text_dim),
        );
        if !voice.enabled {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Spoken cues are switched off for this show.")).color(t.warning));
                if ui.button(tr("Switch on")).clicked() {
                    self.run(e, "voice.set", json!({"enabled": true}));
                }
            });
        }
        ui.add_space(6.0);

        // The title.
        let mut announce = song.announce_title;
        if ui
            .checkbox(
                &mut announce,
                format!("{} \u{201c}{}\u{201d}", tr("Say the title when the song is started:"), song.title),
            )
            .changed()
        {
            self.run(e, "song.set", json!({"song": id, "announce_title": announce}));
        }

        // Adding a cue at the playback position.
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Add a spoken cue where the song is now")).strong());
        let now = e.status.time.max(0.0);
        ui.label(
            RichText::new(actcraft_engine::fill(
                tr("Play or click the waveform to the place where the part starts ({time}), then pick a word or type your own."),
                &[("time", &fmt_time(now))],
            ))
            .color(t.text_dim),
        );
        // The waveform with its parts: click to set the place.
        self.waveform(ui, e, song);
        ui.horizontal_wrapped(|ui| {
            for word in PRESETS {
                if ui.button(tr(word)).on_hover_text(tr("Add this cue at the playback position")).clicked() {
                    self.add_cue(e, id, now, tr(word));
                }
            }
        });
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_cue).hint_text(tr("Your own words")).desired_width(220.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let words = self.new_cue.trim().to_owned();
            if (ui.add_enabled(!words.is_empty(), egui::Button::new(tr("Add"))).clicked() || enter) && !words.is_empty()
            {
                self.add_cue(e, id, now, &words);
                self.new_cue.clear();
            }
        });

        // Every part with its words.
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Parts of the song")).strong());
        if song.regions.is_empty() {
            ui.label(
                RichText::new(tr(
                    "No parts yet. Add a cue above; each cue becomes a part of the song you can also jump to.",
                ))
                .color(t.text_dim),
            );
        } else {
            let lead = f64::from(voice.lead_beats) * 60.0 / song.bpm.max(1.0);
            egui::Grid::new(("cue-grid", id)).num_columns(5).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
                for h in ["Part", "Starts at", "Spoken", "", ""] {
                    ui.label(RichText::new(tr(h)).strong());
                }
                ui.end_row();
                for (i, r) in song.regions.iter().enumerate() {
                    ui.label(&r.name);
                    ui.label(RichText::new(fmt_time(r.start)).monospace());
                    if let Some(words) = edit_text(ui, egui::Id::unique(("cue-words", id, i)), &r.voice, 200.0) {
                        self.run(e, "region.set", json!({"song": id, "region": i, "voice": words}));
                    }
                    let has_words = !r.voice.trim().is_empty();
                    let cue_at = (r.start - lead).max(song.start.max(0.0));
                    if ui
                        .add_enabled(has_words, egui::Button::new(tr("Listen")))
                        .on_hover_text(tr("Play from just before this cue"))
                        .clicked()
                    {
                        self.listen_from(e, (cue_at - PRE_ROLL).max(0.0));
                    }
                    if has_words {
                        if ui.small_button(tr("No words")).on_hover_text(tr("Say nothing before this part")).clicked() {
                            self.run(e, "region.set", json!({"song": id, "region": i, "voice": ""}));
                        }
                    } else if ui.small_button(tr("Say its name")).clicked() {
                        self.run(e, "region.set", json!({"song": id, "region": i, "voice": r.name}));
                    }
                    ui.end_row();
                }
            });
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let pending = e.voices_pending();
            if pending > 0 {
                ui.spinner();
                let n = pending.to_string();
                ui.label(actcraft_engine::fill(tr("{count} phrases being spoken…"), &[("count", &n)]));
            }
            if ui.button(tr("Voice, bus and timing\u{2026}")).on_hover_text(tr("Settings > Spoken cues")).clicked() {
                self.settings_tab = crate::settings::Tab::Voice;
                self.settings_open = true;
            }
        });
    }

    /// Adds a part at `at` whose words are spoken before it, named after the words.
    fn add_cue(&mut self, e: &mut Engine, song: u64, at: f64, words: &str) {
        self.run(e, "region.add", json!({"song": song, "start": at, "name": words, "voice": words}));
        let _ = e.run("voice.prepare", &json!({"song": song}));
    }

    /// Plays from `seconds` without a count-in, to hear a cue.
    fn listen_from(&mut self, e: &mut Engine, seconds: f64) {
        self.run(e, "transport.play", json!({"count_in": false, "from": seconds}));
    }
}
