//! Statistics engine: `calcStatistics`, `calculate_stats_range`, `addStatInfo`, `transferSubAmount`
//! (analysis/05-calendar-view-and-stats.md §12).

use crate::date::{Cal, Field};
use crate::db::BudgetDb;
use crate::filter::Filter;
use crate::model::amount_order;
use crate::numfmt::{drop_dust, snap_to_cent};
use std::cmp::Ordering;

pub const UNCATEGORIZED_INCOME: &str = "(Uncategorized-Income)";
pub const UNCATEGORIZED_EXPENSES: &str = "(Uncategorized-Expenses)";
pub const UNACCOUNTED: &str = "(Unaccounted)";
pub const INDENT: &str = "   ";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Amount {
    pub key: String,
    pub amount: f64,
    pub planned_amount: f64,
    /// Payment index for leaf rows; `None` for categories.
    pub pmt: Option<usize>,
    /// Insertion-ordered sub rows (category → payments).
    pub sub_amounts: Vec<Amount>,
}

impl Amount {
    fn new(key: &str) -> Amount { Amount { key: key.to_string(), ..Default::default() } }
    pub fn is_aggregate(&self) -> bool { !self.sub_amounts.is_empty() }
    /// Find or add the sub row for `description`. Sub keys are stored with the leading indent, so
    /// the lookup matches through it instead of formatting an indented copy per occurrence.
    fn sub_mut(&mut self, description: &str, pmt: Option<usize>) -> &mut Amount {
        if let Some(i) = self.sub_amounts.iter().position(|a| a.key.strip_prefix(INDENT) == Some(description)) { return &mut self.sub_amounts[i]; }
        let mut a = Amount::new(&format!("{INDENT}{description}"));
        a.pmt = pmt;
        self.sub_amounts.push(a);
        self.sub_amounts.last_mut().unwrap()
    }
}

/// One rendered row of `incomeVec` / `expenseVec`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub amount: f64,
    pub planned_amount: f64,
    pub pmt: Option<usize>,
    /// `true` for a sub row (its key already carries the 3-space indent).
    pub is_sub: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub start: Cal,
    pub end: Cal,
    pub full_stats: bool,
    /// Top-level entries in insertion order (`statsMap`).
    pub top: Vec<Amount>,
    pub income: Vec<Row>,
    pub expense: Vec<Row>,
    pub total_income: f64,
    pub total_expense: f64,
    pub total_planned_income: f64,
    pub total_planned_expense: f64,
    /// What balance checkpoints changed in the range, net. Not a transaction: it counts in neither
    /// total, and has its own "Balance corrections" line in the summary and the reports.
    pub corrections: f64,
    /// The amount the budget started with, when its opening checkpoint is in the range.
    pub starting_balance: f64,
    /// Sum of the magnitudes of every amount added: the rounding dust of any sum or difference of
    /// them is bounded by a few ulps of this.
    pub abs_sum: f64,
}

impl Stats {
    pub fn duration_months(&self) -> i32 { Cal::months_diff(&self.start, &self.end) }

    /// Disposable income: income plus (negative) expenses, to the cent. Income and expenses are sums of
    /// different categories, so when they cancel their difference is dust (1e-15), not 0.
    pub fn net(&self) -> f64 { snap_to_cent(self.total_income + self.total_expense) }

    /// How far disposable income is from plan, to the cent.
    pub fn net_vs_plan(&self) -> f64 {
        snap_to_cent(self.net() - snap_to_cent(self.total_planned_income + self.total_planned_expense))
    }

    fn top_mut(&mut self, key: &str) -> &mut Amount {
        if let Some(i) = self.top.iter().position(|a| a.key == key) { return &mut self.top[i]; }
        self.top.push(Amount::new(key));
        self.top.last_mut().unwrap()
    }

