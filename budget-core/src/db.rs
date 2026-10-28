//! Port of `BudgetDB` (window, day array, balances, adjustments, collection API) and `BudgetDay`.
//! Scheduling (`update`, `addPeriod`, loan schedule) lives in `schedule.rs`.

use crate::bgt;
use crate::date::{Cal, Field};
use crate::model::*;
use std::cmp::Ordering;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    /// Index into `BudgetDb::payments`.
    pub pmt: usize,
    pub amount: f64,
    /// Running balance after this transaction.
    pub balance: f64,
    /// `SUFFIX_NONE`, `SUFFIX_ADJUSTED` (`'`) or `SUFFIX_LAST` (`*`).
    pub suffix: char,
}

#[derive(Clone, Debug, Default)]
pub struct BudgetDay {
    pub opening_balance: f64,
    pub closing_balance: f64,
    pub adjustment_diff: f64,
    pub closing_balance_adjusted: bool,
    pub transactions: Vec<Transaction>,
    /// Payments whose transaction on this day was deleted or moved away.
    pub restorable: Vec<usize>,
    /// Payments that had a scheduled transaction placed on this day (duplicates possible).
    pub regulars: Vec<usize>,
    /// `transactions` is in display order (see `sort_transactions`).
    sorted: bool,
}

impl BudgetDay {
    pub fn get(&self, pmt: usize) -> Option<&Transaction> { self.transactions.iter().find(|t| t.pmt == pmt) }
    pub fn get_mut(&mut self, pmt: usize) -> Option<&mut Transaction> { self.transactions.iter_mut().find(|t| t.pmt == pmt) }
    pub fn is_restorable(&self, pmt: usize) -> bool { self.restorable.contains(&pmt) }
    pub fn remove(&mut self, pmt: usize) -> bool {
        match self.transactions.iter().position(|t| t.pmt == pmt) {
            Some(i) => { self.transactions.remove(i); true }
            None => false,
        }
    }
    pub fn remove_restorable(&mut self, pmt: usize) -> bool {
        match self.restorable.iter().position(|&p| p == pmt) {
            Some(i) => { self.restorable.remove(i); true }
            None => false,
        }
    }
    pub fn remove_regular(&mut self, pmt: usize) -> bool {
        match self.regulars.iter().position(|&p| p == pmt) {
            Some(i) => { self.regulars.remove(i); true }
            None => false,
        }
    }
    pub fn unadjusted_balance(&self) -> f64 {
        if self.closing_balance_adjusted { self.closing_balance - self.adjustment_diff } else { self.closing_balance }
    }
    /// Display rounding of the original: truncate toward zero after nudging by `0.05f`.
    pub fn closing_balance_display(&self, exact: bool) -> f64 {
        crate::numfmt::balance_display(self.closing_balance, exact)
    }
}

/// Put a day's transactions in display order once after they change (`disabled` tells whether a
/// payment is disabled): enabled payments first, then by amount.
fn sort_transactions(day: &mut BudgetDay, disabled: impl Fn(usize) -> bool) {
    if day.sorted { return; }
    if day.transactions.len() > 1 {
        day.transactions.sort_by(|a, b| {
            let (da, db) = (disabled(a.pmt), disabled(b.pmt));
            if da == db {
                if a.amount < 0.0 && b.amount < 0.0 { b.amount.partial_cmp(&a.amount).unwrap_or(Ordering::Equal).reverse() }
                else { a.amount.partial_cmp(&b.amount).unwrap_or(Ordering::Equal).reverse() }
            } else if da && !db { Ordering::Greater } else { Ordering::Less }
        });
    }
    day.sorted = true;
}

#[derive(Debug, Default)]
pub struct BudgetDb {
    pub payments: Vec<Payment>,
    /// Container payment: `opt` = balance adjustments, `note` = day notes.
    pub adjustments: Payment,
    days: Vec<BudgetDay>,
    pub start_date: Cal,
    pub end_date: Cal,
    start_date_int: i32,
    end_date_int: i32,
    start_year: i32,
    y_offset: Vec<i32>,
    pub(crate) in_sync: bool,
    pub notes: String,
    pub version: String,
    pub update_count: i32,
    pub file_name: Option<String>,
    /// Process-start "today" (yyyymmdd) used by the tentative logic.
    pub today: i32,
}

impl BudgetDb {
    /// Empty database, rebuilt around `cal`.
    pub fn new(cal: &Cal, future_months: i32, unlimited_history: bool) -> BudgetDb {
        let today = Cal::today().get();
        let mut db = BudgetDb {
            adjustments: Payment::new(today),
            version: "1".into(),
            today,
            ..Default::default()
        };
        db.rebuild_db(cal, future_months, unlimited_history);
        db
    }

