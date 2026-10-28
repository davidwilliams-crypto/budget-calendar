//! Port of `ca.mishell.app.budget.Filter` — the transaction filter language.
//! Grammar: `[-][|] <[~]token>[ <[~]token> ...]`; see analysis/02-filter-and-calculator.md §1.

use crate::model::*;
use crate::numfmt::{format_frac2, sdate_dmy, to_double};

const NBSPACE: char = '\u{2029}';

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

impl Elm {
    fn new(name: Name, exp: String, inverted: bool) -> Elm {
        let val = if matches!(name, Name::All | Name::Lt | Name::Eq | Name::Gt | Name::Range) { to_double(&exp) } else { 0.0 };
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
}

impl Filter {
    /// `new Filter(text)`. Returns `Err` for the one input the original crashes on (a bare `~` token).
    pub fn new(text: &str, today: i32) -> Result<Filter, String> {
        let mut t = text.to_string();
        let deleted = t.starts_with('-');
        if deleted { t.remove(0); }
        let or_mode = t.starts_with('|');
        if or_mode { t.remove(0); }
        let t = t.to_lowercase().replace("\\ ", &NBSPACE.to_string());
        let mut f = Filter { or_mode, deleted, filter: text.to_string(), today, elms: Vec::new() };
        let parts: Vec<&str> = t.split(' ').collect();
        if parts.first().map_or(true, |p| p.is_empty()) { return Ok(f); }
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
                _ => match tok.split_once(':') {
                    Some((prefix, exp)) => Elm::new(name_of(prefix), exp.to_string(), inverted),
                    None => Elm::new(Name::All, tok.clone(), inverted),
                },
            };
            f.elms.push(elm);
        }
        Ok(f)
    }

    pub fn is_empty(&self) -> bool { self.elms.is_empty() }

    /// `isMatch(pmt, amount, date)`: `date` is the occurrence day, `0` (payments list) or `-1` (planned pass).
    pub fn is_match(&self, pmt: &Payment, amount: f64, date: i32) -> bool {
        if self.elms.is_empty() { return true; }
        let cat = pmt.category.as_deref().unwrap_or("");
        let note = pmt.get_note(date).unwrap_or_default();
        let all_hay = format!("{} {} {} {} {}", sdate_dmy(pmt.start_date), pmt.description, cat, format_frac2(amount), note).to_lowercase();
        let mut count = 0;
        for e in &self.elms {
            let mut hay: Option<String> = None;
            let mut boolean = false;
            let mut matched = false;
            match e.name {
                Name::All => hay = Some(all_hay.clone()),
                Name::Cat => hay = pmt.category.as_ref().map(|c| c.to_lowercase()),
                Name::Desc => hay = Some(pmt.description.to_lowercase()),
                Name::Date => hay = Some(sdate_dmy(pmt.start_date).to_lowercase()),
                Name::Lt => { boolean = true; matched = amount < e.val; }
                Name::Eq => { boolean = true; matched = e.val == amount; }
                Name::Gt => { boolean = true; matched = amount > e.val; }
                Name::Note => hay = pmt.get_note(date).map(|n| n.to_lowercase()),
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
                if let Some(h) = &hay {
                    matched = h.contains(&e.exp);
                    if !matched && e.val != 0.0 {
                        matched = amount == e.val;
                    } else if !matched && !e.is_dots && e.name == Name::All {
                        let t = format!("{} {} {}", pmt.description, cat, note).to_lowercase();
                        if t.contains('.') { matched = t.replace('.', "").contains(&e.exp); }
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
}
