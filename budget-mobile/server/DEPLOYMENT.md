# Private service deployment

This is a vendor-neutral Node 24.19 service, not a deployed household server. Hosting, service region, spending permission, account recovery and backup ownership remain deployment decisions. The mobile assets and service must share one HTTPS origin. No third-party package install is required.

## Process configuration

Run `node budget-mobile/server/start.mjs` from the unpacked application source. Configure these values through the host's secret/environment settings:

| Variable | Meaning |
| --- | --- |
| `BUDGET_PUBLIC_ORIGIN` | Exact externally visible HTTPS origin, such as `https://budget.example.invalid`, without a trailing slash/path |
| `BUDGET_DATA_DIR` | Private persistent volume directory; never a static directory or temporary deployment filesystem |
| `BUDGET_SETUP_TOKEN` | Random secret of at least 24 characters, manually given only to the initial household owner |
| `PORT` | Internal HTTP port, default `8787` |
| `BUDGET_HOST` | Internal bind address, default `127.0.0.1`; use a private container interface only when the platform requires it |
| `BUDGET_MAX_PAYLOAD_BYTES` | Optional lower encrypted snapshot size limit, from 1024 to 8388608 bytes; default 8 MiB |
| `BUDGET_TRUSTED_PROXY` | Defaults to `none`, ignoring all forwarded client addresses. Set `render` only for a public Render web service behind its managed ingress; also requires Render's platform-provided `RENDER=true` |

Do not put real configuration values into source files, screenshots or chat. The service logs only its bind address/port at startup. Configure the reverse proxy and host to avoid request bodies, Authorization/Cookie headers, passwords, invitation codes, setup tokens or encryption keys in logs and traces. Native sessions use Authorization headers; cookie sessions use host-only HttpOnly Secure SameSite=Strict cookies. Refer to [the API contract](../SERVER-CONTRACT.md) for client requirements.

Terminate TLS in a managed ingress or reverse proxy with a valid certificate. Expose only HTTPS publicly and forward it to this service over loopback or a private network. Reject plain HTTP or redirect it at ingress before handling credentials. Do not expose the internal HTTP listener directly. Set the ingress request-body limit to at least the configured payload limit plus 64 KiB, and enforce connection/concurrency limits.

On a public Render web service, set `BUDGET_TRUSTED_PROXY=render`, retain the platform-provided `RENDER=true`, and use `BUDGET_HOST=0.0.0.0` so managed ingress can reach the listener. Keep development mode disabled. In this explicit mode, the service requires both a private/loopback socket peer and exactly one valid `CF-Connecting-IP` address for every API request; absent, malformed, scoped or chained addresses fail closed with `400 invalid_client_identity`. IPv6 spelling variants and IPv4-mapped IPv6 normalize to one identity. Health checks do not need this header. Restrict access to the internal listener to the trusted ingress and trusted workspace services: private-network access is part of this boundary. This mode is for Render's public ingress, not a private service receiving arbitrary internal forwarding.

[Render documents that Cloudflare overwrites `CF-Connecting-IP`](https://render.com/articles/host-pocketbase-on-render#making-pocketbase-see-the-real-client-ip), while a caller can spoof the leftmost `X-Forwarded-For` entry. The service never uses `X-Forwarded-For`, `Forwarded` or `X-Real-IP`. Other hosting keeps `BUDGET_TRUSTED_PROXY=none` and sees the canonical direct socket address; configure suitable trusted ingress abuse controls for that host. A header alone never enables proxy trust.

Authenticated API requests have a separate 1,200-request/minute allowance per user, shared by that user's sessions. Anonymous API requests have an independent 1,200-request/minute allowance per client identity. Setup, login and invitation acceptance each allow 10 attempts per client identity per 15 minutes, independently of the general anonymous API allowance; no account-wide failed-login lockout lets an anonymous caller block a named victim. At limiter capacity, expired entries are reclaimed and bounded keyed overflow buckets continue enforcing limits without rejecting all new identities. Under saturation, a small number of new identities can share an overflow bucket. Rate limits return `429` with `Retry-After`.

Authentication JSON bodies are limited to 16 KiB. At most eight authentication requests (two per client) and two password KDFs are admitted concurrently, without an unbounded wait queue. Ordinary authenticated request bodies have a separate pool of four (two per user), so unfinished anonymous uploads cannot consume their body capacity. Temporary concurrency exhaustion returns `503 service_busy` with `Retry-After: 1`; clients should retry with backoff. The host must still enforce connection limits and normal resource monitoring.

Start one service process against one persistent SQLite volume. The database uses WAL, foreign keys, FULL synchronous commits, explicit write transactions and a five-second busy timeout. Avoid network filesystems without reliable SQLite locking. Horizontal multi-instance hosting is outside this service's supported deployment shape. Restart with the same volume to retain users, memberships, sessions, revision history and operation acknowledgements. `/health` returns only `{ok:true}` and is suitable for liveness checks. Graceful SIGINT/SIGTERM closes HTTP/SQLite, with five seconds to drain outstanding requests; clients can safely retry acknowledged commit IDs.

The static directory is exclusively `budget-mobile/public/`, guarded by an extension allowlist, canonical path checks and restrictive CSP. Never place databases, credentials, backups or recovery keys there. Keep the whole runtime volume out of source archives and version control.

## First household

After TLS, storage and backups have been verified with generated fixtures, open the HTTPS address, choose First setup and use the private setup token to create the initial account. Setup is disabled as soon as an account exists. Create the household budget in the client. The owner can generate a recipient-bound invitation code and share it manually; the service sends no email. Each editor signs in with their own account. Email-shaped usernames are identifiers, not verified email addresses. Encryption recovery keys are shared separately and never uploaded to the service.

## Durability and recovery

Back up the **entire** SQLite database, including users, memberships, snapshots and operation receipts. Use a SQLite-aware consistent backup or stop the service cleanly before copying its data directory; copying only a live `budget.sqlite` can omit committed WAL data. Encrypt backups, restrict filesystem/volume access to the service account, retain them outside the static directory and test restoring a generated household onto an isolated instance. The persistent volume and host backups require capacity monitoring. Browsable history is 50 snapshots; immutable retry receipts are additionally bounded at 256 MiB/10,000 operations per budget and 1 GiB overall. There are at most 16 budgets and 100 users. Quota exhaustion returns `507` before any new mutation, while old commit retries remain safe.

A retained encrypted snapshot is restored in the client: download, decrypt with its old revision context, review, reseal for the next revision and submit a normal compare-revision commit. The server deliberately offers no ciphertext-copy restore endpoint. Stale edits remain conflicts. Restoring a whole server backup is a disaster-recovery operation and can rewind the canonical server state; coordinate all devices and retain their pending work before reconnecting them.

The service has no email-based password reset or backdoor recovery endpoint. Decide and test an administrator-assisted account recovery procedure before using real household data. Account credentials alone cannot recover an end-to-end encryption key; retain the client recovery key separately. Revocation blocks subsequent server reads/writes and old retry acknowledgements, but cannot erase an offline copy already downloaded to a device.

## Local generated-fixture checks

`node --test budget-mobile/tests/server.test.mjs` exercises real HTTP and SQLite using generated fixtures only under `target/shared-server-tests/`. Full mobile checks are `npm test --prefix budget-mobile`. Test-local HTTP is permitted only with `BUDGET_DEVELOPMENT=1` and an explicit localhost origin. Development mode removes Secure from cookies, so it must never be enabled on the hosted service. No personal `.bgt` file is needed or read.
