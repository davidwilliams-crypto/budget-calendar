//! Schedule generation: `calendarField`, `addPeriod`, `calcLoanSchedule`, `update(Payment, boolean)`,
//! and the regular-date enumerations.

use crate::date::{Cal, Field, DATE_MAX, SATURDAY, SUNDAY};
use crate::db::BudgetDb;
use crate::model::*;

pub fn calendar_field(freq_type: char) -> Field {
    match freq_type {
        'd' => Field::Day,
        'w' => Field::Week,
        'm' => Field::Month,
        'y' => Field::Year,
        'W' => Field::NthWeekday,
        'L' => Field::LastWeekday,
        _ => Field::Month,
    }
}

/// `addPeriod(cal, field, frequency, startDate, n)` — `n` is the 1-based index of the period.
pub fn add_period(cal: &mut Cal, field: Field, frequency: i32, start_date: i32, n: i32) {
    let frequency = frequency.clamp(1, 1_000_000);
    if field == Field::NthWeekday {
        cal.set_int(start_date);
        cal.add(field, n);
    } else if (field == Field::Month || field == Field::Year) && cal.day() > 27 {
        cal.set_int(start_date);
        cal.add(field, (frequency as i64 * n as i64).clamp(-1_000_000, 1_000_000) as i32);
    } else {
        cal.add(field, frequency);
    }
}

/// Whether a slot date is on the calendar. Every schedule walk stops after `DATE_MAX`, so absurd
/// frequency × count combinations cannot hang a load (or overflow the year).
pub fn in_calendar(c: &Cal) -> bool { c.get() <= DATE_MAX }

/// A payment's nominal slot dates (before weekend shifting or tentative sliding) in order: the
/// start date, then one `add_period` step per slot (analysis/01 §3.3). No end condition is applied
/// (`BudgetDb::scheduled_slots` applies the payment's), but the walk ends with the first date past
/// `DATE_MAX`. That date is still yielded, so a caller can tell the recurrence runs off the
/// calendar; walks that place or count dates stop before it with `take_while(in_calendar)`.
#[derive(Clone, Debug)]
pub struct Slots {
    field: Field,
    frequency: i32,
    start_date: i32,
    /// The slot yielded last (the start date before the first).
    c: Cal,
    /// The 0-based index of `c`; -1 before the first slot.
    n: i32,
}

impl Iterator for Slots {
    type Item = Cal;
    fn next(&mut self) -> Option<Cal> {
        if self.n >= 0 {
            if !in_calendar(&self.c) { return None; }
            add_period(&mut self.c, self.field, self.frequency, self.start_date, self.n + 1);
        }
        self.n += 1;
        Some(self.c)
    }
}

/// The business day on which a weekend date is paid: the following Monday, or with early pay the
/// preceding Friday (analysis/01 §3.6 step 4). `None` for a weekday.
pub fn weekend_shift(c: &Cal, early_pay: bool) -> Option<Cal> {
    let days = match (c.day_of_week(), early_pay) {
        (SATURDAY, false) => 2,
        (SUNDAY, false) => 1,
        (SATURDAY, true) => -1,
        (SUNDAY, true) => -2,
        _ => return None,
    };
    let mut d = *c;
    d.add(Field::Day, days);
    Some(d)
}

impl Payment {
    /// The nominal slot dates of this payment's recurrence (see `Slots`).
    pub fn slots(&self) -> Slots {
        Slots { field: calendar_field(self.freq_type), frequency: self.frequency, start_date: self.start_date,
            c: Cal::from_int(self.start_date), n: -1 }
    }

    /// Where the regular occurrence scheduled on `c` is shown: a business-days payment moves a
    /// weekend date to a business day (`weekend_shift`), except a tentative one, which slides
    /// within its pay period instead.
    pub fn shown_date(&self, c: &Cal) -> Cal {
        if self.is_no_weekends() && !self.is_tentative() { weekend_shift(c, self.is_early_pay()).unwrap_or(*c) } else { *c }
    }

    /// How many slots the payment has: `n_pays` (kept in step with the end mode by `update`), or
    /// no limit when indefinite.
    fn slot_limit(&self) -> usize {
        if self.end_mode() == END_MODE_INDEFINITE { usize::MAX } else { self.n_pays.max(0) as usize }
    }
}

