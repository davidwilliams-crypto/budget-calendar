# Multiple payments visible in each calendar date

The user's reference shows a compact list of several payments in each day, with each payment name and amount on the same line. Replace the two-line payment cards with readable single-line rows: name left, amount right, no overlap, full details on hover. Use the existing restrained visual theme, not the reference's legacy colors. Keep clicking, dragging, context menus, balance editing and future-amount scope unchanged.

Give the calendar more room by tightening the header and replacing the tall financial cards with a compact summary strip. Allocate week heights according to the busiest day in each week so empty weeks do not consume space needed by busy ones. Fit the entire month without scrolling. At 1200×800, the ordinary October budget's 4–5 transactions per busy day should all be visible. At small windows or unusually crowded dates, show as many readable name-and-amount rows as fit, with +N opening the complete day list.

Preserve 14px payment-name text, aligned amounts, all dates and daily balances, native budget format and all data. Use separate clipped name/amount columns; long names truncate with a full tooltip. Week geometry and drop hit testing must use the same row boundaries. No dependencies or budget-data changes.
