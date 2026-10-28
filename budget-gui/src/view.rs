//! Month view-model: everything the widgets draw. A [`Month`] holds what does not depend on the
//! search (built once per document revision and month); [`view`] narrows it to a search, so a
//! keystroke only matches transactions and totals the matching ones.

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::numfmt::{balance_display, format_frac0, format_frac2};
use budget_core::{add_period, calendar_field, stats, BudgetDb, Cal, Field, Payment, END_MODE_INDEFINITE, FLAG_IMPORTANT, FLAG_PAID,
    SUFFIX_ADJUSTED, SUFFIX_LAST, SUFFIX_NONE};
use eframe::egui::Color32;
use egui_phosphor::regular as ph;
use std::rc::Rc;

use crate::theme::rgb;

/// `#,##0` or `#,##0.00`, with a typographic minus.
pub fn money(v: f64, decimals: bool) -> String {
    let s = if decimals { format_frac2(v) } else { format_frac0(v) };
    s.replacen('-', "\u{2212}", 1)
}

/// Percentage share as a whole number, never "-0".
pub fn pct_text(p: f64) -> String {
    let r = p.round();
    format!("{:.0}", if r == 0.0 { 0.0 } else { r }).replacen('-', "\u{2212}", 1)
}

/// A brief-summary share, `?` when it has no base (05 §13).
#[cfg(test)]
pub fn pct_cell(p: Option<f64>) -> String { p.map_or_else(|| "?".to_string(), pct_text) }

/// The summary panel's share column: an unsigned magnitude with a `%` unit (`29%`), so it
/// never reads like a second amount. `signed` keeps U+2212 for the disposable-income row;
/// `?` when there is no base.
pub fn share_cell(p: Option<f64>, signed: bool) -> String {
    p.map_or_else(|| "?".to_string(), |p| format!("{}%", pct_text(if signed { p } else { p.abs() })))
}

/// Whether `money(v, decimals)` shows a minus sign. The colour follows the text, so a value that
/// rounds to zero is never coloured as negative and every figure printed as "−1" is.
pub fn is_negative(v: f64, decimals: bool) -> bool {
    money(v, decimals).starts_with('\u{2212}')
}


/// Day-balance classification in the original's precedence: low, then min, then max. A flat
/// month (every day equal, including an empty budget) has no lowest, highest or low day.
pub fn balance_kind(closing: f64, min: f64, max: f64, threshold: f64) -> BalKind {
    if min == max { BalKind::Normal }
    else if closing + 0.05 < threshold { BalKind::Low }
    else if closing == min { BalKind::Min }
    else if closing == max { BalKind::Max }
    else { BalKind::Normal }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BalKind { Normal, Max, Min, Low }

#[derive(Clone)]
pub struct Tip {
    pub description: String,
    pub category: Option<String>,
    pub amount: f64,
    pub planned: f64,
    pub balance_after: f64,
    pub note: Option<String>,
    pub suffix: char,
}

#[derive(Clone)]
pub struct TranRow {
    pub payment_id: i32,
    pub date: i32,
    pub icon: &'static str,
    pub text: String,
    pub amount: String,
    pub value: f64,
    pub flag: char,
    pub hilite: Option<Color32>,
    pub disabled: bool,
    pub filter: String,
    pub tip: Tip,
    /// Days on which this occurrence may be paid (grace period or tentative period), for hover highlighting.
    pub valid_days: Vec<i32>,
}

/// `adjustment` is the reconciliation difference (closing minus the computed balance), 0 when not adjusted.
#[derive(Clone)]
pub struct Balance { pub text: String, pub value: f64, pub kind: BalKind, pub adjusted: bool, pub adjustment: f64 }

impl Balance {
    /// The strip text: the balance, then `′` on an adjusted day, followed by `(diff)` when |diff| >= 1 (analysis/05 §1.7).
    pub fn strip_text(&self, decimals: bool) -> String {
        let mut txt = self.text.clone();
        if self.adjusted {
            txt.push('\u{2032}');
            if self.adjustment.abs() >= 1.0 { txt.push_str(&format!("({})", money(balance_display(self.adjustment, decimals), decimals))); }
        }
        txt
    }
}

#[derive(Clone)]
pub struct CellData {
    pub day: i32,
    pub date: i32,
    pub in_month: bool,
    pub is_today: bool,
    pub weekend: bool,
    /// Shared with the cached [`Month`], so narrowing to a search copies no row.
    pub rows: Vec<Rc<TranRow>>,
    pub balance: Option<Balance>,
    pub note: Option<String>,
    /// Payments with a deleted or moved-away occurrence on this day: (payment id, short description).
    pub restorable: Vec<(i32, String)>,
}

#[derive(Clone)]
pub struct QuickStats { pub opening: f64, pub closing: f64, pub min: f64, pub max: f64, pub avg: f64, pub income: f64, pub expenses: f64 }

/// `pct` is `None` (shown as `?`) when the month has neither income nor expenses.
#[derive(Clone)]
pub struct BriefRow { pub desc: String, pub amount: f64, pub pct: Option<f64>, pub diff: Option<f64>, pub total: bool, pub filter: String }

/// What a month's search-independent content depends on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonthKey {
    /// The document revision: (budget generation, the engine's update count).
    pub revision: (u64, i32),
    pub month: i32,
    pub today: i32,
    pub decimals: bool,
    pub sunday_first: bool,
    pub min_balance: f64,
}

