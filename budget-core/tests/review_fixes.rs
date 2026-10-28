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
    let text = "v,2\n:\n=,\n-10.0%Infinity, L, 20261001, 1m, 5, 20270901, 64, \n## END-OF-DATA; line_no=5 ##";
    let l = bgt::load_str(text, 20261001, None).unwrap_err();
    assert!(l.log.contains("Invalid data for 'loan amount'"), "{}", l.log);
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

#[test]
fn viewing_the_last_calendar_years_never_builds_days_past_the_calendar() {
    let mut db = BudgetDb::new(&Cal::from_int(99991201), 24, false);
    db.rebuild_db(&Cal::from_int(99991201), 24, false);
    assert_eq!(db.end_date.get(), budget_core::date::DATE_MAX);
    assert!(db.get_day(&Cal::from_int(99991231)).is_some());
    let (_, end) = db.window(&Cal::from_int(99980601), 36, true);
    assert_eq!(end.get(), budget_core::date::DATE_MAX);
}

// ---------- review round 4 ----------

#[test]
fn categories_list_each_folded_name_once() {
    // The list is sorted case-insensitively, so every spelling of one category is adjacent in it:
    // one entry per folded name, the first spelling in payment order.
    let lines = [
        "-10.0, A, 20260101, 1m, 5, 20260101, 48,  , $food",
        "-5.0, B, 20260101, 1m, 5, 20260101, 48,  , $Rent",
        "-1.0, C, 20260101, 1m, 5, 20260101, 48,  , $Food",
        "-2.0, D, 20260101, 1m, 5, 20260101, 48,  , $FOOD",
    ];
    let text = format!("v,2\n:\n=,\n{}\n## END-OF-DATA; line_no={} ##", lines.join("\n"), lines.len() + 4);
    let l = bgt::load_str(&text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
    let db = BudgetDb::from_loaded(l, &Cal::from_int(20260101), 12, false);
    assert_eq!(db.categories(), ["food", "Rent"]);
}

/// Round 4: a stored date such as 3000 or 00990101 is read by `Cal::from_int` as a year or a month,
/// so it must not drag the window start past its end (a capacity overflow in `rebuild_db`).
#[test]
fn short_form_dates_in_a_file_do_not_break_the_window() {
    let files = [
        "v,2
:
=,
-10.0, A, 3000, 1m, 1, 3000, 16,  
## END-OF-DATA; line_no=5 ##",
        "v,2
:
=,
-10.0, A, 20261001, 1m, 5, 20270301, 16,  , 00990101:5.0
## END-OF-DATA; line_no=5 ##",
        "v,2
:, 00991201:5.0
=,
-10.0, A, 20261001, 1m, 5, 20270301, 16,  
## END-OF-DATA; line_no=5 ##",
        "v,2
:
=,
-10.0, A, 202701, 1m, 5, 20270301, 16,  
## END-OF-DATA; line_no=5 ##",
    ];
    for (i, text) in files.iter().enumerate() {
        if bgt::load_str(text, 20261028, None).is_err() { continue; }
        for unlimited in [false, true] {
            let loaded = bgt::load_str(text, 20261028, None).unwrap_or_else(|l| panic!("{}", l.log));
            let mut db = BudgetDb::from_loaded(loaded, &Cal::from_int(20261028), 24, unlimited);
            for month in [20150101, 20261001, 20301201, 20991201] {
                db.rebuild_db(&Cal::from_int(month), 24, unlimited);
                assert!(db.start_date.le(&db.end_date), "file {i}, window {month}");
                // A short-form date must not drag the window away from the month being viewed (a blank calendar).
                assert!(db.is_month_available(&Cal::from_int(month)), "file {i}: month {month} is outside {:?}..{:?}", db.start_date, db.end_date);
            }
        }
    }
}

/// Round 4: Notepad's byte-order mark is not part of the first `.budgetrc` key.
#[test]
fn budgetrc_with_a_byte_order_mark_keeps_its_first_setting() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("bom-budgetrc");
    std::fs::write(&path, b"\xEF\xBB\xBFautosave=false\nport=1234\n").unwrap();
    let cfg = budget_core::config::BudgetConfig::load(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(!cfg.autosave);
    assert_eq!(cfg.port, 1234);
}

/// Round 4: viewing a month long past December 9999 must not build a window that ends before it starts.
#[test]
fn a_month_past_the_last_year_gives_a_valid_window() {
    let mut db = BudgetDb::new(&Cal::from_int(20261028), 12, false);
    for month in [99991201, 100010301, 100500101, 200000101] {
        db.rebuild_db(&Cal::from_int(month), 12, false);
        assert!(db.start_date.le(&db.end_date), "month {month}");
    }
}

/// Round 5: a business-days early-pay rent due on Saturday 1 January 2028 is paid on Friday 31 December
/// 2027, so it belongs to a window that ends on that day just as it does to a wider one.
#[test]
fn an_early_pay_occurrence_due_just_past_the_window_end_is_still_placed() {
    let line = "-1000.0, Rent, 20260101, 1m, 0, 20260101, 21, ";
    let mut short = build(line, 20261001, 12);
    assert_eq!(short.end_date.get(), 20271231);
    let mut wide = build(line, 20271001, 12);
    let amount = |db: &mut BudgetDb| db.get_day(&Cal::from_int(20271231)).map(|d| d.transactions.iter().map(|t| t.amount).sum::<f64>());
    assert_eq!(amount(&mut wide), Some(-1000.0));
    assert_eq!(amount(&mut short), Some(-1000.0));
    // Due on Sunday 2 January 2028: also paid on that Friday, two days before the due date.
    let line = "-1000.0, Rent, 20260102, 1m, 0, 20260102, 21, ";
    let mut short = build(line, 20261001, 12);
    let mut wide = build(line, 20271001, 12);
    assert_eq!(amount(&mut wide), Some(-1000.0));
    assert_eq!(amount(&mut short), Some(-1000.0));
}

/// Round 5: a checkpoint typed equal to the balance shown is not an adjustment (float residue).
#[test]
fn a_checkpoint_equal_to_the_balance_shown_leaves_no_adjustment() {
    use budget_core::model::Payment;
    let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
    let mut p = Payment::new(20261001);
    p.description = "Groceries".into();
    p.amount = -19.99;
    p.freq_type = 'w';
    p.frequency = 1;
    p.n_pays = 10;
    db.add(p);
    for day in [20261008, 20261015, 20261022, 20261029] {
        let day = Cal::from_int(day);
        let shown = db.get_day(&day).unwrap().closing_balance;
        db.update_adjustment(&day, (shown * 100.0).round() / 100.0);
        assert_eq!(db.get_day(&day).unwrap().adjustment_diff, 0.0, "{day:?}");
    }
    // a real difference of a cent still counts
    let day = Cal::from_int(20261105);
    let shown = db.get_day(&day).unwrap().closing_balance;
    db.update_adjustment(&day, shown + 0.01);
    assert!((db.get_day(&day).unwrap().adjustment_diff - 0.01).abs() < 1e-9);
}

/// Round 5: the grace-period days of a month come from one walk over the schedule; they must be what
/// asking day by day gives.
#[test]
fn grace_days_match_a_day_by_day_walk() {
    use budget_core::model::Payment;
    let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
    for (start, ft, freq, grace, tentative) in [(20260105, 'm', 1, 5, false), (20260101, 'w', 2, 3, false), (20260131, 'm', 1, 4, false),
        (20260301, 'd', 3, 2, false), (20260120, 'w', 1, 0, true)] {
        let mut p = Payment::new(start);
        p.description = format!("P{start}{ft}");
        p.amount = -10.0;
        p.freq_type = ft;
        p.frequency = freq;
        p.set_end_mode(budget_core::END_MODE_INDEFINITE);
        p.grace_period_days = grace;
        if tentative { p.set_tentative(true); }
        db.add(p);
    }
    let mut nonempty = 0;
    for i in 0..db.payments.len() {
        let grace = if db.payments[i].is_tentative() { db.period_days(i) } else { db.payments[i].grace_period_days };
        for cal in (20261001..=20261031).chain(20261201..=20261231) {
            let got = db.valid_payment_days(i, cal);
            if !got.is_empty() { nonempty += 1; }
            // reference: the first regular date of the month whose window covers `cal`
            let mut want = Vec::new();
            if grace > 0 {
                let tentative = db.payments[i].is_tentative();
                for day in (cal / 100 * 100 + 1)..=(cal / 100 * 100 + 31) {
                    if !db.is_regular_payment_on(i, day) { continue; }
                    let mut last = Cal::from_int(day);
                    last.add(Field::Day, grace - 1);
                    let inside = day <= cal && cal <= last.get();
                    if !(tentative && !inside) && !(cal > day && !inside) {
                        let mut c = Cal::from_int(day);
                        want = (0..grace).map(|_| { let v = c.get(); c.add(Field::Day, 1); v }).collect();
                        break;
                    }
                }
            }
            assert_eq!(got, want, "payment {i}, {cal}");
        }
    }
    // The comparison is only worth something if the payments do have grace windows in these months.
    assert!(nonempty > 100, "only {nonempty} non-empty grace windows");
}

// ---- Round 7: rounding dust in sums of cent amounts --------------------------------------------

fn payment(description: &str, category: Option<&str>, amount: f64, start: i32, n_pays: i32) -> budget_core::model::Payment {
    let mut p = budget_core::model::Payment::new(start);
    p.description = description.into();
    p.category = category.map(str::to_string);
    p.amount = amount;
    p.n_pays = n_pays;
    p
}

fn db_with(payments: Vec<budget_core::model::Payment>) -> BudgetDb {
    let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
    for p in payments { db.add(p); }
    db
}

#[test]
fn dust_tolerances_scale_with_the_amounts_but_stay_below_half_a_cent() {
    use budget_core::numfmt::{drop_dust, noise_floor, snap_to_cent};
    // two ulps of a ten-billion balance are 3.8e-6: past the fixed millionth, still dust
    let exact = 10_000_000_000.3_f64;
    assert_eq!(snap_to_cent(exact + 3.8e-6), exact);
    assert_eq!(drop_dust(0.01, 1e13), 0.01, "a cent is never dust, whatever the scale");
    assert!(noise_floor(1e30) < 0.005);
}

#[test]
fn dust_helpers_snap_cents_and_nothing_else() {
    use budget_core::numfmt::{drop_dust, snap_to_cent};
    assert_eq!(snap_to_cent(149.85000000000002), 149.85);
    assert_eq!(snap_to_cent(-19.989999999999863), -19.99);
    assert_eq!(snap_to_cent(1234.56 - 234.56 - 1000.0).to_bits(), 0.0f64.to_bits(), "no negative zero");
    assert_eq!(snap_to_cent(3.333333), 3.333333, "a real sub-cent amount stays");
    assert_eq!(drop_dust(1.8e-15, 10.0), 0.0);
    assert_eq!(drop_dust(2e-9, 1e9), 0.0, "dust grows with the size of the amounts");
    assert_eq!(drop_dust(0.01, 1e12), 0.01, "a cent is never dust, even at the 1e12 limit");
}

#[test]
fn a_description_whose_income_and_expense_cancel_leaves_no_phantom_rows() {
    use budget_core::stats::{calc_statistics, UNCATEGORIZED_EXPENSES, UNCATEGORIZED_INCOME};
    // 0.38 + 9.56 - 9.94 is 1.8e-15 in floating point, and 0.38 + 5.85 - 6.23 is -8.9e-16.
    for (planned, amounts) in [(-5.0, [0.38, 9.56, -9.94]), (5.0, [0.38, 5.85, -6.23])] {
        let mut db = db_with(vec![payment("Gift", None, planned, 20261001, 3)]);
        for (i, amount) in amounts.iter().enumerate() {
            db.payments[0].set_custom_amount(20261001 + 100 * i as i32, *amount);
        }
        db.update_payment(0);
        let st = calc_statistics(&mut db, &Cal::from_int(20261001), 3, true, None);
        for r in st.income.iter().chain(st.expense.iter()) {
            assert!(r.amount == 0.0 || r.amount.abs() >= 0.005, "{planned}: dust {} in {}", r.amount, r.key);
        }
        if planned < 0.0 {
            assert!(st.income.iter().all(|r| r.key != UNCATEGORIZED_INCOME && r.key != "   Gift"), "{planned}: {:?}", st.income);
        } else {
            assert!(st.income.iter().all(|r| r.key != UNCATEGORIZED_EXPENSES), "{planned}: {:?}", st.income);
        }
        let entries = budget_core::report::month_by_month_entries(&mut db, &st).unwrap_or_else(|e| panic!("{planned}: {e}"));
        assert!(entries.iter().all(|e| e.description != "Total Income" || e.total != 0.0 || planned > 0.0), "{planned}: a row of zeros");
    }
}

#[test]
fn a_category_emptied_at_large_amounts_is_still_exactly_zero() {
    use budget_core::stats::{calc_statistics_exact, UNCATEGORIZED_EXPENSES};
    let mut db = db_with(vec![payment("Phone", None, -9_659_066.44, 20261001, 2), payment("Netflix", None, -9_244_940.36, 20261001, 2)]);
    db.payments[0].set_custom_amount(20261001, 14_107_578.31);
    db.payments[1].set_custom_amount(20261001, 17_753_824.90);
    db.update_payment(0);
    db.update_payment(1);
    let st = calc_statistics_exact(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261130), true, None);
    assert!(st.income.iter().all(|r| r.key != UNCATEGORIZED_EXPENSES), "{:?}", st.income);
    assert!(st.expense.iter().any(|r| r.key == UNCATEGORIZED_EXPENSES));
}

