//! Payment and transaction drafts. Dialogs return values without mutating the database.
use budget_core::{date::sdate, BudgetDb, Cal, Field, Payment, END_MODE_DATE, END_MODE_INDEFINITE,
    END_MODE_LOAN_PAID, END_MODE_MASK, END_MODE_N_PAYS, FLAG_IMPORTANT, FLAG_NONE, FLAG_PAID};
use eframe::egui::{self, ComboBox, Grid, RichText, ScrollArea, TextEdit};
use crate::theme::{self, Tokens};

pub enum PaymentAction { None, Cancel, Apply(Payment) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionScope { Occurrence, Future }
pub enum TransactionAction { None, Cancel, Apply { amount: f64, note: String, flag: char, scope: TransactionScope }, Delete }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmountKind { Expense, Income }
impl AmountKind {
    fn from_amount(amount: f64) -> Self {
        if amount.is_sign_negative() { Self::Expense } else { Self::Income }
    }
    fn signed(self, text: &str) -> Result<f64, String> {
        let amount = finite_number(text, "Amount")?;
        if amount.is_sign_negative() { return Err("Enter an amount of zero or more, then choose Expense or Income.".into()); }
        Ok(if self == Self::Expense { -amount } else { amount })
    }
}

struct DatePicker { month: Cal }
impl DatePicker {
    fn new(text: &str, fallback: i32) -> Self {
        let date = parse_iso_date(text).or_else(|_| parse_iso_date(&sdate(fallback)))
            .unwrap_or_else(|_| Cal::today().get());
        let mut month = Cal::from_int(date); month.set_day_one();
        Self { month }
    }
    fn shift_month(&mut self, delta: i32) {
        let mut month = self.month; month.add(Field::Month, delta);
        if (1000..=9999).contains(&month.year()) { self.month = month; }
    }
    fn choose(&self, day: i32) -> Option<i32> {
        (1..=self.month.max_day()).contains(&day)
            .then(|| Cal::ymd(self.month.year(), self.month.month(), day).get())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recurrence { Once, Days, Weeks, Months, Years, NthWeekday, LastWeekday, Legacy(char) }

impl Recurrence {
    fn from_payment(p: &Payment) -> Self {
        if p.end_mode() == END_MODE_N_PAYS && p.n_pays == 1 { return Self::Once; }
        match p.freq_type {
            'd' => Self::Days, 'w' => Self::Weeks, 'm' => Self::Months,
            'y' => Self::Years, 'W' => Self::NthWeekday, 'L' => Self::LastWeekday,
            other => Self::Legacy(other),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Once => "Once", Self::Days => "Days", Self::Weeks => "Weeks",
            Self::Months => "Months", Self::Years => "Years",
            Self::NthWeekday => "Nth weekday of each month", Self::LastWeekday => "Last weekday of month",
            Self::Legacy(_) => "Existing recurrence",
        }
    }
    fn freq_type(self, original: char) -> char {
        match self {
            Self::Once => original, Self::Days => 'd', Self::Weeks => 'w', Self::Months => 'm',
            Self::Years => 'y', Self::NthWeekday => 'W', Self::LastWeekday => 'L', Self::Legacy(c) => c,
        }
    }
}

pub struct PaymentEditor {
    pub payment_id: Option<i32>,
    // A full clone retains exceptions, metadata, unknown bits and calendar_days. In particular,
    // do not use Payment::clone_payment, which intentionally drops much of that state.
    original: Payment,
    description: String,
    amount: String,
    amount_kind: AmountKind,
    category: String,
    start_date: String,
    start_picker: Option<DatePicker>,
    recurrence: Recurrence,
    recurring_end_initialized: bool,
    frequency: String,
    end_mode: i32,
    end_date: String,
    end_picker: Option<DatePicker>,
    count: String,
    loan_amount: String,
    enabled: bool,
    no_weekends: bool,
    early_pay: bool,
    tentative: bool,
    reminder: bool,
    sticky: bool,
    error: Option<String>,
    focus_description: bool,
}

impl PaymentEditor {
    pub fn set_error(&mut self, error: String) { self.error = Some(error); }

    fn select_recurrence(&mut self, recurrence: Recurrence) {
        if recurrence != Recurrence::Once && !self.recurring_end_initialized {
            self.end_mode = END_MODE_INDEFINITE;
            self.recurring_end_initialized = true;
        }
        self.recurrence = recurrence;
    }

    pub fn new(date: i32) -> Self {
        let mut editor = Self::edit(&Payment::new(date));
        editor.payment_id = None;
        editor.amount_kind = AmountKind::Expense;
        // Start empty so the "0.00" hint shows and a forgotten amount is caught on save.
        editor.amount.clear();
        editor.loan_amount.clear();
        editor
    }

    pub fn edit(payment: &Payment) -> Self {
        Self {
            payment_id: Some(payment.id), original: payment.clone(), description: payment.description.clone(),
            amount: payment.amount.abs().to_string(), amount_kind: AmountKind::from_amount(payment.amount),
            category: payment.category.clone().unwrap_or_default(),
            start_date: sdate(payment.start_date), start_picker: None, recurrence: Recurrence::from_payment(payment),
            recurring_end_initialized: Recurrence::from_payment(payment) != Recurrence::Once,
            frequency: payment.frequency.to_string(), end_mode: payment.end_mode(), end_date: sdate(payment.end_date), end_picker: None,
            count: payment.n_pays.to_string(), loan_amount: payment.loan_amount.to_string(), enabled: !payment.disabled,
            no_weekends: payment.is_no_weekends(), early_pay: payment.is_early_pay(), tentative: payment.is_tentative(),
            reminder: payment.is_reminder(), sticky: payment.is_sticky(), error: None, focus_description: true,
        }
    }

    fn build(&self, db: &BudgetDb) -> Result<Payment, String> {
        if self.description.trim().is_empty() { return Err("Enter a description.".into()); }
        check_text(&self.description, "Description")?;
        check_text(&self.category, "Category")?;
        let mut p = self.original.clone();
        // Keep an untouched legacy description byte-for-byte; trim anything the user typed.
        if self.description != self.original.description { p.description = self.description.trim().to_string(); }
        p.amount = if self.amount.trim().is_empty() && self.reminder {
            0.0
        } else if self.amount.trim().is_empty() {
            return Err("Enter an amount.".into());
        } else { self.amount_kind.signed(&self.amount)? };
        // Preserve Some("") and whitespace in existing categories when untouched.
        if self.category != self.original.category.as_deref().unwrap_or("") {
            p.category = if self.category.trim().is_empty() { None } else { Some(self.category.trim().to_string()) };
        }
        let except = self.payment_id.and_then(|id| db.get_by_id(id));
        if !db.is_valid_description(&p.description, except) {
            return Err("Description is already used as a category.".into());
        }
        if !db.is_valid_category(p.category.as_deref(), except) {
            return Err("Category is reserved or already used as a payment description.".into());
        }
        p.start_date = parse_iso_date(&self.start_date).map_err(|e| format!("Start date: {e}"))?;
        p.freq_type = self.recurrence.freq_type(p.freq_type);
        // The engine emits the start date itself as the first "last weekday" payment, so a start
        // outside the last week of its month would contradict the label: snap it to the last
        // matching weekday (an unchanged start of an existing payment is left alone).
        if p.freq_type == 'L' && (self.original.freq_type != 'L' || p.start_date != self.original.start_date) {
            p.start_date = last_same_weekday_of_month(p.start_date);
        }
        if self.recurrence == Recurrence::Once {
            p.n_pays = 1;
            p.set_end_mode(END_MODE_N_PAYS);
            // An interval is irrelevant for a single payment, but must remain safe for the engine.
            if p.frequency < 1 || p.frequency > max_frequency(&p) { p.frequency = 1; }
        } else {
            p.frequency = if self.recurrence == Recurrence::NthWeekday && self.original.freq_type != 'W' {
                1
            } else { positive_integer(&self.frequency, "Frequency")? };
            // Write just the end-mode bits, retaining all other known and unknown flags. Legacy
            // end modes can be retained unchanged, but cannot be invented by the form.
            if !matches!(self.end_mode, END_MODE_INDEFINITE | END_MODE_DATE | END_MODE_N_PAYS | END_MODE_LOAN_PAID)
                && self.end_mode != self.original.end_mode() {
                return Err("Choose an end condition.".into());
            }
            p.flags = (p.flags & !END_MODE_MASK) | self.end_mode;
            match self.end_mode {
                END_MODE_DATE => {
                    p.end_date = parse_iso_date(&self.end_date).map_err(|e| format!("End date: {e}"))?;
                    if p.end_date < p.start_date { return Err("End date must be on or after the start date.".into()); }
                }
                END_MODE_N_PAYS => { p.n_pays = positive_integer(&self.count, "Number of payments")?; }
                END_MODE_LOAN_PAID => {
                    p.loan_amount = finite_number(&self.loan_amount, "Loan amount")?;
                    if p.loan_amount <= 0.0 { return Err("Loan amount must be greater than zero.".into()); }
                    if p.amount == 0.0 { return Err("A loan needs a nonzero payment amount.".into()); }
                }
                _ => {}
            }
        }
        p.disabled = !self.enabled;
        p.set_no_weekends(self.no_weekends); p.set_early_pay(self.early_pay);
        p.set_tentative(self.tentative); p.set_reminder(self.reminder); p.set_sticky(self.sticky);
        validate_schedule_bounds(&p)?;
        Ok(p)
    }

    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens, db: &BudgetDb) -> PaymentAction {
        if dialog_escape(ctx) { return PaymentAction::Cancel; }
        let mut action = PaymentAction::None;
        let mut enter = false;
        let mut apply = false;
        egui::Modal::new(egui::Id::new("payment_editor")).frame(t.card()).show(ctx, |ui| {
            ui.set_width(560.0_f32.min((ctx.screen_rect().width() - 64.0).max(240.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
            ui.heading(if self.payment_id.is_some() { "Edit payment" } else { "Add payment" });
            ui.label(RichText::new(if self.payment_id.is_some() {
                "Payment schedule · All occurrences"
            } else { "Create a one-time or repeating payment" }).color(t.text_muted));
            ui.add_space(2.0);
            let reserved_height = if self.error.is_some() { 320.0 } else { 250.0 };
            ScrollArea::vertical().id_salt("payment_editor_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height((ctx.screen_rect().height() - reserved_height).max(160.0)).show(ui, |ui| {
                form_section(ui, "Payment details", t, |ui| {
                    let response = text_field(ui, "Description", &mut self.description, "e.g. Rent or salary", t);
                    enter |= submitted(ui, &response);
                    if self.focus_description { response.request_focus(); self.focus_description = false; }
                    let response = amount_field(ui, &mut self.amount, &mut self.amount_kind, t);
                    enter |= submitted(ui, &response);
                    let label = field_label(ui, "Category · Optional", t);
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 110.0).max(100.0);
                        let response = ui.add(TextEdit::singleline(&mut self.category).desired_width(width).hint_text("e.g. Home")).labelled_by(label.id);
                        enter |= submitted(ui, &response);
                        ComboBox::from_id_salt("payment_category_picker").selected_text("Choose…").width(92.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.category, String::new(), "No category");
                            for category in db.categories() { ui.selectable_value(&mut self.category, category.clone(), category); }
                        }).response.labelled_by(label.id);
                    });
                });
                ui.add_space(4.0);
                form_section(ui, "Schedule", t, |ui| {
                    enter |= date_field(ui, "Start date", "payment_start_date", &mut self.start_date, &mut self.start_picker, self.original.start_date, t);
                    let label = field_label(ui, "Repeat", t);
                    let mut recurrence = self.recurrence;
                    ComboBox::from_id_salt("payment_recurrence").width(ui.available_width())
                        .selected_text(recurrence.label()).show_ui(ui, |ui| {
                        for candidate in [Recurrence::Once, Recurrence::Days, Recurrence::Weeks, Recurrence::Months, Recurrence::Years, Recurrence::NthWeekday, Recurrence::LastWeekday] {
                            ui.selectable_value(&mut recurrence, candidate, candidate.label());
                        }
                    }).response.labelled_by(label.id);
                    if recurrence != self.recurrence { self.select_recurrence(recurrence); }
                    if self.recurrence != Recurrence::Once {
                        if self.recurrence == Recurrence::NthWeekday {
                            ui.label(RichText::new("Every month, on the start date’s numbered weekday.").color(t.text_muted));
                        } else {
                            let unit = match self.recurrence {
                                Recurrence::Days => "days", Recurrence::Weeks => "weeks", Recurrence::Years => "years", _ => "months",
                            };
                            let response = text_field(ui, &format!("Every · {unit}"), &mut self.frequency, "1", t);
                            enter |= submitted(ui, &response);
                        }
                        if self.recurrence == Recurrence::LastWeekday {
                            let hint = match parse_iso_date(&self.start_date).map(last_same_weekday_of_month) {
                                Ok(d) => format!("Uses the start date’s weekday in the last week of the month. First payment: {}.", sdate(d)),
                                Err(_) => "Uses the start date’s weekday in the last week of the month.".to_string(),
                            };
                            ui.label(RichText::new(hint).color(t.text_muted));
                        }
                        let label = field_label(ui, "Ends", t);
                        ComboBox::from_id_salt("payment_end_mode").width(ui.available_width()).selected_text(end_label(self.end_mode)).show_ui(ui, |ui| {
                            for (mode, label) in [(END_MODE_INDEFINITE, "Indefinitely"), (END_MODE_DATE, "On a date"), (END_MODE_N_PAYS, "After a number of payments"), (END_MODE_LOAN_PAID, "When loan is paid")] {
                                ui.selectable_value(&mut self.end_mode, mode, label);
                            }
                        }).response.labelled_by(label.id);
                        match self.end_mode {
                            END_MODE_DATE => { enter |= date_field(ui, "End date", "payment_end_date", &mut self.end_date, &mut self.end_picker, self.original.end_date, t); }
                            END_MODE_N_PAYS => {
                                let response = text_field(ui, "Number of payments", &mut self.count, "e.g. 12", t);
                                enter |= submitted(ui, &response);
                            }
                            END_MODE_LOAN_PAID => {
                                let response = text_field(ui, "Loan principal", &mut self.loan_amount, "e.g. 5000.00", t);
                                select_all_on_focus(ui, &response, &self.loan_amount);
                                enter |= submitted(ui, &response);
                                ui.label(RichText::new("Actual payments recalculate the payoff date and future installments.").color(t.text_muted));
                            }
                            _ => {}
                        }
                    }
                    ui.checkbox(&mut self.enabled, "Enabled in calendar");
                    ui.collapsing("More schedule options", |ui| {
                        ui.checkbox(&mut self.no_weekends, "Move weekend payments to a business day");
                        ui.add_enabled_ui(self.no_weekends, |ui| {
                            ui.checkbox(&mut self.early_pay, "Use the previous business day");
                        });
                        ui.checkbox(&mut self.tentative, "Tentative");
                        if self.tentative {
                            ui.label(RichText::new("Paid status also updates this period’s unpaid estimates.").color(t.text_muted));
                        }
                        ui.checkbox(&mut self.reminder, "Reminder");
                        ui.checkbox(&mut self.sticky, "Carry changed amounts forward");
                    });
                });
            });
            if let Some(error) = &self.error { form_error(ui, error, t); }
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "Cancel", t).clicked() { action = PaymentAction::Cancel; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, if self.payment_id.is_some() { "Apply changes" } else { "Add payment" }, t).clicked() {
                        apply = true;
                    }
                });
            });
        });
        if apply || enter {
            match self.build(db) { Ok(p) => action = PaymentAction::Apply(p), Err(e) => self.error = Some(e) }
        }
        // Clicking the backdrop retains the draft; Escape and Cancel discard it (all editors alike).
        action
    }
}

