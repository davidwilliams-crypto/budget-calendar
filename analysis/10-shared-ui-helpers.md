# 10 – Shared UI helpers (`ca.mishell.ui.*`)

Source: `reference/pseudo/ca.mishell.ui.{MiSWT,Prompt,QuickPicker,QuickPickDialog,NotifierDialog,StyledTextStream,StyledTextSpelling,HunspellSpeller,StyledTextUndo,TableKeyNavigation,GridDataBuilder}.txt`.

## 0. What Budget actually uses from this package (read this first)

Budget carries **private copies** of most of these dialogs inside `ca.mishell.app.budget.Budget` (`pickDate`/`fillPickDate`, `editNotes`, `prompt`, `confirmYesNo`, `confirmYesNoCancel`, `centerShell`, `getButtonDim`, `sdate`). Those copies are documented with the Budget class; the library versions below are the *templates* they were derived from, and differ in details noted in each section.

Direct references from `Budget.part0*.txt` to the library (grep):

| Library symbol | Used by Budget? | Where |
|---|---|---|
| `MiSWT.fadeIn` / `fadeOut` | yes | part01 ~L290 (main shell) |
| `MiSWT.selectChoice` | yes | part05 L248 (drag/drop conflict: "The selected day already has a transaction like this…") |
| `MiSWT.getDarkerColor` | yes | part05 L784 (stats popup "Actions:" text) |
| `MiSWT.setRedraw`, `clear_composite`, `calcWidth` | yes | misc |
| `StyledTextStream` (+ `$Span`) | yes | part05 `showStatsPopUp` (transaction stats popup, plain spans only); `ConfigEditor.createDesc` (`setHTML` for option labels that contain `<…>` tags) |
| `GridDataBuilder` | ConfigEditor only | |
| `MiSWT.pickDate`, `MiSWT.prompt`, `MiSWT.editNotes`, `Prompt`, `QuickPicker`, `QuickPickDialog`, `NotifierDialog`, `StyledTextSpelling`, `HunspellSpeller`, `StyledTextUndo`, `TableKeyNavigation` | **no** | not referenced anywhere in the Budget pseudo code (`grep Spell Budget.part0*` → 0 hits; `QuickPick` hits in Budget are its own `updateQuickPicks`/`lastQuickPicks` button-row feature, unrelated to `QuickPicker`) |

Consequence for the Rust port: only §2 (message boxes), §9 (selectChoice), §11–§14 (fonts, colors, dates, fade) and §16 (StyledTextStream subset) are load-bearing. The rest is documented for completeness.

SWT constant decoding used throughout: Shell `67680` = `APPLICATION_MODAL|DIALOG_TRIM(TITLE|CLOSE|BORDER)`; `65568` = `APPLICATION_MODAL|TITLE`; `67632` = `APPLICATION_MODAL|BORDER|TITLE|RESIZE` (no close box); `66800` = `APPLICATION_MODAL|SHELL_TRIM`. Button `16777224` = `PUSH|CENTER`, `8` = `PUSH`, `16` = `RADIO`. GridData `768` = `FILL_HORIZONTAL`, `1808` = `FILL_BOTH`, `256` = `HORIZONTAL_ALIGN_FILL`; `horizontalAlignment = 3` = `END` (right). System colors: 1 WHITE, 2 BLACK, 3 RED, 15 GRAY, 16 DARK_GRAY, 36 LINK_FOREGROUND. Key `262144` = CTRL. MessageBox: 1 ICON_ERROR, 2 ICON_INFORMATION, 4 ICON_QUESTION, 8 ICON_WARNING, 32 OK, 64 YES, 128 NO, 256 CANCEL, 512 ABORT.

All modal dialogs use the same pattern: build shell → `setVisible(true)` → `while(!shell.isDisposed()) if(!display.readAndDispatch()) display.sleep();` → return a holder value (null when closed via the window X / Cancel).

Common button-bar pattern ("std button bar"): a `Composite` with `GridData{horizontalAlignment=END}` (sometimes `horizontalSpan=2`, sometimes `grabExcessHorizontalSpace`), `RowLayout{pack=false, justify=true, spacing=10}`; every button is sized to the extent of a temporary push button labelled `"--Cancel--"` (`getButtonDim`), so all buttons have equal width.

## 1. `MiSWT.prompt(shell, image, title, message, initialValue, isPassword, validator)` → `String`

Shell: `APPLICATION_MODAL|DIALOG_TRIM`, title, optional image. `GridLayout{marginWidth=10, marginHeight=10, verticalSpacing=10}`.

- `Label(WRAP)` = message.
- `Text(BORDER)` = initial value, `FILL_HORIZONTAL`; if `isPassword` → echo char `*`.
- std button bar: **Ok**, **Cancel**.
- Accept (Enter in the field or Ok): `v = text`; if `validator != null && !validator.apply(v)` → `shell.setFocus()` and stay open; else result = `v`, dispose.
- Cancel / X → result `null`.
- Size = `computeSize`, width ≥ `stringExtent(textFont, "This is a really long enough string to edit???").x`; location `getCenteredPosition(parent, size)` (centred over the parent shell, size clamped to parent bounds). The field gets focus with the whole value selected.

Budget uses its own `Budget.prompt(...)` (has `prompt_was_ctrl_ok`/`prompt_was_alt_ok` flags) – see the Budget doc.

## 2. Message boxes