    /// `BudgetDB.load`: parse, build, then clean up name collisions.
    pub fn from_loaded(l: bgt::Loaded, cal: &Cal, future_months: i32, unlimited_history: bool) -> BudgetDb {
        let today = Cal::today().get();
        let mut db = BudgetDb {
            payments: l.payments,
            adjustments: l.adjustments.unwrap_or_else(|| Payment::new(today)),
            notes: l.notes,
            version: l.version,
            today,
            ..Default::default()
        };
        db.rebuild_db(cal, future_months, unlimited_history);
        for i in 0..db.payments.len() {
            while !db.is_valid_category(db.payments[i].category.as_deref(), None) {
                let c = db.payments[i].category.get_or_insert_with(String::new);
                c.push('-');
            }
        }
        for i in 0..db.payments.len() {
            while !db.is_valid_description(&db.payments[i].description, None) {
                db.payments[i].description.push('-');
            }
        }
        db
    }

    pub fn load(path: &Path, password: Option<&str>, cal: &Cal, future_months: i32, unlimited_history: bool) -> Result<BudgetDb, String> {
        let today = Cal::today().get();
        let l = bgt::load_file(path, today, password).map_err(|l| l.log)?;
        let mut db = BudgetDb::from_loaded(l, cal, future_months, unlimited_history);
        db.file_name = Some(path.display().to_string());
        Ok(db)
    }

    pub fn to_text(&self, password: Option<&str>) -> Result<String, String> {
        bgt::save_str(&self.payments, &self.adjustments, &self.notes, password)
    }

    pub fn save(&self, path: &Path, password: Option<&str>) -> Result<(), String> {
        bgt::save_file(path, &self.to_text(password)?)
    }

    // ---------- window ----------

    pub(crate) fn cal_index(&self, c: &Cal) -> Option<usize> {
        let v = c.get();
        if v < self.start_date_int || v > self.end_date_int { return None; }
        Some((self.y_offset[(c.year() - self.start_year) as usize] + c.day_of_year() - 1) as usize)
    }
    pub(crate) fn cal_index_int(&self, v: i32) -> Option<usize> { self.cal_index(&Cal::from_int(v)) }

    pub fn is_month_available(&self, c: &Cal) -> bool {
        self.start_date.ym() <= c.ym() && c.ym() <= self.end_date.ym()
    }

    /// `rebuildDB(cal, futureMonths, unlimitedHistory)`.
    pub fn rebuild_db(&mut self, cal: &Cal, future_months: i32, unlimited_history: bool) {
        // Where the first occurrence is actually placed: EARLY_PAY moves a weekend start to the prior Friday.
        let placed_start = |p: &Payment| -> i32 {
            let start = Cal::from_int(p.start_date);
            let shown = p.shown_date(&start);
            if p.is_early_pay() && shown != start { shown.get() } else { p.start_date }
        };
        let mut base = *cal;
        base.add(Field::Month, -2);
        let b = base.get();
        let mut latest_before = 0;
        let mut earliest = b;
        for a in &self.adjustments.opt {
            if a.date < b && latest_before < a.date { latest_before = a.date; }
            if earliest > a.date { earliest = a.date; }
        }
        if latest_before == 0 {
            latest_before = b;
            for p in &self.payments {
                latest_before = latest_before.min(placed_start(p));
                for o in &p.opt { if !o.deleted { latest_before = latest_before.min(o.date); } }
            }
        }
        if unlimited_history {
            for p in &self.payments {
                earliest = earliest.min(placed_start(p));
                for o in &p.opt { if !o.deleted { earliest = earliest.min(o.date); } }
            }
        }
        let mut start = Cal::from_int(if unlimited_history { earliest } else { latest_before });
        if unlimited_history { start.set(Field::Month, 0); start.set(Field::Day, 1); }
        for i in 0..self.payments.len() {
            if self.payments[i].end_mode() != END_MODE_LOAN_PAID { continue; }
            let ls = self.calc_loan_schedule(i, None);
            if start.gt(&ls.actual_start) && start.lt(&ls.actual_end) { start.set_cal(&ls.actual_start); }
        }
        start.set(Field::Day, 1);
        self.start_date = start;
        let mut end = *cal;
        end.add(Field::Month, future_months);
        end.set(Field::Month, 11);
        let last = end.max_day();
        end.set(Field::Day, last);
        self.end_date = end;
        self.start_year = start.year();
        let n_years = (end.year() - self.start_year + 1) as usize;
        self.y_offset = Vec::with_capacity(n_years);
        let mut y = start;
        let mut total = 0;
        for _ in 0..n_years {
            self.y_offset.push(total);
            total += y.max_day_of_year();
            y.add(Field::Year, 1);
        }
        self.days = (0..total).map(|_| BudgetDay::default()).collect();
        self.start_date_int = start.get();
        self.end_date_int = end.get();
        for i in 0..self.payments.len() {
            self.payments[i].calendar_days.clear();
            self.update_payment_inner(i, false);
        }
        self.update_adjustments(None);
    }

