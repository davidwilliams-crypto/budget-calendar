# 09 — Preferences, config framework, Preferences dialog, CLI options, misc helpers

Sources (pseudo code): `BudgetConfig`, `ui.config.Config`, `ui.config.ConfigEditor`, `misc.CLIOpts`, `misc.FileOps`, `misc.UnicodishBufferedWriter`, `misc.Platform`, `misc.AppResources`, `misc.JarVerifier`, plus the relevant parts of `Budget` (part01 Preferences handler, part03 `main`/`loadRc`/`saveRc`/`is_trial_version`, part04 registration).

---

## 1. The rc file

| Item | Value |
|---|---|
| Constant `BudgetConfig.RCFILE_NAME` | `System.getProperty("user.home", ".") + File.separator + ".budgetrc"` |
| Actual file used by `Budget.main` | `Budget.dirHome + File.separator + ".budgetrc"` where `dirHome` is `user.home`, or the current directory in "budget stick" mode (a `../../BudgetFiles/data` dir exists next to the app; `../../..` on Mac, `../..` Linux, `..\..` Windows). Overridden by `-c rcfile`. |
| `BudgetConfig(String name)` | uses `name`, or `RCFILE_NAME` when `name == null`. |
| Format | `java.util.Properties` text file (`key=value`, `#` comments, Properties escaping). Written with `Properties.store(out, "")` so the file starts with `#` (empty comment) and `#<date>` lines. |
| Backup on save | `Config.saveProperties`: if the file exists it is first copied to `<file>.bak` (`FileOps.copyFile`); if that copy fails, save aborts with message `unable to backup '<abs>' to '<abs.bak>'`. After writing: on success `.bak` is deleted; on failure `.bak` is renamed back over the file. Returns `null` on success, else an error string (`"Exception: <ex>"`). `Budget.saveRc` shows `Unable to save config '<rc>'\n<msg>` on error. |
| Load | `Config.loadProperties(name, quiet=false)` — missing/unreadable file prints a stack trace and yields an empty `Properties` (all defaults). The boolean result of `load()` is ignored. |

### 1.1 Serialization rules (Config.save / Config.load, reflection over **public, non-static, non-transient, non-final** fields; the property key is the Java field name)

