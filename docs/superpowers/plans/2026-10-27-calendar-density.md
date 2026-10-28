# Calendar density implementation plan

**Goal:** Show several named payments and their amounts directly in each calendar day, matching the supplied reference's information density.

**Spec:** `docs/superpowers/specs/2026-10-27-calendar-density-design.md`

- [x] Calendar task: add failing visual-layout tests for name and amount on the same line and all 4–5 rows visible in ordinary five-week months; replace detail cards with 18px single-line rows; size weeks by demand within available space; use the resulting boundaries for painting and drag/drop. Retain overflow, date and balance actions, compact readable text and all existing interaction tests.
- [x] Root: add a regression for the calendar area available after app chrome at 1200×800 and 900×600; compact the header, financial summary, margins and status bar to prioritize the calendar. Preserve summary balance context menus, menus, search and dirty/draft guards.
- [x] Review: independently check geometry, long-name/amount separation, compact/wide/dark rendering and drag target mapping. Resolve findings.
- [x] Verify: full offline release suite/build; render the copied real budget at standard/compact/six-week/wide sizes; confirm several payments are visible with amounts; install through the existing desktop shortcut without disturbing any open draft or writing the original budget.

Final verification: 158 workspace tests passed; offline release build passed. Independent density review passed 28 targeted tests, including 0–9 amounts at four DPI scales. Full-app regressions cover all 23 busy-day rows at 1200×800, multiple rows and matching amounts at 900×600, and explicit ellipsis/exact hover for oversized summary values. Native copies were rendered for four-, five-, and six-week months, compact/standard/wide and dark layouts. Final standard capture shows every October payment directly, including zero-valued reminders. Opened the installed build with the original `dave.bgt`; no budget test changes were written to the original (SHA256 unchanged). Existing desktop shortcut still uses the same executable.