/// A [`MonthView`] is the month narrowed to the search text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewKey { pub month: MonthKey, pub search: String }

/// The search-independent part of a month view: every transaction row, balances, the quick
/// statistics and the unfiltered summary.
pub struct Month {
    pub key: MonthKey,
    cells: Vec<CellData>,
    /// Payment index of each cell's rows, to match them against a search.
    sources: Vec<Vec<usize>>,
    quick: QuickStats,
    brief: Vec<BriefRow>,
    shares_of_expenses: bool,
    total: usize,
    below_min: bool,
}

pub struct MonthView {
    pub key: ViewKey,
    pub cells: Vec<CellData>,
    pub quick: QuickStats,
    pub brief: Vec<BriefRow>,
    pub shown: usize,
    pub total: usize,
    pub below_min: bool,
    /// Brief-summary shares are of total expenses (the month has no income), not of income.
    pub shares_of_expenses: bool,
    /// A parsed filter narrows this view. Restoring is gated on this, not on the raw search
    /// text, so an unapplied (invalid or leading "-") search does not disable Restore.
    pub filtered: bool,
}

/// Phosphor glyph for a payment, from its icon file name, then description/category keywords.
fn icon_for(image: Option<&str>, desc: &str, cat: Option<&str>) -> &'static str {
    let hay = format!("{} {} {}", image.unwrap_or(""), desc, cat.unwrap_or("")).to_ascii_lowercase();
    const MAP: &[(&str, &str)] = &[
        ("house", ph::HOUSE), ("rent", ph::HOUSE), ("mortgage", ph::HOUSE), ("home", ph::HOUSE),
        ("car", ph::CAR), ("auto", ph::CAR), ("gas", ph::GAS_PUMP), ("fuel", ph::GAS_PUMP),
        ("apple", ph::BASKET), ("grocer", ph::BASKET), ("food", ph::FORK_KNIFE), ("restaurant", ph::FORK_KNIFE),
        ("dollar", ph::CURRENCY_DOLLAR), ("salary", ph::CURRENCY_DOLLAR), ("pay", ph::CURRENCY_DOLLAR), ("income", ph::CURRENCY_DOLLAR),
        ("pig", ph::PIGGY_BANK), ("saving", ph::PIGGY_BANK), ("bank", ph::BANK),
        ("tv", ph::TELEVISION), ("cable", ph::TELEVISION), ("power", ph::LIGHTNING), ("hydro", ph::LIGHTNING), ("electric", ph::LIGHTNING),
        ("mobile", ph::DEVICE_MOBILE), ("phone", ph::PHONE), ("internet", ph::WIFI_HIGH), ("wifi", ph::WIFI_HIGH),
        ("pct", ph::CREDIT_CARD), ("visa", ph::CREDIT_CARD), ("card", ph::CREDIT_CARD), ("credit", ph::CREDIT_CARD), ("loan", ph::BANK),
        ("paper", ph::FILE_TEXT), ("insur", ph::FILE_TEXT), ("tax", ph::FILE_TEXT), ("water", ph::DROP),
        ("medic", ph::FIRST_AID), ("health", ph::FIRST_AID), ("gift", ph::GIFT), ("school", ph::GRADUATION_CAP),
        ("travel", ph::AIRPLANE), ("flight", ph::AIRPLANE), ("coffee", ph::COFFEE), ("music", ph::MUSIC_NOTE),
        ("game", ph::GAME_CONTROLLER), ("pet", ph::PAW_PRINT), ("cloth", ph::T_SHIRT), ("book", ph::BOOK),
        ("bus", ph::BUS), ("train", ph::TRAIN), ("shop", ph::SHOPPING_CART), ("util", ph::LIGHTNING),
    ];
    MAP.iter().find(|(k, _)| hay.contains(k)).map_or(ph::RECEIPT, |(_, g)| g)
}