    /// `addStatInfo(stats, category, description, amount, pmt, planned)`.
    fn add_stat_info(&mut self, category: Option<&str>, description: &str, amount: f64, pmt: Option<(usize, f64)>, planned: bool) {
        let pmt_amount = pmt.map_or(0.0, |(_, a)| a);
        let category = match category {
            None if self.full_stats => Some(if (if planned { pmt_amount } else { amount }) < 0.0 { UNCATEGORIZED_EXPENSES } else { UNCATEGORIZED_INCOME }),
            c => c,
        };
        let pmt_idx = pmt.map(|(i, _)| i);
        self.abs_sum += amount.abs() + pmt_amount.abs();
        match category {
            Some(cat) => {
                let top = self.top_mut(cat);
                top.amount += amount;
                if planned { top.planned_amount += pmt_amount; }
                let sub = top.sub_mut(description, pmt_idx);
                sub.amount += amount;
                if planned { sub.planned_amount += pmt_amount; }
            }
            None => {
                let row = self.top_mut(description);
                if row.pmt.is_none() { row.pmt = pmt_idx; }
                row.amount += amount;
                if planned { row.planned_amount += pmt_amount; }
            }
        }
    }
}

/// `stat_comparator`.
fn stat_cmp(a: &Amount, b: &Amount) -> Ordering { amount_order(a.amount, b.amount) }

/// Whether a row belongs with the expenses: by its actual amount, or, with none, by its planned amount.
fn is_expense_row(a: &Amount) -> bool { a.amount < 0.0 || (a.amount == 0.0 && a.planned_amount < 0.0) }

/// Move the subs of `from` that `moves` accepts into `to`; both totals follow them.
fn transfer_sub_amount(from: &mut Amount, to: &mut Amount, moves: fn(&Amount) -> bool) {
    let mut i = 0;
    while i < from.sub_amounts.len() {
        if moves(&from.sub_amounts[i]) {
            let s = from.sub_amounts.remove(i);
            from.amount -= s.amount;
            from.planned_amount -= s.planned_amount;
            to.amount += s.amount;
            to.planned_amount += s.planned_amount;
            // The other side may already hold the description (kept at 0 for its planned amount when the
            // range nets to nothing): one row, not two.
            match to.sub_amounts.iter_mut().find(|t| t.key == s.key) {
                Some(t) => { t.amount += s.amount; t.planned_amount += s.planned_amount; if t.pmt.is_none() { t.pmt = s.pmt; } }
                None => to.sub_amounts.push(s),
            }
        } else { i += 1; }
    }
}

/// The side of a reconciled description that gave up its amount: its row is dropped when it has no
/// planned amount either, else kept at zero so the planned amount still shows.
fn settle_row(top: &mut Amount, key: &str) {
    if let Some(i) = top.sub_amounts.iter().position(|s| s.key == key) {
        if top.sub_amounts[i].planned_amount == 0.0 { top.sub_amounts.remove(i); } else { top.sub_amounts[i].amount = 0.0; }
    }
}

/// `calcStatistics(cal, nMonths, full)`: from the 1st of `cal`'s month over `n_months` months.
pub fn calc_statistics(db: &mut BudgetDb, cal: &Cal, n_months: i32, full: bool, filter: Option<&Filter>) -> Stats {
    let mut start = *cal;
    start.set_day_one();
    let mut end = start;
    end.add(Field::Month, n_months);
    end.add(Field::Day, -1);
    calc_statistics_exact(db, &start, &end, full, filter)
}

