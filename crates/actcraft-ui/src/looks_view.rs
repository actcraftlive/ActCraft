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
    /// Seconds before a new cue goes on to the next by itself (0: waits for Go).
    pub cue_follow: f64,
    /// A new song lighting cue: its time, cue list and cue.
    pub light_time: f64,
    pub light_list: usize,
    pub light_cue: usize,
    /// An effect being put together.
    pub effect: actcraft_show::Effect,
    pub effect_group: usize,
}

/// A ready-made effect's name in the window's language.
fn template_text(key: &str) -> &'static str {
    match key {
        "breathe" => tr("Breathe"),
        "beat_pulse" => tr("Pulse on the beat"),
        "chase" => tr("Chase"),
        "wave" => tr("Wave"),
        "build" => tr("Build over a bar"),
        "bounce" => tr("Bounce"),
        "red_pulse" => tr("Red pulse"),
        "blue_wave" => tr("Blue wave"),
        "warm_drift" => tr("Warm drift"),
        "sweep" => tr("Sweep"),
        "fan" => tr("Fan out"),
        "nod" => tr("Nod"),
        "tilt_wave" => tr("Tilt wave"),
        "zoom_breathe" => tr("Zoom in and out"),
        "circle" => tr("Circle"),
        "circle_wave" => tr("Circles one after another"),
        "rainbow" => tr("Rainbow"),
        "rainbow_wave" => tr("Rainbow wave"),
        "slow_wave" => tr("Slow wave"),
        "pulse_two_beats" => tr("Pulse every other beat"),
        "breathe_bar" => tr("Breathe over a bar"),
        "build_two_bars" => tr("Build over two bars"),
        "every_other" => tr("Every other light"),
        "slow_chase" => tr("Slow chase"),
        "wave_on_the_bar" => tr("Wave over a bar"),
        "green_pulse" => tr("Green pulse"),
        "white_wave" => tr("White wave"),
        "uv_glow" => tr("UV glow"),
        "amber_chase" => tr("Amber chase"),
        "red_and_blue" => tr("Red and blue"),
        "warm_and_cool" => tr("Warm and cool"),
        "embers" => tr("Embers"),
        "ocean" => tr("Ocean"),
        "colour_chase" => tr("Colour chase"),
        "pan_wave_beat" => tr("Pan wave on the beat"),
        "nod_beat" => tr("Nod on the beat"),
        "sweep_room" => tr("Sweep the room"),
        "circle_bar" => tr("Circle over a bar"),
        "zoom_beat" => tr("Zoom on the beat"),
        "slow_rise" => tr("Slow rise"),
        "half_wave" => tr("Half wave"),
        "soft_chase" => tr("Soft chase"),
        "bar_chase" => tr("Chase over a bar"),
        "ramp_wave" => tr("Ramp wave"),
        "breathe_two_bars" => tr("Breathe over two bars"),
        "slow_bounce" => tr("Slow bounce"),
        "halves" => tr("Halves in turn"),
        "cyan_wave" => tr("Cyan wave"),
        "magenta_pulse" => tr("Magenta pulse"),
        "yellow_drift" => tr("Yellow drift"),
        "sunset" => tr("Sunset"),
        "aurora" => tr("Aurora"),
        "red_and_white" => tr("Red and white"),
        "blue_and_white" => tr("Blue and white"),
        "uv_wave" => tr("UV wave"),
        "white_beat" => tr("White on the beat"),
        "rainbow_beat" => tr("Rainbow on the beat"),
        "amber_breathe" => tr("Amber breathe"),
        "green_chase" => tr("Green chase"),
        "slow_sweep" => tr("Slow sweep"),
        "tilt_sweep" => tr("Tilt sweep"),
        "fan_bar" => tr("Fan over a bar"),
        "tilt_fan" => tr("Tilt fan"),
        "crossing" => tr("Crossing"),
        "slow_circle" => tr("Slow circle"),
        "circles_beat" => tr("Circles on the beat"),
        "sway" => tr("Sway"),
        "tilt_bounce_beat" => tr("Tilt bounce on the beat"),
        "zoom_wave" => tr("Zoom wave"),
        "zoom_build" => tr("Zoom build over a bar"),
        _ => tr("Effect"),
    }
}

