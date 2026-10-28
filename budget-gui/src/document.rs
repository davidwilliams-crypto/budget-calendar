//! Editing operations and optimistic disk-change checks, independent of the UI.
use budget_core::{bgt, BudgetDb, Cal, Field, Payment};
use std::path::Path;

pub fn save_checked(db: &BudgetDb, path: &Path, password: Option<&str>, expected: Option<&[u8]>) -> Result<Vec<u8>, String> {
    if let Some(expected) = expected {
        let current = std::fs::read(path).map_err(|e| format!("The budget file changed or is unavailable: {e}. Use Save as to keep your edits in a separate file."))?;
        if current != expected {
            return Err("The budget file changed on disk since it was opened. Use Save as to keep your edits in a separate file, or reopen it to load the other changes.".into());
        }
    }
    let text = db.to_text(password)?;
    bgt::save_file(path, &text)?;
    Ok(bgt::latin1_encode(&text))
}

pub fn put_payment(db: &mut BudgetDb, payment: Payment, id: Option<i32>) -> Result<i32, String> {
    let mut payment = payment;
    if !crate::editor::amount_in_range(payment.amount) || payment.description.trim().is_empty() {
        return Err("Enter a description and an amount within +/-1,000,000,000,000.".into());
    }
    if let Some(id) = id {
        let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
        payment.id = id;
        db.payments[i] = payment;
        update_preserving_details(db, i);
        Ok(id)
    } else {
        let details = payment.clone();
        let i = db.add(payment);
        restore_details(&mut db.payments[i], details);
        Ok(db.payments[i].id)
    }
}

