//! Shared editing operations, extracted from the desktop document boundary.
use crate::date::{is_calendar_date, DATE_MAX, YEAR_MAX};
use crate::{add_period, calendar_field, in_calendar, BudgetDb, Cal, Field, Payment, END_MODE_DATE, END_MODE_INDEFINITE, END_MODE_LOAN_PAID, END_MODE_N_PAYS};
use std::collections::{BTreeMap, BTreeSet};
const MAX_ANCHOR_RUN: usize = 1000;
pub const MAX_ABS_AMOUNT: f64 = 1e12;
pub fn amount_in_range(v:f64)->bool {v.is_finite() && v.abs()<=MAX_ABS_AMOUNT}
pub const SAME_CATEGORY_ERROR: &str = "The category cannot be the same as the payment's description.";
/// Apply a payment editor result: to every occurrence, or with `from` (an existing payment edited
/// from one of its calendar occurrences, analysis/04 §1.3 step 16) only from that date onward.
/// Returns the id of the record holding the edited details.
pub fn apply_payment(db: &mut BudgetDb, payment: Payment, id: Option<i32>, from: Option<i32>) -> Result<i32, String> {
    match (id, from) {
        (Some(id), Some(date)) => put_payment_from(db, payment, id, date),
        _ => put_payment(db, payment, id),
    }
}

fn check_payment_input(db: &BudgetDb, payment: &Payment, id: Option<i32>) -> Result<(), String> {
    // The desktop editor intentionally retains unchanged legacy fields. A public
    // caller may keep those values but cannot introduce unsupported new values.
    let original = id.and_then(|id| db.get_by_id(id)).map(|i| &db.payments[i]);
    let mut checked = payment.clone();
    if let Some(old) = original {
        let known = crate::NO_WEEKENDS | crate::TENTATIVE | crate::EARLY_PAY | crate::REMINDER | crate::END_MODE_MASK | crate::IS_STICKY;
        if payment.flags & !known == old.flags & !known { checked.flags &= known; }
        if !matches!(payment.end_mode(), END_MODE_INDEFINITE | END_MODE_DATE | END_MODE_N_PAYS | END_MODE_LOAN_PAID) && payment.end_mode() == old.end_mode() {
            checked.set_end_mode(END_MODE_INDEFINITE);
        }
        if !matches!(payment.freq_type, 'd' | 'w' | 'm' | 'y' | 'W' | 'L') && payment.freq_type == old.freq_type { checked.freq_type = 'm'; }
        if !valid_status(payment.flag_value) && payment.flag_value == old.flag_value { checked.flag_value = crate::FLAG_NONE; }
        for f in &mut checked.flag {
            if !valid_status(f.value) && old.flag.iter().any(|old| old == f) { f.value = crate::FLAG_NONE; }
        }
    }
    validate_payment(&checked)?;
    if payment.is_tentative() && payment.end_mode() == END_MODE_LOAN_PAID
        && !original.is_some_and(|p| p.is_tentative() && p.end_mode() == END_MODE_LOAN_PAID) {
        return Err("A loan payment cannot be tentative. Clear Tentative or choose another end condition.".into());
    }
    let except = id.and_then(|id| db.get_by_id(id));
    if !db.is_valid_description(&payment.description, except) || !db.is_valid_category(payment.category.as_deref(), except) {
        return Err("Payment descriptions and categories conflict.".into());
    }
    // An existing payment's unchanged legacy amount may lie outside the range; keep it editable.
    let unchanged_amount = id.and_then(|id| db.get_by_id(id))
        .is_some_and(|i| db.payments[i].amount.to_bits() == payment.amount.to_bits());
    if !(unchanged_amount || amount_in_range(payment.amount)) || payment.description.trim().is_empty() {
        return Err("Enter a description and an amount within +/-1,000,000,000,000.".into());
    }
    if payment.category.as_deref() == Some(payment.description.as_str()) {
        return Err(SAME_CATEGORY_ERROR.into());
    }
    validate_loan(payment)?;
    Ok(())
}

/// Public editing boundary: validate before any schedule is placed or changed.
pub fn validate_payment(p: &Payment) -> Result<(), String> {
    valid_date(p.start_date)?;
    valid_date(p.end_date)?;
    if !matches!(p.freq_type, 'd' | 'w' | 'm' | 'y' | 'W' | 'L') {
        return Err("Unsupported recurrence type.".into());
    }
    if p.flags & !(crate::NO_WEEKENDS | crate::TENTATIVE | crate::EARLY_PAY | crate::REMINDER | crate::END_MODE_MASK | crate::IS_STICKY) != 0
        || !matches!(p.end_mode(), END_MODE_INDEFINITE | END_MODE_DATE | END_MODE_N_PAYS | END_MODE_LOAN_PAID)
        || !valid_status(p.flag_value) || p.flag.iter().any(|f| !valid_status(f.value)) {
        return Err("Unsupported payment flags or status.".into());
    }
    if !p.amount.is_finite() || !p.loan_amount.is_finite() || p.opt.iter().any(|o| !o.amount.is_finite()) {
        return Err("Amounts must be finite.".into());
    }
    if p.description.trim().is_empty() || p.description.len() > 4096 || p.category.as_ref().is_some_and(|s| s.len() > 4096) {
        return Err("Enter a description of at most 4096 bytes.".into());
    }
    if p.end_mode() == END_MODE_DATE && p.end_date < p.start_date { return Err("Payment ends before it starts.".into()); }
    if !(0..=100000).contains(&p.grace_period_days) { return Err("Grace period is out of range.".into()); }
    for date in p.opt.iter().map(|o| o.date).chain(p.flag.iter().map(|f| f.date)) { valid_date(date)?; }
    for n in &p.note { if n.date != crate::URL_NOTE_DATE { valid_date(n.date)?; } if n.note.len() > 65536 { return Err("Note is too long.".into()); } }
    for h in &p.hilighting { if h.date != crate::DEFAULT_COLOR_DATE { valid_date(h.date)?; } }
    if p.end_mode() == END_MODE_LOAN_PAID && (p.loan_amount <= 0.0 || p.amount == 0.0) { return Err("Loan needs positive principal and nonzero payment.".into()); }
    validate_schedule_bounds(p)
}

