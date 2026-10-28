//! Design tokens (colour roles, type scale, spacing) and egui style setup.
//!
//! Colours come from the user's `.budgetrc` palette. A colour that still has its stock value
//! (light or dark default) is replaced by the modern tone of the same role; a colour the user
//! changed is honoured as-is, with a readable foreground derived from its luminance.

use budget_core::config::{Colors, Theme};
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, StrokeKind, TextStyle, Vec2};
use std::sync::Arc;

/// 8-px spacing scale.
pub const SP1: f32 = 4.0;
pub const SP2: f32 = 8.0;
pub const SP3: f32 = 16.0;
pub const SP4: f32 = 24.0;
pub const RADIUS: u8 = 10;

/// Named text styles beyond egui's built-ins (see `apply`).
pub const TITLE: &str = "title";
pub const SECTION: &str = "section";
pub const CAPTION: &str = "caption";
pub const VALUE: &str = "value";
pub const DAY: &str = "day";
pub const SEMI: &str = "semi";
pub const ICON: &str = "icon";

pub fn ts(name: &'static str) -> TextStyle { TextStyle::Name(name.into()) }
pub fn font(ui: &egui::Ui, name: &'static str) -> FontId { ts(name).resolve(ui.style()) }
pub fn rgb(c: u32) -> Color32 { Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8) }
pub fn alpha(c: Color32, a: u8) -> Color32 { Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a) }
/// Opaque tint of `c` over `bg`. egui premultiplies translucent colours in linear space, so a
/// low-alpha fill renders far stronger than intended; blending in gamma space here is predictable.
pub fn tint(bg: Color32, c: Color32, k: f32) -> Color32 { bg.lerp_to_gamma(c, k) }

fn luminance(c: Color32) -> f32 {
    let f = |v: u8| { let v = v as f32 / 255.0; if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}
/// Readable text colour on an arbitrary background.
pub fn on(bg: Color32) -> Color32 {
    let dark = rgb(0x172033);
    let l = luminance(bg);
    if (l + 0.05) / (luminance(dark) + 0.05) > 1.05 / (l + 0.05) { dark } else { Color32::WHITE }
}

/// Darkens (light theme) or lightens (dark theme) a fill for hover feedback.
pub fn hover_shade(c: Color32, dark: bool) -> Color32 {
    if dark { c.lerp_to_gamma(Color32::WHITE, 0.12) } else { c.lerp_to_gamma(Color32::BLACK, 0.08) }
}

/// A keyboard-focus ring (2px accent) that every helper button paints, so focus is always visible.
fn focus_ring(ui: &egui::Ui, resp: &egui::Response, t: &Tokens) {
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect.expand(2.0), 9, Stroke::new(2.0_f32, t.accent), StrokeKind::Outside);
    }
}

/// A filled button whose fill brightens on hover/press. The fill is set through the widget visuals
/// (not `Button::fill`, which would pin one colour for every state).
fn filled_button(ui: &mut egui::Ui, text: egui::RichText, fill: Color32, t: &Tokens) -> egui::Response {
    let resp = ui.scope(|ui| {
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.weak_bg_fill = fill;
        w.hovered.weak_bg_fill = hover_shade(fill, t.dark);
        w.active.weak_bg_fill = hover_shade(hover_shade(fill, t.dark), t.dark);
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::NONE;
        w.active.bg_stroke = Stroke::NONE;
        let resp = ui.add(egui::Button::new(text).corner_radius(7).min_size(Vec2::new(0.0, 36.0)));
        // 1px lighter top edge gives the primary action a subtle raised look.
        let top = resp.rect.shrink2(Vec2::new(7.0, 0.0));
        ui.painter().hline(top.x_range(), resp.rect.min.y + 0.5, Stroke::new(1.0_f32, alpha(Color32::WHITE, if t.dark { 26 } else { 46 })));
        resp
    }).inner;
    focus_ring(ui, &resp, t);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    filled_button(ui, egui::RichText::new(label.into()).color(on(t.accent)).text_style(ts(SEMI)), t.accent, t)
}

pub fn secondary_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    let enabled = ui.is_enabled();
    // Disabled keeps readable text (muted, not faint) so a greyed "Save changes" is still legible.
    let color = if enabled { t.text } else { t.text_muted };
    let resp = ui.add(egui::Button::new(egui::RichText::new(label.into()).color(color))
        .fill(t.surface).stroke(Stroke::new(1.0_f32, t.border_strong)).corner_radius(7).min_size(Vec2::new(0.0, 36.0)));
    focus_ring(ui, &resp, t);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn danger_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    filled_button(ui, egui::RichText::new(label.into()).color(on(t.negative)).text_style(ts(SEMI)), t.negative, t)
}

