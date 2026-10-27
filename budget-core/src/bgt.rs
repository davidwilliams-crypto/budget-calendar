//! The `.bgt` file format (loader `BudgetDB.loadFileData`, saver `BudgetDB.save`).
//! Plain (unencrypted) files only for now; `#db*encrypted` files are rejected with an error.

use crate::crypto::{xor_line, LineCipher, HEADER_BLOWFISH, HEADER_XOR};
use crate::javafmt::{java_double, parse_double, parse_int};
use crate::model::*;
use std::path::Path;

const EOD_PREFIX: &str = "## END-OF-DATA; line_no=";
const EOD_TRAILER: &str = " ##";

/// Result of parsing a file.
#[derive(Debug, Default)]
pub struct Loaded {
    pub payments: Vec<Payment>,
    /// Balance adjustments (`opt`) and day notes (`note`) from the `:` line.
    pub adjustments: Option<Payment>,
    pub notes: String,
    pub version: String,
    pub log: String,
}

/// `str2unicodish` + legacy pass: `,` `\n` `\r` and every char >= 128 become `&#xhhhh;`.
pub fn encode_description(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) >= 128 || c == ',' || c == '\n' || c == '\r' {
            out.push_str(&format!("&#x{:04x};", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// Inverse of `encode_description`, also accepting the legacy U+00FF (`,`) / U+00FE (`\n`) bytes.
pub fn decode_description(s: &str) -> String {
    let v: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        let c = v[i];
        if c == '\u{ff}' { out.push(','); i += 1; }
        else if c == '\u{fe}' { out.push('\n'); i += 1; }
        else if v.len() - i >= 8 && c == '&' && v[i + 1] == '#' && v[i + 2] == 'x' && v[i + 7] == ';' {
            let hex: String = v[i + 3..i + 7].iter().collect();
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(ch) => out.push(ch),
                None => out.push('?'),
            }
            i += 8;
        } else { out.push(c); i += 1; }
    }
    out
}

/// Java `StringTokenizer(line, ",")`: empty tokens are skipped; every token is trimmed.
fn tokens(line: &str) -> Vec<&str> {
    // an all-blank token (the ` ` the saver puts before the options) is a token; only `,,` is skipped
    line.split(',').filter(|t| !t.is_empty()).map(str::trim).collect()
}

fn parse_options(p: &mut Payment, toks: &[&str]) -> Result<(), String> {
    for &tok in toks {
        if tok.is_empty() { continue; }
        if let Some(rest) = tok.strip_prefix('$') {
            let rest = rest.trim();
            if !rest.is_empty() { p.category = Some(decode_description(rest)); }
            continue;
        }
        let n = tok.chars().count();
        if n < 8 { return Err(format!("invalid option: '{tok}'")); }
        let head = tok.get(..8).ok_or_else(|| format!("invalid option: '{tok}'"))?;
        let date = parse_int(head)?;
        if n == 8 { p.add_no_transaction(date); continue; }
        let c = tok[8..].chars().next().unwrap();
        let rest = &tok[8 + c.len_utf8()..];
        match c {
            ';' | '#' | '^' => p.set_flag(date, c),
            '=' => { let t = rest.trim(); if !t.is_empty() { p.set_note(date, Some(&decode_description(t))); } }
            ':' => p.add_custom_amount(date, parse_double(rest)?),
            '&' => p.set_hilighting(date, i32::from_str_radix(rest, 16).map_err(|e| e.to_string())?),
            _ => return Err(format!("Invalid option: '{tok}'")),
        }
    }
    Ok(())
}

/// Parse one payment line (everything after the first token has been checked not to be `:`, `=`, `v`).
fn parse_payment(mut first: &str, toks: &[&str], id: i32, today: i32, field: &mut &'static str) -> Result<Payment, String> {
    let mut p = Payment::new(today);
    p.id = id;
    p.flag_value = FLAG_NONE;
    for (ch, action) in [('^', 0), (';', 1), ('#', 2), ('!', 3)] {
        if let Some(i) = first.find(ch) {
            match action {
                0 => p.flag_value = FLAG_NONE,
                1 => p.flag_value = FLAG_PAID,
                2 => p.flag_value = FLAG_IMPORTANT,
                _ => p.disabled = true,
            }
            first = &first[..i];
        }
    }
    if let Some(i) = first.find('%') {
        *field = "loan amount";
        p.loan_amount = parse_double(&first[i + 1..])?;
        first = &first[..i];
    }
    *field = "amount";
    p.amount = parse_double(first)?;
    let mut k = 0usize;
    macro_rules! next {
        ($f:expr) => {{
            *field = $f;
            let t: &str = toks.get(k).copied().ok_or_else(|| "NoSuchElementException".to_string())?;
            k += 1;
            t
        }};
    }
    p.description = decode_description(next!("description"));
    p.start_date = parse_int(next!("start_date"))?;
    let mut freq = next!("frequency").to_string();
    if let Some(i) = freq.find(';') {
        p.grace_period_days = parse_int(&freq[i + 1..])?;
        freq.truncate(i);
    }
    let ft = freq.chars().last().ok_or("empty frequency")?;
    p.frequency = parse_int(&freq[..freq.len() - ft.len_utf8()])?;
    p.freq_type = ft;
    if !"dwmyWL".contains(ft) {
        return Err(format!("Invalid frequency type '{ft}'"));
    }
    if ft == 'W' { p.frequency = 1; }
    p.n_pays = parse_int(next!("n_pays"))?;
    p.end_date = parse_int(next!("end_date"))?;
    p.end_date = p.end_date.max(p.start_date);
    p.flags = parse_int(next!("flags"))?;
    if p.end_mode() == END_MODE_UNDEFINED {
        if p.n_pays == 0 && p.start_date == p.end_date { p.set_end_mode(END_MODE_INDEFINITE); }
        else if p.n_pays != 0 { p.set_end_mode(END_MODE_N_PAYS); }
        else { p.set_end_mode(END_MODE_DATE); }
    }
    p.image_path = Some(decode_description(next!("image_path")));
    *field = "opts";
    parse_options(&mut p, &toks[k..])?;
    Ok(p)
}

/// `isEncrypted`: first line is one of the two encryption headers.
pub fn is_encrypted_text(text: &str) -> bool {
    let first = text.split('\n').next().unwrap_or("").trim_end_matches('\r');
    first == HEADER_XOR || first == HEADER_BLOWFISH
}

/// `isDecryptable(path, password)`: the file parses with this password.
pub fn is_decryptable(text: &str, password: &str) -> bool {
    load_str(text, 20000101, Some(password)).is_ok()
}

enum Mode<'a> { Plain, Xor(&'a str), Blowfish(LineCipher) }

