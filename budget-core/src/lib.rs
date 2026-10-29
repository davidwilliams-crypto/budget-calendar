//! Budget Calendar engine (port of `ca.mishell.app.budget.BudgetDB` and friends).
//! See ../../SPEC.md and ../../analysis/01-core-data-model-and-engine.md.

pub mod bgt;
pub mod calc;
pub mod commands;
pub mod sharing;
pub mod config;
pub mod crypto;
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