/// A neutral pill (used for the version chip and similar small labels).
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32, t: &Tokens) {
    egui::Frame::new().fill(tint(t.surface, color, if t.dark { 0.18 } else { 0.07 }))
        .corner_radius(5).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.label(egui::RichText::new(text).text_style(ts(CAPTION)).color(color));
        });
}

/// A pill with a leading status dot (e.g. saved / unsaved).
pub fn status_pill(ui: &mut egui::Ui, text: &str, dot: Color32, filled: bool, t: &Tokens) -> egui::Response {
    let fill = if filled { tint(t.surface, dot, if t.dark { 0.22 } else { 0.14 }) } else { tint(t.surface, t.text_muted, 0.08) };
    egui::Frame::new().fill(fill).corner_radius(10).inner_margin(egui::Margin::symmetric(9, 3)).show(ui, |ui| {
        ui.spacing_mut().interact_size.y = 14.0; // keep the pill compact (default row is 34px)
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            ui.label(egui::RichText::new(text).text_style(ts(CAPTION)).color(t.text));
        });
    }).response
}

/// A small keycap chip ("Ctrl", "S") used for shortcut hints. Returns its response.
pub fn keycap(ui: &mut egui::Ui, key: &str, t: &Tokens) -> egui::Response {
    egui::Frame::new().fill(t.surface_hover).stroke(Stroke::new(1.0_f32, t.border_strong)).corner_radius(4)
        .inner_margin(egui::Margin::symmetric(5, 0)).show(ui, |ui| {
            ui.label(egui::RichText::new(key).text_style(ts(CAPTION)).color(t.text_muted));
        }).response
}


/// WCAG relative-luminance contrast ratio between two colours.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[derive(Clone)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub weekend: Color32,
    pub past: Color32,
    pub other_month: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub today: Color32,
    pub positive: Color32,
    pub negative: Color32,
    pub disabled: Color32,
    pub paid: Color32,
    pub important: Color32,
    /// Balance strips: (background, foreground).
    pub bal: (Color32, Color32),
    pub max: (Color32, Color32),
    pub min: (Color32, Color32),
    pub low: (Color32, Color32),
}