pub struct TransactionEditor {
    pub payment_id: i32,
    pub date: i32,
    description: String,
    loan_payment: bool,
    tentative_payment: bool,
    amount: String,
    amount_kind: AmountKind,
    note: String,
    flag: char,
    scope: TransactionScope,
    error: Option<String>,
    focus_amount: bool,
}
impl TransactionEditor {
    pub fn set_error(&mut self, error: String) { self.error = Some(error); }

    pub fn new(payment: &Payment, date: i32, amount: f64) -> Self {
        Self { payment_id: payment.id, date, description: payment.description.clone(), amount: amount.abs().to_string(),
            amount_kind: AmountKind::from_amount(amount),
            loan_payment: payment.end_mode() == END_MODE_LOAN_PAID,
            tentative_payment: payment.is_tentative(),
            note: payment.get_note(date).unwrap_or_default(), flag: payment.get_flag(date),
            scope: TransactionScope::Occurrence, error: None, focus_amount: true }
    }
    fn values(&self) -> Result<(f64, String, char), String> {
        let amount = self.amount_kind.signed(&self.amount)?;
        check_text(&self.note, "Note")?;
        // Unknown legacy flags remain unchanged until the user explicitly chooses a new status.
        Ok((amount, self.note.clone(), self.flag))
    }
    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens) -> TransactionAction {
        if dialog_escape(ctx) { return TransactionAction::Cancel; }
        let mut action = TransactionAction::None;
        let mut enter = false;
        let mut apply = false;
        egui::Modal::new(egui::Id::new("transaction_editor")).frame(t.card()).show(ctx, |ui| {
            ui.set_width(520.0_f32.min((ctx.screen_rect().width() - 64.0).max(240.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
            ui.horizontal(|ui| {
                ui.heading("Edit transaction");
                ui.label(RichText::new(sdate(self.date)).color(t.text_muted));
            });
            ui.label(RichText::new(&self.description).strong().color(t.text));
            let reserved_height = if self.error.is_some() { 360.0 } else { 290.0 };
            ScrollArea::vertical().id_salt("transaction_editor_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height((ctx.screen_rect().height() - reserved_height).max(160.0)).show(ui, |ui| {
                form_section(ui, "Transaction details", t, |ui| {
                    let response = amount_field(ui, &mut self.amount, &mut self.amount_kind, t);
                    enter |= submitted(ui, &response);
                    if self.focus_amount { response.request_focus(); self.focus_amount = false; }
                    let label = field_label(ui, "Apply amount to", t);
                    ui.radio_value(&mut self.scope, TransactionScope::Occurrence, "Only this occurrence").labelled_by(label.id);
                    ui.radio_value(&mut self.scope, TransactionScope::Future, "This and all future occurrences").labelled_by(label.id);
                    let help = match self.scope {
                        TransactionScope::Occurrence => format!("Change the amount for {} only.", sdate(self.date)),
                        TransactionScope::Future => format!("Change amounts from {} onward. Earlier dates keep their amounts.", sdate(self.date)),
                    };
                    ui.label(RichText::new(help).color(t.text_muted));
                    ui.label(RichText::new(format!("Notes and status apply only to {}.", sdate(self.date))).color(t.text_muted));
                    let label = field_label(ui, "Status · Selected date only", t);
                    ComboBox::from_id_salt("transaction_status").width(ui.available_width()).selected_text(flag_label(self.flag)).show_ui(ui, |ui| {
                        for (flag, label) in [(FLAG_NONE, "None"), (FLAG_PAID, "Paid"), (FLAG_IMPORTANT, "Important")] {
                            ui.selectable_value(&mut self.flag, flag, label);
                        }
                    }).response.labelled_by(label.id);
                    let label = field_label(ui, "Note · Optional · Selected date only", t);
                    ui.add(TextEdit::multiline(&mut self.note).desired_width(f32::INFINITY).desired_rows(3)
                        .hint_text("Add a note for this date")).labelled_by(label.id);
                });
                if self.loan_payment {
                    ui.label(RichText::new("Amount changes and deletions recalculate the loan payoff and future installments.").color(t.text_muted));
                }
                if self.tentative_payment {
                    ui.label(RichText::new("For tentative payments, paid status also updates this period’s unpaid estimates.").color(t.text_muted));
                }
            });
            if let Some(error) = &self.error { form_error(ui, error, t); }
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "Cancel", t).clicked() { action = TransactionAction::Cancel; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::primary_button(ui, "Apply changes", t).clicked() { apply = true; }
                });
            });
            ui.separator();
            if ui.add(egui::Button::new(RichText::new("Delete this occurrence…").color(t.negative))
                .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)).clicked() {
                action = TransactionAction::Delete;
            }
        });
        if (apply || enter) && matches!(action, TransactionAction::None) {
            match self.values() {
                Ok((amount, note, flag)) => action = TransactionAction::Apply { amount, note, flag, scope: self.scope },
                Err(e) => self.error = Some(e),
            }
        }
        action
    }
}

