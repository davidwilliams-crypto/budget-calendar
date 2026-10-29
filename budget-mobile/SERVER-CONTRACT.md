# Private budget service API (v1)

The service transports opaque JSON **object** payloads. It never parses budget contents or performs calculations. Clients wrap or encrypt snapshots before upload. Names and activity summaries are ordinary metadata: keep financial contents and keys out of them. Production requires HTTPS; this service binds `127.0.0.1` behind a TLS reverse proxy.

All requests/responses are JSON. Errors are `{ "error": { "code": "...", "message": "..." } }`; conflict errors also have `current` containing the current full snapshot. IDs are strings and revisions are positive integers. Dates are UTC ISO timestamps. Send `Content-Type: application/json` on POST/DELETE bodies. No CORS is enabled.

## Authentication

Browser clients use a host-only `budget_session` HttpOnly, Secure, SameSite=Strict cookie (Secure omitted only in explicit local development). All browser mutations, including login/setup/invite acceptance, must send an `Origin` exactly matching the configured public origin. A missing or different origin is rejected. Fetch should use same-origin credentials. Native Windows clients send `X-Budget-Client: native` on every request, have **no Origin header**, receive `token` in authentication responses, and send `Authorization: Bearer TOKEN` thereafter. The service never puts a session token in a browser JSON response. Native requests with Origin are rejected.

Usernames are case-insensitive; use an email address or a 3–128 character identifier with letters, numbers, `.`, `_`, `-`, `+`, `@`. Passwords are 12–256 characters. Sessions expire after 7 days. No public account registration or email sending exists. First setup requires a manually supplied server setup token; subsequent users register through an unexpired invitation addressed to their username. Email-shaped usernames are not verified email addresses: possession of the manually shared invitation code authorizes joining. Existing users sign in before accepting an invitation.

| Method/path | Body | Response |
| --- | --- | --- |
| `GET /health` | — | `{ok:true}` (no private information) |
| `POST /api/setup` | `{setupToken,username,password}` | `201 {user:{id,username},token?}`; available once only |
| `POST /api/login` | `{username,password}` | `200 {user:{id,username},token?}` |
| `POST /api/logout` | `{}` | `200 {ok:true}`; expires current session/cookie |
| `GET /api/me` | — | `{user:{id,username}}` |
| `GET /api/budgets` | — | `{budgets:[{id,name,revision,updatedAt,role}]}` |
| `POST /api/budgets` | `{id,name,payload} (client-generated UUID id)` | `201` full snapshot; initial revision is `1` |
| `GET /api/budgets/:id` | — | Full snapshot |
| `POST /api/budgets/:id/commits` | `{operationId,expectedRevision,payload,summary}` | Full snapshot at accepted revision |
| `GET /api/budgets/:id/history` | — | `{history:[{revision,author:{id,username},summary,createdAt,operationId}]}` newest first, last 50 revisions |
| `GET /api/budgets/:id/history/:revision` | — | Full snapshot at retained revision |
| `POST /api/budgets/:id/invites` | `{recipient}` | `201 {invite:{id,recipient,expiresAt},code}`; owner only; code shown once for manual sharing |
| `DELETE /api/budgets/:id/invites/:inviteId` | `{}` | `{ok:true}`; owner only, cancels unused invite |
| `POST /api/invites/accept` | `{code}` if signed in; `{code,username,password}` for new user | `{user:{id,username},budgetId,token?}`; new browser account also sets cookie |
| `GET /api/budgets/:id/members` | — | `{members:[{id,username,role}]}` |
| `DELETE /api/budgets/:id/members/:userId` | `{}` | `{ok:true}`; owner only; cannot revoke owner |

A full snapshot is `{id,name,revision,payload,updatedAt,author:{id,username},summary,operationId,role}`. The role is `owner` or `editor`. Both roles may commit/restore. Only the owner manages membership/invitations. Every read/write checks current membership, including an idempotent commit retry. Nonmembers receive `404` to avoid revealing existence. Unauthenticated callers receive `401`.

Each commit has a client-generated unique `operationId` (8–128 ASCII letters/digits/`._:-`) and an `expectedRevision`. A successful commit increments revision once. Stale requests receive `409`, code `revision_conflict`, with `current`. An identical operation ID/body retry by the same user returns the **original** snapshot even after later commits; reused ID with different body or author receives `409 operation_reused`. Persist the entire original request before sending. Payload is at most 8 MiB encoded JSON, name at most 100 characters, summary at most 200 characters. Request size is capped at 8 MiB + 64 KiB. Operators can lower the payload limit to match hosting resources. With encryption, summaries must be generic event labels such as `Updated budget`, without descriptions or amounts.

