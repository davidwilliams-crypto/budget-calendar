//! Bringing the schedule up to date after an edit.
//!
//! Every editing operation in `document` already reschedules the payment it changed in place
//! (`BudgetDb::update_payment`), and deleting a payment re-places all of them. A full rebuild
//! (`BudgetDb::rebuild_db`) after that gives the same transactions and balances unless it would
//! choose another date window, or the in-place update left equal transactions (whose running
//! balances depend on their order) or restorable payments in another order than a rebuild places
//! them (by payment index): only then is it run.
//!
//! A day's list of scheduled payments is left in the in-place order; `stats::calc_statistics`
//! sorts it before use, so the summary rows do not depend on how the schedule was built.

use budget_core::{BudgetDb, Cal, Field};

/// Rebuilds `db` around `cal` unless its in-place updates already equal a rebuild. Returns
/// whether it rebuilt.
pub fn after_edit(db: &mut BudgetDb, cal: &Cal, future_months: i32, unlimited_history: bool) -> bool {
    let current = (db.start_date.get(), db.end_date.get());
    let (start, end) = db.window(cal, future_months, unlimited_history);
    if (start.get(), end.get()) == current && in_rebuild_order(db) { return false; }
    db.rebuild_db(cal, future_months, unlimited_history);
    true
}

