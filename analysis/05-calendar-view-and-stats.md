# 05 – Calendar view, transaction menu, drag-and-drop, HTML output, key validation, statistics

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part05.txt` (plus the listener classes `Budget$N` in part01/part02 and the `lambda$createCalendarView$N` bodies at the end of part06 that it installs).

Conventions used below
- Dates are `int` in `yyyymmdd` form (`SimpleCalendar.get()`); `ym` = `date / 100`. `SimpleCalendar` field ids: 1 = day of month, 2 = month, 3 = year, 6 = day of week (0 = Sunday).
- `today` = `initCal` (the date the app was started).
- Number formats: `numFormatFrac2` = grouping, exactly 2 decimals; `numFormatFrac0` = grouping, 0 decimals (rounds). "CV format" below means `showDecimalsInCV ? Frac2 : Frac0`.
- `isNegative(x, decimals)`: with decimals → `x < -0.01`; without → `(long)x < 0` (so -0.7 is NOT negative without decimals).
- Transaction flag chars (`Payment.getFlag(date)`, default `flag_value = '^'`): `';'` = paid, `'#'` = important, `'^'` = none.
- Transaction suffix chars (set by `BudgetDB.update`): `'*'` = last payment of a finite schedule (last of n_pays / end-date / loan, with no custom options after it), `'\''` = custom (adjusted) amount on that day, `0` = plain. In "deleted" display mode restorable items are synthesised with suffix `'\''`.
- Payment "description" may contain `;`: `getDesc()` returns the part before `;` (short name shown in the calendar) and truncates to `calendarDescriptionMaxLen` with `...` if that is set (default -1 = no limit). Tooltips show the part after `;` when present.
- Config field names vs. their Preferences labels are **swapped** for two cell colours: `cfg.bgColorCell` is labelled "BKG cell past" and `cfg.bgColorCellPast` is labelled "BKG cell future". Below I use the field names and say what they are used for.
- SWT colour `getSystemColor(37)` = COLOR_TRANSPARENT (inherit the parent's background); `7` = yellow; `3` = red.
- Command-line flags that affect this file: `-d` → `use_dnd = false`; `-z` → `dont_show_zero_amount`; `-U` → `opt_show_url`; `-Z{0|1|2}` → `opt_show_balance` (`BALANCE_NONE=0, BALANCE_ALL=1, BALANCE_FIRST_AND_LAST=2`) and `opt_show_balance_supplied = true`. Default `opt_show_balance = BALANCE_ALL` but *not supplied*.

---

## 1. `createCalendarView(scroll, container, cal)` – the month grid

Called from `updateCalendarView()` after it disposed all previous children, drag sources, drop targets and `tempWidgets`. Returns `null`. `container` has a 7-column grid layout (built elsewhere).

### 1.1 Filter banner (above the calendar)
If `calViewFilter != null`:
- `filterInfoComposite` is (re)created in the scroll's parent, background = system yellow, placed above the scroll. Inside, a centred 4-column row (horizontal spacing 15):
  1. icon label: `icons/red-cross.png` (fallback `icons/cross.gif`), tooltip `Clear transaction filter`; mouseDown (`Budget$160`): `setFilterText("")`, `calViewFilter = null`, `updateCalendarView()`.
  2. label, `fontMonth`, red foreground, text `Filter: '<filter>'` or, if the filter is a "deleted" filter, `Filtering DELETED transactions: '<filter>'`.
  3. `filterAllTransactionsMenu` (pop-up) is filled by `fillAllMatchingTransactionMenu` (§7). If it returned 0 items: a plain label `no matching transactions found.`. Otherwise two links in `colorCurrentMonth`:
     - `<a>Show in All Payments Window...</a>` (`Budget$162`): temporarily sets `allPaymentsSavedFilter = calViewFilter.filter`, opens `showPayments(mainShell)`, updates the view if it returns true, restores the saved filter.
     - `<a>All Matching Transactions...</a>` (`Budget$163`): shows `filterAllTransactionsMenu`.
Else: dispose `filterInfoComposite` and `filterAllTransactionsMenu` if present.

### 1.2 Week-day header row
7 centred labels, text = full weekday name `wDayName[(firstDayOfWeek + i) % 7]` (`firstDayOfWeek` = 0 if `cfg.firstDayOfWeekIsSunday` else 1), fg `colorCalendarHeader`, bg `bgColorCalendarHeader`, font `fontDate`, fill-horizontal.

### 1.3 Leading cells (previous month)
`dow = getDayOfWeek(day 1)` (`get(6)`, but when the week starts Monday a Sunday is reported as 7). If `dow > firstDayOfWeek`, for each slot from `firstDayOfWeek` to `dow-1` a Composite with bg `bgColorCellOtherMonths` and fill-both layout data. Only if `use_dnd` **and** the first shown previous-month day is `> minDate`: each such cell gets a drop target (TextTransfer, COPY|MOVE, data = that date) **and** a left-aligned label `" <day>"` with bg `bgColorCellOtherMonths`, fg `colorDisabledPayment`. Otherwise these cells are blank.

### 1.4 Per-month precomputation
- `balances[i] = db.getDay(day i+1).closing_balance` for the month; `minBal = getMin`, `maxBal = getMax` (0 if empty).
- `sumClosing` accumulates for the average.
- `todayCell = null`, `cells = new Composite[daysInMonth]`.
- If `cfg.bestFit`: `bestFit.setSize()` (computes `cell_width` from the main shell client width minus scrollbar, and picks the wider of `fontPosTranAmount`/`fontNegTranAmount` for "1,000.00" as `numFont`).
- `interactive = (calViewFilter == null || !calViewFilter.deleted)`.

### 1.5 Each day cell (day = 1..daysInMonth)
`cell` = Composite in `container`; `cells[day-1]` is a *different* new Composite (see note at end). `cell.setData(date)`.

**Day context menu** (`tempWidgets`), built per cell:
1. `&Add a new payment` → `add_new` (`Budget$146`: `editPayment(mainShell, null, date)`; refresh if true). Enabled only when no filter.
2. `add a &Transaction of ...` → `add_wild_ad` (`Budget$154` → `add_wild_transaction(date)`, §5). Enabled only when no filter.
3. If the day has transactions and `interactive`: separator, `Mark all &PAID` (tooltip `Mark all non-important items as PAID`), `Mark all &UNPAID` (tooltip `Mark all non-important items as UNPAID`). Both start disabled; on first menuShown (`lambda$createCalendarView$0`, guarded by data key `$menu$initialized$`): candidates = day's transactions whose payment is not disabled and whose flag is not `'#'`; `paidCount` = candidates with flag `';'`. If `paidCount > 0` → UNPAID enabled; action: for each candidate `set_pmt_flag(pmt, date, '^', 0.0)` then refresh. If `candidates.size != paidCount` → PAID enabled; action: for each candidate `set_pmt_flag(pmt, date, ';', amount)` then refresh. (`set_pmt_flag`: `setFlag`; if the payment is tentative and flag is `';'` also `setCustomAmount(date, amount)`; if tentative `db.update(pmt)`.)
4. separator; then `add &Note` (no day note) or `&Change note` + `remo&Ve note` (day note exists). Edit → `m_edit_day_note_ad` (`Budget$136`: `editPmtNote(mainShell, date, db.adjustments, "<dd-MMM-yyyy>: Day's note")`, refresh if true). Remove → `Budget$135`: confirm `Are you sure you want to delete this day's note:\n\n"<note>"\n` then `db.setDayNote(date, null)`.
5. If `day.getRestorable()` is non-null: separator, then one item per restorable payment `Restore '<getDesc(desc)>'` → `restore_transaction` (`Budget$155`: `pmt.clear(date)`, `db.update(pmt)`, refresh). Enabled only when no filter.
6. (Added at the very end, only when the balance label is **not** shown for this day) separator + `&Adjust balance (reconcile)` → `adjustBalance(date)`.

