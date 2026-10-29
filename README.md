# Budget Calendar

Budget Calendar provides a Windows desktop budget calendar and an Android installable web app backed by a private household synchronization service. Both use the same Rust calculations. The mobile service and deployable package are built locally; hosting and a real HTTPS address still require configuration. Physical phone installation has not been verified.

## Windows build and local releases

Use the Rust toolchain pinned by `rust-toolchain.toml` and the available offline Cargo cache:

```powershell
cargo test --workspace --offline
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-mobile.ps1
rustc --edition=2021 --test budget-gui/build.rs -o target/version-build-tests.exe
target/version-build-tests.exe
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/archive-release.Tests.ps1
cargo build -p budget-gui --release --offline
target/release/budget-gui.exe --version
target/release/budget-gui.exe --version-json
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/archive-release.ps1
```

Build mobile assets before the final Windows build: its build identity includes the shared Rust engine, mobile assets, service, documentation and release scripts. The archive script checks its exact build-generated inventory against the source. Follow [AGENTS.md](AGENTS.md) for version, changelog, test and archive requirements. Existing versions are immutable. Keep any running application and unsaved draft open while building a new executable.

Personal `.bgt` files remain standalone files. Shared budgets use explicit application sharing actions and their own synchronization state. Never use personal budgets as fixtures or put them in source/release archives. Tests generate data under `target/`.

## Android and household sharing

See [mobile build, packaging and installation](budget-mobile/README.md) and the [deployment guide](budget-mobile/server/DEPLOYMENT.md). `scripts/package-mobile.ps1` creates an immutable versioned ZIP under `target/mobile-release-vVERSION/` with SHA256 manifests, a Node 24.19+ service, PWA assets and Docker deployment files. No npm dependencies are needed.

Deploy a single service with a persistent private SQLite volume behind HTTPS ingress. Supply the real HTTPS public origin and initial setup secret through hosting configuration. The owner creates the household account/budget and private invitations; each editor uses their own account. The current implementation encrypts budget contents on the device; the final privacy choice still needs confirmation before real household use. Share the encryption recovery key separately and keep an offline recovery copy. Open the actual deployed HTTPS address in Android Chrome and choose **Install app** or **Add to Home screen**. No deployed URL or phone-installation result is included in this local release.

The latest 50 revisions are browsable, offline edits survive normal restart, and concurrent edits require visible conflict resolution. A local save becomes synchronized only after server acknowledgement. Keep client exports, encryption recovery keys, service backups and account recovery procedures in place before relying on the household service.
