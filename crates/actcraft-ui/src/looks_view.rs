//! The Presets window (Lighting > Presets): stored looks recalled with one press, and the groups
//! fixtures are picked by.

use actcraft_engine::Engine;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::ActCraftApp;
use crate::i18n::tr;

/// What is being put together in the Presets window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LooksDraft {
    pub preset_name: String,
    /// The group a preset is stored from; `None` for every fixture.
    pub store_from: Option<usize>,
    pub group_name: String,
    /// Fixtures picked for a new group, by index.
    pub picked: Vec<usize>,
    /// Seconds a recalled preset fades in over.
    pub fade: f64,
    /// A new cue list's name; a new cue's preset (index) and fade.
    pub list_name: String,
    pub cue_preset: usize,
    pub cue_fade: f64,
}

impl ActCraftApp {
    /// The Presets window, while it is open.
    pub(crate) fn presets_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        if !self.presets_open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Presets"))
            .open(&mut open)
            .default_width(560.0)
            .default_height(560.0)
            .collapsible(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("presets-scroll").auto_shrink([false, true]).show(ui, |ui| {
                    self.presets_ui(ui, e);
                    ui.separator();
                    self.cue_lists_ui(ui, e);
                    ui.separator();
                    self.groups_ui(ui, e);
                });
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.presets_open = false;
        }
    }

    fn presets_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Presets")).strong());
        ui.horizontal(|ui| {
            let label = ui.label(tr("Fade in over"));
            ui.add(egui::DragValue::new(&mut self.looks_draft.fade).range(0.0..=600.0).speed(0.1).suffix(" s"))
                .labelled_by(label.id);
        });
        let presets = e.show.presets.clone();
        let mut recall = None;
        let mut remove = None;
        if presets.is_empty() {
            ui.label(
                RichText::new(tr("No presets yet: set the fixtures, then store what they show.")).color(t.text_dim),
            );
        }
        egui::Grid::new("presets").num_columns(3).striped(true).show(ui, |ui| {
            for (i, p) in presets.iter().enumerate() {
                if ui.button(&p.name).on_hover_text(tr("Recall this preset")).clicked() {
                    recall = Some(i);
                }
                ui.label(tr("{n} values").replace("{n}", &p.values.len().to_string()));
                if ui.small_button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = recall {
            let fade = self.looks_draft.fade;
            self.run(e, "preset.recall", json!({"preset": i, "fade": fade}));
        }
        if let Some(i) = remove {
            self.run(e, "preset.remove", json!({"preset": i}));
        }
        ui.add_space(6.0);
        let groups: Vec<String> = e.show.groups.iter().map(|g| g.name.clone()).collect();
        let d = &mut self.looks_draft;
        if d.store_from.is_some_and(|g| g >= groups.len()) {
            d.store_from = None;
        }
        let mut store = false;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Name"));
            ui.add(egui::TextEdit::singleline(&mut d.preset_name).desired_width(160.0)).labelled_by(label.id);
            let label = ui.label(tr("From"));
            let shown = d.store_from.and_then(|g| groups.get(g)).map_or(tr("Every fixture").to_owned(), Clone::clone);
            egui::ComboBox::from_id_salt("preset-from")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.store_from, None, tr("Every fixture"));
                    for (i, g) in groups.iter().enumerate() {
                        ui.selectable_value(&mut d.store_from, Some(i), g);
                    }
                })
                .response
                .labelled_by(label.id);
            store = ui
                .add_enabled(!d.preset_name.trim().is_empty(), egui::Button::new(tr("Store preset")))
                .on_hover_text(tr("Keeps what the fixtures show now; a preset of that name is replaced"))
                .clicked();
        });
        if store {
            let d = self.looks_draft.clone();
            let mut prm = json!({"name": d.preset_name.trim()});
            if let Some(g) = d.store_from {
                prm["group"] = json!(g);
            }
            if e.run("preset.store", &prm).is_ok() {
                self.looks_draft.preset_name.clear();
            }
        }
    }

    /// Cue lists: Go and Back, the cues with where the list stands, and new cues and lists.
    fn cue_lists_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Cue lists")).strong());
        let lists = e.show.cue_lists.clone();
        let presets: Vec<String> = e.show.presets.iter().map(|p| p.name.clone()).collect();
        let mut run: Option<(&str, Value)> = None;
        for (li, list) in lists.iter().enumerate() {
            let at = e.cue_position(li);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&list.name).strong());
                if ui.button(tr("Go")).on_hover_text(tr("Fade to the next cue")).clicked() {
                    run = Some(("cue.go", json!({"list": li})));
                }
                if ui.add_enabled(at.is_some_and(|c| c > 0), egui::Button::new(tr("Back"))).clicked() {
                    run = Some(("cue.back", json!({"list": li})));
                }
                let state = match at.and_then(|c| list.cues.get(c).map(|cue| (c, cue))) {
                    Some((c, cue)) => tr("At cue {n} of {count}: {name}")
                        .replace("{n}", &(c + 1).to_string())
                        .replace("{count}", &list.cues.len().to_string())
                        .replace("{name}", &cue.name),
                    None => tr("Not started").to_owned(),
                };
                ui.label(state);
                if e.fading() {
                    ui.label(RichText::new(tr("fading\u{2026}")).color(t.text_dim));
                }
                if ui.small_button(tr("Remove cue list")).clicked() {
                    run = Some(("cue_list.remove", json!({"list": li})));
                }
            });
            egui::Grid::new(("cues", li)).num_columns(4).striped(true).show(ui, |ui| {
                for (ci, cue) in list.cues.iter().enumerate() {
                    let words = format!("{}. {}", ci + 1, cue.name);
                    let text = if at == Some(ci) { RichText::new(words).strong() } else { RichText::new(words) };
                    if ui.selectable_label(at == Some(ci), text).on_hover_text(tr("Go to this cue")).clicked() {
                        run = Some(("cue.jump", json!({"list": li, "cue": ci})));
                    }
                    ui.label(&cue.preset);
                    ui.label(format!("{:.1} s", cue.fade));
                    if ui.small_button(tr("Remove")).clicked() {
                        run = Some(("cue.remove", json!({"list": li, "cue": ci})));
                    }
                    ui.end_row();
                }
            });
            if !presets.is_empty() {
                let d = &mut self.looks_draft;
                d.cue_preset = d.cue_preset.min(presets.len() - 1);
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label(tr("Preset"));
                    egui::ComboBox::from_id_salt(("cue-preset", li))
                        .selected_text(presets.get(d.cue_preset).cloned().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (pi, name) in presets.iter().enumerate() {
                                ui.selectable_value(&mut d.cue_preset, pi, name);
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    let label = ui.label(tr("Fade"));
                    ui.add(egui::DragValue::new(&mut d.cue_fade).range(0.0..=600.0).speed(0.1).suffix(" s"))
                        .labelled_by(label.id);
                    if ui.button(tr("Add cue")).clicked() {
                        run = Some(("cue.add", json!({"list": li, "preset": d.cue_preset, "fade": d.cue_fade})));
                    }
                });
            }
            ui.add_space(6.0);
        }
        if presets.is_empty() {
            ui.label(RichText::new(tr("Cues fade presets in: store a preset first.")).color(t.text_dim));
        }
        let d = &mut self.looks_draft;
        ui.horizontal(|ui| {
            let label = ui.label(tr("New cue list"));
            ui.add(egui::TextEdit::singleline(&mut d.list_name).desired_width(160.0)).labelled_by(label.id);
            if ui.add_enabled(!d.list_name.trim().is_empty(), egui::Button::new(tr("Add cue list"))).clicked() {
                run = Some(("cue_list.add", json!({"name": d.list_name.trim()})));
                d.list_name.clear();
            }
        });
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
        }
    }

    fn groups_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Groups")).strong());
        let names: Vec<String> = e.show.fixtures.iter().map(|f| f.name.clone()).collect();
        let ids: Vec<u64> = e.show.fixtures.iter().map(|f| f.id).collect();
        let groups = e.show.groups.clone();
        let mut remove = None;
        egui::Grid::new("groups").num_columns(3).striped(true).show(ui, |ui| {
            for (i, g) in groups.iter().enumerate() {
                ui.label(&g.name);
                let members: Vec<&str> = g
                    .fixtures
                    .iter()
                    .filter_map(|id| ids.iter().position(|f| f == id).and_then(|i| names.get(i)).map(String::as_str))
                    .collect();
                ui.label(members.join(", "));
                if ui.small_button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.run(e, "group.remove", json!({"group": i}));
        }
        if names.is_empty() {
            ui.label(RichText::new(tr("Patch fixtures first (Lighting > Fixtures).")).color(t.text_dim));
            return;
        }
        ui.add_space(6.0);
        let d = &mut self.looks_draft;
        d.picked.retain(|i| *i < names.len());
        ui.horizontal_wrapped(|ui| {
            for (i, name) in names.iter().enumerate() {
                let mut on = d.picked.contains(&i);
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        d.picked.push(i);
                    } else {
                        d.picked.retain(|p| *p != i);
                    }
                }
            }
        });
        let mut save = false;
        ui.horizontal(|ui| {
            let label = ui.label(tr("Group name"));
            ui.add(egui::TextEdit::singleline(&mut d.group_name).desired_width(160.0)).labelled_by(label.id);
            save = ui
                .add_enabled(
                    !d.group_name.trim().is_empty() && !d.picked.is_empty(),
                    egui::Button::new(tr("Save group")),
                )
                .clicked();
        });
        if save {
            let d = self.looks_draft.clone();
            if e.run("group.set", &json!({"name": d.group_name.trim(), "fixtures": d.picked})).is_ok() {
                self.looks_draft.group_name.clear();
                self.looks_draft.picked.clear();
            }
        }
    }
}
