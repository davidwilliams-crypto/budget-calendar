# 07 – Listeners, part 1 (Budget$1..$32, Budget$100..$183, local classes)

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part01.txt` (listener bodies), install
sites found in part03..part06. Budget$4..$9 live in part02 but are included here for completeness of
the 1..32 range.

## 0. Shared facts needed to read the table

### Constants (Budget.<clinit>, part06 2213+)
| Name | Value |
|---|---|
| `mb_dnd_prefix` | `"MiShell*Budget*TID:"` |
| `SUM_QUESTION` | `"The selected day already has this type of transaction.\nDo you want to sum the amounts?\n"` |
| `PROMPT_PASSWORD / PROMPT_EXPENSE / PROMPT_REMOVE_ADJUSTMENT` | 1 / 2 / 4 (bit flags for `prompt()`) |
| `PROMPT_RET_REMOVE_ADJUSTMENT` | `"2:"` (prompt() return prefix); `"0:"`/`"1:"` = expense checkbox false/true |
| `hilight_color` (int RGB) | 0xFFD700 Gold, 0xFA8072 Salmon, 0xFFC0CB Pink, 0xFF7F50 Coral, 0xBDB76B Dark Khaki, 0xEE82EE Violet, 0x00FF00 Lime, 0x9ACD32 Yellow Green, 0x8FBC8F "Dark See Green" (sic), 0x87CEEB Sky Blue, 0xFFE4E1 Misty Rose |
| `AMOUNT_SEP` | `": "` |
| `GEN_HTML / GEN_EXCEL` | 0 / 1 |
| `BALANCE_NONE/ALL/FIRST_AND_LAST` | 0/1/2 |
| `allPaymentsSavedFilter` | `""` initially |
| `Budget$Bump.BUMP_DELAY` | 1000 ms; `ACT_UP`=1, `ACT_DOWN`=2 |

### Payment constants (Payment.txt)
`END_MODE_INDEFINITE=16`, `END_MODE_DATE=32`, `END_MODE_N_PAYS=48`, `END_MODE_LOAN_PAID=64`,
`DEFAULT_COLOR_DATE=99990101` (pseudo-date used to store the payment's default highlight colour),
`URL_NOTE_DATE=99990102` (pseudo-date whose "note" is the URL).
Flag chars: `';'` (59) = PAID, `'#'` (35) = IMPORTANT, `'^'` (94) = none/unpaid.

### SWT numeric codes seen in the pseudo code
| Code | Meaning |
|---|---|
| keyCode 16777223 / 16777224 | SWT.HOME / SWT.END |
| keyCode 16777221 / 16777222 | SWT.PAGE_UP / SWT.PAGE_DOWN |
| keyCode 16777217 / 16777218 | SWT.ARROW_UP / SWT.ARROW_DOWN |
| keyCode 13 / 115 | CR (Enter) / `'s'` |
| 262144 (keyCode or stateMask) | SWT.CTRL |
| `p0.button == 3` | right mouse button |
| MessageBox return 64 / 128 / 256 | SWT.YES / SWT.NO / SWT.CANCEL |
| `display.addFilter(1,..)` / `(37,..)` | SWT.KeyDown / SWT.MouseWheel |
| `addListener(21,..)` / `(11,..)` | SWT.Close / SWT.Resize |
| filters 2,3,4,5 | KeyUp, MouseDown, MouseUp, MouseMove |
| `p0.detail == 1 / 2` (DND) | DND.DROP_COPY / DND.DROP_MOVE; 0 = DROP_NONE |
| `new Menu(shell, 8)` / `(shell,4)` | SWT.POP_UP / SWT.DROP_DOWN (menus created with style 0 are used as pop-ups via `setMenu`) |
| MenuItem style 8 / 2 / 64 / 32 / 0 | PUSH / SEPARATOR / CASCADE / CHECK / default(push) |
| system colour 37 | SWT.COLOR_WIDGET_BACKGROUND |

### `Budget$Info` (the per-transaction label payload; part02 2575)
Fields: `cal` (SimpleCalendar, the day), `pmt` (Payment), `amount` (double, actual amount that day),
`interactive` (default true), `bg_color`, `bg_label` (used only by the Edit-Payment "Highlighting" button
pseudo-info whose `cal == 99990101`), `menu` (lazily created transaction context menu).

`Budget$TranInfo`: `date` (int yyyymmdd), `amount`, `pmt`, `subinfo` (Vector) – used by the "All
transactions" style menus. `Budget$AvgStats` (six-months average): `n` (# actual transactions),
`nm` (# months), `np` (# planned), `pt/pm/pd/pp` (avg per transaction/month/day/planned-period, NaN when
undefined), `first`, `last` (dates), `total`, `lastTransactions`, `first_last_days_diff`.
`BudgetDB$PaymentsRemaining`: `abs_amount`, `n`.

### Helper semantics referenced below (so the listener rows stay short)
* `updateCalendarView()` – synchronous rebuild of the calendar grid (returns immediately if `do_calendar_update==1`, i.e. a rebuild is already scheduled).
* `scheduleUpdateCalendarView()` – sets `do_calendar_update=1`; the main loop rebuilds after the current event.
* `rebuildApp()` – full shell rebuild.
* `set_pmt_flag(pmt, date, ch, amount)` – `pmt.setFlag(date,ch)`; if `pmt.isTentative()` and ch==';' also `pmt.setCustomAmount(date, amount)`; if tentative `db.update(pmt)`. NOTE: for non-tentative payments **no db.update is called** – the flag lives in the Payment object only.
* `Payment.moveTransaction(dst, src, amount)` – removes flag at src (remembers it), remembers note/highlight at src, `update(dst, amount, deleted=false)`, `update(src, amount, deleted=true)`, re-applies flag/note/highlight at dst and clears them at src.
* `Payment.setCustomAmount(date, amt)` = option(date).amount=amt, deleted=false. `Payment.setNoTransaction(date)` = option(date).amount=pmt.amount, deleted=true. `Payment.clear(date)` removes the option and the flag for that date ("restore to regular").
* `db.addMoveUndo(pmt, srcDate, srcAmount, dstDate, dstTransactionOrNull)` snapshots flag/note/highlight for src and (if existed) dst; `db.doMoveUndo()` restores both and `db.update(pmt)`; `db.hasMoveUndo()` gates the Edit menu item.
* `remove_adjustment(pmt, cal)` – if (endMode==LOAN_PAID && db.isRegularPaymentOn(pmt,date)) or pmt.isSticky(): `pmt.clear(date)` else `pmt.setCustomAmount(date, pmt.amount)`; then `db.update(pmt)`; `updateCalendarView()`.
* `deleteTransaction(cal, pmt)` – `pmt.setNoTransaction(date)`; `db.update(pmt)`.
* `duplicate_transaction(pmt, cal, amount)` – clone payment, `amount`=amount, start=end=date, `n_pays=1`, endMode=N_PAYS(48), `db.add(clone)`, `scheduleUpdateCalendarView()`; returns clone.
* `duplicatePayment(pmt)` – clone, description += `"(dup)"`, `db.add`, opens `editPayment(mainShell, clone, null)`; returns clone.
* `adjustAmount(cal, pmt, amount)` – prompt titled `"Amount Adjustment"`, text `"Enter the new amount, formulas are allowed.\nStandard amount is <a>X</a>. "` + optional `"Six months average is <a>Y</a>.\n"` + optional savings hint (`"You may put … to 'Savings'.\n"` / `"You may want to borrow <a href=..>N</a> from 'Savings'.\n"`) + `"Use: %n to calculate week's income percentage, or %%n month's income percentage."`; flags PROMPT_EXPENSE (+PROMPT_REMOVE_ADJUSTMENT when amount != pmt.amount). Returns false on cancel/empty. `"2:"` → `remove_adjustment`, return true. `$`/`$$` → recompute best-saving and re-prompt. Otherwise evaluates formula via `Calculator.calculate` (with week/month income when `%` present; expense checkbox negates positive results), sign-checks with `confirmYesNo` ("This type of transactions is Income.\nThe adjusted amount is negative (…). Is this correct?" / "This type of transactions is Expense.\nThe adjusted amount is positive (…). Is this correct?"), then `pmt.setCustomAmount(date, v)`, `db.update(pmt)`; Ctrl+OK (`prompt_was_ctrl_ok`) opens `pickWildPaymentDialog(mainShell, null, cal, v)` and sets the picked payment's amount that day to `amountBeforeDate − v` (transfer); Alt+OK opens `editPmtNote`; `scheduleUpdateCalendarView()`; returns true.
* `adjustBalance(cal)` – prompt `"Balance Adjustment"` / `"Enter new balance adjustment (formulas are allowed):"` (+ `"\nBalance before adjustment: <a href=..>X</a>."` and PROMPT_REMOVE_ADJUSTMENT when already adjusted); cancel → nothing; `"2:"` → `db.removeAdjustment(cal)`; else `Calculator.calculate(text)` (error → showError, re-prompt) and `db.updateAdjustment(cal, value)`; `updateCalendarView()`.
* `splitPayment(info)` – when `info.interactive`: `confirmYesNo` `"Payment: '<desc>'\nDo you want to alter transactions starting from <dd-MMM-yyyy>?\n\nThe Edit Payment dialog will be shown to allow altering of: amount, scheduling, description, etc.\n\nNote that preceding and manually adjusted transactions are not affected.\n"`. Then clones the payment starting at that date (endMode INDEFINITE kept, else DATE; endDate ≥ startDate), copies highlights ≥ date, `db.add(clone)`; original: endDate = day-before (if it ended later or was indefinite), endMode=DATE, `clear(date)`, `clearAfter(date)`, `setNoTransaction(date)`, `db.update(orig)`; opens `editPayment(mainShell, clone, null)`; if clone still in DB and `info.amount != pmt.amount` → `clone.setCustomAmount(date, info.amount)`; `db.update(clone)`; balance-consistency self-check of the day before (shows a "BUG REPORT…" log and `System.exit(1)` on mismatch unless explained by Early-Pay); `updateCalendarView()`. Non-interactive path: `clone.amount = info.amount; db.update(clone)`.
* `add_wild_transaction(cal)` – `pickWildPaymentDialog(mainShell, "", cal, 0.0)`; if a payment is picked: `pmt.setCustomAmount(date, pmt.amount)`, `db.update`, `updateCalendarView`, then `adjustAmount(cal, pmt, pmt.amount)`; if that returns false → `deleteTransaction(cal,pmt)` + `updateCalendarView()` (cancelling the amount prompt removes the just-added wild transaction).
* `editPmtNote(shell, cal, pmt, titleOrNull)` – note editor dialog; title = given or `"<dd-MMM-yyyy>: <pmt.description>"`; icon `note.png` for day notes (pmt == `db.adjustments`) else the payment image; returns true when the note changed. Day notes are stored as notes on the special payment `db.adjustments`.
* `hasMatches(filter, cal)` – true if any transaction in cal's month matches `filter`; returns false immediately if `db.getDay()` is null for any day.
* `showTransactionChoices(shell, icon, title, amount, sum)` – `MiSWT.selectChoice` radio dialog, message `"The selected day already has a transaction like this.\n\nWhat do you want to do?\n"`, choices in order: REPLACE = `"OVERWRITE the amount with <amount>"`, ADD_SUM = `"SUM both amounts: <sum>"`, ADD_ANOTHER = `"ADD as a new transaction"`; default REPLACE; buttons "Accept"/"Cancel"; returns null on Cancel.
* `createStatStartEndMenu(shell, months[], listener, lblA, lblB)` – pop-up month picker: years older than (currentYear−2) grouped in a cascade `"<min> - <max>"` (only when ≥2 such years), then one cascade per remaining year (the current year's months are flat in the root menu, surrounded by separators); each month item: text `"<Month>"` or `"<year>, <Month>"` for the current year, `setData(cal)`, `setData("sourceLabel", lblA)`, image `nxt.gif` on today's month; when lblA != null and a year has ≥2 months an extra first item `"<Month> <yr> to <Month> <yr>"` (data = min cal, `"sourceLabel2"`=lblB, `"sourceCal2"`=max cal) selects a whole year range.
* `getHtmlFileName()` – asks for/creates the output file name; null = cancelled.
* `flash_cell` – a cell set here is flashed by the next calendar view build (visual attention).

---

## 1. Master table

Legend for "Installed on": text in quotes is the exact `setText` of the menu item / widget.

### 1.1 Application-level listeners (Budget$1 – Budget$12)

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$1` (Runnable) | daemon thread in `isAppAlreadyRunning(port)` (part03 199) | Loops `serverSocket.accept(); close()` forever – single-instance lock. On IOException prints stack trace unless the socket was closed deliberately. |
| `Budget$2` (SelectionAdapter, captures `val$expired`) | trial window button (part03 1087), sibling of `Budget$3` | If `registration_already_started` is false: set it, `Program.launch("https://www.mishell.ca/")`; if `expired.value` → `System.exit(0)`. |
| `Budget$3` (SelectionAdapter, `val$expired`, `val$tWin`) | trial window "register" button (part03 1091) | `askRegistration()` (prompt "Registration" / "Enter registration key: "; invalid → error "The entered key is INVALID!"; valid → trial off, `updateShellTitle`, info "Registration complete.\nThank you for using Budget Calendar", `cfg.regkey` set, unlimitedHistory enabled, `saveRc`). On success `expired.value=false`, dispose trial window. |
| `Budget$4` (ShellAdapter) | `mainShell.addShellListener(shellAdapter)` (part03 1533) | `shellClosed`: if `shellAdapterInvoked` already → allow. Else set it; `saveAndCleanup()`; if it returns false → `e.doit=false` (veto close). |
| `Budget$5` (Listener SWT.Close on Display) | `display.addListener(21, …)` (part03 1532) | Prints debug line; same logic as $4 (`saveAndCleanup`, veto with `doit=false`); additionally on macOS when cleanup succeeded prints "exiting on SWT.Close of the display" and `System.exit(0)`. |
| `Budget$6` (Listener SWT.Resize on mainShell) | part03 1534 | If `cfg.bestFit` → `do_best_fit = true` (main loop then calls `updateCalendarView()` when idle). |
| `Budget$7` (Runnable timer) | `display.timerExec(20000, …)` (part03 1539) | Every 20 s: if shell alive and today != `initCal`: if `cfg.autoDateSwitch` or `showYesNo(mainShell, "System date changed to a new date.\nDo you want to switch to the new day?")` → `initCal.set(today)`, `rebuildApp()` (timer NOT rescheduled). If the user answered No → timer not rescheduled (never asks again). Otherwise reschedules itself for 20 s. |
| `Budget$8` (MouseAdapter, `val$g_type`) | value label of each quick-stat row created by `makeStatLabel()` (part03 1813); tooltip "Click to switch graph type" | mouseDown: if `g_type != cfg.graphType` → `cfg.graphType = g_type`, `graphCanvas.redraw()`, `forecastLbl.setText("Forecast: " + forecastType[g_type])`. Rows/types: "Opening Balance:"→0, "Closing Balance:"→1, "Minimum Balance:"→6, "Avg Daily Balance:"→2, "Maximum Balance:"→7, "Total Income:"→3, "Total Expenses:"→4, ""(disposable)→5. forecastType names: Opening Balance, Closing Balance, Average Balance, Total Income, Total Expenses, Disposable Income, Minimum Balance, Maximum Balance. |
| `Budget$9` (PaintListener `lbl_underline`) | month links, monthLabel, forecastLbl, "Full Summary", "Notes", stats-dialog From/To/Actions labels | Draws a 1-px underline across the full label width in `cfg.colorCurrentMonth`. |
| `Budget$10` (PaintListener `lbl_underline_textOnly`) | stats-window clickable transaction-name labels (part05 2677) | Underline only under the text extent (on mac from x=2 at the label bottom). |
| `Budget$11` (Listener, KeyDown display filter) | `display.addFilter(1, …)` (part03 1910) | Global keys (only when the focus control's shell is mainShell, or there is no focus control): `bump.reset()` first. HOME → unless focus is the filter text with non-empty text: `curCal = initCal` (day 1) → update. END → unless filter text non-empty: `flash_latest_adjustment()` (jump to the month of `db.getLatestAdjustmentDate()` and set `flash_cell` to that day; nothing if −1). PAGE_UP → previous month (no-op at `minDate` month). PAGE_DOWN → next month (no-op at `maxDate` month). keyCode CTRL (bare Ctrl press) → if `labelInFocus` (transaction label under mouse, not disposed) → `showStatsPopUp(labelInFocus)` (detailed payment info pop-up; dismissed by any key-up/mouse event via `Budget$159`). |
| `Budget$12` (Listener, MouseWheel display filter) | `display.addFilter(37, …)` (part03 1911) | Only when mainShell is the active shell. Ignores events with count==0 or while `bump.isIgnoring` (inside the 1 s window). Ctrl+wheel → `updateFontSize(+1 for up / −1 for down)` (all cfg fonts, rebuild) + `bump.setDelay`. Else if the calendar has a visible vertical scrollbar: scroll origin by `count`; if origin moved → `bump.reset()`, return; else if `bump.bumped()` (first hit of the edge) → return (a second wheel "bump" after the 1 s window is needed). Then if `cfg.wheelSupport`: `bump.getAction` (count>0 = ACT_UP → previous month, else ACT_DOWN → next month; range-limited by min/maxDate), `scheduleUpdateCalendarView()`, `bump.setDelay` (1 s ignore window). |

### 1.2 Details-pane / top bar (Budget$13 – Budget$32)

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$13` (MouseAdapter, `foldLabelListener`, `val$detail_ctnr`) | fold icon label (`fold.png`/`fold-light.png`, tooltip "Fold Details View") (part03 2039); also invoked programmatically when `cfg.startFolded` | Hides the details container (`GridData.exclude=true`, invisible), creates `unfoldComp = createUnfoldComp(detail_ctnr)` (compact bar: unfold icon, month link, "Filter:" text, `<a>Summary</a>`, `<a>Notes</a>`), moves it above, layout. |
| `Budget$14` (MouseAdapter) | the 12 month-name labels (`MMM` format starting at `initCal`'s month, first one in `fontCurMonthLink`; data = offset 0..11) (part03 2077) | mouseDown: target = `initCal` day 1 + offset months; if year or month differs from `curCal` → `curCal = target`, `updateCalendarView()`. |
| `Budget$15` (MouseAdapter) | `left_lbl` (left arrow image) (part03 2095) | **Previous month.** If a filter is active: walk back month by month from `curCal` until the `minDate` month or `hasMatches`. None found → `showInfo("No filter matching transactions found before this month!")`, return. If found and `getDiff(found, curCal) > 35` days: `confirmYesNoCancel("There is no filter matching transactions found in the previous month.\nDo you want to jump directly to the matching date (<MMM yyyy>)?")` – YES → `curCal.set(found)`, update, return; CANCEL → return; NO → fall through. Fall-through / no filter: if `curCal` month == `minDate` month → return; else `curCal.add(MONTH,−1)`, update. |
| `Budget$16` (MouseAdapter) | `right_lbl` (right arrow) (part03 2111) | Mirror of $15 going forward: texts `"No filter matching transactions found after this month!"` and `"There is no filter matching transactions found in the next month.\nDo you want to jump directly to the matching date (<MMM yyyy>)?"`; bounded by `maxDate`. |
| `Budget$17` (MouseAdapter) | `monthLabel` ("<Month>,yyyy") (part03 2127) | mouseDown → `label.getMenu().setVisible(true)` – month picker built by `updateMonthLabelMenu()` (all months minDate..maxDate via `createStatStartEndMenu` with `Budget$32`, no range items). |
| `Budget$18` (SelectionAdapter) | items of the Forecast pop-up menu, one per `forecastType[i]` (data = i) (part03 2148) | If i != `cfg.graphType`: set it, `forecastLbl.setText("Forecast: "+name)`, resize/redraw label, `graphCanvas.redraw()`. |
| `Budget$19` (MouseAdapter, `val$m`) | `forecastLbl` ("Forecast: …") (part03 2161) | mouseDown → shows the forecast menu. |
| `Budget$20` (MouseAdapter) | "Full Summary" label (tooltip "Click -- open full details stats window\nCtrl+Click -- to open at the default location with the default size\n") (part03 2190) | If Ctrl held: reset `cfg.statX/Y/Width/Height = −1`. Then `showStats(mainShell)` (modal stats window). |
| `Budget$21` (SelectionAdapter) | `tranFilterTxt` (the "          Filter:" Text in the details pane; tooltip = `filterToolTipText`) (part03 2204) | `widgetDefaultSelected` (Enter): `calViewFilter=null`; text = trimmed, `&`→U+FF06 (fullwidth ampersand, so SWT mnemonics are not eaten); if non-empty `calViewFilter = new Filter(text)`; `updateCalendarView()`. There is no ModifyListener – the filter applies on Enter only. |
| `Budget$22` (MouseAdapter) | "Notes" label (part03 2210) | `editNotes(mainShell)`; if changed → `notesText.setText(db.notes)` + tooltip. |
| `Budget$23` (PaintListener) | `graphCanvas` (part03 2225) | Fill client area with `cfg.bgColorDetails`, `drawGraph(gc)`. |
| `Budget$24` (MouseAdapter) with inner `$24$1..$4` | `graphCanvas` (part03 2227) | See §4 Graph canvas. |
| `Budget$25` (MouseMoveListener) | `graphCanvas` (part03 2228) | Tooltip while hovering a bar: for graph types 3/4/5 (income/expenses/disposable) and bar != first bar, prefix `"<sum> -- running total: <MMM, yyyy> - <MMM, yyyy>\n\n"` (number format `#,##0;(-#,##0)`). Then either `"DYNAMIC SCALING, use context menu to Freeze the current scaling\n"` (when both presets are 0) or `"FROZEN SCALING [ <min> .. <max> ]\nuse context menu to change to dynamic.\n"`. No bar under the mouse → tooltip unchanged. |
| `Budget$26` (PaintListener, `val$display`) | debug only: `debug_identify_children` (part03 2303) | Draws a red X and rectangle over child controls. Not reachable in normal use. |
| `Budget$27` (MouseAdapter, `val$detail_ctnr`) | unfold icon in `unfoldComp` (`unfold.png`/`unfold-light.png`, tooltip "Show Details View") (part03 2341) | Re-include details container, dispose `unfoldComp`, null `unfoldMonthSelector/unfoldFilterText`, `updateMonthLabelMenu()`, layout. |
| `Budget$28` (MouseAdapter) | `unfoldMonthSelector` Link (`<a>Month yyyy</a>`, tooltip "Click to choose a different month") (part03 2360) | Shows `monthLabel.getMenu()` (same picker as $17). |
| `Budget$29` (SelectionAdapter) | `unfoldFilterText` (folded-bar "Filter:" Text, width 200) (part03 2382) | Enter: copies its text (with `&`→U+FF06) into `tranFilterTxt` and fires `notifyListeners(SWT.DefaultSelection)` → runs `Budget$21`. |
| `Budget$30` (MouseAdapter) | `<a>Summary</a>` link in folded bar (tooltip "Click to open Full Summary Window") (part03 2389) | `showStats(mainShell)`. |
| `Budget$31` (MouseAdapter, `val$link`) | `<a>Notes</a>` link in folded bar (part03 2395) | `editNotes`; if changed update `notesText` text/tooltip and link tooltip `"Click to edit budget notes. Current notes:\n"+db.notes`. |
| `Budget$32` (SelectionAdapter) | items of the month-label picker menu (`updateMonthLabelMenu`, part03 2410) | data = SimpleCalendar; if `inRange(minDate,maxDate)` → `curCal = copy`, `updateCalendarView()`. |

### 1.3 Main menu bar (Budget$100 – Budget$118)

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$100` | Edit ▸ "&Undo transaction move" (`undoMoveMenuItem`; enabled by `Budget$99.menuShown` iff `db != null && db.hasMoveUndo()`) | `db.doMoveUndo()`; `updateCalendarView()`. |
| `Budget$101` (MenuListener, `val$menu`) | File menu (`addMenuListener`) | `menuShown`: dispose every item from index `recentFilesMenuIndex` until a SEPARATOR is reached, then re-add one PUSH item per `cfg.recentFile` at index `recentFilesMenuIndex+i`: text `"&<i+1>: <path>"` (`&` doubled), data `"file-name"`; item disabled when path equals the current `fileName`. (Max 5 recent files.) |
| `Budget$101$1` | each recent-file item | If path == current file → nothing. Else `saveDB()`; if that fails → return. `openBudgetFile(path)`; on failure `showYesNo("Do you want to remove this file from recently open file list?")` → YES removes it from `cfg.recentFile`. |
| `Budget$102` | File ▸ "&New" | `createNew()`. |
| `Budget$103` | File ▸ "&Open..." | `saveDB()` (abort if false); `FileDialog` (SWT.OPEN) filter `*.bgt`, path `getHomePath()`; if a non-empty name chosen → `openBudgetFile(path)`. |
| `Budget$104` | File ▸ "&Save" | `save(false)`. |
| `Budget$105` | File ▸ "save &As" | `save(true)`. |
| `Budget$106` | File ▸ "pass&Word" (`passwordMenuItem`; enabled when `dbKey != null` or `cfg.encryption`) | If a key exists: prompt "Current Password" / "This file already has a password\nEnter the current password" (PROMPT_PASSWORD); cancel → return; mismatch → error "The password is not correct!". Then prompt "New Password" / "Enter the new password:\n(leave this field empty to disable password)\n"; cancel → return; empty → `dbKey=null`, info "Password protection removed!". Else prompt "New Password Confirmation" / "Re-type new password:"; cancel → return; different → error "The entered passwords are different!"; else `dbKey = new`. (Nothing written until the next save.) |
| `Budget$107` | File ▸ "&List of all payments" | `showPayments(mainShell)`; if it returns true → `updateCalendarView()`. |
| `Budget$108` | File ▸ "edit notes" | `editNotes`; if changed → `notesText` text/tooltip = `db.notes`. |
| `Budget$109` (Listener) | File ▸ "preferences" (via `addMenuItem(menu,−2,…)`; on macOS also hooks the system Preferences item id −2) | `cfg.unlimitedHistory.hidden = cfg.unlimitedHistory.boolValue`; open `ConfigEditor(cfg,"Preferences",0)`; if OK → fadeOut, hide shell, `rebuildApp()`, show, focus, fadeIn. |
| `Budget$110` | File ▸ "&Print this month" | `getHtmlFileName()` (null → return); `saveHTML(file, curCal, curCal)`; `Program.launch(html_name(file))`; on exception error "Unable to save HTML file\n<ex>". |
| `Budget$111` | File ▸ "&Excel report" (disabled when `excel_prg==null`) | `generateReport(curCal, curCal, file, GEN_EXCEL)`; `excel_prg.execute(file)`; error "Unable to save the output file\n<ex>". |
| `Budget$112` | File ▸ "&Detailed report" | `generateReport(curCal, curCal, file, GEN_HTML)`; launch; same error text. |
| (lambda `createFileMenu$0`) | File ▸ "&Unpaid && paid report" | `generatePaidUnpaidReport(curCal day 1, curCal last day, file)`; launch; same error text. |
| `Budget$113` | File ▸ "E&xit" (not on macOS) | If `saveAndCleanup()` true: `shellAdapterInvoked=true`, remove shell listener, `mainShell.close()`. |
| `Budget$114` | Help ▸ "&Help " | `Program.launch(html_name(jar_loc + "\\doc\\budget.html"))`. |
| `Budget$115` | Help ▸ "&Tutorial" | `Program.launch("http://www.mishell.ca/online/tutorial.html")`. |
| `Budget$116` | Help ▸ "&Check for Updates Online" | Sets `https.protocols=TLSv1.2`; reads the first line of `https://www.mishell.ca/files/budget.version`; if `getVersion(remote) > getVersion(budgetVersion)` (major*1e6+minor*1e4+rev): `confirmYesNo("A newer version of Budget Calendar (<v>) is available.\nDo you want to download it?")`; YES → Windows: `msiSuggestUninstall` (unless jpackage) then launch `https://www.mishell.ca/files/BudgetCalendar.msi`; Linux: `…/BudgetCalendar.sh`; mac: `…/BudgetCalendar<arm64|>.dmg` (arm64 when `os.arch==aarch64`). Then `saveDB()` true → `System.exit(1)`, else info "Make sure that you exit Budget Calendar before installing the update!". Up to date → info "Your Budget Calendar is up to date.". Any exception → error "Unable to reach www.mishell.ca\n<ex>". |
| `Budget$117` (Listener) | Help ▸ "&About" (`addMenuItem(menu,−1,…)`, mac system About) | MessageBox (ICON_INFORMATION|OK) title "About", message `"Budget Calendar Version <v> (<java.version>)\nMiShell Software Systems, Ontario, Canada.\nhttps://www.mishell.ca/"`. |
| `Budget$118` | Help ▸ separator + "&Register" (only in trial) | `askRegistration()`; on success dispose the menu item. |

### 1.4 Transaction context menu (`createTransactionMenu`, part05 1715) – Budget$119..$145

Menu is built lazily on first right-click (or any button when `cfg.oneButtonMouse`) on a transaction's
image / description / amount label (`lambda$createCalendarView$5` mouseUp, stored in `Info.menu`). Parameters:
`cal`, `transaction`, `info`, `pmt`, `flag` char, `note` (or null). Items in order; every item has
`setData(info)`:

| # | Item text / style | Condition | Listener | Behaviour |
|---|---|---|---|---|
| 1 | "&Edit payment" | always | `Budget$119` (`m_edit_ad`) | `editPayment(mainShell, info.pmt, info.cal)`; true → `updateCalendarView()`. |
| 2 | "duplicate &Payment" | always | `Budget$120` (`m_duplicate_ad`) | `duplicatePayment(info.pmt)` (clone "(dup)", `db.add`, open editor) then `updateCalendarView()` unconditionally. |
| 3 | "alter transactions from <dd-MMM-yyyy>" | `pmt.getEndMode() != LOAN_PAID(64)` | `Budget$121` (`m_split_ad`) | `splitPayment(info)` (see helper; interactive → confirmation prompt). |
| – | separator | | | |
| 4 | "duplicate &Transaction" | always | `Budget$122` (`m_duplicate_transaction`) | `duplicate_transaction(info.pmt, info.cal, info.amount)` (new 1-payment clone on the same day with the current amount; schedules rebuild). |
| 5 | "&Move transaction" | always | `Budget$131` (`m_move_ad`) | `pickDate(mainShell, info.cal)` ("Pick a Date" calendar dialog). null or same date → nothing. `db.getDay(target)==null` → return silently. If the target day already has a transaction of this payment: `confirmYesNoCancel(SUM_QUESTION)`: CANCEL → return; YES → amount = info.amount + existing.amount; NO → amount = info.amount. `db.addMoveUndo(pmt, srcDate, info.amount, dstDate, existingOrNull)`; `pmt.moveTransaction(dst, src, amount)`; `db.update(pmt)`; `updateCalendarView()`. |
| – | separator | | | |
| 6 | "&All transactions" (CASCADE) | always | `Budget$124` (`all_transactions_menu_listener`) on the sub-menu (`setData(info)`, `setData("filter", calViewFilter)`) | See §2.1. |
| 7 | "&Six months average" (CASCADE) | always | `Budget$126` on sub-menu | See §2.2. |
| 8 | "payments remaining" (CASCADE) | `pmt.getEndMode() != INDEFINITE(16)` | `Budget$127` on sub-menu | See §2.3. |
| – | separator | | | |
| 9 | "&Filter these transactions" | always | `Budget$141` (`m_filter_transactions`) | `setFilterText("desc:" + getDesc(pmt.description).replaceAll(" ", "\\ "))` (spaces escaped as backslash-space; `setFilterText` also mirrors into `unfoldFilterText`); `calViewFilter = new Filter(tranFilterTxt.getText() with & → U+FF06)`; `updateCalendarView()`. |
| – | separator | | | |
| 10 | "&Delete transaction" | always | `Budget$140` (`m_delete_transaction`) | `confirmYesNo("Are you sure you want to delete this transaction ?\n'<getDesc(desc)>' on <dd-MMM-yyyy>")`; YES → `deleteTransaction(cal, pmt)` (marks option deleted), `updateCalendarView()`. |
| – | separator | | | |
| 11 | "ad&Just transaction amount" | always | `Budget$132` (`m_adjust_ad`) | `adjustAmount(info.cal, info.pmt, info.amount)`. |
| 12 | "&Remove adjustment" | `transaction.amount != pmt.amount` | `Budget$138` (`m_remove_adjustment`) | `remove_adjustment(info.pmt, info.cal)`. |
| 13 | separator + "URL: <url>" | `opt_show_url` && `pmt.getURL()!=null`; url shown without `https://`/`http://`, truncated to 29 chars + "..." when > 30 | `Budget$139` (`m_open_url`) | `Program.launch(item.getData("url"))` (full URL). |
| – | separator | | | |
| 14 | "mark as paid" (CHECK) | always; checked iff flag==';'; data "flag" = `""` when already paid else `";"` | `Budget$137` (`m_flags_ad`) | `set_pmt_flag(pmt, date, flag.isEmpty() ? '^' : flag.charAt(0), info.amount)`; `updateCalendarView()`. Toggle: paid→none, none/important→paid. |
| 15 | "mark as &Important" (CHECK) | checked iff flag=='#'; data "flag" = `""` when already important else `"#"` | `Budget$137` | Same mechanism: important→none, else→important. |
| 16 | "&Highlighting..." (CASCADE) | always | `Budget$130` (`hilighting_menu_listener`) on sub-menu | See §2.4. |
| – | separator | | | |
| 17a | "add &Note" | note == null | `Budget$134` (`m_edit_note_ad`) | `editPmtNote(mainShell, cal, pmt, null)`; true → `updateCalendarView()`. |
| 17b | "&Change note" + "remo&Ve note" | note != null | `Budget$134` / `Budget$133` (`m_remove_note_ad`) | Change = same as 17a. Remove: `confirmYesNo("Are you sure you want to delete this note:\n\n\"<note>\"\n")`; YES → `pmt.setNote(date, null)`, `updateCalendarView()` (no `db.update` call). |

Other transaction-label listeners (installed in `createCalendarView`, part05 ~1300-1500). "Normal mode" =
`calViewFilter == null || !calViewFilter.deleted`; "deleted mode" otherwise (filter expression starting with `-`).

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$144` (`mouse_edit`) | image label and description label of a transaction (normal mode) | `mouseDoubleClick`: `editPayment(mainShell, pmt, cal)`; true → update. `mouseDown` with Ctrl in stateMask: `editPmtNote(mainShell, cal, pmt, null)`; true → update. |
| `Budget$142` (`mouse_ad`) | amount label (normal mode) | `mouseDoubleClick`: `adjustAmount(cal, pmt, amount)`. |
| `Budget$143` (`restore_ad`) | image/description/amount labels **in deleted mode** (tooltip "Click to restore!") | `mouseDown`: `showYesNo("Do you want to restore '<pmt.description>'\non <dd-MMM-yyyy>?")`; YES → `pmt.clear(date)`, `db.update(pmt)`, `updateCalendarView()`. |
| `lambda$createCalendarView$5` (mouseUp) | image/description/amount labels (normal mode) | Right button (or any button when `cfg.oneButtonMouse`) → lazily `createTransactionMenu`, `menu.setVisible(true)`. |
| `Budget$164` (MouseTrackAdapter, one instance per calendar build) | description label (both modes) and amount label (normal mode) | `mouseEnter`: `labelInFocus = label` (enables the Ctrl pop-up); highlight colour = `cfg.bgColorHighlight`. If the payment has `grace_period_days>0` or `isTentative()`: `valid = db.get_valid_payment_days(pmt, cal)`; colour every calendar cell whose date is in `valid` with `cfg.bgColorImportant`; if the hovered day itself is **not** in `valid` → highlight colour becomes `cfg.colorNegativeAmount`; if `valid` contains the first day of next month → `showNextMonthGraceDays(nextMonth, valid)` (floating mini-calendar at the top-right of the calendar area with valid days coloured `bgColorImportant`; no drop targets). Apply the colour to the image (if any) else the label. `mouseExit`: restore each cell's `"bgcolor"`, restore label/image bg (`"bgcolor"` data or COLOR_WIDGET_BACKGROUND), `dismissNextMonthGraceDays()`, `labelInFocus=null`. |
| `Budget$157` (DragSourceAdapter) | DragSource on the description label (normal mode, `use_dnd`) | See §3. |

Transaction tooltips (for reference): description label `[Note:\n<note>\n\n]Transaction: <desc after ';'>\n[Category: <cat>\n]\n - Hold Ctrl: detailed payment information\n - Double Click: Edit payment\n - Drag this transaction to a different day  \n - Right-Click: Edit,Move,Delete,etc \n - Control+Click: Edit transaction's note`; amount label `Transaction: <desc>\nActual amount: <amt>    Regular: <pmt.amount>\n\n - Hold Ctrl: detailed payment information\n - Double Click: Adjust this transaction's amount  \n - Right-Click: Edit,Move,Delete,etc`.

### 1.5 Day-cell menu & day widgets (createCalendarView) – Budget$145/146/154/155/158/165–168

Per day cell a pop-up `Menu` (each item's data = immutable cal of the day) is built; it is attached to the cell
composite and to the day-number label only in normal mode. Items:

| # | Item | Condition | Listener | Behaviour |
|---|---|---|---|---|
| 1 | "&Add a new payment" (enabled iff no filter active) | always | `Budget$146` (`add_new`) | `editPayment(mainShell, null, cal)` (new payment starting that day); true → `updateCalendarView()`. |
| 2 | "add a &Transaction of ..." (enabled iff no filter) | always | `Budget$154` (`add_wild_ad`) | `add_wild_transaction(cal)` (see helper). |
| 3 | separator, "Mark all &PAID" (tooltip "Mark all non-important items as PAID"), "Mark all &UNPAID" (tooltip "Mark all non-important items as UNPAID") | day has transactions and normal mode | `lambda$createCalendarView$0` (menuShown, run once per menu, guarded by `"$menu$initialized$"` data) | Candidates = the day's transactions whose payment is not disabled and flag != '#'. paidCount = candidates with flag ';'. "Mark all UNPAID" enabled iff paidCount>0 → on click: for each candidate `set_pmt_flag(pmt, date, '^', 0.0)`; `updateCalendarView()`. "Mark all PAID" enabled iff candidates.size != paidCount → for each `set_pmt_flag(pmt, date, ';', tr.amount)`; update. Both disabled when no candidates. |
| 4 | separator, then "add &Note" (no day note) or "&Change note" + "remo&Ve note" | | `Budget$136` (`m_edit_day_note_ad`) / `Budget$135` (`m_remove_day_note_ad`) | Edit: `editPmtNote(mainShell, cal, db.adjustments, "<dd-MMM-yyyy>: Day's note")`; true → update. Remove: `db.getDayNote(cal)`; if non-null `confirmYesNo("Are you sure you want to delete this day's note:\n\n\"<note>\"\n")`; YES → `db.setDayNote(cal, null)`, update. |
| 5 | separator + one "Restore '<getDesc(desc)>'" per `day.getRestorable()` payment (enabled iff no filter) | restorable list non-null | `Budget$155` (`restore_transaction`) (data = cal, `"Payment"` = pmt) | `pmt.clear(date)`; `db.update(pmt)`; `updateCalendarView()`. |
| 6 | separator + "&Adjust balance (reconcile)" | only when the day shows **no balance label** (balance hidden for that day by the show-balance rules) | `Budget$168` | `adjustBalance(cal)`. |

In deleted mode the cell's day object is replaced by a synthetic `BudgetDay` holding the restorable payments
as transactions (flag `'`, 39) and no menu/double-click is attached ("Not interactive").
Day-cell tooltip: `[Note:\n<note>\n\n] Double-Click to:\n      add a wild transaction\n Right-Click to:\n   - Add a new payment\n   - Add/Change/Remove a note  [\n   - Restore transactions:\n    -- <desc>…]` or `"Not interactive: You are in 'Show deleted transactions' mode."`.

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$165` (MouseAdapter) | day cell composite (normal mode) | `mouseDoubleClick` → `add_wild_transaction(cal)`. |
| `Budget$158` (MouseListener `dayNumberMouseListener`) | day-number label (text `"<d>[`][ ...]"`: backtick suffix when restorable transactions exist, " ..." when a day note exists; bg `bgColorImportant` when a note exists; tooltip `[Note:\n<note>\n\n]double click to add or edit day's notes`) – attached in both modes | `mouseDoubleClick` → `editPmtNote(mainShell, cal (parent cell's data), db.adjustments, "<dd-MMM-yyyy>: Day's note")`; true → update. mouseDown/mouseUp no-ops. |
| `Budget$166` | balance-label pop-up menu item "&Adjust balance (reconcile)" (data = cal) | `adjustBalance(cal)`. |
| `Budget$145` (`m_remove_balance_adjustment`) | balance-label menu item "&Remove adjustment" (only when `day.closing_balance_adjusted`) | `db.removeAdjustment(cal)` (clears the adjustment option, recomputes balances); `updateCalendarView()`. |
| `Budget$167` (MouseAdapter) | balance label (normal mode) | `mouseDoubleClick` → `adjustBalance(cal)`. The balance label's menu is attached only in normal mode; its tooltip ends with `" Menu:\n   - Adjust balance (reconcile)     [\n   - Remove adjustment  ]"`. |

### 1.6 "Add a transaction of…" / transfer picker dialog (`pickWildPaymentDialog`, part04 2782) – Budget$147..$151

Dialog title `"Add a transaction of..."` (or, in transfer mode = called with a null filter string from
`adjustAmount` Ctrl+OK: `"Select a payment to transfer <abs amount> to.."` when amount<0 else `" from.."`, and the
label prefixed `"ONLY showing DISABLED and STICKY payments!\n"`). Label "Type to reduce the selection list...",
a Text (pre-filled, all selected) and a Tree (columns: 20px, 25px image, 250px description, amount width of
"-100,000.00") filled by `fillWildPaymentTable` (case-insensitive substring match on description; transfer
mode lists only disabled or sticky payments, and non-indefinite ones must have an option after one year
before the date; empty → one item `<no matching payments found>` with null data; item 0 selected; a single
item with children is expanded). Buttons: "New payment..." (not in transfer mode) and "Cancel". Modal loop
until the shell is disposed; returns `rc.pmt`.

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$147` | Tree `addSelectionListener` | `widgetDefaultSelected` (double-click/Enter on tree): exactly one selected item with non-null Payment data → `rc.pmt = payment`, dispose dialog. |
| `Budget$148` (KeyAdapter) | the Text | ARROW_UP/ARROW_DOWN/PAGE_DOWN/PAGE_UP → `doit=false`, `tree.setFocus()`. Enter (13) → if the tree is non-empty and item 0 has a Payment → `rc.pmt = it`, dispose. Else default handling. |
| `Budget$149` (ModifyListener) | the Text | Every change → `fillWildPaymentTable(tree, text, transfer_mode, cal)`. |
| `Budget$150` | button "New payment..." | dispose dialog; `edit_payments_new_pmt=null`; `editPayment(mainShell, null, cal)`; true → `updateCalendarView()`. (Dialog returns null.) |
| `Budget$151` | button "Cancel" | dispose dialog (`rc.pmt` stays null). |

