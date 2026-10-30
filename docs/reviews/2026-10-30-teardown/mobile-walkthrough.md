# Android PWA live walkthrough: Budget Calendar 0.8.4 at 268e47c

Date: 2026-10-30. Tester: QA lead subagent. Read-only: `git status` clean at end. No user budgets touched.

## Method and environment

- Service: real `budget-mobile/server/start.mjs`, spawned per run on loopback ports with `BUDGET_DEVELOPMENT=1`, random setup token, data under `target/review/mobile-data/`. Stopped by PID; no process or port left over.
- SW update test only: server module against a throwaway copy of `public/` under `target/review/mobile-public-copy-*`.
- Client: Playwright core driving Chromium 156 headless (ms-playwright build 1248), emulating Pixel 8: 390x844, DPR 2, touch, Android UA, en-CA, America/Toronto. Two separate contexts acted as two devices. Used this instead of the built-in pane because it provides offline emulation and isolated profiles.
- Scripts: `scratchpad/wt/`. Raw log: `evidence/mobile/run.log`, `evidence/mobile/p2b.out`. Screenshots: `evidence/mobile/`.

## Could not reach (be honest)

- No real Android device. Never verified: real soft keyboard, native date/month pickers, Android back gesture on dialogs, Add to Home Screen install flow, `beforeinstallprompt`, maskable icon rendering, background tab freezing, storage eviction.
- Keyboard is simulated by shrinking the viewport to 390x420. This models resizing, not Chrome Android's default visual-viewport overlay, so it does NOT prove keyboard behaviour. See M11.
- Android back on a dialog was approximated with Escape, which is what Chrome's CloseWatcher maps to. Not confirmed on hardware.
- Invite expiry (48h), rate-limit 429s, 507 quota, multi-household picker, and loan-end entries were not exercised.
- `Create another household` and restoring from history were not run end to end (history review opened only).

## Harness artefacts discounted (not findings)

- Earlier runs showed slow join (13 s) and re-sign-in (32 s). Re-measured cleanly: join 98 ms, re-sign-in 119 ms, raw `/login` about 60 ms. Those were test-script waits, not product latency.
- One aborted run logged `Target crashed` on the member tab, during a cascade of failed steps caused by my script. It did not recur in later runs. Cause unknown; not reported as a product bug.
- Files `01-` to `04-` from phase 1 were overwritten by an aborted run; the run.log text stands. `02-long-details.png` and `01-long-today.png` were preserved as `BUG-long-category-*.png`.
- Console noise: expected 401 on `/api/me`, 404 on the create probe, and Permissions-Policy warnings for two unrecognized features. No uncaught page errors in any run.

## Timings (localhost, not representative of a phone or network)

| Action | Time |
| --- | --- |
| First load to auth form | 1021 ms |
| Setup owner | 126 ms |
| Create household after key | 103 ms |
| Add entry (submit to closed) | about 60 ms |
| Mark paid | 233 ms incl. 200 ms wait |
| Remote edit visible on other device | 1374 ms (poll is 4 s, so up to about 4 s) |
| Reload while offline to status | 61 ms |
| Conflict appears after reconnect | 30 ms |
| Reapply conflict | 844 ms |
| Unlock with key | 84 ms |

## What worked

Setup, household creation, unlock, add income and expense, recurring edits (this and future split correctly), move, delete, mark paid, checkpoint anchoring, HTML escaping in notes, editor draft surviving background sync (5 s test), offline add, offline reload with SW, pending survival across reload, conflict detection and reapply retaining both sides, encrypted backup with no plaintext leak (`plainLeak:false`), backup restore, session-expiry recovery, revocation blocking. Username case is normalized by the server (`server.mjs:37`), so `Partner.Test` and `partner.test` both work. 320px width has no page overflow with normal data.

## Findings

### High