impl Tokens {
    pub fn new(c: &Colors, dark: bool) -> Tokens {
        let dl = Colors::theme(Theme::Light);
        let dd = Colors::theme(Theme::Dark);
        // user colour if customised, else the modern tone for the role
        let pick = |user: u32, l: u32, d: u32, modern: u32| if user == l || user == d { rgb(modern) } else { rgb(user) };
        let strip = |user: u32, l: u32, d: u32, bg: u32, fg: u32| {
            if user == l || user == d { (rgb(bg), rgb(fg)) } else { let b = rgb(user); (b, on(b)) }
        };
        if dark {
            let surface = pick(c.bgColorCell, dl.bgColorCell, dd.bgColorCell, 0x1b1f27);
            Tokens {
                dark,
                bg: pick(c.bgColorWindow, dl.bgColorWindow, dd.bgColorWindow, 0x111722),
                surface,
                surface_hover: rgb(0x242935),
                weekend: rgb(0x1d222b),
                past: pick(c.bgColorCellPast, dl.bgColorCellPast, dd.bgColorCellPast, 0x1a1f27),
                other_month: pick(c.bgColorCellOtherMonths, dl.bgColorCellOtherMonths, dd.bgColorCellOtherMonths, 0x0f131b),
                border: rgb(0x2b313d),
                border_strong: rgb(0x3b4350),
                text: pick(c.colorDescText, dl.colorDescText, dd.colorDescText, 0xe6e8eb),
                text_muted: rgb(0xa0aec0),
                text_faint: rgb(0x8b97aa),
                accent: pick(c.colorCurrentMonth, dl.colorCurrentMonth, dd.colorCurrentMonth, 0x60a5fa),
                today: pick(c.colorCalendarToday, dl.colorCalendarToday, dd.colorCalendarToday, 0x60a5fa),
                positive: pick(c.colorPositiveAmount, dl.colorPositiveAmount, dd.colorPositiveAmount, 0x4ade80),
                negative: pick(c.colorNegativeAmount, dl.colorNegativeAmount, dd.colorNegativeAmount, 0xf87171),
                disabled: pick(c.colorDisabledPayment, dl.colorDisabledPayment, dd.colorDisabledPayment, 0x7d8799),
                paid: pick(c.bgColorPaid, dl.bgColorPaid, dd.bgColorPaid, 0x34d399),
                important: pick(c.bgColorImportant, dl.bgColorImportant, dd.bgColorImportant, 0xfbbf24),
                bal: strip(c.bgColorBalance, dl.bgColorBalance, dd.bgColorBalance, 0x232833, 0xc9ced6),
                max: strip(c.bgColorMaxBalance, dl.bgColorMaxBalance, dd.bgColorMaxBalance, 0x4a1d3a, 0xf9a8d4),
                min: strip(c.bgColorMinBalance, dl.bgColorMinBalance, dd.bgColorMinBalance, 0x1e3a5f, 0x93c5fd),
                low: strip(c.bgColorBelowMinBalance, dl.bgColorBelowMinBalance, dd.bgColorBelowMinBalance, 0x4a3410, 0xfcd34d),
            }
        } else {
            let surface = pick(c.bgColorCell, dl.bgColorCell, dd.bgColorCell, 0xffffff);
            Tokens {
                dark,
                bg: pick(c.bgColorWindow, dl.bgColorWindow, dd.bgColorWindow, 0xf4f6fa),
                surface,
                surface_hover: rgb(0xf0f3f8),
                weekend: rgb(0xf6f8fb),
                past: pick(c.bgColorCellPast, dl.bgColorCellPast, dd.bgColorCellPast, 0xf8f9fc),
                other_month: pick(c.bgColorCellOtherMonths, dl.bgColorCellOtherMonths, dd.bgColorCellOtherMonths, 0xeef1f6),
                border: rgb(0xe2e8f0),
                border_strong: rgb(0xcbd5e1),
                text: pick(c.colorDescText, dl.colorDescText, dd.colorDescText, 0x1f2328),
                text_muted: rgb(0x475569),
                text_faint: rgb(0x64748b),
                accent: pick(c.colorCurrentMonth, dl.colorCurrentMonth, dd.colorCurrentMonth, 0x2563eb),
                today: pick(c.colorCalendarToday, dl.colorCalendarToday, dd.colorCalendarToday, 0x2563eb),
                positive: pick(c.colorPositiveAmount, dl.colorPositiveAmount, dd.colorPositiveAmount, 0x15803d),
                negative: pick(c.colorNegativeAmount, dl.colorNegativeAmount, dd.colorNegativeAmount, 0xb91c1c),
                disabled: pick(c.colorDisabledPayment, dl.colorDisabledPayment, dd.colorDisabledPayment, 0x6b7280),
                paid: pick(c.bgColorPaid, dl.bgColorPaid, dd.bgColorPaid, 0x16a34a),
                important: pick(c.bgColorImportant, dl.bgColorImportant, dd.bgColorImportant, 0xd97706),
                bal: strip(c.bgColorBalance, dl.bgColorBalance, dd.bgColorBalance, 0xf3f4f6, 0x374151),
                max: strip(c.bgColorMaxBalance, dl.bgColorMaxBalance, dd.bgColorMaxBalance, 0xfce7f3, 0x9d174d),
                min: strip(c.bgColorMinBalance, dl.bgColorMinBalance, dd.bgColorMinBalance, 0xdbeafe, 0x1e40af),
                low: strip(c.bgColorBelowMinBalance, dl.bgColorBelowMinBalance, dd.bgColorBelowMinBalance, 0xfef3c7, 0x92400e),
            }
        }
    }

    pub fn signed(&self, v: f64) -> Color32 { if v < 0.0 { self.negative } else { self.positive } }
    pub fn card(&self) -> egui::Frame {
        egui::Frame::new().fill(self.surface).stroke(Stroke::new(1.0_f32, self.border)).corner_radius(RADIUS).inner_margin(SP3)
    }
}

/// Segoe UI (regular / semibold / bold) when present, else egui's bundled fonts; Phosphor icons as fallback everywhere.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let win = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let mut load = |key: &str, file: &str| -> bool {
        match std::fs::read(win.join(file)) {
            Ok(bytes) => { fonts.font_data.insert(key.into(), Arc::new(FontData::from_owned(bytes))); true }
            Err(_) => false,
        }
    };
    let regular = load("ui", "segoeui.ttf");
    let semi = load("ui-semi", "seguisb.ttf");
    let bold = load("ui-bold", "segoeuib.ttf");
    let mut base: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    // Fallbacks for CJK, symbols and emoji so user-entered names never render as empty boxes.
    // Each entry is optional: a missing file is skipped.
    for (i, file) in ["msyh.ttc", "YuGothR.ttc", "malgun.ttf", "seguisym.ttf", "seguiemj.ttf"].iter().enumerate() {
        let key = format!("fallback-{i}");
        if load(&key, file) { base.push(key); }
    }
    let family = |first: Option<&str>| -> Vec<String> {
        let mut v = base.clone();
        if let Some(f) = first { v.insert(0, f.into()); }
        v
    };
    fonts.families.insert(FontFamily::Proportional, family(regular.then_some("ui")));
    fonts.families.insert(FontFamily::Name("semi".into()), family(semi.then_some("ui-semi").or(regular.then_some("ui"))));
    fonts.families.insert(FontFamily::Name("bold".into()), family(bold.then_some("ui-bold").or(regular.then_some("ui"))));
    ctx.set_fonts(fonts);
}

