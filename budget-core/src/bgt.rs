//! The `.bgt` file format (loader `BudgetDB.loadFileData`, saver `BudgetDB.save`).
//! Reads plain, Blowfish (`#db*encrypted`) and legacy XOR files; saves plain or Blowfish.

use crate::crypto::{xor_line, LineCipher, HEADER_BLOWFISH, HEADER_XOR};
use crate::date::DATE_MAX;
use crate::javafmt::{java_double, parse_double, parse_int};
use crate::model::*;
use std::path::Path;

const EOD_PREFIX: &str = "## END-OF-DATA; line_no=";
const EOD_TRAILER: &str = " ##";
/// Upper bound for a loaded payment count (a daily schedule up to year 9999).
const MAX_N_PAYS: i32 = 4_000_000;

/// Deliberate deviation: clamp a loaded date to `0..=DATE_MAX` so the calendar arithmetic cannot
/// overflow.
fn clamp_date(v: i32, name: &str, log: &mut String, line_no: i32) -> i32 {
    let c = v.clamp(0, DATE_MAX);
    if c != v { log.push_str(&format!("({line_no}): {name} {v} out of range, using {c}\n")); }
    c
}

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
/// Deliberate deviations (all decodable by the original): C0 controls other than tab (a NUL
/// would truncate an encrypted line), leading/trailing spaces and tabs (the loader trims every
/// token) and an `&` that starts an `&#x????;` window (it would be decoded on reload) are escaped
/// too, so every text field round-trips exactly.
pub fn encode_description(s: &str) -> String {
    let v: Vec<char> = s.chars().collect();
    let lead = v.iter().take_while(|c| **c == ' ' || **c == '\t').count();
    let trail = if lead == v.len() { 0 } else { v.iter().rev().take_while(|c| **c == ' ' || **c == '\t').count() };
    let mut out = String::with_capacity(s.len());
    for (i, &c) in v.iter().enumerate() {
        let edge = i < lead || i >= v.len() - trail;
        let entity = c == '&' && v.len() - i >= 8 && v[i + 1] == '#' && v[i + 2] == 'x' && v[i + 7] == ';';
        if (c as u32) > 0xFFFF {
            // Java works on UTF-16 units: write the surrogate pair
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) { out.push_str(&format!("&#x{:04x};", u)); }
        } else if (c as u32) >= 128 || c == ',' || ((c as u32) < 0x20 && c != '\t') || edge || entity {
            out.push_str(&format!("&#x{:04x};", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// `BufferedReader.readLine` terminators: `\r\n`, `\n` or a lone `\r`. Like `split('\n')`, the
/// final unterminated piece (empty after a trailing terminator) is yielded too.
pub(crate) fn logical_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let s = rest?;
        match s.find(['\n', '\r']) {
            Some(i) => {
                let skip = if s[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = Some(&s[i + skip..]);
                Some(&s[..i])
            }
            None => { rest = None; Some(s) }
        }
    })
}

/// Inverse of `encode_description`, also accepting the legacy U+00FF (`,`) / U+00FE (`\n`) bytes.
pub fn decode_description(s: &str) -> String {
    let v: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(v.len());
    let mut pending: Vec<u16> = Vec::new();
    fn flush(pending: &mut Vec<u16>, out: &mut String) {
        for r in char::decode_utf16(pending.drain(..)) { out.push(r.unwrap_or('?')); }
    }
    let mut i = 0;
    while i < v.len() {
        let c = v[i];
        if v.len() - i >= 8 && c == '&' && v[i + 1] == '#' && v[i + 2] == 'x' && v[i + 7] == ';' {
            let hex: String = v[i + 3..i + 7].iter().collect();
            match u16::from_str_radix(&hex, 16) {
                Ok(u) => pending.push(u),
                Err(_) => { flush(&mut pending, &mut out); out.push('?'); }
            }
            i += 8;
            continue;
        }
        flush(&mut pending, &mut out);
        if c == '\u{ff}' { out.push(','); }
        else if c == '\u{fe}' { out.push('\n'); }
        else { out.push(c); }
        i += 1;
    }
    flush(&mut pending, &mut out);
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
        // Deliberate deviation: a negative/huge date would overflow the calendar arithmetic.
        if !(0..=DATE_MAX).contains(&date) { return Err(format!("invalid option: '{tok}'")); }
        if n == 8 { p.add_no_transaction(date); continue; }
        let c = tok[8..].chars().next().unwrap();
        let rest = &tok[8 + c.len_utf8()..];
        match c {
            ';' | '#' | '^' => p.set_flag(date, c),
            '=' => { let t = rest.trim(); if !t.is_empty() { p.set_note(date, Some(&decode_description(t))); } }
            ':' => p.add_custom_amount(date, parse_double(rest)?),
            '&' => {
                // -1 removes (as in the original); anything else is kept to 24-bit RGB so the
                // saved `{:x}` stays readable (a negative value would be written as 8 digits).
                let c = i32::from_str_radix(rest, 16).map_err(|e| e.to_string())?;
                p.set_hilighting(date, if c == -1 { -1 } else { c & 0xFF_FFFF });
            }
            _ => return Err(format!("Invalid option: '{tok}'")),
        }
    }
    Ok(())
}

/// Why a line did not parse: the field being read (named in the load log) and the error.
struct FieldError {
    field: &'static str,
    message: String,
}

/// `map_err` adapter naming the field an error belongs to.
fn in_field(field: &'static str) -> impl Fn(String) -> FieldError + Copy {
    move |message| FieldError { field, message }
}

/// The comma-separated fields of a payment line after the amount, read in order.
struct Fields<'a>(std::slice::Iter<'a, &'a str>);

impl<'a> Fields<'a> {
    fn text(&mut self, field: &'static str) -> Result<&'a str, FieldError> {
        self.0.next().copied().ok_or_else(|| FieldError { field, message: "NoSuchElementException".into() })
    }
    fn int(&mut self, field: &'static str) -> Result<i32, FieldError> {
        parse_int(self.text(field)?).map_err(in_field(field))
    }
}

/// Parse one payment line (everything after the first token has been checked not to be `:`, `=`, `v`).
fn parse_payment(mut first: &str, toks: &[&str], id: i32, today: i32, log: &mut String, line_no: i32) -> Result<Payment, FieldError> {
    let mut p = Payment::new(today);
    p.id = id;
    p.flag_value = FLAG_NONE;
    // Markers after the amount: the default transaction status, and `!` for a disabled payment.
    for (ch, status) in [('^', Some(FLAG_NONE)), (';', Some(FLAG_PAID)), ('#', Some(FLAG_IMPORTANT)), ('!', None)] {
        if let Some(i) = first.find(ch) {
            match status {
                Some(status) => p.flag_value = status,
                None => p.disabled = true,
            }
            first = &first[..i];
        }
    }
    if let Some(i) = first.find('%') {
        p.loan_amount = parse_double(&first[i + 1..]).map_err(in_field("loan amount"))?;
        first = &first[..i];
    }
    p.amount = parse_double(first).map_err(in_field("amount"))?;
    let mut fields = Fields(toks.iter());
    p.description = decode_description(fields.text("description")?);
    p.start_date = clamp_date(fields.int("start_date")?, "start_date", log, line_no);
    let in_frequency = in_field("frequency");
    let mut freq = fields.text("frequency")?.to_string();
    if let Some(i) = freq.find(';') {
        p.grace_period_days = parse_int(&freq[i + 1..]).map_err(in_frequency)?;
        freq.truncate(i);
    }
    let ft = freq.chars().last().ok_or_else(|| in_frequency("empty frequency".into()))?;
    p.frequency = parse_int(&freq[..freq.len() - ft.len_utf8()]).map_err(in_frequency)?;
    p.freq_type = ft;
    if !"dwmyWL".contains(ft) {
        return Err(in_frequency(format!("Invalid frequency type '{ft}'")));
    }
    // Deliberate deviation: clamp so corrupt files cannot hang or overflow the engine.
    if ft == 'W' { p.frequency = 1; }
    let freq_in = p.frequency;
    p.frequency = p.frequency.clamp(1, 100_000);
    if p.frequency != freq_in { log.push_str(&format!("({line_no}): frequency {freq_in} out of range, using {}\n", p.frequency)); }
    p.n_pays = fields.int("n_pays")?;
    let n_in = p.n_pays;
    // A negative count is corrupt: use the default of 1 (not 0, which could read as "indefinite").
    p.n_pays = if n_in < 0 { 1 } else { n_in.min(MAX_N_PAYS) };
    if p.n_pays != n_in { log.push_str(&format!("({line_no}): n_pays {n_in} out of range, using {}\n", p.n_pays)); }
    p.end_date = clamp_date(fields.int("end_date")?, "end_date", log, line_no);
    p.end_date = p.end_date.max(p.start_date);
    p.flags = fields.int("flags")?;
    if p.end_mode() == END_MODE_UNDEFINED {
        if p.n_pays == 0 && p.start_date == p.end_date { p.set_end_mode(END_MODE_INDEFINITE); }
        else if p.n_pays != 0 { p.set_end_mode(END_MODE_N_PAYS); }
        else { p.set_end_mode(END_MODE_DATE); }
    }
    p.image_path = Some(decode_description(fields.text("image_path")?));
    parse_options(&mut p, fields.0.as_slice()).map_err(in_field("opts"))?;
    Ok(p)
}

/// `isEncrypted`: first line is one of the two encryption headers.
pub fn is_encrypted_text(text: &str) -> bool {
    let first = logical_lines(text).next().unwrap_or("");
    first == HEADER_XOR || first == HEADER_BLOWFISH
}

/// `isDecryptable(path, password)`: the file parses with this password.
pub fn is_decryptable(text: &str, password: &str) -> bool {
    load_str(text, 20000101, Some(password)).is_ok()
}

enum Mode<'a> { Plain, Xor(&'a str), Blowfish(Box<LineCipher>) }