#[test]
fn a_pass_through_day_leaves_the_balance_exactly_where_it_was() {
    // 1234.56 - 234.56 - 1000.00 is -5.7e-14 in floating point: the month must stay flat.
    let mut db = db_with(vec![payment("Pay", None, 1234.56, 20261015, 1), payment("Rent", None, -234.56, 20261015, 1), payment("Bills", None, -1000.0, 20261015, 1)]);
    for day in 15..=31 {
        assert_eq!(db.get_day(&Cal::from_int(20261000 + day)).unwrap().closing_balance, 0.0, "day {day}");
    }
}

#[test]
fn merged_weekend_amounts_and_the_last_loan_instalment_are_whole_cents() {
    use budget_core::model::Payment;
    // Daily business-days payment from Friday 2 October 2026: Monday 5 October carries three days.
    let mut daily = Payment::new(20261002);
    daily.description = "Coffee".into();
    daily.amount = -49.95;
    daily.freq_type = 'd';
    daily.frequency = 1;
    daily.set_no_weekends(true);
    daily.set_end_mode(budget_core::END_MODE_INDEFINITE);
    // A loan whose principal is an exact multiple of the instalment: 30 x 19.99.
    let mut loan = Payment::new(20261001);
    loan.description = "Loan".into();
    loan.amount = -19.99;
    loan.freq_type = 'm';
    loan.frequency = 1;
    loan.loan_amount = 599.70;
    loan.set_end_mode(budget_core::END_MODE_LOAN_PAID);
    let mut db = db_with(vec![daily, loan]);
    let monday = db.get_day(&Cal::from_int(20261005)).unwrap().transactions.iter().find(|t| t.pmt == 0).map(|t| t.amount);
    assert_eq!(monday, Some(-149.85));
    let (mut last, mut c) = (None, Cal::from_int(20261001));
    while c.get() < 20291231 {
        if let Some(d) = db.get_day(&c) { if let Some(t) = d.transactions.iter().find(|t| t.pmt == 1) { last = Some(t.amount); } }
        c.add(Field::Day, 1);
    }
    assert_eq!(last, Some(-19.99));
}

