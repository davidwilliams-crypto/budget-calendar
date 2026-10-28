# Changelog

## [0.6.0] - 2026-10-28

Deferred review items, reports and speed.

- **Reports menu:** every report from the original (income and expenses print and Excel, comparative, month-by-month, monthly calendar, monthly detailed and Excel, unpaid & paid) plus Print this month, with a range dialog. Reports open in your browser or spreadsheet program and their temporary files are removed when the app closes. Monthly Detailed and Monthly Excel now include the To month, and the HTML Totals row shows the real expenses total.
- **Future changes split the payment:** "This and all future occurrences" ends the existing payment the day before and continues with a new one from the chosen date, so earlier months stay exactly as they were. Editing a whole payment from a calendar date offers "From this date onward" or "All transactions". Month-end, 29 February and fifth-weekday schedules keep their day, and counts, end dates and loan balances carry over.
- **Backups:** only the newest 10 backups are kept beside a budget; numbering keeps rising so a higher number is always newer, and on Windows every spelling of the file name shares one set.
- **The app follows the clock:** a new day is picked up within 20 seconds, even while the app is idle.
- **Keyboard:** Tab reaches the summary tiles and calendar rows with a visible focus ring; Enter or Space opens a focused row, and closing a dialog returns focus to where you were.
- **Search:** the day dialog lists matching transactions first, then the others; typing in search is about four times faster on large budgets.
- **Never frozen by a slow drive:** recent budgets are checked and opened in the background, showing "Checking…" and "Opening…", so an unreachable network share no longer locks the window.
- **Dialogs:** a file dropped while a dialog is open explains why it did not open; Escape closes the day dialog's menu before the dialog, and clicking outside closes it.
- **Calendar:** a payment that continues after a split no longer shows the "Last payment" marker, and browsing the last calendar years no longer runs past 31 December 9999.
- **Speed:** edits update in place instead of rebuilding the whole schedule when the result is the same, the Payments page caches its list, and balance calculation allocates less.
- **Code quality:** the scheduler no longer changes stored data, one schedule walk and one weekend rule replace several copies, shared date limits (the old maximum date did not exist) and shared name tables, dialog state in one place, and zero clippy warnings.

## [0.5.0] - 2026-10-28

Second full review: deeper calculation, file-safety, interaction and polish fixes.

- Calculations: daily and every-2-day business-days payments merge weekend amounts onto Monday instead of dropping them; loan installments shifted off a weekend are no longer skipped or double-counted; brief-summary shares use expenses when a month has no income, and Total income shows its planned-vs-actual difference.
- Tentative payments: marking an occurrence Paid pins it to that date; tentative loans and weekend options are blocked in the editor with an explanation.
- Restore deleted occurrences from the day's right-click menu. Custom amounts are marked "Adjusted" rather than "Moved off a weekend". Adjusted balances show the adjustment, and per-transaction notes show a pencil marker.
- Hovering a payment highlights its grace-period days.
- Files: CR-only line endings, notes and descriptions with leading or trailing spaces, literal `&#x…;` text, control characters, out-of-range dates and colours, legacy encrypted bytes and emoji in preferences all save and reload correctly.
- Saving works again after the budget file was deleted or moved. Save as to the same file under a different spelling still detects outside changes. Relative paths are stored as full paths.
- Preferences are saved when the window closes and when "Show cents" changes; screenshot mode no longer touches your preferences.
- Forms: amounts accept formulas such as `1200/12` or `2.5k`, show two decimals, and leave unchanged legacy values intact. A category equal to the description is rejected. Editing a payment that started in the past warns that earlier months change. The date picker follows the calendar's first weekday.
- Interaction: a double-click no longer lands inside the editor it just opened; the password box keeps focus after a wrong password; typing a year in the month picker no longer jumps the calendar; the day dialog stays open after editing; shortcuts and right-click are ignored mid-drag; closing the window while an Open is pending closes.
- Interface: today pill, repeated balances greyed, regular-weight names with semibold amounts, green maximum-balance strip, full-width status bar, clearer load-error banner, hover-only Edit and a Disabled badge on Payments, roomier dialog fields, a visible Expense/Income switch, a shared dialog footer, and a + hint for adding a payment to a day.
- Code quality: zero clippy warnings, duplicated rounding and dead code removed, deep formula nesting rejected safely.

## [0.4.0] - 2026-10-27

Full review release: calculation, reliability and interface polish.

- Whole-dollar amounts and balances now round the way the original app does, never show "-0", and no longer crash on non-finite values.
- Fixed hangs and crashes from corrupt schedules (zero/negative/huge frequency, huge loans, extreme years); a damaged budget now shows an error instead of closing the app.
- Corrected schedule dates: 5th-weekday repeats fall back to the last matching weekday, weekend-shifted loan installments and early-pay starts are placed correctly, and "today" uses the local date.
- Summary tiles reconcile with opening and closing balances; balance markers (low/min/max) and negative colouring follow the original rules.
- Payment forms: amounts accept thousands commas, `$` and parentheses, start empty and are required, reject absurd values with a clear message; Enter submits and Escape cancels.
- Safer files: emoji and other non-BMP text survive save/load, short passwords match the original encryption (older files still open), failed saves leave no stray backup, option dates reload correctly.
- Command line opens the first file argument and never silently falls back to the last file; `--version` prints in a console.
- Interface: stronger visual hierarchy, accessible contrast in light and dark, stronger today state, clearer Min/Max/Low balance strips, closing-balance hero tile, tooltip cards, calendar legend, shortcuts sheet (F1 or ?), Alt+Left/Right and T navigation, onboarding for empty budgets, and emoji/CJK font fallback.
- Installer no longer overwrites or deletes the user's sample budget and requires an explicit version.

## [0.3.0] - 2026-10-27

First tracked local release. Earlier local updates used the imported 0.2.0 label and are collected here rather than assigned separate historical versions.

- Open and edit existing `.bgt` budgets, including payment creation, updates and deletion, with safe saves and backups.
- Refresh the desktop interface, payment forms and calendar for clearer everyday editing.
- Show the whole month, including multiple payment rows, readable names and amounts, and drag payments between dates.
- Edit balances and change payment amounts from a selected occurrence into the future.
- Show the app version in the title and header, with build details, dated version history and copyable version information.
- Preserve local releases with executable and source archives, build identity and SHA256 records.

## [0.2.0] - Imported baseline

The project was imported with version 0.2.0. Local changes before the first tracked release shared this label; no separate dated releases were recorded.
