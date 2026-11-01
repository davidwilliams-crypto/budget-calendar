//! Budget Calendar engine (port of `ca.mishell.app.budget.BudgetDB` and friends).
//! See ../../SPEC.md and ../../analysis/01-core-data-model-and-engine.md.

pub mod bgt;
pub mod calc;
pub mod commands;
pub mod sharing;
pub mod config;
pub mod crypto;
pub mod passcode;
pub mod sealed;
pub mod date;
pub mod db;
pub mod filter;
pub mod javafmt;
pub mod model;
pub mod numfmt;
pub mod report;
mod schedule;
pub mod stats;

pub use date::{Cal, Field};
pub use db::{BudgetDay, BudgetDb, Transaction};
pub use schedule::{add_period, calendar_field, in_calendar, weekend_shift, Slots};
pub use model::*;

/// `&#x` + the lowercase hex digits of `value` (zero-padded to `min_digits`) + `;`, appended to
/// `out` without allocating. Matches `format!("&#x{value:0min_digits$x};")` for the entity ranges
/// the savers and report writers use.
pub(crate) fn push_hex_entity(out: &mut String, value: u32, min_digits: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push_str("&#x");
    let mut digits = [0u8; 8]; // a code point needs at most 6
    let mut n = 0;
    let mut v = value;
    while v > 0 {
        digits[n] = HEX[(v & 0xF) as usize];
        n += 1;
        v >>= 4;
    }
    for _ in n..min_digits {
        out.push('0');
    }
    for i in (0..n).rev() {
        out.push(digits[i] as char);
    }
    out.push(';');
}