**H1. Service worker update path can pin households on old code, and a version bump may not fix it.**
- `sw.js:3` installs with `cache.addAll(SHELL)`, which goes through the HTTP cache. `server.mjs:262` serves `.mjs`, `.css` and `.wasm` with `max-age=3600`. `sw.js:5` is cache-first for all shell files with no revalidation and no `skipWaiting`.
- Reproduced on a copy of `public/`:
  - Change `app.mjs` only: old code after 3 reloads and after closing and reopening.
  - Change `app.mjs` and bump `CACHE`: new cache `budget-household-0.8.5` was created, and it contained the OLD `app.mjs` (`{"0.8.3":false,"0.8.5":false}` for the new marker). New worker sat `waiting:true` across reloads.
- Impact: a security or money-calculation fix shipped within an hour of a client's last fetch is baked into the new cache, and that client keeps the old JS and WASM until a further bump. Engine fixes do not reach phones. Nothing tells the user. Nothing enforces a bump either (the cache is still named 0.8.3 at 0.8.4; `public/` did not change, so that is not a defect today, only an unenforced manual step).
- Fix direction: fetch with `cache:'reload'` during install, content-hash the name, prompt to reload when a worker is waiting.

**H2. Being offline, the headline feature, presents as a failure with a raw browser error.**
- `app.mjs:94` stores `error.message` for any sync failure; the 4 s timer at `app.mjs:175` and tab changes at `app.mjs:157` still call the network while offline. `app.mjs:26` ranks `syncError` above `!navigator.onLine`.
- Observed: `Offline | Last synced copy` flips within 5 s to red `Sync failed | Local copy available` plus a pink box reading `Failed to fetch  Retry` (`p2b-16-offline-reloaded.png`). A toast repeats it on tab change. On a phone in a parking garage, the app says it broke. With pending edits the header says "Offline · pending" while the box beneath says "Failed to fetch".
- Fix direction: skip sync while `!navigator.onLine`, map TypeError to friendly copy, keep amber.

**H3. Revoked member with pending edits loses all access to them after one reload.**
- `app.mjs:48-49`: an empty `/budgets` sets `state=null`. Before reload the box says "Your pending work remains on this device; export a backup for recovery" (`app.mjs:94`).
- Reproduced: offline add, owner revokes, reconnect, reload. Result: "Make room for the days ahead" with buttons Create / Accept / Sign in again / Sign out; `hasExport:0` (`p7-09-revoked-pending-reload.png`). The data sits in IndexedDB with no UI route to it, so the app's own recovery instruction becomes impossible. Reload also happens by itself when Android discards the tab. A `beforeunload` prompt was the only guard.
- Same line family: the data also stays readable on disk after sign-out by design (documented), so this is about recoverability, not privacy.

**H4. Conflict review shows the user 14 KB of raw internal JSON.**
- `app.mjs:144` dumps `JSON.stringify(v.working)` in a `<pre>`. Measured 13,955 chars with a near-empty household; opens on `"adjustments"`, `"flags": 48`, `"hilighting"` (`p2b-19-conflict-review.png`). A real budget will be far longer.
- The user is asked to pick a side, one of which is destructive, and cannot tell what differs. There is no "you added X, they added Y". Copy says "Server revision 24" and "1 pending changes".
- Worse, any pending edit plus any remote edit is a blocking conflict (`state.mjs` `receive`), even for unrelated entries, and all edits are refused until resolved. Yet the command queue already merges these cleanly: reapply kept both `OfflineCoffee` and `OwnerConcurrent`. For two spouses with one on patchy signal, this modal will be routine. Auto-reapply when entries do not overlap; ask only on a true collision.

**H5. New households have no starting balance, so the hero number is fiction from day one.**
- `app.mjs:32` renders $0.00 with no prompt. The empty state says "Add your first expense, income, or bill". Set balance exists only as a small quiet link on Calendar (`app.mjs:33`), never on Today. After adding one $12 expense the hero shows -$12.00 with amber lowest balance. After 4 realistic entries the app reported a confident $1,967.51 from nothing. This is the product's core promise and the first thing a new user sees.

**H6. Set balance dialog destroys an existing checkpoint on Save with no warning, and checkpoints are otherwise invisible.**
- `app.mjs:126` never prefills. Reproduced: checkpoint 1500.25 set; reopen shows empty field (`11-checkpoint-reopen.png`); tapping Save immediately removed it: Oct 30 went from $1,500.25 to $1,967.51, with the generic "Saved on this device" toast. No undo. The help line says blank removes, but someone opening to check the value has no other way to see it.
- Checkpointed days have no marker in the grid or day panel (`anyCheckpointText:null`, `10-calendar-after-checkpoint.png`). Users cannot tell an anchored figure from a computed one, and every balance after it silently depends on it.