| Method | MessageBox style | Title | Returns |
|---|---|---|---|
| `showError(shell, msg)` / `showError(msg)` | `ICON_ERROR` (1) | "Error" | – |
| `showError(shell, msg, ex)` | same; message = `msg + "\nError message:" + ex.getMessage() + "\nStack Trace:\n" + trace + "\n"` | "Error" | – |
| `showInfo(shell, msg)` | `ICON_INFORMATION` (2) | "Information" | – |
| `showInfo(shell, title, msg, style)` | `style` | `title` | button code |
| `showWarning(shell, msg)` | `OK|ICON_WARNING` (40) | "Warning" | – |
| `confirmYesNo(shell, msg)` | `YES|NO|ICON_QUESTION` (196) | "Confirmation" | `true` iff YES (64) |
| `confirmYesOrNO(shell, msg)` | `YES|NO|ICON_QUESTION` | "Confirm" | raw code |

Parent shell resolution `getShell(s)`: `s` if non-null, else the display's first shell, else a lazily created hidden `Shell`.

`showErrorMono(shell, msg)`: a `Prompt` (§3) with `NO_EDIT|MULTILINE|MONOSPACED`, title "Error", no message/hint, text foreground RED; only an Ok button.

`showStackTraceError(text)`: `Shell(APPLICATION_MODAL|SHELL_TRIM)` "Error", `StyledText(BORDER|H_SCROLL|V_SCROLL)` read-only `FILL_BOTH`, a "Close" push button, fixed size 800×400, modal loop.

Budget has its own `confirmYesNo` (identical: 196/"Confirmation") and `confirmYesNoCancel` (`YES|NO|CANCEL|ICON_QUESTION` = 452, "Confirmation", returns 64/128/256).

### `dialogYesNoCancelAbort(shell, title, msg, timeoutSec)` → int

Custom shell `APPLICATION_MODAL|TITLE` (title bar but **no close button**, no border). `GridLayout{margins 10, verticalSpacing 10}`; `Label(WRAP)` msg; std button bar (`grabExcessHorizontalSpace=true`) with **Yes** (→ 64) and **No** (→ 128). Default result 256 (CANCEL) if the shell is disposed any other way. If `timeoutSec > 0`: `timerExec(timeoutSec*1000)` → if still open, result = 512 (ABORT) and dispose. Width ≥ `stringExtent(labelFont, "This is a somewhat reasonably long message, with a little gap").x`; placed with `getCenteredPosition(parent)`. Used by `editNotes` ("Note changed. Do you want to save it?").

## 3. `Prompt` class (generic prompt dialog)

