# Competitive brief: personal finance and budgeting, as of 2026-10-30

Prepared for the Budget Calendar review (branch codex/android-household-sharing, 268e47c, v0.8.4).

## How far to trust this

- **[P] Primary:** the vendor's own page, help article or changelog, an official government notice, or a named law firm.
- **[S] Secondary:** trade press, or a review site with no visible stake.
- **[V] Vendor-interested:** a competitor's or affiliate's comparison. Useful for leads, not proof.
- Most of the review web in this category is affiliate content, and it often contradicts itself. Where a figure could not be traced to a primary source, the text says so.
- Real user sentiment (Reddit, app store reviews) was only reachable second-hand. **Complaint themes are directional. No percentage here is measured.**
- Prices are USD unless stated.

## 1. Bottom line

1. **AI has moved from "ask your data" to "act on your data" in about six months.** Rocket Money's Rowan (25 Aug), Monarch's MCP connector (21 Oct) and ChatGPT's finance preview (May) all date from 2026. A chat box is now table stakes and no longer sets anyone apart.
2. **Forecasting is split.** Monarch's forecasting is long-range life planning (retirement, home buying). Simplifi and Actual do account-level balance projection. Neither of the first two leads with a calendar showing what your balance will be on each day. PocketSmith has the longest forecast horizon, and its Canadian feeds are its admitted weakness.
3. **Canada is under-served.** Copilot is US-only and EveryDollar's sync is reported as US-only. Monarch tells Canadians to test their own banks during the trial and bills in USD. PocketSmith warns that many Canadian banks do not allow reliable feeds. Tiller calls its Canadian support "not great (yet)". Open banking is law, but no date is fixed for it to go live.
4. **Budget Calendar's position is credible, but it has a gap.** Day-level deterministic forecasting, no bank credentials and an encrypted household is a combination I did not find elsewhere. The gap is data entry. I found no CSV, OFX or QFX import in `docs/ux.md`, `CHANGELOG.md`, `budget-core/src` or `budget-mobile/public` (grep only, so absence is likely but not proven). The same grep found no what-if or scenario support. Without bank sync, import is the difference between "private" and "tedious".

## 2. Competitor table

