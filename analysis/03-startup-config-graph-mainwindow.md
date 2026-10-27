# Budget Calendar 1.11 — startup, configuration, registration, graph, main window

Source: `reference/pseudo/ca.mishell.app.budget.Budget.part03.txt` plus the listener classes
`Budget$1..$33`, `Budget$24$1..4`, `Budget$Bump`, `Budget$BestFit` (part01/part02), the Budget
class header/constants (end of part02), the static initializer (end of part06), `BudgetConfig`
and `Config` (rc file mechanics), `isValidKey/isBadKey` (part05), `askRegistration` (part04).

Conventions used below: `SimpleCalendar.get()` returns an int `yyyymmdd`; month is 0-based
(Jan = 0); `get()/100` is `yyyymm`; `set(1, 1)` = set day-of-month to 1; `add(2, n)` = add n
months; `add(1, n)` = add n days; `set(2, 11)` = set month to December.

---

## 1. Constants and static state

| Name | Value |
|---|---|
| `budgetVersion` | `"1.11" + revision`, `revision = "." + svnRevision.revision` (`"??"` if the class is missing) |
| `BUDGETRC` | `".budgetrc"` |
| `MAX_N_MONTHS` | 180 |
| `MAX_RECENT_FILES` | 5 |
| `MARGIN_WIDTH` | 8 |
| `BGT_FILE_EXTENSION` | `"*.bgt"` |
| `BUDGET_FILES_DIR` / `BUDGET_DATA_SUBDIR` | `"BudgetFiles"` / `"data"` |
| `LICENSE_TXT` | `"license.txt"` (prefixed with jar dir, see §2.2) |
| `icons_loc` / `images_loc` | `"icons/"` / `"images/"` (prefixed with jar dir) |
| `GRAPH_OPEN_BAL..GRAPH_MAX_BAL` | 0 open, 1 close, 2 avg, 3 income, 4 expenses, 5 diff, 6 min, 7 max |
| `GRAPH_PRESET_UNSET` | 0 |
| `stepList` | `{100, 500, 1000, 2000, 5000, 10000, 50000, 100000, 1000000, 10000000}` |
| `BALANCE_NONE/ALL/FIRST_AND_LAST` | 0 / 1 / 2 |
| `PROMPT_PASSWORD/EXPENSE/REMOVE_ADJUSTMENT` | 1 / 2 / 4 (bit flags for `prompt()`) |
| `mb_dnd_prefix` | `"MiShell*Budget*TID:"` |
| `FAKE_AMPERSAND` | U+FF06 (filter text `&` is replaced with it) |
| `forecastType[]` | `"Opening Balance","Closing Balance","Average Balance","Total Income","Total Expenses","Disposable Income","Minimum Balance","Maximum Balance"` (indexed by graph type) |
| `monthName[]` | `Jan..Dec` (3-letter, English, used by the graph x-axis) |
| `arr[]` | `"1st","2nd","3rd","4th","5th"` |
| `wDayName[]` | initially `Sunday..Saturday`; overwritten in `createShellContents` with locale names (`"EEEEEEEEEEEEEE"` format) for 2010-10-24 (a Sunday) + 0..6, so index 0 = Sunday |
| `hilight_color[]` | 0xFFD700 Gold, 0xFA8072 Salmon, 0xFFC0CB Pink, 0xFF7F50 Coral, 0xBDB76B Dark Khaki, 0xEE82EE Violet, 0x00FF00 Lime, 0x9ACD32 Yellow Green, 0x8FBC8F "Dark See Green", 0x87CEEB Sky Blue, 0xFFE4E1 Misty Rose |
| `SUM_QUESTION` | `"The selected day already has this type of transaction.\nDo you want to sum the amounts?\n"` |
| `AMOUNT_SEP` | `": "` |
| `indent_string` | 3 spaces |
| `normalColors` | `{"black","red"}` |
| `numFormatFrac0/1/2` | locale number format, grouping on, exactly 0 / 1 / 2 fraction digits |
| `decimalSeparator` | locale decimal separator |
| `runningOnMac` | `os.name` starts with "Mac"; `runningOnLinux` contains "Linux"; `runningOnWindows` = `File.separatorChar == '\\'` |
| `dirHome` | `user.home` (or "."); `dirCurrent` = `user.dir`; `dirData` = "." |
| `is_app_store_version` | false |

Instance defaults set in `<init>`: `use_dnd=true`, `trial_version=true`, `graph_bar_width=25`,
`graph_bar_height=110`, `graph_data = 12 × graphData`, `graphHotSpots = Rectangle[2]`,
`opt_show_balance = BALANCE_ALL`, `stat_line_color=true`, `calendarDescriptionMaxLen=-1`,
`recentFilesMenuIndex=-1`, `minusZeroDotZero = -0.0`, `initCal = new SimpleCalendar()` (today).

`filterToolTipText` (tooltip of both Filter boxes), verbatim:

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

## 2. `main` and resource location

### 2.1 `main(String[] args)`
1. If `args[0]` equalsIgnoreCase `--version`: print `budgetVersion` to stdout, `exit(0)`.
2. `Display.setAppName("Budget")`, create Display, `new Budget()`, `updateResourceLocation`, `open(display, args)`, dispose display.
3. Any exception from `open`: if `opt_debug` (`-D`) rethrow. Otherwise write
   `<dirHome>/budget-crash.log` containing
   `"cwd=<abs cwd>\nError message:<msg>\nStack Trace:\n<trace>r<revision>\n"`, dispose the
   display, create a fresh Display+Shell and show `showCrashError` (MessageBox ERROR, title
   `"Error -- Budget Calendar Crashed."`) with text
   `"r<revision>\nError message:<msg>\ncwd=<cwd>\nStack Trace:\n<trace>\nCrash log file is '<path>'\n\n"`.

### 2.2 `updateResourceLocation`
`jar_loc` = parent directory of the code source (jar). If the path contains `idea-projects`,
`jar_loc` = 4 directories up + `/Budget` (dev mode). Then `LICENSE_TXT = jar_loc/license.txt`,
`icons_loc = jar_loc/icons/`, `images_loc = jar_loc/images/`. If parent is null: `jar_loc = "./"`
(and the three paths stay relative). `is_jpackage = jar_loc.endsWith("app")`.
`icon(name)` = `icons_loc + name`; `image(name)` = `images_loc + name`.

### 2.3 Single-instance guard `isAppAlreadyRunning(port)`
Opens `ServerSocket(port, backlog 3, InetAddress.getLocalHost())`, soTimeout 0, and a daemon
thread `"singleInstanceGuard"` that accepts and immediately closes connections forever.
`BindException` → returns true (already running); other IOException → stack trace, false.

---

## 3. `open(display, args)` — startup sequence (in order)

1. `excel_prg = Program.findProgram(".xls")`, falling back to `".xlsx"` (used for Excel export).
2. **Budget-stick mode**: relative dir `../../..` (Mac), `../..` (Linux), `..\..` (Windows) +
   `/BudgetFiles/data`. If it exists and is a directory: `budgetStickMode=true`,
   `dirHome = dirCurrent` (cwd), `dirData` = its canonical (or absolute) path.
