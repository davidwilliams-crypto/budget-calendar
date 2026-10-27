//! The month grid: painter-drawn cells (day number, transaction rows, balance strip) with hover and tooltips.

use eframe::egui::{self, text::{LayoutJob, TextWrapping}, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as ph;

use crate::theme::{self, font, tint, ts, Tokens, CAPTION, DAY, ICON, SECTION, SEMI};
use crate::view::{money, BalKind, CellData, MonthView, TranRow};

const WDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MIN_CELL_H: f32 = 96.0;
const HEADER_H: f32 = 30.0;
const ROW_H: f32 = 19.0;
const STRIP_H: f32 = 20.0;
const PAD: f32 = 6.0;
const TOP: f32 = 30.0;

fn truncated(p: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32, max_w: f32) -> f32 {
    let mut job = LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_w.max(8.0));
    let g = p.layout_job(job);
    let w = g.size().x;
    p.galley(pos - Vec2::new(0.0, g.size().y / 2.0), g, color);
    w
}

/// Draws the grid; returns the filter expression of a clicked transaction row.
pub fn show(ui: &mut Ui, view: &MonthView, t: &Tokens, sunday_first: bool, today: i32, decimals: bool) -> Option<String> {
    let rows = (view.cells.len() / 7).max(1);
    let cell_h = ((ui.available_height() - HEADER_H - 2.0) / rows as f32).floor().max(MIN_CELL_H);
    let total_h = HEADER_H + cell_h * rows as f32 + 2.0;
    let mut clicked = None;
    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let (outer, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::hover());
        let p = ui.painter().clone();
        p.rect(outer, theme::RADIUS, t.surface, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
        let inner = outer.shrink(1.0);
        let cw = inner.width() / 7.0;
        let hair = Stroke::new(1.0_f32, t.border);
        // weekday header
        for c in 0..7 {
            let w = if sunday_first { c } else { (c + 1) % 7 };
            let name = if cw >= 120.0 { WDAYS[w].to_string() } else { WDAYS[w][..3].to_string() };
            let weekend = w == 0 || w == 6;
            let x = inner.min.x + cw * (c as f32 + 0.5);
            p.text(Pos2::new(x, inner.min.y + HEADER_H / 2.0), Align2::CENTER_CENTER, name, font(ui, SEMI), if weekend { t.text_faint } else { t.text_muted });
        }
        let grid_top = inner.min.y + HEADER_H;
        for (i, cell) in view.cells.iter().enumerate() {
            let (r, c) = (i / 7, i % 7);
            let rect = Rect::from_min_size(Pos2::new(inner.min.x + cw * c as f32, grid_top + cell_h * r as f32), Vec2::new(cw, cell_h));
            let last = r + 1 == rows;
            let cr = CornerRadius { nw: 0, ne: 0, sw: if last && c == 0 { theme::RADIUS } else { 0 }, se: if last && c == 6 { theme::RADIUS } else { 0 } };
            let resp = ui.interact(rect, ui.id().with(("cell", cell.date)), Sense::hover());
            let fill = if !cell.in_month { t.other_month }
                else if resp.hovered() { t.surface_hover }
                else if cell.is_today { tint(t.surface, t.today, if t.dark { 0.18 } else { 0.07 }) }
                else if cell.weekend { t.weekend }
                else if cell.date < today { t.past }
                else { t.surface };
            p.rect_filled(rect, cr, fill);
            if let Some(f) = cell_filter(ui, &p, cell, rect, t, today, decimals, &resp) { clicked = Some(f); }
        }
        // hairlines on top of the fills
        p.hline(inner.x_range(), grid_top - 0.5, hair);
        for r in 1..rows { p.hline(inner.x_range(), grid_top + cell_h * r as f32 - 0.5, hair); }
        for c in 1..7 { p.vline(inner.min.x + cw * c as f32 - 0.5, egui::Rangef::new(grid_top, inner.max.y), hair); }
    });
    clicked
}