| Product | Positioning | Price | Platforms | Bank sync | Forecasting | Household | AI |
|---|---|---|---|---|---|---|---|
| **YNAB** | Zero-based method, behaviour change | $14.99/mo, $109/yr; 34-day trial [P] | Web, iOS, Android | Direct Import; file import gained a CSV converter 2026-09-14 [P]. Canadian reliability reported weaker [V] | Not a core feature; none in the recent changelog [P] | Share with up to 6 people, no extra cost (page also says "up to 5 favorites") [P] | Official ChatGPT app is articles plus a debt calculator with **no budget access** [P]. Community MCP servers only [S] |
| **Monarch** | Household finance hub; $100M ARR (2026-10-01) [P] | Core $14.99/mo, $99.99/yr [P]. Plus about $199/yr, annual only [S; official page lists no price] | Web, iOS, Android | Plaid in Canada [V]; Canadian page says confirm your banks in the 7-day trial, billed USD [P] | Forecasting launched 2026-04-13 for retirement, home buying and career breaks; side-by-side forecasts 07-29; Plus tier 04-20 [P] | Reviewed as its strongest point; separate logins, "yours/mine/ours" labels reported [S] | Assistant; **official MCP connector 2026-10-21** for Claude, ChatGPT and others [P]; activity log 07-29 [P] |
| **Copilot** | Design-led, Apple-first | About $13/mo, $95/yr [S, sources vary] | iPhone, iPad, Mac; web since Dec 2025 with gaps [S]; no native Android [S] | **US accounts only** [P, FAQ] | Limited | Not verified | AI categorisation that learns from corrections [S] |
| **Simplifi** | Simple spending plan | Promo $3.99/mo annual, then $6.99/mo tier; first-year discount [P] | Web, iOS, Android | Quicken says US and Canada; older community reports of weak Canadian coverage (2024, may be stale) [S] | Projected Cash Flow per account and date range; excludes Planned Spending and Savings Goals; a Quicken page says up to a year [P] | One extra person only [P] | Quicken AI chat (beta): what-ifs, updates up to 50 transactions, no memory, no external actions [P] |
| **Rocket Money** | Subscriptions and bill negotiation | Free; Premium about $7-14/mo slider [S]; **Premium+ $15/mo** with Rowan [S]; negotiation fee 35-60% [S] | iOS, Android, web | Not verified for Canada | Basic | Not verified | **Rowan**: RCS texting agent, proactive alerts, cancels and negotiates with approval; built with Anthropic [P/S] |
| **Actual Budget** | Open-source, local-first envelope budget | Free; self-host (hosted options exist) | Web/PWA, desktop | SimpleFIN, Pluggy, Enable Banking, Akahu (latter two experimental); Canadian coverage not verified [P] | **Balance Forecast** (experimental 26.6), lowest-point captions and future budget entries (26.7) [P] | Multi-user via self-hosted server; E2EE listed [S] | AI budget analysis report; stable CLI [P] |
| **Lunch Money** | Power-user, multi-currency, API | $10/mo or $100+/yr self-set [S] | Web, lightly used iOS companion [S] | Plaid; Canadian bank claims come only from a competitor [V] | Users are requesting a projected daily balance on the calendar, which suggests it is unbuilt [P, feedback board] | Not verified | Community MCP [S] |
| **PocketGuard** | One number: "In My Pocket" | $12.99/mo, $74.99/yr, lifetime disputed [S] | iOS, Android, web | Recurring sync complaints [S] | Weak; user calls it more an expense tracker [S] | No family plan [S] | Not verified |
| **Goodbudget** | Manual envelopes, couples | Free tier; $10/mo, $80/yr [S] | iOS, Android, web | Premium, US banks reported [S] | None | Real-time shared envelopes across devices [S] | None found |
| **Honeydue** | Free couples tracker | Free, ad-supported [S; one source conflicts] | iOS, Android | Most-cited complaint; Android fix unconfirmed [S] | None | Per-account and per-transaction privacy [S] | None found |
| **Tiller** | Spreadsheet feeds | **$99/yr from 2026-08-01** (was $79) [P] | Sheets, Excel, no mobile | Yodlee plus Plaid; Canada "not great (yet)", expects CDBA to help [P] | User-built | Shared sheet | MCP for Claude in beta [P] |
| **EveryDollar** | Ramsey zero-based | $79.99/yr, $17.99/mo [S] | iOS, Android, web | Premium only, reported US-only [S] | None | Not verified | None found |
| **PocketSmith** | Forecast and calendar veteran | Free (6-month forecast, 2 accounts); Foundation $9.99/mo annual (10 yr, 6 banks, 1 country); Flourish $16.66 (30 yr); Fortune $26.66 (60 yr) [P] | Web, mobile | **Own pricing page warns many Canadian banks do not allow reliable feeds** [P] | Longest horizon found; calendar-based [P/S] | Not verified | None found |
| **CalendarBudget** | Calendar-based budgeting | Not verified | Web, iOS, Android | Not verified | Calendar with reports; one critic says the calendar view is missing, unresolved [S] | Not verified | None found |
| **Budget Calendar (MiShell, `ca.mishell.budget`)** | Calendar instead of ledger | $4.95 Android, 4.8 stars from 469 reviews, about 10K installs [S] | Android, desktop sold separately; Google Drive sync | None mentioned | Calendar with upcoming spend | Drive sync only | None |
| **Budget Calendar (iOS, id951672394)** | 15-year cash flow forecast | $2.99, 2.7 stars from 51 ratings [S] | iPhone | Not verified | Claims 15 years | Not verified | None found |
| **Fudget** | Bare-bones list budgeting | Not verified | iOS, Android | None | None found | None | None |

**Name collision:** at least two shipping apps are already called "Budget Calendar", one of them apparently Canadian (`ca.` package name; country of origin not confirmed). This affects store search, naming and possibly trademark, so get advice before any public launch.

### Named AI entrants

