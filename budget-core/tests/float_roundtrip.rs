//! Amounts must survive JSON exactly. Shared budgets compare decrypted snapshots with the edits that
//! produced them, and the phone engine receives every snapshot as JSON text; a parser that reads a
//! value back one step off makes an untouched payment look edited and can stall a sync queue.
use budget_core::sharing::*;
use budget_core::{BudgetDb, Cal, Payment, END_MODE_INDEFINITE};
use serde_json::json;

/// Unrounded amounts, such as balances computed from older .bgt files, that the default
/// serde_json parser does not read back bit for bit.
const AWKWARD: [f64; 3] = [-107354.81379999999, 0.30000000000000004, 1234.5678901234567];

#[test]
fn money_like_values_read_back_exactly_from_json() {
    let mut off = Vec::new();
    for i in 0..200_000u32 {
        let value = -(i as f64) * 0.537_913_579_1 - 0.000_000_013;
        let read: f64 = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        if read.to_bits() != value.to_bits() { off.push((value, read)); }
    }
    for value in AWKWARD {
        let read: f64 = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        if read.to_bits() != value.to_bits() { off.push((value, read)); }
    }
    assert!(off.is_empty(), "{} values changed on a JSON round trip, e.g. {:?}", off.len(), &off[..off.len().min(3)]);
}

#[test]
fn the_phone_engine_keeps_unrounded_amounts_exact() {
    let today = 20261029;
    let mut db = BudgetDb::new_with_today(&Cal::from_int(20261001), 12, false, today);
    let mut p = Payment::new(20260131);
    p.description = "Mortgage".into();
    p.amount = AWKWARD[0];
    p.set_end_mode(END_MODE_INDEFINITE);
    p.add_custom_amount(20260930, AWKWARD[1]);
    db.add(p);
    db.adjustments.set_custom_amount(20260801, AWKWARD[2]);
    let snap = snapshot_from_db(&db, "fixture", "Test household", "America/Halifax", None).unwrap();
    let command = json!({"type":"notes","notes":"Household"});

    let native = apply_command(&snap, today, &command, "test-op").unwrap();
    let request = json!({"action":"apply","snapshot":snap,"today":today,"operationId":"test-op","command":command});
    let answer: serde_json::Value = serde_json::from_str(&request_json(&request.to_string())).unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    let engine: Snapshot = serde_json::from_value(answer["result"].clone()).unwrap();
    assert_eq!(engine, native, "the engine's JSON path changed an amount");
}