**Deleted-filter mode** (`!interactive`): the real `BudgetDay` is replaced by a synthetic copy (same opening/closing balance and adjustment) whose transactions are the restorable payments, amount = `pmt.amount`, suffix `'\''`; the restorable list is then treated as null (no `` ` `` mark, no "Restore" items).

**Cell** : data = date; if interactive: `setMenu(menu)` (plus one-button-mouse mouseUp opener), double-click (`Budget$165`) → `add_wild_transaction(date)`. Tooltip:
- interactive: `[Note:\n<dayNote>\n\n]` + ` Double-Click to:\n      add a wild transaction\n Right-Click to:\n   - Add a new payment\n   - Add/Change/Remove a note  ` + (if restorable) `\n   - Restore transactions:` followed by `\n    -- <getDesc>` per payment.
- not interactive: `Not interactive: You are in 'Show deleted transactions' mode.`
- If `use_dnd && interactive`: drop target on the cell (data = date, adapter `Budget$156`, §6).

**Cell background** (also stored as data `"bgcolor"`): date == today → `bgColorCellToday` and `todayCell = cell`; date < today → `cfg.bgColorCell` ("BKG cell past"); date > today → `cfg.bgColorCellPast` ("BKG cell future"). Layout: GridLayout 3 columns, verticalSpacing 0, horizontalSpacing 1.

**Day-number label** (left aligned, spans 3 columns): text = `<day>` + (`` ` `` if restorable items exist) + (` ...` if a day note exists). bg = `bgColorImportant` if a day note exists else transparent; fg `colorCalendarDay`; font `fontDate`. Tooltip = `[Note:\n<dayNote>\n\n]` + `double click to add or edit day's notes`. If interactive the day menu is attached. Mouse (`dayNumberMouseListener`, `Budget$158`): double-click → `editPmtNote(mainShell, date, db.adjustments, "<dd-MMM-yyyy>: Day's note")`, refresh if true.

Day 1 → `setVal(openingBalance, day.opening_balance, "")`; last day → `setVal(closingBalance, day.closing_balance, "")`.

**Filter highlight**: if a filter is set and any transaction of the day matches `calViewFilter.isMatch(pmt, amount, date)`, cell bg and day-label bg become `bgColorCellHighlight` (stored as "bgcolor").

**Best fit**: if `cfg.bestFit`: `bestFit.setNumLength("-" + CVformat(max |amount| of the day) + "*")` → `target_width = (cell_width - extent(numFont, s) * labelCompensation - 1) / labelCompensation`.

**Day In/Out accumulators**: `in` = sum of amounts ≥ 0 of non-disabled transactions (+ `adjustment_diff` if ≥ 0); `out` = sum of negative amounts (+ `adjustment_diff` if < 0).

### 1.6 Transaction rows (iteration order = `BudgetDay.transactionIterator()`: non-disabled before disabled; among negatives most negative first; otherwise descending amount)
For each `Transaction t` (`pmt = t.pmt`, `flag = pmt.getFlag(date)`, `note = pmt.getNote(date)`): an `Info{cal=date, pmt, amount=t.amount}` is created. `matches = (no filter) || filter.isMatch(pmt, t.amount, date)`. Non-matching rows are created but hidden (`setVisible(false)`), so they keep their layout slot.

Right-click handler (`lambda$createCalendarView$5`, mouseUp): if `cfg.oneButtonMouse` or `button == 3`: lazily build `info.menu = createTransactionMenu(...)` (§2) and show it.

Shared description tooltip (interactive):
```
[Note:\n<note>\n\n]Transaction: <part after ';' or whole description>\n[Category: <cat>\n]\n - Hold Ctrl: detailed payment information\n - Double Click: Edit payment\n - Drag this transaction to a different day  \n - Right-Click: Edit,Move,Delete,etc \n - Control+Click: Edit transaction's note
```
Not interactive: `Click to restore!`.

Column 1 – **image label** (only if `pmt.image_path` non-empty; otherwise the description spans 2 columns): left aligned, bg transparent, image `images/<image_path>` with background = cell colour. If interactive: right-click handler and `mouse_edit`; else `restore_ad`. `setEnabled(!pmt.disabled)`. Tooltip as above. `imageWidth = image.width + 1` when best fit is on (0 otherwise).

Column 2 – **description label** (left):
- bg: note → `bgColorImportant`, else transparent. Then flag `';'` → bg `bgColorPaid`, font `fontPaidTran`; flag `'#'` → bg `bgColorImportant`, font `fontImportantTran`; else font `fontTran`. fg `colorDescText`.
- Highlight colour: `c = pmt.getHilighting(date)`; if -1 → `pmt.getDefaultHilighting()`. If `c != -1`: bg = RGB(c) and fg = white when `isDarkColor` (§3).
- Text = (`...` if note) + `getDesc(description)`; with best fit → `bestFit.getString(font, text, imageWidth)` (chops 3 chars at a time and appends `...` until `extent < target_width - imageWidth`; returns `...` if shorter than 3 chars).
- If note and no "bgcolor" stored yet → store `bgColorImportant`.
- Listeners: right-click (interactive); `mouse_edit` (`Budget$144`: double-click → `editPayment(mainShell, pmt, date)`; mouseDown with CTRL (`stateMask & 0x40000`) → `editPmtNote(mainShell, date, pmt, null)`) or `restore_ad` (`Budget$143`: mouseDown → `showYesNo("Do you want to restore '<description>'\non <date>?")` → `pmt.clear(date)`, `db.update`, refresh); disabled → fg `colorDisabledPayment`; hover tracker `Budget$164` (§1.8); if `use_dnd && interactive` a DragSource (TextTransfer, COPY|MOVE, data = Info, adapter `Budget$157`).

