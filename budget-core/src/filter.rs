//! Port of `ca.mishell.app.budget.Filter` — the transaction filter language.
//! Grammar: `[-][|] <[~]token>[ <[~]token> ...]`; see analysis/02-filter-and-calculator.md §1.

use crate::model::*;
use crate::numfmt::{format_frac2, sdate_dmy, to_double};
use std::borrow::Cow;
use std::sync::Arc;

const NBSPACE: char = '\u{2029}';

/// The Java app stored '&' as the fullwidth U+FF06 (SWT mnemonics); match both spellings alike.
fn amp(s: &str) -> String { s.replace('\u{ff06}', "&") }
fn lower(s: &str) -> String { amp(&s.to_lowercase()) }

/// A payment's search text, lower-cased once. The `all` haystack is
/// `"{start} {description} {category} {amount} {note}"` lower-cased; its parts are joined by
/// spaces, which no lower-casing rule (not even the Greek final sigma) looks across, so
/// lower-casing the parts separately gives the same text.
#[derive(Debug)]
struct PaymentHay {
    date: String,
    desc: String,
    cat: Option<String>,
    /// `"{start} {description} {category} "`, the fixed start of the `all` haystack.
    head: String,
    /// The payment's own amount (as bits) and its whole `all` haystack without a note: most
    /// transactions are exactly that.
    amount: u64,
    plain: String,
    /// The description or category contains a `.`.
    dots: bool,
}

/// `format_frac2` is ASCII apart from the uncased infinity sign, so only "NaN" changes case.
fn money(amount: f64) -> String {
    let mut money = format_frac2(amount);
    money.make_ascii_lowercase();
    money
}

impl PaymentHay {
    fn new(p: &Payment) -> PaymentHay {
        let date = sdate_dmy(p.start_date).to_lowercase();
        let desc = lower(&p.description);
        let cat = p.category.as_deref().map(lower);
        let head = format!("{date} {desc} {} ", cat.as_deref().unwrap_or(""));
        let plain = format!("{head}{} ", money(p.amount));
        let dots = desc.contains('.') || cat.as_deref().is_some_and(|c| c.contains('.'));
        PaymentHay { date, desc, cat, head, amount: p.amount.to_bits(), plain, dots }
    }

    /// The `all` haystack of one transaction.
    fn all(&self, amount: f64, note: &str) -> Cow<'_, str> {
        match (amount.to_bits() == self.amount, note.is_empty()) {
            (true, true) => Cow::Borrowed(&self.plain),
            (true, false) => Cow::Owned(format!("{}{note}", self.plain)),
            (false, _) => Cow::Owned(format!("{}{} {note}", self.head, money(amount))),
        }
    }
}

/// A prepared payment with the values its text was made from: a payment changed since is never
/// matched against stale text. (The amount needs no check: `PaymentHay::all` compares it.)
#[derive(Debug)]
struct Prepared { id: i32, description: String, category: Option<String>, start_date: i32, hay: PaymentHay }

/// Search text of every payment, prepared once per document change and shared with each
/// [`Filter`] through [`Filter::set_haystacks`]: typing a search then matches without formatting
/// and lower-casing every transaction's payment again.
#[derive(Debug, Default)]
pub struct Haystacks { by_id: Vec<Prepared> }

impl Haystacks {
    pub fn new(payments: &[Payment]) -> Haystacks {
        let mut by_id: Vec<Prepared> = payments.iter().map(|p| Prepared {
            id: p.id, description: p.description.clone(), category: p.category.clone(), start_date: p.start_date, hay: PaymentHay::new(p),
        }).collect();
        by_id.sort_by_key(|e| e.id);
        Haystacks { by_id }
    }

