# Budget Calendar v1.11.4424 — Core Data Model, File Format and Engine

Source: stack-simulated pseudo code of `ca.mishell.app.budget.BudgetDB`, `Payment`, `BudgetDay`, `PaymentOption`, `PaymentFlag`, `Note`, `Transaction`, `ca.mishell.misc.SimpleCalendar`, plus `Blowfish` (key schedule / padding only) and `UnicodishBufferedWriter` (string escaping), cross-checked against `reference/sample.bgt` and the user manual `reference/budget.html`. Two caller sites in `Budget.part01` were read only to pin down the argument order of `moveTransaction`/`addMoveUndo`.

Reading note on the pseudo code (regenerated): a value that feeds both an assignment and a branch is materialised once as `$tN = expr; v = $tN; if (... $tN ...)` — a single call. The post-increment idiom `a[i++] = x` is rendered as `$tN = i; i += 1; a[$tN] = x` (first element goes to index 0). The loop increments written as `i += 1; addPeriod(..., i)` below are pre-increments in the pseudo code (`v3 += 1; addPeriod(..., v3)`) (verified against regenerated pseudo code).

Everything stated as fact below is read directly from the pseudo code. Items marked **UNKNOWN** could not be determined from the files in scope; a consolidated list is at the end.

---

## 1. Data model

### 1.1 Dates

All dates in the model are `int` in `yyyymmdd` form (e.g. `20080514`). `SimpleCalendar` (section 6) is the only arithmetic on them. Month inside `SimpleCalendar` is 0-based, but the `int` form is always 1-based month.

### 1.2 `Payment`

| Field | Type | Default (constructor) | Meaning |
|---|---|---|---|
| `id` | int | 0 | Assigned by `BudgetDB.add` (max existing id + 1) or by the loader (1-based line order of successfully parsed payment lines). Not persisted. |
| `amount` | double | 0.0 | Regular amount. Negative = expense, positive = income. |
| `description` | String | `""` | Free text. Text before the first `;` is the "short description" shown in calendar cells (`getShortDescription`). |
| `startDate` | int | today | Date of the first scheduled transaction. |
| `frequency` | int | 1 | Multiplier for `freq_type` (every N days/weeks/months/years). Forced to 1 for `'W'` by the loader. |
| `freq_type` | char | `'m'` | One of `d w m y W L` (section 3.1). |
| `n_pays` | int | 1 | Number of payments (meaningful for `END_MODE_N_PAYS`; recomputed from `endDate` for `END_MODE_DATE`; recomputed from the loan for `END_MODE_LOAN_PAID`). |
| `endDate` | int | today | Last payment date (meaningful for `END_MODE_DATE`; recomputed for N_PAYS and LOAN). Loader forces `endDate = max(endDate, startDate)`. |
| `image_path` | String | null | Icon file name. The loader always produces a non-null string (possibly `""`). |
| `flags` | int | 48 (`END_MODE_N_PAYS`) | Bit field, see 1.3. |
| `opt` | ArrayList\<PaymentOption\> | empty | Per-date overrides: custom amounts, moved-to transactions, deletions. |
| `flag` | ArrayList\<PaymentFlag\> | null | Per-date flag characters. |
| `note` | ArrayList\<Note\> | null | Per-date notes; also holds the URL under the sentinel date (1.7). |
| `hilighting` | ArrayList\<Payment$Hilight\> | null | Per-date background colours; also holds the default colour under the sentinel date (1.7). |
| `disabled` | boolean | false | Transactions still scheduled and shown, but excluded from balances (4.1). |
| `category` | String | null | Category name or null. |
| `loan_amount` | double | 0.0 | Total loan to pay off (`END_MODE_LOAN_PAID`). Sign is ignored (`Math.abs` everywhere). |
| `flag_value` | char | `'^'` | Payment-wide default flag returned by `getFlag(date)` when the date has no explicit flag. |
| `grace_period_days` | int | 0 | Grace days (used by `get_valid_payment_days`). |
| `calendarDays` | HashSet\<BudgetDay\> | empty | Transient: every `BudgetDay` that currently references this payment (transaction, regular or restorable). |
| `default_bg_color_cache` | Integer | null | Transient cache of the default highlight colour (1.7). |

`clonePayment()` copies `amount, description, endDate, freq_type, frequency, image_path, n_pays, startDate, flags, disabled, category, grace_period_days` and the default highlight colour; it creates an empty `opt` list and does **not** copy `id`, `loan_amount`, `flag_value`, `flag`, `note`, or dated highlights. (`loan_amount` and `flag_value` are therefore reset to 0.0 / `'^'` in a clone.)

`isSameSchedule(other)`: equal `startDate, endDate, freq_type, frequency, disabled, endMode, n_pays`.

`toString()` is diagnostic only (format: `[start - end, desc='…', amount=…, freq=<n><type>, n_pays=…, loan=…, grace_days=…, emode=date|indefinite|n_pays|loan, other: (no weekends)(early pay)(tentative)(sticky)(disabled)` plus optional `\nimage='…'`, `\noptions: …`, `\nflags: …`, `\nnotes: …`, `\nhighliting …`, then `]`).

### 1.3 `flags` bits

| Constant | Value | Accessors | Meaning |
|---|---|---|---|
| `NO_WEEKENDS` | 1 | `isNoWeekends/setNoWeekends` | "Business Days Only": a scheduled Saturday/Sunday is shifted (3.6). |
| `TENTATIVE` | 2 | `isTentative/setTentative` | Transaction slides to today inside its pay period (3.7). |
| `EARLY_PAY` | 4 | `isEarlyPay/setEarlyPay` | With `NO_WEEKENDS`: shift to the previous Friday instead of the next Monday. Has no effect without `NO_WEEKENDS`. |
| `REMINDER` | 8 | `isReminder/setReminder` | "Mark as" convenience flag; no engine behaviour in these classes. |
| `END_MODE_MASK` | 240 (0xF0) | `getEndMode()` = `flags & 240` | |
| `END_MODE_UNDEFINED` | 0 | | Legacy files only; the loader converts it (2.5). |
| `END_MODE_INDEFINITE` | 16 | | No end; schedule runs to the DB window end. |
| `END_MODE_DATE` | 32 | | Ends at `endDate`. |
| `END_MODE_N_PAYS` | 48 | | Exactly `n_pays` payments. |
| `END_MODE_LOAN_PAID` | 64 | | Until `loan_amount` is paid off. |
| `IS_STICKY` | 256 | `isSticky/setSticky` | Regular amount is replaced by the most recent custom amount before the date (3.8). |

`setEndMode(m)` only accepts 16, 32, 48, 64 (anything else is ignored) and does `flags = (flags & ~240) | m`.

### 1.4 `PaymentOption`

`date:int, amount:double, deleted:boolean`. One entry per date per payment (enforced by `Payment.update(date, amount, deleted)` which overwrites an existing entry for that date). Semantics:

* `deleted == false`: the transaction on `date` has amount `amount`. If `date` is a scheduled date this is an "adjusted amount"; if not, it is an extra/moved-to transaction.
* `deleted == true`: the scheduled transaction on `date` is removed (and the day becomes "restorable" for this payment). `amount` is still stored (set to the regular amount by `addNoTransaction`/`setNoTransaction`, or to the moved amount by `moveTransaction`) but is ignored by everything except the loader/saver round trip (the saver drops it; see 2.6).

`toString`: `[%d: %.2f]` or `[%d: DELETED(%.2f)]`.

Payment helpers over `opt`:

