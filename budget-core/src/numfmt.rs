//! Locale-fixed (English) stand-ins for the Java `DecimalFormat` behaviour the app relies on.

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Budget.sdate(int)`: `dd-MMM-yyyy`, e.g. `05-Oct-2026`.
pub fn sdate_dmy(v: i32) -> String {
    let m = ((v / 100) % 100).clamp(1, 12) as usize;
    format!("{:02}-{}-{:04}", v % 100, MONTHS[m - 1], v / 10000)
}

/// `numFormatFrac2.format(x)`: grouping on, exactly two decimals (`-1,234.50`).
pub fn format_frac2(x: f64) -> String {
    let s = format!("{:.2}", x.abs());
    let (int_part, frac) = s.split_once('.').unwrap();
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 { grouped.push(','); }
        grouped.push(c);
    }
    let neg = x < 0.0 || (x == 0.0 && x.is_sign_negative() && false);
    format!("{}{grouped}.{frac}", if neg { "-" } else { "" })
}

/// `DecimalFormat.parse(s, pos)` prefix-lenient parse: longest leading number (optional `-`,
/// digits with `,` grouping, optional fraction, optional exponent). Returns the value and the number
/// of chars consumed, or `None` when no digit was found at the start.
pub fn parse_prefix(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'-' { i += 1; }
    let start_digits = i;
    let mut saw_digit = false;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b',') {
        if b[i].is_ascii_digit() { saw_digit = true; }
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        let j = i + 1;
        let mut k = j;
        while k < b.len() && b[k].is_ascii_digit() { k += 1; }
        if k > j || saw_digit { saw_digit = saw_digit || k > j; i = k; }
    }
    if !saw_digit { return None; }
    let _ = start_digits;
    // exponent
    if i < b.len() && (b[i] == b'E' || b[i] == b'e') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'-' || b[k] == b'+') { k += 1; }
        let ds = k;
        while k < b.len() && b[k].is_ascii_digit() { k += 1; }
        if k > ds { i = k; }
    }
    let text: String = s[..i].chars().filter(|c| *c != ',').collect();
    text.parse::<f64>().ok().map(|v| (v, i))
}

/// `Budget.toDouble`: strip a leading `+`, drop every `$`, lenient parse, `0.0` on failure.
pub fn to_double(s: &str) -> f64 {
    let s = s.strip_prefix('+').unwrap_or(s).replace('$', "");
    parse_prefix(&s).map_or(0.0, |(v, _)| v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        assert_eq!(sdate_dmy(20261005), "05-Oct-2026");
        assert_eq!(format_frac2(-1234.5), "-1,234.50");
        assert_eq!(format_frac2(100.0), "100.00");
        assert_eq!(format_frac2(1234567.891), "1,234,567.89");
        assert_eq!(to_double("2026-10"), 2026.0);
        assert_eq!(to_double("1,500abc"), 1500.0);
        assert_eq!(to_double("abc"), 0.0);
        assert_eq!(to_double("+$-100"), -100.0);
        assert_eq!(parse_prefix("1E3x"), Some((1000.0, 3)));
        assert_eq!(parse_prefix(".5"), Some((0.5, 2)));
    }
}