fn form_section(ui: &mut egui::Ui, title: &str, t: &Tokens, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(t.bg).corner_radius(8).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(t.text));
        ui.add_space(2.0);
        body(ui);
    });
}
fn field_label(ui: &mut egui::Ui, label: &str, t: &Tokens) -> egui::Response {
    ui.label(RichText::new(label).color(t.text_muted))
}
fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, t: &Tokens) -> egui::Response {
    let label = field_label(ui, label, t);
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY).hint_text(hint)).labelled_by(label.id)
}
/// Escape cancels a dialog, unless a popup (combo box, date picker) is open and should close first.
pub(crate) fn dialog_escape(ctx: &egui::Context) -> bool {
    !ctx.memory(|m| m.any_popup_open()) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}
/// True when a single-line field just lost focus because Enter was pressed.
pub(crate) fn submitted(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
/// Select the whole text when a field gains focus so typing replaces it.
fn select_all_on_focus(ui: &egui::Ui, response: &egui::Response, text: &str) {
    if response.gained_focus() {
        if let Some(mut state) = TextEdit::load_state(ui.ctx(), response.id) {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()),
            )));
            state.store(ui.ctx(), response.id);
        }
    }
}
fn amount_field(ui: &mut egui::Ui, amount: &mut String, kind: &mut AmountKind, t: &Tokens) -> egui::Response {
    let label = field_label(ui, "Amount", t);
    ui.horizontal(|ui| {
        ui.selectable_value(kind, AmountKind::Expense, "Expense").labelled_by(label.id);
        ui.selectable_value(kind, AmountKind::Income, "Income").labelled_by(label.id);
        ui.add_space(6.0);
        let response = ui.add(TextEdit::singleline(amount).desired_width(ui.available_width()).hint_text("0.00")
            .horizontal_align(egui::Align::RIGHT)).labelled_by(label.id);
        select_all_on_focus(ui, &response, amount);
        response
    }).inner
}
fn form_error(ui: &mut egui::Ui, error: &str, t: &Tokens) {
    egui::Frame::new().fill(theme::tint(t.surface, t.negative, 0.08)).corner_radius(6)
        .inner_margin(10).show(ui, |ui| {
            ui.label(RichText::new(error).color(t.negative));
        });
}
fn date_field(ui: &mut egui::Ui, label: &str, id: &str, value: &mut String, picker: &mut Option<DatePicker>, fallback: i32, t: &Tokens) -> bool {
    let label = field_label(ui, label, t);
    let mut entered = false;
    let button = ui.horizontal(|ui| {
        let width = (ui.available_width() - 108.0).max(100.0);
        let response = ui.add(TextEdit::singleline(value).desired_width(width).hint_text("YYYY-MM-DD")).labelled_by(label.id);
        entered = submitted(ui, &response);
        theme::secondary_button(ui, "Pick date", t).labelled_by(label.id)
    }).inner;
    let popup_id = ui.make_persistent_id(id);
    if button.clicked() {
        *picker = Some(DatePicker::new(value, fallback));
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }
    egui::popup::popup_below_widget(ui, popup_id, &button, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        if let Some(picker) = picker {
            if let Some(date) = date_picker(ui, picker, value, t) {
                *value = sdate(date);
                ui.memory_mut(|memory| memory.close_popup());
            }
        }
    });
    entered
}
fn date_picker(ui: &mut egui::Ui, picker: &mut DatePicker, selected: &str, t: &Tokens) -> Option<i32> {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    ui.set_width(266.0);
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
    let mut chosen = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(picker.month.get() > 10000101, egui::Button::new("‹")).on_hover_text("Previous month").clicked() {
            picker.shift_month(-1);
        }
        ui.add_sized([190.0, 34.0], egui::Label::new(RichText::new(format!("{} {}", MONTHS[picker.month.month() as usize], picker.month.year())).strong()));
        if ui.add_enabled(picker.month.get() < 99991201, egui::Button::new("›")).on_hover_text("Next month").clicked() {
            picker.shift_month(1);
        }
    });
    Grid::new("date_picker_grid").spacing([4.0, 4.0]).show(ui, |ui| {
        for day in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
            ui.add_sized([34.0, 24.0], egui::Label::new(RichText::new(day).color(t.text_muted)));
        }
        ui.end_row();
        let offset = picker.month.day_of_week();
        for cell in 0..42 {
            let day = cell - offset + 1;
            if let Some(date) = picker.choose(day) {
                let selected = parse_iso_date(selected).ok() == Some(date);
                if ui.add_sized([34.0, 32.0], egui::Button::new(day.to_string()).selected(selected))
                    .on_hover_text(sdate(date)).clicked() { chosen = Some(date); }
            } else { ui.allocate_space(egui::vec2(34.0, 32.0)); }
            if cell % 7 == 6 { ui.end_row(); }
        }
    });
    if theme::secondary_button(ui, "Today", t).clicked() { chosen = Some(Cal::today().get()); }
    chosen
}
fn end_label(mode: i32) -> &'static str {
    match mode { END_MODE_INDEFINITE => "Indefinitely", END_MODE_DATE => "On a date", END_MODE_N_PAYS => "After a number of payments", END_MODE_LOAN_PAID => "When loan is paid", _ => "Existing end condition" }
}
fn flag_label(flag: char) -> &'static str {
    match flag { FLAG_NONE => "None", FLAG_PAID => "Paid", FLAG_IMPORTANT => "Important", _ => "Existing status" }
}
fn check_text(text: &str, label: &str) -> Result<(), String> {
    if text.chars().any(|c| c as u32 > 0xffff) {
        Err(format!("{label} contains a character this budget format cannot save. Remove emoji and other unsupported symbols."))
    } else { Ok(()) }
}
/// Largest absolute amount or balance accepted; keeps running balances far from f64 overflow.
pub(crate) const MAX_ABS_AMOUNT: f64 = 1e12;
pub(crate) fn amount_in_range(v: f64) -> bool { v.is_finite() && v.abs() <= MAX_ABS_AMOUNT }