3. `rcFileName = dirHome + File.separator + ".budgetrc"`. `mainShell = new Shell()`.
4. Mac only: drop any argument that starts with `-p` but is not exactly `-p` (the `-psn_…` arg).
5. Options via `CLIOpts("Budget", args).getopt("prn:dc:ihbDzBZ:UC:")`:

   | opt | effect |
   |---|---|
   | `-r` | `opt_print_current_month` (print current month as HTML and exit, see step 22) |
   | `-d` | `use_dnd = false` |
   | `-D` | `opt_debug` (no crash log; exceptions propagate) |
   | `-z` | `dont_show_zero_amount` |
   | `-B` | `do_bill_reminder_check` |
   | `-Z<n>` | `opt_show_balance = parseInt(optarg)` (leading digits only, else 0), `opt_show_balance_supplied=true` |
   | `-c <rcfile>` | `rcFileName = optarg` |
   | `-C year,name` | cut-off mode; `ASSERT` exactly 2 comma parts (`"bad command line for '-C year,name'"`), year = int, name = output file |
   | `-U` | `opt_show_url` |
   | `-i` | `showInfo(revision + "\n")` and return (exit) |
   | `-h`, `:`, `?` | `showInfo` with the usage text below and return (exit) |
   | `-p`, `-n`, `-b` | declared in the getopt string but not in the switch → `showError("Declared but not handled option '<opt>'")`, `exit(1)` |

   Usage text (verbatim):
   ```
   Command line options:
   -r	print the current budget month and exit;
   -d	disable dnd operations
   -D	debug mode (no budget-crash.log) file
   -z	do not show '0' transaction amounts
   -B	check bill remainders
   -Z{0|1|2}
   	show balance option: 0 - none, 1 - all, 2 - first&last.
   -c rcfile
   	use <rcfile> instead of .budgetrc
   EXPERIMENTAL:
   -C year,name
   	* process the current data file;
   	* remove all entries prior the year;
   	* save into the file 'name'.
   ```
   First non-option argument (if not blank) = budget file name.
6. `loadRc(rcFileName)` (§4). `white_color` = system WHITE, `black_color` = system BLACK.
7. If `cfg.singleInstance` and `isAppAlreadyRunning(cfg.port)`: `showError("Budget Calendar is already running.")`, `exit(1)`.
8. If `!cfg.licAgree`: show `licAgree()` dialog (reads `LICENSE_TXT`, shell title
   `"Budget Calendar License Agreement"`; errors `"Unable to load license"` / `"Unable to display license"`). Declined → `exit(10)`; accepted → `cfg.licAgree = true`.
9. Mac and `cfg.firstRun`: yes/no `"Are you using ONE BUTTON mouse?\nYou can always change it later -- File->preferences->One button mouse.\n"` → `oneButtonMouse = true` on Yes.
10. If `is_jpackage && cfg.firstRunJPACKAGE && runningOnWindows`: `msiSuggestUninstall(true)` (§7).
11. `noImage = icons/no-image.bmp`; `bestFit = new BestFit(display)`.
12. If `is_trial_version()`: `cfg.unlimitedHistory.boolValue = false; .enabled = false`.
13. Date range: `minDate` = today, day 1, minus 1 month; `curCal` = `initCal` (today) day 1;
    `maxDate` = `curCal + (futureNMonths − 1) months`, then month set to December.
14. `loadQuickPickTags()` (§8).
15. File name from the command line overrides `cfg.lastFileName`.
16. If a file name is set and `BudgetDB.isEncrypted(file)`: `askPasswordIfEncrypted` — loop:
    `prompt(null, budgetIcon, "Password", "The budget file is password protected\nEnter the password:", "", PROMPT_PASSWORD)`;
    cancel → `fileName = null`; empty → `showError("Empty password!")` and re-prompt;
    `!BudgetDB.isDecryptable(file, pw)` → `showError("The password appears to be invalid!\n")` and re-prompt.
17. `db = BudgetDB.load(fileName, dbKey, errBuf, curCal, futureNMonths, unlimited)` where
    `unlimited = cfg.unlimitedHistory` forced to true when `-C` was given. On failure:
    stderr errBuf if `-D`; `showError("Unable to open '<file>' file\n" + ("(password may be incorrect)\n" if a key was used) + "An empty budget will be created.")`; `fileName = dbKey = null`.
18. If `fileName != null`: `addRecentFile(absolutePath)` — remove existing entry, insert at index 0, trim the Vector to 5.
19. If `-C`: `cut_off_history(fileName, db, year, name)` (never returns, §7).
20. If `db == null`: `BudgetDB.load(null, null, …, cfg.unlimitedHistory)` (empty budget); failure → `showError("Unable to initialize budget database.\n" + errBuf)`, `exit(1)`.
21. `calc_min_date()`; shell icon = `budgetIcon()`; `checkRegistration()` (§5); `updateShellTitle()`.
22. Window placement:
    - `initialRect != null && !initialMaximized`: `setBounds(initialRect)` then
      `fixShellLocation`: display bounds shrunk by 50 px on each side; if the window rect does
      not intersect it, location = `centerLoc(size)`.
    - else if `initialMaximized`: if `initialRect != null` and its (x,y) is inside display
      bounds → `setLocation(x,y)`; `setMaximized(true)`.
    - else `centerShell(mainShell)`.
23. `attachDisplayListeners()` (§11); `createShellContents()` (§10); if `cfg.startFolded` →
    `foldLabelListener.mouseDown(null)` (fold the details view).
24. `-r`: `getHtmlFileName()`; if non-null `saveHTML(name, curCal, curCal)` and
    `Program.launch(html_name(name))`; errors → `showError("Unable to save HTML file\n" + ex)`; return (no window).
25. `-B` bill reminder (only if `db.fileName != null`): `d` = date of the file's lastModified;
    `end` = today + 1 day; if `d < end`, for each day `d` until `d == end`: for every
    transaction on that day append `"<yyyymmdd>:  <description>  <amount 2dp>\n"`, counting;
    after the 16th line append `"...\n"` and stop that day. If no lines → return (no window).
    Else text = `"These payments are due:\n\n" + lines + "\nDo you want to run Budget Calendar now?\n"`; showYesNo; No → return.
26. `calendarView.layout(); mainShell.layout(); open(); forceActive(); forceFocus()`.
    If `initialMaximized && cfg.bestFit`: `bestFit.setSize()`; if `cell_width` changed → `updateCalendarView()`.
27. `show_current(calendarScroll)`; `display.addListener(SWT.Close, Budget$5)`;
    `mainShell.addShellListener(shellAdapter)`; `mainShell.addListener(SWT.Resize, Budget$6)`
    (sets `do_best_fit = true` if `cfg.bestFit`).
28. If `db.size()==0 && db.adjustmentsSize()==0` → `showHint()`.
29. `display.timerExec(20000, Budget$7)` — midnight check (§11.4).
30. Event loop: while shell not disposed: if `do_calendar_update` → clear, `updateCalendarView()`;
    if `do_rebuild` → `rebuildApp()`, clear; Mac: if shell not visible → print
    `"mainShell is no longer visible, exiting..."` and leave loop; if `!readAndDispatch()`:
    if `do_best_fit` → `updateCalendarView()` and continue; else if `flash_cell != null`:
    3 × { background = `bgColorCellHighlight`, update, sleep 200 ms, background = cell data
    `"bgcolor"`, update, sleep 200 ms } then `flash_cell = null`; `display.sleep()`.
31. After the loop: if `!shellAdapterInvoked` → `saveAndCleanup()`. Return shell.

### Shell close handling
`Budget$4.shellClosed` and `Budget$5` (display `SWT.Close`): if `shellAdapterInvoked` already
set, do nothing; else set it, call `saveAndCleanup()`; false → `event.doit = false` (veto).
Budget$5 additionally `exit(0)` on Mac after a successful cleanup.

### `saveAndCleanup()`
If `!cfg.autosave && fileName != null`: MessageBox YES|NO|CANCEL, title `"Save..."`,
`"Do you want to save your budget?"`: YES → continue to save; NO → `saveRc()`, return true;
CANCEL → `shellAdapterInvoked = false`, return false. Then `saveDB()`; false → return false;
`saveRc()`; return true.

