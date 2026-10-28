# Budget Calendar UI

## Visual system

The desktop interface uses Segoe UI, a restrained blue/slate palette, 14 px body and button text, 12 px captions, clear headings, consistent 34–36 px controls, and 10 px card corners. User-customized budget colors remain supported. Income and expense colors emphasize amounts; current-day and selection states use blue.

The application has a calendar brand mark and matching SVG, PNG, and Windows ICO assets in `budget-gui/assets`. The native window uses the PNG; the desktop shortcut uses the ICO.

## Navigation and layout

- The header identifies the app, current version, file and saved state. Click the version to open build details and dated change history; View also contains About and version history. New payment is the primary action; Save changes enables when there are changes. File contains Open, Save as, recent budgets and Close. View contains appearance, calendar cents, breakdown visibility and shortcut hints.
- Calendar and Payments are persistent workspaces in the same window. Switching pages retains the document and search/selection state.
- Calendar navigation offers previous/next month, a month/year chooser, and Today on wider windows. Search transactions filters immediately; advanced expressions such as `cat:Auto` and `<-100` remain optional. Invalid filters receive a readable explanation.
- A compact financial strip shows opening balance, money in, money out and closing balance with cents. Hover for the exact value, net change and minimum, average and maximum balances. Oversized values use an explicit ellipsis and remain available in tooltips and accessibility information.
- The month grid fits all four, five or six weeks in the available window without vertical scrolling. Each payment is an 18px single-line row, with a readable 14px name on the left and a 13px amount aligned on the right. Separate columns prevent overlap; long names truncate and remain available on hover. Week heights respond to the busiest day so quiet weeks give room to busy ones. The ordinary five-week budget's 4–5 payments per busy date fit at 1200×800. Smaller windows show several rows where space permits; +N in the day header opens every transaction if overflow remains. Current day is blue; minimum/maximum/low balances use small markers.
- The optional monthly breakdown appears at widths of at least 1480 px. The header and financial summary use compact spacing to prioritize the calendar. The app initially maximizes to the available work area; explicit CLI sizes remain available for captures.
- The footer shows document count, search result count when relevant, save state and keyboard hints.

## Version history

The native title and header show the running app version. The version-history dialog shows a deterministic build identifier, dated release notes from `CHANGELOG.md`, and Copy version info. The dialog scrolls within compact windows, and closing it preserves any unfinished budget form.

Version 0.3.0 is the first tracked local release. Every delivered application update receives a new version and changelog entry. `scripts/archive-release.ps1` preserves the executable, icon, notes, verified matching source ZIP, build-input manifest and SHA256 records under `local-releases/vVERSION`; it refuses to replace an existing version with different build contents. `AGENTS.md` records the release workflow for subsequent work.

## Payments workspace

Payments is a full page with live description/category search, All/Enabled/Disabled filters, numeric and date/name sorting, and aligned virtualized rows. All records, including historical ones, are accessible by default. Enabled means not disabled; it does not hide ended or future records.

Rows show description/category, amount, a readable recurrence/start date, status, an Edit action and an overflow menu with confirmed deletion. Selection stays tied to the record when sorting/filtering. Empty and no-results states offer useful recovery actions. Ctrl+P opens Payments and focuses search.

## Editing

New payment / Ctrl+N opens grouped payment and schedule fields. Expense and Income are explicit choices; enter the amount as a positive number. Existing signed amounts and hidden payment metadata are preserved. Start/end dates support both ISO typing and a calendar picker. Choosing a recurring schedule from Once defaults to continuing indefinitely; subsequent end-condition choices are retained.

Click a transaction to edit that date's amount, note and status. Right-click for series editing/deletion and filtering. Click +N more for a day dialog containing every transaction; right-click a day to add a payment there. Loan changes recalculate payoff/future installments, and tentative paid-status changes update the period's unpaid estimates; forms explain these native behaviors.

The amount scope offers **Only this occurrence** (default) or **This and all future occurrences**. Future scope applies the amount from the selected date onward, including future amount exceptions, while keeping earlier amounts unchanged. Notes and status remain specific to the selected date. Deleted occurrences stay deleted and moved dates remain in place. Delete transaction still affects one occurrence.