    /// `setLeftLimit`: start of the window becomes 1 January of its year (no rebuild).
    pub fn set_left_limit(&mut self) {
        self.start_date.set(Field::Month, 0);
        self.start_date.set(Field::Day, 1);
        self.start_date_int = self.start_date.get();
    }

    // ---------- day access ----------

    pub(crate) fn day_add(&mut self, day: usize, pmt: usize, amount: f64, suffix: char) {
        let d = &mut self.days[day];
        d.remove(pmt);
        d.transactions.push(Transaction { pmt, amount, balance: 0.0, suffix });
        d.sorted = false;
        self.payments[pmt].calendar_days.push(day);
    }
    pub(crate) fn day_add_regular(&mut self, day: usize, pmt: usize, amount: f64, suffix: char) {
        self.day_add(day, pmt, amount, suffix);
        self.days[day].regulars.push(pmt);
    }
    /// Place one scheduled slot. When this payment's regular transaction from the same scheduling
    /// run already sits on the day (daily business-days payments move Saturday and Sunday onto the
    /// Monday, or Friday with early pay), the amounts are added instead of the later slot replacing
    /// the earlier one, so placement agrees with the loan schedule and the planned statistics.
    pub(crate) fn day_place_regular(&mut self, day: usize, pmt: usize, amount: f64, suffix: char) {
        if self.days[day].regulars.contains(&pmt) {
            if let Some(t) = self.days[day].get_mut(pmt) {
                t.amount += amount;
                if suffix == SUFFIX_LAST || t.suffix == SUFFIX_LAST { t.suffix = SUFFIX_LAST; } else { t.suffix = SUFFIX_ADJUSTED; }
                self.days[day].regulars.push(pmt);
                self.days[day].sorted = false;
                return;
            }
        }
        self.day_add_regular(day, pmt, amount, suffix);
    }
    pub(crate) fn day_add_restorable(&mut self, day: usize, pmt: usize) {
        let d = &mut self.days[day];
        if !d.restorable.contains(&pmt) {
            d.restorable.push(pmt);
            self.payments[pmt].calendar_days.push(day);
        }
    }
    pub(crate) fn day_mut(&mut self, day: usize) -> &mut BudgetDay { &mut self.days[day] }

    /// `getDay(cal)`: `None` outside the window; balances are brought up to date first.
    pub fn get_day(&mut self, c: &Cal) -> Option<&BudgetDay> {
        let i = self.cal_index(c)?;
        if !self.in_sync { self.calc_balances(None); }
        let payments = &self.payments;
        sort_transactions(&mut self.days[i], |p| payments[p].disabled);
        Some(&self.days[i])
    }

    pub fn n_days(&self) -> usize { self.days.len() }

    /// `calcBalances(from)`.
    pub fn calc_balances(&mut self, from: Option<&Cal>) {
        if self.in_sync { return; }
        self.in_sync = true;
        let mut start = 0;
        let mut running = 0.0;
        if let Some(f) = from {
            if f.in_range(&self.start_date, &self.end_date) {
                if let Some(i) = self.cal_index(f) {
                    if i > 0 { start = i; running = self.days[i - 1].closing_balance; }
                }
            }
        }
        let disabled: Vec<bool> = self.payments.iter().map(|p| p.disabled).collect();
        for day in &mut self.days[start..] {
            sort_transactions(day, |p| disabled[p]);
            day.opening_balance = running;
            for t in day.transactions.iter_mut() {
                if !disabled[t.pmt] { running += t.amount; }
                t.balance = running;
            }
            if day.closing_balance_adjusted {
                day.adjustment_diff = day.closing_balance - running;
                running = day.closing_balance;
            } else {
                day.closing_balance = running;
                day.adjustment_diff = 0.0;
            }
        }
    }

    // ---------- adjustments ----------