### `updateShellTitle()`
`"<fileName or 'New budget'> - Budget Calendar" + (" (unregistered)" if trial)`.

---

## 4. `.budgetrc` (BudgetConfig / Config)

File: `<dirHome>/.budgetrc` (`BudgetConfig.RCFILE_NAME = user.home/.budgetrc`; overridden by
`-c`). Java `Properties` format, written with an empty comment. On save, the existing file is
copied to `.budgetrc.bak` first (failure → `"unable to backup '<rc>' to '<bak>'"`); on write
failure the backup is renamed back, on success it is deleted.

### 4.1 Key naming (Config.save / Config.load)
Every public, non-static, non-transient, non-final field of `BudgetConfig` is a key named
exactly as the field:

| Field type | Stored as | Parsed as |
|---|---|---|
| `String` | the string (omitted when null) | string |
| `boolean` | `true`/`false` | `"true"` or `"1"` = true |
| `int` / `double` | decimal | `Integer.parseInt` / `Double.parseDouble` |
| `Vector<String>` | `name.0`, `name.1`, … | index after the dot |
| `ConfGroup` | `true`/`false` = collapsed state | |
| `ConfBool` | `true`/`false` | `"1"` or `"true"` (case-insens., trimmed) |
| `ConfInteger` / `ConfDouble` | decimal | parse (errors printed, value kept) |
| `ConfString` | string | string |
| `ConfFont` | `FontData.toString()` (SWT font string) | `new FontData(s)` |
| `ConfColor` | `%06x` RRGGBB | hex int → r,g,b (bad hex → black) |
| `ConfAction` (`apply_theme`) | not saved | |

Keys in the ignore set are skipped on load: `graphPastBarColor`, `graphExtraMonthBarColor`,
`graphMinBarColor`, `graphAvgLineColor`, `detailsStatsOnDesc`. Unknown keys go to a
fallback: legacy `recentFile0..recentFile4` (no dot) are mapped into `recentFile`;
`firstDayOfWeek` = `"1"` sets `firstDayOfWeekIsSunday`; anything else prints `skipped: k=v`.

### 4.2 Keys and defaults

Plain fields: `regkey` (null), `lastFileName` (null), `licAgree` (false), `firstRun` (false —
note: default false, so the one-button-mouse question only fires if the rc file says
`firstRun=true`), `firstRunJPACKAGE` (true), `startFolded` (false), `windowMaximized` (false),
`statHeight/statWidth/statX/statY` (-1, Full Summary window geometry), `windowHeight/windowPosX/windowPosY/windowWidth` (-1),
`graphPresetMax` (0.0), `graphPresetMin` (0.0), `graphType` (2 = Average Balance),
`image_selection_zoom` (175), `recentFile.N` (empty).

Group `ui` ("Controls"):

| key | default | label |
|---|---|---|
| `firstDayOfWeekIsSunday` | true | First day of Week is SUNDAY, or MONDAY when unchecked. |
| `autosave` | true | Auto Save, uncheck if you don't want to save the data file automatically. |
| `oneButtonMouse` | false | One Button Mouse or a Touch Screen |
| `wheelSupport` | true | Wheel Support -- allows scrolling the months with the mouse wheel, just like PgUp/PgDn |
| `showDecimalsInCV` | false | Show Decimals in the Calendar View |
| `preserveColorOnPrint` | false | Preserve transactions' colors when printing monthly calendar |
| `bestFit` | true | Try to FIT payment description into the window to avoid scrollbars |
| `alwaysShowBalance` | true | Always Show Balance, or on ONLY the 1st/last day of a month; days with transactions; days with adjusted balance. |
| `useIndefiniteEndMode` | true | Use INDEFINITE End Mode by default when creating a new payment |
| `encryption` | false | Password support, NOTE: Weak Encryption! |
| `autoDateSwitch` | false | Automatically switch to the new day at midnight |
| `unlimitedHistory` | false | Unlimited history, show all the available months prior the current one (if any) |
| `futureNMonths` | 24 | Number of the future months. This is a rolling limit* (editor validation 12..120; `loadRc` clamps 12..180) |
| `maxSavingNMonths` | 2 | Number of months to consider when calculating maximum savings |
| `minBalance4MaxSaving` | 100.00 | Minimum balance for maximum savings calculations (2 decimals) |
| `graphNMonths` | 6 | Number of months in the bar graph* (-1 = up to 6 months without spilling into next year; valid -1 or 1..12) |
| `currencyPrefix` | `"$"` | Currency Prefix |
| `currencySuffix` | `""` | Currency Suffix |

Group `fnt` ("Fonts") — defaults from the system font height `h`; `h09 = (int)(h*0.9)`,
`h12 = (int)(h*1.2)`, `h15 = (int)(h*1.5)`; mono = Monaco (Mac) / Monospace (Linux) /
Consolas (Windows) / "Andale Mono":

| key | default |
|---|---|
| `fontTran` | Verdana h normal |
| `fontCurMonthLink` | Verdana h bold |
| `fontDate` | Verdana h12 bold |
| `fontAmount` | mono h italic |
| `fontPosTranAmount` / `fontNegTranAmount` | mono h normal |
| `fontImportantTran` / `fontPaidTran` | Verdana h normal |
| `fontGraphDayLabel` | Verdana h09 normal |
| `fontLabel` | system font name, h09 bold |
| `fontMonth` | Verdana h15 normal |

Group `clr` ("Colors"), light theme defaults (RGB) / dark theme (`apply_theme` action,
choices `LIGHT(default)` and `DARK`):

| key | light | dark |
|---|---|---|
| `bgColorBalance` | 229,240,255 | 237,166,166 |
| `bgColorMinBalance` | 174,208,255 | 249,197,186 |
| `bgColorBelowMinBalance` | 255,255,0 | 255,255,0 |
| `bgColorMaxBalance` | 220,204,226 | 132,168,226 |
| `bgColorCellOtherMonths` | 204,204,204 | 165,165,165 |
| `bgColorCell` ("BKG cell past") | 204,228,234 | 124,167,206 |
| `bgColorCellToday` | 253,253,244 | 255,246,147 |
| `bgColorCellPast` ("BKG cell future") | 244,244,235 | 166,204,235 |
| `colorPositiveAmount` | 11,7,127 | 0,51,101 |
| `colorNegativeAmount` | 255,76,94 | 255,72,68 |
| `colorPosTranAmount` | 0,0,0 | 0,0,0 |
| `colorNegTranAmount` | 138,0,0 | 255,5,13 |
| `colorNotes` | 188,143,143 | 223,223,223 |
| `bgColorNotes` | 255,255,224 | 32,32,32 |
| `bgColorHighlight` | 217,217,217 | 255,254,121 |
| `bgColorImportant` | 255,255,0 | 255,255,0 |
| `bgColorCellHighlight` | 217,179,255 | 250,253,202 |
| `bgColorPaid` | 202,222,184 | 255,127,130 |
| `bgColorWindow` | 255,255,255 | 83,82,83 |
| `bgColorDetails` | 255,255,255 | 0,0,0 |
| `colorPastPayment` | 255,76,94 | 255,76,94 |
| `colorCalendarToday` | 255,76,94 | 255,76,94 |
| `colorFuturePayment` | 76,178,36 | 76,178,36 |
| `colorCalendarDay` | 11,7,127 | 11,7,127 |
| `colorCurrentMonth` ("months and links") | 0,24,244 | 128,128,255 |
| `colorDescText` | 0,0,0 | 0,0,0 |
| `bgColorCalendarHeader` | 0,0,0 | 83,83,83 |
| `colorCalendarHeader` | 255,255,255 | 255,250,242 |
| `bgColorDatePickerHeader` | 0,0,0 | 0,0,0 |
| `colorDatePickerHeader` | 255,255,255 | 255,255,255 |
| `colorDisabledPayment` | 124,124,124 | 124,124,124 |

