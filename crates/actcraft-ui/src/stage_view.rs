//! The Stage window (Lighting > Stage): the fixtures from above, each in the colour it shows now,
//! placed by dragging or by numbers.

use actcraft_engine::Engine;
use actcraft_engine::fixtures::shown_colour;
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::ActCraftApp;
use crate::i18n::tr;

/// What the Stage window has picked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageView {
    /// The fixture whose place the number fields set.
    pub picked: Option<usize>,
}

/// Where a fixture is drawn (0-1 on both axes): its place, or a row along the middle until placed.
fn spot(place: Option<[f64; 2]>, unplaced_before: usize, unplaced: usize) -> [f64; 2] {
    place.unwrap_or([(unplaced_before as f64 + 0.5) / unplaced.max(1) as f64, 0.5])
}

impl ActCraftApp {
    /// The Stage window, while it is open.
    pub(crate) fn stage_view_window(&mut self, ctx: &egui::Context, e: &mut Engine) {
        let Some(mut view) = self.stage_view else { return };
        let mut open = true;
        egui::Window::new(tr("Stage"))
            .open(&mut open)
            .default_width(760.0)
            .collapsible(false)
            .show(ctx, |ui| self.stage_ui(ui, e, &mut view));
        self.stage_view = Some(view);
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stage_view = None;
        }
    }

    fn stage_ui(&mut self, ui: &mut Ui, e: &mut Engine, view: &mut StageView) {
        let t = self.tokens;
        let fixtures = e.show.fixtures.clone();
        if fixtures.is_empty() {
            ui.label(RichText::new(tr("Patch fixtures first (Lighting > Fixtures).")).color(t.text_dim));
            return;
        }
        ui.label(
            RichText::new(tr(
                "Seen from above, the audience at the bottom. Drag a fixture, or pick it and set its place.",
            ))
            .color(t.text_dim),
        );
        let width = ui.available_width().max(200.0);
        let (stage, _) = ui.allocate_exact_size(vec2(width, (width * 0.5).min(420.0)), Sense::hover());
        let painter = ui.painter_at(stage);
        painter.rect_filled(stage, 4.0, t.surface);
        painter.rect_stroke(stage, 4.0, Stroke::new(1.0, t.stroke), StrokeKind::Inside);
        painter.text(
            stage.center_bottom() - vec2(0.0, 6.0),
            Align2::CENTER_BOTTOM,
            tr("Audience"),
            FontId::proportional(12.0),
            t.text_dim,
        );
        let unplaced = fixtures.iter().filter(|f| f.place.is_none()).count();
        let mut unplaced_before = 0;
        let mut moved = None;
        for (i, f) in fixtures.iter().enumerate() {
            let [x, y] = spot(f.place, unplaced_before, unplaced);
            if f.place.is_none() {
                unplaced_before += 1;
            }
            let centre = Pos2::new(
                stage.left() + 20.0 + (stage.width() - 40.0) * x as f32,
                stage.top() + 16.0 + (stage.height() - 44.0) * y as f32,
            );
            let [r, g, b] = shown_colour(e, f);
            let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            let fill = Color32::from_rgb(byte(r), byte(g), byte(b));
            let hit = Rect::from_center_size(centre, vec2(30.0, 30.0));
            let resp = ui.interact(hit, egui::Id::unique(("stage-fixture", f.id)), Sense::click_and_drag());
            let name = tr("{name}: red {r} %, green {g} %, blue {b} %")
                .replace("{name}", &f.name)
                .replace("{r}", &format!("{:.0}", r * 100.0))
                .replace("{g}", &format!("{:.0}", g * 100.0))
                .replace("{b}", &format!("{:.0}", b * 100.0));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            let picked = view.picked == Some(i);
            painter.circle_filled(centre, 12.0, fill);
            let ring = if picked || resp.has_focus() { Stroke::new(3.0, t.focus) } else { Stroke::new(1.0, t.stroke) };
            painter.circle_stroke(centre, 12.0, ring);
            painter.text(centre + vec2(0.0, 15.0), Align2::CENTER_TOP, &f.name, FontId::proportional(11.0), t.text);
            if resp.clicked() || resp.drag_started() {
                view.picked = Some(i);
            }
            if resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let nx = ((p.x - stage.left() - 20.0) / (stage.width() - 40.0)).clamp(0.0, 1.0);
                let ny = ((p.y - stage.top() - 16.0) / (stage.height() - 44.0)).clamp(0.0, 1.0);
                moved = Some((i, f64::from(nx), f64::from(ny)));
            }
            resp.on_hover_text(&name);
        }
        // Placing by numbers: the keyboard way, and exact.
        if let Some(i) = view.picked.filter(|i| *i < fixtures.len())
            && let Some(f) = fixtures.get(i)
        {
            let [mut x, mut y] = f.place.unwrap_or([0.5, 0.5]);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&f.name).strong());
                let label = ui.label(tr("Left to right"));
                let a =
                    ui.add(egui::DragValue::new(&mut x).range(0.0..=1.0).speed(0.01)).labelled_by(label.id).changed();
                let label = ui.label(tr("Back to front"));
                let b =
                    ui.add(egui::DragValue::new(&mut y).range(0.0..=1.0).speed(0.01)).labelled_by(label.id).changed();
                if a || b {
                    moved = Some((i, x, y));
                }
            });
        }
        if let Some((i, x, y)) = moved {
            self.run(e, "fixture.place", json!({"fixture": i, "x": x, "y": y}));
        }
    }
}
