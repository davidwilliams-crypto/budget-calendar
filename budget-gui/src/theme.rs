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

/// A secondary button raised to the accent (1px accent stroke, accent SEMI text):
/// used for "Save changes" while there is something to save.
pub fn accent_outline_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    let resp = ui.add(egui::Button::new(egui::RichText::new(label.into()).color(t.accent).text_style(ts(SEMI)))
        .fill(tint(t.surface, t.accent, 0.06)).stroke(Stroke::new(1.0_f32, t.accent)).corner_radius(7).min_size(Vec2::new(0.0, 36.0)));
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


/// WCAG AA minimum contrast ratio for text.
pub const AA_TEXT: f32 = 4.5;

/// WCAG relative-luminance contrast ratio between two colours.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg` as text on `bg`: moved toward the readable ink of `bg` (dark ink on light fills, white on dark
/// ones) only as far as WCAG AA needs. A colour that already passes is returned unchanged, so a
/// user-chosen colour keeps its hue wherever it is legible.
pub fn legible(fg: Color32, bg: Color32) -> Color32 {
    if contrast(fg, bg) >= AA_TEXT { return fg; }
    let ink = on(bg);
    (1..=50).map(|i| fg.lerp_to_gamma(ink, i as f32 / 50.0)).find(|c| contrast(*c, bg) >= AA_TEXT).unwrap_or(ink)
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
    /// The "adjusted" transaction marker. Violet rather than the accent, so it never reads as today.
    pub adjusted: Color32,
    /// Balance strips: (background, foreground).
    pub bal: (Color32, Color32),
    pub max: (Color32, Color32),
    pub min: (Color32, Color32),
    pub low: (Color32, Color32),
}

/// A `.budgetrc` palette entry.
type Key = fn(&Colors) -> u32;

