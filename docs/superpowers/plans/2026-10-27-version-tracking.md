# App version tracking implementation plan

**Goal:** Make the current app version visible and keep durable release history and saved builds.

**Spec:** `docs/superpowers/specs/2026-10-27-version-tracking-design.md`

- [x] Metadata task: bump GUI to 0.3.0 in Cargo.toml/Cargo.lock; add build.rs deterministic source/compiler identifier and version.rs display/CLI helpers. Validate CLI JSON, content-sensitive stable ID and history parsing. Own only build.rs, version.rs and GUI manifest/lock entries.
- [x] Archive task: create CHANGELOG.md, scripts/archive-release.ps1 and AGENTS.md workflow; ignore local-releases. Test archive idempotence, version collision rejection and source/binary preservation with workspace fixtures. Own these files only.
- [x] Root: integrate header/title version, View entry and About/version-history modal with copy/close; CLI early exit. Test displayed version, compact modal, history and preserved drafts. Update UX docs.
- [x] Review and verify: independent review; full offline workspace tests/build, --version/JSON, archive actual release, native compact/wide version display and launch. Preserve original budget and any currently open drafts.

Delivered v0.3.0, build `9a927d12e5fc8441`, x86_64-pc-windows-msvc release. Fresh verification: 165 workspace tests, 6 standalone build-script tests and 11 archive test groups passed; release build passed. Independent review passed additional fingerprint and malformed/stale/corrupted archive scenarios. CLI flags exited without GUI/config or budget changes. Native title/header show v0.3.0, the version button opened dated history, and the compact modal/copy/draft regressions passed. The clean prior app was closed, then the versioned app reopened with Dave's existing budget; version history is displayed. Archived executable, matching source ZIP, icon, changelog and verified build manifest under `local-releases/v0.3.0`. Future changes follow root AGENTS.md version/changelog/build/archive rules. No budget file was included in the release archive or used as test data.
