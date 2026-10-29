//! Versioned, clock-independent sharing boundary. No files, network or system clock.
//! Native password exports use OS randomness; raw WASM exports require fresh host entropy.
use crate::{bgt, commands, date::is_calendar_date, BudgetDb, Cal, Field, Payment};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub time_zone: String,
    pub next_id: i32,
    pub payments: Vec<SharedPayment>,
    pub adjustments: Payment,
    pub notes: String,
    pub format_version: String,
    #[serde(default)]
    pub tombstones: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedPayment {
    pub id: String,
    pub payment: Payment,
}

fn check_date(d: i32) -> Result<(), String> {
    if is_calendar_date(d) {
        Ok(())
    } else {
        Err("Use a valid calendar date between years 1000 and 9999.".into())
    }
}
fn identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
}
fn meta(id: &str, name: &str, tz: &str) -> Result<(), String> {
    if !identity(id) {
        return Err("Invalid budget identity.".into());
    }
    if name.trim().is_empty() || name.len() > 256 {
        return Err("Budget name must contain 1 to 256 bytes.".into());
    }
    if tz.is_empty()
        || tz.len() > 128
        || !tz
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return Err("Invalid household time zone.".into());
    }
    Ok(())
}
fn bounded_text(s: &str) -> Result<(), String> {
    if s.len() > 65536 {
        Err("Note is too long.".into())
    } else {
        Ok(())
    }
}

pub fn validate_snapshot(s: &Snapshot) -> Result<(), String> {
    if s.schema_version != SCHEMA_VERSION {
        return Err("Unsupported snapshot schema version.".into());
    }
    meta(&s.id, &s.name, &s.time_zone)?;
    bounded_text(&s.notes)?;
    if s.format_version.len() > 64 || s.payments.len() > 5000 || s.tombstones.len() > 20000 {
        return Err("Snapshot exceeds supported bounds.".into());
    }
    let mut stable = BTreeSet::new();
    let mut numeric = BTreeSet::new();
    let mut max_id = 0;
    for p in &s.payments {
        if !identity(&p.id)
            || !stable.insert(p.id.as_str())
            || p.payment.id <= 0
            || !numeric.insert(p.payment.id)
        {
            return Err("Payment identities must be unique.".into());
        }
        max_id = max_id.max(p.payment.id);
        commands::validate_payment(&p.payment)?;
        if !commands::amount_in_range(p.payment.amount)
            || !commands::amount_in_range(p.payment.loan_amount)
            || p.payment
                .opt
                .iter()
                .any(|o| !commands::amount_in_range(o.amount))
        {
            return Err("Shared amounts exceed +/-1,000,000,000,000.".into());
        }
        if p.payment.opt.len()
            + p.payment.flag.len()
            + p.payment.note.len()
            + p.payment.hilighting.len()
            > 20000
        {
            return Err("Payment metadata exceeds supported bounds.".into());
        }
    }
    if s.next_id <= max_id || s.next_id <= 0 || s.next_id >= i32::MAX - 1 {
        return Err("Invalid next payment identity.".into());
    }
    for id in &s.tombstones {
        if !identity(id) || !stable.insert(id.as_str()) {
            return Err("Invalid or reused deletion identity.".into());
        }
    }
    // Adjustments are a container; their default empty description/end mode are intentional.
    if !s.adjustments.amount.is_finite() || !s.adjustments.loan_amount.is_finite() {
        return Err("Non-finite balance metadata.".into());
    }
    for o in &s.adjustments.opt {
        check_date(o.date)?;
        if !commands::amount_in_range(o.amount) {
            return Err("Balance checkpoint exceeds supported amount bounds.".into());
        }
    }
    for n in &s.adjustments.note {
        if n.date != crate::URL_NOTE_DATE {
            check_date(n.date)?
        }
        bounded_text(&n.note)?;
    }
    for f in &s.adjustments.flag {
        check_date(f.date)?;
        if !commands::valid_status(f.value) {
            return Err("Invalid adjustment status.".into());
        }
    }
    for h in &s.adjustments.hilighting {
        if h.date != crate::DEFAULT_COLOR_DATE {
            check_date(h.date)?
        }
    }
    if s.adjustments.opt.len()
        + s.adjustments.note.len()
        + s.adjustments.flag.len()
        + s.adjustments.hilighting.len()
        > 20000
    {
        return Err("Balance metadata exceeds supported bounds.".into());
    }
    // Match editor naming constraints without renaming existing records.
    for p in &s.payments {
        if p.payment.category.as_deref() == Some(&p.payment.description)
            || p.payment.category.as_deref() == Some(crate::stats::UNACCOUNTED)
            || s.payments
                .iter()
                .any(|other| other.payment.category.as_deref() == Some(&p.payment.description))
        {
            return Err("Payment descriptions and categories conflict.".into());
        }
    }
    Ok(())
}

