//! Month view-model: everything the widgets draw, computed once per (month, filter, db change).

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::numfmt::{format_frac0, format_frac2};
use budget_core::{stats, BudgetDb, Cal, Field, FLAG_IMPORTANT, FLAG_PAID};
use eframe::egui::Color32;
use egui_phosphor::regular as ph;

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

/// The original's `isNegative(x, decimals)`: `x < -0.01` with decimals, else `(long)x < 0`.
pub fn is_negative(v: f64, decimals: bool) -> bool {
    if decimals { v < -0.01 } else { (v as i64) < 0 }
}

/// Balance as shown: exact with decimals, else nudged by `0.05f` and truncated toward zero.
pub fn balance_display(v: f64, decimals: bool) -> f64 {
    if decimals { return v; }
    let nudge = 0.05f32 as f64;
    if v > 0.0 { ((v + nudge) as i64) as f64 } else { ((v - nudge) as i64) as f64 }
}

/// Day-balance classification in the original's precedence: low, then min, then max.
pub fn balance_kind(closing: f64, min: f64, max: f64, threshold: f64) -> BalKind {
    if closing + 0.05 < threshold && min != max { BalKind::Low }
    else if closing == min { BalKind::Min }
    else if closing == max { BalKind::Max }
    else { BalKind::Normal }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BalKind { Normal, Max, Min, Low }

pub struct Tip {
    pub description: String,
    pub category: Option<String>,
    pub amount: f64,
    pub planned: f64,
    pub balance_after: f64,
    pub note: Option<String>,
    pub suffix: char,
}

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
}

pub struct Balance { pub text: String, pub value: f64, pub kind: BalKind, pub adjusted: bool }

pub struct CellData {
    pub day: i32,
    pub date: i32,
    pub in_month: bool,
    pub is_today: bool,
    pub weekend: bool,
    pub rows: Vec<TranRow>,
    pub balance: Option<Balance>,
    pub note: Option<String>,
    pub restorable: bool,
}

pub struct QuickStats { pub opening: f64, pub closing: f64, pub min: f64, pub max: f64, pub avg: f64, pub income: f64, pub expenses: f64 }

pub struct BriefRow { pub desc: String, pub amount: f64, pub pct: f64, pub diff: Option<f64>, pub total: bool, pub filter: String }

pub struct MonthView {
    pub key: (i32, String, u64),
    pub cells: Vec<CellData>,
    pub quick: QuickStats,
    pub brief: Vec<BriefRow>,
    pub shown: usize,
    pub total: usize,
    pub below_min: bool,
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

pub fn build(db: &mut BudgetDb, cfg: &BudgetConfig, cur: &Cal, today: i32, filter: Option<&Filter>, key: (i32, String, u64)) -> MonthView {
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
    let (mut shown, mut total) = (0, 0);
    let mut cells = Vec::new();
    for i in 0..(total_cells + trail) {
        let mut cal = *cur;
        cal.add(Field::Day, i - lead);
        let in_month = i >= lead && i < total_cells;
        let date = cal.get();
        let mut cell = CellData { day: cal.day(), date, in_month, is_today: date == today, weekend: cal.is_weekend(), rows: Vec::new(), balance: None, note: None, restorable: false };
        if in_month {
            if let Some(day) = db.get_day(&cal) {
                let trans: Vec<(usize, f64, f64, char)> = day.transactions.iter().map(|t| (t.pmt, t.amount, t.balance, t.suffix)).collect();
                let closing = day.closing_balance;
                let adjusted = day.closing_balance_adjusted;
                cell.restorable = !day.restorable.is_empty();
                cell.note = db.day_note(&cal);
                for (pi, amount, balance, suffix) in trans {
                    let p = &db.payments[pi];
                    total += 1;
                    if let Some(f) = filter { if !f.is_match(p, amount, date) { continue; } }
                    shown += 1;
                    let flag = p.get_flag(date);
                    let hi = p.get_hilighting(date);
                    let hi = if hi == -1 { p.default_hilighting() } else { hi };
                    let note = p.get_note(date);
                    // The suffix marker is drawn as a coloured dot by the calendar, not as text.
                    let amt = money(amount, decimals);
                    cell.rows.push(TranRow {
                        payment_id: p.id,
                        date,
                        icon: icon_for(p.image_path.as_deref(), &p.description, p.category.as_deref()),
                        text: p.short_description().to_string(),
                        amount: amt,
                        value: amount,
                        flag: if flag == FLAG_PAID || flag == FLAG_IMPORTANT { flag } else { '\0' },
                        hilite: (hi != -1 && !p.disabled).then(|| rgb(hi as u32)),
                        disabled: p.disabled,
                        filter: format!("desc:{}", esc(p.short_description())),
                        tip: Tip { description: p.description.clone(), category: p.category.clone(), amount, planned: p.amount, balance_after: balance, note, suffix },
                    });
                }
                let kind = balance_kind(closing, min, max, cfg.min_balance_4_max_saving);
                cell.balance = Some(Balance { text: money(balance_display(closing, decimals), decimals), value: closing, kind, adjusted });
            }
        }
        cells.push(cell);
    }
    let st = stats::calc_statistics(db, cur, 1, false, filter);
    // The summary tiles must reconcile with the opening/closing balances, which are never filtered,
    // so they always use unfiltered totals; the filter only narrows the calendar and the brief summary.
    let st_all = filter.map(|_| stats::calc_statistics(db, cur, 1, false, None));
    let totals = st_all.as_ref().unwrap_or(&st);
    let mut last = *cur;
    last.set_day_last();
    let opening = db.get_day(cur).map_or(0.0, |d| d.opening_balance);
    let closing = db.get_day(&last).map_or(0.0, |d| d.closing_balance);
    let avg = if closings.is_empty() { 0.0 } else { closings.iter().sum::<f64>() / closings.len() as f64 };
    let quick = QuickStats { opening, closing, min, max, avg, income: totals.total_income, expenses: totals.total_expense };
    // brief summary: rows >= 5% of total income, the rest as "Other…"; income first, then disposable income
    let pct = |a: f64| if st.total_income != 0.0 { a / st.total_income * 100.0 } else { 0.0 };
    let threshold = st.total_income.abs() * 0.05;
    let mut brief = vec![BriefRow { desc: "Total income".into(), amount: st.total_income, pct: pct(st.total_income), diff: None, total: true, filter: ">0".into() }];
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
    MonthView { key, cells, quick, brief, shown, total, below_min: min + 0.05 < cfg.min_balance_4_max_saving && min != max }
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
    fn negative_rule_matches_original() {
        assert!(!is_negative(-2.7e-17, true));
        assert!(!is_negative(-0.01, true));
        assert!(is_negative(-0.02, true));
        assert!(!is_negative(-0.7, false));
        assert!(is_negative(-1.2, false));
        assert!(!is_negative(5.0, false));
    }
    #[test]
    fn balance_kind_precedence() {
        assert_eq!(balance_kind(50.0, 50.0, 500.0, 100.0), BalKind::Low);
        assert_eq!(balance_kind(99.97, 50.0, 500.0, 100.0), BalKind::Normal);
        assert_eq!(balance_kind(99.94, 50.0, 500.0, 100.0), BalKind::Low);
        assert_eq!(balance_kind(500.0, 50.0, 500.0, 100.0), BalKind::Max);
        assert_eq!(balance_kind(50.0, 50.0, 50.0, 100.0), BalKind::Min);
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
