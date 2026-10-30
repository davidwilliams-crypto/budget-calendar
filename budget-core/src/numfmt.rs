//! Locale-fixed (English) stand-ins for the Java `DecimalFormat` behaviour the app relies on.

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Budget.sdate(int)`: `dd-MMM-yyyy`, e.g. `05-Oct-2026`.
pub fn sdate_dmy(v: i32) -> String {
    let m = ((v / 100) % 100).clamp(1, 12) as usize;
    format!("{:02}-{}-{:04}", v % 100, MONTHS[m - 1], v / 10000)
}

/// `1234567` -> `1,234,567` (the integer digits of a formatted magnitude).
pub(crate) fn group_digits(int_part: &str) -> String {
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i).is_multiple_of(3) { grouped.push(','); }
        grouped.push(c);
    }
    grouped
}

/// Java `DecimalFormat` renders non-finite values as `NaN` / the infinity symbol instead of failing.
fn non_finite(x: f64) -> Option<String> {
    if x.is_nan() { Some("NaN".to_string()) }
    else if x.is_infinite() { Some(if x < 0.0 { "-\u{221E}" } else { "\u{221E}" }.to_string()) }
    else { None }
}

/// Balance as the calendar and report show it: exact with decimals, else the nearest whole dollar.
/// The original nudged by `0.05f` and truncated, which showed 5,613.50 as 5,613 and even 5,613.94 as
/// 5,613: a balance read up to 95 cents low, against amounts that were rounded. A half goes away from
/// zero, as in [`format_frac0`].
pub fn balance_display(v: f64, exact: bool) -> f64 { if exact { v } else { v.round() } }

/// Rounding dust: what is left when cent amounts are added in one order and taken away in another
/// (a sum of 0.38 + 9.56 - 9.94 is 1.8e-15, not 0). Dust grows with the size of the amounts involved
/// (`scale` is the sum of their magnitudes) and stays far below half a cent at the 1e12 limit.
pub fn noise_floor(scale: f64) -> f64 { (8.0 * f64::EPSILON * scale).clamp(1e-9, 0.004) }

/// `x`, or 0.0 when it is only rounding dust against the amounts (`scale`) it was computed from.
pub fn drop_dust(x: f64, scale: f64) -> f64 { if x.abs() < noise_floor(scale) { 0.0 } else { x } }

/// The nearest whole cent when `x` is within a millionth of it, else `x` unchanged: three amounts of
/// 49.95 add up to 149.85000000000002, and 1234.56 - 234.56 - 1000 to -5.7e-14, which then fail the
/// exact comparisons (`=-149.85`, "balance unchanged", "is negative") that cents are meant to satisfy.
/// Never returns negative zero.
pub fn snap_to_cent(x: f64) -> f64 {
    let r = (x * 100.0).round() / 100.0;
    // A millionth, or a few ulps of `x` where one ulp is itself larger (balances in the billions).
    let tolerance = (4.0 * f64::EPSILON * x.abs()).clamp(1e-6, 0.004);
    if (x - r).abs() < tolerance { r + 0.0 } else { x }
}

/// `numFormatFrac2.format(x)`: grouping on, exactly two decimals (`-1,234.50`).
/// A value that rounds to zero prints without a sign (`0.00`).
pub fn format_frac2(x: f64) -> String {
    if let Some(s) = non_finite(x) { return s; }
    let s = format!("{:.2}", x.abs());
    let (int_part, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let neg = x < 0.0 && (int_part != "0" || frac != "00");
    format!("{}{}.{frac}", if neg { "-" } else { "" }, group_digits(int_part))
}

/// Grouping on, no decimals, a half going away from zero (`-1,235`). A value that rounds to zero
/// prints as `0`. The original's `numFormatFrac0` rounded a half to even, so -62.50 showed as -62
/// beside -145.50 as -146.
pub fn format_frac0(x: f64) -> String {
    if let Some(s) = non_finite(x) { return s; }
    let s = format!("{:.0}", x.abs().round());
    let neg = x < 0.0 && s != "0";
    format!("{}{}", if neg { "-" } else { "" }, group_digits(&s))
}

/// `DecimalFormat.parse(s, pos)` prefix-lenient parse: longest leading number (optional `-`,
/// digits with well-formed `,` grouping, optional fraction, optional exponent). Returns the value and the number
/// of chars consumed, or `None` when no digit was found at the start.
pub fn parse_prefix(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'-' { i += 1; }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() { i += 1; }
    let mut saw_digit = i > start;
    // Thousands separators only in well-formed groups (a first group of 1-3 digits, then groups of
    // exactly three). Any other comma ends the number, so "12,50" is not read as 1250.
    if (1..=3).contains(&(i - start)) {
        while i < b.len() && b[i] == b',' && is_thousands_group(&b[i + 1..]) { i += 4; }
    }
    if i < b.len() && b[i] == b'.' {
        let j = i + 1;
        let mut k = j;
        while k < b.len() && b[k].is_ascii_digit() { k += 1; }
        if k > j || saw_digit { saw_digit = saw_digit || k > j; i = k; }
    }
    if !saw_digit { return None; }
    // exponent
    if i < b.len() && (b[i] == b'E' || b[i] == b'e') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'-' || b[k] == b'+') { k += 1; }
        let ds = k;
        while k < b.len() && b[k].is_ascii_digit() { k += 1; }
        if k > ds { i = k; }
    }
    let text: String = s[..i].chars().filter(|c| *c != ',').collect();
    text.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v, i))
}

/// Whether `rest` starts with exactly three digits that do not run on into a longer group.
fn is_thousands_group(rest: &[u8]) -> bool {
    rest.len() >= 3 && rest[..3].iter().all(u8::is_ascii_digit) && rest.get(3).is_none_or(|c| !c.is_ascii_digit())
}

/// `Budget.toDouble`: strip a leading `+`, drop every `$`, lenient parse, `0.0` on failure.
pub fn to_double(s: &str) -> f64 {
    let s = s.strip_prefix('+').unwrap_or(s).replace('$', "");
    parse_prefix(&s).map_or(0.0, |(v, _)| v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn non_finite_does_not_panic() {
        assert_eq!(super::format_frac2(f64::NAN), "NaN");
        assert_eq!(super::format_frac2(f64::INFINITY), "\u{221E}");
        assert_eq!(super::format_frac2(f64::NEG_INFINITY), "-\u{221E}");
    }
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
        assert_eq!(format_frac0(1234.99), "1,235");
        assert_eq!(format_frac0(0.4), "0");
        assert_eq!(format_frac0(1.5), "2");
        assert_eq!(format_frac0(-0.4), "0");
        assert_eq!(format_frac0(-1234567.6), "-1,234,568");
        assert_eq!(format_frac2(-0.004), "0.00");
        assert_eq!(format_frac2(f64::INFINITY), "\u{221E}");
        assert_eq!(format_frac2(f64::NEG_INFINITY), "-\u{221E}");
        assert_eq!(format_frac2(f64::NAN), "NaN");
        assert_eq!(format_frac0(f64::NAN), "NaN");
        assert_eq!(parse_prefix("1E3x"), Some((1000.0, 3)));
        assert_eq!(parse_prefix(".5"), Some((0.5, 2)));
    }

    #[test]
    fn whole_dollars_never_drop_a_half_dollar() {
        // 5,613.50 used to show as 5,613: balances were cut, not rounded.
        assert_eq!(format_frac0(balance_display(5613.5, false)), "5,614");
        assert_eq!(format_frac0(balance_display(5613.94, false)), "5,614");
        assert_eq!(format_frac0(balance_display(5613.49, false)), "5,613");
        assert_eq!(format_frac0(balance_display(-5613.5, false)), "-5,614");
        assert_eq!(format_frac0(balance_display(-0.4, false)), "0");
        assert_eq!(balance_display(5613.5, true), 5613.5);
        // Amounts round the same way, so -62.50 and -145.50 do not go opposite ways.
        assert_eq!(format_frac0(-62.5), "-63");
        assert_eq!(format_frac0(-145.5), "-146");
        assert_eq!(format_frac0(0.5), "1");
    }

    #[test]
    fn a_comma_is_only_a_well_formed_thousands_separator() {
        assert_eq!(parse_prefix("1,000+1"), Some((1000.0, 5)));
        assert_eq!(parse_prefix("-1,234"), Some((-1234.0, 6)));
        assert_eq!(parse_prefix("12,345,678.5x"), Some((12345678.5, 12)));
        // any other comma ends the number: a decimal comma or a group that is not three digits
        assert_eq!(parse_prefix("12,50"), Some((12.0, 2)));
        assert_eq!(parse_prefix("1,2345"), Some((1.0, 1)));
        assert_eq!(parse_prefix("1,000,00"), Some((1000.0, 5)));
        assert_eq!(parse_prefix("1234,567"), Some((1234.0, 4)));
        assert_eq!(parse_prefix(",5"), None);
        assert_eq!(to_double("12,50"), 12.0);
    }
}
