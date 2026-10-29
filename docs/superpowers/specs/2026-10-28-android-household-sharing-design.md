# Android household budget sharing proposal

Draft for review, 2026-10-28.

The Windows app and both Android phones should show one household budget. Both people can add and edit entries, see upcoming bills and projected balances, and understand whether their latest changes have reached the other devices. Keeping separate copies creates uncertainty about which balance is current and whether an expense was already entered.

The proposed first delivery is an Android home-screen installable progressive web app (PWA), connected to a private shared-budget service and the existing Windows application. Android and editing access for both people are confirmed requirements. The PWA, hosting arrangement and privacy model remain proposed choices.

## Goals and boundaries

- Both people can record an expense or income on their phone and see the acknowledged change on Windows and the other phone.
- Recurring bills behave consistently across all three devices, including single-occurrence, future-occurrence and whole-series edits.
- Pending, conflicting and failed changes remain visible and recoverable. The interface never calls a local-only edit synced.
- Existing standalone budgets remain usable without an account. Sharing is an explicit action that creates a shared budget while preserving the original file.

The initial scope is one household and one shared budget. Bank connections, automatic transaction categorization, receipt recognition, multiple currencies and a public app-store release are deferred. CSV importing and savings goals follow sharing, forecasting and recovery. Exact Android versions and devices will be recorded before compatibility testing.

## Delivery approaches

| Approach | Value | Tradeoff |
| --- | --- | --- |
| Installable PWA, recommended first | A phone interface with a home-screen entry; one web client can serve both Android phones. | Offline storage, synchronization and installation guidance require implementation and testing. Background execution cannot be the only way edits reach the server. |
| Packaged Android app, possible later | A suitable route if store distribution or deeper device integration becomes necessary. | Adds Android packaging, signing, toolchain and release maintenance alongside Windows. It still needs the same shared-data service and conflict design. |