- **ChatGPT personal finance:** May 2026 preview, Pro only, US, web and iOS, via Plaid across 12,000+ institutions. Synced data is removed within 30 days of disconnecting [S, TechCrunch]. PCWorld's objections were model-training defaults, lingering memories, how intimate the profile is, and answers that change with prompt wording [S].
- **Cleo:** voice since Cleo 3.0 (July 2025) and Autopilot (Feb 2026), which works toward savings goals [P]. Canadian availability not verified.
- **BudgetGPT:** iOS, launched Jan 2026, self-described AI-native [S].
- **Candor:** a read-only Plaid layer for your own agent [V].
- **Origin:** multi-agent finance with partner access [V].
- **Richify:** Canadian-oriented, manual entry today, bank link on the roadmap [V].
- **Neontra:** Canadian and US connections on Premium; sync is the top complaint [V].
- **Waypoint Budget:** Canadian, Plaid plus Flinks, free manual tier, Plus $7.99 CAD, Pro $12.99 CAD shared across a household [V]. **The closest Canadian direct competitor seen.**
- **True North Budgeting:** desktop, $49.99 once, no cloud, launched 2026-02-14 [S, press release]. Evidence of demand for a calm, private, one-time-purchase product.

## 3. What users praise and complain about

Directional; mostly second-hand [S/V].

- **Bank connections** are the most consistent complaint across every app: repeated re-linking, missing transactions and lag. One aggregator says apps that show sync status clearly get milder complaints than apps that fail silently. This cannot be independently verified, but it fits YNAB's and Actual's 2026 work on sync-error visibility.
- **Price creep:** YNAB, Tiller and Simplifi all feature, and comparison content keeps coming back to it. Canadians pay about $20 CAD/mo and carry the exchange risk [V].
- **Setup and method burden:** YNAB's learning curve and weekly demands; tedious manual entry in envelope apps.
- **Cancellation and billing traps:** Rocket Money's most frequent complaint is difficulty cancelling [S].
- **Categorisation errors** erode trust (Monarch, per Reddit summaries [V]).
- **Android as a second-class platform:** Copilot has none; ChatGPT's preview omits it; Honeydue's Android fix lags; reports of slower EveryDollar Android updates [S]. Reported iOS conversion of 32.8% against 19.7% on Android has no stated methodology.
- **Praise goes to** design polish (Copilot), household clarity (Monarch), behaviour change (YNAB) and ownership and price (Actual, Tiller).
- **Retention context:** one report cites 4.2% D30 retention for finance apps. Methodology is unclear, so treat it as "low", not as a number.

## 4. Patterns

### Table stakes in 2026

- Recurring detection and a bills list.
- Natural-language questions over your own data.
- Clear connection health with self-service repair. YNAB now prompts a reset [P]; Actual keeps failures flagged across devices [P].
- Fast swipe-style review. YNAB shipped Card Mode on 2026-09-22 [P].
- Partner access at no extra charge.
- Activity log or history (Monarch, 07-29 and 09-29 [P]).
- Mobile parity with web.
- Proactive low-balance alerts.
- SOC 2 is becoming a trust claim (Monarch, 01-21 [P]).
- Financial AI that always asks before acting.

### Unmet complaints

- Sync that fails loudly, explains itself, and keeps a usable budget when it fails.
- Forecasts that include planned, not-yet-billed spending. Simplifi excludes it [P]; Actual added it only in 26.7 [P]; a Lunch Money user is asking for it [P].
- Canadian-first coverage with CAD billing.
- Joint finances with real privacy, without an ad-supported trade-off.
- Budgeting that does not require handing credentials to an aggregator. Secondary sources report rising Reddit scepticism.
- Pricing that does not feel like rent: a one-time purchase has an audience.

### What nobody does well

These are my inferences from what I reviewed, not a survey:

1. **A daily "what will my balance be, and why" view** that is reliable, explainable and shared. Calendars and forecasts exist, but rarely together with a causal explanation.
2. **Answering "what changed since last week?"** by comparing forecasts. Monarch's side-by-side forecasts are the nearest thing, but they are aimed at long-range plans.
3. **Reconciling plan against reality without a bank feed.**
4. **Household conflict as a designed experience.** Sync is usually last-write or real-time. Few explain who changed what and why in plain terms.
5. **Deterministic AI.** Assistants narrate, and PCWorld shows answers can vary with wording. An assistant that can only cite figures from a deterministic engine, and shows its working, is still open ground.
6. **Private AI.** The market's default is to send your data to a cloud model or a chat product.

## 5. Canada

### Regulation

Primary-sourced where marked.

