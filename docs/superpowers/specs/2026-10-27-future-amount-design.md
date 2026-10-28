# Future transaction amounts

The transaction editor must offer **Only this occurrence** (default) and **This and all future occurrences** for the amount. For example, changing Groceries from expense 150 to 250 with the future scope changes the selected date and later amounts to expense 250. Dates before the selection retain their existing amounts. Notes and status remain scoped to the selected date; the form says so explicitly. Existing skips, moved dates, categories and highlighting must survive.

Keep the existing Rust/egui UI and native `.bgt` save format, backups, disk conflict checks and draft protections. Do not modify Dave's budget as test data. No new dependencies. Scope affects amount only, including future amount exceptions; deletion remains single occurrence. Loan payoff may recalculate, as the existing form already explains.

Approach: keep the existing single-occurrence helper and add a validated future-amount document operation. Use native payment fields/options to retain history and update the recurring amount without enumerating an infinite future. Avoid changing the original start date or creating duplicate payment records. Tests must cover round trips, historical amounts, future exceptions and skips, sticky/tentative schedules and weekend shifting. If a native edge case cannot be represented safely, reject without mutation and explain it.