/// Filter expression that isolates one payment / category (spaces escaped as the filter language requires).
fn esc(s: &str) -> String { s.replace(' ', "\\ ") }

/// The nominal date of the slot after payment `p`'s last one (slot number `n_pays`), as its
/// recurrence would continue; `None` past the last representable date.
fn next_slot(p: &Payment) -> Option<i32> {
    let field = calendar_field(p.freq_type);
    let mut c = Cal::from_int(p.start_date);
    for n in 1..=p.n_pays.max(1) {
        add_period(&mut c, field, p.frequency, p.start_date, n);
        if c.get() > 99_991_231 { return None; }
    }
    Some(c.get())
}

/// Whether payment `pi` goes on as another record, as after "This and all future occurrences":
/// an enabled payment with the same description and category starts on its next scheduled date.
fn continues(db: &BudgetDb, pi: usize) -> bool {
    let p = &db.payments[pi];
    if p.end_mode() == END_MODE_INDEFINITE { return false; }
    let Some(next) = next_slot(p) else { return false };
    db.payments.iter().enumerate()
        .any(|(j, q)| j != pi && !q.disabled && q.start_date == next && q.description == p.description && q.category == p.category)
}

/// The marker shown for payment `pi`'s transaction on `date`: the engine's, except that the Last
/// marker is dropped where the payment continues on another record (see [`continues`]). The
/// occurrence is then marked as the engine marks any other: adjusted when it has a dated amount,
/// was moved off a weekend or adds up several scheduled slots (`slots`).
fn shown_suffix(db: &BudgetDb, pi: usize, date: i32, suffix: char, slots: usize) -> char {
    if suffix != SUFFIX_LAST || !continues(db, pi) { return suffix; }
    let dated = db.payments[pi].opt.iter().any(|o| o.date == date && !o.deleted);
    if dated || slots > 1 || !db.is_regular_payment_on(pi, date) { SUFFIX_ADJUSTED } else { SUFFIX_NONE }
}

