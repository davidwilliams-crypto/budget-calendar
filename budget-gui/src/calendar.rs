//! The month grid: painter-drawn cells (day number, transaction rows, balance strip) with hover and tooltips.

use eframe::egui::{self, text::{LayoutJob, TextWrapping}, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use egui_phosphor::regular as ph;

use crate::theme::{self, font, tint, ts, Tokens, CAPTION, DAY, ICON, SECTION, SEMI};
use budget_core::numfmt::balance_display;
use crate::view::{is_negative, money, BalKind, CellData, MonthView, TranRow};

const WDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const HEADER_H: f32 = 26.0;
const DAY_HEADER_H: f32 = 19.0;
const COMPACT_ROW_H: f32 = 18.0;
const STRIP_H: f32 = 16.0;
const PAD: f32 = 8.0;

/// Stable transaction identity survives redraws and month navigation while dragging.
#[derive(Clone, Copy, Debug)]
pub struct DraggedTransaction { pub id: i32, pub date: i32, pub amount: f64 }

/// All calendar rectangles derive from the allocated area, without a minimum cell height.
#[derive(Clone)]
struct MonthLayout { inner: Rect, header: Rect, grid: Rect, week_bounds: Vec<f32> }

impl MonthLayout {
    fn new(outer: Rect, demand: &[usize]) -> Self {
        let inset = 1.0_f32.min(outer.width() / 2.0).min(outer.height() / 2.0).max(0.0);
        let inner = outer.shrink(inset);
        let header_h = HEADER_H.min(inner.height().max(0.0) / 3.0);
        let header = Rect::from_min_max(inner.min, Pos2::new(inner.max.x, inner.min.y + header_h));
        let grid = Rect::from_min_max(Pos2::new(inner.min.x, header.max.y), inner.max);
        let weeks = demand.len().max(1);
        let base = (DAY_HEADER_H + STRIP_H).min(grid.height().max(0.0) / weeks as f32);
        let mut remaining = ((grid.height() - base * weeks as f32).max(0.0) / COMPACT_ROW_H).floor() as usize;
        let mut rows = vec![0; weeks];
        // Allocate complete, readable rows fairly before giving a busy week a second
        // or later row. Empty weeks leave their space for the populated weeks.
        while remaining > 0 {
            let mut allocated = false;
            for (week, count) in rows.iter_mut().enumerate() {
                if *count < demand.get(week).copied().unwrap_or(0) {
                    *count += 1;
                    remaining -= 1;
                    allocated = true;
                    if remaining == 0 { break; }
                }
            }
            if !allocated { break; }
        }
        let used = base * weeks as f32 + rows.iter().sum::<usize>() as f32 * COMPACT_ROW_H;
        let spare = (grid.height() - used).max(0.0) / weeks as f32;
        let mut week_bounds = Vec::with_capacity(weeks + 1);
        week_bounds.push(grid.min.y);
        for count in rows {
            week_bounds.push(week_bounds.last().copied().unwrap() + base + count as f32 * COMPACT_ROW_H + spare);
        }
        // One final boundary owns the exact bottom edge, including float rounding.
        *week_bounds.last_mut().unwrap() = grid.max.y;
        Self { inner, header, grid, week_bounds }
    }

    fn cell(&self, index: usize) -> Rect {
        let col = index % 7;
        let row = index / 7;
        let x = |c: usize| self.grid.min.x + self.grid.width() * c as f32 / 7.0;
        Rect::from_min_max(Pos2::new(x(col), self.week_bounds[row]), Pos2::new(x(col + 1), self.week_bounds[row + 1]))
    }

    fn index_at(&self, pos: Pos2, cells: usize) -> Option<usize> {
        // Exclusive right/bottom edges prevent an outside drop becoming a nearby date.
        if self.grid.width() <= 0.0 || self.grid.height() <= 0.0 || pos.x < self.grid.min.x ||
            pos.y < self.grid.min.y || pos.x >= self.grid.max.x || pos.y >= self.grid.max.y { return None; }
        let col = (0..=7).map(|c| self.grid.min.x + self.grid.width() * c as f32 / 7.0)
            .take_while(|x| *x <= pos.x).count().saturating_sub(1);
        let row = self.week_bounds.partition_point(|y| *y <= pos.y).saturating_sub(1);
        let index = row * 7 + col;
        (index < cells).then_some(index)
    }
}

#[derive(Clone, Copy)]
struct CellLayout { header: Rect, body: Rect, footer: Rect, row_h: f32, visible: usize }

impl CellLayout {
    fn new(rect: Rect, payments: usize) -> Self {
        // At short heights the date needs a 19px line and the balance a 16px line.
        // Give the date the spare pixels before sacrificing either to transaction rows.
        let height = rect.height().max(0.0);
        let header_h = DAY_HEADER_H.min((height - STRIP_H).max(height * 0.5));
        let footer_h = STRIP_H.min((rect.height() - header_h).max(0.0));
        let header = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + header_h));
        let footer = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - footer_h), rect.max);
        let body = Rect::from_min_max(Pos2::new(rect.min.x, header.max.y), Pos2::new(rect.max.x, footer.min.y));
        let row_h = COMPACT_ROW_H;
        let visible = payments.min(((body.height().max(0.0) + 0.001) / row_h).floor() as usize);
        Self { header, body, footer, row_h, visible }
    }

    fn row(&self, index: usize) -> Rect {
        let inset = 4.0_f32.min(self.body.width().max(0.0) / 2.0);
        let top = self.body.min.y + self.row_h * index as f32;
        Rect::from_min_max(Pos2::new(self.body.min.x + inset, top),
            Pos2::new(self.body.max.x - inset, (top + self.row_h).min(self.body.max.y)))
    }
}

fn truncated(p: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32, max_w: f32) -> f32 {
    truncated_line(p, pos, s, font, color, max_w, None)
}

fn truncated_line(p: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32, max_w: f32, line_height: Option<f32>) -> f32 {
    aligned_line(p, pos, s, font, color, max_w, line_height, false)
}

// Leaf text painter: every argument is a distinct paint attribute, so bundling adds no clarity.
#[allow(clippy::too_many_arguments)]
fn aligned_line(p: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32, max_w: f32, line_height: Option<f32>, right: bool) -> f32 {
    if max_w < 8.0 { return 0.0; }
    let mut job = LayoutJob::simple_singleline(s.to_string(), font, color);
    // Compact names keep the same font size while removing surplus font line leading.
    job.sections[0].format.line_height = line_height;
    job.wrap = TextWrapping::truncate_at_width(max_w);
    let g = p.layout_job(job);
    let w = g.size().x;
    // With reduced leading, font baselines can leave the ink below the logical
    // line. Center the actual glyph mesh so descenders stay inside the row.
    let center_y = if line_height.is_some() && g.mesh_bounds.is_positive() { g.mesh_bounds.center().y } else { g.size().y / 2.0 };
    p.galley(pos - Vec2::new(if right { w } else { 0.0 }, center_y), g, color);
    w
}

/// Small coloured dots that stand in for the old bare apostrophe / asterisk / status markers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker { Paid, Important, Adjusted, Last, Note }

pub const LEGEND: &[(Marker, &str)] = &[
    (Marker::Paid, "Paid: this transaction is marked as paid (dot at the left of the row)"),
    (Marker::Important, "Important: flagged for attention (dot at the left of the row)"),
    (Marker::Adjusted, "Adjusted: the amount or date differs from the regular schedule (changed amount, moved transaction, or moved off a weekend) (dot after the amount)"),
    (Marker::Last, "Last payment of a repeating series (dot after the amount)"),
    (Marker::Note, "The transaction has a note; hover it to read the note (pencil before the name)"),
];

pub fn marker_color(kind: Marker, t: &Tokens) -> Color32 {
    match kind { Marker::Paid => t.paid, Marker::Important => t.important, Marker::Adjusted => t.accent, Marker::Last | Marker::Note => t.text_muted }
}

/// Glyph size of the per-transaction note mark (Phosphor is a fallback of the proportional family).
const NOTE_GLYPH: f32 = 11.0;

fn paint_note_mark(p: &Painter, pos: Pos2, color: Color32) -> Rect {
    p.text(pos, Align2::LEFT_CENTER, ph::NOTE_PENCIL, FontId::proportional(NOTE_GLYPH), color)
}

/// The engine's `'` suffix covers custom amounts, moved transactions and weekend shifts alike
/// (analysis/05: "custom (adjusted) amount"). Only a differing amount is certain from the row.
fn suffix_pill(tip: &crate::view::Tip) -> Option<&'static str> {
    match tip.suffix {
        '\'' if (tip.planned - tip.amount).abs() >= 0.005 => Some("Adjusted amount"),
        '\'' => Some("Adjusted"),
        '*' => Some("Last payment"),
        _ => None,
    }
}

/// A 6px dot with a 1px halo so it stays visible on both light and dark surfaces.
pub fn paint_marker(p: &Painter, pos: Pos2, kind: Marker, t: &Tokens) {
    if kind == Marker::Note { paint_note_mark(p, pos - Vec2::new(NOTE_GLYPH / 2.0, 0.0), marker_color(kind, t)); return; }
    paint_dot(p, pos, marker_color(kind, t), t);
}