/// Capture initial import or an edit which adds no records. Use the operation variant for additions/splits.
pub fn snapshot_from_db(
    db: &BudgetDb,
    id: &str,
    name: &str,
    time_zone: &str,
    previous: Option<&Snapshot>,
) -> Result<Snapshot, String> {
    capture(db, id, name, time_zone, previous, None)
}
pub fn snapshot_from_db_with_operation(
    db: &BudgetDb,
    id: &str,
    name: &str,
    time_zone: &str,
    previous: Option<&Snapshot>,
    operation_id: &str,
) -> Result<Snapshot, String> {
    if !identity(operation_id) {
        return Err("A unique operation identity is required.".into());
    }
    capture(db, id, name, time_zone, previous, Some(operation_id))
}
fn capture(
    db: &BudgetDb,
    id: &str,
    name: &str,
    time_zone: &str,
    previous: Option<&Snapshot>,
    operation: Option<&str>,
) -> Result<Snapshot, String> {
    meta(id, name, time_zone)?;
    if previous.is_some_and(|p| p.id != id) {
        return Err("Previous snapshot belongs to another budget.".into());
    }
    let mut tombstones = previous.map(|p| p.tombstones.clone()).unwrap_or_default();
    let mut payments = Vec::new();
    let mut created = 0;
    for p in &db.payments {
        let existing = previous.and_then(|old| old.payments.iter().find(|r| r.payment.id == p.id));
        let stable = if let Some(r) = existing {
            r.id.clone()
        } else if previous.is_none() {
            format!("{id}:import:{}", p.id)
        } else {
            let op =
                operation.ok_or("New or split payments require a unique operation identity.")?;
            created += 1;
            format!("{id}:{op}:{created}")
        };
        let mut payment = p.clone();
        payment.calendar_days.clear();
        payments.push(SharedPayment {
            id: stable,
            payment,
        });
    }
    if let Some(old) = previous {
        for p in &old.payments {
            if !payments.iter().any(|r| r.id == p.id) {
                tombstones.push(p.id.clone())
            }
        }
    }
    let mut adjustments = db.adjustments.clone();
    adjustments.calendar_days.clear();
    let snapshot = Snapshot {
        schema_version: SCHEMA_VERSION,
        id: id.into(),
        name: name.into(),
        time_zone: time_zone.into(),
        next_id: db
            .next_payment_id
            .max(db.payments.iter().map(|p| p.id).max().unwrap_or(0) + 1)
            .max(previous.map_or(1, |s| s.next_id)),
        payments,
        adjustments,
        notes: db.notes.clone(),
        format_version: db.version.clone(),
        tombstones,
    };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

/// Rebuild using original historical checkpoints, never a synthetic zero at the forecast start.
pub fn snapshot_to_db(
    snapshot: &Snapshot,
    today: i32,
    view_date: i32,
    months: i32,
) -> Result<BudgetDb, String> {
    validate_snapshot(snapshot)?;
    check_date(today)?;
    check_date(view_date)?;
    if !(0..=120).contains(&months) {
        return Err("Forecast horizon exceeds ten years.".into());
    }
    let mut db = BudgetDb::new_with_today(&Cal::from_int(view_date), 0, false, today);
    db.payments = snapshot
        .payments
        .iter()
        .map(|p| p.payment.clone())
        .collect();
    db.adjustments = snapshot.adjustments.clone();
    db.notes = snapshot.notes.clone();
    db.version = snapshot.format_version.clone();
    db.next_payment_id = snapshot.next_id;
    let (start, end) = db.window(&Cal::from_int(view_date), months, false);
    let days = Cal::diff(&start, &end).max(0) as usize + 1;
    if days > 366000 || days.saturating_mul(db.payments.len().max(1)) > 8000000 {
        return Err("Budget history exceeds the shared engine calculation bound.".into());
    }
    db.rebuild_db(&Cal::from_int(view_date), months, false);
    Ok(db)
}

fn required<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    v.get(key).ok_or_else(|| format!("Missing {key}."))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    required(v, key)?
        .as_str()
        .ok_or_else(|| format!("Invalid {key}."))
}
fn integer(v: &Value, key: &str) -> Result<i32, String> {
    required(v, key)?
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| format!("Invalid {key}."))
}
fn optional_date(v: &Value, key: &str) -> Result<Option<i32>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            let d = integer(v, key)?;
            check_date(d)?;
            Ok(Some(d))
        }
    }
}
fn amount(v: &Value, key: &str) -> Result<f64, String> {
    let n = required(v, key)?
        .as_f64()
        .ok_or_else(|| format!("Invalid {key}."))?;
    if commands::amount_in_range(n) {
        Ok(n)
    } else {
        Err("Amount exceeds +/-1,000,000,000,000.".into())
    }
}
fn snapshot(v: &Value) -> Result<Snapshot, String> {
    serde_json::from_value(required(v, "snapshot")?.clone())
        .map_err(|e| format!("Invalid snapshot: {e}"))
}
fn flag(v: &Value) -> Result<char, String> {
    let s = string(v, "flag")?;
    let mut chars = s.chars();
    let f = chars.next().ok_or("Missing status.")?;
    if chars.next().is_some() || !commands::valid_status(f) {
        Err("Invalid status.".into())
    } else {
        Ok(f)
    }
}
fn record(s: &Snapshot, c: &Value) -> Result<i32, String> {
    let id = string(c, "id")?;
    s.payments
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.payment.id)
        .ok_or_else(|| "This payment no longer exists.".into())
}

