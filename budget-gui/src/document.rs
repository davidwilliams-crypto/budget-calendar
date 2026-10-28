//! Editing operations and optimistic disk-change checks, independent of the UI.
use budget_core::date::{is_calendar_date, DATE_MAX, YEAR_MAX};
use budget_core::{add_period, bgt, calendar_field, in_calendar, BudgetDb, Cal, Field, Payment,
    END_MODE_DATE, END_MODE_INDEFINITE, END_MODE_LOAN_PAID, END_MODE_N_PAYS};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// How many occurrences a split may look ahead for a date that keeps a month-end or numbered
/// weekday anchor (a 29 February start needs at most eight years).
const MAX_ANCHOR_RUN: usize = 1000;

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
    // An existing payment's unchanged legacy amount may lie outside the range; keep it editable.
    let unchanged_amount = id.and_then(|id| db.get_by_id(id))
        .is_some_and(|i| db.payments[i].amount.to_bits() == payment.amount.to_bits());
    if !(unchanged_amount || crate::editor::amount_in_range(payment.amount)) || payment.description.trim().is_empty() {
        return Err("Enter a description and an amount within +/-1,000,000,000,000.".into());
    }
    if payment.category.as_deref() == Some(payment.description.as_str()) {
        return Err(crate::editor::SAME_CATEGORY_ERROR.into());
    }
    Ok(())
}

pub fn put_payment(db: &mut BudgetDb, payment: Payment, id: Option<i32>) -> Result<i32, String> {
    let mut payment = payment;
    check_payment_input(db, &payment, id)?;
    if let Some(id) = id {
        let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
        payment.id = id;
        db.payments[i] = payment;
        db.update_payment(i);
        Ok(id)
    } else {
        let i = db.add(payment);
        Ok(db.payments[i].id)
    }
}

pub fn change_transaction(db: &mut BudgetDb, id: i32, date: i32, amount: f64, note: &str, flag: char) -> Result<(), String> {
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let current = db.get_day(&Cal::from_int(date)).and_then(|d| d.get(i)).map(|t| t.amount)
        .ok_or("This transaction no longer exists.")?;
    // An untouched legacy amount outside the range still allows note/status edits.
    if current.to_bits() != amount.to_bits() && !crate::editor::amount_in_range(amount) {
        return Err("Enter an amount within +/-1,000,000,000,000.".into());
    }
    let mut proposed = db.payments[i].clone();
    // set_pmt_flag: paying a tentative occurrence pins it to this date, so it
    // stops sliding to the current day once today advances.
    let freeze = proposed.is_tentative() && flag == budget_core::FLAG_PAID;
    if current != amount || freeze {
        proposed.set_custom_amount(date, amount);
        validate_loan(&proposed)?;
        preserve_future_amount(db, i, date, &mut proposed)?;
    }
    validate_loan(&proposed)?;
    proposed.set_note(date, if note.trim().is_empty() { None } else { Some(note) });
    proposed.set_flag(date, flag);
    db.payments[i] = proposed;
    db.update_payment(i);
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
    let original = original.clone();
    let today = db.today;
    let cut = cut(&original, date);
    let from = history_start(&original)?.min(cut.boundary);
    // Far enough ahead to cover a 29 February anchor's next leap day.
    let mut horizon = Cal::from_int(date);
    horizon.add(Field::Year, 9);
    let horizon = if horizon.year() > YEAR_MAX { DATE_MAX } else { horizon.get() };
    let before = schedule_snapshot(today, std::slice::from_ref(&original), from, horizon)?;
    if !before.contains_key(&date) { return Err("This transaction no longer exists.".into()); }
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
    let pin = later.is_tentative() && flag == budget_core::FLAG_PAID;
    let shown = schedule_snapshot(today, std::slice::from_ref(&later), cut.boundary, date)?;
    if pin || shown.get(&date).map(|v| v.1) != Some(amount) { later.set_custom_amount(date, amount); }
    if loan { check_loan_principal(&later)?; }
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
    if later.end_mode() == END_MODE_LOAN_PAID { check_loan_principal(&later)?; }
    validate_future_payment(&later)?;
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
            db.payments[i] = earlier;
            db.update_payment(i);
            let j = db.add(later);
            db.payments[j].id
        }
        None => {
            db.payments[i] = later;
            db.update_payment(i);
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
    format!("With these changes the payment has no occurrences from {} onward. Apply them to all occurrences instead.", budget_core::date::sdate(date))
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
            return Err(format!("This loan is already paid off before {}.", budget_core::date::sdate(cut.boundary)));
        }
    }
    if let Some(from) = normalize_from {
        let mut regular = regular.clone();
        regular.extend(regular_dates(&later, cut.boundary, last_option_date(&later, cut.boundary)));
        let amount = later.amount;
        normalize_future(&mut later, from, amount, &regular);
    }
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
    let keep_meta = |date: i32| date >= budget_core::DEFAULT_COLOR_DATE || keep(date);
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

/// The native loan scheduler subtracts every dated amount up front. If these already exceed the
/// principal it emits all of them anyway, silently overpaying the loan instead of producing a
/// smaller final payment.
fn check_loan_principal(p: &Payment) -> Result<(), String> {
    let saved: f64 = p.opt.iter().filter(|o| !o.deleted).map(|o| o.amount).sum();
    let paid = if p.amount < 0.0 { -saved } else { saved };
    if !paid.is_finite() || paid > p.loan_amount + 0.005 {
        return Err("This amount would exceed the loan principal still to pay, including custom payments. Use a smaller amount or change only this occurrence.".into());
    }
    Ok(())
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
    let mut snapshot = BudgetDb::new(&Cal::from_int(end), 0, false);
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
    db.update_payment(i);
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
    if !is_calendar_date(date) {
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
        db.update_payment(i);
    } else {
        db.delete_payment(i);
    }
    Ok(())
}

/// Restore a deleted or moved-away occurrence: the original's `restore_transaction`
/// (`pmt.clear(date)`, `db.update(pmt)`; analysis/05 §1.5 item 5).
pub fn restore(db: &mut BudgetDb, id: i32, date: i32) -> Result<(), String> {
    let i = db.get_by_id(id).ok_or("This payment no longer exists.")?;
    let mut proposed = db.payments[i].clone();
    if !proposed.opt.iter().any(|o| o.date == date && o.deleted) {
        return Err("There is no deleted transaction to restore on this date.".into());
    }
    proposed.clear(date);
    validate_loan(&proposed)?;
    db.payments[i] = proposed;
    db.update_payment(i);
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

// The core's scheduling leaves every payment's stored options, flags, notes and highlighting
// alone, so these are plain calls. They remain only as the UI's entry points (main.rs, view.rs).
pub fn rebuild(db: &mut BudgetDb, cal: &Cal, months: i32, history: bool) {
    db.rebuild_db(cal, months, history);
}

pub fn from_loaded(loaded: bgt::Loaded, cal: &Cal, months: i32, history: bool) -> BudgetDb {
    BudgetDb::from_loaded(loaded, cal, months, history)
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
            db.update_payment(0);
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
            let reopened = BudgetDb::from_loaded(loaded, &Cal::from_int(20261001), 12, false);
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
}
