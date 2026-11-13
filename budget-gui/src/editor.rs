//! Payment and transaction drafts. Dialogs return values without mutating the database.
use budget_core::date::{sdate, DATE_MAX, DATE_MIN, YEAR_MAX, YEAR_MIN};
use budget_core::{BudgetDb, Cal, Field, Payment, END_MODE_DATE, END_MODE_INDEFINITE,
    END_MODE_LOAN_PAID, END_MODE_MASK, END_MODE_N_PAYS, FLAG_IMPORTANT, FLAG_NONE, FLAG_PAID};
use eframe::egui::{self, ComboBox, Grid, RichText, ScrollArea, TextEdit};
use egui_phosphor::regular as ph;
use crate::theme::{self, Tokens};

/// Gap between a field's caption and its control, and between one field and the next.
const LABEL_GAP: f32 = 4.0;
const FIELD_GAP: f32 = 12.0;
/// Extra space before a form section (on top of `FIELD_GAP`, giving 16px).
const SECTION_GAP: f32 = 4.0;
/// The transaction editor's destructive footer action.
const DELETE_OCCURRENCE: &str = "Delete occurrence…";

/// `Apply(payment, from)`: `from` is the date from which an existing payment's change applies
/// (edited from a calendar occurrence, analysis/04 §2); `None` applies it to every occurrence.
pub enum PaymentAction { None, Cancel, Apply(Box<Payment>, Option<i32>) }
/// The answer to the original's "Payment's Amount Changed" choice (analysis/04 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeScope { FromDate, All }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionScope { Occurrence, Future }
pub enum TransactionAction { None, Cancel, Apply { amount: f64, note: String, flag: char, scope: TransactionScope }, Delete }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmountKind { Expense, Income }
impl AmountKind {
    /// Income only for a positive amount: the original shows a zero amount as Expense (analysis/04 §1.2).
    fn from_amount(amount: f64) -> Self {
        if amount > 0.0 { Self::Income } else { Self::Expense }
    }
    fn signed(self, text: &str) -> Result<f64, String> {
        let amount = finite_number(text, "Amount")?;
        // The polarity is chosen with the control, so a sign that contradicts it is refused, not switched.
        let selected = match self { Self::Expense => "Expense", Self::Income => "Income" };
        if amount.is_sign_negative() {
            let other = match self { Self::Expense => "", Self::Income => " Choose Expense for money going out." };
            return Err(format!("{selected} is selected, so enter the amount without a minus sign or parentheses.{other}"));
        }
        if self == Self::Expense && has_plus_sign(text) {
            return Err("Expense is selected, so enter the amount without a plus sign. Choose Income for money coming in.".into());
        }
        Ok(if self == Self::Expense { -amount } else { amount })
    }
}

/// Whether amount text starts with a plus sign, after any opening bracket or dollar sign.
fn has_plus_sign(text: &str) -> bool {
    text.trim().trim_start_matches(['(', '$']).trim_start().starts_with('+')
}

/// Seed text for an amount box: the grouped 2-decimal magnitude, as the original shows it.
fn amount_seed(amount: f64) -> String { budget_core::numfmt::format_frac2(amount.abs()) }


struct DatePicker { month: Cal, sunday_first: bool }
impl DatePicker {
    fn new(text: &str, fallback: i32, sunday_first: bool) -> Self {
        let date = parse_iso_date(text).or_else(|_| parse_iso_date(&sdate(fallback)))
            .unwrap_or_else(|_| Cal::today().get());
        let mut month = Cal::from_int(date); month.set_day_one();
        Self { month, sunday_first }
    }
    fn first_weekday(&self) -> i32 { if self.sunday_first { 0 } else { 1 } }
    /// Column headers starting at the configured first day of the week, like the calendar grid.
    fn headers(&self) -> [&'static str; 7] {
        std::array::from_fn(|i| &crate::names::WEEKDAYS[(self.first_weekday() as usize + i) % 7][..2])
    }
    /// Blank cells before the 1st of the month.
    fn leading_blanks(&self) -> i32 { (self.month.day_of_week() + 7 - self.first_weekday()) % 7 }
    fn shift_month(&mut self, delta: i32) {
        let mut month = self.month; month.add(Field::Month, delta);
        if (YEAR_MIN..=YEAR_MAX).contains(&month.year()) { self.month = month; }
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
    grace: String,
    // Seeded amount/loan texts: applying them unchanged keeps the exact loaded values.
    amount_seed: String,
    loan_seed: String,
    error: Option<String>,
    // A built payment the user was warned rewrites past occurrences; applying it again confirms.
    confirm_past: Option<Payment>,
    notice: Option<String>,
    focus_description: bool,
    /// The calendar occurrence the editor was opened from (the original dialog's `cal`), if any.
    opened_from: Option<i32>,
    /// `opened_from` when the payment also has occurrences before it, so a change can split there.
    split_date: Option<i32>,
    /// Shown once an amount or schedule change needs the from-this-date-or-all choice.
    scope: Option<ChangeScope>,
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
        editor.amount_seed.clear();
        editor.loan_seed.clear();
        editor
    }

    pub fn edit(payment: &Payment) -> Self {
        let amount_seed = amount_seed(payment.amount);
        let loan_seed = budget_core::numfmt::format_frac2(payment.loan_amount);
        Self {
            payment_id: Some(payment.id), original: payment.clone(), description: payment.description.clone(),
            amount: amount_seed.clone(), amount_kind: AmountKind::from_amount(payment.amount),
            category: payment.category.clone().unwrap_or_default(),
            start_date: sdate(payment.start_date), start_picker: None, recurrence: Recurrence::from_payment(payment),
            recurring_end_initialized: Recurrence::from_payment(payment) != Recurrence::Once,
            frequency: payment.frequency.to_string(), end_mode: payment.end_mode(), end_date: sdate(payment.end_date), end_picker: None,
            count: payment.n_pays.to_string(), loan_amount: loan_seed.clone(), enabled: !payment.disabled,
            no_weekends: payment.is_no_weekends(), early_pay: payment.is_early_pay(), tentative: payment.is_tentative(),
            reminder: payment.is_reminder(), sticky: payment.is_sticky(), grace: payment.grace_period_days.to_string(),
            amount_seed, loan_seed, error: None, confirm_past: None, notice: None, focus_description: true,
            opened_from: None, split_date: None, scope: None,
        }
    }

    /// Edit `payment` as opened from its calendar occurrence on `date` ("Edit whole payment" on a
    /// transaction); `None` (the Payments page) edits every occurrence.
    pub fn edit_at(payment: &Payment, date: Option<i32>) -> Self {
        let mut editor = Self::edit(payment);
        editor.opened_from = date;
        editor.split_date = date.filter(|&d| crate::document::splits_at(payment, d));
        editor
    }

    /// Whether applying `p` changes the amount or schedule of an existing payment's occurrences.
    fn changes_occurrences(&self, p: &Payment) -> bool {
        self.payment_id.is_some()
            && (p.amount.to_bits() != self.original.amount.to_bits() || !self.original.is_same_schedule(p))
    }

    /// The end condition the form will store (a single payment always ends after one payment).
    fn effective_end_mode(&self) -> i32 {
        if self.recurrence == Recurrence::Once { END_MODE_N_PAYS } else { self.end_mode }
    }
    /// Original enable rules (analysis/04 §1.2 items 6 and 9): a loan payment cannot be tentative,
    /// and a tentative payment can neither end when a loan is paid nor move off weekends.
    fn tentative_enabled(&self) -> bool { self.effective_end_mode() != END_MODE_LOAN_PAID }
    fn loan_end_enabled(&self) -> bool { !self.tentative }
    fn weekend_options_enabled(&self) -> bool { !self.tentative }

    /// A warning when applying `p` rewrites occurrences before `today` (amount or schedule changed).
    /// Only for the Payments page: opened from a calendar occurrence, the editor offers the
    /// from-this-date-or-all choice instead (analysis/04 §1.3 step 16 prompts only then).
    fn past_change_warning(&self, p: &Payment, today: i32) -> Option<String> {
        (self.opened_from.is_none() && self.original.start_date < today && self.changes_occurrences(p)).then(|| format!(
            "This payment already has transactions. Applying changes the amount or schedule of every occurrence \
             since {}, including past months (manually adjusted dates keep their amounts). To change only from a \
             date onward, right-click that date's transaction in the calendar and choose Edit whole payment. \
             Click Apply to all occurrences to continue.", sdate(self.original.start_date)))
    }

