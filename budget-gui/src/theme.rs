//! Design tokens (colour roles, type scale, spacing) and egui style setup, following the Meridian
//! design system: cards on a canvas, one accent, generous radii, a barely-there shadow, Geist type.
//!
//! Colours come from the user's `.budgetrc` palette. A colour that still has its stock value
//! (light or dark default) is replaced by the modern tone of the same role; a colour the user
//! changed is honoured as-is, with a readable foreground derived from its luminance.

use budget_core::config::{Colors, Theme};
use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, StrokeKind, TextStyle, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// Meridian spacing steps: SP1 = space-1 (4), SP2 = space-2 (8), SP3 = space-4 (16), SP4 = space-6 (24).
pub const SP1: f32 = 4.0;
pub const SP2: f32 = 8.0;
pub const SP3: f32 = 16.0;
pub const SP4: f32 = 24.0;
/// Meridian space-3: the gap between cards.
pub const GAP: f32 = 12.0;
/// Meridian space-5: the padding inside a card.
pub const CARD_PAD: f32 = 20.0;
/// Meridian radius-lg: cards.
pub const RADIUS: u8 = 20;
/// Meridian radius-sm: small inner controls, chips inside controls, keycaps.
pub const RADIUS_SM: u8 = 8;
/// Meridian radius-md: buttons, inputs, menus, list rows, banners.
pub const RADIUS_MD: u8 = 12;
/// Meridian radius-xl: dialogs and other floating shells.
pub const RADIUS_XL: u8 = 28;
/// Meridian radius-full: badges, pills, toggles.
pub const RADIUS_FULL: u8 = 255;
/// Meridian control height: buttons and inputs line up at 40px.
pub const CONTROL_H: f32 = 40.0;
/// Meridian dense control height, for table rows, toolbars and the calendar's dense controls.
pub const CONTROL_H_DENSE: f32 = 32.0;

/// Named text styles beyond egui's built-ins (see `apply`).
pub const TITLE: &str = "title";
pub const SECTION: &str = "section";
pub const CAPTION: &str = "caption";
pub const VALUE: &str = "value";
pub const DAY: &str = "day";
pub const SEMI: &str = "semi";
pub const ICON: &str = "icon";
/// Meridian `label`: 11px semibold eyebrows and column headers, sentence case.
pub const LABEL: &str = "label";

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
/// Readable text colour on an arbitrary background: Meridian `ink` on light fills, white on dark ones.
pub fn on(bg: Color32) -> Color32 {
    let dark = rgb(0x0b1433);
    let best = if contrast(dark, bg) > contrast(Color32::WHITE, bg) { dark } else { Color32::WHITE };
    if contrast(best, bg) >= AA_TEXT { return best; }
    // Mid-tone fills where neither the soft ink nor white reaches AA: take pure black or white.
    if contrast(Color32::BLACK, bg) > contrast(Color32::WHITE, bg) { Color32::BLACK } else { Color32::WHITE }
}

/// Darkens (light theme) or lightens (dark theme) a fill for hover feedback.
pub fn hover_shade(c: Color32, dark: bool) -> Color32 {
    if dark { c.lerp_to_gamma(Color32::WHITE, 0.12) } else { c.lerp_to_gamma(Color32::BLACK, 0.08) }
}

/// Meridian's keyboard-focus ring: 2px solid `focus`, 2px outside the control, on every helper button.
pub fn focus_ring(ui: &egui::Ui, resp: &egui::Response, t: &Tokens) {
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect.expand(2.0), RADIUS_MD + 2, Stroke::new(2.0_f32, t.focus), StrokeKind::Outside);
    }
}

/// The same ring drawn inside the response's rectangle, for rows that fill their whole width.
pub fn focus_ring_inside(ui: &egui::Ui, resp: &egui::Response, t: &Tokens) {
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect, RADIUS_SM, Stroke::new(2.0_f32, t.focus), StrokeKind::Inside);
    }
}