/// Parse the text of a `.bgt` file. `today` seeds new payments' default dates; `password` is used
/// only when the first line is an encryption header (a plain file ignores it, like the original).
pub fn load_str(text: &str, today: i32, password: Option<&str>) -> Result<Loaded, Loaded> {
    let mut r = Loaded { version: "1".into(), ..Default::default() };
    let mut adjustments = Payment::new(today);
    let mut errors = 0;
    let mut line_no = 0;
    let mut next_id = 1;
    let mut mode = Mode::Plain;
    for raw in text.split('\n') {
        let raw_line = raw.strip_suffix('\r').unwrap_or(raw);
        line_no += 1;
        if line_no == 1 {
            if raw_line == HEADER_XOR {
                mode = match password { Some(p) => Mode::Xor(p), None => Mode::Plain };
                continue;
            }
            if raw_line == HEADER_BLOWFISH {
                match password.map(LineCipher::new) {
                    Some(Ok(c)) => { mode = Mode::Blowfish(c); continue; }
                    Some(Err(e)) => { r.log.push_str(&format!("unable to open file.\nException: {e}\nfailed")); return Err(r); }
                    None => { r.log.push_str("unable to open file.\nException: NullPointerException (no password)\nfailed"); return Err(r); }
                }
            }
        }
        let decoded;
        let line: &str = match &mode {
            Mode::Plain => raw_line,
            Mode::Xor(p) => { decoded = xor_line(raw_line, p); &decoded }
            Mode::Blowfish(c) => match c.decrypt_line(raw_line) {
                Ok(d) => { decoded = d; &decoded }
                Err(e) => { r.log.push_str(&format!("unable to open file.\nException: {e}\nfailed")); return Err(r); }
            },
        };
        if line.is_empty() || line.starts_with('#') {
            if let Some(rest) = line.strip_prefix(EOD_PREFIX) {
                match rest.find(EOD_TRAILER).and_then(|i| rest[..i].parse::<i32>().ok()) {
                    Some(n) if n == line_no => break,
                    Some(n) => {
                        r.log.push_str(&format!("mismatched number of read lines! line_no={line_no}, file_line_no={n}\n"));
                        errors += 1;
                        break;
                    }
                    None => {
                        r.log.push_str(&format!("misformed end-of-data line: line_no={line_no}, text:'{line}'\n"));
                        errors += 1;
                        break;
                    }
                }
            }
            continue;
        }
        let toks = tokens(line);
        let Some(&first) = toks.first() else { continue };
        let mut field: &'static str = "";
        let res: Result<(), String> = (|| {
            match first {
                ":" => { field = "opts"; parse_options(&mut adjustments, &toks[1..]) }
                "=" => { field = "file notes"; if let Some(t) = toks.get(1) { r.notes = decode_description(t); } Ok(()) }
                "v" => { field = "version"; if let Some(t) = toks.get(1) { r.version = decode_description(t); } Ok(()) }
                _ => {
                    let id = next_id;
                    let p = parse_payment(first, &toks[1..], id, today, &mut field);
                    // the original increments the id right after the amount parses
                    if field != "amount" && field != "loan amount" { next_id += 1; }
                    r.payments.push(p?);
                    Ok(())
                }
            }
        })();
        if let Err(e) = res {
            r.log.push_str(&format!("({line_no}): Invalid data for '{field}'\nException: {e}\n"));
            errors += 1;
        }
    }
    r.adjustments = Some(adjustments);
    if errors != 0 {
        r.log.push_str("failed");
        return Err(r);
    }
    r.log.push_str(&format!("done.\n{} entries loaded.\n", r.payments.len()));
    Ok(r)
}