#[derive(Clone, Debug)]
pub struct LoanSchedule {
    pub end: Cal,
    pub actual_start: Cal,
    pub actual_end: Cal,
    pub n_pays: i32,
    pub last_payment_amount: f64,
    pub abs_amount_remaining: f64,
    pub n_remaining: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaymentsRemaining {
    pub amount: f64,
    pub n: i32,
}

impl BudgetDb {
    /// Payment `i`'s scheduled slots with both stop rules of analysis/01 §3.3: its first `n_pays`,
    /// or for an indefinite payment those up to the end of the window. Like `Slots`, it may end
    /// with a date past `DATE_MAX`.
    fn scheduled_slots(&self, i: usize) -> impl Iterator<Item = Cal> {
        let p = &self.payments[i];
        let until = (p.end_mode() == END_MODE_INDEFINITE).then_some(self.end_date);
        p.slots().take(p.slot_limit()).take_while(move |c| until.is_none_or(|end| !c.gt(&end)))
    }

    /// `get_period_days(p)`: length of the first period in days.
    pub fn period_days(&self, i: usize) -> i32 {
        let p = &self.payments[i];
        let start = Cal::from_int(p.start_date);
        let mut c = start;
        add_period(&mut c, calendar_field(p.freq_type), p.frequency, p.start_date, 1);
        Cal::diff(&start, &c)
    }

    pub fn is_regular_payment_on(&self, i: usize, date: i32) -> bool {
        self.scheduled_slots(i).map(|c| c.get()).find(|&v| v >= date) == Some(date)
    }

    /// `get_valid_payment_days(p, cal)` (01 §3.12): the days on which the occurrence of `cal`'s month
    /// may be paid, `{d, …, d+grace−1}`, where `grace` is the period length for a tentative payment
    /// and `grace_period_days` otherwise. Empty when there is no grace or no matching occurrence.
    pub fn valid_payment_days(&self, i: usize, cal: i32) -> Vec<i32> {
        let p = &self.payments[i];
        let tentative = p.is_tentative();
        // Capped at a year: the set only drives hover highlighting, and an absurd value from a file
        // must not allocate millions of dates.
        let grace = if tentative { self.period_days(i) } else { p.grace_period_days }.min(366);
        if grace <= 0 { return Vec::new(); }
        let mut d = Cal::from_int(cal);
        d.set_day_one();
        let month = d.ym();
        while d.ym() == month {
            let dv = d.get();
            if self.is_regular_payment_on(i, dv) {
                let mut last = d;
                last.add(Field::Day, grace - 1);
                let inside = dv <= cal && cal <= last.get();
                if !(tentative && !inside) && !(cal > dv && !inside) {
                    let mut c = d;
                    return (0..grace).map(|_| { let v = c.get(); c.add(Field::Day, 1); v }).collect();
                }
            }
            d.add(Field::Day, 1);
        }
        Vec::new()
    }

    /// First scheduled date strictly after `date` (possibly past `DATE_MAX`, where the schedule
    /// runs off the calendar), or -1 when the `n_pays` limit is hit. An indefinite payment is not
    /// cut at the window end here.
    pub fn next_regular_payment_after_date(&self, i: usize, date: i32) -> i32 {
        let p = &self.payments[i];
        p.slots().take(p.slot_limit()).map(|c| c.get()).find(|&v| v > date).unwrap_or(-1)
    }

    pub fn number_of_planned_transactions(&self, desc: &str, from: &Cal, to: &Cal) -> i32 {
        let planned = (0..self.payments.len()).filter(|&i| eq_ignore_case(&self.payments[i].description, desc))
            .map(|i| self.scheduled_slots(i).take_while(|c| !c.gt(to) && in_calendar(c)).filter(|c| c.ge(from)).count());
        planned.sum::<usize>() as i32
    }

    /// A slot is modified only when an option sits on the date where its transaction is actually
    /// displayed (the weekend-shifted date for business-days payments). An option on an undisplayed
    /// nominal weekend day is an extra transaction (already counted in the loan `sum`), and a deletion
    /// marker there is stale and dropped by the options pass, so it must not skip the installment.
    fn is_modified_on(&self, i: usize, c: &Cal) -> bool {
        let p = &self.payments[i];
        let shown = p.shown_date(c).get();
        p.opt.iter().any(|o| o.date == shown)
    }

