import { createServer } from 'node:http';
import { DatabaseSync } from 'node:sqlite';
import { createHash, createHmac, randomBytes, randomUUID, scrypt, timingSafeEqual } from 'node:crypto';
import { BlockList, isIP } from 'node:net';
import { promisify } from 'node:util';
import { mkdirSync, realpathSync } from 'node:fs';
import { readFile, realpath, stat } from 'node:fs/promises';
import { resolve, join, relative, extname, isAbsolute } from 'node:path';

const derive = promisify(scrypt);
const PAYLOAD_LIMIT = 8 * 1024 * 1024;
const AUTH_BODY_LIMIT = 16 * 1024;
const RATE_CAPACITY = 10000;
const KDF = { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 };
const MIME = { '.html': 'text/html; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json',
  '.webmanifest': 'application/manifest+json', '.wasm': 'application/wasm', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.ico': 'image/x-icon', '.webp': 'image/webp' };
const hash = value => createHash('sha256').update(value).digest('hex');
const opaqueToken = () => randomBytes(32).toString('base64url');
class HttpError extends Error {
  constructor(status, code, message, current) { super(message); Object.assign(this, { status, code, current }); }
}
const fail = (status, code, message, current) => { throw new HttpError(status, code, message, current); };
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') return `{${Object.keys(value).sort().map(k => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  return JSON.stringify(value);
}
function text(value, field, min, max) {
  if (typeof value !== 'string' || value.length < min || value.length > max || /[\u0000-\u001f\u007f]/u.test(value)) {
    fail(400, 'invalid_request', `Invalid ${field}`);
  }
  return value;
}
function username(value) {
  const name = text(value, 'username', 3, 128).toLowerCase();
  if (!/^[a-z0-9][a-z0-9._+@-]*$/.test(name)) fail(400, 'invalid_request', 'Invalid username');
  return name;
}
function password(value) { return text(value, 'password', 12, 256); }
function operationId(value) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9._:-]{8,128}$/.test(value)) fail(400, 'invalid_request', 'Invalid operationId');
  return value;
}
/** The passcode slot is opaque here: exactly the fixed wrap the clients build, nothing more. */
function passcodeSlot(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || value.version !== 1 || value.kdf !== 'argon2id' || value.kdfVersion !== 19
    || value.memoryKiB !== 65536 || value.iterations !== 3 || value.parallelism !== 4
    // 16-byte salt -> 22 chars + '=='; 12-byte nonce and 48-byte wrap have no padding.
    || !/^[A-Za-z0-9+/]{22}==$/.test(value.salt ?? '') || !/^[A-Za-z0-9+/]{16}$/.test(value.nonce ?? '')
    || !/^[A-Za-z0-9+/]{64}$/.test(value.wrappedKey ?? '')
    || Object.keys(value).some(k => !['version', 'kdf', 'kdfVersion', 'memoryKiB', 'iterations', 'parallelism', 'salt', 'nonce', 'wrappedKey'].includes(k))) {
    fail(400, 'invalid_request', 'Invalid household passcode record');
  }
  return value;
}
function revision(value) {
  if (!Number.isSafeInteger(value) || value < 1) fail(400, 'invalid_request', 'Invalid revision');
  return value;
}
function payload(value, limit) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail(400, 'invalid_request', 'Payload must be an opaque JSON object');
  const encoded = JSON.stringify(value);
  if (Buffer.byteLength(encoded) > limit) fail(413, 'payload_too_large', 'Payload exceeds service limit');
  return encoded;
}
function publicUser(row) { return { id: row.id, username: row.username }; }
function within(root, path) { const rel = relative(root, path); return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel)); }
function canonicalAddress(value) {
  if (typeof value !== 'string' || value.length > 45 || value.includes('%') || !isIP(value)) return null;
  if (isIP(value) === 4) return value;
  const normalized = new URL(`http://[${value}]/`).hostname.slice(1, -1);
  const mapped = /^::ffff:([0-9a-f]+):([0-9a-f]+)$/.exec(normalized);
  if (!mapped) return normalized;
  const high = Number.parseInt(mapped[1], 16); const low = Number.parseInt(mapped[2], 16);
  return `${high >>> 8}.${high & 255}.${low >>> 8}.${low & 255}`;
}
const proxyNetworks = new BlockList();
for (const [address, prefix] of [['127.0.0.0', 8], ['10.0.0.0', 8], ['172.16.0.0', 12], ['192.168.0.0', 16]]) proxyNetworks.addSubnet(address, prefix, 'ipv4');
proxyNetworks.addAddress('::1', 'ipv6');
proxyNetworks.addSubnet('fc00::', 7, 'ipv6');

