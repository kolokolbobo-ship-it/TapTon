//! Central visual identity of TapTone: DARK x NEON GREEN.
//!
//! All colors live here. `apply` installs the palette into egui, and the helpers below draw
//! buttons, cards and icons that match it. Icons are painted with shapes (not font glyphs), so
//! they look the same on every machine and never turn into "tofu" boxes.

use eframe::egui::{
    self, epaint::Shadow, vec2, Align2, Color32, CursorIcon, FontFamily, FontId, Margin, Pos2, Rect, Response,
    RichText, Rounding, Sense, Shape, Stroke, TextStyle, Ui,
};

// ---- Palette ---------------------------------------------------------------------------------

/// Window / page background `#090B0A`.
pub const BG: Color32 = Color32::from_rgb(0x09, 0x0B, 0x0A);
/// Cards and panels `#121713`.
pub const CARD: Color32 = Color32::from_rgb(0x12, 0x17, 0x13);
/// Card under the pointer.
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x19, 0x20, 0x1B);
/// Sidebar background, a touch darker than the cards.
pub const SIDEBAR: Color32 = Color32::from_rgb(0x0C, 0x10, 0x0D);
/// Input fields and slider rails.
pub const INPUT_BG: Color32 = Color32::from_rgb(0x0D, 0x11, 0x0E);
/// Neon accent `#39FF78`.
pub const ACCENT: Color32 = Color32::from_rgb(0x39, 0xFF, 0x78);
/// Accent on hover (slightly lighter).
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x7A, 0xFF, 0xA3);
/// Muted accent for secondary strokes.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1F, 0xA8, 0x50);
/// Dark green tint used behind selected / hovered accent elements.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x10, 0x2B, 0x1A);
/// Borders `#253128`.
pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x31, 0x28);
/// Main text `#F0F5F0`.
pub const TEXT: Color32 = Color32::from_rgb(0xF0, 0xF5, 0xF0);
/// Secondary text.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8C, 0x9B, 0x90);
/// Disabled text and icons.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x4A, 0x57, 0x4E);
/// Errors and destructive actions.
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x6C);
/// Warnings.
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x57);

// ---- egui style ------------------------------------------------------------------------------

/// Install the TapTone look into an egui context. Call once at start-up.
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = INPUT_BG;
    v.faint_bg_color = CARD;
    v.code_bg_color = CARD_HOVER;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_rounding = Rounding::same(14.0);
    v.menu_rounding = Rounding::same(10.0);
    v.window_shadow = Shadow {
        offset: vec2(0.0, 10.0),
        blur: 28.0,
        spread: 0.0,
        color: Color32::from_black_alpha(190),
    };
    v.popup_shadow = Shadow {
        offset: vec2(0.0, 6.0),
        blur: 18.0,
        spread: 0.0,
        color: Color32::from_black_alpha(160),
    };
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;

    let r = Rounding::same(9.0);

    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = INPUT_BG;
    v.widgets.inactive.weak_bg_fill = CARD_HOVER;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = ACCENT_FILL;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, ACCENT);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = ACCENT_FILL;
    v.widgets.active.weak_bg_fill = ACCENT_FILL;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(2.0_f32, ACCENT);
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = CARD_HOVER;
    v.widgets.open.weak_bg_fill = CARD_HOVER;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.open.rounding = r;

    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(10.0, 10.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.window_margin = Margin::same(18.0);
        s.spacing.slider_width = 160.0;
        s.spacing.combo_width = 220.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

// ---- Frames ----------------------------------------------------------------------------------

/// A rounded card with a thin border.
pub fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(14.0, 10.0))
}

/// Smaller section title used inside the settings page.
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).strong().color(ACCENT));
}

// ---- Text buttons ----------------------------------------------------------------------------

/// Filled neon-green call-to-action button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(BG).strong())
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(9.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Outlined neutral button.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(TEXT))
            .fill(CARD_HOVER)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .rounding(Rounding::same(9.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Red-tinted button for destructive actions.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(DANGER).strong())
            .fill(Color32::from_rgb(0x2A, 0x12, 0x15))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x5A, 0x24, 0x2B)))
            .rounding(Rounding::same(9.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Pill-shaped label that can be clicked (used for hotkeys).
pub fn chip(ui: &mut Ui, text: &str, color: Color32, filled: bool) -> Response {
    let fill = if filled { ACCENT_FILL } else { Color32::TRANSPARENT };
    let border = if filled { color } else { BORDER };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, border))
            .rounding(Rounding::same(20.0))
            .min_size(vec2(0.0, 26.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

// ---- Icons -----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Stop,
    Star,
    StarOutline,
    Library,
    History,
    Settings,
    Volume,
    Quit,
}

fn star_points(c: Pos2, outer: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let radius = if i % 2 == 0 { outer } else { outer * 0.42 };
            Pos2::new(c.x + radius * angle.cos(), c.y + radius * angle.sin())
        })
        .collect()
}

