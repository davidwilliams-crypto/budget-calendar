//! Regression tests for the 360 review: loan schedules, early-pay window and huge loans.

use budget_core::{bgt, BudgetDb, Cal, Field};

fn build(line: &str, cal: i32, future: i32) -> BudgetDb {
    let text = format!("v,2\n:\n=,\n{line}\n## END-OF-DATA; line_no=5 ##");
    let l = bgt::load_str(&text, cal, None).unwrap_or_else(|l| panic!("{}", l.log));
    BudgetDb::from_loaded(l, &Cal::from_int(cal), future, false)
}

/// Sum of every placed transaction of payment 0 between two dates, plus the count.
fn placed(db: &mut BudgetDb, from: i32, to: i32) -> (f64, usize) {
    let mut c = Cal::from_int(from);
    let (mut sum, mut n) = (0.0, 0);
    while c.get() <= to {
        if let Some(d) = db.get_day(&c) {
            for t in &d.transactions { if t.pmt == 0 { sum += t.amount; n += 1; } }
        }
        c.add(Field::Day, 1);
    }
    (sum, n)
}

const SHIFTED_LOAN: &str = "-200.0%1000.0, Loan, 20260601, 1m, 5, 20261001, 65, ";

#[test]
fn weekend_shifted_loan_installment_edit_keeps_principal() {
    let mut db = build(SHIFTED_LOAN, 20260601, 12);
    // 2026-08-01 is a Saturday, shown on Monday 2026-08-03.
    assert_eq!(placed(&mut db, 20260601, 20271231).0, -1000.0);
    db.payments[0].set_custom_amount(20260803, -150.0);
    db.update_payment(0);
    assert!((placed(&mut db, 20260601, 20271231).0 + 1000.0).abs() < 1e-9);
}

#[test]
fn weekend_shifted_loan_installment_skip_extends_loan() {
    let mut db = build(SHIFTED_LOAN, 20260601, 12);
    let (_, n_before) = placed(&mut db, 20260601, 20271231);
    db.payments[0].set_no_transaction(20260803);
    db.update_payment(0);
    let (sum, n) = placed(&mut db, 20260601, 20271231);
    // the skipped installment is deferred: the principal is still repaid in full, one period later
    assert!((sum + 1000.0).abs() < 1e-9, "{sum}");
    assert_eq!(n, n_before);
}

#[test]
fn early_pay_weekend_start_before_window_is_placed() {
    // 2026-03-01 is a Sunday; early pay puts the first rent on Friday 2026-02-27.
    let mut db = build("-1000.0, Rent, 20260301, 1m, 0, 20260301, 21, ", 20261001, 3);
    let c = Cal::from_int(20260227);
    let d = db.get_day(&c).expect("window must reach 2026-02-27");
    assert!(d.transactions.iter().any(|t| t.pmt == 0 && t.amount == -1000.0));
}

#[test]
fn loan_float_residue_does_not_add_a_phantom_installment() {
    for (principal, inst, n) in [(2333.10, 33.33, 70), (1678.24, 12.34, 136), (2298.85, 19.99, 115)] {
        let db = build(&format!("-{inst}%{principal}, L, 20260101, 1m, 5, 20261201, 64, "), 20260101, 12);
        assert_eq!(db.payments[0].n_pays, n, "{principal}/{inst}");
    }
}

#[test]
fn huge_or_tiny_loans_terminate() {
    // Roughly a billion daily installments: bounded at year 9999 instead of hanging or panicking.
    let db = build("-0.001%1000000.0, L, 20261001, 1d, 5, 20270901, 64, ", 20261001, 12);
    assert!(db.payments[0].n_pays > 1000 && db.payments[0].end_date > 20261001);
    let db = build("-1.0%1.0E15, L, 20261001, 1d, 5, 20270901, 64, ", 20261001, 12);
    assert!(db.payments[0].n_pays > 0);
    // non-finite loan principal is refused by the loader
    let text = "v,2\n:\n=,\n-10.0%Infinity, L, 20261001, 1m, 5, 20270901, 64, \n## END-OF-DATE ##";
    assert!(bgt::load_str(text, 20261001, None).is_err());
}