/// `calcStatisticsExact(start, end, full)` + `calculate_stats_range`.
pub fn calc_statistics_exact(db: &mut BudgetDb, start: &Cal, end: &Cal, full: bool, filter: Option<&Filter>) -> Stats {
    let mut st = Stats { start: *start, end: *end, full_stats: full, ..Default::default() };
    let opening_date = db.starting_balance_date();
    st.top_mut(UNCATEGORIZED_INCOME);
    st.top_mut(UNCATEGORIZED_EXPENSES);

    // planned pass: each scheduled occurrence is tested on its own day's note and flag, as the calendar does
    // (the sorted indices are copied into one buffer reused across days; the day's borrow of the db ends
    // at that copy, leaving the payments free to read for the rest of the pass)
    let mut regulars: Vec<usize> = Vec::new();
    let mut c = *start;
    while c.le(end) {
        let date = c.get();
        if let Some(day) = db.get_day(&c) {
            // Sorted so the order of equal summary rows does not depend on how the schedule was built.
            regulars.clear();
            regulars.extend_from_slice(&day.regulars);
            regulars.sort_unstable();
            for pi in regulars.iter().copied() {
                let p = &db.payments[pi];
                if let Some(f) = filter { if !f.is_match(p, p.amount, date) { continue; } }
                if p.disabled { continue; }
                st.add_stat_info(p.category.as_deref(), &p.description, 0.0, Some((pi, p.amount)), true);
                if p.amount < 0.0 { st.total_planned_expense += p.amount; } else { st.total_planned_income += p.amount; }
            }
        }
        c.add(Field::Day, 1);
    }
    // actual pass
    let mut c = *start;
    while c.le(end) {
        if let Some(day) = db.get_day(&c) {
            let trans: Vec<(usize, f64)> = day.transactions.iter().map(|t| (t.pmt, t.amount)).collect();
            let adj = day.adjustment_diff;
            let date = c.get();
            for (pi, amount) in trans {
                let p = &db.payments[pi];
                if let Some(f) = filter { if !f.is_match(p, amount, date) { continue; } }
                if p.disabled { continue; }
                st.add_stat_info(p.category.as_deref(), &p.description, amount, Some((pi, p.amount)), false);
            }
            // A balance adjustment is not a transaction: a search that narrows the statistics leaves it out.
            if adj != 0.0 && filter.is_none_or(|f| f.is_empty()) {
                if opening_date == Some(date) { st.starting_balance += adj; } else { st.corrections = snap_to_cent(st.corrections + adj); }
            }
        }
        c.add(Field::Day, 1);
    }

    // reconcile descriptions present in both uncategorized groups: the net shows on the side it falls
    // on and the other side gives up its amount. Each row keeps its own planned amount, so a planned
    // expense whose occurrence turned positive still has a row in the expense list.
    let ii = st.top.iter().position(|a| a.key == UNCATEGORIZED_INCOME).unwrap();
    let ei = st.top.iter().position(|a| a.key == UNCATEGORIZED_EXPENSES).unwrap();
    let both: Vec<String> = st.top[ii].sub_amounts.iter()
        .filter(|s| st.top[ei].sub_amounts.iter().any(|e| e.key == s.key))
        .map(|s| s.key.clone()).collect();
    for key in both {
        let inc_amt = st.top[ii].sub_amounts.iter().find(|s| s.key == key).unwrap().amount;
        let exp_amt = st.top[ei].sub_amounts.iter().find(|s| s.key == key).unwrap().amount;
        // A description whose income and expense cancel must net to exactly 0, not to +-1e-15: the sign picks the side.
        let net = drop_dust(inc_amt + exp_amt, st.abs_sum);
        if net > 0.0 {
            st.top[ei].amount -= exp_amt;
            st.top[ii].amount += exp_amt;
            st.top[ii].sub_amounts.iter_mut().find(|s| s.key == key).unwrap().amount = net;
            settle_row(&mut st.top[ei], &key);
        } else {
            st.top[ii].amount -= inc_amt;
            st.top[ei].amount += inc_amt;
            st.top[ei].sub_amounts.iter_mut().find(|s| s.key == key).unwrap().amount = net;
            settle_row(&mut st.top[ii], &key);
        }
    }
    {
        let (lo, hi) = (ii.min(ei), ii.max(ei));
        let (a, b) = st.top.split_at_mut(hi);
        let (inc, exp) = if ii < ei { (&mut a[lo], &mut b[0]) } else { (&mut b[0], &mut a[lo]) };
        transfer_sub_amount(inc, exp, is_expense_row);
        transfer_sub_amount(exp, inc, |s| !is_expense_row(s));
    }
    st.top.retain(|a| !(matches!(a.key.as_str(), UNCATEGORIZED_INCOME | UNCATEGORIZED_EXPENSES) && a.sub_amounts.is_empty()));
    // Moving amounts between the groups subtracts them in a different order than they were added, which
    // leaves ~1e-15 in a category that is empty in effect: that would sort it under income and make the
    // reports see a nonzero amount where every leaf is zero.
    let scale = st.abs_sum;
    for a in &mut st.top {
        a.amount = drop_dust(a.amount, scale);
        a.planned_amount = drop_dust(a.planned_amount, scale);
        for s in &mut a.sub_amounts {
            s.amount = drop_dust(s.amount, scale);
            s.planned_amount = drop_dust(s.planned_amount, scale);
        }
    }

    for a in &st.top {
        if a.amount < 0.0 { st.total_expense += a.amount; } else { st.total_income += a.amount; }
    }
    let mut sorted = st.top.clone();
    sorted.sort_by(stat_cmp);
    for a in sorted {
        let vec = if is_expense_row(&a) { &mut st.expense } else { &mut st.income };
        vec.push(Row { key: a.key.clone(), amount: a.amount, planned_amount: a.planned_amount, pmt: a.pmt, is_sub: false });
        let mut subs = a.sub_amounts.clone();
        subs.sort_by(stat_cmp);
        for s in subs {
            vec.push(Row { key: s.key, amount: s.amount, planned_amount: s.planned_amount, pmt: s.pmt, is_sub: true });
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bgt;
    use std::path::Path;

    fn sample() -> BudgetDb {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference/sample.bgt");
        let text = bgt::latin1_decode(&std::fs::read(p).unwrap());
        let l = bgt::load_str(&text, 20211201, None).unwrap_or_else(|l| panic!("{}", l.log));
        BudgetDb::from_loaded(l, &Cal::from_int(20211201), 12, false)
    }

    /// December 2021 of sample.bgt as shown in the vendor screenshot's Brief Summary.
    #[test]
    fn december_2021_brief_summary() {
        let mut db = sample();
        let st = calc_statistics(&mut db, &Cal::from_int(20211201), 1, true, None);
        assert_eq!(st.total_income, 3000.0);
        assert_eq!(st.total_expense, -3030.0);
        assert_eq!(st.total_planned_income, 3000.0);
        assert_eq!(st.total_planned_expense, -3030.0);
        let top: Vec<(&str, f64)> = st.expense.iter().filter(|r| !r.is_sub).map(|r| (r.key.as_str(), r.amount)).collect();
        assert_eq!(top, vec![
            ("(Uncategorized-Expenses)", -2300.0), // Rent, Savings, Groceries, Visa Pmt, TV
            ("Auto", -600.0),                      // Car Loan 400, Gasoline 4x30, Car Insurance 80
            ("Utilities", -130.0),                 // Hydro 100, Mobile 30
        ]);
        let inc: Vec<(&str, f64)> = st.income.iter().map(|r| (r.key.as_str(), r.amount)).collect();
        assert_eq!(inc, vec![("(Uncategorized-Income)", 3000.0), ("   Salary", 3000.0)]);
        let uncat: Vec<(&str, f64)> = st.expense.iter().skip(1).take(5).map(|r| (r.key.as_str(), r.amount)).collect();
        assert_eq!(uncat, vec![("   Rent", -1100.0), ("   Savings", -500.0), ("   Groceries", -400.0), ("   Visa Pmt", -250.0), ("   TV", -50.0)]);
        // brief (non-full) stats put uncategorised payments at top level under their own name
        let brief = calc_statistics(&mut db, &Cal::from_int(20211201), 1, false, None);
        assert!(brief.expense.iter().any(|r| r.key == "Rent" && !r.is_sub && r.amount == -1100.0));
        assert_eq!(brief.total_expense, -3030.0);
    }

    /// A payment on the 1st of each month from `start`, `n` occurrences, no category when `cat` is None.
    fn scheduled(desc: &str, cat: Option<&str>, amount: f64, start: i32, n: i32) -> crate::model::Payment {
        let mut p = crate::model::Payment::new(start);
        p.description = desc.into();
        p.category = cat.map(str::to_string);
        p.amount = amount;
        p.n_pays = n;
        p
    }

    fn db_of(payments: Vec<crate::model::Payment>) -> BudgetDb {
        let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
        for p in payments { db.add(p); }
        db
    }

    /// The leaf rows (those carrying payments) must add up to the totals, planned and actual.
    fn assert_rows_reconcile(st: &Stats) {
        let rows: Vec<&Row> = st.income.iter().chain(st.expense.iter()).collect();
        let (mut amount, mut planned) = (0.0, 0.0);
        for (i, r) in rows.iter().enumerate() {
            if r.is_sub || rows.get(i + 1).is_none_or(|next| !next.is_sub) {
                amount += r.amount;
                planned += r.planned_amount;
            }
        }
        assert!((amount - (st.total_income + st.total_expense)).abs() < 1e-9, "rows {amount} vs totals");
        let total_planned = st.total_planned_income + st.total_planned_expense;
        assert!((planned - total_planned).abs() < 1e-9, "planned rows {planned} vs totals {total_planned}");
    }

    #[test]
    fn rows_reconcile_with_the_totals_on_the_sample_budget() {
        let mut db = sample();
        for (start, full) in [(20211201, true), (20210101, true), (20211201, false)] {
            let st = calc_statistics(&mut db, &Cal::from_int(start), 12, full, None);
            assert_rows_reconcile(&st);
        }
    }

    #[test]
    fn planned_pass_tests_each_occurrence_against_its_own_note_and_flag() {
        // Oct 1 is noted "shell" and Paid; Nov and Dec are neither. The filter sees each date's own.
        let mut fuel = scheduled("Fuel", Some("Auto"), -30.0, 20261001, 3);
        fuel.set_note(20261001, Some("shell"));
        fuel.set_flag(20261001, crate::model::FLAG_PAID);
        let mut db = db_of(vec![fuel]);
        let (start, end) = (Cal::from_int(20261001), Cal::from_int(20261231));
        let planned = |db: &mut BudgetDb, text: &str| {
            let f = Filter::new(text, 20261027).unwrap();
            calc_statistics_exact(db, &start, &end, true, Some(&f)).total_planned_expense
        };
        assert_eq!(planned(&mut db, "note:shell"), -30.0);
        assert_eq!(planned(&mut db, "note:shell cat:auto"), -30.0);
        assert_eq!(planned(&mut db, "flag:paid"), -30.0);
        assert_eq!(planned(&mut db, "flag:unpaid"), -60.0);
        assert_eq!(planned(&mut db, "note:rent"), 0.0);
        assert_eq!(calc_statistics_exact(&mut db, &start, &end, true, Some(&Filter::new("note:shell", 20261027).unwrap())).total_expense, -30.0);
    }

    #[test]
    fn a_search_leaves_balance_corrections_out_of_the_statistics() {
        // The Oct 15 balance adjustment is not a transaction, so a search that narrows the list
        // drops it instead of adding it to the matches.
        let mut db = db_of(vec![scheduled("Rent", Some("Housing"), -1000.0, 20261001, 1)]);
        db.update_adjustment(&Cal::from_int(20261015), -3020.0);
        let (start, end) = (Cal::from_int(20261001), Cal::from_int(20261031));
        let f = Filter::new("cat:zzz", 20261027).unwrap();
        let st = calc_statistics_exact(&mut db, &start, &end, true, Some(&f));
        assert_eq!((st.total_expense, st.corrections), (0.0, 0.0));
        // without a search it is shown on its own, and is still not spending
        let st = calc_statistics_exact(&mut db, &start, &end, true, None);
        assert!(st.income.iter().chain(st.expense.iter()).all(|r| !r.key.contains(UNACCOUNTED)), "{:?}", st.expense);
        assert_eq!((st.total_expense, st.corrections), (-1000.0, -2020.0));
    }

    #[test]
    fn a_correction_is_neither_money_in_nor_money_out() {
        // Rent of 1,000 on the 1st of each month, reconciled up in October and down in November.
        let mut db = db_of(vec![scheduled("Rent", None, -1000.0, 20260901, 3)]);
        db.update_adjustment(&Cal::from_int(20261015), 500.0);
        db.update_adjustment(&Cal::from_int(20261115), -700.0);
        for (full, month, correction) in [(true, 20261001, 2500.0), (false, 20261001, 2500.0), (true, 20261101, -200.0)] {
            let st = calc_statistics(&mut db, &Cal::from_int(month), 1, full, None);
            assert_eq!((st.total_income, st.total_expense, st.corrections, st.starting_balance), (0.0, -1000.0, correction, 0.0), "{month}");
            assert_eq!(st.net(), -1000.0, "disposable income is what was earned and spent");
            assert_rows_reconcile(&st);
        }
        let st = calc_statistics(&mut db, &Cal::from_int(20261001), 2, true, None);
        assert_eq!(st.corrections, 2300.0, "corrections net over a range");
    }

    #[test]
    fn the_first_checkpoint_with_no_earlier_history_is_a_starting_balance_not_income() {
        // Nothing happens before the 5th: the bank balance that day is what the household started with.
        let mut db = db_of(vec![scheduled("Salary", None, 3000.0, 20261015, 1), scheduled("Rent", None, -1000.0, 20261005, 1)]);
        db.update_adjustment(&Cal::from_int(20261005), 5000.0);
        let oct = Cal::from_int(20261001);
        let st = calc_statistics(&mut db, &oct, 1, true, None);
        assert_eq!((st.total_income, st.total_expense, st.corrections, st.starting_balance), (3000.0, -1000.0, 0.0, 6000.0));
        assert_eq!(st.net(), 2000.0);
        // A later checkpoint corrects it.
        db.update_adjustment(&Cal::from_int(20261020), 6500.0);
        let st = calc_statistics(&mut db, &oct, 1, true, None);
        assert_eq!((st.corrections, st.starting_balance), (-1500.0, 6000.0));
        // A payment before it means the balance had a history: this is a correction after all.
        db.add(scheduled("Coffee", None, -5.0, 20261002, 1));
        let st = calc_statistics(&mut db, &oct, 1, true, None);
        assert_eq!(st.starting_balance, 0.0);
        assert_eq!(st.corrections, 4505.0);
        // A disabled payment leaves no history.
        let i = db.payments.len() - 1;
        db.payments[i].disabled = true;
        db.update_payment(i);
        assert_eq!(calc_statistics(&mut db, &oct, 1, true, None).starting_balance, 6000.0);
    }

    #[test]
    fn the_starting_balance_is_the_same_in_any_window() {
        let build = |unlimited: bool, view: i32| {
            let mut db = db_of(vec![scheduled("Rent", None, -1000.0, 20260610, 60)]);
            db.adjustments.set_custom_amount(20260605, 4000.0);
            db.adjustments.set_custom_amount(20260805, 3500.0);
            db.rebuild_db(&Cal::from_int(view), 12, unlimited);
            db
        };
        let june = Cal::from_int(20260601);
        for mut db in [build(true, 20261001), build(false, 20260601)] {
            let st = calc_statistics(&mut db, &june, 1, true, None);
            assert_eq!((st.total_income, st.starting_balance, st.corrections), (0.0, 4000.0, 0.0));
        }
        assert_eq!(build(false, 20261201).starting_balance_date(), Some(20260605));
    }

    #[test]
    fn a_planned_expense_that_turns_positive_keeps_its_planned_row() {
        // An uncategorised -30 expense whose only occurrence was changed to +30: the planned -30
        // used to vanish from the rows while Total Expenses still counted it.
        let mut db = db_of(vec![scheduled("Refund", None, -30.0, 20261001, 1)]);
        db.payments[0].set_custom_amount(20261001, 30.0);
        db.update_payment(0);
        let st = calc_statistics_exact(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261031), true, None);
        assert_eq!(st.total_planned_expense, -30.0);
        assert_rows_reconcile(&st);
        assert!(st.expense.iter().any(|r| r.key == "   Refund" && r.amount == 0.0 && r.planned_amount == -30.0), "{:?}", st.expense);
        assert!(st.income.iter().any(|r| r.key == "   Refund" && r.amount == 30.0 && r.planned_amount == 0.0), "{:?}", st.income);
    }

    /// Round 6: moving a description's net to the other group left ~1e-15 in the emptied category, which
    /// then counted as income (or as nonzero), though its leaves were all zero.
    fn two_expenses_that_turned_positive(first: f64, second: f64) -> BudgetDb {
        let mut db = db_of(vec![scheduled("Phone", None, -49.95, 20261001, 2), scheduled("Netflix", None, -19.99, 20261001, 2)]);
        db.payments[0].set_custom_amount(20261001, first);
        db.payments[1].set_custom_amount(20261001, second);
        db.update_payment(0);
        db.update_payment(1);
        db
    }

    #[test]
    fn a_category_emptied_by_the_reconcile_is_exactly_zero_and_stays_an_expense_category() {
        for (first, second) in [(1234.56, 29.99), (49.95, 19.99), (0.07, 0.1), (1e6 + 0.01, 3.3)] {
            let mut db = two_expenses_that_turned_positive(first, second);
            let st = calc_statistics_exact(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261130), true, None);
            let uncategorised = |rows: &[Row]| rows.iter().any(|r| r.key == UNCATEGORIZED_EXPENSES);
            assert!(uncategorised(&st.expense) && !uncategorised(&st.income), "{first} {second}: income {:?} expense {:?}", st.income, st.expense);
            for r in st.income.iter().chain(st.expense.iter()) {
                assert!(r.amount == 0.0 || r.amount.abs() > 1e-6, "residue {} in {}", r.amount, r.key);
                assert!(r.planned_amount == 0.0 || r.planned_amount.abs() > 1e-6, "planned residue {} in {}", r.planned_amount, r.key);
            }
            assert_rows_reconcile(&st);
        }
    }

    #[test]
    fn a_description_that_nets_to_nothing_over_the_range_has_one_row() {
        // +50 in October, -50 in November: the range nets to 0 and the description must list once.
        let mut db = db_of(vec![scheduled("Refund", None, 50.0, 20261001, 2)]);
        db.payments[0].set_custom_amount(20261101, -50.0);
        db.update_payment(0);
        let st = calc_statistics_exact(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261130), true, None);
        let rows = st.income.iter().chain(st.expense.iter()).filter(|r| r.is_sub && r.key == "   Refund").count();
        assert_eq!(rows, 1, "income {:?} expense {:?}", st.income, st.expense);
        assert_rows_reconcile(&st);
    }

    #[test]
    fn a_planned_expense_without_an_actual_stays_in_the_expense_list() {
        // Expenses whose only occurrence was deleted have actual 0 and planned -30: they must
        // list as expenses, not as income rows.
        let mut db = db_of(vec![scheduled("Car loan", Some("Auto"), -30.0, 20261001, 1), scheduled("Gym", None, -20.0, 20261001, 1)]);
        db.payments[0].set_no_transaction(20261001);
        db.payments[1].set_no_transaction(20261001);
        db.update_payment(0);
        db.update_payment(1);
        let st = calc_statistics_exact(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261031), true, None);
        assert_eq!(st.total_planned_expense, -50.0);
        assert_rows_reconcile(&st);
        assert!(st.expense.iter().any(|r| r.key == "Auto" && !r.is_sub && r.planned_amount == -30.0), "{:?}", st.expense);
        assert!(st.expense.iter().any(|r| r.key == "(Uncategorized-Expenses)" && r.planned_amount == -20.0), "{:?}", st.expense);
        assert!(st.income.is_empty(), "{:?}", st.income);
    }

    /// A household budget: a pay and a rent since January 2025, reconciled twice. `loan` adds a loan
    /// that stretches the loaded window back past the older checkpoint.
    fn reconciled(loan: bool, centre: i32, unlimited: bool) -> BudgetDb {
        let every_month = |desc: &str, amount: f64, start: i32| {
            let mut p = scheduled(desc, None, amount, start, 1);
            p.set_end_mode(crate::model::END_MODE_INDEFINITE);
            p
        };
        let mut payments = vec![every_month("Salary", 3000.0, 20250115), every_month("Rent", -1200.0, 20250101)];
        if loan {
            let mut car = every_month("Car", -400.0, 20260105);
            car.set_end_mode(crate::model::END_MODE_LOAN_PAID);
            car.loan_amount = 8000.0;
            payments.push(car);
        }
        let mut db = db_of(payments);
        db.adjustments.set_custom_amount(20250601, 9000.0);
        db.adjustments.set_custom_amount(20260620, 2500.0);
        db.rebuild_db(&Cal::from_int(centre), 12, unlimited);
        db
    }

    /// Moves one month at a time as the arrows do: the window is rebuilt only for a month it cannot show.
    fn page_to(db: &mut BudgetDb, from: i32, to: i32) {
        let (mut month, step) = (Cal::from_int(from), if to < from { -1 } else { 1 });
        while month.get() != to {
            month.add(Field::Month, step);
            if !db.is_month_available(&month) { db.rebuild_db(&month, 12, false); }
        }
    }

    /// Opening balance, closing balance, money in and money out of a month.
    fn figures(db: &mut BudgetDb, month: i32) -> (f64, f64, f64, f64) {
        let first = Cal::from_int(month);
        let mut last = first;
        last.set_day_last();
        let (opening, closing) = (db.get_day(&first).unwrap().opening_balance, db.get_day(&last).unwrap().closing_balance);
        let st = calc_statistics(db, &first, 1, true, None);
        (opening, closing, st.total_income, st.total_expense)
    }

    #[test]
    fn a_months_figures_do_not_depend_on_the_way_it_was_reached() {
        for loan in [false, true] {
            let mut complete = reconciled(loan, 20261001, true);
            for centre in [20261001, 20270301, 20280601, 20250701] {
                let mut month = Cal::from_int(20250301);
                while month.get() <= 20270301 {
                    let mut db = reconciled(loan, centre, false);
                    page_to(&mut db, centre, month.get());
                    assert_eq!(figures(&mut db, month.get()), figures(&mut complete, month.get()), "loan {loan}, from {centre} to {}", month.get());
                    month.add(Field::Month, 1);
                }
            }
        }
    }

    #[test]
    fn the_month_of_the_window_start_is_not_shown_built_from_zero() {
        // Viewing October, the window starts on 1 June because of the 20 June checkpoint.
        let mut db = reconciled(false, 20261001, false);
        assert_eq!(db.start_date.get(), 20260601);
        assert!(!db.is_month_available(&Cal::from_int(20260601)), "June's balances before the 20th are built from zero");
        assert!(db.is_month_available(&Cal::from_int(20260701)));
        assert!(db.is_month_available(&Cal::from_int(20261001)));
        page_to(&mut db, 20261001, 20260601);
        assert!(db.get_day(&Cal::from_int(20260601)).unwrap().opening_balance > 9000.0, "June opens with what May closed on");
    }

    #[test]
    fn filter_and_unaccounted() {
        let mut db = sample();
        let f = Filter::new("cat:auto", 20211201).unwrap();
        let st = calc_statistics(&mut db, &Cal::from_int(20211201), 1, true, Some(&f));
        assert_eq!(st.total_expense, -600.0);
        assert_eq!(st.total_income, 0.0);
        // a balance adjustment inside the range is a correction, not a row of its own
        db.update_adjustment(&Cal::from_int(20211215), 0.0);
        let st = calc_statistics(&mut db, &Cal::from_int(20211201), 1, true, None);
        assert!(st.income.iter().chain(st.expense.iter()).all(|r| !r.key.contains(UNACCOUNTED)));
        assert_ne!(st.corrections, 0.0);
    }
}