Group `grc` ("Forecast Graphics Colors"): `graphTextColor` 0,0,0 / 255,255,255;
`graphGridColor` 204,204,204 both; `graphBarColor` 54,132,201 / 54,98,161;
`graphNegativeBarColor` 255,76,94 / 251,0,26.

Group `adv` ("Advanced", created collapsed): `singleInstance` false, `port` 57778.

### 4.3 `loadRc(rcFileName)`
Default `initialRect = (0, top, 1200, 800 − top)` with `top = 40` on Mac else 0.
`cfg = new BudgetConfig(rcFileName)` (loads file). `fileName = cfg.lastFileName` trimmed, blank →
null. If `cfg.windowHeight != -1`: `initialRect = (windowPosX, windowPosY, windowWidth, windowHeight)`,
`initialMaximized = windowMaximized`. Then `futureNMonths = clamp(.., 12, 180)` and
`graphNMonths = valid_month_n(..)` (-1 kept; else clamped to 1..12).

### 4.4 `saveRc()`
`firstRun = false`; `firstRunJPACKAGE = !is_jpackage`; `lastFileName = fileName`;
window bounds → `windowPosX/Y/Width/Height`; `windowMaximized = shell.getMaximized()`;
`startFolded = (unfoldComp != null)`; `cfg.save()`; error string →
`showError("Unable to save config '<rc>'\n<error>")`.

---

## 5. Registration / trial

### 5.1 `is_trial_version()`
Key lookup order (first line of the file, if any):
1. `cfg.regkey` if non-blank.
2. `regkey.txt` in the current directory.
3. On failure, Windows only: `<dirHome>\AppData\Local\VirtualStore\Program Files\MiShellSoft\Budget\regkey.txt`.
4. If still nothing and `budgetStickMode`: `<dirData>/../regkey.txt`.

If a key was found and `isValidKey(key.trim())`: `cfg.regkey = key`, `unlimitedHistory = true`
and enabled; return **false** (registered). Otherwise return true (trial).

### 5.2 Key validation — `isValidKey(k)` (exact algorithm)
1. `k.length() != 12` → invalid.
2. `isBadKey(k)` → invalid, where `isBadKey`:
   - blacklist: the string `"Mg[5P;x5S7V7"` decoded as `out[i] = (char)(src[i] − 1 − (i % 4))`
     (= `"LeX1O9u1R5S3"`); equal → bad;
   - `!Character.isDigit(k[11])` → bad;
   - any of `k[0..10]` not `Character.isUpperCase` → bad.
   So a key is 11 uppercase letters followed by one digit.
3. Checksum `s = 0`; for `i` in 0..11 **skipping i == 4 and i == 8**: `s += (int)k[i]`; if
   `s > 101` then `s = (s % 101) + 1`. (The fold happens after every addition, in order.)
4. Valid iff `k[4] == (char)('M' + (s / 10) % 10)` **and** `k[8] == (char)('N' + s % 10)`.
   ('M' = 77, 'N' = 78.)

### 5.3 `askRegistration()` (Budget$3 / Help menu)
`prompt(mainShell, null, "Registration", "Enter registration key: ", "", 0)`; cancel → false.
Trim; invalid → `showError("The entered key is INVALID!")`, false. Valid → `trial_version=false`,
`updateShellTitle()`, `showInfo("Registration complete.\nThank you for using Budget Calendar")`,
`cfg.regkey = key`, `unlimitedHistory = true` & enabled, `saveRc()`, true.

### 5.4 `more_than_Ndays(N)`
True if the DB has an oldest adjustment date (`db.getOldestAdjustmentDate() != -1`) and that
date `<` (today − N days); otherwise, **Windows only** (`File.separator == "\\"`), true if
`icons_loc` directory `lastModified/1000 + N*24*3600 < now/1000`. Exceptions → false.

### 5.5 `checkRegistration()`
`trial_version = is_trial_version()`. If trial: `expired = more_than_Ndays(60)`; if
`more_than_Ndays(30)`: open a child Shell (style BORDER|TITLE, no close box), icon = budget
icon, title `"Budget Calendar"`, 1-column GridLayout, containing:
- Label `"This is a trial version of the Budget Calendar.\nTo register your product, click this button:\n"`
- Button `"https://www.mishell.ca/"` + (`" (waiting...100)"` unless expired). Click (Budget$2):
  once only, `Program.launch("https://www.mishell.ca/")`, and `exit(0)` if expired.
- Button `"Enter registration key"` → `askRegistration()`; success → `expired = false`, dispose window.

`centerShell`. Then a 20-second countdown driven by `readAndDispatch`: each wall-clock second,
if the counter is 0 the window is disposed; otherwise, if not expired, the counter decrements
and the first button's text becomes `"https://www.mishell.ca/ (waiting...<n>)"` (so the first
visible value is 19). When the window is gone: if still expired → `System.exit(0)`.
Net effect: 30–60 days → 20 s nag; > 60 days → must register or the app exits.

Trial also forces `unlimitedHistory` off and disabled (open step 12) and appends
`" (unregistered)"` to the window title.

---

## 6. Date range helpers

- `calc_min_date()`: only if `cfg.unlimitedHistory`: start at `initCal` day 1, step back one
  month while `db.getDay(cal) != null`, then step forward one → `minDate` = earliest month that
  has data. Otherwise `minDate` stays "previous month".
- `rebuildApp()`: `minDate` = today day 1 − 1 month; `curCal = initCal` day 1; `maxDate =
  curCal + (futureNMonths−1) months` with month := December; `db.rebuildDB(curCal, futureNMonths,
  unlimitedHistory)`; `calc_min_date()`; dispose all `mainShell` children; `unfoldComp =
  unfoldFilterText = unfoldMonthSelector = null`; `createShellContents()`; layouts;
  `show_current(calendarScroll)`.
- `get_graph_n_month()`: `graphNMonths == -1` ? `min(12 − curCal.getMonth(), 6)` (month 0-based,
  so December gives 1) : `graphNMonths`.
- `valid_month_n(n)`: -1 → -1; `<1` → 1; `>12` → 12. `clamp(v, lo, hi)` standard.

---

## 7. Misc startup helpers

- `msiSuggestUninstall(isJpackage)`: if `C:\Program Files (x86)\MiShellSoft\Budget\uninstall.exe`
  exists: message = (isJpackage ? `"You are on the LATEST version of Budget, the older version could not be removed automatically.\n\n"` : `"WARNING: Budget Calendar was updated to use Microsoft Windows Installer. Before installing, please uinstall the older version.\n\n"`) + `"Would you like to uninstall the older version now?\n\nIf not, you can later safely uninstall 'MiShell*Budget (Remove Only)' without affecting the current version."`; Yes → `exec("explorer.exe", "/select,<path>")`.
