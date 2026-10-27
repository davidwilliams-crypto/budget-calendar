# Budget Calendar — UI design system (budget-gui)

Captures: `screenshot-dec-2021.png` (light, 1200×800), `screenshot-dark.png` (dark, with the today marker), `ux-pass-*.png` (design-loop passes: 4-small = 900×600, 4-wide = 1500×900, 4-empty = no file open, 6-today-filter = active filter). Regenerate with `budget-gui sample.bgt --month 202112 --screenshot out.png [--size WxH] [--dark] [--today yyyymmdd] [--filter expr]`.

## Layout
- One dominant element: the calendar card fills the remaining window; cells scale to the available height (min 96 px, then a solid scrollbar). Cells ≥ 150 px wide show payment icons and 13 px rows; narrower cells drop the icon gutter and use 12 px so descriptions keep their full text at 1200 px.
- Header panel (one hairline): menu row (File · View · Help, file name and key hints at right) + toolbar (‹ Month Year › · Today · 12 month chips · filter box · Open · Save). Chips hide when they would not fit; the summary panel hides below 1100 px (View ▸ Summary panel toggles it).
- Stats strip: one card, 8 tiles separated by hairlines (Opening · Closing · Income · Expenses · Net · Lowest · Average · Highest). Decimals appear when a tile is ≥ 104 px wide.
- Summary: right panel (264 px) with Category / Amount / % (+ "vs plan" only when a difference ≥ 5 exists). Share of income is a 2 px bar under the name. Click a row to filter; the active row is tinted; a "Filtered: … Clear" chip appears below.
- Empty state: centred icon, "Open a budget", primary Open button, recent files, drag-drop hint; dropping a .bgt anywhere opens it (accent overlay while hovering).

## Spacing scale (8 px grid)
SP1 4 · SP2 8 · SP3 16 · SP4 24. Card radius 8, control radius 6, panel margins 16, card padding 16 (summary 12), cell padding 6, row height 19, balance strip 20, weekday header 30.

## Type scale (Segoe UI when installed, else egui's bundled fonts; Phosphor icons as fallback glyphs)
title 20 semibold (month) · value 18 semibold (stat tiles) · section 13 semibold (card titles, totals) · body 13 (rows, table) · day 13 semibold (day numbers) · caption 11 (labels, hints, "+N more") · strip label 9.5 semibold caps (MAX / MIN / LOW). Amounts use the real minus sign and the font's tabular digits, always right-aligned.

## Colour roles (light / dark) — `theme::Tokens`
- bg f4f5f7 / 12151b · surface ffffff / 1b1f27 · hover f0f3f8 / 242935 · border e4e6ea / 2b313d · text 1f2328 / e6e8eb · muted 6b7280 / 9aa3b2 · faint a4aab4 / 5b6472.
- accent (links, chips, today, focus) 2563eb / 60a5fa · positive (income, positive balances) 15803d / 4ade80 · negative (expenses, negative net) b91c1c / f87171 — all ≥ 4.5:1 on their surfaces.
- Balance strips: normal f3f4f6 / 232833; MAX (month high, config `bgColorMaxBalance`) fce7f3·9d174d / 4a1d3a·f9a8d4; MIN (month low, `bgColorMinBalance`) dbeafe·1e40af / 1e3a5f·93c5fd; LOW (below `minBalance4MaxSaving`, `bgColorBelowMinBalance`) fef3c7·92400e / 4a3410·fcd34d.
- Flags: paid = 3 px green left bar + check-circle glyph (`bgColorPaid` role); important = amber bar + warning glyph (`bgColorImportant` role); user highlight = row pill at 35 % of the user's colour; disabled = grey text.
- Cell shades: other month f1f2f4, past fbfbfc, weekend f9fafb, today = surface tinted 7 % accent (18 % dark) with the day number in a filled accent circle.
- Config palette: a `.budgetrc` colour that still equals the stock light or dark default is replaced by the modern tone for that role; a colour the user changed is used verbatim with a luminance-derived foreground. Tints are computed as opaque gamma-space blends (`theme::tint`) because egui premultiplies low-alpha fills in linear space.

## States
- Hover: cells and summary rows tint to `hover`; transaction rows tint 12 % accent with a pointer cursor and a tooltip (full description, category, amount vs planned, balance after, flag, note, weekend-shift / last-payment suffix).
- Filter: Enter applies; box gets an accent border, funnel icon, "shown/total" count and a × clear button. Clicking a calendar row or summary row filters to that payment/category; clicking the active one clears.
- Keyboard: PgUp / PgDn month, Home today, Ctrl+O open, Ctrl+S save, Ctrl+F focus filter (hints in the menu row).
- Feedback: success = 4 s toast bottom-right; errors = dismissible banner above the stats; encrypted files = centred modal with Unlock / Cancel.
