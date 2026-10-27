//! Port of `Payment`, `PaymentOption`, `PaymentFlag`, `Note`, `Payment$Hilight`.

pub const NO_WEEKENDS: i32 = 1;
pub const TENTATIVE: i32 = 2;
pub const EARLY_PAY: i32 = 4;
pub const REMINDER: i32 = 8;
pub const END_MODE_MASK: i32 = 240;
pub const END_MODE_UNDEFINED: i32 = 0;
pub const END_MODE_INDEFINITE: i32 = 16;
pub const END_MODE_DATE: i32 = 32;
pub const END_MODE_N_PAYS: i32 = 48;
pub const END_MODE_LOAN_PAID: i32 = 64;
pub const IS_STICKY: i32 = 256;

/// Sentinel "dates" stored inside the per-date lists.
pub const DEFAULT_COLOR_DATE: i32 = 99990101;
pub const URL_NOTE_DATE: i32 = 99990102;

pub const FLAG_NONE: char = '^';
pub const FLAG_PAID: char = ';';
pub const FLAG_IMPORTANT: char = '#';

/// Transaction suffix characters produced by the engine.
pub const SUFFIX_NONE: char = '\0';
pub const SUFFIX_ADJUSTED: char = '\'';
pub const SUFFIX_LAST: char = '*';

