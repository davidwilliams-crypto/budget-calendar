//! Desktop compatibility wrapper for shared core commands and checked file saving.
pub use budget_core::commands::*;
use budget_core::{bgt, BudgetDb};
use std::path::Path;

pub fn save_checked(db: &BudgetDb, path: &Path, password: Option<&str>, expected: Option<&[u8]>) -> Result<Vec<u8>, String> {
    if let Some(expected) = expected {
        match std::fs::read(path) {
            Ok(current) if current != expected => {
                return Err("The budget file changed on disk since it was opened. Use Save as to keep your edits in a separate file, or reopen it to load the other changes.".into());
            }
            Ok(_) => {}
            // The file is gone: writing it again cannot overwrite anyone's edits.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("The budget file is unavailable: {e}. Use Save as to keep your edits in a separate file.")),
        }
    }
    let text = db.to_text(password)?;
    bgt::save_file(path, &text)?;
    Ok(bgt::latin1_encode(&text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::{bgt, Cal, Field, Payment, END_MODE_INDEFINITE, END_MODE_DATE, END_MODE_N_PAYS, END_MODE_LOAN_PAID, FLAG_PAID, FLAG_NONE};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn unchanged_legacy_flags_remain_editable_in_standalone_documents() {
        let mut db = db(false);
        db.payments[0].flags |= 1024;
        db.payments[0].flag_value = '?';
        let mut p = db.payments[0].clone();
        let id = p.id;
        p.description = "Updated legacy description".into();
        assert_eq!(put_payment(&mut db, p, Some(id)).unwrap(), id);
        assert_eq!(db.payments[0].flags & 1024, 1024);
        assert_eq!(db.payments[0].flag_value, '?');
    }

    #[test]
    fn public_boundary_refuses_new_tentative_loans_without_mutation() {
        let mut db = db(false);
        let before = db.to_text(None).unwrap();
        let mut p = Payment::new(20261001);
        p.description = "Loan".into(); p.amount = -100.0; p.loan_amount = 250.0;
        p.set_end_mode(END_MODE_LOAN_PAID); p.set_tentative(true);
        assert!(put_payment(&mut db, p, None).is_err());
        assert_eq!(db.to_text(None).unwrap(), before);
    }

    #[test]
    fn public_payment_boundary_rejects_invalid_schedule_without_mutation() {
        for fault in 0..5 {
            let mut db = db(false);
            let before = db.to_text(None).unwrap();
            let mut p = db.payments[0].clone();
            let id = p.id;
            match fault {
                0 => p.start_date = 20260230,
                1 => p.frequency = 0,
                2 => p.freq_type = '?',
                3 => p.flag_value = '?',
                _ => p.flags |= 1024,
            }
            assert!(put_payment(&mut db, p, Some(id)).is_err(), "fault {fault}");
            assert_eq!(db.to_text(None).unwrap(), before);
        }
    }

    fn db(sticky: bool) -> BudgetDb {
        let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
        let mut p = Payment::new(20261001);
        p.description = "Monthly bill".into();
        p.amount = -100.0;
        p.set_end_mode(END_MODE_INDEFINITE);
        p.set_sticky(sticky);
        db.add(p);
        db
    }
    fn amount(db: &mut BudgetDb, date: i32) -> Option<f64> {
        db.get_day(&Cal::from_int(date)).unwrap().transactions.first().map(|t| t.amount)
    }
    fn temp_file() -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/document-tests").join(format!("budget-document-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.bgt")
    }

    /// The payment editor holds a copy for as long as it is open, and a date rollover can rebuild
    /// (and move) the window meanwhile; applying that copy must not use its stale day indexes.
    #[test]
    fn an_edit_copied_before_the_window_moved_applies_to_the_current_window() {
        let mut db = BudgetDb::new(&Cal::from_int(20260301), 24, false);
        let mut rent = Payment::new(20250115);
        rent.description = "Rent".into();
        rent.amount = -1000.0;
        rent.set_end_mode(END_MODE_INDEFINITE);
        let i = db.add(rent);
        let id = db.payments[i].id;
        db.adjustments.set_custom_amount(20251115, 5000.0);
        db.adjustments.set_custom_amount(20260110, 5000.0);
        db.rebuild_db(&Cal::from_int(20260301), 24, false);
        assert_eq!(db.start_date.get(), 20251101);
        let mut edited = db.payments[0].clone(); // what an open payment editor holds
        db.rebuild_db(&Cal::from_int(20260401), 24, false); // midnight moves the view to April
        assert_eq!(db.start_date.get(), 20260101, "the window shrank, so the copy's day indexes are stale");
        edited.amount = -1100.0;
        put_payment(&mut db, edited, Some(id)).unwrap();
        for date in [20260115, 20260415, 20281215] {
            let day = db.get_day(&Cal::from_int(date)).unwrap();
            assert_eq!(day.transactions.iter().map(|t| t.amount).collect::<Vec<_>>(), [-1100.0], "{date}");
            assert_eq!(day.regulars, [0], "{date}");
        }
    }

    /// A deleted tentative occurrence slides to today with the schedule; it must stay restorable
    /// there after the date changes (the stored deletion keeps its original date).
    #[test]
    fn a_deleted_tentative_occurrence_can_be_restored_after_the_date_changes() {
        let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
        db.today = 20261027;
        let mut p = Payment::new(20260901);
        p.description = "Groceries".into();
        p.amount = -300.0;
        p.set_end_mode(END_MODE_INDEFINITE);
        p.set_tentative(true);
        let i = db.add(p);
        let id = db.payments[i].id;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut db, 20261027), Some(-300.0));
        delete(&mut db, id, Some(20261027)).unwrap();

        db.today = 20261028; // the next day, by rollover or by reopening the budget
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(db.get_day(&Cal::from_int(20261028)).unwrap().restorable, [i], "offered on today");
        restore(&mut db, id, 20261028).unwrap();
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut db, 20261028), Some(-300.0), "the occurrence is back");
        assert!(db.payments[i].opt.iter().all(|o| !o.deleted), "no deletion is left in the period");
        // Outside any deleted period there is still nothing to restore.
        assert!(restore(&mut db, id, 20261128).is_err());
    }

    #[test]
    fn add_edit_delete_payment_rebuilds_schedule_and_survives_roundtrip() {
        let mut db = db(false);
        let mut p = db.payments[0].clone();
        p.description = "New bill".into(); p.amount = -25.0;
        let id = put_payment(&mut db, p, None).unwrap();
        assert_eq!(db.size(), 2);
        let i = db.get_by_id(id).unwrap();
        let mut edited = db.payments[i].clone(); edited.amount = -40.0;
        put_payment(&mut db, edited, Some(id)).unwrap();
        let day = db.get_day(&Cal::from_int(20261001)).unwrap();
        assert!(day.transactions.iter().any(|t| t.amount == -40.0));
        let text = db.to_text(None).unwrap();
        let loaded = bgt::load_str(&text, 20261027, None).unwrap();
        assert_eq!(loaded.payments.len(), 2);
        assert_eq!(loaded.payments[1].amount, -40.0);
        delete(&mut db, id, None).unwrap();
        assert_eq!(db.size(), 1);
        assert_eq!(db.get_day(&Cal::from_int(20261001)).unwrap().transactions.len(), 1);
    }

    #[test]
    fn one_occurrence_edit_and_delete_leave_other_dates_intact() {
        let mut db = db(false); let id = db.payments[0].id;
        change_transaction(&mut db, id, 20261001, -35.0, "Adjusted", FLAG_PAID).unwrap();
        assert_eq!(amount(&mut db, 20261001), Some(-35.0));
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
        assert_eq!(db.payments[0].get_note(20261001).as_deref(), Some("Adjusted"));
        assert_eq!(db.payments[0].get_flag(20261001), FLAG_PAID);
        delete(&mut db, id, Some(20261001)).unwrap();
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
    }

    #[test]
    fn one_occurrence_edit_does_not_spread_through_sticky_payment() {
        let mut db = db(true); let id = db.payments[0].id;
        change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap();
        assert_eq!(amount(&mut db, 20261001), Some(-35.0));
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
        assert_eq!(amount(&mut db, 20261201), Some(-100.0));
    }

    /// The note on `date` from whichever record of a split payment holds it.
    fn note_on(db: &BudgetDb, date: i32) -> Option<String> { db.payments.iter().find_map(|p| p.get_note(date)) }

    #[test]
    fn future_amount_splits_the_payment_keeping_history_and_replacing_later_exceptions_roundtrip() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260801; p.category = Some("Household".into());
        p.set_custom_amount(20260901, -75.0);
        p.set_no_transaction(20261201);
        p.set_custom_amount(20300101, -12.0);
        p.set_note(20300101, Some("Future note"));
        p.set_flag(20300101, FLAG_PAID); p.set_hilighting(20260901, 123);
        p.set_default_highlighting(0x445566); p.set_url(Some("https://example.com/rent"));
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "Selected only", FLAG_PAID).unwrap();
        assert_eq!(amount(&mut db, 20260801), Some(-100.0));
        assert_eq!(amount(&mut db, 20260901), Some(-75.0));
        assert_eq!(amount(&mut db, 20261001), Some(-250.0));
        assert_eq!(amount(&mut db, 20261101), Some(-250.0));
        assert_eq!(amount(&mut db, 20261201), None);
        // 04 §18: the record ends the day before; a clone with the new amount starts on the date.
        assert_eq!(db.payments.len(), 2);
        let (earlier, later) = (&db.payments[0], &db.payments[1]);
        assert_eq!((earlier.id, earlier.start_date, earlier.end_mode(), earlier.end_date, earlier.amount), (id, 20260801, END_MODE_DATE, 20260930, -100.0));
        assert_eq!((later.start_date, later.end_mode(), later.amount), (20261001, END_MODE_INDEFINITE, -250.0));
        assert_ne!(later.id, id);
        for p in [earlier, later] {
            assert_eq!((p.description.as_str(), p.category.as_deref()), ("Monthly bill", Some("Household")));
            assert_eq!(p.default_hilighting(), 0x445566);
            assert_eq!(p.get_url().as_deref(), Some("https://example.com/rent"));
        }
        // History stays as it was (no dated amounts added); later dated data moves to the clone.
        assert_eq!(earlier.opt, vec![budget_core::PaymentOption { date: 20260901, amount: -75.0, deleted: false }]);
        assert_eq!(earlier.get_hilighting(20260901), 123);
        assert!(later.option_by_date(20261201).is_some_and(|o| o.deleted));
        assert!(later.option_by_date(20300101).is_none(), "a later exception on a scheduled date follows the new amount");
        assert_eq!(later.get_note(20300101).as_deref(), Some("Future note"));
        assert_eq!(later.get_flag(20300101), FLAG_PAID);
        let text = db.to_text(None).unwrap();
        let loaded = bgt::load_str(&text, 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20300101), 12, true);
        assert_eq!(amount(&mut reopened, 20260801), Some(-100.0));
        assert_eq!(amount(&mut reopened, 20260901), Some(-75.0));
        assert_eq!(amount(&mut reopened, 20300101), Some(-250.0));
        assert_eq!(amount(&mut reopened, 20300201), Some(-250.0));
        assert_eq!(note_on(&reopened, 20261001).as_deref(), Some("Selected only"));
        assert_eq!(note_on(&reopened, 20261101), None);
    }

    #[test]
    fn future_amount_does_not_turn_past_months_into_adjusted_transactions() {
        // Regression: the single-record version froze every past occurrence as a dated amount,
        // so all of them showed as deviations from plan (adjusted) and lost the planned amount.
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].start_date = 20251001;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261101, -250.0, "", FLAG_NONE).unwrap();
        assert!(db.payments.iter().all(|p| p.opt.is_empty()), "{:?}", db.payments.iter().map(|p| &p.opt).collect::<Vec<_>>());
        let mut month = Cal::from_int(20251001);
        while month.get() < 20261001 {
            let day = db.get_day(&month).unwrap().transactions[0].clone();
            assert_eq!((day.amount, day.suffix), (-100.0, budget_core::SUFFIX_NONE), "{}", month.get());
            month.add(Field::Month, 1);
        }
        assert_eq!(amount(&mut db, 20261001), Some(-100.0));
        let next = db.get_day(&Cal::from_int(20261201)).unwrap().transactions[0].clone();
        assert_eq!((next.amount, next.suffix), (-250.0, budget_core::SUFFIX_NONE));
        // Planned amounts per record: history keeps 100, the future plans 250.
        assert_eq!((db.payments[0].amount, db.payments[1].amount), (-100.0, -250.0));
    }

    /// Each day's amount of the (possibly split) payment over `[from, to]`, with full history.
    fn daily_amounts(db: &mut BudgetDb, from: i32, to: i32) -> Vec<(i32, Option<f64>)> {
        db.rebuild_db(&Cal::from_int(to), 0, true);
        let mut day = Cal::from_int(from);
        let mut out = Vec::new();
        while day.get() <= to { out.push((day.get(), amount(db, day.get()))); day.add_days(1); }
        out
    }

    #[test]
    fn future_split_on_a_shortened_month_end_or_fifth_weekday_keeps_every_later_date() {
        // A record starting on 30 April would repeat on the 30th, one from 28 Feb 2025 never on the
        // 29th, one from a fourth Friday never on a fifth; the split must keep the original dates.
        for (start, freq_type, selected, anchor) in [(20260131, 'm', 20260430, 20260531),
            (20240229, 'y', 20250228, 20280229), (20260130, 'W', 20260227, 20260529)] {
            let mut db = db(false); db.today = 20260101;
            let id = db.payments[0].id;
            db.payments[0].start_date = start; db.payments[0].freq_type = freq_type;
            let before = daily_amounts(&mut db, start, 20341231);
            assert!(before.iter().any(|&(date, value)| date == selected && value.is_some()));
            change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
            let after = daily_amounts(&mut db, start, 20341231);
            for ((date, old), (_, new)) in before.iter().zip(&after) {
                assert_eq!(*new, if *date < selected { *old } else { old.map(|_| -250.0) }, "{freq_type} {date}");
            }
            let later = &db.payments[1];
            assert_eq!(later.start_date, anchor, "{freq_type}");
            // Only the shortened occurrences before the anchor are dated amounts.
            assert!(!later.opt.is_empty() && later.opt.iter().all(|o| (selected..anchor).contains(&o.date) && o.amount == -250.0), "{:?}", later.opt);
        }
    }

    #[test]
    fn future_split_of_a_counted_month_end_schedule_keeps_its_dates_and_count() {
        // (start, every n months, count, selected): the anchor day recurs within the count, never
        // (every two months from 31 Dec), or only the selected occurrence is left.
        for (start, frequency, count, selected) in [(20260131, 1, 5, 20260228), (20251231, 2, 4, 20260228), (20260131, 1, 4, 20260430)] {
            let mut db = db(false); db.today = 20260101;
            let id = db.payments[0].id;
            let p = &mut db.payments[0];
            p.start_date = start; p.frequency = frequency; p.set_end_mode(END_MODE_N_PAYS); p.n_pays = count;
            let before = daily_amounts(&mut db, start, 20281231);
            change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
            let after = daily_amounts(&mut db, start, 20281231);
            for ((date, old), (_, new)) in before.iter().zip(&after) {
                assert_eq!(*new, if *date < selected { *old } else { old.map(|_| -250.0) }, "{start} every {frequency}: {date}");
            }
            assert_eq!(after.iter().filter(|(_, v)| v.is_some()).count(), count as usize);
        }
    }

    #[test]
    fn future_split_from_a_moved_occurrence_moves_with_it() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].start_date = 20260901;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert!(move_transaction(&mut db, id, 20261001, 20261009, -100.0).unwrap());
        change_future_transactions(&mut db, id, 20261009, -250.0, "Moved one", FLAG_NONE).unwrap();
        for (date, expected) in [(20260901, Some(-100.0)), (20261001, None), (20261009, Some(-250.0)), (20261101, Some(-250.0))] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        assert_eq!(note_on(&db, 20261009).as_deref(), Some("Moved one"));
    }

    #[test]
    fn future_split_from_a_weekend_shifted_occurrence_includes_it() {
        // Sunday 1 Nov 2026 is shown on Monday 2 Nov, or Friday 30 Oct with early pay.
        for early in [false, true] {
            let mut db = db(false); let id = db.payments[0].id;
            db.payments[0].set_no_weekends(true); db.payments[0].set_early_pay(early);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            let shown = if early { 20261030 } else { 20261102 };
            assert_eq!(amount(&mut db, shown), Some(-100.0));
            change_future_transactions(&mut db, id, shown, -250.0, "", FLAG_NONE).unwrap();
            assert_eq!(amount(&mut db, 20261001), Some(-100.0));
            assert_eq!(amount(&mut db, shown), Some(-250.0));
            assert_eq!(amount(&mut db, 20261201), Some(-250.0));
            assert_eq!(db.payments[1].start_date, 20261101);
            assert!(db.payments.iter().all(|p| p.opt.is_empty()));
        }
    }

    #[test]
    fn future_split_keeps_later_deleted_and_moved_occurrences_on_their_dates() {
        let mut db = db(false); let id = db.payments[0].id;
        delete(&mut db, id, Some(20261201)).unwrap();
        assert!(move_transaction(&mut db, id, 20270101, 20270105, -100.0).unwrap());
        change_future_transactions(&mut db, id, 20261101, -250.0, "", FLAG_NONE).unwrap();
        for (date, expected) in [(20261001, Some(-100.0)), (20261101, Some(-250.0)), (20261201, None),
            (20270101, None), (20270105, Some(-250.0)), (20270201, Some(-250.0))] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        // The skipped date can still be restored, now on the record that holds it.
        let later = db.payments[1].id;
        restore(&mut db, later, 20261201).unwrap();
        assert_eq!(amount(&mut db, 20261201), Some(-250.0));
    }

    #[test]
    fn future_split_counts_remaining_payments_and_keeps_the_end_date() {
        for mode in [END_MODE_N_PAYS, END_MODE_DATE] {
            let mut db = db(false); let id = db.payments[0].id;
            let p = &mut db.payments[0];
            p.start_date = 20260601; p.set_end_mode(mode); p.n_pays = 12; p.end_date = 20270501;
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            change_future_transactions(&mut db, id, 20261101, -250.0, "", FLAG_NONE).unwrap();
            let (earlier, later) = (&db.payments[0], &db.payments[1]);
            // A count limit keeps counting (5 made, 7 left); a dated end ends the day before.
            assert_eq!((earlier.end_mode(), earlier.n_pays, earlier.end_date), (mode, 5, if mode == END_MODE_N_PAYS { 20261001 } else { 20261031 }));
            assert_eq!((later.start_date, later.end_mode(), later.n_pays, later.end_date), (20261101, mode, 7, 20270501), "{mode}");
            assert_eq!(amount(&mut db, 20270501), Some(-250.0));
            assert_eq!(amount(&mut db, 20270601), None);
        }
    }

    #[test]
    fn editing_a_payment_from_a_date_onward_splits_it_and_keeps_earlier_months() {
        // 04 §2 "Change the FUTURE transactions only": adjusted transactions are not affected.
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260601;
        p.set_custom_amount(20260801, -80.0);
        p.set_custom_amount(20270301, -60.0);
        p.set_note(20270301, Some("Later note"));
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        let mut edited = db.payments[0].clone();
        edited.amount = -120.0; edited.description = "Monthly bill (new rate)".into();
        let new_id = apply_payment(&mut db, edited, Some(id), Some(20261101)).unwrap();
        assert_ne!(new_id, id);
        for (date, expected) in [(20260601, -100.0), (20260801, -80.0), (20261001, -100.0), (20261101, -120.0),
            (20270301, -60.0), (20270401, -120.0)] {
            assert_eq!(amount(&mut db, date), Some(expected), "{date}");
        }
        let (earlier, later) = (&db.payments[0], &db.payments[1]);
        assert_eq!((earlier.id, earlier.description.as_str(), earlier.amount), (id, "Monthly bill", -100.0));
        assert_eq!((later.id, later.description.as_str(), later.start_date), (new_id, "Monthly bill (new rate)", 20261101));
        assert_eq!(later.get_note(20270301).as_deref(), Some("Later note"));
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, true);
        assert_eq!(amount(&mut reopened, 20261001), Some(-100.0));
        assert_eq!(amount(&mut reopened, 20261201), Some(-120.0));
        // Applied to all occurrences instead, the single record changes from its start.
        let mut db = self::db(false); let id = db.payments[0].id;
        let mut edited = db.payments[0].clone(); edited.amount = -120.0;
        assert_eq!(apply_payment(&mut db, edited, Some(id), None).unwrap(), id);
        assert_eq!((db.payments.len(), amount(&mut db, 20261001)), (1, Some(-120.0)));
    }

    #[test]
    fn future_already_shows_is_false_while_a_later_occurrence_differs() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].start_date = 20260601;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        let shown = amount(&mut db, 20261101).unwrap();
        assert!(future_already_shows(&mut db, id, 20261101, shown));
        assert!(!future_already_shows(&mut db, id, 20261101, shown - 20.0), "later occurrences would be rewritten");
        // a one-off amount on this date, with the regular amount after it
        db.payments[0].set_custom_amount(20261101, shown - 20.0);
        db.update_payment(0);
        assert!(!future_already_shows(&mut db, id, 20261101, shown - 20.0), "December onward still shows the regular amount");
    }

    #[test]
    fn an_untouched_payment_is_unchanged_and_any_edit_is_not() {
        let db = db(false);
        let id = db.payments[0].id;
        let mut p = db.payments[0].clone();
        assert!(payment_unchanged(&db, id, &p));
        p.amount -= 1.0;
        assert!(!payment_unchanged(&db, id, &p));
        assert!(!payment_unchanged(&db, id + 99, &db.payments[0].clone()));
    }

    #[test]
    fn an_untouched_transaction_is_unchanged_and_any_edit_is_not() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        let shown = amount(&mut db, 20261001).unwrap();
        let flag = db.payments[0].get_flag(20261001);
        assert!(transaction_unchanged(&mut db, id, 20261001, shown, "", flag));
        assert!(!transaction_unchanged(&mut db, id, 20261001, shown - 1.0, "", flag));
        assert!(!transaction_unchanged(&mut db, id, 20261001, shown, "Note", flag));
        assert!(!transaction_unchanged(&mut db, id, 20261001, shown, "", FLAG_PAID));
        // an occurrence that does not exist is never "unchanged": the apply reports it
        assert!(!transaction_unchanged(&mut db, id, 20261002, shown, "", flag));
    }

    #[test]
    fn a_split_cannot_make_one_record_a_category_of_the_other() {
        for (description, category) in [("Fitness club", "Monthly bill"), ("Fitness", "Other")] {
            let mut db = db(false); db.today = 20261027;
            let id = db.payments[0].id;
            db.payments[0].start_date = 20260601;
            db.payments[0].category = Some("Fitness".into());
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            let before = db.to_text(None).unwrap();
            let mut edited = db.payments[0].clone();
            edited.description = description.into(); edited.category = Some(category.into());
            let err = apply_payment(&mut db, edited.clone(), Some(id), Some(20261101)).unwrap_err();
            assert!(err.contains("earlier transactions"), "{err}");
            assert_eq!(db.to_text(None).unwrap(), before);
            // for every occurrence it is one record, so the rename is fine
            assert!(apply_payment(&mut db, edited, Some(id), None).is_ok());
        }
    }

    #[test]
    fn editing_a_payment_schedule_from_a_date_onward_follows_the_new_schedule() {
        // Twelve monthly payments from June; from November every two months, still twelve in all.
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260601; p.set_end_mode(END_MODE_N_PAYS); p.n_pays = 12;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        let mut edited = db.payments[0].clone(); edited.frequency = 2;
        apply_payment(&mut db, edited, Some(id), Some(20261101)).unwrap();
        let shown: Vec<_> = daily_amounts(&mut db, 20260601, 20281231).into_iter().filter(|(_, v)| v.is_some()).map(|(d, _)| d).collect();
        assert_eq!(shown, [20260601, 20260701, 20260801, 20260901, 20261001, 20261201, 20270201, 20270401,
            20270601, 20270801, 20271001, 20271201]);
        // Ending before the date leaves nothing to continue: refused without changes.
        let mut db = self::db(false); let id = db.payments[0].id;
        db.payments[0].start_date = 20260601;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        let before = db.to_text(None).unwrap(); let updates = db.update_count;
        let mut edited = db.payments[0].clone(); edited.set_end_mode(END_MODE_DATE); edited.end_date = 20260915;
        assert!(apply_payment(&mut db, edited, Some(id), Some(20261101)).unwrap_err().contains("no occurrences"));
        assert_eq!((db.to_text(None).unwrap(), db.update_count), (before, updates));
    }

    #[test]
    fn editing_a_loan_from_a_date_onward_continues_with_the_unpaid_principal() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0]; p.start_date = 20260801;
        p.set_end_mode(END_MODE_LOAN_PAID); p.loan_amount = 1000.0;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        let mut edited = db.payments[0].clone(); edited.amount = -300.0;
        apply_payment(&mut db, edited, Some(id), Some(20261001)).unwrap();
        assert_eq!(db.payments[1].loan_amount, 800.0);
        for (date, expected) in [(20260801, Some(-100.0)), (20260901, Some(-100.0)), (20261001, Some(-300.0)),
            (20261101, Some(-300.0)), (20261201, Some(-200.0)), (20270101, None)] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
    }

    #[test]
    fn editing_from_the_first_occurrence_or_without_one_changes_every_occurrence() {
        let mut db = db(false); let id = db.payments[0].id;
        assert!(!splits_at(&db.payments[0], 20261001));
        assert!(splits_at(&db.payments[0], 20261101));
        let mut edited = db.payments[0].clone(); edited.amount = -120.0;
        assert_eq!(apply_payment(&mut db, edited, Some(id), Some(20261001)).unwrap(), id);
        assert_eq!(db.payments.len(), 1);
        assert_eq!(amount(&mut db, 20261101), Some(-120.0));
    }

    #[test]
    fn future_amount_resets_sticky_carry_without_losing_old_custom_amounts_and_moves() {
        let mut db = db(true); let id = db.payments[0].id;
        let p = &mut db.payments[0]; p.start_date = 20260701;
        p.set_custom_amount(20260801, -75.0);
        p.set_custom_amount(20261001, -45.0);
        p.set_no_transaction(20261101); p.set_custom_amount(20261104, -45.0);
        p.set_custom_amount(20300704, -12.0);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).unwrap();
        for (date, expected) in [(20260701, Some(-100.0)), (20260801, Some(-75.0)), (20260901, Some(-75.0)),
            (20261001, Some(-250.0)), (20261101, None), (20261104, Some(-250.0)), (20261201, Some(-250.0))] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        // The moved and extra later occurrences keep their dates on the new record.
        assert_eq!(db.payments[1].option_by_date(20300704).unwrap().amount, -250.0);
        assert_eq!(db.payments[1].option_by_date(20261104).unwrap().amount, -250.0);
        assert!(db.payments[0].opt.iter().all(|o| o.date < 20261001));
    }

    #[test]
    fn future_weekly_groceries_150_to_250_keeps_the_previous_year_unchanged() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.description = "Groceries".into(); p.start_date = 20251024;
        p.freq_type = 'w'; p.frequency = 1; p.amount = -150.0;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261023, -250.0, "Weekly budget", FLAG_NONE).unwrap();
        for date in [20251024, 20251226, 20261009, 20261016] { assert_eq!(amount(&mut db, date), Some(-150.0)); }
        for date in [20261023, 20261030, 20261106, 20270101] { assert_eq!(amount(&mut db, date), Some(-250.0)); }
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20301001), 0, true);
        assert_eq!(amount(&mut reopened, 20251024), Some(-150.0));
        assert_eq!(amount(&mut reopened, 20261016), Some(-150.0));
        assert_eq!(amount(&mut reopened, 20301025), Some(-250.0));
    }

    #[test]
    fn future_daily_weekend_groups_preserve_the_entire_actual_history() {
        for sticky in [false, true] {
            for early in [false, true] {
                for selected in [20261002, 20261005, 20261006] {
                    let mut db = db(sticky); let id = db.payments[0].id;
                    let p = &mut db.payments[0]; p.start_date = 20260925;
                    p.freq_type = 'd'; p.set_no_weekends(true); p.set_early_pay(early);
                    p.set_custom_amount(20260930, -70.0);
                    p.set_custom_amount(20261004, -80.0);
                    p.set_no_transaction(20261009);
                    db.rebuild_db(&Cal::from_int(20261001), 12, false);
                    let mut day = Cal::from_int(20260924); let mut previous = Vec::new();
                    while day.get() < selected { previous.push((day.get(), amount(&mut db, day.get()))); day.add_days(1); }
                    change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
                    for (date, value) in previous { assert_eq!(amount(&mut db, date), value, "sticky={sticky}, early={early}, selected={selected}, date={date}"); }
                    // Saturday and Sunday occurrences are added to the business day they move to
                    // (Monday, or Friday with early pay), so a Monday shows three daily amounts.
                    let monday = if early { -250.0 } else { -750.0 };
                    assert_eq!(amount(&mut db, selected), Some(-250.0));
                    assert_eq!(amount(&mut db, 20261012), Some(monday));
                    for date in [20261013, 20261014] { assert_eq!(amount(&mut db, date), Some(-250.0)); }
                    assert_eq!(amount(&mut db, 20261009), None);
                    let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
                    let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, true);
                    assert_eq!(amount(&mut reopened, selected), Some(-250.0));
                    assert_eq!(amount(&mut reopened, 20261102), Some(monday));
                    if early { assert_eq!(amount(&mut reopened, 20261030), Some(-750.0)); }
                }
            }
        }
    }

    #[test]
    fn future_tentative_preserves_empty_past_current_period_and_disabled_carry() {
        for sticky in [false, true] {
            for disabled in [false, true] {
                for selected in [20260903, 20261027, 20261101] {
                    let mut db = db(sticky); db.today = 20261027; let id = db.payments[0].id;
                    let p = &mut db.payments[0]; p.start_date = 20260801;
                    p.set_tentative(true); p.disabled = disabled;
                    p.set_custom_amount(20260903, -75.0);
                    p.set_no_transaction(20261001);
                    p.set_custom_amount(20261203, -40.0);
                    p.set_note(20261203, Some("Keep December"));
                    db.rebuild_db(&Cal::from_int(20261001), 12, false);
                    let history: Vec<_> = [20260801, 20260901, 20260903, 20261001, 20261027]
                        .into_iter().filter(|date| *date < selected).map(|date| (date, amount(&mut db, date))).collect();
                    // A current tentative period consisting solely of a deletion is skipped
                    // unless disabled sticky scheduling explicitly retains its estimate.
                    if amount(&mut db, selected).is_none() { continue; }
                    change_future_transactions(&mut db, id, selected, -250.0, "Selected note", FLAG_PAID).unwrap();
                    for (date, value) in history { assert_eq!(amount(&mut db, date), value, "sticky={sticky}, disabled={disabled}, selected={selected}, date={date}"); }
                    assert_eq!(amount(&mut db, selected), Some(-250.0));
                    assert_eq!(amount(&mut db, 20261203), Some(-250.0));
                    assert_eq!(amount(&mut db, 20270101), Some(-250.0));
                    assert_eq!(db.payments.len(), 2, "the pay period holding the date starts the new record");
                    assert_eq!(note_on(&db, 20261203).as_deref(), Some("Keep December"));
                    assert_eq!(note_on(&db, selected).as_deref(), Some("Selected note"));
                    let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
                    let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, true);
                    reopened.today = 20261027; reopened.rebuild_db(&Cal::from_int(20261001), 12, true);
                    assert_eq!(amount(&mut reopened, 20260801), None);
                    assert_eq!(amount(&mut reopened, selected), Some(-250.0));
                }
            }
        }
    }

    #[test]
    fn future_loan_preserves_paid_history_and_recalculates_the_last_payment() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0]; p.start_date = 20260801;
        p.set_end_mode(budget_core::END_MODE_LOAN_PAID); p.loan_amount = 1000.0;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).unwrap();
        for (date, expected) in [(20260801, Some(-100.0)), (20260901, Some(-100.0)), (20261001, Some(-250.0)),
            (20261101, Some(-250.0)), (20261201, Some(-250.0)), (20270101, Some(-50.0)), (20270201, None)] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, true);
        assert_eq!(amount(&mut reopened, 20260901), Some(-100.0));
        assert_eq!(amount(&mut reopened, 20270101), Some(-50.0));
    }

    #[test]
    fn future_loan_rejects_overpayment_from_historical_selected_and_future_options_atomically() {
        // 1000 principal, 200 already paid. A moved later payment (an extra date) takes the new
        // amount, so 750 twice exceeds the 800 left; 900 alone also exceeds it.
        for (future_exception, value) in [(Some(-600.0), -750.0), (None, -900.0)] {
            let mut db = db(false); let id = db.payments[0].id;
            let p = &mut db.payments[0]; p.start_date = 20260801;
            p.set_end_mode(budget_core::END_MODE_LOAN_PAID); p.loan_amount = 1000.0;
            if let Some(value) = future_exception { p.set_custom_amount(20261215, value); }
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            let before = db.to_text(None).unwrap(); let updates = db.update_count;
            let selected_before = amount(&mut db, 20261001);
            let error = change_future_transactions(&mut db, id, 20261001, value, "Do not save", FLAG_PAID).unwrap_err();
            assert!(error.contains("principal"), "{error}");
            assert_eq!(db.to_text(None).unwrap(), before); assert_eq!(db.update_count, updates);
            assert_eq!(amount(&mut db, 20261001), selected_before);
        }
    }

    #[test]
    fn future_amount_errors_leave_saved_content_days_and_update_count_intact() {
        let mut db = db(false); let id = db.payments[0].id;
        let before = db.to_text(None).unwrap(); let updates = db.update_count;
        let old_amount = amount(&mut db, 20261001);
        for (payment_id, date, value) in [(id, 20260230, -250.0), (id, 20261301, -250.0), (id, i32::MAX, -250.0),
            (id, 20261001, f64::NAN), (id, 20261001, f64::INFINITY), (id, 20261002, -250.0), (999, 20261001, -250.0)] {
            assert!(change_future_transactions(&mut db, payment_id, date, value, "Changed", FLAG_PAID).is_err());
            assert_eq!(db.to_text(None).unwrap(), before); assert_eq!(db.update_count, updates);
            assert_eq!(amount(&mut db, 20261001), old_amount);
        }
        for kind in 0..7 {
            let mut db = self::db(false); let id = db.payments[0].id;
            match kind {
                0 => { db.payments[0].add_custom_amount(20261101, -10.0); db.payments[0].add_custom_amount(20261101, -20.0); }
                1 => db.payments[0].frequency = 0,
                2 => db.payments[0].start_date = 20260230,
                3 => db.payments[0].set_custom_amount(20300230, -10.0),
                4 => db.payments[0].set_custom_amount(20300101, f64::NAN),
                5 => { db.payments[0].set_end_mode(budget_core::END_MODE_LOAN_PAID); db.payments[0].loan_amount = 1e20; }
                _ => { db.payments[0].start_date = 10000101; }
            }
            let before = db.to_text(None); let updates = db.update_count;
            assert!(change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).is_err(), "kind={kind}");
            assert_eq!(db.to_text(None), before); assert_eq!(db.update_count, updates);
        }
    }

    #[test]
    fn future_unsupported_loan_changes_are_rejected_before_any_mutation() {
        for (sticky, tentative, value) in [(true, false, -250.0), (false, true, -250.0), (false, false, 0.0), (false, false, 250.0)] {
            let mut db = db(sticky); let id = db.payments[0].id;
            let p = &mut db.payments[0]; p.set_tentative(tentative);
            p.set_end_mode(budget_core::END_MODE_LOAN_PAID); p.loan_amount = 1000.0;
            let before = db.to_text(None).unwrap(); let updates = db.update_count;
            assert!(change_future_transactions(&mut db, id, 20261001, value, "", FLAG_NONE).is_err());
            assert_eq!(db.to_text(None).unwrap(), before); assert_eq!(db.update_count, updates);
        }
    }

    #[test]
    fn future_finite_schedules_keep_their_end_condition_and_do_not_create_occurrences() {
        for mode in [budget_core::END_MODE_DATE, budget_core::END_MODE_N_PAYS] {
            for freq_type in ['m', 'W', 'L'] {
                let mut db = db(true); let id = db.payments[0].id;
                let p = &mut db.payments[0]; p.start_date = 20260828;
                p.freq_type = freq_type; p.set_end_mode(mode); p.n_pays = 6; p.end_date = 20270131;
                db.rebuild_db(&Cal::from_int(20261001), 12, false);
                let end = db.payments[0].end_date; let count = db.payments[0].n_pays;
                let mut day = Cal::from_int(20260801); let mut baseline = Vec::new();
                while day.get() < 20270301 { baseline.push((day.get(), amount(&mut db, day.get()))); day.add_days(1); }
                let selected = baseline.iter().find(|(date, value)| *date >= 20261001 && value.is_some()).unwrap().0;
                change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
                for (date, value) in baseline {
                    assert_eq!(amount(&mut db, date), if date < selected { value } else { value.map(|_| -250.0) }, "mode={mode}, frequency={freq_type}, date={date}");
                }
                // The new record continues the end condition; together both make the same count.
                let (earlier, later) = (&db.payments[0], &db.payments[1]);
                assert_eq!((later.start_date, later.end_mode(), later.end_date), (selected, mode, end), "frequency={freq_type}");
                assert_eq!(earlier.end_mode(), mode);
                assert_eq!(earlier.n_pays + later.n_pays, count, "mode={mode}, frequency={freq_type}");
                assert!(earlier.end_date < selected);
            }
        }
    }

    #[test]
    fn future_amount_queries_history_outside_the_visible_window_and_supports_later_changes() {
        let mut db = db(true); let id = db.payments[0].id;
        db.update_adjustment(&Cal::from_int(20290101), 500.0);
        db.rebuild_db(&Cal::from_int(20290601), 0, false);
        assert!(db.get_day(&Cal::from_int(20261001)).is_none());
        change_future_transactions(&mut db, id, 20261101, -250.0, "First change", FLAG_NONE).unwrap();
        // The January occurrence now belongs to the record continuing from November.
        assert!(change_future_transactions(&mut db, id, 20270101, -300.0, "", FLAG_NONE).is_err());
        let november = db.payments.iter().find(|p| p.start_date == 20261101).unwrap().id;
        change_future_transactions(&mut db, november, 20270101, -300.0, "Second change", FLAG_PAID).unwrap();
        db.rebuild_db(&Cal::from_int(20301001), 0, true);
        assert_eq!(amount(&mut db, 20261001), Some(-100.0));
        assert_eq!(amount(&mut db, 20261101), Some(-250.0));
        assert_eq!(amount(&mut db, 20261201), Some(-250.0));
        assert_eq!(amount(&mut db, 20270101), Some(-300.0));
        assert_eq!(amount(&mut db, 20301001), Some(-300.0));
        assert_eq!(note_on(&db, 20261101).as_deref(), Some("First change"));
        assert_eq!(note_on(&db, 20270101).as_deref(), Some("Second change"));
        let starts: Vec<_> = db.payments.iter().map(|p| (p.start_date, p.amount)).collect();
        assert_eq!(starts, [(20261001, -100.0), (20261101, -250.0), (20270101, -300.0)]);
        assert!(db.payments.iter().all(|p| p.opt.is_empty()), "no dated amounts are needed");
    }

    #[test]
    fn changing_schedule_keeps_existing_exception_metadata_for_saving() {
        let mut db = db(false);
        let mut p = db.payments[0].clone();
        p.set_no_transaction(20261001);
        p.set_note(20261001, Some("Existing note"));
        p.set_flag(20261001, FLAG_PAID);
        p.start_date = 20261002;
        let options = p.opt.clone();
        let notes = p.note.clone();
        let flags = p.flag.clone();
        let id = p.id;
        put_payment(&mut db, p, Some(id)).unwrap();
        assert_eq!(db.payments[0].opt, options);
        assert_eq!(db.payments[0].note, notes);
        assert_eq!(db.payments[0].flag, flags);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(db.payments[0].opt, options);
    }

    #[test]
    fn sticky_weekend_occurrence_change_keeps_following_shifted_payment_amount() {
        let mut db = db(true);
        let id = db.payments[0].id;
        db.payments[0].set_no_weekends(true);
        db.update_payment(0);
        change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap();
        assert_eq!(amount(&mut db, 20261001), Some(-35.0));
        assert_eq!(amount(&mut db, 20261102), Some(-100.0));
    }

    #[test]
    fn editing_sticky_occurrence_does_not_restore_a_later_skipped_date() {
        for delete_occurrence in [false, true] {
            let mut db = db(true); let id = db.payments[0].id;
            db.payments[0].set_custom_amount(20261001, -150.0);
            db.payments[0].set_no_transaction(20261101);
            db.update_payment(0);
            if delete_occurrence { delete(&mut db, id, Some(20261001)).unwrap(); }
            else { change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap(); }
            assert_eq!(amount(&mut db, 20261101), None);
            assert_eq!(amount(&mut db, 20261201), Some(-150.0));
        }
    }

    #[test]
    fn deleting_sticky_override_leaves_later_amounts_unchanged() {
        let mut db = db(true); let id = db.payments[0].id;
        db.payments[0].set_custom_amount(20261001, -150.0);
        db.update_payment(0);
        assert_eq!(amount(&mut db, 20261101), Some(-150.0));
        delete(&mut db, id, Some(20261001)).unwrap();
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20261101), Some(-150.0));
        assert_eq!(amount(&mut db, 20261201), Some(-150.0));
    }

    #[test]
    fn sticky_shifted_edit_preserves_intermediate_option_and_later_carry() {
        for delete_occurrence in [false, true] {
            let mut db = db(true); let id = db.payments[0].id;
            db.payments[0].set_no_weekends(true);
            db.payments[0].set_custom_amount(20261001, -150.0);
            db.payments[0].set_custom_amount(20261101, -80.0);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            assert_eq!(amount(&mut db, 20261102), Some(-150.0));
            if delete_occurrence { delete(&mut db, id, Some(20261001)).unwrap(); }
            else { change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap(); }
            assert_eq!(amount(&mut db, 20261101), Some(-80.0));
            assert_eq!(amount(&mut db, 20261102), Some(-150.0));
            assert_eq!(amount(&mut db, 20261201), Some(-80.0));
            assert_eq!(amount(&mut db, 20270101), Some(-80.0));
            let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
            let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
            assert_eq!(amount(&mut reopened, 20261102), Some(-150.0));
            assert_eq!(amount(&mut reopened, 20261201), Some(-80.0));
        }
    }

    #[test]
    fn sticky_daily_weekend_group_keeps_last_scheduled_amount() {
        for early in [false, true] {
            let mut db = db(true); let id = db.payments[0].id;
            let p = &mut db.payments[0];
            p.freq_type = 'd'; p.set_no_weekends(true); p.set_early_pay(early);
            p.set_custom_amount(20261004, -80.0);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            let dates = [20261002, 20261003, 20261004, 20261005, 20261006];
            let before: Vec<_> = dates.iter().map(|&date| amount(&mut db, date)).collect();
            change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap();
            for (date, expected) in dates.into_iter().zip(before) {
                assert_eq!(amount(&mut db, date), expected, "date {date}, early {early}");
            }
        }
    }

    #[test]
    fn historical_disabled_tentative_change_preserves_existing_current_occurrence() {
        for delete_occurrence in [false, true] {
            // Check carried, explicitly overridden, and absent (already paid) today.
            for today_mode in 0..3 {
                let mut db = db(true); db.today = 20261027;
                let p = &mut db.payments[0];
                p.start_date = 20260901; p.disabled = true; p.set_tentative(true);
                p.set_custom_amount(20260901, -150.0);
                p.set_no_transaction(20261001);
                if today_mode == 1 { p.set_custom_amount(20261027, -70.0); }
                if today_mode == 2 { p.set_flag(20261001, FLAG_PAID); }
                db.rebuild_db(&Cal::from_int(20261001), 12, false);
                let id = db.payments[0].id;
                let today_amount = amount(&mut db, 20261027);
                let next_amount = amount(&mut db, 20261101);
                assert_eq!(today_amount, match today_mode { 0 => Some(-150.0), 1 => Some(-70.0), _ => None });
                if delete_occurrence { delete(&mut db, id, Some(20260901)).unwrap(); }
                else { change_transaction(&mut db, id, 20260901, -35.0, "", FLAG_NONE).unwrap(); }
                assert_eq!(amount(&mut db, 20261027), today_amount);
                assert_eq!(amount(&mut db, 20261101), next_amount);
                assert!(!db.payments[0].opt.iter().any(|o| o.date > 20261027));
            }
        }
    }

    #[test]
    fn invalid_loan_occurrence_edit_is_rejected_without_mutating_payment() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].set_end_mode(budget_core::END_MODE_LOAN_PAID);
        db.payments[0].loan_amount = 300.0;
        db.update_payment(0);
        let before = db.to_text(None).unwrap();
        // The first input was verified red safely on the unguarded path. The
        // second would never reduce the remaining principal without validation.
        for amount in [10_000_000.0, 1e20] {
            assert!(change_transaction(&mut db, id, 20261001, amount, "rejected", FLAG_PAID).is_err());
            assert_eq!(db.to_text(None).unwrap(), before);
        }
    }

    #[test]
    fn invalid_loan_occurrence_delete_is_rejected_without_mutating_payment() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].set_end_mode(budget_core::END_MODE_LOAN_PAID);
        db.payments[0].loan_amount = 10_000_000.0;
        db.payments[0].set_custom_amount(20261001, -10_000_000.0);
        db.update_payment(0);
        let before = db.to_text(None).unwrap();
        assert!(delete(&mut db, id, Some(20261001)).is_err());
        assert_eq!(db.to_text(None).unwrap(), before);
    }

    #[test]
    fn move_relocates_only_selected_monthly_occurrence_and_metadata_roundtrip() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.set_custom_amount(20261001, -123.45);
        p.set_flag(20261001, FLAG_PAID);
        p.set_note(20261001, Some("Receipt retained"));
        p.set_hilighting(20261001, 0x234567);
        p.set_note(20261201, Some("Unrelated"));
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261001, 20261009, -123.45).unwrap());
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20261009), Some(-123.45));
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
        assert_eq!(db.payments[0].get_note(20261001), None);
        assert_eq!(db.payments[0].get_note(20261009).as_deref(), Some("Receipt retained"));
        assert_eq!(db.payments[0].get_flag(20261009), FLAG_PAID);
        assert_eq!(db.payments[0].get_hilighting(20261009), 0x234567);
        assert_eq!(db.payments[0].get_hilighting(20261001), -1);
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut reopened, 20261001), None);
        assert_eq!(amount(&mut reopened, 20261009), Some(-123.45));
        assert_eq!(reopened.payments[0].get_flag(20261009), FLAG_PAID);
        assert_eq!(reopened.payments[0].get_hilighting(20261009), 0x234567);
        assert_eq!(reopened.payments[0].get_note(20261201).as_deref(), Some("Unrelated"));
    }

    #[test]
    fn move_one_time_transaction_to_adjacent_month() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].set_end_mode(budget_core::END_MODE_N_PAYS);
        db.payments[0].n_pays = 1;
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261001, 20260930, -100.0).unwrap());
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20260930), Some(-100.0));
        assert_eq!(amount(&mut db, 20261101), None);
    }

    /// Moving onto a skipped date keeps the status, note and highlighting that date already holds
    /// unless the moved occurrence sets its own (the original's moveTransaction, analysis/07).
    #[test]
    fn move_onto_a_skipped_date_keeps_the_destination_values_the_occurrence_does_not_set() {
        for default_flag in [FLAG_NONE, budget_core::FLAG_IMPORTANT] {
            let mut db = db(false); let id = db.payments[0].id;
            let p = &mut db.payments[0];
            p.flag_value = default_flag;
            p.set_default_highlighting(0x123456);
            p.set_no_transaction(20261101);
            p.set_flag(20261101, FLAG_PAID);
            p.set_note(20261101, Some("Old skipped transaction"));
            p.set_hilighting(20261101, 0x654321);
            p.set_note(20261201, Some("Unrelated note"));
            db.update_payment(0);
            assert!(move_transaction(&mut db, id, 20261001, 20261101, -100.0).unwrap());
            assert_eq!(amount(&mut db, 20261001), None);
            assert_eq!(amount(&mut db, 20261101), Some(-100.0));
            let moved = &db.payments[0];
            assert_eq!(moved.get_flag(20261101), FLAG_PAID);
            assert_eq!(moved.get_note(20261101).as_deref(), Some("Old skipped transaction"));
            assert_eq!(moved.get_hilighting(20261101), 0x654321);
            assert_eq!(moved.default_hilighting(), 0x123456);
            assert_eq!(moved.get_note(20261201).as_deref(), Some("Unrelated note"));
            let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
            let reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
            assert_eq!(reopened.payments[0].get_flag(20261101), FLAG_PAID);
            assert_eq!(reopened.payments[0].get_note(20261101).as_deref(), Some("Old skipped transaction"));
            assert_eq!(reopened.payments[0].get_hilighting(20261101), 0x654321);
            assert_eq!(reopened.payments[0].default_hilighting(), 0x123456);
        }
    }

    /// The repro: Nov 1 was marked Paid with a note and then deleted; moving Dec 1 onto it must not
    /// erase the note or status the Dec 1 occurrence does not carry.
    #[test]
    fn move_onto_a_deleted_date_keeps_its_note_and_status_when_the_occurrence_has_none() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260101;
        p.set_no_transaction(20261101);
        p.set_flag(20261101, FLAG_PAID);
        p.set_note(20261101, Some("Cancelled, gym closed"));
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261201, 20261101, -100.0).unwrap());
        assert_eq!(amount(&mut db, 20261201), None);
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
        assert_eq!(db.payments[0].get_note(20261101).as_deref(), Some("Cancelled, gym closed"));
        assert_eq!(db.payments[0].get_flag(20261101), FLAG_PAID);
    }

    /// The values the moved occurrence does set replace the destination's, and leave the source.
    #[test]
    fn move_onto_a_deleted_date_takes_the_status_note_and_highlight_the_occurrence_sets() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260101;
        p.set_no_transaction(20261101);
        p.set_flag(20261101, FLAG_PAID);
        p.set_note(20261101, Some("Cancelled, gym closed"));
        p.set_flag(20261201, budget_core::FLAG_IMPORTANT);
        p.set_note(20261201, Some("Moved here"));
        p.set_hilighting(20261201, 0x112233);
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261201, 20261101, -100.0).unwrap());
        let moved = &db.payments[0];
        assert_eq!(moved.get_flag(20261101), budget_core::FLAG_IMPORTANT);
        assert_eq!(moved.get_note(20261101).as_deref(), Some("Moved here"));
        assert_eq!(moved.get_hilighting(20261101), 0x112233);
        assert_eq!(moved.get_note(20261201), None);
        assert_eq!(moved.get_hilighting(20261201), -1);
    }

    #[test]
    fn move_preserves_explicit_none_status_over_a_paid_payment_default() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.flag_value = FLAG_PAID;
        p.set_flag(20261001, FLAG_NONE);
        p.set_no_transaction(20261101);
        p.set_flag(20261101, budget_core::FLAG_IMPORTANT);
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261001, 20261101, -100.0).unwrap());
        assert_eq!(db.payments[0].get_flag(20261101), FLAG_NONE);
        assert!(!db.payments[0].flag.iter().any(|f| f.date == 20261001));
        assert_eq!(db.payments[0].get_flag(20261201), FLAG_PAID);
    }

    #[test]
    fn move_rejects_duplicate_endpoint_metadata_without_losing_legacy_entries() {
        for duplicate_at in [20261001, 20261101] {
            for kind in 0..3 {
                let mut db = db(false); let id = db.payments[0].id;
                let p = &mut db.payments[0];
                p.set_no_transaction(20261101);
                match kind {
                    0 => {
                        p.set_flag(duplicate_at, FLAG_PAID);
                        p.flag.push(budget_core::PaymentFlag { date: duplicate_at, value: budget_core::FLAG_IMPORTANT });
                    }
                    1 => {
                        p.set_note(duplicate_at, Some("First note"));
                        p.note.push(budget_core::Note { date: duplicate_at, note: "Second note".into() });
                    }
                    _ => {
                        p.set_hilighting(duplicate_at, 0x123456);
                        p.hilighting.push(budget_core::Hilight { date: duplicate_at, bg_color: 0x654321 });
                    }
                }
                db.update_payment(0);
                let before = db.to_text(None).unwrap(); let updates = db.update_count;
                assert!(move_transaction(&mut db, id, 20261001, 20261101, -100.0).is_err());
                assert_eq!(db.to_text(None).unwrap(), before);
                assert_eq!(db.update_count, updates);
            }
        }
    }

    #[test]
    fn move_rejects_invalid_stale_and_missing_sources_without_mutation() {
        let mut db = db(false); let id = db.payments[0].id;
        let before = db.to_text(None).unwrap();
        let updates = db.update_count;
        for (payment_id, from, to, value) in [
            (id, 20261001, 20261002, f64::NAN),
            (id, 20261001, 20261002, f64::INFINITY),
            (id, 20260230, 20261002, -100.0),
            (id, 20261001, 20261301, -100.0),
            (id, 20261001, i32::MAX, -100.0),
            (id, 20261001, 99991232, -100.0),
            (id, 20261002, 20261003, -100.0),
            (id, 20261001, 20261003, -99.0),
            (99999, 20261001, 20261003, -100.0),
        ] {
            assert!(move_transaction(&mut db, payment_id, from, to, value).is_err());
            assert_eq!(db.to_text(None).unwrap(), before);
            assert_eq!(db.update_count, updates);
        }
        assert!(!move_transaction(&mut db, id, 20261001, 20261001, -100.0).unwrap());
        assert_eq!(db.to_text(None).unwrap(), before);
        assert_eq!(db.update_count, updates);
    }

    #[test]
    fn move_rejects_scheduled_and_custom_destination_collisions_without_mutation() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].set_custom_amount(20261107, -45.0);
        // A custom option must be protected even if outside the visible window.
        db.payments[0].set_custom_amount(20311107, -45.0);
        db.update_payment(0);
        let before = db.to_text(None).unwrap();
        for destination in [20261101, 20261107, 20311107, 20311101] {
            assert!(move_transaction(&mut db, id, 20261001, destination, -100.0).is_err());
            assert_eq!(db.to_text(None).unwrap(), before);
        }
    }

    #[test]
    fn move_protects_custom_destination_even_when_duplicate_deletions_hide_it() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].add_no_transaction(20261009);
        db.payments[0].add_custom_amount(20261009, -45.0);
        db.payments[0].add_no_transaction(20261009);
        db.update_payment(0);
        assert_eq!(amount(&mut db, 20261009), None);
        let before = db.to_text(None).unwrap();
        assert!(move_transaction(&mut db, id, 20261001, 20261009, -100.0).is_err());
        assert_eq!(db.to_text(None).unwrap(), before);
    }

    #[test]
    fn move_weekend_shifted_source_preserves_daily_business_day_group() {
        for early in [false, true] {
            let mut db = db(true); let id = db.payments[0].id;
            let p = &mut db.payments[0];
            p.freq_type = 'd'; p.set_no_weekends(true); p.set_early_pay(early);
            p.set_custom_amount(20261004, -80.0);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            // The Friday/Monday group is the selected visible occurrence;
            // the destination is an empty weekend date.
            // The group adds up the business day's own occurrence and the weekend ones moved onto it.
            let value = amount(&mut db, if early { 20261002 } else { 20261005 }).unwrap();
            assert!(value < -200.0, "{value}");
            let (source, destination) = if early { (20261002, 20261003) } else { (20261005, 20261003) };
            let after = amount(&mut db, 20261006);
            assert!(move_transaction(&mut db, id, source, destination, value).unwrap());
            assert_eq!(amount(&mut db, source), None);
            assert_eq!(amount(&mut db, destination), Some(value));
            assert_eq!(amount(&mut db, 20261006), after);
        }
    }

    #[test]
    fn move_disabled_sticky_tentative_history_preserves_current_carried_occurrence() {
        let mut db = db(true); db.today = 20261027;
        let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260901; p.disabled = true; p.set_tentative(true);
        p.set_custom_amount(20260901, -150.0);
        p.set_no_transaction(20261001);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut db, 20261027), Some(-150.0));
        assert!(move_transaction(&mut db, id, 20260901, 20260830, -150.0).unwrap());
        assert_eq!(amount(&mut db, 20260901), None);
        assert_eq!(amount(&mut db, 20260830), Some(-150.0));
        assert_eq!(amount(&mut db, 20261027), Some(-150.0));
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
    }

    #[test]
    fn move_queries_source_and_destination_outside_current_window() {
        let mut db = db(false); let id = db.payments[0].id;
        db.update_adjustment(&Cal::from_int(20290101), 500.0);
        db.rebuild_db(&Cal::from_int(20290601), 0, false);
        assert!(db.get_day(&Cal::from_int(20261001)).is_none());
        assert!(move_transaction(&mut db, id, 20261001, 20301004, -100.0).unwrap());
        db.rebuild_db(&Cal::from_int(20301001), 0, true);
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20301004), Some(-100.0));
        assert_eq!(amount(&mut db, 20301101), Some(-100.0));
    }

    #[test]
    fn move_sticky_custom_occurrence_preserves_carry_after_both_endpoints() {
        for (source, destination) in [(20261001, 20261210), (20261201, 20261010)] {
            let mut db = db(true); let id = db.payments[0].id;
            db.payments[0].set_custom_amount(source, -150.0);
            db.update_payment(0);
            assert!(move_transaction(&mut db, id, source, destination, -150.0).unwrap());
            assert_eq!(amount(&mut db, source), None);
            assert_eq!(amount(&mut db, destination), Some(-150.0));
            assert_eq!(amount(&mut db, 20261101), Some(if source == 20261001 { -150.0 } else { -100.0 }));
            assert_eq!(amount(&mut db, 20270101), Some(-150.0));
            assert_eq!(amount(&mut db, 20270201), Some(-150.0));
        }
    }

    #[test]
    fn move_sticky_occurrence_does_not_change_intermediate_custom_options_or_skips() {
        let mut db = db(true); let id = db.payments[0].id;
        db.payments[0].set_custom_amount(20261001, -150.0);
        db.payments[0].set_custom_amount(20261015, -80.0);
        db.payments[0].set_no_transaction(20261101);
        db.payments[0].set_note(20261015, Some("Keep this override"));
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261001, 20261111, -150.0).unwrap());
        assert_eq!(amount(&mut db, 20261015), Some(-80.0));
        assert_eq!(amount(&mut db, 20261101), None);
        assert_eq!(amount(&mut db, 20261111), Some(-150.0));
        assert_eq!(amount(&mut db, 20261201), Some(-80.0));
        assert_eq!(amount(&mut db, 20270101), Some(-80.0));
        assert_eq!(db.payments[0].get_note(20261015).as_deref(), Some("Keep this override"));
    }

    #[test]
    fn move_sticky_weekend_occurrence_keeps_shifted_and_unshifted_carry() {
        let mut db = db(true); let id = db.payments[0].id;
        db.payments[0].set_no_weekends(true);
        db.payments[0].set_custom_amount(20261001, -150.0);
        db.payments[0].set_custom_amount(20261101, -80.0);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert!(move_transaction(&mut db, id, 20261001, 20261210, -150.0).unwrap());
        assert_eq!(amount(&mut db, 20261101), Some(-80.0));
        assert_eq!(amount(&mut db, 20261102), Some(-150.0));
        assert_eq!(amount(&mut db, 20261201), Some(-80.0));
        assert_eq!(amount(&mut db, 20261210), Some(-150.0));
        assert_eq!(amount(&mut db, 20270101), Some(-80.0));
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut reopened, 20270101), Some(-80.0));
    }

    #[test]
    fn move_tentative_keeps_unrelated_future_periods_and_metadata() {
        let mut db = db(true); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].set_tentative(true);
        db.payments[0].set_custom_amount(20261103, -65.0);
        db.payments[0].set_note(20261103, Some("Future period"));
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut db, 20261027), Some(-100.0));
        assert!(move_transaction(&mut db, id, 20261027, 20261029, -100.0).unwrap());
        assert_eq!(amount(&mut db, 20261027), None);
        assert_eq!(amount(&mut db, 20261029), Some(-100.0));
        assert_eq!(amount(&mut db, 20261103), Some(-65.0));
        assert_eq!(amount(&mut db, 20261201), Some(-100.0));
        assert_eq!(db.payments[0].get_note(20261103).as_deref(), Some("Future period"));
    }

    #[test]
    fn move_tentative_across_periods_retains_the_independent_destination_period_occurrence() {
        for sticky in [false, true] {
            for disabled in [false, true] {
                let mut db = db(sticky); db.today = 20261027;
                let id = db.payments[0].id;
                db.payments[0].set_tentative(true);
                db.payments[0].disabled = disabled;
                db.payments[0].set_flag(20261027, FLAG_PAID);
                db.payments[0].set_note(20261101, Some("Keep November"));
                db.rebuild_db(&Cal::from_int(20261001), 12, false);
                assert_eq!(amount(&mut db, 20261027), Some(-100.0));
                assert_eq!(amount(&mut db, 20261101), Some(-100.0));
                assert!(move_transaction(&mut db, id, 20261027, 20261115, -100.0).unwrap());
                assert_eq!(amount(&mut db, 20261027), None);
                assert_eq!(amount(&mut db, 20261115), Some(-100.0));
                assert_eq!(amount(&mut db, 20261101), Some(-100.0));
                assert_eq!(amount(&mut db, 20261201), Some(-100.0));
                assert_eq!(db.payments[0].get_flag(20261115), FLAG_PAID);
                assert_eq!(db.payments[0].get_note(20261101).as_deref(), Some("Keep November"));
                let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
                let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
                reopened.today = 20261027;
                reopened.rebuild_db(&Cal::from_int(20261001), 12, false);
                assert_eq!(amount(&mut reopened, 20261101), Some(-100.0));
                assert_eq!(amount(&mut reopened, 20261115), Some(-100.0));
            }
        }
    }

    #[test]
    fn move_rejects_duplicate_option_endpoints_instead_of_duplicating_or_losing_money() {
        for duplicate_at in [20261001, 20261101] {
            let mut db = db(false); let id = db.payments[0].id;
            let p = &mut db.payments[0];
            if duplicate_at == 20261001 {
                p.add_custom_amount(20261001, -50.0);
                p.add_custom_amount(20261001, -75.0);
            } else {
                p.add_no_transaction(20261101);
                p.add_no_transaction(20261101);
            }
            db.update_payment(0);
            let before = db.to_text(None).unwrap(); let updates = db.update_count;
            let value = if duplicate_at == 20261001 { -75.0 } else { -100.0 };
            let destination = if duplicate_at == 20261001 { 20261009 } else { 20261101 };
            assert!(move_transaction(&mut db, id, 20261001, destination, value).is_err());
            assert_eq!(db.to_text(None).unwrap(), before);
            assert_eq!(db.update_count, updates);
        }
    }

    #[test]
    fn move_loan_validates_before_rescheduling_and_preserves_payoff() {
        let mut db = db(false); let id = db.payments[0].id;
        db.payments[0].set_end_mode(budget_core::END_MODE_LOAN_PAID);
        db.payments[0].loan_amount = 250.0;
        db.update_payment(0);
        assert!(move_transaction(&mut db, id, 20261001, 20261003, -100.0).unwrap());
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20261003), Some(-100.0));
        assert_eq!(amount(&mut db, 20261101), Some(-100.0));
        assert_eq!(amount(&mut db, 20261201), Some(-50.0));
        db.payments[0].loan_amount = 1e20;
        let before = db.to_text(None).unwrap();
        assert!(move_transaction(&mut db, id, 20261003, 20261004, -100.0).is_err());
        assert_eq!(db.to_text(None).unwrap(), before);
    }

    fn closing(db: &mut BudgetDb, date: i32) -> f64 {
        db.get_day(&Cal::from_int(date)).unwrap().closing_balance
    }

    #[test]
    fn balance_set_update_remove_propagates_only_to_next_adjustment() {
        let mut db = db(false);
        db.update_adjustment(&Cal::from_int(20261201), 500.0);
        assert!(set_balance(&mut db, 20261001, Some(1200.0)).unwrap());
        assert_eq!(closing(&mut db, 20261001), 1200.0);
        assert_eq!(closing(&mut db, 20261101), 1100.0);
        assert_eq!(closing(&mut db, 20261201), 500.0);
        assert_eq!(closing(&mut db, 20270101), 400.0);
        assert!(set_balance(&mut db, 20261001, Some(1500.0)).unwrap());
        assert_eq!(closing(&mut db, 20261101), 1400.0);
        assert_eq!(closing(&mut db, 20261201), 500.0);
        assert!(set_balance(&mut db, 20261001, None).unwrap());
        assert_eq!(closing(&mut db, 20261001), -100.0);
        assert_eq!(closing(&mut db, 20261101), -200.0);
        assert_eq!(closing(&mut db, 20261201), 500.0);
    }

    #[test]
    fn balance_adjustment_preserves_payments_day_notes_and_other_adjustment_metadata_roundtrip() {
        let mut db = db(false);
        db.notes = "Budget-wide notes".into();
        db.set_day_note(&Cal::from_int(20261001), Some("Day note"));
        db.adjustments.set_flag(20261001, FLAG_PAID);
        db.adjustments.set_hilighting(20261001, 1234);
        db.adjustments.set_flag(20261201, budget_core::FLAG_IMPORTANT);
        db.update_adjustment(&Cal::from_int(20261201), 700.0);
        let payments = db.payments.clone();
        assert!(set_balance(&mut db, 20261001, Some(4321.12)).unwrap());
        assert!(set_balance(&mut db, 20261001, None).unwrap());
        assert_eq!(db.payments, payments);
        assert_eq!(db.day_note(&Cal::from_int(20261001)).as_deref(), Some("Day note"));
        assert_eq!(db.adjustments.get_flag(20261001), FLAG_PAID);
        assert_eq!(db.adjustments.get_hilighting(20261001), 1234);
        assert_eq!(db.adjustments.get_flag(20261201), budget_core::FLAG_IMPORTANT);
        assert!(set_balance(&mut db, 20261001, Some(4321.12)).unwrap());
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
        assert_eq!(closing(&mut reopened, 20261001), 4321.12);
        assert_eq!(closing(&mut reopened, 20261201), 700.0);
        assert_eq!(reopened.day_note(&Cal::from_int(20261001)).as_deref(), Some("Day note"));
        assert_eq!(reopened.notes, "Budget-wide notes");
    }

    #[test]
    fn balance_errors_and_noops_do_not_mutate_budget() {
        let mut db = db(false);
        db.update_adjustment(&Cal::from_int(20261001), 1234.56);
        let before = db.to_text(None).unwrap(); let updates = db.update_count;
        for (date, value) in [
            (20261001, Some(f64::NAN)), (20261001, Some(f64::NEG_INFINITY)),
            (20260230, Some(10.0)), (20261301, None), (i32::MAX, Some(10.0)),
            (-1, None), (99991232, None),
        ] {
            assert!(set_balance(&mut db, date, value).is_err());
            assert_eq!(db.to_text(None).unwrap(), before);
            assert_eq!(db.update_count, updates);
        }
        assert!(!set_balance(&mut db, 20261001, Some(1234.56)).unwrap());
        assert!(!set_balance(&mut db, 20261002, None).unwrap());
        assert_eq!(db.to_text(None).unwrap(), before);
        assert_eq!(db.update_count, updates);
    }

    #[test]
    fn balance_consolidates_duplicate_target_checkpoints_and_removes_the_entire_adjustment() {
        for requested in [100.0, 200.0, 300.0] {
            let mut db = db(false);
            db.adjustments.add_custom_amount(20261020, 100.0);
            db.adjustments.add_custom_amount(20261020, 200.0);
            db.adjustments.set_flag(20261020, FLAG_PAID);
            db.adjustments.set_note(20261020, Some("Keep this day note"));
            db.adjustments.add_custom_amount(20261120, 900.0);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            assert_eq!(closing(&mut db, 20261020), 200.0);
            assert!(set_balance(&mut db, 20261020, Some(requested)).unwrap());
            assert_eq!(closing(&mut db, 20261020), requested);
            assert_eq!(db.adjustments.opt.iter().filter(|o| o.date == 20261020).count(), 1);
            assert!(!set_balance(&mut db, 20261020, Some(requested)).unwrap());
            assert!(set_balance(&mut db, 20261020, None).unwrap());
            assert_eq!(closing(&mut db, 20261020), -100.0);
            assert!(!db.adjustments.opt.iter().any(|o| o.date == 20261020));
            assert_eq!(db.adjustments.get_flag(20261020), FLAG_PAID);
            assert_eq!(db.adjustments.get_note(20261020).as_deref(), Some("Keep this day note"));
            assert_eq!(closing(&mut db, 20261120), 900.0);
        }
        let mut db = db(false);
        db.adjustments.add_custom_amount(20261020, 100.0);
        db.adjustments.add_custom_amount(20261020, 200.0);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        assert!(set_balance(&mut db, 20261020, None).unwrap());
        assert_eq!(closing(&mut db, 20261020), -100.0);
        assert!(!db.adjustments.opt.iter().any(|o| o.date == 20261020));
    }

    #[test]
    fn save_refuses_external_changes_and_preserves_disk_bytes() {
        let db = db(false); let path = temp_file();
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&path, b"changed by another app").unwrap();
        let err = save_checked(&db, &path, None, Some(b"original")).unwrap_err();
        assert!(err.contains("changed"));
        assert_eq!(std::fs::read(&path).unwrap(), b"changed by another app");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn successful_save_returns_exact_snapshot_for_subsequent_save() {
        let db = db(false); let path = temp_file();
        let snapshot = save_checked(&db, &path, None, None).unwrap();
        assert_eq!(snapshot, std::fs::read(&path).unwrap());
        let again = save_checked(&db, &path, None, Some(&snapshot)).unwrap();
        assert_eq!(again, snapshot);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn save_recreates_a_budget_file_that_disappeared_from_disk() {
        let db = db(false); let path = temp_file();
        let snapshot = save_checked(&db, &path, None, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        let again = save_checked(&db, &path, None, Some(&snapshot)).unwrap();
        assert_eq!(again, std::fs::read(&path).unwrap());
        // A missing folder is still a write failure, not a bogus "changed on disk".
        let gone = path.parent().unwrap().join("missing").join("x.bgt");
        let err = save_checked(&db, &gone, None, Some(&snapshot)).unwrap_err();
        assert!(!err.contains("changed on disk"), "{err}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn marking_a_tentative_occurrence_paid_pins_it_to_that_date() {
        for sticky in [false, true] {
            let mut db = db(sticky); db.today = 20261027;
            db.payments[0].set_tentative(true);
            db.rebuild_db(&Cal::from_int(20261001), 12, false);
            let id = db.payments[0].id;
            assert_eq!(amount(&mut db, 20261027), Some(-100.0));
            change_transaction(&mut db, id, 20261027, -100.0, "", FLAG_PAID).unwrap();
            let option = db.payments[0].option_by_date(20261027).expect("paid tentative occurrence must be pinned");
            assert!(!option.deleted && option.amount == -100.0);
            assert!(!db.payments[0].opt.iter().any(|o| o.date != 20261027), "no extra anchors for tentative payments");
            let text = db.to_text(None).unwrap();
            let loaded = bgt::load_str(&text, 20261029, None).unwrap();
            let mut reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
            reopened.today = 20261029;
            reopened.rebuild_db(&Cal::from_int(20261001), 12, false);
            let day = reopened.get_day(&Cal::from_int(20261027)).unwrap().transactions[0].clone();
            assert_eq!(day.amount, -100.0);
            assert_eq!(day.suffix, budget_core::SUFFIX_ADJUSTED);
            assert_eq!(amount(&mut reopened, 20261029), None);
            assert_eq!(reopened.payments[0].get_flag(20261027), FLAG_PAID);
        }
    }

    /// A 300 loan paid 100 a month from January 2026, with a window that covers it.
    fn loan_300() -> (BudgetDb, i32) {
        let mut db = db(false); db.today = 20260201;
        let id = db.payments[0].id;
        let p = &mut db.payments[0]; p.start_date = 20260101;
        p.set_end_mode(budget_core::END_MODE_LOAN_PAID); p.loan_amount = 300.0;
        db.rebuild_db(&Cal::from_int(20260601), 12, false);
        (db, id)
    }

    #[test]
    fn loan_principal_refuses_a_single_occurrence_amount_that_overpays() {
        let (mut db, id) = loan_300();
        let before = db.to_text(None).unwrap(); let updates = db.update_count;
        let err = change_transaction(&mut db, id, 20260101, -400.0, "", FLAG_NONE).unwrap_err();
        assert!(err.contains("principal"), "{err}");
        assert_eq!(db.to_text(None).unwrap(), before);
        assert_eq!(db.update_count, updates);
        change_transaction(&mut db, id, 20260101, -200.0, "", FLAG_NONE).unwrap();
        assert_eq!(amount(&mut db, 20260101), Some(-200.0));
    }

    #[test]
    fn loan_principal_refuses_apply_to_all_below_the_amounts_already_paid() {
        let (mut db, id) = loan_300();
        change_transaction(&mut db, id, 20260101, -50.0, "", FLAG_NONE).unwrap();
        change_transaction(&mut db, id, 20260201, -250.0, "", FLAG_NONE).unwrap();
        let before = db.to_text(None).unwrap(); let updates = db.update_count;
        let mut edited = db.payments[0].clone(); edited.loan_amount = 200.0;
        let err = apply_payment(&mut db, edited, Some(id), None).unwrap_err();
        assert!(err.contains("principal"), "{err}");
        assert_eq!(db.to_text(None).unwrap(), before);
        assert_eq!(db.update_count, updates);
        assert_eq!(db.payments[0].loan_amount, 300.0);
    }

    #[test]
    fn future_change_from_a_date_before_a_tentative_payment_starts_applies_to_every_occurrence() {
        // An occurrence dragged to 2026-10-06, before the first pay period, is the selected one.
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20270510; p.freq_type = 'm'; p.frequency = 2; p.amount = -271.0; p.set_tentative(true);
        assert!(move_transaction(&mut db, id, 20270710, 20261006, -271.0).unwrap());
        change_future_transactions(&mut db, id, 20261006, -300.0, "", FLAG_NONE).unwrap();
        db.rebuild_db(&Cal::from_int(20280101), 12, true);
        for (date, expected) in [(20261006, Some(-300.0)), (20270510, Some(-300.0)), (20270710, None),
            (20270910, Some(-300.0)), (20271110, Some(-300.0))] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
    }

    #[test]
    fn future_change_with_an_unchanged_amount_keeps_one_record() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].start_date = 20260115;
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261015, -100.0, "Same amount", FLAG_PAID).unwrap();
        assert_eq!(db.payments.len(), 1, "an unchanged amount does not split the payment");
        assert_eq!(note_on(&db, 20261015).as_deref(), Some("Same amount"));
        assert_eq!(db.payments[0].get_flag(20261015), FLAG_PAID);
        assert_eq!(amount(&mut db, 20261015), Some(-100.0));
        assert_eq!(amount(&mut db, 20261115), Some(-100.0));
    }

    #[test]
    fn future_change_with_an_unchanged_amount_still_splits_for_a_later_amount_that_differs() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].start_date = 20260115;
        db.payments[0].set_custom_amount(20261215, -50.0);
        db.rebuild_db(&Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261015, -100.0, "", FLAG_NONE).unwrap();
        assert_eq!(db.payments.len(), 2);
        assert_eq!(amount(&mut db, 20261215), Some(-100.0));
    }
}