fn paint_dot(p: &Painter, pos: Pos2, color: Color32, t: &Tokens) {
    let halo = if t.dark { Color32::from_white_alpha(80) } else { t.surface };
    p.circle_filled(pos, 3.0, color);
    p.circle_stroke(pos, 3.5, Stroke::new(1.0_f32, halo));
}

fn suffix_marker(suffix: char) -> Option<Marker> {
    match suffix { '\'' => Some(Marker::Adjusted), '*' => Some(Marker::Last), _ => None }
}

const GRACE_DAYS_ID: &str = "calendar-grace-days";
const ROW_DETAILS_ID: &str = "calendar-row-details";

/// A click target that is not a Tab stop (a day cell, a balance strip): the keyboard order runs
/// through the payment rows and +N buttons only. `Response::context_menu` re-registers a response
/// with `Sense::click()`, which would make it focusable again, so the response is marked as
/// already sensing that.
fn pointer_only(ui: &Ui, rect: Rect, id: egui::Id) -> egui::Response {
    let mut response = ui.interact(rect, id, Sense::CLICK);
    response.sense |= Sense::FOCUSABLE;
    response
}

pub enum Action {
    Filter(String), Add(i32), Day(i32), EditPayment(i32), DeletePayment(i32),
    EditTransaction { id: i32, date: i32, amount: f64 },
    DeleteTransaction { id: i32, date: i32 },
    MoveTransaction { id: i32, from: i32, to: i32, amount: f64 },
    EditBalance(i32),
    /// Bring back a deleted or moved-away occurrence (05 §1.5 item 5).
    Restore { id: i32, date: i32 },
}

/// Draws the grid and returns a calendar interaction.
pub fn show(ui: &mut Ui, view: &MonthView, t: &Tokens, sunday_first: bool, today: i32, decimals: bool) -> Option<Action> {
    let rows = view.cells.len().div_ceil(7).max(1);
    let size = ui.available_size().max(Vec2::ZERO);
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let demand: Vec<_> = view.cells.chunks(7).map(|week| week.iter().filter(|cell| cell.in_month).map(|cell| cell.rows.len()).max().unwrap_or(0)).collect();
    let layout = MonthLayout::new(outer, &demand);
    let p = ui.painter().with_clip_rect(ui.clip_rect().intersect(outer));
    let mut clicked = None;
    p.rect(outer, theme::RADIUS, t.surface, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
    let cw = layout.grid.width() / 7.0;
    let hair = Stroke::new(1.0_f32, tint(t.surface, t.border, 0.8));
    for c in 0..7 {
        let w = if sunday_first { c } else { (c + 1) % 7 };
        // Full names when there is room, otherwise the 3-letter abbreviation; always small caps-style.
        let name = (if cw >= 120.0 { WDAYS[w] } else { &WDAYS[w][..3] }).to_uppercase();
        let weekend = w == 0 || w == 6;
        let header_cell = Rect::from_min_size(Pos2::new(layout.inner.min.x + cw * c as f32, layout.header.min.y), Vec2::new(cw, layout.header.height()));
        let mut hfont = font(ui, SEMI);
        hfont.size = 11.5;
        let mut job = LayoutJob::simple_singleline(name, hfont, if weekend { t.text_muted } else { t.text });
        job.sections[0].format.extra_letter_spacing = 0.6;
        job.wrap = TextWrapping::truncate_at_width((cw - PAD * 2.0).max(0.0));
        let hp = p.with_clip_rect(header_cell);
        let g = hp.layout_job(job);
        let hcol = if weekend { t.text_muted } else { t.text };
        hp.galley(Pos2::new(header_cell.min.x + PAD, header_cell.center().y - g.size().y / 2.0), g, hcol);
    }
    // Valid payment days of the transaction hovered in the previous frame (01 §3.12, 05 §1.8);
    // rows re-publish them while they stay hovered.
    let grace_id = egui::Id::new(GRACE_DAYS_ID);
    let grace: Vec<i32> = ui.ctx().data_mut(|d| d.remove_temp::<Vec<i32>>(grace_id)).unwrap_or_default();
    // As in the original, restoring is offered only while no (applied) filter narrows the calendar.
    let filtered = view.filtered;
    let dragging = egui::DragAndDrop::has_payload_of_type::<DraggedTransaction>(ui.ctx());
    let changed = balance_changes(view.cells.iter().map(|c| (c.in_month, c.balance.as_ref().map(|b| b.value))));
    for (i, cell) in view.cells.iter().enumerate() {
        let (r, c) = (i / 7, i % 7);
        let rect = layout.cell(i);
        let last = r + 1 == rows;
        let cr = CornerRadius { nw: 0, ne: 0, sw: if last && c == 0 { theme::RADIUS } else { 0 }, se: if last && c == 6 { theme::RADIUS } else { 0 } };
        let resp = pointer_only(ui, rect, ui.id().with(("cell", cell.date)));
        if cell.in_month && !dragging {
            resp.context_menu(|ui| {
                if ui.button("Add payment on this date").clicked() { clicked = Some(Action::Add(cell.date)); ui.close_menu(); }
                if ui.button("View transactions").clicked() { clicked = Some(Action::Day(cell.date)); ui.close_menu(); }
                if !cell.restorable.is_empty() {
                    ui.separator();
                    for (id, desc) in &cell.restorable {
                        let item = ui.add_enabled(!filtered, egui::Button::new(format!("Restore '{desc}'")))
                            .on_disabled_hover_text("Clear the search filter to restore transactions.");
                        if item.clicked() { clicked = Some(Action::Restore { id: *id, date: cell.date }); ui.close_menu(); }
                    }
                }
            });
            // Double-clicking the cell's empty area (rows, the balance strip and the header
            // buttons sense their own clicks) adds a payment on that date.
            if resp.double_clicked() { clicked = Some(Action::Add(cell.date)); }
        }
        let fill = if !cell.in_month { t.other_month }
            else if resp.hovered() { t.surface_hover }
            else if grace.contains(&cell.date) { tint(t.surface, t.important, if t.dark { 0.22 } else { 0.16 }) }
            else if cell.is_today { tint(t.surface, t.today, if t.dark { 0.12 } else { 0.07 }) }
            else if cell.weekend { t.weekend }
            else if cell.date < today { t.past }
            else { t.surface };
        p.rect_filled(rect, cr, fill);
        if cell.is_today && cell.in_month {
            // Today is findable at a glance: 1px inset border plus a 2px bar along the top edge.
            // The fill above is unchanged in geometry, so row layout is unaffected.
            p.rect_stroke(rect, cr, Stroke::new(1.0_f32, tint(fill, t.today, 0.85)), StrokeKind::Inside);
            p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0, t.today);
        }
        if let Some(f) = cell_filter(ui, &p.with_clip_rect(rect), cell, rect, t, today, decimals, &resp, changed[i], filtered) { clicked = Some(f); }
    }
    if ui.ctx().data(|d| d.get_temp::<Vec<i32>>(grace_id)).unwrap_or_default() != grace { ui.ctx().request_repaint(); }
    p.hline(layout.inner.x_range(), layout.grid.min.y - 0.5, hair);
    for r in 1..rows { p.hline(layout.inner.x_range(), layout.cell(r * 7).min.y - 0.5, hair); }
    for c in 1..7 { p.vline(layout.inner.min.x + cw * c as f32 - 0.5, layout.grid.y_range(), hair); }

    // Resolve the target geometrically: rows and footers are children of the same date,
    // so dropping over them must still reach the entire calendar cell.
    if let Some(payload) = egui::DragAndDrop::payload::<DraggedTransaction>(ui.ctx()) {
        let target = ui.ctx().pointer_latest_pos().and_then(|pos| {
            // A lingering informational tooltip must not cover a valid date while dragging.
            // Other windows and popup menus still block calendar drops.
            let over_tooltip = ui.ctx().layer_id_at(pos).is_some_and(|l| l.order == egui::Order::Tooltip);
            if (ui.rect_contains_pointer(layout.grid) || over_tooltip) && ui.clip_rect().contains(pos) {
                layout.index_at(pos, view.cells.len())
            } else { None }
        });
        if let Some(index) = target {
            let cell = &view.cells[index];
            if cell.date != payload.date {
                let rect = layout.cell(index).shrink(2.0);
                p.rect(rect, 4, tint(t.surface, t.accent, 0.12), Stroke::new(2.0_f32, t.accent), StrokeKind::Inside);
            }
        }
        draw_drag_preview(ui, &payload, view, t);
        if ui.input(|i| i.pointer.primary_released()) {
            egui::DragAndDrop::take_payload::<DraggedTransaction>(ui.ctx());
            if let Some(index) = target {
                let to = view.cells[index].date;
                if to != payload.date { clicked = Some(Action::MoveTransaction { id: payload.id, from: payload.date, to, amount: payload.amount }); }
            }
        }
    }
    clicked
}

/// For each cell, whether its day balance moved from the previous in-month day's balance
/// (by at least half a cent). The first in-month balance always counts as changed.
fn balance_changes(cells: impl Iterator<Item = (bool, Option<f64>)>) -> Vec<bool> {
    let mut prev: Option<f64> = None;
    cells.map(|(in_month, value)| match (in_month, value) {
        (true, Some(v)) => {
            let changed = prev.is_none_or(|p| (v - p).abs() >= 0.005);
            prev = Some(v);
            changed
        }
        _ => true,
    }).collect()
}