fn cell_filter(ui: &mut Ui, p: &Painter, cell: &CellData, rect: Rect, t: &Tokens, today: i32, decimals: bool, resp: &egui::Response) -> Option<String> {
    let mut clicked = None;
    // day number
    let day_pos = Pos2::new(rect.min.x + PAD + 11.0, rect.min.y + PAD + 11.0);
    if cell.is_today && cell.in_month {
        p.circle_filled(day_pos, 11.0, t.today);
        p.text(day_pos, Align2::CENTER_CENTER, cell.day.to_string(), font(ui, DAY), Color32::WHITE);
    } else {
        let col = if !cell.in_month { t.text_faint } else if cell.date < today { t.text_muted } else { t.text };
        p.text(Pos2::new(rect.min.x + PAD + 2.0, day_pos.y), Align2::LEFT_CENTER, cell.day.to_string(), font(ui, DAY), col);
    }
    if !cell.in_month { return None; }
    // top-right indicators
    let mut ix = rect.max.x - PAD - 2.0;
    let icon = font(ui, ICON);
    if cell.note.is_some() {
        let r = p.text(Pos2::new(ix, day_pos.y), Align2::RIGHT_CENTER, ph::NOTE_PENCIL, icon.clone(), t.text_muted);
        ix = r.min.x - 4.0;
    }
    if cell.restorable { p.text(Pos2::new(ix, day_pos.y), Align2::RIGHT_CENTER, ph::ARROW_COUNTER_CLOCKWISE, icon, t.text_faint); }
    if resp.hovered() && (cell.note.is_some() || cell.restorable) {
        resp.clone().on_hover_ui(|ui| {
            if let Some(n) = &cell.note { ui.label(RichTextExt::section(n, ui)); }
            if cell.restorable { ui.label(RichTextExt::muted("Has deleted or moved transactions that can be restored", ui)); }
        });
    }
    // transaction rows
    let strip_top = rect.max.y - SP - STRIP_H;
    let avail = (((strip_top - SP) - (rect.min.y + TOP)) / ROW_H).floor().max(0.0) as usize;
    let n = cell.rows.len();
    let show_n = if n > avail { avail.saturating_sub(1) } else { n };
    let mut y = rect.min.y + TOP;
    for row in &cell.rows[..show_n] {
        let rr = Rect::from_min_size(Pos2::new(rect.min.x + 3.0, y), Vec2::new(rect.width() - 6.0, ROW_H));
        if draw_row(ui, p, row, rr, t, decimals) { clicked = Some(row.filter.clone()); }
        y += ROW_H;
    }
    if n > show_n {
        p.text(Pos2::new(rect.min.x + PAD + 2.0, y + ROW_H / 2.0), Align2::LEFT_CENTER, format!("+{} more", n - show_n), font(ui, CAPTION), t.text_muted);
    }
    // balance strip
    if let Some(b) = &cell.balance {
        let sr = Rect::from_min_size(Pos2::new(rect.min.x + SP, strip_top), Vec2::new(rect.width() - 2.0 * SP, STRIP_H));
        let (bg, fg, label) = match b.kind {
            BalKind::Max => (t.max.0, t.max.1, "MAX"),
            BalKind::Min => (t.min.0, t.min.1, "MIN"),
            BalKind::Low => (t.low.0, t.low.1, "LOW"),
            BalKind::Normal => (t.bal.0, if b.value < 0.0 { t.negative } else { t.bal.1 }, ""),
        };
        p.rect_filled(sr, 4, bg);
        let cy = sr.center().y;
        if !label.is_empty() { p.text(Pos2::new(sr.min.x + 6.0, cy), Align2::LEFT_CENTER, label, FontId::new(9.5, egui::FontFamily::Name("semi".into())), fg); }
        let mut txt = b.text.clone();
        if b.adjusted { txt.push('\u{2032}'); }
        let fid = if b.kind == BalKind::Normal { egui::TextStyle::Body.resolve(ui.style()) } else { font(ui, SEMI) };
        p.text(Pos2::new(sr.max.x - 6.0, cy), Align2::RIGHT_CENTER, txt, fid, fg);
        let sresp = ui.interact(sr, ui.id().with(("strip", cell.date)), Sense::hover());
        if sresp.hovered() {
            sresp.on_hover_ui(|ui| {
                ui.label(RichTextExt::section(&format!("Closing balance {}", money(b.value, true)), ui));
                let what = match b.kind { BalKind::Max => "Highest balance of the month", BalKind::Min => "Lowest balance of the month", BalKind::Low => "Below your minimum balance", BalKind::Normal => "" };
                if !what.is_empty() { ui.label(RichTextExt::muted(what, ui)); }
                if b.adjusted { ui.label(RichTextExt::muted("Reconciled (balance adjustment on this day)", ui)); }
            });
        }
    }
    clicked
}

