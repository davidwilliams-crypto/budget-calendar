# Budget Calendar vision: Show Your Work

## Scores (1–5, my judgement; nothing here is user-tested)

| Proposal | Differentiation | User value | Feasibility | Defensibility | Total |
|---|---|---|---|---|---|
| 1. Stays quiet | 4 | 5 | 4 | 3 | 16 |
| 2. Shows its work | 4 | 5 | 3 | 4 | 16 |
| 3. Shared conversation | 5 | 3 | 2 | 4 | 14 |

- **Proposal 1** has the best product shape, but a ribbon alone is copyable. Silent Mark is weak: push is unverified, and a badge set on open tells you nothing new.
- **Proposal 2** has the sharpest framing. The Shoebox and Proposal Contract are too heavy to lead with.
- **Proposal 3** has the most original idea (Ask) and the widest scope. Fair Shares needs a breaking schema and sensitive research.

## Point of view: Show Your Work

**Every figure can prove itself, and the app speaks only when the answer changed.**

The category is getting more talkative, and assistant answers can vary with wording (brief §4). We go the other way: one true sentence, one line, and a trail of evidence behind both, computed deterministically on the household's own device. The word "AI" never appears.

Promise: *See why any day looks the way it does, and try tomorrow without touching today, without giving anyone your bank password.*

## Signature experiences

### 1. The Line
No dashboard, only:

> **Clear until Nov 1.** Lowest $317, after Rent.

Beneath it runs a 90-day daily balance ribbon. Drag it and the sentence follows. Tap the low and the chain opens: last checkpoint, each item, the low, every step linked to its entry.

- **Scope:** launch with Clear and Short against $0.
- **Trap:** on a checkpoint day, opening plus transactions doesn't equal closing (`db.rs:339-343`), and `forecast` exports only `adjusted` (`sharing.rs:366`). The chain needs the adjustment as an explicit step.

### 2. Ghost
While she types an amount or drags an entry, a faint second line peels away and the sentence updates: "Still clear. Lowest becomes $205, Nov 1." There is no mode. Cancel means it never happened; Keep commits once, with a named undo.

- **Why:** `apply` is pure and only `mutate` persists (`app.mjs:101`). This is the best return in all three proposals.
- **Risk:** every call rebuilds (`sharing.rs:350`) and returns full day JSON. Add a compact balances-only action and measure on a low-end Android before promising "live".

### 3. Only What Moved
A line appears only if the outlook changed: "Sam added Brakes. Your low moved from $317 to $42." Tap for both lines overlaid. True collisions use the same view: "You said $1,900, Sam said $1,950." This replaces the Activity tab and the raw JSON dump (`app.mjs:144`).

- **Why:** the server can't write this story, so encryption becomes the feature.
- **Rules:** compute after decryption and never put it in `summary`, stored readable (`server.mjs:243`). Forecast both snapshots with the same `today`. Keep the last-seen snapshot on the device, since history holds 50 revisions.

### 4. Bank Check
This replaces "Check your bank for your actual balance" (`app.mjs:32`). The ribbon thins as the last confirmed date recedes, then one question appears: "What does your bank say today?" One number gives "$27 lower than planned." The stronger version takes CSV or OFX as cards: "Hydro planned $100, bank says $112.40." The file never leaves the device.

- **Trap no proposal caught:** a checkpoint is the day's *closing* balance, after every planned item (`db.rs:339-343`). If Rent is planned today but hasn't posted, every later day is overstated by the rent. The dialog must first ask "Has Rent cleared?" or the trust feature makes the forecast wrong.

### 5. Ask (gated)
Sam sends a ghost instead of a message. Priya sees both futures and can accept, counter by dragging, or leave it. It is a courtesy, never an approval lock, because permission over a partner's spending is a financial-control risk. Build it only if two-person testing shows demand. It needs a server route and its own AAD domain (`crypto.mjs:7`).

## What to cut

- **Silent Mark and push:** revisit only after a prototype.
- **Fair Shares, Mine/Yours/Ours, No-Questions:** Monarch already labels ownership, and the last is possible today as a plain recurring entry.
- **Check-in:** Only What Moved gives the value without an obligation.
- **Local models, receipt photos, Proposal Contract:** add only if a model ships.
- **"Find a Way" and smallest-fix suggestions, for now:** `grace_period_days` defaults to 0 (`model.rs:121`), so the data is mostly absent, and a suggestion implies a biller will agree. Ghost lets people try the move themselves.
- **Chat, MCP, agents, cloud models, streaks, scores.**
- **Mobile chrome:** slogans, income and expense tiles, the time zone line, the "31 days" label, `v{revision}` badges, the Synced pill while healthy.
- **Desktop chrome:** MAX and MIN bands as the main answer, footer hints.

## What must be fixed first

1. **Service worker updates (H1).** Engine fixes can't reliably reach phones, and everything above ships through the engine.
2. **Starting balance (H5).** "Clear until Nov 1" built on $0 is a confident lie.
3. **Checkpoint safety (H6).** Saving the unfilled dialog silently deletes a checkpoint, and checkpoints have no marker. The chain and Bank Check both depend on them, and no proposal listed this.
4. **Automatic merge of unrelated edits (H4).** Otherwise Only What Moved sits behind a blocking conflict.
5. **Offline shown as failure (H2).** "Failed to fetch" undercuts a calm product.
6. **Shared trough function in core,** plus the four-month report limit before Bank Check.
7. **Floor decision.** Proposal 1 moves the floor in step one, but `deny_unknown_fields` and exact version equality (`sharing.rs:11`, `:71`) make that a breaking change. Launch on $0; a shared floor and Tight wait for a planned schema v2, after fix 1.

## Sequence

1. Fixes 1–5, then a small private trial on the deployed service.
2. **The Line**, phone first, then desktop.
3. **Ghost** on phone after the latency measurement. Desktop is modal today and needs a `BudgetDb` clone, so it comes later.
4. **Test with people on tight budgets.** The sentence must stay calm when the news is bad. This is the go/no-go.
5. **Only What Moved.**
6. **Bank Check:** one number, then file cards.
7. Schema v2, then **Ask** if testing supports it.

Resolve the product name before public launch (brief §7). Experiences 1 and 2 are the demo, but "trend setter" is the market's to give. "Nobody does this" is an inference from the brief's sample, not a survey.
