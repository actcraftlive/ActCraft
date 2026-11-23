//! The Fixtures window (Lighting > Fixtures): patch lights by kind into a universe, and set each
//! one's attributes (intensity, colour, pan, tilt...) with sliders.

use actcraft_engine::Engine;
use actcraft_engine::dmx::{CHANNELS, UNIVERSES};
use actcraft_engine::fixtures::{ATTRIBUTES, Attribute, KINDS, attribute_key, value_of};
use egui::{RichText, Ui};
use serde_json::json;

use crate::ActCraftApp;
use crate::i18n::tr;

/// A patch being put together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureDraft {
    /// Index into [`KINDS`], then into the show's fixture types.
    pub kind: usize,
    pub name: String,
    pub universe: u16,
    /// `None`: the first free address.
    pub address: Option<u16>,
    pub count: u16,
    /// The mode of an imported type.
    pub mode: usize,
}

impl Default for FixtureDraft {
    fn default() -> Self {
        Self { kind: 0, name: String::new(), universe: 1, address: None, count: 1, mode: 0 }
    }
}

/// An attribute in words.
fn attribute_text(a: Attribute) -> &'static str {
    match a {
        Attribute::Intensity | Attribute::IntensityFine => tr("Intensity"),
        Attribute::Red => tr("Red"),
        Attribute::Green => tr("Green"),
        Attribute::Blue => tr("Blue"),
        Attribute::White => tr("White"),
        Attribute::Amber => tr("Amber"),
        Attribute::Uv => tr("UV"),
        Attribute::Pan | Attribute::PanFine => tr("Pan position"),
        Attribute::Tilt | Attribute::TiltFine => tr("Tilt position"),
        Attribute::Strobe => tr("Strobe"),
        Attribute::ColourWheel => tr("Colour wheel"),
        Attribute::Gobo => tr("Gobo"),
        Attribute::Zoom => tr("Zoom"),
        Attribute::Focus => tr("Focus"),
        Attribute::Other => tr("Other"),
    }
}

/// A channel in words: like [`attribute_text`], but a fine channel says so.
fn channel_text(a: Attribute) -> &'static str {
    match a {
        Attribute::IntensityFine => tr("Intensity (fine)"),
        Attribute::PanFine => tr("Pan position (fine)"),
        Attribute::TiltFine => tr("Tilt position (fine)"),
        _ => attribute_text(a),
    }
}

/// A kind's name in the window's language.
fn kind_text(key: &str) -> &'static str {
    match key {
        "dimmer" => tr("Dimmer"),
        "rgb" => tr("RGB"),
        "rgb_dimmer" => tr("RGB with dimmer"),
        "rgbw" => tr("RGBW"),
        "rgbw_dimmer" => tr("RGBW with dimmer"),
        "rgbawuv_dimmer" => tr("RGBAW and UV with dimmer and strobe"),
        "moving_head" => tr("Moving head (16-bit pan and tilt, RGBW)"),
        _ => tr("Fixture"),
    }
}

/// A fixture type being made or changed by hand: its name, its modes (name and channels) and
/// the mode shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeEditor {
    pub name: String,
    pub modes: Vec<(String, Vec<Attribute>)>,
    pub mode: usize,
}

impl Default for TypeEditor {
    fn default() -> Self {
        Self { name: String::new(), modes: vec![(String::new(), vec![Attribute::Intensity])], mode: 0 }
    }
}