/// Paint an icon centered at `c`; `s` is the nominal icon size in points.
pub fn paint_icon(p: &egui::Painter, icon: Icon, c: Pos2, s: f32, color: Color32) {
    let stroke = Stroke::new((s * 0.09).max(1.4), color);
    match icon {
        Icon::Play => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.26, c.y - s * 0.38),
                    Pos2::new(c.x - s * 0.26, c.y + s * 0.38),
                    Pos2::new(c.x + s * 0.40, c.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            let r = Rect::from_center_size(c, vec2(s * 0.58, s * 0.58));
            p.rect_filled(r, Rounding::same(s * 0.08), color);
        }
        Icon::Star | Icon::StarOutline => {
            let pts = star_points(c, s * 0.5);
            if icon == Icon::Star {
                for i in 0..10 {
                    p.add(Shape::convex_polygon(
                        vec![c, pts[i], pts[(i + 1) % 10]],
                        color,
                        Stroke::NONE,
                    ));
                }
            }
            p.add(Shape::closed_line(pts, stroke));
        }
        Icon::Library => {
            let cell = s * 0.34;
            let gap = s * 0.10;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let center = Pos2::new(c.x + dx * (cell + gap) / 2.0, c.y + dy * (cell + gap) / 2.0);
                p.rect_filled(
                    Rect::from_center_size(center, vec2(cell, cell)),
                    Rounding::same(s * 0.07),
                    color,
                );
            }
        }
        Icon::History => {
            p.circle_stroke(c, s * 0.42, stroke);
            p.add(Shape::line_segment([c, Pos2::new(c.x, c.y - s * 0.26)], stroke));
            p.add(Shape::line_segment(
                [c, Pos2::new(c.x + s * 0.20, c.y + s * 0.10)],
                stroke,
            ));
        }
        Icon::Settings => {
            let half = s * 0.42;
            for (dy, knob) in [(-0.27, -0.18), (0.0, 0.20), (0.27, -0.05)] {
                let y = c.y + dy * s;
                p.add(Shape::line_segment(
                    [Pos2::new(c.x - half, y), Pos2::new(c.x + half, y)],
                    stroke,
                ));
                p.circle_filled(Pos2::new(c.x + knob * s, y), s * 0.09, color);
            }
        }
        Icon::Volume => {
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(c.x - s * 0.40, c.y - s * 0.15),
                    Pos2::new(c.x - s * 0.14, c.y + s * 0.15),
                ),
                Rounding::same(s * 0.04),
                color,
            );
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.14, c.y - s * 0.15),
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.38),
                    Pos2::new(c.x + s * 0.10, c.y + s * 0.38),
                    Pos2::new(c.x - s * 0.14, c.y + s * 0.15),
                ],
                color,
                Stroke::NONE,
            ));
            let arc: Vec<Pos2> = (-4..=4)
                .map(|i| {
                    let a = i as f32 * 0.14;
                    Pos2::new(c.x + s * 0.12 + s * 0.26 * a.cos(), c.y + s * 0.26 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, stroke));
        }
        Icon::Quit => {
            p.circle_stroke(c, s * 0.38, stroke);
            p.add(Shape::line_segment(
                [Pos2::new(c.x, c.y - s * 0.46), Pos2::new(c.x, c.y - s * 0.08)],
                Stroke::new(stroke.width + 0.6, color),
            ));
        }
    }
}

/// Flat, square icon button. `active` tints it with the accent color.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size + 10.0, size + 10.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        if hovered {
            ui.painter().rect_filled(rect, Rounding::same(8.0), CARD_HOVER);
        }
        let color = if active {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
        };
        paint_icon(ui.painter(), icon, rect.center(), size, color);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Round button with an icon. `primary` = solid accent circle, otherwise outlined.
pub fn round_button(ui: &mut Ui, icon: Icon, diameter: f32, primary: bool, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(diameter, diameter), sense);
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered() && enabled;
        let (bg, fg, stroke) = if !enabled {
            (CARD_HOVER, TEXT_FAINT, Stroke::new(1.0_f32, BORDER))
        } else if primary {
            (if hovered { ACCENT_HOVER } else { ACCENT }, BG, Stroke::NONE)
        } else {
            (
                if hovered { ACCENT_FILL } else { CARD_HOVER },
                ACCENT,
                Stroke::new(1.0_f32, if hovered { ACCENT } else { BORDER }),
            )
        };
        ui.painter().circle(rect.center(), diameter / 2.0, bg, stroke);
        paint_icon(ui.painter(), icon, rect.center(), diameter * 0.5, fg);
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Sidebar navigation row.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(10.0), ACCENT_FILL);
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + 9.0),
                    Pos2::new(rect.left() + 3.0, rect.bottom() - 9.0),
                ),
                Rounding::same(2.0),
                ACCENT,
            );
        } else if hovered {
            p.rect_filled(rect, Rounding::same(10.0), CARD);
        }
        let color = if selected {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
        };
        paint_icon(p, icon, Pos2::new(rect.left() + 24.0, rect.center().y), 20.0, color);
        p.text(
            Pos2::new(rect.left() + 48.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(15.0, FontFamily::Proportional),
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ---- Formatting helpers ----------------------------------------------------------------------

/// `m:ss` for a number of seconds.
pub fn fmt_time(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 {
        secs.round() as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// Rough "5 min ago" text for a unix timestamp.
pub fn fmt_ago(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=4 => "just now".to_string(),
        5..=59 => format!("{d} s ago"),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86400),
    }
}