Drag a payment row onto a date to move that occurrence, including adjacent-month dates. Hold over a month arrow to navigate while dragging. Drops on the same date or outside the calendar do nothing; a destination that already contains the same payment is rejected. Right-click a day balance or the opening/closing summary balance and choose Edit balance to set an absolute closing balance for the displayed date. Later balances recalculate up to the next checkpoint; an existing adjustment can be removed.

Forms have grouped fields, visible scrolling, pinned validation, and consistent Cancel/primary-action footers that fit compact windows. The date picker uses real, bounded calendar dates. Validation never commits invalid drafts.

## Saving and protection

Apply changes in memory, then Save changes / Ctrl+S to write the budget. A clean document is not rewritten. The title, badge and footer indicate applied unsaved changes.

Opening a different budget or closing prompts to save, discard or cancel; an unapplied draft must first be finished or explicitly discarded. Failed/cancelled Save as keeps the current path and unsaved state. Disk changes made by another app are detected; Save as can keep edits separately.

Writes stage the full file and retain the previous version as `.bak`, then `.bak.1`, `.bak.2`, etc. Existing per-date exceptions, notes, encryption and balance adjustments remain supported.

## Verification and captures

CLI: `budget-gui budget.bgt --screenshot out.png --size 1200x800 [--dark]`.

The redesign was checked with a copied budget in standard, compact, wide, and dark layouts, including validation, repeat controls, date selection, live search, and transaction/day dialogs. The original budget is not test data. Historical capture references in earlier design notes predate this redesign.

## Dialog keys

In the payment, transaction and balance dialogs Enter in a single-line field applies the form, and Escape cancels (discarding the draft) unless a dropdown or date picker is open, which closes first. Clicking the backdrop keeps the dialog open. Amount fields accept thousands commas, a leading `$`, and accounting parentheses, up to 1,000,000,000,000.

## Theme and shell refinements

- Calendar cells have three distinct tiers: past days, weekends and leading/trailing days of adjacent months differ visibly in both themes. Day numbers recede to muted in light mode so payment names dominate.
- All text roles (text, muted, faint, positive, negative, disabled, balance strips) meet WCAG AA 4.5:1 on the surface; a unit test enforces this. Buttons show a 2px accent focus ring, accent hover borders and a pointing-hand cursor.
- Payment rows carry a 3px left accent bar in the income/expense colour; income amounts are semibold so polarity does not depend on colour alone.
- Ordinary balance strips show only the right-aligned number on a faint tint. Lowest, highest and below-minimum days get a labelled, fully tinted strip (Low shows a warning glyph).
- The Closing balance tile is the hero (accent border and tint, larger value, arrow with the net change since opening). Money in/out tiles have a coloured left edge.
- Header: brand, then a file icon and name, then a status pill (green dot Saved, amber Unsaved changes). The version chip is quiet text.
- Calendar/Payments segmented tabs have icons and an accent underline. Today is an accent button only when another month is shown.
- Empty budgets show an "Add your first payment" card over the grid. With no file open, a dashed drop zone and recent-file cards (with last-modified age) are shown.
- Summary panel: share-of-income bars are 4px full-width tracks, only filterable rows highlight on hover, percentages use the U+2212 minus, and the disposable-income rule is coloured by sign.
- Keyboard: F1 or ? opens the shortcuts sheet (also View > Keyboard shortcuts). New: Alt+Left/Right change month (also while the search box has focus) and T jumps to the current month. Footer hints use keycap chips.
- Outer gaps (header, summary strip, grid, footer) are all 8px.
- Today's cell has a stronger state: 7% (light) / 12% (dark) accent tint, a 1px accent inset border, a 2px accent bar along the top edge, and a bold day number in the filled circle. Row geometry is unchanged.
- Row markers are 6px coloured dots with a 1px halo (visible on dark): status (Paid green, Important orange) at the left of the name, and after the amount "moved off a weekend" (accent) or "last payment" (muted). The apostrophe/asterisk text suffix is gone from the amount. View > Calendar legend explains each marker.
- The row hover tooltip is a compact card: bold name, category and status pills (Paid, Important, Disabled, Moved off weekend, Last payment), the amount coloured by sign, then planned and balance-after.