    fn build(&self, db: &BudgetDb) -> Result<Payment, String> {
        if self.description.trim().is_empty() { return Err("Enter a description.".into()); }
        let mut p = self.original.clone();
        // Keep an untouched legacy description byte-for-byte; trim anything the user typed.
        if self.description != self.original.description { p.description = self.description.trim().to_string(); }
        p.amount = if self.amount.trim().is_empty() && self.reminder {
            0.0
        } else if self.amount.trim().is_empty() {
            return Err("Enter an amount.".into());
        } else if self.amount == self.amount_seed {
            // Untouched: keep the exact loaded value (not the rounded display text, and never
            // re-validated, so a legacy out-of-range amount does not block other edits).
            if self.amount_kind == AmountKind::from_amount(self.original.amount) { self.original.amount } else { -self.original.amount }
        } else { self.amount_kind.signed(&self.amount)? };
        // Preserve Some("") and whitespace in existing categories when untouched.
        if self.category != self.original.category.as_deref().unwrap_or("") {
            p.category = if self.category.trim().is_empty() { None } else { Some(self.category.trim().to_string()) };
        }
        // The loader would silently rename such a category (analysis/04 §1.3 step 12).
        if p.category.as_deref() == Some(p.description.as_str()) {
            return Err(SAME_CATEGORY_ERROR.into());
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
        // matching weekday (an unchanged start of an existing payment is left alone). A single payment
        // keeps its start date even when it still carries the last-weekday type.
        if self.recurrence == Recurrence::LastWeekday && (self.original.freq_type != 'L' || p.start_date != self.original.start_date) {
            p.start_date = last_same_weekday_of_month(p.start_date);
        }
        if self.recurrence == Recurrence::Once {
            p.n_pays = 1;
            p.set_end_mode(END_MODE_N_PAYS);
            // An interval is irrelevant for a single payment, but must remain safe for the engine.
            if p.frequency < 1 || p.frequency > max_frequency(&p) { p.frequency = 1; }
        } else {
            // The numbered-weekday repeat has no "Every" box, so its hidden text is not validated; the
            // engine ignores its interval and a stored one is kept.
            p.frequency = match self.recurrence {
                Recurrence::NthWeekday => if self.original.freq_type == 'W' { self.original.frequency } else { 1 },
                _ => positive_integer(&self.frequency, "Frequency")?,
            };
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
                    if self.loan_amount != self.loan_seed { p.loan_amount = finite_number(&self.loan_amount, "Loan amount")?; }
                    if p.loan_amount <= 0.0 { return Err("Loan amount must be greater than zero.".into()); }
                    if p.amount == 0.0 { return Err("A loan needs a nonzero payment amount.".into()); }
                }
                _ => {}
            }
        }
        p.disabled = !self.enabled;
        p.set_no_weekends(self.no_weekends); p.set_early_pay(self.early_pay);
        p.set_tentative(self.tentative); p.set_reminder(self.reminder); p.set_sticky(self.sticky);
        // The scheduler places a tentative payment's full amount and never its loan remainder, so the
        // original dialog made these exclusive. A legacy payment that already has both is kept as is.
        let legacy_pair = self.original.is_tentative() && self.original.end_mode() == END_MODE_LOAN_PAID;
        if p.is_tentative() && p.end_mode() == END_MODE_LOAN_PAID && !legacy_pair {
            return Err("A loan payment cannot be tentative. Clear Tentative or choose another end condition.".into());
        }
        // An untouched value is kept as loaded; a typed one must be a whole number of days.
        if self.grace.trim() != self.original.grace_period_days.to_string() {
            p.grace_period_days = match self.grace.trim() {
                "" => 0,
                s => s.parse::<i32>().ok().filter(|g| (0..=366).contains(g))
                    .ok_or("Grace period must be a whole number of days from 0 to 366.")?,
            };
        }
        align_end_date(&mut p);
        validate_schedule_bounds(&p)?;
        Ok(p)
    }

    /// Build and, when the change rewrites past occurrences, ask first: opened from a calendar
    /// occurrence with earlier ones, choose "from this date onward" (preselected, as in the
    /// original) or "all"; otherwise confirm the past-months notice with a second Apply.
    fn apply(&mut self, db: &BudgetDb, today: i32) -> PaymentAction {
        match self.build(db) {
            Err(e) => { self.error = Some(e); self.notice = None; self.confirm_past = None; PaymentAction::None }
            Ok(p) => {
                // Once the scope panel is up its choice stands, even if the edit that raised it was undone
                // since, but never for an apply that changes nothing at all (that must not split the record).
                if let Some(date) = self.split_date.filter(|_| self.changes_occurrences(&p) || (self.scope.is_some() && p != self.original)) {
                    self.error = None;
                    return match self.scope {
                        None => { self.scope = Some(ChangeScope::FromDate); PaymentAction::None }
                        Some(ChangeScope::FromDate) => PaymentAction::Apply(Box::new(p), Some(date)),
                        Some(ChangeScope::All) => PaymentAction::Apply(Box::new(p), None),
                    };
                }
                match self.past_change_warning(&p, today) {
                    Some(warning) if self.confirm_past.is_none() => {
                        self.error = None;
                        self.notice = Some(warning);
                        self.confirm_past = Some(p);
                        PaymentAction::None
                    }
                    _ => PaymentAction::Apply(Box::new(p), None),
                }
            }
        }
    }

    /// The primary button's label, naming the chosen scope once the choice is shown.
    fn apply_label(&self) -> String {
        match (self.scope, self.split_date) {
            (Some(ChangeScope::FromDate), Some(date)) => format!("Apply from {}", sdate(date)),
            (Some(ChangeScope::All), _) => "Apply to all occurrences".into(),
            _ if self.confirm_past.is_some() => "Apply to all occurrences".into(),
            _ if self.payment_id.is_some() => "Apply changes".into(),
            _ => "Add payment".into(),
        }
    }

    /// The disclosure title for the schedule options it hides, with a count of those set, so a
    /// closed disclosure still shows that something is on.
    fn more_options_title(&self) -> String {
        let set = [self.no_weekends, self.early_pay, self.tentative, self.reminder, self.sticky].into_iter().filter(|&on| on).count()
            + usize::from(!matches!(self.grace.trim(), "" | "0"));
        if set == 0 { "More schedule options".into() } else { format!("More schedule options ({set} set)") }
    }

    /// `sunday_first` lays out the date pickers like the calendar grid; `today` decides whether
    /// an edit rewrites past occurrences and needs confirming.
    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens, db: &BudgetDb, sunday_first: bool, today: i32) -> PaymentAction {
        if dialog_escape(ctx) { return PaymentAction::Cancel; }
        let mut action = PaymentAction::None;
        let mut enter = false;
        let mut apply = false;
        egui::Modal::new(egui::Id::new("payment_editor")).frame(t.dialog()).show(ctx, |ui| {
            ui.set_width(560.0_f32.min((ctx.screen_rect().width() - 64.0).max(240.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, FIELD_GAP);
            ui.heading(if self.payment_id.is_some() { "Edit payment" } else { "Add payment" });
            ui.label(RichText::new(match (self.payment_id, self.split_date) {
                (Some(_), Some(date)) => format!("Payment schedule · Opened from {}", sdate(date)),
                (Some(_), None) => "Payment schedule · All occurrences".into(),
                _ => "Create a one-time or repeating payment".into(),
            }).color(t.text_muted));
            ui.add_space(2.0);
            // Keep the pinned error, notice or scope choice and the footer on screen.
            let pinned = if self.scope.is_some() { 150.0 } else if self.notice.is_some() { 110.0 } else { 0.0 };
            let reserved_height = if self.error.is_some() { 320.0 } else { 250.0 } + pinned;
            ScrollArea::vertical().id_salt("payment_editor_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height((ctx.screen_rect().height() - reserved_height).max(160.0)).show(ui, |ui| {
                form_section(ui, "Payment details", t, |ui| {
                    let response = text_field(ui, "Description", &mut self.description, "e.g. Rent or salary", t);
                    enter |= submitted(ui, &response);
                    if self.focus_description { response.request_focus(); self.focus_description = false; }
                    let response = amount_field(ui, &mut self.amount, &mut self.amount_kind, t);
                    enter |= submitted(ui, &response);
                    ui.checkbox(&mut self.enabled, "Enabled in calendar");
                    field(ui, "Category (optional)", t, |ui, label| {
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 110.0).max(100.0);
                            let response = ui.add(TextEdit::singleline(&mut self.category).desired_width(width).hint_text("e.g. Home")).labelled_by(label);
                            enter |= submitted(ui, &response);
                            ComboBox::from_id_salt("payment_category_picker").selected_text("Choose…").width(92.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.category, String::new(), "No category");
                                for category in db.categories() { ui.selectable_value(&mut self.category, category.clone(), category); }
                            }).response.labelled_by(label);
                        });
                    });
                });
                ui.add_space(SECTION_GAP);
                form_section(ui, "Schedule", t, |ui| {
                    enter |= date_field(ui, "Start date", "payment_start_date", &mut self.start_date, &mut self.start_picker, self.original.start_date, sunday_first, t);
                    let mut recurrence = self.recurrence;
                    field(ui, "Repeat", t, |ui, label| {
                        ComboBox::from_id_salt("payment_recurrence").width(ui.available_width())
                            .selected_text(recurrence.label()).show_ui(ui, |ui| {
                            for candidate in [Recurrence::Once, Recurrence::Days, Recurrence::Weeks, Recurrence::Months, Recurrence::Years, Recurrence::NthWeekday, Recurrence::LastWeekday] {
                                ui.selectable_value(&mut recurrence, candidate, candidate.label());
                            }
                        }).response.labelled_by(label);
                    });
                    if recurrence != self.recurrence { self.select_recurrence(recurrence); }
                    if self.recurrence != Recurrence::Once {
                        if self.recurrence == Recurrence::NthWeekday {
                            ui.label(RichText::new("Every month, on the start date’s numbered weekday.").color(t.text_muted));
                        } else {
                            let unit = match self.recurrence {
                                Recurrence::Days => "days", Recurrence::Weeks => "weeks", Recurrence::Years => "years", _ => "months",
                            };
                            field(ui, "Repeat every", t, |ui, label| {
                                ui.horizontal(|ui| {
                                    // The unit sits beside the number, so the box leaves room for it.
                                    let width = (ui.available_width() - 80.0).max(100.0);
                                    let response = ui.add(TextEdit::singleline(&mut self.frequency).desired_width(width).hint_text("1")).labelled_by(label);
                                    enter |= submitted(ui, &response);
                                    ui.label(RichText::new(unit).color(t.text));
                                });
                            });
                        }
                        if self.recurrence == Recurrence::LastWeekday {
                            let hint = match parse_iso_date(&self.start_date).map(last_same_weekday_of_month) {
                                Ok(d) => format!("Uses the start date’s weekday in the last week of the month. First payment: {}.", sdate(d)),
                                Err(_) => "Uses the start date’s weekday in the last week of the month.".to_string(),
                            };
                            ui.label(RichText::new(hint).color(t.text_muted));
                        }
                        let loan_end_enabled = self.loan_end_enabled();
                        field(ui, "Ends", t, |ui, label| {
                            ComboBox::from_id_salt("payment_end_mode").width(ui.available_width()).selected_text(end_label(self.end_mode)).show_ui(ui, |ui| {
                                for (mode, text) in [(END_MODE_INDEFINITE, "Indefinitely"), (END_MODE_DATE, "On a date"), (END_MODE_N_PAYS, "After a number of payments"), (END_MODE_LOAN_PAID, "When loan is paid")] {
                                    ui.add_enabled_ui(mode != END_MODE_LOAN_PAID || loan_end_enabled, |ui| {
                                        ui.selectable_value(&mut self.end_mode, mode, text)
                                            .on_disabled_hover_text("Not available for tentative payments.");
                                    });
                                }
                            }).response.labelled_by(label);
                        });
                        match self.end_mode {
                            END_MODE_DATE => { enter |= date_field(ui, "End date", "payment_end_date", &mut self.end_date, &mut self.end_picker, self.original.end_date, sunday_first, t); }
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
                    // A fixed id keeps the disclosure open or closed while the count in its title changes.
                    egui::CollapsingHeader::new(self.more_options_title()).id_salt("more_schedule_options").show(ui, |ui| {
                        ui.add_enabled_ui(self.weekend_options_enabled(), |ui| {
                            ui.checkbox(&mut self.no_weekends, "Move weekend payments to a business day");
                            ui.add_enabled_ui(self.no_weekends, |ui| {
                                ui.checkbox(&mut self.early_pay, "Use the previous business day");
                            });
                        });
                        ui.add_enabled_ui(self.tentative_enabled(), |ui| {
                            ui.checkbox(&mut self.tentative, "Tentative");
                        });
                        if !self.tentative_enabled() {
                            ui.label(RichText::new("Loan payments cannot be tentative.").color(t.text_muted));
                        }
                        if self.tentative {
                            ui.label(RichText::new("Paid status also updates this period’s unpaid estimates. Tentative payments slide to today and are not moved off weekends.").color(t.text_muted));
                        }
                        // A tentative payment's valid days are its whole period, so grace does not apply.
                        ui.add_enabled_ui(!self.tentative, |ui| {
                            let response = text_field(ui, "Grace period · days", &mut self.grace, "0", t)
                                .on_hover_text("Days after the due date on which paying still counts as on time; highlighted when you hover the payment. 0 turns it off.");
                            enter |= submitted(ui, &response);
                        });
                        ui.checkbox(&mut self.reminder, "Reminder")
                            .on_hover_text("A marker for your own reference. It does not change amounts, dates or balances.");
                        ui.checkbox(&mut self.sticky, "Carry changed amounts forward")
                            .on_hover_text("Later occurrences keep the last amount you changed before them, until the next change. Off: each occurrence uses the payment's amount unless you change it.");
                    });
                });
            });
            if let Some(error) = &self.error { form_error(ui, error, t); }
            if let (Some(scope), Some(date)) = (&mut self.scope, self.split_date) {
                scope_choice(ui, scope, date, t);
            } else if let Some(notice) = &self.notice { form_notice(ui, notice, t); }
            ui.add_space(4.0);
            ui.separator();
            let label = self.apply_label();
            let (applied, cancelled, _) = dialog_footer(ui, &label, None, t);
            if applied { apply = true; }
            if cancelled { action = PaymentAction::Cancel; }
        });
        if (apply || enter) && matches!(action, PaymentAction::None) { action = self.apply(db, today); }
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
    // The seeded text and the exact occurrence amount it stands for.
    amount_seed: String,
    seed_value: f64,
    note: String,
    flag: char,
    scope: TransactionScope,
    error: Option<String>,
    focus_amount: bool,
    guard: ClickGuard,
}
impl TransactionEditor {
    pub fn set_error(&mut self, error: String) { self.error = Some(error); }