    fn get(&self, p: &Payment) -> Option<&PaymentHay> {
        let e = &self.by_id[self.by_id.binary_search_by_key(&p.id, |e| e.id).ok()?];
        (e.start_date == p.start_date && e.description == p.description && e.category == p.category).then_some(&e.hay)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Name { Unknown, All, Cat, Desc, Date, Lt, Eq, Gt, Note, Range, Flag }

fn name_of(prefix: &str) -> Name {
    match prefix {
        "all" => Name::All, "cat" => Name::Cat, "desc" => Name::Desc, "date" => Name::Date,
        "lt" => Name::Lt, "eq" => Name::Eq, "gt" => Name::Gt, "note" => Name::Note,
        "range" => Name::Range, "flag" => Name::Flag, _ => Name::Unknown,
    }
}

#[derive(Clone, Debug)]
struct Elm {
    name: Name,
    exp: String,
    inverted: bool,
    val: f64,
    is_dots: bool,
}

/// The `flag:` words `is_match` tests.
fn is_flag_word(exp: &str) -> bool { matches!(exp, "?" | "unpaid" | "." | "paid" | "!" | "important") }

/// The amount a `<`, `=` or `>` term compares with: `toDouble`, with a trailing `k`/`m`/`b`/`t`
/// multiplier as the calculator reads it (`-1k` is -1,000). The text is lower-cased already.
fn comparison_value(exp: &str) -> f64 {
    let (number, mult) = match exp.chars().last() {
        Some('k') => (&exp[..exp.len() - 1], 1e3),
        Some('m') => (&exp[..exp.len() - 1], 1e6),
        Some('b') => (&exp[..exp.len() - 1], 1e9),
        Some('t') => (&exp[..exp.len() - 1], 1e12),
        _ => (exp, 1.0),
    };
    to_double(number) * mult
}

impl Elm {
    fn new(name: Name, exp: String, inverted: bool) -> Elm {
        let val = match name {
            Name::Lt | Name::Eq | Name::Gt => comparison_value(&exp),
            Name::All | Name::Range => to_double(&exp),
            _ => 0.0,
        };
        Elm { is_dots: exp.contains('.'), name, exp, inverted, val }
    }
}

#[derive(Clone, Debug)]
pub struct Filter {
    pub or_mode: bool,
    /// Leading `-`: the caller shows deleted transactions only. Not applied by `is_match`.
    pub deleted: bool,
    pub filter: String,
    /// `yyyymmdd` used by `range:` (the original reads the clock; set it explicitly for tests).
    pub today: i32,
    elms: Vec<Elm>,
    hay: Option<Arc<Haystacks>>,
}

impl Filter {
    /// `new Filter(text)`. Returns `Err` for the one input the original crashes on (a bare `~` token).
    pub fn new(text: &str, today: i32) -> Result<Filter, String> {
        let mut t = text.to_string();
        let deleted = t.starts_with('-');
        if deleted { t.remove(0); }
        let or_mode = t.starts_with('|');
        // "| rent": spaces may follow the OR marker; the terms start at the first other character.
        if or_mode { t = t[1..].trim_start_matches(' ').to_string(); }
        let t = amp(&t.to_lowercase()).replace("\\ ", &NBSPACE.to_string());
        let mut f = Filter { or_mode, deleted, filter: text.to_string(), today, elms: Vec::new(), hay: None };
        let parts: Vec<&str> = t.split(' ').collect();
        if parts.first().is_none_or(|p| p.is_empty()) { return Ok(f); }
        for part in parts {
            if part.is_empty() { continue; }
            let mut tok = part.replace(NBSPACE, " ");
            let inverted = tok.starts_with('~');
            if inverted {
                tok.remove(0);
                if tok.is_empty() { return Err("StringIndexOutOfBoundsException: token '~'".into()); }
            }
            let first = tok.chars().next().unwrap();
            let elm = match first {
                '<' => Elm::new(Name::Lt, tok[1..].to_string(), inverted),
                '=' => Elm::new(Name::Eq, tok[1..].to_string(), inverted),
                '>' => Elm::new(Name::Gt, tok[1..].to_string(), inverted),
                // "invoice: 12" is a plain search: a prefix is a field only when it names one (or
                // the value is a flag word, which the original accepts after any prefix).
                _ => match tok.split_once(':') {
                    Some((prefix, exp)) if name_of(prefix) != Name::Unknown || is_flag_word(exp) => Elm::new(name_of(prefix), exp.to_string(), inverted),
                    _ => Elm::new(Name::All, tok.clone(), inverted),
                },
            };
            f.elms.push(elm);
        }
        Ok(f)
    }

    pub fn is_empty(&self) -> bool { self.elms.is_empty() }

    /// Match against prepared search text (see [`Haystacks`]). Payments it does not hold, or that
    /// changed since it was made, are prepared on the fly as without it.
    pub fn set_haystacks(&mut self, hay: Option<Arc<Haystacks>>) { self.hay = hay; }

    /// `isMatch(pmt, amount, date)`: `date` is the occurrence day, `0` (payments list) or `-1` (planned pass).
    pub fn is_match(&self, pmt: &Payment, amount: f64, date: i32) -> bool {
        if self.elms.is_empty() { return true; }
        let fresh;
        let ph = match self.hay.as_deref().and_then(|h| h.get(pmt)) {
            Some(prepared) => prepared,
            None => { fresh = PaymentHay::new(pmt); &fresh }
        };
        // The occurrence's note and the whole `all` haystack are made only when an element needs them.
        let (note_cell, all_cell) = (std::cell::OnceCell::new(), std::cell::OnceCell::new());
        // `get_note(0)` is `Some("")` for a payment without notes: an empty note is no note.
        let note = || note_cell.get_or_init(|| pmt.get_note(date).filter(|n| !n.is_empty()).map(|n| lower(&n))).as_deref();
        let all = || all_cell.get_or_init(|| ph.all(amount, note().unwrap_or(""))).as_ref();
        let mut count = 0;
        for e in &self.elms {
            let mut hay: Option<&str> = None;
            let mut boolean = false;
            let mut matched = false;
            match e.name {
                Name::All => hay = Some(all()),
                Name::Cat => hay = ph.cat.as_deref(),
                Name::Desc => hay = Some(&ph.desc),
                Name::Date => hay = Some(&ph.date),
                Name::Lt => { boolean = true; matched = amount < e.val; }
                Name::Eq => { boolean = true; matched = e.val == amount; }
                Name::Gt => { boolean = true; matched = amount > e.val; }
                Name::Note => hay = note(),
                Name::Range => { boolean = true; matched = self.range_match(pmt, e); }
                Name::Flag | Name::Unknown => {
                    let f = pmt.get_flag(date);
                    match e.exp.as_str() {
                        "?" | "unpaid" => { boolean = true; matched = f == FLAG_NONE || f == FLAG_IMPORTANT; }
                        "." | "paid" => { boolean = true; matched = f == FLAG_PAID; }
                        "!" | "important" => { boolean = true; matched = f == FLAG_IMPORTANT; }
                        _ => {}
                    }
                }
            }
            if !boolean {
                if let Some(h) = hay {
                    matched = h.contains(&e.exp);
                    if !matched && e.val != 0.0 {
                        matched = amount == e.val;
                    } else if !matched && !e.is_dots && e.name == Name::All
                        && (ph.dots || note().is_some_and(|n| n.contains('.'))) {
                        // "St.John" also matches "stjohn": the text without its dots.
                        let t = format!("{} {} {}", ph.desc, ph.cat.as_deref().unwrap_or(""), note().unwrap_or(""));
                        matched = t.replace('.', "").contains(&e.exp);
                    }
                }
            }
            if e.inverted { matched = !matched; }
            if matched { count += 1; } else if !self.or_mode { return false; }
        }
        count > 0
    }

    fn range_match(&self, pmt: &Payment, e: &Elm) -> bool {
        let indefinite = pmt.end_mode() == END_MODE_INDEFINITE;
        let today = self.today;
        match e.exp.as_str() {
            "disabled" | "d" => return pmt.disabled,
            _ => {}
        }
        if e.val != 0.0 {
            if !(1.0..=9999.0).contains(&e.val) { return false; }
            let y = e.val as i32;
            if pmt.start_date / 10000 < y && (indefinite || y <= pmt.end_date / 10000) { return true; }
            return !pmt.options_in(y * 10000 + 65, y * 10000 + 1231).is_empty();
        }
        match e.exp.as_str() {
            "active" | "a" => pmt.start_date <= today && (indefinite || today <= pmt.end_date),
            "past" | "p" => pmt.end_date < today && !indefinite,
            "future" | "f" => today < pmt.start_date,
            "indefinite" | "i" => indefinite,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_year_out_of_bounds_does_not_overflow() {
        let today = 20250301;
        for t in ["range:99999999", "range:1e9"] {
            let f = Filter::new(t, today).unwrap();
            assert!(!f.is_match(&p(), 1.0, 0));
        }
    }

    fn p() -> Payment {
        let mut p = Payment::new(20250301);
        p.description = "Hydro bill".into();
        p.category = Some("Utils".into());
        p.start_date = 20250301;
        p.end_date = 20271231;
        p.set_end_mode(END_MODE_DATE);
        p.amount = -120.0;
        p
    }
    const TODAY: i32 = 20261027;
    fn m(f: &str, amount: f64, date: i32) -> bool { Filter::new(f, TODAY).unwrap().is_match(&p(), amount, date) }

    #[test]
    fn worked_examples() {
        assert!(m("cat:utils <-100", -120.0, 20261105));
        assert!(!m("cat:utils <-100", -80.0, 20261105));
        assert!(!m("|salary rent", -120.0, 20261105));
        let f = Filter::new("-", TODAY).unwrap();
        assert!(f.deleted && f.is_empty() && f.is_match(&p(), 1.0, 0));
        assert!(!m("range:future ~cat:auto", -120.0, 20261105));
        assert!(!m("flag:paid", -120.0, 20261105));
        assert!(m("~flag:paid", -120.0, 20261105));
        assert!(m(":unpaid", -120.0, 20261105));
        assert!(!m("date:2026-10", -120.0, 20261105));
        assert!(m("date:mar-2025", -120.0, 20261105));
        assert!(m("120", -120.0, 20261105));
        assert!(!m("=120", -120.0, 20261105));
        assert!(m("=-120", -120.0, 20261105));
        assert!(m("hydro\\ bill", -120.0, 20261105));
        assert!(!m("hydro_bill", -120.0, 20261105));
        assert!(m("range:2026", -120.0, 0));
        assert!(m("range:active", -120.0, 0));
        assert!(!m("range:past", -120.0, 0));
        let mut q = p();
        q.description = "St.John".into();
        assert!(Filter::new("stjohn", TODAY).unwrap().is_match(&q, -1.0, 0));
        assert!(!Filter::new("st.john x", TODAY).unwrap().is_match(&q, -1.0, 0));
        assert!(Filter::new("~", TODAY).is_err());
        assert!(Filter::new("-  cat:x", TODAY).unwrap().is_empty());
    }

    /// The matcher before haystacks: formats and lower-cases everything on every call.
    fn reference_is_match(f: &Filter, pmt: &Payment, amount: f64, date: i32) -> bool {
        if f.elms.is_empty() { return true; }
        let cat = pmt.category.as_deref().unwrap_or("");
        let note = pmt.get_note(date).unwrap_or_default();
        let all_hay = amp(&format!("{} {} {} {} {}", sdate_dmy(pmt.start_date), pmt.description, cat, format_frac2(amount), note).to_lowercase());
        let mut count = 0;
        for e in &f.elms {
            let mut hay: Option<String> = None;
            let mut boolean = false;
            let mut matched = false;
            match e.name {
                Name::All => hay = Some(all_hay.clone()),
                Name::Cat => hay = pmt.category.as_ref().map(|c| amp(&c.to_lowercase())),
                Name::Desc => hay = Some(amp(&pmt.description.to_lowercase())),
                Name::Date => hay = Some(sdate_dmy(pmt.start_date).to_lowercase()),
                Name::Lt => { boolean = true; matched = amount < e.val; }
                Name::Eq => { boolean = true; matched = e.val == amount; }
                Name::Gt => { boolean = true; matched = amount > e.val; }
                Name::Note => hay = pmt.get_note(date).filter(|n| !n.is_empty()).map(|n| amp(&n.to_lowercase())),
                Name::Range => { boolean = true; matched = f.range_match(pmt, e); }
                Name::Flag | Name::Unknown => {
                    let fl = pmt.get_flag(date);
                    match e.exp.as_str() {
                        "?" | "unpaid" => { boolean = true; matched = fl == FLAG_NONE || fl == FLAG_IMPORTANT; }
                        "." | "paid" => { boolean = true; matched = fl == FLAG_PAID; }
                        "!" | "important" => { boolean = true; matched = fl == FLAG_IMPORTANT; }
                        _ => {}
                    }
                }
            }
            if !boolean {
                if let Some(h) = &hay {
                    matched = h.contains(&e.exp);
                    if !matched && e.val != 0.0 {
                        matched = amount == e.val;
                    } else if !matched && !e.is_dots && e.name == Name::All {
                        let t = amp(&format!("{} {} {}", pmt.description, cat, note).to_lowercase());
                        if t.contains('.') { matched = t.replace('.', "").contains(&e.exp); }
                    }
                }
            }
            if e.inverted { matched = !matched; }
            if matched { count += 1; } else if !f.or_mode { return false; }
        }
        count > 0
    }

    #[test]
    fn prepared_haystacks_match_exactly_like_formatting_every_transaction() {
        let mut payments = Vec::new();
        for (n, (desc, cat, note)) in [
            ("Hydro bill", Some("Utils"), None),
            ("AT\u{ff06}T", Some("R&D"), Some("Paid by A&B")),
            ("St.John Ambulance", None, Some("Dr. Who")),
            ("ΟΔΟΣ Σ", Some("ΤΑΞΙΔΙ"), Some("ΣΗΜΕΙΩΣΗ")),
            ("İstanbul TRIP", Some("Travel"), Some("İ.")),
            ("NaN tester", None, None),
            ("", Some(""), Some("")),
        ].into_iter().enumerate() {
            let mut q = p();
            q.id = n as i32 + 1;
            q.description = desc.into();
            q.category = cat.map(str::to_string);
            q.start_date = 20250301 + n as i32;
            if let Some(note) = note { q.set_note(20261105, Some(note)); }
            if n % 2 == 0 { q.set_flag(20261105, FLAG_PAID); }
            payments.push(q);
        }
        let hay = Arc::new(Haystacks::new(&payments));
        let filters = ["hydro", "120", "-120", "120.00", "utils -120", "bill utils", "at&t", "a&b", "r&d", "stjohn", "st.john x",
            "drwho", "οδος", "οδοσ", "ς", "σ", "ταξιδι", "σημειωση", "istanbul", "i̇stanbul", "i̇.", "nan", "∞", "mar-2025",
            "01-mar", "desc:hydro", "cat:util", "~cat:util", "note:a&b", "note:who", "date:mar", "date:2025 hydro",
            "|salary hydro", "|~hydro ~bill", "<-100", ">-100 hydro", "=-120", ":unpaid", "flag:paid", "~flag:!",
            "range:2026", "range:active nan", "x:y", "hydro\\ bill", "bill  utils", "  "];
        let mut compared = 0;
        for text in filters {
            let plain = Filter::new(text, TODAY).unwrap();
            let mut prepared = plain.clone();
            prepared.set_haystacks(Some(hay.clone()));
            for q in &payments {
                for amount in [-120.0, 120.0, 0.0, -0.004, 1234.5, f64::NAN, f64::INFINITY] {
                    for date in [20261105, 0, -1] {
                        let expected = reference_is_match(&plain, q, amount, date);
                        assert_eq!(plain.is_match(q, amount, date), expected, "{text:?} on {:?} {amount} {date}", q.description);
                        assert_eq!(prepared.is_match(q, amount, date), expected, "{text:?} prepared on {:?} {amount} {date}", q.description);
                        compared += 1;
                    }
                }
            }
        }
        assert!(compared > 6000);
    }

    #[test]
    fn a_payment_changed_after_preparing_is_matched_on_its_current_text() {
        let mut q = p();
        q.id = 7;
        let hay = Arc::new(Haystacks::new(std::slice::from_ref(&q)));
        let mut f = Filter::new("water", TODAY).unwrap();
        f.set_haystacks(Some(hay.clone()));
        assert!(!f.is_match(&q, -120.0, 0));
        q.description = "Water bill".into();
        assert!(f.is_match(&q, -120.0, 0), "a renamed payment must not be matched by its old text");
        let mut by_cat = Filter::new("cat:home", TODAY).unwrap();
        by_cat.set_haystacks(Some(hay.clone()));
        q.category = Some("Home".into());
        assert!(by_cat.is_match(&q, -120.0, 0));
        let mut by_date = Filter::new("date:2024", TODAY).unwrap();
        by_date.set_haystacks(Some(hay.clone()));
        q.start_date = 20240101;
        assert!(by_date.is_match(&q, -120.0, 0));
        let mut by_amount = Filter::new("77.00", TODAY).unwrap();
        by_amount.set_haystacks(Some(hay));
        q.amount = 77.0;
        assert!(by_amount.is_match(&q, 77.0, 0) && !by_amount.is_match(&q, -120.0, 0));
    }

    fn named(description: &str) -> Payment {
        let mut q = p();
        q.description = description.into();
        q
    }

    #[test]
    fn or_marker_followed_by_spaces_filters_by_its_terms() {
        // "| rent" used to match every transaction: the spaces left an empty first token.
        let rent = named("Rent");
        let hydro = p();
        for text in ["| rent", "|rent", "|  rent"] {
            let f = Filter::new(text, TODAY).unwrap();
            assert!(f.or_mode, "{text:?} is OR mode");
            assert!(f.is_match(&rent, -120.0, 0), "{text:?} must match Rent");
            assert!(!f.is_match(&hydro, -120.0, 0), "{text:?} must not match Hydro bill");
        }
        // "-" stays the deleted-only filter, and "-  cat:x" keeps its no-element reading.
        assert!(Filter::new("- rent", TODAY).unwrap().is_empty());
    }

    #[test]
    fn comparisons_accept_the_calculator_multiplier_suffixes() {
        // "<-1k" used to read -1k as -1, so -500 (and every amount above -1,000) matched.
        assert!(!m("<-1k", -500.0, 0) && m("<-1k", -1500.0, 0));
        assert!(m(">-1k", -500.0, 0) && !m(">-1k", -1500.0, 0));
        assert!(m("=-1k", -1000.0, 0) && !m("=-1k", -1.0, 0));
        assert!(m("<-1m", -2_000_000.0, 0) && !m("<-1m", -500_000.0, 0));
        assert!(m("<-1b", -2e9, 0) && !m("<-1b", -5e8, 0));
        assert!(m("<-1t", -2e12, 0) && !m("<-1t", -5e11, 0));
        assert!(m("<-2.5K", -2600.0, 0) && !m("<-2.5K", -2400.0, 0));
        assert!(m("lt:-1k", -1500.0, 0) && m("gt:-1k", -500.0, 0));
        assert!(m("<$-1k", -1500.0, 0), "the $ sign is still ignored");
        // unknown trailing text keeps the lenient reading of the number before it (-1, as toDouble)
        assert!(m("<-1kx", -500.0, 0));
    }

    #[test]
    fn note_filter_with_no_notes_is_false() {
        // get_note(0) is Some("") for a payment without notes, which matched every note term.
        let plain = p();
        assert!(!Filter::new("note:", TODAY).unwrap().is_match(&plain, -120.0, 0));
        assert!(!Filter::new("note:shell", TODAY).unwrap().is_match(&plain, -120.0, 0));
        assert!(!Filter::new("note:shell", TODAY).unwrap().is_match(&plain, -120.0, 20261105));
        assert!(Filter::new("~note:shell", TODAY).unwrap().is_match(&plain, -120.0, 0));
        let mut noted = p();
        noted.set_note(20261105, Some("Shell"));
        assert!(Filter::new("note:shell", TODAY).unwrap().is_match(&noted, -120.0, 0));
        assert!(Filter::new("note:shell", TODAY).unwrap().is_match(&noted, -120.0, 20261105));
        assert!(!Filter::new("note:shell", TODAY).unwrap().is_match(&noted, -120.0, 20261106));
    }

    #[test]
    fn a_word_before_a_colon_is_a_plain_term_unless_it_is_a_field() {
        // "invoice: 12" used to read "invoice" as an unknown field and never matched.
        let mut invoice = named("Invoice: 12");
        invoice.category = Some("Auto".into());
        assert!(Filter::new("invoice: 12", TODAY).unwrap().is_match(&invoice, -120.0, 0));
        assert!(Filter::new("invoice:", TODAY).unwrap().is_match(&invoice, -120.0, 0));
        assert!(!Filter::new("invoice: 13", TODAY).unwrap().is_match(&invoice, -120.0, 0));
        assert!(Filter::new("10:30 meeting", TODAY).unwrap().is_match(&named("10:30 Meeting"), -120.0, 0));
        // known fields still work, and so do flag words after any prefix
        assert!(Filter::new("cat:Auto", TODAY).unwrap().is_match(&invoice, -120.0, 0));
        assert!(!Filter::new("cat:Utils", TODAY).unwrap().is_match(&invoice, -120.0, 0));
        let mut paid = p();
        paid.set_flag(20261105, FLAG_PAID);
        assert!(Filter::new("invoice:paid", TODAY).unwrap().is_match(&paid, -120.0, 20261105));
        assert!(Filter::new(":paid", TODAY).unwrap().is_match(&paid, -120.0, 20261105));
    }

    #[test]
    fn ampersands_match_both_the_plain_and_legacy_fullwidth_spelling() {
        let mut q = p();
        q.description = "AT&T".into();
        q.category = Some("R&D".into());
        let mut legacy = q.clone();
        legacy.description = "AT\u{ff06}T".into();
        legacy.category = Some("R\u{ff06}D".into());
        for text in ["desc:at&t", "desc:AT\u{ff06}T", "cat:r&d", "at&t"] {
            let f = Filter::new(text, TODAY).unwrap();
            assert!(f.is_match(&q, -1.0, 0), "{text} must match AT&T");
            assert!(f.is_match(&legacy, -1.0, 0), "{text} must match a Java-saved AT\u{ff06}T");
        }
    }
}
