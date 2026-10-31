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
- `{action:"import",text,id,name,timeZone,today,password?}` → snapshot; text is plain, sealed, or legacy encrypted .bgt content.
- `{action:"export",snapshot,today,password?}` → `{text}`; without a password the original plain .bgt serialization is retained. With a password, exports use the authenticated standalone format below.
- `{action:"validate",snapshot}` → `{valid:true}`.
- `{action:"forecast",snapshot,today,from,to}` → `{days:[{date,openingBalance,closingBalance,adjusted,note,transactions:[{id,internalId,description,category,amount,balance,flag,note,suffix,disabled}]}],lowPoint:{date,balance},totals:{moneyIn,moneyOut}}`. `totals` covers the requested range to the cent; both figures are zero or positive and leave out disabled payments and balance checkpoints.
- `{action:"apply",snapshot,today,operationId,command}` → updated snapshot (atomic).
- `{action:"passcode_wrap",recoveryKey,passcode,budgetId,entropy:{salt,nonce}}` → `{slot}`. Wraps the household key text with a user-chosen passcode (Argon2id 64 MiB/3/4 + AES-256-GCM, AAD binds `budgetId`). `entropy` is caller CSPRNG: standard base64 of exactly 16 (salt) and 12 (nonce) bytes. The passcode needs 8+ characters and is refused when too common.
- `{action:"passcode_unwrap",slot,passcode,budgetId}` → `{recoveryKey}`; the slot's GCM tag is the wrong-passcode test.

Commands use `type` and stable record `id`:
- `{type:"put_payment",id?:string,payment,from?:date}`; omit id to add; use `from` for a series split. As in the desktop editor, a `freq_type` `L` start date moves to the last matching weekday of its month, except a single payment (end mode 48, `n_pays` 1) or the unchanged start of an existing `L` payment. A `W` start date is kept: it already names the nth weekday.
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

## Password-protected standalone files

New password exports are ASCII files with `#db*sealed`, a newline, and one JSON object:

```json
{"version":1,"cipher":"AES-256-GCM","kdf":"argon2id","kdfVersion":19,"memoryKiB":65536,"iterations":3,"parallelism":4,"salt":"base64 of 16 bytes","nonce":"base64 of 12 bytes","ciphertext":"base64 of ciphertext followed by a 16-byte GCM tag"}
```

Argon2id v1.3 derives a 32-byte key using the full UTF-8 password and the RFC 9106
second recommended profile. Passwords must contain 1–65536 UTF-8 bytes, with no
normalization or silent truncation. Each export generates a fresh salt and nonce.
AES-256-GCM authenticates the complete plain serialization before parsing any budget
records. Associated data is the UTF-8 bytes `#db*sealed\n` followed by compact JSON
for this fixed-order tuple:
`[version,cipher,kdf,kdfVersion,memoryKiB,iterations,parallelism,salt,nonce]`.
The JSON envelope property order and whitespace do not affect this canonical AAD.

Readers reject unknown or duplicate fields, additional JSON values, unsupported
algorithms/versions/costs, invalid canonical padded base64 and invalid lengths
before running the KDF. The plaintext cap is 64 MiB; the envelope cap is
`4 * ceil((64 MiB + 16) / 3) + 1024` bytes. The JSON engine's existing 8 MiB
request cap also applies to imports and exports through this API. Authentication
errors return no payments, adjustments or notes. Native files may retain an outer
BOM and CRLF/CR line separator for compatibility. Legacy Blowfish and XOR files
remain readable; an explicit password save migrates them to the sealed format,
while the previous file bytes remain intact in the normal backup.

The WASM module has no imports and cannot obtain OS randomness itself. Raw ABI
password export therefore requires an additional request field:
`entropy:{salt:[16 byte values],nonce:[12 byte values]}`. A trusted host must obtain
fresh, independent bytes from a CSPRNG for **each** export. Missing or malformed
entropy fails closed. Use `loadEngine()` from `public/engine.mjs` in browser clients:
it always generates both arrays with `crypto.getRandomValues`, replaces any
caller-supplied entropy without mutating the caller's request, and fails if secure
randomness is unavailable. Native exports always obtain OS randomness and ignore
any entropy in incoming JSON. Plain exports and all imports require no host entropy.