/// The month of `cur` without a search; `key` names what it was built from.
pub fn month(db: &mut BudgetDb, cfg: &BudgetConfig, cur: &Cal, today: i32, key: MonthKey) -> Month {
    let decimals = cfg.show_decimals_in_cv;
    let n_days = cur.max_day();
    let mut closings = Vec::new();
    for d in 1..=n_days {
        let mut cal = *cur;
        cal.set(Field::Day, d);
        if let Some(day) = db.get_day(&cal) { closings.push(day.closing_balance); }
    }
    let min = closings.iter().cloned().fold(f64::MAX, f64::min);
    let max = closings.iter().cloned().fold(f64::MIN, f64::max);
    let first_dow = cur.day_of_week();
    let lead = if cfg.first_day_of_week_is_sunday { first_dow } else { (first_dow + 6) % 7 };
    let total_cells = lead + n_days;
    let trail = (7 - total_cells % 7) % 7;
    let mut total = 0;
    let (mut cells, mut sources) = (Vec::new(), Vec::new());
    for i in 0..(total_cells + trail) {
        let mut cal = *cur;
        cal.add(Field::Day, i - lead);
        let in_month = i >= lead && i < total_cells;
        let date = cal.get();
        let mut cell = CellData { day: cal.day(), date, in_month, is_today: date == today, weekend: cal.is_weekend(), rows: Vec::new(), balance: None, note: None, restorable: Vec::new() };
        let mut source = Vec::new();
        if in_month {
            if let Some(day) = db.get_day(&cal) {
                let trans: Vec<(usize, f64, f64, char)> = day.transactions.iter().map(|t| (t.pmt, t.amount, t.balance, t.suffix)).collect();
                let regulars = day.regulars.clone();
                let closing = day.closing_balance;
                let adjusted = day.closing_balance_adjusted;
                let adjustment = if adjusted { day.adjustment_diff } else { 0.0 };
                let restorable = day.restorable.clone();
                cell.restorable = restorable.into_iter().map(|pi| (db.payments[pi].id, db.payments[pi].short_description().to_string())).collect();
                cell.note = db.day_note(&cal);
                for (pi, amount, balance, suffix) in trans {
                    let p = &db.payments[pi];
                    total += 1;
                    let flag = p.get_flag(date);
                    let hi = p.get_hilighting(date);
                    let hi = if hi == -1 { p.default_hilighting() } else { hi };
                    let note = p.get_note(date);
                    // The suffix marker is drawn as a coloured dot by the calendar, not as text.
                    let suffix = shown_suffix(db, pi, date, suffix, regulars.iter().filter(|&&r| r == pi).count());
                    let amt = money(amount, decimals);
                    let valid_days = if p.grace_period_days > 0 || p.is_tentative() { db.valid_payment_days(pi, date) } else { Vec::new() };
                    source.push(pi);
                    cell.rows.push(Rc::new(TranRow {
                        payment_id: p.id,
                        date,
                        icon: icon_for(p.image_path.as_deref(), &p.description, p.category.as_deref()),
                        text: p.short_description().to_string(),
                        amount: amt,
                        value: amount,
                        flag: if flag == FLAG_PAID || flag == FLAG_IMPORTANT { flag } else { '\0' },
                        // 05 §1.6: the highlight applies to disabled payments too; only the text is muted.
                        hilite: (hi != -1).then(|| rgb(hi as u32)),
                        disabled: p.disabled,
                        filter: format!("desc:{}", esc(p.short_description())),
                        tip: Tip { description: p.description.clone(), category: p.category.clone(), amount, planned: p.amount, balance_after: balance, note, suffix },
                        valid_days,
                    }));
                }
                let kind = balance_kind(closing, min, max, cfg.min_balance_4_max_saving);
                cell.balance = Some(Balance { text: money(balance_display(closing, decimals), decimals), value: closing, kind, adjusted, adjustment });
            }
        }
        cells.push(cell);
        sources.push(source);
    }
    // The summary tiles must reconcile with the opening/closing balances, which are never filtered,
    // so they always use unfiltered totals; a search only narrows the calendar and the brief summary.
    let st = stats::calc_statistics(db, cur, 1, false, None);
    let mut last = *cur;
    last.set_day_last();
    let opening = db.get_day(cur).map_or(0.0, |d| d.opening_balance);
    let closing = db.get_day(&last).map_or(0.0, |d| d.closing_balance);
    let avg = if closings.is_empty() { 0.0 } else { closings.iter().sum::<f64>() / closings.len() as f64 };
    let quick = QuickStats { opening, closing, min, max, avg, income: st.total_income, expenses: st.total_expense };
    let (brief, shares_of_expenses) = brief(&st);
    Month { key, cells, sources, quick, brief, shares_of_expenses, total, below_min: min + 0.05 < cfg.min_balance_4_max_saving && min != max }
}

/// `month` narrowed to `filter` (the parsed `search`; `None` shows every transaction): the
/// calendar lists the matching transactions and the brief summary totals them.
pub fn view(month: &Month, db: &mut BudgetDb, cur: &Cal, filter: Option<&Filter>, search: &str) -> MonthView {
    let key = ViewKey { month: month.key.clone(), search: search.to_string() };
    let Some(f) = filter else {
        return MonthView { key, cells: month.cells.clone(), quick: month.quick.clone(), brief: month.brief.clone(), shown: month.total,
            total: month.total, below_min: month.below_min, shares_of_expenses: month.shares_of_expenses, filtered: false };
    };
    let mut shown = 0;
    let cells = month.cells.iter().zip(&month.sources).map(|(cell, source)| {
        let rows: Vec<Rc<TranRow>> = cell.rows.iter().zip(source)
            .filter(|(row, &pi)| f.is_match(&db.payments[pi], row.value, row.date))
            .map(|(row, _)| row.clone()).collect();
        shown += rows.len();
        CellData { day: cell.day, date: cell.date, in_month: cell.in_month, is_today: cell.is_today, weekend: cell.weekend, rows,
            balance: cell.balance.clone(), note: cell.note.clone(), restorable: cell.restorable.clone() }
    }).collect();
    let st = stats::calc_statistics(db, cur, 1, false, Some(f));
    let (brief, shares_of_expenses) = brief(&st);
    MonthView { key, cells, quick: month.quick.clone(), brief, shown, total: month.total, below_min: month.below_min, shares_of_expenses, filtered: true }
}