impl ActCraftApp {
    /// Making a fixture type by hand, or changing the chosen one.
    fn type_editor_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let chosen =
            self.fixture_draft.kind.checked_sub(KINDS.len()).and_then(|i| e.show.fixture_types.get(i)).cloned();
        if self.type_editor.is_none() {
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("New fixture type")).clicked() {
                    self.type_editor = Some(TypeEditor::default());
                }
                if let Some(ty) = &chosen
                    && ui.button(tr("Change this fixture type")).clicked()
                {
                    let modes = ty.modes.iter().map(|m| (m.name.clone(), m.channels.clone())).collect();
                    self.type_editor = Some(TypeEditor { name: ty.name.clone(), modes, mode: 0 });
                }
            });
            return;
        }
        let mut save = false;
        let mut close = false;
        ui.group(|ui| {
            let Some(ed) = self.type_editor.as_mut() else { return };
            ui.label(RichText::new(tr("Fixture type")).strong());
            ui.horizontal(|ui| {
                let label = ui.label(tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(220.0)).labelled_by(label.id);
            });
            // The modes, one shown at a time.
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Modes"));
                for (i, (name, channels)) in ed.modes.iter().enumerate() {
                    let words = if name.trim().is_empty() { format!("{} ch", channels.len()) } else { name.clone() };
                    if ui.selectable_label(ed.mode == i, words).clicked() {
                        ed.mode = i;
                    }
                }
                if ed.modes.len() < 32 && ui.button(tr("Add mode")).clicked() {
                    ed.modes.push((String::new(), vec![Attribute::Intensity]));
                    ed.mode = ed.modes.len() - 1;
                }
                if ed.modes.len() > 1 && ui.button(tr("Remove this mode")).clicked() {
                    ed.modes.remove(ed.mode.min(ed.modes.len() - 1));
                    ed.mode = ed.mode.saturating_sub(1);
                }
            });
            ed.mode = ed.mode.min(ed.modes.len().saturating_sub(1));
            let Some((mode_name, channels)) = ed.modes.get_mut(ed.mode) else { return };
            ui.horizontal(|ui| {
                let label = ui.label(tr("Mode name"));
                ui.add(
                    egui::TextEdit::singleline(mode_name).hint_text(tr("Like its channel count")).desired_width(180.0),
                )
                .labelled_by(label.id);
            });
            let mut action = None;
            egui::Grid::new("type-channels").num_columns(2).show(ui, |ui| {
                let count = channels.len();
                for (i, a) in channels.iter_mut().enumerate() {
                    let label = ui.label(tr("Channel {n}").replace("{n}", &(i + 1).to_string()));
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("type-channel", i))
                            .width(160.0)
                            .selected_text(channel_text(*a))
                            .show_ui(ui, |ui| {
                                for choice in ATTRIBUTES {
                                    ui.selectable_value(a, choice, channel_text(choice));
                                }
                            })
                            .response
                            .labelled_by(label.id);
                        if i > 0 && ui.small_button(tr("Up")).clicked() {
                            action = Some((i, -1));
                        }
                        if i + 1 < count && ui.small_button(tr("Down")).clicked() {
                            action = Some((i, 1));
                        }
                        if count > 1 && ui.small_button(tr("Remove")).clicked() {
                            action = Some((i, 0));
                        }
                    });
                    ui.end_row();
                }
            });
            match action {
                Some((i, 0)) => {
                    channels.remove(i);
                }
                Some((i, -1)) => channels.swap(i, i - 1),
                Some((i, _)) => channels.swap(i, i + 1),
                None => {}
            }
            if channels.len() < 64 && ui.button(tr("Add channel")).clicked() {
                channels.push(Attribute::Other);
            }
            ui.horizontal(|ui| {
                save = ui.add_enabled(!ed.name.trim().is_empty(), egui::Button::new(tr("Save fixture type"))).clicked();
                close = ui.button(tr("Cancel")).clicked();
            });
            ui.label(RichText::new(tr("Saving under a name that is there replaces that type.")).color(t.text_dim));
        });
        if save && let Some(ed) = self.type_editor.clone() {
            let modes: Vec<serde_json::Value> = ed
                .modes
                .iter()
                .map(|(name, channels)| {
                    let channels: Vec<&str> = channels.iter().map(|a| attribute_key(*a)).collect();
                    json!({"name": name.trim(), "channels": channels})
                })
                .collect();
            if let Ok(v) = e.run("fixture_type.set", &json!({"name": ed.name.trim(), "modes": modes})) {
                let i = v["type"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
                self.fixture_draft.kind = KINDS.len() + i;
                self.fixture_draft.mode = 0;
                close = true;
            }
        }
        if close {
            self.type_editor = None;
        }
    }

    /// The Fixtures window, while it is open.
    pub(crate) fn fixtures_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        if !self.fixtures_open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Fixtures"))
            .open(&mut open)
            .default_width(640.0)
            .default_height(560.0)
            .collapsible(false)
            .show(ctx, |ui| {
                self.fixture_patch_form(ui, e);
                self.type_editor_ui(ui, e);
                ui.separator();
                egui::ScrollArea::vertical().id_salt("fixtures-scroll").auto_shrink([false, true]).show(ui, |ui| {
                    self.fixture_list_ui(ui, e);
                });
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.fixtures_open = false;
        }
    }

    fn fixture_patch_form(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("Patch fixtures")).strong());
        let types = e.show.fixture_types.clone();
        let mut import = false;
        let d = &mut self.fixture_draft;
        // Built-in kinds first, then the types brought in from fixture files.
        if d.kind >= KINDS.len() + types.len() {
            d.kind = 0;
        }
        let imported = d.kind.checked_sub(KINDS.len()).and_then(|t| types.get(t));
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Kind"));
            let current = match (KINDS.get(d.kind), imported) {
                (Some(k), _) => kind_text(k.key).to_owned(),
                (None, Some(t)) => t.name.clone(),
                _ => String::new(),
            };
            egui::ComboBox::from_id_salt("fixture-kind")
                .width(280.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, k) in KINDS.iter().enumerate() {
                        let words = format!("{} ({} ch)", kind_text(k.key), k.channels.len());
                        if ui.selectable_value(&mut d.kind, i, words).clicked() {
                            d.mode = 0;
                        }
                    }
                    if !types.is_empty() {
                        ui.separator();
                    }
                    for (i, t) in types.iter().enumerate() {
                        if ui.selectable_value(&mut d.kind, KINDS.len() + i, &t.name).clicked() {
                            d.mode = 0;
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if let Some(t) = imported {
                let label = ui.label(tr("Mode"));
                let shown =
                    t.modes.get(d.mode).map_or(String::new(), |m| format!("{} ({} ch)", m.name, m.channels.len()));
                egui::ComboBox::from_id_salt("fixture-mode")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (i, m) in t.modes.iter().enumerate() {
                            ui.selectable_value(&mut d.mode, i, format!("{} ({} ch)", m.name, m.channels.len()));
                        }
                    })
                    .response
                    .labelled_by(label.id);
            }
            import = ui
                .button(tr("Import fixture type\u{2026}"))
                .on_hover_text(tr("A fixture file from the Open Fixture Library (.json)"))
                .clicked();
        });
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Name"));
            ui.add(egui::TextEdit::singleline(&mut d.name).hint_text(tr("Like the kind")).desired_width(180.0))
                .labelled_by(label.id);
        });
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Universe"));
            ui.add(egui::DragValue::new(&mut d.universe).range(1..=UNIVERSES as u16)).labelled_by(label.id);
            let mut first_free = d.address.is_none();
            if ui.checkbox(&mut first_free, tr("First free address")).changed() {
                d.address = if first_free { None } else { Some(1) };
            }
            if let Some(address) = &mut d.address {
                let label = ui.label(tr("Address"));
                ui.add(egui::DragValue::new(address).range(1..=CHANNELS as u16)).labelled_by(label.id);
            }
            let label = ui.label(tr("How many"));
            ui.add(egui::DragValue::new(&mut d.count).range(1..=64)).labelled_by(label.id);
        });
        if import {
            self.ask_file(crate::flows::FileRequest {
                ask: crate::flows::FileAsk::ImportFixtureType,
                kind: crate::flows::FileKind::Open,
                title: tr("Import fixture type"),
                filters: vec![(tr("Open Fixture Library fixture"), actcraft_engine::fixture_library::EXTENSIONS)],
                file_name: None,
                directory: crate::flows::show_dir(e),
            });
        }
        if ui.button(tr("Patch")).clicked() {
            let d = self.fixture_draft.clone();
            let mut prm = json!({"universe": d.universe, "count": d.count});
            match KINDS.get(d.kind) {
                Some(k) => prm["kind"] = json!(k.key),
                None => {
                    prm["type"] = json!(d.kind - KINDS.len());
                    prm["mode"] = json!(d.mode);
                }
            }
            if !d.name.trim().is_empty() {
                prm["name"] = json!(d.name.trim());
            }
            if let Some(a) = d.address {
                prm["address"] = json!(a);
            }
            self.run(e, "fixture.add", prm);
        }
        ui.label(
            RichText::new(tr("Pan, tilt and intensity with a fine channel are set to 16 bits.")).color(t.text_dim),
        );
    }

    fn fixture_list_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let fixtures = e.show.fixtures.clone();
        if fixtures.is_empty() {
            ui.label(RichText::new(tr("No fixtures patched yet.")).color(t.text_dim));
            return;
        }
        let mut remove = None;
        for (i, f) in fixtures.iter().enumerate() {
            let title = format!("{} \u{b7} {}.{:03} \u{b7} {} ch", f.name, f.universe, f.address, f.channels.len());
            egui::CollapsingHeader::new(title).id_salt(("fixture", i)).default_open(i == 0).show(ui, |ui| {
                egui::Grid::new(("fixture-attributes", i)).num_columns(2).show(ui, |ui| {
                    let mut seen: Vec<Attribute> = Vec::new();
                    for a in f.channels.iter().copied().filter(|a| !a.is_fine()) {
                        if seen.contains(&a) {
                            continue;
                        }
                        seen.push(a);
                        let Some(mut v) = value_of(e, f, a) else { continue };
                        let label = ui.label(attribute_text(a));
                        let slider = egui::Slider::new(&mut v, 0.0..=1.0)
                            .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                            .custom_parser(|s| {
                                s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|p| p / 100.0)
                            });
                        if ui.add(slider).labelled_by(label.id).changed() {
                            self.run(e, "fixture.set", json!({"fixture": i, "values": {attribute_key(a): v}}));
                        }
                        ui.end_row();
                    }
                });
                if ui.button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.run(e, "fixture.remove", json!({"fixture": i}));
        }
    }
}