#[test]
fn reports_do_not_print_a_negative_zero_or_a_percentage_of_dust() {
    use budget_core::report::{generate_report, month_by_month_entries, ReportMode};
    use budget_core::stats::calc_statistics;
    // October: income 1234.56, repairs and a mortgage that cancel it in category Home; November: +50.
    let mut db = db_with(vec![
        payment("Rental income", Some("Home"), 1234.56, 20261015, 1),
        payment("Repairs", Some("Home"), -234.56, 20261015, 1),
        payment("Mortgage", Some("Home"), -1000.0, 20261015, 1),
        payment("Bonus", Some("Home"), 50.0, 20261115, 1),
    ]);
    let (start, end) = (Cal::from_int(20261001), Cal::from_int(20261101));
    let resolver = |_: &str| -> Option<String> { None };
    for mode in [ReportMode::Html, ReportMode::Excel] {
        let report = generate_report(&mut db, &start, &end, mode, &resolver);
        assert!(!report.contains("-0.00"), "{mode:?}");
        assert!(!report.contains("color=red>0<small>.00"), "{mode:?}: a red zero");
    }
    let st = calc_statistics(&mut db, &start, 2, true, None);
    let entries = month_by_month_entries(&mut db, &st).unwrap_or_else(|e| panic!("{e}"));
    let home = entries.iter().find(|e| e.description == "Home" && e.category.is_none()).expect("the Home category row");
    assert_eq!(home.amounts[0], 0.0, "a category that nets to nothing is 0, not dust");
    assert_eq!(home.amounts[1], 50.0);
}

