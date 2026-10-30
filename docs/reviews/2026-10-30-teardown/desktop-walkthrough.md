# Desktop walkthrough evidence: Budget Calendar v0.8.4

Date: 2026-10-30. Screenshots and UIA dumps: `evidence/desktop/`.

## Incident during this run (needs the user's attention)

A stray file was created outside the permitted paths:

    G:\My Drive\From One Drive\Desktop\budget.bgt   (37 bytes, created 2026-10-30 15:16:59)

Cause: while driving File > New budget, my filename entry failed silently and I sent IDOK to the native
Save dialog. It accepted the default name `budget.bgt` in the dialog's remembered folder (HOME/USERPROFILE
isolation does not cover the Windows common dialog's last-used folder).

- It is an empty budget. I did not put data in it or open its content; only file metadata was read.
- No user file was overwritten: `start_new_budget` refuses existing paths (main.rs:512), and the creation
  time is the moment of my action.
- My process had it open with autosave on, so I stopped that process immediately, by PID. Nothing else wrote to it.
- NOT deleted: it is in the user's synced folder and removal is the user's call. Safe to delete.
- After this, no native dialog was committed. One Open dialog was opened and dismissed with WM_CLOSE only.
- The review-only `target/review/home/.budgetrc` refers to that path; it is throwaway and was not reused.

No user .bgt, no real .budgetrc and no already-running budget-gui was touched (none was running).

## Method and limits

- Build: `target/release/budget-gui.exe` is STALE (v0.7.1, build 16221256256fe0b3). Testing it would have
  reviewed the wrong code, so I ran a copy of `local-releases/v0.8.4/budget-gui.exe` at
  `target/review/bin/`. Build 10172c081798bb6e, git 268e47c, SHA256 d209720b... matches release.json.
  This deviates from the instruction to use target/release; nothing was rebuilt or overwritten.
- Isolation confirmed from code: `home_dir()` prefers USERPROFILE (main.rs:57), rc at main.rs:389, shared
  cache at sharing.rs:88, reports at temp dir (reports.rs:128). All redirected. Limit: see incident.
- REA native UI tools are macOS only ("Native macOS utilities require macOS"), so they could not be used.
- Substitute driver (scratchpad/ui.ps1), scoped to my PID: .NET UI Automation, PrintWindow, posted key messages.
- Posted MOUSE messages do not reach the app, and real SendInput would seize the user's cursor. So:
  LIVE = keyboard + UIA Invoke. CODE ONLY = right-click menus, drag between days, double-click.
- Not exercised: Reports and print (they launch the default browser or Excel on the user's desktop; only the
  menu was inspected), real pointer drag, DPI other than 144 (150%), Narrator/NVDA speech, default maximized launch.
- Fixture amounts were rounded to 1 decimal until I fixed the generator; run1 figures reflect that.

## Coverage

| Flow | Result | Evidence |
|---|---|---|
| First launch, no file | Works; Open only, no New | 01-first-launch.png |
| New budget | Native Save dialog; empty budget opens | 02-file-menu, r8-empty |
| Add payment | LIVE by keyboard, count 14 to 15 | 13a, 13b |
| Edit one occurrence | LIVE, dialog clear | 14-transaction-dialog |
| Drag between days | CODE ONLY | calendar.rs:369-374, 558-559 |
| Mark paid | LIVE | 16a, 16b |
| Balance checkpoint | Live: no route by keyboard/AT; CODE: right-click only | 28a, calendar.rs:516-523 |
| Search/filter | LIVE, immediate, clear count | 17a, 17b, r7 |
| Payments page | LIVE | 18, 30 |
| Reports, print | Menu only | 06-reports-menu |
| Dark mode | LIVE and render, strong | 25, r3, r6 |
| Minimum 900x600 | Render good | r1, r3 |
| High DPI 150% | Crisp, correct | all shots |
| Invalid input | Partial | 08, 09, r7, r9, r10 |
| 500 payments | Excellent | r5, r6, 30 |

## Timings

- Window created 86-148 ms. Full launch to saved screenshot 480-806 ms, 500 payments 531-616 ms.
- 500 payments: PgDn month change 17-35 ms including my UIA polling.
- Payments page tree 56 nodes with 500 rows, so the list is virtualized.
- Memory 269 MB working set, 280 MB after Payments page.
- Idle CPU 0 ms over 10 s with no automation client. Earlier 828 ms in 5 s was with a UIA client attached.

## Findings

### High

H1. New users are never asked for a starting balance.
main.rs:1310 onboarding is "Add your salary / recurring bills / watch the balance"; r8-empty shows Opening 0.00.
The only way to set real money is right-click on a balance strip (calendar.rs:516-523), with no hint in onboarding.
Scenario: user enters all bills, sees a forecast from 0 that does not match the bank, and has no visible way
to correct it. For a balance forecaster this is the core promise failing in the first session.

H2. Core capabilities are pointer-only.
`pointer_only` removes cells and strips from keyboard focus (calendar.rs:252-257). Reachable only by right-click or
drag: Edit balance (516-523), moving an occurrence (558-559, no date field in the transaction dialog, editor.rs has
no move), Restore deleted occurrence (331-337), Edit whole payment from a date, Filter this payment (569-571).
LIVE: Invoke on a strip does nothing though the node advertises Invoke and is named "Right-click to edit balance"
(28a). A keyboard or screen-reader user cannot reconcile a balance at all.