// Same parameter set as the sibling cell painters; a context struct would be used by this one only.
#[allow(clippy::too_many_arguments)]
fn cell_filter(ui: &mut Ui, p: &Painter, cell: &CellData, rect: Rect, t: &Tokens, today: i32, decimals: bool, resp: &egui::Response, changed: bool, filtered: bool) -> Option<Action> {
    let mut clicked = None;
    let layout = CellLayout::new(rect, cell.rows.len());
    let hp = p.with_clip_rect(layout.header);
    // day number
    let day_pos = Pos2::new(rect.min.x + PAD + 8.0, layout.header.center().y);
    if cell.is_today && cell.in_month {
        // A 17px pill fits the 19px header (a circle would be clipped) and widens for two digits.
        let mut f = bold(font(ui, DAY));
        f.size = 12.5;
        let fg = if t.dark { t.bg } else { Color32::WHITE };
        let g = hp.layout_no_wrap(cell.day.to_string(), f, fg);
        let pill = Rect::from_min_size(Pos2::new(rect.min.x + PAD - 3.0, day_pos.y - 8.5), Vec2::new(17.0_f32.max(g.size().x + 10.0), 17.0));
        hp.rect_filled(pill, 9, t.today);
        hp.galley(pill.center() - g.size() / 2.0, g, fg);
    } else {
        // Light mode lets payment names (14px, full ink) dominate; day numbers recede to muted.
        let col = if !cell.in_month { t.text_faint } else if cell.date < today || !t.dark { t.text_muted } else { t.text };
        hp.text(Pos2::new(rect.min.x + PAD, day_pos.y), Align2::LEFT_CENTER, cell.day.to_string(), font(ui, DAY), col);
    }
    if !cell.in_month { return None; }
    // top-right indicators
    let mut ix = rect.max.x - PAD - 2.0;
    let hidden = cell.rows.len() - layout.visible;
    if hidden > 0 {
        let count_w = (rect.width() - 35.0).clamp(0.0, 42.0);
        let more = Rect::from_min_max(Pos2::new(rect.max.x - 4.0 - count_w, layout.header.min.y), Pos2::new(rect.max.x - 4.0, layout.header.max.y));
        // With a search active the rows (and so +N) are the matching transactions only; the day
        // dialog lists those first, then the day's other transactions.
        let (more_text, info) = if filtered {
            (format!("{hidden} more matching transactions. View this date's transactions, matches first."),
                format!("View the {} matching transactions on {}", cell.rows.len(), cell.date))
        } else {
            (format!("{hidden} more transactions. View all transactions on this date."),
                format!("View all {} transactions on {}", cell.rows.len(), cell.date))
        };
        let response = ui.interact(more, ui.id().with(("more", cell.date)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(more_text);
        let label = format!("+{hidden}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, info.clone()));
        if response.hovered() || response.has_focus() { hp.rect_filled(more, 4, tint(t.surface, t.accent, 0.08)); }
        if response.has_focus() { hp.rect_stroke(more, 4, Stroke::new(1.0_f32, t.accent), StrokeKind::Inside); }
        truncated(&hp.with_clip_rect(more), Pos2::new(more.min.x + 3.0, more.center().y), &label, font(ui, SEMI), t.accent, (more.width() - 6.0).max(0.0));
        if response.clicked() { clicked = Some(Action::Day(cell.date)); }
        ix = more.min.x - 5.0;
    }
    let icon = font(ui, ICON);
    if cell.note.is_some() && ix - 17.0 >= rect.min.x + 35.0 {
        let r = hp.text(Pos2::new(ix, day_pos.y), Align2::RIGHT_CENTER, ph::NOTE_PENCIL, icon.clone(), t.text_muted);
        ix = r.min.x - 4.0;
    }
    let restorable = !cell.restorable.is_empty();
    if restorable && ix - 17.0 >= rect.min.x + 35.0 {
        let r = hp.text(Pos2::new(ix, day_pos.y), Align2::RIGHT_CENTER, ph::ARROW_COUNTER_CLOCKWISE, icon.clone(), t.text_faint);
        ix = r.min.x - 4.0;
    }
    // A hover-only "+" makes adding a payment on this date discoverable without the context menu.
    let dragging = egui::DragAndDrop::has_payload_of_type::<DraggedTransaction>(ui.ctx());
    if !dragging && resp.contains_pointer() && ix - 17.0 >= rect.min.x + 35.0 {
        let add = Rect::from_center_size(Pos2::new(ix - 8.5, day_pos.y), Vec2::splat(17.0));
        let date = budget_core::date::sdate(cell.date);
        let aresp = ui.interact(add, ui.id().with(("add", cell.date)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Add payment on {date}"));
        aresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Add payment on {date}")));
        if aresp.hovered() || aresp.has_focus() { hp.rect_filled(add, 4, tint(t.surface, t.accent, 0.08)); }
        hp.text(add.center(), Align2::CENTER_CENTER, ph::PLUS, icon, t.accent);
        if aresp.clicked() { clicked = Some(Action::Add(cell.date)); }
    }
    if resp.hovered() && (cell.note.is_some() || restorable) {
        resp.clone().on_hover_ui(|ui| {
            if let Some(n) = &cell.note { ui.label(RichTextExt::section(n, ui)); }
            if restorable {
                let names: Vec<&str> = cell.restorable.iter().map(|(_, d)| d.as_str()).collect();
                ui.label(RichTextExt::muted(&format!("Deleted or moved: {}. Right-click the day to restore.", names.join(", ")), ui));
            }
        });
    }
    // transaction rows
    for (index, row) in cell.rows[..layout.visible].iter().enumerate() {
        let rr = layout.row(index);
        if let Some(action) = draw_row(ui, p, row, rr, t, decimals) { clicked = Some(action); }
    }
    // balance strip
    if let Some(b) = &cell.balance {
        let sr = layout.footer;
        let fp = p.with_clip_rect(sr);
        // Ordinary days paint only the number; the few days that matter (lowest, highest, below the
        // minimum) get a labelled, fully tinted strip so they become the accents of the grid.
        let (strip_bg, strip_fg, label) = match b.kind {
            BalKind::Max => (t.max.0, t.max.1, Some("Max")),
            BalKind::Min => (t.min.0, t.min.1, Some("Min")),
            BalKind::Low => (t.low.0, t.low.1, Some("Low")),
            // A repeated balance recedes to faint; a day whose balance moved reads in full ink.
            BalKind::Normal => (tint(t.surface, t.bal.0, 0.5), if changed { t.text } else { t.text_faint }, None),
        };
        fp.rect_filled(sr, 0, strip_bg);
        fp.hline((sr.min.x + 4.0)..=(sr.max.x - 4.0), sr.min.y, Stroke::new(1.0_f32, tint(t.surface, t.border, 0.7)));
        let cy = sr.center().y;
        let fg = if is_negative(b.value, decimals) { t.negative } else { strip_fg };
        let fid = font(ui, CAPTION);
        let amount_fid = if b.kind == BalKind::Normal && changed { bold(fid.clone()) } else { fid.clone() };
        let mut amount = fp.layout_no_wrap(b.strip_text(decimals), amount_fid.clone(), fg);
        // The "(adjustment)" part gives way before the balance digits would be clipped.
        if b.adjusted && sr.max.x - 6.0 - amount.size().x < sr.min.x + 6.0 {
            amount = fp.layout_no_wrap(format!("{}\u{2032}", b.text), amount_fid, fg);
        }
        let amount_left = sr.max.x - 6.0 - amount.size().x;
        if let Some(label) = label {
            let label_width = fp.layout_no_wrap(label.to_string(), fid.clone(), strip_fg).size().x;
            let label_x = sr.min.x + 15.0;
            if label_x + label_width + 8.0 <= amount_left {
                if b.kind == BalKind::Low {
                    fp.text(Pos2::new(sr.min.x + 6.0, cy), Align2::LEFT_CENTER, ph::WARNING, fid.clone(), strip_fg);
                } else {
                    fp.circle_filled(Pos2::new(sr.min.x + 9.0, cy), 2.5, strip_fg);
                }
                fp.text(Pos2::new(label_x, cy), Align2::LEFT_CENTER, label, fid, strip_fg);
            }
        }
        fp.galley(Pos2::new(amount_left, cy - amount.size().y / 2.0), amount, fg);
        let sresp = pointer_only(ui, sr, ui.id().with(("strip", cell.date)));
        sresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Closing balance {} on {}{}. Right-click to edit balance", money(b.value, true), cell.date,
            if b.adjusted { format!(", adjusted by {}", money(b.adjustment, true)) } else { String::new() })));
        if !egui::DragAndDrop::has_payload_of_type::<DraggedTransaction>(ui.ctx()) {
            sresp.context_menu(|ui| {
                if ui.button("Edit balance").clicked() { clicked = Some(Action::EditBalance(cell.date)); ui.close_menu(); }
            });
        }
        if sresp.hovered() {
            sresp.on_hover_ui(|ui| {
                ui.label(RichTextExt::section(&format!("Closing balance {}", money(b.value, true)), ui));
                let what = match b.kind { BalKind::Max => "Highest balance of the month", BalKind::Min => "Lowest balance of the month", BalKind::Low => "Below your minimum balance", BalKind::Normal => "" };
                if !what.is_empty() { ui.label(RichTextExt::muted(what, ui)); }
                if b.adjusted {
                    ui.label(RichTextExt::muted(&format!("Before adjustment {}", money(b.value - b.adjustment, true)), ui));
                    ui.label(RichTextExt::muted(&format!("Adjustment {}", money(b.adjustment, true)), ui));
                }
            });
        }
    }
    clicked
}

fn draw_row(ui: &mut Ui, p: &Painter, row: &TranRow, rr: Rect, t: &Tokens, decimals: bool) -> Option<Action> {
    let p = p.with_clip_rect(rr);
    let resp = ui.interact(rr, ui.id().with(("row", row.date, row.payment_id)), Sense::click_and_drag());
    resp.dnd_set_drag_payload(DraggedTransaction { id: row.payment_id, date: row.date, amount: row.value });
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("calendar-drag-name"), row.text.clone()));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} {}{}. Click to edit; drag to move transaction",
        row.text, row.amount, if row.tip.note.is_some() { ", has a note" } else { "" })));
    let mut action = None;
    // A right-click during a transaction drag cancels the drag (egui ends it on any release); it must not
    // also open the menu of whatever row lies under the pointer.
    if !egui::DragAndDrop::has_payload_of_type::<DraggedTransaction>(ui.ctx()) { resp.context_menu(|ui| {
        if ui.button("Edit transaction (this date)").clicked() { action = Some(Action::EditTransaction { id:row.payment_id, date:row.date, amount:row.value }); ui.close_menu(); }
        if ui.button("Edit whole payment").clicked() { action = Some(Action::EditPayment(row.payment_id)); ui.close_menu(); }
        if ui.button("Filter this payment").clicked() { action = Some(Action::Filter(row.filter.clone())); ui.close_menu(); }
        ui.separator();
        if ui.button("Delete transaction (this date)").clicked() { action = Some(Action::DeleteTransaction { id:row.payment_id, date:row.date }); ui.close_menu(); }
        if ui.button("Delete whole payment").clicked() { action = Some(Action::DeletePayment(row.payment_id)); ui.close_menu(); }
    }); }
    let hovered = resp.hovered();
    let focused = resp.has_focus();
    // Disabled rows keep their highlight colour (05 §1.6) at a lighter tint next to the muted text.
    if let Some(h) = row.hilite { p.rect_filled(rr, 4, tint(t.surface, h, if row.disabled { 0.07 } else { 0.14 })); }
    if hovered || focused { p.rect_filled(rr, 4, tint(t.surface, t.accent, 0.08)); }
    // Keyboard focus (Tab; Enter or Space edits) gets the 2px accent ring the buttons use, drawn
    // inside the row so its geometry is unchanged.
    if focused { p.rect_stroke(rr, 4, Stroke::new(2.0_f32, t.accent), StrokeKind::Inside); }
    let name_y = rr.center().y;
    let status = match row.flag {
        budget_core::FLAG_PAID => Some(t.paid),
        budget_core::FLAG_IMPORTANT => Some(t.important),
        _ => None,
    };
    let income = !is_negative(row.value, decimals) && row.value > 0.0;
    let amount_col = if row.disabled { t.disabled } else if is_negative(row.value, decimals) { t.negative } else { t.positive };
    // A 3px rounded accent bar marks income rows, so paydays stand out and polarity does not
    // depend on colour alone (expenses carry the U+2212 minus). Name positions stay fixed.
    if income && rr.width() >= 16.0 {
        let bar_color = if row.disabled { tint(t.surface, amount_col, 0.4) } else { amount_col };
        let bar = Rect::from_min_max(Pos2::new(rr.min.x, rr.min.y + 3.0), Pos2::new(rr.min.x + 3.0, rr.max.y - 3.0));
        p.rect_filled(bar, 1.5, bar_color);
    }
    let mut x0 = rr.min.x + 9.0;
    if let Some(color) = status {
        let color = if row.disabled { t.disabled } else { color };
        paint_dot(&p, Pos2::new(rr.min.x + 9.0, name_y), color, t);
        x0 += 7.0;
    }
    // Notes are discoverable at a glance (05 §1.6 marks them with a `...` prefix).
    if row.tip.note.is_some() && rr.width() >= 60.0 {
        let mark = paint_note_mark(&p, Pos2::new(x0, name_y), if row.disabled { t.disabled } else { t.text_muted });
        x0 = mark.max.x + 3.0;
    }
    let text_col = if row.disabled { t.disabled } else { t.text };
    let right = (rr.max.x - 6.0).max(x0);
    // The trailing marker dot sits inside the amount column's right edge.
    let marker = suffix_marker(row.tip.suffix);
    let marker_w = if marker.is_some() && right - 9.0 >= x0 + 20.0 { 9.0 } else { 0.0 };
    if marker_w > 0.0 {
        if let Some(m) = marker {
            let c = if row.disabled { t.disabled } else { marker_color(m, t) };
            paint_dot(&p, Pos2::new(right - 3.5, name_y), c, t);
        }
    }
    let right = right - marker_w;
    let text_width = (right - x0).max(0.0);
    // The amount is the figure people scan for, so it carries the weight; the name is regular.
    let mut amount_font = font(ui, SEMI);
    amount_font.size = 13.0;
    // Leave a pixel on either side for glyph ink extending beyond its advance.
    let amount_limit = (text_width - 45.0).max(text_width * 0.48);
    let amount_w = (p.layout_no_wrap(row.amount.clone(), amount_font.clone(), amount_col).size().x + 2.0).max(10.0).min(amount_limit);
    let amount_rect = Rect::from_min_max(Pos2::new(right - amount_w, rr.min.y), Pos2::new(right, rr.max.y));
    let name_rect = Rect::from_min_max(Pos2::new(x0, rr.min.y), Pos2::new((amount_rect.min.x - 5.0).max(x0), rr.max.y));
    let name_font = egui::TextStyle::Body.resolve(ui.style());
    truncated_line(&p.with_clip_rect(name_rect), Pos2::new(x0, name_y), &row.text, name_font,
        text_col, name_rect.width(), Some(rr.height()));
    aligned_line(&p.with_clip_rect(amount_rect), Pos2::new(right - 1.0, name_y), &row.amount, amount_font,
        amount_col, (amount_rect.width() - 2.0).max(0.0), Some(rr.height()), true);
    if hovered && !ui.input(|i| i.pointer.primary_down()) && !egui::DragAndDrop::has_payload_of_type::<DraggedTransaction>(ui.ctx()) {
        let tip = &row.tip;
        if !row.valid_days.is_empty() {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(GRACE_DAYS_ID), row.valid_days.clone()));
        }
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
        let pos = ui.ctx().pointer_latest_pos().unwrap_or(rr.left_bottom()) + Vec2::new(12.0, 12.0);
        // These details contain no controls. Let clicks pass through even when
        // egui constrains a newly sized tooltip over the payment being hovered.
        // One shared id: egui never prunes Area layers, so a per-row id would retain a layer for
        // every row ever hovered. Only one card is shown at a time.
        egui::Area::new(egui::Id::new(ROW_DETAILS_ID)).order(egui::Order::Tooltip).interactable(false)
            .fixed_pos(pos).default_size(Vec2::new(280.0, 150.0)).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(280.0);
                    ui.label(egui::RichText::new(format!("{}  {}", row.icon, tip.description)).text_style(ts(SECTION)).strong());
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(c) = &tip.category { pill(ui, c, t.text_muted, t); }
                        if row.disabled { pill(ui, "Disabled", t.disabled, t); }
                        match row.flag {
                            budget_core::FLAG_PAID => pill(ui, "Paid", t.paid, t),
                            budget_core::FLAG_IMPORTANT => pill(ui, "Important", t.important, t),
                            _ => {}
                        }
                        if let Some(text) = suffix_pill(tip) {
                            pill(ui, text, if tip.suffix == '*' { t.text_muted } else { t.accent }, t);
                        }
                    });
                    ui.add_space(4.0);
                    let line = |ui: &mut Ui, k: &str, v: egui::RichText| {
                        ui.horizontal(|ui| { ui.label(RichTextExt::muted(k, ui)); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(v)); });
                    };
                    line(ui, "Amount", egui::RichText::new(money(tip.amount, decimals)).strong().color(amount_col));
                    if (tip.planned - tip.amount).abs() >= 0.005 { line(ui, "Planned", egui::RichText::new(money(tip.planned, decimals))); }
                    line(ui, "Balance after", egui::RichText::new(money(balance_display(tip.balance_after, decimals), decimals)));
                    if let (Some(first), Some(last)) = (row.valid_days.first(), row.valid_days.last()) {
                        line(ui, "Can be paid", egui::RichText::new(format!("{} to {}", budget_core::date::sdate(*first), budget_core::date::sdate(*last))));
                    }
                    if let Some(n) = &tip.note { ui.add_space(4.0); ui.label(n); }
                    ui.add_space(4.0);
                    ui.label(RichTextExt::faint("Click to edit. Drag to move this transaction. Right-click for payment actions.", ui));
                });
            });
    }
    if resp.clicked() { action = Some(Action::EditTransaction { id:row.payment_id, date:row.date, amount:row.value }); }
    action
}

