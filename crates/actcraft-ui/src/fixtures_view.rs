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
pub(crate) fn attribute_text(a: Attribute) -> &'static str {
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
    pub modes: Vec<ModeDraft>,
    pub mode: usize,
    /// The channel (from 0) whose named ranges are shown for changing.
    pub ranges_of: Option<usize>,
}

/// A mode being made: its name, what each channel does and the named ranges of its channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeDraft {
    pub name: String,
    pub channels: Vec<Attribute>,
    pub ranges: Vec<RangeDraft>,
}

/// A named range of one channel's levels; `channel` counts from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeDraft {
    pub channel: usize,
    pub from: u8,
    pub to: u8,
    pub name: String,
}

impl ModeDraft {
    fn new(channels: Vec<Attribute>) -> Self {
        Self { name: String::new(), channels, ranges: Vec::new() }
    }

    /// Swaps channels `a` and `b`, their ranges going along.
    fn swap(&mut self, a: usize, b: usize) {
        if a >= self.channels.len() || b >= self.channels.len() {
            return;
        }
        self.channels.swap(a, b);
        for r in &mut self.ranges {
            if r.channel == a {
                r.channel = b;
            } else if r.channel == b {
                r.channel = a;
            }
        }
    }

    /// Removes channel `i` and its ranges; the ranges of later channels move up with them.
    fn remove(&mut self, i: usize) {
        if i >= self.channels.len() {
            return;
        }
        self.channels.remove(i);
        self.ranges.retain(|r| r.channel != i);
        for r in &mut self.ranges {
            if r.channel > i {
                r.channel -= 1;
            }
        }
    }

    /// What `fixture_type.set` takes for this mode.
    fn to_json(&self) -> serde_json::Value {
        let channels: Vec<&str> = self.channels.iter().map(|a| attribute_key(*a)).collect();
        let ranges: Vec<serde_json::Value> = self
            .ranges
            .iter()
            .map(|r| {
                json!({"channel": r.channel + 1, "from": r.from.min(r.to), "to": r.from.max(r.to), "name": r.name.trim()})
            })
            .collect();
        json!({"name": self.name.trim(), "channels": channels, "ranges": ranges})
    }
}