* `getOptionIndex(date)` / `getOptionByDate(date)` / `getOption(i)`.
* `getAmountBeforeDate(date)`: amount of the non-deleted option with the greatest date strictly less than `date`; if none, `this.amount`.
* `getOpt(from, to)`: array of all options (deleted or not) with `from <= date <= to`, in list order; `null` if none.
* `getDeleted(from, to)`: list of deleted options in the inclusive range.
* `hasOptionsAfter(date)`: any option (deleted or not) with `date' > date`.
* `getFirstOptionAfter(date)`: the option (deleted or not) with the smallest date strictly greater than `date`, or null.
* `setCustomAmount(date, amt)` = `update(date, amt, false)`; `setNoTransaction(date)` = `update(date, this.amount, true)`. `addCustomAmount`/`addNoTransaction` append unconditionally (loader only; no de-duplication).
* `clear(date)`: remove the option at `date` and the first flag at `date`. `clearBefore(d)`: remove options and flags with `date < d`. `clearAfter(d)`: remove options and flags with `date > d`. `clearAll()`: options only.
* `moveTransaction(dstDate, srcDate, amount)` — note the order (destination first):
  1. find the flag at `srcDate`; remember its char (default `'^'`), remove that flag entry;
  2. `note = getNote(srcDate)`, `hilight = getHilighting(srcDate)` (−1 if none);
  3. `update(dstDate, amount, deleted=false)`; `update(srcDate, amount, deleted=true)`;
  4. if the remembered flag is not `'^'`: `setFlag(dstDate, flag)`;
  5. if `note != null`: `setNote(dstDate, note)`, `setNote(srcDate, null)` (removes);
  6. if `hilight != -1`: `setHilighting(dstDate, hilight)`, `setHilighting(srcDate, -1)` (removes).
  Callers pass `amount` = moved amount, or moved amount + existing destination amount when the user chose to sum (the UI asks when a transaction of the same payment already exists on the destination day).

### 1.5 `PaymentFlag`

`date:int, value:char`. Constants: `FLAG_NONE = '^'` (94), `FLAG_PAID = ';'` (59), `FLAG_IMPORTANT = '#'` (35). Default value `'^'`.

* `setFlag(date, c)`: overwrite if an entry exists for `date`, else append (creating the list). Note that setting `'^'` explicitly creates/keeps an entry (saved as `<date>^`).
* `getFlag(date)`: the entry's char, else `flag_value` (payment default).
* `isPaid(from, to)`: true if any flag entry in the inclusive range has value `';'`.

`toString`: `[<date>: (none)|(paid)|(important)]`.

### 1.6 `Note`

`date:int, note:String`. `setNote(date, s)`: if an entry exists, replace text, or remove the entry when `s == null`; if none exists and `s != null`, append (creating the list). `getNote(date)`: text or null. Special case `getNote(0)`: concatenation of every note text each followed by a single space. `getNoteCount()`. `clearNotesBefore(d)`: remove notes with `date < d`. `getOneLineNote(date)`: `""` if none, else `" -- " + Budget.remove_new_lines(text).trim()`, truncated to the first 78 chars + `"..."` when the result length is ≥ 80. `toString`: `[<date>: <text|(null)>]`.

### 1.7 Sentinel dates stored inside the per-date lists

| Constant | Value | Where | Use |
|---|---|---|---|
| `DEFAULT_COLOR_DATE` | 99990101 | `hilighting` | The payment's default background colour. `setDefaultHighlighting(c)` = `setHilighting(99990101, c)`; `getDefaultHilighting()` returns **only** `default_bg_color_cache` (−1 when null). `setHilighting` keeps the cache in sync whenever `date == 99990101` (−1 clears the cache). The entry is persisted like any other highlight (`99990101&rrggbb`). |
| `URL_NOTE_DATE` | 99990102 | `note` | The payment's URL. `setURL(s)` = `setNote(99990102, s)`; `getURL()` = `getNote(99990102)`. Persisted as `99990102=<url>`. |
| `DATA_FILE_NUM_LINES` | 99990103 | — | Declared only; unused in every file read. |
| `DATA_FILE_EOD` | 99990104 | — | Declared only; unused. |

`BudgetDB.getLatestAdjustmentDate()` skips adjustment options whose year is 9999, so the sentinel range is reserved in the adjustments' `opt` list too (no code in scope writes such an option).

### 1.8 `Payment$Hilight`

`date:int, bg_color:int` (0xRRGGBB). `setHilighting(date, color)`: if an entry exists → remove when `color == -1`, else overwrite; if none exists → do nothing when `color == -1`, else append (creating the list). `getHilighting(date)` → colour or −1. `toString`: `[%d: %06x]`.

### 1.9 `Transaction`, `BudgetDay`

`Transaction`: `pmt:Payment, amount:double, balance:double (running balance after this transaction), suffix:char`. Suffix values produced by the engine: `0` (none), `'\''` (39, "adjusted": custom amount, moved-to, or weekend-shifted), `'*'` (42, last transaction of the payment). `toString`: `amount=%.2f, balance=%.2f, suffix=%c pmt=%s`.

`BudgetDay` (one per calendar day of the window):

| Field | Meaning |
|---|---|
| `opening_balance` | Balance carried in from the previous day. |
| `closing_balance` | Balance at end of day (or the adjusted value). |
| `adjustment_diff` | `closing_balance − computed balance` when adjusted, else 0. |
| `closing_balance_adjusted` | A balance adjustment exists on this day. |
| `sorted` | Transient sort flag. |
| `transactions` | ArrayList\<Transaction\> or null. |
| `restorable` | ArrayList\<Payment\> or null: payments whose transaction on this day was deleted/moved away. |
| `regulars` | ArrayList\<Payment\> or null: payments that had a *scheduled* transaction placed on this day. |

Methods: `add(p, amount, suffix)` — creates a `Transaction`, **removes any existing transaction of the same payment on this day** (one transaction per payment per day), appends it, marks unsorted and registers the day in `p.calendarDays`. `addRegular(p, amount, suffix)` = `add` + append `p` to `regulars` (duplicates possible). `remove(p)` removes the first transaction of `p` (returns whether one existed). `removeRegular(p)` removes one entry from `regulars`. `addRestorable(p)` appends if not present (and registers the day in `p.calendarDays`); `removeRestorable(p)`, `isRestorable(p)`, `getRestorable()` (array or null). `get(p)` returns the transaction of `p` or null. `hasTransactions()` = list non-null (may be empty). `transactionIterator()` sorts on first use.

Transaction sort order (`BudgetDay` static comparator, stable sort):
1. enabled payments before disabled ones;
2. within the same disabled state: if **both** amounts are negative → ascending numeric (−100 before −50, i.e. largest expense first); otherwise descending numeric (largest income first, and any income before any expense).

`getClosingBalance(exact)`: if `exact` → `closing_balance`; else if `closing_balance > 0` → `(double)(int)(closing_balance + 0.05000000074505806)` else `(double)(int)(closing_balance − 0.05000000074505806)`. The constant is `0.05f` widened to double. This is truncation toward zero after nudging by 0.05 (10.96 → 11.0, 10.94 → 10.0, −10.96 → −11.0). `getUnadjustedBalance()` = `closing_balance − adjustment_diff` when adjusted, else `closing_balance`.

### 1.10 `BudgetDB`

| Field | Meaning |
|---|---|
| `payments` | ArrayList\<Payment\>. |
| `adjustments` | A `Payment` object used as a container: its `opt` list holds balance adjustments (date → absolute closing balance, never `deleted`), its `note` list holds day notes. Its other fields are defaults and are not saved (except that the loader would accept `$category`/flags/highlights on the `:` line and store them there — they are written back by `optsString` but have no effect). |
| `budgetDay[]` | One `BudgetDay` per day from 1 January of `startYear` to 31 December of the end year. |
| `startDate`, `endDate` | Window bounds (SimpleCalendar); `startDateAsInt`, `endDateAsInt` cached ints. |
| `startYear`, `yOffset[]` | `yOffset[i]` = index of 1 January of `startYear + i`. |
| `in_sync` | Balances are up to date. |
| `updateCount` | Incremented on every structural change (UI dirty tracking). |
| `notes` | File-level notes (`=` line). Default `""`. |
| `version` | File version string (`v` line). Default `"1"`. |
| `fileName` | Path loaded from, or null for a new DB. |
| `moveUndoInfo` | Single-level undo for the last move (5.6). |
| static `today` | `new SimpleCalendar().get()` evaluated **once at class load**; never refreshed while the app runs. Used by tentative logic. |
| static `bf` | Current `Blowfish` instance (null = unencrypted). Shared static state. |
| static `comparator` | Payment sort order (5.2). |

---

## 2. The `.bgt` file format

