# Budget Calendar mobile and private service

This folder is a portable Android web app and private household service. It is built for deployment; it has not been deployed, and no public address has been assigned. A real hosting account, HTTPS domain, persistent volume, secret configuration and tested backup/recovery procedure are required before household use. Installing it on a physical Android phone still needs device verification.

The service requires Node **24.19.0 or later in the 24.x line**, including built-in `node:sqlite`. It has no external npm dependencies. The browser runs the same Rust calculation engine as Windows, compiled to WebAssembly; rebuild it with `scripts/build-mobile.ps1` after engine changes.

## Build and package

From the repository root in PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-mobile.ps1
node --test budget-mobile/tests/*.test.mjs
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package-mobile.ps1
```

Cargo uses the configured offline cache unless `build-mobile.ps1 -Online` is explicitly selected. The package script reads the application version from `budget-gui/Cargo.toml` and writes `target/mobile-release-vVERSION/budget-mobile-vVERSION.zip` plus `release.json`. The ZIP contains runtime assets, the service, deployment documentation, Docker files and `app-version.json` with version and input SHA256 hashes. The external manifest also records the ZIP SHA256. Identical repeated packaging is safe; changed content at an existing version is refused. The source archive shipped alongside the Windows executable separately preserves Rust source and tests.

Only explicit runtime filenames/extensions are packaged. JSON budget caches, databases, `.env`, personal `.bgt` files, node_modules and runtime data are excluded. Never store private data under `public/`, even if a filename has an allowed static extension.

## Run after deployment configuration

Extract the ZIP into a new application directory. Configure `BUDGET_PUBLIC_ORIGIN` to the exact real HTTPS origin, `BUDGET_DATA_DIR` to a private persistent directory outside `public/`, and `BUDGET_SETUP_TOKEN` through the host's secret configuration. The setup secret must be random and at least 24 characters. Then run:

```text
node server/start.mjs
```

The default listener is loopback port 8787. Place a TLS reverse proxy in front of it, and expose only the HTTPS origin. Production startup requires the configured public origin and setup token. See [server deployment](server/DEPLOYMENT.md) for ingress limits, backups, account recovery, quotas and logging controls. Never enable `BUDGET_DEVELOPMENT` in hosting.

For Docker, build from the extracted directory (or this `budget-mobile/` folder):

```text
docker build -t budget-calendar-household .
```

Run a single container with the host's secret/environment configuration, a named persistent volume mounted at `/data`, and the private port routed to HTTPS ingress. The container listens on port 8787. Keep `/data` outside `/app/public`; its SQLite database and WAL must persist through replacement/restart. Docker copies only direct server modules and public assets, and its deny-by-default context excludes local data. No credentials, domain or deployment are bundled.

## Household setup and Android installation

After deployment checks pass with generated data, open the real HTTPS address in Chrome on Android. Use **First setup** and the private server setup token once to create the owner account. Create a shared budget or publish one explicitly from Windows. Each person uses their own account. The owner creates a recipient-bound invitation and shares its code manually; the service does not send email.

The current implementation encrypts budget contents on the device. The final privacy choice still needs confirmation before real household use; the local implementation does not imply that choice was approved. Give the budget recovery key to the invited editor separately from the invitation code, and keep a secure offline copy. A password or server backup cannot recreate a lost encryption key. Revoking membership prevents future service access but cannot erase copies already stored on a device.

Use Chrome's **Install app** or **Add to Home screen** on the HTTPS page. Repeat on the second phone using its own account and the same budget invitation/key flow. Offline edits remain on that device until the service acknowledges synchronization; inspect visible conflicts before choosing a version. Export backups before clearing browser site storage or moving devices. Local file saves on Windows remain separate from shared synchronization.

Use one active Budget Calendar tab or installed-app window per browser profile. A second tab explains how to return to the existing window, preserving its pending edits. Windows likewise allows shared-budget access in one application window at a time; close that window before opening sharing in another. Standalone Windows budgets remain available independently.

Encrypted client backups preserve the current working snapshot, including the effect of unsynchronized edits. They do not copy the entire replay queue or server history. Restore is reviewed and creates a new revision; it never silently replaces a newer server revision. The service still sees household names, account identities, membership and generic activity labels; financial entries and notes are encrypted in the client.

Contracts and tests: [engine](ENGINE-CONTRACT.md), [service API](SERVER-CONTRACT.md), `tests/`. All automated test households are generated under the repository's `target/` directory.
