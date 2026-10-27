# 04 — Dialogs, Menus, Editing (Budget.part04)

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part04.txt` plus the anonymous listener classes it installs (`Budget$33`–`$130`, `$147`–`$151` in part01/part02), helper methods in part03/part05/part06, `Payment.txt`, `Calculator.txt`.

Conventions used below:
- Dates in the DB are `int` `YYYYMMDD`. `Budget.sdate(x)` formats as `dd-MMM-yyyy` (e.g. `05-Mar-2026`). `BudgetDB.sDate(int)` / `toDate(String)` use `YYYY-MM-DD` text (the format the edit fields use).
- `numFormatFrac2` = locale `DecimalFormat` with exactly 2 fraction digits and grouping (e.g. `1,234.50`). `numFormatFrac0` = 0 fraction digits.
- `showError(shell,msg)` = MessageBox ICON_ERROR titled **"Error"**. `showInfo` = ICON_INFORMATION titled **"Information"**. `confirmYesNo` = ICON_QUESTION|YES|NO titled **"Confirmation"** → true on Yes. `confirmYesNoCancel` = ICON_QUESTION|YES|NO|CANCEL titled "Confirmation", returns the SWT code (YES=64, NO=128, CANCEL=256). `showYesNo` = ICON_INFORMATION|YES|NO titled "Information".
- All dialogs are application-modal child shells with their own `readAndDispatch` loop; closing via the window X behaves like Cancel.
- `centerLoc(size)` centers on the main shell if visible, else on the display (size clamped to the parent bounds).
- Payment flag chars: NONE `'^'` (94), PAID `';'` (59), IMPORTANT `'#'` (35). Frequency type chars: `'d'` day, `'w'` week, `'m'` month, `'y'` year, `'W'` nth weekday of month, `'L'` last weekday of month. End modes (bits 0xF0 of `flags`): INDEFINITE=16, DATE=32, N_PAYS=48, LOAN_PAID=64 ("amount paid"). Other flag bits: NO_WEEKENDS=1, TENTATIVE=2, EARLY_PAY=4, REMINDER=8, IS_STICKY=256.
- Special note/hilight dates: `DEFAULT_COLOR_DATE = 99990101` (payment default highlight colour stored in `hilighting`), `URL_NOTE_DATE = 99990102` (URL stored as a note).
- `Payment()` defaults: amount 0, description "", startDate = endDate = today, n_pays 1, frequency 1, freq_type `'m'`, flags 48 (N_PAYS), disabled false, flag_value `'^'`, grace 0, loan_amount 0.
- `FAKE_AMPERSAND = "＆"`: every `&` typed in descriptions / filters is replaced by this fullwidth ampersand before being stored (SWT would treat `&` as a mnemonic).

---

## 1. `editPayment(parentShell, pmt, cal)` — New/Edit Payment dialog

`new_payment = (pmt == null)`. Returns `true` iff `db.updateCount` changed while the dialog was open (add/update/delete).

### 1.1 Window
- Style: DIALOG_TRIM | APPLICATION_MODAL (not resizable). Icon: budget icon when new, else the payment's image (`images/<image_path>`). Title **"New Payment"** / **"Edit Payment"**.
- `Budget.lastQuickPicks = null` on open. Layout: 2 columns, margins 10, spacing 5. `dateW` = pixel width of the text "2222-22-22" + 4 — used as width hint for every numeric/date field.
- Working payment `P` = `new Payment()` when new, else the payment object itself (edited in place; fields are only written on Ok).
- New payment and `cfg.useIndefiniteEndMode` (default true): `P.setEndMode(INDEFINITE)`.
- `org_amount = P.amount` (signed) is remembered for the amount-changed prompt.

### 1.2 Fields, top to bottom (label → control)
1. **"Description:"** — Text, width 400, value `P.description`. Tooltip: `Use ';' as the separator. \nThe text before ';' will only be shown in the Calendar View.`
2. **"Category:"** — Combo (drop-down, editable), items = `db.getCategories()` (distinct non-null categories, sorted), text = `P.category` or "". Tooltip: `Clear this field to 'Uncategorize'\nUse File -> 'List of all payments' to rename or delete existing categories.`
   Autocomplete (`Budget$34`, only installed if at least one category exists): on each modify while the combo has focus and the event time differs from the last handled one: `t = combo text`; if `upper(t) == lastVal` and `t` non-empty → `t = t minus last char` (this makes Backspace shrink the completion instead of re-completing); `lastVal = upper(t)`; candidates = categories whose upper-case starts with `lastVal`; if there are candidates and `t` non-empty → set text to `candidates[0]` and select the range `[len(t), len(candidates[0]))`; else if `len(t) != len(combo text)` → set text to `t`. The listener is removed/re-added around setText to avoid recursion.
3. **"URL:"** — only if the app was started with the show-URL option (`opt_show_url`). Text width 400, value `P.getURL()` (= note at date 99990102) or "".
4. **"Amount:"** — row: Text (width `dateW`) showing `numFormatFrac2(|P.amount|)`; radio group **"Expense"** / **"Income"** (Income selected iff `P.amount > 0`, else Expense — a zero amount shows Expense); checkbox **"Disabled"** (= `P.disabled`), tooltip `Check it, if you want to disable this payment.\nThe payment transactions will still be placed in the SimpleCalendar,\nbut will not be used to calculate the balances.`
   - When editing an existing payment the amount field tooltip is `Suggested amount is: <numFormatFrac2(sixMonthsAvg(description, today).pt)>` (see §19; `pt` is NaN when no history).
   - Focus listener (`Budget$35`): on focus gained select all. On focus lost: `s = trim(text)`; if `s[0] == '+'` → select Income, deselect Expense, strip the `+`; `v = Calculator.calculate(s)`; text = `numFormatFrac2(|v|)`; if `v < 0` → select Expense. Foreground = black. On any exception: foreground = red and focus is forced back into the field.
5. **"Start Date:\n(YYYY-MM-DD)"** — Text (width `dateW`) = `sDate(cal)` when new and `cal != null`, else `sDate(P.startDate)`; plus a button with icon `icons/cal.bmp` opening `pickDate` (§6) with the current field value. On pick (`Budget$42`): if `parseInt(#pays text) == 0` **and** the start text equals the "on date" text → the "on date" text is also set to the picked date; then start text = picked date; the "Nth weekday" radio label is recomputed; the last-weekday updater runs. Any modification of the start text (`Budget$43`) also recomputes the "Nth weekday" label and runs the last-weekday updater.
6. **"End: "** (label uses `cfg.fontLabel`) — radio group with associated inputs (`setData` = end-mode constant):
   - **"Indefinite"** (16)
   - **"# of pays:"** (48) + Text (width `dateW/2`) = `P.n_pays`
   - **"on date:"** (32) + Text = `sDate(P.endDate)`, except for a new payment with `cal != null` where it is the same string as the Start Date; plus a `cal.bmp` button → `pickDate` (`Budget$37`) sets the text.
   - **"amount paid:"** (64) + Text (width `dateW`) = `P.loan_amount` (raw double to-string, e.g. `0.0`)
   - `updateEndSelection(mode, …)`: selects exactly the radio for `mode`, and enables only the input(s) belonging to the selected mode (`# pays` text; `on date` text + button; `amount paid` text). Radio clicks (`Budget$44`) call it with the clicked mode and set **Tentative** enabled = `(mode != LOAN_PAID)`.
   - Number fields (`# pays`, `amount paid`, `Pay Every`) select-all on focus (`Budget$36`).
7. **"Grace period (days):"** — Text (width `dateW`) = `P.grace_period_days`, tooltip `Use '0' to disable`.
8. **"Pay Every:"** — Text (width `dateW`) = `P.frequency`, enabled unless `P.freq_type == 'W'`; then radios in a 4-column grid: **"Day(s)"** (`'d'`), **"Week(s)"** (`'w'`), **"Month(s)"** (`'m'`), **"Year(s)"** (`'y'`), then a full-width radio whose text is `get_nth_week_msg(startText)` = `"<1st|2nd|3rd|4th|5th> <Weekday> in a month"` (`'W'`; ordinal index = `(day-1)/7`, weekday from the start date, names Sunday..Saturday), then a full-width radio `"Last <Weekday> in a month"` (`'L'`). The radio matching `P.freq_type` is pre-selected.
   - Last-weekday updater (`lambda$editPayment$0`, run at build time and on every start-date change): label = `"Last <Weekday> in a month"` for the start date; enabled iff `startDate.day > daysInMonth - 7`; if it is selected but no longer allowed → deselect it and select **Month(s)**.
   - Modify on the frequency text (`Budget$38`) and clicks on Day/Week/Month/Year (`Budget$39`): if `checkEndPayment` (= "# of pays" radio selected AND `parseInt(#pays) <= 1`; parse failure → false) → `updateEndSelection(INDEFINITE)` (editing the frequency of a single-pay payment flips it to Indefinite). `Budget$39` additionally sets the frequency text enabled = !(Nth weekday radio selected).
9. (empty label) — checkbox row: **"Business Days Only"** (= `isNoWeekends`), **"Early Pay"** (= `isEarlyPay`, enabled = `isNoWeekends`, toggled by Business Days Only — `Budget$40`; tooltip `Check this option if you want to pay on Fridays\ninstead of Mondays.`), **"Tentative"** (= `isTentative`, enabled = `endMode != LOAN_PAID`; tooltip `Tentative transactions will automatically slide to the current date (if the current\ndate is in range) until adjusted or marked as paid. If transaction reaches the end \nof the pay period with no adjustment it will be removed from the calendar.\nCheck documentation for more details.`), **"Sticky"** (= `isSticky`; tooltip `Override transaction's default amount with the last adjustment\nin the calendar before this transaction's date, if available.\n`).
   - Tentative toggle (`Budget$41`) and initial state: **Business Days Only**, the **"amount paid:"** radio and the **Grace period** text are enabled iff Tentative is unchecked.
10. **"Mark as:"** — radios **"None"** (`'^'`), **"Paid"** (`';'`), **"Important"** (`'#'`) pre-selected from `P.flag_value`; then button **"Highlighting"** and a 5-space swatch Label whose background = `P.getDefaultHilighting()` colour if `!= -1`. The button pops up the highlighting menu (§20) bound to `Info{cal = 99990101, pmt = P, bg_color = default colour, bg_label = swatch}`; choosing a colour only updates `info.bg_color` and the swatch (parent background when -1); it is written to the payment on Ok.
11. **"Image:"** — RowLayout: image Button (initially `icons/no-img.bmp`, tooltip `Click here to select an image`; if `P.image_path` non-empty → shows `images/<path>`, `data = path`, tooltip = path). Click → `selectImage` (§5): `null` (cancelled) → nothing; `""` → no-img icon; else the image; in both last cases `data = result`, tooltip = result. Then label `" Quick\n Picks:"` followed by up to 10 quick-pick image buttons (`updateQuickPicks`, refreshed on every Description or Category change with `desc + " " + cat`): upper-case the text; for each `ImgTag{substring, images[]}` in `Budget.imgTags` whose substring occurs in the text, add its images to an ordered set until 10; if the set equals `lastQuickPicks` nothing changes; else dispose all children from index 2 on and create one push button per image (image, `data`=path, tooltip=path); clicking one (`Budget$33`) copies its image and `data` onto the main image button.
12. Horizontal separator spanning both columns.
13. Buttons: when editing, a **"Delete"** button on the left (`Budget$49`): `confirmYesNo(editShell, "Are you sure you want to delete the '<description>' ?\n")` → `db.deletePayment(P)` and close. Then right-aligned **"Ok"** (default button) and **"Cancel"** (close). Button size = size of "--Delete--".

### 1.3 Ok (`Budget$50`) — validation and commit, in order
Any exception during steps 1–11/17 shows `showError(editShell, <current message>)` and keeps the dialog open. The "current message" variable is set *before* each parse; note the quirks below.

1. `desc = trim(description)`; empty → **"Description is empty!"**.
2. URL field (if present): `url = trim(url)`; empty → `null`; else must start with `http://` or `https://` → **"URL string must start with 'http://' or 'https://'"**.
3. `s = trim(amount)`; `expense = Expense radio selected`; `s[0]=='-'` → `expense = true`; `s[0]=='+'` → `expense = false`. (Empty amount → `charAt(0)` throws; the message shown is the stale **"Description is empty!"** — quirk (verified against regenerated pseudo code).) `amt = |toDouble(s)|` (§18; never throws, yields 0 on garbage, so "Unable to parse the amount" is effectively unreachable); if `expense` → `amt = -amt`.
4. `start = toDate(startText)` → **"Unable to parse the start date"**.
5. `loan = toDouble(loanText)` (**"Unable to parse the loan amount"** — unreachable).
6. If **amount paid** radio selected: `unit = max(|amt|, 0.001)`; `unit < 0.01` → **"Payment amount is too small to calculate a schedule."**; `|loan| / unit > 1000` → **"'Amount Paid' schedule: number of transactions exceeds 1000."**
7. `n_pays = Integer.parseInt(#paysText)` → **"Unable to parse the # pays"**.
8. `freq = Integer.parseInt(freqText)` → **"Unable to parse frequency"**; `freq < 1` → `1`.
9. `ftype`: default `'d'`; Week(s) `'w'`; Month(s) `'m'`; Year(s) `'y'`; Nth weekday → `freq = 1`, `'W'`; Last weekday `'L'`.
10. `flag`: None `'^'`, Paid `';'`, Important `'#'`.
11. `end = toDate(onDateText)` → **"Unable to parse the end date"**; `end < start` → `end = start`. (Parsed and stored regardless of end mode.)
12. `cat = trim(category)`. `cat == desc` → `showError(mainShell, "Payment's description can't be the same as category.")`, return (dialog stays).
13. `cat` non-empty and `!db.isValidCategory(cat, P)` → `"You already have a payment with the same description as the entered category: '<cat>'.\nPlease change the category of this payment."`. `isValidCategory(c, self)`: `null` → ok; `"(Unaccounted)"` → invalid; invalid if any *other* payment's description equals `c`.
14. `!db.isValidDescription(desc, P)` → `"You already have a category with the same name as the entered description: '<desc>'.\nPlease change the description of this payment."` (invalid if any other payment's category equals `desc`).
15. `split = false`; `schedChanged = false`. When editing: build a temp Payment with the new `start, end, n_pays, ftype, freq, disabled, endMode` and `schedChanged = !P.isSameSchedule(temp)` (`isSameSchedule` compares startDate, endDate, freq_type, frequency, disabled, endMode, n_pays).
16. If `cal != null` (dialog opened from a calendar cell) **and** editing **and** (`org_amount != amt` **or** `schedChanged`):
    - `schedChanged` → `confirmYesNo(editShell, "You have changed a payment with existing transactions.\n\nALL THE PAST AND FUTURE TRANSACTIONS (except manually adjusted ones) WILL BE CHANGED TO THE NEW AMOUNT AND SCHEDULING!\n\nUse 'Alter transaction from ...' menu item if you want to alter the future transactions only.\n\nDo you want to apply the changes anyway?")`; No → return (dialog stays).
    - else (only the amount changed): if `cal == P.startDate` and `P.opt` is empty → no prompt; otherwise `showSplitOrChange(editShell, cal)` (§2): CANCEL → return; CONFIRM → `split = true`; CHANGE → continue.
17. Commit to `P`: `description = desc.replace("&", "＆")`; `category = null if cat empty else cat`; `amount = amt` **unless** `split`; `disabled`; `startDate = start`; `n_pays`; `frequency = freq`; `freq_type = ftype`; `endDate = end`; `loan_amount = loan`; `image_path = imageButton.data` (may be null); NoWeekends / EarlyPay / Tentative / Sticky from the checkboxes (Early Pay is stored even if the checkbox was disabled); `setDefaultHighlighting(info.bg_color)` (-1 removes it); `setURL(url)` if the field exists (null removes the URL note); `grace = Integer.parseInt(graceText)` → **"Unable to parse the grace period days"**, clamped to `[0, 60]`; `endMode` from the selected radio (Indefinite→16, # of pays→48, on date→32, otherwise 64); `flag_value = flag`. `edit_ok = true`.
18. If `split`: `db.updateMoveUndo(P)`; `db.update(P)`; `edit_payments_new_pmt = null`; `splitPayment(Info{amount = amt, cal, pmt = P, interactive = false})` (§17); `edit_ok = false` (so the outer code does not update again).
19. Close the dialog.

### 1.4 After the dialog closes
If `edit_ok`: new → `db.add(P)`, `Budget.edit_payments_new_pmt = P`; editing → `db.updateMoveUndo(P)`, `db.update(P)`, `edit_payments_new_pmt = null`. Return `updateCount changed`. (Delete goes through `db.deletePayment`, so it also reports `true` provided deletePayment bumps `updateCount` — see UNKNOWN.)

---

## 2. `showSplitOrChange(parent, cal)` → 0/1/2
Title **"Payment's Amount Changed"**, budget icon, single column, margins/spacing 10.
- Label: `You have changed the payment's amount with past transactions.\n\nWhat do you want to do?\n`
- Radio (pre-selected): `Change the FUTURE transactions* only starting on <dd-MMM-yyyy of cal>`
- Radio: `Change ALL the transactions* -- past and future`
- Label: `* -- manually adjusted transactions are not affected.`
- Buttons **"Accept"** (default) / **"Cancel"**. Accept → `SPLIT_CHANGE (2)` if the ALL radio is selected, else `SPLIT_CONFIRM (1)`. Cancel/close → `SPLIT_CANCEL (0)`.

---

## 3. `licAgree()` → bool
Reads the file `license.txt` (relative to the working directory). The read loop is a normal `while ((line = readLine()) != null) text += line + "\n"` — every line is kept (the earlier "drops every other line" reading was an artifact of the old pseudo-code generator; corrected against the regenerated pseudo code). Failure to read → `showError(main, "Unable to load license")` → false; empty text → `"Unable to display license"` → false. Dialog **"Budget Calendar License Agreement"** (budget icon), read-only multi-line text (border, wrap, v-scroll), buttons **"I agree"** (default) and **"Cancel"**, size 640×480 centred. Returns true only via "I agree".

## 4a. `showLog(parent, text)`
Title **"Log"**, icon `icons/note.png`, read-only multi-line text with scrollbars, buttons **"Ok"** (default button but **disabled**) and **"Cancel"**; 640×480. Used for the split-payment bug report (§17).

## 4b. `editNotes(parent)` → bool (budget-wide notes)
Title **"Budget Calendar Notes"**, icon `note.png`; multi-line text = `db.notes` (or ""), tooltip `Use ^Enter or ^S for quick save.`; **Ctrl+S** (keyCode `'s'`) or **Ctrl+Enter** saves (`Budget$58`); **"Ok"** (default) saves (`Budget$59`): `db.notes = remove_slash_r(trim(text))` (strips all `\r`), returns true; **"Cancel"** → false. 640×480. The File-menu caller then refreshes the main-window notes field and its tooltip.

## 4c. `editPmtNote(parent, cal, pmt, title)` → bool (per-transaction note)
Icon: `note.png` if `pmt == db.adjustments` (the balance-adjustment pseudo payment) else the payment image. Title = `title` if given, else `"<dd-MMM-yyyy>: <description>"`. Label `Note (keep it reasonable):`, multi-line text (border+wrap) = `pmt.getNote(cal)` or "", same tooltip and Ctrl+S / Ctrl+Enter shortcuts. Ok/shortcut: `t = remove_slash_r(trim(text))`; `pmt.setNote(cal, t.isEmpty() ? null : t)` (null removes the note); returns true. Cancel → false. Size `max(400, 4*btnW) × max(7*btnH, 280)`.

---

## 5. `selectImage(parent)` → String | null, and `fill_images`
Title **"Select an image..."**, budget icon, background `cfg.bgColorWindow`. Lists `new File("images/").listFiles()` sorted by path. Top row: buttons **"View Zoom-"** (tooltip `Decrease the view image size`) and **"View Zoom+"** (`Increase the view image size`); they change `cfg.image_selection_zoom` (default 175) by ∓25, accepted only within `[monitorZoom, 2*monitorZoom]` (SWT primary-monitor zoom, e.g. 100/125/150…); on change the grid is cleared and rebuilt and the shell re-packed.
`fill_images`: first cell = `icons/no-img.bmp` label with `data = ""`; GridLayout margins 10, spacing 9, `numColumns = (int)sqrt(fileCount) + 1`; `scale = zoom / monitorZoom` when `monitorZoom != 0 && monitorZoom != zoom`, else 1.0; each non-directory file → Label (SHADOW_NONE) with `getScaledImage("images/<rel>", bg, scale)`, `data = <path relative to images/>` (`file.getPath().substring(len("images/"))`). Mouse-down on any label (`Budget$64`) → `return_val = data`, close. Cancel/close → `null`.

---

## 6. `pickDate(parent, initial[, title = "Pick a Date"])` → SimpleCalendar | null, `fillPickDate`
Shell: 7 equal-width columns, margins 10, spacing 2; `data("initial") = copy of initial`. `fillPickDate(shell, cal)` (re-run on navigation; `cal` day forced to 1 and stored as shell data):
- Row 1 (spans 7): **"<<"** (tooltip `Previous Year`, −12 months), **"<"** (`Previous Month`), a flat centred button showing `MMMMMMMMMMMMM,yyyy` (e.g. `March,2026`; foreground `cfg.colorCurrentMonth`, background `cfg.bgColorWindow`) whose click pops up a menu of the 12 month names in that format (`Budget$67`, selecting sets the month and refills), **">"** (`Next Month`), **">>"** (`Next Year`), **"Today"** (`Set Today Year and Month` → `initCal`, the app's "today").
- Row 2: 7 centred 3-letter weekday headers starting at the configured first day (`cfg.firstDayOfWeekIsSunday` → Sunday(0) else Monday(1)); colours `cfg.colorDatePickerHeader` / `cfg.bgColorDatePickerHeader`, font `cfg.fontDate`.
- Leading filler labels (`cfg.bgColorCellOtherMonths`) for `firstDayOfWeek .. dayOfWeek(1st)` where `dayOfWeek` is 0=Sun..6=Sat, remapped Sunday→7 when the week starts on Monday.
- One right-aligned Label per day 1..max: background `cfg.bgColorCell` if the day is **before** today (`initCal`), else `cfg.bgColorCellPast` (as written — looks inverted but this is what the code does; `SimpleCalendar.lt` is a plain `yyyymmdd <` comparison and the reference is `new SimpleCalendar(initCal)` (verified against regenerated pseudo code)); foreground `cfg.colorCalendarDay`; the day equal to the dialog's `initial` date gets foreground `cfg.colorCalendarToday`. Mouse-down (`Budget$71`): `pickDateCal = shellCal with day = label text`, close.
- Trailing filler labels up to the last day of week (`first + 6`).
- Shell size = computed size with height × 1.2. Returns `pickDateCal` (null when closed without a pick).

---

## 7. `prompt(parent, image, title, message, default, flags)` → String | null
Flags: `PROMPT_PASSWORD = 1`, `PROMPT_EXPENSE = 2`, `PROMPT_REMOVE_ADJUSTMENT = 4`. Resets `prompt_was_ctrl_ok / prompt_was_alt_ok`.
- Shell titled `title`, budget icon (or `image`), background = system widget background, margins 10, v-spacing 10.
- A `Link` showing `message` (SWT link markup `<a>` / `<a href="…">`), link colour `cfg.colorCurrentMonth`. Clicking a link (`Budget$74`) puts `event.text` (the `href` if present, else the link body) into the text field, focuses it and selects all; with PROMPT_EXPENSE it also sets the expense checkbox to `text.startsWith("-")`.
- With PROMPT_EXPENSE: checkbox **"expense"**, initially `default.startsWith("-")`; if so the leading `-` is stripped from the default. The checkbox just records `expense`.
- Text field = `default`; PROMPT_PASSWORD → echo char `*`. Verify listener (`Budget$73`): when the inserted text is longer than 5 chars and matches `(\D*)([\d,]+\.\d+)(\D*)`, the insert is replaced by the captured number (group 2) — i.e. pasting "Total: $1,234.56 CAD" yields "1,234.56". With PROMPT_EXPENSE only the first match is used; otherwise the loop keeps replacing with each successive match (last match wins).
- Enter in the field (`Budget$75`) or **"Ok"** (`Budget$77`): record Ctrl (`stateMask & SWT.CTRL`) → `prompt_was_ctrl_ok`, Alt (`SWT.ALT`) → `prompt_was_alt_ok`; `return_val = text`; close. **"Cancel"**/close → `return_val = null`.
- With PROMPT_REMOVE_ADJUSTMENT an extra left button **"Remove adjustment"** (tooltip `Click here to remove adjustment and dismiss this dialog.\nBeware: no confirmation asked!\n`) sets `remove_adjustment = true` and closes.
- Field is focused with all text selected.
- Return value: if `return_val != null` and PROMPT_EXPENSE: if it starts with `+` and expense is checked → `"0:" + rest` (explicit `+` overrides the checkbox); else `(expense ? "1:" : "0:") + return_val`. Else if `return_val == null` and remove_adjustment and PROMPT_REMOVE_ADJUSTMENT → `"2:"`. Else `return_val` (null = cancelled).

## 8. `setMenu(control, menu)`
If `cfg.oneButtonMouse`: mouse-up on the control pops the menu (so one-button Mac users get context menus on plain click).

## 9. `duplicatePayment(pmt)` → Payment | null
`clone = pmt.clonePayment()` (copies amount, description, endDate, freq_type, frequency, image_path, n_pays, startDate, flags, disabled, category, grace, default highlight; **not** per-date options/flags/notes/hilights and **not** `loan_amount`); `clone.description += "(dup)"`; `db.add(clone)`; `editPayment(mainShell, clone, null)`; return clone. The clone stays in the DB even if the edit dialog is cancelled (the user must delete it).

---

## 10. `showPayments(parent)` — "List of all payments" window
Shell **"Payments"** (DIALOG_TRIM|RESIZE|APPLICATION_MODAL), icon `icons/det.bmp`, background `cfg.bgColorWindow`. Returns `updateCount changed` (caller refreshes the calendar).

Layout (2 columns):
- Group **"Filter"**: Text (tooltip = `filterToolTipText`, initial = `Budget.allPaymentsSavedFilter`, a static that survives reopen) and button **"Clear"**.
  - Enter in the text (`Budget$84`): `table.filter = text.replace("&","＆")`, `allPaymentsSavedFilter = same`, refill.
  - Clear (`Budget$85`): text = "", filter = "", refill (does not touch the saved filter).
- Group **"Colors"** (legend + quick filters, `Budget$82` sets the text and the table filter and refills, without saving): `<a>Active</a> transactions` (data `range:active`), `<a>Past</a> transactions` (foreground `cfg.colorPastPayment`, `range:past`), `<a>Future</a> transactions` (`cfg.colorFuturePayment`, `range:future`), `<a>Disabled</a> payment` (`cfg.colorDisabledPayment`, `range:disabled`).
- Table (FULL_SELECTION|BORDER, header+lines) spanning both columns, `data("filter") = allPaymentsSavedFilter`. Columns (title, width, align, comparator):

  | # | Title | W | Align | Sort |
  |---|---|---|---|---|
  |0|`Img.`|30|L|—|
  |1|`Amount`|80|R|`amount_comp`|
  |2|`Category`|120|L|`cat_comp`|
  |3|`Description`|300|L|`desc_comp` (initial sort column, ascending, `icons/up.gif`)|
  |4|`Start Date`|100|C|`start_date_comp`|
  |5|`End Date`|100|C|`end_date_comp`|
  |6|`# of pays`|80|R|`n_pays_comp`|
  |7|`freq.`|50|R|`freq_comp`|
  |8|`Bus.`|40|L|—|
  |9|`flags`|50|L|—|

  Header click (`Budget$86`): same column → `sort_direction = -sort_direction`; other column → it becomes `sortColumn`, direction 1. All header images cleared, the sort column gets `up.gif` (1) or `down.gif` (−1); refill. (`sortColumn`/`sort_direction` are statics; reset to Description/1 every time the window opens.)
- Button **"Close"** (right aligned). Window size: height exactly 600, width = computed + 30.

Comparators (all multiply by `sort_direction`): `n_pays_comp` = `Integer.compare(n_pays)`; `start_date_comp` / `end_date_comp` = compare the int dates; `amount_comp` = `Double.compare(amount)` (signed); `freq_comp` = string compare of `frequency + freq_type` (e.g. "1m" vs "2w" — lexicographic); `cat_comp` = compare `category or ""`, ties → `desc_comp`; `desc_comp` = `description.compareTo`, ties → `amount_comp`.

`fillPaymentsTable(table, selectPmt)`: `db.sort()`; `Filter f = new Filter(table.filter)`; if `selectPmt == null` use the current selection; rows = payments with `id != 0` for which `f.isMatch(pmt, pmt.amount, 0)`; sort with the sort column's comparator; per row: col0 image (if any), `Amount` = `numFormatFrac2(amount)` (signed), `Category` or "", `Description` (raw, with `;`), `Start Date` = `dd-MMM-yyyy` (row foreground `cfg.colorFuturePayment` if `startDate > today`), `End Date`/`# of pays` = `"---"`/`"---"` for INDEFINITE else `sdate(endDate)` (foreground `cfg.colorPastPayment` if `endDate < today`) and `n_pays`, `freq.` = `"<frequency><type char>"` (e.g. `1m`, `2w`, `1W`), `Bus.` = `"yes"` if NoWeekends, `flags` = `"[E]"` if EarlyPay + `"[T]"` if Tentative + `"[L]"` if LOAN_PAID mode; disabled → foreground `cfg.colorDisabledPayment` (applied last, overrides). The `selectPmt` row is selected and shown.

Double-click a row (`Budget$87`) → `editPayment(shell, pmt, null)`; refill if changed.

Context menu (right click; also plain click with one-button mouse):
- **&Edit** — edit the selected payment, refill selecting it.
- **&Add a new payment** — `editPayment(shell, null, today)`; refill selecting `edit_payments_new_pmt`.
- (sep) **&Duplicate payment** — `duplicatePayment` (§9), refill selecting the clone.
- (sep) **Delete payment** — `confirmYesNo(shell, "Are you sure you want to delete the '<description>' ?\n")` → `db.deletePayment`, refill.
- (sep) **Add a 'wild' transaction** — `pickDate(shell, curCal, "Pick New Transaction Date")`; on pick `pmt.setCustomAmount(date, pmt.amount)`; `db.update(pmt)`; refill.
- (sep) **All transactions** ▸ (cascade; menu data `payment_table`, `shell`; listener `all_transactions_menu_listener`, §19) and **Matched transactions** ▸ (same plus data `matching-only = "yes"`).
- (sep) **Edit Category** (`Budget$93`, only when the selected payment has a category): loop `prompt(shell, null, "Edit Category Name", "Enter a new category name:", current, 0)`; null/blank → return; `!db.isValidCategory(new, pmt)` → `showError("The new name is not valid, please choose a different one.")` and re-prompt; if some payment already has category == new → `confirmYesNo("Edited category name matches an existing category name.\nDo you want to merge all payments of this category with the matched one?")`, No → return; then every payment whose category equals the old name gets the new name; refill. (No `db.update` is called — the change lives in the payment objects; whether `updateCount` is bumped is UNKNOWN.)
- **Delete Category** (`Budget$94`): `confirmYesNo("All payments that belong to this category will be set to an empty category name.\n\nDo you want to continue?")` → set `category = null` on every payment with that category; refill.
- (sep) **Filter description** — filter := `"desc:" + description` (text + table), refill. **Filter category** — `"cat:" + category` (only if the payment has one).

`filterToolTipText` (exact):
```
Filter expression format:
[-][|] <[~]token>[ <[~]token> [...]]
  '-' filters and displays DELETED transactions only -- can be used without pararameters
  '|' sets the 'OR' search mode (the default is the 'AND' mode );
  '~' inverts its token condition
token:
-- >n, <n or =n -- n is the amount
-- 'cat:<substring>' to search categories only
-- 'note:<substring>' to search notes only
-- 'desc:<substring>' to search description only
-- 'date:<date>' to search date only
-- 'range:<n|active|past|future|disabled|i[ndefinite]>' to filter-out by date range
-- '[flag]:<paid or .|unpaid or ?|important or !> -- filter paid, unpaid, important.
Misc:
-- Use '\ ' when searching for a space.
```

---

## 11. `createNew()`, `showHint()` (new budget wizard)
`createNew`: `saveDB()` false → abort. `fileName = null; dbKey = null; curCal = today with day 1`; `db = BudgetDB.load(null, null, err, curCal, cfg.futureNMonths, cfg.unlimitedHistory)`; null → `showError("Unable to initialize the new budget database.")` and `System.exit(1)`. Then `calc_min_date()`, `updateMonthLabelMenu()`, `updateShellTitle()`, `updateCalendarView()` (if the view exists), `showHint()`.

`showHint` — shell **"New Budget..."** (TITLE|APPLICATION_MODAL, parent = active shell), 2 columns: budget icon at left; at right:
- Link (width 450): `You created a new budget!\nHave a glance at the <a>docs</a>, and the <a>tutorial</a>.` — `docs` → opens `<jar_loc>\doc\budget.html` (as `file://` URL, backslashes → `/`, spaces → `%20` on Mac); `tutorial` → `http://www.mishell.ca/online/tutorial.html`.
- Label: `\nEnter the starting balance on <dd-MMM-yyyy of the 1st of the current month>:`
- Text = `"0"` (width 70, indent 15), focused with selection 0..10.
- Grey label (`cfg.colorDisabledPayment`, width 435): `leave this unchanged if not known, and change it later by double clicking on the balance field of a day.`
- Label: `\nAdding payments -- right click on a day cell and choose 'Add a new payment' menu item\n`
- Checkbox (= `cfg.oneButtonMouse`): `I use a one button mouse (or see File->preferences later)`
- **"Ok"** (centred, spans 2): `v = Calculator.calculate(text)`; error → `showError(dialog, "Invalid amount!\n" + message)`, refocus/select the field, stay. If `v != 0` or the checkbox differs from cfg: `cfg.oneButtonMouse = checkbox`; if `v != 0`: `cal = 1st of month − 1 day`; `db.updateAdjustment(cal, v)` (balance adjustment on the last day of the previous month); `updateCalendarView()`. Close. (Closing via X skips everything.)

## 12. `save(saveAs)`, `saveDB()`, `saveNewBudget()`, `getHomePath()`
- `getHomePath()`: in "stick mode" → `dirData`; else `user.home`, and on Windows prefers `<home>\Documents` if it exists, else `<home>\My Documents` if it exists.
- `save(saveAs)`: if `fileName == null || saveAs`: SWT FileDialog SAVE titled **"Save as..."**, filter `*.bgt`, filter path = `getHomePath()`; cancel/empty → false; if the name does not contain `.bgt` append `.bgt`; if the file exists → `confirmYesNo(main, "The '<path>' file already exists.\nDo you want to overwrite it ?")`, No → false; `fileName = path`; `updateShellTitle()`. Then `db.save(fileName, dbKey, err)`; false → `showError(err)` → false. Then `addRecentFile(absolutePath)`, `saveRc()` (writes `.budgetrc`), true.
- `saveDB()` (called before New/Open/Exit/update): `fileName == null` → if the DB has no payments and no adjustments → true (nothing to save); else `saveNewBudget()`. Otherwise `db.save(fileName, dbKey, err)` (error → showError, false) → true.
- `saveNewBudget()`: `confirmYesNoCancel(main, "Do you want to save the new budget ?")` → NO → true (discard), CANCEL → false, YES → FileDialog SAVE **"Save new budget..."** (`*.bgt`, no filter path); cancel → false; append `.bgt` if missing; **quirk:** if the file exists, `confirmYesNo("The '<path>' file already exists.\nDo you want to overwrite it ?")` is inverted — answering *Yes* returns false (not saved), answering *No* proceeds to overwrite (verified against regenerated pseudo code): `confirmYesNo` returns true only for `SWT.YES`, and `saveNewBudget` tests `== false → proceed`, whereas Save As (`save`) tests `!= false → proceed`). Then `db.save` (error → false), `fileName = path`, true. Does not add to recent files or save rc.
- Autosave / backup: not implemented in these methods; `db.save` (BudgetDB) may create backups — UNKNOWN here.

---

## 13. Menus (`createFileMenu`, `createEditMenu`, `createHelpMenu`)
There are **no keyboard accelerators** (`setAccelerator` is never called in the Budget class); `&` only marks mnemonics. `addMenuItem(menu, id, text, listener)` additionally hooks the macOS system menu item with the given SWT id (`-2` = Preferences, `-1` = About), replacing its existing listeners, and still adds a normal item.

**&File**

| Item | Action |
|---|---|
| `&New` | `createNew()` |
| `&Open...` | `saveDB()` (abort if false) → FileDialog OPEN, `*.bgt`, path `getHomePath()` → `openBudgetFile(path)` |
| `&Save` | `save(false)` |
| `save &As` | `save(true)` |
| `pass&Word` (enabled iff `dbKey != null` or `cfg.encryption`) | If a password exists: `prompt("Current Password", "This file already has a password\nEnter the current password", "", PASSWORD)`; cancel → return; mismatch → `showError("The password is not correct!")`. Then `prompt("New Password", "Enter the new password:\n(leave this field empty to disable password)\n", "", PASSWORD)`; cancel → return; empty → `dbKey = null`, `showInfo("Password protection removed!")`; else `prompt("New Password Confirmation", "Re-type new password:", "", PASSWORD)`; cancel → return; different → `showError("The entered passwords are different!")`; else `dbKey = new password` (applied at next save). |
| `&List of all payments` | `showPayments(main)` → `updateCalendarView()` if it returned true |
| (sep) `edit notes` | `editNotes(main)` → refresh main-window notes text/tooltip |
| (sep) `preferences` (Mac: system Preferences item) | `cfg.unlimitedHistory.hidden = cfg.unlimitedHistory.boolValue`; `ConfigEditor(cfg, "Preferences").open(main)`; if accepted: fade out main shell, hide, `rebuildApp()`, show, focus, fade in |
| (sep) `&Print this month` | `getHtmlFileName()` (null → return) → `saveHTML(file, curCal, curCal)` → launch in browser; error → `showError("Unable to save HTML file\n" + ex)` |
| `&Excel report` (disabled when no Excel program was found) | `generateReport(curCal, curCal, file, GEN_EXCEL)` → `excel_prg.execute(file)`; error → `"Unable to save the output file\n" + ex` |
| `&Detailed report` | `generateReport(curCal, curCal, file, GEN_HTML)` → launch; same error text |
| `&Unpaid && paid report` (shows as "Unpaid & paid report") | `generatePaidUnpaidReport(firstOfMonth, lastOfMonth, file)` → launch; same error text |
| (sep) (sep) | recent files are inserted between these two separators on every menu show (`Budget$101`): items `&<n>: <path>` (with `&` doubled) for each of `cfg.recentFile` (max 5), the currently open file disabled; click → if not the current file: `saveDB()` (abort if false) → `openBudgetFile(path)`; on failure `showYesNo("Do you want to remove this file from recently open file list?")` → remove from `cfg.recentFile` |
| `E&xit` (not on Mac) | `saveAndCleanup() == 1` → mark the shell adapter invoked, remove the shell listener, `mainShell.close()` |

**&Edit**
- `&Undo transaction move` — enabled on menu show iff `db.hasMoveUndo()`; action `db.doMoveUndo()`; `updateCalendarView()`.

**&Help**
- `&Help ` → opens `<jar_loc>\doc\budget.html`.
- `&Tutorial` → `http://www.mishell.ca/online/tutorial.html`.
- `&Check for Updates Online` → sets `https.protocols=TLSv1.2`, reads the first line of `https://www.mishell.ca/files/budget.version`; `getVersion(s)` = `a*1000000 + b*10000 + c` from `a.b.c` (each component = leading digits only, missing → 0; only the first 3 parts count). If remote > `budgetVersion` ("1.11.<revision>"): `confirmYesNo("A newer version of Budget Calendar (<v>) is available.\nDo you want to download it?")`; No → return; Windows: (`msiSuggestUninstall` when not jpackage) launch `https://www.mishell.ca/files/BudgetCalendar.msi`; Linux: `…/BudgetCalendar.sh`; Mac: `…/BudgetCalendar<arm64|>.dmg` (arm64 when `os.arch == aarch64`); then `saveDB()` true → `System.exit(1)`, else `showInfo("Make sure that you exit Budget Calendar before installing the update!")`. Up to date → `showInfo("Your Budget Calendar is up to date.")`. Any exception → `showError("Unable to reach www.mishell.ca\n" + ex)`.
- `&About` (Mac: system About) → MessageBox ICON_INFORMATION|OK titled **"About"**: `Budget Calendar Version <budgetVersion> (<java.version>)\nMiShell Software Systems, Ontario, Canada.\nhttps://www.mishell.ca/`
- If `trial_version`: (sep) `&Register` → `askRegistration()`; on success the menu item disposes itself.

`askRegistration()`: `prompt(main, null, "Registration", "Enter registration key: ", "", 0)`; null → false; `key = trim`; `!Budget.isValidKey(key)` → `showError("The entered key is INVALID!")`, false; else `trial_version = false`, `updateShellTitle()`, `showInfo("Registration complete.\nThank you for using Budget Calendar")`, `cfg.regkey = key`, `cfg.unlimitedHistory = true` and enabled, `saveRc()`, true. (`isValidKey` is in part05 — out of scope.)

---

## 14. `updateCalendarView()`
If `do_calendar_update` is set (an update is already scheduled) → return. Clear `do_best_fit`, `flash_cell`, `labelInFocus`; main notes text + tooltip = `db.notes`; with redraw off: dispose all calendar children, drag sources, temp widgets, drop targets; `createCalendarView(calendarScroll, calendarView, curCal)`; layout; if the shown month (`YYYYMM`) differs from the last built one (or none) → `show_current(calendarScroll)` (scroll to the current cell); remember `lastCreateViewCal = curCal`; redraw on; redraw the graph canvas; `setMonthLabel()`; redraw.

---

## 15. `adjustBalance(cal)` — balance adjustment prompt
`day = db.getDay(cal)`; `text = numFormatFrac2(day.closing_balance)`; `before = day.getUnadjustedBalance()` (= `closing_balance − adjustment_diff` if adjusted, else closing). Loop:
- `msg = "Enter new balance adjustment (formulas are allowed):"` + (adjusted ? `"\nBalance before adjustment: <a href=\"<fmt(before)>\"><fmt(before)></a>."` : "").
- `r = prompt(main, null, "Balance Adjustment", msg, text, adjusted ? PROMPT_REMOVE_ADJUSTMENT : 0)`; null → return.
- adjusted and `r == "2:"` → `db.removeAdjustment(cal)`, `updateCalendarView()`, return.
- `v = Calculator.calculate(r)`; exception → `showError(main, ex.message)` and loop again (the prompt default is now the text the user typed).
- `db.updateAdjustment(cal, v)` (sets the closing balance of that day to `v`), `updateCalendarView()`.

## 16. Max-savings algorithm
`get_best_saving(cal, nMonths, amount, minBalance, mode)`:
```
min = 1e9
d = cal; end = (cal + nMonths months) with day = 1
while d <= end:
    day = db.getDay(d); if day == null: break
    if day.closing_balance_adjusted: amount = 0      // from an adjusted day on, the hypothetical amount no longer applies
    min = min(min, day.closing_balance - amount)
    d = next day
if mode == "$"  and min >  minBalance: return minBalance - min   // negative: surplus that can be moved to savings
if mode == "$$" and min <  minBalance: return minBalance - min   // positive: shortfall to borrow
return 0
```
(`amount` is the transaction amount being adjusted; subtracting it from each future closing balance simulates removing it. `cfg.maxSavingNMonths` default 2, `cfg.minBalance4MaxSaving` default 100.00.)
- `getBestSavings(cal)` → one value per day of `cal`'s month: `-get_best_saving(day, N, 0, minBal, "$")` (positive amounts).
- `getMonthPlusMinus(cal)` → per day of the month, running total `cum += (Σ planned amounts of non-disabled regular payments due that day) − (Σ actual transaction amounts of non-disabled payments that day)`; `out[i] = -cum`; stops at the first missing day.
- `truncate_to_10(x)`: `u = minBal4MaxSaving < 100 ? 1 : 10`; result = `sign(x) * trunc(|x| / (10u)) * 10u` as `long` → truncates toward zero to a multiple of 10 (or 100 when the configured minimum balance is ≥ 100).
- `roundUp(x) = ceil(x/10)*10`.
- `getSavingsAmounts(x)`: `"<a href=\"F\">F</a>"` with `F = fmt(x)`; if `x < -1000`: `x2 = (int)x/1000*1000` (truncated toward zero to thousands) → append `" (<a href=\"F2\">F2</a>"`; if `x2 <= -1000` → `x3 = x2/2`, append `", <a href=\"F3\">F3</a>)"` else `")"`. Example: x = −2,650 → `-2,650.00 (-2,000.00, -1,000.00)`.

## 17. `adjustAmount(cal, pmt, curAmount)` — "Amount Adjustment" prompt → bool
```
default = fmt(curAmount)
s  = truncate_to_10(get_best_saving(cal, N, curAmount, minBal, "$"))
if s != 0: savingsHint = "You may put " + getSavingsAmounts(s) + " to 'Savings'.\n"   // s is negative, e.g. "-500.00"
else:
    b = roundUp(get_best_saving(cal, N, curAmount, minBal, "$$"))
    if b != 0: savingsHint = "You may want to borrow <a href=\"fmt(b)\">fmt(b)</a> from 'Savings'.\n"
six = getSixMonthsAvg(pmt.description, cal)
sixHint = six.np != 0 ? "Six months average is <a>" + fmt(six.pt) + "</a>.\n" : "\n"
loop:
  r = prompt(main, null, "Amount Adjustment",
      "Enter the new amount, formulas are allowed.\nStandard amount is <a>" + fmt(pmt.amount) + "</a>. "
      + sixHint + savingsHint
      + "Use: %n to calculate week's income percentage, or %%n month's income percentage.",
      default, PROMPT_EXPENSE | (curAmount != pmt.amount ? PROMPT_REMOVE_ADJUSTMENT : 0))
  if r == null: return false
  if curAmount != pmt.amount and r == "2:": remove_adjustment(pmt, cal); return true
  expense = r.startsWith("1:"); e = r.substring(2).trim(); if e empty: return false
  if e == "$" or e == "$$": default = fmt(get_best_saving(cal, N, curAmount, minBal, e)); goto loop   // show the raw savings/borrow figure
  week = month = 0; if e contains "%": week = getWeekIncome(cal); month = getMonthIncome(cal)
  v = Calculator.calculate(e, week, month)   // exception -> showError(main, msg); default = e; goto loop
  if v > 0 and expense: v = -v
  if e contains "%": default = fmt(v); goto loop      // percentages are shown for confirmation first
  if pmt.amount > 0 and v < 0 and !confirmYesNo("This type of transactions is Income.\nThe adjusted amount is negative (" + fmt(v) + "). Is this correct?"): goto loop
  if pmt.amount < 0 and v > 0 and !confirmYesNo("This type of transactions is Expense.\nThe adjusted amount is positive (" + fmt(v) + "). Is this correct?"): goto loop
  pmt.setCustomAmount(cal, v); db.update(pmt)
  if Ctrl was held on Ok: other = pickWildPaymentDialog(main, null, cal, v)   // transfer mode
        if other: other.setCustomAmount(cal, other.getAmountBeforeDate(cal) - v); db.update(other)
  if Alt was held on Ok: editPmtNote(main, cal, pmt, null)
  scheduleUpdateCalendarView(); return true
```
Notes: the expense checkbox in the prompt is pre-set from the sign of `default`; because the Ok text comes back with a `1:`/`0:` prefix, a positive expression typed with the "expense" box checked becomes negative, while a negative expression keeps its sign regardless. The "sum or replace" choice (`SUM_QUESTION`: `The selected day already has this type of transaction.\nDo you want to sum the amounts?\n`, `TransactionDND.REPLACE/ADD_ANOTHER/ADD_SUM`) is **not** part of `adjustAmount`; it belongs to drag-and-drop / wild-transaction code outside part04.

`getWeekIncome(cal)`: step back to the configured first day of the week, then sum `amount > 0` of transactions whose payment is not disabled over 7 days. `getMonthIncome(cal)`: same over all days of `cal`'s month.

`Calculator.calculate(expr, weekIncome, monthIncome)` (used by every formula field): tokens split on space/tab and `+ - / * ( )`; `$` removed from tokens; numbers parsed with the locale `NumberFormat`, `%` characters inside a numeric token are ignored, optional one-letter suffix `k/K`=×1e3, `m/M`=×1e6, `b/B`=×1e9, `t/T`=×1e12; a leading `+` on a number is allowed; unary `+`/`-` in operand position; tokens `%n` → `n * weekIncome / 100`, `%%n` → `n * monthIncome / 100`; precedence `* /` (2) over `+ -` (1); parentheses; errors: "Empty expression", "Unexpected end of expression", "Syntax error near '<tok>'", "Division by zero!", "Not enough operands in the stack".

## 18. Small helpers
- `deleteTransaction(cal, pmt)`: `pmt.setNoTransaction(cal)` (option with `deleted = true`, amount = pmt.amount), `db.update(pmt)`.
- `toDouble(s)` (static): strip a leading `+`, remove all `$`, parse with `numFormatFrac2` (locale grouping/decimal) → `doubleValue`; any failure → `0.0`.
- `parseDouble(s)`: `Double.parseDouble(s without "$")` → `0.0` on failure.
- `getDesc(s)`: cut at the first `;`; if `calendarDescriptionMaxLen != -1` and the result is longer → `s.substring(0, len-3) + "..."` (uses the string's own length, not the max — only trims 3 characters; quirk (verified against regenerated pseudo code)).
- `remove_slash_r` removes `\r`; `remove_new_lines` turns `\r`/`\n` into spaces.
- `escape_html_text`: `& < > " '` → entities; code points > 255 → `&#x…;`.
- `duplicate_transaction(pmt, cal, amount)`: clone (§9), `amount`, `startDate = endDate = cal`, `n_pays = 1`, end mode N_PAYS, `db.add`, `scheduleUpdateCalendarView()`; returns the clone (a one-off payment on that day).
- `isOldPayment(pmt, from, to)`: true iff `amount != 0` and not INDEFINITE and no options in `[from, to]` and `endDate <= from`.
- `set_pmt_flag(pmt, date, flag, amount)`: `pmt.setFlag(date, flag)`; if tentative and `flag == PAID` → `pmt.setCustomAmount(date, amount)` (freezes a tentative transaction); if tentative → `db.update(pmt)` (non-tentative flag changes do not trigger a DB update here).
- `remove_adjustment(pmt, cal)`: if (`endMode == LOAN_PAID` and `db.isRegularPaymentOn(pmt, cal)`) or `pmt.isSticky()` → `pmt.clear(cal)` (removes the option **and** the flag at that date); otherwise `pmt.setCustomAmount(cal, pmt.amount)` (explicit option equal to the standard amount); `db.update`, `updateCalendarView()`.

## 19. Transaction menus
`fillAllTransactionsMenu(pmt, menu, filter, matchingOnly, cal, onSelect)`:
1. Walk days from `minDate` while `db.getDay(d) != null`; collect `TranInfo{date, amount, pmt}` for transactions whose payment description equals `pmt.description` (all payments sharing the description, including disabled); when `matchingOnly && filter != null` skip those with `!filter.isMatch(pmt, amount, date)`.
2. Group into year → month (`TranInfo` with `subinfo`); sums (`year.amount`, `month.amount`, `total`, `totalUpToCurMonth` = months with `YYYYMM <= curCal`) exclude disabled payments.
3. No entries → one disabled item `No transactions found` + (` -- try enabling 'Unlimited history' in Preferences` unless `cfg.unlimitedHistory`); return.
4. Per year: cascade item `"<yyyy>: <fmt(sum)>"` (icon `icons/nxt.gif` when year == `cal` year). Per month: if the month has more than one transaction → cascade `"<Mon>-<yyyy>: <fmt(sum)>"` (nxt.gif when same `YYYYMM` as `cal`), else its single day item goes directly under the year. Day item: `("* " if filter matches) + "<dd-MMM-yyyy>: <fmt(amount)>" + pmt.getOneLineNote(date)` (`" -- <note on one line>"`, cut to 78 chars + `...`); enabled iff that payment is not disabled; `data = TranInfo`; nxt.gif when same `YYYYMM` as `cal` and placed directly under the year; `onSelect` attached. A matched item also prefixes `"* "` onto ancestor cascade items (only when the grand-parent chain exists — items directly under the year do not propagate).
5. If any entries: separator + disabled `"Total: <fmt(total)> (<fmt(uptoCur)>/<fmt(total-uptoCur)>)"`.
6. If `!matchingOnly`: `db.getAllDeletedOptions(description, minDate, maxDate)` non-empty → separator + cascade **"deleted transactions"** filled by `fillDeletedTransactionsMenu`.

`fillDeletedTransactionsMenu(menu, options sorted by date, shellToClose)`: cascades `"<yyyy>"` (nxt.gif if `curCal` year) → `"<Mon>"` (nxt.gif if `curCal` month) → day items `"<d>"` with `data = PaymentOption` and `closeThisShell = shell`. Selecting (`deletedOptSelected`, `Budget$123`): if the month is within `[minDate, maxDate]`: close the shell (if any), `curCal = that month`, `updateCalendarView()`, `flash_cell = cells[day-1]`.

`all_transactions_menu_listener` (`Budget$124`, on menu show): `filter` = menu data `"filter"`, or built from the payments-table filter text when the menu has `payment_table`; `matchingOnly = data("matching-only") != null`; if the menu already has items and an `Info` is bound → nothing (cached). Otherwise target = `Info.pmt/cal` or the table's selected payment with `cal = today` (no selection → clear the menu, return); clear items; fill with a selection handler that navigates like the deleted-options one (closing the shell stored in `data("shell")`).

Six-month average menu (`six_month_avg_menu_listener`, `Budget$126`), items are copy-to-clipboard via `add_six_mi` (`"<label>:\t<fmt>"`, skipped when NaN; click copies the formatted number): `"<MMM-yyyy> to <MMM-yyyy>"`, `"# of tran. actual/planned: <n>/<np>"`, `"# of months: <nm>"`, `"(click an item to copy)"`, `Avg. per transaction` (pt), `Avg. per month` (pm), `Avg. per day` (pd), `Avg. per planned period` (pp), `Avg. amount per <k> days` (when `first_last_days_diff != 0`, `k = round(diff / (n-1))`, `k >= 1`, value = `total / diff * k`), `Total amount`. No data → disabled `no data found.`.

`getSixMonthsAvg(desc, cal)`: window = `[firstOfMonth(cal) − 6 months, firstOfMonth(cal))`; count transactions (`n`), `total`, distinct months (`nm`), `first`/`last` dates, for non-disabled payments whose description `equalsIgnoreCase(desc)`; if `n >= 3` → `first_last_days_diff = days(first, last)`; then `first` := 1st of its month, `last` := last day of its month; `pt = total/n`, `pm = total/nm`, `pd = pm/30.25`, `np = db.getNumberOfPlannedTransactions(desc, first, last)`, `pp = total/np` (NaN when the divisor is 0).

Payments-remaining menu (`Budget$127`, `add_rem_item` = copyable item): `"<n>"`, `"Amount: ~<currencyPrefix><numFormatFrac0(roundUp(|abs_amount|))><currencySuffix>"`, `"Last Payment on: <dd-MMM-yyyy endDate>"`.

## 20. Highlight colours
`hilight_color` / `hilight_color_name` (RGB int `0xRRGGBB`):

| idx | int | hex | name |
|---|---|---|---|
|0|16766720|FFD700|Gold|
|1|16416882|FA8072|Salmon|
|2|16761035|FFC0CB|Pink|
|3|16744272|FF7F50|Coral|
|4|12433259|BDB76B|Dark Khaki|
|5|15631086|EE82EE|Violet|
|6|65280|00FF00|Lime|
|7|10145074|9ACD32|Yellow Green|
|8|9419919|8FBC8F|Dark See Green (sic)|
|9|8900331|87CEEB|Sky Blue|
|10|16770273|FFE4E1|Misty Rose|

- `int2color(c)` → `Color((c>>16)&255, (c>>8)&255, c&255)`. `get_RGB(c)` → `null` if `c` is one of the table values, else the RGB. `get_customer_rgbs(db)` → distinct custom (non-table) colours found in any payment's `hilighting` list (used to pre-populate the Windows ColorDialog custom palette).
- Highlighting menu (`hilighting_menu_listener`, `Budget$130`; menu data = `Info{cal, pmt, bg_color, bg_label}`; built once): **"Custom"** (with a 40×20 swatch of the current colour if any) → `Budget$129`: `ColorDialog` (initial RGB = `pmt.getHilighting(cal)` if set; on Windows `setRGBs(get_customer_rgbs)`); result `c = r<<16|g<<8|b`; separator; the 11 named colours with 40×20 swatches (`Budget$128`); separator; **"default"** (colour −1).
- Applying a colour: if `info.cal == 99990101` (edit dialog) → `info.bg_color = c`, swatch background = colour (or the parent's background when −1); else → `pmt.setHilighting(cal, c)` (−1 removes) and `updateCalendarView()`.

## 21. `pickWildPaymentDialog(parent, filterText, cal, amount)` → Payment | null
`transfer = (filterText == null)`. Shell (TITLE|RESIZE|APPLICATION_MODAL) titled `"Select a payment to transfer <fmt(|amount|)> to.."` when `amount < 0`, `"… from.."` otherwise (transfer mode), else **"Add a transaction of..."**. Label: (`"ONLY showing DISABLED and STICKY payments!\n"` in transfer mode) + `Type to reduce the selection list...`. Text = `filterText` (select all, focused). Tree (FULL_SELECTION|BORDER) with 4 columns: 20 px, 25 px (image), 250 px (description), `width("-100,000.00")+6` px right-aligned (amount). `fillWildPaymentTable` (part05): `db.sort()`; payments with `id != 0` whose upper-case description contains the upper-case filter; in transfer mode only payments that are **disabled and sticky** and (INDEFINITE or have an option after `cal − 1 year`); row shows image, `getDesc(description)`, amount = `getAmountBeforeDate(cal)` in transfer mode else `pmt.amount`; consecutive payments with the same description are grouped under one tree item (details in part05); disabled rows in `cfg.colorDisabledPayment`. Double-click a leaf (`Budget$147`) → result. Keys in the text (`Budget$148`): Up/Down/PageUp/PageDown → focus the tree; Enter → first tree item. Text modify → refill. Buttons: **"New payment..."** (not in transfer mode; `Budget$150`): close, `edit_payments_new_pmt = null`, `editPayment(main, null, cal)` → `updateCalendarView()` if changed; **"Cancel"**. Size `computeSize(-1, 480)`.

---

## UNKNOWN / not determined from part04
1. `editPayment`: whether `db.deletePayment` bumps `updateCount` (needed for the Delete button to make `editPayment` return true) — BudgetDB scope.
2. `showPayments` → Edit Category / Delete Category change `Payment.category` in place without `db.update`; whether the window returns true (and the calendar refreshes) depends on `updateCount` — UNKNOWN.
3. `save`/`saveDB`: autosave and backup behaviour is not in these methods (`BudgetDB.save`, `saveAndCleanup`, `saveRc`, `addRecentFile` are elsewhere).
4. `adjustAmount`: the "sum or replace" duplicate-transaction choice (`SUM_QUESTION`, `TransactionDND`) is not in part04.
5. `fillPickDate`: the past/future cell colouring looks inverted (`bgColorCell` for days before today, `bgColorCellPast` for today and later) — reproduced as written; `SimpleCalendar.lt` verified as a plain date comparison, so the inversion is real.
6. `pickWildPaymentDialog`: grouping logic of same-description payments in `fillWildPaymentTable` (part05) not fully traced.
7. `Budget.isValidKey` (registration key format) is in part05.
8. `editPayment` suggested-amount tooltip when `pt` is NaN (Java `DecimalFormat` prints the locale NaN symbol, normally "NaN").

Verified pass: 2026-10-27 — 1 statements corrected, 5 confirmed.