/// ISO-8859-1 bytes -> text.
pub fn latin1_decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}
/// Text -> ISO-8859-1 bytes (chars above 0xFF become `?`, like Java).
pub fn latin1_encode(s: &str) -> Vec<u8> {
    s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
}

pub fn load_file(path: &Path, today: i32, password: Option<&str>) -> Result<Loaded, Loaded> {
    match std::fs::read(path) {
        Ok(b) => load_str(&latin1_decode(&b), today, password),
        Err(e) => Err(Loaded { log: format!("unable to open '{}' file.\nException: {e}\nfailed", path.display()), ..Default::default() }),
    }
}

/// `optsString(p)`.
pub fn opts_string(p: &Payment) -> String {
    let mut s = String::new();
    if let Some(c) = &p.category { s.push_str(", $"); s.push_str(&encode_description(c)); }
    for o in &p.opt {
        if o.deleted { s.push_str(&format!(", {}", o.date)); }
        else { s.push_str(&format!(", {}:{}", o.date, java_double(o.amount))); }
    }
    for f in &p.flag { s.push_str(&format!(", {}{}", f.date, f.value)); }
    for n in &p.note { s.push_str(&format!(", {}={}", n.date, encode_description(&n.note))); }
    for h in &p.hilighting { s.push_str(&format!(", {}&{:x}", h.date, h.bg_color)); }
    s
}

/// One payment line, exactly as the original writes it (no newline).
pub fn payment_line(p: &Payment) -> String {
    let loan = if p.end_mode() == END_MODE_LOAN_PAID || p.loan_amount != 0.0 { format!("%{}", java_double(p.loan_amount)) } else { String::new() };
    let dis = if p.disabled { "!" } else { "" };
    let fl = if p.flag_value != FLAG_NONE { p.flag_value.to_string() } else { String::new() };
    let grace = if p.grace_period_days != 0 { format!(";{}", p.grace_period_days) } else { String::new() };
    let img = p.image_path.as_deref().map(encode_description).unwrap_or_default();
    format!(
        "{}{loan}{dis}{fl}, {}, {}, {}{}{grace}, {}, {}, {}, {} {}",
        java_double(p.amount), encode_description(&p.description), p.start_date, p.frequency, p.freq_type,
        p.n_pays, p.end_date, p.flags, img, opts_string(p)
    )
}

