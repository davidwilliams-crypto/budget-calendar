//! Schedule generation: `calendarField`, `addPeriod`, `calcLoanSchedule`, `update(Payment, boolean)`,
//! and the regular-date enumerations.

use crate::date::{Cal, Field, SATURDAY, SUNDAY};
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
    if field == Field::NthWeekday {
        cal.set_int(start_date);
        cal.add(field, n);
    } else if (field == Field::Month || field == Field::Year) && cal.day() > 27 {
        cal.set_int(start_date);
        cal.add(field, frequency * n);
    } else {
        cal.add(field, frequency);
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
    /// `get_period_days(p)`: length of the first period in days.
    pub fn period_days(&self, i: usize) -> i32 {
        let p = &self.payments[i];
        let start = Cal::from_int(p.start_date);
        let mut c = start;
        add_period(&mut c, calendar_field(p.freq_type), p.frequency, p.start_date, 1);
        Cal::diff(&start, &c)
    }

    pub fn is_regular_payment_on(&self, i: usize, date: i32) -> bool {
        let p = &self.payments[i];
        let field = calendar_field(p.freq_type);
        let mode = p.end_mode();
        let mut c = Cal::from_int(p.start_date);
        let mut n = 0;
        loop {
            if mode != END_MODE_INDEFINITE && n >= p.n_pays { return false; }
            if mode == END_MODE_INDEFINITE && c.gt(&self.end_date) { return false; }
            let v = c.get();
            if v == date { return true; }
            if v > date { return false; }
            n += 1;
            add_period(&mut c, field, p.frequency, p.start_date, n);
        }
    }

    /// First scheduled date strictly after `date`, or -1 when the `n_pays` limit is hit.
    pub fn next_regular_payment_after_date(&self, i: usize, date: i32) -> i32 {
        let p = &self.payments[i];
        let field = calendar_field(p.freq_type);
        let mode = p.end_mode();
        let mut c = Cal::from_int(p.start_date);
        let mut n = 0;
        loop {
            if mode != END_MODE_INDEFINITE && n >= p.n_pays { return -1; }
            if c.get() > date { return c.get(); }
            n += 1;
            add_period(&mut c, field, p.frequency, p.start_date, n);
        }
    }

    pub fn number_of_planned_transactions(&self, desc: &str, from: &Cal, to: &Cal) -> i32 {
        let mut count = 0;
        for p in &self.payments {
            if !p.description.eq_ignore_ascii_case(desc) { continue; }
            let field = calendar_field(p.freq_type);
            let mode = p.end_mode();
            let mut c = Cal::from_int(p.start_date);
            let mut n = 0;
            loop {
                if mode != END_MODE_INDEFINITE && n >= p.n_pays { break; }
                if mode == END_MODE_INDEFINITE && c.gt(&self.end_date) { break; }
                if c.gt(to) { break; }
                if c.ge(from) { count += 1; }
                n += 1;
                add_period(&mut c, field, p.frequency, p.start_date, n);
            }
        }
        count
    }

    fn is_modified_on(&self, i: usize, c: &Cal) -> bool {
        let v = c.get();
        self.payments[i].opt.iter().any(|o| o.date == v)
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
            end: Cal::from_int(max_opt),
            actual_start: Cal::from_int(p.start_date.min(min_opt)),
            actual_end: Cal::from_int(max_opt),
            n_pays: 0,
            last_payment_amount: 0.0,
            abs_amount_remaining: 0.0,
            n_remaining: 0,
        };
        let field = calendar_field(p.freq_type);
        let mut c = Cal::from_int(p.start_date);
        let is_expense = p.amount < 0.0;
        let mut remaining = p.loan_amount.abs() - if is_expense { -sum } else { sum };
        let abs_amt = p.amount.abs();
        while remaining > 0.0 {
            ls.n_pays += 1;
            if !self.is_modified_on(i, &c) {
                if c.gt(&ls.end) { ls.end.set_int(c.get()); }
                if remaining <= abs_amt {
                    ls.last_payment_amount = if is_expense { -remaining } else { remaining };
                    if as_of.is_some() { trans.push((c.get(), ls.last_payment_amount)); }
                    break;
                }
                remaining -= abs_amt;
                if as_of.is_some() { trans.push((c.get(), p.amount)); }
            }
            add_period(&mut c, field, p.frequency, p.start_date, ls.n_pays);
            if abs_amt == 0.0 && remaining > 0.0 { break; } // ponytail: guard against the original's infinite loop
        }
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
        let field = calendar_field(p.freq_type);
        let end = Cal::from_int(p.end_date);
        let mut c = Cal::from_int(p.start_date);
        let mut n = 1;
        loop {
            add_period(&mut c, field, p.frequency, p.start_date, n);
            if c.gt(&end) { break; }
            n += 1;
        }
        self.payments[i].n_pays = n;
    }

    pub fn payments_remaining(&self, i: usize, as_of: &Cal) -> PaymentsRemaining {
        let p = &self.payments[i];
        let v = as_of.get();
        let field = calendar_field(p.freq_type);
        match p.end_mode() {
            END_MODE_INDEFINITE => PaymentsRemaining { amount: -1.0, n: -1 },
            END_MODE_N_PAYS => {
                let mut c = Cal::from_int(p.start_date);
                let mut n = 0;
                for k in 0..p.n_pays.max(1) {
                    if c.get() >= v { n += 1; }
                    add_period(&mut c, field, p.frequency, p.start_date, k + 1);
                }
                PaymentsRemaining { amount: n as f64 * p.amount, n }
            }
            END_MODE_DATE => {
                let end = Cal::from_int(p.end_date);
                let mut c = Cal::from_int(p.start_date);
                let mut n = 0;
                let mut k = 0;
                while !c.gt(&end) {
                    if c.get() >= v { n += 1; }
                    k += 1;
                    add_period(&mut c, field, p.frequency, p.start_date, k);
                }
                PaymentsRemaining { amount: n as f64 * p.amount, n }
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
                let mut c = Cal::from_int(p.start_date);
                let mut n = 0;
                while n + 1 < p.n_pays {
                    add_period(&mut c, field, frequency, start_date, n + 1);
                    n += 1;
                }
                p.end_date = c.get();
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
        let mut c = Cal::from_int(start_date);
        let mut n = 0;
        loop {
            if mode != END_MODE_INDEFINITE && n >= n_pays { break; }
            if mode == END_MODE_INDEFINITE && c.gt(&self.end_date) { break; }
            let cv = c.get();
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
            } else if no_weekends && matches!(c.day_of_week(), SATURDAY | SUNDAY) {
                let w = c.day_of_week();
                let mut d = c;
                if early_pay { d.add(Field::Day, if w == SUNDAY { -2 } else { -1 }); }
                else { d.add(Field::Day, if w == SUNDAY { 1 } else { 2 }); }
                if let Some(di) = self.cal_index(&d) {
                    self.day_add_regular(di, i, amt, if is_last { SUFFIX_LAST } else { SUFFIX_ADJUSTED });
                }
            } else if let Some(di) = self.cal_index(&c) {
                self.day_add_regular(di, i, amt, if is_last { SUFFIX_LAST } else { SUFFIX_NONE });
            }
            n += 1;
            add_period(&mut c, field, frequency, start_date, n);
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