Column 3 – **amount label** (right aligned, fill horizontal):
- `v = t.amount`; if `v` equals -0.0 → 0.0. If `dont_show_zero_amount && v == 0` → text `"  "`; else text = CVformat(v) + (suffix char if `t.suffix != 0` else U+2800 BRAILLE BLANK).
- bg transparent; fg `colorNegTranAmount` if `isNegative(v, showDecimals)` else `colorPosTranAmount`; font `fontNegTranAmount` / `fontPosTranAmount` likewise. Disabled → fg `colorDisabledPayment`.
- Listeners: right-click; hover tracker (interactive only); `mouse_ad` (`Budget$142`: double-click → `adjustAmount(date, pmt, amount)`) or `restore_ad`.
- Tooltip (interactive): `Transaction: <part after ';' or whole>\nActual amount: <Frac2 v>    Regular: <Frac2 pmt.amount>\n\n - Hold Ctrl: detailed payment information\n - Double Click: Adjust this transaction's amount  \n - Right-Click: Edit,Move,Delete,etc`.

Every label stores the `Info` as its data (`"Amount"`-less), the image label too.

### 1.7 Balance label (bottom of the cell)
`show` =
- if `opt_show_balance_supplied`: `ALL` → true; `FIRST_AND_LAST` → day 1 or last day; otherwise (`NONE` or any other value) false;
- else: `cfg.alwaysShowBalance || day == 1 || day == last || day.hasTransactions() || day.closing_balance_adjusted`.

If shown: right-aligned label, layout fill-horizontal + grab both + vertical align END, span 3, font `fontAmount`, data = date.
- Marker: U+2800 by default; if `closing_balance_adjusted`: `'` when `-1 < adjustment_diff < 1`, else `'(<CVformat adjustment_diff>)`.
- Text = CVformat(`day.getClosingBalance(showDecimals)`) + marker. (`getClosingBalance(false)` truncates toward zero after adding/subtracting 0.05.)
- bg: if `closing_balance + 0.05 < cfg.minBalance4MaxSaving` and `minBal != maxBal` → `bgColorBelowMinBalance`; else if `closing_balance == minBal` → `bgColorMinBalance`; else if `== maxBal` → `bgColorMaxBalance`; else `bgColorBalance`.
- fg: `colorNegativeAmount` if `isNegative(closing_balance, showDecimals)` else `colorPositiveAmount`.
- Tooltip (only set when interactive): `[<Frac2 closing_balance>\n` if !showDecimals`]` + `[Before adjustment amount: <Frac2 unadjusted>\nAdjustment amount: <Frac2 adjustment_diff>\n\n` if adjusted`]` + `[Day's In:<CV in> Out:<CV out> Total:<CV in+out>\n` if in or out ≠ 0`]` + ` Menu:\n   - Adjust balance (reconcile)     ` + `[\n   - Remove adjustment  ` if adjusted`]`.
- Menu (`mainShell` pop-up): `&Adjust balance (reconcile)` → `adjustBalance(date)` (`Budget$166`); if adjusted also `&Remove adjustment` → `m_remove_balance_adjustment` (`Budget$145`: `db.removeAdjustment(date)`, refresh). Attached (and double-click → `adjustBalance`, `Budget$167`) only when interactive.
If not shown: the day menu gets separator + `&Adjust balance (reconcile)` (`Budget$168`).

### 1.8 Hover tracker `Budget$164` (description + amount labels)
mouseEnter: `labelInFocus = label` (used by the Ctrl key: `Budget$11` shows `showStatsPopUp(labelInFocus)` when keyCode == CTRL and the main shell is active, §9). `hl = bgColorHighlight`. If `pmt.grace_period_days > 0` or `pmt.isTentative()`: `validDays = db.get_valid_payment_days(pmt, date)` (the grace/tentative window inside this month); every `cells[i]` whose date is in the set gets bg `bgColorImportant`; if the hovered day itself is **not** in the set `hl = colorNegativeAmount`; if the set contains the 1st of next month → `showNextMonthGraceDays(nextMonth, validDays)` (§6.4). Finally the label's image (if any) or the label gets bg `hl`.
mouseExit: restore every `cells[i]` bg from its `"bgcolor"` data; restore the label (or its image) bg from the label's `"bgcolor"` data (transparent if none); `dismissNextMonthGraceDays()`; `labelInFocus = null`.

### 1.9 Trailing cells (next month) and totals
After the last day, if its weekday ≠ `lastDayOfWeek` (`firstDayOfWeek + 6`): filler composites with `bgColorCellOtherMonths`; if `use_dnd` and the first next-month day `< maxDate`, each gets a drop target and a `" <day>"` label (fg `colorDisabledPayment`), same as §1.3.

Then `container` is sized to its computed size and the scroll min size set. Monthly stats: `st = calcStatistics(curCal, 1, fullStats=false)` (§11):
- `setVal(totalIncome, st.total_income, "'" if total_income ≠ total_planned_income else "")`
- `setVal(totalExpenses, st.total_expense, "'" if ≠ planned)`
- `setVal(totalsDiff, income + expense, "'" if (incomeDiff + expenseDiff) ≠ 0)`
- `setVal(totalAvgBal, sumClosing / daysInMonth, "")`, `setVal(minBalance, minBal, "")`, `setVal(maxBalance, maxBal, "")`
- tooltips: `Planned Income: <prefix><Frac2 planned><suffix>\nDifference: <CV diff>` and `Planned Expenses: ...`.
- `updateStatPane(st)` (§12).

`setVal(label, v, extra)`: fg `colorNegativeAmount` if `isNegative(v, true)` else `colorPositiveAmount`; text = `currencyPrefix + Frac2(v) + currencySuffix + extra`.

**Note on `cells[]`**: `cells[day-1]` *is* the visible cell Composite — the regenerated pseudo code shows `$t = new Composite(container, 0); cell = $t; cells[day-1] = $t` (one object; the earlier "separate empty Composite" reading was an artifact of the old generator duplicating the `new`). All cell-level highlighting done through `cells[i]` (grace days, stats hover, flash) targets the visible cell of day i+1.

---