/// An effect's shape in words.
fn shape_text(s: actcraft_engine::effects::Shape) -> &'static str {
    use actcraft_engine::effects::Shape;
    match s {
        Shape::Sine => tr("Smooth wave"),
        Shape::Square => tr("On and off"),
        Shape::Saw => tr("Ramp up"),
        Shape::Triangle => tr("Up and down"),
    }
}

/// Seconds as m:ss.s.
fn at_text(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}

/// Where `seconds` is on the beat grid, as "bar.beat" (both from 1, as the grid numbers them).
fn bar_text(clock: &actcraft_engine::BeatClock, seconds: f64) -> String {
    let (bar, beat) = clock.bar_beat(seconds);
    format!("{}.{}", bar + 1, u16::from(beat) + 1)
}

/// A song lighting cue's cue, in words: "2. Chorus", or that it is gone.
fn light_cue_name(e: &Engine, c: &actcraft_show::LightCue) -> String {
    e.show
        .cue_lists
        .iter()
        .find(|l| l.name == c.list)
        .and_then(|l| l.cues.get(c.cue))
        .map_or_else(|| tr("(not there)").to_owned(), |cue| format!("{}. {}", c.cue + 1, cue.name))
}

impl ActCraftApp {
    /// While blind is on: says so, where fixtures are changed or shown, with a way out.
    pub(crate) fn blind_note(&mut self, ui: &mut Ui, e: &mut Engine) {
        if !e.dmx.blind() {
            return;
        }
        let t = self.tokens;
        let mut leave = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(tr("Blind is on: changes here stay off the lights; store them as presets."))
                    .color(t.warning)
                    .strong(),
            );
            leave = ui.button(tr("Leave blind")).clicked();
        });
        if leave {
            self.run(e, "lighting.blind", json!({"on": false}));
        }
    }

    /// The song as a lighting lane: its bars, where playback is, and each lighting cue as a mark.
    /// A click on the lane sets where a new cue goes (on the beat with Snap to beat); a click on
    /// a mark jumps there, and dragging a mark moves its cue (the list below moves by beats).
    fn light_lane(
        &mut self,
        ui: &mut Ui,
        e: &Engine,
        song: &actcraft_show::Song,
        clock: &actcraft_engine::BeatClock,
        run: &mut Option<(&'static str, Value)>,
    ) {
        use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};
        let t = self.tokens;
        ui.checkbox(&mut self.snap_to_beat, tr("Snap to beat"));
        let playing_here = e.current_song == Some(song.id);
        let now = if playing_here { e.status.time.max(0.0) } else { 0.0 };
        let last = song.light_cues.last().map_or(0.0, |c| c.time + 4.0);
        let end = song.duration.max(last).max(now).max(30.0);
        let width = ui.available_width().max(200.0);
        let (rect, lane) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
        let lane_name = tr("Lighting lane");
        lane.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, lane_name));
        let x_of = |s: f64| rect.left() + (s / end * f64::from(width)) as f32;
        let time_at = |x: f32| (f64::from((x - rect.left()) / width) * end).clamp(0.0, end);
        let snap = self.snap_to_beat;
        let placed = |s: f64| if snap { clock.snap(s, 1).max(0.0) } else { s };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, t.radius, t.surface);
        // Bar lines, numbered, thinned out so the numbers don't run into each other.
        let px_bar = (clock.beat_len() * f64::from(clock.beats_per_bar) / end * f64::from(width)) as f32;
        if px_bar >= 2.0 {
            let every = (40.0 / px_bar).ceil().max(1.0) as i64;
            for line in clock.grid(0.0, end, 20_000).iter().filter(|l| l.bar_start) {
                if line.bar.rem_euclid(every) == 1 % every {
                    let x = x_of(line.time);
                    painter.vline(x, rect.y_range(), Stroke::new(1.0, t.text_dim.gamma_multiply(0.6)));
                    painter.text(
                        pos2(x + 3.0, rect.bottom() - 2.0),
                        Align2::LEFT_BOTTOM,
                        line.bar.to_string(),
                        FontId::monospace(11.0),
                        t.text_dim,
                    );
                }
            }
        }
        if playing_here {
            painter.vline(x_of(now), rect.y_range(), Stroke::new(2.0, t.warning));
        }
        for (i, c) in song.light_cues.iter().enumerate() {
            let x = x_of(c.time);
            let hit = Rect::from_min_max(pos2(x - 6.0, rect.top()), pos2(x + 6.0, rect.bottom()));
            let r = ui.interact(hit, egui::Id::unique(("light-mark", song.id, i)), Sense::click_and_drag());
            let name = light_cue_name(e, c);
            let label = format!("{name}, {} {}, {}", tr("Bar"), bar_text(clock, c.time), at_text(c.time));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            let mut shown = x;
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
            {
                shown = x_of(placed(time_at(p.x)));
            }
            if r.drag_stopped()
                && let Some(p) = r.interact_pointer_pos()
            {
                let to = placed(time_at(p.x));
                *run = Some(("song.light_cue_move", json!({"song": song.id, "light_cue": i, "time": to})));
            } else if r.clicked() {
                *run = Some(("transport.seek", json!({"seconds": c.time})));
            }
            let colour = if r.hovered() || r.dragged() || r.has_focus() { t.focus } else { t.accent };
            painter.vline(shown, rect.y_range(), Stroke::new(3.0, colour));
            // Near the right end the name goes left of the mark, so it stays readable.
            let (at, align) = if shown > rect.right() - 100.0 {
                (pos2(shown - 4.0, rect.top() + 2.0), Align2::RIGHT_TOP)
            } else {
                (pos2(shown + 4.0, rect.top() + 2.0), Align2::LEFT_TOP)
            };
            painter.text(at, align, &name, FontId::proportional(12.0), t.text);
            r.on_hover_text(tr("Click to jump there; drag to move the cue"));
        }
        if run.is_none()
            && lane.clicked()
            && let Some(p) = lane.interact_pointer_pos()
        {
            self.looks_draft.light_time = placed(time_at(p.x));
        }
        lane.on_hover_text(tr("Click to set where a new cue goes"));
    }

    /// The song's Lighting tab: the cues it fires as it plays, and adding one.
    pub(crate) fn song_light_tab(&mut self, ui: &mut Ui, e: &mut Engine, song: &actcraft_show::Song) {
        let t = self.tokens;
        ui.label(
            RichText::new(tr("Lighting cues this song fires as it plays (a jump skips the ones in between)."))
                .color(t.text_dim),
        );
        let clock = actcraft_engine::BeatClock::new(song.bpm, song.beats_per_bar, song.beat_anchor);
        let mut run: Option<(&str, Value)> = None;
        self.light_lane(ui, e, song, &clock, &mut run);
        egui::Grid::new("song-light-cues").num_columns(6).striped(true).show(ui, |ui| {
            for (i, c) in song.light_cues.iter().enumerate() {
                let name = light_cue_name(e, c);
                ui.label(at_text(c.time));
                ui.label(format!("{} {}", tr("Bar"), bar_text(&clock, c.time)));
                ui.label(&c.list);
                ui.label(&name);
                // Moving by a beat, for the keyboard (the lane above also takes a drag).
                let beat = clock.beat_len();
                for (words, by) in [(tr("Beat earlier"), -beat), (tr("Beat later"), beat)] {
                    let r = ui.small_button(words);
                    let label = format!("{words} ({name})");
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if r.clicked() {
                        let to = clock.snap(c.time + by, 1).max(0.0);
                        run = Some(("song.light_cue_move", json!({"song": song.id, "light_cue": i, "time": to})));
                    }
                }
                let r = ui.small_button(tr("Remove"));
                let label = format!("{} ({name})", tr("Remove"));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                if r.clicked() {
                    run = Some(("song.light_cue_remove", json!({"song": song.id, "light_cue": i})));
                }
                ui.end_row();
            }
        });
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
        }
        let lists = e.show.cue_lists.clone();
        if lists.iter().all(|l| l.cues.is_empty()) {
            ui.label(RichText::new(tr("Make a cue list with cues first (Lighting > Presets).")).color(t.text_dim));
            return;
        }
        let now = e.status.time;
        let d = &mut self.looks_draft;
        d.light_list = d.light_list.min(lists.len().saturating_sub(1));
        let cues = lists.get(d.light_list).map(|l| l.cues.clone()).unwrap_or_default();
        d.light_cue = d.light_cue.min(cues.len().saturating_sub(1));
        let mut add = false;
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(tr("At"));
            ui.add(egui::DragValue::new(&mut d.light_time).range(0.0..=86_400.0).speed(0.1).suffix(" s"))
                .labelled_by(label.id);
            if ui.button(tr("Now")).on_hover_text(tr("Where playback is")).clicked() {
                d.light_time = (now * 10.0).round() / 10.0;
            }
            ui.label(format!("{} {}", tr("Bar"), bar_text(&clock, d.light_time)));
            let label = ui.label(tr("Cue list"));
            egui::ComboBox::from_id_salt("light-list")
                .selected_text(lists.get(d.light_list).map(|l| l.name.clone()).unwrap_or_default())
                .show_ui(ui, |ui| {
                    for (i, l) in lists.iter().enumerate() {
                        ui.selectable_value(&mut d.light_list, i, &l.name);
                    }
                })
                .response
                .labelled_by(label.id);
            let label = ui.label(tr("Cue"));
            egui::ComboBox::from_id_salt("light-cue")
                .selected_text(
                    cues.get(d.light_cue).map(|c| format!("{}. {}", d.light_cue + 1, c.name)).unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    for (i, c) in cues.iter().enumerate() {
                        ui.selectable_value(&mut d.light_cue, i, format!("{}. {}", i + 1, c.name));
                    }
                })
                .response
                .labelled_by(label.id);
            add = ui.add_enabled(!cues.is_empty(), egui::Button::new(tr("Add lighting cue"))).clicked();
        });
        if add {
            let d = self.looks_draft.clone();
            self.run(
                e,
                "song.light_cue_add",
                json!({"song": song.id, "time": d.light_time, "list": d.light_list, "cue": d.light_cue}),
            );
        }
    }

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
                    self.output_ui(ui, e);
                    ui.separator();
                    self.presets_ui(ui, e);
                    ui.separator();
                    self.cue_lists_ui(ui, e);
                    ui.separator();
                    self.effects_ui(ui, e);
                    ui.separator();
                    self.sequences_ui(ui, e);
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
            let label = ui.label(tr("Preset name"));
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

    /// What reaches the lights: blackout, freeze and the master.
    fn output_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        let t = self.tokens;
        let mut run: Option<(&str, Value)> = None;
        ui.horizontal_wrapped(|ui| {
            let blackout = e.dmx.blackout;
            let words = format!("{} \u{b7} {}", tr("Blackout"), if blackout { tr("on") } else { tr("off") });
            let fill = if blackout { RichText::new(words).color(t.danger).strong() } else { RichText::new(words) };
            if ui
                .selectable_label(blackout, fill)
                .on_hover_text(tr("Every fixture dark; positions and levels stay"))
                .clicked()
            {
                run = Some(("dmx.blackout", Value::Null));
            }
            let frozen = e.frozen();
            let words = format!("{} \u{b7} {}", tr("Freeze"), if frozen { tr("on") } else { tr("off") });
            if ui.selectable_label(frozen, words).on_hover_text(tr("Hold fades and effects where they are")).clicked() {
                run = Some(("lighting.freeze", Value::Null));
            }
            let blind = e.dmx.blind();
            let words = format!("{} \u{b7} {}", tr("Blind"), if blind { tr("on") } else { tr("off") });
            let fill = if blind { RichText::new(words).color(t.warning).strong() } else { RichText::new(words) };
            if ui
                .selectable_label(blind, fill)
                .on_hover_text(tr("Fixture changes and recalled presets stay off the lights"))
                .clicked()
            {
                run = Some(("lighting.blind", Value::Null));
            }
            let label = ui.label(tr("Master"));
            let mut master = e.dmx.master;
            let slider = egui::Slider::new(&mut master, 0.0..=1.0)
                .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                .custom_parser(|s| s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|p| p / 100.0));
            if ui.add(slider).labelled_by(label.id).changed() {
                run = Some(("dmx.master", json!({"level": master})));
            }
        });
        if let Some((id, prm)) = run {
            self.run(e, id, prm);
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
            egui::Grid::new(("cues", li)).num_columns(6).striped(true).show(ui, |ui| {
                for (ci, cue) in list.cues.iter().enumerate() {
                    let words = format!("{}. {}", ci + 1, cue.name);
                    let text = if at == Some(ci) { RichText::new(words).strong() } else { RichText::new(words) };
                    if ui.selectable_label(at == Some(ci), text).on_hover_text(tr("Go to this cue")).clicked() {
                        run = Some(("cue.jump", json!({"list": li, "cue": ci})));
                    }
                    ui.label(&cue.preset);
                    ui.label(format!("{:.1} s", cue.fade));
                    ui.label(if cue.follow > 0.0 {
                        tr("then on after {s} s").replace("{s}", &format!("{:.1}", cue.follow))
                    } else {
                        String::new()
                    });
                    // What else the cue does: start a sequence or an effect, or stop them all.
                    const STOP_ALL: &str = "\u{0}stop";
                    let now = if cue.stops.iter().any(|s| s == "*") {
                        STOP_ALL.to_owned()
                    } else {
                        cue.starts.first().cloned().unwrap_or_default()
                    };
                    let shown = |v: &str| match v {
                        "" => tr("Nothing more").to_owned(),
                        STOP_ALL => tr("Stop all sequences and effects").to_owned(),
                        name => tr("Start {name}").replace("{name}", name),
                    };
                    let mut pick = now.clone();
                    let r = egui::ComboBox::from_id_salt(("cue-starts", li, ci))
                        .width(170.0)
                        .selected_text(shown(&now))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut pick, String::new(), shown(""));
                            for name in
                                e.show.sequences.iter().map(|s| &s.name).chain(e.show.effects.iter().map(|f| &f.name))
                            {
                                ui.selectable_value(&mut pick, name.clone(), shown(name));
                            }
                            ui.selectable_value(&mut pick, STOP_ALL.to_owned(), shown(STOP_ALL));
                        })
                        .response;
                    let name = format!("{}, {}", tr("Also"), cue.name);
                    ui.ctx().accesskit_node_builder(r.id, |n| n.set_label(name));
                    if pick != now {
                        let (starts, stops): (Vec<String>, Vec<&str>) = match pick.as_str() {
                            "" => (Vec::new(), Vec::new()),
                            STOP_ALL => (Vec::new(), vec!["*"]),
                            name => (vec![name.to_owned()], Vec::new()),
                        };
                        run = Some(("cue.set", json!({"list": li, "cue": ci, "starts": starts, "stops": stops})));
                    }
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
                    let label = ui.label(tr("Then on after"));
                    ui.add(egui::DragValue::new(&mut d.cue_follow).range(0.0..=600.0).speed(0.1).suffix(" s"))
                        .labelled_by(label.id)
                        .on_hover_text(tr(
                            "The next cue comes by itself after this long (after the last, the first); 0 waits for Go",
                        ));
                    if ui.button(tr("Add cue")).clicked() {
                        run = Some((
                            "cue.add",
                            json!({"list": li, "preset": d.cue_preset, "fade": d.cue_fade, "follow": d.cue_follow}),
                        ));
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

    /// Effects: each with Start or Stop, and a new one on a group.
    fn effects_ui(&mut self, ui: &mut Ui, e: &mut Engine) {
        use actcraft_engine::effects::{Shape, shape_key};
        use actcraft_engine::fixtures::{ATTRIBUTES, attribute_key};
        let t = self.tokens;
        ui.label(RichText::new(tr("Effects")).strong());
        let effects = e.show.effects.clone();
        let running = e.running_effects();
        let mut run: Option<(&str, Value)> = None;
        egui::Grid::new("effects").num_columns(4).striped(true).show(ui, |ui| {
            for (i, fx) in effects.iter().enumerate() {
                let on = running.contains(&fx.name);
                let words = if on { format!("{} \u{b7} {}", fx.name, tr("running")) } else { fx.name.clone() };
                if ui.selectable_label(on, words).on_hover_text(tr("Start or stop this effect")).clicked() {
                    run = Some(if on {
                        ("effect.stop", json!({"effect": i}))
                    } else {
                        ("effect.start", json!({"effect": i}))
                    });
                }
                ui.label(format!("{} \u{b7} {}", fx.group, crate::fixtures_view::attribute_text(fx.attribute)));
                let cycle = if fx.beats > 0.0 {
                    tr("{n} beats").replace("{n}", &format!("{}", fx.beats))
                } else {
                    format!("{:.1} s", fx.period)
                };
                ui.label(format!("{} \u{b7} {cycle}", shape_text(fx.shape)));
                if ui.small_button(tr("Remove")).clicked() {
                    run = Some(("effect.remove", json!({"effect": i})));
                }
                ui.end_row();
            }
        });
        let groups: Vec<String> = e.show.groups.iter().map(|g| g.name.clone()).collect();
        if groups.is_empty() {
            ui.label(RichText::new(tr("Effects move a group: save a group first.")).color(t.text_dim));
        } else {
            let d = &mut self.looks_draft;
            d.effect_group = d.effect_group.min(groups.len() - 1);
            let fx = &mut d.effect;
            ui.horizontal_wrapped(|ui| {
                // A ready-made effect fills the fields below; they can still be changed.
                let label = ui.label(tr("Start from"));
                egui::ComboBox::from_id_salt("effect-template")
                    .selected_text(tr("A ready-made effect"))
                    .show_ui(ui, |ui| {
                        for t in actcraft_engine::effects::TEMPLATES {
                            if ui.selectable_label(false, template_text(t.key)).clicked() {
                                fx.name = template_text(t.key).to_owned();
                                fx.attribute = t.attribute;
                                fx.shape = t.shape;
                                fx.period = t.period;
                                fx.beats = t.beats;
                                fx.low = t.low;
                                fx.high = t.high;
                                fx.spread = t.spread;
                                fx.also = t
                                    .also
                                    .iter()
                                    .map(|(a, offset)| actcraft_engine::effects::EffectPart {
                                        attribute: *a,
                                        offset: *offset,
                                        ..Default::default()
                                    })
                                    .collect();
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(tr("Effect name"));
                ui.add(egui::TextEdit::singleline(&mut fx.name).desired_width(120.0)).labelled_by(label.id);
                let label = ui.label(tr("Group"));
                egui::ComboBox::from_id_salt("effect-group")
                    .selected_text(groups.get(d.effect_group).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, g) in groups.iter().enumerate() {
                            ui.selectable_value(&mut d.effect_group, i, g);
                        }
                    })
                    .response
                    .labelled_by(label.id);
                let label = ui.label(tr("Attribute"));
                egui::ComboBox::from_id_salt("effect-attribute")
                    .selected_text(crate::fixtures_view::attribute_text(fx.attribute))
                    .show_ui(ui, |ui| {
                        for a in ATTRIBUTES.into_iter().filter(|a| !a.is_fine()) {
                            ui.selectable_value(&mut fx.attribute, a, crate::fixtures_view::attribute_text(a));
                        }
                    })
                    .response
                    .labelled_by(label.id);
                let label = ui.label(tr("Shape"));
                egui::ComboBox::from_id_salt("effect-shape")
                    .selected_text(shape_text(fx.shape))
                    .show_ui(ui, |ui| {
                        for s in [Shape::Sine, Shape::Square, Shape::Saw, Shape::Triangle] {
                            ui.selectable_value(&mut fx.shape, s, shape_text(s));
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
            ui.horizontal_wrapped(|ui| {
                let label = ui.label(tr("Seconds per cycle"));
                ui.add(egui::DragValue::new(&mut fx.period).range(0.05..=600.0).speed(0.05)).labelled_by(label.id);
                let label = ui.label(tr("or beats per cycle"));
                ui.add(egui::DragValue::new(&mut fx.beats).range(0.0..=64.0).speed(0.25))
                    .labelled_by(label.id)
                    .on_hover_text(tr("At the playing song's tempo; 0 uses the seconds"));
            });
            ui.horizontal_wrapped(|ui| {
                for (key, value) in
                    [(tr("From value"), &mut fx.low), (tr("To value"), &mut fx.high), (tr("Spread"), &mut fx.spread)]
                {
                    let label = ui.label(key);
                    ui.add(egui::Slider::new(value, 0.0..=1.0)).labelled_by(label.id);
                }
            });
            // More attributes on the same wave, each later in the cycle (a circle, a rainbow).
            let mut drop = None;
            for (k, part) in fx.also.iter_mut().enumerate() {
                let which = actcraft_engine::fill(tr("also {n}"), &[("n", &(k + 1).to_string())]);
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label(actcraft_engine::fill(tr("Also moves ({n})"), &[("n", &(k + 1).to_string())]));
                    egui::ComboBox::from_id_salt(("effect-also", k))
                        .selected_text(crate::fixtures_view::attribute_text(part.attribute))
                        .show_ui(ui, |ui| {
                            for a in ATTRIBUTES.into_iter().filter(|a| !a.is_fine()) {
                                ui.selectable_value(&mut part.attribute, a, crate::fixtures_view::attribute_text(a));
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    for (key, value) in [
                        (tr("From value"), &mut part.low),
                        (tr("To value"), &mut part.high),
                        (tr("Later in the cycle"), &mut part.offset),
                    ] {
                        ui.label(key);
                        let r = ui.add(egui::Slider::new(value, 0.0..=1.0));
                        let name = format!("{key} ({which})");
                        ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(name));
                    }
                    let r = ui.small_button(tr("Remove"));
                    let name = format!("{} ({which})", tr("Remove"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    if r.clicked() {
                        drop = Some(k);
                    }
                });
            }
            if let Some(k) = drop {
                fx.also.remove(k);
            }
            let room = fx.also.len() < actcraft_show::limits::MAX_EFFECT_PARTS;
            if ui
                .add_enabled(room, egui::Button::new(tr("Move another attribute too")))
                .on_hover_text(tr("Pan and tilt make a circle; red, green and blue a rainbow"))
                .clicked()
            {
                fx.also.push(actcraft_engine::effects::EffectPart { offset: 0.25, ..Default::default() });
            }
            let also: Vec<Value> = fx
                .also
                .iter()
                .map(|p| {
                    json!({"attribute": attribute_key(p.attribute), "low": p.low, "high": p.high, "offset": p.offset})
                })
                .collect();
            if ui.add_enabled(!fx.name.trim().is_empty(), egui::Button::new(tr("Save effect"))).clicked() {
                run = Some((
                    "effect.set",
                    json!({"name": fx.name.trim(), "group": d.effect_group, "attribute": attribute_key(fx.attribute),
                        "shape": shape_key(fx.shape), "period": fx.period, "beats": fx.beats,
                        "low": fx.low, "high": fx.high, "spread": fx.spread, "also": also}),
                ));
            }
        }
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

#[cfg(test)]
mod tests {
    #[test]
    fn every_ready_made_effect_has_a_name_in_the_window() {
        for t in actcraft_engine::effects::TEMPLATES {
            assert_ne!(super::template_text(t.key), super::tr("Effect"), "{} has no name", t.key);
            assert_eq!(super::template_text(t.key), t.name, "{}: the window says what the engine says", t.key);
        }
        assert!(actcraft_engine::effects::TEMPLATES.len() >= 70);
    }
}
