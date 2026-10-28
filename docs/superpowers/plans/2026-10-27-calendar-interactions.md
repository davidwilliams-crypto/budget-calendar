# Calendar interaction implementation plan

**Goal:** Show the whole month, improve names, move occurrences by dragging, and edit balances by context menu.

**Spec:** `docs/superpowers/specs/2026-10-27-calendar-interaction-design.md`

**Architecture:** Calendar owns bounded geometry, full-width name rendering, typed drag payloads and context actions. Document helpers own validated move/balance mutations and preservation. A small balance_editor module owns draft UI; main integrates actions, compact chrome, month-hover navigation and existing save/dirty protections.

- [ ] Calendar agent: fit all weeks with no ScrollArea, improve names, count overflow, drag/drop highlight/preview, balance context menu and bounded-layout tests.
- [ ] Document agent: move occurrence and set/remove balance adjustment with red/green preservation, collision, validation and save/reload tests.
- [ ] Balance UI agent: signed balance dialog with clear date/scope, cancel/apply/remove, validation and compact layout.
- [ ] Root: integrate actions, compact summary/header where needed, summary-card balance context menus and month navigation while dragging.
- [ ] Root: full suite/build and independent review; native four/five/six-week and drag/balance checks using a copy; update docs and launch original without test changes.