H3. "Money out" counts balance checkpoints as spending.
stats.rs:249-250 adds the adjustment to total_expense; shown at main.rs:1047 tiles. LIVE: October Money out
17,473.00 (later 17,658.50) when real payments total about 4,029; the rest is a -13,444 checkpoint.
The fixture gap is deliberately large, but a routine reconcile distorts it the same way, and a positive one inflates
Money in. Inherited legacy behaviour ("(Unaccounted)" in the summary), but the tile has no label for it, and the
Summary panel explaining it is absent at 1280 wide (04-household vs r4-wide-1920).

### Medium

M1. Screen readers cannot tell status or date of a payment.
calendar.rs:563-564 name is "Gasoline -55. Click to edit; drag to move"; Paid and Important are omitted (identical
name before and after marking paid, tree 16) and no date is given. Tree is flat, no day/grid structure. Visually
status is only a small coloured dot (calendar.rs:602-605), so paid vs important relies on colour.

M2. Unnamed and misleading controls.
Search box name="" (main.rs:1027, no labelled_by), also Payments search; prev/next month named with an icon glyph
(main.rs:968, 985, icon_button 2034); combos name="" (Repeat, Status, Sort by); payment row "..." button is an
unnamed glyph; primary buttons carry a private-use glyph in the name; menus are Buttons, not MenuItems.
Tree otherwise strong: dialog fields are labelled, Payments rows read "Car Loan, -420.00, Every month, Enabled".

M3. Modals do not isolate assistive tech.
LIVE: with the shortcuts sheet open, UIA Invoke on the background Save changes saved the file (22-saved.png, file
timestamp changed). Trees during dialogs include every background control. Scenario: a screen-reader user tabs by
virtual cursor into background actions behind an unsaved dialog.

M4. UIA ValuePattern on text fields is a silent no-op.
LIVE: SetValue on Amount moved focus but the value stayed empty (08d). Voice-control and automation tools report
success and enter nothing.

M5. Marking paid takes four steps and has no shortcut.
Click payment, open Status combo, pick Paid, Apply (14, 15, 16). The right-click menu has no Mark paid
(calendar.rs:569-574). This is the most repeated monthly action in a bill tracker.

M6. Esc silently throws away a filled form.
LIVE: typed payment gone with no prompt (11c). Deliberate per editor.rs:519, lines 375 and 561, but a single stray
key loses a long recurring setup, while closing the app does guard drafts (main.rs:297).

M7. Empty state offers no way to create a budget.
01-first-launch: "Open a budget" with Open only. New budget sits in File, top right, with no shortcut (Ctrl+N is New
payment). A first-time user has nothing to open. New also starts with a Save dialog defaulting to `budget.bgt` in the
last-used folder (main.rs:503-505), which is exactly how the stray file above landed in a synced folder on one Enter.

M8. File dialogs are not owned by the window.
No set_parent at main.rs:504, 582, 690 or sharing.rs:199 (grep of main.rs found none). LIVE: dialog at 0,0 sized
1800x720 while the app sat at 266,266. rfd blocks the UI thread, so the main window can be frontmost yet frozen.

M9. Load failure offers no recovery from backups.
r9-corrupt: "Line 4: Invalid data for 'end_date'" shows an internal field name, and the only action is Open another
(main.rs:106-114), though saves keep `.bak` files beside the budget (run1.bgt.bak observed).

M10. Search gives no answer.
17a: "Showing 5 of 21" with tiles still on the whole month, no sum of matches, and no hint of matches in other months.
"How much on gas?" is the natural question. `cat:` alone passes as valid (r7), against docs/ux.md:13.

### Low

L1. Dialog tab order is Description, Expense, Income, Amount: two stops before the next field a user wants (12a-d).
L2. Payments page: blank Status column for enabled rows, no monthly total, no next-due date (18).
L3. Whole-dollar calendar uses half-even (numfmt.rs:65-72): -62.50 shows -62, -145.50 shows -146, so rows look
inconsistent. UNRESOLVED: balance 5,613.50 displayed 5,613, which half-even would make 5,614; not explained.
L4. File menu shows two stacked separators when there are no recent files (02-file-menu).
L5. Terminology: "15 payments" and "5 of 21 transactions" sit side by side unexplained (17a).
L6. Below minimum size the header overlaps (19-resize-small), but this was forced by SetWindowPos; I could not
confirm a user can reach it, and min size at main.rs:2222 may hold for real resizing.
L7. Release process: target/release is v0.7.1 while the archive is 0.8.4 (built under target/sharing-release-v0.8.0).
Anyone following the "use target/release" habit tests a stale app.

## Tested and fine

Dark mode quality; 900x600 layout with "+8" overflow; 500-payment rendering; autofocus on Description; Enter submit;
red invalid amount; PgUp/PgDn; F1 sheet; save with .bak; missing-file banner; idle CPU.

Not defects, retested: main window at -32000 during the first Save dialog did not reproduce; shortcuts sheet ignoring
Esc once did not reproduce on two clean tries, so it is unexplained and not reported.