impl Tokens {
    /// Each role is listed once with its modern light and dark tone. Roles tied to a `.budgetrc`
    /// colour use that colour instead when the user changed it (it matches neither stock palette).
    pub fn new(c: &Colors, dark: bool) -> Tokens {
        let stock = [Colors::theme(Theme::Light), Colors::theme(Theme::Dark)];
        let customised = |key: Key| stock.iter().all(|s| key(s) != key(c));
        let tone = |light: u32, night: u32| rgb(if dark { night } else { light });
        let user = |key: Key, light: u32, night: u32| if customised(key) { rgb(key(c)) } else { tone(light, night) };
        // Balance strips: (background, foreground); a custom background gets a readable foreground.
        let strip = |key: Key, light: (u32, u32), night: (u32, u32)| {
            if customised(key) { let bg = rgb(key(c)); (bg, on(bg)) } else { (tone(light.0, night.0), tone(light.1, night.1)) }
        };
        Tokens {
            dark,
            //                         .budgetrc colour                    light      dark
            bg:            user(|c| c.bgColorWindow,                   0xf4f6fa, 0x111722),
            surface:       user(|c| c.bgColorCell,                     0xffffff, 0x1b1f27),
            surface_hover: tone(                                       0xf0f3f8, 0x242935),
            weekend:       tone(                                       0xf6f8fb, 0x222831),
            past:          user(|c| c.bgColorCellPast,                 0xe6ebf2, 0x151921),
            other_month:   user(|c| c.bgColorCellOtherMonths,          0xeef1f6, 0x0f131b),
            border:        tone(                                       0xe2e8f0, 0x2b313d),
            border_strong: tone(                                       0xcbd5e1, 0x3b4350),
            text:          user(|c| c.colorDescText,                   0x1f2328, 0xe6e8eb),
            text_muted:    tone(                                       0x475569, 0xa0aec0),
            text_faint:    tone(                                       0x5b6779, 0x8b97aa),
            accent:        user(|c| c.colorCurrentMonth,               0x2563eb, 0x60a5fa),
            today:         user(|c| c.colorCalendarToday,              0x2563eb, 0x60a5fa),
            positive:      user(|c| c.colorPositiveAmount,             0x15803d, 0x4ade80),
            negative:      user(|c| c.colorNegativeAmount,             0xb91c1c, 0xf87171),
            disabled:      user(|c| c.colorDisabledPayment,            0x6b7280, 0x7d8799),
            paid:          user(|c| c.bgColorPaid,                     0x16a34a, 0x34d399),
            important:     user(|c| c.bgColorImportant,                0xd97706, 0xfbbf24),
            adjusted:      tone(                                       0x6d28d9, 0xa78bfa),
            bal:           strip(|c| c.bgColorBalance,         (0xf3f4f6, 0x374151), (0x232833, 0xc9ced6)),
            max:           strip(|c| c.bgColorMaxBalance,      (0xdcfce7, 0x166534), (0x123524, 0x86efac)),
            // Neutral slate: the lowest day reads as information, not as the accent selection.
            min:           strip(|c| c.bgColorMinBalance,      (0xe2e8f0, 0x334155), (0x2d3748, 0xe2e8f0)),
            low:           strip(|c| c.bgColorBelowMinBalance, (0xfef3c7, 0x92400e), (0x4a3410, 0xfcd34d)),
        }
    }

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
            // Repeated (unchanged) day balances use faint text on the ordinary balance strip tint.
            let ratio = contrast(t.text_faint, tint(t.surface, t.bal.0, 0.5));
            assert!(ratio >= 4.5, "faint on the ordinary balance strip in {} theme is {ratio:.2}:1", if dark { "dark" } else { "light" });
        }
    }

    fn dump(t: &Tokens) -> String {
        let h = |c: Color32| format!("{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        let pair = |p: (Color32, Color32)| format!("{}/{}", h(p.0), h(p.1));
        [h(t.bg), h(t.surface), h(t.surface_hover), h(t.weekend), h(t.past), h(t.other_month), h(t.border), h(t.border_strong),
            h(t.text), h(t.text_muted), h(t.text_faint), h(t.accent), h(t.today), h(t.positive), h(t.negative), h(t.disabled),
            h(t.paid), h(t.important), h(t.adjusted), pair(t.bal), pair(t.max), pair(t.min), pair(t.low)].join(" ")
    }

    /// Every role keeps its exact value: the stock palettes map to the modern tones, and customised
    /// `.budgetrc` colours are honoured with a derived readable foreground.
    #[test]
    fn every_colour_role_keeps_its_value_in_both_themes() {
        let stock = budget_core::config::BudgetConfig::default().colors;
        let custom = Colors {
            bgColorWindow: 0x101112, bgColorCell: 0x202122, bgColorCellPast: 0x303132, bgColorCellOtherMonths: 0x404142,
            colorDescText: 0x505152, colorCurrentMonth: 0x606162, colorCalendarToday: 0x707172, colorPositiveAmount: 0x808182,
            colorNegativeAmount: 0x909192, colorDisabledPayment: 0xa0a1a2, bgColorPaid: 0xb0b1b2, bgColorImportant: 0xc0c1c2,
            bgColorBalance: 0xd0d1d2, bgColorMaxBalance: 0xe0e1e2, bgColorMinBalance: 0xf0f1f2, bgColorBelowMinBalance: 0x0a0b0c,
            ..Colors::theme(Theme::Light)
        };
        let dark_stock = Colors::theme(Theme::Dark);
        let light = "f4f6fa ffffff f0f3f8 f6f8fb e6ebf2 eef1f6 e2e8f0 cbd5e1 1f2328 475569 5b6779 2563eb 2563eb 15803d b91c1c \
            6b7280 16a34a d97706 6d28d9 f3f4f6/374151 dcfce7/166534 e2e8f0/334155 fef3c7/92400e";
        let cases = [
            (&stock, false, light),
            (&stock, true, "111722 1b1f27 242935 222831 151921 0f131b 2b313d 3b4350 e6e8eb a0aec0 8b97aa 60a5fa 60a5fa 4ade80 \
                f87171 7d8799 34d399 fbbf24 a78bfa 232833/c9ced6 123524/86efac 2d3748/e2e8f0 4a3410/fcd34d"),
            // The stock dark palette counts as stock in the light theme too.
            (&dark_stock, false, light),
            (&custom, false, "101112 202122 f0f3f8 f6f8fb 303132 404142 e2e8f0 cbd5e1 505152 475569 5b6779 606162 707172 808182 \
                909192 a0a1a2 b0b1b2 c0c1c2 6d28d9 d0d1d2/172033 e0e1e2/172033 f0f1f2/172033 0a0b0c/ffffff"),
            (&custom, true, "101112 202122 242935 222831 303132 404142 2b313d 3b4350 505152 a0aec0 8b97aa 606162 707172 808182 \
                909192 a0a1a2 b0b1b2 c0c1c2 a78bfa d0d1d2/172033 e0e1e2/172033 f0f1f2/172033 0a0b0c/ffffff"),
        ];
        for (colors, dark, expected) in cases {
            assert_eq!(dump(&Tokens::new(colors, dark)), expected, "dark={dark}");
        }
    }

    /// The largest per-channel difference between two colours, 0 to 255.
    fn channel_gap(a: Color32, b: Color32) -> u8 {
        a.r().abs_diff(b.r()).max(a.g().abs_diff(b.g())).max(a.b().abs_diff(b.b()))
    }

    #[test]
    fn past_weekend_and_weekday_cells_are_visibly_distinct() {
        // Future weekdays are the surface. The old light past and weekend differed by 2 units and read as one tier,
        // so every pair here needs a gap of 6.
        let cfg = budget_core::config::BudgetConfig::default();
        for dark in [false, true] {
            let t = Tokens::new(&cfg.colors, dark);
            let mode = if dark { "dark" } else { "light" };
            for (pair, a, b) in [("past/weekday", t.past, t.surface), ("weekend/weekday", t.weekend, t.surface),
                ("past/weekend", t.past, t.weekend), ("other month/past", t.other_month, t.past)] {
                let gap = channel_gap(a, b);
                assert!(gap >= 6, "{pair} differ by only {gap} channel units in the {mode} theme");
            }
            assert_ne!(t.other_month, t.bg, "{mode}: leading/trailing days must not vanish into the window");
        }
    }

    #[test]
    fn adjusted_marker_and_lowest_day_band_avoid_the_accent_and_pass_contrast() {
        let cfg = budget_core::config::BudgetConfig::default();
        for dark in [false, true] {
            let t = Tokens::new(&cfg.colors, dark);
            let mode = if dark { "dark" } else { "light" };
            // The adjusted marker is a dot on the row and a pill in the hover card, on the surface or its hover tint.
            let pill = tint(t.surface, t.adjusted, 0.16);
            for (place, bg) in [("surface", t.surface), ("hover", t.surface_hover), ("pill", pill)] {
                let ratio = contrast(t.adjusted, bg);
                assert!(ratio >= 4.5, "adjusted on the {place} in the {mode} theme is {ratio:.2}:1");
            }
            assert_ne!(t.adjusted, t.accent, "adjusted must not read as today");
            // The lowest-day band is neutral: a slate with no hue of its own, not the accent selection.
            let spread = |c: Color32| c.r().max(c.g()).max(c.b()) - c.r().min(c.g()).min(c.b());
            assert!(spread(t.min.0) <= 40 && spread(t.min.1) <= 40, "min band is not neutral in the {mode} theme");
            assert_ne!(t.min.0, t.accent);
            assert!(contrast(t.min.1, t.min.0) >= 4.5);
        }
    }

    #[test]
    fn legible_moves_text_only_as_far_as_aa_needs() {
        let white = Color32::WHITE;
        let dark = Color32::from_rgb(0x1b, 0x1f, 0x27);
        // A colour that already passes is kept exactly, so user colours keep their hue.
        assert_eq!(legible(rgb(0x15803d), white), rgb(0x15803d));
        for (fg, bg) in [(rgb(0x16a34a), rgb(0xf0f9f3)), (rgb(0x7d8799), rgb(0x2a2f3a)), (rgb(0xf87171), rgb(0x1e3a5f)), (rgb(0xf87171), rgb(0x4a3410))] {
            let out = legible(fg, bg);
            assert!(contrast(out, bg) >= 4.5, "{fg:?} on {bg:?} is {:.2}:1 after legible", contrast(out, bg));
        }
        assert!(contrast(legible(rgb(0xfbbf24), dark), dark) >= 4.5);
    }
}