    pub fn new(payment: &Payment, date: i32, amount: f64) -> Self {
        Self { payment_id: payment.id, date, description: payment.description.clone(), amount: amount_seed(amount),
            amount_kind: AmountKind::from_amount(amount), amount_seed: amount_seed(amount), seed_value: amount,
            loan_payment: payment.end_mode() == END_MODE_LOAN_PAID,
            tentative_payment: payment.is_tentative(),
            note: payment.get_note(date).unwrap_or_default(), flag: payment.get_flag(date),
            scope: TransactionScope::Occurrence, error: None, focus_amount: true, guard: ClickGuard::default() }
    }
    fn values(&self) -> Result<(f64, String, char), String> {
        let amount = if self.amount == self.amount_seed {
            if self.amount_kind == AmountKind::from_amount(self.seed_value) { self.seed_value } else { -self.seed_value }
        } else { self.amount_kind.signed(&self.amount)? };
        // Unknown legacy flags remain unchanged until the user explicitly chooses a new status.
        Ok((amount, self.note.clone(), self.flag))
    }
    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens) -> TransactionAction {
        if dialog_escape(ctx) { return TransactionAction::Cancel; }
        let mut action = TransactionAction::None;
        let mut enter = false;
        let mut apply = false;
        // A calendar row opens this editor on its first click; a double-click's second click is ignored.
        let (ignore_pointer, refocus) = self.guard.update(ctx);
        egui::Modal::new(egui::Id::new("transaction_editor")).frame(t.dialog()).show(ctx, |ui| {
            if ignore_pointer { ui.disable(); }
            ui.set_width(520.0_f32.min((ctx.screen_rect().width() - 64.0).max(240.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, FIELD_GAP);
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
                    if self.focus_amount && !ignore_pointer {
                        response.request_focus();
                        select_all(ui, &response, &self.amount);
                        self.focus_amount = false;
                    }
                    field(ui, "Apply amount to", t, |ui, label| {
                        ui.radio_value(&mut self.scope, TransactionScope::Occurrence, "Only this occurrence").labelled_by(label);
                        ui.radio_value(&mut self.scope, TransactionScope::Future, "This and all future occurrences").labelled_by(label);
                        let help = match self.scope {
                            TransactionScope::Occurrence => format!("Change the amount for {} only.", sdate(self.date)),
                            TransactionScope::Future => format!("Change amounts from {} onward; earlier dates keep theirs. The payment continues as a new record from this date.", sdate(self.date)),
                        };
                        ui.label(RichText::new(help).color(t.text_muted));
                    });
                    ui.label(RichText::new(format!("Notes and status apply only to {}.", sdate(self.date))).color(t.text_muted));
                    field(ui, "Status · Selected date only", t, |ui, label| {
                        ComboBox::from_id_salt("transaction_status").width(ui.available_width()).selected_text(flag_label(self.flag)).show_ui(ui, |ui| {
                            for (flag, text) in [(FLAG_NONE, "None"), (FLAG_PAID, "Paid"), (FLAG_IMPORTANT, "Important")] {
                                ui.selectable_value(&mut self.flag, flag, text);
                            }
                        }).response.labelled_by(label);
                    });
                    field(ui, "Note · Optional · Selected date only", t, |ui, label| {
                        ui.add(TextEdit::multiline(&mut self.note).desired_width(f32::INFINITY).desired_rows(3)
                            .hint_text("Add a note for this date")).labelled_by(label);
                    });
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
            let (applied, cancelled, deleted) = dialog_footer(ui, "Apply changes", Some(DELETE_OCCURRENCE), t);
            if applied { apply = true; }
            if cancelled { action = TransactionAction::Cancel; }
            if deleted { action = TransactionAction::Delete; }
        });
        // The ignored press may have taken focus from the amount; give it back so typing replaces the amount.
        if refocus { self.focus_amount = true; ctx.request_repaint(); }
        if (apply || enter) && matches!(action, TransactionAction::None) {
            match self.values() {
                Ok((amount, note, flag)) => action = TransactionAction::Apply { amount, note, flag, scope: self.scope },
                Err(e) => self.error = Some(e),
            }
        }
        action
    }
}

pub(crate) fn form_section(ui: &mut egui::Ui, title: &str, t: &Tokens, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(t.surface_hover).corner_radius(theme::RADIUS_MD).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().color(t.text));
        ui.add_space(2.0);
        body(ui);
    });
}
fn field_label(ui: &mut egui::Ui, label: &str, t: &Tokens) -> egui::Response {
    ui.label(RichText::new(label).color(t.text_muted))
}
/// A caption and its control(s), kept `LABEL_GAP` apart so the caption reads as belonging to the
/// control below it rather than the field above (fields themselves are `FIELD_GAP` apart).
pub(crate) fn field<R>(ui: &mut egui::Ui, label: &str, t: &Tokens, add: impl FnOnce(&mut egui::Ui, egui::Id) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = LABEL_GAP;
        let label = field_label(ui, label, t);
        add(ui, label.id)
    }).inner
}
fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, t: &Tokens) -> egui::Response {
    field(ui, label, t, |ui, label| {
        ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY).hint_text(hint)).labelled_by(label)
    })
}
/// One dialog footer for every form: an optional destructive ghost action on the left, then
/// (right to left) the primary action and Cancel beside it. Returns `(apply, cancel, destroy)`.
pub(crate) fn dialog_footer(ui: &mut egui::Ui, primary: &str, destructive: Option<&str>, t: &Tokens) -> (bool, bool, bool) {
    let (mut apply, mut cancel, mut destroy) = (false, false, false);
    ui.horizontal(|ui| {
        if let Some(text) = destructive {
            let response = ui.add(egui::Button::new(RichText::new(destructive_label(text)).color(t.negative))
                .fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE).corner_radius(theme::RADIUS_MD)
                .min_size(egui::vec2(0.0, theme::CONTROL_H)));
            theme::focus_ring(ui, &response, t);
            destroy = response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            apply = theme::primary_button(ui, primary, t).clicked();
            cancel = theme::secondary_button(ui, "Cancel", t).clicked();
        });
    });
    (apply, cancel, destroy)
}
/// Escape cancels a dialog, unless a popup (combo box, date picker) is open and should close first.
pub(crate) fn dialog_escape(ctx: &egui::Context) -> bool {
    !ctx.memory(|m| m.any_popup_open()) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}