| Field type | Save format | Load parsing |
|---|---|---|
| `String` | raw value; **omitted if null** | raw |
| `boolean` | `true` / `false` | `true` or `1` → true, anything else false |
| `int` | decimal | `Integer.parseInt` (exception → stack trace, field unchanged) |
| `double` | `Double.toString` | `Double.parseDouble` |
| `Color` | `%06x` → `rrggbb` lowercase hex, no `#` | `Integer.parseInt(v,16)`; parse failure → 0 → black (0,0,0) |
| `Font` | SWT `FontData.toString()` (SWT's platform-specific serialized form, e.g. on Windows `1\|Verdana\|9.0\|0\|WINDOWS\|1\|-12\|0\|0\|0\|400\|0\|0\|0\|1\|0\|0\|0\|0\|Verdana`) | `new FontData(string)` |
| `Config$Int` | decimal | `Integer.parseInt` |
| `Config$ConfGroup` | `true`/`false` = **collapsed state** of the group (keys `ui`, `fnt`, `clr`, `grc`, `adv`) | `v.equals("true")` |
| `Config$ConfAction` | **never saved** | n/a (setValue is a no-op) |
| any other `Config$ConfVal` | `toString()` — ConfBool `true`/`false`; ConfInteger decimal; ConfDouble `Double.toString`; ConfColor `rrggbb`; ConfFont FontData string; ConfString raw | `setValue(String)`: ConfBool: trimmed `"1"` or case-insensitive `"true"`; ConfColor: skipped if equal to current `toString()`, else hex parse (bad → black); ConfFont: skipped if unchanged else `new FontData`; ConfInteger/ConfDouble: parse, on NumberFormatException stack trace and value unchanged; ConfString raw |
| `Vector<String>` | one key per element: `<name>.<index>` (e.g. `recentFile.0` … `recentFile.4`); empty vector writes nothing; a Vector whose first element is not a String throws | key with `.N` suffix: `N` parsed (negative → exception), vector grown to `N+1` and element set |
| anything else | throws `NoClassDefFoundError("'<name>' -- unsupported type")` | same |

Load details:
- Keys in the **ignore set** are skipped silently. BudgetConfig sets it to `{graphPastBarColor, graphExtraMonthBarColor, graphMinBarColor, graphAvgLineColor, detailsStatsOnDesc}` (preferences removed in earlier versions; they are dropped on the next save).
- Unknown key (NoSuchFieldException) → the `BiConsumer` unknown-key handler is called (default handler prints `unknown field: k=v` to stderr). BudgetConfig's handler (`lambda$new$1`):
  - legacy keys `recentFile0` … `recentFile4` → stored into `recentFile[i]`;
  - legacy key `firstDayOfWeek` → `firstDayOfWeekIsSunday = (value == "1")`;
  - anything else → stderr `skipped: k=v`.
- Any other exception per key → stack trace, continue. `load` returns true only if no key failed.

### 1.2 Defaults store
`storeDefaultConfVals()` (called in the constructor **after** `setDefaultValues` and **before** `load`) snapshots `toString()` of every `ConfVal` in every group; `restoreDefaultConfVals()` / `restoreDefaultConfVals(group)` re-apply them through `setValue`. Only `ConfVal`s (not the plain public fields like window geometry) are covered.

### 1.3 `Config` static helpers
- `getFullRCPath(name)` = `user.home + File.separator + name`.
- `getDocumentsPath()` = `user.home`; on Windows (`File.separator == "\\"`) prefers `<home>\Documents` if it is a directory, else `<home>\My Documents`, else `<home>`.
- Neither is referenced anywhere else in the pseudo code (unused by Budget).

### 1.4 Order of operations in `BudgetConfig(String)`
1. Plain fields get hard defaults (table below); groups and ConfVals constructed in declaration order (this order = order in the Preferences dialog).
2. `setConfigFileName`.
3. `setDefaultValues(display)` → fonts + `setLightThemeColor`.
4. `storeDefaultConfVals()`.
5. `setIgnores(...)`.
6. `currencyPrefix.rowable = currencySuffix.rowable = true`.
7. `apply_theme.setConsumer(lambda$new$0)`: `"LIGHT(default)"` → `setLightThemeColor`, `"DARK"` → `setDarkThemeColor`, else `UnsupportedOperationException`.
8. `load(lambda$new$1)`.

After construction `Budget.loadRc` additionally clamps `futureNMonths` to `[12..180]` (`Budget.clamp`) and `graphNMonths` through `Budget.valid_month_n` (`-1` kept; else clamped to `[1..12]`).

### 1.5 `BudgetConfig.apply(Map originals)` (called by the editor's Apply)
Returns `null` when valid, else a `Map<ConfVal,String>` of errors:
- `futureNMonths` outside `[12..120]` → `"Invalid number of future months. Valid number is [12..120]."` (note: loadRc clamps to 180, apply allows only 120).
- `graphNMonths` where `valid_month_n(n) != n` → `"Invalid number of forecast months. Valid numbers are: -1, or [1..12]."`
The `originals` argument is not used.

---

## 2. Complete preference table

Legend: *Type* is the Java type (plain field) or the ConfVal class. "Light"/"Dark" columns only apply to colors (`rrggbb` as saved; RGB in parentheses). Default fonts: `h` = system font height (points, `Display.getSystemFont().getFontData()[0].getHeight()`), `s = (int)(h*0.9)`, `m = (int)(h*1.2)`, `l = (int)(h*1.5)`; `MONO` = `"Consolas"` on Windows, `"Monaco"` on Mac, `"Monospace"` on Linux, else `"Andale Mono"`; styles NORMAL/BOLD/ITALIC = SWT 0/1/2.

### 2.1 Plain (non-UI) fields — not shown in the Preferences dialog

| Key | Type | Default | Meaning / where used |
|---|---|---|---|
| `regkey` | String | `null` (not written while null) | Registration key. `is_trial_version()` uses it; if empty it tries `regkey.txt` in cwd, then (Windows) `<home>\AppData\Local\VirtualStore\Program Files\MiShellSoft\Budget\regkey.txt`, then (stick mode) `<dirData>/../regkey.txt`. A valid key (`Budget.isValidKey`, out of scope) is copied into `regkey` and sets `unlimitedHistory = true, enabled`. Trial → `unlimitedHistory` forced `false` and disabled in the editor. The Help→Register dialog stores the key and calls `saveRc()`. |
| `lastFileName` | String | `null` | Data file opened last; `loadRc` uses it (trimmed; blank → null) as the startup file; `saveRc` writes current `fileName`. |
| `licAgree` | boolean | `false` | Licence accepted. If false at startup the licence dialog is shown; refusing → `System.exit(10)`; accepting sets true. |
| `firstRun` | boolean | `false` | On Mac, if **true**, asks "Are you using ONE BUTTON mouse?\nYou can always change it later -- File->preferences->One button mouse.\n" and sets `oneButtonMouse`. `saveRc` always writes `false`; since the default is also false the prompt only fires when an rc file explicitly contains `firstRun=true`. |
| `firstRunJPACKAGE` | boolean | `true` | If running from a jpackage build (`jar_loc` ends with `app`) on Windows and this is true → `msiSuggestUninstall()` (suggest removing old MSI install). `saveRc` writes `!is_jpackage`. |
| `startFolded` | boolean | `false` | Written as `unfoldComp != null` at save (main window was in folded/compact mode). At startup, if true, the fold-label mouseDown is simulated (start folded). |
| `windowMaximized` | boolean | `false` | Main shell maximized state (`saveRc`). |
| `windowPosX`, `windowPosY`, `windowWidth`, `windowHeight` | int | `-1` each | Main shell bounds. Used only when `windowHeight != -1`; otherwise default `Rectangle(0, 40 if Mac else 0, 1200, 800 − (40 if Mac))`. |
| `statX`, `statY`, `statWidth`, `statHeight` | int | `-1` each | Statistics window geometry; `statX == -1` means "not set" (centered). |
| `graphPresetMin`, `graphPresetMax` | double | `0.0`, `0.0` | Forecast-graph frozen scaling. Both `0.0` → dynamic scaling. Set via the graph context menu (custom value prompts "Custom hint for minimum/maximum to freeze…", "freeze current", "dynamic" resets both to 0). If min > max they are swapped at use. If only one is set the other takes the last computed graph min/max. |
| `graphType` | int | `2` | Forecast graph type selector (semantics documented with the graph; UNKNOWN here). |
| `image_selection_zoom` | int | `175` | Zoom percent for the image/attachment viewer; context menu changes it by ±25. |
| `recentFile.0` … `recentFile.4` | Vector\<String\> | empty | Recent data files, most recent first, max 5 (`addRecentFile`: remove dup, insert at 0, trim to 5). Legacy keys `recentFile0..4` are also accepted on load. |
| `ui`, `fnt`, `clr`, `grc`, `adv` | ConfGroup collapsed flag | `false,false,false,false,true` | Whether each Preferences group is collapsed. |

### 2.2 Group `ui` — "Controls"

| Key | Type | Default | Label (help text shown in dialog; tooltip in *italics*) |
|---|---|---|---|
| `firstDayOfWeekIsSunday` | ConfBool | `true` | First day of Week is SUNDAY, or MONDAY when unchecked. (legacy key `firstDayOfWeek=1`) |
| `autosave` | ConfBool | `true` | Auto Save, uncheck if you don't want to save the data file automatically. |
| `oneButtonMouse` | ConfBool | `false` | One Button Mouse or a Touch Screen |
| `wheelSupport` | ConfBool | `true` | Wheel Support -- allows scrolling the months with the mouse wheel, just like **PgUp/PgDn** |
| `showDecimalsInCV` | ConfBool | `false` | Show Decimals in the Calendar View |
| `preserveColorOnPrint` | ConfBool | `false` | Preserve transactions' colors when printing monthly calendar |
| `bestFit` | ConfBool | `true` | Try to **FIT** payment description into the window to avoid scrollbars |
| `alwaysShowBalance` | ConfBool | `true` | Always Show Balance, or on ONLY the 1st/last day of a month; days with transactions; days with adjusted balance. |
| `useIndefiniteEndMode` | ConfBool | `true` | Use **INDEFINITE** End Mode by default when creating a new payment |
| `encryption` | ConfBool | `false` | Password support, **NOTE: Weak Encryption!** |
| `autoDateSwitch` | ConfBool | `false` | Automatically switch to the new day at midnight |
| `unlimitedHistory` | ConfBool | `false` | Unlimited history, show all the available months prior the current one (if any). Forced false+disabled in trial mode; hidden in the dialog once it is true (`hidden = boolValue` before opening the editor). |
| `futureNMonths` | ConfInteger | `24` | Number of the future months. This is a rolling limit* — *this numbers starts on the current month and is a rolling window of months*. Valid `[12..120]` (apply) / clamped `[12..180]` (load). |
| `maxSavingNMonths` | ConfInteger | `2` | Number of months to consider when calculating **maximum savings** — *'Max Savings' value is calculated based on the minimum balance and the number of months to consider.\nYou can also think of this as 'Disposable Income' based on the look ahead months.\nThis number will be shown when you adjust a transaction's amount, or when you\nhover over 'Disposable Income' in the 'Brief Statistics' table.* |
| `minBalance4MaxSaving` | ConfDouble (2 decimals) | `100.0` | Minimum balance for **maximum savings** calculations — *same tooltip as above* |
| `graphNMonths` | ConfInteger | `6` | Number of months in the bar graph* — *Special case: -1 -- to display up to 6 months but not 'spill-over' to the next year*. Valid `-1` or `[1..12]`. |
| `currencyPrefix` | ConfString (rowable, width hint = width of "sample" in the system font) | `"$"` | Currency Prefix |
| `currencySuffix` | ConfString (rowable, same width) | `""` | Currency Suffix |

### 2.3 Group `fnt` — "Fonts" (all rowable; sample text shown in the dialog; no tooltips)

| Key | Label | Sample text | Default font (name, size, style) |
|---|---|---|---|
| `fontAmount` | Balance Amounts | `1,000.00` | MONO, h, ITALIC |
| `fontCurMonthLink` | Current Month Link | `Jan...` | Verdana, h, BOLD |
| `fontTran` | Transaction | `Groceries...` | Verdana, h, NORMAL |
| `fontDate` | Date | `12` | Verdana, m, BOLD |
| `fontGraphDayLabel` | Graph labels | `Jan` | Verdana, s, NORMAL |
| `fontImportantTran` | Important Transactions | `Salary...` | Verdana, h, NORMAL |
| `fontLabel` | Labels | `Income...` | *system font name*, s, BOLD |
| `fontMonth` | Month | `Feb...` | Verdana, l, NORMAL |
| `fontNegTranAmount` | Negative amount | `-100.00` | MONO, h, NORMAL |
| `fontPosTranAmount` | Positive amount | `+100.00` | MONO, h, NORMAL |
| `fontPaidTran` | Paid transaction | `Car loan...` | Verdana, h, NORMAL |

(Field declaration order above is the dialog order.)

### 2.4 Group `clr` — "Colors" (all rowable)

First item: `apply_theme` — ConfAction, label "Apply theme *", tooltip "Click a link to apply the named color theme", links `LIGHT(default)` and `DARK`. Never saved. Clicking a link replaces **all** colour values below (including the `grc` group) with that theme set, then marks the dialog modified.

Note the label/field mismatch: field `bgColorCell` is labelled **"BKG cell past"** and field `bgColorCellPast` is labelled **"BKG cell future"**. Which cell actually uses which is decided by the calendar renderer (other analysis); the rc key follows the field name.

| Key | Label | Light (default) | Dark |
|---|---|---|---|
| `bgColorBalance` | BKG balance | `e5f0ff` (229,240,255) | `eda6a6` (237,166,166) |
| `bgColorMinBalance` | BKG MIN balance | `aed0ff` (174,208,255) | `f9c5ba` (249,197,186) |
| `bgColorBelowMinBalance` | BKG below MIN balance | `ffff00` (255,255,0) | `ffff00` (255,255,0) |
| `bgColorMaxBalance` | BKG MAX balance | `dccce2` (220,204,226) | `84a8e2` (132,168,226) |
| `bgColorCellOtherMonths` | BKG cell other months | `cccccc` (204,204,204) | `a5a5a5` (165,165,165) |
| `bgColorCell` | BKG cell past | `cce4ea` (204,228,234) | `7ca7ce` (124,167,206) |
| `bgColorCellToday` | BKG cell today | `fdfdf4` (253,253,244) | `fff693` (255,246,147) |
| `bgColorCellPast` | BKG cell future | `f4f4eb` (244,244,235) | `a6cceb` (166,204,235) |
| `colorPositiveAmount` | Positive amount | `0b077f` (11,7,127) | `003365` (0,51,101) |
| `colorNegativeAmount` | Negative amount | `ff4c5e` (255,76,94) | `ff4844` (255,72,68) |
| `colorPosTranAmount` | Positive tran. amount | `000000` (0,0,0) | `000000` (0,0,0) |
| `colorNegTranAmount` | Negative tran. amount | `8a0000` (138,0,0) | `ff050d` (255,5,13) |
| `colorNotes` | Notes | `bc8f8f` (188,143,143) | `dfdfdf` (223,223,223) |
| `bgColorNotes` | BKG Notes | `ffffe0` (255,255,224) | `202020` (32,32,32) |
| `bgColorHighlight` | BKG highlight | `d9d9d9` (217,217,217) | `fffe79` (255,254,121) |
| `bgColorImportant` | BKG important | `ffff00` (255,255,0) | `ffff00` (255,255,0) |
| `bgColorCellHighlight` | BKG cell highlight | `d9b3ff` (217,179,255) | `fafdca` (250,253,202) |
| `bgColorPaid` | BKG paid | `cadeb8` (202,222,184) | `ff7f82` (255,127,130) |
| `bgColorWindow` | BKG window | `ffffff` (255,255,255) | `535253` (83,82,83) |
| `bgColorDetails` | BKG details | `ffffff` (255,255,255) | `000000` (0,0,0) |
| `colorPastPayment` | past payment | `ff4c5e` (255,76,94) | `ff4c5e` (255,76,94) |
| `colorCalendarToday` | calendar today | `ff4c5e` (255,76,94) | `ff4c5e` (255,76,94) |
| `colorFuturePayment` | future payment | `4cb224` (76,178,36) | `4cb224` (76,178,36) |
| `colorCalendarDay` | cal day | `0b077f` (11,7,127) | `0b077f` (11,7,127) |
| `colorCurrentMonth` | months and links | `0018f4` (0,24,244) | `8080ff` (128,128,255) |
| `colorDescText` | payment description | `000000` (0,0,0) | `000000` (0,0,0) |
| `bgColorCalendarHeader` | BKG headers | `000000` (0,0,0) | `535353` (83,83,83) |
| `colorCalendarHeader` | header | `ffffff` (255,255,255) | `fffaf2` (255,250,242) |
| `bgColorDatePickerHeader` | BKG date picker header | `000000` (0,0,0) | `000000` (0,0,0) |
| `colorDatePickerHeader` | date picker header | `ffffff` (255,255,255) | `ffffff` (255,255,255) |
| `colorDisabledPayment` | disabled payment | `7c7c7c` (124,124,124) | `7c7c7c` (124,124,124) |

### 2.5 Group `grc` — "Forecast Graphics Colors" (rowable; also set by the theme links)

| Key | Label | Light (default) | Dark |
|---|---|---|---|
| `graphTextColor` | text color | `000000` (0,0,0) | `ffffff` (255,255,255) |
| `graphGridColor` | Grid lines | `cccccc` (204,204,204) | `cccccc` (204,204,204) |
| `graphBarColor` | Positive Bars | `3684c9` (54,132,201) | `3662a1` (54,98,161) |
| `graphNegativeBarColor` | Negative bars | `ff4c5e` (255,76,94) | `fb001a` (251,0,26) |

### 2.6 Group `adv` — "Advanced" (collapsed by default)

| Key | Type | Default | Label / tooltip |
|---|---|---|---|
| `singleInstance` | ConfBool | `false` | Single Instance mode which disallows multiple instances of Budget Calendar, see **port#** below. At startup, if true and `isAppAlreadyRunning(port)` → error "Budget Calendar is already running." and `exit(1)`. |
| `port` | ConfInteger | `57778` | Local Server TCP/IP **port** to be used to prevent multiple instances of this application.\n**IMPORTANT**: Some Anti-Virus software may complain -- this one is legitimate* — *A TCP/IP listener is installed on the port for the sole purpose of preventing\nmultiple instances of this application running at the same time.\nNo data exchange/processing of any sort is done!* |

---

## 3. Config framework value types (`Config$*`)

`ConfVal` (abstract) fields: `confGroup`, `label`, `toolTipText` (null → `""`), `comment` (type tag: `"bool"`, `"number"`, `"string"`, `"color"`, `"font"`, `"action,theme"`), `enabled` (true), `hidden` (false), `rowable` (false; true for ConfColor, ConfFont, and explicitly for the currency strings), `uiScratch` (editor text for numbers, see §4).

| Class | Payload | Notes |
|---|---|---|
| `ConfBool` | `boolValue` | |
| `ConfInteger` | `intValue` | |
| `ConfDouble` | `doubleValue`, `decimalPlaces` (default 2; `setDecimalPlaces` chainable) | decimals only affect editor display (`%.Nf`) |
| `ConfString` | `stringValue`, `stringType` (`FILE`/`DIR`/`STRING`, default STRING), `wHint` (−1 = fill) | `defaultValue()` / `setWHint()` chainable |
| `ConfColor` | `colorValue` | |
| `ConfFont` | `fontValue`, `sampleText` (default "Sample Text") | |
| `ConfAction` | `actions[]`, `consumer` | not persisted |
| `ConfGroup` | `description`, `collapsed` | registered in `config.groups` on construction |
| `Config$Int` | mutable int box | unused by Budget |

---

## 4. Preferences dialog (`ConfigEditor`)

Invoked from Budget (part01, `Budget$109`): `cfg.unlimitedHistory.hidden = cfg.unlimitedHistory.boolValue; if (new ConfigEditor(cfg, "Preferences", 0).open(mainShell)) { fadeOut; hide; rebuildApp(); show; focus; fadeIn }`. The editor **never writes the rc file**; persistence happens through `Budget.saveRc()` (exit path, registration, etc.).

### 4.1 Construction
`ConfigEditor(config, title, flags)` — `flags` is stored but unused. Loads three icons once (static): `/icons/settings-group-collapsed.png`, `/icons/settings-group-expanded.png`, `/icons/ic_pulldown_menu.png` (classpath resource; fallback: search each `java.class.path` entry, stripping a trailing `/bin`, for the file; missing → stderr "unable to find an image resource '<name>'" and null image). Snapshots **originals**: `toString()` of every ConfVal of every group.

### 4.2 Window
- Shell style `0x10830` = `SWT.TOOL | SWT.TITLE | SWT.RESIZE | SWT.APPLICATION_MODAL`; title = `title`; background mode INHERIT_DEFAULT, widget-background colour. **Escape is swallowed** (Traverse listener sets `doit=false` for `TRAVERSE_ESCAPE`), so the dialog closes only via Apply/Cancel/close box.
- Layout: 1 column GridLayout, spacing 3.
  1. Top row (RowLayout, centred): a search `Text` (`SEARCH|ICON_CANCEL|ICON_SEARCH|BORDER|SINGLE`), tooltip "Type a string to match -- only matching elements will be shown.", width = width of "This is an example search string"; pre-filled with the current filter. Typing sets `filter` and schedules `rebuildOpts(false)` after **333 ms** (`timerExec`).
  2. A `ScrolledComposite` (`V_SCROLL|H_SCROLL|BORDER`, expand horizontal, not vertical) containing `opts` (2-column GridLayout, spacing 2, white background, `DOUBLE_BUFFERED`), width hint = width of 40 × "W" (`preferredWidth`).
  3. Bottom row (5-column GridLayout): buttons **Defaults** (tooltip "Set ALL values to their defaults.", grabs horizontal space), **Apply** (default button, disabled until something changes), **Cancel**; all sized to "--Defaults--".
- Size: `rebuildOpts(true)` then `computeSize`; minimum = (computed width, displayHeight/4); size = (max(computed width, width of 70 × "W"), min(displayHeight, 40 × system-font line height)); centred on the parent via `MiSWT.centerLoc`. On resize the content is re-laid-out (`lambda$open$0`; note it calls `sc.setMinHeight(size.x)` — uses width for height, looks like an upstream bug (verified against regenerated pseudo code)).
- Modal loop runs until the shell is disposed. Afterwards, if `rc == false` (Cancel/close) **every ConfVal is restored from originals** via `setValue`. Returns `rc`.

### 4.3 Group menu (`groupMenu`, pop-up), opened by right-click on a group header, or left-click / menu-detect on the header's menu icon
Items in order: **&Expand all**, **&Collapse all**, separator, **Restore group's defaults**, **Restore group's originals**, separator, **Increase all fonts**, **Decrease all fonts**. On `menuShown` the two font items are enabled only if the group contains a `ConfFont`.
Handler (`ConfigEditor$1`):
- Expand/Collapse all: set `collapsed` of **all** groups to false/true.
- Restore group's defaults: `config.restoreDefaultConfVals(group)`; `modified()`.
- Restore group's originals: re-apply the originals snapshot for that group; `modified()`.
- Increase/Decrease all fonts: every `ConfFont` in the group gets `MiSWT.changeFontSize(font, ±1)`; `modified()`.
- then `rebuildOpts(false)`.
(Each of these is a single MenuItem: the regenerated pseudo code renders `v3 = this.restoreDefaults = new MenuItem(...)` as `$t = new MenuItem(...); v3 = $t; this.restoreDefaults = $t`, with text/listener attached to `v3`; the identity checks in the handler compare against that one item.)

### 4.4 Group header behaviour (`ConfigEditor$9`, mouseDown on the header label)
- Button 3 (right): open group menu for this group.
- Ctrl + click (stateMask & SWT.CTRL): set **all** groups' `collapsed` to the opposite of this group's state.
- Plain click: toggle this group's `collapsed`.
- then `rebuildOpts(false)`.

### 4.5 `rebuildOpts(boolean initial)` — content rendering
1. Dispose all children of `opts`.
2. If the filter is non-blank: banner CLabel "Only matching items are shown." (yellow background, red foreground, bigger bold font, span 3).
3. Every group containing an item in `errors` is force-expanded.
4. For each group (in `config.groups` order):
   - `vals = getGroupConfVals(group)` (ordered map ConfVal → field name, in field declaration order).
   - Filtering (filter non-blank, compared upper-case): if the group description contains the filter the group is expanded and all its items kept; otherwise every item is kept only if it has an error or `(label + toolTipText + value + comment)` with HTML tags (`<[^>]*>`) stripped contains the filter; group is expanded.
   - Items with `hidden == true` and no error are removed.
   - Empty group → skipped entirely.
   - Header composite (background RGB 255,245,231, span 2, min height of a button, vertical indent 0 for the first group else 1/3 button height): CLabel with collapsed/expanded icon, tooltip "click to expand this group"/"click to collapse this group", text = group description, bigger (+3) bold font; plus a menu-icon CLabel with tooltip "Open group's menu to adjust, reset, etc.".
   - If the group is collapsed and `initial == false` → its items are not rendered (on the initial sizing pass everything is rendered so the width is computed for all content).
   - Items: non-rowable items get their own 2-column cell; consecutive **rowable** items are packed into one wrapping `RowLayout` row (width hint `preferredWidth`). Each item cell: a 3-px marker CLabel (red background if the item has an error; the first error cell is remembered and scrolled into view), then an error-text CLabel in red if any, then `fillConfType`.
5. If no group was rendered and a filter banner exists: replace it with "\nNo matching items found! Try a different search.\n".
6. Resize `opts` (`computeSize` with the scrolled client width, or −1 on the initial pass which also sets `sc.setMinWidth`), set `sc.setMinHeight`, scroll to the first error.

### 4.6 Per-type editors (`fillConfType`) and the description label (`createDesc`)
Description label: text = `label` + (`""` for ConfBool, `":"` otherwise); tooltip = `"[" + fieldName + "]\n\n" + toolTipText` (so the rc key is visible to the user). If the label contains `<` and `>` it is rendered through `StyledTextStream.setHTML` (bold etc.); clicking a link in it calls `Program.launch(linkText)`. Otherwise a plain CLabel.

| Type | Widget | Behaviour |
|---|---|---|
| ConfInteger | label + `Text` (BORDER, width of "12345678") | shows `uiScratch` if set else `intValue`; every edit stores raw text into `uiScratch` and `modified()`; editable = `enabled` |
| ConfDouble | label + `Text` (width "12345678.00") | shows `uiScratch` or `String.format("%.<decimalPlaces>f", value)`; same scratch handling |
| ConfBool | check `Button` then label | selection → `boolValue`; enabled = `enabled` |
| ConfColor | right-aligned label (grabs space) + bordered CLabel swatch (width "12345", dark-red outline) | tooltip "Click to change\n\n"+tooltip; mouseDown → `ColorDialog` preset to the current RGB; a choice sets `colorValue`, repaints swatch, `modified()` |
| ConfFont | right-aligned label + bordered CLabel showing `sampleText` in the font (width "This is a text string.", gray outline, black on white) | mouseDown → `FontDialog` preset with the font; a choice sets `fontValue`, `modified()`, `rebuildOpts(false)` |
| ConfString | label + `Text` (BORDER; width `wHint` if ≠ −1 else fill) | every edit → `stringValue`, `modified()`. For `FILE`/`DIR` types a 3rd column **browse** button opens `FileDialog`/`DirectoryDialog` with filter path = current value; a non-empty result is stored. |
| ConfAction | label + one `Link` per action (`<a>name</a>`, bigger bold font) | click → `consumer.accept(name)`, `rebuildOpts(false)`, `modified()` |
| other | label + "'<field>' not implemented!" | |

### 4.7 Buttons
- **Defaults**: `confirmYesNo("This will reset all of the options to their default!\nAre you sure?")` → `restoreDefaultConfVals()` (all groups), `modified()`, rebuild.
- **Apply**: `errors = verifyNumbers()` — for every ConfInteger/ConfDouble with a non-null `uiScratch`, parse it into the value; a `NumberFormatException` produces `"Invalid number! " + message`. If none, `errors = config.apply(originals)`. If errors remain → `rebuildOpts(false)` (errors shown inline, dialog stays open). Else `rc = true` and dispose.
- **Cancel**: if Apply is enabled (something changed) → `confirmYesNo("Do you want to close without applying changes?")`, No keeps the dialog; otherwise dispose (→ originals restored, returns false).
- `modified()` just enables Apply.

---

## 5. Command line (`CLIOpts` + `Budget.main`)

### 5.1 `CLIOpts` — a getopt/getopt_long clone
Constructor `CLIOpts(progname, argv)`; state `optind` (0), `in_elm_pos` (1), `optopt`, `optarg`, `longind`, `lastOption`. Static `opterr = true` (print errors), `is_debugging = false`.

`getoptLong(optstring, Opt[] longopts)` — returns the option char, `'?'` (63) on error, `':'` (58) for a missing argument when the optstring starts with `:`, `-1` at end:
- optstring prefixes: leading `+` is stripped (no effect); leading `-` makes every non-option argument be returned as char `1` with `optarg` = the argument; leading `:` switches the missing-argument code to `':'`.
- At the start of an element: past the end → −1 (`lastOption = "��"`). Empty element or not starting with `-` → −1 (or char 1 in `-` mode). Exactly `"-"` → `optopt='-'`, −1 (not consumed). `"--"` → consumed, −1.
- `--name` / `--name=value`: looked up in `longopts` by name (`longind` set). Unknown → stderr `<prog>: illegal option -- '--name'`, returns `'?'`. `no_argment` option given `=value` → `no argument allowed for -- '--name'`, `'?'`. `required_argument`: value after `=`, else the next argv element (none → `option requires an argument -- '--name'`, returns `'?'`/`':'`). The element(s) are consumed and the Opt's callback (`OptProcArg.proc.accept(optarg)` / `OptProcNoArg.proc.accept(name)`) is invoked; an exception in the callback prints a stack trace and returns `'?'`.
- Short options (`-abc`): char at `in_elm_pos`; `lastOption = "-x"`. Not in optstring → `illegal option -- '-x'`, `'?'`, and the **rest of the element becomes `optarg`** and the element is consumed. If the optstring has `x:` the argument is the rest of the element, else the next argv element (none → `option requires an argument`, `'?'`/`':'`). Otherwise, if more chars follow, `in_elm_pos++` and return (bundled flags); else advance to the next element.
- `getopt(optstring)` = `getoptLong(optstring, null)`. Accessors: `optarg()`, `optopt()`, `option()` (last option text such as `-c` or `--name`), `optind()`, `longindex()`, `getArgs()` (remaining argv from `optind`).
- `argsFromFile(File)`: all lines of the file **that start with `#`** are kept (the filter looks inverted — probably a bug (verified against regenerated pseudo code): `filter(line -> line.startsWith("#"))`; unused by Budget).
- Declarative layer (unused by Budget): `parse(prog, argv, requiredSet, repeatableSet, Opt...)` rejects duplicate option names, non-Proc Opts, repeated non-repeatable options ("a command line option '%s' is not repeatable."), any parse error ("illegal command line"), and missing required options unless `--help` was given. `parseInto(obj, prog, argv)` builds long options from the object's public fields (String `=<string>`, boolean flag, int, double, String[] repeatable append, enum via `valueOf`, or custom `setX(String)`/`setX()` setters), adds `--help` (prints `printHelpMessage` and `System.exit(0)` unless `is_debugging`), honours `@REQUIRED`, `@DESCRIPTION`, `@NOTES` annotations; `printHelpMessage` prints `  --%-30s %s` lines with `[default]` or `*REQUIRED*` and per-field descriptions (`@@@ NO DESCRIPTION @@@` placeholder).

Short-option "argument is the next element" branch, as rendered by the regenerated pseudo code: `$t = optind + 1; optind = $t; optarg = argv[$t]` (i.e. `argv[++optind]`), then the common exit does `optind++; in_elm_pos = 1` — both the option element and its argument are consumed. This is standard getopt semantics (argument = next element), consistent with the long-option branch and the documented `-c rcfile` usage. (The earlier reading `argv[optind + 1]` was an artifact of the old generator.)

### 5.2 Budget's actual option handling (`Budget.main`, part03)
Pre-processing: on macOS every argument that starts with `-p` but is not exactly `-p` is removed (the `-psn_…` process serial number Finder passes).

Loop: `getopt("prn:dc:ihbDzBZ:UC:")` (short options only; long options unused). Default `rcFileName = dirHome/.budgetrc` is set **before** parsing so `-c` can override it.

| Option | Effect | In usage text? |
|---|---|---|
| `-r` | `opt_print_current_month = true`: after the UI is built, `saveHTML(getHtmlFileName(), curCal, curCal)` and `Program.launch(html)`, then exit (returns null from the builder). Errors → "Unable to save HTML file\n…". | yes |
| `-d` | `use_dnd = false` (disable drag-and-drop) | yes |
| `-D` | `opt_debug = true` (no `budget-crash.log`) | yes |
| `-z` | `dont_show_zero_amount = true` | yes |
| `-B` | `do_bill_reminder_check = true` (bill reminder check against the data file's mtime at startup) | yes |
| `-Z<n>` | `opt_show_balance = parseInt(leading digits of arg, 0 on failure)`, `opt_show_balance_supplied = true`; 0 = BALANCE_NONE, 1 = BALANCE_ALL (default), 2 = BALANCE_FIRST_AND_LAST | yes |
| `-c rcfile` | `rcFileName = optarg` | yes |
| `-C year,name` | split on `,`; must be 2 parts (`Budget.ASSERT(..., "bad command line for '-C year,name'")`); `year = parseInt`, `name`; later `cut_off_history(fileName, db, year, name)` — process the current data file, remove entries before `year`, save as `name`. | yes (EXPERIMENTAL) |
| `-U` | `opt_show_url = true` (shows URL-related UI; details elsewhere) | **no** |
| `-i` | `showInfo(mainShell, revision + "\n")` and exit | **no** |
| `-h`, `?` (unknown option), `:` (missing argument) | shows the usage dialog below and exits | yes (`-h` implied) |
| `-p`, `-n <arg>`, `-b` | declared in the optstring but fall into `default:` → `showError("Declared but not handled option '-x'")` and `System.exit(1)` | no |

First remaining non-option argument (`argv[optind]`), if non-blank, is the data file to open (otherwise `cfg.lastFileName`).

Usage dialog text (shown with `showInfo`):
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

Startup sequence after option parsing (for context): `loadRc(rcFileName)` → single-instance check (`singleInstance` && `isAppAlreadyRunning(port)` → "Budget Calendar is already running.", exit 1) → licence (`licAgree`) → Mac one-button-mouse question (`firstRun`) → jpackage/MSI check (`firstRunJPACKAGE`) → trial check (`is_trial_version`, forces `unlimitedHistory` off) → UI.

---

## 6. `FileOps`

| Method | Behaviour |
|---|---|
| `getFileData(path, unused)` | Reads the file line by line and concatenates lines **without separators**; on error prints a stack trace; returns the **path string itself** if the result is empty. |
| `copyFile(StyledTextStream log, src, dst)` | Channel copy with a 100 KB direct buffer. Failures append messages to `log`: `<dst>: Unable to open: <msg>`, `<dst>: Write failed` (short write; dst deleted), `<dst>: Unable to copy: <msg>` (dst deleted). Returns success. |
| `copyFile(src, dst)` | Creates `dst` if missing, `transferFrom` whole file; returns false on IOException (stack trace) or if closing a stream fails. Used by `Config.saveProperties` for the `.bak`. |
| `compareFiles(a, b)` | Reads both in 256 KB chunks; true only if both reach EOF together and every chunk is byte-equal; false on any exception (`Unable to compare to dst: …`). |
| `createCopyBackup(file, suffix, Consumer<IOException> onError)` | `assert suffix != null && !blank`; copies `file` → `new File(file.getAbsoluteFile() + suffix)` with `REPLACE_EXISTING`; on IOException calls `onError` and returns false. **No rotation, no numbering** — it is a single overwrite-in-place copy named `<absolute path><suffix>`. **No caller exists in the pseudo code** (BudgetDB has its own `copyFile`); the data-file backup scheme is in BudgetDB (other analysis). |
| `close_stream(obj)` | Closes a FileInputStream/FileOutputStream; failure → stderr `Failed to close a stream:…`. |

## 7. `UnicodishBufferedWriter extends BufferedWriter`

- `write(String)` escapes and writes: every char with code ≥ 128, and also `','` (44), `'\n'` (10), `'\r'` (13), becomes `&#xHHHH;` (exactly 4 lowercase hex digits, 8 output chars); every other char is written as is. So the escaped stream is pure 7-bit ASCII with no commas/newlines — safe for the comma-separated data format and single-line records.
- `writeRaw(String)` writes verbatim (used for HTML boilerplate and `"\n"` record terminators).
- `printf(fmt, args...)` = `write(String.format(...))` (escaped).
- `str2unicodish(String) → char[]` (static; also used by BudgetDB and the Budget HTML/CSV exporters) and `char2unicodish(c, buf, pos)` (returns 1 or 8 = chars produced).
- `unicodish2str(String)` reverses: whenever at least 8 chars remain and the window is `&#x????;` (positions 0–2 = `&#x`, position 7 = `;`) the 4 hex chars are parsed to a char (invalid hex → `'?'`), consuming 8 chars; otherwise one char is copied. Sequences shorter than 8 chars from the end are left literal.
- `td()` is an empty no-op.
- Encoding: the writer wraps whatever `Writer` it is given — Budget always passes `new FileWriter(path)`, i.e. the **platform default charset**; since `write()` output is ASCII-only, the only charset-dependent bytes are those passed through `writeRaw`.

## 8. `Platform`, `AppResources`, `JarVerifier`

- **`Platform`**: plain enum `WINDOWS_x64`, `MAC_ARM64`, `MAC_x64`. No logic; no references found elsewhere in the pseudo code (Budget uses its own `runningOnMac/Linux/Windows` flags).
- **`AppResources`**:
  - `getJarFile(Class)` = file of the class's code source (`ProtectionDomain.getCodeSource().getLocation().toURI().getPath()`).
  - `getJarLocation(Class)` = parent directory of that file, cached per class; on any exception prints the trace and returns `new File(".")`. (This is how `Budget.jar_loc` / jpackage detection — `jar_loc.endsWith("app")` — is derived.)
  - `getJarResource(JarFile, path)`: path backslashes normalised to `/`, then the first non-directory entry whose name matches **case-insensitively**; `FileNotFoundException(path)` if none. `getJarResourceAllBytes` reads it fully.
  - `getSVNRevision(Class)`: reflectively reads `public static String revision` of class `<package>.svnRevision`; `"??"` on failure (prints trace). This produces the `Budget.revision` string shown by `-i` and the title bar.
- **`JarVerifier`**:
  - `getX509Certificate(byte[])` parses an X.509 certificate.
  - `verify(File jar, X509Certificate trusted)`: opens the jar; **no manifest → `SecurityException("The provider is not signed")`**; reads every non-directory entry to the end (forces the JarFile signature check, which itself throws `SecurityException` on a tampered entry); then for each entry: no certificates → allowed only for names under `META-INF/`, otherwise `SecurityException("The provider contains unsigned file:<name>")`; with certificates, the array is split into chains (`getAChain`: consecutive certs where `cert[i+1].subject == cert[i].issuer`) and at least one chain's **first (leaf) certificate must `equals()` the trusted certificate**, else `SecurityException("The provider is not signed by a trusted signer:<name>")`. Returns normally on success.
  - **No caller anywhere in the pseudo code** (the only references to `JarVerifier` are inside its own file). It is dead code in 1.11.4424 and is not part of the registration/trial logic (that is `Budget.isValidKey` + `regkey`). Nothing happens on startup because of it.

---

## 9. Notes for the Rust port
- Keep the property keys exactly as the field names above; keep the colour format `rrggbb` (lowercase, no `#`) and booleans `true`/`false` (accept `1` on read).
- Font values are SWT `FontData` strings; a port must either parse that format (`version|name|height|style|platform…`) for backward compatibility or treat unparsable values as "use default".
- Theme application is a one-shot copy of 35 colour values, not a persisted "theme" setting.
- Group collapsed flags (`ui`,`fnt`,`clr`,`grc`,`adv`) are persisted in the same file.
- Cancel semantics: all ConfVals revert to the snapshot taken when the dialog was constructed; Apply validates numbers first, then `apply()`, and leaves the dialog open on error.

## 10. UNKNOWN / unresolved
1. **`graphType`** semantics (default 2) — used by the forecast graph; not analysed here.
2. **`-U` (`opt_show_url`)** and **`-i`** effects beyond setting the flag / showing the revision — covered by the UI analysis, not here.
3. Exact `FontData.toString()` layout per platform is SWT-defined (not in the pseudo code); the Windows example above is from SWT knowledge, not from the dump.
4. `ConfigEditor.flags` constructor argument — stored, never read.
5. `CLIOpts.argsFromFile` keeps only lines starting with `#` (looks inverted — verified against regenerated pseudo code); unused by Budget so irrelevant to the port.
6. `lambda$open$0` passes the content **width** to `sc.setMinHeight` — upstream bug (verified against regenerated pseudo code); effect on the real dialog not verified.
7. The 5 ignored legacy keys (`graphPastBarColor`, `graphExtraMonthBarColor`, `graphMinBarColor`, `graphAvgLineColor`, `detailsStatsOnDesc`) — original meaning unknown; they are simply dropped.

Verified pass: 2026-10-27 — 2 statements corrected, 2 confirmed.