/// Parse the text of a `.bgt` file. `today` seeds new payments' default dates; `password` is used
/// only when the first line is an encryption header (a plain file ignores it, like the original).
pub fn load_str(text: &str, today: i32, password: Option<&str>) -> Result<Loaded, Box<Loaded>> {
    let first = load_with(text, today, password, false);
    if first.is_err() {
        // Files saved by earlier builds with a 1-3 byte password used a zero-padded key.
        if let Some(p) = password.filter(|p| !p.is_empty() && p.len() < 4) {
            if logical_lines(text).next() == Some(HEADER_BLOWFISH) {
                if let ok @ Ok(_) = load_with(text, today, Some(p), true) { return ok; }
            }
        }
    }
    first
}

fn load_with(text: &str, today: i32, password: Option<&str>, legacy_key: bool) -> Result<Loaded, Box<Loaded>> {
    let mut r = Loaded { version: "1".into(), ..Default::default() };
    let mut adjustments = Payment::new(today);
    let mut errors = 0;
    let mut line_no = 0;
    let mut next_id = 1;
    let mut mode = Mode::Plain;
    for raw_line in logical_lines(text) {
        line_no += 1;
        if line_no == 1 {
            if raw_line == HEADER_XOR {
                mode = match password { Some(p) => Mode::Xor(p), None => Mode::Plain };
                continue;
            }
            if raw_line == HEADER_BLOWFISH {
                match password.map(|p| if legacy_key { LineCipher::new_legacy(p) } else { LineCipher::new(p) }) {
                    Some(Ok(c)) => { mode = Mode::Blowfish(Box::new(c)); continue; }
                    Some(Err(e)) => { r.log.push_str(&format!("unable to open file.\nException: {e}\nfailed")); return Err(Box::new(r)); }
                    None => { r.log.push_str("unable to open file.\nException: NullPointerException (no password)\nfailed"); return Err(Box::new(r)); }
                }
            }
        }
        let decoded;
        let line: &str = match &mode {
            Mode::Plain => raw_line,
            Mode::Xor(p) => { decoded = xor_line(raw_line, p); &decoded }
            Mode::Blowfish(c) => match c.decrypt_line(raw_line) {
                Ok(d) => { decoded = d; &decoded }
                Err(e) => { r.log.push_str(&format!("unable to open file.\nException: {e}\nfailed")); return Err(Box::new(r)); }
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
        let res = match first {
            ":" => parse_options(&mut adjustments, &toks[1..]).map_err(in_field("opts")),
            "=" => { if let Some(t) = toks.get(1) { r.notes = decode_description(t); } Ok(()) }
            "v" => { if let Some(t) = toks.get(1) { r.version = decode_description(t); } Ok(()) }
            _ => {
                let p = parse_payment(first, &toks[1..], next_id, today, &mut r.log, line_no);
                // the original increments the id right after the amount parses
                if !matches!(&p, Err(e) if e.field == "amount" || e.field == "loan amount") { next_id += 1; }
                p.map(|p| r.payments.push(p))
            }
        };
        if let Err(FieldError { field, message }) = res {
            r.log.push_str(&format!("({line_no}): Invalid data for '{field}'\nException: {message}\n"));
            errors += 1;
        }
    }
    r.adjustments = Some(adjustments);
    if errors != 0 {
        r.log.push_str("failed");
        return Err(Box::new(r));
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

pub fn load_file(path: &Path, today: i32, password: Option<&str>) -> Result<Loaded, Box<Loaded>> {
    match std::fs::read(path) {
        Ok(b) => load_str(&latin1_decode(&b), today, password),
        Err(e) => Err(Box::new(Loaded { log: format!("unable to open '{}' file.\nException: {e}\nfailed", path.display()), ..Default::default() })),
    }
}

/// `optsString(p)`.
pub fn opts_string(p: &Payment) -> String {
    let mut s = String::new();
    if let Some(c) = &p.category { s.push_str(", $"); s.push_str(&encode_description(c)); }
    for o in &p.opt {
        if o.deleted { s.push_str(&format!(", {:08}", o.date)); }
        else { s.push_str(&format!(", {:08}:{}", o.date, java_double(o.amount))); }
    }
    for f in &p.flag { s.push_str(&format!(", {:08}{}", f.date, f.value)); }
    for n in &p.note { s.push_str(&format!(", {:08}={}", n.date, encode_description(&n.note))); }
    for h in &p.hilighting { s.push_str(&format!(", {:08}&{:x}", h.date, (h.bg_color as u32) & 0xFF_FFFF)); }
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

/// Plain wording for the common Windows failures (5 = access denied, 32 = sharing violation).
fn friendly_io_error(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(5) | Some(32) => "The file is read-only or open in another program.".to_string(),
        _ => e.to_string(),
    }
}

/// Skip byte-identical saves; otherwise stage a complete file and retain the prior `.bak[.N]`.
pub fn save_file(path: &Path, content: &str) -> Result<(), String> {
    use std::fs::{self, OpenOptions};
    use std::io::{ErrorKind, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    // Append to the native path rather than its display text, which can be lossy.
    let sibling = |suffix: &str| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        std::path::PathBuf::from(name)
    };
    let bytes = latin1_encode(content);
    let original = match fs::read(path) {
        Ok(old) if old == bytes => return Ok(()),
        Ok(old) => Some(old),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(format!("File Save: cannot read original: {error}")),
    };

    let (temp_path, mut temp_file) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = sibling(&format!(".tmp.{}.{id}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists || fs::symlink_metadata(&candidate).is_ok() => continue,
            Err(error) => return Err(format!("File Save: cannot create temporary file: {error}")),
        }
    };

    let mut backup_path: Option<std::path::PathBuf> = None;
    let result = (|| -> Result<(), String> {
        temp_file.write_all(&bytes).map_err(|error| format!("File Save: {error}"))?;
        temp_file.flush().map_err(|error| format!("File Save: {error}"))?;
        temp_file.sync_all().map_err(|error| format!("File Save: {error}"))?;
        drop(temp_file);

        if let Some(old) = original {
            // Continue after the newest backup so index order is age order even after pruning.
            let mut index = existing_backups(path).last().map_or(0, |(newest, _)| newest + 1);
            let mut backup = loop {
                let suffix = if index == 0 { ".bak".to_string() } else { format!(".bak.{index}") };
                let candidate = sibling(&suffix);
                match OpenOptions::new().write(true).create_new(true).open(&candidate) {
                    Ok(file) => { backup_path = Some(candidate.clone()); break file }
                    // Windows may return AccessDenied for an existing directory.
                    Err(error) if error.kind() == ErrorKind::AlreadyExists || fs::symlink_metadata(&candidate).is_ok() => {
                        index = index.checked_add(1).ok_or("File Save: too many backup files")?;
                    }
                    Err(error) => return Err(format!("File Save: cannot create backup: {error}")),
                }
            };
            backup.write_all(&old).map_err(|error| format!("File Save: cannot write backup: {error}"))?;
            backup.flush().map_err(|error| format!("File Save: cannot flush backup: {error}"))?;
            backup.sync_all().map_err(|error| format!("File Save: cannot sync backup: {error}"))?;
        }

        // Same-directory rename replaces the destination atomically on supported filesystems.
        fs::rename(&temp_path, path).map_err(|error| format!("File Save: cannot replace original: {}", friendly_io_error(&error)))
    })();
    if result.is_err() {
        // Only this invocation's create_new temporary file and its own backup belong to us; the
        // original is untouched, so a backup of it would only pile up on every retry.
        let _ = fs::remove_file(&temp_path);
        if let Some(b) = &backup_path { let _ = fs::remove_file(b); }
    } else if backup_path.is_some() {
        prune_backups(path);
    }
    result
}

/// Backups kept beside a budget; older ones are removed after a successful save.
pub const MAX_BACKUPS: usize = 10;

/// `(index, path)` of every `.bak` (index 0) and `.bak.N` entry beside `path`, oldest first.
/// Entries of any kind count so a stray directory still reserves its index.
fn existing_backups(path: &Path) -> Vec<(u64, std::path::PathBuf)> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return Vec::new() };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let prefix = format!("{name}.bak");
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(u64, std::path::PathBuf)> = entries.flatten().filter_map(|entry| {
        let file_name = entry.file_name();
        let rest = file_name.to_str()?.strip_prefix(&prefix)?;
        let index = if rest.is_empty() { 0 } else {
            let digits = rest.strip_prefix('.')?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) { return None }
            digits.parse().ok()?
        };
        Some((index, entry.path()))
    }).collect();
    found.sort();
    found
}

/// Delete all but the newest `MAX_BACKUPS` backup files. Failures are ignored: the save
/// itself already succeeded, and anything that is not a regular file is left alone.
fn prune_backups(path: &Path) {
    let files: Vec<_> = existing_backups(path).into_iter()
        .filter(|(_, p)| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
        .collect();
    for (_, old) in &files[..files.len().saturating_sub(MAX_BACKUPS)] {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn non_bmp_text_roundtrips_and_java_surrogates_decode() {
        let s = "Pay \u{1F600} x, \u{e9}";
        let enc = super::encode_description(s);
        assert!(enc.contains("&#xd83d;&#xde00;"));
        assert_eq!(super::decode_description(&enc), s);
        assert_eq!(super::decode_description("&#xd83d;&#xde00;"), "\u{1F600}");
        assert_eq!(super::decode_description("a&#xd83d;b"), "a?b");
    }

    #[test]
    fn short_password_legacy_zero_padded_file_still_opens() {
        let text = "v,2\n:\n=,\n## END-OF-DATA; line_no=5 ##\n";
        let enc: Vec<String> = text.lines().map(|l| super::LineCipher::new_legacy("ab").unwrap().encrypt_line(l)).collect();
        let file = format!("{}\n{}\n", super::HEADER_BLOWFISH, enc.join("\n"));
        assert!(super::is_decryptable(&file, "ab"));
    }

    use super::*;
    #[test]
    fn corrupt_frequency_is_clamped_on_load() {
        for (f, n) in [(0, 5), (-1, 5), (i32::MAX, i32::MAX)] {
            let mut p = Payment::new(20240131);
            p.frequency = f;
            p.freq_type = 'm';
            p.n_pays = n;
            let adj = Payment::new(20240131);
            let text = save_str(&[p], &adj, "", None).unwrap();
            let l = load_str(&text, 20240131, None).unwrap_or_else(|l| panic!("{}", l.log));
            assert!((1..=100_000).contains(&l.payments[0].frequency));
            assert!((1..=MAX_N_PAYS).contains(&l.payments[0].n_pays));
        }
    }

    #[test]
    fn negative_n_pays_becomes_one_and_is_logged() {
        let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, -5, 20260101, 0,  \n## END-OF-DATA; line_no=5 ##";
        let l = load_str(text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
        assert_eq!(l.payments[0].n_pays, 1);
        assert_ne!(l.payments[0].end_mode(), END_MODE_INDEFINITE);
        assert!(l.log.contains("n_pays -5"), "{}", l.log);
    }

    #[test]
    fn non_finite_amounts_are_rejected() {
        for amt in ["NaN", "Infinity", "-Infinity"] {
            let text = format!("v,2\n:\n=,\n{amt}, A, 20260101, 1m, 5, 20261201, 48,  \n## END-OF-DATA; line_no=5 ##");
            assert!(load_str(&text, 20260101, None).is_err(), "{amt}");
        }
        let text = "v,2\n:\n=,\n-10.0%Infinity, A, 20260101, 1m, 5, 20261201, 64,  \n## END-OF-DATA; line_no=5 ##";
        assert!(load_str(text, 20260101, None).is_err());
        let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  , 20260301:NaN\n## END-OF-DATA; line_no=5 ##";
        assert!(load_str(text, 20260101, None).is_err());
    }

    #[test]
    fn option_date_before_year_1000_roundtrips() {
        let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  , 00010101:5\n## END-OF-DATA; line_no=5 ##";
        let l = load_str(text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log));
        let adj = Payment::new(20260101);
        let saved = save_str(&l.payments, &adj, "", None).unwrap();
        assert!(saved.contains("00010101:5.0"), "{saved}");
        assert!(load_str(&saved, 20260101, None).is_ok());
    }

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