/// A compact rounded status badge: coloured text on a faint tint of the same colour.
fn pill(ui: &mut Ui, text: &str, color: Color32, t: &Tokens) {
    let fill = tint(t.surface, color, 0.16);
    egui::Frame::new().fill(fill).corner_radius(8).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).text_style(ts(CAPTION)).color(color).strong());
    });
}

fn bold(mut f: FontId) -> FontId { f.family = egui::FontFamily::Name("semi".into()); f }

fn draw_drag_preview(ui: &Ui, payload: &DraggedTransaction, view: &MonthView, t: &Tokens) {
    let Some(pointer) = ui.ctx().pointer_latest_pos() else { return; };
    let name = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::new("calendar-drag-name")))
        .or_else(|| view.cells.iter().flat_map(|c| &c.rows).find(|r| r.payment_id == payload.id && r.date == payload.date).map(|r| r.text.clone()))
        .unwrap_or_else(|| "Move transaction".into());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("calendar-transaction-drag")));
    let screen = ui.ctx().screen_rect();
    let size = Vec2::new(230.0_f32.min(screen.width()), 53.0_f32.min(screen.height()));
    let min = (pointer + Vec2::new(14.0, 16.0)).min(screen.max - size).max(screen.min);
    let rect = Rect::from_min_size(min, size);
    painter.rect(rect, 6, t.surface, Stroke::new(1.0_f32, t.accent), StrokeKind::Inside);
    let p = painter.with_clip_rect(rect.shrink(5.0));
    truncated(&p, Pos2::new(rect.min.x + 9.0, rect.min.y + 15.0), &name, font(ui, SEMI), t.text, rect.width() - 18.0);
    truncated(&p, Pos2::new(rect.min.x + 9.0, rect.min.y + 36.0), &format!("{} · drop on a date", money(payload.amount, true)), font(ui, CAPTION), t.text_muted, rect.width() - 18.0);
}

