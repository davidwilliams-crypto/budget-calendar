# Changelog

## [0.12.0] - 2026-11-13

A new look: Budget Calendar now follows the Meridian design system on Windows and on the phone. Calmer surfaces, one clear accent, and figures that line up.

- **Cards on a canvas:** the calendar, summary and tiles sit on soft white cards with generous corners and a barely-there shadow, on a pale blue-grey page, instead of bordered boxes. Dialogs are rounder and float above the window. The dark appearance uses Meridian's deep navy surfaces.
- **One clear headline:** Closing balance is now the one dark panel on the screen, so the figure that matters stands out; it turns amber when the month ends below your minimum balance. Money in and Money out lose their coloured edge bars; the amount's colour already says which is which.
- **New typeface:** all text is set in Geist, built into the app, so it looks the same on every PC. Every digit has the same width, so amounts and balances line up in their columns. Headings, labels and captions follow one size scale, and labels are sentence case ("Sunday", "Recent") instead of capitals.
- **Calmer controls:** the File, Edit, View and Reports menus and the month arrows are quiet toolbar buttons with no outline. Calendar/Payments and Expense/Income are segmented switches with the selected option filled blue. The main buttons are 40px tall with 12px corners and no raised edge; Save changes is a soft blue button while there is something to save. Inputs and checkboxes keep a clearly visible outline, and keyboard focus shows a 2px ring just outside the control.
- **Accessible colours:** every text colour still meets WCAG AA (4.5:1) on every surface it appears on, in both appearances, including the new calendar shading for past days, weekends and other months. Colours you customised in your settings are still honoured.
- **Phone app:** the household app takes the same palette, cards, controls and sentence-case labels, and switches to the dark appearance with the phone. Its headline balance card is the flat dark panel instead of a green gradient. It uses Geist only where the phone has it installed, and the phone's own font otherwise.
- **Credits:** Geist is used under the SIL Open Font License 1.1; the licence and a note on the one change (tabular digits by default) are in `budget-gui/assets/fonts`, and the About box names it.

## [0.11.1] - 2026-11-04

A polish pass: the monthly chores take fewer clicks, searches answer "how much?", and a first budget starts from your real balance.

- **Mark paid in one click:** right-click a payment on the calendar, or open More in a day's list, and choose **Mark as paid**. A paid one offers **Mark as unpaid**. Only that date changes; its amount and note are kept, and Undo reverses it. Before, marking a bill paid took four steps through the transaction dialog.
- **Search totals:** while a search narrows the calendar, the "Showing 5 of 21" chip also shows what the matching transactions add up to this month, so a search for "gas" says how much went on gas.
- **Start from your real balance:** the first-payment card now begins with **Set today's balance…**, which opens the balance editor for today. Forecasts no longer have to start from zero.
- **Readable load errors:** a budget file that cannot be read names the part in plain words, such as "Line 4: a payment's end date could not be read", instead of the loader's internal field name ('end_date').
- **Screen readers:** calendar payments are read with their date and whether they are paid or important, not only their name and amount. The previous and next month buttons, both search boxes and the search's clear button now have names instead of an icon glyph or nothing.
- **Tidier File menu:** with no recent budgets, the File menu no longer shows two separators stacked together.

## [0.11.0] - 2026-11-04

Ready to hand to someone else: a friendlier installer, a clear way to start, and signed downloads.

- **Start a budget from the first screen:** with no budget open, the window now offers **Start a new budget…** as its main button, with **Open…** beside it. Before, a first-time user saw only Open… and had to find File > New budget.
- **Friendlier installer:** setup now has a welcome screen, can start Budget Calendar when it finishes and offers a desktop shortcut. If Budget Calendar is open while you install or uninstall, it asks you to save and close it instead of failing part-way. `.bgt` files get the app's icon straight away, and the setup program carries version details in its file properties.
- **Runs on a clean PC:** the Windows executable now includes the C runtime it needs, so it no longer depends on the separately installed Visual C++ Redistributable.
- **No bundled sample:** the installer no longer copies a sample budget into the program folder. An existing copy from an earlier install is left alone.
- **Signed releases on GitHub:** a release workflow builds and tests each tagged version on Windows, signs the executable and setup program with Azure Artifact Signing, archives the release and drafts a GitHub release with the installer and a plain-language install guide (`docs/INSTALL.md`). Setup is described in `docs/RELEASING.md`.

## [0.10.1] - 2026-10-31

A speed pass over the engine and the calendar: the same answers, noticeably less work to produce them.

- **Shared budgets sync faster:** every edit on a shared budget used to re-scan the whole payment list against every other payment to check naming conflicts and stable identities — work that grew with the square of the budget size, twice per edit. Those checks and the balance forecast's per-transaction lookups are now hash lookups, so a large household budget edits and forecasts in time proportional to its size rather than to its square.
- **Smoother calendar on Windows:** the sign used to pick a figure's colour was taken from a fully formatted money string several times per row every frame; it is now read straight from the value. The accessible colour-contrast search is remembered per colour pair instead of being recomputed for every row, every frame.
- **Leaner statistics and reports:** the monthly statistics that drive the summary panel and search no longer copy each payment's description and category per occurrence, and report and save text escaping no longer allocates per character. Month views, searches, printed reports and saves all do less allocating on the same data.
- **Engine:** the phone's WebAssembly calculation engine was rebuilt and re-stamped from these sources; native parity and all workspace, mobile and integration tests pass.

