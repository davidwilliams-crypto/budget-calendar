//! Month view-model: everything the widgets draw, computed once per (month, filter, db change).

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::numfmt::format_frac2;
use budget_core::{stats, BudgetDb, Cal, Field, FLAG_IMPORTANT, FLAG_PAID};
use eframe::egui::Color32;
use egui_phosphor::regular as ph;

use crate::theme::rgb;

/// `#,##0` or `#,##0.00`, with a typographic minus.
pub fn money(v: f64, decimals: bool) -> String {
    let s = format_frac2(v);
    let s = if decimals { s } else { s[..s.len() - 3].to_string() };
    s.replacen('-', "\u{2212}", 1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
                    let mut amt = money(amount, decimals);
                    if suffix != '\0' { amt.push(suffix); }
                    cell.rows.push(TranRow {
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
                let kind = if closing == max { BalKind::Max }
                    else if closing == min { BalKind::Min }
                    else if closing < cfg.min_balance_4_max_saving { BalKind::Low }
                    else { BalKind::Normal };
                cell.balance = Some(Balance { text: money(closing, decimals), value: closing, kind, adjusted });
            }
        }
        cells.push(cell);
    }
    let st = stats::calc_statistics(db, cur, 1, false, filter);
    let mut last = *cur;
    last.set_day_last();
    let opening = db.get_day(cur).map_or(0.0, |d| d.opening_balance);
    let closing = db.get_day(&last).map_or(0.0, |d| d.closing_balance);
    let avg = if closings.is_empty() { 0.0 } else { closings.iter().sum::<f64>() / closings.len() as f64 };
    let quick = QuickStats { opening, closing, min, max, avg, income: st.total_income, expenses: st.total_expense };
    // brief summary: rows >= 5% of total income, the rest as "Other…"; income first, then disposable income
    let pct = |a: f64| if st.total_income != 0.0 { a / st.total_income * 100.0 } else { 0.0 };
    let threshold = st.total_income.abs() * 0.05;
    let mut brief = vec![BriefRow { desc: "Total income".into(), amount: st.total_income, pct: 100.0, diff: None, total: true, filter: ">0".into() }];
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
    MonthView { key, cells, quick, brief, shown, total, below_min: min < cfg.min_balance_4_max_saving }
}