/// The brief summary of `st` and whether its shares are of expenses (the month has no income).
fn brief(st: &stats::Stats) -> (Vec<BriefRow>, bool) {
    // brief summary: rows >= 5% of the base, the rest as "Other…"; income first, then disposable income.
    // 05 §13: shares are of total income, or of total expenses when there is no income, else `?`.
    let base = if st.total_income != 0.0 { st.total_income } else { st.total_expense };
    let pct = |a: f64| (base != 0.0).then(|| a / base * 100.0);
    let threshold = base.abs() * 0.05;
    let income_diff = st.total_income - st.total_planned_income;
    let mut brief = vec![BriefRow { desc: "Total income".into(), amount: st.total_income, pct: pct(st.total_income),
        diff: (income_diff.abs() >= 5.0).then_some(income_diff), total: true, filter: ">0".into() }];
    let mut other = 0.0;
    for r in st.income.iter().chain(st.expense.iter()).filter(|r| !r.is_sub) {
        if r.amount.abs() >= threshold {
            let diff = r.amount - r.planned_amount;
            let filter = if r.pmt.is_some() { format!("desc:{}", esc(r.key.trim())) } else { format!("cat:{}", esc(&r.key)) };
            brief.push(BriefRow { desc: r.key.trim().to_string(), amount: r.amount, pct: pct(r.amount), diff: (diff.abs() >= 5.0).then_some(diff), total: false, filter });
        } else {
            other += r.amount;
        }
    }
    if other != 0.0 { brief.push(BriefRow { desc: "Other…".into(), amount: other, pct: pct(other), diff: None, total: false, filter: String::new() }); }
    let net = st.total_income + st.total_expense;
    brief.push(BriefRow { desc: "Disposable income".into(), amount: net, pct: pct(net), diff: None, total: true, filter: String::new() });
    (brief, st.total_income == 0.0 && st.total_expense != 0.0)
}

#[cfg(test)]
mod build_tests {
    use super::*;
    use budget_core::Payment;

    fn db_with(payments: Vec<Payment>) -> (BudgetDb, Cal) {
        let cur = Cal::from_int(20261001);
        let mut db = BudgetDb::new(&cur, 12, false);
        for p in payments { db.add(p); }
        db.rebuild_db(&cur, 12, false);
        (db, cur)
    }

    fn payment(date: i32, desc: &str, amount: f64) -> Payment {
        let mut p = Payment::new(date); p.description = desc.into(); p.amount = amount; p
    }

    fn month(db: &mut BudgetDb, cur: &Cal, filter: Option<&Filter>) -> MonthView {
        let base = super::month(db, &BudgetConfig::default(), cur, 20261027, MonthKey::default());
        view(&base, db, cur, filter, filter.map_or("", |f| f.filter.as_str()))
    }

    fn cell(view: &MonthView, date: i32) -> &CellData { view.cells.iter().find(|c| c.date == date && c.in_month).unwrap() }

    #[test]
    fn brief_shares_fall_back_to_expenses_without_income_and_show_unknown_without_either() {
        // 05 §13: an expense-only (e.g. cat:Auto filtered) month must not read 0% everywhere.
        let (mut db, cur) = db_with(vec![payment(20261005, "Car loan", -400.0), payment(20261012, "Gasoline", -120.0),
            payment(20261019, "Car insurance", -80.0)]);
        let view = month(&mut db, &cur, None);
        let share = |desc: &str| view.brief.iter().find(|r| r.desc == desc).map(|r| pct_cell(r.pct)).unwrap();
        assert_eq!((share("Car loan"), share("Gasoline"), share("Car insurance")), ("67".into(), "20".into(), "13".into()));
        assert_eq!(share("Disposable income"), "100");
        let (mut empty, cur) = db_with(Vec::new());
        let view = month(&mut empty, &cur, None);
        assert!(view.brief.iter().all(|r| r.pct.is_none()));
        assert_eq!(pct_cell(view.brief[0].pct), "?");
        assert!(!view.shares_of_expenses);
    }

