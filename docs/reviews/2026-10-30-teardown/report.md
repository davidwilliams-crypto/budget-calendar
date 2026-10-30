# Budget Calendar teardown report

Version 0.8.4, branch `codex/android-household-sharing` at 268e47c. Review date 2026-10-30. Read-only review; generated test data only.

## Verdict

Not ready to be trusted with a household's real money, and the sync service should not be deployed yet. The foundations are better than that sounds: the workspace tests pass (536 passed, 4 ignored), the encryption held up in testing, desktop speed is excellent, and offline editing works. No finding reached critical severity. The product's weakness is trust: it can be wrong or lose work without saying so. The single biggest risk is that an ordinary phone edit quietly changes the plan: fixing a typo can move every later payment or mark a whole series PAID, nothing warns the user, and there is no undo. In a forecasting app, a confident wrong number is worse than an error message.

For the "trend setter" goal: the idea is distinctive and the raw material exists, but today's first-run experience is behind the market. Fix trust first. That claim is the market's to give; it cannot be built by polish alone.

## Scorecard

Grades are the review lead's judgement from the findings below, not a measured score.

| Area | Grade | Why |
|---|---|---|
| Core calculation engine | B- | Solid in common cases and well tested; the window-start bug and opening balance booked as income are real |
| Phone edit flows (money accuracy) | D | Four high defects where the form sends values that silently change the schedule or totals |
| Desktop file safety | C | Atomic saves and sealed files are good; no autosave, no restore UI, retention can erase the last good copy |
| Household sync | D+ | Basic path works; conflicts lose data, cost grows with use, and a household freezes after about 2,500 edits |
| Security and privacy | C+ | Budget contents stay encrypted on the server; account controls, key rotation and on-device protection are missing |
| Server operations | D | No useful logs, no migrations, no real health check, fragile rate limiting |
| Desktop UX | C+ | Fast and stable, but reconcile is hidden and every task is a modal |
| Phone UX | C- | Looks pleasant, but Today shows no bills, Paid does nothing, and offline looks like failure |
| First run | D | Neither platform reaches a true forecast; no starting balance, import or sample |
| Accessibility | D | Core money tasks need a mouse on Windows; one crash affects assistive tech users |
| Reports and insight | D+ | Limited range, one wrong percentage, no phone reporting, dated output |
| Visual design | C+ | Dark mode and dense layouts are strong; the two platforms look unrelated and the balance is never drawn |
| Desktop performance | A- | 500 payments open in about 0.6 s; month paging 17-35 ms; idle CPU 0 ms |
| Release engineering | C- | Archives are careful, but no CI, no signing, no update path, and mobile tests sit outside the release steps |

## Fix before anyone trusts this with real money

