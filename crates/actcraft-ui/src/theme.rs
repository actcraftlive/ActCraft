//! Theme tokens. Every colour, size and spacing the window uses comes from here, so a theme
//! change reaches every panel. The default is a dark theme for dark stages.
//!
//! Text colours keep at least 4.5:1 contrast against their backgrounds, controls and focus at
//! least 3:1 (WCAG 2.2 AA).

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// Window background.
    pub bg: Color32,
    /// Panels and sidebars.
    pub panel: Color32,
    /// Raised surfaces: track strips, list rows.
    pub surface: Color32,
    pub surface_hover: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub playing: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub stroke: Color32,
    pub focus: Color32,
    pub radius: u8,
    pub gap: f32,
    pub pad: f32,
    pub strip_width: f32,
    pub sidebar_width: f32,
    pub text_size: f32,
    pub big_text_size: f32,
}

/// The dark stage theme.
pub const DARK: Tokens = Tokens {
    bg: Color32::from_rgb(0x12, 0x13, 0x16),
    panel: Color32::from_rgb(0x1a, 0x1c, 0x21),
    surface: Color32::from_rgb(0x24, 0x27, 0x2e),
    surface_hover: Color32::from_rgb(0x2e, 0x32, 0x3b),
    text: Color32::from_rgb(0xe8, 0xea, 0xee),
    text_dim: Color32::from_rgb(0xa4, 0xab, 0xb8),
    accent: Color32::from_rgb(0x4f, 0x9c, 0xf0),
    accent_text: Color32::from_rgb(0x0b, 0x0c, 0x0e),
    playing: Color32::from_rgb(0x3d, 0xc9, 0x7a),
    danger: Color32::from_rgb(0xf0, 0x5d, 0x5d),
    warning: Color32::from_rgb(0xf2, 0xb4, 0x3c),
    stroke: Color32::from_rgb(0x5a, 0x61, 0x6e),
    focus: Color32::from_rgb(0x8c, 0xc4, 0xff),
    radius: 6,
    gap: 8.0,
    pad: 10.0,
    strip_width: 96.0,
    sidebar_width: 300.0,
    text_size: 14.0,
    big_text_size: 28.0,
};

/// Applies the tokens to egui's style.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let mut v = Visuals::dark();
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.bg;
    v.faint_bg_color = t.surface;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent.gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, t.focus);
    let r = CornerRadius::same(t.radius);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.stroke);
    // `bg_fill` draws slider rails and check boxes: it must stand out 3:1 from the panels.
    v.widgets.inactive.bg_fill = t.stroke;
    v.widgets.inactive.weak_bg_fill = t.surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.stroke);
    v.widgets.hovered.bg_fill = t.surface_hover;
    v.widgets.hovered.weak_bg_fill = t.surface_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, t.focus);
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.weak_bg_fill = t.accent;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.gap, t.gap * 0.75);
        s.spacing.button_padding = egui::vec2(t.pad, t.pad * 0.5);
        s.spacing.interact_size.y = 28.0;
        // Vertical faders get a usable length.
        s.spacing.slider_width = 150.0;
        for (style, font) in s.text_styles.iter_mut() {
            font.size = match style {
                egui::TextStyle::Heading => t.text_size * 1.4,
                egui::TextStyle::Small => t.text_size * 0.8,
                _ => t.text_size,
            };
        }
    });
}

/// `#rrggbb` to a colour, or `fallback`.
pub fn colour(hex: &str, fallback: Color32) -> Color32 {
    actcraft_show::parse_colour(hex).map_or(fallback, |[r, g, b]| Color32::from_rgb(r, g, b))
}