/// The single amount parser for payment, transaction, loan and balance fields. Accepts an optional
/// sign, a `$`, well-formed thousands commas ("1,234.50") and accounting parentheses ("(50)" is -50).
pub(crate) fn parse_amount(text: &str, label: &str) -> Result<f64, String> {
    let generic = || format!("{label} must be a number, for example 1,250.00.");
    let mut body = text.trim();
    let mut negate = false;
    if let Some(inner) = body.strip_prefix('(').and_then(|b| b.strip_suffix(')')) { body = inner.trim(); negate = true; }
    let sign = match body.chars().next() { Some(c @ ('+' | '-')) => { body = &body[1..]; Some(c) } _ => None };
    if let Some(rest) = body.strip_prefix('$') { body = rest; }
    if body.contains(',') {
        let (int_part, frac_part) = body.split_once('.').unwrap_or((body, ""));
        let groups: Vec<&str> = int_part.split(',').collect();
        let well_formed = !frac_part.contains(',')
            && (1..=3).contains(&groups[0].len())
            && groups.iter().all(|g| g.bytes().all(|b| b.is_ascii_digit()))
            && groups[1..].iter().all(|g| g.len() == 3);
        if !well_formed {
            return Err(format!("{label} may use commas only as thousands separators, for example 1,234.50."));
        }
    }
    let cleaned = format!("{}{}", sign.map(String::from).unwrap_or_default(), body.replace(',', ""));
    let value = match cleaned.parse::<f64>() {
        Ok(v) if v.is_finite() => v,
        _ => return Err(generic()),
    };
    let value = if negate { if sign.is_some() { return Err(generic()); } -value } else { value };
    if value.abs() > MAX_ABS_AMOUNT {
        return Err(format!("{label} is too large (maximum 1,000,000,000,000)."));
    }
    Ok(value)
}
fn finite_number(text: &str, label: &str) -> Result<f64, String> { parse_amount(text, label) }
fn positive_integer(text: &str, label: &str) -> Result<i32, String> {
    match text.trim().parse::<i32>() { Ok(v) if v > 0 => Ok(v), _ => Err(format!("{label} must be a positive whole number.")) }
}
/// The last date in the month of `date` that falls on the same weekday as `date`.
fn last_same_weekday_of_month(date: i32) -> i32 {
    let mut c = Cal::from_int(date);
    loop {
        let mut next = c;
        next.add(Field::Day, 7);
        if next.month() != c.month() { return c.get(); }
        c = next;
    }
}

