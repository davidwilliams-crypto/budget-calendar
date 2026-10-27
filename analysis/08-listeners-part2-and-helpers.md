# 08 — Listeners in Budget.part02 (Budget$4..$9, $33..$99) and helper classes

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part02.txt`. Installation sites were located by grepping `new Budget$N(` in part03/part04/part06 (and part01 for context).

Scope note: the File menu (`Budget$101..$108`), Help menu, recent files, Preferences, reports, and the Highlighting menu builder (`Budget$130`) are anonymous classes numbered >= 100 and live in **part01**, not part02; they are referenced here only where a part02 listener calls into them. The fragment at the very top of part02 (lines 1-8) is the tail of `Budget$32.widgetSelected` (month-label menu item: if the item's calendar is within `[minDate, maxDate]`, `curCal = copy` and `updateCalendarView()`).

## Constants decoded (used throughout)

| Raw value | Meaning |
|---|---|
| End mode `16` / `48` / `32` / `64` | Indefinite / "# of pays" / "on date" / "amount paid" (loan). Stored via `Payment.setEndMode`. |
| `freq_type` `100 'd'`, `119 'w'`, `109 'm'`, `121 'y'`, `87 'W'`, `76 'L'` | Day(s), Week(s), Month(s), Year(s), "Nth weekday in a month", "Last weekday in a month". |
| `flag_value` `94 '^'`, `59 ';'`, `35 '#'` | Mark as: None, Paid, Important. |
| `PROMPT_PASSWORD=1`, `PROMPT_EXPENSE=2`, `PROMPT_REMOVE_ADJUSTMENT=4` | `prompt()` flag bits. |
| `SPLIT_CANCEL=0`, `SPLIT_CONFIRM=1`, `SPLIT_CHANGE=2` | `showSplitOrChange` result. |
| `stateMask & 262144` / `& 65536` | CTRL held / ALT held. keyCode `115`='s', `13`=Enter. |
| `MessageBox` style `196` / `194` / `448` / `452` | ICON_WARNING+YES+NO (`confirmYesNo`, title "Confirmation") / ICON_QUESTION+YES+NO (`showYesNo`, title "Information") / YES+NO+CANCEL / ICON_WARNING+YES+NO+CANCEL. Return `64`=YES, `128`=NO, `256`=CANCEL. |
| `getSystemColor(2)` / `(3)` | black / red. |
| `SimpleCalendar.set/add(1, n)` / `(2, n)` | field 1 = day of month, field 2 = month. |
| `"＆"` (`FAKE_AMPERSAND`) | Every `&` typed by the user in a description/filter is replaced by fullwidth `＆` (SWT would treat `&` as mnemonic). |

Shared helpers called by these listeners (part03/part04):

- `confirmYesNo(shell, msg)` → MessageBox "Confirmation", Yes/No, returns Yes.
- `showYesNo(shell, msg)` → MessageBox "Information", Yes/No.
- `showError(shell, msg)` → error box.
- `checkEndPayment(numPaysBtn, numPaysTxt)` → true iff "# of pays" radio selected AND `parseInt(text) <= 1` (parse failure → stack trace, false).
- `updateEndSelection(mode, indefBtn, numPaysBtn, numPaysTxt, onDateBtn, onDateTxt, onDateImg, loanBtn, loanTxt)` → selects exactly the radio for `mode` and enables only the matching input(s): `48` enables numPaysTxt, `32` enables onDateTxt + calendar button, `64` enables loanTxt.
- `get_nth_week_msg(dateText)` → `arr[weekOfMonth] + " " + wDayName[dayOfWeek] + " in a month"` with `arr = {"1st","2nd","3rd","4th","5th"}`.
- `get_last_weekday_msg(cal)` → `"Last " + wDayName[dow] + " in a month"`.
- `pickDate(shell, cal[, title="Pick a Date"])` → modal date picker, returns picked `SimpleCalendar` or null.
- `fillPaymentsTable(table, selectPmt)` → re-fills the Payments table using `table.getData("filter")` (a `Filter`), sorted by `Budget.sortColumn.getData("comp")`; re-selects `selectPmt` (or the previous selection when null).
- `prompt(shell, image, title, message, initial, flags)` → see Budget$72..$78.

---

## Part A — Main window / lifecycle listeners

| Class | Installed at | Behavior |
|---|---|---|
| **Budget$4** `ShellAdapter.shellClosed` | `this.shellAdapter = new Budget$4(this)` in the constructor (part03:89); added with `mainShell.addShellListener(shellAdapter)` (part03:1533). | If `shellAdapterInvoked` is already true → nothing. Else set it true and call `saveAndCleanup()`; if that returns false → `event.doit = false` (close vetoed). `saveAndCleanup()`: if `!cfg.autosave && fileName != null` → MessageBox (YES/NO/CANCEL) title **"Save..."**, message **"Do you want to save your budget?"**: Yes → continue to save; No → `saveRc()` and return true (budget NOT saved); Cancel → `shellAdapterInvoked = false`, return false. Then `saveDB()` (false → return false), `saveRc()`, return true. `saveDB()`: if `fileName == null`: empty DB (no payments, no adjustments) → true, else `saveNewBudget()`; otherwise `db.save(fileName, dbKey, err)`, failure → `showError(mainShell, err)`. |
| **Budget$5** `Listener.handleEvent` | `display.addListener(SWT.Close(21), new Budget$5(this))` (part03:1532). | Prints `"display: SWT.close, shellAdapterInvoked=" + flag`. Same logic as $4 (guard flag, `saveAndCleanup()`, veto with `doit=false`); additionally if cleanup succeeded and `runningOnMac` → prints `"exiting on SWT.Close of the display"` to stderr and `System.exit(0)`. |
| **Budget$6** `Listener.handleEvent` | `mainShell.addListener(SWT.Resize(11), new Budget$6(this))` (part03:1534). | If `cfg.bestFit` → `do_best_fit = true`. The main event loop (part03:1557) consumes it when idle: `updateCalendarView()` (which clears the flag at part04:1859). |
| **Budget$7** `Runnable.run` | `display.timerExec(20000, new Budget$7(this, display))` once after the shell opens (part03:1539). | Every 20 s: if `mainShell` not disposed and `new SimpleCalendar()` (today) `!= initCal`: if `cfg.autoDateSwitch` OR `showYesNo(mainShell, "System date changed to a new date.\nDo you want to switch to the new day?")` → `initCal.set(today); rebuildApp()`. If the user answered No → the timer is NOT re-armed (never asks again this session). Otherwise re-arms itself with `timerExec(20000, this)`. |
| **Budget$8** `MouseAdapter.mouseDown` | `makeStatLabel(parent, name, value, g_type)` (part03:1813) — the value label of each quick-stats row, tooltip "Click to switch graph type". | If `g_type != cfg.graphType` → `cfg.graphType = g_type; graphCanvas.redraw(); forecastLbl.setText("Forecast: " + forecastType[cfg.graphType])`. |
| **Budget$9** `PaintListener.paintControl` | `this.lbl_underline = new Budget$9(this)` (part03:94); attached to labels elsewhere. | Draws a 1-px line along the bottom edge of the label (`y = size.y-1`, full width) in `cfg.colorCurrentMonth`. |
| **Budget$99** `MenuAdapter.menuShown` | `createEditMenu`: the "&Edit" menu (part04:1662). | `undoMoveMenuItem` ("&Undo transaction move", handler Budget$100 in part01) enabled iff `db != null && db.hasMoveUndo()`. |
| **Budget$79** `MouseAdapter.mouseUp` | `setMenu(control, menu)` (part04:1254) — only when `cfg.oneButtonMouse`. | Any mouse-up on the control shows the context menu (`menu.setVisible(true)`). |

---

## Part B — Edit Payment dialog (`editPayment(Shell parent, Payment pmt, SimpleCalendar cal)`, part04:1-472)

Dialog facts needed to understand the listeners: title **"New Payment"** when `pmt == null` else **"Edit Payment"**; `v4 = new_payment = (pmt == null)`; the edited object `v9` is a fresh `Payment` for new (end mode forced to Indefinite when `cfg.useIndefiniteEndMode`) or `pmt` itself for edit (fields are written directly on Ok). `org_amount = v9.amount` captured at open. Widgets: Description (`makeField`, tooltip "Use ';' as the separator. \nThe text before ';' will only be shown in the Calendar View."), Category combo (editable, tooltip "Clear this field to 'Uncategorize'\nUse File -> 'List of all payments' to rename or delete existing categories."), optional URL field (only when `opt_show_url`), Amount text (shows `abs(amount)` formatted; tooltip for existing payments "Suggested amount is: <6-month average per transaction>"), radios Expense/Income (Income selected iff `amount > 0`), Disabled checkbox (tooltip "Check it, if you want to disable this payment.\nThe payment transactions will still be placed in the SimpleCalendar,\nbut will not be used to calculate the balances."), Start Date text + calendar button (new payment with `cal` → start = cal), End group: radios Indefinite(16) / "# of pays:"(48)+text / "on date:"(32)+text+calendar button / "amount paid:"(64)+text, Grace period (days) text (tooltip "Use '0' to disable"), Pay Every: frequency text + radios Day(s)/Week(s)/Month(s)/Year(s)/"<Nth> <Weekday> in a month"/"Last <Weekday> in a month"; checkboxes Business Days Only / Early Pay (tooltip "Check this option if you want to pay on Fridays\ninstead of Mondays.") / Tentative (tooltip "Tentative transactions will automatically slide to the current date (if the current\ndate is in range) until adjusted or marked as paid. If transaction reaches the end \nof the pay period with no adjustment it will be removed from the calendar.\nCheck documentation for more details.") / Sticky (tooltip "Override transaction's default amount with the last adjustment\nin the calendar before this transaction's date, if available.\n"); Mark as: None/Paid/Important radios + "Highlighting" button + color swatch label; Image button (tooltip "Click here to select an image" or the image path) + " Quick\n Picks:" row; Delete (edit only), Ok (default button), Cancel. Initial enable state: frequency text disabled when `freq_type == 'W'`; Tentative disabled when end mode is "amount paid"; Business Days Only, "amount paid" radio and Grace days disabled when Tentative is checked; Early Pay enabled iff Business Days Only.

After the dialog closes: if `edit_ok`: new → `db.add(v9); edit_payments_new_pmt = v9`; edit → `db.updateMoveUndo(v9); db.update(v9); edit_payments_new_pmt = null`. Returns `db.updateCount` changed.

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$34** `ModifyListener` (fields `lastVal`, `lastTime`) | Category combo, only if there is at least one existing category (part04:74). | Inline auto-complete. Runs only when the combo has focus and `event.time != lastTime`. `text = combo text`; if `upper(text) == lastVal && text.length > 0` (the user pressed Backspace removing the auto-selected tail, so the typed prefix is unchanged) → `text = text minus last char`. `lastVal = upper(text)`. Candidates = categories whose upper-case starts with `lastVal`. Listener temporarily removed; if a candidate exists and text non-empty → `combo.setText(candidate[0]); setSelection(text.length, candidate.length)` (completed tail selected); else if `text.length != combo text length` → `combo.setText(text)`. Listener re-added; `lastTime = event.time`. |
| **Budget$35** `FocusAdapter` | Amount text (part04:128). | `focusGained`: select all (0..1000). `focusLost`: `s = text.trim()`; if `s[0] == '+'` → select Income radio, deselect Expense, strip the '+'. `v = Calculator.calculate(s, 0, 0)` (expression evaluator); `text = numFormatFrac2.format(abs(v))`; if `v < 0` → select Expense, deselect Income; foreground black. Any exception → foreground red and `setFocus()` back on the amount field. |
| **Budget$36** `FocusAdapter.focusGained` | Shared instance on "# of pays" text, "amount paid" text, frequency text. | Select all on focus. |
| **Budget$37** `SelectionAdapter` | Calendar button next to "on date:" (part04:194). | `pickDate(editShell, cal(on_date text))`; if not null → `on_date_txt = sDate(picked)`. |
| **Budget$38** `ModifyListener` | Frequency ("Pay Every") text (part04:243). | If `checkEndPayment(numPaysBtn, numPaysTxt)` (i.e. "# of pays" selected with value <= 1) → `updateEndSelection(16 Indefinite, ...)`. |
| **Budget$39** `SelectionAdapter` | Shared on Day(s)/Week(s)/Month(s)/Year(s) radios only (part04:261-264). The Nth/Last radios do not have it, but SWT fires Selection on the radio being deselected, so clicking Nth-weekday still triggers it through the previously selected radio. | Same `checkEndPayment` → Indefinite switch as $38; then `frequency text enabled = !nth_weekday.getSelection()`. |
| **Budget$40** `SelectionAdapter` | Business Days Only checkbox (part04:281). | `earlyPay.enabled = businessDaysOnly.selection`. |
| **Budget$41** `SelectionAdapter` | Tentative checkbox (part04:295). | `enabled = !tentative.selection` applied to: Business Days Only checkbox, "amount paid" radio, Grace days text. |
| **Budget$42** `SelectionAdapter` | Calendar button next to Start Date (part04:327). | `picked = pickDate(editShell, cal(start text))`; if not null: if `parseInt(numPaysTxt) == 0 && startText.equals(onDateText)` → `onDateTxt = sDate(picked)` (end follows start); `startTxt = sDate(picked)` (fires $43); `nth_weekday radio text = get_nth_week_msg(start)`; `lastWeekdayUpdater.update()`. |
| **Budget$43** `ModifyListener` | Start Date text (part04:328). | `nth_weekday radio text = get_nth_week_msg(startText)`; `lastWeekdayUpdater.update()`. |
| `lambda$editPayment$0` (the `Budget$Updater` "lastWeekdayUpdater", part06:2201) | Created at part04:258 and run once at dialog build. | `cal = cal(startText)`; last_weekday radio text = `get_last_weekday_msg(cal)`; `enabled = cal.isLastWeekday()`; if the Last-weekday radio is selected but the date is not a last weekday → deselect it and select Month(s). |
| **Budget$44** `SelectionAdapter` | Shared on the four End radios (Indefinite, # of pays, on date, amount paid) (part04:329-333). | `mode = (Integer) widget.getData()`; `updateEndSelection(mode, ...)`; `tentative.enabled = (mode != 64)`. |
| **Budget$45** `SelectionListener` (both methods) | "Highlighting" button (part04:364). | Shows the popup menu `v65` (menu data = `Budget$Info{cal=99990101, pmt, bg_color=pmt.getDefaultHilighting(), bg_label=swatch}`; items built by `hilighting_menu_listener` = Budget$130 in part01, which updates `info.bg_color` / swatch — out of this file's scope). |
| **Budget$46** `SelectionAdapter` | Image button (part04:383). | `path = selectImage(editShell)`; null (cancelled) → return. `path.length > 0` → button image = `getImage(image(path))`, else `icon("no-img.bmp")`. Button resized to its computed size; `setData(path)`; tooltip = path. |
| **Budget$47** / **Budget$48** `ModifyListener` | Description text / Category combo (part04:386-387). | `updateQuickPicks(quickPickContainer, descText + " " + catText, imgButton)`: upper-cases the string, collects up to 10 image names from every `ImgTag` whose `substring` occurs in it; if the set differs from `lastQuickPicks`, disposes children from index 2 onward (keeps the image button and the "Quick Picks:" label), creates one push button per image (image, data = path, tooltip = path) with Budget$33, and re-layouts. |
| **Budget$33** `SelectionAdapter` | Each quick-pick button (part03:2841). | `imgButton.setImage(clickedButton.getImage()); imgButton.setData(clickedButton.getData())` (the relative image path). |
| **Budget$49** `SelectionAdapter` | "Delete" button (edit mode only; `button.setData(pmt)`) (part04:409). | `confirmYesNo(editShell, "Are you sure you want to delete the '" + pmt.description + "' ?\n")` → `db.deletePayment(pmt)`; `editShell.dispose()`. (`edit_ok` stays false; `updateCount` changed so `editPayment` returns true.) |
| **Budget$50** `SelectionAdapter` | "Ok" button (part04:436). | See detailed algorithm below. |
| **Budget$51** `SelectionAdapter` | "Cancel" button (part04:441). | `editShell.dispose()`. |

### Budget$50 — Ok button algorithm (validation + save)

Every step sets a "current error message" (`v2`); any exception thrown anywhere in the body results in `showError(editShell, v2)` and the dialog stays open. The three category/description checks use `showError(mainShell, ...)` and `return` without exception.

1. `desc = description.trim()`; empty → error **"Description is empty!"**.
2. If the URL field exists: `url = trim`; empty → `null`; otherwise must start with `http://` or `https://` else error **"URL string must start with 'http://' or 'https://'"**.
3. `s = amount.trim()`; `expense = expenseRadio.selection`; if `s[0]=='-'` → `expense = true`; else if `s[0]=='+'` → `expense = false`. Error text **"Unable to parse the amount"**: `amount = abs(Budget.toDouble(s))`; if `expense` → `amount = -amount`. (Convention: stored negative = expense, positive = income.)
4. **"Unable to parse the start date"**: `start = BudgetDB.toDate(startText)` (int yyyymmdd).
5. **"Unable to parse the loan amount"**: `loan = toDouble(loanText)`. If the "amount paid" radio is selected: `p = max(abs(amount), 0.001)`; `p < 0.01` → error **"Payment amount is too small to calculate a schedule."**; `abs(loan)/p > 1000` → error **"'Amount Paid' schedule: number of transactions exceeds 1000."**
6. **"Unable to parse the # pays"**: `n_pays = parseInt(numPaysText)`. **"Unable to parse frequency"**: `freq = parseInt(freqText)`, `freq < 1 → 1`.
7. `freq_type`: default `'d'`(100); Week→`'w'`, Month→`'m'`, Year→`'y'`, Nth weekday→`'W'` **and `freq = 1`**, Last weekday→`'L'`.
8. `flag = '^'` (None); Paid→`';'`; Important→`'#'`.
9. **"Unable to parse the end date"**: `end = toDate(onDateText)`; `end < start → end = start`.
10. `cat = category.trim()`. `cat.equals(desc)` → `showError(mainShell, "Payment's description can't be the same as category.")`, return. `cat` non-empty and `!db.isValidCategory(cat, pmt)` → `showError(mainShell, "You already have a payment with the same description as the entered category: '" + cat + "'.\nPlease change the category of this payment.")`, return. `!db.isValidDescription(desc, pmt)` → `showError(mainShell, "You already have a category with the same name as the entered description: '" + desc + "'.\nPlease change the description of this payment.")`, return.
11. `doSplit = false; scheduleChanged = false`. If editing (not new): build a temp `Payment` with `startDate, endDate, n_pays, freq_type, frequency, disabled, endMode(from radios: Indefinite 16 / # pays 48 / on date 32 / else 64)`; `scheduleChanged = !pmt.isSameSchedule(temp)`.
12. If `cal != null` (dialog opened from a calendar transaction) and editing and (`org_amount != amount` or `scheduleChanged`):
    - `scheduleChanged` → `confirmYesNo(editShell, "You have changed a payment with existing transactions.\n\nALL THE PAST AND FUTURE TRANSACTIONS (except manually adjusted ones) WILL BE CHANGED TO THE NEW AMOUNT AND SCHEDULING!\n\nUse 'Alter transaction from ...' menu item if you want to alter the future transactions only.\n\nDo you want to apply the changes anyway?")`; No → return (dialog stays open).
    - only the amount changed: if `cal.get() == pmt.startDate && pmt.opt.size() == 0` (first transaction, no options) → just apply. Otherwise `showSplitOrChange(editShell, cal)`: `0` (Cancel) → return; `1` (SPLIT_CONFIRM, "future only") → `doSplit = true`; `2` (SPLIT_CHANGE, "all") → apply to all.
13. Apply to `pmt`: `description = desc.replace("&", "＆")`; `category = cat` or `null` if empty; **if `!doSplit`** `amount = amount`; `disabled, startDate, n_pays, frequency, freq_type, endDate, loan_amount`; `image_path = (String) imgButton.getData()`; `setNoWeekends, setEarlyPay, setTentative, setSticky` from checkboxes; `setDefaultHighlighting(info.bg_color)`; `setURL(url)` if URL field exists; **"Unable to parse the grace period days"**: `grace = parseInt(graceText)` clamped to `[0, 60]`; `setEndMode(...)` from radios; `flag_value = flag`; `edit_ok = true`.
14. If `doSplit`: `db.updateMoveUndo(pmt); db.update(pmt); edit_payments_new_pmt = null`; `info = Budget$Info{amount, cal, pmt, interactive=false}`; `splitPayment(info)` (non-interactive: skips its confirmation and splits the payment at `cal`, applying the new amount to the future part); `edit_ok = false` (so `editPayment` does not update again; `updateCount` already changed).
15. `editShell.dispose()`.

### `showSplitOrChange(shell, cal)` (part04:474) — Budget$52 / $53

Modal dialog titled **"Payment's Amount Changed"**: label "You have changed the payment's amount with past transactions.\n\nWhat do you want to do?\n"; radio A (pre-selected) **"Change the FUTURE transactions* only starting on <sdate(cal)>"**; radio B **"Change ALL the transactions* -- past and future"**; footnote "* -- manually adjusted transactions are not affected."; buttons **Accept** (default) and **Cancel**. Returns `Budget$1SplitValue.value` (initially 0).

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$52** | Accept | `value = changeAllRadio.selection ? 2 (SPLIT_CHANGE) : 1 (SPLIT_CONFIRM)`; dispose. |
| **Budget$53** | Cancel | dispose (value stays 0 = SPLIT_CANCEL). |

---

## Part C — Simple text dialogs

| Class | Dialog / widget | Behavior |
|---|---|---|
| **Budget$54** | `licAgree()` (part04:560) — "Budget Calendar License Agreement", read-only text of `license.txt`, buttons "I agree" (default) / "Cancel", size 640x480. "I agree". | `editInfo.update = true`; dispose. `licAgree` returns `update`. (Load failure → `showError(mainShell, "Unable to load license")`; empty → "Unable to display license".) |
| **Budget$55** | licAgree "Cancel" | dispose. |
| **Budget$56** | `showLog(shell, text)` (part04:631) — "Log" window, read-only text, "Ok" button (**created disabled**) / "Cancel". "Ok". | dispose (never reachable since disabled). |
| **Budget$57** | showLog "Cancel" | dispose. |
| **Budget$58** `KeyAdapter.keyPressed` | `editNotes(shell)` (part04:682) — "Budget Calendar Notes", multi-line text pre-filled with `db.notes`, tooltip "Use ^Enter or ^S for quick save.", Ok/Cancel, 640x480. On the text. | If (`keyCode == 's'` or `== Enter`) and CTRL held → `db.notes = remove_slash_r(text.trim())` (strips all `\r`); `update = true`; dispose; `doit = false`. |
| **Budget$59** | editNotes "Ok" | Same save as $58; dispose. `editNotes` returns `update`. |
| **Budget$60** | editNotes "Cancel" | dispose. |
| **Budget$61** `KeyAdapter.keyPressed` | `editPmtNote(shell, cal, pmt, title)` (part04:743) — per-transaction note. Title = `title` or `"<sdate(cal)>: <pmt.description>"`; icon = note.png for the adjustments pseudo-payment else the payment image; label "Note (keep it reasonable):"; multi-line text pre-filled with `pmt.getNote(cal.get())`; tooltip "Use ^Enter or ^S for quick save."; Ok/Cancel; size `max(400, 4*btnW) x max(7*btnH, 280)`. On the text. | Ctrl+S / Ctrl+Enter → `s = remove_slash_r(text.trim())`; `pmt.setNote(cal.get(), s.length > 0 ? s : null)`; `update = true`; `doit = false`; dispose. |
| **Budget$62** | editPmtNote "Ok" | Same as $61; dispose. Returns `update`. |
| **Budget$63** | editPmtNote "Cancel" | dispose. |

---

## Part D — Image chooser `selectImage(shell)` (part04:824)

Window "Select an image..." lists every file in the `images/` directory (sorted, directories skipped) as scaled labels in a grid of `floor(sqrt(n))+1` columns, preceded by a "no image" label (data `""`). Buttons "View Zoom-" / "View Zoom+" change `cfg.image_selection_zoom` by ±25 within `[monitorZoom, 2*monitorZoom]` and rebuild the grid (lambdas, part06:2177-2199). Returns `return_val`.

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$64** `MouseAdapter.mouseDown` | Every image label (`fill_images`, part04:869/897). | `return_val = (String) label.getData()` (path relative to `images/`, or `""` for no image); dispose. Closing the window without a click returns `null`. |

---

## Part E — Date picker `pickDate` / `fillPickDate` (part04:936-1100)

Shell with 7 equal columns, `setData("initial", copy of initial cal)`; header row: `<<`, `<`, month button (`"MMMMMMMMMMMMM,yyyy"` text, foreground `colorCurrentMonth`), `>`, `>>`, `Today`; weekday names (3 letters, `colorDatePickerHeader` / `bgColorDatePickerHeader`), leading/trailing filler labels in `bgColorCellOtherMonths`, day labels right-aligned (`bgColorCell` for days before today else `bgColorCellPast`, foreground `colorCalendarDay`, today's date in `colorCalendarToday`). The shell's `getData()` holds the month being displayed. `pickDate` returns `pickDateCal` (null when closed without choosing).

| Class | Button | Behavior |
|---|---|---|
| **Budget$65** | `<<` (tooltip "Previous Year") | `cal.add(MONTH, -12)`; `fillPickDate`; layout; redraw. |
| **Budget$66** | `<` ("Previous Month") | `cal.add(MONTH, -1)`; refill. |
| **Budget$67** | Month/year button | Builds a popup `Menu` with 12 items (day set to 1, month 0..11, text formatted with the same `SimpleDateFormat`), each with data = month index and listener Budget$67$1; shows it. |
| **Budget$67$1** | Each month item | `cal.set(DAY, 1); cal.set(MONTH, index)`; refill; layout; dispose the menu; redraw. |
| **Budget$68** | `>` ("Next Month") | `cal.add(MONTH, 1)`; refill. |
| **Budget$69** | `>>` ("Next Year") | `cal.add(MONTH, 12)`; refill. |
| **Budget$70** | `Today` ("Set Today Year and Month") | refill with `new SimpleCalendar(initCal)`. |
| **Budget$71** `MouseAdapter.mouseDown` | Every day label | `pickDateCal = copy(shell cal); pickDateCal.set(DAY, parseInt(label text))`; dispose the shell. |

---

## Part F — Generic `prompt()` dialog (part04:1138) — Budget$72..$78

`prompt(shell, image, title, message, initial, flags)`: modal shell (title, optional image), a `Link` showing `message` (its `<a>` links are clickable, link color `colorCurrentMonth`), an "expense" checkbox (only with `PROMPT_EXPENSE`; `exFlag.expense = initial.startsWith("-")`, and the leading `-` is stripped from the initial text), a text field (`setEchoChar('*')` with `PROMPT_PASSWORD`), optional "Remove adjustment" button (with `PROMPT_REMOVE_ADJUSTMENT`; tooltip "Click here to remove adjustment and dismiss this dialog.\nBeware: no confirmation asked!\n"), Ok / Cancel. Text gets focus with all selected. Resets `prompt_was_ctrl_ok`/`prompt_was_alt_ok` to false at entry.

Return value: if `return_val != null` and EXPENSE flag: `return_val.startsWith("+") && expense` → `"0:" + return_val.substring(1)` (forced income); else `(expense ? "1:" : "0:") + return_val`. Else if `return_val == null && exFlag.remove_adjustment && REMOVE_ADJUSTMENT flag` → `"2:"`. Else `return_val` (null on Cancel/close).

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$72** | "expense" checkbox | `exFlag.expense = chk.selection`. |
| **Budget$73** `VerifyListener` | Text field | Paste sanitizer: if the inserted `text.length > 5` and `numPattern` (`"(\D*)([\d,]+\.\d+)(\D*)"`) is set: iterate `matcher.find()`; each time `group(2)` (the numeric part like `1,234.56`) is not null → `event.text = group(2)`. With the EXPENSE flag the loop stops after the first match; without it, the last match wins. |
| **Budget$74** | The message `Link` | Clicking an `<a>` in the message: `text = link text`; focus; select all; with EXPENSE flag: `exFlag.expense = linkText.startsWith("-")` and the checkbox is set accordingly. (Used by callers that put suggested values as links in the message.) |
| **Budget$75** `widgetDefaultSelected` | Text field (Enter) | `prompt_was_alt_ok = ALT held; prompt_was_ctrl_ok = CTRL held; return_val = text; dispose`. |
| **Budget$76** | "Remove adjustment" button | `exFlag.remove_adjustment = true`; dispose (no confirmation). |
| **Budget$77** | Ok | Same as $75 (ctrl/alt flags recorded, `return_val = text`, dispose). |
| **Budget$78** | Cancel | dispose (`return_val` stays null). |

Helper local classes (part01): `Budget$1ExpenseFlag { boolean expense; boolean remove_adjustment; }`, `Budget$1SplitValue { int value = 0; }`.

---

## Part G — "Payments" window (File → List of all payments; `showPayments(shell)`, part04:1271)

Resizable shell "Payments" (icon det.bmp). Top: group "Filter" with a text (tooltip = `filterToolTipText`, initial text `allPaymentsSavedFilter`) and a "Clear" button; group "Colors" with four links "<a>Active</a> transactions", "<a>Past</a> transactions" (`colorPastPayment`), "<a>Future</a> transactions" (`colorFuturePayment`), "<a>Disabled</a> payment" (`colorDisabledPayment`), each with data `range:active` / `range:past` / `range:future` / `range:disabled`. Table (`table.setData("filter", allPaymentsSavedFilter)`, header + lines visible) with columns: `Img.`(30, left, not sortable), `Amount`(80, right, `amount_comp`), `Category`(120, `cat_comp`), `Description`(300, `desc_comp`, **initial sort column, ascending, shows up.gif**), `Start Date`(100, center, `start_date_comp`), `End Date`(100, center, `end_date_comp`), `# of pays`(80, right, `n_pays_comp`), `freq.`(50, right, `freq_comp`), `Bus.`(40, not sortable), `flags`(50, not sortable). "Close" button bottom-right. Window height forced to 600, width = computed+30. Returns whether `db.updateCount` changed.

`fillPaymentsTable` excludes payments with `id == 0` and applies `Filter.isMatch(pmt, pmt.amount, 0)`.

Comparators (`sort_direction` is +1/-1):

| Comparator | Definition |
|---|---|
| **Budget$80** = `cat_comp` | `c1 = category or ""`, `c2` likewise; equal → delegate to `desc_comp`; else `sort_direction * c1.compareTo(c2)`. |
| **Budget$81** = `desc_comp` | equal descriptions → delegate to `amount_comp`; else `sort_direction * d1.compareTo(d2)`. |
| `n_pays_comp`, `start_date_comp`, `end_date_comp` | `sort_direction * Integer.compare(...)` on `n_pays` / `startDate` / `endDate`. |
| `amount_comp` | `sort_direction * Double.compare(amount1, amount2)`. |
| `freq_comp` | `sort_direction * (frequency + freq_type as string).compareTo(...)` (string compare of e.g. `"2m"`). |

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$82** | The four color-legend links (shared) | `data = link.getData()`; if not null → `filterTxt = data`; `table.setData("filter", filterTxt.replace("&","＆"))`; `fillPaymentsTable(table, null)`. (Does not update `allPaymentsSavedFilter`.) |
| **Budget$83** | "Close" | dispose. |
| **Budget$84** `widgetDefaultSelected` | Filter text (Enter) | `table filter = text.replace("&","＆")`; `allPaymentsSavedFilter = same`; refill. |
| **Budget$85** | "Clear" | `filterTxt = ""`; table filter = ""; refill. (`allPaymentsSavedFilter` not changed.) |
| **Budget$86** | Every sortable column header (shared) | If `sortColumn == column` → `sort_direction = -sort_direction`; else `sortColumn = column; sort_direction = 1`. Clear images on all columns; set `up.gif` (dir 1) or `down.gif` on `sortColumn`; refill. |
| **Budget$87** `widgetDefaultSelected` | Table (double-click / Enter on a row) | `editPayment(dialogShell, rowPayment, null)`; if it returns true → refill (keeping selection). |
| **Budget$88** | Context menu "&Edit" | Selected row → `editPayment(dc, pmt, null)`; if changed → `fillPaymentsTable(table, pmt)`. |
| **Budget$89** | "&Add a new payment" | `editPayment(dc, null, new SimpleCalendar(initCal))`; if changed → `fillPaymentsTable(table, edit_payments_new_pmt)`. |
| **Budget$90** | "&Duplicate payment" | `dup = duplicatePayment(pmt)` = `pmt.clonePayment()`, `description += "(dup)"`, `db.add(dup)`, then `editPayment(mainShell, dup, null)` (note: the duplicate stays in the DB even if the dialog is cancelled); `fillPaymentsTable(table, dup)`. |
| **Budget$91** | "Delete payment" | `confirmYesNo(dc, "Are you sure you want to delete the '" + desc + "' ?\n")` → `db.deletePayment(pmt)`; refill. |
| **Budget$92** | "Add a 'wild' transaction" | `d = pickDate(dc, curCal, "Pick New Transaction Date")`; if not null → `pmt.setCustomAmount(d.get(), pmt.amount); db.update(pmt); fillPaymentsTable(table, pmt)`. |
| `all_transactions_menu_listener` | Cascade menus "All transactions" and "Matched transactions" (menu data `payment_table`, `shell`, and `matching-only="yes"` for the second) | Part01 (`MenuListener` field), out of scope here. |
| **Budget$93** | "Edit Category" | Only if the selected payment has a category. Loop: `n = prompt(dc, null, "Edit Category Name", "Enter a new category name:", current, 0)`; null or blank → return. `!db.isValidCategory(n, pmt)` → `showError(dc, "The new name is not valid, please choose a different one.")` and re-prompt. If any payment already has category `n` → `confirmYesNo(dc, "Edited category name matches an existing category name.\nDo you want to merge all payments of this category with the matched one?")`; No → return. Then every payment whose category equals the old one gets `category = n` (direct field assignment; no `db.update` call — see UNKNOWN). `fillPaymentsTable(table, pmt)`. |
| **Budget$94** | "Delete Category" | Only if selected payment has a category. `confirmYesNo(dc, "All payments that belong to this category will be set to an empty category name.\n\nDo you want to continue?")` → every payment with that category gets `category = null`; refill. |
| **Budget$95** | "Filter description" | table filter and filter text = `"desc:" + pmt.description`; `fillPaymentsTable(table, pmt)`. |
| **Budget$96** | "Filter category" | Only if category non-null: filter = `"cat:" + pmt.category`; refill. |

---

## Part H — "New Budget..." hint dialog (`showHint()`, part04:1500) — Budget$97 / $98

Shown after `createNew()` and at startup when the DB has no payments and no adjustments. Contents: budget icon; Link "You created a new budget!\nHave a glance at the <a>docs</a>, and the <a>tutorial</a>."; label "\nEnter the starting balance on <sdate(first day of current month)>:"; text "0"; grey note "leave this unchanged if not known, and change it later by double clicking on the balance field of a day."; "\nAdding payments -- right click on a day cell and choose 'Add a new payment' menu item\n"; checkbox "I use a one button mouse (or see File->preferences later)" (initial = `cfg.oneButtonMouse`); "Ok".

| Class | Installed on | Behavior |
|---|---|---|
| **Budget$97** | The docs/tutorial Link | link text `"docs"` → `Program.launch(html_name(jar_loc + "\doc\budget.html"))` (`file://` URL, backslashes → `/`, spaces → `%20` on Mac); `"tutorial"` → `Program.launch("http://www.mishell.ca/online/tutorial.html")`. |
| **Budget$98** | "Ok" | `v = Calculator.calculate(balanceText, 0, 0)`; exception → `showError(ed, "Invalid amount!\n" + ex.getMessage())`, refocus + select all, return. If `v != 0` or `cfg.oneButtonMouse != checkbox`: `cfg.oneButtonMouse = checkbox`; if `v != 0` → `cal.add(DAY, -1)` (last day of previous month) and `db.updateAdjustment(cal, v)` (opening balance adjustment); `updateCalendarView()`. Dispose. |

---

## Part I — Helper data classes

### Budget$Amount
Fields: `double amount`, `double planned_amount`, `String key` (description, or category name for aggregates), `HashMap sub_amounts` (key → Amount, for category aggregates), `Payment pmt`, `Amount agg_parent`. `is_aggregate()` = `sub_amounts != null && !empty`. `toString` = `Amount{'key', amounts=amount/planned}`.

### Budget$AmountKey
Immutable `(String category, String description)`. Constructors: from an `Amount` (`category = agg_parent != null ? agg_parent.key : null`, `description = key`), from `(description)` (category null), from `(category, description)`. `equals`/`hashCode` on both fields.

### Budget$AvgStats (result of `getSixMonthsAvg(description, cal)`)
`n` transactions found, `nm` distinct months with a transaction, `np` planned transactions in the range, `pt` = total/n (avg per transaction; NaN if n=0), `pm` = total/nm (avg per month), `pd` = pm/30.25 (avg per day), `pp` = total/np, `first`/`last` dates (int yyyymmdd; after scanning, `first` is snapped to the 1st of its month and `last` to the last day of its month), `total`, `lastTransactions` (list of `TranInfo`, newest first), `first_last_days_diff` (days between first and last, only when n >= 3). Scan: the 6 months before the 1st of `cal`'s month, non-disabled transactions whose `pmt.description` equalsIgnoreCase the given description.

### Budget$BarInfo
`int y, h; boolean clipped_top, clipped_bottom` — one graph bar's screen rectangle with clipping flags.

### Budget$BestScale
`double min, max; int nLines = 9` — graph axis scale.

### Budget$Border
`int top, left, bottom, right`.

### Budget$BestFit — calendar-cell description truncation
Fields: `GC s_gc` (measuring GC on the display), `int target_width`, `int cell_width = 10`, `final int cell_margin` (= default `GridLayout.marginWidth`, i.e. 5), `double labelCompensation = 1.0`, `Font numFont`.

- **Constructor**: creates the GC with the default Label font, measures `"This a text to Extent"` both with `GC.stringExtent` (w_gc) and with a temporary `Label.computeSize` (w_label); `labelCompensation = w_label / w_gc` (ratio converting GC text width to actual label width). Exceptions ignored.
- **setSize()** (called when `cfg.bestFit` at view build and after the shell opens maximized): `w = mainShell.clientArea.width`; if the shell is not yet visible and is maximized → `w = display.bounds.width`. `f = runningOnMac ? 2.0 : 1.5`. `cell_width = (int)((w - verticalScrollBarWidth * f - 16) / 7) + 2 - 2 - 2*cell_margin - 2 - 2` = `(int)((w - sb*f - 16)/7) - 2*cell_margin - 4`. `numFont` = whichever of `cfg.fontPosTranAmount` / `cfg.fontNegTranAmount` renders `"1,000.00"` wider.
- **setNumLength(s)** (called per month with `s = "-" + format(max |amount| in the month) + "*"`): `target_width = cell_width - (int)(extent_numFont(s).x * labelCompensation) - 1`, then `target_width = (int)(target_width / labelCompensation)` — i.e. the GC-space width left for the description after reserving the widest possible amount string.
- **getString(font, text, reserve)** (called per transaction label with `reserve` = extra width to keep free, e.g. for an image): if no GC → return text. `suffix = ""`; loop: if `extent_font(text + suffix).x < target_width - reserve` → return `text + suffix`; else if `text.length < 3` → return `"..."`; else `text = text minus its last 3 chars`, `suffix = "..."`, repeat. So descriptions are shortened 3 characters at a time and end with `"..."`.
- `do_best_fit` flag: set by Budget$6 on resize; main loop calls `updateCalendarView()` when idle.

### Budget$Bump — mouse-wheel "bump" month switching (used by Budget$12 in part01)
Constants `ACT_UP = 1`, `ACT_DOWN = 2`, `BUMP_DELAY = 1000` ms. Fields `bumped`, `ignoreUntilTime = -1`, `lastTime = -1` (event times masked to unsigned 32-bit).
- `isIgnoring(e)`: records `lastTime = e.time`; true if `e.time < ignoreUntilTime`.
- `getAction(e)`: sets `ignoreUntilTime = e.time + 1000`; returns `ACT_UP` if `e.count > 0` (wheel up) else `ACT_DOWN`.
- `bumped(e)`: first call → `bumped = true`, `ignoreUntilTime = e.time + 1000`, returns true (the wheel hit the scroll limit: swallow this event); second call → `bumped = false`, returns false (now allow the month switch).
- `reset()`: `bumped = false; ignoreUntilTime = -1`. `setDelay(e)`: `ignoreUntilTime = e.time + 1000`.
Semantics in Budget$12: while the calendar scroll can still move, scrolling resets the bump; when scrolling no longer changes the origin, the first wheel event at the limit is swallowed (`bumped`), the next one within/after the delay switches `curCal` by ±1 month (when `cfg.wheelSupport`, bounded by `minDate`/`maxDate`) and then ignores further wheel events for 1 s. Ctrl+wheel changes font size and sets the delay.

### Budget$ImgTag
`String substring` (upper-cased tag), `String[] images`. Loaded by `loadQuickPickTags()` from `images/image.tags`: each line `tag1,tag2,...=img1,img2,...` creates one ImgTag per tag sharing the image list.

### Budget$Info
`SimpleCalendar cal; Payment pmt; double amount; boolean interactive = true; int bg_color; Label bg_label; Menu menu = null`. Carrier for transaction-context operations (split, highlighting menu).

### Budget$Stats
`final boolean fullStats; SimpleCalendar startCal, endCal` (immutable copies); `Vector incomeVec, expenseVec; final HashMap statsMap; double total_income, total_expense, total_planned_income, total_planned_expense; int firstColWidth = -1`. `get_duration()` = `SimpleCalendar.getMonthsDiff(start, end)`; `get_range_string()` = `"MMM, yyyy"` when duration 0 else `"MMM, yyyy - MMM, yyyy (<duration+1>m)"`; `get_flat_map()` = `AmountKey → Amount` for every top-level Amount and every sub-amount.

### Budget$TranInfo
`int date; double amount; Payment pmt; Vector subinfo` — either `(date, amount, pmt)` or `(date, amount, subinfo)`.

### Budget$TransactionDND (enum)
`REPLACE, ADD_ANOTHER, ADD_SUM`. Used by `showTransactionChoices` (part05:243) with the labels "OVERWRITE the amount with <a>", "SUM both amounts: <b>", "ADD as a new transaction" under the message "The selected day already has a transaction like this.\n\nWhat do you want to do?\n" (default REPLACE).

### Budget$UpdateCallback / Budget$Updater
Both are single-method `update()` callbacks (abstract class / interface). `Updater` is used for the last-weekday radio updater in the Edit Payment dialog; `UpdateCallback` instances are stored on widgets under data tag `"updateCallback"`.

### Budget$comp_entry
`final String key; double v1, v2`. `comp_entry_from(vec1, vec2)`: one entry per `Amount` in vec1 with `v1 = amount`; for each `Amount` in vec2, find by key and set `v2` (or add a new entry with only `v2`); result filtered to entries where `v1 != 0 || v2 != 0`. (Two-period comparison rows.)

### Budget$editInfo
`boolean update = false` — dialog result holder.

### Budget$graphData
`SimpleCalendar cal; double val, sum; int x_max` — one of 12 graph columns (`graph_data[12]`).

### Budget$month_by_month_entry
`final AmountKey key; double total = 0; List amounts; String style = ""`. Constructed from a description string (category null) or from an `Amount`. `is_aggregate()` = `key.category == null`.

### Budget$span_color (record)
`(String background, String foreground)` — HTML report colors.

---

## UNKNOWN

- **Budget$93 (Edit Category)**: category is reassigned on the `Payment` objects directly with no `db.update(...)` call; whether this marks the DB dirty / bumps `updateCount` (and hence whether `showPayments` reports a change) depends on `BudgetDB` internals not read here.
- **Budget$45 / Budget$Info.bg_color**: the Highlighting menu content and how it writes `info.bg_color` / the swatch label is in `Budget$130` (part01), not verified here.
- **Budget$50 `reserve` argument of `BestFit.getString`** in the calendar builder (part05:1396, `v50`): its source (likely image width) was not traced.
- **Budget$7**: `SimpleCalendar.eq` is assumed to compare the date part only (the timer compares a fresh calendar against `initCal`).
- **Budget$34**: exact SWT re-entrancy behavior relied on via `event.time` comparison; the documented logic (`upper(text) == lastVal && text.length > 0 → drop last char`) is what the bytecode does (verified against regenerated pseudo code), but the intended UX for Backspace ("remove one more typed char") is inferred.

Verified pass: 2026-10-27 — 0 statements corrected, 1 confirmed.