// ---------- review round 3 ----------

#[test]
fn planned_transaction_count_folds_unicode_case() {
    let db = build("-10.0, Café, 20240115, 1m, 0, 20240115, 16, ", 20240115, 12);
    let (from, to) = (Cal::from_int(20240201), Cal::from_int(20240331));
    for desc in ["CAFÉ", "café", "Café"] {
        assert_eq!(db.number_of_planned_transactions(desc, &from, &to), 2, "{desc}");
    }
}

fn days(from: i32, n: i32) -> Vec<i32> {
    let mut c = Cal::from_int(from);
    (0..n).map(|_| { let v = c.get(); c.add(Field::Day, 1); v }).collect()
}

#[test]
fn valid_payment_days_follow_grace_period() {
    let db = build("-50.0, Rent, 20240105, 1m;5, 0, 20240105, 17, ", 20240105, 12);
    assert_eq!(db.payments[0].grace_period_days, 5);
    assert_eq!(db.valid_payment_days(0, 20240205), days(20240205, 5));
    assert_eq!(db.valid_payment_days(0, 20240208), days(20240205, 5));
    // before the scheduled day of the month: the coming window is shown
    assert_eq!(db.valid_payment_days(0, 20240203), days(20240205, 5));
    // after the grace window: nothing is valid any more this month
    assert!(db.valid_payment_days(0, 20240212).is_empty());
    // a window that crosses the month end spills into the next month
    let db = build("-50.0, Rent, 20240129, 1m;5, 0, 20240129, 16, ", 20240105, 12);
    assert_eq!(db.valid_payment_days(0, 20240129), vec![20240129, 20240130, 20240131, 20240201, 20240202]);
    // no grace period: empty
    let db = build("-50.0, Rent, 20240105, 1m, 0, 20240105, 16, ", 20240105, 12);
    assert!(db.valid_payment_days(0, 20240205).is_empty());
    // tentative: the whole period is valid
    let db = build("-10.0, T, 20240101, 1w, 0, 20240101, 18, ", 20240101, 12);
    assert_eq!(db.valid_payment_days(0, 20240103), days(20240101, 7));
}

/// Every transaction of payment 0 on `date`.
fn on(db: &mut BudgetDb, date: i32) -> Vec<(f64, char)> {
    db.get_day(&Cal::from_int(date)).map_or_else(Vec::new, |d| d.transactions.iter().filter(|t| t.pmt == 0).map(|t| (t.amount, t.suffix)).collect())
}

#[test]
fn daily_business_day_loan_merges_weekend_installments() {
    let mut db = build("-20.0%1000.0, L, 20260105, 1d, 1, 20260105, 65, img", 20260101, 12);
    assert_eq!(db.payments[0].n_pays, 50);
    let (sum, _) = placed(&mut db, 20260101, 20271231);
    assert!((sum + 1000.0).abs() < 0.005, "{sum}");
    // Saturday and Sunday are moved onto Monday 2026-01-12 together with Monday's own installment.
    assert_eq!(on(&mut db, 20260112), vec![(-60.0, budget_core::SUFFIX_ADJUSTED)]);
    // early pay merges onto Friday
    let mut db = build("-20.0%1000.0, L, 20260105, 1d, 1, 20260105, 69, img", 20260101, 12);
    let (sum, _) = placed(&mut db, 20251201, 20271231);
    assert!((sum + 1000.0).abs() < 0.005, "{sum}");
    assert_eq!(on(&mut db, 20260109), vec![(-60.0, budget_core::SUFFIX_ADJUSTED)]);
    // every-two-days business-day loan
    let mut db = build("-532.8%3202.9799999999996, L, 20260718, 2d, 1, 20260718, 65, img", 20260701, 12);
    let (sum, _) = placed(&mut db, 20260601, 20271231);
    assert!((sum + 3202.98).abs() < 0.005, "{sum}");
}

