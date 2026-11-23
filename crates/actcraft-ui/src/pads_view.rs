//! The pads: a row of buttons along the bottom of the window, each running a command or a macro
//! with one press (`pad.press`), and their editor.

use actcraft_engine::Engine;
use egui::{Color32, RichText, Ui, vec2};
use serde_json::json;

use crate::sheet_view::MARK_COLOURS;
use crate::{ActCraftApp, tr};

/// A pad being put together: label, colour (index into the named colours) and what it runs
/// (a command id, or `$` and a macro's name).
#[derive(Debug, Clone, Default)]
pub(crate) struct PadDraft {
    pub label: String,
    pub colour: usize,
    pub runs: String,
}

/// Black or white words, whichever stands out more from a pad's colour (WCAG contrast).
fn text_on(rgb: [u8; 3]) -> Color32 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let [r, g, b] = rgb;
    let l = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    let on_black = (l + 0.05) / 0.05;
    let on_white = 1.05 / (l + 0.05);
    if on_black >= on_white { Color32::BLACK } else { Color32::WHITE }
}

impl ActCraftApp {
    /// The pads, and the editor while it is open.
    pub(crate) fn pads_bar(&mut self, ui: &mut Ui, e: &mut Engine) {
        let pads = e.show.pads.clone();
        let mut press = None;
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, pad) in pads.iter().enumerate() {
                let [r, g, b] = actcraft_show::parse_colour(&pad.colour).unwrap_or([79, 156, 240]);
                let fill = Color32::from_rgb(r, g, b);
                let text = text_on([r, g, b]);
                let button = egui::Button::new(RichText::new(&pad.label).size(16.0).color(text))
                    .fill(fill)
                    .min_size(vec2(96.0, 40.0));
                let hint = if pad.macro_name.is_empty() {
                    let label =
                        actcraft_engine::registry().get(&pad.command).map_or(pad.command.as_str(), |c| c.spec.label);
                    tr(label).to_owned()
                } else {
                    format!("${}", pad.macro_name)
                };
                if ui.add(button).on_hover_text(hint).clicked() {
                    press = Some(i);
                }
                if self.pads_editing && ui.small_button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
            }
            let edit = if self.pads_editing {
                tr("Done")
            } else if pads.is_empty() {
                tr("Add pads\u{2026}")
            } else {
                tr("Edit pads")
            };
            if ui
                .selectable_label(self.pads_editing, edit)
                .on_hover_text(tr("Buttons that run a command or a macro with one press"))
                .clicked()
            {
                self.pads_editing = !self.pads_editing;
            }
        });
        if let Some(i) = press {
            self.run(e, "pad.press", json!({"pad": i}));
        }
        if let Some(i) = remove {
            self.run(e, "pad.remove", json!({"pad": i}));
        }
        if self.pads_editing {
            self.pad_editor(ui, e);
        }
    }

    /// The form for a new pad: its label, colour and what it runs.
    fn pad_editor(&mut self, ui: &mut Ui, e: &mut Engine) {
        let macros: Vec<String> = e.show.macros.iter().map(|m| m.name.clone()).collect();
        let d = &mut self.pad_draft;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("Label"));
            ui.add(egui::TextEdit::singleline(&mut d.label).desired_width(140.0)).labelled_by(label.id);
            let label = ui.label(tr("Colour"));
            egui::ComboBox::from_id_salt("pad-colour")
                .selected_text(tr(MARK_COLOURS.get(d.colour).map_or("", |c| c.0)))
                .show_ui(ui, |ui| {
                    for (i, (name, _)) in MARK_COLOURS.iter().enumerate() {
                        ui.selectable_value(&mut d.colour, i, tr(name));
                    }
                })
                .response
                .labelled_by(label.id);
            let label = ui.label(tr("Runs"));
            let mut commands: Vec<(&'static str, &'static str)> = actcraft_engine::registry()
                .iter()
                .filter(|c| actcraft_engine::remote::runnable(c.spec.id))
                .map(|c| (c.spec.id, tr(c.spec.label)))
                .collect();
            commands.sort_by(|a, b| a.1.cmp(b.1));
            let shown = match d.runs.strip_prefix('$') {
                Some(name) => format!("${name}"),
                None => commands
                    .iter()
                    .find(|c| c.0 == d.runs)
                    .map_or(tr("Choose a command").to_owned(), |c| c.1.to_owned()),
            };
            egui::ComboBox::from_id_salt("pad-runs")
                .width(240.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for name in &macros {
                        let key = format!("${name}");
                        if ui.selectable_label(d.runs == key, &key).clicked() {
                            d.runs = key;
                        }
                    }
                    for (id, name) in &commands {
                        if ui.selectable_label(d.runs == *id, *name).clicked() {
                            d.runs = (*id).to_owned();
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let ready = !self.pad_draft.label.trim().is_empty() && !self.pad_draft.runs.is_empty();
        if ui.add_enabled(ready, egui::Button::new(tr("Add pad"))).clicked() {
            let d = std::mem::take(&mut self.pad_draft);
            let colour = MARK_COLOURS.get(d.colour).map_or("#4f9cf0", |c| c.1);
            let mut prm = json!({"label": d.label.trim(), "colour": colour});
            match d.runs.strip_prefix('$') {
                Some(name) => prm["macro"] = json!(name),
                None => prm["command"] = json!(d.runs),
            }
            self.run(e, "pad.add", prm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_words_take_the_colour_that_stands_out() {
        // Every named colour gets words with at least 4.5:1 contrast.
        for (name, hex) in MARK_COLOURS {
            let rgb = actcraft_show::parse_colour(hex).unwrap_or([0, 0, 0]);
            let lin = |c: u8| {
                let c = f64::from(c) / 255.0;
                if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
            };
            let l = 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
            let ratio = if text_on(rgb) == Color32::BLACK { (l + 0.05) / 0.05 } else { 1.05 / (l + 0.05) };
            assert!(ratio >= 4.5, "{name}: {ratio:.2}");
        }
        assert_eq!(text_on([0, 0, 0]), Color32::WHITE);
        assert_eq!(text_on([255, 255, 255]), Color32::BLACK);
    }
}
