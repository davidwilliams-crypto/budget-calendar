# Security remediation implementation plan

**Goal:** Close the four validated medium findings from scan `8c70f361-d559-4b6d-aecf-defaf5d64e8b` while preserving existing budgets and workflows. The user explicitly requested best-in-class fixes for all four.

**Architecture:** Enforce safe URL opening at the shared native launcher; separate authenticated and anonymous resource quotas and explicitly validate trusted Render identity; replace new password-protected standalone writes with a versioned Argon2id/AES-GCM envelope at the common file boundary. Legacy readers remain available and migration occurs only on an explicit successful save.

**Constraints:** Preserve personal `.bgt` files, caches and open drafts. All fixtures are generated under repository `target/`. Reuse the current clean feature checkout and configured toolchain/cache; agents own disjoint source groups. No live attack or personal-budget tests. Root owns version 0.8.3, integration, review, releases and deployment.

## 1. URL authority boundary

Replace the vendored Windows `cmd /C start` URL path with a shell-free Windows API. Validate HTTP(S) URLs without NUL/control/userinfo, and canonicalize every shared-budget origin before it becomes a hyperlink or retained connection. Keep report viewing and ordinary HTTPS/loopback development behavior. Prove malicious metacharacter/encoding inputs cannot become shell commands without launching a real browser during tests.

## 2. Service isolation and bounded resources

Authenticate before choosing a signed-in user/session API quota. Keep anonymous API and authentication limits independent. Default deployments trust no forwarding headers; explicit Render mode consumes one validated provider-controlled client-IP header and rejects missing/malformed identity. Bound request/body and password-KDF concurrency and limiter memory; reclaim expired entries at capacity. Test separate external identities behind one proxy, untrusted spoofing, canonical IP equivalence, legitimate login/read/commit survival, admission release, and existing auth/invitation/receipt behavior.

Render's own deployment guidance recommends `CF-Connecting-IP` and warns that the leftmost `X-Forwarded-For` value is spoofable: https://render.com/articles/host-pocketbase-on-render . The selected live mode must be configured explicitly and verified after rollout without reading existing secret values.

## 3. Standalone confidentiality and integrity

Use Argon2id version 1.3 with 64 MiB, three iterations and four lanes, a fresh 128-bit salt and 256-bit derived key, following RFC 9106's memory-constrained recommendation: https://datatracker.ietf.org/doc/html/rfc9106 . Encrypt and authenticate the whole serialized plaintext and format metadata using AES-256-GCM and a fresh 96-bit nonce. Strictly validate version, fields, encodings, sizes and KDF bounds before expensive work. Never return partial decrypted financial data or log it on failure. Retain the complete UTF-8 password rather than truncating it.

Preserve plain serialization and existing Blowfish/XOR/short-password/BOM/newline readers. On explicit successful Save, write the modern format and retain the original bytes in normal backups. Test tampering, truncation, record duplication/reordering, wrong/long/Unicode passwords, unknown/duplicate metadata and cost/size bombs, plus legacy migration and exact-byte file-conflict detection.

Keep the raw WASM module's empty-import ABI. Portable crypto does not enable unsupported random imports: native saves use OS entropy; the official browser wrapper generates fresh CSPRNG entropy for encrypted export through an additive JSON field, replacing any caller-supplied entropy. Missing entropy in direct raw-WASM calls fails closed. Verify modern encrypted native/browser import/export interoperability and rebuild the shipped engine.

## 4. Native integration and final review

Run password derivation/decryption once in the file worker instead of synchronously checking a password and decrypting it again. Preserve captured-file bytes, keyboard retry/cancel and unsaved-draft guards. A fresh bypass/regression reviewer examines all final changes after focused red/green checks. Confirm every reported bypass or regression before revising the patch; complete one review cycle.

## 5. Release and activation

Run relevant security regressions, full workspace/mobile tests, native/WASM parity, standalone build-script tests, archive/package checks, release builds and CLI version readback. Commit exact final sources, archive immutable Windows and mobile 0.8.3 releases, verify hashes, and push the existing private feature branch. Deploy the exact tested commit to the existing Render service with its required explicit trust configuration; preserve its persistent volume and other environment settings. Verify live health and released asset hashes. Leave any running application draft alone.

Record a retained fix report against the completed scan's target-bound artifact collection, with findings, source changes, trigger and legitimate-control results, commands, limitations, release identity and deployment evidence. Do not modify the sealed original scan.