const SP: f32 = 4.0;

fn draw_row(ui: &mut Ui, p: &Painter, row: &TranRow, rr: Rect, t: &Tokens, decimals: bool) -> bool {
    let compact = rr.width() < 150.0;
    let resp = ui.interact(rr, ui.id().with(("row", rr.min.y as i32, &row.text)), Sense::click());
    let hovered = resp.hovered();
    if let Some(h) = row.hilite { p.rect_filled(rr, 4, tint(t.surface, h, 0.35)); }
    if hovered { p.rect_filled(rr, 4, tint(t.surface, t.accent, 0.12)); }
    let cy = rr.center().y;
    let (icon_col, bar) = if row.disabled { (t.disabled, None) }
        else if row.flag == budget_core::FLAG_PAID { (t.paid, Some(t.paid)) }
        else if row.flag == budget_core::FLAG_IMPORTANT { (t.important, Some(t.important)) }
        else { (t.text_muted, None) };
    if let Some(b) = bar { p.rect_filled(Rect::from_min_size(Pos2::new(rr.min.x + 1.0, rr.min.y + 3.0), Vec2::new(3.0, rr.height() - 6.0)), 2, b); }
    let glyph = if row.flag == budget_core::FLAG_PAID { ph::CHECK_CIRCLE } else if row.flag == budget_core::FLAG_IMPORTANT { ph::WARNING_CIRCLE } else { row.icon };
    let show_icon = !compact || row.flag != '\0' || row.disabled;
    if show_icon { p.text(Pos2::new(rr.min.x + 8.0, cy), Align2::LEFT_CENTER, glyph, font(ui, ICON), icon_col); }
    let amount_col = if row.disabled { t.disabled } else { t.signed(row.value) };
    let mut body = egui::TextStyle::Body.resolve(ui.style());
    if compact { body.size = 12.0; }
    let ar = p.text(Pos2::new(rr.max.x - 5.0, cy), Align2::RIGHT_CENTER, &row.amount, body.clone(), amount_col);
    let text_col = if row.disabled { t.disabled } else { t.text };
    let x0 = rr.min.x + if show_icon { 26.0 } else { 7.0 };
    truncated(p, Pos2::new(x0, cy), &row.text, body, text_col, ar.min.x - 8.0 - x0);
    if hovered {
        let tip = &row.tip;
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_ui(|ui| {
            ui.set_max_width(280.0);
            ui.label(RichTextExt::section(&tip.description, ui));
            if let Some(c) = &tip.category { ui.label(RichTextExt::muted(c, ui)); }
            ui.add_space(4.0);
            let line = |ui: &mut Ui, k: &str, v: String| {
                ui.horizontal(|ui| { ui.label(RichTextExt::muted(k, ui)); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(v)); });
            };
            line(ui, "Amount", money(tip.amount, decimals));
            if (tip.planned - tip.amount).abs() >= 0.005 { line(ui, "Planned", money(tip.planned, decimals)); }
            line(ui, "Balance after", money(tip.balance_after, decimals));
            let status = match (row.disabled, row.flag) {
                (true, _) => Some("Disabled payment"),
                (_, budget_core::FLAG_PAID) => Some("Paid"),
                (_, budget_core::FLAG_IMPORTANT) => Some("Important"),
                _ => None,
            };
            if let Some(s) = status { ui.label(RichTextExt::muted(s, ui)); }
            match tip.suffix {
                '\'' => { ui.label(RichTextExt::muted("Moved off a weekend", ui)); }
                '*' => { ui.label(RichTextExt::muted("Last payment", ui)); }
                _ => {}
            }
            if let Some(n) = &tip.note { ui.add_space(4.0); ui.label(n); }
            ui.add_space(4.0);
            ui.label(RichTextExt::faint("Click to show only this payment", ui));
        });
    }
    resp.clicked()
}

/// Small RichText recipes shared with main.rs.
pub struct RichTextExt;
impl RichTextExt {
    pub fn section(s: &str, _ui: &Ui) -> egui::RichText { egui::RichText::new(s).text_style(ts(SECTION)) }
    pub fn muted(s: &str, ui: &Ui) -> egui::RichText { egui::RichText::new(s).color(ui.visuals().weak_text_color()) }
    pub fn faint(s: &str, ui: &Ui) -> egui::RichText { egui::RichText::new(s).text_style(ts(CAPTION)).color(ui.visuals().weak_text_color()) }
}