impl Default for TypeEditor {
    fn default() -> Self {
        Self { name: String::new(), modes: vec![ModeDraft::new(vec![Attribute::Intensity])], mode: 0, ranges_of: None }
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
                    // Its named ranges come along, so saving keeps them.
                    let modes = ty
                        .modes
                        .iter()
                        .map(|m| ModeDraft {
                            name: m.name.clone(),
                            channels: m.channels.clone(),
                            ranges: m
                                .ranges
                                .iter()
                                .map(|r| RangeDraft {
                                    channel: usize::from(r.channel.max(1)) - 1,
                                    from: r.from,
                                    to: r.to,
                                    name: r.name.clone(),
                                })
                                .collect(),
                        })
                        .collect();
                    self.type_editor = Some(TypeEditor { name: ty.name.clone(), modes, mode: 0, ranges_of: None });
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
                for (i, mode) in ed.modes.iter().enumerate() {
                    let words = if mode.name.trim().is_empty() {
                        format!("{} ch", mode.channels.len())
                    } else {
                        mode.name.clone()
                    };
                    if ui.selectable_label(ed.mode == i, words).clicked() {
                        ed.mode = i;
                        ed.ranges_of = None;
                    }
                }
                if ed.modes.len() < 32 && ui.button(tr("Add mode")).clicked() {
                    ed.modes.push(ModeDraft::new(vec![Attribute::Intensity]));
                    ed.mode = ed.modes.len() - 1;
                    ed.ranges_of = None;
                }
                if ed.modes.len() > 1 && ui.button(tr("Remove this mode")).clicked() {
                    ed.modes.remove(ed.mode.min(ed.modes.len() - 1));
                    ed.mode = ed.mode.saturating_sub(1);
                    ed.ranges_of = None;
                }
            });
            ed.mode = ed.mode.min(ed.modes.len().saturating_sub(1));
            let ranges_of = &mut ed.ranges_of;
            let Some(mode) = ed.modes.get_mut(ed.mode) else { return };
            ui.horizontal(|ui| {
                let label = ui.label(tr("Mode name"));
                ui.add(
                    egui::TextEdit::singleline(&mut mode.name)
                        .hint_text(tr("Like its channel count"))
                        .desired_width(180.0),
                )
                .labelled_by(label.id);
            });
            let mut action = None;
            egui::Grid::new("type-channels").num_columns(2).show(ui, |ui| {
                let count = mode.channels.len();
                for i in 0..count {
                    let n = (i + 1).to_string();
                    let label = ui.label(tr("Channel {n}").replace("{n}", &n));
                    let in_use = mode.ranges.iter().filter(|r| r.channel == i).count();
                    ui.horizontal(|ui| {
                        if let Some(a) = mode.channels.get_mut(i) {
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
                        }
                        // The named ranges of this channel (off, dimmer, strobe): shown under the list.
                        let words = tr("Ranges ({n})").replace("{n}", &in_use.to_string());
                        let open = *ranges_of == Some(i);
                        let r = ui.selectable_label(open, &words);
                        let name = format!("{words}, {}", tr("Channel {n}").replace("{n}", &n));
                        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, &name));
                        if r.clicked() {
                            *ranges_of = if open { None } else { Some(i) };
                        }
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
                    mode.remove(i);
                    *ranges_of = None;
                }
                Some((i, -1)) => {
                    mode.swap(i, i - 1);
                    *ranges_of = ranges_of.map(|c| {
                        if c == i {
                            i - 1
                        } else if c == i - 1 {
                            i
                        } else {
                            c
                        }
                    });
                }
                Some((i, _)) => {
                    mode.swap(i, i + 1);
                    *ranges_of = ranges_of.map(|c| {
                        if c == i {
                            i + 1
                        } else if c == i + 1 {
                            i
                        } else {
                            c
                        }
                    });
                }
                None => {}
            }
            if mode.channels.len() < 64 && ui.button(tr("Add channel")).clicked() {
                mode.channels.push(Attribute::Other);
            }
            if let Some(c) = *ranges_of {
                ranges_ui(ui, mode, c);
            }
            ui.horizontal(|ui| {
                save = ui.add_enabled(!ed.name.trim().is_empty(), egui::Button::new(tr("Save fixture type"))).clicked();
                close = ui.button(tr("Cancel")).clicked();
            });
            ui.label(RichText::new(tr("Saving under a name that is there replaces that type.")).color(t.text_dim));
        });
        if save && let Some(ed) = self.type_editor.clone() {
            let modes: Vec<serde_json::Value> = ed.modes.iter().map(ModeDraft::to_json).collect();
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
                self.blind_note(ui, e);
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
        let mut export = false;
        // Library types not in the show yet: picking one brings it in.
        let library: Vec<serde_json::Value> = actcraft_engine::fixture_library::library()
            .into_iter()
            .filter(|t| !types.iter().any(|s| Some(s.name.as_str()) == t["name"].as_str()))
            .collect();
        let mut from_library: Option<serde_json::Value> = None;
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
                    if !library.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(tr("From the library")).small());
                    }
                    for t in &library {
                        let name = t["name"].as_str().unwrap_or_default();
                        let words = tr("{name} (library)").replace("{name}", name);
                        if ui.selectable_label(false, words).clicked() {
                            from_library = Some(t.clone());
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
                .on_hover_text(tr(
                    "An ActCraft fixture file or one from the Open Fixture Library (.json), or an .r20 file",
                ))
                .clicked();
            if imported.is_some() {
                export = ui
                    .button(tr("Export fixture type\u{2026}"))
                    .on_hover_text(tr("Save this fixture type as a file another show or computer can import"))
                    .clicked();
            }
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
        // A library type comes in with its mode and range names in the window's language.
        if let Some(mut doc) = from_library {
            for mode in doc["modes"].as_array_mut().into_iter().flatten() {
                if let Some(name) = mode["name"].as_str() {
                    mode["name"] = json!(tr(name));
                }
                for range in mode["ranges"].as_array_mut().into_iter().flatten() {
                    if let Some(name) = range["name"].as_str() {
                        range["name"] = json!(tr(name));
                    }
                }
            }
            if let Ok(v) = e.run("fixture_type.set", &doc) {
                let i = v["type"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
                self.fixture_draft.kind = KINDS.len() + i;
                self.fixture_draft.mode = 0;
            }
        }
        let chosen_type = self.fixture_draft.kind.checked_sub(KINDS.len());
        if export
            && let Some(i) = chosen_type
            && let Some(t) = e.show.fixture_types.get(i)
        {
            let name = crate::flows::safe_file_name(&t.name);
            self.ask_file(crate::flows::FileRequest {
                ask: crate::flows::FileAsk::ExportFixtureType { fixture_type: i },
                kind: crate::flows::FileKind::Save,
                title: tr("Export fixture type"),
                filters: vec![(tr("ActCraft fixture file (.json)"), &["json"])],
                file_name: Some(format!("{name}.json")),
                directory: crate::flows::show_dir(e),
            });
        }
        if import {
            self.ask_file(crate::flows::FileRequest {
                ask: crate::flows::FileAsk::ImportFixtureType,
                kind: crate::flows::FileKind::Open,
                title: tr("Import fixture type"),
                filters: vec![(tr("Fixture file (.json, .r20)"), actcraft_engine::fixture_library::EXTENSIONS)],
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
        let mut hue: Option<(usize, String)> = None;
        for (i, f) in fixtures.iter().enumerate() {
            let title = format!("{} \u{b7} {}.{:03} \u{b7} {} ch", f.name, f.universe, f.address, f.channels.len());
            egui::CollapsingHeader::new(title).id_salt(("fixture", i)).default_open(i == 0).show(ui, |ui| {
                egui::Grid::new(("fixture-attributes", i)).num_columns(2).show(ui, |ui| {
                    let mut seen: Vec<Attribute> = Vec::new();
                    // Channels that do several things, or nothing named, get their own rows below.
                    for a in f.channels.iter().copied().filter(|a| !a.is_fine() && *a != Attribute::Other) {
                        let ranged = f.channels.iter().position(|c| *c == a).is_some_and(|p| has_ranges(f, p + 1));
                        if seen.contains(&a) || ranged {
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
                self.fixture_channels_ui(ui, e, i, f);
                // A Hue light of the paired bridge that shows what this fixture sends.
                if let Some(Ok(lights)) = &self.platform.hue_lights {
                    ui.horizontal(|ui| {
                        ui.label(tr("Hue light"));
                        let none = tr("None");
                        let shown = lights
                            .iter()
                            .find(|(id, _)| *id == f.hue)
                            .map_or_else(|| none.to_owned(), |(id, name)| format!("{id}: {name}"));
                        egui::ComboBox::from_id_salt(("fixture-hue", i))
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(f.hue.is_empty(), none).clicked() {
                                    hue = Some((i, String::new()));
                                }
                                for (id, name) in lights {
                                    if ui.selectable_label(f.hue == *id, format!("{id}: {name}")).clicked() {
                                        hue = Some((i, id.clone()));
                                    }
                                }
                            })
                            .response
                            .widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::ComboBox,
                                    true,
                                    format!("{} ({})", tr("Hue light"), f.name),
                                )
                            });
                    });
                }
                if ui.button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some((i, light)) = hue {
            self.run(e, "fixture.hue", json!({"fixture": i, "light": light}));
        }
        if let Some(i) = remove {
            self.run(e, "fixture.remove", json!({"fixture": i}));
        }
    }
}

/// Whether a fixture's channel (from 1) has named ranges.
fn has_ranges(f: &actcraft_show::Fixture, channel: usize) -> bool {
    f.ranges.iter().any(|r| usize::from(r.channel) == channel)
}

impl ActCraftApp {
    /// A fixture's channels that do several things (a choice of their ranges and a slider through
    /// the chosen one) or nothing named (a level 0-255), one row each.
    fn fixture_channels_ui(&mut self, ui: &mut Ui, e: &mut Engine, i: usize, f: &actcraft_show::Fixture) {
        let levels = e.dmx.edited(usize::from(f.universe)).copied();
        let mut run: Option<(&str, serde_json::Value)> = None;
        egui::Grid::new(("fixture-channels", i)).num_columns(3).show(ui, |ui| {
            for (k, a) in f.channels.iter().enumerate() {
                let channel = k + 1;
                if *a != Attribute::Other && !has_ranges(f, channel) {
                    continue;
                }
                let level = levels.as_ref().and_then(|l| l.get(usize::from(f.address) + k - 1)).copied().unwrap_or(0);
                let words = actcraft_engine::fill(tr("Channel {n}"), &[("n", &channel.to_string())]);
                ui.label(&words);
                let name = format!("{words} ({})", f.name);
                if has_ranges(f, channel) {
                    let ranges: Vec<(usize, &actcraft_show::ChannelRange)> =
                        f.ranges.iter().enumerate().filter(|(_, r)| usize::from(r.channel) == channel).collect();
                    let now = ranges.iter().find(|(_, r)| (r.from..=r.to).contains(&level));
                    let shown = now.map_or_else(|| level.to_string(), |(_, r)| r.name.clone());
                    egui::ComboBox::from_id_salt(("fixture-range", i, channel))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (ri, r) in &ranges {
                                let text = format!("{} ({}\u{2013}{})", r.name, r.from, r.to);
                                if ui.selectable_label(now.is_some_and(|(n, _)| n == ri), text).clicked() {
                                    run = Some(("fixture.range", json!({"fixture": i, "range": ri})));
                                }
                            }
                        })
                        .response
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &name));
                    // Through the chosen range, when it is more than one level.
                    if let Some((ri, r)) = now
                        && r.to > r.from
                    {
                        let mut v = f64::from(level - r.from) / f64::from(r.to - r.from);
                        let slider = egui::Slider::new(&mut v, 0.0..=1.0)
                            .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                            .custom_parser(|s| {
                                s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|p| p / 100.0)
                            });
                        let resp = ui.add(slider);
                        let slider_name = format!("{} ({})", r.name, f.name);
                        ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(slider_name));
                        if resp.changed() {
                            run = Some(("fixture.range", json!({"fixture": i, "range": ri, "value": v})));
                        }
                    } else {
                        ui.label("");
                    }
                } else {
                    let mut v = level;
                    let resp = ui.add(egui::DragValue::new(&mut v).range(0..=255));
                    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name.clone()));
                    if resp.changed() {
                        run = Some(("fixture.channel", json!({"fixture": i, "channel": channel, "level": v})));
                    }
                    ui.label("");
                }
                ui.end_row();
            }
        });
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
        }
    }
}

