//! egui's painted shapes as SVG: lines, paths, circles, ellipses, rectangles, curves and text,
//! for a still picture of something the window draws (the browser view's scores). Meshes and
//! custom callbacks are left out; text is written as text in the window's font family.

use std::fmt::Write as _;

use egui::epaint::{ClippedShape, ColorMode, PathStroke, Shape, Stroke};
use egui::{Color32, Pos2, Rect};

/// Largest SVG written; shapes beyond it are left out.
const MAX_BYTES: usize = 8 << 20;

fn colour(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0) }
}

fn fill(c: Color32) -> String {
    if c.a() == 0 { "none".to_owned() } else { colour(c) }
}

fn stroke(s: Stroke) -> String {
    if s.width <= 0.0 || s.color.a() == 0 {
        "stroke=\"none\"".to_owned()
    } else {
        format!("stroke=\"{}\" stroke-width=\"{:.2}\"", colour(s.color), s.width)
    }
}

fn path_stroke(s: &PathStroke) -> String {
    match s.color {
        ColorMode::Solid(c) => stroke(Stroke::new(s.width, c)),
        _ => "stroke=\"none\"".to_owned(),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn points(ps: &[Pos2]) -> String {
    ps.iter().map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect::<Vec<_>>().join(" ")
}

fn shape(out: &mut String, s: &Shape) {
    if out.len() > MAX_BYTES {
        return;
    }
    let _ = match s {
        Shape::Vec(list) => {
            for s in list {
                shape(out, s);
            }
            Ok(())
        }
        Shape::LineSegment { points: [a, b], stroke: st } => writeln!(
            out,
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" {}/>",
            a.x,
            a.y,
            b.x,
            b.y,
            stroke(*st)
        ),
        Shape::Circle(c) => writeln!(
            out,
            "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.2}\" fill=\"{}\" {}/>",
            c.center.x,
            c.center.y,
            c.radius,
            fill(c.fill),
            stroke(c.stroke)
        ),
        Shape::Ellipse(e) => writeln!(
            out,
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.2}\" ry=\"{:.2}\" transform=\"rotate({:.2} {:.1} {:.1})\" fill=\"{}\" {}/>",
            e.center.x,
            e.center.y,
            e.radius.x,
            e.radius.y,
            e.angle.to_degrees(),
            e.center.x,
            e.center.y,
            fill(e.fill),
            stroke(e.stroke)
        ),
        Shape::Path(p) if p.points.len() >= 2 => {
            let tag = if p.closed { "polygon" } else { "polyline" };
            let f = if p.closed { fill(p.fill) } else { "none".to_owned() };
            writeln!(out, "<{tag} points=\"{}\" fill=\"{f}\" {}/>", points(&p.points), path_stroke(&p.stroke))
        }
        Shape::Rect(r) => {
            let radius = f32::from(r.corner_radius.nw);
            writeln!(
                out,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{radius:.1}\" fill=\"{}\" {}/>",
                r.rect.min.x,
                r.rect.min.y,
                r.rect.width(),
                r.rect.height(),
                fill(r.fill),
                stroke(r.stroke)
            )
        }
        Shape::QuadraticBezier(q) => {
            let [a, b, c] = q.points;
            let f = if q.closed { fill(q.fill) } else { "none".to_owned() };
            writeln!(
                out,
                "<path d=\"M{:.1},{:.1} Q{:.1},{:.1} {:.1},{:.1}\" fill=\"{f}\" {}/>",
                a.x,
                a.y,
                b.x,
                b.y,
                c.x,
                c.y,
                path_stroke(&q.stroke)
            )
        }
        Shape::CubicBezier(cb) => {
            let [a, b, c, d] = cb.points;
            let f = if cb.closed { fill(cb.fill) } else { "none".to_owned() };
            writeln!(
                out,
                "<path d=\"M{:.1},{:.1} C{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{f}\" {}/>",
                a.x,
                a.y,
                b.x,
                b.y,
                c.x,
                c.y,
                d.x,
                d.y,
                path_stroke(&cb.stroke)
            )
        }
        Shape::Text(text) => {
            let format = text.galley.job.sections.first().map(|s| s.format.clone()).unwrap_or_default();
            let colour_of = text.override_text_color.unwrap_or(if format.color == Color32::PLACEHOLDER {
                text.fallback_color
            } else {
                format.color
            });
            let family = if format.font_id.family == egui::FontFamily::Monospace { "monospace" } else { "sans-serif" };
            for row in &text.galley.rows {
                for g in &row.glyphs {
                    if g.chr.is_whitespace() {
                        continue;
                    }
                    let x = text.pos.x + row.pos.x + g.pos.x;
                    let y = text.pos.y + row.pos.y + g.pos.y;
                    let _ = writeln!(
                        out,
                        "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{:.1}\" font-family=\"{family}\" fill=\"{}\">{}</text>",
                        format.font_id.size,
                        colour(colour_of),
                        escape(&g.chr.to_string())
                    );
                }
            }
            Ok(())
        }
        _ => Ok(()),
    };
}

/// The shapes as one SVG document over `screen`.
pub fn shapes_to_svg(shapes: &[ClippedShape], screen: Rect) -> String {
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.0} {:.0} {:.0} {:.0}\" width=\"{:.0}\" height=\"{:.0}\">\n",
        screen.min.x,
        screen.min.y,
        screen.width(),
        screen.height(),
        screen.width(),
        screen.height()
    );
    for clipped in shapes {
        let t = clipped.transform_after_tessellation;
        if t == egui::emath::TSTransform::IDENTITY {
            shape(&mut out, &clipped.shape);
        } else {
            let _ = writeln!(
                out,
                "<g transform=\"matrix({s} 0 0 {s} {:.1} {:.1})\">",
                t.translation.x,
                t.translation.y,
                s = t.scaling
            );
            shape(&mut out, &clipped.shape);
            out += "</g>
";
        }
    }
    out += "</svg>\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clipped(shape: Shape) -> ClippedShape {
        ClippedShape {
            clip_rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 50.0)),
            shape,
            transform_after_tessellation: egui::emath::TSTransform::IDENTITY,
        }
    }

    #[test]
    fn shapes_become_svg_elements() {
        let clip = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 50.0));
        let mut moved = clipped(Shape::circle_filled(Pos2::ZERO, 1.0, Color32::BLACK));
        moved.transform_after_tessellation = egui::emath::TSTransform::from_translation(egui::vec2(3.0, 4.0));
        let shapes = vec![
            clipped(Shape::line_segment([Pos2::new(0.0, 1.0), Pos2::new(10.0, 1.0)], Stroke::new(1.0, Color32::BLACK))),
            clipped(Shape::circle_filled(Pos2::new(5.0, 5.0), 2.0, Color32::RED)),
            clipped(Shape::convex_polygon(
                vec![Pos2::ZERO, Pos2::new(4.0, 0.0), Pos2::new(2.0, 3.0)],
                Color32::BLACK,
                Stroke::NONE,
            )),
            moved,
        ];
        assert!(shapes_to_svg(&shapes, clip).contains("<g transform=\"matrix(1 0 0 1 3.0 4.0)\">"));
        let svg = shapes_to_svg(&shapes, clip);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<line x1=\"0.0\" y1=\"1.0\" x2=\"10.0\""));
        assert!(svg.contains("<circle cx=\"5.0\" cy=\"5.0\" r=\"2.00\" fill=\"#ff0000\""));
        assert!(svg.contains("<polygon points=\"0.0,0.0 4.0,0.0 2.0,3.0\" fill=\"#000000\""));
        assert_eq!(escape("a<&>"), "a&lt;&amp;&gt;");
    }
}