## [0.10.0] - 2026-10-31

Unlock with a passcode you choose, and a full security review of everything that protects a household.

- **Household passcodes:** creating a household now chooses a passcode of three or more unrelated words instead of forcing you to manage the long recovery key. Every member types that passcode to unlock the household on a new device — Windows, phone or browser. The passcode unwraps the household's encryption key on the device; the service never receives the passcode, and a wrong passcode saves nothing.
- **The recovery key becomes the backup:** it is still created, shown once and copyable at any time, and it keeps working. If the passcode is ever lost, the recovery key unlocks the household; if the key is lost, the passcode does. A household that prefers key-only can remove its passcode entirely.
- **Set, change, remove:** the household owner sets or changes the passcode from Shared budget on Windows or from Settings on the phone. Changing it asks for the current passcode or recovery key, and verifies it against the household before saving, so a mistyped current secret can never lock the household out. Removing the passcode is immediate and returns the household to key-only unlocking.
- **How it protects the budget:** the passcode is stretched with Argon2id at the same cost as password-protected budget files (64 MiB, 3 passes), then wraps the random 256-bit household key with AES-256-GCM bound to the household's identity. Only the wrapped copy is stored with the household, and the service validates nothing but its exact shape. A full security review of this design and of the whole app — encryption, service, phone and Windows — is recorded in `docs/security-review-2026-10-31.md`; it found no high-severity issues.
- **Plain-language wording:** invitations, unlock screens, settings and help now speak of the household passcode first and the recovery key as its backup, with the trade-off (a service break-in plus a guessed passcode could decrypt a budget) stated where you choose the passcode.
- **Engine:** the shared Rust calculation engine gained the two passcode actions, and the phone's WebAssembly engine, its native parity tests and the service-worker build were rebuilt and re-stamped for this release.

## [0.9.0] - 2026-10-30

Fixes from the full product review: figures you can trust, work you cannot lose.

- **Undo everywhere:** Windows adds Undo and Redo (Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z) and an Edit menu that names the action. On the phone, every save offers Undo for about 8 seconds, and Activity lists your recent edits. An undo on a shared budget is a new change that syncs, and it leaves a partner's newer change alone.
- **Crash recovery on Windows:** a protected copy of unsaved work is kept within about 5 seconds of an edit or when the window loses focus. After a crash, the budget offers to restore that work as unsaved changes; your file is not touched until you save. Password-protected budgets stay protected, and a damaged one still offers its copy. A crash also leaves a short log with no financial data.
- **Restore previous version:** File > Restore previous version lists a budget's backups with date, time and size, and opens one as unsaved changes. Quick saves can no longer erase the last good version: the first backup of each of the last 30 days is kept alongside the newest ones.
- **Damaged files:** an empty file is refused plainly. A file that may have been cut short, or an older file with no end marker, is held back until you restore an earlier version, open it anyway or cancel.
- **Correct figures on Windows:** paging to an early month no longer shows balances built from zero, and a month's figures no longer depend on how you reached it. Balance corrections and a starting balance no longer count as money in or out; they have their own lines in the summary and reports. Reports offer every month with data, built from your full history. The summary panel appears from 1280 wide.
- **Whole-dollar rounding:** whole-dollar figures round to the nearest dollar, half away from zero. 5,613.50 now shows 5,614; it used to show 5,613, and balances could read up to 95 cents low.
- **Safe phone edits:** editing a repeating entry no longer moves its schedule, changes its amount or marks every date paid unless you change those things. Moving a date shows what changes before saving, and warns if earlier payments will come off the schedule. Repeat options say what they do, such as "Last Friday of the month". Disabled payments are left out of totals and Coming up, and appear muted on the calendar.
- **Phone balances:** Set balance shows the existing figure, saving it unchanged does nothing, removing it needs a confirmed step, and days with a set balance are marked.
- **Phone updates and offline:** a calm "Update ready" prompt appears when a new version arrives, and never over an open form or unsent changes. Offline now shows an amber status with the number of changes waiting and the last sync, not "Failed to fetch".
- **Shared budget safety:** "Use current shared version" asks first, says how many unsynced changes will be discarded, and keeps a protected copy you can get back. Ctrl+S during a conflict no longer stops you resolving it. Closing a dialog opened from a keyboard-focused menu no longer crashes the app when an accessibility tool is active.
- **Release checks:** `scripts/release-gate.ps1` runs every test in one command and fails on any failed or skipped step. Phone packages are refused if the calculation engine is older than its source. A CI workflow is included but has not run yet.

## [0.8.4] - 2026-10-29

- **Steady shared-budget screen:** the dialog fits compact and high-DPI windows, keeps its Close button visible and uses a stable status line so background connection checks do not move controls. Account forms and long content scroll within the dialog; errors appear once.
- **Quiet background sync:** unchanged checks no longer clone and rewrite the protected budget caches or rebuild the calendar. Idle windows wake at the next check or completed worker instead of redrawing four times per second; saved commit retries retain their exact durable request without another cache flush.
- **Responsive conflict review:** remote conflict data is decrypted once and complete comparison contents are prepared only when requested, preserving local edits and the existing explicit conflict choices.

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