    /// `calcLoanSchedule(p, asOf)`; exact double arithmetic, no rounding.
    pub fn calc_loan_schedule(&self, i: usize, as_of: Option<&Cal>) -> LoanSchedule {
        let p = &self.payments[i];
        let mut min_opt = p.start_date;
        let mut max_opt = p.start_date;
        let mut sum = 0.0;
        let mut trans: Vec<(i32, f64)> = Vec::new();
        for o in &p.opt {
            if o.deleted { continue; }
            sum += o.amount;
            if min_opt > o.date { min_opt = o.date; }
            if max_opt < o.date { max_opt = o.date; }
            if as_of.is_some() { trans.push((o.date, o.amount)); }
        }
        let mut ls = LoanSchedule {
            end: Cal::from_int(p.start_date), // set after the loop
            actual_start: Cal::from_int(p.start_date.min(min_opt)),
            actual_end: Cal::from_int(max_opt),
            n_pays: 0,
            last_payment_amount: 0.0,
            abs_amount_remaining: 0.0,
            n_remaining: 0,
        };
        // A walk yields its start date and continues after every slot on the calendar.
        let mut slots = p.slots();
        let mut c = slots.next().expect("a schedule walk yields its start date");
        let is_expense = p.amount < 0.0;
        let mut remaining = p.loan_amount.abs() - if is_expense { -sum } else { sum };
        let abs_amt = p.amount.abs();
        // Half-a-cent tolerance absorbs floating-point residue when the principal is an exact multiple.
        // The schedule is bounded at the last representable date so absurd loans cannot hang the app.
        while remaining >= 0.005 && in_calendar(&c) {
            ls.n_pays += 1;
            if !self.is_modified_on(i, &c) {
                if remaining <= abs_amt + 0.005 {
                    ls.last_payment_amount = if is_expense { -remaining } else { remaining };
                    if as_of.is_some() { trans.push((c.get(), ls.last_payment_amount)); }
                    break;
                }
                remaining -= abs_amt;
                if as_of.is_some() { trans.push((c.get(), p.amount)); }
            }
            c = slots.next().expect("a schedule walk continues after a slot on the calendar");
            if abs_amt == 0.0 { break; } // zero-amount guard: the original loops forever here
        }
        // Spec 3.5: `endCal` is the last slot date (the start date when the loop never runs).
        ls.end = c;
        if ls.end.gt(&ls.actual_end) { ls.actual_end.set_int(ls.end.get()); }
        if let Some(a) = as_of {
            let v = a.get();
            for (d, amt) in &trans {
                if *d > v { ls.abs_amount_remaining += amt; ls.n_remaining += 1; }
            }
            ls.abs_amount_remaining = ls.abs_amount_remaining.abs();
        }
        ls
    }

    /// `recalculate_n_pays`: number of scheduled dates `<= endDate` (always >= 1).
    pub fn recalculate_n_pays(&mut self, i: usize) {
        let p = &self.payments[i];
        let end = Cal::from_int(p.end_date);
        // The start date always counts, then every later slot up to the end date.
        let later = p.slots().skip(1).take_while(|c| !c.gt(&end) && in_calendar(c)).count();
        self.payments[i].n_pays = 1 + later as i32;
    }

    pub fn payments_remaining(&self, i: usize, as_of: &Cal) -> PaymentsRemaining {
        let p = &self.payments[i];
        let v = as_of.get();
        let remaining = |n: usize| PaymentsRemaining { amount: n as f64 * p.amount, n: n as i32 };
        match p.end_mode() {
            END_MODE_INDEFINITE => PaymentsRemaining { amount: -1.0, n: -1 },
            END_MODE_N_PAYS => {
                remaining(p.slots().take(p.n_pays.max(1) as usize).take_while(in_calendar).filter(|c| c.get() >= v).count())
            }
            END_MODE_DATE => {
                let end = Cal::from_int(p.end_date);
                remaining(p.slots().take_while(|c| !c.gt(&end) && in_calendar(c)).filter(|c| c.get() >= v).count())
            }
            END_MODE_LOAN_PAID => {
                let ls = self.calc_loan_schedule(i, Some(as_of));
                PaymentsRemaining { amount: ls.abs_amount_remaining, n: ls.n_remaining }
            }
            _ => PaymentsRemaining { amount: 0.0, n: 0 },
        }
    }