pub fn change_transaction(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> Result<(), String> {
    if !crate::editor::amount_in_range(amount) { return Err("Enter an amount within +/-1,000,000,000,000.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let current = db.get_day(&Cal::from_int(date)).and_then(|d| d.get(i)).map(|t| t.amount)
        .ok_or("This transaction no longer exists.")?;
    let mut proposed = db.payments[i].clone();
    if current != amount {
        proposed.set_custom_amount(date, amount);
        validate_loan(&proposed)?;
        preserve_future_amount(db, i, date, &mut proposed)?;
    }
    validate_loan(&proposed)?;
    proposed.set_note(date, if note.trim().is_empty() { None } else { Some(note) });
    proposed.set_flag(date, flag);
    db.payments[i] = proposed;
    update_preserving_details(db, i);
    Ok(())
}

/// Change the recurring amount while recording the actual, finite history as
/// native dated options. Future exceptions retain their dates and metadata.
pub fn change_future_transactions(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> Result<(), String> {
    valid_date(date)?;
    if !crate::editor::amount_in_range(amount) { return Err("Enter an amount within +/-1,000,000,000,000.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let original = &db.payments[i];
    validate_future_payment(original)?;
    if note.chars().any(|c| c as u32 > 0xffff) {
        return Err("This note contains a character the budget format cannot save.".into());
    }
    if !matches!(flag, budget_core::FLAG_NONE | budget_core::FLAG_PAID | budget_core::FLAG_IMPORTANT) {
        return Err("Choose a valid transaction status.".into());
    }
    let loan = original.end_mode() == budget_core::END_MODE_LOAN_PAID;
    if loan && (original.is_sticky() || original.is_tentative() || amount == 0.0 || amount.is_sign_negative() != original.amount.is_sign_negative()) {
        return Err("Future amount changes require a nonzero loan payment in its existing direction, without sticky or tentative scheduling. Change only this occurrence instead.".into());
    }
    let mut first = original.start_date;
    for option in &original.opt { if !option.deleted { first = first.min(option.date); } }
    // Early weekend placement can precede the original recurrence start.
    if !original.is_tentative() { first = first.min(actual_date(original, original.start_date)); }
    valid_date(first)?;
    let mut boundary = Cal::from_int(date);
    boundary.add(Field::Day, 3);
    if boundary.year() > 9999 { boundary = Cal::from_int(99991231); }
    let before = future_amount_snapshot(db.today, original, first.min(date), boundary.get())?;
    if !before.contains_key(&date) { return Err("This transaction no longer exists.".into()); }
    if before.len() + original.opt.len() > 10_000 {
        return Err("This payment has too many historical occurrences to preserve safely in one change. Change only this occurrence instead.".into());
    }
    let mut proposed = original.clone();
    proposed.amount = amount;
    for option in &mut proposed.opt {
        if option.date >= date && !option.deleted { option.amount = amount; }
    }
    // Only freeze transactions that actually existed. In particular, a past
    // tentative period without a saved occurrence must remain empty.
    let saved: std::collections::BTreeSet<_> = proposed.opt.iter().map(|o| o.date).collect();
    for (&day, &old_amount) in before.range(..date) {
        if !saved.contains(&day) { proposed.opt.push(budget_core::PaymentOption { date: day, amount: old_amount, deleted: false }); }
    }
    proposed.set_custom_amount(date, amount);
    // Late weekend groups may have nominal dates before the new carry anchor.
    // Their actual business-day dates still belong to the selected future.
    if !loan {
        for (&day, _) in before.range(date..) { proposed.set_custom_amount(day, amount); }
    }
    proposed.set_note(date, if note.trim().is_empty() { None } else { Some(note) });
    proposed.set_flag(date, flag);
    if loan {
        let saved_total: f64 = proposed.opt.iter().filter(|option| !option.deleted).map(|option| option.amount).sum();
        let principal_paid = if proposed.amount < 0.0 { -saved_total } else { saved_total };
        // The native loan scheduler subtracts every dated option up front. If
        // these already exceed principal it emits all of them anyway, silently
        // overpaying the loan instead of producing a smaller final payment.
        if !principal_paid.is_finite() || principal_paid > proposed.loan_amount {
            return Err("This amount would exceed the loan principal after including historical and future custom payments. Use a smaller amount or change only this occurrence.".into());
        }
    }
    validate_future_payment(&proposed)?;
    let after = future_amount_snapshot(db.today, &proposed, first.min(date), boundary.get())?;
    if before.range(..date).ne(after.range(..date)) || after.get(&date) != Some(&amount) {
        return Err("This schedule cannot retain its earlier transactions safely. Change only this occurrence instead.".into());
    }
    if !loan && (before.range(date..).map(|(d, _)| d).ne(after.range(date..).map(|(d, _)| d))
        || after.range(date..).any(|(_, value)| *value != amount)) {
        return Err("This schedule cannot change future amounts without moving transactions. Change only this occurrence instead.".into());
    }
    // No live budget state, cached days or update counters are touched until
    // every validation and native-scheduler comparison has succeeded.
    db.payments[i] = proposed;
    update_preserving_details(db, i);
    Ok(())
}

fn validate_future_payment(p: &Payment) -> Result<(), String> {
    valid_date(p.start_date)?;
    if !p.amount.is_finite() || p.opt.iter().any(|o| !o.amount.is_finite()) {
        return Err("This payment has a non-finite saved amount. Repair the payment before changing future amounts.".into());
    }
    if !matches!(p.end_mode(), budget_core::END_MODE_INDEFINITE | budget_core::END_MODE_DATE | budget_core::END_MODE_N_PAYS | budget_core::END_MODE_LOAN_PAID) {
        return Err("This payment has an unsupported end condition.".into());
    }
    if p.end_mode() == budget_core::END_MODE_DATE {
        valid_date(p.end_date)?;
        if p.end_date < p.start_date { return Err("This payment ends before it starts.".into()); }
    }
    if p.end_mode() == budget_core::END_MODE_LOAN_PAID
        && (!p.loan_amount.is_finite() || p.loan_amount <= 0.0 || p.amount == 0.0) {
        return Err("This loan needs a positive finite principal and a nonzero payment.".into());
    }
    crate::editor::validate_schedule_bounds(p)?;
    let mut dates = std::collections::BTreeSet::new();
    for option in &p.opt {
        valid_date(option.date)?;
        if !dates.insert(option.date) {
            return Err("This payment has duplicate saved amount entries. Resolve them before changing future amounts.".into());
        }
    }
    Ok(())
}

fn future_amount_snapshot(today: i32, payment: &Payment, from: i32, to: i32) -> Result<std::collections::BTreeMap<i32, f64>, String> {
    // Native nonsticky schedules need an option for each historical occurrence.
    // Bound the work before allocating native BudgetDay storage for legacy files
    // with millennia of daily history. Rejection leaves the live budget intact.
    if Cal::diff_int(from, to) > 366_000 {
        return Err("This payment has more than 1,000 years of history. Change only this occurrence instead.".into());
    }
    let mut snapshot = BudgetDb::new(&Cal::from_int(to), 0, false);
    snapshot.today = today;
    snapshot.payments.push(payment.clone());
    snapshot.adjustments.set_custom_amount(from, 0.0);
    rebuild(&mut snapshot, &Cal::from_int(to), 0, false);
    let mut values = std::collections::BTreeMap::new();
    let mut day = Cal::from_int(from);
    while day.get() <= to {
        if let Some(transaction) = snapshot.get_day(&day).and_then(|d| d.get(0)) {
            values.insert(day.get(), transaction.amount);
        }
        day.add(Field::Day, 1);
    }
    Ok(values)
}

/// Move one scheduled occurrence, retaining its metadata and all unrelated exceptions.
pub fn move_transaction(db: &mut BudgetDb, id: i32, from: i32, to: i32, amount: f64) -> Result<bool, String> {
    valid_date(from)?;
    valid_date(to)?;
    if !crate::editor::amount_in_range(amount) { return Err("Enter an amount within +/-1,000,000,000,000.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let original = db.payments[i].clone();
    // Validate before running even an isolated scheduler: a malformed loan or
    // recurrence must never turn a rejected drop into an unbounded calculation.
    crate::editor::validate_schedule_bounds(&original)?;
    let mut source = isolated_payment_db(db.today, &original, from);
    let current = source.get_day(&Cal::from_int(from)).and_then(|d| d.get(0)).map(|t| t.amount)
        .ok_or("This transaction no longer exists.")?;
    if current != amount { return Err("This transaction changed while it was being moved. Try dragging it again.".into()); }
    if from == to { return Ok(false); }
    if [from, to].into_iter().any(|date| {
        original.opt.iter().filter(|o| o.date == date).take(2).count() > 1
            || original.flag.iter().filter(|f| f.date == date).take(2).count() > 1
            || original.note.iter().filter(|n| n.date == date).take(2).count() > 1
            || original.hilighting.iter().filter(|h| h.date == date).take(2).count() > 1
    }) {
        return Err("This transaction has duplicate saved amounts, status, notes, or highlighting. Resolve those entries before moving it.".into());
    }
    if original.opt.iter().any(|o| o.date == to && !o.deleted) {
        return Err("This payment already has a transaction on the destination date.".into());
    }
    let mut destination = isolated_payment_db(db.today, &original, to);
    if destination.get_day(&Cal::from_int(to)).and_then(|d| d.get(0)).is_some() {
        return Err("This payment already has a transaction on the destination date.".into());
    }
    let source_flag = original.flag.iter().find(|f| f.date == from).map(|f| f.value);
    let mut proposed = original;
    // A skipped date can retain its old metadata. The moved occurrence owns
    // these fields, including absence/inheritance of a per-date override.
    proposed.flag.retain(|f| f.date != to);
    proposed.note.retain(|n| n.date != to);
    proposed.hilighting.retain(|h| h.date != to);
    proposed.move_transaction(to, from, current);
    // Native move omits explicit FLAG_NONE, which can override a paid default.
    if let Some(flag) = source_flag { proposed.set_flag(to, flag); }
    validate_loan(&proposed)?;
    // Deleting an override and inserting it elsewhere can each change sticky
    // carry. Check both boundaries against the untouched original schedule.
    // The preservation helper also needs today's actual tentative occurrence.
    let mut baseline = isolated_payment_db(db.today, &db.payments[i], db.today);
    preserve_future_amount(&mut baseline, 0, from.min(to), &mut proposed)?;
    preserve_future_amount(&mut baseline, 0, from.max(to), &mut proposed)?;
    validate_loan(&proposed)?;
    preserve_tentative_destination(&baseline, &mut proposed, from, to)?;
    validate_loan(&proposed)?;
    db.payments[i] = proposed;
    update_preserving_details(db, i);
    db.calc_balances(None);
    Ok(true)
}

/// Set an absolute closing-balance checkpoint, or remove just that checkpoint.
pub fn set_balance(db: &mut BudgetDb, date: i32, amount: Option<f64>) -> Result<bool, String> {
    valid_date(date)?;
    if amount.is_some_and(|v| !crate::editor::amount_in_range(v)) { return Err("Enter a balance within +/-1,000,000,000,000.".into()); }
    let mut matching = db.adjustments.opt.iter().filter(|o| o.date == date);
    let existing = matching.next();
    let unique = matching.next().is_none();
    if match (existing, amount) {
        (None, None) => true,
        (Some(option), Some(value)) => unique && option.amount == value,
        _ => false,
    } { return Ok(false); }
    let cal = Cal::from_int(date);
    // Finish any pending payment calculation before the core's partial balance
    // refresh starts from the preceding day's closing balance.
    db.calc_balances(None);
    // Legacy files may contain several checkpoints for one date. The native
    // engine displays the last one but edits/removes only the first; replace
    // the entire dated option group so a successful edit has its stated effect.
    db.adjustments.opt.retain(|o| o.date != date);
    match amount {
        Some(value) => db.update_adjustment(&cal, value),
        None => {
            let flags = db.adjustments.flag.clone();
            db.remove_adjustment(&cal);
            // Payment::clear also removes a flag. A balance edit only owns the
            // adjustment option, so retain day-level metadata verbatim.
            db.adjustments.flag = flags;
        }
    }
    db.calc_balances(None);
    Ok(true)
}

fn valid_date(date: i32) -> Result<(), String> {
    if !(10000101..=99991231).contains(&date) || Cal::from_int(date).get() != date {
        return Err("Enter a valid calendar date between years 1000 and 9999.".into());
    }
    Ok(())
}

fn isolated_payment_db(today: i32, payment: &Payment, date: i32) -> BudgetDb {
    let cal = Cal::from_int(date);
    let mut snapshot = BudgetDb::new(&cal, 0, false);
    snapshot.today = today;
    snapshot.payments.push(payment.clone());
    // Keep storage near the queried date even for a very old recurrence. The
    // synthetic checkpoint affects balances only, never transaction scheduling.
    let mut anchor = cal;
    anchor.set(Field::Day, 1);
    anchor.add(Field::Month, -3);
    snapshot.adjustments.set_custom_amount(anchor.get(), 0.0);
    rebuild(&mut snapshot, &cal, 0, false);
    snapshot
}

fn preserve_tentative_destination(baseline: &BudgetDb, proposed: &mut Payment, from: i32, to: i32) -> Result<(), String> {
    if !baseline.payments[0].is_tentative() { return Ok(()); }
    let Some(period) = regular_on_or_before(baseline, to) else { return Ok(()); };
    let target = if regular_on_or_before(baseline, baseline.today) == Some(period) { baseline.today } else { period };
    if target == from || target == to { return Ok(()); }
    let mut original = isolated_payment_db(baseline.today, &baseline.payments[0], target);
    let Some(amount) = original.get_day(&Cal::from_int(target)).and_then(|d| d.get(0)).map(|t| t.amount) else { return Ok(()); };
    let mut candidate = isolated_payment_db(baseline.today, proposed, target);
    if candidate.get_day(&Cal::from_int(target)).and_then(|d| d.get(0)).map(|t| t.amount) != Some(amount) {
        // An explicit destination option suppresses a tentative period's
        // generated estimate. Retain that independent occurrence as an option.
        proposed.set_custom_amount(target, amount);
    }
    Ok(())
}

fn regular_on_or_before(db: &BudgetDb, date: i32) -> Option<i32> {
    let start = db.payments[0].start_date;
    if date < start { return None; }
    // Find the last occurrence through the native recurrence query, retaining
    // its numbered-weekday/month-end rules without duplicating schedule math.
    // Query thresholds are integer comparisons, so intermediate thresholds do
    // not need to be valid calendar dates.
    let (mut left, mut right) = (start - 1, date);
    while left + 1 < right {
        let middle = left + (right - left) / 2;
        let next = db.next_regular_payment_after_date(0, middle);
        if next >= 0 && next <= date { left = middle; } else { right = middle; }
    }
    Some(right)
}

fn actual_date(p: &Payment, scheduled: i32) -> i32 {
    let mut actual = Cal::from_int(scheduled);
    if p.is_no_weekends() && actual.is_weekend() {
        let sunday = actual.day_of_week() == budget_core::date::SUNDAY;
        actual.add(Field::Day, if p.is_early_pay() { if sunday { -2 } else { -1 } } else if sunday { 1 } else { 2 });
    }
    actual.get()
}

fn validate_loan(proposed: &Payment) -> Result<(), String> {
    if proposed.end_mode() == budget_core::END_MODE_LOAN_PAID {
        crate::editor::validate_schedule_bounds(proposed)?;
    }
    Ok(())
}

pub fn delete(db: &mut BudgetDb, id: i32, date: Option<i32>) -> Result<(), String> {
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    if let Some(date) = date {
        let mut proposed = db.payments[i].clone();
        proposed.set_no_transaction(date);
        validate_loan(&proposed)?;
        preserve_future_amount(db, i, date, &mut proposed)?;
        validate_loan(&proposed)?;
        db.payments[i] = proposed;
        update_preserving_details(db, i);
    } else {
        let original: Vec<_> = db.payments.iter().filter(|p| p.id != id).cloned().collect();
        db.delete_payment(i);
        for (p, details) in db.payments.iter_mut().zip(original) { restore_details(p, details); }
    }
    Ok(())
}

fn preserve_future_amount(db: &mut BudgetDb, i: usize, date: i32, proposed: &mut Payment) -> Result<(), String> {
    let original = &db.payments[i];
    if !original.is_sticky() { return Ok(()); }
    // Tentative schedules choose an occurrence within each period independently;
    // inserting regular-date anchors would introduce extra transactions there.
    if original.is_tentative() {
        let today = db.today;
        if original.disabled && today > date && proposed.option_by_date(today).is_none() {
            // Disabled sticky tentative payments are the exception: an existing
            // current-period occurrence uses the amount carried into today.
            // Snapshot the real occurrence, so paid/absent periods stay absent.
            let existing = db.get_day(&Cal::from_int(today)).and_then(|d| d.get(i)).map(|t| t.amount);
            if let Some(amount) = existing {
                if proposed.amount_before_date(today) != amount { proposed.set_custom_amount(today, amount); }
            }
        }
        return Ok(());
    }
    let mut cursor = Cal::from_int(date);
    cursor.add(Field::Day, -3);
    let mut scheduled = db.next_regular_payment_after_date(i, cursor.get());
    let mut last_changed = date;
    while scheduled >= 0 {
        let actual = actual_date(original, scheduled);
        if actual > 99991231 { return Err("This occurrence cannot be changed without extending beyond year 9999.".into()); }
        // Several daily weekend occurrences can land on one business day. The
        // scheduler keeps the last one, then applies the date's explicit option.
        let mut next = db.next_regular_payment_after_date(i, scheduled);
        while next >= 0 && actual_date(original, next) == actual {
            scheduled = next;
            next = db.next_regular_payment_after_date(i, scheduled);
        }
        let original_option = original.option_by_date(actual);
        if actual > date && !original_option.is_some_and(|o| o.deleted) {
            let desired = original_option.map_or_else(|| original.amount_before_date(scheduled), |o| o.amount);
            let current = proposed.option_by_date(actual).map_or_else(|| proposed.amount_before_date(scheduled), |o| o.amount);
            if current != desired {
                proposed.set_custom_amount(actual, desired);
                last_changed = last_changed.max(actual);
            }
        }
        // An anchor on a shifted date can itself change the later carried amount.
        // Continue until both the displayed occurrence and subsequent carry agree.
        let mut after = Cal::from_int(scheduled);
        after.add(Field::Day, 1);
        if scheduled >= last_changed && original.amount_before_date(after.get()) == proposed.amount_before_date(after.get()) {
            return Ok(());
        }
        scheduled = next;
    }
    Ok(())
}

fn restore_details(payment: &mut Payment, original: Payment) {
    payment.opt = original.opt;
    payment.flag = original.flag;
    payment.note = original.note;
    payment.hilighting = original.hilighting;
}

fn update_preserving_details(db: &mut BudgetDb, i: usize) {
    let details = db.payments[i].clone();
    db.update_payment(i);
    restore_details(&mut db.payments[i], details);
}

pub fn rebuild(db: &mut BudgetDb, cal: &Cal, months: i32, history: bool) {
    let original = db.payments.clone();
    db.rebuild_db(cal, months, history);
    for (p, details) in db.payments.iter_mut().zip(original) { restore_details(p, details); }
}

pub fn from_loaded(loaded: bgt::Loaded, cal: &Cal, months: i32, history: bool) -> BudgetDb {
    let original = loaded.payments.clone();
    let mut db = BudgetDb::from_loaded(loaded, cal, months, history);
    for (p, details) in db.payments.iter_mut().zip(original) { restore_details(p, details); }
    db
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::{bgt, END_MODE_INDEFINITE, FLAG_PAID, FLAG_NONE};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

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

    #[test]
    fn future_amount_freezes_nonsticky_history_and_replaces_far_future_exceptions_roundtrip() {
        let mut db = db(false); db.today = 20261027;
        let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.start_date = 20260801; p.category = Some("Household".into());
        p.set_custom_amount(20260901, -75.0);
        p.set_no_transaction(20261201);
        p.set_custom_amount(20300101, -12.0);
        p.set_note(20300101, Some("Future note"));
        p.set_flag(20300101, FLAG_PAID); p.set_hilighting(20260901, 123);
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "Selected only", FLAG_PAID).unwrap();
        assert_eq!(amount(&mut db, 20260801), Some(-100.0));
        assert_eq!(amount(&mut db, 20260901), Some(-75.0));
        assert_eq!(amount(&mut db, 20261001), Some(-250.0));
        assert_eq!(amount(&mut db, 20261101), Some(-250.0));
        assert_eq!(amount(&mut db, 20261201), None);
        assert_eq!(db.payments.len(), 1);
        assert_eq!(db.payments[0].start_date, 20260801);
        assert_eq!(db.payments[0].category.as_deref(), Some("Household"));
        assert_eq!(db.payments[0].get_hilighting(20260901), 123);
        assert_eq!(db.payments[0].get_note(20300101).as_deref(), Some("Future note"));
        assert_eq!(db.payments[0].get_flag(20300101), FLAG_PAID);
        let text = db.to_text(None).unwrap();
        let loaded = bgt::load_str(&text, 20261027, None).unwrap();
        let mut reopened = from_loaded(loaded, &Cal::from_int(20300101), 12, true);
        assert_eq!(amount(&mut reopened, 20260801), Some(-100.0));
        assert_eq!(amount(&mut reopened, 20260901), Some(-75.0));
        assert_eq!(amount(&mut reopened, 20300101), Some(-250.0));
        assert_eq!(amount(&mut reopened, 20300201), Some(-250.0));
        assert_eq!(reopened.payments[0].get_note(20261001).as_deref(), Some("Selected only"));
        assert_eq!(reopened.payments[0].get_note(20261101), None);
    }

    #[test]
    fn future_amount_resets_sticky_carry_without_losing_old_custom_amounts_and_moves() {
        let mut db = db(true); let id = db.payments[0].id;
        let p = &mut db.payments[0]; p.start_date = 20260701;
        p.set_custom_amount(20260801, -75.0);
        p.set_custom_amount(20261001, -45.0);
        p.set_no_transaction(20261101); p.set_custom_amount(20261104, -45.0);
        p.set_custom_amount(20300704, -12.0);
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).unwrap();
        for (date, expected) in [(20260701, Some(-100.0)), (20260801, Some(-75.0)), (20260901, Some(-75.0)),
            (20261001, Some(-250.0)), (20261101, None), (20261104, Some(-250.0)), (20261201, Some(-250.0))] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        assert_eq!(db.payments[0].option_by_date(20300704).unwrap().amount, -250.0);
    }

    #[test]
    fn future_weekly_groceries_150_to_250_keeps_the_previous_year_unchanged() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.description = "Groceries".into(); p.start_date = 20251024;
        p.freq_type = 'w'; p.frequency = 1; p.amount = -150.0;
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261023, -250.0, "Weekly budget", FLAG_NONE).unwrap();
        for date in [20251024, 20251226, 20261009, 20261016] { assert_eq!(amount(&mut db, date), Some(-150.0)); }
        for date in [20261023, 20261030, 20261106, 20270101] { assert_eq!(amount(&mut db, date), Some(-250.0)); }
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = from_loaded(loaded, &Cal::from_int(20301001), 0, true);
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
                    rebuild(&mut db, &Cal::from_int(20261001), 12, false);
                    let mut day = Cal::from_int(20260924); let mut previous = Vec::new();
                    while day.get() < selected { previous.push((day.get(), amount(&mut db, day.get()))); day.add_days(1); }
                    change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
                    for (date, value) in previous { assert_eq!(amount(&mut db, date), value, "sticky={sticky}, early={early}, selected={selected}, date={date}"); }
                    for date in [selected, 20261012, 20261013, 20261014] { assert_eq!(amount(&mut db, date), Some(-250.0)); }
                    assert_eq!(amount(&mut db, 20261009), None);
                    let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
                    let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, true);
                    assert_eq!(amount(&mut reopened, selected), Some(-250.0));
                    assert_eq!(amount(&mut reopened, 20261102), Some(-250.0));
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
                    rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
                    assert_eq!(db.payments[0].get_note(20261203).as_deref(), Some("Keep December"));
                    let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
                    let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, true);
                    reopened.today = 20261027; rebuild(&mut reopened, &Cal::from_int(20261001), 12, true);
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
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
        change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).unwrap();
        for (date, expected) in [(20260801, Some(-100.0)), (20260901, Some(-100.0)), (20261001, Some(-250.0)),
            (20261101, Some(-250.0)), (20261201, Some(-250.0)), (20270101, Some(-50.0)), (20270201, None)] {
            assert_eq!(amount(&mut db, date), expected, "{date}");
        }
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, true);
        assert_eq!(amount(&mut reopened, 20260901), Some(-100.0));
        assert_eq!(amount(&mut reopened, 20270101), Some(-50.0));
    }

    #[test]
    fn future_loan_rejects_overpayment_from_historical_selected_and_future_options_atomically() {
        for (future_exception, value) in [(Some(-600.0), -750.0), (None, -900.0)] {
            let mut db = db(false); let id = db.payments[0].id;
            let p = &mut db.payments[0]; p.start_date = 20260801;
            p.set_end_mode(budget_core::END_MODE_LOAN_PAID); p.loan_amount = 1000.0;
            if let Some(value) = future_exception { p.set_custom_amount(20261201, value); }
            rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
            let before = db.to_text(None).unwrap(); let updates = db.update_count;
            assert!(change_future_transactions(&mut db, id, 20261001, -250.0, "", FLAG_NONE).is_err(), "kind={kind}");
            assert_eq!(db.to_text(None).unwrap(), before); assert_eq!(db.update_count, updates);
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
                rebuild(&mut db, &Cal::from_int(20261001), 12, false);
                let end = db.payments[0].end_date; let count = db.payments[0].n_pays;
                let mut day = Cal::from_int(20260801); let mut baseline = Vec::new();
                while day.get() < 20270301 { baseline.push((day.get(), amount(&mut db, day.get()))); day.add_days(1); }
                let selected = baseline.iter().filter(|(date, value)| *date >= 20261001 && value.is_some()).next().unwrap().0;
                change_future_transactions(&mut db, id, selected, -250.0, "", FLAG_NONE).unwrap();
                for (date, value) in baseline {
                    assert_eq!(amount(&mut db, date), if date < selected { value } else { value.map(|_| -250.0) }, "mode={mode}, frequency={freq_type}, date={date}");
                }
                assert_eq!(db.payments[0].end_mode(), mode); assert_eq!(db.payments[0].end_date, end); assert_eq!(db.payments[0].n_pays, count);
            }
        }
    }

    #[test]
    fn future_amount_queries_history_outside_the_visible_window_and_supports_later_changes() {
        let mut db = db(true); let id = db.payments[0].id;
        db.update_adjustment(&Cal::from_int(20290101), 500.0);
        rebuild(&mut db, &Cal::from_int(20290601), 0, false);
        assert!(db.get_day(&Cal::from_int(20261001)).is_none());
        change_future_transactions(&mut db, id, 20261101, -250.0, "First change", FLAG_NONE).unwrap();
        change_future_transactions(&mut db, id, 20270101, -300.0, "Second change", FLAG_PAID).unwrap();
        rebuild(&mut db, &Cal::from_int(20301001), 0, true);
        assert_eq!(amount(&mut db, 20261001), Some(-100.0));
        assert_eq!(amount(&mut db, 20261101), Some(-250.0));
        assert_eq!(amount(&mut db, 20261201), Some(-250.0));
        assert_eq!(amount(&mut db, 20270101), Some(-300.0));
        assert_eq!(amount(&mut db, 20301001), Some(-300.0));
        assert_eq!(db.payments[0].get_note(20261101).as_deref(), Some("First change"));
        assert_eq!(db.payments[0].get_note(20270101).as_deref(), Some("Second change"));
        assert_eq!(db.payments[0].opt.len(), 4);
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
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
            rebuild(&mut db, &Cal::from_int(20261001), 12, false);
            assert_eq!(amount(&mut db, 20261102), Some(-150.0));
            if delete_occurrence { delete(&mut db, id, Some(20261001)).unwrap(); }
            else { change_transaction(&mut db, id, 20261001, -35.0, "", FLAG_NONE).unwrap(); }
            assert_eq!(amount(&mut db, 20261101), Some(-80.0));
            assert_eq!(amount(&mut db, 20261102), Some(-150.0));
            assert_eq!(amount(&mut db, 20261201), Some(-80.0));
            assert_eq!(amount(&mut db, 20270101), Some(-80.0));
            let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
            let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
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
            rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
                rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
        update_preserving_details(&mut db, 0);
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
        let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
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
        update_preserving_details(&mut db, 0);
        assert!(move_transaction(&mut db, id, 20261001, 20260930, -100.0).unwrap());
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20260930), Some(-100.0));
        assert_eq!(amount(&mut db, 20261101), None);
    }

    #[test]
    fn move_to_skipped_date_discards_stale_target_metadata_and_inherits_source_defaults() {
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
            update_preserving_details(&mut db, 0);
            assert!(move_transaction(&mut db, id, 20261001, 20261101, -100.0).unwrap());
            assert_eq!(amount(&mut db, 20261001), None);
            assert_eq!(amount(&mut db, 20261101), Some(-100.0));
            let moved = &db.payments[0];
            assert_eq!(moved.get_flag(20261101), default_flag);
            assert!(!moved.flag.iter().any(|f| f.date == 20261101));
            assert_eq!(moved.get_note(20261101), None);
            assert_eq!(moved.get_hilighting(20261101), -1);
            assert_eq!(moved.default_hilighting(), 0x123456);
            assert_eq!(moved.get_note(20261201).as_deref(), Some("Unrelated note"));
            let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
            let reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
            assert_eq!(reopened.payments[0].get_flag(20261101), default_flag);
            assert_eq!(reopened.payments[0].get_note(20261101), None);
            assert_eq!(reopened.payments[0].get_hilighting(20261101), -1);
            assert_eq!(reopened.payments[0].default_hilighting(), 0x123456);
        }
    }

    #[test]
    fn move_preserves_explicit_none_status_over_a_paid_payment_default() {
        let mut db = db(false); let id = db.payments[0].id;
        let p = &mut db.payments[0];
        p.flag_value = FLAG_PAID;
        p.set_flag(20261001, FLAG_NONE);
        p.set_no_transaction(20261101);
        p.set_flag(20261101, budget_core::FLAG_IMPORTANT);
        update_preserving_details(&mut db, 0);
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
                update_preserving_details(&mut db, 0);
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
        update_preserving_details(&mut db, 0);
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
        update_preserving_details(&mut db, 0);
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
            rebuild(&mut db, &Cal::from_int(20261001), 12, false);
            // The Friday/Monday group is the selected visible occurrence;
            // the destination is an empty weekend date.
            let (source, destination, value) = if early { (20261002, 20261003, -100.0) } else { (20261005, 20261003, -80.0) };
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
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
        rebuild(&mut db, &Cal::from_int(20290601), 0, false);
        assert!(db.get_day(&Cal::from_int(20261001)).is_none());
        assert!(move_transaction(&mut db, id, 20261001, 20301004, -100.0).unwrap());
        rebuild(&mut db, &Cal::from_int(20301001), 0, true);
        assert_eq!(amount(&mut db, 20261001), None);
        assert_eq!(amount(&mut db, 20301004), Some(-100.0));
        assert_eq!(amount(&mut db, 20301101), Some(-100.0));
    }

    #[test]
    fn move_sticky_custom_occurrence_preserves_carry_after_both_endpoints() {
        for (source, destination) in [(20261001, 20261210), (20261201, 20261010)] {
            let mut db = db(true); let id = db.payments[0].id;
            db.payments[0].set_custom_amount(source, -150.0);
            update_preserving_details(&mut db, 0);
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
        update_preserving_details(&mut db, 0);
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
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
        assert!(move_transaction(&mut db, id, 20261001, 20261210, -150.0).unwrap());
        assert_eq!(amount(&mut db, 20261101), Some(-80.0));
        assert_eq!(amount(&mut db, 20261102), Some(-150.0));
        assert_eq!(amount(&mut db, 20261201), Some(-80.0));
        assert_eq!(amount(&mut db, 20261210), Some(-150.0));
        assert_eq!(amount(&mut db, 20270101), Some(-80.0));
        let loaded = bgt::load_str(&db.to_text(None).unwrap(), 20261027, None).unwrap();
        let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
        assert_eq!(amount(&mut reopened, 20270101), Some(-80.0));
    }

    #[test]
    fn move_tentative_keeps_unrelated_future_periods_and_metadata() {
        let mut db = db(true); db.today = 20261027;
        let id = db.payments[0].id;
        db.payments[0].set_tentative(true);
        db.payments[0].set_custom_amount(20261103, -65.0);
        db.payments[0].set_note(20261103, Some("Future period"));
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
                rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
                let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
                reopened.today = 20261027;
                rebuild(&mut reopened, &Cal::from_int(20261001), 12, false);
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
            update_preserving_details(&mut db, 0);
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
        update_preserving_details(&mut db, 0);
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
        let mut reopened = from_loaded(loaded, &Cal::from_int(20261001), 12, false);
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
            rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
        rebuild(&mut db, &Cal::from_int(20261001), 12, false);
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
}
