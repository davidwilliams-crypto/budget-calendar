# Future amount implementation plan

**Goal:** Offer single-occurrence and selected-and-future amount changes in the transaction editor.

**Spec:** `docs/superpowers/specs/2026-10-27-future-amount-design.md`

**Architecture:** `editor.rs` owns a typed amount scope and UI. `document.rs` owns a staged, validated future mutation using native fields/options. `main.rs` dispatches scope and retains normal dirty/save/rebuild behavior. No dependency or file-format changes.

- [x] Document task: add failing schedule/round-trip tests; implement `change_future_transactions(db, id, date, amount, note, flag) -> Result<(), String>`; verify selected/later amounts and unchanged history, metadata and skips, including sticky, tentative, shifted and finite schedules. Reject invalid changes atomically.
- [x] Editor task: add `TransactionScope::{Occurrence, Future}` and include `scope` in `TransactionAction::Apply`. Default Occurrence; show an amount scope selector with clear date and help; preserve one-date note/status semantics and compact dialog usability. Test real emitted action and invalid draft handling.
- [x] Root: integrate scope dispatch; add a lifecycle save/reload regression for both choices; update UX documentation.
- [x] Review and verify: independently review mutation preservation; run full offline release workspace tests/build, render compact form and calendar, preserve the original budget and existing open drafts. Update the launchable release build without discarding user work.

Verification: 146 workspace tests passed; release build passed. Independent review passed 9 focused tests, including 137 schedule combinations and native round trips; its loan overpayment finding was fixed and rechecked. Compact light/dark forms and the six-week calendar were rendered and inspected. The desktop shortcut still targets `target/release/budget-gui.exe` with Dave's original file. The prior executable was renamed inside `target/release` so the user's unfinished Groceries form could stay open; restarting uses the new build. The original budget was not changed during testing.
