//! Java-compatible number text: `Double.toString` (JDK 19+ shortest-repr rules) and
//! `Double.parseDouble` tolerance, so saved `.bgt` files match the original byte for byte.

/// `Double.toString(d)`: `-80.0`, `1500.0`, `0.1`, `1.0E7`, `1.2345E-4`.
pub fn java_double(d: f64) -> String {
    if d.is_nan() { return "NaN".into(); }
    if d.is_infinite() { return if d > 0.0 { "Infinity".into() } else { "-Infinity".into() }; }
    if d == 0.0 { return if d.is_sign_negative() { "-0.0".into() } else { "0.0".into() }; }
    let s = format!("{:e}", d); // shortest round-trip digits, e.g. "-8e1", "1.5e3"
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let sign = if mant.starts_with('-') { "-" } else { "" };
    let digits: String = mant.trim_start_matches('-').chars().filter(|c| *c != '.').collect();
    let abs = d.abs();
    if (1e-3..1e7).contains(&abs) {
        let point = exp + 1;
        let (int_part, frac) = if point <= 0 {
            ("0".to_string(), "0".repeat((-point) as usize) + &digits)
        } else if point as usize >= digits.len() {
            (digits.clone() + &"0".repeat(point as usize - digits.len()), "0".to_string())
        } else {
            (digits[..point as usize].to_string(), digits[point as usize..].to_string())
        };
        format!("{sign}{int_part}.{frac}")
    } else {
        let frac = if digits.len() > 1 { &digits[1..] } else { "0" };
        format!("{sign}{}.{frac}E{exp}", &digits[..1])
    }
}

/// `Double.parseDouble` for the forms the saver emits (plus a trailing `d`/`f`, which Java accepts).
pub fn parse_double(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    t.parse::<f64>().map_err(|e| format!("NumberFormatException: \"{s}\" ({e})"))
}

pub fn parse_int(s: &str) -> Result<i32, String> {
    s.trim().parse::<i32>().map_err(|e| format!("NumberFormatException: \"{s}\" ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_java() {
        assert_eq!(java_double(-80.0), "-80.0");
        assert_eq!(java_double(1500.0), "1500.0");
        assert_eq!(java_double(4280.0), "4280.0");
        assert_eq!(java_double(0.1), "0.1");
        assert_eq!(java_double(-100.0), "-100.0");
        assert_eq!(java_double(123.45), "123.45");
        assert_eq!(java_double(1e7), "1.0E7");
        assert_eq!(java_double(12345678.9), "1.23456789E7");
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(0.001), "0.001");
        assert_eq!(parse_double("1.0E7").unwrap(), 1e7);
    }
}
