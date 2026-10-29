# Android and Windows household sharing implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. Check off work only after its checks pass.

**Goal:** Deliver an installable Android web app and a Windows release that safely edit the same private household budget, including offline edits and visible conflicts.

**Architecture:** Reuse the existing Rust engine through a versioned JSON interface compiled both natively and to WebAssembly. A separately deployable HTTPS service handles accounts, membership, atomic revisions and retained snapshots; clients own budget calculations. The existing `.bgt` workflow remains available and is never used as a distributed synchronization file.

**Tech stack:** Rust, serde JSON, raw WebAssembly ABI, browser JavaScript/IndexedDB/service worker, Node 24 with SQLite, HTTPS. Cryptography is isolated from transport so the requested privacy choice can be finalized without changing the domain engine.

**Spec:** ../specs/2026-10-28-android-household-sharing-design.md

## Global constraints

- Both Android users can add and edit; Windows participates in the same shared budget.
- Never access user budget files for development or testing. Generate fixtures under `target/`.
- Preserve standalone files and open unsaved work; do not replace a running executable.
- No secret, credentials, database, user budget or recovery key in Git, source archives or application logs.
- No silent replacement of concurrent edits. Server acknowledgement is distinct from a local save.
- Preserve complete recurring-payment semantics and pass household-local `today` into calculations.
- Hosted online service is confirmed. Privacy choice and deployment provider are resolved before production data is uploaded.
- New desktop release is 0.8.0, dated 2026-10-29, with all required repository release checks and archive.
- Current feature branch and checkout are reused. Agents own disjoint files; parent owns integration, root manifests, releases and Git operations.

## Review focus

1. Offline changes survive normal restart and network retries never duplicate accepted writes.
2. Concurrent edits and edit-versus-delete retain both versions until an explicit resolution.
3. Membership revocation blocks new reads and writes, including previously queued requests.
4. Mobile and desktop calculations agree across month boundaries, tentative bills and recurrence splits.
5. Saving or switching between local and shared budgets never overwrites an unrelated file or an editor draft.

## Task 1: Shared calculation and editing boundary

**Files:** `budget-core/src/commands.rs`, `budget-core/src/sharing.rs`, `budget-core/src/model.rs`, `budget-core/src/db.rs`, `budget-core/src/lib.rs`, `budget-core/Cargo.toml`; `budget-engine/Cargo.toml`, `budget-engine/src/lib.rs`; domain tests. GUI `document.rs` remains a compatibility wrapper after extraction.

**Interfaces:** A versioned snapshot preserving all persisted payment fields and stable string record IDs; explicit-clock constructors; `sharing::request_json(&str) -> String` for `{action, ...}` requests and `{ok,result}` / `{ok:false,error}` responses. Actions: import/export, validate, forecast, apply. `budget-engine` exports allocator, request, result pointer/length and deallocator for a build without wasm-bindgen. Native callers use the same function.

- [x] Pin native fixture parity and invalid-input atomicity tests before extraction.
- [x] Extract existing pure mutation functions and validation without changing standalone behavior.
- [x] Add stable snapshot identity, atomic commands, bounds and explicit dates; preserve legacy duplicate options and sentinel metadata.
- [x] Compile native and WASM; test the same fixtures through both interfaces, including recurrence and paid status.

## Task 2: Private synchronization service

**Files:** `budget-mobile/server/`, `budget-mobile/tests/server.test.mjs`, server package/run/deployment files.

**Interfaces:** JSON HTTPS API, `/api/setup`, `/api/login`, `/api/logout`, `/api/me`, `/api/budgets`, `/api/budgets/:id`, `/api/budgets/:id/commits`, `/api/budgets/:id/history`, `/api/budgets/:id/invites`, `/api/invites/accept`, `/api/budgets/:id/members`. Opaque budget payloads; commit body `{operationId,expectedRevision,payload,summary}`. Successful response returns revision and metadata; stale revision returns HTTP 409 with current snapshot. Repeating an operation ID with the same content returns its original result; reusing it for different content fails. Owner and editor membership are checked for every request.

- [x] Write authorization, duplicate retry, restart durability, concurrent commit, expired invite, revocation and restore tests.
- [x] Implement setup-token-gated first owner, separate account sessions, expiring recipient-bound invitations, revocation and SQLite transactions.
- [x] Bound requests, snapshots and retained history; hash passwords with a standard password KDF; apply secure cookie/origin controls and login throttling.
- [x] Implement static delivery, health check and production HTTPS requirements. Keep all runtime data under a configured ignored directory.
- [x] Test against actual local HTTP and SQLite, using generated fixtures only.

## Task 3: Android installable client

**Files:** `budget-mobile/public/` application modules, manifest, service worker, assets; client state tests.

**Interfaces:** Engine JSON contract from task 1; API from task 2; an IndexedDB store for last acknowledged state, pending edits and device credentials. Shared crypto functions expose seal/open without revealing keys to the service if end-to-end encryption is chosen.