### 2.1 Encoding and line structure

* Text, read and written as **ISO-8859-1**, lines terminated by `\n` (the reader accepts `\r\n` too via `BufferedReader.readLine`).
* The last line (END-OF-DATA) is written **without** a trailing newline.
* Non-ASCII characters never appear in payload fields because of the escaping in 2.3; so the file is effectively ASCII, except legacy files that used bytes 0xFF/0xFE (2.3).
* Fields are separated by `,` (the loader uses `StringTokenizer(line, ",")`, so empty fields are *skipped*, and every token is `trim()`med). The saver separates with `", "`.
* A line that is empty or starts with `#` after decryption is a comment, except the END-OF-DATA trailer (2.2).

Line order written by `save`:

```
#db*encrypted                      (only when a password is given)
v,2
:<adjustment options>
=,<encoded notes>
<payment line> × N                 (in current in-memory order)
## END-OF-DATA; line_no=<L> ##    (no newline after it)
```

`L` is the 1-based line number of the trailer itself, counting the `#db*encrypted` line when present. The loader counts every physical line it reads (including the header and the trailer) and requires equality; see 2.2.

### 2.2 END-OF-DATA trailer (`is_end_of_data`)

Constants: `$END_OF_DATA_LINE_NO = "## END-OF-DATA; line_no="`, `$END_OF_DATA_LINE_TRAILER = " ##"`.

Loader, for every line that is empty or starts with `#`: if it starts with the prefix:
* If it does not end with `" ##"`, a stderr message is printed (not fatal).
* The number between the prefix and the first `" ##"` is parsed. If `" ##"` is absent → exception. If the number ≠ the loader's own line count → exception `mismatched number of read lines!`. Either exception propagates to the outer handler, which counts an error and makes the whole load **fail**.
* If text follows `" ##"` → stderr warning. If another line exists after the trailer → stderr warning (`more data past end-of-data`). Reading stops here.
* Any other `#…` or empty line is ignored.

Files without a trailer (like `sample.bgt`) load fine: reading ends at EOF.

### 2.3 String escaping (`encodeDescription` / `decodeDescription`)

Applied to: description, category, image path, notes (file notes and per-date notes/URL). Not applied to numbers.

`encodeDescription(s)` = `UnicodishBufferedWriter.str2unicodish(s)` followed by a legacy pass that maps `','`→U+00FF and `'\n'`→U+00FE. Since `str2unicodish` already escapes those characters, the legacy pass is a no-op on current saves.

`str2unicodish`: every char `c` with `c >= 128` or `c == ','` or `c == '\n'` or `c == '\r'` becomes the 8-character sequence `&#xHHHH;` (lowercase hex, 4 digits, e.g. `,` → `&#x002c;`); all other chars are copied.

`decodeDescription(s)`: scans left to right: U+00FF → `','`; U+00FE → `'\n'`; a window of ≥ 8 chars matching `&#x????;` (positions 0..2 `&#x`, position 7 `;`) → the char with that hex value (parse failure → `'?'`), consuming 8 chars; otherwise copy one char. Thus both the current (`&#x…;`) and legacy (0xFF/0xFE) encodings are readable.

### 2.4 Header lines

* `v,2` — version. Loader: first token `v`; `version = decode(next token)` (default `"1"` when absent). The saver always writes `2`.
* `:` + `optsString(adjustments)` — balance adjustments and day notes (2.6 for the option syntax). Loader: first token `:` → the remaining tokens are parsed as options **into the `adjustments` payment**. E.g. `:, 20071205:1500.0, 20120116:500.0, 20120301=Pay day`.
* `=,<encoded notes>` — file notes. Loader: `notes = decode(next token)` or `""` if there is no further token (the saver writes `=,` for empty notes, giving exactly that case).

### 2.5 Payment line

Written as (Java string concatenation; `D(x)` = `Double.toString`, `E` = `encodeDescription`):

```
<D(amount)><%D(loan_amount)?><!?><flagchar?>, <E(description)>, <startDate>, <frequency><freq_type><;grace?>, <n_pays>, <endDate>, <flags>, <E(image_path) or ""> <optsString>
```

Field 1 (amount with suffixes), in writing order:
1. `D(amount)` (Java `Double.toString`: `-80.0`, `1500.0`, `0.1`, and scientific notation `1.0E7` for |x| ≥ 10^7 or < 10^-3; the loader uses `Double.parseDouble` which accepts all of these — a Rust writer must emit Java-compatible text, and a Rust reader must accept the `E` form).
2. `%` + `D(loan_amount)` when `endMode == END_MODE_LOAN_PAID` **or** `loan_amount != 0.0`.
3. `!` when `disabled`.
4. the payment default flag char when `flag_value != '^'` (so `;` or `#`).

Loader for field 1 (order of search, each one truncating the string at the found index): `'^'` → `flag_value='^'`; `';'` → `flag_value=';'`; `'#'` → `flag_value='#'`; `'!'` → `disabled=true`; `'%'` → `loan_amount = parseDouble(text after %)`; then `amount = parseDouble(rest)`. Because each search runs on the already-truncated prefix, the first match in the order `^ ; #` wins if several are present. Example: `-100.0#` → amount −100, every transaction flagged important by default. `-400.0%5000.0!` → amount −400, loan 5000, disabled.

Remaining fields, in order (all `trim()`med):
2. description → `decodeDescription`.
3. `startDate` → `parseInt`.
4. frequency: if it contains `;`, `grace_period_days = parseInt(after ;)` and the part before is kept; then `frequency = parseInt(all but last char)`, `freq_type = last char`. `freq_type` must be in `"dwmyWL"`, otherwise the line is an error (`Invalid frequency type`). If `freq_type == 'W'` then `frequency = 1`.
5. `n_pays` → `parseInt`.
6. `endDate` → `parseInt`, then `endDate = max(endDate, startDate)`.
7. `flags` → `parseInt`. If `getEndMode() == 0` (legacy): `n_pays == 0 && startDate == endDate` → `END_MODE_INDEFINITE`; else `n_pays != 0` → `END_MODE_N_PAYS`; else `END_MODE_DATE`.
8. image path → `decodeDescription` of the token (mandatory token; the saver guarantees one by emitting `" "` before the options, so a payment with no image and no options still has a trailing `" "` token which trims to `""`).
9. zero or more option tokens (2.6).

Any exception while parsing a line (number format, missing token, bad option) appends `(<line>): Invalid data for '<field>'\nException: …` to the log, increments the error count and continues with the next line — but at the end **any error makes `loadFileData` return false** (log gets `failed`, and `errinfo.txt` is written to `Budget.dirHome` when loading for real). Load is all-or-nothing.

Successfully parsed payments get `id = 1, 2, 3…` in file order (the counter increments right after the amount parses, so a line that fails later still consumes an id).

### 2.6 Option tokens (`optsString`, loader `opts` loop)

`optsString(p)` emits, in this order, each prefixed by `", "`:
1. `$` + `E(category)` if `category != null`.
2. For every `PaymentOption` in list order: deleted → `<date>` (8 digits only); else `<date>:<D(amount)>`.
3. For every `PaymentFlag`: `<date><value>` (e.g. `20061215#`, `20061225^`, `20070101;`).
4. For every `Note`: `<date>=<E(text)>` (including `99990102=<url>`).
5. For every `Hilight`: `<date>&<hex>` where `hex = Integer.toHexString(bg_color)` — lowercase, **no zero padding** (0x0000ff → `ff`); includes `99990101&<hex>` for the default colour.

Loader, per trimmed token (empty tokens skipped):
* starts with `$` → `category = decode(rest.trim())` if non-empty (anywhere among the options).
* length < 8 → error `invalid option`.
* `date = parseInt(first 8 chars)`; if the token is exactly 8 chars → `addNoTransaction(date)` (deleted marker, amount = regular amount).
* else by the 9th char: `;` `#` `^` → `setFlag(date, c)`; `=` → `setNote(date, decode(rest from index 9, trimmed))` if non-empty; `:` → `addCustomAmount(date, parseDouble(from index 9))`; `&` → `setHilighting(date, parseInt(from index 9, radix 16))`; anything else → error `Invalid option`.
* There is **no** `%` option character; `%` only exists in the amount field.

