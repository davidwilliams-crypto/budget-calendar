//! The searchable, full-width payment library.

use budget_core::{BudgetDb, Cal, Payment, END_MODE_N_PAYS};
use eframe::egui::{
    self,
    text::{LayoutJob, TextWrapping},
    Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};
use egui_phosphor::regular as ph;
use std::rc::Rc;

use crate::calendar::Action;
use crate::names::{MONTHS, WEEKDAYS};
use crate::theme::{self, font, tint, Tokens, CAPTION, SEMI};
use crate::view::{is_negative, money};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum StatusFilter {
    #[default]
    All,
    Active,
    Disabled,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SortOrder {
    #[default]
    Name,
    Amount,
    StartNewest,
    StartOldest,
}

/// "payment" for exactly one, otherwise "payments".
pub fn payments_noun(n: usize) -> &'static str { if n == 1 { "payment" } else { "payments" } }

/// Indices of the payments matching the search and status filter, in the chosen order.
fn visible_payments(
    payments: &[Payment],
    search: &str,
    status: StatusFilter,
    sort: SortOrder,
) -> Vec<usize> {
    let search = search.trim().to_lowercase();
    // Lower-cased once per payment rather than once per comparison.
    let names: Vec<String> = payments.iter().map(|p| p.description.to_lowercase()).collect();
    let mut matches: Vec<usize> = (0..payments.len())
        .filter(|&i| {
            let p = &payments[i];
            let status_matches = match status {
                StatusFilter::All => true,
                StatusFilter::Active => !p.disabled,
                StatusFilter::Disabled => p.disabled,
            };
            status_matches
                && (search.is_empty()
                    || names[i].contains(&search)
                    || p.category
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search))
        })
        .collect();
    matches.sort_by(|&a, &b| {
        let (pa, pb) = (&payments[a], &payments[b]);
        let name_order = || {
            names[a]
                .cmp(&names[b])
                .then(pa.start_date.cmp(&pb.start_date))
                .then(pa.id.cmp(&pb.id))
        };
        match sort {
            SortOrder::Name => name_order(),
            SortOrder::Amount => pa.amount.total_cmp(&pb.amount).then_with(name_order),
            SortOrder::StartNewest => pb.start_date.cmp(&pa.start_date).then_with(name_order),
            SortOrder::StartOldest => pa.start_date.cmp(&pb.start_date).then_with(name_order),
        }
    });
    matches
}

fn schedule_label(payment: &Payment) -> String {
    if payment.end_mode() == END_MODE_N_PAYS && payment.n_pays == 1 {
        return "Once".into();
    }
    let interval = payment.frequency;
    let every = |unit: &str| {
        if interval == 1 {
            format!("Every {unit}")
        } else {
            format!("Every {interval} {unit}s")
        }
    };
    match payment.freq_type {
        'd' => every("day"),
        'w' => every("week"),
        'm' => every("month"),
        'y' => every("year"),
        'W' | 'L' => {
            let start = Cal::from_int(payment.start_date);
            let weekday = WEEKDAYS[start.day_of_week() as usize];
            if payment.freq_type == 'W' {
                let ordinal =
                    ["First", "Second", "Third", "Fourth", "Fifth"][start.week_of_month() as usize];
                format!("{ordinal} {weekday} monthly")
            } else if interval == 1 {
                format!("Last {weekday} monthly")
            } else {
                format!("Last {weekday} every {interval} months")
            }
        }
        _ => "Custom schedule".into(),
    }
}

impl SortOrder {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name: A–Z",
            Self::Amount => "Amount: low to high",
            Self::StartNewest => "Start date: newest first",
            Self::StartOldest => "Start date: oldest first",
        }
    }
}

/// What the visible list was filtered and sorted with.
#[derive(Clone, PartialEq)]
struct ListKey {
    /// The document revision (budget generation, engine update count) and its payment count.
    revision: (u64, i32),
    payments: usize,
    search: String,
    status: StatusFilter,
    sort: SortOrder,
}