- [x] Write tests for durable pending changes, replay deduplication and conflict preservation.
- [x] Implement sign-in, setup/join, Today/agenda/calendar, low-point forecast, quick expense/income, recurring editing and paid status.
- [x] Use the Rust WASM engine for calculations and validation; do not reimplement scheduling in JavaScript.
- [x] Add membership, activity, backup/export and restore flows, plus local-versus-synced state.
- [x] Make PWA assets installable and cached; refresh on foreground/reconnect as well as polling.
- [x] Test narrow widths, keyboard flows, two independent phone-sized browser clients and the native Windows client, offline/restart/reconnect and concurrent edits.

## Task 4: Windows shared-budget integration

**Files:** `budget-gui/src/sharing.rs` and supporting sync client module; small integration points in `main.rs`; `budget-gui/Cargo.toml`.

**Interfaces:** Same snapshots/API as mobile. Network calls run in workers. A local shared document stores its acknowledged baseline and pending snapshot separately from standalone `.bgt` files. Stable IDs survive native edits and reopening.

- [x] Test native request/response, durable pending writes, conflict detection and file-switch protections before integration.
- [x] Add Shared budget menu/dialog with server address, sign-in, create/join/open, sync state, history and recovery.
- [x] Publish current budget only after an explicit in-app sharing action; keep the original standalone file intact.
- [x] Preserve dirty editor drafts while remote updates arrive; apply remote snapshots only at a safe document boundary.
- [x] Reuse existing editing UI, reconcile snapshot differences by stable record identity, and preserve local conflicts for resolution.

## Task 5: Integration, deployment and release

**Files:** build helpers, `README.md`, deployment guide, `.gitignore`, archive source inventory, version/changelog and lockfile.

- [x] Run two-mobile-client plus native-client integration with generated budgets, including offline replay, conflict and revocation cases.
- [x] Review authentication, encryption boundary, key recovery, snapshot restore and data exclusion independently; repair findings before release.
- [x] Build the deployable service/PWA package.
- [ ] Select and configure hosting using the user's account and permitted cost; do not invent a deployed URL or claim phone installation was tested without a device.
- [x] Run all workspace tests, standalone build-fingerprint tests, archive script tests, release build, version CLI checks and archive validation.
- [x] Archive the final immutable 0.8.0 build and report build ID, checks, archive, hosted URL or precise remaining deployment requirement.

## Progress and decisions

- 2026-10-29: User approved implementation, both Android phones, both editors, synchronization with Windows and hosted online service.
- Existing calculation/mutation code and save behavior audited read-only. Rust WebAssembly target installed successfully.
- Ruling: reuse current checkout on `codex/android-household-sharing` rather than create another checkout; this preserves the configured build cache and avoids touching a running release.
- Ruling: implement independent domain/service/client files in parallel under explicit ownership, with integration and release controlled by the parent.
- Provider choice must support native Windows authentication as well as phone browser authentication. The available Sites host has a platform sign-in/service-access boundary; compatibility must be resolved before selecting it for this application.

## Verification and remaining activation work

- The full Rust workspace has 511 passing tests and four intentionally ignored tests. The separate native/service integration test also passes against the generated three-client household. Native/WASM parity passes all four fixture groups.
- The mobile/service suite passes 30 checks. Real Chromium regressions cover occurrence scope, repeated encrypted downloads, offline/restart, exclusive tab ownership, delayed network/decryption/storage responses across household and account switches, and rejected access with pending work retained.
- The three-client runner passes eight end-to-end assertions, including reviewed concurrent reapply, a native Windows commit readable by both phone clients, encrypted backup download and restore as a new revision, and member revocation. Fixtures and screenshot: target/three-client-1793272204236/.
- Independent review findings were repaired: concurrent device-cache writers, whole-container adjustment merges, duplicated backup content, stale asynchronous household/account context, and invitation pruning during password hashing. Both native and browser clients reject oversized changes before treating them as durably saved.
- Build-fingerprint tests: 13; archive tests: 31 including real NSIS compile; mobile package checks: 8; icon checks: 4. Final release identity and archive checks are recorded in the generated release manifests.
- The Android delivery is a PWA. Automated browser trials are not physical-phone installation tests or a measured five-second/95-percent propagation trial. The native integration exercises the production client against a local fixture service, not a hosted TLS endpoint.
- Hosting has not been provisioned and no personal budget has been uploaded. User confirmed hosted service; hosting account/provider, allowed recurring cost, production privacy/recovery setup, real HTTPS origin, and real-device installation remain activation steps. The implementation uses client-side AES-GCM with a household recovery key; account recovery cannot recreate that key.
- Docker deployment inputs are packaged, but no Docker image was built here because the local Docker daemon was not running. The Node service and SQLite persistence were exercised directly.

## Delivered local release

- Version 0.8.0, build `b75e379e30528840`; release executable and both version CLI modes verified without opening a GUI.
- Windows archive: `local-releases/v0.8.0/`, including executable, icon, changelog, exact build input manifest, source ZIP and verified SHA256 release manifest. Release source commit: `88137f5c9540031a7d6313f984b0b95d8c4851c4`.
- Mobile/service package: `target/mobile-release-v0.8.0/budget-mobile-v0.8.0.zip` with matching `release.json`; ZIP checksum verified.
- Release build used `target/sharing-release-v0.8.0/` so the running application's executable and unsaved work were left alone. Personal `.bgt` files were neither read nor changed.
- Source remains on the local `codex/android-household-sharing` branch. No new service was deployed and this feature branch has not been pushed. Hosting selection/account access is the next required input.