/// Swallows the second click of the double-click whose first click opened a dialog. Without it the
/// second click lands on whatever control the modal placed under the pointer (e.g. Delete).
#[derive(Default)]
pub(crate) struct ClickGuard { opened_at: Option<f64>, stale: bool }

impl ClickGuard {
    /// Call once per frame before drawing. Returns `(ignore_pointer, ended)`: while `ignore_pointer`
    /// the dialog content must be disabled; `ended` is true on the (still ignored) release frame.
    pub(crate) fn update(&mut self, ctx: &egui::Context) -> (bool, bool) {
        let delay = ctx.options(|o| o.input_options.max_double_click_delay);
        let (time, pressed, released, down, since_click) = ctx.input(|i| (i.time, i.pointer.any_pressed(),
            i.pointer.any_released(), i.pointer.any_down(), i.pointer.time_since_last_click() as f64));
        let opened_at = *self.opened_at.get_or_insert(time);
        // The press continues a click that happened when (or before) the dialog opened.
        if pressed && since_click < delay && time - since_click <= opened_at + 1e-3 { self.stale = true; }
        let ignore = self.stale;
        let ended = ignore && released && !down;
        if ended { self.stale = false; }
        (ignore, ended)
    }
}

/// True when a single-line field just lost focus because Enter was pressed.
pub(crate) fn submitted(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
/// Select the whole text when a field gains focus so typing replaces it.
fn select_all_on_focus(ui: &egui::Ui, response: &egui::Response, text: &str) {
    if response.gained_focus() { select_all(ui, response, text); }
}
/// Select the whole text of a field so typing replaces it.
fn select_all(ui: &egui::Ui, response: &egui::Response, text: &str) {
    if let Some(mut state) = TextEdit::load_state(ui.ctx(), response.id) {
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()),
        )));
        state.store(ui.ctx(), response.id);
    }
}
/// The trash icon and text of a footer's destructive ghost action.
fn destructive_label(text: &str) -> String { format!("{}  {text}", ph::TRASH) }

/// Height shared by the amount input, the date input's picker button and the polarity control.
const CONTROL_HEIGHT: f32 = 34.0;

/// The colour that marks an amount kind: expenses negative, income positive.
fn kind_color(kind: AmountKind, t: &Tokens) -> egui::Color32 {
    match kind { AmountKind::Expense => t.negative, AmountKind::Income => t.positive }
}

/// Expense/Income as a Meridian segmented control, like the Calendar/Payments switch: a sunken track with the
/// selected option filled with the accent.
fn polarity_control(ui: &mut egui::Ui, kind: &mut AmountKind, label: egui::Id, t: &Tokens) {
    egui::Frame::new().fill(t.surface_hover).corner_radius(theme::RADIUS_MD).inner_margin(3).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (candidate, text) in [(AmountKind::Expense, "Expense"), (AmountKind::Income, "Income")] {
            let selected = *kind == candidate;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(84.0, CONTROL_HEIGHT - 8.0), egui::Sense::click());
            let response = response.labelled_by(label).on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text));
            if response.clicked() { *kind = candidate; }
            let selected = *kind == candidate;
            let hovered = response.hovered();
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, theme::RADIUS_SM, t.accent);
            }
            if response.has_focus() {
                painter.rect_stroke(rect.expand(2.0), theme::RADIUS_SM + 2, egui::Stroke::new(2.0_f32, t.focus), egui::StrokeKind::Outside);
            }
            let color = if selected { theme::on(t.accent) } else if hovered { t.text } else { t.text_muted };
            let style = if selected { theme::ts(theme::SEMI) } else { egui::TextStyle::Body };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, style.resolve(ui.style()), color);
        }
    });
}