- `cut_off_history(file, db, year, outName)`: append `.bgt` if missing; `ASSERT(db != null, "no data file to work with!")`; `ASSERT(cfg.unlimitedHistory.enabled, "no registration!")`; `ASSERT(!exists(outName), "file '<out>' already exists!")`; stderr `"<file>: cutting-off on <year>"`; `ASSERT(db.cutOffHistoryPrior(year), "no data reduction!")`; stderr `"saving into '<out>':"`; `db.save(outName, null, err)` false → print err, `exit(1)`; `"done."`, `exit(0)`. (`ASSERT` prints the message to stderr and `exit(1)`.)
- `saveNewBudget()`: `confirmYesNoCancel("Do you want to save the new budget ?")`: NO(128) → true, CANCEL(256) → false, YES → SAVE FileDialog, title `"Save new budget..."`, filter `*.bgt`; append `.bgt` if the name does not contain `.bgt`; if the file exists ask `"The '<f>' file already exists.\nDo you want to overwrite it ?"` (No → return false); `db.save(name, dbKey, err)` false → showError(err), false; else `fileName = name`, true.
- `getMin/getMax/getAvg(double[])`: plain min/max/mean; empty array → 0.0.
- `isDarkColor(c)`: `r < 10 && g < 10` → dark; else HSB brightness `< 0.7` → dark.
- Dialog helpers: `showError` (MessageBox ERROR, title `"Error"`), `showInfo` (INFORMATION,
  `"Information"`), `showYesNo` (QUESTION|YES|NO (194), title `"Information"`, returns `open()==YES`).
- `sdate(int|cal)` = `"dd-MMM-yyyy"`; `sdate_MY(cal)` = `"MMMMMMMMMMMM-yyyy"` (full month-year).

---

## 8. Images and quick picks

- `getImage(path, bg)`: cache key `path + "(" + blue + "," + green + "," + red + ")"` (note B,G,R
  order). Loads `new Image(display, path)`; failure → `noImage`. The image's `setBackground(bg)`
  is applied once at cache time. Returns the cached image.
- `getImageAlt(path, altPath, bg)`: `getImage(path)`; if it is `noImage` → `getImage(altPath)`.
- `getScaledImage(path, bg, factor)`: factor == 1.0 → the cached image; else a **new** image
  `scaledTo((int)(w*factor), (int)(h*factor))` (not cached).
- `loadQuickPickTags()`: reads `images/image.tags` line by line (`while ((line = readLine()) != null)`,
  one read per iteration (verified against regenerated pseudo code)). Each line `tags=images` is split on
  `=`; lines that do not yield exactly 2 parts are ignored. Left part split on `,` → tag words,
  right part split on `,` → image file names; both must be non-empty. For each tag word an
  `ImgTag{ substring = word.toUpperCase(), images = all images of that line }` is appended to
  the global `imgTags` (file order). Any exception → stderr `"Failed to load 'images.tags': …"`.
- `updateQuickPicks(container, text, targetButton)`: `text = text.toUpperCase()`. Build a
  `HashSet<String>` of image names: iterate `imgTags` in order; if `text.indexOf(tag.substring) != -1`
  add every image of that tag, stopping everything as soon as the set reaches 10 entries.
  If the set equals `lastQuickPicks` → return (no UI change). Else store it, `setRedraw(false)`,
  dispose container children from index 2 onward (the first two children are kept), then for
  each name (HashSet iteration order — unspecified) create a push `Button` with
  `getImage(images/<name>, button bg)` (if the image is null the button is disposed), data and
  tooltip = name; clicking it (Budget$33) sets the target button's image and data to the
  clicked button's. `layout()`, `setRedraw(true)`.

---

## 9. Forecast bar graph

### 9.1 Canvas size — `getGraphDim()`
With font `fontGraphDayLabel`: `graph_bar_width = max(width("10,000 "), 30)`;
`e = extent(" (-100,000)")`; `n = graphNMonths == -1 ? 6 : graphNMonths`;
`width = graph_bar_width * n + 7 + e.x`; `statsH = quickStatsCtnr.computeSize().y`;
`graph_bar_height = statsH − 2*e.y` (minus one more `e.y` on Mac/Linux); `height = statsH + 2`.
The canvas gets `computeTrim` of that, `horizontalIndent 10`, `verticalAlignment BEGINNING`.

### 9.2 Data — `calcGraphData(n)` (per month i = 0..n−1, month = curCal day 1 + i months)
`days = actual days in month`; `graph_data[i].cal` = month start.
- 0 Opening: `opening_balance` of day 1 (0 if no BudgetDay).
- 1 Closing: `closing_balance` of the last day.
- 2 Average: sum of `closing_balance` over the days that exist, divided by `days` (the calendar
  length, not the count of existing days).
- 6 Minimum / 7 Maximum: min/max of `closing_balance`; initialised from day 1 if it exists,
  otherwise from 0.0 (missing day 1 biases toward 0).