fn parse_iso_date(text: &str) -> Result<i32, String> {
    let text = text.trim();
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-'
        || !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return Err("Use YYYY-MM-DD.".into());
    }
    let year: i32 = text[..4].parse().map_err(|_| "Invalid year.")?;
    let month: i32 = text[5..7].parse().map_err(|_| "Invalid month.")?;
    let day: i32 = text[8..].parse().map_err(|_| "Invalid day.")?;
    if !(1000..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err("Enter a real date in years 1000–9999.".into());
    }
    let date = year * 10000 + month * 100 + day;
    if Cal::ymd(year, month - 1, day).get() != date { return Err("This date does not exist.".into()); }
    Ok(date)
}

// Bound all values before the scheduler multiplies intervals or loops through finite payments.
fn period_span(p: &Payment) -> i32 {
    let start = Cal::from_int(p.start_date);
    match p.freq_type {
        'd' => Cal::diff(&start, &Cal::from_int(99991231)),
        'w' => Cal::diff(&start, &Cal::from_int(99991231)) / 7,
        'y' => 9999 - start.year(),
        _ => (9999 - start.year()) * 12 + 11 - start.month(),
    }
}
fn max_frequency(p: &Payment) -> i32 { period_span(p).max(1) }
pub(crate) fn validate_schedule_bounds(p: &Payment) -> Result<(), String> {
    if p.frequency <= 0 || p.frequency > max_frequency(p) {
        return Err("Frequency would exceed the supported calendar range (through year 9999).".into());
    }
    // The engine's numbered-weekday recurrence is monthly regardless of its stored interval.
    let frequency = if p.freq_type == 'W' { 1 } else { p.frequency };
    let max_count = period_span(p) / frequency + 1;
    match p.end_mode() {
        END_MODE_N_PAYS if p.n_pays <= 0 || p.n_pays > max_count => {
            return Err("Number of payments would extend beyond year 9999.".into());
        }
        END_MODE_LOAN_PAID => {
            let sum: f64 = p.opt.iter().filter(|o| !o.deleted).map(|o| o.amount).sum();
            let remaining = (p.loan_amount.abs() - if p.amount < 0.0 { -sum } else { sum }).max(0.0);
            // Every exception can displace one regular payment. This conservative upper bound
            // prevents a huge/near-zero loan from locking the scheduler or overflowing dates.
            let count = (remaining / p.amount.abs()).ceil() + p.opt.len() as f64;
            if !count.is_finite() || count > max_count as f64 {
                return Err("Loan would require payments beyond year 9999; increase the payment or reduce the loan amount.".into());
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::*;

    fn transaction_context() -> (egui::Context, Tokens) {
        let ctx = egui::Context::default();
        let t = Tokens::new(&config::BudgetConfig::default().colors, false);
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &t);
        (ctx, t)
    }

    fn transaction_frame(ctx: &egui::Context, t: &Tokens, editor: &mut TransactionEditor,
        size: egui::Vec2, events: Vec<egui::Event>) -> (egui::FullOutput, TransactionAction) {
        let mut action = TransactionAction::None;
        let output = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default()
        }, |ctx| { action = editor.show(ctx, t); });
        (output, action)
    }

    fn transaction_label(output: &egui::FullOutput, label: &str) -> egui::Rect {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        }).unwrap_or_else(|| panic!("Transaction control must be visible: {label}"))
    }

    fn click_transaction_label(ctx: &egui::Context, t: &Tokens, editor: &mut TransactionEditor,
        size: egui::Vec2, label: &str) -> TransactionAction {
        // Centered modals need a layout frame after validation changes their height.
        for _ in 0..2 { transaction_frame(ctx, t, editor, size, Vec::new()); }
        let (output, _) = transaction_frame(ctx, t, editor, size, Vec::new());
        let pos = transaction_label(&output, label).center();
        transaction_frame(ctx, t, editor, size, vec![egui::Event::PointerMoved(pos)]);
        transaction_frame(ctx, t, editor, size, vec![egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE,
        }]);
        transaction_frame(ctx, t, editor, size, vec![egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE,
        }]).1
    }

    #[test]
    fn transaction_scope_choices_preserve_the_typed_draft() {
        // Clearing or recreating the draft when choosing scope would lose these edits.
        let (ctx, t) = transaction_context();
        let size = egui::vec2(1200.0, 1000.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        editor.amount = "250.75".into(); editor.note = "Pantry restock".into(); editor.flag = FLAG_IMPORTANT;
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        for label in ["This and all future occurrences", "Only this occurrence"] {
            click_transaction_label(&ctx, &t, &mut editor, size, label);
            assert_eq!(editor.values().unwrap(), (-250.75, "Pantry restock".into(), FLAG_IMPORTANT));
        }
    }

    #[test]
    fn transaction_apply_emits_the_default_and_chosen_amount_scope() {
        // Applying must use the visible choice, including changing back from Future.
        let (ctx, t) = transaction_context();
        let size = egui::vec2(1200.0, 1000.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        editor.amount = "250".into(); editor.note = "Weekly groceries".into(); editor.flag = FLAG_PAID;
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        for (choice, expected) in [(None, TransactionScope::Occurrence),
            (Some("This and all future occurrences"), TransactionScope::Future),
            (Some("Only this occurrence"), TransactionScope::Occurrence)] {
            if let Some(label) = choice { click_transaction_label(&ctx, &t, &mut editor, size, label); }
            let action = click_transaction_label(&ctx, &t, &mut editor, size, "Apply changes");
            let TransactionAction::Apply { amount, note, flag, scope } = action else { panic!("Valid draft must apply"); };
            assert_eq!((amount, note, flag, scope), (-250.0, "Weekly groceries".into(), FLAG_PAID, expected));
        }
    }

    #[test]
    fn transaction_validation_and_document_errors_retain_the_future_draft() {
        // A failed apply must keep scope and edits available for correction/retry.
        let (ctx, t) = transaction_context();
        let size = egui::vec2(1200.0, 1000.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        editor.amount = "NaN".into(); editor.note = "Pantry restock".into(); editor.flag = FLAG_IMPORTANT;
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        click_transaction_label(&ctx, &t, &mut editor, size, "This and all future occurrences");
        assert!(matches!(click_transaction_label(&ctx, &t, &mut editor, size, "Apply changes"), TransactionAction::None));
        assert!(editor.error.is_some());
        assert_eq!((&*editor.amount, &*editor.note, editor.flag, editor.scope),
            ("NaN", "Pantry restock", FLAG_IMPORTANT, TransactionScope::Future));
        editor.amount = "250".into();
        editor.set_error("The budget changed on disk. Reload it before saving.".into());
        let action = click_transaction_label(&ctx, &t, &mut editor, size, "Apply changes");
        let TransactionAction::Apply { amount, note, flag, scope } = action else { panic!("Corrected draft must apply"); };
        assert_eq!((amount, note, flag, scope), (-250.0, "Pantry restock".into(), FLAG_IMPORTANT, TransactionScope::Future));
    }

    #[test]
    fn deleting_with_future_amount_selected_still_emits_single_occurrence_delete() {
        let (ctx, t) = transaction_context();
        let size = egui::vec2(900.0, 600.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        click_transaction_label(&ctx, &t, &mut editor, size, "This and all future occurrences");
        assert!(matches!(click_transaction_label(&ctx, &t, &mut editor, size, "Delete this occurrence…"), TransactionAction::Delete));
    }

    #[test]
    fn compact_transaction_keeps_scope_and_footer_visible_with_errors() {
        // Unbounded content or insufficient footer space would clip these controls at 900×600.
        let size = egui::vec2(900.0, 600.0);
        for dark in [false, true] {
            for error in [None, Some("Note contains a character this budget format cannot save. Remove emoji and other unsupported symbols.")] {
                let (ctx, _) = transaction_context();
                let t = Tokens::new(&config::Colors::theme(if dark { config::Theme::Dark } else { config::Theme::Light }), dark);
                theme::apply(&ctx, &t);
                let mut payment = Payment::new(20261027);
                payment.description = "A long grocery payment description that wraps onto another line in this compact transaction editor".into();
                let mut editor = TransactionEditor::new(&payment, 20261027, -150.0);
                if let Some(error) = error { editor.set_error(error.into()); }
                for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
                let (output, _) = transaction_frame(&ctx, &t, &mut editor, size, Vec::new());
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for label in ["2026-10-27", "Only this occurrence", "This and all future occurrences", "Cancel", "Apply changes", "Delete this occurrence…"] {
                    let rect = transaction_label(&output, label);
                    assert!(screen.contains_rect(rect), "Control {label} outside compact viewport: {rect:?}");
                }
                let modal = ctx.memory(|memory| memory.area_rect(egui::Id::new("transaction_editor"))).unwrap();
                assert!(screen.contains_rect(modal), "Compact modal outside viewport (dark={dark}, error={}): {modal:?}", error.is_some());
            }
        }
    }

    fn db() -> BudgetDb { BudgetDb::new(&Cal::from_int(20261001), 12, false) }
    fn valid_editor() -> PaymentEditor {
        let mut p = Payment::new(20261027);
        p.description = "Groceries".into();
        p.amount = -25.50;
        PaymentEditor::edit(&p)
    }

    #[test]
    fn description_must_contain_non_whitespace() {
        let db = db();
        let mut e = valid_editor();
        e.description = " \n\t".into();
        assert!(e.build(&db).is_err());
    }

    #[test]
    fn amounts_must_be_finite_numbers() {
        let db = db();
        for text in ["NaN", "inf", "-inf", "1e999", "", "£12"] {
            let mut e = valid_editor(); e.amount = text.into();
            assert!(e.build(&db).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn dates_reject_normalization_and_require_four_digit_iso_year() {
        for text in ["2026-02-29", "2026-04-31", "2026-00-01", "2026-13-01", "2026-01-00", "2026-1-1", "20261027", "0099-01-01", "0999-12-31", "10000-01-01"] {
            assert!(parse_iso_date(text).is_err(), "accepted {text:?}");
        }
        assert_eq!(parse_iso_date("2024-02-29").unwrap(), 20240229);
        assert_eq!(parse_iso_date("1000-01-01").unwrap(), 10000101);
    }

    #[test]
    fn end_date_cannot_precede_start() {
        let mut e = valid_editor(); e.recurrence = Recurrence::Months;
        e.end_mode = END_MODE_DATE; e.end_date = "2026-10-26".into();
        assert!(e.build(&db()).is_err());
    }

    #[test]
    fn recurrence_requires_positive_frequency_and_count() {
        let db = db();
        for (frequency, count) in [("0", "1"), ("-1", "1"), ("1", "0"), ("1", "-3"), ("abc", "2")] {
            let mut e = valid_editor(); e.recurrence = Recurrence::Months;
            e.end_mode = END_MODE_N_PAYS; e.frequency = frequency.into(); e.count = count.into();
            assert!(e.build(&db).is_err(), "accepted frequency={frequency}, count={count}");
        }
    }

    #[test]
    fn finite_schedule_cannot_overflow_calendar_or_integer() {
        let db = db();
        for (recurrence, frequency, count) in [(Recurrence::Years, "1", "99999"), (Recurrence::Days, "2147483647", "2"), (Recurrence::Weeks, "2147483647", "2")] {
            let mut e = valid_editor(); e.recurrence = recurrence; e.end_mode = END_MODE_N_PAYS;
            e.frequency = frequency.into(); e.count = count.into();
            assert!(e.build(&db).is_err());
        }
    }

    #[test]
    fn loan_requires_positive_finite_principal_and_nonzero_payment() {
        let db = db();
        for (amount, loan) in [("0", "100"), ("10", "0"), ("10", "-100"), ("10", "NaN"), ("0.000000001", "1e99")] {
            let mut e = valid_editor(); e.recurrence = Recurrence::Months;
            e.end_mode = END_MODE_LOAN_PAID; e.amount = amount.into(); e.loan_amount = loan.into();
            assert!(e.build(&db).is_err(), "accepted amount={amount}, loan={loan}");
        }
    }

    #[test]
    fn unsupported_non_bmp_input_is_rejected_in_all_editable_text() {
        let db = db();
        let mut e = valid_editor(); e.description = "Gift 🎁".into();
        assert!(e.build(&db).is_err());
        e.description = "Gift".into(); e.category = "Fun 🎉".into();
        assert!(e.build(&db).is_err());
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, -5.0);
        tx.note = "Paid 👍".into(); assert!(tx.values().is_err());
    }

    #[test]
    fn namespace_validation_uses_core_rules_and_excludes_current_payment() {
        let mut db = db();
        let mut p = Payment::new(20261027); p.description = "Income".into(); p.category = Some("Pay".into());
        let i = db.add(p);
        assert!(PaymentEditor::edit(&db.payments[i]).build(&db).is_ok());
        let mut e = valid_editor(); e.description = "Pay".into(); assert!(e.build(&db).is_err());
        e.description = "Food".into(); e.category = "Income".into(); assert!(e.build(&db).is_err());
        e.category = "(Unaccounted)".into(); assert!(e.build(&db).is_err());
    }

    #[test]
    fn editing_preserves_all_unexposed_fields_and_calendar_references() {
        let mut db = db();
        let mut p = Payment::new(20261027);
        p.description = "Original".into(); p.amount = -20.0;
        p.frequency = 2; p.freq_type = 'w'; p.n_pays = 8;
        p.flags |= 0x4000 | NO_WEEKENDS | TENTATIVE | EARLY_PAY | REMINDER | IS_STICKY;
        p.category = Some("Household".into()); p.loan_amount = 1234.5;
        p.image_path = Some("icons/a.png".into()); p.flag_value = FLAG_IMPORTANT; p.grace_period_days = 7;
        p.set_custom_amount(20261110, -30.0); p.set_no_transaction(20261124);
        p.set_flag(20261110, FLAG_PAID); p.set_note(20261110, Some("Kept note"));
        p.set_default_highlighting(0x112233); p.set_hilighting(20261110, 0x445566);
        let i = db.add(p);
        let mut expected = db.payments[i].clone();
        let mut e = PaymentEditor::edit(&expected);
        e.description = "Changed".into(); e.amount = "42.25".into();
        expected.description = "Changed".into(); expected.amount = -42.25;
        assert_eq!(e.build(&db).unwrap(), expected);
    }

    #[test]
    fn once_and_recurring_fields_create_expected_schedule() {
        let db = db();
        let mut e = valid_editor(); e.recurrence = Recurrence::Once;
        let once = e.build(&db).unwrap();
        assert_eq!(once.amount, -25.50); assert_eq!(once.n_pays, 1); assert_eq!(once.end_mode(), END_MODE_N_PAYS);
        e.recurrence = Recurrence::LastWeekday; e.frequency = "2".into(); e.end_mode = END_MODE_N_PAYS; e.count = "3".into();
        let p = e.build(&db).unwrap();
        assert_eq!((p.freq_type, p.frequency, p.n_pays, p.start_date), ('L', 2, 3, 20261027));
    }

    #[test]
    fn last_weekday_start_snaps_to_last_matching_weekday() {
        let db = db();
        let mut e = valid_editor();
        e.recurrence = Recurrence::LastWeekday; e.frequency = "1".into(); e.end_mode = END_MODE_INDEFINITE;
        e.start_date = "2026-10-13".into(); // a Tuesday mid-month
        assert_eq!(e.build(&db).unwrap().start_date, 20261027);
        e.start_date = "2026-02-03".into();
        assert_eq!(e.build(&db).unwrap().start_date, 20260224);
    }

    #[test]
    fn transaction_loads_only_selected_dates_note_and_flag() {
        let mut p = Payment::new(20261027); p.id = 7; p.amount = -10.0;
        p.set_note(20261027, Some("First")); p.set_flag(20261027, FLAG_PAID);
        p.set_note(20261127, Some("Other")); p.set_flag(20261127, FLAG_IMPORTANT);
        let e = TransactionEditor::new(&p, 20261027, -12.5);
        assert_eq!((e.payment_id, e.date), (7, 20261027));
        assert_eq!(e.values().unwrap(), (-12.5, "First".into(), FLAG_PAID));
        assert_eq!(p.get_note(20261127).as_deref(), Some("Other"));
    }

    #[test]
    fn transaction_amount_and_flag_are_validated() {
        let mut e = TransactionEditor::new(&Payment::new(20261027), 20261027, 1.0);
        for amount in ["NaN", "-inf", "not money"] { e.amount = amount.into(); assert!(e.values().is_err()); }
        e.amount = "12.25".into(); e.amount_kind = AmountKind::Expense; e.flag = FLAG_IMPORTANT;
        assert_eq!(e.values().unwrap(), (-12.25, String::new(), FLAG_IMPORTANT));
    }

    #[test]
    fn new_payments_use_positive_entry_for_an_expense() {
        let mut e = PaymentEditor::new(20261027);
        e.description = "Weekly shop".into();
        e.amount = "42.25".into();
        assert_eq!(e.build(&db()).unwrap().amount, -42.25);
    }

    #[test]
    fn expense_editor_rejects_signed_input() {
        let mut e = valid_editor();
        e.amount = "-42.25".into();
        assert!(e.build(&db()).is_err());
    }

    #[test]
    fn choosing_monthly_from_once_repeats_indefinitely() {
        let mut e = PaymentEditor::new(20261027);
        e.description = "Rent".into(); e.amount = "1200".into();
        e.select_recurrence(Recurrence::Months);
        let payment = e.build(&db()).unwrap();
        assert_eq!(payment.freq_type, 'm');
        assert_eq!(payment.end_mode(), END_MODE_INDEFINITE);
    }

    #[test]
    fn toggling_once_preserves_a_chosen_recurring_end_condition() {
        let mut p = Payment::new(20261027); p.description = "Rent".into();
        p.set_end_mode(END_MODE_DATE); p.end_date = 20271027;
        let mut e = PaymentEditor::edit(&p);
        e.select_recurrence(Recurrence::Once);
        e.select_recurrence(Recurrence::Months);
        assert_eq!(e.build(&db()).unwrap().end_date, 20271027);
        assert_eq!(e.build(&db()).unwrap().end_mode(), END_MODE_DATE);
        let mut e = PaymentEditor::new(20261027);
        e.description = "Rent".into(); e.amount = "10".into();
        e.select_recurrence(Recurrence::Months);
        e.end_mode = END_MODE_N_PAYS; e.count = "6".into();
        e.select_recurrence(Recurrence::Once);
        e.select_recurrence(Recurrence::Months);
        let payment = e.build(&db()).unwrap();
        assert_eq!((payment.end_mode(), payment.n_pays), (END_MODE_N_PAYS, 6));
    }

    #[test]
    fn signed_amounts_round_trip_unchanged_and_can_change_direction() {
        for amount in [-25.50, 25.50, -0.0, 0.0, -f64::MIN_POSITIVE, f64::MIN_POSITIVE] {
            let mut p = Payment::new(20261027); p.description = "Value".into(); p.amount = amount;
            assert_eq!(PaymentEditor::edit(&p).build(&db()).unwrap().amount.to_bits(), amount.to_bits());
            let tx = TransactionEditor::new(&p, 20261027, amount);
            assert_eq!(tx.values().unwrap().0.to_bits(), amount.to_bits());
        }
        let mut e = valid_editor(); e.amount_kind = AmountKind::Income;
        assert_eq!(e.build(&db()).unwrap().amount, 25.50);
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, 42.25);
        tx.amount_kind = AmountKind::Expense;
        assert_eq!(tx.values().unwrap().0, -42.25);
        tx.amount = "-42.25".into();
        assert!(tx.values().is_err());
    }

    #[test]
    fn picker_uses_typed_month_and_offers_only_real_dates() {
        let picker = DatePicker::new("2024-02-12", 20261027);
        assert_eq!(picker.choose(29), Some(20240229));
        assert_eq!(picker.choose(30), None);
        assert_eq!(picker.choose(0), None);
        let picker = DatePicker::new("2026-02-29", 20261027);
        assert_eq!(picker.choose(1), Some(20261001));
    }

    #[test]
    fn picker_month_navigation_crosses_years_within_supported_dates() {
        let mut picker = DatePicker::new("2026-12-31", 20261231);
        picker.shift_month(1);
        assert_eq!(picker.choose(1), Some(20270101));
        picker.shift_month(-2);
        assert_eq!(picker.choose(30), Some(20261130));
        let mut first = DatePicker::new("1000-01-01", 10000101);
        first.shift_month(-1);
        assert_eq!(first.choose(1), Some(10000101));
        let mut last = DatePicker::new("9999-12-31", 99991231);
        last.shift_month(1);
        assert_eq!(last.choose(31), Some(99991231));
    }

    #[test]
    fn picker_with_an_unset_legacy_end_date_still_opens_on_a_valid_month() {
        let picker = DatePicker::new("0000-00-00", 0);
        let chosen = picker.choose(1).unwrap();
        assert!(parse_iso_date(&sdate(chosen)).is_ok());
    }

    #[test]
    fn new_payment_starts_with_empty_amount_fields_and_requires_an_amount() {
        let mut e = PaymentEditor::new(20261027);
        assert!(e.amount.is_empty() && e.loan_amount.is_empty());
        e.description = "Rent".into();
        assert!(e.build(&db()).is_err(), "A forgotten amount must not save as 0.00");
        e.amount = "0".into();
        assert_eq!(e.build(&db()).unwrap().amount, -0.0);
        e.amount.clear(); e.reminder = true;
        assert_eq!(e.build(&db()).unwrap().amount, 0.0);
    }

    #[test]
    fn amounts_accept_grouping_dollar_and_parentheses_but_reject_ambiguity() {
        for (text, expected) in [("1,200.50", 1200.5), ("$1200", 1200.0), ("-$1,234.50", -1234.5), ("+5", 5.0),
            ("(50)", -50.0), ("1,234,567", 1234567.0), ("1e3", 1000.0), (" 12 ", 12.0)] {
            assert_eq!(parse_amount(text, "Amount").unwrap(), expected, "{text}");
        }
        for text in ["1,5", "1,23", "12,34.5", "1,2345", ",500", "1,000,00", "1.000,50", "$", "(5", "-(5)", "1 200", "£12"] {
            assert!(parse_amount(text, "Amount").is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn amounts_beyond_the_maximum_are_rejected_with_a_clear_message() {
        for text in ["1e308", "1000000000001", "-1e300"] {
            let err = parse_amount(text, "Amount").unwrap_err();
            assert!(err.contains("too large"), "{text}: {err}");
        }
        assert_eq!(parse_amount("1000000000000", "Amount").unwrap(), 1e12);
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, 1.0);
        tx.amount = "1e308".into();
        assert!(tx.values().is_err());
    }

    #[test]
    fn dates_trim_whitespace_and_new_descriptions_are_stored_trimmed() {
        assert_eq!(parse_iso_date(" 2026-10-27 ").unwrap(), 20261027);
        let mut e = PaymentEditor::new(20261027);
        e.description = "Rent ".into(); e.amount = "5".into(); e.start_date = "2026-10-27 ".into();
        let p = e.build(&db()).unwrap();
        assert_eq!((p.description.as_str(), p.start_date), ("Rent", 20261027));
        let mut legacy = Payment::new(20261027); legacy.description = "Old ".into(); legacy.amount = -1.0;
        assert_eq!(PaymentEditor::edit(&legacy).build(&db()).unwrap().description, "Old ");
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]
    }

    #[test]
    fn escape_cancels_and_enter_in_the_amount_field_submits() {
        let (ctx, t) = transaction_context();
        let size = egui::vec2(1200.0, 1000.0);
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut tx, size, Vec::new()); }
        assert!(matches!(transaction_frame(&ctx, &t, &mut tx, size, key(egui::Key::Enter)).1, TransactionAction::Apply { .. }));
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        assert!(matches!(transaction_frame(&ctx, &t, &mut tx, size, key(egui::Key::Escape)).1, TransactionAction::Cancel));
        let mut e = PaymentEditor::new(20261027);
        let mut action = PaymentAction::None;
        let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events: key(egui::Key::Escape), ..Default::default() }, |ctx| { action = e.show(ctx, &t, &db()); });
        assert!(matches!(action, PaymentAction::Cancel));
    }
}
