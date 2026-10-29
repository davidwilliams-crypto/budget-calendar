# Changelog

## [0.8.3] - 2026-10-29

- **Safe Windows links:** opening a web address uses a Windows URL-opening API without a command interpreter. Shared-budget links accept validated server origins.
- **Independent service limits:** signed-in clients have separate request allowances from anonymous traffic. Explicit trusted Render client identification, bounded request/password work and expiring limiter storage prevent a single caller from consuming the household's shared quota.
- **Modern encrypted files:** password-protected standalone budgets use salted Argon2id key derivation and whole-file AES-256-GCM authentication. Old encrypted budgets still open and upgrade when explicitly saved; existing file backups remain readable. Plain standalone files and shared budgets retain their established formats.

## [0.8.2] - 2026-10-29

- **Choose a household before unlocking:** when your account has more than one household, the Android/web unlock screen now lets you choose which budget to open. A new phone can open the intended budget using its recovery key without first unlocking another household.

## [0.8.1] - 2026-10-29

- **Recover your household key:** File > Shared budget now lets you copy the recovery key for the currently open shared budget after setup. The key stays hidden on screen and is copied only when you choose the button, so you can unlock the same household on another device without creating a new budget.

## [0.8.0] - 2026-10-29

- **Shared household budgets:** Windows can create or open a private shared budget, keep a protected local copy, and synchronize edits with two separate household accounts. Sharing is an explicit action; the original standalone budget file stays separate.
- **Android app:** an installable web app provides Today, upcoming entries, a calendar, a 31-day lowest-balance forecast, quick income and expenses, recurring edits, paid status, balance checkpoints and notes. The Rust calculation and editing engine is shared with Windows.
- **Offline editing and recovery:** pending edits survive normal restart. Retried writes and interrupted budget creation reuse their original identity. Concurrent edits pause for review; recovery exports and retained history help recover work. Windows waits for open editor drafts before applying remote changes.
- **Private service:** separate owner/editor accounts, expiring invitations, membership revocation, encrypted budget contents, and durable revision history. Device recovery keys are required to open encrypted budgets. The deployment package includes the service, Android assets and hosting instructions; this release does not provision hosting or upload any personal budget.
- **Release packaging:** source archives and build fingerprints now include the mobile application, shared engine and service. A separate versioned mobile deployment ZIP records SHA256 checksums and refuses changed content at an existing release version.

## [0.7.2] - 2026-10-28

- **New app identity:** a sculpted emerald calendar and rising cashflow motif, a gold highlighted day, and a midnight-blue tile replace the plain blue calendar icon. The same artwork appears in the app header and native window.
- **Windows icons:** the executable now embeds the icon, so Explorer, desktop shortcuts and file associations display the app's identity. Ten sizes from 16 to 256 pixels support small icons and high-DPI displays; setup and uninstall programs use the matching artwork.

## [0.7.1] - 2026-10-28

Fourth review: wrong figures in reports and balances, emoji in text, safer files, keyboard and calendar contrast.