    #[test]
    fn summary_shares_are_unsigned_percentages_except_a_negative_disposable_income() {
        let (mut db, cur) = db_with(vec![payment(20261001, "Salary", 1000.0), payment(20261005, "Rent", -1250.0)]);
        let view = month(&mut db, &cur, None);
        assert!(!view.shares_of_expenses);
        let row = |desc: &str| view.brief.iter().find(|r| r.desc == desc).unwrap();
        assert_eq!(share_cell(row("Rent").pct, false), "125%");
        assert_eq!(share_cell(row("Total income").pct, false), "100%");
        assert_eq!(share_cell(row("Disposable income").pct, true), "\u{2212}25%");
        assert_eq!(share_cell(None, false), "?");
        assert_eq!(share_cell(Some(-0.2), true), "0%");
        let (mut db, cur) = db_with(vec![payment(20261005, "Car loan", -400.0)]);
        assert!(month(&mut db, &cur, None).shares_of_expenses);
    }

    #[test]
    fn a_search_narrows_the_cached_month_and_its_summary_but_not_the_balance_tiles() {
        let (mut db, cur) = db_with(vec![payment(20261001, "Salary", 1000.0), payment(20261005, "Rent", -600.0),
            payment(20261005, "Gym", -40.0), payment(20261020, "Groceries", -120.0)]);
        let base = super::month(&mut db, &BudgetConfig::default(), &cur, 20261027, MonthKey::default());
        let all = view(&base, &mut db, &cur, None, "");
        assert_eq!((all.shown, all.total, all.filtered), (4, 4, false));
        let f = Filter::new("<-100", 20261027).unwrap();
        let narrowed = view(&base, &mut db, &cur, Some(&f), "<-100");
        assert_eq!((narrowed.shown, narrowed.total, narrowed.filtered), (2, 4, true));
        assert_eq!(cell(&narrowed, 20261005).rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["Rent"]);
        assert!(cell(&narrowed, 20261001).rows.is_empty() && cell(&narrowed, 20261001).balance.is_some());
        // The tiles reconcile with the unfiltered balances; the summary totals the matches only.
        assert_eq!((narrowed.quick.income, narrowed.quick.expenses), (1000.0, -760.0));
        assert_eq!(narrowed.brief[0].amount, 0.0);
        assert_eq!(narrowed.brief.last().unwrap().amount, -720.0);
        assert!(narrowed.shares_of_expenses && !all.shares_of_expenses);
        // The same cached month serves the next keystroke and clearing the search.
        let rent = Filter::new("rent", 20261027).unwrap();
        assert_eq!(view(&base, &mut db, &cur, Some(&rent), "rent").shown, 1);
        assert_eq!(view(&base, &mut db, &cur, None, "").shown, 4);
    }

    fn monthly(start: i32, desc: &str, amount: f64) -> Payment {
        let mut p = payment(start, desc, amount);
        p.category = Some("Home".into());
        p.set_end_mode(budget_core::END_MODE_INDEFINITE);
        p
    }

    /// The engine's and the calendar's marker of the payment's transaction on `date`.
    fn suffixes(db: &mut BudgetDb, date: i32, desc: &str) -> (char, char) {
        let cur = { let mut c = Cal::from_int(date); c.set_day_one(); c };
        let pi = db.payments.iter().enumerate().filter(|(_, p)| p.description == desc).map(|(i, _)| i).collect::<Vec<_>>();
        let engine = db.get_day(&Cal::from_int(date)).unwrap().transactions.iter()
            .find(|t| pi.contains(&t.pmt)).unwrap().suffix;
        let view = month(db, &cur, None);
        let row = cell(&view, date).rows.iter().find(|r| r.text == desc).map(|r| r.tip.suffix).unwrap();
        (engine, row)
    }