- 3 Income / 4 Expenses: `calcStatistics(monthStart, 1, 0)` → `total_income` / `total_expense`.
- 5 Disposable (diff): `closing(last day) − opening(day 1)` (missing days contribute 0).
`graph_data[i].val = value`; `sum[0] = val[0]`, `sum[i] = sum[i−1] + val[i]`. If `n < 12`,
entries `n..11` copy `cal/val/x_max` from entry `n−1`. (`x_max` is only refreshed in
`drawGraph`, so the copies carry the previous draw's value.)

### 9.3 Scale — `getBestScale(data)`
`dynamic = (graphPresetMin == 0.0 && graphPresetMax == 0.0)`. `min/max` = data min/max when
dynamic, else the presets. When dynamic: if both ≥ 0 → `min = 0`; if both < 0 → `max = 0`.
Then for each `step` in `stepList`: `top = (int)ceil(max/step)*step`, `bottom =
(int)floor(min/step)*step`, `nLines = |top − bottom| / step` (int); if `nLines < 10` → `max = top`,
`min = bottom`, stop. If no step satisfies this, min/max stay unrounded and `nLines` is the
last computed value. Result `nLines = (nLines == 0 ? 1 : nLines)` (default field value 9 is
always overwritten). Side effect: `graph_last_min = (int)min`, `graph_last_max = (int)max`.

### 9.4 Drawing — `drawGraph(gc)`
1. `n = get_graph_n_month()`. Cap: `limit = initCal` day 1 + `futureNMonths` months, month :=
   December, + 1 month (= January after the December of the year reached). For `i` in 1..n, if
   `curCal day1 + i months == limit` → `n = i`.
2. `data = calcGraphData(n)`. If `graphPresetMin > graphPresetMax` swap them (persisted).
3. `scale = getBestScale(data)`; `w = graph_bar_width`; `W = w*n`; `H = graph_bar_height`;
   font = `fontGraphDayLabel`; `labelH = extent("1.0").y`; `range = |(int)(max − min)|`;
   `crossesZero = max >= 0 && min < 0`; `zeroY = crossesZero ? (int)((0 − min)/range * H) : H`.
4. Fill `(0,0,W+100,H+30)` with `bgColorDetails`.
5. Bars, `i = 0..n−1`: colour `graphBarColor` if `data[i] >= 0` else `graphNegativeBarColor`;
   `bar = getBarInfo(min, max, data[i], range, H, crossesZero, zeroY)`;
   `fillRectangle(i*w + 1, labelH + H − bar.y, w, bar.h)`. If `bar.clipped_top`: system YELLOW,
   line width 10, line from `(i*w+1, labelH)` to `(i*w+w, labelH)`; if `clipped_bottom`: same at
   `y = labelH+H`, x to `i*w+w−1`. Line width back to 1. `graph_data[i].x_max = (i+1)*w + 1`.
6. Vertical grid (`graphGridColor`) at `x = i*w`, `i = 0..n`, from `labelH` to `labelH+H`.
7. Month names `monthName[(curCal.month + i) % 12]` in `graphTextColor`, centred in each column,
   at `y = labelH + H + 2`.
8. Horizontal grid: `step = range / nLines` (double); lines `k = 0..nLines` at
   `y = labelH + k*H/nLines` from `x = 0` to `W + 3`.
9. Y labels, format `" #,##0; (-#,##0)"` of `max − step*k`: compute `maxW/maxH` over all
   labels; `parenW = width(")")`; draw each at `x = W + 7 + (maxW − labelW) − (value >= 0 ? parenW : 0)`,
   `y = k*H/nLines + labelH/2` (right-aligned; negatives wear parentheses). For `k == 0` and
   `k == nLines` store `graphHotSpots[0]` (max) / `[1]` (min) = `Rectangle(W+5, y, maxW, maxH)`;
   if scaling is frozen (any preset non-zero) draw that rectangle outline.
10. If `crossesZero`: zero line in `graphTextColor` at `y = labelH + H − zeroY`, x 0..W+3.
11. Bar value labels: longest `numFormatFrac0` value string + `" "`; font `Helvetica` at the
    gc font's height, normal; while its width ≥ `w` and height > 7 decrease the size by 1 pt.
    If the final height ≥ 7: draw each value (`numFormatFrac0`), colour `colorPositiveAmount` /
    `colorNegativeAmount` (value < 0), centred in the column at
    `y = labelH + H − bar.y − textH − 1` (just above the bar's top edge).

`getBarInfo(min, max, val, range, H, crossesZero, zeroY)` → `{y, h, clipped_top, clipped_bottom}`
(`y` = distance from the bottom of the plot to the bar's top edge):
- `val > max` → `val = max`, `clipped_top`; `val < min` → `val = min`, `clipped_bottom`.
- `min > 0`: `h = y = (int)((val − min)/range * H)`.
- else if `max < 0`: `y = H`, `h = |(int)((val − max)/range * H)|`.
- else: `h = |(int)(val/range * H)|`; `val < 0` → `y = zeroY`; else `crossesZero` → `y = zeroY + h`;
  else `y = (int)((val − min)/range * H)`.
(All ratios are truncated with `(int)`.)

### 9.5 Mouse on the canvas
- `getGraphData(x, y)` = first `graph_data[i]` with `x < x_max`, else null.
- Mouse move (Budget$25): over a bar: if type ∈ {Income, Expenses, Disposable} and the bar is
  not the first: prefix `"<sum> -- running total: <first MMM, yyyy> - <this MMM, yyyy>\n\n"`
  (sum formatted `"#,##0;(-#,##0)"`). Then tooltip = prefix +
  `"DYNAMIC SCALING, use context menu to Freeze the current scaling\n"` or
  `"FROZEN SCALING [ <min> .. <max> ]\nuse context menu to change to dynamic.\n"`.
- Mouse down (Budget$24): button 3 → popup menu: `"Freeze current scaling"` (if dynamic;
  sets presets to `graph_last_min/max`) or `"Use dynamic scaling"` (presets := 0, 0); separator;
  `"Custom Maximum"`; `"Custom Minimum"`. Left click: inside `graphHotSpots[0]` → Custom
  Maximum; inside `[1]` → Custom Minimum; else on a bar → if that month differs from `curCal`:
  `curCal = that month`, `updateCalendarView()`, tooltip cleared.
- Custom Minimum/Maximum: `prompt(mainShell, null, "Custom Value",
  "Custom hint for minimum|maximum to freeze:\nThis hint value may be adjusted to have the best visual appearance.",
  current preset formatted "0.00", 0)`; result → `parseDouble`; if the *other* preset is 0.0 it
  is set to `graph_last_max` / `graph_last_min`; redraw. (A preset of exactly 0 counts as unset.)
- Graph type switch: clicking any Quick Stats value (Budget$8) or choosing from the
  `forecastLbl` popup menu (Budget$18/$19; items = `forecastType[0..7]`) sets `cfg.graphType`,
  redraws, and sets `forecastLbl` text to `"Forecast: " + forecastType[type]`.

---

## 10. Main window — `createShellContents()`

Prerequisite: `wDayName` rebuilt from the locale; `stat_line_color = true`; `mainShell` uses
`FillLayout`; menu bar rebuilt (`createFileMenu`, `createEditMenu`, `createHelpMenu` — other
document).

Colour helpers: `bg_darker_color` = `bgColorDetails` ±: dark theme → each channel `+32`
(clamped), light → each channel `−10`.

Hierarchy (GridData constants: 2 = VERTICAL_ALIGN_BEGINNING, 4 = V_CENTER, 64 = H_CENTER,
128 = H_END, 256 = H_FILL, 768 = FILL_HORIZONTAL, 1028 = V_CENTER|GRAB_VERTICAL,
1040 = FILL_VERTICAL, 1808 = FILL_BOTH):

```
mainShell (FillLayout)
└─ outer Composite  GridLayout(1 col) marginW/H 1, vSpacing 0; bg = bg_darker_color when dark
   ├─ detail container  style BORDER|SHADOW_OUT (light) / 0 (dark); GridData FILL_HORIZONTAL;
   │  bg bgColorDetails; GridLayout 5 cols, margins 2, hSpacing 8
   │  row 1: [fold col] [Quick Stats] [forecastLbl] [Brief Summary header row] ["Notes" link]
   │  row 2: (fold & Quick Stats span 2 rows) [graphCanvas] [stats_scroll] [notesText]
   └─ calendarScroll  ScrolledComposite (BORDER unless dark)|H_SCROLL|V_SCROLL, FILL_BOTH
      └─ calendarView  GridLayout 7 equal columns, margins 8, spacing 2; bg bgColorWindow
         (filled by createCalendarView — other document)
```

Details:
- **Fold column**: Composite (bg `bg_darker_color`, vSpan 2, vertical FILL, GridLayout marginW 2)
  with a Label showing `icons/fold.png` (`fold-light.png` in dark theme), tooltip
  `"Fold Details View"`, GridData 1028. Click (Budget$13 = `foldLabelListener`): detail
  container `GridData.exclude = true`, hidden; `unfoldComp = createUnfoldComp(detail)` moved
  above it and shown; parent `layout(true)`.
- **Quick Stats** (`quickStatsCtnr`): GridLayout 2 cols, hSpacing 1, vSpacing 2, marginH 0;
  GridData VERTICAL_ALIGN_BEGINNING, vSpan 2. Contents:
  1. Month links row: Composite 12 cols, hSpacing 3, marginH 0, H_CENTER, hSpan 2. Twelve
     Labels for `initCal + k months` (k = 0..11, today's month first), text `"MMM"` (locale),
     fg `colorCurrentMonth`, bg `bgColorDetails`, font `fontCurMonthLink` for k=0 else `fontTran`,
     data = k, underline painter (`lbl_underline`: 1-px line in `colorCurrentMonth` along the
     bottom of the label). Click (Budget$14): `cal = initCal day1 + k months`; if year/month
     differ from `curCal` → `curCal = cal`, `updateCalendarView()`.
  2. Month navigation row: Composite 3 cols, marginH 0, H_CENTER, hSpan 2, `widthHint` = the
     maximum packed width while the label shows each of the next 12 months. `left_lbl`
     (`icons/left_arrow.gif` / `left_arrow_gray.gif`), `monthLabel` (font `fontMonth`, fg
     `colorCurrentMonth`, H_CENTER + grab), `right_lbl` (`right_arrow.gif` / `right_arrow_gray.gif`).
     `monthLabel` has the underline painter, a popup menu (`updateMonthLabelMenu`) opened on click
     (Budget$17), and text from `setMonthLabel`.
  3. Eight stat rows via `makeStatLabel(parent, title, "0.0", graphType)` in this order:
     `"Opening Balance:"`→0, `"Closing Balance:"`→1, `"Minimum Balance:"`→6,
     `"Avg Daily Balance:"`→2, `"Maximum Balance:"`→7, `"Total Income:"`→3,
     `"Total Expenses:"`→4, `""`→5 (the income−expenses difference row has no caption).
     Each row = two right-aligned Labels (SWT.RIGHT) with GridData H_FILL and font `fontTran`;
     rows alternate background starting with `bg_darker_color` then the parent background
     (`stat_line_color` toggles per call). Caption fg = white if the row bg is dark else system
     black; value fg = `colorPositiveAmount`; value tooltip `"Click to switch graph type"`,
     click → graph type (Budget$8).
- **forecastLbl**: `"Forecast: " + forecastType[graphType]`, font `fontTran`, bg `bgColorDetails`,
  fg `colorCurrentMonth`, underlined; popup Menu with the 8 forecast types; click opens it.
- **Brief Summary header row**: Composite 5 cols, margins 0, hSpacing 5, GridData H_FILL:
  Label `"Brief Summary /"` (fontTran; fg white when dark else black); Label `"Full Summary"`
  (fg `colorCurrentMonth`, underlined, tooltip `"Click -- open full details stats window\nCtrl+Click -- to open at the default location with the default size\n"`;
  click → `showStats(mainShell)`, Ctrl+click first resets `statX/Y/Width/Height = -1`);
  Label `"          Filter:"` (tooltip `filterToolTipText`); `tranFilterTxt` = Text BORDER, font
  `fontTran`, fg system RED, GridData FILL_HORIZONTAL, tooltip `filterToolTipText`. Enter
  (Budget$21): `calViewFilter = null`; text trimmed with `&` → U+FF06; non-empty →
  `new Filter(text)`; `updateCalendarView()`.
- **"Notes" label**: fg `colorCurrentMonth`, font `fontTran`, underlined; click (Budget$22) →
  `editNotes(mainShell)`; on true `notesText` text and tooltip := `db.notes`.
- **graphCanvas**: size from `getGraphDim` (§9.1); paints bg `bgColorDetails` then `drawGraph`.
- **stats_scroll** (Brief Summary table container): ScrolledComposite H_SCROLL|V_SCROLL,
  GridData FILL_VERTICAL, `heightHint` = graph height, `horizontalIndent 10`, expand both;
  content `stats_view` (filled elsewhere: columns Description, Amount, %, +/-, Graph).
- **notesText**: Text MULTI|LEFT, GridData FILL_BOTH with width/heightHint 20, text and
  tooltip = `db.notes`, read-only, bg `bgColorNotes`, fg `colorNotes`.

### 10.1 `setMonthLabel()`
`monthLabel` text = `SimpleDateFormat("MMMMMMMMMMMMM,yyyy")` of `curCal` (e.g. `December,2021`);
parent re-laid out. Left arrow image = gray when `curCal.yyyymm == minDate.yyyymm`, right arrow
gray when `== maxDate.yyyymm`. If folded, `unfoldMonthSelector` text = `"<a>" + text + "</a>"`.

### 10.2 `updateMonthLabelMenu()`
Collect every month start from `minDate` (day 1) while `inRange(minDate, maxDate)`; menu =
`createStatStartEndMenu(mainShell, months, Budget$32, null, null)` attached to `monthLabel`.
Selecting an item (Budget$32): if the month is in range → `curCal = month`, `updateCalendarView()`.

### 10.3 Arrow clicks (Budget$15 previous / Budget$16 next)
If a filter is active: walk month by month toward min/max until `hasMatches(filter, month)`;
none → `showInfo("No filter matching transactions found before|after this month!")`, stop.
If found and `getDiff(found, curCal) > 35` days: `confirmYesNoCancel("There is no filter matching transactions found in the previous|next month.\nDo you want to jump directly to the matching date (<MMMMMMMMMMMM-yyyy>)?")`:
YES → `curCal = found`, update; CANCEL → nothing; NO → fall through. Fall-through / no filter:
if `curCal` is at `minDate`/`maxDate` month → nothing, else `curCal ± 1 month`, `updateCalendarView()`.

`hasMatches(filter, month)`: for day 1..last: if `db.getDay(day) == null` → return **false**
immediately; any transaction with `filter.isMatch(pmt, amount, yyyymmdd)` → true; else false.

### 10.4 Folded ("details view" hidden) bar — `createUnfoldComp(detail)`
Composite in the outer composite, bg `bgColorDetails`, GridData(FILL, 0, grabH, no grabV),
GridLayout 6 cols, hSpacing 15, marginH 0:
1. Label `icons/unfold.png` (`unfold-light.png` dark), bg `bg_darker_color`, tooltip
   `"Show Details View"`, GridData(0, CENTER(16777216), false, true). Click (Budget$27): detail
   container `exclude = false` & visible, dispose `unfoldComp`, null the three unfold fields,
   `updateMonthLabelMenu()`, parent `layout(true)`.
2. `unfoldMonthSelector` Link, initial `"<a>September 2000 </a>"` to size it (`widthHint` =
   computed width + 10), fg white/black by darkness, link colour `colorCurrentMonth`, tooltip
   `"Click to choose a different month"`; then `setMonthLabel()` sets the real text; click →
   `monthLabel`'s menu.
3. Label `"Filter:"` (tooltip `filterToolTipText`).
4. `unfoldFilterText` Text BORDER, `widthHint 200`, font `fontTran`, fg RED, initial =
   `tranFilterTxt` text. Enter (Budget$29): copy text (`&` → U+FF06) into `tranFilterTxt` and
   `notifyListeners(SWT.DefaultSelection)` on it (so the same filter logic runs).
5. Link `"<a>Summary</a>"` → `showStats`, tooltip `"Click to open Full Summary Window"`.
6. Link `"<a>Notes</a>"`, tooltip `"Click to edit budget notes. Current notes:\n" + db.notes`;
   click → `editNotes`, then refresh `notesText` and this tooltip.

`setFilterText(s)` sets `tranFilterTxt` and, when folded, `unfoldFilterText`.
`cfg.startFolded` is written from `unfoldComp != null` on `saveRc` and re-applied at startup.

### 10.5 `calendarLabelWidthHint(control, pad)`
Max of `computeSize().x` while the control (Button or Label) shows `pad + "MMMMMMMMMMMMM,yyyy" + pad`
for each of the next 12 months after `curCal`.

### 10.6 `flash_latest_adjustment()`
`d = db.getLatestAdjustmentDate()`; -1 → nothing. If `curCal.yyyymm != d.yyyymm` → `curCal = d`
day 1, `updateCalendarView()`. Then find the calendar cell (`cells[]`) whose data calendar
`get() == d` → `flash_cell` (the main loop flashes it, §3 step 30).

---

## 11. Display-level listeners — `attachDisplayListeners()`

`display.addFilter(SWT.KeyDown, Budget$11)` and `display.addFilter(SWT.MouseWheel, Budget$12)`.

### 11.1 Keyboard (Budget$11)
`bump.reset()`. If the focus control's shell is not `mainShell` → ignore. Then by `keyCode`:
- `SWT.HOME` (16777223): ignored when focus is in `tranFilterTxt` with non-empty text; else
  `curCal = initCal` (today) day 1 → `updateCalendarView()`.
- `SWT.END` (16777224): same filter-text exception; `flash_latest_adjustment()`.
- `SWT.PAGE_UP` (16777221): previous month unless `curCal` month == `minDate` month.
- `SWT.PAGE_DOWN` (16777222): next month unless at `maxDate` month.
- `SWT.CTRL` (262144): if `labelInFocus` (hovered transaction label) exists and is not
  disposed → `showStatsPopUp(labelInFocus)`.

### 11.2 Mouse wheel (Budget$12) and `Bump`
Only when the focus control's shell is `mainShell`. Ignore if `count == 0` or
`bump.isIgnoring(e)` (event time < `ignoreUntilTime`).
- **Ctrl+wheel**: `updateFontSize(count > 0 ? +1 : −1)`; `bump.setDelay(e)` (ignore wheel for 1000 ms).
- Else if `calendarScroll`'s vertical bar is visible: `origin.y −= count`; if the origin
  changed → `bump.reset()`, done. If it did not change (at an edge): `bump.bumped(e)` — the
  first bump at the edge is absorbed (`bumped = true`, ignore 1000 ms) and returns; a second
  wheel event after that delay proceeds (and clears `bumped`).
- Then, if `cfg.wheelSupport`: `bump.getAction(e)` (sets ignore 1000 ms) → `count > 0` =
  ACT_UP → previous month, else ACT_DOWN → next month, each guarded by min/max month;
  `scheduleUpdateCalendarView()` (deferred to the main loop); `bump.setDelay(e)`.

`Bump`: `BUMP_DELAY = 1000` ms; `isIgnoring` also records `lastTime`; event times are
masked to 32-bit unsigned.

### 11.3 Font size — `updateFontSize(delta)`
`changeFontSize(font, delta)` adds `delta` points to every `FontData` height. Applied to
`fontTran, fontCurMonthLink, fontDate, fontAmount, fontGraphDayLabel, fontPosTranAmount,
fontNegTranAmount, fontImportantTran, fontPaidTran` (not `fontLabel`, `fontMonth`). Then
`rebuildApp()` (Mac: `scheduleRebuild()`). The overload `changeFontSize(font, style, factor)`
multiplies heights by `factor` and sets the style. The enlarged fonts are persisted via `.budgetrc`.

### 11.4 Midnight / date change (Budget$7, every 20 000 ms)
If the shell is alive and `today != initCal`: if `cfg.autoDateSwitch` → `initCal = today`,
`rebuildApp()`; else ask `"System date changed to a new date.\nDo you want to switch to the new day?"`
— Yes → same; No → the timer is **not** re-armed. Otherwise re-arm for another 20 s.

### 11.5 Other
- `isActiveShell(shell)`: focus control exists and its shell == shell.
- `getWDayName(i)` = `wDayName[(firstDayOfWeek + i) % 7]`, `firstDayOfWeek` = 0 when
  `firstDayOfWeekIsSunday` else 1.
- `getSixMonthsAvg` etc. see §12.

---

## 12. Edit-dialog helpers and tables

### 12.1 `fillPaymentsTable(table, selectPmt)` — "List of All Payments"
`today = initCal.get()`; `filter = new Filter(table.getData("filter"))`; `db.sort()`; redraw off;
`removeAll`. If `selectPmt == null`, keep the currently selected row's payment. Collect every
`db.get(i)` with `id != 0` that `filter.isMatch(pmt, pmt.amount, 0)`; sort with
`sortColumn.getData("comp")` if a sort column is set. Per row (`TableItem`): column 0 image =
`getImage(images/<image_path>, bgColorWindow)` when `image_path` non-empty; `setData(pmt)`;
texts by column index: **2** amount (`numFormatFrac2`), **3** category or `""`, **4** description,
**5** start date `dd-MMM-yyyy`, **6** end date or `"---"` (INDEFINITE), **7** `n_pays` or `"---"`,
**8** `frequency + freq_type` (e.g. `1m`), **9** `"yes"` if no-weekends, **10** flags
`"[E]"` early-pay + `"[T]"` tentative + `"[L]"` end mode LOAN_PAID (64). Column 1 receives no
text here (UNKNOWN: fillPaymentsTable — what column 1 holds; the table columns are created
elsewhere). Row colour: `startDate > today` → `colorFuturePayment`; non-indefinite and
`endDate < today` → `colorPastPayment`; `disabled` → `colorDisabledPayment` (later wins).
Row of `selectPmt` is selected; `showSelection()`; redraw on.

### 12.2 Widget factories
- `makeLabel(parent, text)`: Label, font `fontLabel`, GridData HORIZONTAL_ALIGN_END.
- `makeField(parent, label, value, widthHint)`: `makeLabel` + Text BORDER with `widthHint`.
- `makeRadio(parent, text, mode, current)`: radio Button, selected iff `mode == current`, GridData VERTICAL_ALIGN_CENTER.

### 12.3 End-mode radios (Payment end modes: 16 INDEFINITE, 32 DATE, 48 N_PAYS, 64 LOAN_PAID)
`updateEndSelection(mode, rIndef, rNPays, tNPays, rDate, tDate, bDatePick, rLoan, tLoan)`:
`rIndef.selected = (mode==16)`; `rNPays.selected = (mode==48)`, `tNPays.enabled = (mode==48)`;
`rDate.selected = (mode==32)`, `tDate.enabled = bDatePick.enabled = (mode==32)`;
`rLoan.selected = (mode==64)`, `tLoan.enabled = (mode==64)`.
`checkEndPayment(radio, text)`: true (= invalid) iff the radio is selected and
`Integer.parseInt(text) <= 1`; parse errors → false.

### 12.4 Recurrence messages
- `get_nth_week_msg(dateStr)`: `arr[(day−1)/7] + " " + wDayName[dayOfWeek] + " in a month"` →
  e.g. `"2nd Tuesday in a month"` (`getWeekOfMonth = (d−1)/7`, 0-based; `getDayOfWeek` 0 = Sunday).
- `get_last_weekday_msg(cal)`: `"Last " + wDayName[dayOfWeek] + " in a month"`.

### 12.5 `getSixMonthsAvg(description, cal)` — exact algorithm
`cal` := its month's day 1. Iterate day by day from `cal − 6 months` while `day < cal`:
for each transaction on that day whose payment is not disabled and whose
`pmt.description.equalsIgnoreCase(description)`: if it is the first match `first = day`;
`n++`; `total += amount`; if the day's `yyyymm` differs from the last counted one → `nm++`;
`last = day`; insert `TranInfo(day, amount, pmt)` at index 0 of `lastTransactions` (newest first).
After the loop: if `n >= 3` → `first_last_days_diff = getDiff(first, last)` (days).
`first` := `(first/100)*100 + 1` (1st of its month; 1 when no match), `last` := last day of
`last`'s month. `total`, `n`, `nm` stored; `pt = total/n` if `n > 0` (per transaction);
if `nm > 0`: `pm = total/nm` (per month), `pd = pm / 30.25` (per day);
`np = db.getNumberOfPlannedTransactions(description, first, last)`; if `np != 0` → `pp = total/np`.
Unset ratios stay NaN.

---

## 13. UNKNOWN / not determined here
- `fillPaymentsTable`: contents of table column 1 (no text set in this method).
- `open` / main loop: whether `updateCalendarView()` clears `do_best_fit` (not visible in part03).
- `createStatStartEndMenu`, `showStats`, `showStatsPopUp`, `editNotes`, `showHint`,
  `createCalendarView`, `updateCalendarView`, `saveDB`, `calcStatistics`, `prompt`,
  `confirmYesNoCancel`, `licAgree` body, `budgetIcon`, `centerLoc` — referenced but outside
  this file's scope.
- `drawGraph` cap: the `limit` month is computed from `initCal + futureNMonths` with month set
  to December plus one month, which is one year later than `maxDate + 1` when `initCal`'s month
  is not December; documented literally, intent unclear.

Verified pass: 2026-10-27 — 1 statements corrected, 0 confirmed.
