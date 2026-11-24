//! Sequences in the Presets window: each with Start or Stop, how it runs (loop, once, back and
//! forth; seconds or beats), and its steps: the look, its length and the part spent moving there,
//! moved up or down or removed. Record step stores the look on stage now as the next step.

use actcraft_engine::Engine;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::{ActCraftApp, tr};

impl ActCraftApp {
    pub(crate) fn sequences_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        crate::help_view::section_title(ui, tr("Sequences"), crate::help_view::Topic::Sequences);
        ui.label(
            RichText::new(tr(
                "Animations from looks: each step is a look the lights move to and hold. Set the lights, then Record step; a cue can start a sequence.",
            ))
            .color(t.text_dim),
        );
        let sequences = e.show.sequences.clone();
        let presets: Vec<String> = e.show.presets.iter().map(|p| p.name.clone()).collect();
        let running = e.running_sequences();
        let mut run: Option<(&str, Value)> = None;
        for (si, seq) in sequences.iter().enumerate() {
            let on = running.contains(&seq.name);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&seq.name).strong());
                let words = if on { tr("Stop") } else { tr("Start") };
                let r = ui.button(words);
                let name = format!("{words} ({})", seq.name);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if r.clicked() {
                    run = Some(("sequence.toggle", json!({"sequence": si})));
                }
                let label = ui.label(tr("Plays"));
                let mut mode = seq.mode;
                let mode_text = |m: actcraft_show::SequenceMode| match m {
                    actcraft_show::SequenceMode::Loop => tr("Over and over"),
                    actcraft_show::SequenceMode::Once => tr("Once"),
                    actcraft_show::SequenceMode::Bounce => tr("Back and forth"),
                };
                egui::ComboBox::from_id_salt(("sequence-mode", si))
                    .selected_text(mode_text(mode))
                    .show_ui(ui, |ui| {
                        for m in [
                            actcraft_show::SequenceMode::Loop,
                            actcraft_show::SequenceMode::Once,
                            actcraft_show::SequenceMode::Bounce,
                        ] {
                            ui.selectable_value(&mut mode, m, mode_text(m));
                        }
                    })
                    .response
                    .labelled_by(label.id);
                if mode != seq.mode {
                    let key = match mode {
                        actcraft_show::SequenceMode::Loop => "loop",
                        actcraft_show::SequenceMode::Once => "once",
                        actcraft_show::SequenceMode::Bounce => "bounce",
                    };
                    run = Some(("sequence.set", json!({"name": seq.name, "mode": key})));
                }
                let mut beats = seq.in_beats;
                if ui
                    .checkbox(&mut beats, tr("In beats"))
                    .on_hover_text(tr("Step lengths count beats of the song's click instead of seconds"))
                    .changed()
                {
                    run = Some(("sequence.set", json!({"name": seq.name, "in_beats": beats})));
                }
                let r = ui.small_button(tr("Remove"));
                let name = format!("{} ({})", tr("Remove"), seq.name);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if r.clicked() {
                    run = Some(("sequence.remove", json!({"sequence": si})));
                }
            });
            let unit = if seq.in_beats { tr(" beats") } else { " s" };
            egui::Grid::new(("sequence-steps", si)).num_columns(4).striped(true).show(ui, |ui| {
                let count = seq.steps.len();
                for (k, step) in seq.steps.iter().enumerate() {
                    let which = tr("Step {n}").replace("{n}", &(k + 1).to_string());
                    ui.label(&which);
                    // Its look.
                    let mut pick = presets.iter().position(|p| *p == step.preset);
                    let r = egui::ComboBox::from_id_salt(("sequence-step-look", si, k))
                        .width(140.0)
                        .selected_text(step.preset.clone())
                        .show_ui(ui, |ui| {
                            for (pi, name) in presets.iter().enumerate() {
                                ui.selectable_value(&mut pick, Some(pi), name);
                            }
                        })
                        .response;
                    let name = format!("{}, {which}", tr("Look"));
                    ui.ctx().accesskit_node_builder(r.id, |n| n.set_label(name));
                    if let Some(pi) = pick.filter(|pi| presets.get(*pi) != Some(&step.preset)) {
                        run = Some(("sequence.step_set", json!({"sequence": si, "step": k, "preset": pi})));
                    }
                    ui.horizontal(|ui| {
                        let mut length = step.length;
                        let r = ui.add(egui::DragValue::new(&mut length).range(0.01..=3600.0).speed(0.05).suffix(unit));
                        let name = format!("{}, {which}", tr("Length"));
                        ui.ctx().accesskit_node_builder(r.id, |n| n.set_label(name));
                        if r.changed() {
                            run = Some(("sequence.step_set", json!({"sequence": si, "step": k, "length": length})));
                        }
                        let mut fade = (step.fade * 100.0).round();
                        let r = ui
                            .add(egui::DragValue::new(&mut fade).range(0.0..=100.0).speed(1.0).suffix(" %"))
                            .on_hover_text(tr("The part of the step spent moving there; the rest holds"));
                        let name = format!("{}, {which}", tr("Move"));
                        ui.ctx().accesskit_node_builder(r.id, |n| n.set_label(name));
                        if r.changed() {
                            run = Some(("sequence.step_set", json!({"sequence": si, "step": k, "fade": fade / 100.0})));
                        }
                    });
                    ui.horizontal(|ui| {
                        let button = |ui: &mut Ui, words: &str| {
                            let r = ui.small_button(words);
                            let name = format!("{words} ({which})");
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                            r.clicked()
                        };
                        if k > 0 && button(ui, tr("Up")) {
                            run = Some(("sequence.step_move", json!({"sequence": si, "step": k, "to": k - 1})));
                        }
                        if k + 1 < count && button(ui, tr("Down")) {
                            run = Some(("sequence.step_move", json!({"sequence": si, "step": k, "to": k + 1})));
                        }
                        if button(ui, tr("Remove")) {
                            run = Some(("sequence.step_remove", json!({"sequence": si, "step": k})));
                        }
                    });
                    ui.end_row();
                }
            });
            let r = ui.button(tr("Record step")).on_hover_text(tr(
                "Store the look on stage now (positions, colours, everything) as this sequence's next step",
            ));
            let name = format!("{} ({})", tr("Record step"), seq.name);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            if r.clicked() {
                run = Some(("sequence.step_add", json!({"sequence": si, "length": 1.0})));
            }
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            let label = ui.label(tr("New sequence"));
            ui.add(egui::TextEdit::singleline(&mut self.sequence_draft).desired_width(160.0)).labelled_by(label.id);
            let ok = !self.sequence_draft.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(tr("Add sequence"))).clicked() {
                run = Some(("sequence.set", json!({"name": self.sequence_draft.trim()})));
                self.sequence_draft.clear();
            }
        });
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
        }
    }
}