#[derive(Default)]
pub struct PaymentList {
    pub search: String,
    pub selected: Option<i32>,
    pub focus_search: bool,
    status: StatusFilter,
    sort: SortOrder,
    /// Indices of the visible payments, kept until the document, search, filter or order changes.
    visible: Option<(ListKey, Rc<[usize]>)>,
}

impl PaymentList {
    /// `revision` identifies the document state; the filtered and sorted list is recomputed only
    /// when it, the search, the status filter or the order changes.
    pub fn show(&mut self, ui: &mut Ui, db: &BudgetDb, t: &Tokens, today: i32, revision: (u64, i32)) -> Option<Action> {
        // Selection belongs to the record, so filtering and sorting never change it.
        if self.selected.is_some_and(|id| db.get_by_id(id).is_none()) {
            self.selected = None;
        }
        let count = self.visible(db, revision).len();
        heading(ui, db, t, count);
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0_f32, t.border))
            .corner_radius(theme::RADIUS)
            .inner_margin(12.0)
            .show(ui, |ui| {
                self.search_row(ui, t);
                ui.add_space(4.0);
                self.filter_row(ui, t);
            });
        ui.add_space(12.0);
        // The search, filter or order may have changed above.
        let rows = self.visible(db, revision);
        if rows.is_empty() {
            return self.empty_state(ui, db, t).then_some(Action::Add(today));
        }
        self.table(ui, db, t, today, &rows)
    }

    /// The visible payments' indices, filtered and sorted only when their inputs changed.
    fn visible(&mut self, db: &BudgetDb, revision: (u64, i32)) -> Rc<[usize]> {
        let key = ListKey { revision, payments: db.payments.len(), search: self.search.clone(), status: self.status, sort: self.sort };
        if let Some((cached, rows)) = &self.visible {
            if *cached == key {
                return rows.clone();
            }
        }
        let rows: Rc<[usize]> = visible_payments(&db.payments, &self.search, self.status, self.sort).into();
        self.visible = Some((key, rows.clone()));
        rows
    }

    fn reset_filters(&mut self) {
        self.search.clear();
        self.status = StatusFilter::All;
    }

    fn search_row(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(ph::MAGNIFYING_GLASS).color(t.text_muted));
            let search = ui.add_sized(
                [ui.available_width() - 85.0, 34.0],
                egui::TextEdit::singleline(&mut self.search)
                    .id_source("payment_library_search")
                    .hint_text("Search description or category")
                    .margin(Vec2::new(10.0, 7.0)),
            );
            crate::accessible_name(ui.ctx(), &search, "Search payments");
            crate::note_text_focus(ui.ctx(), &search);
            if self.focus_search {
                search.request_focus();
                self.focus_search = false;
            }
            ui.add_enabled_ui(!self.search.is_empty(), |ui| {
                if theme::secondary_button(ui, "Clear", t).clicked() {
                    self.search.clear();
                }
            });
        });
    }

    /// Status chips, the sort order and Reset filters.
    fn filter_row(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for (filter, label) in [
                (StatusFilter::All, "All"),
                (StatusFilter::Active, "Enabled"),
                (StatusFilter::Disabled, "Disabled"),
            ] {
                let selected = self.status == filter;
                let response = ui.add(
                    egui::Button::new(RichText::new(label).color(if selected {
                        t.accent
                    } else {
                        t.text_muted
                    }))
                    .fill(if selected {
                        // 9%: the accent label is 4.5:1 on it (10% is 4.49:1 in the light theme).
                        tint(t.surface, t.accent, 0.09)
                    } else {
                        t.surface
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if selected {
                            tint(t.border, t.accent, 0.65)
                        } else {
                            t.border
                        },
                    ))
                    .min_size(Vec2::new(58.0, 34.0)),
                );
                crate::theme::focus_ring(ui, &response, t);
                if response.clicked() {
                    self.status = filter;
                }
                if filter == StatusFilter::Active {
                    response.on_hover_text(
                        "All enabled payments, including historical records.",
                    );
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Sort by").color(t.text_muted));
            egui::ComboBox::from_id_salt("payment_library_sort")
                .selected_text(self.sort.label())
                .width(186.0)
                .show_ui(ui, |ui| {
                    for sort in [
                        SortOrder::Name,
                        SortOrder::Amount,
                        SortOrder::StartNewest,
                        SortOrder::StartOldest,
                    ] {
                        ui.selectable_value(&mut self.sort, sort, sort.label());
                    }
                });
            if !self.search.is_empty() || self.status != StatusFilter::All {
                ui.add_space(8.0);
                let reset = ui.add(
                    egui::Button::new(RichText::new("Reset filters").color(t.accent))
                        .frame(false)
                        .min_size(Vec2::new(0.0, 34.0)),
                );
                crate::theme::focus_ring(ui, &reset, t);
                if reset.clicked() {
                    self.reset_filters();
                }
            }
        });
    }

    /// No payments at all, or none matching. Returns whether New payment was chosen.
    fn empty_state(&mut self, ui: &mut Ui, db: &BudgetDb, t: &Tokens) -> bool {
        let mut add_first = false;
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0_f32, t.border))
            .corner_radius(theme::RADIUS)
            .inner_margin(32.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(if db.payments.is_empty() {
                            "No payments yet"
                        } else {
                            "No matching payments"
                        })
                        .size(18.0)
                        .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(if db.payments.is_empty() {
                            "Choose New payment above to add your first income or expense."
                        } else {
                            "Try a different description, category, or status."
                        })
                        .color(t.text_muted),
                    );
                    ui.add_space(8.0);
                    if !db.payments.is_empty() {
                        if theme::secondary_button(ui, "Reset filters", t).clicked() {
                            self.reset_filters();
                        }
                    } else if theme::primary_button(ui, "New payment", t).on_hover_text("Ctrl+N").clicked() {
                        add_first = true;
                    }
                    ui.add_space(24.0);
                });
            });
        add_first
    }

    /// The column headings and the virtualised rows of `rows` (indices into `db.payments`).
    fn table(&mut self, ui: &mut Ui, db: &BudgetDb, t: &Tokens, today: i32, rows: &[usize]) -> Option<Action> {
        let mut action = None;
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0_f32, t.border))
            .corner_radius(theme::RADIUS)
            .inner_margin(egui::Margin::symmetric(12, 4))
            .show(ui, |ui| {
                // Reserve the vertical scrollbar's width in both header and body.
                let width = ui.available_width() - ui.spacing().scroll.allocated_width();
                let columns = Columns::new(width);
                column_headings(ui, t, &columns, width);
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical()
                    .id_salt("payment_library_rows")
                    .auto_shrink([false, false])
                    .show_rows(ui, 58.0, rows.len(), |ui, range| {
                        for &index in &rows[range] {
                            let Some(payment) = db.payments.get(index) else { continue };
                            let (row, _) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::hover());
                            if let Some(a) = self.row(ui, payment, row, &columns, t, today) {
                                action = Some(a);
                            }
                        }
                    });
            });
        action
    }

    /// One payment's row in `row`: selection, text columns, status badge and actions.
    fn row(&mut self, ui: &mut Ui, payment: &Payment, row: Rect, columns: &Columns, t: &Tokens, today: i32) -> Option<Action> {
        let mut action = None;
        let response = ui.interact(row, ui.id().with(("payment_row", payment.id)), Sense::click());
        let selected = self.selected == Some(payment.id);
        let schedule = schedule_label(payment);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                format!(
                    "{}, {}, {}, {}",
                    payment.description,
                    money(payment.amount, true),
                    schedule,
                    if payment.disabled { "Disabled" } else { "Enabled" }
                ),
            )
        });
        let fill = if selected {
            tint(t.surface, t.accent, if t.dark { 0.14 } else { 0.07 })
        } else if response.hovered() {
            t.surface_hover
        } else {
            t.surface
        };
        ui.painter().rect_filled(row, 4.0, fill);
        if selected {
            ui.painter().vline(row.left(), row.y_range().shrink(9.0), Stroke::new(3.0_f32, t.accent));
        }
        if response.clicked() {
            self.selected = Some(payment.id);
        }
        if response.double_clicked() {
            action = Some(Action::EditPayment(payment.id));
        }
        let start_label = start_label(payment.start_date, today);
        row_text(ui, payment, row, columns, t, &schedule, &start_label);
        // Only the exception state is marked; enabled rows stay quiet.
        if let Some(status) = status_badge(payment) {
            let mut status_ui = ui.new_child(
                UiBuilder::new()
                    .id_salt(("payment_status", payment.id))
                    .max_rect(Rect::from_min_size(
                        Pos2::new(row.left() + columns.status, row.top() + 15.0),
                        Vec2::new(88.0, 30.0),
                    ))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            theme::badge(&mut status_ui, status, t.text_muted, t);
        }
        crate::theme::focus_ring_inside(ui, &response, t);
        response.on_hover_text(format!(
            "{}\n{}\n{}\n{}\n{}\nDouble-click to edit this payment",
            payment.description,
            money(payment.amount, true),
            payment.category.as_deref().unwrap_or("Uncategorized"),
            schedule,
            start_label
        ));
        if let Some(a) = self.row_actions(ui, payment, row, columns, t, selected) {
            action = Some(a);
        }
        ui.painter().hline(row.x_range(), row.bottom(), Stroke::new(1.0_f32, tint(t.surface, t.border, 0.7)));
        action
    }

    /// The row's Edit button (hovered or selected row only) and its More menu.
    fn row_actions(&mut self, ui: &mut Ui, payment: &Payment, row: Rect, columns: &Columns, t: &Tokens, selected: bool) -> Option<Action> {
        let mut action = None;
        // The row already owns its layout allocation; positioned children must
        // not advance the parent cursor or shorten subsequent virtual rows.
        let mut actions_ui = ui.new_child(
            UiBuilder::new()
                .id_salt(("payment_actions", payment.id))
                .max_rect(Rect::from_min_size(
                    Pos2::new(row.left() + columns.actions, row.top() + 11.0),
                    Vec2::new(columns.actions_width, 36.0),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        actions_ui.spacing_mut().item_spacing.x = 4.0;
        // Edit appears only on the row under the pointer or the selected
        // row; elsewhere its slot is reserved so the menu never moves.
        let row_hovered = ui.rect_contains_pointer(row);
        let (slot, _) = actions_ui.allocate_exact_size(Vec2::new(EDIT_SLOT_WIDTH, 36.0), Sense::hover());
        if show_row_edit(selected, row_hovered) {
            let mut edit_ui = actions_ui.new_child(
                UiBuilder::new()
                    .id_salt(("payment_edit", payment.id))
                    .max_rect(slot)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if theme::secondary_button(&mut edit_ui, "Edit", t)
                .on_hover_text("Edit this payment and its schedule")
                .clicked()
            {
                self.selected = Some(payment.id);
                action = Some(Action::EditPayment(payment.id));
            }
        }
        let more = actions_ui
            .menu_button(ph::DOTS_THREE, |ui| {
                if ui.button(RichText::new("Delete payment…").color(t.negative)).clicked() {
                    self.selected = Some(payment.id);
                    action = Some(Action::DeletePayment(payment.id));
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text("More payment actions");
        // The button shows only an icon glyph; a screen reader needs to hear which payment it belongs to.
        crate::accessible_name(actions_ui.ctx(), &more, &format!("More actions for {}", payment.description));
        action
    }
}

/// "Payments  N of M payments" and the page's introduction.
fn heading(ui: &mut Ui, db: &BudgetDb, t: &Tokens, count: usize) {
    ui.horizontal(|ui| {
        ui.heading("Payments");
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("{count} of {} {}", db.payments.len(), payments_noun(db.payments.len())))
                .color(t.text_muted),
        );
    });
    ui.add_space(2.0);
    ui.label(
        RichText::new("Manage recurring and one-time payments, including historical records.")
            .color(t.text_muted),
    );
    ui.add_space(16.0);
}

fn column_headings(ui: &mut Ui, t: &Tokens, columns: &Columns, width: f32) {
    let (header, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
    for (text, offset, align) in [
        ("Payment", columns.name, Align2::LEFT_CENTER),
        ("Amount", columns.amount_right, Align2::RIGHT_CENTER),
        ("Schedule / start", columns.schedule, Align2::LEFT_CENTER),
        ("Status", columns.status, Align2::LEFT_CENTER),
        ("Actions", columns.actions, Align2::LEFT_CENTER),
    ] {
        ui.painter().text(
            Pos2::new(header.left() + offset, header.center().y),
            align,
            text,
            font(ui, CAPTION),
            t.text_muted,
        );
    }
    ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0_f32, t.border));
}

/// The name, category, amount, schedule and start columns of a row.
fn row_text(ui: &Ui, payment: &Payment, row: Rect, columns: &Columns, t: &Tokens, schedule: &str, start_label: &str) {
    let x = row.left();
    let label_y = row.top() + 19.0;
    let detail_y = row.top() + 40.0;
    truncated(ui, Pos2::new(x + columns.name, label_y), &payment.description, font(ui, SEMI), t.text, columns.name_width);
    truncated(
        ui,
        Pos2::new(x + columns.name, detail_y),
        payment.category.as_deref().filter(|c| !c.is_empty()).unwrap_or("Uncategorized"),
        font(ui, CAPTION),
        t.text_muted,
        columns.name_width,
    );
    let amount_clip = Rect::from_min_max(
        Pos2::new(x + columns.amount_right - 112.0, row.top()),
        Pos2::new(x + columns.amount_right, row.bottom()),
    );
    ui.painter().with_clip_rect(ui.clip_rect().intersect(amount_clip)).text(
        Pos2::new(x + columns.amount_right, row.center().y),
        Align2::RIGHT_CENTER,
        money(payment.amount, true),
        font(ui, SEMI),
        // The colour follows the sign the amount is printed with, so a value that shows no minus is never red.
        if payment.disabled { t.text_muted } else if is_negative(payment.amount, true) { t.negative } else { t.positive },
    );
    truncated(ui, Pos2::new(x + columns.schedule, label_y), schedule, egui::TextStyle::Body.resolve(ui.style()), t.text, columns.schedule_width);
    truncated(ui, Pos2::new(x + columns.schedule, detail_y), start_label, font(ui, CAPTION), t.text_muted, columns.schedule_width);
}

/// Width reserved for a row's Edit button whether or not it is drawn.
const EDIT_SLOT_WIDTH: f32 = 52.0;

/// The status column marks only the exception state: disabled payments.
fn status_badge(payment: &Payment) -> Option<&'static str> {
    payment.disabled.then_some("Disabled")
}

/// The per-row Edit button shows on the hovered or the selected row only.
fn show_row_edit(selected: bool, hovered: bool) -> bool {
    selected || hovered
}

/// `2 Jan 2026` (day without a leading zero).
fn short_date(date: i32) -> String {
    let d = Cal::from_int(date);
    format!("{} {} {}", d.day(), MONTHS[d.month() as usize], d.year())
}

/// "Starts 2 Jan 2027" for a future start, "Since 2 Jan 2026" otherwise.
fn start_label(start_date: i32, today: i32) -> String {
    let verb = if start_date > today { "Starts" } else { "Since" };
    format!("{verb} {}", short_date(start_date))
}

struct Columns {
    name: f32,
    name_width: f32,
    amount_right: f32,
    schedule: f32,
    schedule_width: f32,
    status: f32,
    actions: f32,
    actions_width: f32,
}

impl Columns {
    fn new(width: f32) -> Self {
        let schedule_width = if width >= 1000.0 {
            220.0
        } else if width >= 790.0 {
            190.0
        } else {
            156.0
        };
        let actions_width = 102.0;
        let status_width = 88.0;
        let gap = 18.0;
        let actions = width - actions_width;
        let status = actions - status_width - gap;
        let schedule = status - schedule_width - gap;
        let amount_right = schedule - gap;
        let name_width = (amount_right - 112.0 - gap).max(90.0);
        Self {
            name: 8.0,
            name_width,
            amount_right,
            schedule,
            schedule_width,
            status,
            actions,
            actions_width,
        }
    }
}

fn truncated(ui: &Ui, pos: Pos2, text: &str, font: FontId, color: Color32, width: f32) {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(8.0));
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos - Vec2::new(0.0, galley.size().y / 2.0), galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::{END_MODE_DATE, END_MODE_INDEFINITE};

    fn payment(id: i32, description: &str, date: i32, amount: f64) -> Payment {
        let mut p = Payment::new(date);
        p.id = id;
        p.description = description.into();
        p.amount = amount;
        p
    }

    fn ids(payments: Vec<&Payment>) -> Vec<i32> {
        payments.into_iter().map(|p| p.id).collect()
    }

    /// The visible records themselves (the list keeps their indices).
    fn visible_payments<'a>(records: &'a [Payment], search: &str, status: StatusFilter, sort: SortOrder) -> Vec<&'a Payment> {
        super::visible_payments(records, search, status, sort).into_iter().map(|i| &records[i]).collect()
    }

    #[test]
    fn the_visible_list_is_reused_until_the_document_search_filter_or_order_changes() {
        let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
        db.add(payment(0, "Rent", 20261001, -900.0));
        db.add(payment(0, "Hydro", 20261005, -80.0));
        let revision = (1, db.update_count);
        let mut list = PaymentList::default();
        let all = list.visible(&db, revision);
        assert_eq!(all.iter().map(|&i| db.payments[i].description.as_str()).collect::<Vec<_>>(), ["Hydro", "Rent"]);
        assert!(Rc::ptr_eq(&all, &list.visible(&db, revision)), "an unchanged list is not filtered and sorted again");
        list.search = "rent".into();
        let rent = list.visible(&db, revision);
        assert_eq!(rent.len(), 1);
        assert!(Rc::ptr_eq(&rent, &list.visible(&db, revision)));
        list.search.clear();
        list.sort = SortOrder::Amount;
        assert_eq!(list.visible(&db, revision).iter().map(|&i| db.payments[i].amount).collect::<Vec<_>>(), [-900.0, -80.0]);
        list.status = StatusFilter::Disabled;
        assert!(list.visible(&db, revision).is_empty());
        list.status = StatusFilter::All;
        let before = list.visible(&db, revision);
        db.add(payment(0, "Gym", 20261010, -40.0));
        assert_eq!(list.visible(&db, (1, db.update_count)).len(), 3, "an edit refreshes the list");
        assert!(!Rc::ptr_eq(&before, &list.visible(&db, (2, db.update_count))), "so does another budget");
    }

    #[test]
    fn default_library_keeps_historical_disabled_and_future_records() {
        let mut historical = payment(1, "Old payment", 20100101, -10.0);
        historical.disabled = true;
        let records = vec![payment(2, "Future payment", 20300101, -20.0), historical];
        assert_eq!(
            ids(visible_payments(
                &records,
                "",
                StatusFilter::default(),
                SortOrder::default()
            )),
            [2, 1]
        );
        assert_eq!(records.len(), 2);
        assert!(records[1].disabled);
    }

    #[test]
    fn live_search_matches_description_and_category_ignoring_case_and_outer_spaces() {
        let mut hydro = payment(1, "Hydro bill", 20260101, -100.0);
        hydro.category = Some("Utilities".into());
        let records = vec![hydro, payment(2, "Rent", 20260101, -1000.0)];
        assert_eq!(
            ids(visible_payments(
                &records,
                "  HYDRO  ",
                StatusFilter::All,
                SortOrder::Name
            )),
            [1]
        );
        assert_eq!(
            ids(visible_payments(
                &records,
                "util",
                StatusFilter::All,
                SortOrder::Name
            )),
            [1]
        );
        assert!(
            visible_payments(&records, "no match", StatusFilter::All, SortOrder::Name).is_empty()
        );
        assert_eq!(
            visible_payments(&records, "  ", StatusFilter::All, SortOrder::Name).len(),
            2
        );
    }

    #[test]
    fn status_filter_uses_disabled_flag_and_composes_with_search() {
        let active = payment(1, "Rent", 20100101, -1000.0);
        let mut disabled = payment(2, "Old rent", 20100101, -900.0);
        disabled.disabled = true;
        let records = vec![disabled, active];
        assert_eq!(
            ids(visible_payments(
                &records,
                "rent",
                StatusFilter::Active,
                SortOrder::Name
            )),
            [1]
        );
        assert_eq!(
            ids(visible_payments(
                &records,
                "rent",
                StatusFilter::Disabled,
                SortOrder::Name
            )),
            [2]
        );
        assert!(visible_payments(
            &records,
            "mortgage",
            StatusFilter::Disabled,
            SortOrder::Name
        )
        .is_empty());
    }

    #[test]
    fn sort_orders_numeric_amounts_dates_and_name_ties_deterministically() {
        let records = vec![
            payment(4, "Rent", 20260201, -100.0),
            payment(3, "Alpha", 20260301, 500.0),
            payment(2, "rent", 20260101, -20.0),
            payment(1, "Rent", 20260101, -20.0),
        ];
        assert_eq!(
            ids(visible_payments(
                &records,
                "",
                StatusFilter::All,
                SortOrder::Name
            )),
            [3, 1, 2, 4]
        );
        assert_eq!(
            ids(visible_payments(
                &records,
                "",
                StatusFilter::All,
                SortOrder::Amount
            )),
            [4, 1, 2, 3]
        );
        assert_eq!(
            ids(visible_payments(
                &records,
                "",
                StatusFilter::All,
                SortOrder::StartNewest
            )),
            [3, 4, 1, 2]
        );
        assert_eq!(
            ids(visible_payments(
                &records,
                "",
                StatusFilter::All,
                SortOrder::StartOldest
            )),
            [1, 2, 4, 3]
        );
    }

    #[test]
    fn schedule_distinguishes_one_time_from_indefinite_and_date_limited_repeats() {
        let mut p = payment(1, "Rent", 20261027, -100.0);
        assert_eq!(schedule_label(&p), "Once");
        p.set_end_mode(END_MODE_INDEFINITE);
        assert_eq!(schedule_label(&p), "Every month");
        p.set_end_mode(END_MODE_DATE);
        p.freq_type = 'w';
        p.frequency = 2;
        assert_eq!(schedule_label(&p), "Every 2 weeks");
        p.freq_type = 'd';
        p.frequency = 1;
        assert_eq!(schedule_label(&p), "Every day");
        p.freq_type = 'y';
        p.frequency = 3;
        assert_eq!(schedule_label(&p), "Every 3 years");
    }

    #[test]
    fn monthly_weekday_schedules_describe_the_actual_weekday_and_ordinal() {
        let mut p = payment(1, "Payday", 20261027, 100.0); // Fourth Tuesday.
        p.set_end_mode(END_MODE_INDEFINITE);
        p.freq_type = 'W';
        assert_eq!(schedule_label(&p), "Fourth Tuesday monthly");
        p.freq_type = 'L';
        p.frequency = 2;
        assert_eq!(schedule_label(&p), "Last Tuesday every 2 months");
    }

    #[test]
    fn status_column_marks_only_disabled_payments() {
        let mut p = payment(1, "Rent", 20260101, -100.0);
        assert_eq!(status_badge(&p), None);
        p.disabled = true;
        assert_eq!(status_badge(&p), Some("Disabled"));
    }

    #[test]
    fn row_edit_button_shows_only_for_hovered_or_selected_rows() {
        assert!(!show_row_edit(false, false));
        assert!(show_row_edit(true, false));
        assert!(show_row_edit(false, true));
        assert!(show_row_edit(true, true));
    }

    #[test]
    fn start_label_drops_leading_zero_and_says_since_for_past_dates() {
        assert_eq!(start_label(20260102, 20261027), "Since 2 Jan 2026");
        assert_eq!(start_label(20261027, 20261027), "Since 27 Oct 2026");
        assert_eq!(start_label(20271205, 20261027), "Starts 5 Dec 2027");
    }
}