### 1.7 Payments-list ("List of all payments") related – Budget$123, $124, $152, $153

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$123` (`deletedOptSelected`) | day items of the "deleted transactions" sub-menu (year ▸ month cascades built by `fillDeletedTransactionsMenu`; data = `PaymentOption`, optional `"closeThisShell"` = payments window) | target = option.date set to day 1; if in range: dispose `closeThisShell` (if given), `curCal = target`, `updateCalendarView()`, `flash_cell = cells[day−1]`. |
| `Budget$124` (`all_transactions_menu_listener`) | (a) transaction context menu "&All transactions" sub-menu (data=info, "filter"=calViewFilter); (b) payments-window table context menu "All transactions" and "Matched transactions" sub-menus (data "payment_table", "shell", and "matching-only"="yes" for the second) | See §2.1. |
| `Budget$124$1` (`val$closeThisShell`) | items created by `fillAllTransactionsMenu` from $124 (data = TranInfo) | cal=date day 1; if in range → dispose `closeThisShell` (payments window) if any, `curCal`, `updateCalendarView()`, `flash_cell = cells[day−1]`. |
| `Budget$152`, `Budget$153` | items of `pickPayment()` menus (part05 105) – payment chooser menu grouped by description with an "old payments" sub-menu | both: `pickPaymentPmt = item.getData()`. **`pickPayment()` has no caller in the pseudo code – dead code.** |

### 1.8 Filter info bar (shown above the calendar when `calViewFilter != null`) – Budget$160..$163

Bar (bg COLOR_WHITE=7? system colour 7, text in red system colour 3): red-cross icon (tooltip "Clear
transaction filter"), label `"Filter: '<expr>'"` or `"Filtering DELETED transactions: '<expr>'"`, then either
label "no matching transactions found." or two links `<a>Show in All Payments Window...</a>` and
`<a>All Matching Transactions...</a>`.

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$160` (MouseAdapter) | red-cross icon | `setFilterText("")`; if a filter was active → `calViewFilter=null`, `updateCalendarView()`. |
| `Budget$161` | items of `filterAllTransactionsMenu` (built by `fillAllMatchingTransactionMenu`: year cascades `"<yyyy>: <total>"`, month cascades `"<Mon>-<yyyy>: <total>"` when >1 in month, items `"dd-MMM-yyyy: <desc>   <amount><one-line note>"` disabled when the payment is disabled, `nxt.gif` markers on today's year/month, trailing disabled `"Total: <all> (<up to current month>/<after>)"`; empty → disabled "No transactions found[ -- try enabling 'Unlimited history' in Preferences.]") | cal = date day 1; in range → `curCal`, `updateCalendarView()` (no flash). |
| `Budget$162` | link "Show in All Payments Window..." | save `allPaymentsSavedFilter`, set it to `calViewFilter.filter`, `showPayments(mainShell)` (true → update), restore the saved value. |
| `Budget$163` | link "All Matching Transactions..." | `filterAllTransactionsMenu.setVisible(true)`. |
| `Budget$159` (Listener, `val$pmtInfoShell`) | display filters KeyUp/MouseDown/MouseUp/MouseMove while `showStatsPopUp` is open (part05 801) | Any such event disposes the pop-up (ending its nested event loop; the label tooltip is restored afterwards). |

### 1.9 Stats rows (quick stats pane & Full Summary window) – Budget$169..$171

`add_row(stats, parent, text, amount, perMonth, isSub, Budget$Amount, diff, isQuick)` creates the row label
with data "Amount" (`Budget$Amount`: `pmt`, `key`, `sub_amounts`, `amount`, …) and "Stats". Quick-pane rows
(`isQuick`) get `stat_ma`; Full-Summary rows get `stat_transactions_ma` (payment rows there are drawn as
links with `lbl_underline_textOnly`); all rows get `stat_mta`.

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$169` (`stat_ma`) | quick-stats category/payment rows in the main window | mouseDown: if Amount has a `pmt` → `calViewFilter = Filter("desc:<description, spaces → "\\ ">")`. Else by label text (trimmed): empty → return; "Total Income" → `">0"`; "Total Expenses" → `"<0"`; "Disposable Income" → return (no filter); otherwise `"cat:<text with spaces escaped>"`. `setFilterText(filter.filter)` (or "" if null); `updateCalendarView()`. |
| `Budget$170` (`stat_transactions_ma`) | Full Summary window rows | mouseDown: only when Amount.pmt and Stats present. Builds a pop-up on the active shell with `fillStatsTransactionMenu(menu, stats, pmt, Budget$170$1)` (all transactions in `stats.startCal..endCal` whose `getDesc(description)` equals this one, grouped year ▸ month, "Total: …" line; also returns the distinct Payment objects involved); if any: separator + one item per payment: text `"Edit payment"` when exactly one, else `"Edit payment [<sdate(start)> ... <sdate(end) or empty when indefinite>]"`, data=payment, listener `Budget$170$2`. Shows the menu. |
| `Budget$170$1` | transaction items in that menu | cal=date day 1; in range → `curCal`, `updateCalendarView()` (main window; the stats window stays open). |
| `Budget$170$2` | "Edit payment…" items | `cb = activeShell.getData("updateCallback")`; if present: `editPayment(activeShell, payment, null)` then `cb.update()` (→ `Budget$173` recomputes the stats window). |
| `Budget$171` (`stat_mta`) | all stats rows | mouseEnter: remember bg/fg in "bgcolor"/"fgcolor", set bg `cfg.bgColorHighlight`, fg white/black by darkness. If a filter is active → stop. If text == "Disposable Income": `getBestSavings(curCal)` (truncated to int) and `getMonthPlusMinus(curCal)`; for each calendar cell hide children 1..n−2 (transactions) and overlay a centred label `"<savings> [<plus-minus>]"` (0-dp format, fg `colorPositiveAmount`, bg `bgColorMaxBalance` for the max day / `bgColorMinBalance` for the min / widget bg otherwise, font `fontMonth`, GridData.exclude so it is disposed on exit). Otherwise if Amount has a pmt or sub_amounts: for each day with transactions, cells containing a matching transaction (same pmt object, or same description, or – when `sub_amounts` present – `key.equals(category)`) get bg `cfg.bgColorCellHighlight`, and in such cells all non-matching transaction children are hidden (day label and balance label kept). mouseExit: restore label colours; if filter active stop; else restore each cell bg from "bgcolor", dispose excluded overlay children, make all other children visible. |

### 1.10 Full Summary window toolbar – Budget$172..$183

| Class | Installed on | Behaviour |
|---|---|---|
| `Budget$172` (captures startLbl, endLbl, dfy, infoLbl) | items of both From/To month menus (`createStatStartEndMenu(shell, months, $172, startLbl, endLbl)` on the From label and `(…, endLbl, startLbl)` on the To label) | setRedraw off on both labels; `sourceLabel.setData(cal)`; if `"sourceLabel2"` present (whole-year item) `sourceLabel2.setData(sourceCal2)`; `full_stats_update(dfy, startLbl, endLbl, infoLbl)`; redraw on. |
| `Budget$173` (UpdateCallback) | `shell.setData("updateCallback", …)` | `update()` → `full_stats_update(...)`. |
| `Budget$174` (MouseAdapter) | From-date and To-date labels | mouseDown → `label.getMenu().setVisible(true)`. Each menu also carries a separator + "&Last 30 days" / "Last 365 days" (`lambda$showStats$1/2/3/0`): recompute `showStatsStats = calcStatisticsExact(today−29d | today−1y+1d, today, true)`, info label "(30d)" / "(1y)", `display_stats`. |
| `Budget$175` | Actions ▸ "Print" | `generateStatsReport(showStatsStats, file)`; launch; error "Unable to save HTML file\n<ex>". |
| `Budget$176` | Actions ▸ "Excel Report" (disabled without excel) | `generateExcelStatsReport(showStatsStats, file)`; `excel_prg.execute`; error "Unable to save output file\n<ex>". |
| `Budget$177` | Actions ▸ "Print Monthly Calendar" | `saveHTML(file, startLbl cal, endLbl cal)`; launch; error "Unable to save HTML file\n<ex>". |
| `Budget$178` | Actions ▸ "Monthly Excel Report" (disabled without excel) | `generateReport(start, end, file, GEN_EXCEL)`; execute; error "Unable to save the output file\n<ex>". |
| `Budget$179` | Actions ▸ "Monthly Detailed Report" | `generateReport(start, end, file, GEN_HTML)`; launch; same error. |
| `Budget$180` | Actions ▸ "&Unpaid && paid Report" | `generatePaidUnpaidReport(showStatsStats.startCal, showStatsStats.endCal, file)`; launch; same error. |
| (lambdas) | Actions ▸ separator, "&Comparative report to...", "&Month-by-Month report" | `compare_stats_report(shell, months, showStatsStats)` (pops the `Budget$183` month menu) / `report_month_by_month(shell, showStatsStats)`. |
| `Budget$181` (MouseAdapter, `val$m`) | "Actions..." label | shows the Actions menu. |
| `Budget$182` (DisposeListener) | stats shell | saves bounds into `cfg.statX/statY/statWidth/statHeight`. |
| `Budget$183` (captures base_stats) | items of the "&Comparative report to..." month menu (months in `[start − duration months, end]`) | `do_comparative_report(base_stats, chosenCal)`. |

### 1.11 Local classes
* `Budget$1Bool { boolean value }` – mutable flag (`expired`) shared by trial-window listeners $2/$3.
* `Budget$1ExpenseFlag { expense=false; remove_adjustment=false }` – result holder of `prompt()` (expense checkbox / "remove adjustment" button).
* `Budget$1Helper { abstract add_year(Menu, int year, boolean asCascade) }` – local interface used by `createStatStartEndMenu` (implemented by lambda `createStatStartEndMenu$0`).
* `Budget$1SplitValue { int value=0 }` – holder for the split dialog result (`SPLIT_CANCEL=0/CONFIRM=1/CHANGE=2`).
* `Budget$1TranEntry { int date; Transaction tr }` and `Budget$1TranSum { amount, income, expenses }` – `TranSum(Vector<TranEntry>)` sums amounts <0 into `expenses`, ≥0 into `income`, `amount = income+expenses` (used by report generation).
* `Budget$1pmtRc { Payment pmt=null }` – result holder of `pickWildPaymentDialog`.

---

## 2. Cascade sub-menus of the transaction context menu

### 2.1 "&All transactions" – `Budget$124.menuShown`
1. `info = menu.getData()`, `filter = menu.getData("filter")`; if null and a `"payment_table"` is attached, use the table's `"filter"` data string (trimmed, non-empty) as a `Filter`.
2. `matchingOnly = menu.getData("matching-only") != null`.
3. If the menu already has items **and** `info != null` → return (filled once). With info==null (payments window) it is refilled on every show.
4. Determine payment & reference date: with `info` → `info.pmt`, `info.cal`; without → selected row of `payment_table` (none selected → dispose all items and return), date = today.
5. If a `"shell"` is attached dispose all existing items.
6. `fillAllTransactionsMenu(pmt, menu, filter, matchingOnly, refCal, new Budget$124$1(shell))`:
   * scans every day from `minDate` while `db.getDay()` is non-null, collecting transactions whose `pmt.description` equals the payment's description (if `matchingOnly` and a filter exists, only those `filter.isMatch`).
   * None → single disabled item `"No transactions found"` + `" -- try enabling 'Unlimited history' in Preferences"` when `!cfg.unlimitedHistory`.
   * Else year cascades `"<yyyy>: <year total>"` (`nxt.gif` on refCal's year); inside, month cascades `"<Mon>-<yyyy>: <month total>"` only when the month has >1 transactions (otherwise the day item sits directly under the year); day items `"[* ]dd-MMM-yyyy: <amount><one-line note>"` (`* ` prefix when it matches the filter, propagated up to the parent cascade texts), disabled when the payment is disabled, `nxt.gif` on items of refCal's month placed directly under the year, data = TranInfo, listener `$124$1`. Totals exclude disabled payments.
   * Trailing separator + disabled `"Total: <all> (<through current month>/<after>)"`.
   * If `!matchingOnly`: `db.getAllDeletedOptions(desc, minDate, maxDate)` non-empty → separator + cascade "deleted transactions" (year ▸ month ▸ day items, listener `Budget$123`, `nxt.gif` on current year/month).

### 2.2 "&Six months average" – `Budget$126.menuShown` (filled once)
`avg = getSixMonthsAvg(pmt.description, info.cal)`. `avg.n == 0` → single disabled item "no data found.".
Else disabled items: `"<MMM-yyyy> to <MMM-yyyy>"` (`sdate().substring(3)` of first/last), `"# of tran. actual/planned: <n>/<np>"`,
`"# of months: <nm>"`, `"(click an item to copy)"`; then via `add_six_mi` (skipped when the value is NaN) items
`"<label>:\t<value 2dp>"` with data = formatted value and listener `Budget$125`: "Avg. per transaction" (pt),
"Avg. per month" (pm), "Avg. per day" (pd), "Avg. per planned period" (pp); if `first_last_days_diff != 0` and
`k = round(first_last_days_diff/(n−1)) >= 1`: `"Avg. amount per <k> days"` = total/first_last_days_diff·k; finally "Total amount" (total).
`Budget$125`: copies the item's data string to the system clipboard (TextTransfer).

### 2.3 "payments remaining" – `Budget$127.menuShown` (filled once)
`r = db.getPaymentsRemaining(pmt, cal)`; three items via `add_rem_item` (data = text, listener `Budget$125` → copy):
`"<r.n>"`, `"Amount: ~<currencyPrefix><roundUp(|abs_amount|) 0dp><currencySuffix>"`, `"Last Payment on: <sdate(pmt.endDate)>"`.

### 2.4 "&Highlighting..." – `Budget$130.menuShown` (filled once); also used by the Edit-Payment dialog "Highlighting" button (`Budget$45` shows it; info.cal = 99990101 pseudo date, `bg_label` = colour swatch label)
Items: "Custom" (data "info", listener `Budget$129`; shows a 40×20 swatch image of the current colour when
`pmt.getHilighting(date) != −1`); separator; one item per `hilight_color_name[i]` with 40×20 swatch, data
"info"+"color"=int, listener `Budget$128`; separator; "default" (color = −1, listener `Budget$128`).
* `Budget$128`: needs info & color, else return. If `info.cal != 99990101` → `pmt.setHilighting(date, color)`, `updateCalendarView()` (no db.update). Else (edit dialog) `info.bg_color = color`; swatch bg = parent bg when −1 else `int2color`.
* `Budget$129`: `ColorDialog` on mainShell, preset to the current highlight (if any); on Windows `setRGBs(get_customer_rgbs(db))` (custom palette from colours already used in the DB); on OK same application as $128 with `(r<<16|g<<8|b)`.

---

## 3. Drag & drop (`Budget$157` source, `Budget$156` target)

Enabled when `use_dnd` (default true) and in normal mode. DragSource (DND.MOVE|COPY = 3, TextTransfer)
is on the **description label**; DropTargets (MOVE|COPY, TextTransfer) are on every day cell of the current month
(data = immutable cal), on the leading/trailing "other month" cells (data = that date; only when those dates are
inside `minDate..maxDate`), and on every day label of the floating previous/next-month shells.

### `Budget$157` (DragSourceAdapter)
* `dragStart`: `dropShellTargetPrev = createDropTargetShell(mainShell, cal−1 month, null)` – a borderless always-on-top shell (style 16392) with bg `cfg.bgColorImportant` showing "<MMM yyyy>" title (`sdate_MY`), 3-letter weekday headers, and one right-aligned day label per day (bg `bgColorCell` for dates before today else `bgColorCellPast`; each label is a DropTarget with the full `dropTargetAdapter`); placed at the calendar's absolute top-left shifted up by its own height (y clamped ≥ 0). Then disposes any existing `dropShellTargetNext`, `dismissNextMonthGraceDays()`, creates `dropShellTargetNext` for cal+1 month at the top-right (x = client width − shell width − 2). Both made visible.
* `dragSetData`: for TextTransfer: `data = mb_dnd_prefix + cal.get() + ":" + pmt.id + ":" + amount + ":" + pid` (e.g. `MiShell*Budget*TID:20260115:42:-123.45:1234`).
* `dragFinished`: disposes both floating shells (null-safe). Also called manually by the drop handler with `null`.

### `Budget$156.drop`
1. If `data == null` or detail not MOVE(2)/COPY(1): `detail = DROP_NONE`, `dragSourceAdapter.dragFinished(null)`, return.
2. Text must be longer than the prefix and start with `mb_dnd_prefix`; otherwise just `dragFinished`.
3. Split on `':'` → `[prefix, srcDate, pmtId, amount, pid]`; `dstDate = target.getData().get()`.
4. `pid != ProcessHandle.current().pid()` → `showInfo("Budget Calendar doesn't support cross process Drag and Drop.\nIf this is not a mistake, please email support@mishell.ca with details of why you want to drag-and-drop from one budget to another.\n")`; `dragFinished`; return.
5. `dstDate == srcDate` → nothing (dragFinished).
6. `pmt = db.getByID(id)`; `day = db.getDay(dst)`; null day → dragFinished, return. `amount = parseDouble(text)`; `existing = day.get(pmt)`.
7. **Existing transaction of the same payment on the target day**: `sum = amount + existing.amount`; `choice = showTransactionChoices(mainShell, budgetIcon(), "Transaction Copy" | "Transaction Move", amount, sum)`:
   * null (Cancel) → dragFinished, return (no change).
   * ADD_SUM → `amount = sum`, then as REPLACE.
   * REPLACE → COPY: `pmt.setCustomAmount(dst, amount)`; MOVE: `db.addMoveUndo(pmt, src, amount, dst, existing)`, `pmt.moveTransaction(dst, src, amount)`.
   * ADD_ANOTHER → `duplicate_transaction(pmt, dst, amount)` (new one-off payment); if MOVE additionally `pmt.setNoTransaction(src)`.
   * anything else → `RuntimeException("Unsupported DND operation!")`.
8. **No existing transaction**: COPY → `pmt.setCustomAmount(dst, amount)` then `adjustAmount(dst, pmt, amount)` (prompts for the amount; cancelling leaves the copied transaction in place); MOVE → `db.addMoveUndo(pmt, src, amount, dst, null)`, `pmt.moveTransaction(dst, src, amount)`.
9. `db.update(pmt)`; `scheduleUpdateCalendarView()`; `dragFinished(null)`.

Move vs Copy is decided by SWT (`event.detail`): plain drag = MOVE, Ctrl-drag = COPY (platform default); the code never changes `detail` except to cancel.

---

## 4. Graph canvas (`Budget$24` mouseDown, `Budget$25` move)
`dynamic = (cfg.graphPresetMin == 0 && cfg.graphPresetMax == 0)`.
* **Right button (3)**: pop-up menu: if dynamic "Freeze current scaling" (`$24$3`: presets = `graph_last_min/max`, redraw) else "Use dynamic scaling" (`$24$4`: presets = 0,0, redraw); separator; "Custom Maximum" (`$24$2`); "Custom Minimum" (`$24$1`).
  * `$24$1`: prompt title "Custom Value", text `"Custom hint for minimum to freeze:\nThis hint value may be adjusted to have the best visual appearance."`, default = current min (`0.00` format); on OK `graphPresetMin = parseDouble(v)`; if `graphPresetMax == 0` → `graphPresetMax = graph_last_max`; redraw.
  * `$24$2`: same for the maximum ("Custom hint for maximum to freeze:…"), fills min from `graph_last_min` when 0.
* **Left click inside `graphHotSpots[0]`** (max-axis label area) → runs the Custom Maximum action; **inside `graphHotSpots[1]`** → Custom Minimum.
* **Left click on a bar** (`getGraphData(x,y)` non-null): target = bar's month, day 1; if it differs from `curCal` (year or month) → `curCal = target`, `updateCalendarView()`, tooltip cleared.

---

## 5. Keyboard / mouse shortcut summary
| Key | Where | Effect |
|---|---|---|
| Home | main window (not while the filter text has content and focus) | go to today's month |
| End | main window (same caveat) | jump to the month of the latest balance adjustment and flash the cell |
| Page Up / Page Down | main window | previous / next month (bounded by min/maxDate) |
| Ctrl (press) | while hovering a transaction label | detailed payment pop-up (`showStatsPopUp`) |
| Ctrl + mouse wheel | main window | font size ±1 (all calendar fonts) and rebuild |
| Mouse wheel | calendar | scroll; at the edge, after a 1 s "bump", previous/next month when `cfg.wheelSupport` |
| Enter in filter text | details pane / folded bar | apply filter |
| Enter / arrows in "Add a transaction of…" text | dialog | select first match / move focus to the tree |
| Ctrl + click on transaction description or image | calendar | edit the transaction's note |
| Ctrl + click on "Full Summary" | details pane | open stats window with default geometry |
| Double-click day cell / day number / transaction name or image / amount / balance | calendar | wild transaction / day note / edit payment / adjust amount / adjust balance |
| Right-click transaction / day cell / balance | calendar | context menus (§1.4, §1.5) |
| Ctrl+OK, Alt+OK in the Amount Adjustment prompt | prompt | transfer amount to another payment / edit note afterwards |

---

## 6. UNKNOWN / undetermined
* `Budget$26`: only reachable from `debug_identify_children`, whose callers were not traced (assumed debug-only).
* `Budget$152`/`Budget$153`: `pickPayment()` has no caller anywhere in the pseudo code – treated as dead code; its menu structure (grouping of same-description payments, "old" payments sub-menu, `isOldPayment` rule) was not fully transcribed.
* `Budget$147`: meaning of tree column index 1 (width 25) beyond holding the payment image – not needed for behaviour.
* `Budget$11`: SWT keyCode 262144 is `SWT.CTRL`; on macOS the code compares the literal CTRL constant, so whether the Cmd key triggers the pop-up there is UNKNOWN.
* `Budget$12`: the semantics of `Budget$Bump.bumped()` for the two-step month switch were inferred (first wheel event at the scroll edge is swallowed and starts a 1 s ignore window; the next non-ignored edge event switches the month).
* `Budget$156`: the SWT platform decides MOVE vs COPY (`event.detail`); the Ctrl-for-copy convention is assumed from SWT defaults, not visible in the code.
* `Budget$116`: `msiSuggestUninstall` body not read (part03 1607).
* `Budget$160`-bar colours: system colour 7 for the bar background and 3 for the text were not mapped to names (likely COLOR_WHITE / COLOR_RED).

Verified pass: 2026-10-27 — 0 statements corrected, 0 confirmed.
