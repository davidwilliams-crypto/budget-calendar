# Shared budget responsiveness implementation plan

> **For agentic workers:** Execute the layout and polling tasks independently, using test-driven development and a fresh final review.

**Goal:** Keep the Windows shared-budget dialog steady, fully reachable and responsive without altering personal budgets or weakening sync durability.

**Architecture:** Give the existing egui modal a viewport-bounded allocation, permanent header/status/footer and independently scrolling body. Remove disk writes and whole-document clones from unchanged polling, wake precisely on the next deadline or worker completion, and cache conflict rendering data.

**Tech stack:** Existing Rust/egui 0.31, reqwest worker, Windows DPAPI cache and generated Rust fixtures.

**Spec:** User request and attached screenshot: shared dialog jumps, looks unfinished and appears to partially freeze the PC. Preserve its existing sign-in, recovery, household creation, conflict, backup and mobile-link workflows.

## Evidence and constraints

- An idle GET currently persists both DPAPI cache files on the UI thread; its unchanged receipt repeats the pair. Generated release fixtures measured 75–449 ms per pair depending on size/queue.
- Conditional Connecting/error content moves controls 42/65 points; fixed 530-point width overflows a 480-point viewport. Close is clipped inside the body.
- Conflict decoding/validation and expanded JSON formatting currently repeat every frame.
- A false `receive()` result can mean a newly stored conflict. Skip only provably unchanged revisions; preserve new conflict persistence, offline requests, receipt application to current state and unsaved/draft/report/drag guards.
- Reuse this existing clean feature branch. Never touch personal .bgt/cache/credentials or close the running app to replace it.

## Task 1: Stable shared dialog

- [ ] Add real egui regressions and confirm failure against the old panel: busy/error stability, narrow/short/resized/DPI containment, visible Close and scrollable forms/conflict actions.
- [ ] Move rendering to `budget-gui/src/sharing/panel.rs`, keeping command handling in `sharing.rs`.
- [ ] Allocate deterministic viewport-clamped dimensions, permanent status line and footer; use explicit fixed scroll body/gutter and wrapped controls. Show errors once; no animated background polling spinner.
- [ ] Cache decrypted conflict preview and expanded pretty JSON once per conflict, preserving recovery-key privacy.
- [ ] Run new and existing sharing UI tests.

## Task 2: Quiet idle polling

- [ ] Reproduce idle polling cache writes, unchanged receipt writes/deferred payload and 250-ms redraw schedule with generated tests.
- [ ] Borrow document for idle GET identity instead of cloning/preparing/persisting it. Ignore unchanged remote revisions before clone/apply/write, even while a dialog is open.
- [ ] Preserve durable first request preparation and POST acknowledgements against current queued state; preserve higher revision conflicts and local edit guards.
- [ ] Replace repeated 250-ms redraws with the remaining five-second deadline; have completion workers request a repaint.
- [ ] Make status concise so errors/raw timestamps cannot stretch the main window or move controls. Run timer/waker, queue, conflict, failure and no-write regressions.

## Task 3: Review and deliver

- [ ] Fresh independent review for clipping, lifecycle/durability regressions, missed wakes and new freeze paths; address concrete findings.
- [ ] Bump GUI and lockfile to 0.8.4, add dated changelog; full workspace tests, build-script tests, release archive tests and release executable build.
- [ ] Verify both version CLI commands. Commit final sources, archive immutable v0.8.4 with matching source/manifest/checksums; push private feature branch.
- [ ] Show a generated-fixture screenshot and report version/build/tests/archive. Leave the user's open app alone; the new executable takes effect when launched after the user finishes the existing window.

## Review focus

- Every footer/action stays reachable at 320–1920 logical width and short/high-DPI windows.
- Background checks do not move controls, animate at display refresh rate or perform unchanged cache writes.
- A new conflict is still durable even though `receive()` returns false.
- Worker completion and future polling deadlines still wake an idle window.
- Edits made while POST is running survive receipts, storage failure and account/household changes.
