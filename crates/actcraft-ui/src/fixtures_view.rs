//! The Fixtures window (Lighting > Fixtures): patch lights by kind into a universe, and set each
//! one's attributes (intensity, colour, pan, tilt...) with sliders.

use actcraft_engine::Engine;
use actcraft_engine::dmx::{CHANNELS, UNIVERSES};
use actcraft_engine::fixtures::{Attribute, KINDS, attribute_key, value_of};
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

impl ActCraftApp {
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
