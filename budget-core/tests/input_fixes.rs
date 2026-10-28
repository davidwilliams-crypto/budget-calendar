//! Regression tests for typed input: calculator formulas and the number reader.

use budget_core::calc;

#[test]
fn a_decimal_comma_is_a_syntax_error_not_a_thousands_separator() {
    // "12,50" would read as 1250; the comma is not a well-formed thousands group, so it is an error.
    assert_eq!(calc::calculate("12,50+3").unwrap_err(), "Syntax error near '12,50'");
    assert_eq!(calc::calculate("12,50").unwrap_err(), "Syntax error near '12,50'");
    // well-formed thousands groups still count
    assert_eq!(calc::calculate("1,000+1").unwrap(), 1001.0);
    assert_eq!(calc::calculate("1,234,567+1").unwrap(), 1234568.0);
    assert_eq!(calc::calculate("$1,200.50 - 200").unwrap(), 1000.5);
}
