use budget_core::sharing::*;
use budget_core::{BudgetDb, Cal, Field, Payment, END_MODE_INDEFINITE, FLAG_PAID};
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

const TODAY: i32 = 20261029;
fn empty() -> Snapshot {
    let v = ask(json!({"action":"new","id":"fixture","name":"Test household","timeZone":"America/Halifax","today":TODAY}));
    serde_json::from_value(v["result"].clone()).unwrap()
}
fn put(s: &Snapshot, command: Value, op: &str) -> Result<Snapshot, String> {
    apply_command(s, TODAY, &command, op)
}
fn entry(freq_type: &str, start: i32) -> Value {
    json!({"description":"Fixture entry","amount":-10,"start_date":start,"end_date":start,"flags":16,"frequency":1,"freq_type":freq_type})
}
fn forecast_of(s: &Snapshot, from: i32, to: i32) -> Value {
    let v = ask(json!({"action":"forecast","snapshot":s,"today":TODAY,"from":from,"to":to}));
    assert_eq!(v["ok"], true, "{v}");
    v["result"].clone()
}
fn dates(s: &Snapshot, from: i32, to: i32) -> Vec<i64> {
    forecast_of(s, from, to)["days"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| !d["transactions"].as_array().unwrap().is_empty())
        .map(|d| d["date"].as_i64().unwrap())
        .collect()
}
/// The desktop editor's `last_same_weekday_of_month`, kept here as the behaviour to agree with.
fn desktop_snap(date: i32) -> i32 {
    let mut c = Cal::from_int(date);
    loop {
        let mut next = c;
        next.add(Field::Day, 7);
        if next.month() != c.month() {
            return c.get();
        }
        c = next;
    }
}
#[test]
fn new_last_weekday_payment_starts_on_the_last_matching_weekday() {
    let s = put(&empty(), json!({"type":"put_payment","payment":entry("L",20261001)}), "op-1").unwrap();
    assert_eq!(s.payments[0].payment.start_date, 20261029);
    assert_eq!(dates(&s, 20261001, 20261231), [20261029, 20261126, 20261231]);
}
#[test]
fn last_weekday_snap_agrees_with_the_desktop_editor_on_every_day_of_a_leap_year() {
    let mut day = Cal::from_int(20240101);
    while day.get() <= 20241231 {
        let s = put(&empty(), json!({"type":"put_payment","payment":entry("L",day.get())}), "op-1").unwrap();
        assert_eq!(s.payments[0].payment.start_date, desktop_snap(day.get()), "{}", day.get());
        day.add(Field::Day, 1);
    }
}
#[test]
fn saved_last_weekday_start_is_left_alone_until_it_changes() {
    // A legacy record whose start date sits outside the last week of its month.
    let mut db = BudgetDb::new_with_today(&Cal::from_int(20261001), 12, false, TODAY);
    let mut p = Payment::new(20261006);
    p.description = "Legacy".into();
    p.freq_type = 'L';
    p.set_end_mode(END_MODE_INDEFINITE);
    db.add(p);
    let s = snapshot_from_db(&db, "fixture", "Test household", "America/Halifax", None).unwrap();
    let id = s.payments[0].id.clone();
    let mut payment = serde_json::to_value(&s.payments[0].payment).unwrap();
    payment["description"] = json!("Renamed");
    let unchanged = put(&s, json!({"type":"put_payment","id":id,"payment":payment}), "op-1").unwrap();
    assert_eq!(unchanged.payments[0].payment.start_date, 20261006);
    payment["start_date"] = json!(20261013);
    let moved = put(&s, json!({"type":"put_payment","id":id,"payment":payment}), "op-2").unwrap();
    assert_eq!(moved.payments[0].payment.start_date, 20261027);
}
#[test]
fn changing_a_series_to_last_weekday_snaps_its_unchanged_start() {
    let s = put(&empty(), json!({"type":"put_payment","payment":entry("m",20261001)}), "op-1").unwrap();
    let id = s.payments[0].id.clone();
    let mut payment = serde_json::to_value(&s.payments[0].payment).unwrap();
    payment["freq_type"] = json!("L");
    let s = put(&s, json!({"type":"put_payment","id":id,"payment":payment}), "op-2").unwrap();
    assert_eq!(s.payments[0].payment.start_date, 20261029);
}
#[test]
fn last_weekday_snap_applies_to_a_this_and_future_edit() {
    let s = put(&empty(), json!({"type":"put_payment","payment":entry("m",20260105)}), "op-1").unwrap();
    let id = s.payments[0].id.clone();
    let mut payment = serde_json::to_value(&s.payments[0].payment).unwrap();
    payment["freq_type"] = json!("L");
    payment["start_date"] = json!(20261105);
    let s = put(&s, json!({"type":"put_payment","id":id,"payment":payment,"from":20261105}), "op-2").unwrap();
    assert_eq!(s.payments[1].payment.start_date, 20261126);
    assert_eq!(dates(&s, 20261001, 20261231), [20261005, 20261126, 20261231]);
}
#[test]
fn single_payment_keeps_its_date_even_with_the_last_weekday_type() {
    let mut payment = entry("L", 20261001);
    payment["flags"] = json!(48);
    payment["n_pays"] = json!(1);
    let s = put(&empty(), json!({"type":"put_payment","payment":payment}), "op-1").unwrap();
    assert_eq!(s.payments[0].payment.start_date, 20261001);
    assert_eq!(dates(&s, 20261001, 20261231), [20261001]);
}
#[test]
fn nth_weekday_start_already_names_its_week_and_stays_put() {
    // The second Tuesday, as on desktop, which has no snap for this type.
    let s = put(&empty(), json!({"type":"put_payment","payment":entry("W",20261013)}), "op-1").unwrap();
    assert_eq!(s.payments[0].payment.start_date, 20261013);
    assert_eq!(dates(&s, 20261001, 20270131), [20261013, 20261110, 20261208, 20270112]);
}
#[test]
fn last_weekday_snap_never_repairs_a_bad_date_or_passes_the_end_date() {
    let bad = put(&empty(), json!({"type":"put_payment","payment":entry("L",20260230)}), "op-1");
    assert!(bad.is_err());
    let mut payment = entry("L", 20261001);
    payment["flags"] = json!(32);
    payment["end_date"] = json!(20261015);
    let ended = put(&empty(), json!({"type":"put_payment","payment":payment}), "op-1");
    assert_eq!(ended.unwrap_err(), "Payment ends before it starts.");
}
#[test]
fn forecast_totals_leave_out_disabled_payments_and_balance_changes() {
    let mut s = empty();
    for (i, (amount, disabled)) in [(500.0, false), (-100.1, false), (-0.2, false), (-70.0, true), (900.0, true)].iter().enumerate() {
        let mut payment = entry("m", 20261105);
        payment["description"] = json!(format!("Entry {i}"));
        payment["amount"] = json!(amount);
        payment["disabled"] = json!(disabled);
        s = put(&s, json!({"type":"put_payment","payment":payment}), &format!("op-{i}")).unwrap();
    }
    s = put(&s, json!({"type":"balance","date":20261110,"amount":12345}), "checkpoint").unwrap();
    let month = forecast_of(&s, 20261101, 20261130);
    assert_eq!(month["totals"], json!({"moneyIn":500.0,"moneyOut":100.3}));
    assert!(month["lowPoint"].is_object() && month["days"].is_array());
    assert_eq!(forecast_of(&s, 20261105, 20261105)["totals"], json!({"moneyIn":500.0,"moneyOut":100.3}));
    assert_eq!(forecast_of(&s, 20261106, 20261204)["totals"], json!({"moneyIn":0.0,"moneyOut":0.0}));
    assert_eq!(forecast_of(&s, 20261101, 20261231)["totals"], json!({"moneyIn":1000.0,"moneyOut":200.6}));
}
#[test]
fn passcode_actions_wrap_and_unwrap_the_household_key_through_the_engine() {
    let key = "BC1-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 32 zero bytes, url-safe
    let wrap = ask(json!({"action":"passcode_wrap","recoveryKey":key,"passcode":"copper river lantern",
        "budgetId":"0f9e5e52-1111-4111-8111-111111111111",
        "entropy":{"salt":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"nonce":[0,0,0,0,0,0,0,0,0,0,0,0]}}));
    assert_eq!(wrap["ok"], json!(true), "wrap must succeed: {wrap}");
    let slot = wrap["result"]["slot"].clone();
    assert_eq!(slot["version"], json!(1));
    assert_eq!(slot["memoryKiB"], json!(65536));
    let unwrap_ok = ask(json!({"action":"passcode_unwrap","slot":slot,"passcode":"copper river lantern",
        "budgetId":"0f9e5e52-1111-4111-8111-111111111111"}));
    assert_eq!(unwrap_ok["result"]["recoveryKey"], json!(key));
    let wrong = ask(json!({"action":"passcode_unwrap","slot":slot,"passcode":"copper river latern",
        "budgetId":"0f9e5e52-1111-4111-8111-111111111111"}));
    assert_eq!(wrong["ok"], json!(false));
    assert!(wrong["error"].as_str().unwrap().contains("incorrect passcode"));
    // The slot is bound to its household.
    let other = ask(json!({"action":"passcode_unwrap","slot":slot,"passcode":"copper river lantern",
        "budgetId":"0f9e5e52-1111-4111-8111-111111111112"}));
    assert_eq!(other["ok"], json!(false));
}
#[test]
fn passcode_wrap_enforces_policy_and_fresh_full_length_entropy() {
    for passcode in ["short", "12345678"] {
        let weak = ask(json!({"action":"passcode_wrap","recoveryKey":"BC1-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "passcode":passcode,"budgetId":"0f9e5e52",
            "entropy":{"salt":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"nonce":[0,0,0,0,0,0,0,0,0,0,0,0]}}));
        assert_eq!(weak["ok"], json!(false), "{passcode} must be refused");
    }
    let short_salt = ask(json!({"action":"passcode_wrap","recoveryKey":"BC1-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "passcode":"copper river lantern","budgetId":"0f9e5e52",
        "entropy":{"salt":[0,0,0,0,0,0,0,0],"nonce":[0,0,0,0,0,0,0,0,0,0,0,0]}}));
    assert_eq!(short_salt["ok"], json!(false), "entropy must be full length");
    let bad_key = ask(json!({"action":"passcode_wrap","recoveryKey":"not-a-key",
        "passcode":"copper river lantern","budgetId":"0f9e5e52",
        "entropy":{"salt":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"nonce":[0,0,0,0,0,0,0,0,0,0,0,0]}}));
    assert_eq!(bad_key["ok"], json!(false));
}