#[test]
fn daily_business_day_payment_has_no_phantom_plan_deviation() {
    let mut db = build("-15.0, Parking, 20260105, 1d, 1, 20260125, 33, img", 20260101, 12);
    let st = budget_core::stats::calc_statistics(&mut db, &Cal::from_int(20260101), 1, false, None);
    assert!((st.total_expense + 315.0).abs() < 1e-9, "{}", st.total_expense);
    assert!((st.total_planned_expense - st.total_expense).abs() < 1e-9, "{} vs {}", st.total_planned_expense, st.total_expense);
}

#[test]
fn option_on_nominal_weekend_date_does_not_skip_the_displayed_installment() {
    // 2026-03-01 is a nominal Sunday occurrence shown on Monday 2026-03-02.
    for opt in ["20260301", "20260301:-100.0", "20260302", "20260302:-100.0"] {
        let mut db = build(&format!("-845.19%42270.93, Loan, 20251207, 2w, 1, 20251207, 65, img, {opt}"), 20251207, 48);
        let first = (db.payments[0].n_pays, db.payments[0].end_date, placed(&mut db, 20251101, 20301231).0);
        assert!((first.2 + 42270.93).abs() < 0.01, "{opt}: {}", first.2);
        for _ in 0..2 {
            db.update_payment(0);
            let again = (db.payments[0].n_pays, db.payments[0].end_date, placed(&mut db, 20251101, 20301231).0);
            assert_eq!(again.0, first.0, "{opt}");
            assert_eq!(again.1, first.1, "{opt}");
            assert!((again.2 - first.2).abs() < 1e-6, "{opt}");
        }
    }
}

#[test]
fn scheduling_leaves_stored_options_and_flags_alone() {
    // The original's update dropped a custom amount equal to the scheduled one (2026-02-05), a
    // deletion marker on a day without a transaction (2026-02-10), and a past tentative period's
    // deletion marker together with that date's flag (2026-01-05).
    for line in ["-50.0, Rent, 20260105, 1m, 0, 20260105, 16, img, 20260205:-50.0, 20260210, 20260305:-60.0",
        "-10.0, T, 20260101, 1w, 0, 20260101, 18, img, 20260105, 20260105;"] {
        let text = format!("v,2\n:\n=,\n{line}\n## END-OF-DATA; line_no=5 ##");
        let l = bgt::load_str(&text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
        let (opt, flag) = (l.payments[0].opt.clone(), l.payments[0].flag.clone());
        let mut db = BudgetDb::from_loaded(l, &Cal::from_int(20260101), 12, false);
        let shown = placed(&mut db, 20251101, 20271231);
        for _ in 0..2 {
            assert_eq!((&db.payments[0].opt, &db.payments[0].flag), (&opt, &flag), "{line}");
            db.update_payment(0);
            assert_eq!(placed(&mut db, 20251101, 20271231), shown, "{line}");
        }
    }
}

#[test]
fn huge_frequency_times_count_loads_quickly() {
    for line in ["-10.0, A, 20260101, 100000d, 4000000, 20260101, 48,  ", "-10.0, A, 20260101, 100000w, 4000000, 20260101, 48,  "] {
        let t = std::time::Instant::now();
        let db = build(line, 20260101, 12);
        assert!(t.elapsed().as_secs_f64() < 2.0, "{line}: {:?}", t.elapsed());
        assert!(db.payments[0].end_date <= 99_991_231);
        let t = std::time::Instant::now();
        let _ = db.is_regular_payment_on(0, 99_991_231);
        let _ = db.next_regular_payment_after_date(0, 99_991_231);
        let _ = db.payments_remaining(0, &Cal::from_int(20260101));
        let _ = db.number_of_planned_transactions("A", &Cal::from_int(20260101), &Cal::from_int(99_991_231));
        assert!(t.elapsed().as_secs_f64() < 2.0, "{line}: {:?}", t.elapsed());
    }
}