/** Private, dependency-free HTTP API. TLS termination belongs to the hosting reverse proxy. */
export function createBudgetServer(options) {
  const { setupToken, development = false, now = Date.now, inviteTtlMs = 48 * 3600000,
    sessionTtlMs = 7 * 86400000, historyLimit = 50, maxPayloadBytes = PAYLOAD_LIMIT,
    trustedProxy = 'none', renderEnvironment = false } = options;
  if (!['none', 'render'].includes(trustedProxy)) throw new Error('Unsupported trusted proxy configuration');
  if (trustedProxy === 'render' && renderEnvironment !== true) throw new Error('Render proxy mode requires the Render platform environment');
  let origin;
  try { origin = new URL(options.publicOrigin); } catch { throw new Error('Configure a public HTTPS origin'); }
  if (origin.origin !== options.publicOrigin || origin.username || origin.password ||
      (origin.protocol !== 'https:' && !(development && origin.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(origin.hostname)))) {
    throw new Error('A public HTTPS origin is required (HTTP localhost is development only)');
  }
  if (typeof setupToken !== 'string' || setupToken.length < 24) throw new Error('A setup token of at least 24 characters is required');
  if (!Number.isInteger(historyLimit) || historyLimit < 20 || historyLimit > 200) throw new Error('History retention must be 20–200 snapshots');
  if (!Number.isInteger(maxPayloadBytes) || maxPayloadBytes < 1024 || maxPayloadBytes > PAYLOAD_LIMIT) throw new Error('Payload limit must be 1 KiB–8 MiB');
  if (!(inviteTtlMs > 0) || !(sessionTtlMs > 0)) throw new Error('Session and invitation lifetimes must be positive');
  if (!options.dataDir) throw new Error('A private persistent dataDir is required');
  const requestedDataDir = resolve(options.dataDir);
  const publicDir = options.publicDir ? realpathSync(resolve(options.publicDir)) : null;
  mkdirSync(requestedDataDir, { recursive: true, mode: 0o700 });
  const dataDir = realpathSync(requestedDataDir);
  if (publicDir && within(publicDir, dataDir)) throw new Error('Data directory must be outside public assets');
  const db = new DatabaseSync(join(dataDir, 'budget.sqlite'));
  db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;
    CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, salt TEXT NOT NULL, password_hash TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS sessions (token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), kind TEXT NOT NULL, expires_at INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS budgets (id TEXT PRIMARY KEY, name TEXT NOT NULL, revision INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS members (budget_id TEXT NOT NULL REFERENCES budgets(id), user_id TEXT NOT NULL REFERENCES users(id), role TEXT NOT NULL, PRIMARY KEY(budget_id,user_id));
    CREATE TABLE IF NOT EXISTS snapshots (budget_id TEXT NOT NULL REFERENCES budgets(id), revision INTEGER NOT NULL, payload TEXT NOT NULL, author_id TEXT NOT NULL REFERENCES users(id), summary TEXT NOT NULL, created_at TEXT NOT NULL, operation_id TEXT, PRIMARY KEY(budget_id,revision));
    CREATE TABLE IF NOT EXISTS operations (budget_id TEXT NOT NULL REFERENCES budgets(id), operation_id TEXT NOT NULL, author_id TEXT NOT NULL REFERENCES users(id), request_hash TEXT NOT NULL, result TEXT NOT NULL, PRIMARY KEY(budget_id,operation_id));
    CREATE TABLE IF NOT EXISTS invites (id TEXT PRIMARY KEY, code_hash TEXT NOT NULL UNIQUE, budget_id TEXT NOT NULL REFERENCES budgets(id), recipient TEXT NOT NULL, expires_at INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS passcodes (budget_id TEXT PRIMARY KEY REFERENCES budgets(id), slot TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by TEXT NOT NULL REFERENCES users(id));
    CREATE INDEX IF NOT EXISTS session_expiry ON sessions(expires_at);
    CREATE INDEX IF NOT EXISTS member_user ON members(user_id);`);
  const sql = query => db.prepare(query);
  const transaction = fn => {
    db.exec('BEGIN IMMEDIATE');
    try { const result = fn(); db.exec('COMMIT'); return result; } catch (error) { db.exec('ROLLBACK'); throw error; }
  };
  const overflowSecret = randomBytes(32);
  const limiter = () => ({ entries: new Map(), overflow: new Array(1024), nextCleanup: 0, usingOverflow: false });
  const anonymousRate = limiter(); const authenticationRate = limiter();
  const authenticatedRate = limiter(); const invitationRate = limiter();
  function throttle(rate, key, limit, windowMs = 15 * 60000) {
    const time = now();
    let entry = rate.entries.get(key);
    if (!entry && rate.entries.size >= RATE_CAPACITY && time >= rate.nextCleanup) {
      for (const [stored, value] of rate.entries) if (value.reset <= time) rate.entries.delete(stored);
      rate.nextCleanup = time + 60000;
    }
    if (!entry) {
      const slot = rate.usingOverflow || rate.entries.size >= RATE_CAPACITY
        ? createHmac('sha256', overflowSecret).update(key).digest().readUInt16BE(0) % rate.overflow.length : null;
      const overflow = slot === null ? null : rate.overflow[slot];
      if (rate.entries.size < RATE_CAPACITY) {
        // Keep an unexpired fallback allowance when cleanup makes room for this identity.
        entry = overflow?.reset > time ? overflow : { count: 0, reset: time + windowMs };
        rate.entries.set(key, entry);
      } else {
        // Saturation must neither disable limiting nor lock out every new client. The keyed,
        // fixed-size fallback bounds memory and prevents callers choosing known bucket collisions.
        rate.usingOverflow = true;
        entry = overflow;
        if (!entry || entry.reset <= time) rate.overflow[slot] = entry = { count: 0, reset: time + windowMs };
      }
    }
    if (entry.reset <= time) { entry.count = 0; entry.reset = time + windowMs; }
    if (entry.count >= limit) { const error = new HttpError(429, 'rate_limited', 'Too many requests'); error.retryAfter = Math.max(1, Math.ceil((entry.reset - time) / 1000)); throw error; }
    entry.count++;
  }
  function clientIdentity(req) {
    const peer = canonicalAddress(req.socket.remoteAddress);
    if (trustedProxy === 'none') return peer ?? 'unknown';
    if (!peer || !proxyNetworks.check(peer, isIP(peer) === 4 ? 'ipv4' : 'ipv6')) fail(400, 'invalid_client_identity', 'Trusted proxy connection required');
    const client = canonicalAddress(req.headers['cf-connecting-ip']);
    if (!client) fail(400, 'invalid_client_identity', 'Trusted proxy client address required');
    return client;
  }
  const busy = () => { const error = new HttpError(503, 'service_busy', 'The service is busy; retry shortly'); error.retryAfter = 1; throw error; };
  function admission(limit, perClient) {
    let active = 0; const clients = new Map();
    return key => {
      const count = clients.get(key) ?? 0;
      if (active >= limit || count >= perClient) busy();
      active++; clients.set(key, count + 1);
      return () => { active--; const left = clients.get(key) - 1; if (left) clients.set(key, left); else clients.delete(key); };
    };
  }
  const admitAuthentication = admission(8, 2);
  const admitAnonymousBody = admission(8, 2);
  const admitAuthenticatedBody = admission(4, 2);
  const admitKdf = admission(2, 2);
  const iso = () => new Date(now()).toISOString();
  const membership = (budgetId, userId, ownerOnly = false) => {
    const row = sql('SELECT b.*,m.role FROM budgets b JOIN members m ON m.budget_id=b.id WHERE b.id=? AND m.user_id=?').get(budgetId, userId);
    if (!row) fail(404, 'not_found', 'Budget not found');
    if (ownerOnly && row.role !== 'owner') fail(403, 'owner_required', 'Owner access required');
    return row;
  };
  function snapshot(budget, rev = budget.revision) {
    const row = sql('SELECT s.*,u.username FROM snapshots s JOIN users u ON u.id=s.author_id WHERE s.budget_id=? AND s.revision=?').get(budget.id, rev);
    if (!row) fail(404, 'not_found', 'Snapshot is no longer retained');
    return { id: budget.id, name: budget.name, revision: row.revision, payload: JSON.parse(row.payload),
      updatedAt: row.created_at, author: { id: row.author_id, username: row.username }, summary: row.summary,
      operationId: row.operation_id, role: budget.role };
  }
  const kdf = async (secret, salt) => {
    const release = admitKdf('password');
    try { return Buffer.from(await derive(secret, salt, 64, KDF)); } finally { release(); }
  };
  async function newUserCredentials(name, secret) {
    const salt = randomBytes(16).toString('hex');
    return { id: randomUUID(), username: name, salt, password_hash: (await kdf(secret, salt)).toString('hex') };
  }
  const insertUser = user => sql('INSERT INTO users(id,username,salt,password_hash) VALUES(?,?,?,?)').run(user.id, user.username, user.salt, user.password_hash);
  function authenticate(req, native) {
    let token;
    if (native) {
      const auth = req.headers.authorization;
      if (typeof auth === 'string' && /^Bearer [A-Za-z0-9_-]{43}$/.test(auth)) token = auth.slice(7);
    } else {
      token = /(?:^|;\s*)budget_session=([A-Za-z0-9_-]{43})(?:;|$)/.exec(req.headers.cookie ?? '')?.[1];
    }
    if (!token) return null;
    const row = sql('SELECT u.*,s.token_hash FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.kind=? AND s.expires_at>?').get(hash(token), native ? 'native' : 'browser', now());
    return row ?? null;
  }
  function session(user, native, res) {
    const token = opaqueToken();
    sql('DELETE FROM sessions WHERE expires_at<=?').run(now());
    sql('INSERT INTO sessions(token_hash,user_id,kind,expires_at) VALUES(?,?,?,?)').run(hash(token), user.id, native ? 'native' : 'browser', now() + sessionTtlMs);
    if (!native) res.setHeader('Set-Cookie', `budget_session=${token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=${Math.floor(sessionTtlMs / 1000)}${development ? '' : '; Secure'}`);
    return native ? { user: publicUser(user), token } : { user: publicUser(user) };
  }
  async function body(req, user, identity, limit) {
    const release = (user ? admitAuthenticatedBody : admitAnonymousBody)(user?.id ?? identity);
    try {
      if (req.headers['content-type']?.split(';')[0].trim().toLowerCase() !== 'application/json') fail(415, 'unsupported_media_type', 'Use application/json');
      if (Number(req.headers['content-length']) > limit) { req.resume(); fail(413, 'payload_too_large', 'Request too large'); }
      const chunks = []; let length = 0;
      for await (const chunk of req) {
        length += chunk.length;
        if (length > limit) { req.resume(); fail(413, 'payload_too_large', 'Request too large'); }
        chunks.push(chunk);
      }
      let parsed;
      try { parsed = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { fail(400, 'invalid_json', 'Invalid JSON body'); }
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) fail(400, 'invalid_request', 'Use a JSON object');
      return parsed;
    } finally { release(); }
  }
  const send = (res, status, data) => { res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' }); res.end(JSON.stringify(data)); };
  function commit(user, budgetId, input) {
    operationId(input.operationId); revision(input.expectedRevision);
    const encoded = payload(input.payload, maxPayloadBytes);
    const summary = text(input.summary ?? 'Updated budget', 'summary', 0, 200);
    const fingerprint = hash(canonical({ expectedRevision: input.expectedRevision, payload: input.payload, summary }));
    return transaction(() => {
      // Check membership before receipts. Revoked users must not read their old acknowledgements.
      const budget = membership(budgetId, user.id);
      const receipt = sql('SELECT * FROM operations WHERE budget_id=? AND operation_id=?').get(budgetId, input.operationId);
      if (receipt) {
        if (receipt.author_id !== user.id || receipt.request_hash !== fingerprint) fail(409, 'operation_reused', 'Operation ID was used for different content');
        return { ...JSON.parse(receipt.result), role: budget.role };
      }
      if (budget.revision !== input.expectedRevision) fail(409, 'revision_conflict', 'Budget changed; review the current snapshot', snapshot(budget));
      const used = sql('SELECT count(*) AS n,coalesce(sum(length(CAST(result AS BLOB))),0) AS bytes FROM operations WHERE budget_id=?').get(budgetId);
      const allUsed = sql('SELECT coalesce(sum(length(CAST(result AS BLOB))),0) AS bytes FROM operations').get();
      if (used.n >= 10000 || used.bytes + Buffer.byteLength(encoded) + 4096 >= 256 * 1024 * 1024 ||
        allUsed.bytes + Buffer.byteLength(encoded) + 4096 >= 1024 * 1024 * 1024) fail(507, 'storage_limit', 'Budget storage limit reached; contact the operator');
      const next = budget.revision + 1;
      sql('INSERT INTO snapshots(budget_id,revision,payload,author_id,summary,created_at,operation_id) VALUES(?,?,?,?,?,?,?)').run(budgetId, next, encoded, user.id, summary, iso(), input.operationId);
      sql('UPDATE budgets SET revision=? WHERE id=?').run(next, budgetId);
      const result = snapshot({ ...budget, revision: next });
      sql('INSERT INTO operations(budget_id,operation_id,author_id,request_hash,result) VALUES(?,?,?,?,?)').run(budgetId, input.operationId, user.id, fingerprint, JSON.stringify(result));
      sql('DELETE FROM snapshots WHERE budget_id=? AND revision<=?').run(budgetId, next - historyLimit);
      return result;
    });
  }
  async function staticFile(req, res, path) {
    if (!publicDir || !['GET', 'HEAD'].includes(req.method)) fail(404, 'not_found', 'Not found');
    let decoded;
    try { decoded = decodeURIComponent(path); } catch { fail(404, 'not_found', 'Not found'); }
    if (decoded.includes('\\') || decoded.includes('\0') || decoded.split('/').some(part => part.startsWith('.') || part.includes(':'))) fail(404, 'not_found', 'Not found');
    const file = resolve(publicDir, '.' + (decoded === '/' ? '/index.html' : decoded));
    if (!within(publicDir, file) || !MIME[extname(file)]) fail(404, 'not_found', 'Not found');
    let actual; let info;
    try { actual = await realpath(file); info = await stat(actual); } catch { fail(404, 'not_found', 'Not found'); }
    if (!within(publicDir, actual) || !info.isFile() || info.size > 32 * 1024 * 1024) fail(404, 'not_found', 'Not found');
    const contents = await readFile(actual);
    res.setHeader('Cache-Control', ['.html', '.webmanifest'].includes(extname(file)) || /(?:service-worker|sw)\.m?js$/.test(file) ? 'no-cache' : 'public, max-age=3600');
    res.writeHead(200, { 'Content-Type': MIME[extname(file)], 'Content-Length': contents.length });
    res.end(req.method === 'HEAD' ? undefined : contents);
  }
  const server = createServer(async (req, res) => {
    let releaseAuthentication;
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'no-referrer');
    res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');
    res.setHeader('X-Frame-Options', 'DENY');
    res.setHeader('Permissions-Policy', 'camera=(), microphone=(), geolocation=()');
    res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; connect-src 'self'; worker-src 'self'; manifest-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'");
    if (!development) res.setHeader('Strict-Transport-Security', 'max-age=31536000');
    try {
      const url = new URL(req.url, 'http://internal.invalid');
      const path = url.pathname;
      if (req.method === 'GET' && path === '/health') return send(res, 200, { ok: true });
      if (!path.startsWith('/api/')) return await staticFile(req, res, path);
      const native = req.headers['x-budget-client'] === 'native';
      if (native && req.headers.origin !== undefined) fail(403, 'invalid_origin', 'Native requests must not contain Origin');
      if (!native && !['GET', 'HEAD'].includes(req.method) && req.headers.origin !== origin.origin) fail(403, 'invalid_origin', 'Same-origin request required');
      if (req.headers['sec-fetch-site'] === 'cross-site') fail(403, 'invalid_origin', 'Same-origin request required');
      const identity = clientIdentity(req);
      const user = authenticate(req, native);
      const authRoute = ['/api/setup', '/api/login', '/api/invites/accept'].includes(path) && req.method === 'POST';
      if (user) throttle(authenticatedRate, user.id, 1200, 60000);
      else if (!authRoute) throttle(anonymousRate, identity, 1200, 60000);
      const readBody = () => body(req, user, identity, authRoute ? AUTH_BODY_LIMIT : maxPayloadBytes + 64 * 1024);
      if (authRoute) {
        throttle(authenticationRate, `${path}:${identity}`, 10);
        releaseAuthentication = admitAuthentication(identity);
        const input = await readBody();
        if (path === '/api/setup') {
          if (sql('SELECT 1 FROM users LIMIT 1').get()) fail(409, 'already_setup', 'Setup is already complete');
          if (typeof input.setupToken !== 'string' || !timingSafeEqual(Buffer.from(hash(input.setupToken)), Buffer.from(hash(setupToken)))) fail(403, 'setup_denied', 'Invalid setup token');
          const name = username(input.username); const secret = password(input.password);
          const credentials = await newUserCredentials(name, secret);
          transaction(() => {
            if (sql('SELECT 1 FROM users LIMIT 1').get()) fail(409, 'already_setup', 'Setup is already complete');
            insertUser(credentials);
          });
          return send(res, 201, session(credentials, native, res));
        }
        if (path === '/api/login') {
          const name = username(input.username); const secret = text(input.password, 'password', 1, 256);
          const found = sql('SELECT * FROM users WHERE username=?').get(name);
          const calculated = await kdf(secret, found?.salt ?? 'a37e3bc6cc89ad7394322d60317fe155');
          const expected = found ? Buffer.from(found.password_hash, 'hex') : Buffer.alloc(64);
          if (!timingSafeEqual(calculated, expected) || !found) fail(401, 'invalid_credentials', 'Invalid username or password');
          return send(res, 200, session(found, native, res));
        }
        text(input.code, 'invitation code', 32, 128);
        const invite = sql('SELECT * FROM invites WHERE code_hash=?').get(hash(input.code));
        if (!invite || invite.used || invite.expires_at <= now()) fail(410, 'invite_unavailable', 'Invitation is expired, cancelled, or already used');
        let joining = user;
        if (!joining) {
          const name = username(input.username);
          if (name !== invite.recipient) fail(403, 'recipient_mismatch', 'Invitation belongs to another recipient');
          if (sql('SELECT 1 FROM users WHERE username=?').get(name)) fail(401, 'sign_in_required', 'Sign in before accepting this invitation');
          joining = await newUserCredentials(name, password(input.password));
        }
        if (joining.username !== invite.recipient) fail(403, 'recipient_mismatch', 'Invitation belongs to another recipient');
        transaction(() => {
          const fresh = sql('SELECT * FROM invites WHERE id=?').get(invite.id);
          if (!fresh || fresh.used || fresh.expires_at <= now()) fail(410, 'invite_unavailable', 'Invitation is expired, cancelled, or already used');
          if (!user) {
            if (sql('SELECT 1 FROM users WHERE username=?').get(joining.username)) fail(409, 'account_exists', 'Sign in before accepting this invitation');
            if (sql('SELECT count(*) AS n FROM users').get().n >= 100) fail(507, 'storage_limit', 'Account limit reached');
            insertUser(joining);
          }
          sql("INSERT INTO members(budget_id,user_id,role) VALUES(?,?,'editor') ON CONFLICT(budget_id,user_id) DO NOTHING").run(invite.budget_id, joining.id);
          sql('UPDATE invites SET used=1 WHERE id=?').run(invite.id);
        });
        const result = user ? { user: publicUser(user) } : session(joining, native, res);
        return send(res, 200, { ...result, budgetId: invite.budget_id });
      }
      if (!user) fail(401, 'authentication_required', 'Sign in required');
      if (path === '/api/me' && req.method === 'GET') return send(res, 200, { user: publicUser(user) });
      if (path === '/api/logout' && req.method === 'POST') {
        await readBody();
        sql('DELETE FROM sessions WHERE token_hash=?').run(user.token_hash);
        if (!native) res.setHeader('Set-Cookie', `budget_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0${development ? '' : '; Secure'}`);
        return send(res, 200, { ok: true });
      }
      if (path === '/api/budgets') {
        if (req.method === 'GET') {
          const budgets = sql('SELECT b.id,b.name,b.revision,m.role,s.created_at AS updatedAt FROM budgets b JOIN members m ON m.budget_id=b.id JOIN snapshots s ON s.budget_id=b.id AND s.revision=b.revision WHERE m.user_id=? ORDER BY b.name').all(user.id);
          return send(res, 200, { budgets });
        }
        if (req.method === 'POST') {
          const input = await readBody();
          if (typeof input.id !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(input.id)) fail(400, 'invalid_request', 'A UUID budget id is required');
          const name = text(input.name, 'name', 1, 100); const encoded = payload(input.payload, maxPayloadBytes);
          const result = transaction(() => {
            if (sql('SELECT 1 FROM budgets WHERE id=?').get(input.id)) fail(409, 'budget_exists', 'Budget ID is already in use');
            if (sql('SELECT count(*) AS n FROM budgets').get().n >= 16) fail(507, 'storage_limit', 'Household budget limit reached');
            sql('INSERT INTO budgets(id,name,revision) VALUES(?,?,1)').run(input.id, name);
            sql("INSERT INTO members(budget_id,user_id,role) VALUES(?,?,'owner')").run(input.id, user.id);
            sql("INSERT INTO snapshots(budget_id,revision,payload,author_id,summary,created_at) VALUES(?,1,?,?,'Created budget',?)").run(input.id, encoded, user.id, iso());
            return snapshot(membership(input.id, user.id));
          });
          return send(res, 201, result);
        }
      }
      const match = /^\/api\/budgets\/([A-Za-z0-9-]{1,128})(?:\/(commits|history|invites|members|passcode)(?:\/([A-Za-z0-9-]{1,128}))?)?$/.exec(path);
      if (!match) fail(404, 'not_found', 'Not found');
      const [, budgetId, route, item] = match;
      const budget = membership(budgetId, user.id);
      if (route === 'passcode' && !item) {
        // A member device unlocks with the passcode; only the owner chooses it.
        if (req.method === 'GET') {
          const row = sql('SELECT slot,updated_at AS updatedAt FROM passcodes WHERE budget_id=?').get(budgetId);
          if (!row) fail(404, 'not_found', 'This household has no passcode');
          return send(res, 200, { slot: JSON.parse(row.slot), updatedAt: row.updatedAt });
        }
        if (req.method === 'PUT') {
          membership(budgetId, user.id, true);
          const input = await readBody();
          const slot = passcodeSlot(input.slot);
          const encoded = JSON.stringify(slot);
          if (Buffer.byteLength(encoded) > 4096) fail(413, 'payload_too_large', 'Passcode record exceeds service limit');
          transaction(() => { membership(budgetId, user.id, true); sql('INSERT INTO passcodes(budget_id,slot,updated_at,updated_by) VALUES(?,?,?,?) ON CONFLICT(budget_id) DO UPDATE SET slot=excluded.slot,updated_at=excluded.updated_at,updated_by=excluded.updated_by').run(budgetId, encoded, iso(), user.id); });
          return send(res, 200, { ok: true, updatedAt: iso() });
        }
        if (req.method === 'DELETE') {
          membership(budgetId, user.id, true);
          await readBody();
          transaction(() => { membership(budgetId, user.id, true); sql('DELETE FROM passcodes WHERE budget_id=?').run(budgetId); });
          return send(res, 200, { ok: true });
        }
      }
      if (!route && req.method === 'GET') return send(res, 200, snapshot(budget));
      if (route === 'commits' && !item && req.method === 'POST') return send(res, 200, commit(user, budgetId, await readBody()));
      if (route === 'history' && req.method === 'GET') {
        if (item) return send(res, 200, snapshot(budget, revision(Number(item))));
        const history = sql('SELECT s.revision,s.author_id,u.username,s.summary,s.created_at,s.operation_id FROM snapshots s JOIN users u ON u.id=s.author_id WHERE budget_id=? ORDER BY revision DESC LIMIT ?').all(budgetId, historyLimit).map(row => ({ revision: row.revision, author: { id: row.author_id, username: row.username }, summary: row.summary, createdAt: row.created_at, operationId: row.operation_id }));
        return send(res, 200, { history });
      }
      if (route === 'members' && req.method === 'GET' && !item) {
        const members = sql('SELECT u.id,u.username,m.role FROM members m JOIN users u ON u.id=m.user_id WHERE m.budget_id=? ORDER BY m.role DESC,u.username').all(budgetId);
        return send(res, 200, { members });
      }
      if (route === 'members' && req.method === 'DELETE' && item) {
        membership(budgetId, user.id, true); await readBody();
        transaction(() => {
          membership(budgetId, user.id, true);
          const member = sql('SELECT role FROM members WHERE budget_id=? AND user_id=?').get(budgetId, item);
          if (member?.role === 'owner') fail(400, 'cannot_revoke_owner', 'Owner membership cannot be revoked');
          // Cancel earlier invitations to prevent a revoked recipient reusing an old outstanding code.
          const recipient = sql('SELECT username FROM users WHERE id=?').get(item)?.username;
          if (recipient) sql('UPDATE invites SET used=1 WHERE budget_id=? AND recipient=?').run(budgetId, recipient);
          sql('DELETE FROM members WHERE budget_id=? AND user_id=?').run(budgetId, item);
        });
        return send(res, 200, { ok: true });
      }
      if (route === 'invites') {
        membership(budgetId, user.id, true);
        if (req.method === 'POST' && !item) {
          throttle(invitationRate, user.id, 30, 3600000);
          const input = await readBody(); const recipient = username(input.recipient);
          const id = randomUUID(); const code = opaqueToken(); const expiry = now() + inviteTtlMs;
          transaction(() => {
            sql('DELETE FROM invites WHERE used=1 OR expires_at<=?').run(now());
            if (sql('SELECT count(*) AS n FROM invites WHERE budget_id=?').get(budgetId).n >= 200) fail(507, 'storage_limit', 'Outstanding invitation limit reached');
            sql('INSERT INTO invites(id,code_hash,budget_id,recipient,expires_at) VALUES(?,?,?,?,?)').run(id, hash(code), budgetId, recipient, expiry);
          });
          return send(res, 201, { invite: { id, recipient, expiresAt: new Date(expiry).toISOString() }, code });
        }
        if (req.method === 'DELETE' && item) {
          await readBody();
          sql('UPDATE invites SET used=1 WHERE id=? AND budget_id=?').run(item, budgetId);
          return send(res, 200, { ok: true });
        }
      }
      fail(404, 'not_found', 'Not found');
    } catch (error) {
      req.resume();
      if (res.headersSent || res.destroyed) { res.destroy(); return; }
      if (error instanceof HttpError) {
        if (error.retryAfter) res.setHeader('Retry-After', error.retryAfter);
        return send(res, error.status, { error: { code: error.code, message: error.message }, ...(error.current ? { current: error.current } : {}) });
      }
      // Never log request bodies, credential material, ciphertext or database exception details.
      send(res, 500, { error: { code: 'internal_error', message: 'The service could not complete this request' } });
    } finally { releaseAuthentication?.(); }
  });
  server.requestTimeout = 30000;
  server.headersTimeout = 15000;
  server.maxHeadersCount = 50;
  let closed = false;
  return { server, close: async () => {
    if (closed) return;
    closed = true;
    await new Promise((done, reject) => {
      const timeout = setTimeout(() => server.closeAllConnections(), 5000);
      timeout.unref();
      server.close(error => {
        clearTimeout(timeout);
        if (error && error.code !== 'ERR_SERVER_NOT_RUNNING') reject(error); else done();
      });
    });
    db.close();
  } };
}
