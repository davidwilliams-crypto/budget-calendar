# 06 - Stats window ("Income vs. Expenses") and all HTML/Excel reports

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part06.txt` (plus `fillStats`/`add_row`/`calculate_stats_range`/`saveHTML` in part05, helper classes at the end of part02, anonymous listeners `Budget$169..$183` in part01). Everything below was read from the pseudo code; nothing is inferred from running the app.

Conventions used in this file:

* `Frac0`, `Frac1`, `Frac2` = `DecimalFormat.getNumberInstance()` with min=max fraction digits 0/1/2 and **grouping on** (e.g. `1,234.56`). Locale-dependent separators.
* `sdate(x)` = `"dd-MMM-yyyy"` (e.g. `05-Mar-2024`). `sdate_MY(x)` = `"MMMMMMMMMMMM-yyyy"` = full month name + `-yyyy` (`March-2024`).
* `MMM, yyyy` strings come from `SimpleCalendar.toString("MMM, yyyy")` which strips dots (`Mar, 2024`).
* `nbsp3` = `&nbsp;&nbsp;&nbsp;`, `nbsp4` = `&nbsp;&nbsp;&nbsp;&nbsp;`.
* `indent_string` = three spaces `"   "`. Leaf (per-payment) stats keys are stored as `"   " + description`; category keys are not indented.
* Flags: `';'` (59) = PAID, `'#'` (35) = IMPORTANT, `'^'` (94) = none (`PaymentFlag`).
* `GEN_HTML = 0`, `GEN_EXCEL = 1`.
* `normalColors = ["black","red"]` (index 0 for amount >= 0, 1 for < 0).

---

## 1. Shared helpers

### 1.1 `amount2html(double v, String cssClass, String[] colors, boolean decimals)`

```
fmt    = decimals ? Frac2 : Frac0
if v == -0.0 -> v = 0.0
color  = v < 0 ? colors[1] : colors[0]
open   = cssClass == null ? ">" : " class='" + cssClass + "'>"
s      = "<FONT color=" + color + open + fmt.format(v)
if s contains the locale decimal separator at index i:
     s = s[0..i) + "<small>" + s[i..] + "</small></font>"
else s = s + "</font>"
```
Note the `<small>` wraps the separator and the fraction digits. The 3-argument overload uses `cfg.showDecimalsInCV` as `decimals`.

* `f2html(v)` = `amount2html(v, null, normalColors, true)` (always 2 decimals).
* `f2html_no_zero(v)`: if `((int)(v*100)) / 100 == 0` (integer division, i.e. `|v| < 1.0`) return `""`; else `amount2html(v, null, normalColors, cfg.showDecimalsInCV)`.

### 1.2 `escape_html_text(s)`
null/empty -> `""`. Replaces `&`->`&amp;`, `<`->`&lt;`, `>`->`&gt;`, `"`->`&quot;`, `'`->`&#39;`; code points > 255 -> `&#x{hex};`; others copied.

### 1.3 `getDesc(desc)`
Cuts the description at the first `;` (everything from `;` on is dropped). Then, if `calendarDescriptionMaxLen != -1` (it is always `-1` in this build) and longer, truncates with `...`. So effectively: text before the first semicolon.

### 1.4 `get_normalized_note(note, mode, leadingBR)`
* null -> `""`.
* `GEN_EXCEL`: `"  (Note: " + firstLine + "...)"` if the note has a newline, otherwise `"  (Note: " + note + ")"`.
* `GEN_HTML`: `(leadingBR ? "<BR>" : "") + "<FONT COLOR=gray>Note: <I>" + note + "</I></FONT>"`, then every `\n` replaced by `<BR>`. The note text is NOT HTML-escaped.

### 1.5 `html_string(pmt, text)`
`pmt.disabled ? "<FONT color=gray>" + text + "</FONT>" : text`.

### 1.6 `getExcelNumberStyle()` = `STYLE="vnd.ms-excel.numberformat:###0.00" ` (trailing space).

### 1.7 `repeat(s, n)` = s repeated n times (`""` if n <= 0).

### 1.8 `html_merge_styles(style, extra)`
If `style` is blank -> `" style='" + extra + "'"`; else `style.replace(";'", ";" + extra + "'")` (appends `extra` inside the existing quoted style, relying on the style ending with `;'`).

### 1.9 `embeddedSRC(path, isExcel)`
If `!isExcel` and `path` starts with `file://` and the file exists: returns `"data:;base64," + Base64(fileBytes)` (no MIME type). Otherwise (Excel, or missing file) returns `path` unchanged. `get_image_full_path(p)` = `"file://" + p`. Icons: `icon(name)` = `"icons/" + name`; payment images: `image(name)` = `"images/" + name` (relative paths; `images_loc`/`icons_loc` may be re-pointed at startup - outside this scope).

### 1.10 `genRepImage(value, totalIncome, totalExpense, width)`
```
max = max(|totalIncome|, |totalExpense|); if max == 0 -> 1
gif = value < 0 ? "red_g.gif" : "blue_g.gif"      (in icons dir, embedded as base64 if present)
"<img src=\"" + src + "\" height=10 width=" + (int)(width * |value| / max) + ">\n"
```

### 1.11 `UnicodishBufferedWriter`
All report writers except `saveHTML` use it. `write(s)` converts every char that is >= 128, or is `,` (44), `\n` (10) or `\r` (13) into `&#xHHHH;` (4 hex digits, lowercase). `writeRaw(s)` writes verbatim. Consequence: commas and newlines inside anything passed through `write()` appear as `&#x002c;` / `&#x000a;` in the file (browsers render them normally). Only the document head of the month-by-month report and its row newlines use `writeRaw`.

### 1.12 Opening the output (`getHtmlFileName`, `html_name`, launching)
* `getHtmlFileName()`: `File.createTempFile("bgt-report-", ".html")` in the JVM temp dir, `deleteOnExit()`, returns absolute path. On exception shows error `"Unable to create output file:\n" + ex` and returns null (callers silently return).
* `html_name(path)`: `"file://" + path.replace('\\','/')`; on macOS additionally every space -> `%20`.
* HTML reports: `Program.launch(html_name(path))` = OS default handler for the URL (browser).
* Excel reports: `excel_prg = Program.findProgram(".xls")`, falling back to `Program.findProgram(".xlsx")` (set in `open()`); the report is still written as `.html` and opened with `excel_prg.execute(path)`. If `excel_prg == null` the Excel menu items are created disabled.
* All report actions catch `Exception` and show `showError(mainShell, "Unable to save HTML file\n"+ex)` / `"Unable to save the output file\n"+ex` / (month-by-month) `"Unable to save HTML file!\n"+ex` / (Excel stats) `"Unable to save output file\n"+ex`.
* `openBudgetFile(path)` is unrelated to reports: asks the password if `BudgetDB.isEncrypted`, `BudgetDB.load(path, key, errBuf, calFromInitCal(day=1), cfg.futureNMonths, cfg.unlimitedHistory)`; on null shows `"Unable to load the '<path>' file."`; on success sets `curCal`, `db`, `fileName`, `dbKey`, adds to recent files, `checkRegistration`, `calc_min_date`, `updateMonthLabelMenu`, `updateShellTitle`, `updateCalendarView`.

### 1.13 `fixShellLocation(display, size, loc)` / `isInBounds`
Display bounds shrunk by 50 px on every side; if the rectangle `(loc, size)` does not intersect it, `loc = centerLoc(size)`. `centerLoc(size)` centers on the main shell bounds if visible, else on the display. `isInBounds(display, point)` = `display.getBounds().contains(point)` (unused here).

---

## 2. Statistics model (`Budget$Stats`, `calcStatistics`, `calculate_stats_range`)

