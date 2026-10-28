# Budget Calendar visual and usability redesign

The user explicitly requested a broad professionalism and usability pass, clarifying that the whole app looks amateur. Redesign the existing Rust/egui desktop application as one restrained Windows product rather than accumulating small cosmetic changes.

Use a coherent blue/slate visual language, Segoe UI at comfortable sizes, strong spacing and alignment, consistent 34–38 px controls, clear primary/secondary/destructive actions, and fewer competing controls. Preserve all budget calculations, serialization, safe saves, exceptions, and unsaved-change guards.

Replace the crowded three-row toolbar and twelve month chips with a clear brand/file/status header, one New payment action, Save, application menus, and Calendar/Payments navigation. Give the month a prominent title and predictable previous/next/Today controls. Make transaction search live and plainly labeled; advanced filter syntax stays available without being required.

Use four readable financial summary cards and calmer calendar cells, with income/expense color limited to amounts, clear current-day emphasis, and predictable overflow. The right insights panel appears only when there is enough width. Ensure the app works at 900x600 and larger sizes in light and dark themes.

Make Payments a full workspace instead of a floating utility window. Include live search, status filters, sorting, aligned descriptive rows, visible edit actions, clear destructive actions, empty states, and stable selection. All historical/disabled records remain accessible.

Redesign payment/transaction forms with grouped fields, positive amount entry plus explicit Expense/Income choice, date picking, a sensible repeat default, clear scope labels, readable validation, and consistent modal footers. Keep cloning/preservation/validation behavior. Explain native loan and tentative status semantics accurately.

Verify the live app against an ignored budget copy, check major layouts and forms at standard and compact sizes, run the existing complete test suite plus meaningful behavior tests, obtain independent review, rebuild and reopen the original budget. Keep the user's original budget unchanged during verification. Keep the existing desktop shortcut valid.