/// Whole file text including the END-OF-DATA trailer (no final newline). With a password every
/// line is Blowfish-encrypted and the `#db*encrypted` header is prepended (it counts as a line).
pub fn save_str(payments: &[Payment], adjustments: &Payment, notes: &str, password: Option<&str>) -> Result<String, String> {
    let cipher = match password { Some(p) => Some(LineCipher::new(p)?), None => None };
    let enc = |s: String| match &cipher { Some(c) => c.encrypt_line(&s), None => s };
    let mut out = String::new();
    let mut n = 0;
    if cipher.is_some() { n += 1; out.push_str(HEADER_BLOWFISH); out.push('\n'); }
    let mut line = |s: String, out: &mut String| { n += 1; out.push_str(&enc(s)); out.push('\n'); };
    line("v,2".into(), &mut out);
    line(format!(":{}", opts_string(adjustments)), &mut out);
    line(format!("=,{}", encode_description(notes)), &mut out);
    for p in payments { line(payment_line(p), &mut out); }
    n += 1;
    out.push_str(&enc(format!("{EOD_PREFIX}{n}{EOD_TRAILER}")));
    Ok(out)
}

/// `BudgetDB.save` disk procedure: skip when byte-identical, back up to `.bak[.N]`, write, delete backup on success.
pub fn save_file(path: &Path, content: &str) -> Result<(), String> {
    let bytes = latin1_encode(content);
    let mut backup: Option<std::path::PathBuf> = None;
    if path.exists() {
        if let Ok(old) = std::fs::read(path) {
            if old == bytes { return Ok(()); }
        }
        let base = format!("{}.bak", path.display());
        let mut cand = std::path::PathBuf::from(&base);
        let mut i = 0;
        while cand.exists() {
            if i > 100 { return Err("Too many backup files, try using 'Save as..'!".into()); }
            i += 1;
            cand = std::path::PathBuf::from(format!("{base}.{i}"));
        }
        std::fs::copy(path, &cand).map_err(|_| "Failed to create a backup file, try using 'Save as..'!".to_string())?;
        backup = Some(cand);
    }
    std::fs::write(path, &bytes).map_err(|e| format!("File Save:{e}"))?;
    if let Some(b) = backup { let _ = std::fs::remove_file(b); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec() {
        assert_eq!(encode_description("a,b\nc"), "a&#x002c;b&#x000a;c");
        assert_eq!(decode_description("a&#x002c;b&#x000a;c\u{ff}d"), "a,b\nc,d");
        assert_eq!(decode_description("&#xzzzz;"), "?");
    }

    #[test]
    fn encrypted_roundtrip() {
        let mut p = Payment::new(20240101);
        p.description = "Rent".into();
        p.amount = -1100.0;
        let adj = Payment::new(20240101);
        let text = save_str(&[p.clone()], &adj, "my notes", Some("pw")).unwrap();
        assert!(text.starts_with("#db*encrypted\n"));
        assert!(is_encrypted_text(&text));
        assert!(!is_decryptable(&text, "wrong"));
        assert!(is_decryptable(&text, "pw"));
        let l = load_str(&text, 20240101, Some("pw")).unwrap_or_else(|l| panic!("{}", l.log));
        assert_eq!(l.notes, "my notes");
        assert_eq!(l.payments[0].description, "Rent");
        assert_eq!(l.payments[0].amount, -1100.0);
        assert!(load_str(&text, 20240101, None).is_err());
        // legacy XOR file reads back too
        // (a real legacy file counted its header in the trailer; drop the trailer, which is optional)
        let plain = save_str(&[p], &adj, "", None).unwrap();
        let plain = &plain[..plain.rfind('\n').unwrap()];
        let xored: String = plain.split('\n').map(|l| xor_line(l, "k")).collect::<Vec<_>>().join("\n");
        let legacy = format!("{HEADER_XOR}\n{xored}");
        let l = load_str(&legacy, 20240101, Some("k")).unwrap_or_else(|l| panic!("{}", l.log));
        assert_eq!(l.payments[0].description, "Rent");
    }
}