fn forecast(s: &Snapshot, today: i32, from: i32, to: i32) -> Result<Value, String> {
    check_date(from)?;
    check_date(to)?;
    if to < from || Cal::diff_int(from, to) > 3660 {
        return Err("Forecast must cover at most ten years in date order.".into());
    }
    let a = Cal::from_int(from);
    let b = Cal::from_int(to);
    let months = (b.year() - a.year()) * 12 + b.month() - a.month();
    let mut db = snapshot_to_db(s, today, from, months)?;
    let mut date = a;
    let mut days = Vec::new();
    let mut low = (from, f64::INFINITY);
    loop {
        let d = db
            .get_day(&date)
            .ok_or("Date is outside the calculated calendar.")?
            .clone();
        if d.closing_balance < low.1 {
            low = (date.get(), d.closing_balance)
        }
        let transactions:Vec<Value>=d.transactions.iter().map(|t|{
            let p=&db.payments[t.pmt];let id=&s.payments.iter().find(|r|r.payment.id==p.id).unwrap().id;
            json!({"id":id,"internalId":p.id,"description":p.description,"category":p.category,"amount":t.amount,"balance":t.balance,"flag":p.get_flag(date.get()).to_string(),"note":p.get_note(date.get()).unwrap_or_default(),"suffix":t.suffix.to_string(),"disabled":p.disabled})
        }).collect();
        days.push(json!({"date":date.get(),"openingBalance":d.opening_balance,"closingBalance":d.closing_balance,"adjusted":d.closing_balance_adjusted,"note":db.day_note(&date),"transactions":transactions}));
        if date.get() == to {
            break;
        }
        date.add(Field::Day, 1);
    }
    Ok(json!({"days":days,"lowPoint":{"date":low.0,"balance":low.1}}))
}

pub fn apply_command(
    s: &Snapshot,
    today: i32,
    command: &Value,
    operation_id: &str,
) -> Result<Snapshot, String> {
    if !identity(operation_id) {
        return Err("A unique operation identity is required.".into());
    }
    let c = command;
    let kind = string(c, "type")?;
    let view = optional_date(c, "date")?
        .or(optional_date(c, "from")?)
        .unwrap_or(today);
    let mut db = snapshot_to_db(s, today, view, 12)?;
    let mut name = s.name.clone();
    match kind {
        "put_payment" => {
            let p: Payment = serde_json::from_value(required(c, "payment")?.clone())
                .map_err(|e| format!("Invalid payment: {e}"))?;
            let id = match c.get("id") {
                None | Some(Value::Null) => None,
                Some(_) => Some(record(s, c)?),
            };
            commands::apply_payment(&mut db, p, id, optional_date(c, "from")?)?;
        }
        "transaction" => {
            let id = record(s, c)?;
            let date = integer(c, "date")?;
            check_date(date)?;
            let n = amount(c, "amount")?;
            let note = string(c, "note")?;
            bounded_text(note)?;
            let f = flag(c)?;
            match string(c, "scope")? {
                "one" => commands::change_transaction(&mut db, id, date, n, note, f)?,
                "future" => commands::change_future_transactions(&mut db, id, date, n, note, f)?,
                _ => return Err("Invalid occurrence scope.".into()),
            }
        }
        "move" => {
            commands::move_transaction(
                &mut db,
                record(s, c)?,
                integer(c, "from")?,
                integer(c, "to")?,
                amount(c, "amount")?,
            )?;
        }
        "delete" => commands::delete(&mut db, record(s, c)?, optional_date(c, "date")?)?,
        "restore" => commands::restore(&mut db, record(s, c)?, integer(c, "date")?)?,
        "balance" => {
            let n = if required(c, "amount")?.is_null() {
                None
            } else {
                Some(amount(c, "amount")?)
            };
            commands::set_balance(&mut db, integer(c, "date")?, n)?;
        }
        "day_note" => {
            let d = integer(c, "date")?;
            check_date(d)?;
            let note = if required(c, "note")?.is_null() {
                None
            } else {
                Some(string(c, "note")?)
            };
            if let Some(n) = note {
                bounded_text(n)?
            }
            db.set_day_note(&Cal::from_int(d), note);
        }
        "notes" => {
            bounded_text(string(c, "notes")?)?;
            db.notes = string(c, "notes")?.into();
        }
        "rename" => {
            name = string(c, "name")?.into();
        }
        _ => return Err("Unsupported command.".into()),
    }
    snapshot_from_db_with_operation(&db, &s.id, &name, &s.time_zone, Some(s), operation_id)
}