    fn update_adjustments(&mut self, cal: Option<&Cal>) {
        self.update_count += 1;
        for d in &mut self.days { d.closing_balance_adjusted = false; }
        for k in 0..self.adjustments.opt.len() {
            let o = self.adjustments.opt[k].clone();
            if let Some(i) = self.cal_index_int(o.date) {
                self.days[i].closing_balance_adjusted = true;
                self.days[i].closing_balance = o.amount;
            }
        }
        if self.in_sync {
            self.in_sync = false;
            if cal.is_some() { self.calc_balances(cal); }
        }
    }
    pub fn update_adjustment(&mut self, cal: &Cal, amount: f64) {
        self.adjustments.set_custom_amount(cal.get(), amount);
        self.update_adjustments(Some(cal));
    }
    pub fn remove_adjustment(&mut self, cal: &Cal) {
        self.adjustments.clear(cal.get());
        self.update_adjustments(Some(cal));
    }
    pub fn adjustments_size(&self) -> usize { self.adjustments.opt.len() }
    pub fn get_adjustment(&self, i: usize) -> &PaymentOption { &self.adjustments.opt[i] }
    pub fn latest_adjustment_date(&self) -> i32 {
        self.adjustments.opt.iter().filter(|o| o.date / 10000 != 9999).map(|o| o.date).max().unwrap_or(-1)
    }
    pub fn oldest_adjustment_date(&self) -> i32 {
        let mut m = self.today;
        for o in &self.adjustments.opt { m = m.min(o.date); }
        for p in &self.payments { for o in &p.opt { m = m.min(o.date); } }
        m
    }
    pub fn day_note(&self, cal: &Cal) -> Option<String> { self.adjustments.get_note(cal.get()) }
    pub fn set_day_note(&mut self, cal: &Cal, text: Option<&str>) { self.adjustments.set_note(cal.get(), text) }

    // ---------- collection ----------

    pub fn size(&self) -> usize { self.payments.len() }
    pub fn get(&self, i: usize) -> &Payment { &self.payments[i] }
    pub fn get_by_id(&self, id: i32) -> Option<usize> { self.payments.iter().position(|p| p.id == id) }

    /// `add(p)`: assigns `max id + 1`, appends, schedules; returns the index.
    pub fn add(&mut self, mut p: Payment) -> usize {
        p.id = self.payments.iter().map(|q| q.id).max().unwrap_or(0) + 1;
        p.calendar_days.clear();
        self.payments.push(p);
        let i = self.payments.len() - 1;
        self.update_payment(i);
        i
    }

    /// `update(p)`: re-schedule one payment after it changed.
    pub fn update_payment(&mut self, i: usize) { self.update_payment_inner(i, true); }

    pub(crate) fn remove_from_budget_days(&mut self, i: usize) {
        let days = std::mem::take(&mut self.payments[i].calendar_days);
        for d in days {
            let day = &mut self.days[d];
            day.remove(i);
            day.regulars.retain(|&p| p != i);
            day.restorable.retain(|&p| p != i);
        }
        self.update_count += 1;
        self.in_sync = false;
    }

    pub fn delete_payment(&mut self, i: usize) {
        self.payments.remove(i);
        self.relayout();
    }

    /// `sort()`: description (case-insensitive), then start date, then end date.
    pub fn sort(&mut self) {
        self.payments.sort_by(|a, b| {
            fold_key(&a.description).cmp(&fold_key(&b.description))
                .then(a.start_date.cmp(&b.start_date))
                .then(a.end_date.cmp(&b.end_date))
        });
        self.relayout();
    }

    /// Re-place every payment from scratch (after reordering or deleting payments).
    fn relayout(&mut self) {
        for d in &mut self.days { *d = BudgetDay::default(); }
        for i in 0..self.payments.len() {
            self.payments[i].calendar_days.clear();
            self.update_payment_inner(i, false);
        }
        self.update_adjustments(None);
        self.in_sync = false;
    }

    pub fn is_valid_category(&self, cat: Option<&str>, except: Option<usize>) -> bool {
        let Some(cat) = cat else { return true };
        if cat == crate::stats::UNACCOUNTED { return false; }
        !self.payments.iter().enumerate().any(|(i, p)| Some(i) != except && p.description == cat)
    }
    pub fn is_valid_description(&self, desc: &str, except: Option<usize>) -> bool {
        !self.payments.iter().enumerate().any(|(i, p)| Some(i) != except && p.category.as_deref() == Some(desc))
    }
    pub fn categories(&self) -> Vec<String> {
        let mut v: Vec<String> = self.payments.iter().filter_map(|p| p.category.clone()).collect();
        v.sort_by_key(|s| fold_key(s));
        v.dedup();
        v
    }
    pub fn most_changed_payment(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (i, p) in self.payments.iter().enumerate() {
            if best.is_none_or(|(_, n)| p.opt.len() > n) { best = Some((i, p.opt.len())); }
        }
        best.map(|(i, _)| i)
    }
    /// Distinct deleted-option dates (one per date) of every payment with exactly this description.
    pub fn all_deleted_options(&self, desc: &str, from: i32, to: i32) -> Vec<PaymentOption> {
        let mut v: Vec<PaymentOption> = Vec::new();
        for p in self.payments.iter().filter(|p| p.description == desc) {
            for o in p.deleted_in(from, to) {
                if !v.iter().any(|x| x.date == o.date) { v.push(o); }
            }
        }
        v.sort_by_key(|o| o.date);
        v
    }
}