### 2.1 Data classes
* `Budget$Amount { double amount; double planned_amount; String key; HashMap<String,Amount> sub_amounts (always non-null, created in ctor); Payment pmt; Amount agg_parent; }` ; `is_aggregate()` = `sub_amounts != null && size > 0`.
* `Budget$AmountKey { String category; String description; }` value-equal on both. `AmountKey(Amount a)` = `(a.agg_parent == null ? null : a.agg_parent.key, a.key)`; `AmountKey(desc)` = `(null, desc)`.
* `Budget$Stats { boolean fullStats; immutable startCal, endCal; Vector incomeVec, expenseVec; HashMap statsMap (category key -> Amount); double total_income, total_expense, total_planned_income, total_planned_expense; int firstColWidth = -1; }`
  * `get_duration()` = `getMonthsDiff(startCal, endCal)` = `(endY*12+endM) - (startY*12+startM)` (0 for a single month).
  * `get_range_string()` = duration 0 ? `"MMM, yyyy"` of start : `"<start MMM, yyyy> - <end MMM, yyyy> (<duration+1>m)"`.
  * `get_flat_map()` = `AmountKey -> Amount` for every category and every leaf.

### 2.2 Ranges
* `calcStatistics(cal, nMonths, full)`: start = `cal` with day 1; end = start + nMonths months - 1 day (last day of the last month).
* `calcStatisticsExact(start, end, full)`: exact days.

### 2.3 `calculate_stats_range(stats)` - what is counted
1. Two empty categories are pre-created: `(Uncategorized-Income)` and `(Uncategorized-Expenses)`.
2. **Planned pass**: for every day in `[start, end]`, for every payment in `day.getRegulars()` (regular scheduled occurrences on that day): skip if `calViewFilter != null && !filter.isMatch(pmt, pmt.amount, -1)`; skip if `pmt.disabled`; `addStatInfo(category, description, 0.0, pmt, planned=true)` (adds `pmt.amount` to `planned_amount` of the category and of the leaf); `total_planned_expense += pmt.amount` if negative else `total_planned_income += pmt.amount`.
3. **Actual pass**: for every day with transactions: for each `Transaction tr` (`tr.pmt`, `tr.amount`): skip if filter mismatch (`isMatch(pmt, tr.amount, dayInt)`); skip if `pmt.disabled`; `addStatInfo(category, description, tr.amount, pmt, false)`. Then, if `day.adjustment_diff != 0`: `addStatInfo(null, "(Unaccounted)", adjustment_diff, null, false)`.
   * Deleted transactions are not in `day.transactions` (they are in `restorable`) and therefore excluded. Future months are included as long as they are within the range (the DB is loaded with `cfg.futureNMonths` months ahead). Disabled payments are excluded from both actual and planned numbers.
4. `addStatInfo(cat, desc, amt, pmt, planned)`: if `cat == null && fullStats`: `cat = ((planned ? pmt.amount : amt) < 0) ? "(Uncategorized-Expenses)" : "(Uncategorized-Income)"`. Category `Amount` (key = cat) gets `amount += amt` and, if planned, `planned_amount += pmt.amount`. Leaf key = `"   " + desc` stored in `category.sub_amounts`; leaf `pmt` = first payment seen with that description, `agg_parent` = category; `leaf.amount += amt`, planned likewise. (If `cat == null` and not fullStats, the leaf is put directly into `statsMap` with un-indented key - only used by the quick-stats pane, never in the Stats window which always passes `full = true`.)
5. **Uncategorized reconciliation**: for every description present in both uncategorized categories: `sum = inc.amount + exp.amount`; if `sum > 0` the expense leaf is removed (`expCat.amount -= exp.amount; incCat.amount += exp.amount; incLeaf.amount = sum`) else symmetric. Then `transferSubAmount(incCat -> expCat, negativeOnly)` moves leaves with `amount < 0` (adjusting `amount` and `planned_amount` of both categories) and `transferSubAmount(expCat -> incCat, nonNegativeOnly)` moves leaves with `amount >= 0`. Uncategorized categories with no leaves are removed from `statsMap`.
6. Totals: for each category: `amount < 0 ? total_expense += amount : total_income += amount` (a category is income or expense by its **net** sign).
7. Ordering (`stat_comparator`, `lambda$new$0`): if both amounts < 0 -> ascending (most negative first); otherwise descending (largest first). Categories sorted; a category with `amount < 0` goes to `expenseVec` else `incomeVec`, immediately followed by its leaves sorted with the same comparator. So each vector is `[cat, leaf, leaf, cat, leaf, ...]`; leaves are recognisable by the 3-space indent of their key.

---

## 3. The Stats window (`showStats`)

### 3.1 Window
* `Shell(parent, 67696)` = `APPLICATION_MODAL | BORDER | CLOSE | TITLE | RESIZE` (no min/max). Title `"Income vs. Expenses"`, icon `icons/det.bmp`, background `cfg.bgColorWindow`, `GridLayout` 1 column.
* Opening the window **clears any active calendar filter**: `setFilterText(""); calViewFilter = null; updateCalendarView()`.
* Initial stats: `showStatsStats = calcStatistics(curCal, 1, true)` (the month shown in the main window).
* Date format for the From/To labels: `"d-MMM-yyyy"` (no zero padding, e.g. `1-Mar-2024`).
* Size: `computeSize + 20` in width; `width = max(cfg.statWidth, width)`; `height = cfg.statHeight` if `!= -1`. Location: `centerLoc(size)`, overridden by `cfg.statX`/`cfg.statY` when `!= -1`, then `fixShellLocation`. On dispose the bounds are stored in `cfg.statX/statY/statWidth/statHeight`. The method then runs its own `readAndDispatch` loop until the shell is disposed (modal).
* `shell.setData("updateCallback", Budget$173)` - an `UpdateCallback` whose `update()` calls `full_stats_update` (used after "Edit payment" from the transactions popup).

### 3.2 Top row (Composite, `GridLayout(6)`, `FILL_HORIZONTAL`)
1. Label `"From"` - font `cfg.fontDate`, aligned END, fg white if window bg is dark else black (`isDarkColor`: r<10&&g<10, or HSB brightness < 0.7).
2. **Start label** (`CENTER`): font `fontDate`, text = stats start date, fg `cfg.colorCurrentMonth`, underlined by `lbl_underline` (1-px line across the full label width in `colorCurrentMonth`), size hint = computed size x 1.1 (both axes). `setData(startCal)`. Clicking (mouseDown) opens its menu (3.3). The menu is attached with `setMenu`.
3. Label `"to"` (END).
4. **End label**: same styling, text = stats end date, same width/height hints, `setData(startCal)` (yes - initially both labels carry the start month), own menu.
5. **Info label**: text `"( 1m )"`, aligned CENTER (`GridData(64)`), shows the range length.
6. **`"Actions..."` label**: fg `colorCurrentMonth`, underlined, `GridData(640)` = grab horizontal + align END; mouseDown shows the Actions popup menu (3.5).

### 3.3 From/To menus (`createStatStartEndMenu(shell, months, listener, thisLabel, otherLabel)`)
`months` = every month (day 1) from `minDate` to `maxDate` inclusive (`showStats` builds the array by stepping `minDate` one month at a time while `inRange(minDate, maxDate)`).