pub fn valid_status(flag: char) -> bool { matches!(flag, crate::FLAG_NONE | crate::FLAG_PAID | crate::FLAG_IMPORTANT) }

pub fn put_payment(db: &mut BudgetDb, payment: Payment, id: Option<i32>) -> Result<i32, String> {
    let mut payment = payment;
    align_end_date(&mut payment);
    check_payment_input(db, &payment, id)?;
    if let Some(id) = id {
        let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
        payment.id = id;
        db.replace_payment(i, payment);
        Ok(id)
    } else {
        let i = db.add(payment);
        Ok(db.payments[i].id)
    }
}

/// Whether applying these values to one occurrence would change nothing: the amount, note and status
/// are what the occurrence already has (an editor applied untouched). Such an apply leaves the budget
/// clean instead of writing an explicit status entry.
pub fn transaction_unchanged(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> bool {
    let Some(i) = db.get_by_id(id) else { return false };
    let Some(current) = db.get_day(&Cal::from_int(date)).and_then(|d| d.get(i)).map(|t| t.amount) else { return false };
    let old = db.payments[i].get_note(date).unwrap_or_default();
    current.to_bits() == amount.to_bits()
        && (old == note || (old.trim().is_empty() && note.trim().is_empty()))
        && db.payments[i].get_flag(date) == flag
}

/// Whether `edited` is the payment `id` exactly as it is stored (an editor applied untouched).
pub fn payment_unchanged(db: &BudgetDb, id: i32, edited: &Payment) -> bool {
    // An editor opened before the window moved carries older placed days and so counts as a change:
    // that only means the apply goes through as it always did.
    db.get_by_id(id).is_some_and(|i| db.payments[i] == *edited)
}

/// The same for "this and all future occurrences": the occurrence is unchanged and every later one
/// already shows the amount, so nothing would be split or rewritten.
pub fn future_unchanged(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> bool {
    transaction_unchanged(db, id, date, amount, note, flag) && future_already_shows(db, id, date, amount)
}

/// Whether every occurrence of payment `id` from `date` on already shows `amount`, so a "this and all
/// future occurrences" apply rewrites nothing later (it then only changes the one occurrence).
pub fn future_already_shows(db: &mut BudgetDb, id: i32, date: i32, amount: f64) -> bool {
    let Some(i) = db.get_by_id(id) else { return false };
    let original = db.payments[i].clone();
    if validate_future_payment(&original).is_err() { return false; }
    let cut = cut(&original, date);
    let Ok(from) = history_start(&original).map(|f| f.min(cut.boundary)) else { return false };
    let mut horizon = Cal::from_int(date);
    horizon.add(Field::Year, 9);
    let horizon = if horizon.year() > YEAR_MAX { DATE_MAX } else { horizon.get() };
    let Ok(before) = schedule_snapshot(db.today, std::slice::from_ref(&original), from, horizon) else { return false };
    before.contains_key(&date) && future_shows_amount(&original, &before, date, amount)
}

pub fn change_transaction(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> Result<(), String> {
    valid_date(date)?;
    if !valid_status(flag) { return Err("Unsupported status.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let current = db.get_day(&Cal::from_int(date)).and_then(|d| d.get(i)).map(|t| t.amount)
        .ok_or("This transaction no longer exists.")?;
    // An untouched legacy amount outside the range still allows note/status edits.
    if current.to_bits() != amount.to_bits() && !amount_in_range(amount) {
        return Err("Enter an amount within +/-1,000,000,000,000.".into());
    }
    let mut proposed = db.payments[i].clone();
    // set_pmt_flag: paying a tentative occurrence pins it to this date, so it
    // stops sliding to the current day once today advances.
    let freeze = proposed.is_tentative() && flag == crate::FLAG_PAID;
    if current != amount || freeze {
        proposed.set_custom_amount(date, amount);
        validate_loan(&proposed)?;
        preserve_future_amount(db, i, date, &mut proposed)?;
    }
    validate_loan(&proposed)?;
    proposed.set_note(date, if note.trim().is_empty() { None } else { Some(note) });
    proposed.set_flag(date, flag);
    db.replace_payment(i, proposed);
    Ok(())
}

/// Change the selected occurrence and every later one to `amount` by splitting the payment there,
/// as the original's `splitPayment` does (analysis/04 §18, 07 §1): the record keeps every earlier
/// occurrence unchanged and ends before the selected one; a new record with the same details and
/// the new amount continues the same dates from it. Later dated amounts (custom or moved
/// occurrences) take the new amount; deletions, moved dates, notes, status and highlighting stay
/// on their dates. The note and status apply to the selected date only.
pub fn change_future_transactions(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> Result<(), String> {
    valid_date(date)?;
    if !amount_in_range(amount) { return Err("Enter an amount within +/-1,000,000,000,000.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let original = db.payments[i].clone();
    validate_future_payment(&original)?;
    if !matches!(flag, crate::FLAG_NONE | crate::FLAG_PAID | crate::FLAG_IMPORTANT) {
        return Err("Choose a valid transaction status.".into());
    }
    let today = db.today;
    let cut = cut(&original, date);
    let from = history_start(&original)?.min(cut.boundary);
    // Far enough ahead to cover a 29 February anchor's next leap day.
    let mut horizon = Cal::from_int(date);
    horizon.add(Field::Year, 9);
    let horizon = if horizon.year() > YEAR_MAX { DATE_MAX } else { horizon.get() };
    let before = schedule_snapshot(today, std::slice::from_ref(&original), from, horizon)?;
    if !before.contains_key(&date) { return Err("This transaction no longer exists.".into()); }
    // Occurrences from the date on already show this amount, so nothing needs a new record: only the
    // selected occurrence's note and status change, as with "Only this occurrence".
    if future_shows_amount(&original, &before, date, amount) {
        return change_transaction(db, id, date, amount, note, flag);
    }
    let loan = original.end_mode() == crate::END_MODE_LOAN_PAID;
    if loan && (original.is_sticky() || original.is_tentative() || amount == 0.0 || amount.is_sign_negative() != original.amount.is_sign_negative()) {
        return Err("Future amount changes require a nonzero loan payment in its existing direction, without sticky or tentative scheduling. Change only this occurrence instead.".into());
    }
    let regular = regular_dates(&original, cut.boundary, last_option_date(&original, cut.boundary));
    let (earlier, mut later) = match cut.first {
        Some((k, nominal)) if k > 0 => {
            let mut edited = original.clone();
            edited.amount = amount;
            let paid = loan_paid(&original, &before, cut.boundary);
            (Some(earlier_part(&original, &cut)), later_part(&edited, &cut, (k, nominal), k, paid, Some(date), &regular)?)
        }
        first => {
            // Nothing is scheduled before the date, so the whole payment is "from the date on"
            // (or only dated amounts remain after it): change the record in place.
            let mut later = original.clone();
            if first.is_some() { later.amount = amount; }
            normalize_future(&mut later, date, amount, &regular);
            (None, later)
        }
    };
    later.set_note(date, if note.trim().is_empty() { None } else { Some(note) });
    later.set_flag(date, flag);
    // The selected occurrence shows exactly the entered amount, also where a business day adds up
    // weekend occurrences or a sticky payment carries an earlier dated amount. As in
    // `change_transaction` (set_pmt_flag, 04 §18), paying a tentative occurrence pins it there.
    let pin = later.is_tentative() && flag == crate::FLAG_PAID;
    let shown = schedule_snapshot(today, std::slice::from_ref(&later), cut.boundary, date)?;
    if pin || shown.get(&date).map(|v| v.1) != Some(amount) { later.set_custom_amount(date, amount); }
    validate_future_payment(&later)?;
    // The original's self-check (splitPayment compares the balance before the split date):
    // every earlier transaction must be unchanged, and no later one may move or appear.
    let parts: Vec<Payment> = earlier.iter().cloned().chain(std::iter::once(later.clone())).collect();
    let after = schedule_snapshot(today, &parts, from, horizon)?;
    if before.range(..date).ne(after.range(..date)) || after.get(&date).map(|v| v.1) != Some(amount) {
        return Err("This schedule cannot retain its earlier transactions safely. Change only this occurrence instead.".into());
    }
    if !loan && before.range(date..).map(|(d, _)| d).ne(after.range(date..).map(|(d, _)| d)) {
        return Err("This schedule cannot change future amounts without moving transactions. Change only this occurrence instead.".into());
    }
    // No live budget state, cached days or update counters are touched until
    // every validation and native-scheduler comparison has succeeded.
    commit_split(db, i, earlier, later);
    Ok(())
}

/// Edit an existing payment from one of its occurrences onward (the original's "Change the
/// FUTURE transactions only starting on <date>", analysis/04 §2 and §1.3 step 18): the record
/// keeps every earlier occurrence unchanged and ends before `date`; a new record with the edited
/// details continues from it. Manually adjusted transactions are not affected, as the original's
/// dialog notes. Without earlier occurrences this is the same as editing every occurrence.
fn put_payment_from(db: &mut BudgetDb, edited: Payment, id: i32, date: i32) -> Result<i32, String> {
    valid_date(date)?;
    check_payment_input(db, &edited, Some(id))?;
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let original = db.payments[i].clone();
    let cut = cut(&original, date);
    let Some((k, nominal)) = cut.first.filter(|&(k, _)| k > 0) else { return put_payment(db, edited, Some(id)) };
    validate_future_payment(&original)?;
    if edited.end_mode() == END_MODE_LOAN_PAID && (edited.is_sticky() || edited.is_tentative() || edited.amount == 0.0
        || edited.amount.is_sign_negative() != original.amount.is_sign_negative()) {
        return Err("A loan changed from a date onward needs a nonzero payment in its existing direction, without sticky or tentative scheduling. Apply it to all occurrences instead.".into());
    }
    let today = db.today;
    let from = history_start(&original)?.min(cut.boundary);
    let mut last = Cal::from_int(cut.boundary);
    last.add(Field::Day, -1);
    let before = schedule_snapshot(today, std::slice::from_ref(&original), from, last.get())?;
    let earlier = earlier_part(&original, &cut);
    let paid = loan_paid(&original, &before, cut.boundary);
    // An unchanged recurrence continues the same dates; a changed one is followed from the date.
    // Either way a number of payments counts the ones already made before the date.
    let same_recurrence = edited.start_date == original.start_date && edited.freq_type == original.freq_type
        && edited.frequency == original.frequency;
    let first = if same_recurrence { (k, nominal) } else {
        // Counted and loan schedules are open-ended here; `later_part` applies their limits.
        let mut schedule = edited.clone();
        if matches!(schedule.end_mode(), END_MODE_LOAN_PAID | END_MODE_N_PAYS) { schedule.set_end_mode(END_MODE_INDEFINITE); }
        self::cut(&schedule, date).first.ok_or_else(|| no_occurrences_from(date))?
    };
    let mut later = later_part(&edited, &cut, first, k, paid, None, &BTreeSet::new())?;
    // A sticky payment carries its last dated amount into the new record unless the amount changed.
    if original.is_sticky() && later.is_sticky() && !later.is_tentative() && edited.amount == original.amount {
        let carried = original.amount_before_date(cut.boundary);
        let first_shown = later.opt.iter().filter(|o| !o.deleted).map(|o| o.date).min()
            .map_or(actual_date(&later, later.start_date), |d| d.min(actual_date(&later, later.start_date)));
        if carried != later.amount && later.option_by_date(first_shown).is_none() {
            later.set_custom_amount(first_shown, carried);
        }
    }
    validate_future_payment(&later)?;
    // The two records now coexist, so a description may not be the other's category (the editor only
    // compared the edit with other payments); a reload would silently rename the category.
    if later.category.as_deref() == Some(earlier.description.as_str()) || earlier.category.as_deref() == Some(later.description.as_str()) {
        return Err(format!("From a date onward, the earlier transactions stay named \"{}\"{}. Choose a different description or category, or apply the change to all occurrences.",
            earlier.description, earlier.category.as_deref().map_or_else(String::new, |c| format!(" in category \"{c}\""))));
    }
    let after = schedule_snapshot(today, &[earlier.clone(), later.clone()], from, last.get())?;
    if before != after {
        return Err("This change cannot keep the earlier transactions unchanged. Apply it to all occurrences instead.".into());
    }
    Ok(commit_split(db, i, Some(earlier), later))
}

/// Replace payment `i` by its earlier part (when split) and add the later part as a new record.
/// Returns the id of the record holding the later occurrences.
fn commit_split(db: &mut BudgetDb, i: usize, earlier: Option<Payment>, later: Payment) -> i32 {
    match earlier {
        Some(earlier) => {
            db.replace_payment(i, earlier);
            let j = db.add(later);
            db.payments[j].id
        }
        None => {
            db.replace_payment(i, later);
            db.payments[i].id
        }
    }
}

/// Whether editing `p` from its occurrence on `date` splits it: some regular occurrence is shown
/// before the date and another on or after it.
pub fn splits_at(p: &Payment, date: i32) -> bool {
    matches!(cut(p, date).first, Some((k, _)) if k > 0)
}

fn no_occurrences_from(date: i32) -> String {
    format!("With these changes the payment has no occurrences from {} onward. Apply them to all occurrences instead.", crate::date::sdate(date))
}

/// A payment's dates split at a date (analysis/04 §18 `splitPayment`).
struct Cut {
    /// Options, flags, notes and highlighting dated on or after this belong to the later part.
    boundary: i32,
    /// Index and nominal date of the first regular occurrence shown on or after the split date.
    first: Option<(i32, i32)>,
}

fn cut(p: &Payment, date: i32) -> Cut {
    let mut first = None;
    if p.is_tentative() {
        // Before the first pay period no period holds the date, so the whole payment continues from it,
        // as a regular payment does when its first occurrence lies after the date.
        if date < p.start_date { return Cut { boundary: date, first: Some((0, p.start_date)) }; }
        // A tentative occurrence slides within its pay period, so the period holding the date splits.
        nominal_slots(p, |n, nominal| { if nominal <= date { first = Some((n, nominal)); true } else { false } });
        first = first.filter(|&(n, nominal)| {
            let mut end = Cal::from_int(nominal);
            add_period(&mut end, calendar_field(p.freq_type), p.frequency, p.start_date, n + 1);
            date < end.get()
        });
        Cut { boundary: first.map_or(date, |(_, nominal)| nominal), first }
    } else {
        // Weekend shifting is monotonic, so every earlier slot is shown before the date.
        nominal_slots(p, |n, nominal| { if actual_date(p, nominal) >= date { first = Some((n, nominal)); false } else { true } });
        Cut { boundary: date, first }
    }
}

/// Visit the nominal dates (before weekend shifting or tentative sliding) of a payment's regular
/// slots in order with their index, as the scheduler places them, until `visit` returns false.
/// Count-limited schedules (and loans) use the synchronised `n_pays`; dated ones stop after `end_date`.
fn nominal_slots(p: &Payment, mut visit: impl FnMut(i32, i32) -> bool) {
    for (n, c) in p.slots().enumerate() {
        let (n, date) = (n as i32, c.get());
        let ended = match p.end_mode() {
            END_MODE_INDEFINITE => false,
            END_MODE_DATE => n > 0 && date > p.end_date,
            _ => n >= p.n_pays.max(1),
        };
        if ended || !in_calendar(&c) || !visit(n, date) { return; }
    }
}

/// Whether a payment starting on `date` continues `p`'s dates. Monthly and yearly repeats from day
/// 29–31 fall back to the month end, and a fifth weekday to the fourth, so starting on such a
/// shortened occurrence would keep the shorter day.
fn keeps_anchor(p: &Payment, date: i32) -> bool {
    let (start, at) = (Cal::from_int(p.start_date), Cal::from_int(date));
    match calendar_field(p.freq_type) {
        Field::Month | Field::Year => at.day() == start.day(),
        Field::NthWeekday => at.week_of_month() == start.week_of_month(),
        _ => true,
    }
}

/// The original record's part: every regular occurrence before the cut and the dated data before
/// its boundary. A count limit keeps its count; any other end condition ends the day before.
fn earlier_part(p: &Payment, cut: &Cut) -> Payment {
    let (k, nominal) = cut.first.expect("an earlier part needs a split occurrence");
    let mut earlier = p.clone();
    retain_dated(&mut earlier, |date| date < cut.boundary);
    if p.end_mode() == END_MODE_N_PAYS {
        earlier.n_pays = k;
    } else {
        let mut end = Cal::from_int(nominal);
        end.add(Field::Day, -1);
        earlier.set_end_mode(END_MODE_DATE);
        earlier.end_date = end.get();
    }
    earlier
}

/// The new record continuing `edited`'s occurrences from its slot `first` (the clone in the
/// original's `splitPayment`), with the dated data on or after the cut's boundary. Its end
/// condition continues the edited one: the number of payments left after the `made` ones already
/// shown before the cut, the same end date, or the loan principal not yet paid. When the first
/// occurrence fell back from a day 29–31 or fifth-weekday anchor, the record starts at the next
/// occurrence that keeps it and the ones before become dated amounts. With `normalize_from`, dated
/// amounts from that date on take the new amount (see `normalize_future`); `regular` adds the
/// original's scheduled dates to the new record's.
fn later_part(edited: &Payment, cut: &Cut, (k, _): (i32, i32), made: i32, loan_paid: f64,
    normalize_from: Option<i32>, regular: &BTreeSet<i32>) -> Result<Payment, String> {
    let loan = edited.end_mode() == END_MODE_LOAN_PAID;
    let remaining = (edited.end_mode() == END_MODE_N_PAYS).then(|| edited.n_pays.max(1) - made);
    if remaining.is_some_and(|r| r < 1) { return Err(no_occurrences_from(cut.boundary)); }
    // Counted schedules stop after `remaining`; a loan continues until its principal is paid.
    let mut schedule = edited.clone();
    if loan || remaining.is_some() { schedule.set_end_mode(END_MODE_INDEFINITE); }
    let (mut run, mut anchored) = (Vec::new(), false);
    nominal_slots(&schedule, |n, nominal| {
        if n < k { return true; }
        run.push(nominal);
        anchored = keeps_anchor(edited, nominal);
        !anchored && run.len() <= MAX_ANCHOR_RUN && remaining.is_none_or(|r| run.len() < r as usize)
    });
    let Some(&first) = run.first() else { return Err(no_occurrences_from(cut.boundary)) };
    let (start, extras, single) = if anchored {
        (run[run.len() - 1], run[..run.len() - 1].to_vec(), false)
    } else if run.len() > MAX_ANCHOR_RUN {
        return Err("This schedule cannot be continued from this date. Change only this occurrence instead.".into());
    } else {
        // The schedule ends before the anchor day recurs: one regular payment, the rest dated.
        (first, run[1..].to_vec(), true)
    };
    let mut later = edited.clone();
    later.id = 0;
    retain_dated(&mut later, |date| date >= cut.boundary);
    later.start_date = start;
    if single {
        later.set_end_mode(END_MODE_N_PAYS);
        later.n_pays = 1;
    } else if let Some(remaining) = remaining {
        later.n_pays = remaining - extras.len() as i32;
    } else if loan {
        later.loan_amount = edited.loan_amount.abs() - loan_paid;
        if later.loan_amount < 0.005 {
            return Err(format!("This loan is already paid off before {}.", crate::date::sdate(cut.boundary)));
        }
    }
    if let Some(from) = normalize_from {
        let mut regular = regular.clone();
        regular.extend(regular_dates(&later, cut.boundary, last_option_date(&later, cut.boundary)));
        let amount = later.amount;
        normalize_future(&mut later, from, amount, &regular);
    }
    align_end_date(&mut later);
    let direction = if later.amount < 0.0 { -1.0 } else { 1.0 };
    let mut principal = later.loan_amount - direction * later.opt.iter().filter(|o| !o.deleted).map(|o| o.amount).sum::<f64>();
    for nominal in extras {
        let date = if later.is_tentative() { nominal } else { actual_date(&later, nominal) };
        // A deleted occurrence stays deleted and an existing dated amount stays.
        if later.option_by_date(date).is_some() { continue; }
        let mut value = if later.is_sticky() { later.amount_before_date(date) } else { later.amount };
        if loan {
            let paid = value.abs().min(principal.max(0.0));
            if paid < 0.005 { break; }
            principal -= paid;
            value = direction * paid;
        }
        later.set_custom_amount(date, value);
    }
    Ok(later)
}

/// Give dated amounts from `from` on the new amount, as the original's split did by dropping them:
/// an amount on a scheduled date is removed so the occurrence follows the payment again, and a
/// moved or extra occurrence (or a tentative one, whose date marks its place in the period) keeps
/// its date. Deletions stay.
fn normalize_future(p: &mut Payment, from: i32, amount: f64, regular: &BTreeSet<i32>) {
    let tentative = p.is_tentative();
    p.opt.retain_mut(|o| {
        if o.deleted || o.date < from { return true; }
        if !tentative && regular.contains(&o.date) { return false; }
        o.amount = amount;
        true
    });
}

/// Dates in `[from, until]` on which `p` shows a regular (non-tentative) occurrence.
fn regular_dates(p: &Payment, from: i32, until: i32) -> BTreeSet<i32> {
    let mut dates = BTreeSet::new();
    if p.is_tentative() || until < from { return dates; }
    let mut schedule = p.clone();
    if schedule.end_mode() == END_MODE_LOAN_PAID { schedule.set_end_mode(END_MODE_INDEFINITE); }
    nominal_slots(&schedule, |_, nominal| {
        // Weekend shifting moves an occurrence by at most two days.
        if nominal > until + 2 { return false; }
        let date = actual_date(&schedule, nominal);
        if (from..=until).contains(&date) { dates.insert(date); }
        true
    });
    dates
}

fn last_option_date(p: &Payment, from: i32) -> i32 {
    p.opt.iter().filter(|o| !o.deleted && o.date >= from).map(|o| o.date).max().unwrap_or(from)
}

/// Keep the dated options, flags, notes and highlighting for which `keep` holds. The payment-wide
/// default colour and URL are stored at sentinel dates and stay with every part.
fn retain_dated(p: &mut Payment, keep: impl Fn(i32) -> bool) {
    let keep_meta = |date: i32| date >= crate::DEFAULT_COLOR_DATE || keep(date);
    p.opt.retain(|o| keep(o.date));
    p.flag.retain(|f| keep(f.date));
    p.note.retain(|n| keep_meta(n.date));
    p.hilighting.retain(|h| keep_meta(h.date));
}

/// Principal paid by the transactions shown before `boundary` (meaningful for loans only).
fn loan_paid(p: &Payment, snapshot: &BTreeMap<i32, (usize, f64)>, boundary: i32) -> f64 {
    let total: f64 = snapshot.range(..boundary).map(|(_, v)| v.1).sum();
    if p.amount < 0.0 { -total } else { total }
}

/// Whether every occurrence from `date` on already shows `amount` and no saved amount from `date` on
/// differs from it (`shown` is the snapshot from `date`), so changing the future amounts changes
/// nothing. Several occurrences on one day (a weekend group) do not count as showing `amount`.
fn future_shows_amount(p: &Payment, shown: &BTreeMap<i32, (usize, f64)>, date: i32, amount: f64) -> bool {
    let same = |value: f64| value.to_bits() == amount.to_bits();
    shown.range(date..).all(|(_, &(count, total))| count == 1 && same(total))
        && p.opt.iter().all(|o| o.deleted || o.date < date || same(o.amount))
}

/// The first date on which `p` can show a transaction.
fn history_start(p: &Payment) -> Result<i32, String> {
    let mut first = p.start_date;
    for option in &p.opt { if !option.deleted { first = first.min(option.date); } }
    // Early weekend placement can precede the recurrence start.
    if !p.is_tentative() { first = first.min(actual_date(p, p.start_date)); }
    valid_date(first)?;
    Ok(first)
}

fn validate_future_payment(p: &Payment) -> Result<(), String> {
    valid_date(p.start_date)?;
    if !p.amount.is_finite() || p.opt.iter().any(|o| !o.amount.is_finite()) {
        return Err("This payment has a non-finite saved amount. Repair the payment before changing future amounts.".into());
    }
    if !matches!(p.end_mode(), crate::END_MODE_INDEFINITE | crate::END_MODE_DATE | crate::END_MODE_N_PAYS | crate::END_MODE_LOAN_PAID) {
        return Err("This payment has an unsupported end condition.".into());
    }
    if p.end_mode() == crate::END_MODE_DATE {
        valid_date(p.end_date)?;
        if p.end_date < p.start_date { return Err("This payment ends before it starts.".into()); }
    }
    if p.end_mode() == crate::END_MODE_LOAN_PAID
        && (!p.loan_amount.is_finite() || p.loan_amount <= 0.0 || p.amount == 0.0) {
        return Err("This loan needs a positive finite principal and a nonzero payment.".into());
    }
    validate_schedule_bounds(p)?;
    let mut dates = std::collections::BTreeSet::new();
    for option in &p.opt {
        valid_date(option.date)?;
        if !dates.insert(option.date) {
            return Err("This payment has duplicate saved amount entries. Resolve them before changing future amounts.".into());
        }
    }
    Ok(())
}

/// The number and total of `payments`' transactions on each day of `[from, to]`, scheduled in an
/// isolated budget so a change can be compared with the original schedule before anything is
/// applied.
fn schedule_snapshot(today: i32, payments: &[Payment], from: i32, to: i32) -> Result<BTreeMap<i32, (usize, f64)>, String> {
    // A window shorter than two months would start at the payments' history instead of `from`.
    let mut end = Cal::from_int(from);
    end.add(Field::Month, 3);
    let end = end.get().max(to).min(DATE_MAX);
    // Bound the work before allocating native BudgetDay storage for legacy files
    // with millennia of daily history. Rejection leaves the live budget intact.
    if Cal::diff_int(from, end) > 366_000 {
        return Err("This payment has more than 1,000 years of history. Change only this occurrence instead.".into());
    }
    let mut snapshot = BudgetDb::new_with_today(&Cal::from_int(end), 0, false, today);
    snapshot.today = today;
    snapshot.payments.extend(payments.iter().cloned());
    snapshot.adjustments.set_custom_amount(from, 0.0);
    snapshot.rebuild_db(&Cal::from_int(end), 0, false);
    let mut values = BTreeMap::new();
    let mut day = Cal::from_int(from);
    while day.get() <= to {
        if let Some(d) = snapshot.get_day(&day).filter(|d| !d.transactions.is_empty()) {
            values.insert(day.get(), (d.transactions.len(), d.transactions.iter().map(|t| t.amount).sum()));
        }
        day.add(Field::Day, 1);
    }
    Ok(values)
}

/// Move one scheduled occurrence, retaining its metadata and all unrelated exceptions.
pub fn move_transaction(db: &mut BudgetDb, id: i32, from: i32, to: i32, amount: f64) -> Result<bool, String> {
    valid_date(from)?;
    valid_date(to)?;
    if !amount_in_range(amount) { return Err("Enter an amount within +/-1,000,000,000,000.".into()); }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let original = db.payments[i].clone();
    // Validate before running even an isolated scheduler: a malformed loan or
    // recurrence must never turn a rejected drop into an unbounded calculation.
    validate_schedule_bounds(&original)?;
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
    // The moved occurrence's status, note and highlighting replace the destination's where it sets
    // them; a value it does not set leaves the destination's in place (the original's moveTransaction).
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
    db.replace_payment(i, proposed);
    db.calc_balances(None);
    Ok(true)
}

/// Set an absolute closing-balance checkpoint, or remove just that checkpoint.
pub fn set_balance(db: &mut BudgetDb, date: i32, amount: Option<f64>) -> Result<bool, String> {
    valid_date(date)?;
    if amount.is_some_and(|v| !amount_in_range(v)) { return Err("Enter a balance within +/-1,000,000,000,000.".into()); }
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
    if !is_calendar_date(date) {
        return Err("Enter a valid calendar date between years 1000 and 9999.".into());
    }
    Ok(())
}

fn isolated_payment_db(today: i32, payment: &Payment, date: i32) -> BudgetDb {
    let cal = Cal::from_int(date);
    let mut snapshot = BudgetDb::new_with_today(&cal, 0, false, today);
    snapshot.today = today;
    snapshot.payments.push(payment.clone());
    // Keep storage near the queried date even for a very old recurrence. The
    // synthetic checkpoint affects balances only, never transaction scheduling.
    let mut anchor = cal;
    anchor.set(Field::Day, 1);
    anchor.add(Field::Month, -3);
    snapshot.adjustments.set_custom_amount(anchor.get(), 0.0);
    snapshot.rebuild_db(&cal, 0, false);
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

/// The date a (non-tentative) payment shows its occurrence scheduled on `scheduled`.
fn actual_date(p: &Payment, scheduled: i32) -> i32 {
    p.shown_date(&Cal::from_int(scheduled)).get()
}

fn validate_loan(proposed: &Payment) -> Result<(), String> {
    if proposed.end_mode() == crate::END_MODE_LOAN_PAID {
        validate_schedule_bounds(proposed)?;
    }
    Ok(())
}

pub fn delete(db: &mut BudgetDb, id: i32, date: Option<i32>) -> Result<(), String> {
    if let Some(date) = date { valid_date(date)?; }
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    if let Some(date) = date {
        let mut proposed = db.payments[i].clone();
        proposed.set_no_transaction(date);
        validate_loan(&proposed)?;
        preserve_future_amount(db, i, date, &mut proposed)?;
        validate_loan(&proposed)?;
        db.replace_payment(i, proposed);
    } else {
        db.delete_payment(i);
    }
    Ok(())
}

/// Restore a deleted or moved-away occurrence: the original's `restore_transaction`
/// (`pmt.clear(date)`, `db.update(pmt)`; analysis/05 §1.5 item 5).
pub fn restore(db: &mut BudgetDb, id: i32, date: i32) -> Result<(), String> {
    valid_date(date)?;
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let mut proposed = db.payments[i].clone();
    // A tentative period's deletion is shown on today (or the period's first day) wherever it is
    // stored, because scheduling slides it without changing the stored date (analysis/01 §3.7).
    let (first, last) = match proposed.is_tentative().then(|| proposed.period_containing(date)).flatten() {
        Some(period) => period,
        None => (date, date),
    };
    let deleted: Vec<i32> = proposed.opt.iter().filter(|o| o.deleted && first <= o.date && o.date <= last).map(|o| o.date).collect();
    if deleted.is_empty() {
        return Err("There is no deleted transaction to restore on this date.".into());
    }
    for d in deleted { proposed.clear(d); }
    validate_loan(&proposed)?;
    db.replace_payment(i, proposed);
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
        if actual > DATE_MAX { return Err("This occurrence cannot be changed without extending beyond year 9999.".into()); }
        // Several daily weekend occurrences can land on one business day. The
        // scheduler adds their amounts, then applies the date's explicit option.
        let mut slots = vec![scheduled];
        let mut next = db.next_regular_payment_after_date(i, scheduled);
        while next >= 0 && actual_date(original, next) == actual {
            scheduled = next;
            slots.push(next);
            next = db.next_regular_payment_after_date(i, scheduled);
        }
        let merged = |p: &Payment| slots.iter().map(|&s| p.amount_before_date(s)).sum::<f64>();
        let original_option = original.option_by_date(actual);
        if actual > date && !original_option.is_some_and(|o| o.deleted) {
            let desired = original_option.map_or_else(|| merged(original), |o| o.amount);
            let current = proposed.option_by_date(actual).map_or_else(|| merged(proposed), |o| o.amount);
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

// Bound all values before the scheduler multiplies intervals or loops through finite payments.
fn period_span(p: &Payment) -> i32 {
    let start = Cal::from_int(p.start_date);
    match p.freq_type {
        'd' => Cal::diff(&start, &Cal::from_int(DATE_MAX)),
        'w' => Cal::diff(&start, &Cal::from_int(DATE_MAX)) / 7,
        'y' => YEAR_MAX - start.year(),
        _ => (YEAR_MAX - start.year()) * 12 + 11 - start.month(),
    }
}
pub fn max_frequency(p: &Payment) -> i32 { period_span(p).max(1) }
/// The largest repeat interval a saved file keeps: the loader clamps a larger one on reopen (SPEC.md §6).
const MAX_STORED_FREQUENCY: i32 = 100_000;

/// The loader raises a stored end date to the start date (budget-core `bgt`). The in-memory end date
/// is raised the same way, so saving and reopening give one end date. A date end is never moved:
/// the editor refuses an end before the start instead.
pub fn align_end_date(p: &mut Payment) {
    if p.end_mode() != END_MODE_DATE { p.end_date = p.end_date.max(p.start_date); }
}

pub fn validate_schedule_bounds(p: &Payment) -> Result<(), String> {
    if p.frequency <= 0 || p.frequency > max_frequency(p) {
        return Err("Frequency would exceed the supported calendar range (through year 9999).".into());
    }
    if p.frequency > MAX_STORED_FREQUENCY {
        return Err("Repeat every cannot exceed 100,000: a larger interval would change when the budget is reopened.".into());
    }
    // The engine's numbered-weekday recurrence is monthly regardless of its stored interval.
    let frequency = if p.freq_type == 'W' { 1 } else { p.frequency };
    let max_count = period_span(p) / frequency + 1;
    match p.end_mode() {
        END_MODE_N_PAYS if p.n_pays <= 0 || p.n_pays > max_count => {
            return Err("Number of payments would extend beyond year 9999.".into());
        }
        END_MODE_LOAN_PAID => {
            let sum: f64 = p.opt.iter().filter(|o| !o.deleted).map(|o| o.amount).sum();
            let paid = if p.amount < 0.0 { -sum } else { sum };
            // The scheduler subtracts every saved payment from the principal up front, so more than the
            // principal would overpay the loan silently. Every edit path refuses it, naming the principal.
            if !paid.is_finite() || paid > p.loan_amount.abs() + 0.005 {
                return Err(format!("The saved payments would exceed the loan principal of {}, custom payments included. Lower a payment or raise the principal.",
                    crate::numfmt::format_frac2(p.loan_amount.abs())));
            }
            let remaining = p.loan_amount.abs() - paid;
            // Every exception can displace one regular payment. This conservative upper bound
            // prevents a huge/near-zero loan from locking the scheduler or overflowing dates.
            let count = (remaining / p.amount.abs()).ceil() + p.opt.len() as f64;
            if !count.is_finite() || count > max_count as f64 {
                return Err("Loan would require payments beyond year 9999; increase the payment or reduce the loan amount.".into());
            }
        }
        _ => {}
    }
    Ok(())
}

