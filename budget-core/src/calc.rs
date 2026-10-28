//! Port of `ca.mishell.misc.Calculator`: `+ - * / ( )`, `$` ignored, `k/m/b/t` suffixes,
//! `%n` = n % of week income, `%%n` = n % of month income.

use crate::numfmt::parse_prefix;

const ETX: &str = "\u{2386}";

fn tokenize(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t: String = cur.trim().chars().filter(|c| *c != '$').collect();
        if !t.is_empty() { out.push(t); }
        cur.clear();
    };
    for c in expr.chars() {
        if " \t+-/*()".contains(c) {
            flush(&mut cur, &mut out);
            if c != ' ' && c != '\t' { out.push(c.to_string()); }
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out.push(ETX.to_string());
    out
}

/// The multiplier a suffix adds to a number: none (`""`), or one of `k`, `m`, `b`, `t` in either case.
fn multiplier(suffix: &str) -> Option<f64> {
    match suffix {
        "" => Some(1.0),
        "k" | "K" => Some(1e3),
        "m" | "M" => Some(1e6),
        "b" | "B" => Some(1e9),
        "t" | "T" => Some(1e12),
        _ => None,
    }
}

/// `isNumber`: `%` dropped, optional `+`, lenient parse, single multiplier suffix allowed.
fn is_number(s: &str) -> Option<f64> {
    let s: String = s.chars().filter(|c| *c != '%').collect();
    let body = if let Some(r) = s.strip_prefix('+') { if r.is_empty() { return None; } r } else { &s };
    let (v, used) = parse_prefix(body)?;
    let mult = multiplier(&body[used..])?;
    // A product beyond a double is "Number too large" (see `bad_number`), not a finite number.
    Some(v * mult).filter(|x| x.is_finite())
}

/// A well-formed decimal number, with its multiplier, whose value is beyond a double (`1e999`,
/// `1e308k`). Read here rather than through `parse_prefix`, which only refuses it as non-finite.
fn overflows(tok: &str) -> bool {
    let t: String = tok.chars().filter(|c| !matches!(c, '%' | ',')).collect();
    let last = t.char_indices().next_back().map_or(0, |(i, _)| i);
    let (body, mult) = match multiplier(&t[last..]) {
        Some(m) if last > 0 => (&t[..last], m),
        _ => (t.as_str(), 1.0),
    };
    let literal = body.starts_with(|c: char| c.is_ascii_digit() || c == '.') && body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E'));
    literal && body.parse::<f64>().is_ok_and(|v| (v * mult).is_infinite())
}

/// The error for a token that is not a number: "Number too large" for one beyond a double.
fn bad_number(tok: &str) -> String {
    if overflows(tok) { "Number too large".into() } else { syntax_error(tok) }
}

/// The number after `%` or `%%`: the whole of it must be a number, so `%5k` and `%5abc` are errors.
fn percent_operand(tok: &str, rest: &str) -> Result<f64, String> {
    match parse_prefix(rest) {
        Some((n, used)) if used == rest.len() => Ok(n),
        Some(_) => Err(syntax_error(tok)),
        None if overflows(rest) => Err("Number too large".into()),
        None => Err(format!("Unparseable number: \"{rest}\"")),
    }
}

fn weight(tok: &str) -> i32 {
    match tok.chars().next() { Some('+') | Some('-') => 1, Some('*') | Some('/') => 2, _ => -1 }
}

fn syntax_error(tok: &str) -> String {
    if tok == ETX { "Unexpected end of expression".into() } else { format!("Syntax error near '{tok}'") }
}

fn apply(values: &mut Vec<f64>, op: &str) -> Result<(), String> {
    if values.len() < 2 { return Err("Not enough operands in the stack".into()); }
    let b = values.pop().unwrap();
    let a = values.pop().unwrap();
    values.push(match op.chars().next().unwrap() {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        '/' => { if b == 0.0 { return Err("Division by zero!".into()); } a / b }
        _ => a,
    });
    Ok(())
}

/// Deliberate deviation: the original recurses until StackOverflowError; in Rust a stack overflow
/// aborts the process (losing unsaved work), so nesting is bounded.
const MAX_PAREN_DEPTH: usize = 64;

fn process(tokens: &[String], pos: &mut usize, values: &mut Vec<f64>, week: f64, month: f64, depth: usize) -> Result<(), String> {
    let mut ops: Vec<String> = Vec::new();
    let mut expect_operand = true;
    loop {
        let Some(tok) = tokens.get(*pos).cloned() else { return Err("Unexpected end of expression".into()) };
        *pos += 1;
        if expect_operand {
            let mut sign = 1.0;
            let mut tok = tok;
            if tok == "-" || tok == "+" {
                sign = if tok == "-" { -1.0 } else { 1.0 };
                let Some(next) = tokens.get(*pos).cloned() else { return Err("Unexpected end of expression".into()) };
                *pos += 1;
                if next != "(" && is_number(&next).is_none() { return Err(bad_number(&next)); }
                tok = next;
            }
            if tok == "(" {
                if depth >= MAX_PAREN_DEPTH { return Err("Expression too deeply nested".into()); }
                process(tokens, pos, values, week, month, depth + 1)?;
                if sign < 0.0 { values.push(-1.0); apply(values, "*")?; }
            } else if let Some(rest) = tok.strip_prefix("%%") {
                let n = percent_operand(&tok, rest)?;
                values.push(n * month / 100.0 * sign);
            } else if let Some(rest) = tok.strip_prefix('%') {
                let n = percent_operand(&tok, rest)?;
                values.push(n * week / 100.0 * sign);
            } else {
                let v = is_number(&tok).ok_or_else(|| bad_number(&tok))?;
                values.push(v * sign);
            }
            expect_operand = false;
        } else {
            if tok == ")" || tok == ETX {
                while let Some(op) = ops.pop() { apply(values, &op)?; }
                return Ok(());
            }
            let w = weight(&tok);
            if w == -1 { return Err(syntax_error(&tok)); }
            while ops.last().is_some_and(|t| weight(t) >= w) {
                let op = ops.pop().unwrap();
                apply(values, &op)?;
            }
            ops.push(tok);
            expect_operand = true;
        }
    }
}

/// `Calculator.calculate(expr, weekIncome, monthIncome)`.
pub fn calculate_with_income(expr: &str, week_income: f64, month_income: f64) -> Result<f64, String> {
    if expr.trim().is_empty() { return Err("Empty expression".into()); }
    let tokens = tokenize(expr);
    let mut pos = 0;
    let mut values = Vec::new();
    process(&tokens, &mut pos, &mut values, week_income, month_income, 0)?;
    if values.len() != 1 || pos != tokens.len() { return Err("Unexpected end of expression".into()); }
    // Overflow (e.g. "1e308*10") must not reach a balance as infinity or NaN.
    if !values[0].is_finite() { return Err("Result is too large".into()); }
    Ok(values[0])
}

pub fn calculate(expr: &str) -> Result<f64, String> { calculate_with_income(expr, 0.0, 0.0) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worked_examples() {
        let c = |s: &str| calculate_with_income(s, 1000.0, 4300.0);
        assert_eq!(c("100+20*3").unwrap(), 160.0);
        assert_eq!(c("(100+20)*3").unwrap(), 360.0);
        assert_eq!(c("-(5+5)*2").unwrap(), -20.0);
        assert_eq!(c("$1,200.50 - 200").unwrap(), 1000.5);
        assert_eq!(c("2.5k/2").unwrap(), 1250.0);
        assert_eq!(c("%10").unwrap(), 100.0);
        assert_eq!(c("%%25 + 100").unwrap(), 1175.0);
        assert_eq!(c("-%10").unwrap(), -100.0);
        assert_eq!(c("10%").unwrap(), 10.0);
        assert_eq!(c("1234 - 23/2").unwrap(), 1222.5);
        assert_eq!(c("5 5").unwrap_err(), "Syntax error near '5'");
        assert_eq!(c("5+").unwrap_err(), "Unexpected end of expression");
        assert_eq!(c("10/0").unwrap_err(), "Division by zero!");
        assert_eq!(c("  ").unwrap_err(), "Empty expression");
        assert_eq!(c("--5").unwrap_err(), "Syntax error near '-'");
        assert_eq!(c("(1+2").unwrap_err(), "Unexpected end of expression");
        assert_eq!(c("1+2)").unwrap_err(), "Unexpected end of expression");
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        // 1 MiB: the Windows main-thread stack, where the unbounded recursion aborted.
        std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
            let nest = |n: usize| "(".repeat(n) + "1" + &")".repeat(n);
            assert_eq!(calculate(&nest(10_000)).unwrap_err(), "Expression too deeply nested");
            assert_eq!(calculate(&nest(64)).unwrap(), 1.0);
            assert_eq!(calculate(&nest(65)).unwrap_err(), "Expression too deeply nested");
            assert_eq!(calculate(&format!("-{}", nest(32))).unwrap(), -1.0);
        }).unwrap().join().unwrap();
    }

    #[test]
    fn percentages_take_no_trailing_text() {
        // "%5k" and "%5abc" used to read as 5% and silently drop the rest.
        let c = |s: &str| calculate_with_income(s, 1000.0, 4300.0);
        assert_eq!(c("%5k").unwrap_err(), "Syntax error near '%5k'");
        assert_eq!(c("%5abc").unwrap_err(), "Syntax error near '%5abc'");
        assert_eq!(c("%%5x + 1").unwrap_err(), "Syntax error near '%%5x'");
        assert_eq!(c("100 + %5abc").unwrap_err(), "Syntax error near '%5abc'");
        assert_eq!(c("%5.5").unwrap(), 55.0);
        assert_eq!(c("%1,000").unwrap(), 10000.0);
    }

    #[test]
    fn numbers_too_large_are_reported_as_such() {
        // "1e999" used to report "Syntax error near '1e999'", hiding that the number is too large.
        assert_eq!(calculate("1e999").unwrap_err(), "Number too large");
        assert_eq!(calculate("-1e999").unwrap_err(), "Number too large");
        assert_eq!(calculate("1+1e999").unwrap_err(), "Number too large");
        assert_eq!(calculate("1e308k").unwrap_err(), "Number too large");
        assert_eq!(calculate("1e308*10").unwrap_err(), "Result is too large");
    }

    #[test]
    fn overflowing_results_are_errors_not_infinity() {
        assert_eq!(calculate("1e308*10").unwrap_err(), "Result is too large");
        assert_eq!(calculate("-1e308*10").unwrap_err(), "Result is too large");
        assert_eq!(calculate("1t*1t*1t").unwrap(), 1e36); // finite: range limits belong to callers
    }
}