- The Consumer-Driven Banking Act framework was completed by Bill C-15, Royal Assent March 2026. **The Bank of Canada, not FCAC, is now the overseer.** Some sources, including Issuant, still say FCAC and should be treated as out of date.
- Draft regulations were published in Canada Gazette Part I on 2026-06-27; comments closed 2026-08-26 [P].
- Full implementation is intended **within one year of final publication in Part II**, and no final publication was found. Published launch dates conflict (Q3 2026, Q4 2026, early 2027); the one-year clock suggests a realistic date no earlier than late 2027 for the complete framework, though phases could start sooner. **That date is my inference.**
- Phase 1 is read-only, staggered: deposit and payment accounts, then lending, then registered and non-registered investments. Write access is targeted for mid-2027 and depends on the Real-Time Rail [S].
- Rules in draft: no charge for data sharing, 99.5% API availability, at least 24 months of data, consent of up to 12 months with re-authentication [P, DLA Piper].
- Penalties reach $1M for individuals and $10M for entities [P].
- Screen scraping is prohibited by the Act, but that ban is **not yet in force** and has no published date [S, Flinks].
- Small apps will probably need an accredited third-party provider. Mandated banks are chosen by the Minister and the criteria are not public. Credit unions can opt in, so coverage may stay uneven [S].

### Connectivity today

- Plaid, Flinks and Yodlee, mostly credential-based [V]. Flinks is reportedly stronger on credit unions; Plaid-based US apps are weaker, with TD, Desjardins and credit unions named [V].
- Wealthsimple, KOHO and similar see only their own accounts [V].

### File import

- Sources suggest RBC, TD and CIBC offer CSV or QFX downloads. Scotiabank personal accounts are unclear and may need PDF conversion [S; verify with each bank before building].
- Interac e-Transfer memo handling was not found.

### Implication

Bank sync would cost compliance, aggregator fees and breakage, and the rules are moving. File import does not depend on any of that, and it matches the no-credentials promise. Revisit aggregator work once the final regulations and the provider registry exist.

## 6. Interaction trends

- **Conversation moving out of the app:** Rowan uses RCS messages. Monarch's MCP puts your finances into general assistants. Tiller and the open-source projects have similar routes, and the Actual CLI went stable.
- **WebMCP** exposes tools to browser agents; Chrome 149 origin trial; Google names prompt injection as the main threat [S].
- **Approval as a brand promise:** Rowan's "Your approval, every time" language [P].
- **Transparency patterns** (Smashing, May 2026): dynamic checklist for high-stakes work, partial-success reporting, attributing failure to the dependency, audit trail or "show work" [S].
- **Scenario comparison:** Monarch's side-by-side forecasts, Simplifi's chat what-ifs.
- **On-device AI:** Chrome's Prompt API runs Gemini Nano locally, multimodal [S]. It could allow private receipt or statement parsing in a PWA, but Android support, device coverage and accuracy need testing. **Not verified for this use.**
- **Card-based triage:** YNAB Card Mode.
- **Fold and multi-display layouts:** YNAB shipped a folding-iPhone layout on 2026-10-23 [P].
- **Generative UI** remains early, with no strong consumer finance deployment found [S].
- **Android Live Updates** are for ongoing, user-initiated, time-sensitive activity [S], and I found nothing on PWA access. Do not plan around them.

## 7. Recommended moves for Budget Calendar

Ordered by leverage. The review should test each against the code.

1. **Own a single line:** "See every day's balance before it happens, and why, together, without giving anyone your bank password."
2. **Ship CSV, OFX and QFX import with reconcile.** Show planned against actual and offer one-tap matching. This is the biggest gap against the rest of this brief.
3. **Make the forecast explainable.** Tap any low point to see the chain of items that caused it. Show what changed since you last looked, and who in the household changed it.
4. **Add what-if without touching the budget.** "Move rent", "buy this on the 12th", and compare two futures on the calendar. Simplifi and Monarch have forms of this, but not at daily level.
5. **Extend the mobile horizon.** The Android 31-day lowest-balance view (`CHANGELOG.md:26`) is good for bills but short against Simplifi's year and PocketSmith's free 6 months. Desktop's range should be available on the phone.
6. **Design conflicts as a feature.** Plain-language review of what each person changed. The existing explicit conflict flow (`CHANGELOG.md:7`, `:27`) is the base. Nobody in this review makes this good.
7. **Proactive, quiet alerts from the engine:** "Thursday will go below your floor". Check PWA push reliability on Android.
8. **AI on your terms, later and optional.** Rules: figures come only from the Rust engine, the assistant cites the items used, changes are proposed as a previewed diff with undo, and it is off by default. Prefer a local or user-chosen model. An encrypted-sync product cannot casually add a server-side model without breaking its own promise.
9. **Treat financial data as prompt-injection-prone** if any agent or MCP surface is added. Payment descriptions and notes are user-controlled text.
10. **Consider a fair price:** a CAD-priced one-time or low annual price would attack the price-creep complaint. Actual is free, and True North charges $49.99 once.
11. **Resolve the name** before public launch.