### 2.7 Worked decode of `sample.bgt`

```
1  v,2
```
Version `"2"`.

```
2  :, 20071205:1500.0, 20080514:4280.0, 20080513:4480.0, 20120116:500.0
```
Four balance adjustments (closing balance forced to): 2007-12-05 → 1500.00; 2008-05-14 → 4280.00; 2008-05-13 → 4480.00; 2012-01-16 → 500.00. No day notes.

```
3  =,
```
File notes = `""`.

```
4  -80.0, Car Insurance, 20040825, 1m, 1, 20040825, 16, paper.gif , $Auto, 20080526:700.0, 20061225^
```
Expense 80.00, default flag none, enabled, no loan. Description "Car Insurance", first on 2004-08-25, every 1 month, `n_pays`=1 and `endDate`=20040825 are ignored because flags=16 = `END_MODE_INDEFINITE`. Image `paper.gif`. Category `Auto`. Custom amount +700.00 on 2008-05-26 (not a scheduled date → an extra transaction with `'` suffix). Explicit "none" flag on 2006-12-25.

```
5  -400.0, Car Loan, 20040815, 1m, 1, 20040815, 28, car0.gif , $Auto, 20061215#
```
flags 28 = 16 INDEFINITE + 8 REMINDER + 4 EARLY_PAY (EARLY_PAY is inert without NO_WEEKENDS). Monthly on the 15th from 2004-08-15. Flag `#` (important) on 2006-12-15.

```
6  -30.0, Gasoline, 20040807, 1w, 1, 20040807, 16, car1.gif , $Auto, 20061223^
```
Weekly (Saturdays, 2004-08-07 is a Saturday) indefinite. Flag none on 2006-12-23.

```
7  -100.0, Groceries, 20040802, 1w, 1, 20040809, 16, apple.gif , 20041006:-100.0, 20061218^
```
Weekly on Mondays from 2004-08-02, indefinite (endDate 20040809 ignored), no category. Custom amount −100.00 on 2004-10-06 (a Wednesday, not a scheduled day → an extra transaction; had it been a scheduled day the engine would drop the option as redundant, see 3.9). Flag none on 2006-12-18.

```
8  -100.0#, Hydro, 20040815, 1m, 1, 20040815, 17, power1.png , $Utilities, 20061215^, 20070115#, 20080515^
```
`flag_value = '#'`: every Hydro transaction is "important" unless overridden. flags 17 = INDEFINITE + NO_WEEKENDS (a 15th falling on Sat/Sun moves to the following Monday, suffix `'`). Per-date overrides: none on 2006-12-15 and 2008-05-15, important on 2007-01-15.

```
9  -30.0, Mobile, 20041013, 1m, 1, 20041013, 17, mobile.gif , $Utilities, 20041013
```
Monthly on the 13th, NO_WEEKENDS. The 8-digit option `20041013` deletes the very first scheduled transaction; 2004-10-13 is restorable.

```
10 -1100.0, Rent, 20040801, 1m, 1, 20040807, 16, house.gif 
```
Monthly on the 1st, indefinite, no options (trailing `" "` token → `image_path = "house.gif"`, then nothing).

```
11 1500.0, Salary, 20040730, 2w, 1, 20040806, 16, dollar1.gif 
```
Income 1500.00 every 2 weeks from Friday 2004-07-30, indefinite.

```
12 -500.0, Savings, 20050316, 1m, 1, 20050316, 16, pig.gif , 20080517:100.0, 20120626&ee82ee, 20120525&fa8072
```
Monthly on the 16th. Custom amount +100.00 on 2008-05-17 (not a scheduled date → extra transaction). Highlights: 2012-06-26 background 0xEE82EE (violet), 2012-05-25 background 0xFA8072 (salmon).

```
13 -50.0, TV, 20040810, 1m, 1, 20040810, 16, tvset.gif 
```
Monthly on the 10th, indefinite.

```
14 -250.0, Visa Pmt, 20050318, 1m, 1, 20050318, 17, pct.gif , 20061218:-500.0, 20080918=This is a note Sep-08, 20080818=This is another note Aug-08
```
Monthly on the 18th, NO_WEEKENDS. Adjusted amount −500.00 on 2006-12-18 (a Monday and a scheduled day → the regular transaction gets amount −500 and suffix `'`). Notes on 2008-09-18 and 2008-08-18.

Line 15 is empty (ignored). No END-OF-DATA trailer — accepted.

### 2.8 Encryption

Constants: `encrypted = "#db*Encrypted"` (legacy XOR format), `encrypted2 = "#db*encrypted"` (Blowfish format). `isEncrypted(path)`: true if the first line of the file (read with the platform default charset) equals either string.

Loader (`loadFileData(path, password, …)`), line 1 handling:
* `#db*Encrypted` → skip the line; `bf` stays null; `password` is kept → every following line is decoded with the **XOR** scheme.
* `#db*encrypted` → `bf = new Blowfish(); bf.setPassword(password)`; following lines decoded with Blowfish.
* anything else → `password = null`; the line is processed as data (plain file; a password supplied by the caller is ignored).

`decryptString(line, password)`: if `bf != null`: `bytes = bf.decrypt(hex2bytes(line))`, result = `new String(bytes, 0, index of first 0x00 byte or length)` (platform default charset). Else `encryptString(line, password)` (XOR is symmetric).

`encryptString(s, password)`:
* if `password` non-null, non-empty **and** `bf == null` → XOR: for i in 0..len: `c = s[i] ^ password[i mod len(password)]`; if `c` is `'\n'` (10) or `'\r'` (13) the original character is kept unchanged (so the output never contains line breaks, and the transform stays an involution on those positions); result is a `char` string.
* else if `bf != null` → `bytes2hex(bf.encrypt(s.getBytes()))` — lowercase hex, 2 chars per byte.
* else → `s` unchanged.

`save` sets `bf = null` when `password == null`, else a fresh `Blowfish` with the password; therefore **save always produces the Blowfish format** (`#db*encrypted`), and the XOR format is read-only legacy. Every line (including `v,2` and the trailer) is encrypted independently; the `#db*encrypted` header is plain text. Password verification = `isDecryptable(path, password)` = run `loadFileData` with a throw-away payment list and `props == null` and return its success flag (a wrong key yields garbage that fails to parse).

Blowfish (`ca.mishell.misc.Blowfish`): standard Blowfish-ECB. `setPassword(String)` uses `password.getBytes()` (platform default charset) truncated to `MAX_PASSWD = 56` bytes; an empty key leaves the P-array/S-boxes at their π constants (no key mixing). `encrypt(bytes)`: zero-pad to a multiple of 8, encode each 8-byte block in place; `decrypt(bytes)`: decode each 8-byte block (no unpadding — the caller truncates at the first NUL). Words are big-endian. `hex2bytes` prepends `"0"` to an odd-length string. The P-array initial constants are the standard ones (e.g. `PA_Init[0] = 0x243F6A88`); verify a Rust implementation against the published Blowfish test vectors.

### 2.9 Save procedure and backups (`save(path, password, log)`)

1. Appends `Unable to save '<path>'\n` to `log` unconditionally (the caller shows the log only on failure).
2. Builds the whole file in memory (ISO-8859-1), as in 2.1.
3. If the target file exists and its length and bytes are identical to the new content → return `true` without touching disk (no backup).
4. Otherwise, if the file exists: pick a backup name `path + ".bak"`; while that name exists, try `path + ".bak.1"`, `.bak.2`, … up to `.bak.100`; if `.bak.100` also exists → log `Too many backup files, try using 'Save as..'!`, return false. Copy the existing file to the backup name (`copyFile`); on failure → log `Failed to create a backup file, try using 'Save as..'!`, return false.
5. Write the bytes to `path`. On exception log `File Save:…` and fail.
6. **If the write succeeded the backup file is deleted.** The `.bak` therefore only survives a failed write.
7. Return true iff no error occurred.

### 2.10 `toDate` / `sDate` / `split` (helpers)

