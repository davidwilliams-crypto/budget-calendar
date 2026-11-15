//! Regression tests for the core-calc review: series edits "from a date onward" and the dates a
//! split recognises as scheduled.

use budget_core::commands;
use budget_core::*;

/// The amount each transaction of payment `id` shows from `from` to `to`, by date.
fn shown(db: &mut BudgetDb, id: i32, from: i32, to: i32) -> Vec<(i32, f64)> {
    let i = db.get_by_id(id).unwrap();
    let mut out = Vec::new();
    let mut c = Cal::from_int(from);
    while c.get() <= to {
        if let Some(t) = db.get_day(&c).and_then(|d| d.get(i)) { out.push((c.get(), t.amount)); }
        c.add(Field::Day, 1);
    }
    out
}

#[test]
fn editing_from_a_date_past_the_last_scheduled_payment_keeps_the_earlier_ones() {
    // Three monthly payments of 100 from 5 January; the last one was dragged to 10 April, so the
    // series has no regular payment left on or after that date. Editing that transaction "from
    // 10 April onward" used to fall back to editing every occurrence and rewrote January to March.
    let mut db = BudgetDb::new_with_today(&Cal::from_int(20260101), 12, false, 20260101);
    let mut p = Payment::new(20260105);
    p.description = "Gym".into();
    p.amount = -100.0;
    p.n_pays = 3;
    let id = commands::apply_payment(&mut db, p, None, None).unwrap();
    assert!(commands::move_transaction(&mut db, id, 20260305, 20260410, -100.0).unwrap());
    let before = shown(&mut db, id, 20260101, 20260409);
    assert_eq!(before, [(20260105, -100.0), (20260205, -100.0)]);

    let mut edited = db.payments[db.get_by_id(id).unwrap()].clone();
    edited.amount = -150.0;
    // Either the edit is refused or it leaves every earlier transaction as it was.
    if commands::apply_payment(&mut db, edited, Some(id), Some(20260410)).is_ok() {
        let mut earlier = Vec::new();
        for p in db.payments.iter().map(|p| p.id).collect::<Vec<_>>() {
            earlier.extend(shown(&mut db, p, 20260101, 20260409));
        }
        earlier.sort_by_key(|e| e.0);
        assert_eq!(earlier, before, "an edit from 10 April onward changed earlier transactions");
    }
}

#[test]
fn a_future_amount_change_lets_an_early_paid_month_end_occurrence_follow_the_new_amount() {
    // Monthly on the 1st, business days with early pay: 1 February 2026 is a Sunday, so that
    // payment is made on Friday 30 January. Its custom amount there is on a scheduled date, so a
    // future amount change must drop it (the occurrence then follows the payment), not keep it as a
    // custom amount that a later change of the series amount would no longer reach.
    let mut db = BudgetDb::new_with_today(&Cal::from_int(20251101), 12, false, 20251001);
    let mut p = Payment::new(20251101);
    p.description = "Rent".into();
    p.amount = -100.0;
    p.set_end_mode(END_MODE_INDEFINITE);
    p.set_no_weekends(true);
    p.set_early_pay(true);
    let id = commands::apply_payment(&mut db, p, None, None).unwrap();
    commands::change_transaction(&mut db, id, 20260130, -150.0, "", FLAG_NONE).unwrap();
    assert_eq!(shown(&mut db, id, 20260130, 20260130), [(20260130, -150.0)]);

    commands::change_future_transactions(&mut db, id, 20251201, -200.0, "", FLAG_NONE).unwrap();
    let later = db.payments.iter().find(|p| p.start_date == 20251201).expect("the later part").clone();
    assert!(later.option_by_date(20260130).is_none(), "custom amount kept on a scheduled date: {:?}", later.opt);

    // The series amount then changes for every occurrence, 30 January included.
    let mut edited = later.clone();
    edited.amount = -250.0;
    commands::apply_payment(&mut db, edited, Some(later.id), None).unwrap();
    assert_eq!(shown(&mut db, later.id, 20260130, 20260130), [(20260130, -250.0)]);
}