**Do not chase:** bill negotiation, investment tracking, credit scores or budgeting-method evangelism. These pull away from the calendar wedge.

## 8. Not verified

- Bank-by-bank Canadian success rates. No independent test found.
- YNAB AI features beyond the ChatGPT app. Third-party claims of AI coaching have no support in official pages.
- Monarch's MCP read/write scope. The help article returned 403; the connector URL and OAuth detail come from a search summary.
- Monarch's assistant details and Plus price on the official page.
- Rocket Money, Cleo and Lunch Money availability in Canada.
- Rowan's date is from a press release, and exact terms may change; it was described as limited to select subscribers at launch.
- CalendarBudget and Fudget current pricing and activity.
- No direct Reddit or app store data was read.
- The absence of import and scenarios in Budget Calendar rests on keyword searches of the paths listed in section 1.

## Sources

**YNAB:** https://www.ynab.com/pricing | https://www.ynab.com/whats-new | https://support.ynab.com/en_us/updates-to-ynab-S1f4aRLeC.md | https://support.ynab.com/en_us/ynab-and-chatgpt-H1QI25JRZe.md | https://truthifi.com/providermcps/ynab-mcp

**Monarch:** https://www.monarch.com/pricing | https://www.monarch.com/whats-new | https://www.monarch.com/canada | https://www.monarch.com/blog/monarch-plus | https://help.monarch.com/hc/en-us/articles/50207234679956-Monarch-MCP-Connector | https://frontdeskreview.com/software/budgeting-apps/monarch-money/ | https://betakit.com/as-intuit-winds-down-mint-financial-planning-app-monarch-money-comes-to-canada/ | https://useorigin.com/resources/blog/the-best-budgeting-app-for-couples-in-2026

**Copilot:** https://www.copilot.money/faq | https://feedback.copilot.money/feature-requests/p/international-support | https://getfinny.app/blog/copilot-money-pricing-2026 | https://www.thepennyhoarder.com/budgeting/budgeting-copilot-money-review/

**Simplifi:** https://www.quicken.com/products/simplifi/ | https://support.simplifi.quicken.com/en/articles/5274786-using-projected-cash-flow-on-the-mobile-app | https://support.simplifi.quicken.com/en/articles/13929605-what-you-can-do-with-quicken-ai-chat-in-simplifi | https://www.quicken.com/features/projected-cashflow | https://community.simplifimoney.com/discussion/comment/25585

**Rocket Money:** https://www.rocketmoney.com/rowan | https://help.rocketmoney.com/en/articles/16760933-what-is-rowan | https://finviz.com/news/385379/rocket-moneys-rowan-rewrites-what-ai-can-do-in-personal-finance | https://www.emarketer.com/content/rocket-money-launches-anthropic-powered-virtual-assistant | https://www.rocketmoney.com/learn/personal-finance/how-much-does-rocket-money-cost | https://www.fincomparelab.com/reviews/rocket-money-review/

**Actual:** https://actualbudget.org/blog/release-26.7.0 | https://actualbudget.org/blog/release-26.6.0 | https://actualbudget.org/blog/release-26.8.0/ | https://actualbudget.org/docs/vision/ | https://learnwithhasan.com/self-hosted-apps/actual-budget/

**Lunch Money:** https://getfinny.app/blog/lunch-money-review-2026 | https://feedback.lunchmoney.app/calendar/p/daily-balance-and-projected-balance-in-the-calendar | https://waypointbudget.com/blog/waypoint-budget-vs-lunch-money | https://www.getdrio.com/mcp/io-github-akutishevsky-lunchmoney-mcp

