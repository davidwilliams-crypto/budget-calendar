# Security review — 2026-10-31 (release 0.10.0)

Scope: everything a household runs or hosts — the Windows app (`budget-gui`), the shared Rust
engine (`budget-core`/`budget-engine`, native and WebAssembly), the phone app
(`budget-mobile/public`), and the private sync service (`budget-mobile/server/server.mjs`) —
reviewed at the working tree that produced release 0.10.0. The review was performed by reading
every security-relevant module and the tests that pin the behaviour; it is a code review, not a
third-party audit.

## Verdict

**No high-severity findings.** The encryption design is sound and unchanged in its foundations;
the 0.10.0 passcode feature preserves the random 256-bit household key and adds a wrapped copy
of it, with costs and validation consistent everywhere. Two medium findings from the earlier
design remain by deliberate trade-off (documented below), and four low-severity observations
carry over. The pre-existing clippy warnings in sharing code predate this review and are
cosmetic, not security bugs.

## What protects a household, layer by layer

| Secret | Where it lives | Protection |
| --- | --- | --- |
| Household budget key (random 256-bit, `BC1-…` text) | Devices only | Never sent to the service; on Windows inside a DPAPI-encrypted per-user cache, in the browser as a non-extractable `CryptoKey` in IndexedDB |
| Household passcode (user-chosen, 0.10.0) | Heads of household members | Never sent anywhere; used locally with Argon2id to unwrap the budget key from the service-stored slot |
| Account password (service login) | Service | scrypt (N=32768, r=8, p=1) with per-user 16-byte salt; constant-time compare with a dummy hash for unknown users |
| Session token | Service (SHA-256 hash) + device | 32 random bytes; bearer token (native) or HttpOnly, Secure, SameSite=Strict cookie (browser); 7-day expiry |
| Invitation codes | Service (SHA-256 hash) | 32 random bytes, 48-hour expiry, recipient-bound, one use |
| Sync payload | Service | AES-256-GCM under the household key; associated data binds budget id **and revision** |
| Standalone `.bgt` password files | Disk | Argon2id (64 MiB, t=3, p=4) + AES-256-GCM whole-file, parameters authenticated as AAD |
| Passcode slot (0.10.0) | Service | AES-256-GCM wrap of the household key under an Argon2id-derived key; identical cost table as password files |

Cryptographic primitives come from RustCrypto (`aes-gcm` 0.10.3, `aes` 0.8.4 with zeroize,
`argon2` 0.6) and, in the browser, WebCrypto AES-GCM with the Argon2id performed inside the
same Rust engine compiled to WebAssembly. Randomness is `getrandom`/`OsRng`/`crypto.getRandomValues`.
Keys and derived secrets are `Zeroizing` in Rust. Random 96-bit nonces per encryption are safe
at this app's message volumes.

## The 0.10.0 passcode change — security analysis

**Design.** The random key remains the root secret. A member device can unwrap it with a
passcode the owner chose: `ke = Argon2id(passcode, salt)` (64 MiB / 3 passes / 4 lanes — the
same profile as password-protected files), then the household key is recovered from
`wrappedKey = AES-256-GCM(ke, key, aad = "BudgetCalendar:passcode:1:{budgetId}")`. The GCM tag
is the wrong-passcode test; there is no separate verifier to attack. The slot is stored on the
household (owner writes, members read); the service validates only its exact fixed structure.

**What this intentionally gives up.** Before 0.10.0, a full service-database leak yielded
ciphertexts under a random 256-bit key — practically unbreakable. With a passcode slot stored,
the same leak plus offline guessing of the passcode can decrypt the budget. This is the
inherent cost of "a passcode I can manage" and is mitigated, not eliminated, by:

- Argon2id at 64 MiB / 3 passes per guess — about 0.5–1.5 s and 64 MiB per attempt on this
  class of device, which prices bulk guessing well above a household attacker's reach;
- a minimum of 8 characters plus a small common-passcode denylist, enforced wherever a slot is
  created (Rust engine action, so every client inherits it);
- the recovery key continuing to work, so a household that wants the previous posture can
  simply remove the passcode and use keys only.

**What is verified by tests.** Wrap/unwrap round-trip and wrong-passcode failure (Rust unit,
engine action, native/WASM/browser-wrapper parity, phone browser end-to-end, desktop HTTP
fixture); tampered or cost-reduced slots refused before any Argon2 work; the slot is bound to
its budget id; owner-only writes and member reads on the service; a revoked member loses the
slot; a phone changing the passcode must first prove the current passcode or key and the
candidate key is verified against the household before any re-wrap, so a mistyped current
secret cannot overwrite a good slot with an unusable one; creation failures still open the
household and name the remedy.

## Findings