    #[test]
    fn a_split_payment_continues_without_a_last_marker_where_the_new_record_starts() {
        let (mut db, _) = db_with(vec![monthly(20260115, "Rent", -900.0)]);
        let id = db.payments[0].id;
        crate::document::change_future_transactions(&mut db, id, 20261115, -950.0, "", budget_core::FLAG_NONE).unwrap();
        assert_eq!(db.payments.len(), 2, "the payment was split");
        // The earlier record ends with October; the engine marks that as its last payment.
        assert_eq!(suffixes(&mut db, 20261015, "Rent"), (SUFFIX_LAST, SUFFIX_NONE));
        assert_eq!(suffixes(&mut db, 20261115, "Rent").1, SUFFIX_NONE);
        // A continuation that is disabled, or starts elsewhere, does not carry the payment on.
        db.payments[1].disabled = true;
        assert_eq!(suffixes(&mut db, 20261015, "Rent").1, SUFFIX_LAST);
        db.payments[1].disabled = false;
        db.payments[1].category = Some("Other".into());
        assert_eq!(suffixes(&mut db, 20261015, "Rent").1, SUFFIX_LAST);
    }

    #[test]
    fn a_payment_that_really_ends_keeps_its_last_marker() {
        let mut ending = monthly(20260115, "Loan", -300.0);
        ending.set_end_mode(budget_core::END_MODE_N_PAYS);
        ending.n_pays = 10;
        // Same name, but starting a month after the next date the loan would have had.
        let later = monthly(20261215, "Loan", -300.0);
        let (mut db, _) = db_with(vec![ending, later]);
        assert_eq!(suffixes(&mut db, 20261015, "Loan"), (SUFFIX_LAST, SUFFIX_LAST));
    }

    #[test]
    fn a_continued_occurrence_moved_off_a_weekend_shows_as_adjusted_instead_of_last() {
        // The 17th of October 2026 is a Saturday: a business-days payment is paid on Monday the 19th.
        let mut p = monthly(20260117, "Insurance", -80.0);
        p.set_no_weekends(true);
        let (mut db, _) = db_with(vec![p]);
        let id = db.payments[0].id;
        crate::document::change_future_transactions(&mut db, id, 20261117, -85.0, "", budget_core::FLAG_NONE).unwrap();
        assert_eq!(suffixes(&mut db, 20261019, "Insurance"), (SUFFIX_LAST, SUFFIX_ADJUSTED));
    }

    #[test]
    fn total_income_row_shows_the_difference_from_plan() {
        let mut salary = payment(20261015, "Salary", 3000.0);
        salary.set_custom_amount(20261015, 2500.0);
        let (mut db, cur) = db_with(vec![salary]);
        let view = month(&mut db, &cur, None);
        assert_eq!(view.brief[0].desc, "Total income");
        assert_eq!(view.brief[0].diff, Some(-500.0));
    }

    #[test]
    fn empty_budget_labels_no_day_as_min_max_or_low() {
        // Every day of an empty budget holds the same balance, so none is the lowest or highest day.
        let (mut db, cur) = db_with(Vec::new());
        let view = month(&mut db, &cur, None);
        let kinds: Vec<BalKind> = view.cells.iter().filter(|c| c.in_month).filter_map(|c| c.balance.as_ref().map(|b| b.kind)).collect();
        assert_eq!(kinds.len(), 31, "every day of October has a balance");
        assert!(kinds.iter().all(|k| *k == BalKind::Normal), "{kinds:?}");
    }

    #[test]
    fn disabled_payments_keep_their_highlight_colour() {
        let mut p = payment(20261015, "Parked", -50.0);
        p.disabled = true;
        p.set_default_highlighting(0xFFD700);
        let (mut db, cur) = db_with(vec![p]);
        let view = month(&mut db, &cur, None);
        let row = &cell(&view, 20261015).rows[0];
        assert!(row.disabled);
        assert_eq!(row.hilite, Some(rgb(0xFFD700)));
    }

    #[test]
    fn adjusted_balance_exposes_the_adjustment_amount() {
        let (mut db, cur) = db_with(vec![payment(20261015, "Groceries", -12.0)]);
        let computed = db.get_day(&Cal::from_int(20261015)).unwrap().closing_balance;
        crate::document::set_balance(&mut db, 20261015, Some(computed + 250.0)).unwrap();
        let view = month(&mut db, &cur, None);
        let b = cell(&view, 20261015).balance.as_ref().unwrap();
        assert!(b.adjusted);
        assert!((b.adjustment - 250.0).abs() < 1e-9);
        assert!((b.value - b.adjustment - computed).abs() < 1e-9);
        assert_eq!(b.strip_text(false), "238\u{2032}(250)");
        let small = Balance { text: "1,000".into(), value: 1000.0, kind: BalKind::Normal, adjusted: true, adjustment: 0.99 };
        assert_eq!(small.strip_text(false), "1,000\u{2032}");
        assert_eq!(Balance { adjustment: -1.0, ..small }.strip_text(true), "1,000\u{2032}(\u{2212}1.00)");
        assert_eq!(cell(&view, 20261016).balance.as_ref().unwrap().strip_text(false), "238");
    }

