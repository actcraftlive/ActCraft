//! The pads: a row of buttons along the bottom of the window, each running a command or a macro
//! with one press (`pad.press`), and their editor.

use actcraft_engine::Engine;
use actcraft_show::limits::MAX_PAD_PAGES;
use actcraft_show::{FaderTarget, ToggleTarget};
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
    /// A fader instead (then `runs` is empty).
    pub fader: Option<FaderTarget>,
    /// Or a toggle.
    pub toggle: Option<ToggleTarget>,
}

/// The volume a fader shows now, in dB.
fn fader_db(e: &Engine, target: FaderTarget) -> f64 {
    match target {
        FaderTarget::Master => e.show.mixer.master_volume_db,
        FaderTarget::Song => e.current().map_or(0.0, |s| s.volume_db),
        FaderTarget::Bus(b) => e.show.buses.get(usize::from(b)).map_or(0.0, |bus| bus.volume_db),
    }
}

/// The command that sets a fader's volume.
fn fader_command(e: &Engine, target: FaderTarget, db: f64) -> Option<(&'static str, serde_json::Value)> {
    match target {
        FaderTarget::Master => Some(("mixer.master_volume", json!({"db": db}))),
        FaderTarget::Song => e.current_song.map(|id| ("song.set", json!({"song": id, "volume_db": db}))),
        FaderTarget::Bus(b) => Some(("bus.set", json!({"bus": b, "volume_db": db}))),
    }
}

/// What a toggle switches, in words.
fn toggle_text(target: ToggleTarget) -> String {
    match target {
        ToggleTarget::Metronome => tr("Switch the click").to_owned(),
        ToggleTarget::LoopSong => tr("Switch looping the song").to_owned(),
        ToggleTarget::Autoplay => tr("Switch playing the next song").to_owned(),
        ToggleTarget::Blackout => tr("Switch the blackout").to_owned(),
        ToggleTarget::Freeze => tr("Switch holding fades and effects").to_owned(),
        ToggleTarget::Blind => tr("Switch blind").to_owned(),
    }
}

/// A fader's target in words.
fn fader_text(target: FaderTarget) -> String {
    match target {
        FaderTarget::Master => tr("Master volume").to_owned(),
        FaderTarget::Song => tr("Song volume").to_owned(),
        FaderTarget::Bus(b) => {
            actcraft_engine::fill(tr("Bus {bus} volume"), &[("bus", &(u16::from(b) + 1).to_string())])
        }
    }
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

/// Going through the pad pages: the page before, which page shows, the next page, and (while
/// editing) a new page. Gives the command to run.
fn pad_pages_ui(ui: &mut Ui, page: usize, pages: usize, editing: bool) -> Option<(&'static str, serde_json::Value)> {
    let mut run = None;
    if ui.add_enabled(pages > 1, egui::Button::new(tr("Page before"))).clicked() {
        run = Some(("pad.page_before", serde_json::Value::Null));
    }
    let of = actcraft_engine::fill(
        tr("Page {page} of {pages}"),
        &[("page", &(page + 1).to_string()), ("pages", &pages.to_string())],
    );
    ui.label(RichText::new(of).strong());
    if ui.add_enabled(pages > 1, egui::Button::new(tr("Next page"))).clicked() {
        run = Some(("pad.page_next", serde_json::Value::Null));
    }
    if editing
        && ui
            .add_enabled(pages < MAX_PAD_PAGES, egui::Button::new(tr("New page")))
            .on_hover_text(tr("An empty pad page; new pads go on the page shown"))
            .clicked()
    {
        run = Some(("pad.page", json!({"page": pages})));
    }
    ui.separator();
    run
}

/// A pad's buttons while editing: to another page, and remove. Their names say which pad.
fn pad_edit_buttons(
    ui: &mut Ui,
    pad: &actcraft_show::Pad,
    i: usize,
    pages: usize,
    remove: &mut Option<usize>,
    move_to: &mut Option<(usize, usize)>,
) {
    // Its page, chosen from the pages there are and a new one after the last.
    let page_text = |p: usize| actcraft_engine::fill(tr("Page {page}"), &[("page", &(p + 1).to_string())]);
    let now = usize::from(pad.page);
    let name = format!("{} ({})", tr("Page"), pad.label);
    let r = egui::ComboBox::from_id_salt(("pad-page", i))
        .width(80.0)
        .selected_text(page_text(now))
        .show_ui(ui, |ui| {
            for p in 0..(pages + 1).min(MAX_PAD_PAGES) {
                if ui.selectable_label(p == now, page_text(p)).clicked() && p != now {
                    *move_to = Some((i, p));
                }
            }
        })
        .response;
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &name));
    let name = format!("{} ({})", tr("Remove"), pad.label);
    let r = ui.small_button(tr("Remove"));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if r.clicked() {
        *remove = Some(i);
    }
}