## 2. `createTransactionMenu(date, t, info, pmt, flag, note)` – transaction context menu
Pop-up on `mainShell`, registered in `tempWidgets`. Items in order (data = `info` unless noted):
1. `&Edit payment` → `Budget$119`: `editPayment(mainShell, pmt, date)`; refresh if true.
2. `duplicate &Payment` → `Budget$120`: `duplicatePayment(pmt)` (clone, description + `(dup)`, `db.add`, open editor) then refresh.
3. If `pmt.getEndMode() != END_MODE_LOAN_PAID(64)`: `alter transactions from <dd-MMM-yyyy>` → `Budget$121`: `splitPayment(info)`.
4. separator; `duplicate &Transaction` → `Budget$122`: `duplicate_transaction(pmt, date, amount)`.
5. `&Move transaction` → `Budget$131`: `pickDate(mainShell, date)`; if a different date is picked: `target = db.getDay(newDate)` (abort if null); if `target.get(pmt)` exists → `confirmYesNoCancel(SUM_QUESTION)` (`The selected day already has this type of transaction.\nDo you want to sum the amounts?\n`): Yes → add existing amount, No → overwrite, Cancel → abort. Then `db.addMoveUndo(pmt, oldDate, amount, newDate, existingTransaction)`, `pmt.moveTransaction(newDate, oldDate, newAmount)` (moves custom amount, flag, note, highlighting), `db.update(pmt)`, refresh.
6. separator; `&All transactions` cascade (drop-down menu with data `info`, `"filter"` = current `calViewFilter`, listener `all_transactions_menu_listener` `Budget$124` → `fillAllTransactionsMenu(pmt, menu, filter, matchingOnly, date, jumpListener)` – filled elsewhere, not in part05).
7. `&Six months average` cascade (`Budget$126`, filled on show using `getSixMonthsAvg(description, date)`): if `n == 0` a disabled `no data found.`; else disabled items `<MMM-yyyy> to <MMM-yyyy>`, `# of tran. actual/planned: n/np`, `# of months: nm`, `(click an item to copy)`, then clickable (copy to clipboard, `Budget$125`) rows via `add_six_mi`: `Avg. per transaction` (pt), `Avg. per month` (pm), `Avg. per day` (pd), `Avg. per planned period` (pp), `Avg. amount per <N> days` where `N = round(first_last_days_diff / (n-1))` if `first_last_days_diff != 0 && N >= 1`, value `total / first_last_days_diff * N`, and `Total amount`.
8. If end mode ≠ `INDEFINITE(16)`: `payments remaining` cascade (`Budget$127`): `r = db.getPaymentsRemaining(pmt, date)`; items `<r.n>`, `Amount: ~<prefix><Frac0 roundUp(|r.abs_amount|)><suffix>` (`roundUp = ceil(x/10)*10`), `Last Payment on: <dd-MMM-yyyy endDate>`.
9. separator; `&Filter these transactions` → `Budget$141`: `setFilterText("desc:" + getDesc(description) with each space replaced by "\ ")`, `calViewFilter = new Filter(filterText with '&' → '＆')`, refresh.
10. separator; `&Delete transaction` → `Budget$140`: confirm `Are you sure you want to delete this transaction ?\n'<getDesc>' on <dd-MMM-yyyy>` → `deleteTransaction(date, pmt)`, refresh.
11. separator; `ad&Just transaction amount` → `Budget$132`: `adjustAmount(date, pmt, amount)`.
12. If `t.amount != pmt.amount`: `&Remove adjustment` → `Budget$138` `remove_adjustment`: if (loan payment and `db.isRegularPaymentOn(pmt, date)`) or sticky → `pmt.clear(date)` else `pmt.setCustomAmount(date, pmt.amount)`; `db.update`, refresh.
13. If `opt_show_url` and `pmt.getURL() != null`: separator; item `URL: <url>` where the display string drops a leading `https://` or `http://` and is cut to 29 chars + `...` when longer than 30; action `Program.launch(url)` (`Budget$139`).
14. separator; CHECK item `mark as paid`, checked iff `flag == ';'`, data `"flag"` = `""` if currently paid else `";"`; CHECK item `mark as &Important`, checked iff `flag == '#'`, data `""`/`"#"`. Both → `m_flags_ad` (`Budget$137`): `set_pmt_flag(pmt, date, flag.isEmpty() ? '^' : flag[0], amount)`, refresh. (Checking one does not uncheck the other in code; the flag is a single char so the last action wins.)
15. `&Highlighting...` cascade (`Budget$130`, filled on show): `Custom` (data info; shows a 40×20 swatch of the current per-date colour if any) → `Budget$129` opens a `ColorDialog` preset to the current colour, on Windows with custom RGBs collected from all non-standard highlight colours in the DB; result → `pmt.setHilighting(date, rgb)`, refresh. Separator; one item per `hilight_color[i]` named `hilight_color_name[i]` with swatch: Gold 0xFFD700, Salmon 0xFA8072, Pink 0xFFC0CB, Coral 0xFF7F50, Dark Khaki 0xBDB76B, Violet 0xEE82EE, Lime 0x00FF00, Yellow Green 0x9ACD32, "Dark See Green" 0x8FBC8F, Sky Blue 0x87CEEB, Misty Rose 0xFFE4E1 → `Budget$128`: `pmt.setHilighting(date, color)`, refresh. Separator; `default` (colour -1 = remove the per-date entry). (When `info.cal == 99990101` – the payment editor's default-colour preview – the listeners update `info.bg_color`/`info.bg_label` instead.)
16. separator; `add &Note` (no note) → `Budget$134`: `editPmtNote(mainShell, date, pmt, null)`; or `&Change note` (same) + `remo&Ve note` → `Budget$133`: confirm `Are you sure you want to delete this note:\n\n"<note>"\n` → `pmt.setNote(date, null)`, refresh.

---

## 3. `isDarkColor(c)`
`true` if `red < 10 && green < 10` (blue ignored); otherwise `true` iff HSB brightness `< 0.7`.

## 4. `scheduleUpdateCalendarView()` / `scheduleRebuild()`
Just set `do_calendar_update = true` / `do_rebuild = true` (polled by the main loop elsewhere). `updateCalendarView()` returns immediately while `do_calendar_update == 1`.

## 5. Wild transactions
### `add_wild_transaction(date)`
`pmt = pickWildPaymentDialog(mainShell, "", date, 0.0)`; if non-null: `pmt.setCustomAmount(date, pmt.amount)`, `db.update(pmt)`, refresh; then `adjustAmount(date, pmt, pmt.amount)` (amount prompt) – if it returns false (cancelled) → `deleteTransaction(date, pmt)` and refresh again.

### `fillWildPaymentTable(tree, filter, transferMode, date)` (payment picker list used by that dialog; column 1 image, 2 description, 3 amount)
- `filter` upper-cased; `db.sort()`; iterate all payments; skip `id == 0`; skip if `description.toUpperCase()` does not contain the filter.
- In `transferMode` only payments that are `disabled && isSticky()` and (end mode indefinite OR `getFirstOptionAfter(date minus 1 year) != null`) are listed.
- Row: image (if any, bg window colour), text `getDesc(description)`, data = payment, amount = `Frac2(transferMode ? pmt.getAmountBeforeDate(date) : pmt.amount)`.
- Consecutive payments with the **same description** (list is sorted) are grouped: the first becomes the parent; each following one becomes a child row (`"  " + getDesc`, `Frac2 amount`, disabled colour if disabled) unless a child with the same amount already exists under that parent (then it is skipped). Parent gets `colorDisabledPayment` if it has children and all of them are disabled; a single (ungrouped) disabled payment also gets that colour.
- Empty result → one row `<no matching payments found>` with null data. If exactly one top-level item with more than one child → expanded. First item selected.

### `pickPayment()` (pop-up payment chooser, returns `pickPaymentPmt`)
Menu of all payments (`id != 0`, sorted) with image; same-description runs become cascades named by the description (one in the main menu, one in the `Older payments...` submenu) whose items read `<description> (<Frac2 amount>) <dd-MMM-yyyy start>` + `...` if indefinite or ` to <end>` if `n_pays != 1`; single payments read `<description> (<Frac2 amount>)`. A payment goes to the `Older payments...` cascade when `isOldPayment(pmt, curCal-6 months, curCal+2 months)` = amount ≠ 0, not indefinite, no options in that range and `endDate <= curCal-6 months`. Empty cascades are disposed; the `Older payments...` entry is added only if non-empty. Selection listeners `Budget$152/153` set `pickPaymentPmt` (bodies not in part05).

### `showTransactionChoices(shell, icon, title, amount, sum)`
`MiSWT.selectChoice` radio dialog (`Accept`/`Cancel`), message `The selected day already has a transaction like this.\n\nWhat do you want to do?\n`, choices in order: `REPLACE` = `OVERWRITE the amount with <Frac2 amount>` (default), `ADD_SUM` = `SUM both amounts: <Frac2 sum>`, `ADD_ANOTHER` = `ADD as a new transaction`. Returns the enum or `null` on Cancel.

---

## 6. Drag and drop of a transaction
Drag data string (`Budget$157.dragSetData`): `"MiShell*Budget*TID:" + date + ":" + pmt.id + ":" + amount + ":" + pid`.

### 6.1 `dragStart` (`Budget$157`)
Creates two floating month pickers (`createDropTargetShell`, set = null): `dropShellTargetPrev` for `date - 1 month`, placed at the absolute top-left of `calendarScroll` moved up by its own height (y clamped to ≥ 0); `dropShellTargetNext` for `date + 1 month`, same y, x = `scroll.x + (mainShell.clientArea.width - shell.width - 2)`. Any existing next shell and the grace-days shell are disposed first. `dragFinished` disposes both.

### 6.2 `drop` (`Budget$156`)
- Reject (`detail = NONE`) unless `data != null` and `detail` is MOVE(2) or COPY(1).
- Ignore strings not starting with the prefix. Parse `srcDate`, `id`, `amount`, `pid`; `dstDate` = drop target's data. If `pid != current pid` → `showInfo("Budget Calendar doesn't support cross process Drag and Drop.\nIf this is not a mistake, please email support@mishell.ca with details of why you want to drag-and-drop from one budget to another.\n")`. If `dstDate == srcDate` → nothing.
- `pmt = db.getByID(id)`, `dstDay = db.getDay(dstDate)` (null → abort).
- If `dstDay.get(pmt)` exists (same payment already on target day): `choice = showTransactionChoices(mainShell, budgetIcon(), "Transaction " + (COPY ? "Copy" : "Move"), amount, amount + existing.amount)`:
  - `null` → abort (no change).
  - `REPLACE` → newAmount = amount; `ADD_SUM` → newAmount = sum. Then COPY: `pmt.setCustomAmount(dstDate, newAmount)`; MOVE: `db.addMoveUndo(pmt, srcDate, amount, dstDate, existing)`, `pmt.moveTransaction(dstDate, srcDate, newAmount)`.
  - `ADD_ANOTHER` → `duplicate_transaction(pmt, dstDate, amount)`; if MOVE also `pmt.setNoTransaction(srcDate)`.
- Else: COPY → `pmt.setCustomAmount(dstDate, amount)` then `adjustAmount(dstDate, pmt, amount)` (prompt); MOVE → `addMoveUndo` + `moveTransaction(dstDate, srcDate, amount)`.
- `db.update(pmt)`, `scheduleUpdateCalendarView()`, `dragFinished(null)`.

### 6.3 `createDropTargetShell(parent, monthCal, graceSet)` / `fillPickDropPickDate`
Shell style NO_TRIM|ON_TOP, bg `bgColorImportant`, GridLayout 7 equal columns, margins 10, spacing 1, data `"initial"` = month. Contents:
- Title label (span 7, centred): `sdate_MY(month)` = `MMMMMMMMMMMM-yyyy` (e.g. `November-2026`), white bg, `fontMonth`.
- 7 centred 3-letter weekday names (`getWDayName(i).substring(0,3)`), fg `colorDatePickerHeader`, bg `bgColorDatePickerHeader`, font = `fontTran` scaled ×1.5 (normal style).
- Leading blanks (bg `bgColorCellOtherMonths`) for slots before day 1; then one right-aligned label per day with font ×1.5: bg = `bgColorCell` when `day < today` else `bgColorCellPast`; overridden to `bgColorImportant` if `graceSet` contains the date. When `graceSet == null` (DnD mode) each day label gets a DropTarget (TextTransfer, COPY|MOVE, data = that date, adapter `Budget$156`). Trailing blanks to the end of the week. (The label `setData(date)` call is applied to the *previous* widget – harmless, the drop target carries the correct date.)
- `pack()`.

### 6.4 Grace-period highlight shell
`showNextMonthGraceDays(nextMonth, set)`: if not already shown, create the shell with `set`, position like the "next" DnD shell, show. `dismissNextMonthGraceDays()`: dispose and null. Shown only from the hover tracker (§1.8) when a tentative/grace-period payment's valid days spill into the next month.

### 6.5 Geometry helpers
`getBorders(shell)`: `size = shell.getSize()`, `ca = shell.getClientArea()`; `left = right = bottom = (size.x - (ca.width - ca.x)) / 2`; `top = size.y - bottom - (ca.height - ca.y)`.
`getAbsLocation(control)`: sum `getLocation()` of the control and its ancestors up to (excluding) the top-level shell, then add `mainShell` border `left`/`top`.
`show_current(scroll)`: if `todayCell` exists and its bottom/right edge is beyond the visible client area, set the scroll origin so that it is visible with a 20 px margin (x and y independently; otherwise origin (0,0)).

---

## 7. `fillAllMatchingTransactionMenu(menu, filter, listener)` → item count
Scan every day from `minDate` until `db.getDay()` returns null; collect `TranInfo(date, amount, pmt)` for transactions with `filter.isMatch(pmt, amount, date)`. Group into year → month nodes; amounts of **non-disabled** payments are summed into month/year/`total`, and into `toDate` when the month `<= curCal.ym`.
- No matches: a disabled item `No transactions found` + (if `!cfg.unlimitedHistory`) ` -- try enabling 'Unlimited history' in Preferences.`; return 0.
- Per year: cascade `<yyyy>: <Frac2 yearTotal>` (`AMOUNT_SEP = ": "`), icon `icons/nxt.gif` if it is today's year. Per month: if the month has more than one transaction, cascade `<Mon>-<yyyy>: <Frac2 monthTotal>` (icon if current month); otherwise its single transaction goes directly under the year. Transaction item: `<dd-MMM-yyyy>: <getDesc>   <Frac2 amount><oneLineNote>` (`getOneLineNote` = ` -- <note without newlines>` cut to 78 chars + `...` if ≥ 80), icon when placed directly under the year and it is the current month, enabled iff payment not disabled, data = TranInfo, selection → `listener` (`Budget$161`: jump `curCal` to the 1st of that month if within `[minDate, maxDate]` and refresh).
- Footer: separator + disabled `Total: <Frac2 total> (<Frac2 toDate>/<Frac2 total - toDate>)`.

## 8. `fillStatsTransactionMenu(menu, stats, pmt, listener)` → `Payment[]`
Same structure but the range is `stats.startCal..stats.endCal`, the match is `getDesc(t.pmt.description).equals(getDesc(pmt.description))`, the set of distinct matching payments (always including `pmt`) is returned, no icons, and the empty text suffix is ` -- try enabling 'Unlimited history'`.

## 9. `showStatsPopUp(label)` – Ctrl "detailed payment information" popup
`info = label.getData()` (return if null). Shell NO_TRIM|ON_TOP, bg RGB(243,226,169), read-only disabled StyledText, black fg, line spacing = `extent(fontLabel,"W").y / 7`, GridLayout margin = that extent's x. `decimalsFont` = bold, 1.1× the text font (cached). `fmt` = CV format. Content (bold = span style 1, italic = 2, `d` = decimalsFont):
```
<getDesc>:                                   (bold)
  Amount: <d fmt amount>, regular: <d fmt pmt.amount>
[\nNote:\n  <note>\n]                         (bold title, italic body)
[\nWarning: Avg. transaction amount is greater than planned amount!\n]   (when avg.pt < pmt.amount and not within5)  -- red text
```
`avg = getSixMonthsAvg(description, date)` (previous 6 whole months before this month; `pt` = total/n, `pm` = total/nm, `pd` = pm/30.25, `pp` = total/np, `np` = planned count in that range). If `avg.n != 0`:
```
\nLast transactions (up to 4):\n  <dd-MMM-yyyy>: <d amount>; ... \n
\n<nm> month(s) average: <MMM-yyyy first> to <MMM-yyyy last>\n
   # of tran. actual/planned: <n>/<np>, # of months: <nm>\n
   Avg. per tr/month/day/period: <pt>/ <pm>/ <pd>/ <pp or [n/a]>\n
[   Avg. amount/days: <total/diff*N>/<N>\n]        (N = round(diff/(n-1)) >= 1)
   Total amount: <total>\n
[   Planned/year: <plannedCountThisYear * pmt.amount>\n]   (if count > 0)
[Current pace/year: <pm*12>\n]                      (if pm not NaN)
```
If end mode ≠ indefinite: `r = db.getPaymentsRemaining(pmt, date)`, `months = getMonthsDiff(date, endDate)`; if `r.n != 0 && months > 0`: `\nPayments remaining:\n  <r.n> (~<getYMdurationString(months)>)~<d fmt roundUp(|abs_amount|)>, last payment is on <dd-MMM-yyyy endDate>\n`.
Footer (italic, white darkened 0.5): `\nActions:\n  - Double-Click: Adjust this transaction's amount  \n  - Right-Click: Edit,Move,Delete,etc`.
Position: 3 px below-right of the label's absolute bottom-right; if it would exceed the main shell's right edge → placed to the left of the label; same for the bottom edge → above. The label's tooltip is suppressed while shown. Dismissed on any KeyUp / MouseDown / MouseUp / MouseMove (display filters), then the tooltip is restored.

`within5(a,b)` = `a == b || |a-b| < 5`.

---

## 10. HTML report helpers
- `getHtmlFileName()`: temp file `bgt-report-*.html` (delete on exit); on failure `showError("Unable to create output file:\n<ex>")` and null.
- `get_image_full_path(p)` = `"file://" + p`. `embeddedSRC(src, noEmbed)`: if `!noEmbed` and `src` starts with `file://` and the file exists → `data:;base64,<bytes>`; otherwise `src`.
- `amount2html(v, cssClass, colors[pos,neg], decimals)`: `-0.0 → 0`; colour = `colors[v < 0 ? 1 : 0]`; `<FONT color=<c>` + (`>` or ` class='<cls>'>`) + formatted number; if the number contains the locale decimal separator the part from the separator on is wrapped in `<small>…</small>`; closing `</font>`. 3-arg overload uses `cfg.showDecimalsInCV`; `f2html(v)` = 2 decimals, colours `["black","red"]`.
- `saveHTML(file, from, to)`: writes one `<TABLE>` per month from `from` while month `< to` or `== to` (inclusive). Header/CSS exactly as in the pseudo (title `Budget Report`, a `noprint` checkbox `hide running balance` that toggles visibility of `.bamt`). Per month: `<P ALIGN=CENTER><FONT size=+2><B>MMMMMMMMMMMM-yyyy</B></FONT></P>`, `<TH>` weekday names, rows from the week start to the week end; other-month cells `<TD class='day' WIDTH="14%">&nbsp;`; day cells `<TD class='day' VALIGN=TOP WIDTH="14%"><day><BR>` + inner table rows per transaction: `<IMG WIDTH="12" HEIGHT="12" SRC=…>` (not embedded), `<SPAN style='background-color: <bg>;'><FONT COLOR=<fg>><desc (unicodish)></FONT></SPAN>`, amount via `amount2html(amount,"tamt", disabled ? ["#C0C0C0","#FF8080"] : ["black","red"])` + `'` if `amount != pmt.amount` else `&nbsp;`. At the end of each week a second `<TR>` of `<TD class='bal'>` balances (`amount2html(getClosingBalance(showDecimals),"bamt",["black","red"])` + `'` if adjusted). Month footer `Totals: Income = <CV income>, Expenses = <CV expenses> (<CV income - expenses>)` where expenses are accumulated as a positive number (non-disabled transactions and adjustments only).
- `get_html_span_color(t, date)`: if `!cfg.preserveColorOnPrint` → background `white`, foreground `#C0C0C0` if disabled else `black`. Otherwise: bg white / `bgColorPaid` (flag `;`) / `bgColorImportant` (flag `#`); fg `(192,192,192)` if disabled else black; per-date or default highlighting overrides bg and makes fg white when dark; returned as `#rrggbb` strings.

---

## 11. Registration key validation (exact)
```
isBadKey(k):
  blacklist = ["Mg[5P;x5S7V7"] decoded as: for i in 0..11: chr(c[i] - 1 - (i % 4)); decoded value:
     i:0 'M'-1-0='L', 1 'g'-1-1='e', 2 '['-1-2='X', 3 '5'-1-3='1', 4 'P'-1-0='O', 5 ';'-1-1='9', 6 'x'-1-2='u', 7 '5'-1-3='1', 8 'S'-1-0='R', 9 '7'-1-1='5', 10 'V'-1-2='S', 11 '7'-1-3='3'  → "LeX1O9u1R5S3"
  if k equals a decoded blacklist entry → bad
  if k[11] is not a digit → bad
  if any of k[0..10] is not upper-case → bad
  else good

isValidKey(k):
  length must be 12, and not isBadKey
  sum = 0; for i in 0..11, skipping i == 4 and i == 8: sum += k[i]; if sum > 101: sum = sum % 101 + 1
  valid iff k[4] == chr(77 + (sum / 10) % 10)   ('M'..'V')
        and k[8] == chr(78 + sum % 10)          ('N'..'W')
```
Note `isBadKey` runs before the length check only through `isValidKey` (which checks length first), so `k[11]` is safe.

---

## 12. Statistics engine
Data types: `Stats{startCal, endCal, fullStats, statsMap: HashMap<String,Amount>, incomeVec, expenseVec, total_income, total_expense, total_planned_income, total_planned_expense, firstColWidth=-1}`; `Amount{key, amount, planned_amount, sub_amounts: HashMap (always non-null in this build), pmt, agg_parent}`. `indent_string = "   "` (3 spaces).

- `calcStatistics(cal, nMonths, full)`: range = 1st of `cal`'s month … (1st + nMonths months − 1 day). `calcStatisticsExact(start, end, full)`. Both call `calculate_stats_range`.
- `addEmptyCategory(stats, name)`: top-level `Amount` with that key.
- `addStatInfo(stats, category, description, amount, pmt, planned)`:
  - If `category == null && stats.fullStats`: category = `(Uncategorized-Expenses)` if (`planned ? pmt.amount : amount`) `< 0` else `(Uncategorized-Income)`.
  - If category non-null: get/create top-level `Amount(category)`; `amount += amount`; if planned `planned_amount += pmt.amount`; sub-map = its `sub_amounts`; sub-key = `indent_string + description`. If category is still null (non-full stats): sub-map = `statsMap` itself (the row lands at top level under the plain `description` — the indent prefix is only added inside the category branch; verified in the Rust port against the screenshot).
  - get/create sub `Amount{key, pmt, agg_parent = category Amount}`; `amount += amount`; if planned `planned_amount += pmt.amount`.
- `calculate_stats_range(stats)`:
  1. Create `(Uncategorized-Income)` and `(Uncategorized-Expenses)` top-level entries.
  2. Planned pass: for each day in range, for each `pmt` in `day.getRegulars()` (scheduled payments, including days where the transaction was moved/deleted): skip if filter set and `!filter.isMatch(pmt, pmt.amount, -1)`; skip disabled; `addStatInfo(cat, desc, 0.0, pmt, planned=true)`; `total_planned_expense += pmt.amount` if negative else `total_planned_income += pmt.amount`.
  3. Actual pass: for each day, each transaction: skip if filter set and `!isMatch(pmt, amount, date)`; skip disabled; `addStatInfo(cat, desc, amount, pmt, false)`. Then if `day.adjustment_diff != 0`: `addStatInfo(null, "(Unaccounted)", adjustment_diff, null, false)` (so with full stats it lands under Uncategorized-Income/Expenses by sign; without, at top level as `"   (Unaccounted)"` with no pmt).
  4. Descriptions present in **both** uncategorized groups: `net = inc.amount + exp.amount`; if `net > 0` remove from expenses (`expCat.amount -= exp.amount; incCat.amount += exp.amount; inc.amount = net`) else remove from income (`incCat.amount -= inc.amount; expCat.amount += inc.amount; exp.amount = net`).
  5. `transferSubAmount(incCat → expCat, negativeOnly=false)`: move every sub with `amount < 0`; `transferSubAmount(expCat → incCat, true)`: move every sub with `amount >= 0`. Each move adjusts both parents' `amount` and `planned_amount`.
  6. Remove either uncategorized category if it has no subs.
  7. `total_income` / `total_expense` = sum of top-level `Amount.amount` by sign (≥ 0 → income).
  8. Sort top-level amounts with `stat_comparator`: if both negative → ascending (most negative first); otherwise descending (largest first; positives precede negatives). Each top-level goes to `expenseVec` if `amount < 0` else `incomeVec`, immediately followed by its subs sorted with the same comparator (subs follow their parent regardless of their own sign).

Note: `calculate_stats_range` honours `calViewFilter`, so the stats pane reflects the active filter.

## 13. Brief Summary pane
### `updateStatPane(stats)`
Dispose children of `stats_view`; `fillStats(stats, stats_view, calViewFilter == null ? 49 : 4079, details=true)`; layout; if the computed height is below `stats_scroll` layout `heightHint`, set `heightHint = h - h/3`.

Mode bits (`p2`): 1 = `Total Income` row, 2 = income category rows, 4 = `Total Expenses` row, 8 = expense category rows, 16 = brief (top expense categories + `Other...`), 32 = `Disposable Income` row. 49 = Disposable + Total Income + brief expenses; 4079 = everything except brief (1|2|4|8|32 and unused high bits).

### `fillStats(stats, parent, mode, details)`
`stat_line_color = 0`; parent bg = `bgColorDetails` if details else `bgColorWindow`; GridLayout 5 columns (details) or 6, spacing 1, marginHeight 0. Headers (`add_hdr`: `fontTran`, header colours, width hint = extent of a sample string): `Description` (left; sample `Disposable Income   ` for details, `   (Uncategorized-Expenses)  ` otherwise; its width hint becomes `stats.firstColWidth`), `Amount` (right, sample `-10,000  `), `avg/mth` (only when !details, sample `-avg/mth+1`), `%` (sample `-100  `), `+/-` (sample `-10,000  `, tooltip `This is the difference between planned and the actual amounts`), `Graph` (left, width 170).
`months = getMonthsDiff(start, end) + 1`. Rows (`add_row(stats, parent, text, amount, amount/months, indent, Amount, diff, details)`):
- 32: `Disposable Income` = `total_income + total_expense`, diff 0; tooltip `When hovering Calendar View Displays in each day:\n-- max disposable amount, and\n-- in square brackets the difference between planned\nand actual amounts on that day`.
- 1: `Total Income`, diff `total_income - total_planned_income`.
- 2: each `incomeVec` entry: `getDesc(key)`, indented, diff `amount - planned_amount`.
- 4: `Total Expenses`, diff `total_expense - total_planned_expense`.
- 8 or 16: `expenseVec` entries; in brief mode entries whose key starts with `indent_string` (sub rows) are skipped, at most 6 rows are added, then `Other...` = `total_expense - shownSum` (indented, no Amount, diff 0).
Finally `setLayoutDeferred(false)`, size the parent and the enclosing ScrolledComposite min size.

### `add_row(...)` → the description label
- Row bg: when `stat_line_color == 0` → `bgColorDetails` (details) / `bgColorWindow`; else `bg_darker_color` (= `bgColorDetails` ±(32 if dark / −10 if light) per channel). Toggle `stat_line_color` after each row, so rows alternate starting with the base colour.
- Description label (left): text = (`"  "` if indent) + text; `fontTran`; fg white if the row bg is dark else black; data `"Amount"` = Amount, `"Stats"` = stats; hover `stat_mta` (`Budget$171`). Details: width hint 20 and `addEllipsisIfNeeded(label, stats.firstColWidth)` (replace the tail with `…` until it fits; `…` alone if nothing fits); click `stat_ma` (`Budget$169`). Non-details: click `stat_transactions_ma` (`Budget$170`); the extra "link style" branch (indent 20, trimmed text, `colorCurrentMonth`, underline painter `lbl_underline_textOnly`) requires `Amount.sub_amounts == null`, which never happens in this build (constructor always allocates it) – dead code.
- Amount label (right): CVformat(amount); fg `colorNegativeAmount`/`colorPositiveAmount` by `isNegative(amount, showDecimals)`.
- avg/mth label (right, non-details only): CVformat(avg), same colour.
- `%` label (right): `Frac0(amount / total_income * 100)` if `total_income != 0`, else `Frac0(amount / total_expense * 100)` if `total_expense != 0`, else `?`; same colour.
- `+/-` label (right): empty if `|diff| < 5`, else CVformat(diff), colour by sign of diff; tooltip when shown: `The difference between actual amount of <CV amount> and planned amount of <CV amount - diff>`.
- Graph label (left): image 170×9 from `gen_image(amount, total_income, total_expense, 170, 9, rowBase)`: background filled with the base colour (`bgColorDetails`/`bgColorWindow`, not the alternating colour), then a bar of width `170 * |amount| / max(|total_income|, |total_expense|)` (denominator 1 if both 0) in `graphBarColor` if `amount > 0` else `graphNegativeBarColor`.
- `gen_hilight_color_image(w, h, rgb)`: solid swatch.

### Click handlers
- `Budget$169` (`stat_ma`, Brief pane): if the row's Amount has a `pmt` → `calViewFilter = Filter("desc:" + pmt.description with spaces escaped as "\ ")` (full description, not `getDesc`). Else by (trimmed, possibly ellipsised) label text: empty → nothing; `Total Income` → `>0`; `Total Expenses` → `<0`; `Disposable Income` → nothing; anything else → `cat:<text with escaped spaces>`. Then `setFilterText(filter)` and refresh.
- `Budget$170` (`stat_transactions_ma`, detailed statistics windows): if Amount has `pmt` and Stats: pop-up filled by `fillStatsTransactionMenu` (§8) with jump listener `Budget$170$1`; if any payments: separator + per payment `Edit payment` (single) or `Edit payment [<start> ... <end or empty if indefinite>]` → `Budget$170$2`: `editPayment(activeShell, pmt, null)` then the active shell's `"updateCallback"`.update().

### Hover `Budget$171` (`stat_mta`)
mouseEnter: remember bg/fg, set bg `bgColorHighlight` (fg white if dark). If a filter is active, stop. Then:
- Text `Disposable Income`: `best = getBestSavings(curCal)` (per day, `-get_best_saving(day, cfg.maxSavingNMonths, 0, minBalance4MaxSaving, "$")`, truncated to int), `pm = getMonthPlusMinus(curCal)` (per day the negated running sum of (planned regulars − actual transactions), non-disabled only). For every `cells[i]`: hide children except the first (day number) and last; add a centred overlay label (layout `exclude = true`, bounds = full cell width, vertically centred, `fontMonth`, fg `colorPositiveAmount`) with text `<Frac0 best[i]> [<Frac0 pm[i]>]`, bg `bgColorMaxBalance` if `best[i]` is the max, `bgColorMinBalance` if the min, else transparent.
- Otherwise, for an Amount that has a `pmt` or is a category: for each day of the current month with transactions, a transaction matches when its payment is the same object or has the same description as `Amount.pmt`, or (category row) `t.pmt.category.equals(Amount.key)`. On a match: cell bg `bgColorCellHighlight`; children hidden except index 0, the last one, and labels whose `Info.pmt` is the matched payment.
mouseExit: restore the label's bg/fg; if no filter, for every cell restore bg from `"bgcolor"`, dispose children whose layout data has `exclude`, make the rest visible.

---

## UNKNOWN / undetermined
- `createCalendarView`: effect of `opt_show_balance` values other than 0/1/2 (treated as "never show") – inferred, not confirmed.
- `pickPayment`: bodies of `Budget$152`/`Budget$153` (assumed to set `pickPaymentPmt` and close the menu) are outside part05 and were not read.
- `showStatsPopUp`: `SimpleCalendar.getYMdurationString` output format not read.
- `fillStats`: brief mode shows up to 6 expense categories (`++n > 5` tested after the row is added, so the loop breaks after the 6th (verified against regenerated pseudo code)); the intended limit may have been 5.

Verified pass: 2026-10-27 — 1 statements corrected, 1 confirmed.