/// A filled button (Meridian `mr-btn`) whose fill changes on hover/press. The fill is set through the
/// widget visuals (not `Button::fill`, which would pin one colour for every state).
fn filled_button(ui: &mut egui::Ui, text: egui::RichText, fill: Color32, hover: Color32, t: &Tokens) -> egui::Response {
    let resp = ui.scope(|ui| {
        let pressed = hover_shade(hover, t.dark);
        // `apply` sets an override text colour for every label, and an uncoloured `RichText` takes it
        // (egui bakes it into the galley), which would hide the per-state ink set below.
        ui.style_mut().visuals.override_text_color = None;
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.weak_bg_fill = fill;
        w.hovered.weak_bg_fill = hover;
        w.active.weak_bg_fill = pressed;
        // The label takes the ink of the state's own fill: shading the fill toward (or away from) the
        // rest-state ink can drop a custom accent below 4.5:1 on hover, press or keyboard focus.
        w.inactive.fg_stroke.color = on(fill);
        w.hovered.fg_stroke.color = on(hover);
        w.active.fg_stroke.color = on(pressed);
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::NONE;
        w.active.bg_stroke = Stroke::NONE;
        // Meridian buttons are flat: no raised edge, gradient or glow.
        ui.add(egui::Button::new(text).corner_radius(RADIUS_MD).min_size(Vec2::new(0.0, CONTROL_H)))
    }).inner;
    focus_ring(ui, &resp, t);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Meridian `mr-btn`: the one primary action, filled with `accent`.
pub fn primary_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    filled_button(ui, egui::RichText::new(label.into()).text_style(ts(SEMI)), t.accent, t.accent_hover, t)
}

/// Meridian `mr-btn--ghost`: cancel, dismiss and other tertiary actions, outlined in `border-strong`.
pub fn secondary_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    let enabled = ui.is_enabled();
    // Disabled keeps readable text (muted, not faint) so a greyed "Save changes" is still legible.
    let color = if enabled { t.text } else { t.text_muted };
    let resp = ui.scope(|ui| {
        // Per-state visuals (not `Button::fill`/`stroke`, which pin one look for every state): the
        // ground sinks on hover and press; the outline stays `border-strong`.
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.weak_bg_fill = t.surface;
        w.hovered.weak_bg_fill = t.surface_hover;
        w.active.weak_bg_fill = tint(t.surface_hover, t.border, 0.5);
        w.inactive.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
        w.hovered.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
        w.active.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
        ui.add(egui::Button::new(egui::RichText::new(label.into()).color(color)).corner_radius(RADIUS_MD).min_size(Vec2::new(0.0, CONTROL_H)))
    }).inner;
    focus_ring(ui, &resp, t);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Meridian `mr-btn--secondary`: a `surface-tint` fill with `accent-text`. Used for "Save changes"
/// while there is something to save, beside the primary "New payment".
pub fn accent_outline_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    let resp = ui.scope(|ui| {
        ui.style_mut().visuals.override_text_color = None;
        let w = &mut ui.style_mut().visuals.widgets;
        // On hover the label turns `ink`, as Meridian's secondary button does; the fill stays the tint.
        w.inactive.weak_bg_fill = t.surface_tint;
        w.hovered.weak_bg_fill = t.surface_tint;
        w.active.weak_bg_fill = t.surface_tint;
        w.inactive.fg_stroke.color = t.accent_text;
        w.hovered.fg_stroke.color = t.text;
        w.active.fg_stroke.color = t.text;
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::NONE;
        w.active.bg_stroke = Stroke::NONE;
        ui.add(egui::Button::new(egui::RichText::new(label.into()).text_style(ts(SEMI))).corner_radius(RADIUS_MD).min_size(Vec2::new(0.0, CONTROL_H)))
    }).inner;
    focus_ring(ui, &resp, t);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn danger_button(ui: &mut egui::Ui, label: impl Into<String>, t: &Tokens) -> egui::Response {
    filled_button(ui, egui::RichText::new(label.into()).text_style(ts(SEMI)), t.negative, hover_shade(t.negative, t.dark), t)
}