    #[test]
    fn deleted_occurrences_are_listed_as_restorable_and_restore_brings_them_back() {
        let (mut db, cur) = db_with(vec![payment(20261015, "Groceries", -12.0)]);
        let id = db.payments[0].id;
        crate::document::delete(&mut db, id, Some(20261015)).unwrap();
        let view = month(&mut db, &cur, None);
        let c = cell(&view, 20261015);
        assert!(c.rows.is_empty());
        assert_eq!(c.restorable, vec![(id, "Groceries".to_string())]);
        crate::document::restore(&mut db, id, 20261015).unwrap();
        let view = month(&mut db, &cur, None);
        let c = cell(&view, 20261015);
        assert_eq!(c.rows.len(), 1);
        assert_eq!(c.rows[0].value, -12.0);
        assert!(c.restorable.is_empty());
        assert!(db.payments[0].opt.is_empty());
    }
}

#[cfg(test)]
mod money_tests {
    use super::{balance_display, balance_kind, is_negative, money, BalKind};
    #[test]
    fn balance_strip_uses_nudge_and_truncate() {
        let s = |v: f64| money(balance_display(v, false), false);
        assert_eq!(s(10.96), "11");
        assert_eq!(s(10.94), "10");
        assert_eq!(s(10.6), "10");
        assert_eq!(s(-10.96), "\u{2212}11");
        assert_eq!(s(1234.96), "1,235");
        assert_eq!(s(-0.4), "0");
        assert_eq!(balance_display(10.96, true), 10.96);
    }
    #[test]
    fn negative_colour_follows_the_displayed_sign() {
        // A value that prints without a sign is never coloured negative; one that prints "−1" always is.
        assert!(!is_negative(-2.7e-17, true));
        assert!(!is_negative(-0.004, true));
        assert!(is_negative(-0.01, true), "-0.01 prints as −0.01");
        assert!(is_negative(-0.02, true));
        assert!(is_negative(-0.7, false), "-0.7 prints as −1");
        assert!(!is_negative(-0.4, false), "-0.4 prints as 0");
        assert_eq!(money(-0.4, false), "0");
        assert!(is_negative(-1.2, false));
        assert!(!is_negative(5.0, false));
    }
    #[test]
    fn balance_kind_precedence() {
        assert_eq!(balance_kind(50.0, 50.0, 500.0, 100.0), BalKind::Low);
        assert_eq!(balance_kind(99.97, 50.0, 500.0, 100.0), BalKind::Normal);
        assert_eq!(balance_kind(99.94, 50.0, 500.0, 100.0), BalKind::Low);
        assert_eq!(balance_kind(500.0, 50.0, 500.0, 100.0), BalKind::Max);
        // A flat month has no lowest, highest or low day, whatever the threshold.
        assert_eq!(balance_kind(50.0, 50.0, 50.0, 100.0), BalKind::Normal);
        assert_eq!(balance_kind(0.0, 0.0, 0.0, 100.0), BalKind::Normal);
    }
    #[test]
    fn tiny_negatives_and_pct_have_no_minus() {
        assert_eq!(money(-1e-13, true), "0.00");
        assert_eq!(money(-0.004, true), "0.00");
        assert_eq!(super::pct_text(-0.3), "0");
        assert_eq!(super::pct_text(-12.4), "\u{2212}12");
    }
    #[test]
    fn whole_dollars_round_instead_of_truncate() {
        assert_eq!(money(1234.99, false), "1,235");
        assert_eq!(money(0.99, false), "1");
        assert_eq!(money(-0.4, false), "0");
        assert_eq!(money(-1234.5, true), "\u{2212}1,234.50");
        assert_eq!(money(f64::INFINITY, true), "\u{221E}");
    }
}
