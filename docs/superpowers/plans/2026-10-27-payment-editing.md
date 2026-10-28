# Payment editing implementation plan

> **For agentic workers:** Use subagent-driven development for independent tasks; keep files scoped and review the completed integration.

**Goal:** Add, edit, delete, and safely save existing `.bgt` payments.

**Architecture:** Reuse `BudgetDb` scheduling and `.bgt` serialization. Put draft validation and form UI in `budget-gui/src/editor.rs`. Integrate document state, a payment list, calendar actions, and unsaved-change protection in `main.rs`. Strengthen `bgt::save_file` without changing its public API.

**Tech stack:** Rust, egui/eframe 0.31, existing dependencies only.

**Spec:** `docs/superpowers/specs/2026-10-27-payment-editing-design.md`

## Global constraints

- Preserve unedited `.bgt` fields, per-date exceptions, encryption, balance adjustments, and notes.
- Never write the user's original budget during automated or UI verification.
- Changes are explicit-save only; cancelled or failed actions keep the current document.
- No new dependencies, no pushes, no changes to the original vendor application.

## Review focus

- Editing a recurring payment must preserve its existing exception lists.
- Occurrence edits and deletions must affect only the selected calendar date.
- Invalid dates, non-finite numbers, and malformed recurrence must be rejected before mutation.
- Save As cancellation/failure and disk conflicts must not clear unsaved state or switch the current path.
- Opening another file or closing must not silently discard committed changes or an open draft.

## Task 1: Draft form and validation

- [x] Write and run failing tests for validation and hidden-field preservation.
- [x] Implement `editor.rs` drafts and egui payment/transaction dialogs.
- [x] Verify focused tests and review.

## Task 2: Safe `.bgt` writes

- [x] Write and run failing tests for retained backup, replacement, and failed saves.
- [x] Implement temporary write, durable flush, backup, and replacement in `bgt::save_file`.
- [x] Verify focused tests and review.

## Task 3: GUI integration and document protection

- [x] Write meaningful document lifecycle/conflict and CRUD round-trip tests.
- [x] Implement Payments window, calendar edit/delete actions, dirty state, save/open/close guards, disk conflict checks, and stable payment IDs.
- [x] Verify the workspace tests and release build.

## Task 4: Review and live verification

- [x] Obtain independent code review and resolve material findings.
- [x] Exercise add, edit, save, reopen, deletion confirmation/cancellation, and unsaved cancellation using an ignored budget copy in the Windows UI; deletion and its save/reload behavior covered by automated CRUD tests.
- [x] Verify the original budget's hash, build the final app, and launch with the original budget.
