# Shared Rust engine contract (schema 1)

Both native clients and raw WebAssembly call `budget_core::sharing::request_json`.
Every request is JSON; every response is `{ "ok": true, "result": ... }` or
`{ "ok": false, "error": "message" }`. Dates are YYYYMMDD integers. All operations
that calculate or edit require explicit household-local `today` (no runtime clock).

## Snapshot

`{schemaVersion:1,id,name,timeZone,nextId,payments:[{id,payment}],adjustments,notes,formatVersion,tombstones:[]}`.
`id` and each record `id` are durable strings supplied/generated within the budget;
`payment.id` is the retained positive numeric native ID. `nextId` is a durable
monotonic counter used for numeric native IDs. New shared IDs derive from a caller-supplied unique operation ID, so offline clients never allocate the same shared identity. `tombstones` retain deleted record IDs.
Native callers preserve identity using `snapshot_from_db(db,id,name,time_zone,previous)`; additions/splits with a previous snapshot use `snapshot_from_db_with_operation(db,id,name,time_zone,previous,operation_id)`. Each editing operation must have a unique client-generated UUID operationId.
All persisted Payment fields use Rust snake_case: `id,amount,description,start_date,
frequency,freq_type,n_pays,end_date,image_path,flags,opt,flag,note,hilighting,disabled,
category,loan_amount,flag_value,grace_period_days`. Transient calendar indexes are
omitted. Options: `{date,amount,deleted}`, flags: `{date,value}`, notes:
`{date,note}`, highlights: `{date,bg_color}`. Omitted Payment fields get existing
model defaults; new payments must set start/end dates and a nonempty description.
Status values: `^` none, `;` paid, `#` important. End modes in `flags`: 16 indefinite,
32 date, 48 count, 64 loan. Low bits: 1 weekend, 2 tentative, 4 early, 8 reminder;
256 sticky. Frequency types: d/w/m/y/W/L. Duplicate legacy options retain order;
URL note 99990102 and default highlight 99990101 survive snapshot round trips.

## Requests

- `{action:"new",id,name,timeZone,today}` → empty snapshot.
- `{action:"import",text,id,name,timeZone,today,password?}` → snapshot; text is legacy .bgt content.
- `{action:"export",snapshot,today,password?}` → `{text}` legacy .bgt content.
- `{action:"validate",snapshot}` → `{valid:true}`.
- `{action:"forecast",snapshot,today,from,to}` → `{days:[{date,openingBalance,closingBalance,adjusted,note,transactions:[{id,internalId,description,category,amount,balance,flag,note,suffix,disabled}]}],lowPoint:{date,balance}}`.
- `{action:"apply",snapshot,today,operationId,command}` → updated snapshot (atomic).

Commands use `type` and stable record `id`:
- `{type:"put_payment",id?:string,payment,from?:date}`; omit id to add; use `from` for a series split.
- `{type:"transaction",id,date,amount,note,flag,scope:"one"|"future"}`.
- `{type:"move",id,from,to,amount}`.
- `{type:"delete",id,date?:date}`; omit date to delete entire record.
- `{type:"restore",id,date}` restores a skipped occurrence.
- `{type:"balance",date,amount:number|null}` sets/removes a checkpoint.
- `{type:"day_note",date,note:string|null}`.
- `{type:"notes",notes}` sets budget notes.
- `{type:"rename",name}`.

A returned snapshot is a local domain result, never a server acknowledgement.
Forecast bounds are inclusive; balances preserve real historical checkpoints.

## Raw WebAssembly ABI

Build `budget-engine` for `wasm32-unknown-unknown`, artifact `budget_engine.wasm`.
Exports: `memory`, `alloc(len)->ptr`, `dealloc(ptr,len)`, `request(ptr,len)`,
`result_ptr()->ptr`, `result_len()->len`. UTF-8 input allocation belongs to caller;
copy bytes into `memory`, invoke request, then deallocate the input. Result memory
belongs to engine, remains valid until the next request, and must not be deallocated.
Read result using a **fresh memory buffer view after request**, because Rust may grow
memory. Each instance is sequential (one request at a time). Inputs are capped at 8 MiB.
Native `budget-engine` reexports the same `request_json` function and has a CLI reading
one JSON request per stdin line, emitting one response per stdout line for parity tests.
