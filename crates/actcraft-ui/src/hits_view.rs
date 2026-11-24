//! The Hits part of the Presets window: presets put on for a moment (a burst of fog, a flash),
//! outside the cue lists, each with a Fire button, and making a new one.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use actcraft_engine::Engine;

use crate::{ActCraftApp, tr};

impl ActCraftApp {
    pub(crate) fn hits_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        crate::help_view::section_title(ui, tr("Hits"), crate::help_view::Topic::Hits);
        ui.label(
            RichText::new(tr(
                "A preset on for a moment, such as fog for half a second, outside the cue lists; then its channels go back.",
            ))
            .color(t.text_dim),
        );
        let hits = e.show.hits.clone();
        let presets: Vec<String> = e.show.presets.iter().map(|p| p.name.clone()).collect();
        let on = e.hits_on();
        let mut run: Option<(&str, Value)> = None;
        for (i, hit) in hits.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&hit.name).strong());
                let r = ui.button(tr("Fire")).on_hover_text(tr("Put it on now, for its time"));
                let name = format!("{} ({})", tr("Fire"), hit.name);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if r.clicked() {
                    run = Some(("hit.fire", json!({"hit": i})));
                }
                let words = format!("{} \u{b7} {:.2} s", hit.preset, hit.seconds);
                ui.label(if presets.contains(&hit.preset) {
                    RichText::new(words)
                } else {
                    RichText::new(format!("{words} \u{b7} {}", tr("its preset is gone"))).color(t.warning)
                });
                if on.contains(&hit.name) {
                    ui.label(RichText::new(tr("On")).color(t.playing).strong());
                }
                let r = ui.small_button(tr("Remove"));
                let name = format!("{} ({})", tr("Remove"), hit.name);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if r.clicked() {
                    run = Some(("hit.remove", json!({"hit": i})));
                }
            });
        }
        if presets.is_empty() {
            ui.label(RichText::new(tr("Store a preset first: a hit puts a preset on.")).color(t.text_dim));
        } else {
            let d = &mut self.looks_draft;
            d.hit_preset = d.hit_preset.min(presets.len() - 1);
            if d.hit_seconds <= 0.0 {
                d.hit_seconds = 0.5;
            }
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(tr("New hit"));
                ui.add(egui::TextEdit::singleline(&mut d.hit_name).desired_width(120.0)).labelled_by(label.id);
                let label = ui.label(tr("Preset"));
                egui::ComboBox::from_id_salt("hit-preset")
                    .selected_text(presets.get(d.hit_preset).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, p) in presets.iter().enumerate() {
                            ui.selectable_value(&mut d.hit_preset, i, p);
                        }
                    })
                    .response
                    .labelled_by(label.id);
                let label = ui.label(tr("On for"));
                ui.add(egui::DragValue::new(&mut d.hit_seconds).range(0.05..=600.0).speed(0.05).suffix(" s"))
                    .labelled_by(label.id);
                let ok = !d.hit_name.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new(tr("Add hit"))).clicked() {
                    run = Some((
                        "hit.set",
                        json!({"name": d.hit_name.trim(), "preset": d.hit_preset, "seconds": d.hit_seconds}),
                    ));
                    d.hit_name.clear();
                }
            });
        }
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
        }
    }
}