impl ActCraftApp {
    /// The pads, and the editor while it is open.
    pub(crate) fn pads_bar(&mut self, ui: &mut Ui, e: &mut Engine) {
        let pads = e.show.pads.clone();
        let mut press = None;
        let mut remove = None;
        let mut moved: Option<(FaderTarget, f64)> = None;
        let mut page_run: Option<(&str, serde_json::Value)> = None;
        let mut move_to: Option<(usize, usize)> = None;
        let (page, pages) = (e.pad_page(), e.pad_pages());
        ui.horizontal_wrapped(|ui| {
            if pages > 1 || self.pads_editing {
                page_run = pad_pages_ui(ui, page, pages, self.pads_editing);
            }
            for i in e.shown_pads() {
                let Some(pad) = pads.get(i) else { continue };
                if let Some(target) = pad.fader {
                    // A fader: its label and the volume it moves, in dB.
                    let label = ui.label(RichText::new(&pad.label).strong());
                    let mut db = fader_db(e, target);
                    ui.spacing_mut().slider_width = 140.0;
                    if ui
                        .add(egui::Slider::new(&mut db, -60.0..=12.0).suffix(" dB").fixed_decimals(1))
                        .labelled_by(label.id)
                        .changed()
                    {
                        moved = Some((target, db));
                    }
                    if self.pads_editing {
                        pad_edit_buttons(ui, pad, i, pages, &mut remove, &mut move_to);
                    }
                    continue;
                }
                let [r, g, b] = actcraft_show::parse_colour(&pad.colour).unwrap_or([79, 156, 240]);
                if let Some(target) = pad.toggle {
                    // A toggle: filled in its colour and saying "on" while on, outlined while off.
                    let on = actcraft_engine::pads::toggle_state(e, target);
                    let state = if on { tr("on") } else { tr("off") };
                    let words = format!("{} \u{b7} {state}", pad.label);
                    let colour = Color32::from_rgb(r, g, b);
                    let button = if on {
                        egui::Button::new(RichText::new(words).size(16.0).color(text_on([r, g, b]))).fill(colour)
                    } else {
                        egui::Button::new(RichText::new(words).size(16.0)).stroke(egui::Stroke::new(2.0, colour))
                    };
                    let r = ui.add(button.min_size(vec2(96.0, 40.0)).selected(on)).on_hover_text(toggle_text(target));
                    if r.clicked() {
                        press = Some(i);
                    }
                    if self.pads_editing {
                        pad_edit_buttons(ui, pad, i, pages, &mut remove, &mut move_to);
                    }
                    continue;
                }
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
                if self.pads_editing {
                    pad_edit_buttons(ui, pad, i, pages, &mut remove, &mut move_to);
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
            crate::help_view::help_button(ui, crate::help_view::Topic::Pads);
        });
        if let Some(i) = press {
            self.run(e, "pad.press", json!({"pad": i}));
        }
        if let Some((target, db)) = moved
            && let Some((id, prm)) = fader_command(e, target, db)
        {
            self.run(e, id, prm);
        }
        if let Some(i) = remove {
            self.run(e, "pad.remove", json!({"pad": i}));
        }
        if let Some((pad, page)) = move_to {
            self.run(e, "pad.move", json!({"pad": pad, "page": page}));
        }
        if let Some((id, prm)) = page_run {
            self.run(e, id, prm);
        }
        if self.pads_editing {
            self.pad_editor(ui, e);
        }
    }

    /// The form for a new pad: its label, colour and what it runs.
    fn pad_editor(&mut self, ui: &mut Ui, e: &mut Engine) {
        let macros: Vec<String> = e.show.macros.iter().map(|m| m.name.clone()).collect();
        let buses = u8::try_from(e.show.buses.len()).unwrap_or(0);
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
            let shown = match (d.fader, d.toggle, d.runs.strip_prefix('$')) {
                (Some(target), _, _) => fader_text(target),
                (None, Some(target), _) => toggle_text(target),
                (None, None, Some(name)) => format!("${name}"),
                (None, None, None) => commands
                    .iter()
                    .find(|c| c.0 == d.runs)
                    .map_or(tr("Choose a command").to_owned(), |c| c.1.to_owned()),
            };
            let mut targets = vec![FaderTarget::Master, FaderTarget::Song];
            targets.extend((0..buses).map(FaderTarget::Bus));
            let toggles = [
                ToggleTarget::Metronome,
                ToggleTarget::LoopSong,
                ToggleTarget::Autoplay,
                ToggleTarget::Blackout,
                ToggleTarget::Freeze,
                ToggleTarget::Blind,
            ];
            egui::ComboBox::from_id_salt("pad-runs")
                .width(240.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for target in &targets {
                        if ui.selectable_label(d.fader == Some(*target), fader_text(*target)).clicked() {
                            d.fader = Some(*target);
                            d.toggle = None;
                            d.runs.clear();
                        }
                    }
                    for target in &toggles {
                        if ui.selectable_label(d.toggle == Some(*target), toggle_text(*target)).clicked() {
                            d.toggle = Some(*target);
                            d.fader = None;
                            d.runs.clear();
                        }
                    }
                    ui.separator();
                    let plain = d.fader.is_none() && d.toggle.is_none();
                    for name in &macros {
                        let key = format!("${name}");
                        if ui.selectable_label(plain && d.runs == key, &key).clicked() {
                            d.runs = key;
                            d.fader = None;
                            d.toggle = None;
                        }
                    }
                    for (id, name) in &commands {
                        if ui.selectable_label(plain && d.runs == *id, *name).clicked() {
                            d.runs = (*id).to_owned();
                            d.fader = None;
                            d.toggle = None;
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        let d = &self.pad_draft;
        let ready = !d.label.trim().is_empty() && (!d.runs.is_empty() || d.fader.is_some() || d.toggle.is_some());
        if ui.add_enabled(ready, egui::Button::new(tr("Add pad"))).clicked() {
            let d = std::mem::take(&mut self.pad_draft);
            let colour = MARK_COLOURS.get(d.colour).map_or("#4f9cf0", |c| c.1);
            let mut prm = json!({"label": d.label.trim(), "colour": colour});
            match (d.fader, d.toggle, d.runs.strip_prefix('$')) {
                (Some(FaderTarget::Master), _, _) => prm["fader"] = json!("master"),
                (Some(FaderTarget::Song), _, _) => prm["fader"] = json!("song"),
                (Some(FaderTarget::Bus(b)), _, _) => {
                    prm["fader"] = json!("bus");
                    prm["bus"] = json!(b);
                }
                (None, Some(t), _) => {
                    prm["toggle"] = json!(match t {
                        ToggleTarget::Metronome => "metronome",
                        ToggleTarget::LoopSong => "loop_song",
                        ToggleTarget::Autoplay => "autoplay",
                        ToggleTarget::Blackout => "blackout",
                        ToggleTarget::Freeze => "freeze",
                        ToggleTarget::Blind => "blind",
                    });
                }
                (None, None, Some(name)) => prm["macro"] = json!(name),
                (None, None, None) => prm["command"] = json!(d.runs),
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