#[test]
fn the_excel_stats_report_marks_a_difference_from_plan_only_when_there_is_one() {
    use budget_core::report::generate_excel_stats_report;
    use budget_core::stats::calc_statistics;
    // Three payments of one category on one day, each exactly as planned: no "+/-" cell on the category.
    let mut db = db_with(vec![
        payment("Water", Some("Bills"), -19.99, 20261015, 1),
        payment("Power", Some("Bills"), -29.99, 20261015, 1),
        payment("Phone", Some("Bills"), -49.95, 20261015, 1),
    ]);
    let st = calc_statistics(&mut db, &Cal::from_int(20261001), 1, true, None);
    let report = generate_excel_stats_report(&st);
    // The two totals rows always carry the grey cell; nothing else may.
    assert_eq!(report.matches("aaAAaa").count(), 2, "{report}");
}

#[test]
fn a_zero_based_month_has_a_disposable_income_of_exactly_zero() {
    use budget_core::report::{generate_month_by_month_report, generate_stats_report, month_by_month_entries};
    use budget_core::stats::calc_statistics;
    let resolver = |_: &str| -> Option<String> { None };
    // Pay 9.94 less fees of 0.38 and 9.56: 9.94 - 9.94 is not 0 once the fees are added up first.
    let mut db = db_with(vec![payment("Pay", None, 9.94, 20261005, 1), payment("Fee1", None, -0.38, 20261010, 1), payment("Fee2", None, -9.56, 20261015, 1)]);
    let st = calc_statistics(&mut db, &Cal::from_int(20261001), 1, true, None);
    assert_eq!(st.net(), 0.0);
    let report = generate_stats_report(&st, &resolver);
    let row = report.split("<TR").find(|r| r.contains("Disposable Income")).expect("the Disposable Income row");
    assert!(!row.contains("-0."), "{row}");
    // Month-by-Month: October nets to nothing, so November's change has no percentage of dust.
    let mut db = db_with(vec![payment("Interest", None, 0.38, 20261015, 1), payment("Refund", None, 9.56, 20261016, 1),
        payment("Fee", None, -9.94, 20261017, 1), payment("Salary", None, 100.0, 20261115, 1)]);
    let st = calc_statistics(&mut db, &Cal::from_int(20261001), 3, true, None);
    let entries = month_by_month_entries(&mut db, &st).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(entries[0].amounts[0], 0.0);
    let html = generate_month_by_month_report(&mut db, &st, false).unwrap_or_else(|e| panic!("{e}"));
    assert!(html.contains("title='+100'"), "no percentage against a zero month");
}