Android Chrome supports installing a PWA with its own app window. Installation and offline behavior are separate capabilities: cached content may be stale, and synchronization needs an explicit design. See the [MDN installation guide](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Installing), [installability requirements](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable), and [offline operation guidance](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Offline_and_background_operation). Tauri is one possible packaged route with [Android build prerequisites](https://v2.tauri.app/start/prerequisites/); no framework selection or mobile build has been validated for this project yet.

## Everyday use

As either household member, I want to add an expense where I spend it so that our upcoming balance reflects it. I also want to change a bill or mark it paid without having to use the Windows computer.

**Today** opens with the chosen date, projected closing balance, the lowest projected balance and its date within a visible forecast horizon, upcoming bills and a prominent Add entry action. Quick add accepts expense or income, amount, description, date and optional category or note. A confirmation shows the saved entry and whether it is pending or synced. Balance wording distinguishes a forecast from a verified bank balance.

**Upcoming and calendar** offer a readable list first, with a month view for planning. Selecting an entry exposes amount, note, paid status, recurrence and edit/delete actions. Editing a recurring payment asks whether the change applies to this occurrence, this and future occurrences, or the whole series where supported by the existing rules. The preview explains effects on loan payoff and tentative estimates. Validation rejects unsupported combinations without changing the budget.

**Sharing and activity** show the two members, connection state, last successful sync and recent changes with their author. One owner manages invitations and membership; both members can add, edit and delete budget entries. These permission details are proposed defaults. Undo or restore must account for subsequent edits rather than replacing someone else's newer work silently.

## Shared data and architecture

The recommended shape is a private service holding the canonical shared budget, with the Windows app and mobile PWA as clients. Each person signs in separately. An expiring invitation is tied to the intended recipient and accepted after sign-in. The server checks membership and role on every read and write. The owner can cancel an invitation or revoke a membership; revocation stops subsequent server access and queued writes. Previously downloaded information cannot be recalled from an offline device.

Use durable IDs for the budget and its records, an explicit schema version, and server-managed revisions. A recurring edit, move or split is one atomic operation. Keep deletion records, called tombstones, so an older offline copy cannot recreate removed entries. Retried operations carry unique operation IDs so a network retry cannot add the same expense twice.

For the first version, use a budget-wide revision check to keep conflict handling understandable. A client submits its operation against the version it edited. If the version changed, retain the proposed edit and show the relevant current values and author. The user can discard the pending edit or reapply it after reviewing the refreshed budget. Do not silently accept the last writer. Finer-grained merging is a later improvement if this proves too disruptive for two people.

Persist the last acknowledged budget and an ordered queue of local operations on each client. Show Offline, Pending, Synced, Conflict or Sync failed as appropriate. Reconnect and foreground refresh both drain the queue; a conflict pauses dependent operations. Local saves and queue failures must be surfaced immediately. Browser storage may be cleared or evicted, so only server acknowledgement supports a cross-device durability claim; the UI must explain pending edits before sign-out or clearing local data.

Keep domain calculations and validation in shared Rust code. The server can run the engine directly; a feasibility check must establish whether the required portions can also run in WebAssembly for consistent offline previews. If that is unsuitable, revisit the packaged Android option or explicitly agree to narrower offline behavior before implementation. Inject the same household date and time-zone rules into every calculation, including tentative payments.

Legacy `.bgt` remains an import/export format. Import creates a new shared identity; repeated import must not create accidental duplicate entries or overwrite a live shared budget. A complete shared-budget backup also preserves IDs, revisions and history that `.bgt` does not contain. Restoring a snapshot creates a new server revision and forces stale client queues through conflict review. Existing standalone save, backup and external-change behavior remains available.

## Evidence from the current application

| Existing behavior | Design consequence |
| --- | --- |
| `budget-core` exports the model, database, statistics and scheduling helpers in [lib.rs](D:/Claude/rea/Budget/budget-core/src/lib.rs:4). Balances are calculated in [db.rs](D:/Claude/rea/Budget/budget-core/src/db.rs:309). | Reuse the existing calculations and fixture expectations. |
| Validated edit operations live in [document.rs](D:/Claude/rea/Budget/budget-gui/src/document.rs:32) and depend on GUI editor helpers at [document.rs](D:/Claude/rea/Budget/budget-gui/src/document.rs:39). | Extract a reusable command and validation boundary before multiple clients edit the same budget. |
| The loader assigns IDs from one on each load in [bgt.rs](D:/Claude/rea/Budget/budget-core/src/bgt.rs:281); the payment serializer omits IDs at [bgt.rs](D:/Claude/rea/Budget/budget-core/src/bgt.rs:383). | Add durable sharing identities rather than syncing record positions or current integer IDs. |
| [save_checked](D:/Claude/rea/Budget/budget-gui/src/document.rs:12) compares opened file bytes before saving, and [save_file](D:/Claude/rea/Budget/budget-core/src/bgt.rs:471) stages writes and retains backups. | Preserve these local protections; add atomic server revision checks and explicit conflict recovery for sharing. |
| Whole-payment deletion removes a record in [db.rs](D:/Claude/rea/Budget/budget-core/src/db.rs:421); occurrence deletion is a schedule option in [model.rs](D:/Claude/rea/Budget/budget-core/src/model.rs:164). | Collaboration tombstones must be separate from skipped recurrence dates. |
| Legacy file encryption uses Blowfish-ECB and an older XOR reader in [crypto.rs](D:/Claude/rea/Budget/budget-core/src/crypto.rs:1). The clock uses a Windows local-time path with a UTC fallback in [date.rs](D:/Claude/rea/Budget/budget-core/src/date.rs:245). | Design account security separately from legacy file encryption; explicitly align calendar dates across clients. |

## Phased roadmap

1. **Confirm delivery and prove engine portability.** Settle hosting, privacy and account ownership. Exercise the shared commands and proposed mobile runtime using generated fixtures. Produce a clickable phone flow and choose PWA or packaged delivery from that evidence.
2. **Deliver household sharing.** Add private membership, durable storage, Windows synchronization, mobile Today/upcoming/quick add, recurring edits, offline queues, conflict resolution and an activity trail. Today includes the lowest projected balance/date and its forecast horizon. Shared snapshots and a tested restore path are release prerequisites. An earlier read-only preview may help validate the interface, but it does not satisfy the confirmed requirement that both people can edit.
3. **Improve forecast and recovery.** Expand the basic low-balance preview into an interactive forecast graph and reversible what-if scenarios. Expand history browsing, undo and recovery from accidental changes. These features use the same acknowledged-versus-pending distinction.
4. **Consider optional conveniences.** Evaluate CSV import, savings goals, notifications and Android packaging against actual household use. Keep these from delaying dependable shared editing.

No fixed delivery date is proposed. Hosting/privacy decisions and engine portability precede an implementation estimate. Application releases must follow the repository's version, regression, build-fingerprint and local-archive requirements in [AGENTS.md](D:/Claude/rea/Budget/AGENTS.md:1).

## Acceptance and proposed success targets

These are proposed acceptance targets, not measured performance. Test with generated budgets under `target/`; never use the user's `.bgt` files or an open unsaved draft as fixtures.

| Area | Acceptance criterion |
| --- | --- |
| Three-device editing | Given one Windows client and both Android phones, each member can add/edit an entry and the other clients display the acknowledged result. Both phones install and reopen the selected delivery format successfully. |
| Everyday speed | In a supervised fixture trial, each person completes a simple expense entry within 30 seconds after opening Today. Tune the design from failures rather than claiming this target in advance. |
| Foreground synchronization | With all clients foregrounded on the agreed test network, target visible propagation within 5 seconds for at least 95 of 100 acknowledged edits. Record network and device conditions with results. |
| Recurrence parity | Generated cases cover monthly boundaries, leap days, weekend shifts, loans, tentative payments, moved/skipped dates and each edit scope. All supported clients produce the same occurrences, paid states and displayed balances for the same budget date. |
| Offline and retries | Disconnect each phone, queue additions and edits, restart it, then reconnect. Pending operations survive the normal restart; retries create no duplicates. A simulated storage failure never reports the edit saved. |
| Conflicts and deletion | Concurrent amount changes, edit-versus-delete, recurrence splits and restore-versus-stale-edit produce an explicit resolution flow. No accepted edit disappears silently in the fixture suite. |
| Access | Uninvited accounts cannot read or change the budget. Expired/cancelled invitations fail clearly. A revoked member's next request and queued writes are rejected. |
| Recovery and compatibility | Restore a generated shared snapshot, reconnect a stale client, and retain a recoverable copy of rejected edits. Import/export supported `.bgt` content without altering the source file; unsupported data or limitations are reported before accepting migration. |
| Household usefulness | After two weeks of optional use, both people confirm that the shared app is their normal way to view and update this budget and can explain whether a shown entry is pending or synced. Collect this feedback directly; budget-content analytics are unnecessary. |

## Decisions still required

| Decision | Owner and timing |
| --- | --- |
| Accept the proposed PWA route or require a packaged Android app from the outset. | User, before implementation planning; verify exact Android devices and browser versions during the feasibility step. |
| Choose managed hosting versus self-hosting, a recurring-cost limit, service region and responsibility for maintenance/backups. | User with engineering, before provisioning or deployment. |
| Decide whether the service operator may process budget contents, or whether end-to-end encryption is required. End-to-end encryption changes calculation placement, key sharing, account recovery and backup design. | User with engineering, before finalizing the shared service architecture. |
| Choose the sign-in method, owner account, account recovery process and how to recover if the owner loses access. | User with engineering, before authentication implementation. |
| Set the household time zone, history retention and backup retention. Confirm what pending edits should be retained locally after access is revoked. | User with engineering, before synchronization acceptance tests are finalized. |

The builder may choose ordinary UI components, schema layout, transport and test tooling within these requirements. Changes to privacy, offline capabilities, recurrence semantics or data ownership need an explicit design decision.