* `toDate(s)`: if `s` contains `-`: split on `-`; `y = max(parseInt(part0), 2000)`, `m = clamp(parseInt(part1), 1, 12)` if present else 1, `d = clamp(parseInt(part2), 1, 31)` if present else 1; `v = y*10000 + m*100 + d`. Else `v = parseInt(s)`. Any exception → `v = 20000101`. Returns `new SimpleCalendar(v).get()` (normalises short forms and day overflow, see 6.2).
* `sDate(v)`: `"yyyy-MM-dd"` using `DecimalFormat("0000")`/`("00")`.
* `split(s, c)`: splits on the character; trailing empty segment yields a `null` slot in the returned array (array length = separators + 1).

---

## 3. Schedule generation

### 3.1 Frequency types → calendar fields (`calendarField`)

| `freq_type` | field | meaning |
|---|---|---|
| `'d'` | `DAY` (1) | every `frequency` days |
| `'w'` | `WEEK` (5) | every `frequency` weeks |
| `'m'` | `MONTH` (2) | every `frequency` months |
| `'y'` | `YEAR` (3) | every `frequency` years |
| `'W'` | `NTH_WEEKDAY` (7) | the same N-th weekday of each month (frequency forced to 1) |
| `'L'` | `LAST_WEEKDAY` (8) | the last such weekday of each `frequency`-th month |
| other | `MONTH` (2) | default |

### 3.2 `addPeriod(cal, field, frequency, startDate, n)` — the single stepping primitive

`n` is the 1-based index of the period being computed (callers pass the count of steps taken so far).

```
if field == NTH_WEEKDAY:   cal.set(startDate); cal.add(NTH_WEEKDAY, n)          // anchored, frequency ignored
else if (field == MONTH or field == YEAR) and cal.getDay() > 27:
                           cal.set(startDate); cal.add(field, frequency * n)      // anchored (keeps 29/30/31)
else:                      cal.add(field, frequency)                              // incremental
```

Consequences: monthly/yearly schedules on days 1..27 step incrementally; on 28..31 they are recomputed from the start date so that e.g. 31 Jan → 29 Feb → 31 Mar (clamped per month, never drifting). `LAST_WEEKDAY` and `DAY`/`WEEK` are incremental. For `LAST_WEEKDAY` the weekday used is the current `cal`'s weekday, so the start date must itself be the intended weekday (the UI is expected to ensure it).

`get_period_days(p)` = `getDiff(start, addPeriod(copy of start, …, n=1))` — length of the first period in days.

### 3.3 Enumerating regular dates

All enumerations use the same loop shape (`isRegularPaymentOn`, `nextRegularPaymentAfterDate`, `getNumberOfPlannedTransactions`, `update`):

```
cal = startDate; i = 0
loop:
  if endMode != INDEFINITE and i >= n_pays: stop
  if endMode == INDEFINITE and cal > db.endDate: stop        // (only where stated)
  <visit cal>
  i += 1; addPeriod(cal, field, frequency, startDate, i)
```

* `isRegularPaymentOn(p, date)`: visit → `cal == date` ⇒ true; `cal > date` ⇒ false. Applies both stop rules. Relies on `n_pays` being correct for DATE/LOAN modes (it is kept in sync by `update`).
* `nextRegularPaymentAfterDate(p, date)`: returns the first `cal > date`, or −1 when the `n_pays` limit is hit. For INDEFINITE it does **not** stop at `db.endDate`.
* `getNumberOfPlannedTransactions(desc, from, to)`: over all payments whose description `equalsIgnoreCase(desc)`, counts visited dates with `from <= cal <= to`; stops when `cal > to`. Applies both stop rules.

### 3.4 End modes and `n_pays`/`endDate` synchronisation (first half of `update`)

`update(p)` (public) = `update(p, true)`; `rebuildDB` calls `update(p, false)`. Before placing transactions:

* `END_MODE_N_PAYS`: `n_pays = max(n_pays, 1)`; `endDate` := the date of payment number `n_pays` (start, then `n_pays − 1` `addPeriod` steps).
* `END_MODE_DATE`: `endDate = max(endDate, startDate)`; `n_pays` := number of scheduled dates `<= endDate` (always ≥ 1: `n = 1; loop { addPeriod(cal, n); if cal > endDate break; n++ }`). `recalculate_n_pays(p)` is this same computation as a separate method.
* `END_MODE_LOAN_PAID`: `ls = calcLoanSchedule(p, null)`; `endDate = ls.endCal`, `n_pays = ls.n_pays`, and the loop below uses `ls.last_payment_amount` for the final payment.
* `END_MODE_INDEFINITE` / undefined: nothing.

### 3.5 Loan pay-off (`calcLoanSchedule(p, asOf)`)

```
sum = 0; minOpt = maxOpt = p.startDate; trans = (asOf != null) ? [] : null
for o in p.opt where !o.deleted:
    sum += o.amount; minOpt = min(minOpt, o.date); maxOpt = max(maxOpt, o.date)
    trans += (o.date, o.amount)
actualStartCal = min(p.startDate, minOpt); actualEndCal = maxOpt; endCal = maxOpt (tracked but overwritten below)
n_pays = 0; cal = startDate
isExpense = p.amount < 0
remaining = |loan_amount| − (isExpense ? −sum : sum)      // options already paid reduce the loan
abs_amt = |p.amount|
while remaining > 0:
    n_pays += 1
    if no option (deleted or not) exists exactly on cal:    // isModifiedOn
        if remaining <= abs_amt:
            last_payment_amount = isExpense ? −remaining : remaining
            trans += (cal, last_payment_amount); break
        remaining −= abs_amt; trans += (cal, p.amount)
    // a modified date consumes a slot but contributes nothing here
    addPeriod(cal, field, frequency, startDate, n_pays)
endCal = cal                                   // date of the last payment (or startDate if loop never ran)
if endCal > actualEndCal: actualEndCal = endCal
if asOf != null:
    abs_amount_remaining = | Σ trans.amount where trans.date > asOf |;  n_remaining = count of those
```

No rounding anywhere: plain `double` arithmetic and comparisons. `n_pays` counts every schedule slot up to and including the final one, including slots that had an option. If `remaining <= 0` initially, `n_pays = 0`, `last_payment_amount = 0.0`, `endCal = startDate`. If `p.amount == 0` and `remaining > 0` the loop never terminates (the UI must reject a zero amount for loan mode). The option check uses the unshifted schedule date (weekend shifting is not considered).

`getPaymentsRemaining` for LOAN mode returns `(abs_amount_remaining, n_remaining)` of `calcLoanSchedule(p, asOf)`.

### 3.6 Placing regular transactions (second half of `update`)

Let `isLast` start false, `tentative = p.isTentative()`, `sticky = p.isSticky()`, `noWeekends = p.isNoWeekends()`. If `removeFirst`, `removeFromBudgetDays(p)` first (removes `p` from the transactions, regulars and restorable lists of every day in `p.calendarDays`, clears the set, `updateCount++`, `in_sync = false`). Then `in_sync = false` and the loop of 3.3 (both stop rules), visiting each `cal` as follows:

