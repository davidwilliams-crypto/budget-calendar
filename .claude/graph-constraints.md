# graph-run constraints

Binding on how graph-run cuts, briefs and checks work in this repository. Each rule comes from a confirmed failure.

- [2026-11-15] RULE: run unit gate checks one at a time (or at low load). `lifecycle_tests::a_sealed_backup_asks_for_its_password_and_stays_a_draft` and `rejected_password_keeps_the_field_usable_from_the_keyboard` wait a fixed 10 s for a sealed-file open that takes ~7 s alone, and time out when several cargo test runs share the machine.
  FROM: code-review run; core-calc, gui-ui and gui-doc each went red on these two tests at load ~6–9 on 4 cores with no related change, and all passed on sequential re-runs.
- [2026-11-15] RULE: any change under `budget-core/src` or `budget-engine/src` needs the phone engine rebuilt (`wasm32-unknown-unknown`), restamped with `scripts/stamp-mobile-engine.ps1 -BuiltFrom`, and then `scripts/stamp-mobile-worker.ps1` run, because the service-worker cache name hashes `budget_engine.wasm`. A unit check that rebuilds the engine temporarily must exempt both stamp steps; integration must check both for real. Only one unit may own `budget_engine.wasm` and `sw.js`, normally integration.
  FROM: code-review run; core-files check went red on worker-stamp after a temporary engine rebuild (harness exempted only engine-stamp).
- [2026-11-15] RULE: GUI sharing, undo-on-shared-budget and recovery tests only run on Windows (shared-cache locking and DPAPI). On Linux, fix a defect there only when a new test avoids the locked cache and DPAPI; otherwise report it for Windows CI.
  FROM: code-review run; 29 sharing/undo tests fail on Linux on master with "Secure shared cache locking is available on Windows."; recovery tests are cfg(windows).
- [2026-11-15] RULE: browser tests (`budget-mobile/tests/*.browser.mjs`, `three-client-integration.mjs`) listen on fixed ports 8794–8797; parallel checks must serialize them (e.g. a shared `flock`). A new test step that did not exist on master must also pass; comparing only master's steps misses it.
  FROM: code-review run; parallel validation needed serialization, and the mobile unit's new browser test was initially not compared.
- [2026-11-15] RULE: a test that starts several threads must not assume they run in the order they were spawned; synchronise on an observable signal (for example a gate's entry counter) before starting the next one. Check every new concurrency test with 50 or more isolated runs before accepting it.
  FROM: float_roundtrip run; `file_probe::tests::an_answer_from_before_check_again_never_replaces_the_newer_one` (added in 0.11.3) failed 6 of 60 isolated runs on master because the second probe thread could reach the probe first. After waiting for the first check at the gate: 200 of 200 passed.