**PocketGuard, Goodbudget, Honeydue, EveryDollar:** https://www.thepennyhoarder.com/budgeting/pocketguard-review/ | https://getfinny.app/blog/pocketguard-pricing-2026 | https://www.fincomparelab.com/guides/goodbudget-pricing/ | https://www.cnbc.com/select/goodbudget-app-review/ | https://www.fincomparelab.com/reviews/honeydue-review/ | https://www.nerdwallet.com/finance/learn/honeydue-app-review | https://www.fincomparelab.com/guides/everydollar-pricing/ | https://www.nerdwallet.com/finance/learn/everydollar-app-review

**Tiller:** https://community.tiller.com/t/update-to-tillers-annual-price-starting-in-august-2026/34853

**Calendar and forecast products:** https://my.pocketsmith.com/plans | https://www.techradar.com/pro/software-services/pocketsmith-review | https://calendarbudget.com/?p=3771 | https://ca.trustpilot.com/review/calendarbudget.com | https://alternativeto.net/software/calendarbudget/about | https://apps.apple.com/us/app/budget-calendar/id951672394 | https://apppricinglab.com/app/apple/951672394 | https://apppricinglab.com/app/google_play/ca.mishell.budget | https://theseahawk.org/3959/culture/start-a-budget-with-fudget

**AI entrants:** https://techcrunch.com/2026/05/15/openai-launches-chatgpt-for-personal-finance-will-let-you-connect-bank-accounts/ | https://www.pcworld.com/article/3142629/chatgpt-can-access-your-bank-accounts-now-heres-why-im-not-ready.html | https://finance.yahoo.com/news/budgetgpt-launches-ai-native-budgeting-211300192.html | https://candor.money/best-ai-personal-finance-tools | https://useorigin.com/resources/blog/ai-in-personal-finance-2026-comparing-the-top-tools-and-approaches | https://web.meetcleo.com/blog/introducing-autopilot | https://web.meetcleo.com/blog/Introducing-cleo-3-0 | https://lifestyle.kbew98country.com/story/88340/brothers-launch-true-north-budgeting-privacy-first-desktop-app-offers-calm-alternative-as-subscription-fatigue-grows/

**Canada:** https://gazette.gc.ca/rp-pr/p1/2026/2026-06-27/html/reg3-eng.html | https://www.canada.ca/en/department-finance/news/2026/06/government-pre-publishes-regulations-to-prevent-fraud-and-facilitate-the-next-phase-of-consumer-driven-banking.html | https://www.dlapiper.com/insights/publications/2026/08/preparing-for-open-banking-key-takeaways-from-canadas-draft-consumer-driven-banking-regulations | https://www.bennettjones.com/Insights/Blogs/2026/07/Canada-Advances-Consumer-Driven-Banking-Framework-with-Proposed-Regulations | https://newsroom.wealthsimple.com/submission-to-finance-canada-on-the-consumer-driven-banking-regulations | https://www.flinks.com/blog/open-banking-canada-2026-launch-fintech-institutions | https://www.issuant.com/articles/canada-open-banking-rollout-consumer-driven-banking | https://openbankingtracker.com/regulation/canada-open-banking | https://waypointbudget.com/blog/budget-app-bank-sync-canada-2026 | https://www.richify.ai/ca/best-budgeting-apps-canada | https://docs.flinks.com/guides/connect/supported-institutions | https://invoicedataextraction.com/blog/rbc-bank-statement-to-excel | https://invoicedataextraction.com/blog/scotiabank-statement-to-excel | https://invoicedataextraction.com/blog/cibc-bank-statement-to-excel

**Complaints and market context:** https://getfinny.app/blog/best-budget-apps-reddit-recommends-2026 | https://unstar.app/blog/ynab-rocket-money-monarch-everydollar-copilot-budget-apps-ranked-2026 | https://www.useluminix.com/reports/industry-analysis/competitive-landscape-personal-finance-and-budgeting-apps-2026/source/2

**Interaction trends:** https://smashingmagazine.com/2026/05/practical-interface-patterns-ai-transparency/ | https://ppc.land/chrome-149-origin-trial-puts-webmcp-in-developers-hands-at-last/ | https://ppc.land/chromes-webmcp-could-end-ai-agents-pixel-parsing-nightmare/ | https://www.techradar.com/computing/software/android-16s-answer-to-ios-live-activities-is-coming-soon-here-are-the-apps-itll-support-including-google-maps | https://www.orioninc.com/blog/designing-intelligent-trustworthy-experiences-agentic-ai-financial-firms/
