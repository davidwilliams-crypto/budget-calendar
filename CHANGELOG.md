# Changelog

## [0.4.0] - 2026-10-27

Full review release: calculation, reliability and interface polish.

- Whole-dollar amounts and balances now round the way the original app does, never show "-0", and no longer crash on non-finite values.
- Fixed hangs and crashes from corrupt schedules (zero/negative/huge frequency, huge loans, extreme years); a damaged budget now shows an error instead of closing the app.
- Corrected schedule dates: 5th-weekday repeats fall back to the last matching weekday, weekend-shifted loan installments and early-pay starts are placed correctly, and "today" uses the local date.
- Summary tiles reconcile with opening and closing balances; balance markers (low/min/max) and negative colouring follow the original rules.
- Payment forms: amounts accept thousands commas, `$` and parentheses, start empty and are required, reject absurd values with a clear message; Enter submits and Escape cancels.
- Safer files: emoji and other non-BMP text survive save/load, short passwords match the original encryption (older files still open), failed saves leave no stray backup, option dates reload correctly.
- Command line opens the first file argument and never silently falls back to the last file; `--version` prints in a console.
- Interface: stronger visual hierarchy, accessible contrast in light and dark, stronger today state, clearer Min/Max/Low balance strips, closing-balance hero tile, tooltip cards, calendar legend, shortcuts sheet (F1 or ?), Alt+Left/Right and T navigation, onboarding for empty budgets, and emoji/CJK font fallback.
- Installer no longer overwrites or deletes the user's sample budget and requires an explicit version.

## [0.3.0] - 2026-10-27

First tracked local release. Earlier local updates used the imported 0.2.0 label and are collected here rather than assigned separate historical versions.

- Open and edit existing `.bgt` budgets, including payment creation, updates and deletion, with safe saves and backups.
- Refresh the desktop interface, payment forms and calendar for clearer everyday editing.
- Show the whole month, including multiple payment rows, readable names and amounts, and drag payments between dates.
- Edit balances and change payment amounts from a selected occurrence into the future.
- Show the app version in the title and header, with build details, dated version history and copyable version information.
- Preserve local releases with executable and source archives, build identity and SHA256 records.

## [0.2.0] - Imported baseline

The project was imported with version 0.2.0. Local changes before the first tracked release shared this label; no separate dated releases were recorded.