- **Month-by-Month report:** it no longer refuses with "amounts do NOT match" when one occurrence of an uncategorised payment changed sign (for example an expense of -50 whose November amount became +50). A description used by both an income and an expense payment is counted once, and a description whose income and expense cancel over the range is listed once instead of twice in every income-and-expense table.
- **Rounding dust in cent amounts:** sums of cent amounts can differ from the exact figure by a few quadrillionths (1234.56 - 234.56 - 1000.00 is -0.00000000000006, three amounts of 49.95 add up to 149.85000000000002). That dust is now treated as zero or snapped to the cent where it mattered. A month whose balance does not change is flat again (no Min/Max/Low labels) when a pay day nets to nothing. A weekend-merged amount and the last instalment of a loan are whole cents, so the filter `=-149.85` finds them and the printed calendar no longer marks the last instalment as adjusted. When several uncategorised expenses turn positive, or a description's income and expense cancel exactly, the emptied category stays in the right section instead of showing as income, Month-by-Month no longer refuses with "missing '(Uncategorized-Expenses)'" or prints a percentage of dust, and Print percentages are not divided by it. Disposable income is exactly 0 in a zero-based month (Pay 9.94 less fees of 0.38 and 9.56 used to print `-0.0` with a red bar, and made Month-by-Month show a percentage of dust), the summary does not add an "Other…" row for small rows that cancel to the cent, a Month-by-Month cell of exactly one dollar is not blanked, and the summary's "vs plan" column appears at exactly 5.00 while the printed reports' "+/-" columns stay hidden until the difference is more than 5.00, whatever the dust. Reports no longer print a red `0.00` or `-0.00` (whole-dollar figures are red exactly when they print a minus), and the Excel Stats Report no longer adds a `+/-` cell where actual and plan agree. A summary row of exactly 5.00% of income (75.02 of 1,500.40) is listed instead of being folded into "Other…". The Comparative report keeps its cents for amounts in the hundreds of millions. A filter such as `=2.01k` finds 2,010.00. The rounding tolerances grow with the size of the amounts, so balances in the billions stay flat too.
- **Comparative report:** amounts keep their cents (19.99 no longer prints as 19.98). The same description under two categories stays two rows instead of one overwriting the other, and a payment that only the second range has is listed under its own category.
- **Excel reports:** the Monthly Excel report carries no picture links, so opening it never looks for `images` or `icons` as network names. Descriptions and categories that start with `=`, `+`, `-` or `@` are indented so a spreadsheet does not read them as formulas. Picture names are escaped in the HTML reports.
- **Balances:** a rent or other business-days, early-pay payment due on a weekend just past the end of the built months (for example Saturday 1 January, paid Friday 31 December) is shown in the last month whatever month you are viewing; it used to appear only after you moved forward and back. A balance checkpoint you type equal to the balance shown no longer leaves a zero "(Unaccounted)" row in the summary and reports.
- **Emoji and other symbols:** payment descriptions, categories and transaction notes may contain them, as the file format has saved them since 0.4.0. A payment or occurrence that already had an emoji could not be edited at all; now it can.
- **Amounts:** an amount typed in the payment, transaction or balance box as a formula or with more than two decimals is stored in whole cents, so `10/3` is 3.33 each month and the months add up to the balance shown.
- **Editing:** Apply with nothing changed on a payment or a transaction closes the editor with "Nothing changed." instead of marking the budget unsaved and adding a status entry to the file. Once the "From this date onward / All transactions" choice is showing, Apply follows it even if you undid the amount change that brought it up, unless you undid everything, in which case nothing is split. After the "already has transactions" notice, the next click on "Apply to all occurrences" applies even if you corrected a typo in between. A note-only change under "This and all future occurrences" says "Transaction applied." rather than "amounts updated". A budget that finishes opening while you drag a transaction waits for the drop, and switching to another window mid-drag cancels the drag instead of leaving a ghost that turns your next click into a drop. File > Close budget now also cancels a budget that is still being read, instead of opening it moments later (or asking you to save changes on an empty window). Under "This and all future occurrences" the message says amounts were updated whenever later occurrences were rewritten, even if the amount you left in the box was the one shown.
- **Changing from a date onward:** a new description or category that would clash with the earlier transactions' own category or description is refused with an explanation instead of being renamed when the budget is next opened.
- **Files:** a budget holding a short date number (such as `3000` or `202701`) no longer crashes the app when its months are built. A `.budgetrc` saved by Notepad with a byte-order mark keeps its first setting.
- **Month navigation and keys:** Page Up, Page Down, Home, T and Alt+arrows do nothing without a budget, like the month controls. Page Up, Page Down, Home and T (and `?`) keep working while a tile, arrow or row has keyboard focus; only a text box takes them (Alt+arrows work there too). The arrows and keys stop at January 1000 and December 9999. Close budget on the Payments page returns to the calendar toolbar. `--today=` takes a real date only and also opens the month of that date.
- **Prompts:** the unsaved-changes prompt names a failed save only, not an unrelated message such as a file that did not open, and forgets a failed Save as once you edit. A password prompt receives Escape before any dialog behind it.
- **Hover cards:** near the right and bottom edges of the window a payment's hover card now opens above and to the left of the pointer instead of being pushed over it. Pushed over the pointer, the card made its own payment row lose the hover and the click (the card flickered, and a press landed on the card instead of opening the editor). The "Disabled" badge on the Payments page no longer swallows the click meant for its row.
- **Keyboard focus:** the Payments table rows and status chips, "Reset filters", the summary breakdown rows, "Discard changes" and the delete/remove buttons in dialogs show the 2px accent focus ring when you Tab to them.
- **Hover feedback:** the outline buttons (Cancel, Save changes while there is something to save, Clear, Edit) now show an accent border and tint on hover and press, as the other buttons do.
- **Dragging:** the day you are about to drop on is tinted behind its content instead of being covered by an opaque box, so its date, payments and balance stay readable while you decide.
- **Narrow windows:** at the minimum window width the "Unsaved changes" pill no longer overlaps the Reports button, the status bar drops the gestures hint on windows narrower than 1000 px while there are unsaved changes or a search, rather than overlap the shortcut keys, and in the day dialog a long description is shortened before its amount instead of running underneath it.
- **Calendar contrast:** payment names and amounts, the `+N` label, hover cards, the selected summary row, the search chip, the first-run steps and the "Changes waiting to be saved" note meet 4.5:1 on past, weekend, today and highlighted tiers too. A hovered payment row stands out on today's and weekend cells. Screen readers hear dates, not eight-digit numbers. The primary and danger buttons (and Unlock) keep a readable label on hover, press and keyboard focus even with a custom accent or negative colour, and the selected Payments status chip meets 4.5:1. The adjustment shown in brackets on a balance strip rounds to whole dollars instead of dropping the cents.
- **Custom colours:** a `.budgetrc` that customises the day-cell colours now applies "BKG cell past" to days before today and "BKG cell future" to the rest (weekends included), as the original does, and no longer recolours cards, dialogs and text boxes. A custom balance-strip colour gets readable ink on ordinary strips, and ink on mid-tone colours falls back to black or white when the soft ink cannot reach 4.5:1.
- **Speed:** the grace-period highlight of a payment that starts in a very old year (for example a daily payment mistyped as year 1026) no longer takes a second per hover or month change.
- **Documented limit:** a report still open in a spreadsheet cannot be deleted when the app exits and stays in the temporary folder.

