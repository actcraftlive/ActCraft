//! The DMX monitor (every level of one universe at a glance, one channel set by hand) and
//! Settings > Lighting (where the levels go on the network).

use actcraft_engine::Engine;
use actcraft_engine::dmx::{CHANNELS, UNIVERSES};
use actcraft_show::{DmxOutput, DmxProtocol};
use egui::{Rect, RichText, Sense, Ui, vec2};
use serde_json::json;

use crate::ActCraftApp;
use crate::i18n::tr;

/// Channels in one row of the monitor come in groups of this many.
const GROUP: usize = 8;

/// What the DMX monitor shows and edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmxMonitor {
    /// The universe shown, from 1.
    pub universe: u16,
    /// The channel the level control sets, from 1.
    pub channel: u16,
}

impl Default for DmxMonitor {
    fn default() -> Self {
        Self { universe: 1, channel: 1 }
    }
}

fn protocol_text(p: DmxProtocol) -> &'static str {
    match p {
        DmxProtocol::ArtNet => "Art-Net",
        DmxProtocol::Sacn => "sACN",
    }
}

impl ActCraftApp {
    /// Settings > Lighting: where the DMX levels go on the network.
    pub(crate) fn lighting_settings(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        ui.label(RichText::new(tr("DMX outputs")).strong());
        ui.label(
            RichText::new(tr("Each output sends one universe to lighting desks, nodes and fixtures on the network."))
                .color(t.text_dim),
        );
        let outputs = e.show.dmx_outputs.clone();
        let mut remove = None;
        if outputs.is_empty() {
            ui.label(RichText::new(tr("No outputs yet: DMX levels stay in ActCraft.")).color(t.text_dim));
        } else {
            egui::Grid::new("dmx-outputs").num_columns(6).striped(true).show(ui, |ui| {
                for head in [tr("Universe"), tr("Protocol"), tr("Network universe"), tr("To"), tr("Priority")] {
                    ui.label(RichText::new(head).strong());
                }
                ui.end_row();
                for (i, o) in outputs.iter().enumerate() {
                    ui.label(o.universe.to_string());
                    ui.label(protocol_text(o.protocol));
                    ui.label(o.network_universe.to_string());
                    ui.label(if o.address.is_empty() {
                        tr("Everyone on the network").to_owned()
                    } else {
                        o.address.clone()
                    });
                    ui.label(if o.protocol == DmxProtocol::Sacn {
                        o.priority.to_string()
                    } else {
                        "\u{2013}".to_owned()
                    });
                    if ui.button(tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        }
        if let Some(i) = remove {
            self.run(e, "dmx.output_remove", json!({"output": i}));
        }
        ui.add_space(8.0);
        let d = &mut self.dmx_draft;
        let mut changed_universe = false;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Universe"));
            changed_universe |= ui
                .add(egui::DragValue::new(&mut d.universe).range(1..=UNIVERSES as u16))
                .labelled_by(label.id)
                .changed();
            let label = ui.label(tr("Protocol"));
            egui::ComboBox::from_id_salt("dmx-protocol")
                .selected_text(protocol_text(d.protocol))
                .show_ui(ui, |ui| {
                    for p in [DmxProtocol::ArtNet, DmxProtocol::Sacn] {
                        if ui.selectable_label(d.protocol == p, protocol_text(p)).clicked() && d.protocol != p {
                            d.protocol = p;
                            changed_universe = true;
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if changed_universe {
                // Art-Net counts universes from 0, sACN from 1.
                let n = if d.protocol == DmxProtocol::ArtNet { d.universe.saturating_sub(1) } else { d.universe };
                d.network_universe = DmxOutput::clamp_network_universe(d.protocol, n);
            }
            let label = ui.label(tr("Network universe"));
            let (low, high) = match d.protocol {
                DmxProtocol::ArtNet => (0, DmxOutput::ARTNET_MAX),
                DmxProtocol::Sacn => (DmxOutput::SACN_MIN, DmxOutput::SACN_MAX),
            };
            ui.add(egui::DragValue::new(&mut d.network_universe).range(low..=high)).labelled_by(label.id);
        });
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("To"));
            ui.add(
                egui::TextEdit::singleline(&mut d.address)
                    .hint_text(tr("Everyone on the network"))
                    .desired_width(160.0),
            )
            .labelled_by(label.id)
            .on_hover_text(tr("The IPv4 address of one node or desk, like 192.168.1.20; empty sends to everyone"));
            if d.protocol == DmxProtocol::Sacn {
                let label = ui.label(tr("Priority"));
                ui.add(egui::DragValue::new(&mut d.priority).range(0..=DmxOutput::PRIORITY_MAX))
                    .labelled_by(label.id)
                    .on_hover_text(tr("Receivers take the source with the highest priority (100 normally)"));
            }
        });
        let address_ok = d.address.trim().is_empty() || d.target().is_some();
        if !address_ok {
            ui.label(RichText::new(tr("That is not an IPv4 address.")).color(t.danger));
        }
        if ui.add_enabled(address_ok, egui::Button::new(tr("Add output"))).clicked() {
            let d = self.dmx_draft.clone();
            let protocol = if d.protocol == DmxProtocol::Sacn { "sacn" } else { "artnet" };
            self.run(
                e,
                "dmx.output_add",
                json!({"universe": d.universe, "protocol": protocol, "network_universe": d.network_universe,
                    "address": d.address.trim(), "priority": d.priority}),
            );
        }
    }

    /// The DMX monitor window, while it is open.
    pub(crate) fn dmx_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let Some(mut m) = self.dmx_monitor else { return };
        let mut open = true;
        egui::Window::new(tr("DMX monitor"))
            .open(&mut open)
            .default_width(820.0)
            .default_height(560.0)
            .collapsible(false)
            .show(ctx, |ui| self.dmx_monitor_ui(ui, e, &mut m));
        self.dmx_monitor = Some(m);
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dmx_monitor = None;
        }
    }

    fn dmx_monitor_ui(&mut self, ui: &mut Ui, e: &mut Engine, m: &mut DmxMonitor) {
        let t = self.tokens;
        let levels = e.dmx.universe(usize::from(m.universe)).copied().unwrap_or([0; CHANNELS]);
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Universe"));
            ui.add(egui::DragValue::new(&mut m.universe).range(1..=UNIVERSES as u16)).labelled_by(label.id);
            ui.separator();
            let label = ui.label(tr("Channel"));
            ui.add(egui::DragValue::new(&mut m.channel).range(1..=CHANNELS as u16)).labelled_by(label.id);
            let label = ui.label(tr("Level"));
            let mut level = levels.get(usize::from(m.channel).saturating_sub(1)).copied().unwrap_or(0);
            if ui.add(egui::Slider::new(&mut level, 0..=255)).labelled_by(label.id).changed() {
                self.run(e, "dmx.set", json!({"universe": m.universe, "channel": m.channel, "value": level}));
            }
            ui.separator();
            if ui.button(tr("This universe to zero")).clicked() {
                self.run(e, "dmx.clear", json!({"universe": m.universe}));
            }
        });
        ui.label(
            RichText::new(tr("Levels 0-255 of every channel; click one to set it, or pick it by number."))
                .color(t.text_dim),
        );
        ui.add_space(4.0);
        let cell = vec2(26.0, 30.0);
        // As many groups of channels in a row as the window is wide (at least one group).
        let room = (ui.available_width() - 40.0) / (cell.x + 2.0);
        let per_row = ((room.max(1.0) as usize) / GROUP).max(1) * GROUP;
        egui::ScrollArea::vertical().id_salt("dmx-levels-scroll").auto_shrink([false, true]).show(ui, |ui| {
            self.dmx_grid(ui, &levels, per_row, cell, m);
        });
    }

    fn dmx_grid(&self, ui: &mut Ui, levels: &[u8; CHANNELS], per_row: usize, cell: egui::Vec2, m: &mut DmxMonitor) {
        let t = self.tokens;
        egui::Grid::new("dmx-levels").spacing(vec2(2.0, 2.0)).min_col_width(cell.x).show(ui, |ui| {
            for (row, chunk) in levels.chunks(per_row).enumerate() {
                ui.label(RichText::new(format!("{}", row * per_row + 1)).monospace().color(t.text_dim));
                for (i, level) in chunk.iter().enumerate() {
                    let channel = row * per_row + i + 1;
                    let (rect, r) = ui.allocate_exact_size(cell, Sense::click());
                    let chosen = usize::from(m.channel) == channel;
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 2.0, t.surface);
                    // The level as a bar from the bottom, the number on top.
                    let height = rect.height() * f32::from(*level) / 255.0;
                    let bar = Rect::from_min_max(rect.left_bottom() - vec2(0.0, height), rect.right_bottom());
                    painter.rect_filled(bar, 2.0, t.accent.gamma_multiply(0.45));
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        level.to_string(),
                        egui::FontId::monospace(10.0),
                        t.text,
                    );
                    if chosen {
                        painter.rect_stroke(
                            rect.shrink(1.0),
                            2.0,
                            egui::Stroke::new(2.0, t.focus),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let name = tr("Channel {channel}: {level}")
                        .replace("{channel}", &channel.to_string())
                        .replace("{level}", &level.to_string());
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    if r.on_hover_text(&name).clicked() {
                        m.channel = u16::try_from(channel).unwrap_or(1);
                    }
                }
                ui.end_row();
            }
        });
    }
}