/// Small RichText recipes shared with main.rs.
pub struct RichTextExt;
impl RichTextExt {
    pub fn section(s: &str, _ui: &Ui) -> egui::RichText { egui::RichText::new(s).text_style(ts(SECTION)) }
    pub fn muted(s: &str, ui: &Ui) -> egui::RichText { egui::RichText::new(s).color(ui.visuals().weak_text_color()) }
    pub fn faint(s: &str, ui: &Ui) -> egui::RichText { egui::RichText::new(s).text_style(ts(CAPTION)).color(ui.visuals().weak_text_color()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_days_whose_balance_moves_are_marked_changed() {
        // Leading adjacent-month day, then 100, 100, 100.004 (below half a cent), 90, 90, trailing day.
        let cells = [(false, Some(5.0)), (true, Some(100.0)), (true, Some(100.0)), (true, Some(100.004)),
            (true, Some(90.0)), (true, None), (true, Some(90.0)), (false, Some(1.0))];
        assert_eq!(super::balance_changes(cells.into_iter()), [true, true, false, false, true, true, false, true]);
        assert!(super::balance_changes(std::iter::empty()).is_empty());
    }

    #[test]
    fn suffix_markers_map_to_dots() {
        assert_eq!(super::suffix_marker('\''),Some(super::Marker::Adjusted));
        assert_eq!(super::suffix_marker('*'), Some(super::Marker::Last));
        assert_eq!(super::suffix_marker(' '), None);
        assert_eq!(super::LEGEND.len(), 5);
    }

    #[test]
    fn adjusted_amounts_are_not_described_as_weekend_moves() {
        // The engine's `'` also marks every custom amount, e.g. 35 instead of 100 on a Thursday.
        let tip = |amount: f64, suffix: char| crate::view::Tip { description: String::new(), category: None, amount,
            planned: -100.0, balance_after: 0.0, note: None, suffix };
        assert_eq!(super::suffix_pill(&tip(-35.0, '\'')), Some("Adjusted amount"));
        assert_eq!(super::suffix_pill(&tip(-100.0, '\'')), Some("Adjusted"));
        assert_eq!(super::suffix_pill(&tip(-100.0, '*')), Some("Last payment"));
        assert_eq!(super::suffix_pill(&tip(-100.0, '\0')), None);
        let (_, legend) = super::LEGEND.iter().find(|(m, _)| *m == super::Marker::Adjusted).unwrap();
        assert!(legend.contains("changed amount") && !legend.starts_with("Moved"), "{legend}");
    }

    use super::*;
    use crate::view::{Balance, QuickStats, Tip};

    fn fixture(weeks: usize, payments: usize) -> MonthView {
        let cells = (0..weeks * 7).map(|i| {
            let mut date = budget_core::Cal::from_int(20261001);
            date.add(budget_core::Field::Day, i as i32);
            CellData {
                day: date.day(), date: date.get(), in_month: true, is_today: false,
                weekend: false, note: None, restorable: Vec::new(),
                rows: (0..payments).map(|n| TranRow {
                    payment_id: n as i32 + 10, date: date.get(), icon: "", text: format!("Payment name {n}"),
                    amount: "123.45".into(), value: 123.45, flag: '\0', hilite: None,
                    disabled: false, filter: String::new(),
                    tip: Tip { description: format!("Payment name {n}"), category: None, amount: 123.45,
                        planned: 123.45, balance_after: 1000.0, note: None, suffix: '\0' },
                    valid_days: Vec::new(),
                }).collect(),
                balance: Some(Balance { text: format!("B{}", date.get()), value: 1000.0, kind: BalKind::Normal, adjusted: false, adjustment: 0.0 }),
            }
        }).collect();
        MonthView { key: (20261001, String::new(), 0), cells, shown: payments * weeks * 7,
            total: payments * weeks * 7, brief: Vec::new(), below_min: false, shares_of_expenses: false, filtered: false,
            quick: QuickStats { opening: 0.0, closing: 0.0, min: 0.0, max: 0.0, avg: 0.0, income: 0.0, expenses: 0.0 } }
    }

    fn context() -> (egui::Context, Tokens) {
        let ctx = egui::Context::default();
        let t = Tokens::new(&budget_core::config::BudgetConfig::default().colors, false);
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &t);
        (ctx, t)
    }

    fn frame(ctx: &egui::Context, t: &Tokens, view: &MonthView, size: Vec2, events: Vec<egui::Event>) -> (egui::FullOutput, Option<Action>) {
        let mut action = None;
        let output = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), events, ..Default::default() }, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                action = show(ui, view, t, true, 20261001, true);
            });
        });
        (output, action)
    }

    fn text_shapes(output: &egui::FullOutput) -> Vec<(&egui::epaint::TextShape, Rect)> {
        output.shapes.iter().filter_map(|s| match &s.shape {
            egui::Shape::Text(text) => Some((text, s.clip_rect)), _ => None,
        }).collect()
    }

    #[test]
    fn entire_month_dates_and_balances_fit_the_available_calendar() {
        // Removing bounded allocation or reintroducing a minimum cell height must fail this.
        for weeks in [4, 5, 6] {
            for size in [Vec2::new(620.0, 240.0), Vec2::new(860.0, 310.0), Vec2::new(884.0, 347.0), Vec2::new(860.0, 386.0), Vec2::new(1200.0, 540.0)] {
                let (ctx, t) = context();
                let view = fixture(weeks, 3);
                let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
                let shapes = text_shapes(&output);
                let balances: Vec<_> = shapes.iter().filter(|(s, _)| s.galley.job.text.starts_with('B') && s.galley.job.text != "Balance").collect();
                assert_eq!(balances.len(), weeks * 7);
                for (text, clip) in balances {
                    let actual = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(actual.min.y >= -0.5 && actual.max.y <= size.y + 0.5,
                        "{weeks} weeks at {size:?}: balance {:?} is outside viewport: {actual:?}", text.galley.job.text);
                    assert!(clip.min.y <= actual.min.y && clip.max.y >= actual.max.y,
                        "balance {:?} is clipped vertically: {actual:?} in {clip:?}", text.galley.job.text);
                }
                let dates: Vec<_> = shapes.iter().filter(|(s, _)| !s.galley.job.text.is_empty() && s.galley.job.text.chars().all(|c| c.is_ascii_digit())).collect();
                assert_eq!(dates.len(), weeks * 7);
                for (text, clip) in dates {
                    let actual = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(actual.min.y >= -0.5 && actual.max.y <= size.y + 0.5);
                    assert!(clip.min.y - 0.01 <= actual.min.y && clip.max.y + 0.01 >= actual.max.y, "{weeks} weeks at {size:?}: date {} bounds {actual:?} in {clip:?}", text.galley.job.text);
                }
            }
        }
    }

    #[test]
    fn payment_name_and_amount_share_a_readable_row_in_separate_columns() {
        // Two-line cards or a shared clip that lets amounts overlap names fail this.
        let (ctx, t) = context();
        let (output, _) = frame(&ctx, &t, &fixture(4, 1), Vec2::new(860.0, 540.0), Vec::new());
        let shapes = text_shapes(&output);
        let (name, name_clip) = shapes.iter().find(|(s, _)| s.galley.job.text == "Payment name 0").unwrap();
        let (amount, amount_clip) = shapes.iter().find(|(s, _)| s.galley.job.text == "123.45").unwrap();
        let name_ink = name.galley.mesh_bounds.translate(name.pos.to_vec2());
        let amount_ink = amount.galley.mesh_bounds.translate(amount.pos.to_vec2());
        assert!((name_ink.center().y - amount_ink.center().y).abs() < 0.5, "name and amount must share one line");
        assert!(name_clip.max.x <= amount_clip.min.x, "name and amount must have disjoint clips");
        assert!(name_clip.height() <= 18.01);
        assert!(name.galley.job.sections[0].format.font_id.size >= 14.0);
        assert!(amount.galley.job.sections[0].format.font_id.size >= 13.0);
        assert!(amount_clip.expand(0.01).contains_rect(amount_ink), "ordinary amounts remain fully readable: {amount_ink:?} in {amount_clip:?}");
    }

    #[test]
    fn ordinary_five_week_budget_shows_all_four_or_five_payments_per_day() {
        // Uniform week heights or two-line rows hide payments in this realistic allocation.
        let (ctx, t) = context();
        let mut view = fixture(5, 5);
        for (week, count) in [4, 5, 5, 5, 4].into_iter().enumerate() {
            for cell in &mut view.cells[week * 7..(week + 1) * 7] { cell.rows.truncate(count); }
        }
        let (output, _) = frame(&ctx, &t, &view, Vec2::new(1176.0, 630.0), Vec::new());
        let shapes = text_shapes(&output);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "123.45").count(), 161,
            "all payments and their amounts must be visible at the standard window size");
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "Payment name 4").count(), 21);
        assert!(!shapes.iter().any(|(s, _)| s.galley.job.text.starts_with('+')));
    }

    #[test]
    fn busy_weeks_use_space_that_empty_weeks_do_not_need() {
        // Giving every week the same height loses rows even though the month has spare space.
        let (ctx, t) = context();
        let mut view = fixture(5, 8);
        for (week, count) in [1, 8, 0, 5, 2].into_iter().enumerate() {
            for cell in &mut view.cells[week * 7..(week + 1) * 7] { cell.rows.truncate(count); }
        }
        let (output, _) = frame(&ctx, &t, &view, Vec2::new(1176.0, 630.0), Vec::new());
        let shapes = text_shapes(&output);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "123.45").count(), 112);
        assert!(!shapes.iter().any(|(s, _)| s.galley.job.text.starts_with('+')));
    }

    #[test]
    fn crowded_day_keeps_overflow_in_header_above_payments() {
        // Reserving a transaction row for +N must fail this.
        let (ctx, t) = context();
        let (output, _) = frame(&ctx, &t, &fixture(4, 20), Vec2::new(860.0, 540.0), Vec::new());
        let shapes = text_shapes(&output);
        let (name, _) = shapes.iter().find(|(s, _)| s.galley.job.text == "Payment name 0").unwrap();
        let (more, _) = shapes.iter().find(|(s, _)| s.galley.job.text.starts_with('+')).unwrap();
        assert!(more.galley.mesh_bounds.translate(more.pos.to_vec2()).max.y <= name.galley.mesh_bounds.translate(name.pos.to_vec2()).min.y + 0.5);
    }

    #[test]
    fn compact_six_week_month_keeps_readable_names_between_header_and_footer() {
        let (ctx, t) = context();
        // Native 900x600 at 150% DPI leaves this logical calendar allocation.
        let (output, _) = frame(&ctx, &t, &fixture(6, 4), Vec2::new(884.0, 347.0), Vec::new());
        let shapes = text_shapes(&output);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "Payment name 0").count(), 42,
            "every populated date must expose a name in the actual compact calendar area");
        for (text, clip) in shapes.iter().filter(|(s, _)| s.galley.job.text == "Payment name 0") {
            let ink = text.galley.mesh_bounds.translate(text.pos.to_vec2());
            assert!(clip.min.y - 0.01 <= ink.min.y && clip.max.y + 0.01 >= ink.max.y,
                "compact 14px payment glyphs must fit without clipping: {ink:?} in {clip:?}");
        }
        let (name, name_clip) = shapes.iter().find(|(s, _)| s.galley.job.text == "Payment name 0").expect("compact month must show one name per date");
        let (overflow, _) = shapes.iter().find(|(s, _)| s.galley.job.text == "+3").unwrap();
        let (balance, _) = shapes.iter().find(|(s, _)| s.galley.job.text == "B20261001").unwrap();
        let name_ink = name.galley.mesh_bounds.translate(name.pos.to_vec2());
        assert!(overflow.pos.y + overflow.galley.size().y <= name_ink.min.y + 0.01);
        assert!(name_ink.max.y <= balance.pos.y + 0.01);
        assert!(name.galley.job.sections[0].format.font_id.size >= 14.0);
        assert!(name_clip.width() > 40.0);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "123.45").count(), 42,
            "every compact payment must also show its amount on the same row");
    }

    #[test]
    fn huge_amounts_and_long_names_have_bounded_separate_columns() {
        // Unbounded amount reservation can erase the name or paint over adjacent dates.
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        view.cells[0].rows[0].text = "A very long payment name with useful detail".into();
        view.cells[0].rows[0].amount = "999,999,999,999,999,999.99".into();
        let (output, _) = frame(&ctx, &t, &view, Vec2::new(884.0, 430.0), Vec::new());
        let shapes = text_shapes(&output);
        let (name, name_clip) = shapes.iter().find(|(s, _)| s.galley.job.text.starts_with("A very long")).unwrap();
        let (amount, amount_clip) = shapes.iter().find(|(s, _)| s.galley.job.text.starts_with("999,999")).unwrap();
        assert!(name_clip.width() >= 40.0, "names retain a readable column");
        assert!(name_clip.max.x <= amount_clip.min.x && amount_clip.max.x < 127.0);
        assert!(name.galley.elided && amount.galley.elided, "both columns truncate independently");
        let ink = amount.galley.mesh_bounds.translate(amount.pos.to_vec2());
        assert!(amount_clip.expand(0.01).contains_rect(ink), "{ink:?} in {amount_clip:?}");
    }

    #[test]
    fn ordinary_signed_amounts_stay_complete_when_a_readable_name_also_fits() {
        // A fixed half-width amount column needlessly truncates routine mortgage values.
        for (width, value) in [(884.0, "−1,234.56"), (1176.0, "−123,456.78")] {
            let (ctx, t) = context();
            let mut view = fixture(5, 5);
            view.cells[0].rows[0].amount = value.into();
            let (output, _) = frame(&ctx, &t, &view, Vec2::new(width, 630.0), Vec::new());
            let shapes = text_shapes(&output);
            let (name, name_clip) = shapes.iter().find(|(s, _)| s.galley.job.text == "Payment name 0").unwrap();
            let (amount, amount_clip) = shapes.iter().find(|(s, _)| s.galley.job.text == value).unwrap();
            assert!(!amount.galley.elided, "{value} should remain complete at {width}px with space for the name");
            assert!(name_clip.width() >= 40.0 && name_clip.max.x <= amount_clip.min.x);
            assert!(name.galley.job.sections[0].format.font_id.size >= 14.0);
        }
    }

    #[test]
    fn zero_and_single_digit_amounts_are_visible_in_their_columns() {
        // A tiny measured amount must not fall below the renderer's minimum text width.
        for value in ["0", "7"] {
            let (ctx, t) = context();
            let mut view = fixture(5, 1);
            view.cells[0].rows[0].amount = value.into();
            let (output, _) = frame(&ctx, &t, &view, Vec2::new(1176.0, 630.0), Vec::new());
            let shapes = text_shapes(&output);
            let (amount, clip) = shapes.iter().find(|(s, _)| s.galley.job.text == value &&
                s.galley.job.sections[0].format.font_id.size == 13.0)
                .unwrap_or_else(|| panic!("single-digit amount {value} must be painted"));
            assert!(!amount.galley.elided);
            let ink = amount.galley.mesh_bounds.translate(amount.pos.to_vec2());
            assert!(clip.expand(0.01).contains_rect(ink));
        }
    }

    #[test]
    fn cramped_calendar_gives_busy_weeks_multiple_rows_before_later_rows() {
        // Proportional shrinking wastes fractional row space; first-week priority starves later weeks.
        let (ctx, t) = context();
        let mut view = fixture(5, 5);
        for (week, count) in [4, 5, 5, 5, 4].into_iter().enumerate() {
            for cell in &mut view.cells[week * 7..(week + 1) * 7] { cell.rows.truncate(count); }
        }
        let (output, _) = frame(&ctx, &t, &view, Vec2::new(884.0, 430.0), Vec::new());
        let shapes = text_shapes(&output);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "123.45").count(), 84);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "Payment name 1").count(), 35);
        assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == "Payment name 2").count(), 14);
        for (label, count) in [("+1", 7), ("+2", 14), ("+3", 14)] {
            assert_eq!(shapes.iter().filter(|(s, _)| s.galley.job.text == label).count(), count);
        }
    }

    #[test]
    fn unequal_week_boundaries_drive_hit_testing_including_exact_edges() {
        // Reintroducing uniform index arithmetic sends drops on busy weeks to the wrong date.
        let layout = MonthLayout::new(Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(702.0, 628.0)), &[0, 5, 1, 4, 2]);
        for (week, top, height) in [(0, 47.0, 76.8), (1, 123.8, 166.8), (2, 290.6, 94.8), (3, 385.4, 148.8), (4, 534.2, 112.8)] {
            let cell = layout.cell(week * 7);
            assert!((cell.min.y - top).abs() < 0.01 && (cell.height() - height).abs() < 0.01);
            assert_eq!(layout.index_at(Pos2::new(61.0, cell.min.y), 35), Some(week * 7));
            assert_eq!(layout.index_at(Pos2::new(61.0, cell.max.y - 0.1), 35), Some(week * 7));
            if week > 0 { assert_eq!(layout.index_at(Pos2::new(61.0, cell.min.y - 0.1), 35), Some((week - 1) * 7)); }
        }
    }

    #[test]
    fn exact_painted_column_edges_select_the_date_on_the_right() {
        // Inverse multiplication can round an exact left boundary into the previous column.
        for origin in [Pos2::ZERO, Pos2::new(12.0, 140.0), Pos2::new(13.37, 29.27)] {
            for width in [70.0, 360.0, 884.0, 1176.0, 1700.0] {
                let layout = MonthLayout::new(Rect::from_min_size(origin, Vec2::new(width, 540.0)), &[1, 5, 0, 2]);
                for index in 0..28 {
                    let cell = layout.cell(index);
                    assert_eq!(layout.index_at(Pos2::new(cell.min.x, cell.center().y), 28), Some(index),
                        "left edge of date {index} at {origin:?}, width {width}");
                    assert_eq!(layout.index_at(Pos2::new(cell.max.x - 0.1, cell.center().y), 28), Some(index));
                }
            }
        }
    }

    #[test]
    fn cell_regions_stay_separate_at_tiny_sizes_and_multiple_widths() {
        // A minimum footer/body size or row count rounded upward would overlap these regions.
        for weeks in [4, 5, 6] {
            for width in [70.0, 360.0, 860.0, 1200.0] {
                for height in [0.0, 12.0, 80.0, 180.0, 310.0, 540.0] {
                    let outer = Rect::from_min_size(Pos2::new(13.0, 29.0), Vec2::new(width, height));
                    let month = MonthLayout::new(outer, &vec![40; weeks]);
                    for index in 0..weeks * 7 {
                        let cell = month.cell(index);
                        let layout = CellLayout::new(cell, 40);
                        for region in [cell, layout.header, layout.body, layout.footer] {
                            assert!(region.is_finite() && region.width() >= 0.0 && region.height() >= 0.0);
                            assert!(outer.expand(0.001).contains_rect(region));
                        }
                        assert!(layout.header.max.y <= layout.body.min.y);
                        assert!(layout.body.max.y <= layout.footer.min.y);
                        for row in 0..layout.visible { assert!(layout.body.expand(0.001).contains_rect(layout.row(row))); }
                    }
                }
            }
        }
    }

    #[test]
    fn drop_mapping_includes_adjacent_month_dates_and_excludes_outside_edges() {
        let layout = MonthLayout::new(Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(702.0, 508.0)), &[3; 6]);
        assert_eq!(layout.index_at(Pos2::new(11.0, 47.0), 42), Some(0));
        assert_eq!(layout.index_at(Pos2::new(710.9, 526.9), 42), Some(41));
        assert_eq!(layout.index_at(Pos2::new(111.1, 47.0), 42), Some(1));
        for outside in [Pos2::new(10.9, 48.0), Pos2::new(711.0, 48.0), Pos2::new(12.0, 46.9), Pos2::new(12.0, 527.0)] {
            assert_eq!(layout.index_at(outside, 42), None, "{outside:?}");
        }
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn begin_drag(ctx: &egui::Context, t: &Tokens, view: &MonthView) -> Pos2 {
        let size = Vec2::new(860.0, 540.0);
        let (output, _) = frame(ctx, t, view, size, Vec::new());
        let source = text_shapes(&output).iter().find(|(s, _)| s.galley.job.text == "Payment name 0").unwrap().0.pos + Vec2::new(10.0, 7.0);
        frame(ctx, t, view, size, vec![egui::Event::PointerMoved(source)]);
        frame(ctx, t, view, size, vec![button(source, true)]);
        frame(ctx, t, view, size, vec![egui::Event::PointerMoved(source + Vec2::new(12.0, 1.0))]);
        let payload = egui::DragAndDrop::payload::<DraggedTransaction>(ctx).expect("row must start a typed drag");
        assert_eq!((payload.id, payload.date, payload.amount), (10, 20261001, 123.45));
        source
    }

    #[test]
    fn dragging_a_row_to_an_adjacent_month_cell_emits_its_stable_identity() {
        // Omitting row drag sense, losing payload between frames, or using in-month-only targets fails this.
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        view.cells[1].in_month = false;
        begin_drag(&ctx, &t, &view);
        let target = Pos2::new(180.0, 120.0);
        frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::PointerMoved(target)]);
        let (_, action) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![button(target, false)]);
        assert!(matches!(action, Some(Action::MoveTransaction { id: 10, from: 20261001, to: 20261002, amount: 123.45 })));
        assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_none());
    }

    #[test]
    fn drag_preview_and_identity_survive_replacing_the_visible_month() {
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        begin_drag(&ctx, &t, &view);
        for cell in &mut view.cells {
            let mut date = budget_core::Cal::from_int(cell.date);
            date.add(budget_core::Field::Month, 1);
            cell.date = date.get();
            cell.rows.clear();
        }
        let target = Pos2::new(180.0, 120.0);
        let (preview, _) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::PointerMoved(target)]);
        assert!(text_shapes(&preview).iter().any(|(s, _)| s.galley.job.text == "Payment name 0"));
        assert!(text_shapes(&preview).iter().any(|(s, _)| s.galley.job.text.contains("drop on a date")));
        assert!(preview.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r)
            if r.stroke.width == 2.0 && r.stroke.color == t.accent && r.rect.contains(target))),
            "destination must be highlighted in the first frame after changing month");
        let (_, action) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![button(target, false)]);
        assert!(matches!(action, Some(Action::MoveTransaction { id: 10, from: 20261001, to: 20261102, amount: 123.45 })));
    }

    #[test]
    fn drops_over_date_headers_transaction_rows_and_balance_footers_reach_the_cell() {
        for target in [Pos2::new(180.0, 38.0), Pos2::new(180.0, 59.0), Pos2::new(180.0, 143.0)] {
            let (ctx, t) = context();
            let view = fixture(4, 1);
            begin_drag(&ctx, &t, &view);
            frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::PointerMoved(target)]);
            let (_, action) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![button(target, false)]);
            assert!(matches!(action, Some(Action::MoveTransaction { id: 10, from: 20261001, to: 20261002, amount: 123.45 })), "{target:?}");
        }
    }

    #[test]
    fn same_day_and_outside_drops_and_escape_do_not_move_or_edit() {
        for destination in [Pos2::new(80.0, 120.0), Pos2::new(900.0, 120.0), Pos2::new(180.0, 12.0)] {
            let (ctx, t) = context();
            let view = fixture(4, 1);
            begin_drag(&ctx, &t, &view);
            frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::PointerMoved(destination)]);
            let (_, action) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![button(destination, false)]);
            assert!(action.is_none());
            assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_none());
        }
        let (ctx, t) = context();
        let view = fixture(4, 1);
        begin_drag(&ctx, &t, &view);
        frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
        let destination = Pos2::new(180.0, 120.0);
        let (_, action) = frame(&ctx, &t, &view, Vec2::new(860.0, 540.0), vec![egui::Event::PointerMoved(destination), button(destination, false)]);
        assert!(action.is_none());
        assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_none());
    }

    #[test]
    fn clicking_a_payment_still_edits_the_selected_occurrence() {
        let (ctx, t) = context();
        let view = fixture(4, 1);
        let size = Vec2::new(860.0, 540.0);
        let source = Pos2::new(25.0, 59.0);
        frame(&ctx, &t, &view, size, Vec::new());
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(source)]);
        frame(&ctx, &t, &view, size, vec![button(source, true)]);
        let (_, action) = frame(&ctx, &t, &view, size, vec![button(source, false)]);
        assert!(matches!(action, Some(Action::EditTransaction { id: 10, date: 20261001, amount: 123.45 })));
        assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_none());
    }

    #[test]
    fn fifth_visible_row_can_be_clicked_and_dragged_across_unequal_weeks() {
        // Retaining the old card hit rectangles or uniform drop mapping edits/moves the wrong date.
        for dragging in [false, true] {
            let (ctx, t) = context();
            let mut view = fixture(5, 5);
            for (week, count) in [4, 5, 5, 5, 4].into_iter().enumerate() {
                for cell in &mut view.cells[week * 7..(week + 1) * 7] { cell.rows.truncate(count); }
            }
            let size = Vec2::new(1176.0, 630.0);
            let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
            let shapes = text_shapes(&output);
            let (_, clip) = shapes.iter().find(|(s, _)| s.galley.job.text == "Payment name 4").unwrap();
            let source = clip.center();
            frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(source)]);
            frame(&ctx, &t, &view, size, vec![button(source, true)]);
            if dragging {
                frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(source + Vec2::new(12.0, 1.0))]);
                assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_some());
                // The short first week ends at 136.6px. Uniform week heights
                // would still map this point to the first week, on October 2.
                let target = Pos2::new(220.0, 139.0);
                frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(target)]);
                let (_, action) = frame(&ctx, &t, &view, size, vec![button(target, false)]);
                assert!(matches!(action, Some(Action::MoveTransaction { id: 14, from: 20261008, to: 20261009, amount: 123.45 })));
            } else {
                let (_, action) = frame(&ctx, &t, &view, size, vec![button(source, false)]);
                let edited = match action { Some(Action::EditTransaction { id, date, amount }) => Some((id, date, amount)), _ => None };
                assert_eq!(edited, Some((14, 20261008, 123.45)), "fifth-row source {source:?}");
            }
            assert!(egui::DragAndDrop::payload::<DraggedTransaction>(&ctx).is_none());
        }
    }

    #[test]
    fn crowded_day_header_opens_all_transactions_when_no_name_rows_fit() {
        let (ctx, t) = context();
        let view = fixture(6, 20);
        let size = Vec2::new(860.0, 236.0);
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        assert!(!text_shapes(&output).iter().any(|(s, _)| s.galley.job.text.starts_with("Payment name")));
        let pos = text_shapes(&output).iter().find(|(s, _)| s.galley.job.text == "+20").unwrap().0.pos + Vec2::new(8.0, 8.0);
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(pos)]);
        frame(&ctx, &t, &view, size, vec![button(pos, true)]);
        let (_, action) = frame(&ctx, &t, &view, size, vec![button(pos, false)]);
        assert!(matches!(action, Some(Action::Day(20261001))));
    }

    #[test]
    fn filtered_overflow_counts_hidden_matching_rows_and_says_so() {
        // A filtered view holds only the matching rows, so +N is the number of hidden matches.
        let (ctx, t) = context();
        ctx.style_mut(|s| s.interaction.tooltip_delay = 0.0);
        let mut view = fixture(4, 20);
        view.filtered = true;
        let size = Vec2::new(860.0, 540.0);
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        let shapes = text_shapes(&output);
        let visible = shapes.iter().filter(|(s, _)| s.galley.job.text.starts_with("Payment name")).count() / view.cells.len();
        assert!(visible > 0);
        let label = format!("+{}", 20 - visible);
        let (more, _) = shapes.iter().find(|(s, _)| s.galley.job.text == label).unwrap_or_else(|| panic!("{label} must count the hidden matches"));
        let pos = more.pos + Vec2::new(8.0, 8.0);
        let mut tip = false;
        for _ in 0..30 {
            let (o, _) = frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(pos)]);
            tip |= text_shapes(&o).iter().any(|(s, _)| s.galley.job.text.starts_with(&format!("{} more matching transactions", 20 - visible)));
            if tip { break; }
        }
        assert!(tip, "the tooltip says the hidden rows are matches");
    }

    #[test]
    fn right_clicking_a_balance_offers_its_dated_edit_action() {
        let (ctx, t) = context();
        let view = fixture(4, 1);
        let size = Vec2::new(860.0, 540.0);
        let pos = Pos2::new(65.0, 143.0);
        frame(&ctx, &t, &view, size, Vec::new());
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(&ctx, &t, &view, size, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE }]);
        }
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        let edit = text_shapes(&output).iter().find(|(s, _)| s.galley.job.text == "Edit balance").expect("footer context menu must offer balance editing").0;
        let edit_pos = edit.pos + Vec2::new(10.0, 7.0);
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(edit_pos)]);
        frame(&ctx, &t, &view, size, vec![button(edit_pos, true)]);
        let (_, action) = frame(&ctx, &t, &view, size, vec![button(edit_pos, false)]);
        assert!(matches!(action, Some(Action::EditBalance(20261001))));
    }

    fn secondary(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn right_click_menu(ctx: &egui::Context, t: &Tokens, view: &MonthView, pos: Pos2) -> egui::FullOutput {
        let size = Vec2::new(860.0, 540.0);
        frame(ctx, t, view, size, Vec::new());
        frame(ctx, t, view, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] { frame(ctx, t, view, size, vec![secondary(pos, pressed)]); }
        frame(ctx, t, view, size, Vec::new()).0
    }

    fn click_text(ctx: &egui::Context, t: &Tokens, view: &MonthView, output: &egui::FullOutput, label: &str) -> Option<Action> {
        let size = Vec2::new(860.0, 540.0);
        let pos = text_shapes(output).iter().find(|(s, _)| s.galley.job.text == label).unwrap_or_else(|| panic!("{label} must be shown")).0.pos + Vec2::new(10.0, 7.0);
        frame(ctx, t, view, size, vec![egui::Event::PointerMoved(pos)]);
        frame(ctx, t, view, size, vec![button(pos, true)]);
        frame(ctx, t, view, size, vec![button(pos, false)]).1
    }

    #[test]
    fn deleted_occurrences_can_be_restored_from_the_day_menu_only_without_a_filter() {
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        view.cells[0].restorable = vec![(42, "Rent".into())];
        let output = right_click_menu(&ctx, &t, &view, Pos2::new(70.0, 38.0));
        assert!(matches!(click_text(&ctx, &t, &view, &output, "Restore 'Rent'"), Some(Action::Restore { id: 42, date: 20261001 })));
        let (ctx, t) = context();
        view.key.1 = "rent".into();
        view.filtered = true;
        let output = right_click_menu(&ctx, &t, &view, Pos2::new(70.0, 38.0));
        assert!(click_text(&ctx, &t, &view, &output, "Restore 'Rent'").is_none(), "restoring is disabled while filtered (05 §1.5)");
    }

    #[test]
    fn right_click_during_a_drag_opens_no_menu_for_another_row() {
        let (ctx, t) = context();
        let view = fixture(4, 1);
        begin_drag(&ctx, &t, &view);
        let size = Vec2::new(860.0, 540.0);
        let other = Pos2::new(205.0, 59.0);
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(other)]);
        for pressed in [true, false] { frame(&ctx, &t, &view, size, vec![secondary(other, pressed)]); }
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        assert!(!text_shapes(&output).iter().any(|(s, _)| s.galley.job.text == "Edit transaction (this date)"));
        assert!(!ctx.memory(|m| m.any_popup_open()));
    }

    #[test]
    fn hovering_many_rows_does_not_retain_a_layer_per_row() {
        let (ctx, t) = context();
        let view = fixture(4, 1);
        let size = Vec2::new(860.0, 540.0);
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        let rows: Vec<Pos2> = text_shapes(&output).iter().filter(|(s, _)| s.galley.job.text == "Payment name 0").map(|(s, _)| s.pos + Vec2::new(10.0, 7.0)).collect();
        assert!(rows.len() >= 20);
        let mut counts = Vec::new();
        for pos in rows {
            frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(pos)]);
            frame(&ctx, &t, &view, size, Vec::new());
            counts.push(ctx.memory(|m| m.layer_ids().len()));
        }
        assert_eq!(counts.first(), counts.last(), "layers grew while hovering: {counts:?}");
    }

    #[test]
    fn transaction_notes_are_marked_on_the_row() {
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        let size = Vec2::new(860.0, 540.0);
        let (plain, _) = frame(&ctx, &t, &view, size, Vec::new());
        let name_x = |o: &egui::FullOutput| text_shapes(o).iter().find(|(s, _)| s.galley.job.text == "Payment name 0").unwrap().0.pos.x;
        let before = name_x(&plain);
        view.cells[0].rows[0].tip.note = Some("Paid by cheque #123".into());
        let (noted, _) = frame(&ctx, &t, &view, size, Vec::new());
        assert_eq!(text_shapes(&noted).iter().filter(|(s, _)| s.galley.job.text == ph::NOTE_PENCIL).count(), 1);
        assert!(name_x(&noted) > before + 5.0, "the name makes room for the note mark");
    }

    #[test]
    fn adjusted_balance_strip_shows_the_adjustment_and_hover_lists_it() {
        let (ctx, t) = context();
        let mut view = fixture(4, 1);
        view.cells[0].balance = Some(Balance { text: "1,250".into(), value: 1250.0, kind: BalKind::Normal, adjusted: true, adjustment: 250.0 });
        let size = Vec2::new(860.0, 540.0);
        let (output, _) = frame(&ctx, &t, &view, size, Vec::new());
        assert!(text_shapes(&output).iter().any(|(s, _)| s.galley.job.text == "1,250\u{2032}(250.00)"));
        let pos = Pos2::new(65.0, 143.0);
        frame(&ctx, &t, &view, size, vec![egui::Event::PointerMoved(pos)]);
        let mut texts = Vec::new();
        for _ in 0..90 {
            let (o, _) = frame(&ctx, &t, &view, size, Vec::new());
            texts = text_shapes(&o).iter().map(|(s, _)| s.galley.job.text.clone()).collect::<Vec<_>>();
            if texts.iter().any(|s| s.starts_with("Adjustment")) { break; }
        }
        assert!(texts.iter().any(|s| s == "Before adjustment 1,000.00"), "{texts:?}");
        assert!(texts.iter().any(|s| s == "Adjustment 250.00"));
    }
}
