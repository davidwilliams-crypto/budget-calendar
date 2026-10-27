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

/// `isNumber`: `%` dropped, optional `+`, lenient parse, single multiplier suffix allowed.
fn is_number(s: &str) -> Option<f64> {
    let s: String = s.chars().filter(|c| *c != '%').collect();
    let body = if let Some(r) = s.strip_prefix('+') { if r.is_empty() { return None; } r } else { &s };
    let (v, used) = parse_prefix(body)?;
    let rest = &body[used..];
    let mult = match rest {
        "" => 1.0,
        "k" | "K" => 1e3,
        "m" | "M" => 1e6,
        "b" | "B" => 1e9,
        "t" | "T" => 1e12,
        _ => return None,
    };
    Some(v * mult)
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

fn process(tokens: &[String], pos: &mut usize, values: &mut Vec<f64>, week: f64, month: f64) -> Result<(), String> {
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
                if next != "(" && is_number(&next).is_none() { return Err(syntax_error(&next)); }
                tok = next;
            }
            if tok == "(" {
                process(tokens, pos, values, week, month)?;
                if sign < 0.0 { values.push(-1.0); apply(values, "*")?; }
            } else if let Some(rest) = tok.strip_prefix("%%") {
                let (n, _) = parse_prefix(rest).ok_or_else(|| format!("Unparseable number: \"{rest}\""))?;
                values.push(n * month / 100.0 * sign);
            } else if let Some(rest) = tok.strip_prefix('%') {
                let (n, _) = parse_prefix(rest).ok_or_else(|| format!("Unparseable number: \"{rest}\""))?;
                values.push(n * week / 100.0 * sign);
            } else {
                let v = is_number(&tok).ok_or_else(|| syntax_error(&tok))?;
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
            while ops.last().map_or(false, |t| weight(t) >= w) {
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
    process(&tokens, &mut pos, &mut values, week_income, month_income)?;
    if values.len() != 1 || pos != tokens.len() { return Err("Unexpected end of expression".into()); }
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
}
