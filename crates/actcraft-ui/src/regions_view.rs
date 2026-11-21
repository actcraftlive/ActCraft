//! Regions in the window: a bar of parts to jump to live, the leave-loop button, and an editor.

use actcraft_engine::Engine;
use actcraft_show::{RegionMode, Song};
use egui::{RichText, Ui};
use serde_json::json;

use crate::{ActCraftApp, edit_text, fmt_time, theme, tr};

pub(crate) fn mode_label(m: RegionMode) -> &'static str {
    match m {
        RegionMode::Play => "Play",
        RegionMode::Loop => "Loop",
        RegionMode::Skip => "Skip",
        RegionMode::StopAfter => "Stop after",
    }
}

fn mode_key(m: RegionMode) -> &'static str {
    match m {
        RegionMode::Play => "play",
        RegionMode::Loop => "loop",
        RegionMode::Skip => "skip",
        RegionMode::StopAfter => "stop_after",
    }
}

const MODES: [RegionMode; 4] = [RegionMode::Play, RegionMode::Loop, RegionMode::Skip, RegionMode::StopAfter];

impl ActCraftApp {
    /// Parts of the song as buttons: click to jump there (also while playing).
    pub(crate) fn region_bar(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        if song.regions.is_empty() && !e.status.in_loop {
            return;
        }
        let t = self.tokens;
        let now = e.status.time;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Parts")).strong());
            for (i, r) in song.regions.iter().enumerate() {
                let playing_here = e.current_song == Some(song.id) && r.start <= now && now < r.end;
                let mut text = r.name.clone();
                if r.mode != RegionMode::Play {
                    text = format!("{text} ({})", tr(mode_label(r.mode)).to_lowercase());
                }
                if r.cue {
                    text = format!("{}: {text}", tr("Cue"));
                }
                let colour = theme::colour(&r.colour, t.accent);
                let button = egui::Button::new(RichText::new(text).color(t.text))
                    .selected(playing_here)
                    .stroke(egui::Stroke::new(2.0, colour));
                if ui.add(button).on_hover_text(format!("{} {}", tr("Jump to"), fmt_time(r.start))).clicked() {
                    self.run(e, "region.jump", json!({"song": song.id, "region": i}));
                }
            }
            if e.status.in_loop {
                ui.separator();
                let leave =
                    egui::Button::new(RichText::new(tr("Leave loop")).strong().color(t.accent_text)).fill(t.warning);
                if ui.add(leave).on_hover_text(tr("Finish this pass of the loop and play on (L)")).clicked() {
                    self.run(e, "region.leave_loop", serde_json::Value::Null);
                }
            }
        });
    }

    /// Region list with every setting, under the waveform.
    pub(crate) fn region_editor(&mut self, ui: &mut Ui, e: &mut Engine, song: &Song) {
        let t = self.tokens;
        let id = song.id;
        let now = e.status.time.max(0.0);
        egui::CollapsingHeader::new(RichText::new(format!("{} ({})", tr("Regions"), song.regions.len())).strong())
            .id_salt(("regions", id))
            .default_open(!song.regions.is_empty())
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("Add region at playhead")).on_hover_text(tr("Two bars from the playback position (R)")).clicked() {
                        self.run(e, "region.add", json!({"song": id}));
                    }
                    if ui.button(tr("Regions from file markers")).on_hover_text(tr("Markers stored in the first track's WAV file")).clicked() {
                        self.run(e, "region.import_markers", json!({"song": id}));
                    }
                });
                if song.regions.is_empty() {
                    ui.label(RichText::new(tr("Regions name the parts of a song (verse, chorus, outro). They can loop, be skipped or stop playback, and you can jump to them live.")).color(t.text_dim));
                    return;
                }
                egui::Grid::new(("region-grid", id)).num_columns(8).spacing([8.0, 6.0]).striped(true).show(ui, |ui| {
                    for h in ["", "Name", "Start", "End", "What happens", "Cue", "", ""] {
                        ui.label(RichText::new(tr(h)).strong());
                    }
                    ui.end_row();
                    for (i, r) in song.regions.iter().enumerate() {
                        let mut c = theme::colour(&r.colour, t.accent).to_array();
                        let mut rgb = [c[0], c[1], c[2]];
                        if ui.color_edit_button_srgb(&mut rgb).on_hover_text(tr("Region colour")).changed() {
                            c = [rgb[0], rgb[1], rgb[2], 255];
                            let hex = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
                            self.run(e, "region.set", json!({"song": id, "region": i, "colour": hex}));
                        }
                        if let Some(name) = edit_text(ui, egui::Id::unique(("region-name", id, i)), &r.name, 140.0) {
                            self.run(e, "region.set", json!({"song": id, "region": i, "name": name}));
                        }
                        ui.horizontal(|ui| {
                            let mut s = r.start;
                            if ui.add(egui::DragValue::new(&mut s).range(0.0..=86_400.0).speed(0.01).fixed_decimals(2).suffix(" s")).changed() {
                                self.run(e, "region.set", json!({"song": id, "region": i, "start": s}));
                            }
                            if ui.small_button(tr("Here")).on_hover_text(tr("Start at the playback position")).clicked() {
                                self.run(e, "region.set", json!({"song": id, "region": i, "start": now}));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut en = r.end;
                            if ui.add(egui::DragValue::new(&mut en).range(0.0..=86_400.0).speed(0.01).fixed_decimals(2).suffix(" s")).changed() {
                                self.run(e, "region.set", json!({"song": id, "region": i, "end": en}));
                            }
                            if ui.small_button(tr("Here")).on_hover_text(tr("End at the playback position")).clicked() {
                                self.run(e, "region.set", json!({"song": id, "region": i, "end": now}));
                            }
                        });
                        egui::ComboBox::from_id_salt(("region-mode", id, i)).selected_text(tr(mode_label(r.mode))).show_ui(ui, |ui| {
                            for m in MODES {
                                if ui.selectable_label(r.mode == m, tr(mode_label(m))).clicked() {
                                    self.run(e, "region.set", json!({"song": id, "region": i, "mode": mode_key(m)}));
                                }
                            }
                        });
                        let mut cue = r.cue;
                        if ui.checkbox(&mut cue, "").on_hover_text(tr("Show as a cue for live jumps")).changed() {
                            self.run(e, "region.set", json!({"song": id, "region": i, "cue": cue}));
                        }
                        if ui.small_button(tr("Jump")).clicked() {
                            self.run(e, "region.jump", json!({"song": id, "region": i}));
                        }
                        if ui.small_button("\u{00d7}").on_hover_text(tr("Remove region")).clicked() {
                            self.run(e, "region.remove", json!({"song": id, "region": i}));
                        }
                        ui.end_row();
                    }
                });
            });
    }
}
