# Budget Calendar (MiShell, v1.11.4424) — Reverse-Engineering Spec for a Rust Port

This document is the entry point. It summarises what the application is, how it was analysed, every feature it has, and how to structure a Rust re-implementation. The exact behavioural detail lives in the ten documents under `analysis/`; this file tells you which one to open.

## 1. Source material and method

| Item | Location |
|---|---|
| Installed app | `C:\Program Files\MiShellSoft\Budget` (jpackage launcher `Budget.exe` + bundled JRE 25.0.2 + `app\Budget.jar` + `app\swt.jar`) |
| Main code | `Budget.jar` → package `ca.mishell.app.budget` (Budget, BudgetDB, BudgetDay, Payment, PaymentOption, PaymentFlag, Note, Transaction, Filter, BudgetConfig) plus helpers in `ca.mishell.misc` and `ca.mishell.ui`; 389 class files, 1.24 MB |
| UI toolkit | Eclipse SWT (native widgets); single `Budget` class of ~250 KB bytecode holds almost all UI logic in 183 anonymous listener classes |
| Resources | `app\images\*.gif|png` (156 icons, 18x18) + `app\images\image.tags` (word→icon quick-pick map), `app\icons\*`, `app\doc\budget.html` (user manual, 2 screenshots), `app\sample.bgt`, `app\license.txt` |
| Runtime | No Java installed on this machine and policy blocked running a downloaded decompiler, so the bytecode was disassembled with a purpose-written Python class-file reader (`reference/jclass.py`) and converted to Java-like pseudo code by a stack-simulating script (`reference/jpseudo.py`). Output: `reference/pseudo/*.txt` (the 500 KB `Budget` class is split into `part01..part06`). REA was used for the archive inventory (Evidence `ev_817df8…`), its native-UI capture is macOS-only so no runtime observation was done. |
| Copies for reference | `reference/sample.bgt`, `reference/image.tags`, `reference/budget.html` |

Pseudo-code conventions (needed if you open `reference/pseudo`): labels `Lnn:` + `goto`; `if (c) goto L;` branches; `p0..` parameters, `vN` locals, `$sN` cross-branch temporaries, `$tN` named call results, `$aN` arrays; boolean `x == 0` means false. The generator is not a decompiler: loops appear as if/goto and some inferred field types are missing, but every constant, string, call and branch is exact.

Legal note: the vendor licence (`app\license.txt`) forbids decompiling. The analysis was done on the user's own installation for the user's own re-implementation; the outcome is a behavioural description, not vendor code.

## 2. What the application is

A personal cash-flow forecasting tool. The user defines recurring **payments** (income or expense, a schedule, an end rule). The engine expands each payment into dated **transactions** over a window of months, applies per-date overrides (custom amounts, deletions, moves, notes, flags, colours), applies absolute **balance adjustments** (reconciliations), and computes a running balance for every day. The main window shows one month as a 7-column calendar: each day cell lists its transactions with icon, description and amount, and a bottom bar with the day's closing balance (pink = month maximum, blue = month minimum). Above the calendar: quick stats for the month, a forecast bar graph for the next N months, a Brief Summary table of categories/payments (planned vs actual), a text filter and notes.

Everything is stored in a single text file `*.bgt` (optionally Blowfish-encrypted). User preferences go to `~/.budgetrc` (Java `Properties` format). Reports are HTML files opened in the browser or in Excel.

## 3. Feature inventory

Each line gives the feature and the analysis file + section that specifies it exactly.