#[derive(Clone, Debug, PartialEq)]
pub struct PaymentOption {
    pub date: i32,
    pub amount: f64,
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentFlag {
    pub date: i32,
    pub value: char,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub date: i32,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hilight {
    pub date: i32,
    pub bg_color: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Payment {
    pub id: i32,
    pub amount: f64,
    pub description: String,
    pub start_date: i32,
    pub frequency: i32,
    pub freq_type: char,
    pub n_pays: i32,
    pub end_date: i32,
    pub image_path: Option<String>,
    pub flags: i32,
    pub opt: Vec<PaymentOption>,
    pub flag: Vec<PaymentFlag>,
    pub note: Vec<Note>,
    pub hilighting: Vec<Hilight>,
    pub disabled: bool,
    pub category: Option<String>,
    pub loan_amount: f64,
    pub flag_value: char,
    pub grace_period_days: i32,
    /// Transient: indices of every `BudgetDay` that references this payment.
    pub(crate) calendar_days: Vec<usize>,
}

impl Default for Payment {
    fn default() -> Payment { Payment::new(0) }
}

impl Payment {
    /// `new Payment()`; `today` is the yyyymmdd used for start/end date.
    pub fn new(today: i32) -> Payment {
        Payment {
            id: 0,
            amount: 0.0,
            description: String::new(),
            start_date: today,
            frequency: 1,
            freq_type: 'm',
            n_pays: 1,
            end_date: today,
            image_path: None,
            flags: END_MODE_N_PAYS,
            opt: Vec::new(),
            flag: Vec::new(),
            note: Vec::new(),
            hilighting: Vec::new(),
            disabled: false,
            category: None,
            loan_amount: 0.0,
            flag_value: FLAG_NONE,
            grace_period_days: 0,
            calendar_days: Vec::new(),
        }
    }

    pub fn end_mode(&self) -> i32 { self.flags & END_MODE_MASK }
    pub fn set_end_mode(&mut self, m: i32) {
        if matches!(m, END_MODE_INDEFINITE | END_MODE_DATE | END_MODE_N_PAYS | END_MODE_LOAN_PAID) {
            self.flags = (self.flags & !END_MODE_MASK) | m;
        }
    }
    fn bit(&self, b: i32) -> bool { self.flags & b != 0 }
    fn set_bit(&mut self, b: i32, on: bool) { if on { self.flags |= b } else { self.flags &= !b } }
    pub fn is_no_weekends(&self) -> bool { self.bit(NO_WEEKENDS) }
    pub fn set_no_weekends(&mut self, v: bool) { self.set_bit(NO_WEEKENDS, v) }
    pub fn is_tentative(&self) -> bool { self.bit(TENTATIVE) }
    pub fn set_tentative(&mut self, v: bool) { self.set_bit(TENTATIVE, v) }
    pub fn is_early_pay(&self) -> bool { self.bit(EARLY_PAY) }
    pub fn set_early_pay(&mut self, v: bool) { self.set_bit(EARLY_PAY, v) }
    pub fn is_reminder(&self) -> bool { self.bit(REMINDER) }
    pub fn set_reminder(&mut self, v: bool) { self.set_bit(REMINDER, v) }
    pub fn is_sticky(&self) -> bool { self.bit(IS_STICKY) }
    pub fn set_sticky(&mut self, v: bool) { self.set_bit(IS_STICKY, v) }

    /// Text before the first `;`.
    pub fn short_description(&self) -> &str {
        self.description.split(';').next().unwrap_or("")
    }

    // ----- options -----
    pub fn option_index(&self, date: i32) -> Option<usize> {
        self.opt.iter().position(|o| o.date == date)
    }
    pub fn option_by_date(&self, date: i32) -> Option<&PaymentOption> {
        self.opt.iter().find(|o| o.date == date)
    }
    /// Amount of the non-deleted option with the greatest date strictly before `date`, else the regular amount.
    pub fn amount_before_date(&self, date: i32) -> f64 {
        let mut best: Option<&PaymentOption> = None;
        for o in &self.opt {
            if o.deleted || o.date >= date { continue; }
            if best.map_or(true, |b| b.date < o.date) { best = Some(o); }
        }
        best.map_or(self.amount, |o| o.amount)
    }
    fn update(&mut self, date: i32, amount: f64, deleted: bool) {
        match self.option_index(date) {
            Some(i) => { self.opt[i].amount = amount; self.opt[i].deleted = deleted; }
            None => self.opt.push(PaymentOption { date, amount, deleted }),
        }
    }
    pub fn set_custom_amount(&mut self, date: i32, amount: f64) { self.update(date, amount, false) }
    pub fn set_no_transaction(&mut self, date: i32) { let a = self.amount; self.update(date, a, true) }
    /// Loader only: appends without de-duplication.
    pub fn add_no_transaction(&mut self, date: i32) {
        self.opt.push(PaymentOption { date, amount: self.amount, deleted: true })
    }
    pub fn add_custom_amount(&mut self, date: i32, amount: f64) {
        self.opt.push(PaymentOption { date, amount, deleted: false })
    }
    /// Remove the option at `date` and the first flag at `date`.
    pub fn clear(&mut self, date: i32) {
        if let Some(i) = self.option_index(date) { self.opt.remove(i); }
        if let Some(i) = self.flag.iter().position(|f| f.date == date) { self.flag.remove(i); }
    }
    pub fn clear_before(&mut self, date: i32) {
        self.opt.retain(|o| o.date >= date);
        self.flag.retain(|f| f.date >= date);
    }
    pub fn clear_after(&mut self, date: i32) {
        self.opt.retain(|o| o.date <= date);
        self.flag.retain(|f| f.date <= date);
    }
    pub fn clear_all(&mut self) { self.opt.clear() }
    pub fn has_options_after(&self, date: i32) -> bool { self.opt.iter().any(|o| o.date > date) }
    pub fn first_option_after(&self, date: i32) -> Option<&PaymentOption> {
        self.opt.iter().filter(|o| o.date > date).min_by_key(|o| o.date)
    }
    /// All options (deleted or not) with `from <= date <= to`, in list order.
    pub fn options_in(&self, from: i32, to: i32) -> Vec<PaymentOption> {
        self.opt.iter().filter(|o| from <= o.date && o.date <= to).cloned().collect()
    }
    pub fn deleted_in(&self, from: i32, to: i32) -> Vec<PaymentOption> {
        self.opt.iter().filter(|o| o.deleted && from <= o.date && o.date <= to).cloned().collect()
    }
    /// `moveTransaction(dst, src, amount)` — destination first, like the original.
    pub fn move_transaction(&mut self, dst: i32, src: i32, amount: f64) {
        let mut flag = FLAG_NONE;
        if let Some(i) = self.flag.iter().position(|f| f.date == src) {
            flag = self.flag[i].value;
            self.flag.remove(i);
        }
        let note = self.get_note(src).map(|s| s.to_string());
        let hi = self.get_hilighting(src);
        self.update(dst, amount, false);
        self.update(src, amount, true);
        if flag != FLAG_NONE { self.set_flag(dst, flag); }
        if let Some(n) = note { self.set_note(dst, Some(&n)); self.set_note(src, None); }
        if hi != -1 { self.set_hilighting(dst, hi); self.set_hilighting(src, -1); }
    }

    // ----- flags -----
    pub fn set_flag(&mut self, date: i32, value: char) {
        match self.flag.iter_mut().find(|f| f.date == date) {
            Some(f) => f.value = value,
            None => self.flag.push(PaymentFlag { date, value }),
        }
    }
    pub fn get_flag(&self, date: i32) -> char {
        self.flag.iter().find(|f| f.date == date).map_or(self.flag_value, |f| f.value)
    }
    pub fn is_paid(&self, from: i32, to: i32) -> bool {
        self.flag.iter().any(|f| from <= f.date && f.date <= to && f.value == FLAG_PAID)
    }

    // ----- notes -----
    pub fn set_note(&mut self, date: i32, text: Option<&str>) {
        if let Some(i) = self.note.iter().position(|n| n.date == date) {
            match text {
                Some(t) => self.note[i].note = t.to_string(),
                None => { self.note.remove(i); }
            }
        } else if let Some(t) = text {
            self.note.push(Note { date, note: t.to_string() });
        }
    }
    /// `getNote(0)` returns every note followed by a space.
    pub fn get_note(&self, date: i32) -> Option<String> {
        if date == 0 {
            let mut s = String::new();
            for n in &self.note { s.push_str(&n.note); s.push(' '); }
            return Some(s);
        }
        self.note.iter().find(|n| n.date == date).map(|n| n.note.clone())
    }
    pub fn note_count(&self) -> usize { self.note.len() }
    pub fn clear_notes_before(&mut self, date: i32) { self.note.retain(|n| n.date >= date) }
    pub fn set_url(&mut self, url: Option<&str>) { self.set_note(URL_NOTE_DATE, url) }
    pub fn get_url(&self) -> Option<String> { self.get_note(URL_NOTE_DATE) }

    // ----- highlighting -----
    pub fn set_hilighting(&mut self, date: i32, color: i32) {
        if let Some(i) = self.hilighting.iter().position(|h| h.date == date) {
            if color == -1 { self.hilighting.remove(i); } else { self.hilighting[i].bg_color = color; }
        } else if color != -1 {
            self.hilighting.push(Hilight { date, bg_color: color });
        }
    }
    pub fn get_hilighting(&self, date: i32) -> i32 {
        self.hilighting.iter().find(|h| h.date == date).map_or(-1, |h| h.bg_color)
    }
    pub fn set_default_highlighting(&mut self, color: i32) { self.set_hilighting(DEFAULT_COLOR_DATE, color) }
    pub fn default_hilighting(&self) -> i32 { self.get_hilighting(DEFAULT_COLOR_DATE) }

    /// `clonePayment()`: schedule, text and default colour only; no id, options, flags, notes, loan.
    pub fn clone_payment(&self) -> Payment {
        let mut p = Payment::new(self.start_date);
        p.amount = self.amount;
        p.description = self.description.clone();
        p.end_date = self.end_date;
        p.freq_type = self.freq_type;
        p.frequency = self.frequency;
        p.image_path = self.image_path.clone();
        p.n_pays = self.n_pays;
        p.start_date = self.start_date;
        p.flags = self.flags;
        p.disabled = self.disabled;
        p.category = self.category.clone();
        p.grace_period_days = self.grace_period_days;
        p.set_default_highlighting(self.default_hilighting());
        p
    }

    pub fn is_same_schedule(&self, o: &Payment) -> bool {
        self.start_date == o.start_date && self.end_date == o.end_date && self.freq_type == o.freq_type
            && self.frequency == o.frequency && self.disabled == o.disabled && self.end_mode() == o.end_mode()
            && self.n_pays == o.n_pays
    }
}