    /// `update(Payment, boolean removeFirst)`: synchronise end mode, place regular transactions, apply options.
    pub(crate) fn update_payment_inner(&mut self, i: usize, remove_first: bool) {
        self.update_count += 1;
        let (field, frequency, start_date, mode) = {
            let p = &self.payments[i];
            (calendar_field(p.freq_type), p.frequency, p.start_date, p.end_mode())
        };
        let mut last_payment_amount = 0.0;
        match mode {
            END_MODE_N_PAYS => {
                let p = &mut self.payments[i];
                if p.n_pays < 1 { p.n_pays = 1; }
                // The date of payment number `n_pays`, without stepping past the calendar.
                let last = p.slots().take(p.n_pays as usize).take_while(in_calendar).last();
                p.end_date = last.unwrap_or_else(|| Cal::from_int(p.start_date)).get();
            }
            END_MODE_DATE => {
                let p = &mut self.payments[i];
                if p.end_date < p.start_date { p.end_date = p.start_date; }
                self.recalculate_n_pays(i);
            }
            END_MODE_LOAN_PAID => {
                let ls = self.calc_loan_schedule(i, None);
                let p = &mut self.payments[i];
                p.end_date = ls.end.get();
                p.n_pays = ls.n_pays;
                last_payment_amount = ls.last_payment_amount;
            }
            _ => {}
        }
        if remove_first { self.remove_from_budget_days(i); }
        self.in_sync = false;

        let (tentative, sticky, no_weekends, early_pay, n_pays, amount, disabled) = {
            let p = &self.payments[i];
            (p.is_tentative(), p.is_sticky(), p.is_no_weekends(), p.is_early_pay(), p.n_pays, p.amount, p.disabled)
        };
        let today = self.today;
        let mut is_last = false;
        for (n, c) in self.scheduled_slots(i).take_while(in_calendar).enumerate() {
            let (n, cv) = (n as i32, c.get());
            let mut amt = amount;
            if mode == END_MODE_LOAN_PAID && n + 1 == n_pays {
                amt = last_payment_amount;
                if !self.payments[i].has_options_after(cv) { is_last = true; }
            }
            if sticky { amt = self.payments[i].amount_before_date(cv); }
            if !is_last && mode != END_MODE_INDEFINITE && n + 1 == n_pays && !self.payments[i].has_options_after(cv) {
                is_last = true;
            }
            if tentative {
                let mut period_end = c;
                add_period(&mut period_end, field, frequency, start_date, n + 1);
                period_end.add(Field::Day, -1);
                if period_end.gt(&c) {
                    let first = cv;
                    let last = period_end.get();
                    let opts = self.payments[i].options_in(first, last);
                    let mut current_or_future = first >= today;
                    let today_in_period = first <= today && today <= last;
                    let target = if today_in_period { current_or_future = true; Cal::from_int(today) } else { c };
                    if opts.is_empty() {
                        if current_or_future {
                            if let Some(d) = self.cal_index(&target) { self.day_add_regular(d, i, amount, SUFFIX_NONE); }
                        }
                    } else if sticky && disabled {
                        let tv = target.get();
                        for o in &opts {
                            if o.deleted && o.date != first && o.date != tv { self.payments[i].clear(o.date); }
                        }
                        if !self.payments[i].is_paid(first, last) {
                            if let Some(d) = self.cal_index(&target) {
                                if today_in_period {
                                    let a = self.payments[i].amount_before_date(tv);
                                    self.day_add_regular(d, i, a, SUFFIX_NONE);
                                } else if current_or_future {
                                    self.day_add_regular(d, i, amount, SUFFIX_NONE);
                                }
                            }
                        }
                    } else {
                        let mut n_deleted = 0;
                        for o in &opts {
                            if o.deleted { n_deleted += 1; self.payments[i].clear(o.date); }
                        }
                        if opts.len() == n_deleted && current_or_future {
                            self.payments[i].set_no_transaction(target.get());
                        }
                    }
                }
            } else {
                // A weekend-shifted occurrence carries the adjusted suffix unless it is the last.
                let shifted = if no_weekends { weekend_shift(&c, early_pay) } else { None };
                let suffix = if is_last { SUFFIX_LAST } else if shifted.is_some() { SUFFIX_ADJUSTED } else { SUFFIX_NONE };
                if let Some(di) = self.cal_index(&shifted.unwrap_or(c)) {
                    self.day_place_regular(di, i, amt, suffix);
                }
            }
        }

        // options pass
        let mut k = 0;
        while k < self.payments[i].opt.len() {
            let o = self.payments[i].opt[k].clone();
            let Some(di) = self.cal_index_int(o.date) else { k += 1; continue };
            if o.deleted {
                let removed = self.day_mut(di).remove(i);
                if removed || tentative {
                    self.day_add_restorable(di, i);
                    k += 1;
                } else {
                    self.payments[i].opt.remove(k);
                }
                continue;
            }
            let mut suffix = SUFFIX_ADJUSTED;
            let end_date = self.payments[i].end_date;
            if mode != END_MODE_INDEFINITE && !self.payments[i].has_options_after(o.date) && o.date >= end_date {
                suffix = SUFFIX_LAST;
            }
            let existing = self.day_mut(di).get(i).map(|t| (t.amount, t.suffix));
            match existing {
                None => { self.day_add(di, i, o.amount, suffix); k += 1; }
                Some((amt, _)) if amt == o.amount && !tentative => { self.payments[i].opt.remove(k); }
                Some(_) => {
                    let t = self.day_mut(di).get_mut(i).unwrap();
                    t.amount = o.amount;
                    if t.suffix == SUFFIX_NONE { t.suffix = suffix; }
                    k += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod freq_tests {
    use super::*;

    #[test]
    fn add_period_zero_or_negative_frequency_advances() {
        for f in [0, -3, i32::MAX] {
            let mut c = Cal::from_int(20240131);
            add_period(&mut c, Field::Month, f, 20240131, 3);
            assert!(c.get() > 20240131);
        }
    }

    #[test]
    fn add_period_huge_frequency_with_early_start_day_does_not_overflow() {
        for field in [Field::Month, Field::Year, Field::Day, Field::Week] {
            let mut c = Cal::from_int(20240115);
            add_period(&mut c, field, i32::MAX, 20240115, 3);
            assert!(c.get() > 20240115);
        }
    }

    #[test]
    fn slots_step_from_the_anchor_and_stop_after_the_calendar() {
        let mut p = Payment::new(20240131);
        p.freq_type = 'm';
        let dates: Vec<i32> = p.slots().take(4).map(|c| c.get()).collect();
        assert_eq!(dates, [20240131, 20240229, 20240331, 20240430]);
        // The walk yields the first date past DATE_MAX and then ends.
        p.start_date = 99971231; p.freq_type = 'y';
        let dates: Vec<i32> = p.slots().map(|c| c.get()).collect();
        assert_eq!(dates, [99971231, 99981231, 99991231, 100001231]);
    }

    #[test]
    fn weekend_shift_moves_to_monday_or_back_to_friday() {
        // 2026-10-03 is a Saturday, 2026-10-04 a Sunday.
        let shift = |d: i32, early: bool| weekend_shift(&Cal::from_int(d), early).map(|c| c.get());
        assert_eq!((shift(20261003, false), shift(20261004, false)), (Some(20261005), Some(20261005)));
        assert_eq!((shift(20261003, true), shift(20261004, true)), (Some(20261002), Some(20261002)));
        assert_eq!(shift(20261005, false), None);
        let mut p = Payment::new(20261003);
        assert_eq!(p.shown_date(&Cal::from_int(20261003)).get(), 20261003);
        p.set_no_weekends(true);
        assert_eq!(p.shown_date(&Cal::from_int(20261003)).get(), 20261005);
        p.set_tentative(true);
        assert_eq!(p.shown_date(&Cal::from_int(20261003)).get(), 20261003);
    }
}