fn dispatch(v: Value) -> Result<Value, String> {
    match string(&v, "action")? {
        "new" => {
            let today = integer(&v, "today")?;
            check_date(today)?;
            let db = BudgetDb::new_with_today(&Cal::from_int(today), 0, false, today);
            Ok(serde_json::to_value(snapshot_from_db(
                &db,
                string(&v, "id")?,
                string(&v, "name")?,
                string(&v, "timeZone")?,
                None,
            )?)
            .unwrap())
        }
        "validate" => {
            validate_snapshot(&snapshot(&v)?)?;
            Ok(json!({"valid":true}))
        }
        "import" => {
            let today = integer(&v, "today")?;
            check_date(today)?;
            let text = string(&v, "text")?;
            let password = v.get("password").and_then(Value::as_str);
            let loaded = bgt::load_str(text, today, password).map_err(|e| e.log)?;
            if loaded.log != format!("done.\n{} entries loaded.\n", loaded.payments.len()) {
                return Err(format!(
                    "Import needs repair before sharing: {}",
                    loaded.log
                ));
            }
            // Validate before building any schedule; unsupported migration data is reported.
            for p in &loaded.payments {
                commands::validate_payment(p)?
            }
            let mut db = BudgetDb::new_with_today(&Cal::from_int(today), 0, false, today);
            db.payments = loaded.payments;
            db.adjustments = loaded.adjustments.unwrap_or_else(|| Payment::new(today));
            db.notes = loaded.notes;
            db.version = loaded.version;
            Ok(serde_json::to_value(snapshot_from_db(
                &db,
                string(&v, "id")?,
                string(&v, "name")?,
                string(&v, "timeZone")?,
                None,
            )?)
            .unwrap())
        }
        "export" => {
            let s = snapshot(&v)?;
            validate_snapshot(&s)?;
            check_date(integer(&v, "today")?)?;
            let payments: Vec<_> = s.payments.iter().map(|r| r.payment.clone()).collect();
            let password = v.get("password").and_then(Value::as_str);
            #[cfg(not(target_arch = "wasm32"))]
            let text = bgt::save_str(&payments, &s.adjustments, &s.notes, password)?;
            #[cfg(target_arch = "wasm32")]
            let text = match password {
                None => bgt::save_str(&payments, &s.adjustments, &s.notes, None)?,
                Some(password) => {
                    let entropy = v.get("entropy").ok_or("Password export requires fresh CSPRNG entropy from the browser host.")?;
                    let entropy = serde_json::from_value::<crate::sealed::HostEntropy>(entropy.clone())
                        .map_err(|_| "Password export requires a 16-byte salt and 12-byte nonce from the browser CSPRNG.")?;
                    bgt::save_str_with_entropy(&payments, &s.adjustments, &s.notes, password, entropy)?
                }
            };
            Ok(json!({"text":text}))
        }
        "forecast" => forecast(
            &snapshot(&v)?,
            integer(&v, "today")?,
            integer(&v, "from")?,
            integer(&v, "to")?,
        ),
        "apply" => Ok(serde_json::to_value(apply_command(
            &snapshot(&v)?,
            integer(&v, "today")?,
            required(&v, "command")?,
            string(&v, "operationId")?,
        )?)
        .unwrap()),
        _ => Err("Unsupported engine action.".into()),
    }
}
/// All errors are data; password export uses native OS entropy or fresh browser host entropy.
pub fn request_json(request: &str) -> String {
    let result = if request.len() > MAX_REQUEST_BYTES {
        Err("Request exceeds 8 MiB.".into())
    } else {
        serde_json::from_str(request)
            .map_err(|e| format!("Invalid JSON: {e}"))
            .and_then(dispatch)
    };
    match result {
        Ok(value) => json!({"ok":true,"result":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}
