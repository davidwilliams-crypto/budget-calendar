# Professional UI implementation plan

**Goal:** Make the whole existing application look coherent, professional, and easier to use.

**Spec:** `docs/superpowers/specs/2026-10-27-professional-ui-design.md`

**Architecture:** Keep the Rust/egui engine and saved budget format. Share visual tokens and button styles in theme.rs. Move the Payments workspace into payment_list.rs; keep calendar and editor modules scoped. Main.rs owns application navigation, live search, summary layout, and integration.

## Constraints

- Preserve existing financial behavior and safe save/unsaved protections.
- No dependencies, file migration, external uploads, or writes to the original budget during checks.
- Existing desktop shortcut must continue to launch the final updated executable.
- Test behavior changes meaningfully; visually inspect styling and layout changes.

## Tasks

- [x] Root: visual tokens, application header/navigation, four financial cards, live search and integration.
- [x] Forms agent: grouped payment/transaction forms, explicit expense/income, date picking, sensible recurrence defaults; preserve existing validation and metadata.
- [x] Calendar agent: calm, readable calendar, cell/row spacing, balanced emphasis, compact layout.
- [x] Payments agent: full Payments workspace with search/filter/sort and clear row actions.
- [x] Root: complete tests and release build; independent review; live visual checks at 1200x800, 900x600, and dark theme using a copy.
- [x] Root: verify original hash and update/reopen app without losing human edits; keep shortcut valid.