/// Visuals + type scale for the current tokens.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let semi = FontFamily::Name("semi".into());
    let prop = FontFamily::Proportional;
    ctx.style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, prop.clone())),
            (TextStyle::Body, FontId::new(14.0, prop.clone())),
            (TextStyle::Button, FontId::new(14.0, prop.clone())),
            (TextStyle::Heading, FontId::new(24.0, semi.clone())),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
            (ts(TITLE), FontId::new(24.0, semi.clone())),
            (ts(SECTION), FontId::new(15.0, semi.clone())),
            (ts(CAPTION), FontId::new(12.0, prop.clone())),
            (ts(VALUE), FontId::new(25.0, semi.clone())),
            (ts(DAY), FontId::new(14.0, semi.clone())),
            (ts(SEMI), FontId::new(14.0, semi.clone())),
            (ts(ICON), FontId::new(17.0, prop.clone())),
        ].into();
        s.animation_time = 0.12;
        s.spacing.item_spacing = Vec2::new(SP2, SP2);
        s.spacing.button_padding = Vec2::new(12.0, 7.0);
        s.spacing.interact_size = Vec2::new(40.0, 34.0);
        s.spacing.menu_margin = SP2.into();
        s.spacing.window_margin = SP3.into();
        let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.override_text_color = Some(t.text);
        v.panel_fill = t.bg;
        v.window_fill = t.surface;
        v.window_stroke = Stroke::new(1.0_f32, t.border);
        v.window_corner_radius = RADIUS.into();
        v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 40 }) };
        v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 30 }) };
        v.menu_corner_radius = RADIUS.into();
        v.extreme_bg_color = t.surface;
        v.faint_bg_color = t.surface_hover;
        v.hyperlink_color = t.accent;
        v.warn_fg_color = t.important;
        v.error_fg_color = t.negative;
        v.selection.bg_fill = tint(t.surface, t.accent, 0.3);
        v.selection.stroke = Stroke::new(2.0_f32, t.accent);
        let w = &mut v.widgets;
        let cr: CornerRadius = 6.into();
        w.noninteractive.bg_fill = t.surface;
        w.noninteractive.weak_bg_fill = t.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.noninteractive.corner_radius = cr;
        w.inactive.bg_fill = t.surface;
        w.inactive.weak_bg_fill = t.surface;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, t.border);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.inactive.corner_radius = cr;
        w.hovered.bg_fill = t.surface_hover;
        w.hovered.weak_bg_fill = t.surface_hover;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, tint(t.surface, t.accent, 0.6));
        w.hovered.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.hovered.corner_radius = cr;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = tint(t.surface, t.accent, 0.2);
        w.active.weak_bg_fill = tint(t.surface, t.accent, 0.2);
        w.active.bg_stroke = Stroke::new(1.0_f32, t.accent);
        w.active.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.active.corner_radius = cr;
        w.active.expansion = 0.0;
        w.open = w.hovered;
        s.visuals = v;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roles_meet_wcag_aa_on_the_surface_in_both_themes() {
        let cfg = budget_core::config::BudgetConfig::default();
        for dark in [false, true] {
            let t = Tokens::new(&cfg.colors, dark);
            for (name, c) in [("text", t.text), ("muted", t.text_muted), ("faint", t.text_faint),
                ("positive", t.positive), ("negative", t.negative), ("disabled", t.disabled),
                ("bal fg", t.bal.1), ("min fg", t.min.1), ("max fg", t.max.1), ("low fg", t.low.1)] {
                let against = if name.ends_with("fg") {
                    match name { "bal fg" => t.bal.0, "min fg" => t.min.0, "max fg" => t.max.0, _ => t.low.0 }
                } else { t.surface };
                let ratio = contrast(c, against);
                assert!(ratio >= 4.5, "{name} on its background in {} theme is {ratio:.2}:1", if dark { "dark" } else { "light" });
            }
        }
    }

    #[test]
    fn past_weekend_and_other_month_cells_are_three_distinct_tiers() {
        let cfg = budget_core::config::BudgetConfig::default();
        for dark in [false, true] {
            let t = Tokens::new(&cfg.colors, dark);
            assert_ne!(t.past, t.surface);
            assert_ne!(t.weekend, t.surface);
            assert_ne!(t.other_month, t.past);
            assert_ne!(t.other_month, t.bg, "dark={dark}: leading/trailing days must not vanish into the window");
        }
    }
}