### Medium

**M1. A long unbroken note or category wrecks the layout persistently, on every device.**
- `styles.css` `.help` and `.entry-sub` have no `overflow-wrap`. A long day note with no spaces gave `scrollW:5742` on a 390 viewport: the mobile layout zooms out, and bottom navigation became unclickable in my test (`BUG-long-daynote-calendar.png`). A 190-char unspaced category does the same on Today, and the detail sheet renders off-screen so its buttons are not reachable (`BUG-long-category-details.png`). A pasted URL in a note is the realistic trigger. It syncs, so it breaks the partner's phone too. My test string was deliberately extreme (840 chars); threshold not measured.

**M2. Sign-out drops the owner on First setup.**
- `authMode` is never reset (`app.mjs:150-153`, `app.mjs:11`). After signing out, the tab was `First setup` and the submit was `Create owner account` (`p7-07-after-signout.png`). The required token field fails validation (`checkValidity:false`), so retyping credentials cannot sign in until the user notices the tab and switches. Setup is permanently disabled at that point, so the tab is a dead end on any live server too (`app.mjs:27` always shows it).

**M3. Android back and Escape discard the entry draft with no warning, and in-app navigation adds no history.**
- `modal` (`app.mjs:23`) has no `cancel` guard. Filled amount and description, pressed Escape: sheet closed, reopen blank. Same with `×`. Tab changes make no history entries (history length 2 after Calendar then Settings), so back leaves the app rather than the previous tab. Hardware not confirmed.

**M4. Dismissing unlock strands the member on a misleading "create" screen.**
- `app.mjs:62` then `app.mjs:29`. Escape/× on unlock shows "Make room for the days ahead" with Create household first and no Unlock button (`p2b-08-unlock-dismissed.png`). Only a reload recovers it. A new invited partner can easily create a separate empty household, which is the failure the invite exists to avoid.

**M5. Recovery key and invite code have no Copy or Share action.**
- `app.mjs:109` and `app.mjs:146`: the only buttons are `×` plus Create or Cancel. The app instructs saving a 47-char key in a password manager and sending it privately, from a phone, via long-press. `user-select:all` helps slightly. On Android no screen ever shows the key again: Settings has no reveal (`recoveryShow:false`), unlike Windows since 0.8.1. A phone-only owner who skipped saving cannot onboard a partner. The confirmation is a self-attested checkbox with no verification.
- Related: the key rejected when wrapped by a messaging app. A newline mid-key gave "Enter the complete recovery key" (`app.mjs:75` only trims ends).

**M6. Invitations cannot be seen or cancelled after closing the dialog.**
- `app.mjs:146`: Cancel exists only on the one-time screen. Closing it leaves a live 48 h code with no list and no way to revoke it. If the code goes to the wrong person, the owner has no action beyond waiting. (Codes are recipient-bound, which limits this.)

**M7. One-time entries are edited as if they were a series.**
- `editor-scope` default is `one` for every record. Editing a one-off Groceries entry: description, date and category are disabled, the scope list offers "This and future occurrences" and "Whole series" (`13-edit-onetime.png`). To fix a typo you must pick "Whole series" on something that has no series. Delete offers "Only Nov 3" and "Whole series" (`14-delete-onetime.png`) with copy about tombstones. Delete has no undo.

**M8. Add entry is a 20-control, 1533px form.**
- Quick expense requires a sheet showing all recurrence, loan, grace, sticky controls open by default (`app.mjs:121`, `<details ${record?'':'open'}>`). Submit sits at y=1527 in a 776 sheet, about two screens of scrolling. No autofocus on Amount: focus lands on `×`. Category has no suggestions from prior entries. The common action in a budget app should take two fields.

