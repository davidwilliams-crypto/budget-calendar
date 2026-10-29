use budget_core::sharing::*;
use budget_core::{BudgetDb, Cal, Payment, END_MODE_INDEFINITE, FLAG_PAID};
use serde_json::{json, Value};
fn fixture() -> Snapshot {
    let mut db = BudgetDb::new_with_today(&Cal::from_int(20261001), 12, false, 20261029);
    let mut p = Payment::new(20260131);
    p.description = "Rent".into();
    p.amount = -100.;
    p.set_end_mode(END_MODE_INDEFINITE);
    p.add_custom_amount(20260930, -125.);
    p.add_custom_amount(20260930, -140.);
    p.set_url(Some("https://example.invalid"));
    p.set_default_highlighting(1234);
    p.set_flag(20261031, FLAG_PAID);
    db.add(p);
    db.adjustments.set_custom_amount(20260801, 2000.);
    db.notes = "Household".into();
    snapshot_from_db(&db, "fixture", "Test household", "America/Halifax", None).unwrap()
}
fn ask(v: Value) -> Value {
    serde_json::from_str(&request_json(&v.to_string())).unwrap()
}
#[test]
fn snapshot_roundtrip_retains_all_persisted_fields_and_stable_ids() {
    let snap = fixture();
    let db = snapshot_to_db(&snap, 20261029, 20261001, 12).unwrap();
    let again = snapshot_from_db(&db, &snap.id, &snap.name, &snap.time_zone, Some(&snap)).unwrap();
    assert_eq!(snap, again);
    let p = &snap.payments[0].payment;
    assert_eq!(p.opt.len(), 2);
    assert_eq!(p.get_url().as_deref(), Some("https://example.invalid"));
    assert_eq!(p.default_hilighting(), 1234);
}
#[test]
fn forecast_preserves_historical_checkpoint_and_month_end_paid_status() {
    let snap = fixture();
    let v = ask(
        json!({"action":"forecast","snapshot":snap,"today":20261029,"from":20261030,"to":20261130}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let days = v["result"]["days"].as_array().unwrap();
    assert_eq!(days[0]["openingBalance"], 1760.);
    assert_eq!(days[1]["transactions"][0]["flag"], ";");
    assert_eq!(days.last().unwrap()["closingBalance"], 1560.);
}
#[test]
fn invalid_commands_fail_atomically_and_identity_survives_edits_and_delete() {
    let snap = fixture();
    let id = &snap.payments[0].id;
    let before = serde_json::to_value(&snap).unwrap();
    for command in [
        json!({"type":"transaction","id":id,"date":20260230,"amount":-1,"note":"","flag":";","scope":"one"}),
        json!({"type":"move","id":id,"from":20261031,"to":20261101,"amount":1e99}),
    ] {
        assert_eq!(
            ask(
                json!({"action":"apply","snapshot":snap,"today":20261029,"operationId":"test-op","command":command})
            )["ok"],
            false
        );
        assert_eq!(serde_json::to_value(&snap).unwrap(), before);
    }
    let v = ask(
        json!({"action":"apply","snapshot":snap,"today":20261029,"operationId":"test-op","command":{"type":"delete","id":id}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["tombstones"][0], *id);
}
#[test]
fn malformed_requests_and_duplicate_ids_are_rejected() {
    assert_eq!(
        ask(json!({"action":"forecast","today":20260230}))["ok"],
        false
    );
    let mut snap = fixture();
    snap.payments.push(snap.payments[0].clone());
    assert_eq!(
        ask(json!({"action":"validate","snapshot":snap}))["ok"],
        false
    );
}
#[test]
fn independent_offline_additions_use_operation_identity_not_a_shared_counter() {
    let snap = fixture();
    let payment =
        json!({"amount":-25,"description":"Coffee","start_date":20261029,"end_date":20261029});
    let create = |op: &str| {
        ask(
            json!({"action":"apply","snapshot":snap,"today":20261029,"operationId":op,"command":{"type":"put_payment","payment":payment}}),
        )
    };
    let a = create("device-a-op-1");
    let b = create("device-b-op-1");
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(b["ok"], true, "{b}");
    assert_ne!(
        a["result"]["payments"][1]["id"],
        b["result"]["payments"][1]["id"]
    );
    assert_eq!(
        a,
        create("device-a-op-1"),
        "replaying a pure operation produces identical results"
    );
}
#[test]
fn deleting_highest_native_record_then_adding_never_reuses_its_identity() {
    let snap = fixture();
    let mut db = snapshot_to_db(&snap, 20261029, 20261029, 12).unwrap();
    let old = db.payments[0].id;
    db.delete_payment(0);
    let mut p = Payment::new(20261029);
    p.description = "New".into();
    p.amount = -1.;
    let i = db.add(p);
    assert!(db.payments[i].id > old);
    let out = snapshot_from_db_with_operation(
        &db,
        &snap.id,
        &snap.name,
        &snap.time_zone,
        Some(&snap),
        "delete-readd",
    )
    .unwrap();
    assert_ne!(out.payments[0].id, snap.payments[0].id);
    assert_eq!(out.tombstones, vec![snap.payments[0].id.clone()]);
}
#[test]
fn future_split_is_atomic_and_preserves_paid_history() {
    let mut snap = fixture();
    snap.payments[0].payment.opt.clear();
    let v = ask(
        json!({"action":"apply","snapshot":snap,"today":20261029,"operationId":"future-op","command":{"type":"transaction","id":snap.payments[0].id,"date":20261031,"amount":-150,"note":"Revised","flag":";","scope":"future"}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let forecast = ask(
        json!({"action":"forecast","snapshot":v["result"],"today":20261102,"from":20260930,"to":20261130}),
    );
    assert_eq!(forecast["ok"], true, "{forecast}");
    let days = forecast["result"]["days"].as_array().unwrap();
    assert_eq!(days[0]["transactions"][0]["amount"], -100.);
    assert_eq!(days[31]["transactions"][0]["amount"], -150.);
    assert_eq!(days[31]["transactions"][0]["flag"], ";");
    assert_eq!(days.last().unwrap()["transactions"][0]["amount"], -150.);
}
#[test]
fn tentative_paid_occurrence_is_pinned_with_explicit_clock() {
    let mut snap = fixture();
    let p = &mut snap.payments[0].payment;
    p.start_date = 20261001;
    p.end_date = 20261001;
    p.opt.clear();
    p.set_tentative(true);
    let v = ask(
        json!({"action":"apply","snapshot":snap,"today":20261029,"operationId":"paid-op","command":{"type":"transaction","id":snap.payments[0].id,"date":20261029,"amount":-100,"note":"Paid","flag":";","scope":"one"}}),
    );
    assert_eq!(v["ok"], true, "{v}");
    let forecast = ask(
        json!({"action":"forecast","snapshot":v["result"],"today":20261030,"from":20261029,"to":20261030}),
    );
    assert_eq!(forecast["ok"], true, "{forecast}");
    assert_eq!(
        forecast["result"]["days"][0]["transactions"][0]["flag"],
        ";"
    );
    assert_eq!(
        forecast["result"]["days"][1]["transactions"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