/// Meridian `mr-badge--plain`: a short label on a quiet pill (the version chip and similar).
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32, t: &Tokens) {
    egui::Frame::new().fill(tint(t.surface, color, if t.dark { 0.18 } else { 0.07 }))
        .corner_radius(RADIUS_FULL).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).text_style(ts(CAPTION)).color(color)).selectable(false));
        });
}

/// Meridian `mr-badge`: a pill with a leading status dot (e.g. saved / unsaved). The word carries the meaning.
pub fn status_pill(ui: &mut egui::Ui, text: &str, dot: Color32, filled: bool, t: &Tokens) -> egui::Response {
    let fill = if filled { tint(t.surface, dot, if t.dark { 0.22 } else { 0.14 }) } else { t.surface_hover };
    egui::Frame::new().fill(fill).corner_radius(RADIUS_FULL).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
        ui.spacing_mut().interact_size.y = 14.0; // keep the pill compact (default row is 34px)
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(6.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 3.0, dot);
            ui.label(egui::RichText::new(text).text_style(ts(CAPTION)).color(t.text));
        });
    }).response
}

/// A small keycap chip ("Ctrl", "S") used for shortcut hints. Returns its response.
pub fn keycap(ui: &mut egui::Ui, key: &str, t: &Tokens) -> egui::Response {
    egui::Frame::new().fill(t.surface_hover).stroke(Stroke::new(1.0_f32, t.border_strong)).corner_radius(RADIUS_SM)
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
///
/// Called many times per frame for the handful of colour pairs a theme produces, so the powf-based
/// contrast walk is memoized per `(fg, bg)` pair; repeated pairs are a map lookup.
pub fn legible(fg: Color32, bg: Color32) -> Color32 {
    thread_local! {
        static CACHE: RefCell<HashMap<[u8; 8], Color32>> = RefCell::new(HashMap::new());
    }
    let key = [fg.r(), fg.g(), fg.b(), fg.a(), bg.r(), bg.g(), bg.b(), bg.a()];
    if let Some(c) = CACHE.with(|m| m.borrow().get(&key).copied()) { return c; }
    let c = legible_uncached(fg, bg);
    CACHE.with(|m| m.borrow_mut().insert(key, c));
    c
}

fn legible_uncached(fg: Color32, bg: Color32) -> Color32 {
    if contrast(fg, bg) >= AA_TEXT { return fg; }
    let ink = on(bg);
    (1..=50).map(|i| fg.lerp_to_gamma(ink, i as f32 / 50.0)).find(|c| contrast(*c, bg) >= AA_TEXT).unwrap_or(ink)
}

#[derive(Clone)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    /// Meridian `surface-sunken`: inset rows, tracks, hover grounds.
    pub surface_hover: Color32,
    /// Meridian `surface-tint`: quiet accent fills (secondary buttons, selected chips).
    pub surface_tint: Color32,
    /// Meridian `surface-ink`: the one dark emphasis panel per view, with `on-ink` text on it.
    pub surface_ink: Color32,
    pub on_ink: Color32,
    pub on_ink_muted: Color32,
    pub weekend: Color32,
    /// Day cells before today, and after it: the original's "BKG cell past" and "BKG cell future".
    pub past: Color32,
    pub future: Color32,
    /// Either day-cell colour comes from `.budgetrc`: the weekend tier steps aside so it shows.
    pub custom_cells: bool,
    pub other_month: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    /// Meridian `accent-hover`: hover and pressed fill of anything filled with the accent.
    pub accent_hover: Color32,
    /// Meridian `accent-text`: accent-coloured text and links (the accent itself is tuned for fills).
    pub accent_text: Color32,
    /// Meridian `focus`: the keyboard-focus ring.
    pub focus: Color32,
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
        // A customised accent keeps its hue everywhere: its hover is shaded from it and its text is made legible.
        let accent = user(|c| c.colorCurrentMonth, 0x2152e5, 0x7da2ff);
        let surface = tone(0xffffff, 0x10172a);
        let accent_custom = customised(|c| c.colorCurrentMonth);
        Tokens {
            dark,
            // Meridian tokens: canvas, surface, surface-sunken, border, ink and the rest. The calendar's own tiers
            // (weekend, past, other month) and faint text are blends of those tokens, chosen so each tier stays
            // visibly distinct and every text role keeps 4.5:1 on every cell.
            //                         .budgetrc colour                    light      dark
            bg:            user(|c| c.bgColorWindow,                   0xe8edf7, 0x070b16),
            // Cards, dialogs and fields keep the Meridian surface whatever the day-cell colours are.
            surface,
            surface_hover: tone(                                       0xf2f5fb, 0x0a101f),
            surface_tint:  tone(                                       0xdbe6fe, 0x1a2c5e),
            surface_ink:   tone(                                       0x0b1433, 0x1c2540),
            on_ink:        tone(                                       0xffffff, 0xeef2fb),
            on_ink_muted:  tone(                                       0xa9b6d6, 0xa3afc9),
            weekend:       tone(                                       0xf8fafd, 0x171f35),
            // The original's field names are swapped against its labels (analysis/05 §1.5): `bgColorCell`
            // ("BKG cell past") paints days before today, `bgColorCellPast` ("BKG cell future") the rest.
            past:          user(|c| c.bgColorCell,                     0xf2f5fb, 0x0a101f),
            future:        user(|c| c.bgColorCellPast,                 0xffffff, 0x10172a),
            custom_cells:  customised(|c| c.bgColorCell) || customised(|c| c.bgColorCellPast),
            other_month:   user(|c| c.bgColorCellOtherMonths,          0xe2e8f4, 0x05080f),
            border:        tone(                                       0xdbe2f0, 0x222b45),
            border_strong: tone(                                       0x7b88a4, 0x66749a),
            text:          user(|c| c.colorDescText,                   0x0b1433, 0xeef2fb),
            text_muted:    tone(                                       0x4f5a73, 0x9aa6c1),
            text_faint:    tone(                                       0x5d6882, 0x7986a8),
            accent,
            accent_hover:  if accent_custom { hover_shade(accent, dark) } else { tone(0x1a43c4, 0x9bb8ff) },
            accent_text:   if accent_custom { legible(accent, surface) } else { tone(0x1c47cc, 0x9bb8ff) },
            focus:         if accent_custom { accent } else { tone(0x2152e5, 0x9bb8ff) },
            today:         user(|c| c.colorCalendarToday,              0x2152e5, 0x7da2ff),
            positive:      user(|c| c.colorPositiveAmount,             0x0a6e55, 0x4fd1a5),
            negative:      user(|c| c.colorNegativeAmount,             0xb42a18, 0xff8a73),
            disabled:      user(|c| c.colorDisabledPayment,            0x5d6882, 0x7986a8),
            paid:          user(|c| c.bgColorPaid,                     0x0a6e55, 0x4fd1a5),
            important:     user(|c| c.bgColorImportant,                0x855000, 0xf2b84b),
            // Not a Meridian token: Meridian has one accent, and this marker must never read as today.
            adjusted:      tone(                                       0x6d28d9, 0xa78bfa),
            // Balance strips use Meridian's sunken ground and its status pairs (`success` on `success-tint`, ...).
            bal:           strip(|c| c.bgColorBalance,         (0xf2f5fb, 0x4f5a73), (0x0a101f, 0x9aa6c1)),
            max:           strip(|c| c.bgColorMaxBalance,      (0xd7f2e8, 0x0a6e55), (0x0f2f29, 0x4fd1a5)),
            // Neutral `border` ground: the lowest day reads as information, not as the accent selection.
            min:           strip(|c| c.bgColorMinBalance,      (0xdbe2f0, 0x0b1433), (0x222b45, 0xeef2fb)),
            low:           strip(|c| c.bgColorBelowMinBalance, (0xfdefd0, 0x855000), (0x33270d, 0xf2b84b)),
        }
    }

    /// Meridian `shadow-card`: a barely-there shadow in light; in dark, a faint outline instead.
    pub fn card_shadow(&self) -> Shadow {
        if self.dark { Shadow::NONE } else { Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_rgba_unmultiplied(11, 20, 51, 13) } }
    }

    /// Meridian `shadow-pop`: menus, popovers, dialogs and anything floating above cards.
    pub fn pop_shadow(&self) -> Shadow {
        if self.dark { Shadow { offset: [0, 16], blur: 40, spread: 0, color: Color32::from_black_alpha(128) } }
        else { Shadow { offset: [0, 16], blur: 40, spread: 0, color: Color32::from_rgba_unmultiplied(11, 20, 51, 36) } }
    }

    /// The outline that stands in for a shadow in the dark theme (white at 5% for cards, 8% for floating shells).
    fn dark_outline(&self, floating: bool) -> Stroke {
        if self.dark { Stroke::new(1.0_f32, tint(self.surface, Color32::WHITE, if floating { 0.08 } else { 0.05 })) } else { Stroke::NONE }
    }

    /// Meridian `mr-card`: `surface`, `radius-lg`, `space-5` padding, `shadow-card`, no border.
    pub fn card(&self) -> egui::Frame {
        egui::Frame::new().fill(self.surface).stroke(self.dark_outline(false)).corner_radius(RADIUS).inner_margin(CARD_PAD).shadow(self.card_shadow())
    }

    /// A Meridian dialog: `surface`, `radius-xl`, `shadow-pop`.
    pub fn dialog(&self) -> egui::Frame {
        egui::Frame::new().fill(self.surface).stroke(self.dark_outline(true)).corner_radius(RADIUS_XL).inner_margin(SP4).shadow(self.pop_shadow())
    }
}