`new Prompt(parentShell, image, title, message, hint, initialText, flags)`; `open()` returns `Object` (the accept handler's result; `null` = cancelled). Flags: `PROMPT_PASSWORD=1, MULTILINE=2, SELECTION=4, MONOSPACED=8, NO_EDIT=16, WRAP=32, NO_CANCEL=64`.

Layout (shell `APPLICATION_MODAL|TITLE`, i.e. title bar, no close box, not resizable):
1. `GridLayout`: margins and verticalSpacing = 5 when `NO_EDIT`, else 10.
2. `w = 4 × calcWidth(systemFont, "This is an example")` – the reference width.
3. If `message != null`: `Label(WRAP)` with `widthHint = w`.
4. Input (`FILL_HORIZONTAL`):
   - `SELECTION` → `Combo(BORDER|DROP_DOWN)` with `initialText`; items added later via `setSelectionList(List<String>)`.
   - else `Text` with style `(NO_EDIT ? 0 : BORDER) | (MULTILINE ? MULTI|V_SCROLL : 0) | (WRAP ? WRAP : 0)`, text = `initialText`; `PASSWORD` → echo `*`; `MULTILINE` → `heightHint = 6 × height("X")`; `editable = !NO_EDIT`; monospaced font is applied **only when `NO_EDIT && MONOSPACED`** (quirk: an editable prompt never gets the mono font (verified against regenerated pseudo code)).
5. If `hint != null`: a `ScrolledComposite(V_SCROLL)` containing a `Link(WRAP)`; foreground = `getDarkerColor(shellBg, 0.4)`; if hint does not end with `"\n"` append `"\n "`; font = `MONOSPACED ? changeFontSize(monoFont, -1) : getBiggerFont(-1, ITALIC)` (system font 1pt smaller, italic). Size: `s = computeLinkSize(link)`; `maxW = 0.8 × monitorWidth`; if `s.x > w || s.x > maxW` → `s = link.computeSize(min(s.x, maxW×0.9), DEFAULT)`. If the hint contains `href="` → clicking a link runs `Program.launch(e.text)`. GridData `FILL_HORIZONTAL|GRAB_VERTICAL`, `widthHint = max(w, min(maxW, s.x×1.05))`, `heightHint = min(0.8×monitorHeight, s.y×1.05)`.
6. std button bar (span 2): **Ok**; **Cancel** only when `!NO_CANCEL && !NO_EDIT`.
7. `NO_CANCEL` → a Traverse listener swallows `TRAVERSE_ESCAPE`.

Behaviour: `open()` resets the result, focuses combo/text with everything selected (or the Ok button when not editable), installs a key listener: **Ctrl+Enter** (CTRL as the only modifier, keyCode CR or LF) → `verify()`. Enter in a single-line Text / Combo default-selection / Ok click → `verify()`. `verify()`: `result = acceptHandler.apply(currentText)` (default handler = identity); `null` → `shell.setFocus()` and stay open; otherwise dispose. Shell size = `computeSize` clamped to 95 % of the monitor, `centerLoc(parent, size)`.

## 4. `QuickPicker` (type-to-filter engine) and `QuickPickDialog`

Not used by Budget (see §0) – summarised but complete enough to port.

### 4.1 `QuickPicker.Builder(text, tree)` → `build()`

Options: `setData(DataHandler)`, `setMenuHandler`, `setShowImage(bool, default true)`, `setFilterDelay(ms ≥ 0, default 0)`, `addColumn(header, style, width)`, messages: `setMatchesNotFoundMessage` ("<no matching items found>"), `setTypingHintMessage` ("<type to match items>"), `setTooManyItemsMessage` ("<too many items, showing only some>"), `setMatchingErrorMessage` ("<matching error occurred>").

`DataHandler<T>`: `getMatches(filter, MatchesHandler)` (async-capable; calls `handleResult(filter, ok, Set<T> matches, tooMany, msg)`), `getStrings(T) → String[]` (column texts), `getImage(T)` (default null), `accept(picker, T)`, `accept(picker, Set<T>)` (default: stderr warning + beep), `acceptFuzzy(picker, typedText, lastMatches)` (default beep). `MenuHandler.handleMenu(picker, Set<T> selection)`.

Tree columns created: col 0 LEFT width 1 (dummy); col 1 LEFT width = `calcWidth("WW")` if images shown else 0 (image column); then either one default column LEFT width `calcWidth("Ww"×15)` or the configured columns. If any column has a header → header visible, header background = shell background, header foreground = `isDarkColor(bg) ? (240,240,240) : getDarkerColor(bg, 0.75)`. Item strings are written to columns starting at index 2 (`col = 2; item.setText(col++, s)` (verified against regenerated pseudo code); info items also use column 2).

Matching algorithm: the picker itself does not match – it forwards the filter string to `DataHandler.getMatches`. `request_matches(filter, fromTyping)`: ignored if `filter` equals the last requested filter; else it is appended to a `requested_filters` queue and `getMatches` is called; the result is marshalled with `asyncExec` to `process_result`, after which queue entries up to and including that filter are dropped.

`process_result(filter, ok, set, tooMany, msg, fromTyping)`:
- `!ok` → tree cleared, info item `msg ?? matchingError`, tree disabled.
- `fromTyping` and the text no longer equals `filter` → stale, ignored.
- `last_matches_set = set`; `populate_tree`; if `fromTyping` → `update_text` (auto-complete); text foreground = RED if `set` empty else default.

`populate_tree`: clear; if `tooMany` → info item first; for each item: `TreeItem` (image in col 1, data = item, strings in cols 2…); the first item whose *any* string `equalsIgnoreCase(filter)` is remembered and selected at the end; `top_item` = first real item; `n_matches` = count; tree enabled iff set non-empty; if filter blank and no matches → typing-hint info item (when message non-null); if tree empty → not-found info item; `setTopItem(first)`. Info items: no image, font `getBiggerFont(-1, ITALIC)`, foreground GRAY, text in column 2, `data == null` (so never selectable as a value).

`update_text(filter, set)` (auto-complete, only after typing): if `filter == lastVal && !filter.isEmpty()` the user has just deleted the auto-selected suffix → treat as "remove one more char": `filter = filter[0..len-1]`. `lastVal = filter`. If matches non-empty and filter non-empty: take the **first** match's strings; the first string that `startsWith(filter)` (case-sensitive, then case-insensitive) is put in the field with the completed suffix selected (`setSelection(filter.len, s.len)`); if none matched and this was the backspace case → field = filter with caret at end. Otherwise, if the field length differs from the filter → field = filter, caret at end. Modify listener is detached during these writes.

Modify listener (typing): only when the field has focus and the event time differs from the last one. If `filter_delay == 0` or the modification was not caused by a key press (`key_time != e.time`) → request immediately; else `delay_count++` and `timerExec(delay)` → when the count returns to 0 → request with the current text.

Key handling in the text field (no modifier pressed):
- **Up/Down/PageUp/PageDown**: consumed. Nothing if `n_matches == 0`. No tree selection → select `top_item`. Else `idx = indexOf(selected) + (−1 | +1 | −visibleLines | +visibleLines)` (`visibleLines = max(1, clientHeight / itemHeight)`), in multi mode the current item is deselected first, `idx` clamped to `[top_item_index, top_item_index + n_matches − 1]`, item selected.
- **Enter**: consumed. If requests are pending (queue non-empty or delay pending) → `wait_request_completion()` (re-requests, paints the field background `getAccentedColor(bg, 0.8)` while spinning the event loop; aborts returning false if another key arrives, the field is disposed or the filter changed). If `last_matches_set == null` → beep. If the tree has a selection → `accept_tree_selection()` (1 item → `accept(T)`, >1 → `accept(Set)`); else → `acceptFuzzy(picker, text, last_matches_set)`.
- **Delete** with a selection in the field: consumed; the selected text is removed *without* firing the modify listener; caret at selection start (so deleting the auto-completed suffix does not re-filter).
- Tree: default selection (double-click / Enter) → `accept_tree_selection`; plain selection → focus back to the text field; **Ctrl+A** in a MULTI tree selects all data items; right-click (`MenuDetect`) with a non-empty selection → `menuHandler.handleMenu(picker, selection)`.

Public API: `updateTree()` / `updateTree(filter)` (force re-request), `setFilter(s)` (request + set text), `setText(s)` (silent), `getFilter()`, `getCount()`, `removeListeners()`.

### 4.2 `QuickPickDialog`

`Style` enum: `NO_TREE, NEEDS_TO_MATCH, MULTI_SELECTION, SKIP_INITIAL_SEARCH, DELAY_SEARCH, WIDE_COLUMN, REVERSE_ORDER`. `DEFAULT_PROMPT = "Type to reduce the selection list"`.

Static helpers: `pick(shell, title, initial, Map<String,Image>, styles)`, `pick(shell, title, initial, Set<String>, styles, Consumer<QuickPickDialog> configure)`, `pick(shell, title, initial, map, styles, menuHandler, hintText, BottomLeftButton)` – all return the picked `String` or null.

String data handler (`getStringDataHandler(map, styles)`): matches = keys whose `toUpperCase()` **contains** `filter.toUpperCase()` (substring, case-insensitive), collected in a `TreeSet(CASE_INSENSITIVE_ORDER)` (alphabetical), reversed with `REVERSE_ORDER`; always `ok=true, tooMany=false`. Strings = `{key}`; image = `map.get(key)`. `acceptFuzzy`: with `NEEDS_TO_MATCH` the typed text is accepted only if it is in the match set (else beep); otherwise free text is accepted as typed. `MULTI_SELECTION` with the string handler only prints a warning (not supported).

`pick(initialText, data, menu, hint, bottomLeft)` (may be called once):
- Shell (created in ctor) `APPLICATION_MODAL | (NO_TREE ? 0 : TITLE|RESIZE)`, title. `GridLayout{cols = hint ? 2 : 1, margins 10, spacing 5}`.
- Prompt label (if `prompt_text != null`): DARK_GRAY, `getBiggerFont(-1, ITALIC)`.
- Hint (right column, if given): `ScrolledComposite(V_SCROLL)` + `Label(WRAP)`, foreground `getDarkerColor(bg, 0.4)`, font `changeFontSize(mono, -1)`; width capped at 60 % of the monitor; GridData `FILL_HORIZONTAL|GRAB_VERTICAL`, `widthHint = min(cap, w×1.05)`, `heightHint = min(0.8×monitorH, h×1.05)`, `verticalSpan 3`, `horizontalIndent = calcWidth("w")`, `verticalIndent = indent/2`, `verticalAlignment = BEGINNING`.
- Text field: `Text(BORDER|SEARCH|ICON_CANCEL|ICON_SEARCH)` in a zero-spacing composite, `FILL_HORIZONTAL`.
- `Tree(BORDER|FULL_SELECTION | (MULTI_SELECTION ? MULTI : 0))`, `FILL_BOTH`. Builder: typing hint = `null` (no hint item), custom not-found message if set, `DELAY_SEARCH` → 333 ms, `WIDE_COLUMN` (only when no columns added) → one LEFT column of width `2 × calcWidth("Ww"×15)`, then `addColumn` calls made on the dialog.
- Unless `NO_TREE`: a bottom bar (span 2, FILL, grab) with `GridLayout(bottomLeft ? 2 : 1){marginWidth 0}`: optional bottom-left `Button(PUSH|CENTER)` (`GridData GRAB_HORIZONTAL|HORIZONTAL_ALIGN_BEGINNING`), configured by `bottomLeft.onCreate(button)`; click → `rc = onClick(picker, text)`; `rc != null` → close with it; else if the handler changed the text → `updateTree(text)`. **Cancel** button (`GRAB_HORIZONTAL|HORIZONTAL_ALIGN_END`) → close with null.
- `NO_TREE` → the tree is disposed (text-only prompt with fuzzy accept).
- Size: `computeSize(preferredW, preferredH)`; if `y < 480` and no preferred height and not `NO_TREE` → `y = 480`. Location: explicit `setLocation(x,y)` if both set, else size clamped to 95 % of the monitor and `centerLoc(parentShell, size)`.
- Initial text: `SKIP_INITIAL_SEARCH` → silent `setText(initial)`, select all, `updateTree("")` (list everything); else `text.setText(initial)` (fires the filter) and caret moved to the end.
- Result: `accepted(obj)` stores `rc` and disposes asynchronously; `cancel()` = `accepted(null)`.

## 5. `NotifierDialog.notify(Rectangle area, String msg)` – toast popup

Not referenced by Budget (UNKNOWN whether any Budget feature such as bill reminders uses it – no call site exists in the pseudo code).

- Hidden parent `Shell(NO_TRIM)`. Toast = `Shell(parent, NO_TRIM|ON_TOP|TOOL|NO_FOCUS)`, `FillLayout`, foreground black, `setBackgroundMode(INHERIT_DEFAULT)`; inner `Composite` with `GridLayout(1){margins 5}`; on Resize the background image is repainted: vertical gradient from (247,220,111) to (247,220,111) (i.e. solid pale yellow) and a 2 px border in (178,144,127) inset by 1 px. Content: `CLabel(WRAP)` in **bold** (same family/size), centred, text = `msg`. `pack()`.
- Position: centre of `area` (or the primary monitor's client area if `area == null`). Before showing, every already-active toast is moved up by 80 px (newest first); any toast whose new `y < 80` (i.e. would go above 0) is disposed.
- Animation: alpha starts at 0; every 50 ms `alpha += 50` until it would exceed 225 → alpha = 225; then after 1500 ms fade-out: every 50 ms `alpha −= 20`; at ≤ 0 → dispose. Constants `DISPLAY_TIME=1500, FADE_TIMER=50, FADE_IN_STEP=50, FADE_OUT_STEP=20, FINAL_ALPHA=225`. `_titleFgColor (40,73,97)` is unused.

## 6. Date picker: `MiSWT.pickDate(shell, SimpleCalendar initial[, title])` and `fill_pick_date`

`SimpleCalendar` fields: `1 = DAY`, `2 = MONTH (0-based)`, `3 = YEAR`, `6 = DAY_OF_WEEK (0 = Sunday … 6 = Saturday)`; `getActualMaximum(DAY)` = days in month.

Shell: `APPLICATION_MODAL|DIALOG_TRIM`, title default **"Pick a Date"**. `GridLayout{numColumns=7, makeColumnsEqualWidth=true, marginWidth=10, marginHeight=10, verticalSpacing=2, horizontalSpacing=2}`. `shell.setData("initial", copy of initial)`. After filling, size = `computeSize`, then `height += height/5`; location `centerLoc(parent, size)`. Returns the picked `SimpleCalendar` or `null` if closed.

`fill_pick_date(holder, shell, cal)` (rebuilds everything; redraw suspended on Windows while visible):
1. `cal1 = copy(cal)` with `DAY = 1`; stored as `shell.setData(cal1)` (the "current month" state).
2. Header composite spanning 7 columns, `HORIZONTAL_ALIGN_FILL`, background WHITE, `GridLayout(6)`:
   - `Button(PUSH)` `"  <<  "` tooltip "Previous Year" → `cal.add(MONTH, −12)`, refill.
   - `Button(PUSH)` `"  <  "` tooltip "Previous Month" → `add(MONTH, −1)`.
   - Month button `Button(CENTER)` (default push style), text = `"      " + format("MMMMMMMMMMMMM, yyyy") + "      " + "⋮"` (U+22EE), `FILL_HORIZONTAL` with `widthHint = cal_label_width_hint` (max width over the next 12 months of `"      " + "MMMMMMMMMMMMM,yyyy" + "      "`). Click → popup `Menu` with 12 items (`"January, 2026"` … for the displayed year, same format), choosing one sets `DAY=1, MONTH=i` on the shell calendar and refills.
   - `Button(PUSH)` `"  >  "` tooltip "Next Month" → `+1`; `"  >>  "` "Next Year" → `+12`.
   - `Button(PUSH)` `"Today"` tooltip "Set Today Year and Month" → refill with `new SimpleCalendar()` (today).
3. Seven `Label(CENTER)` headers `"Sun" "Mon" … "Sat"` (first 3 chars of English names, **week always starts on Sunday** in the library version), foreground WHITE, background BLACK, `FILL_HORIZONTAL`.
4. Leading blanks: `dow = cal1.get(DAY_OF_WEEK)` of day 1; `dow` empty `Label`s (`FILL_BOTH`).
5. Day cells 1..daysInMonth: `Label(RIGHT)`, `FILL_BOTH`, text = day number, background `cellBg = getAccentedColor(shellBackground, 0.85)`, foreground `getComplementaryColor(cellBg)`; **today** → foreground RED. Mouse-down on a cell → result = copy of the shell calendar with `DAY = parseInt(label text)` (year/month = displayed month), shell disposed.
6. Trailing blanks: `dow` of the last day; if `dow != 6` add `6 − dow` empty labels.
7. `layout()`.

Budget's private `Budget.fillPickDate` differs: honours `cfg.firstDayOfWeekIsSunday` (Sunday or Monday start, blanks computed from `getFirstDayOfWeek()/getLastDayOfWeek()`), button texts `"<<" "<" ">" ">>" "Today"` without padding, month button text without padding/`⋮` and with `cfg.colorCurrentMonth` / `cfg.bgColorWindow`, weekday labels use `cfg.colorDatePickerHeader`/`bgColorDatePickerHeader`/`fontDate`, day cells `bgColorCell` (or `bgColorCellPast` when before `initCal`), foreground `colorCalendarDay`, the **initial** date (not today) gets `colorCalendarToday`, blank cells `bgColorCellOtherMonths`, dialog icon = budget icon, result stored in `pickDateCal`. Everything else (layout, sizes, navigation, click semantics) matches.

## 7. Notes editor: `MiSWT.editNotes(shell, image, title, text, editable[, EditNotesAttrs])` → `String`

`EditNotesAttrs`: `setFont(Font)`, `setTextToSelect(String)`, `setupStyledText(Consumer<EditNotesContext{notesShell, text, spelling, rv, lowerLeftComposite}>)`.

- Shell **without parent**: `Shell(APPLICATION_MODAL|BORDER|TITLE|RESIZE)` (resizable, no close box), title, image. `GridLayout{1 col, margins 10, spacing 5}`.
- `StyledText(BORDER|MULTI|WRAP|V_SCROLL)`, `editable`, tabs = 8, optional font, `Ctrl+A` bound to select-all. If `HunspellSpeller.getInstance() != null && editable` → `StyledTextSpelling` attached (§17). `StyledTextUndo.addTo(text)` (Ctrl+Z/Ctrl+Y). Text = `text ?? ""`. `FILL_BOTH` with `heightHint = 18 × height("This is about 10")`, `widthHint = 6 × width("This is about 10")`.
- If `textToSelect` given and found (case-insensitive `indexOf`) → caret + selection on it, `showSelection()`.
- Lower composite (`GridData(FILL, TOP, grabH, !grabV)`, `GridLayout(2)`): the `setup` consumer may add widgets; if it added none → `Label` "Quick save: Ctrl+S or Ctrl+Enter" (empty when not editable), LEFT/TOP, foreground GRAY. Then std button bar: **Ok** (default button) → result = `text.getText().trim()`, dispose; **Cancel** → cancel handler.
- Cancel handler: if original `text != null` and differs from the current content → `dialogYesNoCancelAbort(shell, "⚠ Warning", "Note changed. Do you want to save it?", 0)`: YES → result = trimmed text and close; NO → close without result; anything else → stay. If unchanged or original null → close (result null).
- **Esc** (Traverse char 27) → cancel handler. Display-level KeyDown filter: **Ctrl+Enter** or **Ctrl+S** → result = trimmed text, close (filter removed after the loop).
- Zoom handler (`attachTextZoomHandler`): **Ctrl+B** inserts `" •\t"` and moves the caret +3; **Ctrl+=** → font +1 pt; **Ctrl+−** → −1 pt; **Ctrl+wheel** up/down → ±1 pt (style kept, `getFont(f, -1, ±1)`).
- Size `computeSize`, `centerLoc(parentOrNull, size)`. Returns null when cancelled.

Budget's own `Budget.editNotes(shell)` ("Budget Calendar Notes", plain `Text(BORDER|MULTI|H_SCROLL|V_SCROLL)`, tooltip "Use ^Enter or ^S for quick save.", edits `db.notes`) is a different, simpler dialog – see the Budget doc.

## 8. `selectImage(shell, dirPath)` → file name or null

Shell `APPLICATION_MODAL|DIALOG_TRIM` "Select an image...". Files of `dirPath` (`File.listFiles`, `Arrays.sort` = path order; null → empty). `GridLayout{numColumns = (int)sqrt(n) + 1, margins 10, spacing 6}`. For every non-directory entry an `Image` is loaded from `dir + sep + name` and shown in a `Label(FLAT)` with `data = name`; load failures are skipped silently. Mouse-down on a label → result = file **name** (not path), dispose. `centerLoc(parent)`.

## 9. `selectChoice(shell, image, title, msg, Map<K,String> choices, K default)` → K or null

Shell `APPLICATION_MODAL|DIALOG_TRIM`, image, title; `GridLayout{1 col, margins 10, spacing 10}`; `Label` msg; one `Button(RADIO)` per map entry in iteration order (text = value, data = key, selected iff `key == default` by reference); std button bar (span 2): **Accept** (default button) → result = data of the selected radio, dispose; **Cancel** → null. `pack()`, `centerLoc(parent, size)`, `open()`. Budget: drag-and-drop conflict dialog, map of `Budget$TransactionDND` → label, default `REPLACE`.

## 10. `licenseAgreement(title, text, filePath)` → boolean

If `text == null` it is read from `filePath` line by line (joined with `"\n"`); on any exception `text = "Unable to open '<path>'\n\nPlease find a way to read the license terms.\nDo you agree with the terms?\n"`. Blank text → `showError("Unable to display license")`, return false. Shell `APPLICATION_MODAL|DIALOG_TRIM`, title, icon `getImage('©')`; `GridLayout{1 col, margins 10, spacing 5}`; `Text(BORDER|MULTI|WRAP|V_SCROLL)` read-only `FILL_BOTH`; std button bar (span 2): **I agree** (default) → true, **Cancel** → false. Fixed 640×480, centred on the display.

## 11. Font helpers

| Method | Result |
|---|---|
| `getFont(Font f, int style, float scale)` | `new Font(name, (int)(height × scale), style == −1 ? f.style : style)` (truncation, no minimum) |
| `getFont(Font f, int style, int delta)` | `new Font(name, height + delta, style == −1 ? f.style : style)` |
| `getScaledFont(Font f, float scale)` | `new Font(f.device, name, max(1, (int)(height × scale)), f.style)` |
| `getBiggerFont(int delta, int style)` | system font family, `height + delta`, given style (0 normal, 1 bold, 2 italic) |
| `changeFontSize(Font f, int delta)` | every `FontData.height += delta`, `new Font(device, fontData[])` |
| `getMonospacedFont()` | cached: `"Monaco"` on macOS, `"Consolas"` elsewhere, system font height, NORMAL |
| `isMonoFont(f)` | `charWidth('i') == charWidth('w')` |
| `calcWidth(f, s)` / `stringExtent(f, s)` | via a static `GC` on the display |
| `getButtonDim(parent, text)` / `(parent, n)` | size of a temporary `PUSH|CENTER` button with `text` / with `"We".repeat(n/2 + 1)` |

## 12. Colour helpers (exact formulas; components are 0..255 ints, `d ∈ [0,1]` else `IllegalArgumentException("d is out of [0..1]: d")`)

- `isDarkColor(c)`: `if (r < 10 && g < 10) return true;` (blue ignored!) else `brightness(HSB) < 0.7` where SWT brightness = `max(r,g,b)/255`.
- `isLightColor(c) = !isDarkColor(c)`.
- `getDarkerColor(c, d)`: `((int)(r×d), (int)(g×d), (int)(b×d))`.
- `getLigherColor(c, d)`: `min(255, (int)(r + r×d))` per channel (black stays black).
- `getAccentedColor(c, d)`: light → `getDarkerColor(c, d)`; dark → `getLigherColor(c, 1 − d)`.
- `getComplementaryColor(c)`: `(255−r, 255−g, 255−b)`.
- `color2hex(c)`: `"#%02x%02x%02x"`.

## 13. Date strings

- `sdate(int yyyymmdd)`: `SimpleDateFormat("dd-MMM-yyyy")` in the default locale, then **all `.` characters removed** (locales whose short month names carry a dot) → e.g. `"05-Jan-2026"`.
- `sdateMonth(int)`: `SimpleDateFormat("MMMMMMMMMMMM")` = full month name (`"January"`).
- (Budget has its own `Budget.sdate` / `sdate_MY`; the date picker uses `"MMMMMMMMMMMMM, yyyy"` → `"January, 2026"`.)

## 14. `fadeIn(shell)` / `fadeOut(shell)`

`doFade(shell, alphas, 100)`: for each alpha: `Thread.sleep(100)` (UI thread blocks), `shell.setAlpha((int)(255 × a))`, drain pending events with `readAndDispatch` until none, abort if disposed. `fadeOut` alphas `.95 .9 .8 .6 .3 0`; `fadeIn` alphas `.2 .5 .9 1.0` (never explicitly 0 first – caller sets it). Total ≈ 600 ms / 400 ms.

## 15. Misc geometry / images

- `getImage(char ch)` → 48×48 32-bit ARGB `Image` (palette masks R `0x0000FF00`, G `0x00FF0000`, B `0xFF000000`, `transparentPixel = 0`): fully transparent fill, a semi-transparent grey disc `(128,128,128,α128)` covering the whole 48×48, then `ch` in white: start from the system font and grow it 1 pt at a time while `stringExtent(ch).y < 48`, then shrink 1 pt once; drawn transparently at `x = (48 − w)/2`, `y = (−leading/2) + (48 − (fontHeight − leading))/2`. Used for the `©` licence icon.
- `computeLinkSize(Link)`: `s = link.computeSize()`; for each text line `s.x = max(s.x, stringExtent(font, line).x)`; `s.y = max(s.y, Σ(lineHeight + lineHeight*15/100))` (integer arithmetic; `sum[0] += extent.y + extent.y*15/100` (verified against regenerated pseudo code)).
- `centerLoc(Composite parent, Point size)`: reference rect = `parent.bounds` if parent non-null and visible; else the display bounds if there is one monitor; else the bounds of the monitor containing the cursor (fallback primary). Returns `(max(rx, rx + (rw − w)/2), max(ry, ry + (rh − h)/2))`.
- `centerLoc(Point size)`: display bounds, size clamped to it, centred.
- `getCenteredPosition(Shell parent, Point size)`: **mutates** `size` (clamped to the parent's bounds), returns the centre position over the parent's bounds.
- `fixShellLocation(display, size, loc)`: display bounds inset by 50 px on every side; if `Rectangle(loc, size)` does not intersect it → `centerLoc(size)`, else `loc` unchanged.
- `centerShell(shell)`: computeSize, `centerLoc(null, size)`, set, show, focus.
- `setRedraw(control, b)`: only effective on Windows. `clear_composite(c, from)`: disposes children from index `from`. `isEmpty(s)`: null or blank.

## 16. `StyledTextStream` (rich text over `StyledText`) – summary

Wraps an `org.eclipse.swt.custom.StyledText` (either an existing one or a new read-only one with no caret, double-click disabled, arrow cursor, background inherited). `Span` = text run with optional `font`, `foreground`, `background` (also `int 0xRRGGBB` setters), `fontStyle` (1 BOLD, 2 ITALIC), `underline`, `href`, `toolTipText`, click handlers (`consumer` = left click, `doubleClickConsumer`, `mouseUpConsumer`, `mouseDownConsumer`), `menu` (right click), `data`, `cursor`, HTML extras (`addHTMLStyle`, `addHTMLAttr`, `noHTML`). `isLink()` = underline + `COLOR_LINK_FOREGROUND`.

Adding: `add(Span)` appends the text, applies a `StyleRange`, auto-scrolls to the last line (`setAutoScroll`), appends HTML if collecting, fires `setLogChangedListener`. Convenience: `add(String)`, `add(int fontStyle, String)`, `add(Color, String)`, `add(Span template, String)` (template's style, temporary text), `addError(s)` (RED), `addLink(text, consumer)`, `nl()`, `addSpaces(n)`, `addCamelCase(double)` (format `#,##0.00`; integer part normal, the last 3 chars `.dd` in a 0.85-scaled font – Budget's stats popup does the same by hand with `decimalsFont`), `setHTML(s)` (clear + `addHTML`), `addHTML(s)`.

Interactivity: a span that has any handler, menu or tooltip gets a `StyleRange.data` copy and lazily installs mouse listeners on the first such span: hand cursor over action spans; balloon `ToolTip` (`SWT.BALLOON`) shown on hover (`toolTipText`, defaulting to `href`) unless a click happened < 1.5 s ago; left click → `consumer`, right click → `menu.setVisible(true)`, otherwise `mouseUpConsumer`; `mouseDownConsumer`; double click → fires a MouseUp notification then `doubleClickConsumer`. Handlers receive `SpanEvent{e, text (href or span text), data, span}`. `href` spans without their own consumer get the default consumer: if a `setSelectionListener` is registered → `widgetSelected(SelectionEvent{text = href, stateMask})`; else `Program.launch(href)` when no modifier is held. `setEnabled(false)` greys (GRAY) action spans.

Tables (`getTableBuilder()` → `addCell(colSpan, rowSpan=1, LEFT|RIGHT|CENTER)`, `addCell()`, `nextRow()`, `setMinimumWidth`, `setPreferredMaxWidth`, `addHTMLStyle`; `addTable(builder)` → `Table`): cells are rendered as one text line per row using **tab stops**: every cell's spans are normalised (non-LEFT cells get a leading `\t`, every cell a trailing `\t`), cells are separated by a `" "` span, rows by `"\n"`; column widths are measured with a GC/scratch `StyledText` (`padding` = width of a space, `min_width` honoured, spanned cells distribute their excess across their columns, `preferredMaxWidth`/`minWidth` adjust the last column); per-line tab stops: for RIGHT cells one stop at `right − cellWidth`, for CENTER at `left + (colWidth − cellWidth)/2`, plus one at each column's right edge. Cells wider than their column are truncated with `…` via `setOverflowSpan` / `fit_span` (shrinks by 3 chars per try); the dropped spans' `data` are kept in `cell.invisible_data`. `Cell.update(spans)` / `updateText(s)` / `Span.updateText(s)` re-render a cell in place (shifts later action ranges/tables, re-tabs). `setVIndent` → `setLineVerticalIndent`. Not used by Budget.

### 16.1 Mini-HTML subset (`html2spans`, used by `setHTML/addHTML`)

Pre-pass (case-insensitive): `<big>` and `</small>` → `<+>`; `</big>` and `<small>` → `<->`.

Per-character state: flags `BOLD=32, UNDERLINE=64, ITALIC=128, LINK=256` plus a signed 5-bit size level in bits 0–4; plus the current `href`. Tags recognised only when at least 3 more characters follow the `<`:

| Markup | Effect |
|---|---|
| `<b>` … `</b>` | bold |
| `<i>` … `</i>` | italic |
| `<u>` … `</u>` | underline |
| `<+>` / `<big>` / `</small>` | size level +1 |
| `<->` / `<small>` / `</big>` | size level −1 (closing `</+>` / `</->` invert the sign) |
| `<a href='url'>text</a>` or `<a href="url">…</a>` or `<a>text</a>` | `text` becomes a link: underline, LINK colour, `href = url` (or null), default href consumer. Only `href=` / `HREF=` immediately after `<a` (spaces allowed) are parsed; the run must be closed by `</a>`/`</A>`; malformed anchors are dropped |
| `&lt;` / `&gt;` | literal `<` / `>` — but the parser skips only 3 of the 4 characters, so the trailing `;` is emitted as text (`&lt;` → `<;`) (verified against regenerated pseudo code); other entities are left as text |
| anything else | not supported: a one-letter unknown tag `<x>` is silently dropped; **`<br>` is not a tag** (renders as the text `r>` — `<`, `b` consumed as a tag, then `r`, `>` emitted literally (verified against regenerated pseudo code)); line breaks must be literal `\n` |

Consecutive characters with identical (flags, href) are merged into one `Span`. Size level `k` (−3..+4 effective) → `font = getScaledFont(textFont, fontRatio[clamp(3 + k, 0, 7)])` with `fontRatio = {0.5, 0.65, 0.8, 1.0, 1.15, 1.25, 1.5, 1.75}`. (Index handling (verified against regenerated pseudo code): after `<` the index is advanced past an optional `/`, the tag letter is read with a post-increment, and the `>` check is made at the following position; the size level is the sign-extended low 5 bits of the flag word.)

ConfigEditor relies on this for option labels that contain `<…>` (e.g. links in a setting's label) and routes link clicks through `setSelectionListener`.

### 16.2 HTML collection (`setCollectHTML(true)`, `getHTML()`, `clearHTML()`, `onFirstAdd(Runnable)`)

Each added span (unless `noHTML()`) is appended as HTML: `"\n"` alone → `<br>\n`; otherwise `<span style='font-size:X.XXrem;[font-family:monospace;]background-color:#rrggbb;color:#rrggbb;font-style:italic;font-weight:bold;text-decoration:underline;<custom styles>' attr='v'…>` (`color` omitted for spans with a consumer or underline; an empty style collapses to `<span>`), optional `<a href='…'>`, text with `\n` → `<br>\n` and `<` → `&lt;`, `</a></span>`. Single quotes are encoded `&#39;`. Tables emit `<table [style='…']><tr>\n<td align=left|right|center [bgcolor=#rrggbb] [style='…']>spans\n…</table>` (the `<td` literal is lost in the pseudo code – inferred). `onFirstAdd` runs once before the first HTML append.

## 17. Brief descriptions

**`StyledTextSpelling`** – live spell checking for a `StyledText`: a `LineStyleListener` underlines misspelled words (word pattern `(\w|')+`) with a red squiggle (`UNDERLINE_SQUIGGLE`, `COLOR_RED`). Words that are numbers, ALL CAPS, CamelCase/mixed with digits, or contain `_`/`$` are skipped; an ignore list is kept. Lookups run on a background `SpellerThread` shared per `Speller` instance, results cached case-insensitively; the control is redrawn at most every 333 ms. Right-click on a misspelled word (or **Ctrl+Space** at the caret) opens a menu: suggestions (max 12, first ten prefixed `&1 `…`&9 `, `&0 `; exactly one suggestion is applied immediately without a menu; none → disabled "no suggestions"), separator, "&Add to dictionary", "&Ignore". A `CustomSelector` can supply the suggestion set for Ctrl+Space. **Not used by Budget** – only by `MiSWT.editNotes`, which Budget does not call.

**`HunspellSpeller`** – `Speller` implementation that pipes words to an external `hunspell -a -d <lang> -p <personalDict>` process (default command `hunspell`, language `en_US`, personal dictionary `~/userdict`; on Windows `setHunspellCommand(null)` probes `c:/msys64/{mingw64,ucrt64,mingw32}/bin/hunspell.exe`). Parses ispell-style replies: `*` ok, `#` unknown without suggestions (empty set), `& word n off: s1, s2…` → ordered suggestion set. `addWord` sends `*word` + `#` (add to personal dictionary and save). `getInstance()` is a singleton (initialisation errors are reported via `getSpellerInfo()`). **Confirmed not used by Budget** (no `Spell` in `Budget.part0*`).

**`StyledTextUndo`** – undo/redo for `StyledText` via `ExtendedModifyListener`: **Ctrl+Z** undo, **Ctrl+Y** redo (selection set to the restored range). Consecutive single-character insertions are merged into one change while they stay in the same whitespace/non-whitespace class; adjacent single-character deletions (backspace or delete) merge likewise. Memory budget `DEFAULT_UNDO = 102400` characters (`UNLIMITED_UNDO = −1`); oldest changes are dropped beyond it (`isPristine()` is then false). Used by `MiSWT.editNotes` only.

**`TableKeyNavigation`** – type-ahead search for an SWT `Table`: unmodified alphabetic keys accumulate into a buffer (reset after 1 s of inactivity or on Esc); after each key the rows from the current selection (+1 when the buffer has one char) onward are scanned for any column whose text contains the buffer case-insensitively; the first hit is selected and shown, otherwise the display beeps and the key is not added. Not referenced by Budget.

**`GridDataBuilder`** – fluent `GridData` builder: `wHint, hHint, hSpan, vSpan, hAlign, vAlign, vGrab, hGrab, hIndent, vIndent, build()`. Used by `ConfigEditor`.

## UNKNOWN

1. `add_html_table`: the literal before `align=` is lost (`<?stack-underflow?>.append("left"|"right"|"center")` — still unresolved in the regenerated pseudo code, the operand is lost across the alignment `switch`), assumed `"<td align="`.
2. `dialogYesNoCancelAbort` has a title bar but no close button; whether the 256 (CANCEL) default can actually be returned (e.g. Alt+F4) is platform behaviour, unverified.
3. Whether `NotifierDialog`, `QuickPickDialog`, `Prompt`, `TableKeyNavigation` are used by anything shipped with Budget Calendar – no call sites exist in the Budget pseudo code; they appear to be library leftovers.
4. `fadeIn` never sets alpha 0 first – the caller's prior alpha state (Budget part01 ~L290) decides the visual start.

Verified pass: 2026-10-27 — 5 statements corrected, 4 confirmed.