#[test]
fn a_difference_of_exactly_five_dollars_is_not_a_difference_in_the_print_report() {
    use budget_core::report::generate_stats_report;
    use budget_core::stats::calc_statistics;
    let resolver = |_: &str| -> Option<String> { None };
    // planned -29.99, actual -34.99: the difference is -5.0000000000000036 in floating point
    let mut db = db_with(vec![payment("Dinner", None, -29.99, 20261010, 1)]);
    db.payments[0].set_custom_amount(20261010, -34.99);
    db.update_payment(0);
    let st = calc_statistics(&mut db, &Cal::from_int(20261001), 1, true, None);
    let report = generate_stats_report(&st, &resolver);
    assert!(!report.contains("-5.00"), "the +/- column stays blank until the difference is more than 5.00: {report}");
}

#[test]
fn the_whole_dollar_calendar_total_difference_is_zero_not_minus_zero() {
    use budget_core::report::{generate_monthly_calendar, CalendarOptions};
    let mut db = db_with(vec![payment("Pay", None, 9.94, 20261005, 1), payment("Fee1", None, -0.38, 20261010, 1), payment("Fee2", None, -9.56, 20261015, 1)]);
    let opts = CalendarOptions { show_decimals: false, ..Default::default() };
    let resolver = |_: &str| -> Option<String> { None };
    let html = generate_monthly_calendar(&mut db, &Cal::from_int(20261001), &Cal::from_int(20261001), &opts, &resolver);
    assert!(html.contains("Totals:") && !html.contains("(-0)"), "{html}");
}
