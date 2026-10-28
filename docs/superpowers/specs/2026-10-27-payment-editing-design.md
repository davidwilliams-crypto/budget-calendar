# Payment editing

Implement the user's request to add, change, delete, and save payments in an existing `.bgt` budget using the existing Rust/egui app and `budget-core` scheduler. No migration or new dependencies.

Use a searchable Payments window for all payments, including disabled and historical ones. Add and Edit open a form for description, category, signed amount, start date, recurrence, end condition, and schedule flags. Editing clones the existing payment and preserves every unexposed field and per-date exception. Calendar rows expose Edit transaction and Edit payment separately; changing one transaction changes only that date's amount, note, and paid/important status. Deleting a transaction skips that occurrence; deleting a payment removes its entire series and asks for confirmation.

Keep changes in memory until Save/Ctrl+S. Mark unsaved changes in the title, and prompt to save/discard/cancel before opening another file or closing. Invalid form data must not change the database. A failed or cancelled save leaves the current document and unsaved state intact. If the file changed on disk after opening, refuse to overwrite it and offer Save As. Successful saves retain a backup of the prior file and write through a temporary file before replacement.

Approaches considered: inline calendar editing is compact but cannot reach hidden, historical, or disabled payments; a separate Payments window covers every record and preserves the existing calendar. Use the Payments window plus calendar editing shortcuts. This is an implementation decision within the requested CRUD feature.

Verification: meaningful validation and round-trip tests, occurrence-versus-series tests, save failure/conflict tests, full workspace tests, release build, and a Windows UI add/edit/delete/save/reopen exercise against a private ignored copy of the user's budget. Leave the user's original budget unchanged during verification and launch it in the updated app afterwards.

Loan payments retain the engine's payoff calculation: changing or skipping a loan installment recalculates future installments and the payoff date. State this explicitly in the transaction editor and deletion confirmation. Other payments' occurrence operations preserve later scheduled amounts and skipped dates, including sticky payments with weekend adjustments. Calendar click opens the transaction editor; filtering remains available in the context menu and summary.

Tentative payments also retain their native paid-status behavior: marking an occurrence Paid updates the unpaid estimate for that period. Explain this in the transaction editor instead of promising that status changes affect no other estimate.