## [0.7.0] - 2026-10-28

Third review: data safety, correctness of numbers and loans, and interface clarity.

- **Starting and closing:** File offers New budget (it never replaces an existing file) and Close budget (asks first when there are unsaved edits). With no budget open, the month controls and Today are disabled.
- **Backups:** only backups this app made are pruned. A backup you copied yourself, such as `home.bgt.bak`, is never deleted. The app records its own backups in `<budget>.baklog` beside the budget. Backups made by earlier versions are no longer pruned automatically.
- **Loans:** a loan's saved payments may not exceed its principal. Every edit that would overpay is refused with a message naming the principal, including changes to one occurrence and Apply to all. A loan that already overpays still opens and saves, but each edit to it is refused until it is corrected.
- **Amounts:** a decimal comma inside a formula (`12,50+3`) is an error, not 1,253; commas are accepted only as thousands separators. A typed `+` under Expense is refused, as a minus under Income is. A zero amount opens as Expense. An amount beyond the range reports "too large".
- **Changing from a date:** "This and all future occurrences" leaves the payment as one record when the amount is unchanged, and applies to every occurrence from the date for tentative payments too. Moving an occurrence onto a skipped date keeps that date's note and status unless the moved occurrence sets its own.
- **Files:** a byte-order mark from Notepad no longer stops a budget from opening. Non-finite amounts are refused before saving. Option dates must be real calendar days. Settings are saved through a temporary file.
- **Filter and search:** `| rent` finds Rent only. `<-1k` means below -1,000. `invoice: 12` finds "Invoice: 12". Searching by note or flag works on each occurrence. A search leaves the (Unaccounted) line out of the brief summary.
- **Brief summary:** an expense that changes to a positive amount stays in the expense list, and its planned amount stays on its row, so rows add up to the totals.
- **Reports:** descriptions, categories and notes are escaped in every report, so `<` and `&` in your text no longer break or inject markup. Comparison totals above 21.47 million print correctly. Enter on a Reports item opens its range dialog; Enter on a shortcut keeps the dialog open.
- **Calendar:** amounts stay whole in narrow cells and names truncate first. A month with no balance change shows no Min labels. Text on pills, balance strips and adjacent-month days meets the 4.5:1 contrast minimum. Past, weekend and weekday cells are visibly different. The lowest-day band and the adjusted marker no longer use the accent blue. Summary figures take their colour from the sign they print.
- **Search results:** an active search shows a clearable chip above the grid with the number of matches; the footer names the calendar gestures.
- **Recent budgets:** a budget spelled with different capitals is listed once. A missing recent budget can be removed from its menu. Save as adds `.bgt` unless the name already ends with it.
- **Command line:** `--month=`, `--today=`, `--size=` and `--filter=` are accepted as well as the separate-argument form.
- **Keyboard:** PageUp, PageDown, Home, T and Alt+arrows change the month only on the calendar page.
- **Releases:** the build identifier no longer depends on how the source files were checked out. Archives record the commit and refuse uncommitted changes. The installer must match the archived version, and it restores any previous `.bgt` file association when uninstalled.

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