/// Meridian's one family, Geist, for everything: embedded as "Geist Tabular" (Geist with its tabular figures as the
/// default digits, see `assets/fonts/NOTICE.txt`) in the weights Meridian uses, 400, 500 and 600. Segoe UI, then
/// egui's bundled fonts, cover characters Geist lacks; Phosphor icons and CJK/emoji fallbacks follow.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (key, bytes) in [
        ("geist", &include_bytes!("../assets/fonts/GeistTabular-Regular.ttf")[..]),
        ("geist-medium", &include_bytes!("../assets/fonts/GeistTabular-Medium.ttf")[..]),
        ("geist-semi", &include_bytes!("../assets/fonts/GeistTabular-SemiBold.ttf")[..]),
    ] {
        fonts.font_data.insert(key.into(), Arc::new(FontData::from_static(bytes)));
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let win = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let mut load = |key: &str, file: &str| -> bool {
        match std::fs::read(win.join(file)) {
            Ok(bytes) => { fonts.font_data.insert(key.into(), Arc::new(FontData::from_owned(bytes))); true }
            Err(_) => false,
        }
    };
    let segoe = load("ui", "segoeui.ttf");
    let mut base: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    if segoe { base.insert(0, "ui".into()); }
    // Fallbacks for CJK, symbols and emoji so user-entered names never render as empty boxes.
    // Each entry is optional: a missing file is skipped.
    for (i, file) in ["msyh.ttc", "YuGothR.ttc", "malgun.ttf", "seguisym.ttf", "seguiemj.ttf"].iter().enumerate() {
        let key = format!("fallback-{i}");
        if load(&key, file) { base.push(key); }
    }
    let family = |first: &str| -> Vec<String> {
        let mut v = base.clone();
        v.insert(0, first.into());
        v
    };
    fonts.families.insert(FontFamily::Proportional, family("geist"));
    fonts.families.insert(FontFamily::Name("medium".into()), family("geist-medium"));
    fonts.families.insert(FontFamily::Name("semi".into()), family("geist-semi"));
    // Meridian's weights stop at 600: "bold" is the semibold face.
    fonts.families.insert(FontFamily::Name("bold".into()), family("geist-semi"));
    ctx.set_fonts(fonts);
}