**Restore is client-mediated:** GET a retained snapshot, decrypt using its original budget ID/revision, review it, reseal for `currentRevision + 1`, and POST an ordinary commit with `expectedRevision: currentRevision` and summary `Restored budget`. The service has no ciphertext-copy restore endpoint: encrypted associated data is bound to the revision and must be resealed. A concurrent change returns the ordinary conflict response.

History browsing retains the latest 50 snapshots. Accepted-operation receipts retain their original opaque result for durable retry idempotency, including retries after a snapshot leaves browsable history. Receipt storage is limited to 10,000 operations or 256 MiB per budget and 1 GiB service-wide; additional new commits return `507 storage_limit` before mutation while existing retries still work. The service permits at most 16 budgets and 100 accounts. Include operation receipts in backups. These limits intentionally retain retry safety rather than expiring an acknowledged operation ID and silently applying it twice. Reaching a quota requires an operator-assisted export/migration to a new identity.

Invitations expire after 48 hours, are recipient-bound and one-use. Expired, cancelled, used, or recipient-mismatched codes fail without joining. At most 200 outstanding invitations may exist per budget; expired/used invitations are pruned when issuing another. A revoked member may rejoin only with a newly issued invitation. Session logout affects only the current device. Rate limits return `429` with `Retry-After`; validation returns `400` or `413`; invalid credentials use generic `401 invalid_credentials`.

General API allowances are 1,200 requests per minute per authenticated user (across their sessions), independently of anonymous clients' 1,200 requests per minute per client identity. Authentication endpoints each allow 10 attempts per client identity per 15 minutes and use a separate allowance from ordinary anonymous API calls. There is no anonymous account-name lockout. Authentication bodies have a 16 KiB limit. Bounded authentication, body and password-hashing concurrency returns `503 service_busy` with `Retry-After: 1` when temporarily full; retry with backoff. Anonymous body processing and authenticated body processing have separate capacity. Client-rate tracking reclaims expired entries and uses bounded keyed overflow buckets at capacity; overflow clients can share a limit, but saturation does not disable limiting or reject all new identities.

## Running and embedding

Node 24.19+ with built-in `node:sqlite` is required; there are no npm dependencies. Run `node budget-mobile/server/start.mjs` with `BUDGET_DATA_DIR` (persistent private directory), `BUDGET_PUBLIC_ORIGIN` (HTTPS URL) and `BUDGET_SETUP_TOKEN` (random secret, at least 24 characters) configured. Optional `BUDGET_HOST` defaults to localhost, `PORT` to 8787, and `BUDGET_MAX_PAYLOAD_BYTES` to 8388608 (allowed range 1024–8388608). `BUDGET_DEVELOPMENT=1` permits an HTTP localhost origin for generated-fixture development only. Do not set development mode on hosting. Static assets are served only from the configured `publicDir`, with an explicit extension allowlist and no directory listing. `/health` is suitable for liveness checks.

`BUDGET_TRUSTED_PROXY` defaults to `none`; forwarded address headers are ignored. Public Render web services may explicitly set it to `render`, requiring platform-provided `RENDER=true`, a private/loopback socket transport and a single valid `CF-Connecting-IP` header on API requests. Missing or malformed identities fail with `400 invalid_client_identity`. The internal listener must be reachable only by trusted ingress/workspace services. Render's managed Cloudflare ingress overwrites this header; `X-Forwarded-For` is never trusted. Do not enable this mode for another provider or arbitrary private forwarding. See [deployment requirements](server/DEPLOYMENT.md).

`createBudgetServer({dataDir,publicDir,setupToken,publicOrigin,development=false,now=Date.now,inviteTtlMs=172800000,sessionTtlMs=604800000,historyLimit=50,maxPayloadBytes=8388608,trustedProxy='none',renderEnvironment=false})` from `server/server.mjs` returns `{server,close}`. `renderEnvironment` must reflect trusted process/platform configuration, never request headers. Call `server.listen(port,'127.0.0.1')` behind a local proxy, or bind the private container interface required by the configured host; `close()` shuts down HTTP and SQLite, allowing five seconds for open connections to drain. `now` and TTL overrides support real-HTTP generated-fixture tests. Keep the data directory outside the static directory, version control and release archives. Back up the whole SQLite database using SQLite's backup facilities or a stopped service; do not copy a live database without its WAL. Account recovery and hosting backup policy must be configured before real household use.