The 24 rows below hold the 18 high findings from the deep review (two are merged in #6) plus 7 high items from the live walkthroughs. Nothing reached critical. Effort: S under a day or so, M a few days, L a week or more, XL multiple weeks (rough estimates).

### Wrong figures

| # | Where | What happens | Fix | Effort |
|---|---|---|---|---|
| 1 | `budget-mobile/public/app.mjs:122` | A "this and future" edit always rewrites the start date from the displayed occurrence, even when only the category changed. Rent due on the 1st but shown on Mon Nov 2 moves to the 2nd forever; a month-end mortgage edited from February moves to the 28th (reproduced on the shipped WASM engine). A whole-series edit keeps the start date unless the date box is changed, but changing it removes every earlier occurrence from the schedule. Desktop shows the same moved schedule after sync | Send the original start date unless the user changed the date; preview any real move | S |
| 2 | `budget-mobile/public/app.mjs:122` | Fixing a typo on a paid occurrence marks every occurrence of the series PAID (reproduced on all 15). `get_flag` falls back at `budget-core/src/model.rs:229` | Never write occurrence status into the series default | S |
| 3 | `budget-mobile/public/app.mjs:121` | "Last day of month" actually means the last matching weekday; "Weekdays" means one nth weekday a month. A daily weekday expense posts monthly, understating spending by about 95% | Rename to match the engine, hide Every, snap dates in the shared engine | S |
| 4 | `budget-mobile/public/app.mjs:32` | Next-31-days totals and Coming up include disabled payments, so tiles disagree with the balance | Filter them, or return totals from the engine | S |
| 5 | `budget-gui/src/main.rs:761` (window: `budget-core/src/db.rs:220`) | Paging back to the first month of the loaded window shows balances built from zero and books the whole bank balance as income. Going one month further shows different, correct figures. Reports print the bad month | Treat the first window month as unavailable; build reports from a full-history copy; test that figures do not depend on navigation | M |
| 6 | `budget-gui/src/calendar.rs:516`, `budget-gui/src/main.rs:2143` | A new user never reaches a real forecast: no starting balance step, guidance vanishes after one payment, and the strip's hand cursor does nothing on left-click. A seed balance then appears as income (`budget-core/src/stats.rs:198`, `:249`) | Guided first run with one definition of done; single-click reconcile; store a true opening balance | L (calendar click alone is S) |
| 7 | `budget-core/src/stats.rs:249` (shown `budget-gui/src/main.rs:1047`) **[walkthrough]** | Money out counts balance corrections as spending: 17,473 shown against about 4,029 of real payments in the test data. The explaining panel is absent at 1280 wide | Show adjustments on their own line, outside Money in and out | M |
| 8 | `budget-mobile/public/app.mjs:126` **[walkthrough]** | Set balance is not prefilled. Reopening it to look and tapping Save deleted a 1,500.25 checkpoint with no warning. Checkpoints have no marker anywhere | Prefill, require an explicit remove, add a marker | S |

### Lost work and lost data

| # | Where | What happens | Fix | Effort |
|---|---|---|---|---|
| 9 | `budget-gui/src/main.rs:1464` | No autosave or crash journal. A panic, power loss or Windows restart loses everything since the last Ctrl+S, and the next launch does not mention it. The `autosave` setting exists but nothing reads it. Shutdown loss is inferred from code, not tested with a reboot | Debounced encrypted recovery file, restore offer on open, panic hook | L |
| 10 | `budget-gui/src/main.rs:2179` | Closing a dialog opened from a keyboard-focused menu item crashes the app when an accessibility client is active (5 of 5 tries, including View > Calendar legend > Esc). Unsaved work goes with it. A skeptic could not reproduce it through a UI Automation invoke alone, and plain mouse users are very likely safe, from code reading rather than a live test | Restore focus only to widgets that still exist; add a test with accesskit on | S |
| 11 | `budget-gui/src/sharing/panel.rs:132` | "Use current shared version" discards all queued offline edits with one unconfirmed click, with no copy kept. Phone asks first | Confirm with a count, and keep a recoverable discarded copy | S |
| 12 | `budget-gui/src/shared_client.rs:168` | Conflict resolution silently loses the other person's same-record changes, such as a paid mark, and Windows revives remote deletions while phone refuses. Copy on phone says the opposite | Field-level three-way merge; replay intent, not whole records; explicit delete-versus-edit prompt | L |
| 13 | `budget-mobile/public/app.mjs:109` | A phone-only household can lose its budget permanently. Android can never re-show the key, and "encrypted backups" are useless without it. The app does not say so plainly | Plain warning, proof the key was saved, a recovery key row in Settings, re-show behind a device gesture | L |
| 14 | `budget-core/src/sharing.rs:277` | A growing budget locks every phone edit, then Today. With a 300-month loan, this hit at 1.9 years of 3 entries a day. Add, delete and checkpoint all fail with the same text, so the user cannot fix it | Stop scaling work by days times all payments; always allow delete and checkpoint; add archiving and early warning | L |
| 15 | `budget-mobile/public/app.mjs:48` **[walkthrough; deep review rated this medium]** | A revoked member with pending edits loses all access to them after one reload, right after being told to export them | Open a read-only recovery view from local data | M |
| 16 | `budget-mobile/public/app.mjs:144` **[walkthrough]** | Conflict review shows about 14 KB of raw internal JSON and blocks all editing, even for unrelated edits that merge cleanly | Auto-merge non-overlapping edits; show a human difference for real collisions | M to L |

### Serious trust and access failures

| # | Where | What happens | Fix | Effort |
|---|---|---|---|---|
| 17 | `budget-mobile/server/server.mjs:246` | Each commit receipt stores the full encrypted budget forever. On an 80 KB budget, commit 2,498 returned `507 storage_limit`: about 4 months at 20 edits a day. Every device then retries with no backoff. Commit time also doubled by 2,400 commits | Keep only a small receipt, keep byte counters in a row, add a client state for 507 | M |
| 18 | `budget-mobile/server/server.mjs:340` | No password change, no sign-out-everywhere, no session list. A stolen phone cannot be cut off by its owner for up to 7 days | Add both endpoints and UI on both clients | M |
| 19 | `budget-gui/src/sharing.rs:272` | A shared budget does not reopen at launch. Queued edits sit unsynced with no warning, and the user can drift into an old standalone file | Remember the household, auto-open it, show unsynced counts | M |
| 20 | `budget-gui/src/calendar.rs:255` | Reconciling, moving a bill and restoring need a mouse on Windows. Strips advertise a button that does nothing (WCAG 2.1.1, 2.5.7) | Focusable day cell; put these actions in the day dialog; Shift+F10; Move-to-date | L |
| 21 | `budget-gui/src/reports.rs:250` | Regular reconcilers can report only about four past months, and the app falsely says the rest does not exist. An unrelated loan changes the range | Offer all months, built from a full-history copy; remove the wording | M |
| 22 | `budget-mobile/public/sw.js:3`, `:5`; `budget-mobile/server/server.mjs:262` **[walkthrough; deep review split this into two mediums]** | Phones can stay on old code. A test bump created a new cache holding the old `app.mjs`, and the new worker stayed waiting. Every money fix above depends on this path | Fetch with `cache:'reload'`, generate the cache name from a hash, prompt to reload | M |
| 23 | `budget-mobile/public/app.mjs:26`, `:94`, `:175` **[walkthrough; deep review rated this medium]** | Offline, the headline feature, turns red within 5 seconds with "Failed to fetch" | Typed offline state, no sync attempts offline, amber status | S |
| 24 | `budget-mobile/public/app.mjs:32` **[walkthrough; same root cause as #6]** | Phone hero is a confident $0.00, then a figure built from nothing | Show "Add your balance to start" until one exists | S |

## Ship-readiness blockers for the household service

Do these before the first real household. Several are cheap only because nothing is deployed yet.

- [ ] Replace receipt storage so normal use cannot freeze a household (`server.mjs:246`, `:238`).
- [ ] Add password change and session revocation (`server.mjs:340`), an admin tool for account recovery, and restrict who can create budgets (`server.mjs:358`).
- [ ] Prove phone updates work end to end, including schema mismatch and a user-visible reload (`sw.js:1`, `:3`).
- [ ] Add a schema version and migration runner before any data exists (`server.mjs:96`).
- [ ] Add per-non-2xx structured logs and a `/ready` check that touches the database (`server.mjs:422`, `:279`).
- [ ] Refuse to start on an empty, unmounted data directory (`server.mjs:92`).
- [ ] Add a restore marker so clients notice a server rewind, then write and test the restore runbook. Today clients stay silently stuck (`state.mjs:11`, `shared_client.rs:143`). Verified backups are already a stated deployment gate in `DEPLOYMENT.md:37-41`.
- [ ] Fix client identity behind proxies and IPv6 /64 rotation, and count only failures toward the sign-in limit (`server.mjs:142`, `:144`, `:292`).
- [ ] Add a data-key layer so a key can be rotated after revocation. This format change gets much more expensive after launch (`app.mjs:164`).
- [ ] Add KDF parameter columns and raise the cost (`server.mjs:14`, marked unverified).
- [ ] Add backoff and classify permanent errors on both clients (`app.mjs:87`, `sharing.rs:403`), and a cheap revision check so idle clients stop downloading the whole budget (`app.mjs:93`, `sharing.rs:405`). Measured on Windows: about 108 MB an hour per open window.
- [ ] Add a set-time-zone command and a validated picker. A phone-accepted zone can make a household permanently unopenable on Windows (`app.mjs:108`, `sharing.rs:451`).
- [ ] Update chrono-tz before 2026-11-01. Windows and current tz data disagree for British Columbia from 2026-11-02 (`shared_client.rs:117`).
- [ ] Close the open product decision in `budget-mobile/README.md:43`, and correct the sign-in copy that says data is encrypted on the device (`state.mjs:3`, `app.mjs:27`).
- [ ] Run the whole flow on a real Android phone. Nothing here has been tested on hardware.

## Theme-level patterns

1. **Silent failure is the house style.** Truncated files open as healthy, conflicts drop changes, repairs on load go unmentioned, a dropped CSV does nothing, and server 500s log nothing. A fix applied one bug at a time will miss the next. Adopt a rule: any change the user did not ask for must be visible.
2. **Business logic lives in the phone form.** The biggest money bugs come from JavaScript assembling a whole record to send to a correct engine. Move intent into the engine (send only changed fields, return totals from the engine) so two clients cannot diverge.
3. **Whole-snapshot design meets real use.** Receipts, the pending queue, polling, history and commit cost all grow because each tap copies the whole budget. Coalescing and deltas fix a family of findings at once.
4. **The balance checkpoint is underspecified, and it carries the product.** It is an end-of-day figure while the app asks for a bank figure; it is booked as income or spending, invisible on phone, and silently deletable. Many findings touch it.
5. **Engine vocabulary reached the user.** Tombstone, replay queue, revision, checkpoint, sticky, occurrence. The two platforms also name the same things differently.
6. **Built for a sighted mouse user.** Accessibility is a late layer: flat screen-reader trees, nothing announced, and pointer-only money tasks.
7. **Legacy fidelity was kept past its usefulness.** Reports preserve original quirks, including a wrong percentage, instead of treating the legacy as a source of behaviour worth checking.
8. **Release quality depends on memory.** No CI, no check on the shipped WASM, optional mobile tests and manual cache names. The earlier rounds' care is real, but nothing enforces it.

## Medium and low findings

153 medium and 57 low. Rows are kept even where they overlap, so each file reference survives. All medium findings were confirmed. Low items marked † were read from code but not run (one is rated plausible). Paths are shortened: `app` = `budget-mobile/public/app.mjs`, `server` = `budget-mobile/server/server.mjs`, `gui/` = `budget-gui/src/`, `core/` = `budget-core/src/`, `css` = `budget-mobile/public/styles.css`.

### Medium: money and core

| Where | Issue | Effort |
|---|---|---|
| `core/schedule.rs:356` | Sticky loan overpays principal with no warning (1,600 paid on 1,000) | S |
| `app:121` | Whole-series edit saves the occurrence's amount as the series amount | S |
| `core/commands.rs:385` | Split records are unlinked, so later edits break loan totals and stop short of "all future" | L |
| `core/stats.rs:198` | Starting balance counts as income in summary and reports | M |
| `core/sharing.rs:277` | One mistyped year can brick a household on every device, and the entry cannot be deleted | M |
| `app:126` | Bank figure is stored as end of day, overstating the forecast by whatever is still due today | M |
| `core/report.rs:385` | Comparative report prints a wrong percentage with the opposite sign to its amount | S |
| `core/stats.rs:198` | Adjustments are the only evidence of real spending, and the forecast ignores them | L |
| `core/config.rs:277` | Low-balance floor has no UI, is not shared, and platforms warn by different rules | M |
| `app:13` | Every amount shows CA$ on phones set to English (US); no currency setting | S |
| `app:13` | Money and dates differ by device; hard-coded CAD, Sunday start, no language support | L |
| `gui/shared_client.rs:138` | A number that fails serde_json's float round trip makes Windows reject its own accepted commit | S |

### Medium: files, backups and upgrades (desktop)

| Where | Issue | Effort |
|---|---|---|
| `core/bgt.rs:351` | A truncated or empty .bgt opens as healthy, then the next save makes the damage permanent | M |
| `gui/main.rs:582` | Backups exist, but the app offers no way to find or restore them | M |
| `core/bgt.rs:629` | Backup retention counts saves, not time; ten quick saves erase the last good version | M |
| `core/bgt.rs:539` | Saving replaces the file object, losing private ACLs, hidden attribute and hard links | M |
| `gui/document.rs:13` | External changes are found only at save; a moved file is silently recreated | L |
| `gui/main.rs:1838` | No undo or redo anywhere | M |
| `gui/main.rs:734` | Save and file-association opens block the UI thread on slow drives | M |
| `gui/main.rs:734` | Sealed passwords can never be changed, removed or set | M |
| `core/bgt.rs:413` | First Save silently makes a budget unreadable to older installs, which report it as corrupt | M |
| `core/bgt.rs:533` | Retained legacy backups let an attacker bypass Argon2id | M |
| `gui/sharing.rs:443` | Budgets that open normally are refused for sharing, with no payment named | M |
| `gui/reports.rs:122` | Reports cannot be saved, and an Excel report edited in place is deleted at exit | M |
| `gui/reports.rs:122` | Plaintext report temp files survive any crash or kill | S |
| `gui/main.rs:2226` | A crash or startup failure leaves no message, log or report | M |

### Medium: sync, household and sharing

| Where | Issue | Effort |
|---|---|---|
| `budget-mobile/public/state.mjs:11` | Reapply is all-or-nothing; one stale edit blocks the rest | M |
| `budget-mobile/public/storage.mjs:3` | Phone storage is neither persistent nor strictly durable, despite "Saved on this device" | S |
| `gui/shared_client.rs:145` | Non-overlapping edits always stop for manual review; the auto-merge path is never called | M |
| `budget-mobile/public/state.mjs:6` | Offline replay can erase all 50 history revisions; queue grows quadratically | L |
| `app:93` | Phone poll downloads, decrypts and rewrites the budget every 4 s when nothing changed | S |
| `gui/sharing.rs:405` | Windows "quiet" sync still downloads the whole budget (about 108 MB an hour) | M |
| `gui/sharing.rs:165` | A failed household creation can block all future creation, with no cancel | S |
| `gui/sharing.rs:181` | Offline edit cost grows on the UI thread and eventually hits a hard refusal | L |
| `gui/shared_client.rs:122` | Ctrl+S during a conflict blocks resolving it until the app closes | S |
| `app:48` | Pending work for a revoked or unlisted household becomes unreachable (see #15) | M |
| `budget-mobile/public/state.mjs:11` | After a server restore, clients silently ignore the server and edits never sync | M |
| `app:144` | "Export my pending work" saves a snapshot whose restore would undo the partner's work (finding's path had a doubled `public/`; corrected here) | M |
| `app:144` | Conflict wording hides the person, and resolving can silently overwrite the partner | M |
| `budget-mobile/public/state.mjs:6` | Android queues a commit for a no-op save, unlike Windows | S |
| `app:49` | A removed member gets no explanation and loses access to their record | M |
| `server:383` | No way to leave, transfer ownership, or delete a household or account | L |
| `app:146` | Joining needs four items sent by hand, and no screen says what is needed | L |
| `app:119` | Neither person is told what sharing exposes or what roles mean | M |
| `app:28` | A pasted invitation code with a trailing space is reported as expired or used | S |
| `server:202` | Sessions expire after a hard 7 days, with a Retry button that cannot work | S |
| `app:34` | No safety design for financial control: exact-time tracking, no private entries | XL |
| `gui/sharing/panel.rs:182` | No sign-out on Windows, no "remove from this device" anywhere, no app lock | M |
| `gui/sharing/panel.rs:75` | Credentials have no coherent mental model; privacy is a slogan | M |
| `app:108` | A zone the phone accepts can make a household unopenable on Windows | M |
| `core/sharing.rs:451` | Household time zone can never be changed | M |
| `gui/sharing.rs:89` | Windows households default to America/Halifax instead of the PC's zone | S |
| `gui/shared_client.rs:117` | Bundled tzdata disagrees with phones on "today" in BC from 2026-11-02 | S |

### Medium: security and privacy

| Where | Issue | Effort |
|---|---|---|
| `budget-mobile/public/state.mjs:3` | Whole budget sits in plaintext IndexedDB after sign-out, while copy says encrypted | M |
| `server:142` | Rate limiting uses full IPv6 addresses; one /64 bypasses it, and a flood locks out new clients | M |
| `gui/sharing.rs:415` | Recovery key copied to the clipboard enters clipboard history and is never cleared | S |
| `app:164` | Household key can never be rotated | L |
| `gui/sharing.rs:206` | "Export local backup" writes a plain .bgt with no warning; web password export is unreachable | S |
| `gui/sharing.rs:439` | Household name defaults to the file name and is uploaded readable; summaries reveal action types | S |
| `server:358` | A revoked member keeps an account and can fill all budget slots | M |

### Medium: server and release

| Where | Issue | Effort |
|---|---|---|
| `server:238` | Every commit re-reads all receipts, blocking the single thread | S |
| `server:92` | A missing volume silently creates an empty database and reports healthy | S |
| `server:422` | 500s are unlogged and `/health` never touches the database | S |
| `server:144` | Behind a non-Render proxy, 10 bad requests block every sign-in | S |
| `server:247` | History is counted in edits, so the restore window may be one session | M |
| `installer/budget.nsi:37` | Executable needs the VC++ runtime, which is not provided (exposure on clean PCs untested) | S |
| `AGENTS.md:9` | No CI or single release gate; most mobile tests are outside the documented path | M |
| `scripts/package-mobile.ps1:62` | Nothing detects a stale shipped WASM engine | S |
| `installer/build-installer.ps1:95` | Unsigned releases with no version metadata; SmartScreen will warn | M |
| `core/sharing.rs:11` | No Windows update path or minimum-client signal; newer snapshots are rejected | L |
| `installer/budget.nsi:40` | Decompiled vendor code is tracked and the vendor sample ships. Get legal advice before wider distribution | M |
| `rust-toolchain.toml:2` | Pins a floating GNU toolchain no release uses; real setup is undocumented | S |
| `installer/budget.nsi:37` | Installer has no running-app handling and has never been exercised | M |
| `scripts/package-mobile.ps1:34` | Mobile package "immutability" lives in `target/`, with no commit identity | M |
| `budget-mobile/tests/three-client-integration.mjs:47` | Three-client test silently drops its Windows step and passes | S |
| `budget-engine/tests/parity.mjs:23` | Parity test covers few schedule features and runs in no test path | M |

### Medium: phone engineering

| Where | Issue | Effort |
|---|---|---|
| `app:176` | Cold start waits on the network before local data; requests have no timeout | M |
| `budget-mobile/public/sw.js:3` | No update path; old clients fail on newer snapshots (see #22) | M |
| `budget-mobile/public/sw.js:1` | Install can pin stale or mixed assets; manual cache name (see #22) | M |
| `app:26` | Offline reported as failure with a raw browser error (see #23) | S |
| `app:37` | Clearing the month picker breaks Calendar until restart | S |
| `app:157` | Android Back has no in-app meaning and silently discards forms | M |
| `app:62` | Dismissing unlock strands the user on the create-household screen | S |
| `budget-mobile/public/writer-lock.mjs:7` | The installing Chrome tab can lock out the installed app (device behaviour unverified) | M |
| `app:87` | Failed commits retry every 4 s, ignoring Retry-After and permanent errors | S |
| `app:176` | Boot treats any server status as fatal; 7-day session hides local data | S |
| `budget-mobile/public/index.html:2` | On-screen keyboard is unhandled for bottom sheets (not device-tested) | S |
| `budget-mobile/public/sw.js:5` | Queued edits stay on the device until the app reopens | L |
| `app:157` | Activity and Settings do not open until the network answers | S |

### Medium: everyday flows and copy

| Where | Issue | Effort |
|---|---|---|
| `gui/view.rs:236` | Nothing on desktop answers "when is my next low point?"; forecast graph was dropped | L |
| `gui/editor.rs:374` | "Can I afford this?" has no answer without changing the budget | XL |
| `gui/balance_editor.rs:56` | Reconciling is blind: no difference shown; an uncleared bill overstates the forecast | L |
| `gui/calendar.rs:585` | Paid is a nearly invisible label: no overdue state, four steps to set | M |
| `gui/main.rs:1827` | Stopping a bill is offered only as delete, which rewrites history | S |
| `gui/editor.rs:567` | Every interaction is a modal that hides the numbers it changes | L |
| `gui/main.rs:1724` | New payment ignores the viewed month, then jumps away | S |
| `gui/editor.rs:966` | Add form is shaped like the engine: ISO-only dates, no schedule preview, no duplicate | M |
| `gui/editor.rs:99` | No pay presets; one full dialog per item | M |
| `gui/main.rs:1848` | No bank or budget import; dropping a CSV shows "Drop to open", then does nothing | XL |
| `installer/budget.nsi:39` | First launch has no way to start; installed sample is orphaned | M |
| `budget-mobile/ENGINE-CONTRACT.md:29` | Existing .bgt budgets cannot reach the phone directly, though the engine can import | M |
| `app:124` | Implementation language reaches users | S |
| `app:126` | Desktop and phone use different words for the same concepts | M |
| `app:121` | Advanced options misdescribe Tentative, Reminder, Sticky and Pay early | S |
| `app:32` | Forecast does not show how fresh it is | M |
| `server:38` | Server error text shown verbatim, despite stable codes | S |
| `app:121` | No notifications; the Reminder checkbox does nothing | XL |
| `app:158` | Tapping a day shows nothing new; tabs keep the old scroll position | M |
| `app:159` | Paid is a dead end: three taps, no effect, no undo | M |
| `app:102` | Saves never show the effect on the forecast | M |
| `app:26` | Synced timestamp misleads; offline shows no age | S |
| `app:34` | Activity is a server log; partner changes arrive silently | L |
| `app:100` | During a conflict the app accepts a full form, then refuses and traps the draft | M |
| `app:32` | Today's first screen shows no bills and spends space on slogans | M |
| `app:121` | A hidden invalid field makes Save do nothing, with no message | S |
| `app:35` | Install is buried; first launch may be blocked by the browser tab | M |
| `css:2` | Sheets are hard to reach and dismiss with one hand | M |
| `app:33` | Calendar and hero show a coming overdraft with almost no signal | M |

### Medium: reports

| Where | Issue | Effort |
|---|---|---|
| `core/report.rs:268` | Three reports lose all styling because the CSS is entity-escaped | S |
| `app:33` | Phone has no reporting: no categories, month totals or plan comparison | L |
| `core/report.rs:791` | Printed calendar: one long name clips the week; months split across pages | S |
| `core/report.rs:321` | Excel reports store placeholders; Monthly Excel totals would include disabled payments | M |
| `core/report.rs:569` | Reports do not separate past from forecast, and show no plan column | M |
| `gui/main.rs:2122` | Summary panel never appears on common laptops; its toggle silently does nothing | M |
| `gui/main.rs:1050` | Desktop never states today's balance; hero figures differ by platform | S |
| `gui/reports.rs:43` | Report menu is organised by legacy format, not by questions | M |
| `core/report.rs:736` | Unpaid report mixes overdue, future and expected income | S |

### Medium: accessibility

| Where | Issue | Effort |
|---|---|---|
| `app:30` | Every state change wipes the DOM and drops focus to the page top | L |
| `app:23` | Dialogs have no accessible name; step changes drop focus, including on the key screen | S |
| `app:21` | Status messages are unreliable for screen readers; messages raised in a dialog are invisible | M |
| `app:33` | Calendar has no structure, year, today or selected state | M |
| `css` | Tab focus lands hidden under the bottom nav (WCAG 2.4.11) | S |
| `css` | Focus ring and input borders are far below 3:1 | S |
| `css` | Text contrast fails, including the lowest-balance warning | S |
| `css` | At 200% text size the hero is clipped to a wrong-looking figure | M |
| `app:27` | Sign-in tabs are a broken ARIA pattern; failure clears the form | S |
| `gui/calendar.rs:563` | Calendar reads as nameless rows with no date, status or Low marker | M |
| `gui/main.rs:1367` | Nothing is ever announced to assistive tech | M |
| `gui/editor.rs:599` | Dropdowns have no name and hide their value; search and glyph buttons are unnamed | M |
| `gui/main.rs:1819` | Modals open with no focus; first Tab lands on Delete | M |
| `gui/calendar.rs:468` | Day notes and details exist only on hover | M |
| `gui/main.rs:2222` | Zoom is undocumented and collapses at 200%; OS text and contrast settings are ignored | L |
| `gui/calendar.rs:119` | Hebrew and Arabic names render backwards | L |

### Medium: visual design and architecture

| Where | Issue | Effort |
|---|---|---|
| `gui/calendar.rs:483` | The daily balance over time is never drawn on either platform | L |
| `css:2` | Desktop and phone look like two companies and disagree on colour and sign | L |
| `gui/theme.rs:271` | Highest balance gets the colour; lowest is neutral grey | S |
| `gui/calendar.rs:591` | Red marks every ordinary expense, so it has no alarm left | S |
| `gui/calendar.rs:206` | Past and future tiers form a stripe pattern and weight the past | S |
| `gui/theme.rs:333` | Almost no motion, so edits never show cause and effect | M |
| `gui/main.rs:1067` | No real numeric hero; balance token is dead | S |
| `core/report.rs:70` | Reports are a 1990s visual language | L |
| `gui/main.rs:404` | Dark mode, summary toggle and window size are forgotten at every launch | S |
| `gui/main.rs:2147` | No single modal authority, causing real behaviour defects | L |

### Low

| Where | Issue | Effort |
|---|---|---|
| `core/stats.rs:176` † | "vs plan" shows false variance on a loan's final payment | S |
| `core/schedule.rs:364` † | Tentative payment with a one-day period is accepted but never appears | S |
| `core/bgt.rs:445` | A transient lock fails the save with a misleading message | S |
| `core/config.rs:459` | A transient `.budgetrc` read failure resets and overwrites settings | S |
| `core/bgt.rs:338` | File format version is ignored; silent load repairs become permanent | S |
| `gui/main.rs:420` | Startup says nothing when the last budget is unreachable | S |
| `gui/main.rs:1392` † | Password prompt reports every failure as a wrong password | S |
| `core/bgt.rs:546` † | Files stranded by a crash during save are never cleaned up | S |
| `core/bgt.rs:371` † | UTF-8 .bgt from Notepad has accents permanently corrupted | S |
| `server:280` † | Static path is unthrottled and buffers whole files | S |
| `server:203` † | `BUDGET_DEVELOPMENT=1` silently removes Secure cookies on a public origin | S |
| `server:14` † | Password hashing is below the OWASP floor (from memory), and parameters are not stored | S |
| `server:395` † | Outstanding invitations cannot be listed, and can be cancelled only from one dialog | S |
| `budget-mobile/public/state.mjs:8` † | Web ack skips the checks Windows performs | S |
| `budget-mobile/Dockerfile:8` † | Volume permissions are default; health check ignores PORT | S |
| `gui/main.rs:974` | Far-future year freezes the UI (24 s, 10 GB at year 9999) | L |
| `gui/sharing.rs:403` † | Sync opens a new client per poll and never backs off, even after 401 | S |
| `gui/main.rs:582` † | Native file dialogs have no owner window and block rendering | S |
| `gui/theme.rs:299` † | About 50 MB of fallback fonts loaded for every user | S |
| `budget-mobile/public/storage.mjs:3` † | IndexedDB connection lifecycle is unmanaged | M |
| `budget-mobile/public/manifest.webmanifest:1` † | Install surface is below Android expectations | S |
| `app:121` † | Client code is not reviewable as written (very long lines) | L |
| `server:96` | No schema version | S |
| `server:426` † | Node's 5 s keep-alive left in place behind a proxy (needs checking on the host) | S |
| `scripts/archive-release.ps1:116` | No third-party licence notices; no stated product licence | S |
| `scripts/archive-release.ps1:116` | Archived source ZIP cannot pass its own tests or build the installer | S |
| `installer/budget.nsi:72` | .bgt association has no icon or shell notification | S |
| `AGENTS.md:11` † | Release identity held only in gitignored folders; no tags; 0.8.x unmerged | S |
| `gui/main.rs:4102` † | Performance tests are ignored and assert nothing | S |
| `gui/main.rs:1887` | Keyboard flow stops at Tab; no way back from Payments; Ctrl+S ignored in dialogs | L |
| `gui/editor.rs:254` † | Payment and category names can collide, with a dead-end error | S |
| `gui/editor.rs:15` † | Inconsistent words for core objects in destructive actions | S |
| `app:27` † (plausible) | Phone app cannot be tried without an account on someone's server | L |
| `app:123` † | Entry sheet hides recurrence and drops category when a note exists | S |
| `app:35` † | Household cannot be renamed from the phone | S |
| `gui/editor.rs:831` † | Date picker drops focus after choosing | S |
| `app:37` † | Household picker switches immediately on change | S |
| `css:2` | No design system: 32 font sizes, 14 radii, 48 raw colours | M |
| `css:2` | Font is not shipped; no tabular numerals | S |
| `gui/calendar.rs:127` | Calendar rows have jittering baselines and three right edges | S |
| `css:2` † | Bottom sheet is inset by the browser's default dialog max-width | S |
| `gui/editor.rs:573` † | Dates appear in at least three formats | S |
| `gui/main.rs:992` † | Today button appears as a second primary | S |
| `gui/main.rs:1192` † | Summary chart has redundant rows and colliding columns | S |
| `app:31` † | Arrow direction means opposite things across platforms | S |
| `gui/calendar.rs:496` † | A checkpoint is a cryptic footnote | S |
| `gui/sharing/panel.rs:169` † | Recovery key and export handling has small sharp edges | S |
| `core/report.rs:393` † | Comparative bar shows only the other range | S |
| `gui/reports.rs:590` † | Range shortcuts only look backward | S |
| `app:30` | An unresolvable zone wedges the phone at boot (old browsers) | S |
| `app:171` | Selected date does not roll over | S |
| `gui/editor.rs:866` † | Date picker's Today ignores the household zone | S |
| `gui/sharing.rs:257` † | A `futureNMonths` of 121-180 stops Windows opening any shared budget | S |
| `core/schedule.rs:24` † | A stored short start date with Weekdays jumps to year 22 | S |
| `core/crypto.rs:27` † | A password spelling that worked by accident becomes permanent on first save | S |
| `gui/sharing/panel.rs:66` † | Shared dialog backdrop stops short of the window edges | S |
| `gui/main.rs:1566` † | A conflict blocks read-only actions such as opening a day | S |

### Walkthrough-only items not listed above

These were seen live and do not appear as separate deep-review findings. Details are in the evidence files.

- **Phone, medium:** long unbroken text breaks layout on every device; sign-out lands on First setup; no Copy or Share for key or invite; one-time entries edited as a series; Add entry is 20 controls and 1533px tall; calendar labels are 8.5px and collide at $12,345.67, with no month arrows; Today omits overdue bills; no dark mode; second-tab screen does not recover by itself; raw exception text reaches users; large figures break page width.
- **Phone, low:** 12 items (L1-L12), including `1e3` accepted as an amount and dates with no year across a year boundary.
- **Desktop, medium:** modals do not isolate assistive tech (a background Save ran behind the shortcuts sheet); UIA `ValuePattern` is a silent no-op on text fields; Esc discards a filled form; the corrupt-file banner shows an internal field name; search gives no total or hint of other months.
- **Desktop, low:** 7 items (L1-L7). One is an open question: a balance of 5,613.50 displayed as 5,613, which half-even rounding does not explain. Check this one, because a display rounding mystery in a money app should not stay open.

## The competitive picture

Sources and caveats are in `docs/reviews/2026-10-30-teardown/competitive-brief.md`.

- **The market moved to agents in 2026.** Rocket Money launched Rowan (texting, proactive alerts, approval before acting). Monarch shipped a connector and Forecasting. Quicken AI runs what-ifs.
- **Forecasting is crowded at long range, thin at day level.** PocketSmith goes 60 years, Monarch targets life planning, Actual added a balance forecast. We found nobody doing an explainable, shared, day-level view. That is an inference from a sample, not a survey.
- **Table stakes we lack:** recurring detection, import or bank feed, natural-language questions, visible self-repairing sync health, swipe triage, activity people can read, and low-balance alerts. Budget Calendar has no import of any kind, and data entry is its main gap.
- **Complaints we could answer:** silent sync failures, forecasts missing planned spend, price creep, weak Android, no Canadian-first option, and credential anxiety. These are directional; no direct Reddit or app store data was read.
- **Canada:** open banking law is complete, but final regulations were not found. Copilot is US-only, Monarch bills USD, and Tiller's Canadian support is limited. The closest local rival seen is Waypoint Budget, whose claims are self-published.
- **Our position:** daily forecasting, no bank credentials and an encrypted household is genuinely distinctive. It is also currently out of reach, because the first-run path never delivers it.
- **Name collision:** at least two shipping apps are called Budget Calendar, including `ca.mishell.budget` on Android. Resolve this before public launch.

## The trend-setter vision: Show Your Work

**Every figure can prove itself, and the app speaks only when the answer changed.** While the category gets more talkative, go the other way: one true sentence, one line, and evidence behind both, computed on the household's own device. The word "AI" never appears. Scores below are judgement; nothing has been user-tested.

1. **The Line.** "Clear until Nov 1. Lowest $317, after Rent." Beneath it, a 90-day balance ribbon. Drag it and the sentence follows; tap the low and the chain of causes opens. Launch with Clear and Short against $0.
2. **Ghost.** While typing an amount or dragging an entry, a faint second line shows the new future. Cancel means it never happened. The engine's `apply` is already pure (`app.mjs:101`), so this is the best return available. Measure speed on a low-end Android before promising "live".
3. **Only What Moved.** "Sam added Brakes. Your low moved from $317 to $42." Computed after decryption, so the server cannot write this story and encryption becomes a feature. This replaces the Activity tab and the raw JSON conflict view.
4. **Bank Check.** The ribbon thins as the last confirmed date recedes, then one question appears. A checkpoint is the day's closing balance (`db.rs:339-343`), so it must first ask "Has Rent cleared?" or the trust feature makes the forecast wrong.
5. **Ask (gated).** Sam sends a ghost instead of a message. It stays a courtesy, never an approval lock, because permission over a partner's spending is a financial-control risk. Build only if two-person testing shows demand.

**Cut for now:** push-based quiet marks, shared-cost splitting, check-ins, local models, receipt photos, suggested fixes, chat, agents, streaks and scores. Also remove slogans, income and expense tiles, the time zone line, version badges, and the healthy Synced pill from the phone, and the Min and Max bands as the main answer on desktop.

**Go/no-go test:** the sentence must stay calm when the news is bad. Test with people on tight budgets before going further.

## Roadmap

### Now: stop the silent damage (weeks)

- [ ] Fix the phone edit form: start date, series status, series amount (#1, #2, `app.mjs:121`).
- [ ] Rename mobile recurrence labels and snap dates in the engine (#3).
- [ ] Exclude disabled payments from phone totals (#4).
- [ ] Fix the window-start month and report range (#5, #21).
- [ ] Prefill Set balance and mark checkpoints (#8).
- [ ] Separate adjustments and opening balance from Money in and out (#7, `stats.rs:198`).
- [ ] Fix the accessibility crash (#10) and the Ctrl+S conflict trap (`shared_client.rs:122`).
- [ ] Add a confirmation and recoverable copy to "Use current shared version" (#11).
- [ ] Add a recovery journal and panic hook (#9, `main.rs:2226`).
- [ ] Add undo on both platforms (`main.rs:1838`).
- [ ] Fix truncated-file detection and add Restore previous version (`bgt.rs:351`, `main.rs:582`).
- [ ] Fix the service worker update path (#22) and offline status (#23).
- [ ] Check the 5,613.50 display question.
- [ ] Add one release gate plus CI, including the WASM check and all mobile tests (`AGENTS.md:9`, `package-mobile.ps1:62`).
- [ ] Delete the stray file noted under Coverage if you agree.

### Next: make it safe to share and worth starting

- [ ] Complete the household service blockers above, then run a small private trial.
- [ ] Add a guided first run with balance, pay, bills and a forecast reveal (#6, #24).
- [ ] Add a sample household and New budget on the first screen (`budget.nsi:39`).
- [ ] Add CSV/OFX import with recurring detection, parsed on the device (`main.rs:1848`).
- [ ] Auto-merge unrelated edits, and do field-level conflicts with a human view (#12, #16).
- [ ] Reopen shared budgets at launch (#19).
- [ ] Rework recovery key handling and invite flow (#13, `app.mjs:146`).
- [ ] Fix the capacity wall with archiving and bounded work (#14).
- [ ] Make Windows keyboard reachable and accessible (#20, `calendar.rs:563`, `main.rs:1367`).
- [ ] Fix phone focus, contrast and dialog naming (`app.mjs:30`, `app.mjs:23`, `styles.css`).
- [ ] Run one copy pass and adopt one vocabulary (`app.mjs:124`, `app.mjs:126`).
- [ ] Sign releases, add licences, and link the C runtime statically (`build-installer.ps1:95`, `archive-release.ps1:116`, `budget.nsi:37`).
- [ ] Get legal advice on the vendor material (`budget.nsi:40`).

### Later: earn the "trend setter" claim

- [ ] Ship The Line, phone first (`calendar.rs:483`, `app.mjs:32`).
- [ ] Ship Ghost after measuring speed on a low-end phone (`app.mjs:102`).
- [ ] Test the sentence with people on tight budgets.
- [ ] Ship Only What Moved (`app.mjs:34`).
- [ ] Ship Bank Check, first one number, then file cards (`balance_editor.rs:56`, `app.mjs:32`).
- [ ] Add a shared cross-platform design system and motion (`styles.css:2`, `theme.rs:333`).
- [ ] Modernise reports and add phone insight (`report.rs:70`, `app.mjs:33`).
- [ ] Add local-first alerts after a reliability prototype (`app.mjs:121`).
- [ ] Add a safety design with specialist review (`app.mjs:34`).
- [ ] Plan schema v2 for a shared floor, then decide on Ask.
- [ ] Resolve the product name.

## Coverage and limits

**Numbers.** Deep review: 0 critical, 18 high, 153 medium, 57 low (228 in all). All high and medium items were confirmed; 42 of the low items are read from code or plausible, not run. Skeptics refuted 3 further findings. The two walkthroughs reported 9 high items: 2 are folded into #6 and #20, and 7 have their own rows above. Baseline at this commit: 536 workspace tests passed, 4 ignored. On a separate run, Node unit tests and parity (5 of 5) passed.

**Refuted, so do not spend time on them:**
1. The 900x600 minimum window exceeding laptop screens. Maximized windows fit the work area, and the calendar compresses.
2. "Required SQLite-aware backup is missing." Stop-and-copy is documented, and verified backups are already a stated deployment gate. A shipped backup tool remains a useful convenience.
3. "Parsers need fuzzing to prevent phone traps." A 120-second harness ran 59,955 extreme snapshots and about 1.2 million mutated files with no panic. It ran natively, not in WASM, and is strong evidence, not proof.

**Could not reach.**
- **Real Android hardware:** soft keyboard, native pickers, back gesture, install flow, tab freezing and storage eviction are all unverified. Phone testing used emulated Chromium.
- **Phone flows:** invite expiry, live rate limits and storage limits in the walkthrough, the multi-household picker, loan-end entries, and full restore from history. Some were covered separately by server probes.
- **Windows:** real mouse input (right-click and drag are code review only), screen-reader speech, default maximized launch, DPI other than 150%, clean-PC install, and any real installer run. Reports were not launched live from the app; report output was rendered separately from generated data in Chromium, and Excel and LibreOffice were not tested.
- **Real user data:** none was opened by design, so how often legacy file shapes occur is unknown.
- **Not user-tested:** all UX judgements and vision scores are expert opinion.

**Evidence notes.**
- The desktop walkthrough tested the archived v0.8.4 binary because `target/release/budget-gui.exe` is stale at v0.7.1.
- The phone walkthrough's "this and future split correctly" does not contradict #1; the defect appears when the displayed date differs from the schedule anchor.
- Its "reapply retained both sides" concerns unrelated edits; #12 concerns the same record.
- Walkthrough and deep review disagreed on severity for #15, #22 and #23. This report takes the higher rating because each was reproduced live.
- A finding's path had a doubled `public/`, corrected in the table.

**Incident needing your attention.** During desktop testing a stray file was created outside the permitted paths: `G:\My Drive\From One Drive\Desktop\budget.bgt` (37 bytes, an empty budget, created 2026-10-30 15:16:59). A Save dialog was committed in its remembered folder. No user file was overwritten, and only its metadata was read. It was left in place because it sits in your synced folder and removal is your decision. It is safe to delete. Separately, the same mistake shows a product weakness: New budget can create a file in a synced folder with one Enter.

No tracked file was changed by this review.