/// Visuals + type scale for the current tokens.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let semi = FontFamily::Name("semi".into());
    let medium = FontFamily::Name("medium".into());
    let prop = FontFamily::Proportional;
    ctx.style_mut(|s| {
        // Meridian's type scale: body 14, small 12 (500), label 11 (600), h3 16, h2 20, metric 32.
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, medium.clone())),
            (TextStyle::Body, FontId::new(14.0, prop.clone())),
            (TextStyle::Button, FontId::new(14.0, prop.clone())),
            (TextStyle::Heading, FontId::new(20.0, semi.clone())),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
            (ts(TITLE), FontId::new(20.0, semi.clone())),
            (ts(SECTION), FontId::new(16.0, semi.clone())),
            (ts(CAPTION), FontId::new(12.0, medium.clone())),
            (ts(LABEL), FontId::new(11.0, semi.clone())),
            (ts(VALUE), FontId::new(32.0, semi.clone())),
            (ts(DAY), FontId::new(14.0, semi.clone())),
            (ts(SEMI), FontId::new(14.0, semi.clone())),
            (ts(ICON), FontId::new(16.0, prop.clone())),
        ].into();
        s.animation_time = 0.12;
        s.spacing.item_spacing = Vec2::new(SP2, SP2);
        // Meridian: buttons pad `space-4` sideways. The calendar is a dense view, so egui's own controls
        // (inputs, menu items, row buttons) take Meridian's dense 32px; the primary, secondary and ghost
        // buttons above keep the full 40px.
        s.spacing.button_padding = Vec2::new(SP3, 6.0);
        s.spacing.interact_size = Vec2::new(40.0, CONTROL_H_DENSE);
        s.spacing.icon_width = 18.0;
        s.spacing.icon_width_inner = 10.0;
        s.spacing.menu_margin = SP2.into();
        s.spacing.window_margin = SP3.into();
        let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.override_text_color = Some(t.text);
        v.panel_fill = t.bg;
        v.window_fill = t.surface;
        // Floating shells take `shadow-pop`; in dark a faint outline stands in for the shadow's edge.
        v.window_stroke = t.dialog().stroke;
        v.window_corner_radius = RADIUS_XL.into();
        v.window_shadow = t.pop_shadow();
        v.popup_shadow = t.pop_shadow();
        v.menu_corner_radius = RADIUS_MD.into();
        v.extreme_bg_color = t.surface;
        v.faint_bg_color = t.surface_hover;
        v.hyperlink_color = t.accent_text;
        v.warn_fg_color = t.important;
        v.error_fg_color = t.negative;
        v.selection.bg_fill = t.surface_tint;
        v.selection.stroke = Stroke::new(2.0_f32, t.accent);
        let w = &mut v.widgets;
        // egui shares one radius between inputs, checkboxes and plain buttons: Meridian's radius-sm keeps an
        // 18px checkbox square-ish while still softening inputs and menu items.
        let cr: CornerRadius = RADIUS_SM.into();
        w.noninteractive.bg_fill = t.surface;
        w.noninteractive.weak_bg_fill = t.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.noninteractive.corner_radius = cr;
        w.inactive.bg_fill = t.surface;
        w.inactive.weak_bg_fill = t.surface;
        // Inputs and checkboxes are outlines a person must see: `border-strong` (3:1 on every ground).
        w.inactive.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.inactive.corner_radius = cr;
        w.hovered.bg_fill = t.surface_hover;
        w.hovered.weak_bg_fill = t.surface_hover;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, t.border_strong);
        w.hovered.fg_stroke = Stroke::new(1.0_f32, t.text);
        w.hovered.corner_radius = cr;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = t.surface_tint;
        w.active.weak_bg_fill = t.surface_tint;
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
        [h(t.bg), h(t.surface), h(t.surface_hover), h(t.weekend), h(t.past), h(t.future), h(t.other_month), h(t.border), h(t.border_strong),
            h(t.text), h(t.text_muted), h(t.text_faint), h(t.accent), h(t.today), h(t.positive), h(t.negative), h(t.disabled),
            h(t.paid), h(t.important), h(t.adjusted), pair(t.bal), pair(t.max), pair(t.min), pair(t.low)].join(" ")
    }

    /// Every role keeps its exact value: the stock palettes map to the Meridian tones, and customised
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
        let light = "e8edf7 ffffff f2f5fb f8fafd f2f5fb ffffff e2e8f4 dbe2f0 7b88a4 0b1433 4f5a73 5d6882 2152e5 2152e5 0a6e55 b42a18 \
            5d6882 0a6e55 855000 6d28d9 f2f5fb/4f5a73 d7f2e8/0a6e55 dbe2f0/0b1433 fdefd0/855000";
        let cases = [
            (&stock, false, light),
            (&stock, true, "070b16 10172a 0a101f 171f35 0a101f 10172a 05080f 222b45 66749a eef2fb 9aa6c1 7986a8 7da2ff 7da2ff 4fd1a5 \
                ff8a73 7986a8 4fd1a5 f2b84b a78bfa 0a101f/9aa6c1 0f2f29/4fd1a5 222b45/eef2fb 33270d/f2b84b"),
            // The stock dark palette counts as stock in the light theme too.
            (&dark_stock, false, light),
            (&custom, false, "101112 ffffff f2f5fb f8fafd 202122 303132 404142 dbe2f0 7b88a4 505152 4f5a73 5d6882 606162 707172 808182 \
                909192 a0a1a2 b0b1b2 c0c1c2 6d28d9 d0d1d2/0b1433 e0e1e2/0b1433 f0f1f2/0b1433 0a0b0c/ffffff"),
            (&custom, true, "101112 10172a 0a101f 171f35 202122 303132 404142 222b45 66749a 505152 9aa6c1 7986a8 606162 707172 808182 \
                909192 a0a1a2 b0b1b2 c0c1c2 a78bfa d0d1d2/0b1433 e0e1e2/0b1433 f0f1f2/0b1433 0a0b0c/ffffff"),
        ];
        for (colors, dark, expected) in cases {
            assert_eq!(dump(&Tokens::new(colors, dark)), expected, "dark={dark}");
        }
    }

    /// Round 5: the day-cell colours colour the cells only. "BKG cell past" paints days before today,
    /// "BKG cell future" the rest, and either one replaces the weekend tier.
    #[test]
    fn custom_day_cell_colours_follow_the_originals_meaning_and_leave_the_surface_alone() {
        let stock = budget_core::config::BudgetConfig::default().colors;
        let plain = Tokens::new(&stock, false);
        assert!(!plain.custom_cells);
        let past = Tokens::new(&Colors { bgColorCell: 0x202122, ..stock.clone() }, false);
        assert!(past.custom_cells);
        assert_eq!((past.past, past.future, past.surface), (rgb(0x202122), plain.future, plain.surface));
        let future = Tokens::new(&Colors { bgColorCellPast: 0x303132, ..stock.clone() }, true);
        assert!(future.custom_cells);
        assert_eq!((future.future, future.surface), (rgb(0x303132), Tokens::new(&stock, true).surface));
        assert_eq!(future.past, Tokens::new(&stock, true).past);
    }

    /// Round 5: where neither the soft ink nor white reaches AA (grey levels 119 to 134) `on` takes black.
    #[test]
    fn on_always_reaches_aa_even_on_mid_tones() {
        for g in 0..=255u8 {
            let bg = Color32::from_rgb(g, g, g);
            assert!(contrast(on(bg), bg) >= AA_TEXT, "grey {g}: {:.2}:1", contrast(on(bg), bg));
        }
        // Meridian `ink` on light fills, white on dark ones
        assert_eq!(on(rgb(0xf3f4f6)), rgb(0x0b1433));
        assert_eq!(on(rgb(0x2563eb)), Color32::WHITE);
    }

    /// Round 9: the primary and danger buttons must PAINT their label in the ink of their fill, at rest and
    /// under the pointer. An uncoloured `RichText` takes `Visuals::override_text_color` (set for every
    /// label), which silently ignores the per-state `fg_stroke`: white-on-blue became dark-on-blue.
    #[test]
    fn primary_and_danger_labels_are_painted_in_the_ink_of_their_fill() {
        for dark in [false, true] {
            let t = Tokens::new(&budget_core::config::BudgetConfig::default().colors, dark);
            let ctx = egui::Context::default();
            install_fonts(&ctx);
            apply(&ctx, &t);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 300.0));
            let run = |events: Vec<egui::Event>| ctx.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    primary_button(ui, "PrimaryLabel", &t);
                    danger_button(ui, "DangerLabel", &t);
                });
            });
            // The colour the label is actually painted in: its own section colour, or the fallback for a placeholder.
            let painted = |output: &egui::FullOutput, label: &str| -> (egui::Pos2, Color32) {
                for clipped in &output.shapes {
                    if let egui::Shape::Text(text) = &clipped.shape {
                        if text.galley.job.text == label {
                            let section = text.galley.job.sections[0].format.color;
                            return (text.pos, if section == Color32::PLACEHOLDER { text.fallback_color } else { section });
                        }
                    }
                }
                panic!("{label} must be painted");
            };
            run(Vec::new());
            let rest = run(Vec::new());
            for (label, fill, hover) in [("PrimaryLabel", t.accent, t.accent_hover), ("DangerLabel", t.negative, hover_shade(t.negative, dark))] {
                let (pos, colour) = painted(&rest, label);
                let ratio = contrast(colour, fill);
                assert!(ratio >= AA_TEXT, "{label} at rest in the {} theme is {ratio:.2}:1 ({colour:?} on {fill:?})", if dark { "dark" } else { "light" });
                // under the pointer the fill shades; the ink follows it
                let over = pos + egui::vec2(8.0, 6.0);
                run(vec![egui::Event::PointerMoved(over)]);
                let hovered = run(vec![egui::Event::PointerMoved(over)]);
                let (_, colour) = painted(&hovered, label);
                let ratio = contrast(colour, hover);
                assert!(ratio >= AA_TEXT, "{label} hovered in the {} theme is {ratio:.2}:1", if dark { "dark" } else { "light" });
            }
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

    /// Meridian sets figures tabular everywhere: the embedded Geist gives every digit one width in every weight,
    /// so amounts and balances line up in their columns.
    #[test]
    fn geist_is_the_ui_font_and_its_digits_are_tabular() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        apply(&ctx, &Tokens::new(&budget_core::config::BudgetConfig::default().colors, false));
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        for family in [FontFamily::Proportional, FontFamily::Name("medium".into()), FontFamily::Name("semi".into())] {
            let id = FontId::new(14.0, family.clone());
            let widths: Vec<f32> = ctx.fonts(|f| ('0'..='9').map(|c| f.glyph_width(&id, c)).collect());
            assert!(widths.iter().all(|w| (w - widths[0]).abs() < 0.01), "{family:?} digits differ in width: {widths:?}");
            // Proportional letters prove it is Geist and not a monospaced fallback.
            assert!(ctx.fonts(|f| f.glyph_width(&id, 'i') < f.glyph_width(&id, 'W')));
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