fn amount_field(ui: &mut egui::Ui, amount: &mut String, kind: &mut AmountKind, t: &Tokens) -> egui::Response {
    field(ui, "Amount", t, |ui, label| {
        ui.horizontal(|ui| {
            polarity_control(ui, kind, label, t);
            ui.add_space(4.0);
            // The figure takes the chosen polarity's colour, so the sign is clear before typing.
            let response = ui.add(TextEdit::singleline(amount).desired_width(ui.available_width()).hint_text("0.00")
                .min_size(egui::vec2(0.0, CONTROL_HEIGHT)).text_color(kind_color(*kind, t))
                .vertical_align(egui::Align::Center).horizontal_align(egui::Align::RIGHT)).labelled_by(label);
            select_all_on_focus(ui, &response, amount);
            response
        }).inner
    })
}
pub(crate) fn form_error(ui: &mut egui::Ui, error: &str, t: &Tokens) {
    egui::Frame::new().fill(theme::tint(t.surface, t.negative, 0.08)).corner_radius(theme::RADIUS_SM)
        .inner_margin(10).show(ui, |ui| {
            ui.label(RichText::new(error).color(t.negative));
        });
}
fn form_notice(ui: &mut egui::Ui, notice: &str, t: &Tokens) {
    egui::Frame::new().fill(theme::tint(t.surface, t.text_muted, 0.10)).corner_radius(theme::RADIUS_SM)
        .inner_margin(10).show(ui, |ui| {
            ui.label(RichText::new(notice).color(t.text));
        });
}
/// The original's "Payment's Amount Changed" choice (analysis/04 §2), pinned above the footer.
fn scope_choice(ui: &mut egui::Ui, scope: &mut ChangeScope, date: i32, t: &Tokens) {
    egui::Frame::new().fill(theme::tint(t.surface, t.text_muted, 0.10)).corner_radius(theme::RADIUS_SM)
        .inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            let label = ui.label(RichText::new(format!("This payment has transactions before {}. Apply the change to:", sdate(date))).color(t.text));
            ui.radio_value(scope, ChangeScope::FromDate, format!("Transactions from {} onward", sdate(date))).labelled_by(label.id);
            ui.radio_value(scope, ChangeScope::All, "All transactions, past and future").labelled_by(label.id);
            ui.label(RichText::new(match scope {
                ChangeScope::FromDate => "Earlier transactions keep their amount and schedule; the payment continues as a new record from this date. Transactions adjusted by hand keep their amounts.",
                ChangeScope::All => "Transactions adjusted by hand keep their amounts.",
            }).color(t.text_muted));
        });
}
#[allow(clippy::too_many_arguments)]
fn date_field(ui: &mut egui::Ui, label: &str, id: &str, value: &mut String, picker: &mut Option<DatePicker>, fallback: i32, sunday_first: bool, t: &Tokens) -> bool {
    let mut entered = false;
    let button = field(ui, label, t, |ui, label| ui.horizontal(|ui| {
        // The icon button is 36px wide, so the input lines up with full-width fields' right edge.
        // desired_width excludes the TextEdit's own horizontal margins (4px each side).
        let width = (ui.available_width() - 36.0 - ui.spacing().item_spacing.x - 8.0).max(100.0);
        let response = ui.add(TextEdit::singleline(value).desired_width(width).hint_text("YYYY-MM-DD")
            .min_size(egui::vec2(0.0, CONTROL_HEIGHT)).vertical_align(egui::Align::Center)).labelled_by(label);
        entered = submitted(ui, &response);
        let button = ui.add_sized([36.0, CONTROL_HEIGHT], egui::Button::new(RichText::new(ph::CALENDAR_BLANK).color(t.text))
            .fill(t.surface).stroke(egui::Stroke::new(1.0_f32, t.border_strong)).corner_radius(theme::RADIUS_MD));
        theme::focus_ring(ui, &button, t);
        button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Pick date"));
        button.labelled_by(label).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Pick date")
    }).inner);
    let popup_id = ui.make_persistent_id(id);
    if button.clicked() {
        *picker = Some(DatePicker::new(value, fallback, sunday_first));
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
    ui.set_width(266.0);
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
    let mut chosen = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(picker.month.ym() > Cal::from_int(DATE_MIN).ym(), egui::Button::new("‹")).on_hover_text("Previous month").clicked() {
            picker.shift_month(-1);
        }
        ui.add_sized([190.0, 34.0], egui::Label::new(RichText::new(format!("{} {}", crate::names::MONTHS_LONG[picker.month.month() as usize], picker.month.year())).strong()));
        if ui.add_enabled(picker.month.ym() < Cal::from_int(DATE_MAX).ym(), egui::Button::new("›")).on_hover_text("Next month").clicked() {
            picker.shift_month(1);
        }
    });
    Grid::new("date_picker_grid").spacing([4.0, 4.0]).show(ui, |ui| {
        for day in picker.headers() {
            ui.add_sized([34.0, 24.0], egui::Label::new(RichText::new(day).color(t.text_muted)));
        }
        ui.end_row();
        let offset = picker.leading_blanks();
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
/// Largest absolute amount or balance accepted; keeps running balances far from f64 overflow.
pub(crate) use budget_core::commands::MAX_ABS_AMOUNT;

pub(crate) use budget_core::commands::SAME_CATEGORY_ERROR;

/// True when amount text is a formula rather than a plain number: an operator after the first
/// character, `*`/`/`, a `%` income percentage, or a `k`/`m`/`b`/`t` multiplier suffix.
fn looks_like_formula(text: &str) -> bool {
    let body = text.trim();
    let tail = body.char_indices().skip(1).map(|(_, c)| c);
    body.contains(['*', '/', '%']) || tail.clone().any(|c| c == '+' || c == '-')
        || body.ends_with(['k', 'K', 'm', 'M', 'b', 'B', 't', 'T'])
}

/// The single amount parser for payment, transaction, loan and balance fields. Accepts an optional
/// sign, a `$`, well-formed thousands commas ("1,234.50") and accounting parentheses ("(50)" is -50),
/// or a calculator formula (SPEC.md 3.2: `+ - * / ( )`, `k/m/b/t` suffixes). These fields have no
/// income context, so `%n` income percentages are refused rather than silently evaluating to 0.
/// The result is rounded to cents as the original's amount box is (it reformats to two decimals
/// before reading the value), so `10/3` is stored as 3.33 and months add up to the balance shown.
pub(crate) fn parse_amount(text: &str, label: &str) -> Result<f64, String> {
    let value = match parse_plain_amount(text, label) {
        Ok(v) => v,
        Err(_) if looks_like_formula(text) => {
            if text.contains('%') {
                return Err(format!("{label} cannot use % of income here. Enter the amount or a formula such as 1200-350."));
            }
            budget_core::calc::calculate(text).map_err(|e| format!("{label}: {e}."))?
        }
        Err(plain) => return Err(plain),
    };
    if value.abs() > MAX_ABS_AMOUNT || !value.is_finite() {
        return Err(too_large(label));
    }
    // A typed "-0" keeps its sign: the polarity checks read it.
    Ok(format!("{value:.2}").parse::<f64>().unwrap_or(value))
}

fn too_large(label: &str) -> String { format!("{label} is too large (maximum 1,000,000,000,000).") }

fn parse_plain_amount(text: &str, label: &str) -> Result<f64, String> {
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
        // A digit string too long for f64 parses to infinity: that is too large, not malformed.
        Ok(v) if v.is_infinite() && cleaned.bytes().any(|b| b.is_ascii_digit()) => return Err(too_large(label)),
        _ => return Err(generic()),
    };
    let value = if negate { if sign.is_some() { return Err(generic()); } -value } else { value };
    if value.abs() > MAX_ABS_AMOUNT {
        return Err(too_large(label));
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

/// The calendar range the editors accept (`YEAR_MIN..=YEAR_MAX`). The loader clamps stored dates
/// into `0..=DATE_MAX` (SPEC.md §6); the editors refuse anything outside the years the calendar can
/// show, so a typed date is never silently changed on the next load.
const YEAR_RANGE_ERROR: &str = "Use a year from 1000 to 9999.";

fn parse_iso_date(text: &str) -> Result<i32, String> {
    let text = text.trim();
    // Year, month and day; the year may have any number of digits (or a minus sign) so an
    // out-of-range year gets a message about the year rather than about the format.
    let (year_text, rest) = text.char_indices().skip(1).find(|&(_, c)| c == '-')
        .map_or((text, ""), |(i, _)| (&text[..i], &text[i + 1..]));
    let two_digits = |s: &str| s.len() == 2 && s.bytes().all(|c| c.is_ascii_digit());
    let (month_text, day_text) = rest.split_once('-').unwrap_or(("", ""));
    let unsigned_year = year_text.strip_prefix('-').unwrap_or(year_text);
    if !two_digits(month_text) || !two_digits(day_text) || unsigned_year.is_empty()
        || !unsigned_year.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Use YYYY-MM-DD.".into());
    }
    let year = match year_text.parse::<i64>() {
        Ok(year) if (YEAR_MIN as i64..=YEAR_MAX as i64).contains(&year) && year_text.len() == 4 => year as i32,
        _ => return Err(YEAR_RANGE_ERROR.into()),
    };
    let month: i32 = month_text.parse().map_err(|_| "Invalid month.")?;
    let day: i32 = day_text.parse().map_err(|_| "Invalid day.")?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err("This date does not exist.".into());
    }
    let date = year * 10000 + month * 100 + day;
    if Cal::ymd(year, month - 1, day).get() != date { return Err("This date does not exist.".into()); }
    Ok(date)
}

pub(crate) use budget_core::commands::{align_end_date, validate_schedule_bounds, max_frequency};

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
    fn second_click_of_the_opening_double_click_activates_nothing_and_keeps_amount_focus() {
        // A calendar row opens the editor on the first click of a double-click; the second click
        // must not press "Delete occurrence…" that the modal placed under the pointer.
        let size = egui::vec2(900.0, 600.0);
        let payment = Payment::new(20261027);
        let (probe_ctx, t) = transaction_context();
        let mut probe = TransactionEditor::new(&payment, 20261027, -150.0);
        for _ in 0..3 { transaction_frame(&probe_ctx, &t, &mut probe, size, Vec::new()); }
        let (output, _) = transaction_frame(&probe_ctx, &t, &mut probe, size, Vec::new());
        let pos = transaction_label(&output, &destructive_label(DELETE_OCCURRENCE)).center();
        let press = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let (ctx, t) = transaction_context();
        let run = |events: Vec<egui::Event>| { let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() }, |_| {}).platform_output; };
        // First click on the calendar row (the editor does not exist yet).
        run(vec![egui::Event::PointerMoved(pos)]);
        run(vec![press(true)]);
        run(vec![press(false)]);
        let mut editor = TransactionEditor::new(&payment, 20261027, -150.0);
        for _ in 0..3 { assert!(matches!(transaction_frame(&ctx, &t, &mut editor, size, Vec::new()).1, TransactionAction::None)); }
        for pressed in [true, false] {
            assert!(matches!(transaction_frame(&ctx, &t, &mut editor, size, vec![press(pressed)]).1, TransactionAction::None),
                "the double-click's second click must not delete");
        }
        for _ in 0..2 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        transaction_frame(&ctx, &t, &mut editor, size, vec![egui::Event::Text("35".into())]);
        assert_eq!(editor.amount, "35", "typing still replaces the amount (focus {:?})", ctx.memory(|m| m.focused()));
        // A later, deliberate click on Delete still works.
        for _ in 0..30 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        transaction_frame(&ctx, &t, &mut editor, size, vec![press(true)]);
        assert!(matches!(transaction_frame(&ctx, &t, &mut editor, size, vec![press(false)]).1, TransactionAction::Delete));
    }

    #[test]
    fn deleting_with_future_amount_selected_still_emits_single_occurrence_delete() {
        let (ctx, t) = transaction_context();
        let size = egui::vec2(900.0, 600.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        click_transaction_label(&ctx, &t, &mut editor, size, "This and all future occurrences");
        assert!(matches!(click_transaction_label(&ctx, &t, &mut editor, size, &destructive_label(DELETE_OCCURRENCE)), TransactionAction::Delete));
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
                let delete = destructive_label(DELETE_OCCURRENCE);
                for label in ["2026-10-27", "Only this occurrence", "This and all future occurrences", "Cancel", "Apply changes", delete.as_str()] {
                    let rect = transaction_label(&output, label);
                    assert!(screen.contains_rect(rect), "Control {label} outside compact viewport: {rect:?}");
                }
                let modal = ctx.memory(|memory| memory.area_rect(egui::Id::new("transaction_editor"))).unwrap();
                assert!(screen.contains_rect(modal), "Compact modal outside viewport (dark={dark}, error={}): {modal:?}", error.is_some());
            }
        }
    }

    #[test]
    fn transaction_footer_has_delete_left_and_cancel_beside_apply_on_one_row() {
        // Delete used to trail below the footer as a separate link.
        let (ctx, t) = transaction_context();
        let size = egui::vec2(900.0, 600.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        let (output, _) = transaction_frame(&ctx, &t, &mut editor, size, Vec::new());
        let delete = transaction_label(&output, &destructive_label(DELETE_OCCURRENCE));
        let cancel = transaction_label(&output, "Cancel");
        let apply = transaction_label(&output, "Apply changes");
        assert!(delete.right() < cancel.left() && cancel.right() < apply.left(), "{delete:?} {cancel:?} {apply:?}");
        for rect in [delete, cancel] {
            assert!((rect.center().y - apply.center().y).abs() < 2.0, "footer controls share a row: {rect:?} vs {apply:?}");
        }
    }

    #[test]
    fn polarity_segments_switch_the_amount_sign() {
        let (ctx, t) = transaction_context();
        let size = egui::vec2(1200.0, 1000.0);
        let mut editor = TransactionEditor::new(&Payment::new(20261027), 20261027, -150.0);
        editor.amount = "40".into();
        for _ in 0..3 { transaction_frame(&ctx, &t, &mut editor, size, Vec::new()); }
        click_transaction_label(&ctx, &t, &mut editor, size, "Income");
        assert_eq!(editor.amount_kind, AmountKind::Income);
        assert_eq!(editor.values().unwrap().0, 40.0);
        click_transaction_label(&ctx, &t, &mut editor, size, "Expense");
        assert_eq!(editor.values().unwrap().0, -40.0);
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
        assert_eq!(parse_iso_date("9999-12-31").unwrap(), 99991231);
    }

    #[test]
    fn start_and_end_years_outside_the_calendar_are_refused_with_a_year_message() {
        // The loader clamps such dates (SPEC.md §6), so the editor must not store them.
        for text in ["0000-01-01", "0001-01-01", "0999-12-31", "10000-01-01", "99999-12-31", "-0500-06-01", "123456789012345678901-01-01"] {
            assert_eq!(parse_iso_date(text).unwrap_err(), YEAR_RANGE_ERROR, "{text}");
        }
        for text in ["2026-1-01", "2026/10/27", "+2026-10-27", "26-10-27x", "", "-"] {
            assert_eq!(parse_iso_date(text).unwrap_err(), "Use YYYY-MM-DD.", "{text:?}");
        }
        let mut e = valid_editor(); e.start_date = "10000-01-01".into();
        assert_eq!(e.build(&db()).unwrap_err(), format!("Start date: {YEAR_RANGE_ERROR}"));
        let mut e = valid_editor(); e.recurrence = Recurrence::Months; e.end_mode = END_MODE_DATE;
        e.end_date = "0999-12-31".into();
        assert_eq!(e.build(&db()).unwrap_err(), format!("End date: {YEAR_RANGE_ERROR}"));
        // A legacy date the loader clamped to year 0 must be corrected before saving the payment.
        let mut legacy = Payment::new(20261027); legacy.description = "Old".into(); legacy.amount = -1.0; legacy.start_date = 0;
        assert_eq!(PaymentEditor::edit(&legacy).build(&db()).unwrap_err(), format!("Start date: {YEAR_RANGE_ERROR}"));
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
    fn emoji_in_editable_text_is_accepted_because_the_file_format_saves_it() {
        let db = db();
        let mut e = valid_editor(); e.description = "Gift 🎁".into(); e.category = "Fun 🎉".into();
        let p = e.build(&db).unwrap();
        assert_eq!((p.description.as_str(), p.category.as_deref()), ("Gift 🎁", Some("Fun 🎉")));
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, -5.0);
        tx.note = "Paid 👍".into();
        assert_eq!(tx.values().unwrap().1, "Paid 👍");
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
    fn grace_period_is_edited_and_validated() {
        let db = db();
        let mut e = valid_editor();
        assert_eq!(e.build(&db).unwrap().grace_period_days, 0);
        e.grace = " 5 ".into();
        assert_eq!(e.build(&db).unwrap().grace_period_days, 5);
        e.grace = String::new();
        assert_eq!(e.build(&db).unwrap().grace_period_days, 0);
        for bad in ["-1", "2.5", "x", "367"] { e.grace = bad.into(); assert!(e.build(&db).is_err(), "{bad}"); }
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
        let picker = DatePicker::new("2024-02-12", 20261027, true);
        assert_eq!(picker.choose(29), Some(20240229));
        assert_eq!(picker.choose(30), None);
        assert_eq!(picker.choose(0), None);
        let picker = DatePicker::new("2026-02-29", 20261027, true);
        assert_eq!(picker.choose(1), Some(20261001));
    }

    #[test]
    fn picker_month_navigation_crosses_years_within_supported_dates() {
        let mut picker = DatePicker::new("2026-12-31", 20261231, true);
        picker.shift_month(1);
        assert_eq!(picker.choose(1), Some(20270101));
        picker.shift_month(-2);
        assert_eq!(picker.choose(30), Some(20261130));
        let mut first = DatePicker::new("1000-01-01", 10000101, true);
        first.shift_month(-1);
        assert_eq!(first.choose(1), Some(10000101));
        let mut last = DatePicker::new("9999-12-31", 99991231, true);
        last.shift_month(1);
        assert_eq!(last.choose(31), Some(99991231));
    }

    #[test]
    fn picker_with_an_unset_legacy_end_date_still_opens_on_a_valid_month() {
        let picker = DatePicker::new("0000-00-00", 0, true);
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
            events: key(egui::Key::Escape), ..Default::default() }, |ctx| { action = e.show(ctx, &t, &db(), true, 20261027); });
        assert!(matches!(action, PaymentAction::Cancel));
    }

    fn loan_editor() -> PaymentEditor {
        let mut e = PaymentEditor::new(20261001);
        e.description = "Car loan".into(); e.amount = "300".into();
        e.select_recurrence(Recurrence::Months);
        e.end_mode = END_MODE_LOAN_PAID; e.loan_amount = "1000".into();
        e
    }

    #[test]
    fn a_new_loan_payment_cannot_be_tentative() {
        // The scheduler places a tentative payment's full amount, overpaying the loan's last installment.
        let mut e = loan_editor();
        assert!(e.build(&db()).is_ok());
        assert!(!e.tentative_enabled() || !e.tentative, "a loan end disables Tentative");
        e.tentative = true;
        assert!(!e.loan_end_enabled() && !e.weekend_options_enabled());
        let err = e.build(&db()).unwrap_err();
        assert!(err.contains("cannot be tentative"), "{err}");
        // A single payment never stores the loan end, so Tentative is available again.
        e.recurrence = Recurrence::Once;
        assert!(e.tentative_enabled() && e.build(&db()).is_ok());
        let mut e = loan_editor();
        assert!(!e.tentative_enabled());
        e.end_mode = END_MODE_INDEFINITE;
        assert!(e.tentative_enabled());
    }

    #[test]
    fn a_legacy_tentative_loan_can_still_be_edited_unchanged() {
        let mut p = Payment::new(20261001); p.description = "Old loan".into(); p.amount = -300.0;
        p.freq_type = 'm'; p.set_end_mode(END_MODE_LOAN_PAID); p.loan_amount = 1000.0; p.set_tentative(true);
        let mut e = PaymentEditor::edit(&p);
        e.description = "Old loan (renamed)".into();
        assert!(e.build(&db()).is_ok());
    }

    #[test]
    fn category_equal_to_the_description_is_rejected() {
        // The loader would otherwise rename the category to "Rent-" on the next open.
        let mut e = PaymentEditor::new(20261027);
        e.description = "Rent".into(); e.category = " Rent ".into(); e.amount = "1100".into();
        assert_eq!(e.build(&db()).unwrap_err(), SAME_CATEGORY_ERROR);
        e.category = "Home".into();
        assert_eq!(e.build(&db()).unwrap().category.as_deref(), Some("Home"));
    }

    #[test]
    fn picker_columns_follow_the_first_day_of_the_week() {
        // 2026-10-01 is a Thursday; 2026-11-01 is a Sunday.
        let sunday = DatePicker::new("2026-10-15", 0, true);
        let monday = DatePicker::new("2026-10-15", 0, false);
        assert_eq!((sunday.leading_blanks(), monday.leading_blanks()), (4, 3));
        assert_eq!(sunday.headers()[0], "Su");
        assert_eq!(monday.headers(), ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]);
        assert_eq!(DatePicker::new("2026-11-15", 0, true).leading_blanks(), 0);
        assert_eq!(DatePicker::new("2026-11-15", 0, false).leading_blanks(), 6);
    }

    #[test]
    fn amount_boxes_show_two_decimals_and_keep_the_exact_untouched_value() {
        let mut p = Payment::new(20261027); p.description = "Rent".into(); p.amount = -1100.0;
        assert_eq!(PaymentEditor::edit(&p).amount, "1,100.00");
        p.amount = -49.99000000000001;
        let e = PaymentEditor::edit(&p);
        assert_eq!(e.amount, "49.99");
        assert_eq!(e.build(&db()).unwrap().amount.to_bits(), p.amount.to_bits());
        let mut e = PaymentEditor::edit(&p);
        e.amount = "49.990".into();
        assert_eq!(e.build(&db()).unwrap().amount, -49.99);
        let tx = TransactionEditor::new(&p, 20261027, -49.99000000000001);
        assert_eq!(tx.amount, "49.99");
        assert_eq!(tx.values().unwrap().0.to_bits(), (-49.99000000000001f64).to_bits());
    }

    #[test]
    fn an_untouched_legacy_amount_beyond_the_maximum_does_not_block_other_edits() {
        let mut db = db();
        let mut p = Payment::new(20261027); p.description = "Huge".into(); p.amount = -2e12;
        let i = db.add(p);
        let mut e = PaymentEditor::edit(&db.payments[i]);
        e.description = "Huge (renamed)".into();
        let built = e.build(&db).unwrap();
        assert_eq!(built.amount.to_bits(), (-2e12f64).to_bits());
        let id = db.payments[i].id;
        crate::document::put_payment(&mut db, built, Some(id)).unwrap();
        e.amount = "3000000000000".into();
        assert!(e.build(&db).unwrap_err().contains("too large"));
    }

    #[test]
    fn typed_amounts_are_stored_in_whole_cents() {
        for (text, expected) in [("10/3", 3.33), ("2.675", 2.67), ("1200/7", 171.43), ("-10/3", -3.33), ("0.004", 0.0), ("-0.004", 0.0), ("19.99", 19.99)] {
            assert_eq!(parse_amount(text, "Amount").unwrap(), expected, "{text}");
        }
        let mut e = valid_editor(); e.amount = "10/3".into();
        assert_eq!(e.build(&db()).unwrap().amount, -3.33);
    }

    #[test]
    fn amount_fields_evaluate_calculator_formulas() {
        for (text, expected) in [("1200-350", 850.0), ("2.5k", 2500.0), ("(100+20)*3", 360.0), ("$1,200.50 - 200", 1000.5)] {
            assert_eq!(parse_amount(text, "Amount").unwrap(), expected, "{text}");
        }
        assert!(parse_amount("%10", "Amount").unwrap_err().contains("% of income"));
        assert!(parse_amount("1e308*10", "Amount").is_err());
        assert!(parse_amount("2t", "Amount").unwrap_err().contains("too large"));
        assert!(parse_amount("5+", "Amount").is_err());
        let mut e = valid_editor(); e.amount = "100+20".into();
        assert_eq!(e.build(&db()).unwrap().amount, -120.0);
    }

    #[test]
    fn changing_a_payment_with_past_occurrences_asks_once_before_applying() {
        let mut p = Payment::new(20250101); p.description = "Rent".into(); p.amount = -1100.0; p.freq_type = 'm';
        let mut e = PaymentEditor::edit(&p);
        e.amount = "1200".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None));
        assert!(e.notice.as_deref().unwrap().contains("2025-01-01"));
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(ref q, None) if q.amount == -1200.0));
        // Fixing a typo after the notice must not swallow the next click: the button promised an apply.
        let mut e = PaymentEditor::edit(&p);
        e.amount = "1200".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None));
        e.description = "Rent (new)".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(ref q, None) if q.description == "Rent (new)"));
        // Renaming changes no occurrence; a payment that has not started yet has no past.
        let mut e = PaymentEditor::edit(&p); e.description = "Home rent".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, None)));
        let mut e = PaymentEditor::edit(&p); e.amount = "1200".into();
        assert!(matches!(e.apply(&db(), 20250101), PaymentAction::Apply(_, None)));
        assert!(matches!(PaymentEditor::new(20200101).apply(&db(), 20261027), PaymentAction::None));
    }

    fn monthly_rent() -> Payment {
        let mut p = Payment::new(20250101); p.id = 3; p.description = "Rent".into(); p.amount = -1100.0;
        p.freq_type = 'm'; p.set_end_mode(END_MODE_INDEFINITE);
        p
    }

    #[test]
    fn editing_from_a_calendar_occurrence_offers_from_that_date_or_all() {
        // 04 §1.3 step 16 / §2: opened from a cell, an amount or schedule change asks
        // "Change the FUTURE transactions only starting on <date>" (preselected) or "ALL".
        let p = monthly_rent();
        for (change, choose_all) in [("amount", false), ("amount", true), ("schedule", false)] {
            let mut e = PaymentEditor::edit_at(&p, Some(20261101));
            assert_eq!(e.split_date, Some(20261101));
            if change == "amount" { e.amount = "1200".into(); } else { e.frequency = "2".into(); }
            assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None), "the first Apply asks");
            assert_eq!(e.scope, Some(ChangeScope::FromDate));
            assert!(e.notice.is_none(), "the choice replaces the past-months notice");
            assert_eq!(e.apply_label(), "Apply from 2026-11-01");
            if choose_all { e.scope = Some(ChangeScope::All); assert_eq!(e.apply_label(), "Apply to all occurrences"); }
            let expected = if choose_all { None } else { Some(20261101) };
            assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, from) if from == expected), "{change} {choose_all}");
        }
        // No prompt for a rename, or from the first occurrence (nothing before it to keep).
        let mut e = PaymentEditor::edit_at(&p, Some(20261101)); e.description = "Home rent".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, None)));
        let mut e = PaymentEditor::edit_at(&p, Some(20250101)); e.amount = "1200".into();
        assert_eq!(e.split_date, None);
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, None)));
        // The Payments page (no date) keeps the all-occurrences notice.
        let mut e = PaymentEditor::edit_at(&p, None); e.amount = "1200".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None));
        assert!(e.notice.is_some() && e.scope.is_none());
    }

    #[test]
    fn a_visible_scope_choice_stands_after_the_triggering_change_is_undone() {
        // Round 5: change the amount (the panel and "Apply from <date>" appear), set it back and
        // rename: the button still says "Apply from 2026-11-01", so that is what it must do.
        let mut e = PaymentEditor::edit_at(&monthly_rent(), Some(20261101));
        e.amount = "1200".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None));
        assert_eq!(e.apply_label(), "Apply from 2026-11-01");
        e.amount = e.amount_seed.clone();
        e.description = "Home rent".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, Some(20261101))));
        e.scope = Some(ChangeScope::All);
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, None)));
        // Undone completely, "Apply from <date>" has nothing to split off.
        e.scope = Some(ChangeScope::FromDate);
        e.description = "Rent".into();
        assert!(matches!(e.apply(&db(), 20261027), PaymentAction::Apply(_, None)));
    }

    #[test]
    fn the_from_date_choice_and_footer_fit_a_compact_window() {
        let size = egui::vec2(900.0, 600.0);
        for dark in [false, true] {
            let ctx = egui::Context::default();
            let t = Tokens::new(&config::Colors::theme(if dark { config::Theme::Dark } else { config::Theme::Light }), dark);
            theme::install_fonts(&ctx);
            theme::apply(&ctx, &t);
            let mut e = PaymentEditor::edit_at(&monthly_rent(), Some(20261101));
            e.amount = "1200".into();
            assert!(matches!(e.apply(&db(), 20261027), PaymentAction::None));
            let mut output = None;
            for _ in 0..4 {
                output = Some(ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() },
                    |ctx| { let _ = e.show(ctx, &t, &db(), true, 20261027); }));
            }
            let output = output.unwrap();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for label in ["Transactions from 2026-11-01 onward", "All transactions, past and future", "Cancel", "Apply from 2026-11-01"] {
                let rect = transaction_label(&output, label);
                assert!(screen.contains_rect(rect), "{label} outside the compact window (dark={dark}): {rect:?}");
            }
        }
    }

    #[test]
    fn loan_principal_below_the_amounts_already_paid_is_refused_by_the_editor() {
        // Saved payments of 50 and 250 total 300; lowering the principal to 200 would overpay.
        let mut db = db();
        let mut p = Payment::new(20260101); p.description = "Car loan".into(); p.amount = -100.0; p.freq_type = 'm';
        p.set_end_mode(END_MODE_LOAN_PAID); p.loan_amount = 300.0;
        p.set_custom_amount(20260101, -50.0); p.set_custom_amount(20260201, -250.0);
        let i = db.add(p);
        let mut e = PaymentEditor::edit(&db.payments[i]);
        e.loan_amount = "200".into();
        let err = e.build(&db).unwrap_err();
        assert!(err.contains("principal"), "{err}");
        e.loan_amount = "300".into();
        assert!(e.build(&db).is_ok());
    }

    #[test]
    fn a_typed_plus_sign_under_expense_is_refused_like_a_minus_under_income() {
        let db = db();
        let mut e = valid_editor(); // Expense
        e.amount = "+42.25".into();
        let err = e.build(&db).unwrap_err();
        assert!(err.contains("plus sign") && err.contains("Expense is selected"), "{err}");
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, 42.25);
        tx.amount_kind = AmountKind::Expense; tx.amount = "+5".into();
        assert!(tx.values().is_err());
        let mut e = valid_editor(); e.amount_kind = AmountKind::Income; e.amount = "+42.25".into();
        assert_eq!(e.build(&db).unwrap().amount, 42.25);
    }

    #[test]
    fn zero_amounts_open_as_expense_like_the_original() {
        let mut p = Payment::new(20261027); p.description = "Zero".into(); p.amount = 0.0;
        assert_eq!(PaymentEditor::edit(&p).amount_kind, AmountKind::Expense);
        assert_eq!(TransactionEditor::new(&p, 20261027, 0.0).amount_kind, AmountKind::Expense);
        assert_eq!(TransactionEditor::new(&p, 20261027, -0.0).amount_kind, AmountKind::Expense);
        assert_eq!(TransactionEditor::new(&p, 20261027, 1.0).amount_kind, AmountKind::Income);
    }

    #[test]
    fn a_negative_amount_refusal_names_the_polarity_already_selected() {
        let db = db();
        let mut e = valid_editor(); // Expense
        for text in ["(50)", "-0"] {
            e.amount = text.into();
            let err = e.build(&db).unwrap_err();
            assert!(err.contains("Expense is selected") && !err.contains("choose Expense or Income"), "{text}: {err}");
        }
        let mut e = valid_editor(); e.amount_kind = AmountKind::Income;
        e.amount = "(50)".into();
        let err = e.build(&db).unwrap_err();
        assert!(err.contains("Income is selected"), "{err}");
    }

    #[test]
    fn a_last_weekday_start_snaps_only_when_the_repeat_is_last_weekday() {
        let db = db();
        let mut p = Payment::new(20261027); p.description = "Last Tuesday".into(); p.amount = -25.5; p.freq_type = 'L';
        p.set_end_mode(END_MODE_INDEFINITE);
        let mut e = PaymentEditor::edit(&p);
        assert_eq!(e.recurrence, Recurrence::LastWeekday);
        e.start_date = "2026-10-13".into(); // a Tuesday outside the last week of October
        assert_eq!(e.build(&db).unwrap().start_date, 20261027);
        e.recurrence = Recurrence::Once;
        let once = e.build(&db).unwrap();
        assert_eq!((once.start_date, once.n_pays, once.end_mode()), (20261013, 1, END_MODE_N_PAYS));
    }

    #[test]
    fn daily_and_weekly_repeats_above_the_stored_limit_are_refused() {
        // The loader clamps stored frequencies to 100,000, so a larger one would change on reopen.
        let db = db();
        for recurrence in [Recurrence::Days, Recurrence::Weeks] {
            let mut e = valid_editor(); e.recurrence = recurrence; e.end_mode = END_MODE_INDEFINITE;
            e.frequency = "150000".into();
            let err = e.build(&db).unwrap_err();
            assert!(err.contains("100,000"), "{recurrence:?}: {err}");
            e.frequency = "100000".into();
            assert_eq!(e.build(&db).unwrap().frequency, 100000);
        }
        // Monthly repeats from year 1000 can reach 107,988 months, so the same limit applies.
        let mut e = valid_editor(); e.recurrence = Recurrence::Months; e.end_mode = END_MODE_INDEFINITE;
        e.start_date = "1000-01-01".into(); e.frequency = "100001".into();
        assert!(e.build(&db).unwrap_err().contains("100,000"));
    }

    #[test]
    fn moving_an_indefinite_start_later_saves_the_end_date_the_file_reopens_with() {
        let mut db = db();
        let mut p = Payment::new(20260101); p.description = "Gym".into(); p.amount = -40.0;
        p.set_end_mode(END_MODE_INDEFINITE); p.end_date = 20260630; // stale, left from an earlier end date
        let i = db.add(p);
        let id = db.payments[i].id;
        let mut e = PaymentEditor::edit(&db.payments[i]);
        e.start_date = "2026-10-01".into();
        let built = e.build(&db).unwrap();
        crate::document::put_payment(&mut db, built, Some(id)).unwrap();
        let saved = db.payments[db.get_by_id(id).unwrap()].clone();
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        assert_eq!(loaded.payments[0].start_date, 20261001);
        assert_eq!(loaded.payments[0].end_date, saved.end_date, "reopened end date");
    }

    #[test]
    fn an_nth_weekday_repeat_validates_only_the_fields_it_shows() {
        // The "Every" box is hidden for the numbered-weekday repeat, so its text must not block a save.
        let db = db();
        let mut p = Payment::new(20261027); p.description = "Second Tuesday".into(); p.amount = -25.5; p.freq_type = 'W';
        p.set_end_mode(END_MODE_INDEFINITE);
        let mut e = PaymentEditor::edit(&p);
        assert_eq!(e.recurrence, Recurrence::NthWeekday);
        e.frequency = "abc".into();
        let built = e.build(&db).unwrap();
        assert_eq!((built.freq_type, built.frequency), ('W', p.frequency));
        e.frequency = "0".into();
        assert!(e.build(&db).is_ok());
        let mut e = valid_editor(); e.recurrence = Recurrence::NthWeekday; e.frequency = "abc".into();
        assert_eq!(e.build(&db).unwrap().frequency, 1);
    }

    #[test]
    fn a_literal_beyond_the_amount_range_is_reported_as_too_large() {
        let overflow = format!("1{}", "0".repeat(400));
        for text in ["1e999", "-1e999", overflow.as_str()] {
            let err = parse_amount(text, "Amount").unwrap_err();
            assert!(err.contains("too large"), "{text}: {err}");
        }
        for text in ["NaN", "inf", "-inf"] {
            let err = parse_amount(text, "Amount").unwrap_err();
            assert!(err.contains("must be a number") && !err.contains("too large"), "{text}: {err}");
        }
        let mut tx = TransactionEditor::new(&Payment::new(20261027), 20261027, 1.0);
        tx.amount = "1e999".into();
        assert!(tx.values().unwrap_err().contains("too large"));
    }

    fn payment_frame(ctx: &egui::Context, t: &Tokens, editor: &mut PaymentEditor, size: egui::Vec2) -> egui::FullOutput {
        ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() },
            |ctx| { let _ = editor.show(ctx, t, &db(), true, 20261027); })
    }

    fn has_label(output: &egui::FullOutput, label: &str) -> bool {
        output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label))
    }

    #[test]
    fn enabled_sits_under_the_amount_and_the_captions_match_the_form() {
        let size = egui::vec2(1200.0, 1000.0);
        let (ctx, t) = transaction_context();
        let mut e = PaymentEditor::edit(&monthly_rent());
        for _ in 0..3 { payment_frame(&ctx, &t, &mut e, size); }
        let output = payment_frame(&ctx, &t, &mut e, size);
        let amount = transaction_label(&output, "Amount").center().y;
        let enabled = transaction_label(&output, "Enabled in calendar").center().y;
        let category = transaction_label(&output, "Category (optional)").center().y;
        assert!(amount < enabled && enabled < category, "{amount} {enabled} {category}");
        assert!(!has_label(&output, "Category · Optional"));
        // The repeat caption is the last row inside the scroll area at this size; the unit beside
        // the number and the disclosure are below it, so only the caption is checked on screen.
        assert!(has_label(&output, "Repeat every"));
    }

    #[test]
    fn the_more_options_title_counts_the_hidden_options_that_are_set() {
        let mut e = PaymentEditor::edit(&monthly_rent());
        assert_eq!(e.more_options_title(), "More schedule options");
        e.reminder = true; e.sticky = true;
        assert_eq!(e.more_options_title(), "More schedule options (2 set)");
        e.grace = " 5 ".into();
        assert_eq!(e.more_options_title(), "More schedule options (3 set)");
        e.grace = "0".into();
        assert_eq!(e.more_options_title(), "More schedule options (2 set)");
    }

}