* `currentYear` = today's year; month name format `"MMMMMMMMMMMM"` (full month name).
* `oldYears` = years in `months` with `year < currentYear - 2`. If `oldYears.size() >= 2`: one cascade item titled `"<minOldYear> - <maxOldYear>"` whose submenu lists those years **descending**, each as a year submenu (see `add_year`). If fewer than 2 old years, `oldYears` is cleared and those years are listed normally.
* Remaining years (`TreeSet`, ascending): for `year == currentYear` a separator is added before and after, and its months are inlined into the top-level menu (`add_year(menu, year, submenu=false)`); other years get a cascade item titled with the year (`add_year(menu, year, true)`).
* `add_year` (lambda `createStatStartEndMenu$0`): months of that year sorted ascending. If `otherLabel != null` and there are >= 2 months of that year: first item text `"<MonthName min> <year> to <MonthName max> <year>"` with data = min month, `sourceLabel` = thisLabel, `sourceLabel2` = otherLabel, `sourceCal2` = max month, followed by a separator. Then one item per month: text = `(year == currentYear ? year + ", " : "") + MonthName`, data = that month, `sourceLabel` = thisLabel; the month equal to `initCal` (today's month, compared by `getYm`) gets the icon `icons/nxt.gif`.
* After creation, `lambda$showStats$1` appends to each label's menu: separator, `"&Last 30 days"`, `"Last 365 days"`.
* The "to" label's menu is built with the labels swapped (`createStatStartEndMenu(shell, months, listener, endLabel, startLabel)`), so its whole-year item sets the end label to January and the start label to December; `full_stats_update` swaps them back.

**Selection (`Budget$172.widgetSelected`)**: `sourceLabel.setData(item.data)`; if `sourceLabel2` present: `sourceLabel2.setData(sourceCal2)`; then `full_stats_update(fmt, startLabel, endLabel, infoLabel)` (labels have redraw suspended meanwhile).

### 3.4 `full_stats_update` / `display_stats` / Last 30 days / Last 365 days
`full_stats_update`:
```
start = startLabel.data; end = endLabel.data; if start > end swap; store back into the labels
endExcl = copy(end).set(day,1).add(month,1)
n = getMonthsDiff(start, endExcl)        // number of months, >= 1
years = n / 12; months = n % 12
infoLabel.text = "( " + (years != 0 ? years + "y " : "") + (months > 0 ? months + "m " : "") + ")"
                 // e.g. "( 1m )", "( 1y )", "( 1y 2m )"
showStatsStats = calcStatistics(start, n, true)
display_stats(...)
```
`display_stats`: start/end label texts = `d-MMM-yyyy` of `showStatsStats.startCal/endCal`; disposes all children of the table composite (`stat_sv`) and calls `fillStats(showStatsStats, stat_sv, MODE_ALL = 4079, false)`.

`"&Last 30 days"` (`lambda$showStats$0` with true): info label `"(30d)"`; `showStatsStats = calcStatisticsExact(today - 29 days, today, true)`.
`"Last 365 days"`: info label `"(1y)"`; `showStatsStats = calcStatisticsExact(today - 1 year + 1 day, today, true)`.
In both cases only the label **texts** are updated; the labels' **data** (used by the three monthly reports, see 3.5) keep the previously selected months.

### 3.5 Actions menu (`new Menu(shell, 0)`, used as a popup; the From/To menus are `Menu(shell, SWT.POP_UP)`), items in order
| Item | Action |
|---|---|
| `Print` (`Budget$175`) | `generateStatsReport(showStatsStats, tmp)`; open in browser (section 4) |
| `Excel Report` (`Budget$176`, disabled when no Excel) | `generateExcelStatsReport(showStatsStats, tmp)`; `excel_prg.execute(tmp)` (section 5) |
| `Print Monthly Calendar` (`Budget$177`) | `saveHTML(tmp, startLabel.data, endLabel.data)`; browser (section 10) |
| `Monthly Excel Report` (`Budget$178`, disabled when no Excel) | `generateReport(startLabel.data, endLabel.data, tmp, GEN_EXCEL)`; Excel (section 8) |
| `Monthly Detailed Report` (`Budget$179`) | `generateReport(startLabel.data, endLabel.data, tmp, GEN_HTML)`; browser |
| `&Unpaid && paid Report` (`Budget$180`) | `generatePaidUnpaidReport(showStatsStats.startCal, showStatsStats.endCal, tmp)`; browser (section 9) |
| separator | |
| `&Comparative report to...` (`lambda$showStats$4`) | `compare_stats_report(shell, months, showStatsStats)` (section 6) |
| `&Month-by-Month report` (`lambda$showStats$5`) | `report_month_by_month(shell, showStatsStats)` (section 7) |

Note the three "Monthly ..." items use the **label data** (month granularity, as selected via the menus - not affected by Last 30/365 days), whereas Print/Excel/Unpaid/Comparative/Month-by-Month use `showStatsStats`.

The same reports exist in the main window File menu: `&Print this month` (`Budget$110`: `saveHTML(tmp, curCal, curCal)`), `&Excel report` (`Budget$111`: `generateReport(curCal, curCal, tmp, GEN_EXCEL)`), `&Detailed report` (`Budget$112`: `generateReport(curCal, curCal, tmp, GEN_HTML)`), `&Unpaid && paid report` (`lambda$createFileMenu$0`: `generatePaidUnpaidReport(curCal day 1, curCal last day, tmp)`). The CLI option `-r` ("print the current budget month and exit") runs `saveHTML(tmp, curCal, curCal)` + launch and returns before the main loop.

### 3.6 The table (`fillStats(stats, parent, mode, quickPane=false)` + `add_row`)
`fillStats` is shared with the quick-stats pane of the main window (`quickPane = true`); the Stats window always calls it with `MODE_ALL = 4079` and `quickPane = false`. Mode bits: 1 TOTAL_INCOME, 2 INCOME, 4 TOTAL_EXPENSES, 8 EXPENSES, 16 TOP_EXPENSES (not in MODE_ALL), 32 DISPOSABLE.

* Parent background `cfg.bgColorWindow`; `GridLayout(6 columns)` (5 in the quick pane: no avg/mth), horizontal/vertical spacing 1, marginHeight 0.
* Header labels (`add_hdr`): font `cfg.fontTran`, fg `cfg.colorCalendarHeader`, bg `cfg.bgColorCalendarHeader`, `GridData(HORIZONTAL_ALIGN_FILL)`; width hint = computed width of a sizing string:
  `Description` (left, sized with `"Disposable Income   "`; this width is stored in `stats.firstColWidth`), `Amount` (right, `"-10,000  "`), `avg/mth` (right, `"-avg/mth+1"`), `%` (right, `"-100  "`), `+/-` (right, `"-10,000  "`, tooltip `"This is the difference between planned and the actual amounts"`), `Graph` (left, width 170).
* `nMonths = getMonthsDiff(start, end) + 1`.
* Rows, in order (each via `add_row(stats, parent, desc, amount, avg, indented, Amount, diff, quickPane)`):
  1. `"Disposable Income"`: `ti + te`, avg `/nMonths`, not indented, Amount null, diff 0; tooltip `"When hovering Calendar View Displays in each day:\n-- max disposable amount, and\n-- in square brackets the difference between planned\nand actual amounts on that day"`.
  2. `"Total Income"`: `ti`, diff `ti - total_planned_income`.
  3. each `incomeVec` entry: `getDesc(key)` (categories un-indented, leaves keep their 3 leading spaces), `amount`, `amount/nMonths`, indented=true, diff `amount - planned_amount`.
  4. `"Total Expenses"`: `te`, diff `te - total_planned_expense`.
  5. each `expenseVec` entry (same as 3). (With TOP_EXPENSES only the first 6 categories plus an `"Other..."` row would be shown - not used in this window.)
* `add_row` details:
  * Row background alternates: `stat_line_color` starts false; a row uses `bg_darker_color` when the flag is true, else `cfg.bgColorWindow` (`cfg.bgColorDetails` in the quick pane); the flag toggles after every row. So row 1 = window colour, row 2 = darker, ...
  * Description label (left): text = `(indented ? "  " : "") + desc`; font `fontTran`; fg white/black by darkness of the row bg; `setData("Amount", Amount)`, `setData("Stats", stats)`; mouse-track listener `stat_mta`; mouse listener `stat_transactions_ma` (window) or `stat_ma` (quick pane). There is code to style leaf rows as links (indent 20, trimmed text, fg `colorCurrentMonth`, text-only underline) but it requires `Amount.sub_amounts == null`, which never happens (`sub_amounts` is always allocated), so it is dead.
  * Amount label (right): `fmt = showDecimalsInCV ? Frac2 : Frac0`; fg `cfg.colorNegativeAmount` if `isNegative(amount, showDecimals)` (`v < -0.01` when decimals shown, else `(long)v < 0`) else `cfg.colorPositiveAmount`.
  * avg/mth label (window only): `fmt.format(avg)`, same fg as amount.
  * `%` label: `"?"` if `ti == 0 && te == 0`; `Frac0(amount / ti * 100)` if `ti != 0`; else `Frac0(amount / te * 100)`. Same fg as amount.
  * `+/-` label: `""` if `|diff| < 5`, else `fmt.format(diff)`; fg by sign of diff; tooltip when `|diff| >= 5`: `"The difference between actual amount of %s and planned amount of %s"` with `fmt(amount)`, `fmt(amount - diff)`.
  * Graph label: image 170x9 (`gen_image`): background = row area colour (`bgColorWindow` / `bgColorDetails`), bar width `(int)(170 * |amount| / max(|ti|,|te|))` (max forced to 1 if 0), bar colour `cfg.graphBarColor` if `amount > 0` else `cfg.graphNegativeBarColor`.
* After filling: `parent.setSize(computeSize)`, `ScrolledComposite.setMinSize(size)`.

### 3.7 Mouse behaviour on description labels
* **Hover** (`stat_mta`, `Budget$171`): saves bg/fg in data `bgcolor`/`fgcolor`, sets bg `cfg.bgColorHighlight` and fg white/black by its darkness. If no calendar filter is active: hovering `"Disposable Income"` overlays, in every day cell of the main calendar (current month), a centred label `"<bestSavings> [<monthPlusMinus>]"` (`Frac0` of the int-truncated values from `getBestSavings(curCal)` / `getMonthPlusMinus(curCal)`), fg `colorPositiveAmount`, bg `bgColorMaxBalance` for the max day, `bgColorMinBalance` for the min day, else `display.getSystemColor(37)` (`SWT.COLOR_WIDGET_DISABLED_FOREGROUND` in current SWT), font `fontMonth`; other cell children are hidden. Hovering a payment/category row highlights (bg `cfg.bgColorCellHighlight`) every day cell of the current month that contains a transaction of that payment (same `Payment` object or same description) or, for a category row, any payment of that category; inside those cells only the matching transaction labels stay visible. `mouseExit` restores colours and visibility and disposes the overlay labels.
* **Click in the Stats window** (`stat_transactions_ma`, `Budget$170`): only for rows whose `Amount.pmt != null` (leaf rows). Opens a popup menu built by `fillStatsTransactionMenu(menu, stats, pmt, Budget$170$1)`:
  * Scans every day of the stats range; collects transactions whose `getDesc(description)` equals `getDesc(pmt.description)` (all payment objects met are collected too).
  * If none: one disabled item `"No transactions found"` + (`cfg.unlimitedHistory` ? `""` : `" -- try enabling 'Unlimited history'"`).
  * Else: cascade per year `"<year>: <Frac2 yearSum>"` (`AMOUNT_SEP = ": "`); inside, per month: if the month has more than one transaction a cascade `"<monthName>-<year>: <Frac2 monthSum>"`, otherwise the single transaction item goes directly in the year menu. Transaction item: `"<dd-MMM-yyyy>: <getDesc(desc)>   <Frac2 amount><oneLineNote>"` (`getOneLineNote` = `" -- " + note without newlines`, cut to 78 chars + `"..."` if >= 80); disabled when the payment is disabled; data = `TranInfo`. Sums exclude disabled payments. Then separator and a disabled item `"Total: <Frac2 total> (<Frac2 totalUpToCurrentMonth>/<Frac2 rest>)"` where "up to current month" = `getYm(date) <= curCal.getYm()`.
  * Selecting a transaction (`Budget$170$1`): sets `curCal` to that month (day 1) if within `[minDate, maxDate]` and refreshes the main calendar.
  * Then (if any payments were found) a separator and one item per distinct `Payment`: text `"Edit payment"` if exactly one, otherwise `"Edit payment [<sdate start> ... <sdate end or '' when endMode == 16>]"`; selecting (`Budget$170$2`) runs `editPayment(activeShell, pmt, null)` then the shell's `updateCallback.update()` (= `full_stats_update`, recomputing the table).
* **Click in the quick-stats pane** (`stat_ma`, `Budget$169`, not the Stats window): leaf with `pmt` -> `calViewFilter = Filter("desc:" + description with spaces escaped as "\\ ")`; `"Total Income"` -> `">0"`; `"Total Expenses"` -> `"<0"`; `"Disposable Income"` -> nothing; any other (category) -> `"cat:" + name` (spaces escaped). Then the filter text box is set to the filter and the calendar view refreshed.
* There is **no click-to-copy** anywhere in the Stats window (the only clipboard code, `Budget$125`, belongs to a transaction context menu elsewhere).

---

## 4. "Print" -> `generateStatsReport(stats, path)` (HTML)

```
<HTML>
<HEAD>
	<TITLE>Budget Report</TITLE>
</HEAD>
<BODY bgcolor=ffffff>
<style type="text/css">
BODY { font: 10px Arial, Helvetica, sans-serif; }
P, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }
</style>
<TABLE BORDER=0 CELLSPACING=2>
<CAPTION>Budget from <B>{sdate(start)}</B> to <B>{sdate(end)}</B></CAPTION>
<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B><TD><B>Graph</B>
... rows ...
</TABLE></HTML>
```
(no `</BODY>`). `stat_rep_row_color` is reset to false first. Rows via `addRepRow(w, stats, desc, amount, indented, diff)`:

```
pct = (ti == 0) ? (te == 0 ? "?" : Frac1(amount/te*100)) : Frac1(amount/ti*100)
bg  = stat_rep_row_color ? "EEeeEE" : "FFffFF"        (first row FFffFF, then alternating)
"<TR bgcolor=" bg "><TD>" (indented ? nbsp3 : "") desc
"<TD ALIGN=RIGHT>" nbsp3 Frac2(amount)
"<TD ALIGN=RIGHT>" nbsp3 pct
"<TD ALIGN=RIGHT>" nbsp3 (|diff| > 5 ? Frac2(diff) : "")
toggle stat_rep_row_color
"<TD>" genRepImage(amount, ti, te, 300)
```
Rows: `Disposable Income` (`ti+te`, not indented, diff `(ti+te)-(pti+pte)`), `Total Income` (diff `ti-pti`), every `incomeVec` entry (desc = `escape_html_text(getDesc(key))`, prefixed with `nbsp4` when the key starts with the 3-space indent; indented=true; diff `amount-planned`), `Total Expenses` (diff `te-pte`), every `expenseVec` entry. Note the per-entry desc keeps its leading 3 spaces after the `nbsp4`.

---

## 5. "Excel Report" -> `generateExcelStatsReport(stats, path)`

Same head as section 4 (`\t<TITLE>Budget Report</TITLE>`, same style), then:

```
<TABLE BORDER=0 CELLSPACING=2>
<CAPTION>Budget from <B>{sdate(start)}</B> to <B>{sdate(end)}</B></CAPTION>
<TR bgcolor=eeEEee><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B>
```
Let `S` = `STYLE="vnd.ms-excel.numberformat:###0.00" ` and `den` = `"B3"` if `ti != 0` else `"B2"`. Row numbering assumes header = row 1 (the CAPTION is not a row): Disposable = row 2, Total Income = row 3, income entries from row 4, Total Expenses = row `4 + incomeVec.size()`, expense entries after it.

```
<TR><TD><B>Disposable Income:</B><TD ALIGN=RIGHT {S} FORMULA='=B3+B{incomeVec.size()+4}'>0.001<TD ALIGN=RIGHT {S} FORMULA='=B2/{den}*100'>?
<TD ALIGN=RIGHT>
<TR><TD><B>Total Income:</B><TD ALIGN=RIGHT {S}{getStatFormula('B', 4, incomeVec)}>0.001<TD ALIGN=RIGHT {S}>100.00
<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{Frac2(ti - pti)}</FONT>
```
Each entry (row counter `r` starting at 4):
```
desc = escape_html_text(getDesc(key)); if indented: desc = nbsp4 + desc, sumFormula = ""
else if entry.sub_amounts != null: sumFormula = " FORMULA='=SUM(B{r+1}..B{r+size(sub_amounts)})'"
<TR><TD>{nbsp3}{desc}<TD ALIGN=RIGHT {S}{sumFormula}>{Frac2(amount)}<TD ALIGN=RIGHT {S}FORMULA='=B{r}/{den}*100.0'>?
[ <TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{Frac2(amount - planned)}</FONT>   only if amount - planned != 0 ]
r++
```
Then:
```
<TR><TD><B>Total Expenses:</B><TD ALIGN=RIGHT {S}{getStatFormula('B', r+1, expenseVec)}>0.001<TD ALIGN=RIGHT {S}FORMULA='=B{r}/{den}*100.0'>?
<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{Frac2(te - pte)}</FONT>
r++
... expense entries exactly like income entries ...
</TABLE></HTML>
```
`getStatFormula(col, firstRow, vec)`: `" FORMULA=\"="` + for every entry whose key does **not** start with the indent (i.e. categories) `"+" + col + row` (row increments for every entry, leaves included); if no category was found `"0.0"`; then `"\" "`. Example: `FORMULA="=+B4+B7" `. Amount cells keep the literal placeholder `0.001` / `?`, expecting Excel to evaluate the formulas. The Frac2 values use grouping (`1,234.56`), unlike section 8.

---

## 6. "Comparative report to..." (`compare_stats_report`, `do_comparative_report`, `generate_comparison_report`, `add_comp_row`)

* `compare_stats_report(shell, months, base)`: `d = base.get_duration()`; `lower = base.startCal - d months`; candidate months = all `months` **not** in `[lower, base.endCal]` (i.e. any start month whose `d+1`-month window would overlap the base window is excluded). A popup menu is built with `createStatStartEndMenu(shell, candidates, Budget$183, null, null)` (same year/month structure, no whole-year items because the labels are null; **no** "Last 30/365 days" items) and shown immediately. Choosing a month (`Budget$183`) -> `do_comparative_report(base, month)`.
* `do_comparative_report(base, month)`: `other = calcStatistics(month, d + 1, true)` (same number of months as the base), temp file, `generate_comparison_report(path, base, other)`, launch browser.
* `generate_comparison_report` (`stat_rep_row_color` reset to false):
```
<HTML>
<HEAD>
<TITLE>Budget Comparison Report</TITLE>
</HEAD>
<BODY bgcolor=ffffff>
<style type="text/css">
BODY { font: 10px Arial, Helvetica, sans-serif; }
P, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }
</style>
<TABLE BORDER=0 CELLSPACING=2>
<CAPTION>Stats comparison of two ranges <B>{base.range_string}</B> vs <B>{other.range_string}</B></CAPTION>
<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=CENTER><B>{base.range_string}<br>(base)<br>amount</B><TD ALIGN=CENTER><B>{other.range_string}<br>(other)<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>%</B><TD><B>Graph</B>
rows...
</TABLE></HTML>
```
  Rows: `Disposable Income` (`base.ti+te` vs `other.ti+te`), `Total Income`, then `comp_entry_from(base.incomeVec, other.incomeVec)`, `Total Expenses`, then `comp_entry_from(base.expenseVec, other.expenseVec)`. Entry desc = `escape_html_text(getDesc(key))` with `nbsp4` prefix when indented; indented flag = true for entries.
* `comp_entry_from(baseVec, otherVec)`: list of `comp_entry{key, v1, v2}`: first every base entry in base order with `v1 = amount`; then for every other entry: if an entry with the same key exists set its `v2`, otherwise append a new entry with only `v2`. Finally entries with `v1 == 0 && v2 == 0` are dropped. (Matching is by raw key, so a leaf keeps its indent and a category/leaf with the same name cannot collide.)
* `add_comp_row(w, base, desc, v1, indented, v2)`:
```
v1 = trunc(v1*100)/100 ; v2 = trunc(v2*100)/100      (truncate to cents toward zero)
diff = v1 - v2
pct  = (v1 == 0 || diff == 0) ? +Infinity : diff / v1 * 100
bg   = stat_rep_row_color ? "EEeeEE" : "FFffFF"
<TR bgcolor={bg}><TD>{indented ? nbsp3 : ""}{desc}
<TD ALIGN=RIGHT>{nbsp3}{f2html(v1)}
<TD ALIGN=RIGHT>{nbsp3}{f2html(v2)}
<TD ALIGN=RIGHT>{nbsp3}{|diff| > 5 ? f2html(diff) : ""}
<TD ALIGN=RIGHT>{nbsp3}{|diff| > 5 && isFinite(pct) ? (pct > 0 ? "+" : "") + Frac0(pct) : ""}
toggle stat_rep_row_color
<TD>{genRepImage(v2, base.ti, base.te, 300)}
```
  Note: the graph bar is of the **other** value scaled against the **base** totals, and diff is base minus other.

---

## 7. "Month-by-Month report" (`report_month_by_month`, `get_month_by_month_entries`, `generate_month_by_month_report`)

* `report_month_by_month`: if `stats.get_duration() == 0` -> `showInfo(shell, "Please select more than one month in the Full Stats dialog.")` and return. Else temp file, generate, launch browser. Exceptions (including the consistency `RuntimeException`s below) -> `"Unable to save HTML file!\n" + ex` and stack trace to stderr.

### 7.1 `get_month_by_month_entries(stats)` -> `List<month_by_month_entry>`
`month_by_month_entry { AmountKey key; double total; List<Double> amounts; String style = ""; }`, `is_aggregate()` = `key.category == null`.
Let `M = duration + 1`; `monthly[i] = calcStatistics(start + i months, 1, true)`; `flat[i] = monthly[i].get_flat_map()`.
`STYLE_SEP = " style='padding-top: 1em; padding-bottom: 1em; border-top: 1px solid black;'"`.

1. `Disposable Income` entry (key `(null,"Disposable Income")`, style `STYLE_SEP`): `amounts[i] = monthly[i].ti + monthly[i].te`, total = sum. Added first.
2. `totalIncome` entry (`"Total Income"`) only if `stats.incomeVec` non-empty; `totalExpenses` (`"Total Expenses"`) only if `expenseVec` non-empty; `firstExpense = expenseVec[0]` (or null).
3. If `totalIncome != null`: add it (style `STYLE_SEP`).
4. For every `Amount a` in `incomeVec` followed by `expenseVec`: when `a == firstExpense`, add `totalExpenses` (style `STYLE_SEP`) before it. Create entry `e(key = AmountKey(a))`:
   * category (`a.is_aggregate()`): `amounts` = `M` nulls (filled in step 5).
   * leaf: for every month: `v = flat[i].get(key)`; if missing and `key.category == "(Uncategorized-Income)"` retry with `("(Uncategorized-Expenses)", desc)` and vice-versa; `amount = v == null ? 0 : v.amount`; append; `total += amount`.
5. Category entries (not the three specials): `total = 0`; for each month `i`: sum `amounts[i]` of the entries that follow it in the list up to (excluding) the next aggregate entry; `amounts[i] = sum`, `total += sum`.
6. `Total Income`: for each month: sum of `amounts[i]` of **aggregate** entries located after it and before `totalExpenses` (or list end); appended to its `amounts`, added to `total`. `Total Expenses`: same from its position to the end.
7. Remove leaf entries whose amounts are all `0.0`.
8. Remove category entries whose amounts are all `0.0` **and** which are last in the list or directly followed by another aggregate entry (i.e. have no remaining leaves).
9. Consistency checks (throw `RuntimeException`): for every `Amount` in `stats.get_flat_map()`: an entry with its key must exist unless `amount == 0` (`"missing '%s' in the resultant table"`); `ceil(entry.total) == ceil(amount)` (`"'%s' amounts do NOT match: base_stats=%.2f, mbm=%.2f"`); `ceil(stats.total_income) == ceil(totalIncome.total)` (`"base stats income doesn't match! got=%.2f, expected=%.2f"`), same for expenses.

### 7.2 `generate_month_by_month_report(path, stats)` (`stat_rep_row_color` reset; head via `writeRaw`)
```
<HTML>
<HEAD>
<TITLE>Budget Month-by-Month Comparison Report</TITLE>
</HEAD>
<BODY bgcolor=ffffff>
<style type="text/css">
BODY { font: 10px Arial, Helvetica, sans-serif; }
P, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }
table tr:hover td { background: #ffffc5; cursor: pointer; }
</style>
<TABLE BORDER=0 CELLSPACING=2>
<CAPTION>Month-By-Month report <B>{range_string}</B></CAPTION>
<TR bgcolor=ddDDdd><TD><B>Description</B>
   for i in 0..M-1: <TD ALIGN=CENTER><B>{(start+i months).toString("MMM, yyyy")}<br>amount</B>
<TD ALIGN=CENTER><B>Total<br>amount</B><TD ALIGN=CENTER><B>Avg/month<br>amount</B>
```
Rows (row flag starts true and is flipped **before** each row, so row 1 = `ffFFff`, row 2 = `eeEEee`, ...):
```
desc = escape_html_text(getDesc(key.description))
if desc starts with "   ":  desc = nbsp4 + desc ; bold = "" ; spanStyle = " style='font-size: 10px'"
else:                       bold = "<B>"       ; spanStyle = ""           (the <B> is never closed)
writeRaw("\n")
<TR bgcolor={ffFFff|eeEEee}><TD{entry.style}>{bold}{desc}
for each month amount a (prev starts 0, first = true):
    tip = first ? "base amount"
        : String.format("%+,.0f", a - prev) + (prev != 0 ? String.format(" (%+,.0f%%)", (a/prev - 1)*100) : "")
    <TD align=right{entry.style}><span title='{tip}'{spanStyle}>{bold}{f2html_no_zero(a)}</span>
    prev = a; first = false
<TD align=right{html_merge_styles(entry.style, "border-left: 1px solid black; border-right: 1px solid black;")}><span{spanStyle}>{nbsp3}{bold}{f2html_no_zero(total)}</span>
<TD align=right{entry.style}><span style='font-size: 11px'>{f2html_no_zero(total / M)}</span>
```
End: `</TABLE></HTML>`. Values below 1.00 in absolute value render as empty cells (`f2html_no_zero`); amounts honour `cfg.showDecimalsInCV`.

---

## 8. Monthly Detailed Report / Monthly Excel Report (`generateReport(start, end, path, mode)`)

Iterates months: `m = start` with day 1; after each month `m += 1 month; continue while m < end`. Since both the From/To label data and `curCal` are day-1 calendars, the **end month is excluded** unless `start == end` (e.g. From Jan to Mar produces Jan and Feb). This is the observed logic of the pseudo code (`if (v11.lt(p1)) goto L120`); `saveHTML` (section 10) by contrast includes the end month.

Setup:
```
isExcel  = mode == GEN_EXCEL
numAttr  = ' LOTUS_FORMAT="Fixed;2" ' + (isExcel ? 'STYLE="vnd.ms-excel.numberformat:###0.00" ' : '')
fmt      = isExcel ? DecimalFormat(2 digits, grouping OFF) : Frac2      // Excel numbers are written like 1234.56
firstDataRow = 3 ; row = 0 ; (row = next Excel row number; header=1, opening balance=2, first transaction=3; CAPTION is not counted)
write "<HTML>\n<HEAD>\n<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n"
```
Amount rendering helper used below: `num(x)` = `isExcel ? fmt(x) : f2html(x)`; `bal(x)` = `isExcel ? (x < 0 ? "<FONT COLOR=ff0000>" : "") + fmt(x) : f2html(x)` (the Excel red font is never closed).

Per month:
```
balance = closing_balance of db.getDay(day before the 1st) or 0.0 if that day is null ; income = expenses = 0
first = 1st of month ; last = last day of month
openFormula = row == 0 ? "" : ' FORMULA="=G' + (row-1) + '"'
if !isExcel || row == 0:
    <TABLE BORDER=0 CELLSPACING=2><CAPTION>Budget from <B>{sdate(first)}</B> to <B>{sdate(last)}</B></CAPTION>\n
else:
    <TR><TR><TR><TD COLSPAN=7><BR><CENTER>Budget from <B>{sdate(first)}</B> to <B>{sdate(last)}</B></CENTER></TR>\n   ; row += 2
<TR bgcolor=EEEEEE><TD><B>Date</B><TD><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2><B>Amount</B><TD><B>Balance</B>\n
<TR bgcolor=ffffff><TD ALIGN=CENTER><TD><TD ALIGN=LEFT><TD ALIGN=LEFT><B>Opening balance{&nbsp; x30}</B>\n<TD ALIGN=RIGHT><TD ALIGN=RIGHT><TD ALIGN=RIGHT{numAttr}{openFormula}><B>{isExcel ? fmt(balance) : f2html(balance)}</B></TR>\n
row += isExcel ? 3 : 4
shade = true          // row colour toggle: "eeeeee" when true, "ffffff" when false; toggled after every written row
```
Columns are: A Date, B image, C Category, D Payee, E positive amount, F negative amount, G balance.

For every day `d` from `first` to `last` (`showDate = true` at the start of each day; set false after the first row written for that day):

**Day-note row** (if `db.getDayNote(d) != null`):
```
note    = get_normalized_note(note, mode, leadingBR=false)
formula = ' FORMULA="=G{row-1}+E{row}+F{row}"'
C = 'BGCOLOR="#eeeeee"' or '#ffffff'
img = '<IMG SRC="' + embeddedSRC("file://icons/note.png", isExcel) + '">'
<TR {C}><TD ALIGN=CENTER {C}>{showDate ? sdate(d) : ""}<TD WIDTH=18>{img}<TD ALIGN=LEFT {C}><TD ALIGN=LEFT {C}>{note}<TD ALIGN=RIGHT {C}{numAttr}><TD ALIGN=RIGHT {C}{numAttr}><TD ALIGN=RIGHT {C}{numAttr} {formula}>{bal(balance)}</TR>\n
row++ ; toggle shade ; showDate = false
```

**Transaction rows** (for every `Transaction t` of the day in `BudgetDay` order: enabled before disabled; among income descending by amount; among expenses most negative first):
```
pmt = t.pmt
if !pmt.disabled: balance += t.amount
img = pmt.image_path non-empty ? '<IMG SRC="' + embeddedSRC("file://images/" + image_path, isExcel) + '">' : ""
formula = ' FORMULA="=G{row-1}' + (pmt.disabled ? "" : '+E{row}+F{row}') + '"'
C = BGCOLOR by shade
note = get_normalized_note(pmt.getNote(dayInt), mode, leadingBR=true)
flagIcon = flag == PAID ? '<font color=green>&#x2714</font> ' : flag == IMPORTANT ? '<font color=red>&#x26A0</font> ' : ''
amt(x) = pmt.disabled ? amount2html(x, null, ["gray","gray"], true) : num(x)
posCell = t.amount >= 0 ? amt(t.amount) : ""      negCell = t.amount < 0 ? amt(t.amount) : ""
balCell = (!isExcel && pmt.disabled) ? "<TD>" : '<TD ALIGN=RIGHT ' + C + numAttr + ' ' + formula + '>' + bal(balance)
<TR {C}><TD ALIGN=CENTER {C}>{showDate ? sdate(d) : ""}<TD WIDTH=18>{img}<TD ALIGN=LEFT {C}>{html_string(pmt, category or "")}<TD ALIGN=LEFT {C}>{flagIcon}{html_string(pmt, pmt.description)}{note}<TD ALIGN=RIGHT {C}{numAttr}>{posCell}<TD ALIGN=RIGHT {C}{numAttr}>{negCell}{balCell}</TR>\n
row++ ; toggle shade
if !pmt.disabled: (t.amount < 0 ? expenses : income) += t.amount
showDate = false
```
Category and description are written raw (not `getDesc`, not escaped). Disabled transactions are listed (gray) but do not affect balance/totals; in HTML their balance cell is empty.

**Balance-adjustment row** (after the transactions, for every day, if `balance != day.closing_balance`):
```
diff = day.closing_balance - balance ; balance = day.closing_balance
formula = ' FORMULA="=G{row-1}+E{row}+F{row}"'
<TR {C}><TD ALIGN=CENTER {C}>{showDate ? sdate(d) : ""}<TD WIDTH=18><TD><TD ALIGN=RIGHT {C}><I>Balance adjustment</I><TD ALIGN=RIGHT {C}{numAttr}><I>{diff >= 0 ? num(diff) : ""}</I><TD ALIGN=RIGHT {C}{numAttr}><I>{diff < 0 ? num(diff) : ""}</I><TD ALIGN=RIGHT {C}{numAttr} {formula}><I>{bal(balance)}</I></TR>\n
row++ ; toggle shade ; (diff < 0 ? expenses : income) += diff
```
(`db.getDay(d)` is dereferenced unconditionally - a null day would throw; within the loaded range days always exist.)

**Month footer**:
```
<TR bgcolor={shade ? eeeeee : ffffff}><TD><TD COLSPAN=3 ALIGN=RIGHT><B>Totals:</B><TD ALIGN=RIGHT {numAttr} FORMULA="=@sum(E{firstDataRow}..E{row-1})"><B>{num(income)}</B><TD ALIGN=RIGHT {numAttr} FORMULA="=@sum(F{firstDataRow}..F{row-1})"><B>{isExcel ? fmt(expenses) : f2html(income)}</B><TD ALIGN=RIGHT {numAttr} FORMULA="=E{row}+F{row}"><B>{num(income + expenses)}</B></TR>\n
<TR bgcolor={!shade ? eeeeee : ffffff}><TD><TD><TD COLSPAN=2 ALIGN=RIGHT><B>Closing balance:</B><TD ALIGN=RIGHT><B></B><TD ALIGN=RIGHT><B></B><TD ALIGN=RIGHT {numAttr} FORMULA="=G{row-1}"><B>{bal(balance)}</B></TR>\n
if !isExcel: </TABLE>\n<P><BR></P>
row += 2 ; firstDataRow = row + 4
```
Quirks reproduced from the code (both (verified against regenerated pseudo code)): in HTML mode the **Expenses total cell prints the income total** (`f2html(v15)` instead of `v17`); for the second month onward `firstDataRow` points at the opening-balance row (one row before the first transaction: `firstDataRow = row + 4` after the footer, while the next month adds 2 caption rows + 3 header rows before its first data row) - harmless for the sum since E/F of that row are empty.
End of document: `</BODY>\n</HTML>\n`.

---

## 9. Unpaid & paid report (`generatePaidUnpaidReport(start, end, path)`)

Collect, day by day over `[start, end]`, every non-disabled transaction: flag `';'` -> `paid`; otherwise -> `unpaid`, and if `'#'` additionally -> `important`. `TranSum` per list: `income` = sum of amounts >= 0, `expenses` = sum of amounts < 0, `amount = income + expenses`. Colours `["black","red"]`, all amounts via `amount2html(x, null, colors, true)` (2 decimals).

```
<HTML>
<HEAD><TITLE>Budget Report</TITLE></HEAD><BODY bgcolor=ffffff>
<TABLE BORDER=0 CELLSPACING=2><CAPTION>Paid/Unpaid Report from <B>{sdate(start)}</B> to <B>{sdate(end)}</B></CAPTION>
<TR bgcolor=EEEEEE><TD><B>Date</B><TD><!--image--><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2 ALIGN=CENTER><B>Amount</B></TR>
<TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** PAID ***</FONT></TR>
   if paid empty:  <TR bgcolor=ffffff><TD><TD><TD COLSPAN=4><FONT COLOR=gray>No entries!</TR><TR></TR>
   else: rows... then
         <TR><TD COLSPAN=4 ALIGN=RIGHT>PAID SUBTOTALS:<TD ALIGN=RIGHT>{income}<TD ALIGN=RIGHT>{expenses}<TD ALIGN=RIGHT>{amount}</TR><TR></TR>
<TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** UNPAID ***</FONT></TR>
   if unpaid empty: same "No entries!" row
   else: (lastDate reset to 0) rows... then
         <TR><TD COLSPAN=4 ALIGN=RIGHT>UNPAID SUBTOTALS:<TD ALIGN=RIGHT>{income}<TD ALIGN=RIGHT>{expenses}<TD ALIGN=RIGHT>{amount}</TR><TR></TR>
         if important non-empty:
         <TR><TD COLSPAN=4 ALIGN=RIGHT><font color=red>&#x26A0</font> IMPORTANT SUBTOTALS:<TD ALIGN=RIGHT>{income}<TD ALIGN=RIGHT>{expenses}<TD ALIGN=RIGHT>{amount}</TR><TR></TR>
</BODY>
</HTML>
```
(`bgcolor=#FCF#CF` is literally that; no `</TABLE>`.) Row (lambda `generatePaidUnpaidReport$0`; shade flag starts false -> first row `ffffff`, then alternating; the shade flag is NOT reset between sections, only `lastDate` is):
```
C = 'BGCOLOR="#ffffff"' / '"#eeeeee"'
<TR {C}><TD ALIGN=CENTER {C}>{date != lastDate ? sdate(date) : ""}<TD WIDTH=18>{img or ""}<TD ALIGN=LEFT {C}>{category or ""}<TD ALIGN=LEFT {C}>{flagIcon}{escape_html_text(getDesc(description))}{get_normalized_note(pmt.getNote(date), GEN_HTML, true)}<TD ALIGN=RIGHT>{amount >= 0 ? amt : ""}<TD ALIGN=RIGHT>{amount < 0 ? amt : ""}</TR>\n
toggle shade ; lastDate = date
```
`img` = `<IMG SRC="data:;base64,...">` of `images/<image_path>` when set (always embedded, HTML mode). An `IOException` inside the row writer sets a failure flag; after closing the writer the method throws `IOException("failed to write into " + path)`. Returns `true` otherwise.

---

## 10. Printed monthly calendar (`saveHTML(path, start, end)`) - "Print Monthly Calendar", "Print this month", CLI `-r`

Plain `BufferedWriter` (not unicodish); descriptions are converted with `str2unicodish` explicitly. `fmt = showDecimalsInCV ? Frac2 : Frac0` (used only for the totals line). `disabledColors = ["#C0C0C0","#FF8080"]`, `normal = ["black","red"]`.

Head (the `$HTML_STYLE` constant):
```
<HTML>
<HEAD>
<TITLE>Budget Report</TITLE>
<style type='text/css'>
table { font-size:12px; border-width:0px; border-collapse: collapse; }
th { white-space: nowrap; font-size:13px; font-family: Helvetica; background-color:#f0f0f3; border-width: 1px;padding: 8px; border-style: solid; border-color: #729ea5; text-align:center; }
td.day { white-space: nowrap; text-align: left; font-size: 17px; font-family: Terminus,Consolas,Profont,'Andale Mono'; font-weight: bold; border-width: 1px; border-bottom: 0px; padding: 8px; border-style: solid; border-color: #729ea5; }
td.bal { white-space: nowrap; text-align: right; font-size: 13px; font-family: Terminus,Consolas,Profont,'Andale Mono'; font-weight: bold; font-style:oblique; border-width: 1px; border-top: 0px; padding: 8px;border-style: solid; border-color: #729ea5;}
td.dsc { white-space: nowrap; font-size:12px; font-family: Helvetica; border-width: 0px; text-align: left; padding-top: 5px; }
td.amt { white-space: nowrap; font-size:12px; font-family: Terminus,Consolas,Profont,'Andale Mono'; border-width: 0px; text-align: right; padding-top: 5px; }
@media print{ .noprint{  display:none; }</style>
</HEAD>
<BODY bgcolor=ffffff>
<input class='noprint' type='checkbox' onchange="var v = (this.checked ? 'hidden' : 'visible'); [].forEach.call(document.querySelectorAll('.bamt'), function (el){el.style.visibility = v;});"><span class='noprint'>hide running balance</span></input>
```
(note the unclosed `@media print{` brace, copied verbatim).

Months: `m = start` (day 1); after each month `m += 1`; continue while `m < end || m == end` (end month **included**; `eq` compares the full date, so `end` must also be a day-1 calendar for the last month to be printed - it is, for both the label data and `curCal`).

Per month (`firstDOW` = 0 Sunday if `cfg.firstDayOfWeekIsSunday` else 1 Monday; `lastDOW = firstDOW + 6`; `dow(c)` = `c.get(DAY_OF_WEEK)` with Sunday mapped to 7 when the week starts on Monday; weekday names `wDayName[(firstDOW + i) % 7]`):
```
<P ALIGN=CENTER><FONT size=+2><B>{sdate_MY(m)}</B></FONT></P>
<TABLE BORDER=1 CELLSPACING=2 WIDTH="100%" >
<TR>  then 7 x  <TH>{weekday name}
d = day 1 moved back until dow(d) == firstDOW
e = last day of month moved forward until dow(e) == lastDOW, then +1 (exclusive)
income = expenses = 0
for d < e:
    if dow(d) == firstDOW: <TR>
    if d not in this month:  <TD class='day' WIDTH="14%">&nbsp;
    else:
        <TD class='day' VALIGN=TOP WIDTH="14%">{dayOfMonth}<BR>\n
        day = db.getDay(d); if day.adjustment_diff < 0: expenses -= adjustment_diff else income += adjustment_diff
        <TABLE BORDER=0 CELLSPACING=1 WIDTH="100%">\n
        for each transaction t (BudgetDay order):
            if !pmt.disabled: (t.amount < 0 ? expenses -= t.amount : income += t.amount)
            img = image_path ? '<IMG WIDTH="12" HEIGHT="12" SRC="' + embedded images/<path> + '">' : ""
            sc  = get_html_span_color(t, d)
            <TR><TD class='dsc'>{img}<SPAN style='background-color: {sc.background};'><FONT COLOR={sc.foreground}>{unicodish(getDesc(description))}</FONT></SPAN><TD class='amt'>{amount2html(t.amount, "tamt", pmt.disabled ? disabledColors : normal)}{t.amount != pmt.amount ? "'" : "&nbsp;"}</TR>\n
        </TABLE>\n
    if dow(d) == lastDOW:
        </TR><TR>\n ; go back 6 days; for 7 days:
            not in month: <TD class='bal' WIDTH="14%">&nbsp;
            else:         <TD class='bal'>{amount2html(day.getClosingBalance(showDecimalsInCV), "bamt", normal)}{day.closing_balance_adjusted ? "'" : "&nbsp;"}\n
        back 1 day ; </TR>\n
    d += 1 day
</TR>\n</TABLE>\n<P class='bal' ALIGN=CENTER><B>Totals:</B> Income = <B>{fmt(income)}</B>, Expenses = <B>{fmt(expenses)}</B> ({fmt(income - expenses)})</P>\n<BR><BR>
```
End: `</BODY>\n</HTML>\n`. So every calendar week is two table rows: the day cells (number + inner table of transactions) and a `td.bal` row with the running closing balance of each day (the checkbox hides the `.bamt` spans). An apostrophe after an amount marks a manually changed transaction amount (`t.amount != pmt.amount`); an apostrophe after a balance marks an adjusted closing balance. `expenses` is accumulated as a positive magnitude. `getClosingBalance(false)` = `(int)(bal +/- 0.05)` (rounded away from zero to whole units). Disabled transactions are printed in gray/`#FF8080` but excluded from the totals.

`get_html_span_color(t, d)`:
* `cfg.preserveColorOnPrint == false`: background `"white"`, foreground `"#C0C0C0"` if disabled else `"black"`.
* else: fg = `#c0c0c0` if disabled else black; bg = white, or `cfg.bgColorPaid` when the day flag is PAID, or `cfg.bgColorImportant` when IMPORTANT; then the payment's hilighting colour for that date (`getHilighting(date)`, falling back to `getDefaultHilighting()`, `-1` = none) replaces bg, and fg becomes white if that colour is dark. Both returned as `"#%06x"`.

---

## 11. Summary of inclusion rules

| Report | Range | Disabled payments | Deleted transactions | Balance adjustments |
|---|---|---|---|---|
| Stats table / Print / Excel / Comparative / Month-by-Month | `showStatsStats` (months, or exact days for Last 30/365) | excluded | excluded (not in day lists) | included as `(Unaccounted)` leaf under the sign-matching Uncategorized category |
| Monthly Detailed / Excel | From label month .. To label month **exclusive** (single month if equal) | listed gray, not summed | excluded | `Balance adjustment` rows, summed into income/expenses |
| Unpaid & paid | `showStatsStats` start..end by day | excluded | excluded | not shown |
| Printed calendar | From .. To months inclusive | listed gray, not summed | excluded | `adjustment_diff` added to the month totals; adjusted balances marked with `'` |
| Planned amounts (`+/-` columns) | `day.getRegulars()` of the same range | excluded | n/a | n/a |

---

## 12. UNKNOWN / unverified points

* `generateReport`: the end-month exclusion relies on the From/To label data and `curCal` being day-1 calendars. Label data always is (built from `minDate` + N months, or `Stats.startCal`); `curCal` is set to day 1 in `openBudgetFile`, but whether every navigation path keeps `curCal` at day 1 was not verified (outside this scope). If `curCal` had a later day, File > Excel/Detailed report would still produce exactly one month.
* `fillStatsTransactionMenu` / `Payment.getEndMode() == 16`: the meaning of end-mode 16 (used to omit the end date in `"Edit payment [...]"`) was not looked up (Payment class, other scope).
* `saveHTML` `wDayName` contents (weekday name strings) and `cfg.preserveColorOnPrint` default were not read (constructor/config scope).
* Excel `FORMULA=` / `LOTUS_FORMAT=` attributes are written verbatim; whether current Excel versions still honour them is not determinable from the code.

Verified pass: 2026-10-27 — 0 statements corrected, 2 confirmed.