**M9. Calendar is unreadable with ordinary balances and has no month arrows.**
- Cell labels are 8.48px with no shrink logic. `$12,345.67` measures 45px in a 38px cell at 390 and 28px at 320, so labels collide and the selected cell clips to `$12,34` (`p4-02-cal-12k-320.png`). A $12k balance is normal. No prev/next or swipe (`monthNavButtons:0`); the month picker is the only route. At 320 the title and picker crowd.

**M10. Today omits overdue bills and hides upcoming ones without saying so.**
- `app.mjs:32`: upcoming starts today, so unpaid past items (Phone, Oct 20) appear nowhere on Today. `slice(0,12)` truncates: 15 bills, 12 shown, no "3 more" (`totalMention:null`), though the totals include all. Totals also count already-paid items. There is no overdue view, so paid status has little to act on. (In my mark-paid test the balance did not move, but that day was checkpointed, so this is not proof that paid never affects figures.)

**M11. Bottom sheet under the keyboard (code-level; not verified on hardware).**
- `index.html` viewport has no `interactive-widget=resizes-content`, and `dialog` is fixed to the bottom with `max-height:92dvh`. Chrome Android's default overlays the keyboard on the visual viewport, so the sheet may not shrink. In the viewport-shrink simulation the fields stayed visible, but that does not model this default. Test on a device.

**M12. No dark mode.**
- No `prefers-color-scheme` in `styles.css`, no `color-scheme` (`["rgb(243, 247, 244)","normal",true]` under dark emulation; `18-dark-today.png`). For a phone-first app checked at night, the page stays bright, and native controls do not adapt. `theme-color` is fixed.

**M13. Second-tab screen does not recover by itself.**
- `writer-lock.mjs` uses `ifAvailable`. After closing the owning tab, the paused tab stayed paused 3 s later and needed Retry. On Android an old tab behind the installed PWA is hard to find, and the instruction assumes tabs are visible. Could queue on the lock. Frozen-tab behaviour unverified.

**M14. Raw exception text reaches users.**
- Bad restore file: `Unexpected token 'o', "not json" is not valid JSON` (`app.mjs:133`). Bad time zone: `Invalid time zone specified: Mars/Base` (`app.mjs:107`, a free-text field where a picker belongs). Blank description: "Enter a description of at most 4096 bytes." which names a limit the user did not hit. Failed auth clears every field including username and token (`app.mjs:27`, `usernameKept:""`).

**M15. Large figures break the page width.**
- `.metric strong` does not wrap or scale: at `$1,000,000,006,699.99` page width was 408, and at $10,002,739.98 expenses 457 with bottom nav off-screen (`16-large-today2.png`). Extreme values, but seven-figure totals are reachable with imported data. `999999999999.99` was accepted without a sanity check.

### Low

- L1. Toast "Saved on this device · pending sync" stays 6.5 s after status already reads Synced, sits at `bottom:180px` over content, and covers calendar cells (`09-calendar.png`).
- L2. FAB covers the last row amount at scroll end: `$2,100.00` is partly hidden (`08-today-scroll-end.png`). `.app-shell` padding 110px is less than FAB top offset 149px.
- L3. Activity does not refresh on incoming changes (22 stayed 22 after 5 s), and needs manual Refresh.
- L4. History review shows only "16 recurring or one-time records" and a restore button: nothing to judge it by.
- L5. Dates lack a year across year boundaries: `NextYear` row reads "Jan 1" on 2027 data (`app.mjs:14`).
- L6. Edit shows `42.5`, not `42.50`. `1e3` is accepted as an amount and saved as $1,000.00.
- L7. Revoke uses a native `confirm()`, off-style, in a designed app (`app.mjs:164`). Same for discard.
- L8. Nav inactive contrast 3.34:1 at 10.4px; eyebrow 4.36:1 on page; quiet buttons 35px tall, below 44px; icons are Unicode glyphs that vary by device.
- L9. Manifest lacks a maskable icon, shortcuts and screenshots.
- L10. Detail sheet title with long text squeezes the close button to an oval (`[366,352]` overflow).
- L11. "Add a day note" stays that label when a note exists. `1 pending changes` grammar at `app.mjs:144`.
- L12. Sign-in form shows "At least 12 characters." on login, where it is meaningless.
