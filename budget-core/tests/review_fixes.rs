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