1. **Amount.** `amt = p.amount`, except: LOAN mode and `i + 1 == n_pays` → `amt = last_payment_amount`, and if `!p.hasOptionsAfter(cal)` then `isLast = true`. Then if `sticky`: `amt = p.getAmountBeforeDate(cal)` (overrides even the loan's last amount).
2. **Last marker.** If not yet `isLast`, mode ≠ INDEFINITE, `i + 1 == n_pays` and `!p.hasOptionsAfter(cal)` → `isLast = true`. (`isLast` once set stays set for later iterations, but by construction this is the final iteration for finite modes.)
3. **Tentative** → section 3.7 (then continue to the next slot).
4. **Weekend shift.** Else if `noWeekends` and `cal` is Saturday (6) or Sunday (0): `d = copy(cal)`; if `EARLY_PAY`: Sunday → `d −= 2 days`, Saturday → `d −= 1 day` (previous Friday); else Sunday → `+1`, Saturday → `+2` (next Monday). If `d` is inside the window: `day(d).addRegular(p, amt, isLast ? '*' : '\'')`. The shifted transaction always carries the `'` suffix (unless it is the last).
5. **Normal.** Else if `cal` inside the window: `day(cal).addRegular(p, amt, isLast ? '*' : 0)`.

Then `i += 1; addPeriod(...)`.

### 3.7 Tentative transactions (inside the loop above)

```
periodEnd = copy(cal); addPeriod(periodEnd, field, frequency, startDate, i + 1); periodEnd −= 1 day
if !(periodEnd > cal): do nothing for this slot          // period of 1 day (e.g. '1d'): tentative payments place nothing
first = cal.get(); last = periodEnd.get()
opts = p.getOpt(first, last)                              // all options in the period, deleted or not; null if none
currentOrFuture = first >= BudgetDB.today
todayInPeriod  = first <= today <= last
target = todayInPeriod ? today : cal;  if todayInPeriod: currentOrFuture = true
if opts == null:
    if currentOrFuture and target in window: day(target).addRegular(p, p.amount, 0)   // note: p.amount, not amt
    // past period without options: nothing is placed ("simply removed")
else if sticky and p.disabled:
    for o in opts: if o.deleted and o.date != first and o.date != target: p.clear(o.date)
    if !p.isPaid(first, last) and target in window:
        if todayInPeriod:        day(target).addRegular(p, p.getAmountBeforeDate(target), 0)
        else if currentOrFuture: day(target).addRegular(p, p.amount, 0)
else:
    nDeleted = 0
    for o in opts: if o.deleted: nDeleted++; p.clear(o.date)        // deleted markers in the period are discarded
    if nDeleted == opts.length and currentOrFuture: p.setNoTransaction(target)   // re-created at the (slid) target date
    // non-deleted options in the period are placed by the options pass (3.9)
```

`today` is the process-start date (1.10). Note that this branch **mutates the payment's option list** during a rebuild, so loading and immediately saving a file with tentative payments can change it.

### 3.8 Sticky amounts

`IS_STICKY`: in step 1 of 3.6 the regular amount is `getAmountBeforeDate(cal)` — the amount of the latest non-deleted option strictly before `cal`, or `p.amount` if none. Combined with `disabled` (manual: "emulate a balance of some other continuous amounts") the tentative branch uses `getAmountBeforeDate(target)` as well.

### 3.9 Applying options (final pass of `update`)

For each option `o` in `p.opt` (iterating with removal):

* `idx = calIndex(o.date)`; if −1 (outside the window) → leave the option untouched, continue.
* **Deleted:** `removed = day.remove(p)`. If `removed || tentative` → `day.addRestorable(p)`. Otherwise (nothing to delete and not tentative) → **the option is removed from `p.opt`** (stale deletion clean-up).
* **Not deleted:** `suffix = '\''`; if mode ≠ INDEFINITE and `!p.hasOptionsAfter(o.date)` and `o.date >= p.endDate` → `suffix = '*'`. `tx = day.get(p)`:
  * no transaction → `day.add(p, o.amount, suffix)` (an extra/moved-to transaction);
  * transaction exists and `tx.amount == o.amount` and `!tentative` → **the option is removed from `p.opt`** (redundant custom amount);
  * otherwise `tx.amount = o.amount`; if `tx.suffix == 0` then `tx.suffix = suffix` (an existing `'*'` or `'` is kept).

`update` returns 0.

Consequences worth preserving in a rewrite: (a) a custom amount equal to the computed regular amount silently disappears on the next update (and is not saved); (b) a deleted marker on a date with no scheduled transaction disappears (non-tentative); (c) a moved transaction = deleted marker at the source (source day restorable) + custom amount at the destination with suffix `'`; (d) options outside the window are preserved unchanged.

### 3.10 Restorable transactions

`BudgetDay.restorable` lists payments with a deleted option on that day (3.9). The UI offers "Restore '<name>'" on such days (day number suffixed with `'` in the cell). Restoring is done by the UI through `Payment.clear(date)` / option edits followed by `db.update(p)` — the DB has no dedicated restore method.

### 3.11 Balance adjustments (`adjustments` payment)

* `updateAdjustment(cal, amount)`: `adjustments.setCustomAmount(cal.get(), amount)` then `updateAdjustments(cal)`.
* `removeAdjustment(cal)`: `adjustments.clear(cal.get())` then `updateAdjustments(cal)`.
* `adjustmentsSize()`, `getAdjustment(i)` (the `PaymentOption`).
* `updateAdjustments(cal)`: `updateCount++`; clears `closing_balance_adjusted` on every day; for each adjustment option inside the window sets `closing_balance_adjusted = true` and `closing_balance = amount`; if balances were in sync, marks them out of sync and, when `cal != null`, immediately runs `calcBalances(cal)`.
* `getLatestAdjustmentDate()`: max option date whose year ≠ 9999, or −1. `getOldestAdjustmentDate()`: min over `today`, all adjustment dates and **all** payment option dates (deleted included).
* `adjustment_diff` is computed by `calcBalances` (4.1).

### 3.12 Valid payment days (`get_valid_payment_days(p, cal)`)

Used for hover highlighting. `grace = tentative ? get_period_days(p) : grace_period_days`. Iterate `d` from the first day of `cal`'s month up to (excluding) the first day of the next month; at the first `d` with `isRegularPaymentOn(p, d)` that passes the filter, return the set `{d, d+1, …, d+grace−1}` (empty when `grace == 0`). Filter: skip `d` if `tentative` and `cal ∉ [d, d+grace−1]`; skip `d` if `cal > d` and `cal ∉ [d, d+grace−1]`. If no scheduled day passes, the set is empty.

---

## 4. Balance engine

### 4.1 `calcBalances(from)`

```
if in_sync: return
in_sync = true
start = 0; running = 0.0
if from != null and from in [startDate, endDate]:
    i = calIndex(from); if i > 0: start = i; running = budgetDay[i−1].closing_balance   (else start = 0)
for i in start .. len−1:
    day = budgetDay[i]; day.opening_balance = running
    for tx in day.transactions (sorted order, 1.9):
        if !tx.pmt.disabled: running += tx.amount
        tx.balance = running                       // disabled transactions show the unchanged running balance
    if day.closing_balance_adjusted:
        day.adjustment_diff = day.closing_balance − running; running = day.closing_balance
    else:
        day.closing_balance = running; day.adjustment_diff = 0.0
```

The opening balance of the first day of the window is 0.0; the "initial balance" is expressed as a balance adjustment. No rounding is applied; display rounding is `BudgetDay.getClosingBalance(false)` (1.9).

`getDay(cal)`: returns null when `calIndex == −1`; otherwise runs `calcBalances(null)` if out of sync and returns the day.

### 4.2 Window, indexing (`rebuildDB(cal, futureMonths, unlimitedHistory)`)

```
base = cal − 2 months (int b)
latestBefore = 0; earliest = b
for a in adjustments.opt: if a.date < b and a.date > latestBefore: latestBefore = a.date
                          if a.date < earliest: earliest = a.date
if latestBefore == 0:                       // no adjustment older than base
    latestBefore = b
    for p in payments: latestBefore = min(latestBefore, p.startDate, dates of p's non-deleted options)
if unlimitedHistory:
    for p in payments: earliest = min(earliest, p.startDate, dates of p's non-deleted options)
startDate = new SimpleCalendar(unlimitedHistory ? earliest : latestBefore)
if unlimitedHistory: startDate.set(MONTH, 0); startDate.set(DAY, 1)          // 1 January of that year
for p in payments with endMode == LOAN:
    ls = calcLoanSchedule(p, null)
    if startDate > ls.actualStartCal and startDate < ls.actualEndCal: startDate = ls.actualStartCal
startDate.set(DAY, 1)                                                        // first of the month
endDate = cal + futureMonths months, then MONTH = 11, DAY = last day        // 31 December of that year
startYear = startDate.year; nYears = endDate.year − startYear + 1
yOffset[k] = Σ days of years startYear .. startYear+k−1 ; budgetDay = new BudgetDay[Σ days of all nYears years]
startDateAsInt/endDateAsInt cached
for p in payments: p.calendarDays.clear(); update(p, false)
updateAdjustments(null)
```

So in the default mode the window starts on the 1st of the month of the most recent adjustment that is older than two months before the viewed month (balances before an adjustment are irrelevant), falling back to the earliest data when there is no such adjustment. The `budgetDay` array always starts at 1 January of `startYear` even though `startDateAsInt` may be later.

`calIndex(cal)`: −1 if `cal.get() < startDateAsInt` or `> endDateAsInt`; else `yOffset[year − startYear] + dayOfYear − 1`.

`setLeftLimit(int)`: ignores its argument; sets `startDate` to 1 January of its current year and refreshes `startDateAsInt` (the array already covers that range). No rebuild.

`isMonthAvailable(cal)`: `startDate.ym <= cal.ym <= endDate.ym` (`ym = year*100 + month0`).

### 4.3 `cutOffHistoryPrior(year)`

Purpose: discard history before 1 January `year` and replace it with one balance adjustment.

1. `calcBalances(startDate)` (no-op if in sync). `cut = 31 December (year−1)`; `day = getDay(cut)`; if null → return false. `balance = day.closing_balance`; `c = cut.get()`.
2. `adjustments.clearBefore(c)`, `adjustments.clearNotesBefore(c)` (options/flags/notes with date `< c`). For every payment: `clearBefore(c)`, `clearNotesBefore(c)`. Counts are printed to stderr.
3. For every payment `p`:
   * no options left, mode ≠ INDEFINITE and `endDate < c` → **remove the payment**;
   * mode == INDEFINITE → `n = nextRegularPaymentAfterDate(p, c)`; if −1 print an error and leave it, else `startDate = endDate = n`;
   * `endDate < c` (options remain) → `n_pays = 1; startDate = getFirstOptionAfter(c).date` (null dereference if the only remaining options are exactly on `c` — edge case, UNKNOWN whether reachable in practice);
   * `startDate >= c` → unchanged;
   * otherwise (schedule spans the cut, warning printed): LOAN → unchanged. Else `period = get_period_days(p)`; `n = nextRegularPaymentAfterDate(p, c − period days)`; if `n != −1`: `startDate = n; recalculate_n_pays(p)`. Else walk `d` backwards from `cut` while `getDay(d) != null`: if the day has a transaction of `p` and `isRegularPaymentOn(p, d)`: `adjustments.setCustomAmount(d, day.closing_balance); startDate = d; recalculate_n_pays(p)`; stop.
4. `updateAdjustment(cut, balance)`; return true.

The caller then saves the DB to a new file with no password.

---

## 5. Other `BudgetDB` APIs

### 5.1 Construction / loading

`load(path, password, log, cal, futureMonths, unlimited)`: `path == null` → empty DB (constructor exceptions swallowed → may return null). Otherwise `loadFileData`; on failure return null; else construct (exceptions during construction are appended to the log and the DB is still returned) and set `fileName`.

Constructor: `notes`/`version` from properties (defaults `""`/`"1"`), `rebuildDB(cal, futureMonths, unlimited)`, then name clean-up: for every payment, while `!isValidCategory(p.category, null)` append `"-"` to the category; then for every payment, while `!isValidDescription(p.description, null)` append `"-"` to the description. (Because `null` is passed as the excluded payment, a category equal to the payment's own description is also renamed.)

* `isValidCategory(cat, except)`: `null` → true; `"(Unaccounted)"` → false; equal to the `description` of any payment other than `except` → false; else true.
* `isValidDescription(desc, except)`: equal to the non-null `category` of any payment other than `except` → false; else true.

### 5.2 Collection management

* `add(p)`: `p.id = max(id of all payments) + 1` (1 when empty); append; `update(p)`; returns the id.
* `update(p)` → `update(p, true)` (3.4–3.9). Returns 0.
* `deletePayment(p)`: cancels the move-undo if it refers to `p`; `removeFromBudgetDays(p)`; remove from list; `updateCount++`; `in_sync = false`; returns 0.
* `get(i)`, `size()`, `getByID(id)` (linear search, null if absent), `isPaymentInDB(p)` (identity `contains`).
* `sort()`: `Collections.sort(payments, comparator)` with comparator: `description.compareToIgnoreCase`; ties → `startDate` ascending; ties → `endDate` ascending. The saved file order is whatever the in-memory order is (the loader keeps file order; `sort` is explicit).
* `getMostChangedPayment()`: the first payment with the maximum `opt.size()` (strict `>` scan), or null when empty.
* `getCategories()`: distinct non-null categories, sorted with `compareToIgnoreCase`.
* `getAllDeletedOptions(desc, from, to)`: `TreeSet` ordered by `date` only (so one entry per date) of `getDeleted(from, to)` over every payment whose `description.equals(desc)` (case-sensitive).
* `getPaymentsRemaining(p, asOf)` → `(abs_amount, n)`:
  * INDEFINITE → `(−1.0, −1)`;
  * N_PAYS → iterate `max(1, n_pays)` scheduled dates from `startDate`, `n` = count with `date >= asOf`; result `(n * p.amount, n)` (signed, despite the field name);
  * DATE → iterate from `startDate` stepping until `cal > endDate` (the date that overshoots is not counted), `n` = count with `date >= asOf`; result `(n * p.amount, n)`;
  * LOAN → `calcLoanSchedule(p, asOf)` → `(abs_amount_remaining, n_remaining)` (absolute value);
  * undefined → `(0.0, 0)`.
* `getNumberOfPlannedTransactions` — 3.3.

### 5.3 Day notes

Stored in `adjustments.note`: `getDayNote(cal)` = `adjustments.getNote(cal.get())`; `setDayNote(cal, text)` = `adjustments.setNote(cal.get(), text)` (null removes). Persisted on the `:` line as `<date>=<encoded text>`. Cleared by `cutOffHistoryPrior` for dates before the cut.

### 5.4 Move undo (`MoveUndoInfo`)

Single slot: `available, dst_existed, pmt, src_date, dst_date, src_note, dst_note, src_flag, dst_flag, src_amount, dst_amount, src_hilight (−1), dst_hilight (−1)`.

* `addMoveUndo(p, srcDate, srcAmount, dstDate, dstTransactionOrNull)` — called by the UI **before** `p.moveTransaction(dstDate, srcDate, amount)`: records `src_flag = p.getFlag(srcDate)`, `src_note = p.getNote(srcDate)`, `src_hilight = p.getHilighting(srcDate)`; if a destination transaction existed: `dst_existed = true`, `dst_amount = tx.amount`, `dst_note/dst_flag/dst_hilight` from `p` at `dstDate`.
* `doMoveUndo()`: if available: if `dst_existed` → `setCustomAmount(dst, dst_amount)`, `setFlag(dst, dst_flag)`, `setNote(dst, dst_note)` if non-null, `setHilighting(dst, dst_hilight)`; else `p.clear(dst)`. Then `setCustomAmount(src, src_amount)`, `setFlag(src, src_flag)`, `setNote(src, src_note)` if non-null, `setHilighting(src, src_hilight)`; `update(p)`; slot cleared. (The source is restored as a custom amount; if it equals the regular amount, 3.9 removes the option on that same `update`.)
* `hasMoveUndo()`, `updateMoveUndo(p)` (invalidate if the slot refers to `p`).

### 5.5 Misc

`getOldestAdjustmentDate`, `getLatestAdjustmentDate` (3.11). `copyFile(src, dst)` — NIO channel copy, returns whether all bytes were transferred. `getFileBytes(file)` — whole file or null.

---

## 6. `SimpleCalendar`

### 6.1 Representation

Fields `y`, `m` (0–11), `d` (1–31), `is_mutable`. `get()`/`yyyymmdd()` = `y*10000 + (m+1)*100 + d`. `getYm()` = `y*100 + m` (0-based month). Static `getYear(int)`, `getMonth(int)` (0-based), `getDay(int)`, `getYm(int)` first normalise the int through a shared instance (`set(int)` then `get()`), so e.g. `20071232` normalises to `20080101`.

Field constants: `DAY = DAY_OF_MONTH = 1`, `MONTH = 2`, `YEAR = 3`, `DAY_OF_YEAR = 4`, `WEEK = 5`, `DAY_OF_WEEK = 6`, `NTH_WEEKDAY = 7`, `LAST_WEEKDAY = 8`. Weekdays `SUNDAY = 0 … SATURDAY = 6`. `DATE_MIN = 1`, `DATE_MAX = 99991131` (sentinel; not a real date). Leap years: proleptic Gregorian (`%4 && (!%100 || %400)`), no lower bound. `getActualMaximum`: DAY → days in month, MONTH → 11, YEAR → 2400, DAY_OF_WEEK → 6, DAY_OF_YEAR → 365/366; other fields throw.

### 6.2 Constructors

* `()` today (local time zone). `(SimpleCalendar)` copy. `(long millis)`, `(Calendar)`, `(Date)` via `java.util.Calendar`.
* `(int v)`: if `v < 10000` → `v = v*10000 + 101` (year → 1 January); else if `v < 1000000` → `v = v*100 + 1` (yyyymm → day 1); then `set(v)`.
* `(y, m0, d)`.
* `getImmutable(c)`: copy with `is_mutable = false`; mutators throw `UnsupportedOperationException`. `getToday()` returns a shared immutable instance refreshed at most once per 60 s; `now()` = its int.

### 6.3 Setting (normalisation rules)

* `set(int v)` = `set(v/10000, (v/100)%100 − 1, v%100)`.
* `set(y, m, d)`: `this.y = y`; if `0 <= m <= 11`: `this.m = m`, and if `1 <= d <= 28` `this.d = d` else `set(DAY, d)`; otherwise `set(MONTH, m)` then `set(DAY, d)`.
* `set(YEAR, v)`: `y = v`; clamp `d` to the month length.
* `set(MONTH, v)`: `m = v`; if `m < 0`: `y += (m+1)/12 − 1` (Java truncating division), `m %= 12`, if `m < 0` `m += 12`; else if `m > 11`: `y += m/12; m %= 12`. Then clamp `d` to the month length. (−1 → previous December, −12 → same month previous year, 12 → January next year.)
* `set(DAY, v)`: `d = v`; while `d < 1`: step to the previous month (decrement `y` when wrapping from January) and `d += daysInMonth(new m)`. If `d > 28`: while `d > daysInMonth(m)`: `d −= daysInMonth(m)` and step to the next month (increment `y` when wrapping from December). Overflow therefore rolls across months/years exactly.
* `set(DAY_OF_WEEK, w)`: `add(DAY, w − dayOfWeek)` (move within the current Sun–Sat week).
* `set(NTH_WEEKDAY, targetMonth)`: `w = dayOfWeek(); n = getWeekOfMonth() = (d−1)/7`; `d = 1; m = targetMonth; normalise month`; `w1 = dayOfWeek of the 1st`; `d = (w1 <= w) ? 1 + w − w1 : 8 − w1 + w` (first `w` of the month); `d += 7*n`; then **clamp** `d` to the month length (a non-existent 5th weekday lands on the last day of the month, whatever weekday that is).
* `set(LAST_WEEKDAY, targetMonth)`: `w = dayOfWeek(); d = 1; m = targetMonth; normalise; d = daysInMonth; wl = dayOfWeek of that last day; d −= (wl >= w) ? (wl − w) : (7 − (w − wl))`.
* `setDayOne()` (d = 1), `setDayLast()` (d = month length). `set(SimpleCalendar)` copies via int.

### 6.4 Arithmetic

* `add(DAY, n)` → `set(DAY, d+n)`; `add(WEEK, n)` → `set(DAY, d+7n)`; `add(MONTH, n)` → `set(MONTH, m+n)`; `add(YEAR, n)` → `set(YEAR, y+n)`; `add(NTH_WEEKDAY, n)` → `set(NTH_WEEKDAY, m+n)`; `add(LAST_WEEKDAY, n)` → `set(LAST_WEEKDAY, m+n)`; `DAY_OF_YEAR`/`DAY_OF_WEEK` throw. Returns `this`.
* `nextDay()`: `d < 28 ? d++ : add(DAY, 1)`; `prevDay()`: `d > 1 ? d−− : add(DAY, −1)`; `addDays(n)`, `addMonths(n)`, `nextMonth()`, `prevMonth()`.
* `getDayOfYear()` = `d + DOY[m]` (DOY = {0,31,59,90,120,151,181,212,243,273,304,334}; leap variant {0,31,60,91,121,152,182,213,244,274,305,335}). Static `getDayOfYear(int)` likewise.
* `getDayOfWeek()` = `((y−1) + (y−1)/4 − (y−1)/100 + (y−1)/400 + dayOfYear) % 7`, 0 = Sunday (Zeller-style; 2000-01-01 → 6 = Saturday). Static `getDayOfWeek(int)` likewise (no normalisation).
* `getWeekOfMonth()` = `(d − 1) / 7` (0-based). `isWeekend()` = Sat or Sun. `isLastWeekday()` = `d > daysInMonth − 7`.
* `getDiff(a, b)` = number of days from `a` to `b` (positive when `a <= b`, negative otherwise): same year → `doy(b) − doy(a)`; else `(daysInYear(a.y) − doy(a)) + doy(b) + Σ daysInYear(y) for a.y < y < b.y`. Overloads `getDiff(SimpleCalendar, int)`, `getDiff(int, int)` use shared scratch instances (synchronised).
* `getMonthsDiff(a, b)` = `(b.y*12 + b.m) − (a.y*12 + a.m)`.
* `getYMdurationString(months)`: `"<y>y "` if `months > 11`, then `"<m>m"` if the remainder is non-zero (e.g. 14 → `"1y 2m"`, 12 → `"1y "`, 0 → `""`).

### 6.5 Comparison

`lt/gt` compare `y*10000 + m*100 + d` (0-based month; order-equivalent); `le/ge` = equals or lt/gt; `lt(int)` compares `get()` with the normalised int; `eq`/`equals` compare `y, m, d`; `hashCode` = `get()`; `compareTo` = `get() − other.get()`. `inRange(a, b)` = `a <= this <= b` inclusive. `inRange(a, n)` = `this ∈ [a, a + n − 1 days]`.

### 6.6 Formatting

`toString()` = `%04d-%02d-%02d` (1-based month); static `toString(int)` same after normalisation. `toString(pattern)` = `SimpleDateFormat(pattern)` on a `Date` at 12:00:00.000 local time, then every `'.'` removed (`removeDots`, for locales that abbreviate months with a dot). Pattern constants: `FORMAT_dd_MMM_yyyy = "dd-MMM-yyyy"`, `FORMAT_yyyy_MMM_dd = "yyyy-MMM-dd"`, `FORMAT_MMM_yyyy = "MMM, yyyy"`. `getTime()` returns that noon `Date`.

---

## 7. UNKNOWN / not determinable from the files in scope

1. `Payment.DATA_FILE_NUM_LINES` (99990103) and `DATA_FILE_EOD` (99990104): declared, never referenced in any pseudo file; purpose unknown (possibly reserved for a planned format change).
2. `BudgetDB.getLatestAdjustmentDate` skips adjustment options in year 9999, but nothing in scope writes such options; the originating feature is unknown.
3. `cutOffHistoryPrior`: whether the `getFirstOptionAfter(c)` null case (only remaining options exactly on 31 Dec) is reachable through the UI.
4. `calcLoanSchedule` with `amount == 0` and an unpaid loan does not terminate; whether the payment editor forbids this is outside scope (`Budget` UI classes).
5. `Blowfish` key schedule / round function were read only to the extent needed for the file format (56-byte key cap, zero padding, ECB, big-endian words, lowercase hex); the P/S constants beyond `PA_Init[0]` were not individually verified against the reference tables.
6. Platform default charset used by `String.getBytes()` / `new String(byte[])` in the encryption path (depends on the JVM: UTF-8 on Java 18+, otherwise the Windows ANSI code page); affects only passwords/data with non-ASCII bytes.
7. `Budget.remove_new_lines` (used by `getOneLineNote`) — implementation not in scope; presumably replaces line breaks with spaces.

Verified pass: 2026-10-27 — 1 statements corrected, 1 confirmed.