/// The named ranges of channel `c` (from 0) of a mode being made: from, to and name of each,
/// removing one, adding one after the last.
fn ranges_ui(ui: &mut Ui, mode: &mut ModeDraft, c: usize) {
    let n = (c + 1).to_string();
    ui.group(|ui| {
        ui.label(RichText::new(tr("Ranges of channel {n}").replace("{n}", &n)).strong());
        let mut remove = None;
        let mut k = 0;
        for (i, r) in mode.ranges.iter_mut().enumerate().filter(|(_, r)| r.channel == c) {
            k += 1;
            let which = format!("{} {k}", tr("Range"));
            ui.horizontal(|ui| {
                ui.label(&which);
                let named = |ui: &Ui, id: egui::Id, words: &str| {
                    let name = format!("{words}, {which}");
                    ui.ctx().accesskit_node_builder(id, |node| node.set_label(name));
                };
                ui.label(tr("From"));
                let from = ui.add(egui::DragValue::new(&mut r.from).range(0..=255));
                named(ui, from.id, tr("From"));
                ui.label(tr("To"));
                let to = ui.add(egui::DragValue::new(&mut r.to).range(0..=255));
                named(ui, to.id, tr("To"));
                ui.label(tr("Name"));
                let text = ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(200.0));
                named(ui, text.id, tr("Name"));
                let gone = ui.small_button(tr("Remove"));
                let name = format!("{} ({which})", tr("Remove"));
                gone.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if gone.clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            mode.ranges.remove(i);
        }
        if mode.ranges.len() < 256 && ui.button(tr("Add range")).clicked() {
            // It starts above the channel's last range and has a name, so it can be saved at once.
            let from = mode.ranges.iter().filter(|r| r.channel == c).map(|r| r.to.saturating_add(1)).max().unwrap_or(0);
            let name = format!("{} {}", tr("Range"), k + 1);
            mode.ranges.push(RangeDraft { channel: c, from, to: 255, name });
        }
    });
}

#[cfg(test)]
mod range_tests {
    use super::*;

    #[test]
    fn ranges_go_along_when_channels_move_or_go() {
        let range = |channel: usize, name: &str| RangeDraft { channel, from: 0, to: 9, name: name.into() };
        let mut m = ModeDraft::new(vec![Attribute::Pan, Attribute::Other, Attribute::Red, Attribute::Other]);
        m.ranges = vec![range(1, "a"), range(3, "b")];
        m.swap(1, 2);
        assert_eq!(m.channels[2], Attribute::Other);
        assert_eq!(m.ranges.iter().map(|r| r.channel).collect::<Vec<_>>(), vec![2, 3]);
        m.remove(0);
        assert_eq!(m.ranges.iter().map(|r| r.channel).collect::<Vec<_>>(), vec![1, 2]);
        m.remove(1);
        assert_eq!(m.ranges.iter().map(|r| (r.channel, r.name.as_str())).collect::<Vec<_>>(), vec![(1, "b")]);
        let json = m.to_json();
        assert_eq!(json["ranges"][0]["channel"], json!(2), "counted from 1 for the command");
    }
}

#[cfg(test)]
mod library_tests {
    /// Every mode and range name in ActCraft's fixture library has a row in each language file, as
    /// they are shown in the window's language.
    #[test]
    fn library_names_are_translated() {
        crate::set_language("de");
        let mut missing = Vec::new();
        for t in actcraft_engine::fixture_library::library() {
            for m in t["modes"].as_array().into_iter().flatten() {
                let names =
                    std::iter::once(&m["name"]).chain(m["ranges"].as_array().into_iter().flatten().map(|r| &r["name"]));
                for n in names.filter_map(|n| n.as_str()) {
                    if crate::tr(n) == n {
                        missing.push(n.to_owned());
                    }
                }
            }
        }
        crate::set_language("en");
        assert!(missing.is_empty(), "de.tsv lacks {missing:?}");
    }
}