### 3.1 Data and engine (`analysis/01-core-data-model-and-engine.md`)
- Payment model: amount sign = income/expense, description with `;` short form, category, start date, frequency `N` × {day, week, month, year, "Nth weekday of month", "last weekday of month"}, end mode {indefinite, on date, # of pays, until loan amount paid}, grace days, flags {Business Days Only, Early Pay, Tentative, Sticky, Reminder}, disabled, default flag char (none/paid/important), icon, URL, default highlight colour. §1
- Per-date overrides: custom amount, deleted marker (restorable), moved transaction (= delete + custom), per-date flag (`^` none, `;` paid, `#` important), per-date note, per-date highlight colour. §1.4–1.8
- `.bgt` file format: `v,2` header, `:` adjustments/day-notes line, `=,` notes line, one line per payment, option tokens, `## END-OF-DATA; line_no=N ##` trailer, `&#xHHHH;` escaping, ISO-8859-1, backup rotation, worked decode of `sample.bgt`. §2
- Encryption: `#db*encrypted` header + Blowfish-ECB per line, hex encoded, zero-padded, 56-byte key cap; legacy `#db*Encrypted` XOR read-only. §2.8
- Scheduler: stepping primitive `addPeriod` (anchored for monthly/yearly on day > 27 and for Nth-weekday), end-mode synchronisation of `n_pays`/`endDate`, loan pay-off schedule (exact loop, no rounding), weekend shift (+1/+2 or −1/−2 days, `'` suffix), tentative slide-to-today with pay-period logic, sticky amounts, option application pass with two silent clean-ups, `*` last-payment suffix. §3
- Balance engine: window computation (2 months back from latest adjustment, future N months to 31 Dec), per-day opening/closing, disabled payments excluded, adjustment diff, display rounding `(int)(x ± 0.05f)`. §4
- Other DB API: ids, sort order, category/description collision rules, categories list, deleted-options set, payments remaining, planned-transaction counts, single-level move undo, day notes (stored in the adjustments payment's notes), cut-off-history (CLI `-C`; not ported, see §6). §5
- `SimpleCalendar`: `yyyymmdd` int dates, normalisation, Nth/last weekday arithmetic, diffs, formats. §6

### 3.2 Filter language and formula calculator (`analysis/02-filter-and-calculator.md`)
- Filter box (calendar view and All Payments window): `-` deleted-only, `|` OR mode, `~` negate, `cat:`, `desc:`, `date:`, `note:`, `range:{n|active|past|future|disabled|indefinite}`, `flag:{paid|.|unpaid|?|important|!}`, `<n` `=n` `>n`, `\ ` escaped space; what each matches against. §1
- Calculator used in amount/balance prompts: `+ - * / ( )`, `$` ignored, `k/m/b/t` suffixes, `%n` = n % of week income, `%%n` = n % of month income, error texts. §2
- Dead code confirmed: `Expression`, `StockPrice`, `JSONObject`, `SimpleDoubleParser`, `JarVerifier`, `Platform`, `HunspellSpeller`, `QuickPickDialog`, `NotifierDialog`, `Prompt`, `TableKeyNavigation` (library classes never referenced from Budget).

### 3.3 Startup, configuration, graph, main window (`analysis/03-startup-config-graph-mainwindow.md`)
- CLI options (`-r -d -D -z -B -Z{0|1|2} -c rcfile -C year,name --version`; the port replaces them, see §6), crash log, single-instance TCP guard on port 57778. §2–3
- Startup sequence, password prompt loop, recent files, window placement, 20 s date-change timer, event loop flags. §3
- `.budgetrc` keys, defaults, encodings. §4 (full table in doc 09)
- Registration/trial: key lookup order, exact 12-char key checksum algorithm, 30/60-day nag with 20 s countdown. §5
- Forecast graph: 8 types (open/close/avg/income/expenses/diff/min/max balance), data per month, best-scale algorithm with `stepList`, bar drawing, frozen vs dynamic scaling, custom min/max, click-to-switch, bar click → month. §9
- Main window layout tree, Quick Stats pane (12 month links, arrows, 8 stat rows), fold/unfold details bar, keyboard (Home/End/PgUp/PgDn), wheel with "bump" edge logic, Ctrl+wheel font zoom. §10–11
- Six-month average algorithm (suggested amount tooltip), quick-pick icon suggestion from `image.tags` words. §12

### 3.4 Dialogs, menus, editing (`analysis/04-dialogs-menus-editing.md`)
- New/Edit Payment dialog: every control, validation order and messages, Ok algorithm, "Payment's Amount Changed" choice (change all / future only), schedule-change confirmation. §1
- Alter transactions from a date (`splitPayment`): original payment truncated to the day before, clone starts at the split date, self-check that balances match. §18
- Notes editors (budget-wide, per-transaction), image chooser with zoom, date picker, generic prompt (return protocol), duplicate payment / duplicate transaction. §3–9
- List of All Payments window: columns, sort comparators, filter, context menu (Edit, Add, Duplicate, Delete, wild transaction, All/Matched transactions, Edit/Delete Category, Filter by description/category), colour legend, range links. §10
- New budget wizard (starting balance prompt, hint dialog), save/save-as/autosave. §11–12
- Full File/Edit/Help menu trees. §13
- Balance adjustment prompt (formulas, "put X to Savings"/"borrow" hints), max-savings algorithm (`get_best_saving`, `truncate_to_10`, `roundUp`). §15–16
- Amount adjustment prompt (`%n`/`%%n`, sign sanity prompts, sum-or-replace when a duplicate exists). §17
- Highlight colour table (11 named colours + custom). §20

### 3.5 Calendar view and statistics (`analysis/05-calendar-view-and-stats.md`)
- Month grid construction: leading/trailing cells, per-cell colours (past/today/future), day-number marks (`` ` `` restorable, ` ...` note), transaction rows (icon, description with best-fit truncation, amount with `'`/`*` suffix, paid/important/highlight styles, disabled greyed), balance bar rules (`BALANCE_NONE/ALL/FIRST_AND_LAST`, min/max/below-min colours, adjustment markers), tooltips, hover highlighting of grace-period days. §1
- Transaction context menu (16 items, enable rules). §2
- Wild transactions ("Add a transaction of…" picker, OVERWRITE / SUM / ADD-another choices). §5
- Drag-and-drop between days incl. floating prev/next-month drop shells; Copy vs Move semantics. §6
- Statistics engine: planned vs actual per category/payment, Disposable Income, Uncategorized-Income/Expenses, Unaccounted (adjustments), 5 %-of-income brief threshold, ordering. §12
- Brief Summary pane rendering, click-to-filter, hover previews. §13
- Registration key validation (exact). §11

### 3.6 Stats window and reports (`analysis/06-stats-window-and-reports.md`)
- "Income vs. Expenses" window: From/To month menus, Last 30/365 days, 6-column table, click → per-payment transactions popup. §3
- Reports (all HTML, exact skeletons quoted): Print (stats), Excel stats report (with `FORMULA=`/`LOTUS_FORMAT` attributes), Comparative report of two ranges, Month-by-Month report, Monthly Detailed report, Monthly Excel report, Unpaid & paid report, printed monthly calendar (`-r` and "Print this month"). §4–10
- How reports are opened: temp `bgt-report-*.html`, browser, or Excel via `.xls/.xlsx` association. §1.12
- Inclusion-rule summary (disabled/deleted/future/adjustments). §11

### 3.7 Listener-by-listener behaviour (`analysis/07-listeners-part1.md`, `analysis/08-listeners-part2-and-helpers.md`)
- Master tables mapping every one of the 183 anonymous listener classes to its widget/menu item and exact effect (prompts verbatim, DB calls, rebuild triggers). Use these when a UI action's precise behaviour is needed.
- Helper classes: `BestFit` (cell description truncation), `Bump` (wheel edge logic), `Stats`, `comp_entry`, `month_by_month_entry`, etc. (08 Part I)

### 3.8 Preferences and configuration (`analysis/09-preferences-config-cli.md`)
- Complete preference table: 13 booleans, 4 ints, 1 double, 2 strings (Controls), 11 fonts, 31 + 4 colours with LIGHT and DARK theme RGB values, Advanced (single instance, port), plus non-UI fields (window geometry, graph presets, recent files, regkey, licAgree…). §2
- Config framework serialisation rules and the Preferences dialog behaviour. §1, §3–4
- CLI parser semantics. §5

### 3.9 Shared UI helpers (`analysis/10-shared-ui-helpers.md`)
- Message boxes, prompt, date picker, notes editor, image chooser, choice dialog, colour/font formulas, fade in/out, the mini-HTML subset used by the stats pane. Budget mostly carries private copies of these dialogs; the doc says which library versions are actually used.

## 4. Data model summary (for the Rust core)

```
Payment { id, amount: f64, description, category: Option<String>, start_date: Date, frequency: i32,
          freq_type: 'd'|'w'|'m'|'y'|'W'|'L', n_pays, end_date, flags: u32, loan_amount, grace_period_days,
          image_path, disabled, flag_value: '^'|';'|'#',
          opt: Vec<PaymentOption{date, amount, deleted}>,          // one per date
          flag: Vec<PaymentFlag{date, value}>, note: Vec<Note{date, text}>,
          hilighting: Vec<Hilight{date, rgb}> }                    // sentinel dates 99990101 (default colour), 99990102 (URL)
flags: NO_WEEKENDS=1 TENTATIVE=2 EARLY_PAY=4 REMINDER=8 END_MODE_MASK=0xF0 {INDEFINITE=16 DATE=32 N_PAYS=48 LOAN_PAID=64} IS_STICKY=256
BudgetDB { payments, adjustments: Payment (opt = balance adjustments, note = day notes), days: Vec<BudgetDay>, window start/end, notes, version }
BudgetDay { opening, closing, adjustment_diff, adjusted, transactions: Vec<Transaction{pmt, amount, balance, suffix}>, restorable, regulars }
Date = i32 yyyymmdd
```

File format in one line per payment (saver): `amount[%loan][!][flag], desc, yyyymmdd, Nf[;grace], n_pays, yyyymmdd, flags, image[, $category][, options…]` where options are `date` (deleted), `date:amount`, `date^|;|#`, `date=note`, `date&rrggbb`. See 01 §2 for the loader's exact tolerance rules.

## 5. Recommended Rust architecture

Two crates in one workspace:

1. `budget-core` (no GUI dependency; unit-testable):
   - `date.rs` — port of `SimpleCalendar` on `i32` dates (do not use chrono for the model; the app's own normalisation rules matter: day overflow, Nth/last weekday, `DATE_MIN/MAX`).
   - `model.rs` — structs above, flag helpers, option/flag/note/highlight accessors with the exact overwrite/remove semantics (01 §1.4–1.8).
   - `bgt.rs` — loader/saver. Tolerant tokenizer (`,`-split, trim, skip empties), `&#xHHHH;` codec, Java `Double.toString`-compatible number writer (`-80.0`, `1.0E7`), END-OF-DATA line count, backup naming, byte-identical-skip rule.
   - `crypto.rs` — Blowfish-ECB via the `blowfish` crate (`cipher` traits), hex, zero padding, truncate at first NUL, legacy XOR reader.
   - `schedule.rs` — `add_period`, end-mode sync, loan schedule, `update(payment)` placement loop incl. weekend shift, tentative, sticky, options pass.
   - `balance.rs` — window computation, `calc_balances`, adjustments, display rounding.
   - `filter.rs`, `calc.rs` — filter language and calculator.
   - `stats.rs` — statistics engine (05 §12) and six-month average (03 §12.5), max-savings (04 §16).
   - `report.rs` — HTML generators (06), string-for-string where Excel compatibility matters.
   - `config.rs` — `.budgetrc` reader/writer: Java `Properties` syntax (`key=value`, `\` escapes, `\uXXXX`, `#`/`!` comments) with the key table from 09 §2 so an existing user file keeps working.
2. `budget-gui` — desktop UI. Candidates: `egui/eframe` (fastest to get the custom-drawn calendar, graph and stats table right; immediate-mode fits the "rebuild the whole view on change" design of the original), `iced`, or `slint`. The original uses native SWT widgets but almost every cell is custom-painted labels, so a canvas-oriented toolkit loses nothing. Keep a `ViewModel` layer that mirrors `Budget`'s fields (`cur_cal`, `min_date`/`max_date`, filter, graph type, show-balance mode) so the listener tables in docs 07/08 map 1:1 to handlers.

Crates to use: `blowfish` + `cipher` (encryption), `open`/`opener` (launch reports), `dirs` (home/Documents), `rfd` (file dialogs), `image` (gif/png icons). Do not use serde for `.bgt`; the format is positional and tolerant.

Suggested implementation order: date → model → bgt load/save round trip of `reference/sample.bgt` → schedule + balances (verified: the manual screenshot `app\doc\image002.jpg` is `sample.bgt` in December 2021; `budget-core` reproduces its transaction layout and totals, income 3,000.00 / expenses −3,030.00. The screenshot's running balances come from a different adjustment state and are not reproducible from the shipped file) → filter/calculator → stats → calendar GUI → dialogs → graph → reports → preferences → encryption/registration (registration can be dropped; keep the key validator only if `.budgetrc` compatibility with `regkey` matters).

## 6. Behaviour fidelity: quirks to decide on

The analyses mark these as verified behaviour of the original. Decide per item whether to reproduce or fix; they affect file compatibility or visible output.
- A custom amount equal to the computed regular amount is silently dropped on the next rebuild and not saved; a stale deletion marker likewise (01 §3.9).
- Tentative payments mutate the option list during rebuild (01 §3.7); `today` for tentative logic is frozen at process start.
- `.bak` backup is deleted after a successful save, so it only survives a failed write (01 §2.9).
- `date:` filter matches the payment's start date in `dd-MMM-yyyy` form, not the occurrence date (02 §1.3); `_` is not a space escape despite the manual.
- `getDesc` truncation removes only 3 characters then appends `...` (04 §18) — effectively never shortens.
- Monthly Detailed report excludes the "To" month while the printed calendar includes it; the HTML Totals expenses cell prints the income total; some tags unclosed (06).
- `saveNewBudget` overwrite confirmation is inverted: answering Yes to "Do you want to overwrite it?" cancels the save (04 §12; verified in regenerated pseudo code: `if (confirmYesNo(...) == false) goto save; return false`).
- Flag/note/highlight changes from the context menu do not call `db.update`, so they are not reflected until the next rebuild/save (07 note).
- Balance display rounding uses `0.05f` widened to double (01 §1.9).

Deliberate deviations already taken by the port:
- The original command line (`-r -d -D -z -B -Z{0|1|2} -c rcfile`) and the EXPERIMENTAL `-C year,name` cut-off-history mode (01 §4.3, 03 §7) are not ported. `budget-core` has no cut-off function; the GUI accepts only its own options (`[file] --month --screenshot --size --dark --today --filter --version --version-json`, docs/ux.md). Revisit only if a user asks to truncate old history.
- Business-days payments whose Saturday and Sunday occurrences land on the same business day as another occurrence of the same payment (daily or every-2-days repeats) show one transaction carrying the sum of those occurrences. The original kept only the last one while its loan schedule and planned totals counted all of them, so loans looked paid off with principal unscheduled and "vs plan" showed phantom differences.
- Amount fields (payment, loan, transaction, balance) evaluate calculator formulas but refuse `%n`/`%%n` income percentages, since no income context is passed; the original's payment dialog evaluated them against zero income. Formula results that overflow are errors, not infinity.
- Editing a payment from a calendar cell (04 §1.3 step 16, §2): the port offers the "from <date> onward" / "all" choice (future preselected) for schedule changes too, where the original only showed its "ALL ... WILL BE CHANGED" warning, and for amount changes from the payment's first occurrence when it has no earlier transactions it applies to all without asking (the original asked unless `cal == startDate` and no options existed). Opened from the Payments page (no cell) it shows a confirming notice when the amount or schedule of a payment that started before today changes. "From <date> onward" follows the edited schedule from the date, counting a number of payments from the series' start and continuing a loan with its unpaid principal; manually adjusted transactions keep their amounts, as the dialog's footnote says.
- `splitPayment` (04 §18, 07 §1), used by that choice and by the transaction editor's "This and all future occurrences": the clone receives the options, flags, notes and highlights dated on or after the split (the original's `clearAfter` dropped them, so later deleted occurrences reappeared and moved ones vanished), and the transaction editor's split gives later amount exceptions the new amount (the original's drop had that effect). The original record keeps a count limit as a smaller count instead of becoming dated; the clone keeps the remaining count, the end date or the loan with its unpaid principal (the original gave it a dated end, so a loan's payoff no longer followed the new amount). A split on a shortened occurrence (day 29–31 in a shorter month, or a fifth weekday falling back to the fourth) starts the clone at the next occurrence that keeps the anchor day and stores the ones before as dated amounts, where the original's clone repeated the shorter day. A weekend-shifted occurrence splits at its nominal date, a tentative one at its pay period. The original's self-check (balance before the split) became a comparison of every earlier transaction, plus unchanged later dates for the transaction editor; a mismatch refuses the change without touching the budget instead of showing a bug report and exiting.
- Editors accept dates in years 1000–9999 only (the calendar's range); the loader still clamps stored dates to `0..=99991231`, so the editors refuse other years with a year-specific message rather than storing a date the next load would change.
- Scheduling never changes a payment's stored options or flags. The original's in-pass clean-ups (a custom amount equal to the computed one and a stale deletion marker dropped by the options pass, 01 §3.9; a tentative period's deleted markers cleared and re-created at its target, 01 §3.7) still shape each placement, applied to a working copy, but are not kept, so loading, editing and saving never drops or adds a dated exception by itself.
- A loan installment counts as changed only when an option sits on the date where it is displayed (the weekend-shifted business day). The original also matched the nominal weekend date, so an option there skipped an installment that was still placed, overpaying the loan.
- `.bgt` text fields round-trip exactly: besides `,` `\n` `\r` and chars >= 128, the saver escapes C0 control characters other than tab (a NUL truncated an encrypted line), leading/trailing spaces and tabs (the loader trims every token) and an `&` that starts an `&#x????;` window (the decoder would consume it). All escapes are `&#xhhhh;`, which the original also decodes; files without such text save byte-identically.
- Loading: lone-CR line endings are line terminators like `\r\n`/`\n` (the original's `readLine`), in `.bgt` and `.budgetrc`; start/end dates outside `0..=99991231` are clamped and logged like the frequency clamp, and an option date outside that range is an invalid option, so corrupt values cannot overflow the calendar; a highlight colour is kept to 24-bit RGB (`-1` still removes) so a negative value cannot be saved as an unreadable 8-digit hex; Blowfish-decrypted lines are read as ISO-8859-1 like plain files (the legacy `0xFF`/`0xFE` comma/newline bytes survive).
- A UTF-8 byte-order mark at the start of a `.bgt` (Notepad writes one) is skipped before the header check; the original reported the first line as invalid.
- Saving refuses a NaN or infinite amount (payment, loan, option or balance adjustment) with an error before anything is written, because the port's loader refuses such values; the original wrote them.
- An option date must be exactly eight digits and a real calendar day: `20260230` or `2026030 :5` is an invalid option (the payment line is reported like any other bad field), where the original normalised the date silently.
- The calculator and the number reader take a comma as a thousands separator only in a well-formed group (1-3 digits, then groups of exactly three). The original's `DecimalFormat` accepted any grouping, so `12,50` read as 1250; now `12,50+3` is a syntax error. Filter values such as `>12,50` read as 12.
- `.budgetrc`: characters above U+FFFF are written as a `\uXXXX` surrogate pair (as Java does) and recombined on load; a final line ending in a continuation backslash keeps its entry; a malformed `\u` escape drops only that entry (its default stays) where Java discarded the whole file.
- `.budgetrc` is replaced through a synced temporary file and a rename, so a failed or interrupted save keeps the previous settings. The original's `.bak` copy is not made: the rename makes it unnecessary, and the copy could overwrite and then delete the user's own `.budgetrc.bak`.
- The formula calculator rejects more than 64 nested parentheses ("Expression too deeply nested"); the original recursed until a stack overflow, which in the port would abort the process.
- Reports: Monthly Detailed and Monthly Excel include the To month (the GUI passes the month after To to the generator, whose loop stops before its end month), and the HTML Totals row shows the expenses total in the Expenses cell instead of repeating income. Other report quirks (unclosed tags, formula row offsets) are kept.
- Backups: the original deleted `.bak` after a successful save. The port keeps the previous version as `.bak`, `.bak.1`, … with numbering that continues upward. It logs each backup it makes in `<budget file>.baklog` beside the budget, and after each successful save removes all but the newest 10 logged backups that still exist (`bgt::MAX_BACKUPS`). A backup made by hand, any other look-alike file and backups from earlier builds (not logged) are never removed. On Windows backup names are matched ignoring case, like the file system, so every spelling of a budget's path shares one numbering and cap.
- The schedule window ends no later than 31 December 9999 (`date::DATE_MAX`); the original computed December of the year `futureMonths` ahead even when that passed the calendar.
- Restoring a deleted tentative occurrence clears the deletion anywhere in its pay period: the port's scheduler shows the period's deletion on today without moving the stored date, where the original moved the stored marker itself (01 §3.7).

## 7. Consolidated unknowns

Items no document could settle from static reading (each doc has its own list; the significant ones):
- Exact default-locale month abbreviations used by `dd-MMM-yyyy` (affects `date:` filter and reports); assume English.
- SWT `FontData.toString()` layout for font preferences (platform-specific string; copy from an existing `.budgetrc` if compatibility is needed).
- Whether modern Excel still honours `FORMULA=`/`LOTUS_FORMAT=` attributes in HTML (reports were designed for old Excel).
- Five legacy `.budgetrc` keys are read and ignored (`graphPastBarColor`, `graphExtraMonthBarColor`, `graphMinBarColor`, `graphAvgLineColor`, `detailsStatsOnDesc`).
- Runtime-only behaviour (fade timings, exact pixel layout under different DPI) was not observed; values in the docs are the constants from the code.

## 8. How to re-generate the pseudo code

```
python -I reference/jpseudo.py <dir with extracted .class files> <output dir>
```
`reference/jclass.py` is the class-file reader (`--sig` for signatures only). Both are self-contained Python 3, no dependencies.