Nothing new is rated high or critical. Findings that remain open are deliberate trade-offs or
accepted residuals from the 0.8/0.9 reviews, restated here with their status.

1. **Medium — accepted trade-off.** A service-database leak plus successful offline passcode
   guessing can decrypt the household budget (see above). Recovery-key-only households keep the
   original guarantee by removing or never setting a passcode.
2. **Medium — inherent to sync.** Anyone with a member's device access (or the browser profile)
   reads the household budget: the Windows cache is DPAPI-bound to the logged-in Windows user,
   and the browser key is non-extractable but usable by any script in that origin. Device
   login/screen-lock remains the real boundary; documented in the phone's own help text.
3. **Low — legacy read compatibility.** Old `.bgt` line encryption is Blowfish-ECB (and an XOR
   scheme before that). Reading them is required to open historical files; the app never writes
   them. Password-protected exports since 0.8.3 use the sealed format. Keep migrating old files.
4. **Low — residual.** On the phone, the household key's `BC1-` text is held in JavaScript
   memory for the session after a passcode unlock (needed for passcode changes and key display),
   alongside the non-extractable key. It is cleared on household switch and sign-out. Equivalent
   exposure already existed whenever the key was typed.
5. **Low — residual.** Desktop password/key/passcode fields are Rust `String`s that cannot be
   zeroized on drop; the underlying crypto keys are zeroized. A memory-compromise attacker with
   this access already has broader reach.
6. **Low — operational.** The service must sit behind an HTTPS reverse proxy with loopback-only
   listening; `createBudgetServer` enforces an HTTPS public origin (HTTP only for localhost
   development) and refuses to start otherwise. Hosting mistakes are the main realistic risk;
   `server/DEPLOYMENT.md` covers ingress, quotas, backups and logs.

## Checks performed

- **Cipher usage.** Nonce generation, AAD binding (budget id + revision for sync; header +
  parameter tuple for sealed files; budget id for passcode slots), tag verification before any
  plaintext use, length caps on all inputs (8 MiB snapshots, 6 MiB engine requests, 64 MiB
  files, 64 KiB passwords), `deny_unknown_fields` on every serialized security record.
- **Service.** Every route requires membership (non-members get `404` without existence
  disclosure); owner-only routes re-check inside their transaction; commits are idempotent by
  operation id with request-hash binding; invitation reuse/expiry/recipient binding; rate
  limits per user, per client identity and per authentication route with bounded memory;
  body-size and content-type checks before parsing; SQLite access fully parameterized;
  static-file serving rejects traversal, hidden files, drive-absolute names and non-allowlisted
  extensions; security headers (CSP without `unsafe-inline`, HSTS, nosniff, DENY framing,
  Permissions-Policy); request bodies never logged. The three new passcode routes follow the
  same pattern and are covered by tests.
- **Phone app.** All HTML interpolations re-checked: every user-derived string passes `esc()`;
  remaining raw interpolations are numbers from the local engine, static literals, or
  `encodeURIComponent` output. The service worker caches a fixed same-origin shell list; API
  calls bypass it. The browser wrapper replaces caller entropy for password export and
  passcode wrapping (verified by parity test).
- **Windows app.** Server origins normalized and pinned to scheme/host (`https`, or loopback
  `http` for testing), rejecting credentials, paths, queries, fragments and control
  characters; link opening restricted to canonical origins; the shared cache is protected with
  DPAPI and a per-window lock, written atomically after size checks; report HTML escapes
  descriptions, categories and notes (0.7.0); reports open via the OS URL API, not a shell.
- **Dependencies.** No runtime npm dependencies at all (Node stdlib only). Rust: `reqwest`
  0.12.28 with **rustls** 0.23.46 (no native TLS), `aes-gcm` 0.10.3, `aes` 0.8.4, `argon2`
  0.6, `zeroize` 1.x, `egui`/`eframe` 0.31. No known published advisories affect these
  versions as locked. `cargo audit` cannot run in this offline environment — re-run it once
  the machine is online; the lockfile is committed for exact reproduction.
- **Regression.** Full workspace tests, engine parity, every mobile unit and browser test, the
  three-client integration (including the Windows native step) and the release-gate script
  tests all pass on this tree; see the 0.10.0 release record for the run summary.

## Recommendations

1. Keep the Argon2id cost table fixed and identical across formats; any future change needs a
   new slot version and migration, never a silent loosening.
2. When the machine is online, run `cargo audit` and `npm audit` (dev-only) against the
   committed lockfiles and record results beside this review.
3. Prefer passphrases of three or more unrelated words — the UI asks for exactly that; the
   denylist is a floor, not a defence.
4. If a household's threat model includes the service host being compromised, remove the
   passcode and distribute the recovery key by hand instead (owner action, immediately
   effective).