/// Whether every day lists its transactions and restorable payments in the order a rebuild
/// places them, which schedules the payments by index: transactions that sort equal (same amount
/// and enabled state) and restorable payments by ascending payment index. (This also brings the
/// balances up to date, which drawing the month needs next anyway.)
fn in_rebuild_order(db: &mut BudgetDb) -> bool {
    let disabled: Vec<bool> = db.payments.iter().map(|p| p.disabled).collect();
    let (mut day, end) = (db.start_date, db.end_date.get());
    while day.get() <= end {
        let Some(d) = db.get_day(&day) else { return false };
        // NaN compares equal to everything in the day sort, so its order is not reproducible.
        if d.transactions.iter().any(|t| t.amount.is_nan()) { return false; }
        let tied_out_of_order = d.transactions.windows(2).any(|w| {
            disabled[w[0].pmt] == disabled[w[1].pmt] && w[0].amount == w[1].amount && w[0].pmt > w[1].pmt
        });
        if tied_out_of_order || d.restorable.windows(2).any(|w| w[0] >= w[1]) { return false; }
        day.add(Field::Day, 1);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document;
    use budget_core::{Payment, END_MODE_DATE, END_MODE_INDEFINITE, END_MODE_LOAN_PAID, FLAG_NONE};

    type DaySnapshot = (i32, f64, f64, bool, f64, Vec<(usize, f64, f64, char)>, Vec<usize>, Vec<usize>);
    type MonthStats = Vec<(String, f64, f64, Option<usize>, bool)>;

    /// Everything a rebuild determines: the window, every day in it (the scheduled payments as a
    /// set, see the module comment), each month's brief and full statistics and the saved payments.
    fn snapshot(db: &mut BudgetDb) -> ((i32, i32), Vec<DaySnapshot>, Vec<MonthStats>, String) {
        let window = (db.start_date.get(), db.end_date.get());
        let mut days = Vec::new();
        let mut day = db.start_date;
        while day.get() <= window.1 {
            let d = db.get_day(&day).unwrap();
            let mut regulars = d.regulars.clone();
            regulars.sort_unstable();
            days.push((day.get(), d.opening_balance, d.closing_balance, d.closing_balance_adjusted, d.adjustment_diff,
                d.transactions.iter().map(|t| (t.pmt, t.amount, t.balance, t.suffix)).collect(), regulars, d.restorable.clone()));
            day.add(Field::Day, 1);
        }
        let mut months = Vec::new();
        let mut month = db.start_date;
        while month.get() <= window.1 {
            for full in [false, true] {
                let st = budget_core::stats::calc_statistics(db, &month, 1, full, None);
                months.push(st.income.iter().chain(&st.expense).map(|r| (r.key.clone(), r.amount, r.planned_amount, r.pmt, r.is_sub)).collect());
            }
            month.add(Field::Month, 1);
        }
        let text = db.to_text(None).unwrap();
        (window, days, months, text)
    }

    fn payment(description: &str, start: i32, amount: f64, freq: char) -> Payment {
        let mut p = Payment::new(start);
        p.description = description.into();
        p.amount = amount;
        p.freq_type = freq;
        p.set_end_mode(END_MODE_INDEFINITE);
        p
    }

    const CUR: i32 = 20261001;

    /// Payments with distinct amounts, so in-place updates keep the rebuild order.
    fn budget(ties: bool) -> BudgetDb {
        let cur = Cal::from_int(CUR);
        let mut db = BudgetDb::new(&cur, 12, false);
        db.today = 20261027;
        db.payments.push(payment("Salary", 20250115, 3200.0, 'm'));
        db.payments.push(payment("Rent", 20250101, -1450.0, 'm'));
        db.payments.push(payment("Groceries", 20250103, -85.25, 'w'));
        db.payments.push(payment("Gym", 20250110, if ties { -85.25 } else { -40.0 }, 'w'));
        let mut ending = payment("Phone", 20250120, -55.0, 'm');
        ending.set_end_mode(END_MODE_DATE);
        ending.end_date = 20270120;
        db.payments.push(ending);
        db.payments.push(payment("Insurance", 20250301, -600.0, 'y'));
        for (n, p) in db.payments.iter_mut().enumerate() { p.id = n as i32 + 1; }
        db.adjustments.set_custom_amount(20250601, 2500.0);
        db.rebuild_db(&cur, 12, false);
        db
    }

    type Edit = fn(&mut BudgetDb) -> Result<(), String>;

    /// Each edit leaves an in-place updated budget; finishing it with `after_edit` must equal a
    /// full rebuild, day for day.
    fn check(name: &str, ties: bool, edit: Edit, cur: i32) -> bool {
        let (mut fast, mut full) = (budget(ties), budget(ties));
        let cal = Cal::from_int(cur);
        edit(&mut fast).unwrap();
        edit(&mut full).unwrap();
        let rebuilt = after_edit(&mut fast, &cal, 12, false);
        full.rebuild_db(&cal, 12, false);
        assert!(snapshot(&mut fast) == snapshot(&mut full), "{name}: the in-place update must equal a rebuild");
        rebuilt
    }

    #[test]
    fn finishing_an_edit_in_place_equals_a_full_rebuild() {
        let edits: [(&str, Edit); 9] = [
            ("one occurrence", |db| document::change_transaction(db, 2, 20261001, -1500.0, "late", FLAG_NONE)),
            ("future amounts", |db| document::change_future_transactions(db, 3, 20261009, -90.0, "", FLAG_NONE)),
            ("delete occurrence", |db| document::delete(db, 4, Some(20261009))),
            ("delete payment", |db| document::delete(db, 5, None)),
            ("restore", |db| { document::delete(db, 2, Some(20261101))?; document::restore(db, 2, 20261101) }),
            ("move", |db| document::move_transaction(db, 1, 20261015, 20261016, 3200.0).map(|_| ())),
            ("balance", |db| document::set_balance(db, 20261020, Some(1234.0)).map(|_| ())),
            ("new payment", |db| document::put_payment(db, payment("Bonus", 20261020, 500.0, 'y'), None).map(|_| ())),
            ("earlier start", |db| { let mut p = db.payments[2].clone(); p.start_date = 20200103; document::put_payment(db, p, Some(3)).map(|_| ()) }),
        ];
        let mut skipped = 0;
        for (name, edit) in edits {
            if !check(name, false, edit, CUR) { skipped += 1; }
            check(&format!("{name} with tied amounts"), true, edit, CUR);
            check(&format!("{name} viewing another year"), false, edit, 20280301);
        }
        assert!(skipped >= 6, "most edits keep the window and the order, so they skip the rebuild ({skipped})");
    }

    /// Many payments sharing a few amounts (so equal transactions are common), edited one after
    /// another: after each edit the days and balances equal a budget rebuilt every time.
    #[test]
    fn a_long_series_of_edits_keeps_every_balance_equal_to_rebuilding() {
        let build = || {
            let cur = Cal::from_int(CUR);
            let mut db = BudgetDb::new(&cur, 12, false);
            db.today = 20261027;
            let mut seed: u32 = 7;
            let mut next = move |n: u32| { seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345); (seed >> 8) % n };
            for n in 0..150 {
                let freq = ['m', 'w', 'y', 'd'][next(4) as usize];
                let mut p = payment(&format!("P{n}"), 20250101 + next(12) as i32 * 100 + next(28) as i32, [-10.0, -25.0, -50.0, 100.0][next(4) as usize], freq);
                p.id = n + 1;
                p.category = (n % 3 == 0).then(|| format!("C{}", n % 5));
                if n % 7 == 0 { p.set_end_mode(END_MODE_DATE); p.end_date = 20270301; }
                if freq == 'd' { p.frequency = 9; }
                db.payments.push(p);
            }
            db.adjustments.set_custom_amount(20250301, 1000.0);
            db.rebuild_db(&cur, 12, false);
            (db, next)
        };
        let ((mut fast, mut next), (mut full, _)) = (build(), build());
        let cal = Cal::from_int(CUR);
        let days = |db: &mut BudgetDb| { let (w, d, _, t) = snapshot(db); (w, d, t) };
        let mut skipped = 0;
        for round in 0..40 {
            let id = 1 + next(150) as i32;
            let i = fast.get_by_id(id).unwrap();
            let date = (20261001..=20261231).find(|&d| fast.get_day(&Cal::from_int(d)).is_some_and(|day| day.get(i).is_some()));
            let edit = |db: &mut BudgetDb| match (round % 4, date) {
                (0, Some(date)) => document::change_transaction(db, id, date, -30.0, "edited", FLAG_NONE),
                (1, Some(date)) => document::delete(db, id, Some(date)),
                (2, _) => document::put_payment(db, payment(&format!("New{round}"), 20261005, -25.0, 'm'), None).map(|_| ()),
                (_, Some(date)) => document::change_future_transactions(db, id, date, -50.0, "", FLAG_NONE),
                _ => Ok(()),
            };
            if edit(&mut fast).is_err() { continue; }
            edit(&mut full).unwrap();
            if !after_edit(&mut fast, &cal, 12, false) { skipped += 1; }
            full.rebuild_db(&cal, 12, false);
            assert!(days(&mut fast) == days(&mut full), "round {round}: the in-place update must equal a rebuild");
        }
        assert!(skipped > 0);
    }

    #[test]
    fn a_rebuild_runs_when_the_window_or_the_order_would_change() {
        // Without the June 2025 checkpoint the window starts with the earliest payment instead.
        assert!(check("earlier start", false, |db| document::set_balance(db, 20250601, None).map(|_| ()), CUR));
        // Viewing a later year moves the window's end.
        assert!(check("other year", false, |db| document::change_transaction(db, 2, 20261001, -1500.0, "", FLAG_NONE), 20280301));
        // Groceries (index 2) and Gym (index 3) cost the same on shared Fridays: after Groceries is
        // updated in place it follows Gym, which a rebuild would not do.
        assert!(check("tie", true, |db| document::change_transaction(db, 3, 20261002, -85.25, "note", FLAG_NONE), CUR));
        assert!(!check("no tie", false, |db| document::change_transaction(db, 3, 20261002, -85.25, "note", FLAG_NONE), CUR));
    }

    #[test]
    fn the_window_matches_the_engine_rebuild() {
        let mut loan = payment("Car loan", 20180115, -400.0, 'm');
        loan.set_end_mode(END_MODE_LOAN_PAID);
        loan.loan_amount = 20000.0;
        let mut weekend = payment("Early", 20190105, -10.0, 'm'); // a Saturday
        weekend.set_no_weekends(true);
        weekend.set_early_pay(true);
        let mut dated = payment("Dated", 20240301, -5.0, 'm');
        dated.set_custom_amount(20170710, -7.0);
        let cases: Vec<(Vec<Payment>, Vec<i32>)> = vec![
            (vec![], vec![]),
            (vec![payment("Rent", 20250101, -1450.0, 'm')], vec![]),
            (vec![payment("Rent", 20250101, -1450.0, 'm')], vec![20240601, 20260715]),
            (vec![loan, weekend.clone()], vec![]),
            (vec![weekend, dated], vec![20100101]),
        ];
        for (payments, adjustments) in cases {
            for cur in [20261001, 20190301, 20300601] {
                for (months, history) in [(12, false), (0, false), (24, true)] {
                    let mut db = BudgetDb::new(&Cal::from_int(cur), months, history);
                    db.payments = payments.clone();
                    for date in &adjustments { db.adjustments.set_custom_amount(*date, 100.0); }
                    let (start, end) = db.window(&Cal::from_int(cur), months, history);
                    let predicted = (start.get(), end.get());
                    db.rebuild_db(&Cal::from_int(cur), months, history);
                    assert_eq!(predicted, (db.start_date.get(), db.end_date.get()), "cur {cur}, {months} months, history {history}");
                }
            }
        }
    }
}
