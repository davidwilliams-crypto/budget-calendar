import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { createHook } from 'node:async_hooks';

let createBudgetServer;
try { ({ createBudgetServer } = await import('../server/server.mjs')); } catch (error) {
  if (error.code !== 'ERR_MODULE_NOT_FOUND') throw error;
}
const setupToken = 'fixture-only-setup-token-123456789';
const password = 'fixture password 123456';
const ownerName = 'owner@example.invalid';
const payload = { ciphertext: 'generated-fixture', iv: 'fixture' };

async function fixture(t, extra = {}) {
  assert.equal(typeof createBudgetServer, 'function', 'private HTTP service must be implemented');
  const root = resolve('target/shared-server-tests');
  await mkdir(root, { recursive: true });
  const dir = await mkdtemp(join(root, 'service-'));
  const publicDir = join(dir, 'public');
  await mkdir(publicDir);
  await writeFile(join(publicDir, 'index.html'), '<!doctype html><title>Generated fixture</title>');
  await writeFile(join(publicDir, 'secret.sqlite'), 'not public');
  await writeFile(join(dir, 'outside.txt'), 'not public');
  let clock = Date.parse('2026-10-29T12:00:00Z');
  let app;
  let base;
  async function start() {
    app = createBudgetServer({ dataDir: join(dir, 'data'), publicDir, setupToken,
      publicOrigin: 'http://localhost:8787', development: true, now: () => clock, ...extra });
    await new Promise(resolveListen => app.server.listen(0, '127.0.0.1', resolveListen));
    base = `http://127.0.0.1:${app.server.address().port}`;
  }
  await start();
  t.after(async () => { await app.close(); });
  const request = async (method, path, body, token, headers = {}) => {
    const encoded = body === undefined ? undefined : JSON.stringify(body);
    const response = await fetch(base + path, { method, headers: {
      'X-Budget-Client': 'native', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(token ? { Authorization: `Bearer ${token}` } : {}), ...headers,
    }, body: encoded, signal: AbortSignal.timeout(15000) });
    const text = await response.text();
    let data; try { data = JSON.parse(text); } catch { data = text; }
    return { status: response.status, data, headers: response.headers };
  };
  const setup = async () => {
    const r = await request('POST', '/api/setup', { setupToken, username: ownerName, password });
    assert.equal(r.status, 201);
    assert.equal(typeof r.data.token, 'string');
    return r.data;
  };
  const create = async token => {
    const id = randomUUID();
    const r = await request('POST', '/api/budgets', { id, name: 'Fixture household', payload }, token);
    assert.equal(r.status, 201);
    assert.equal(r.data.id, id);
    assert.equal(r.data.revision, 1);
    return r.data;
  };
  const joinUser = async (owner, budget, username = 'editor@example.invalid') => {
    const invite = await request('POST', `/api/budgets/${budget.id}/invites`, { recipient: username }, owner.token);
    assert.equal(invite.status, 201);
    const joined = await request('POST', '/api/invites/accept', { code: invite.data.code, username, password });
    assert.equal(joined.status, 200);
    return joined.data;
  };
  return { request, setup, create, joinUser, dir, advance: ms => { clock += ms; },
    restart: async () => { await app.close(); await start(); },
    browser: async (method, path, body, cookie, origin = 'http://localhost:8787', extraHeaders = {}) => {
      const response = await fetch(base + path, { method, headers: {
        ...(origin ? { Origin: origin } : {}), ...(cookie ? { Cookie: cookie } : {}),
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...extraHeaders,
      }, body: body === undefined ? undefined : JSON.stringify(body) });
      return { status: response.status, data: await response.json(), headers: response.headers };
    }, raw: path => fetch(base + path) };
}

test('setup is secret gated, one-time, and credentials remain distinct from bearer sessions', async t => {
  const f = await fixture(t);
  assert.equal((await f.request('POST', '/api/setup', { setupToken: 'wrong', username: ownerName, password })).status, 403);
  const owner = await f.setup();
  assert.equal((await f.request('POST', '/api/setup', { setupToken, username: 'intruder', password })).status, 409);
  assert.equal((await f.request('POST', '/api/login', { username: ownerName, password: 'wrong password' })).status, 401);
  assert.equal((await f.request('GET', '/api/me')).status, 401);
  assert.equal((await f.request('GET', '/api/me', undefined, owner.token)).data.user.id, owner.user.id);
  const login = await f.request('POST', '/api/login', { username: ownerName.toUpperCase(), password });
  assert.equal(login.status, 200);
  await f.request('POST', '/api/logout', {}, owner.token);
  assert.equal((await f.request('GET', '/api/me', undefined, owner.token)).status, 401);
  assert.equal((await f.request('GET', '/api/me', undefined, login.data.token)).status, 200);
  const database = await readFile(join(f.dir, 'data', 'budget.sqlite'));
  assert.equal(database.includes(Buffer.from(password)), false);
  assert.equal(database.includes(Buffer.from(login.data.token)), false);
});

test('cookie authentication never returns bearer tokens and browser mutations require exact origin', async t => {
  const f = await fixture(t);
  const owner = await f.setup();
  const login = await f.browser('POST', '/api/login', { username: ownerName, password });
  assert.equal(login.status, 200);
  assert.equal(login.data.token, undefined);
  const cookie = login.headers.get('set-cookie');
  assert.match(cookie, /HttpOnly/);
  assert.match(cookie, /SameSite=Strict/);
  assert.equal((await f.browser('GET', '/api/me', undefined, cookie)).status, 200);
  for (const origin of [undefined, 'https://attacker.invalid']) {
    assert.equal((await f.browser('POST', '/api/budgets', { id: randomUUID(), name: 'Fixture', payload }, cookie, origin === undefined ? null : origin)).status, 403);
  }
  assert.equal((await f.browser('POST', '/api/logout', {}, cookie, 'http://localhost:8787', { 'X-Budget-Client': 'native' })).status, 403);
  assert.equal((await f.request('GET', '/api/me', undefined, owner.token, { Origin: 'https://attacker.invalid' })).status, 403);
  assert.equal((await f.request('GET', '/api/me', undefined, owner.token, { Origin: '' })).status, 403);
});

test('atomic revision commits preserve both sides of conflict and retry receipts after later commits and restart', async t => {
  const f = await fixture(t);
  const owner = await f.setup();
  const budget = await f.create(owner.token);
  const editor = await f.joinUser(owner, budget);
  const first = { operationId: 'fixture-operation-1', expectedRevision: 1, payload: { ciphertext: 'first' }, summary: 'Updated budget' };
  const concurrent = { operationId: 'fixture-operation-2', expectedRevision: 1, payload: { ciphertext: 'second' }, summary: 'Updated budget' };
  const results = await Promise.all([f.request('POST', `/api/budgets/${budget.id}/commits`, first, owner.token),
    f.request('POST', `/api/budgets/${budget.id}/commits`, concurrent, editor.token)]);
  assert.deepEqual(results.map(r => r.status).sort(), [200, 409]);
  const acceptedIndex = results.findIndex(r => r.status === 200);
  const accepted = results[acceptedIndex].data;
  const conflict = results.find(r => r.status === 409).data;
  assert.equal(conflict.error.code, 'revision_conflict');
  assert.deepEqual(conflict.current.payload, accepted.payload);
  const body = acceptedIndex === 0 ? first : concurrent;
  const token = acceptedIndex === 0 ? owner.token : editor.token;
  const later = await f.request('POST', `/api/budgets/${budget.id}/commits`, {
    operationId: 'fixture-operation-later', expectedRevision: 2, payload: { ciphertext: 'later' }, summary: 'Updated budget',
  }, owner.token);
  assert.equal(later.data.revision, 3);
  await f.restart();
  const retry = await f.request('POST', `/api/budgets/${budget.id}/commits`, body, token);
  assert.equal(retry.status, 200);
  assert.equal(retry.data.revision, 2);
  assert.deepEqual(retry.data.payload, accepted.payload);
  const reused = await f.request('POST', `/api/budgets/${budget.id}/commits`, { ...body, payload: { ciphertext: 'changed' } }, token);
  assert.equal(reused.status, 409);
  assert.equal(reused.data.error.code, 'operation_reused');
  assert.equal((await f.request('GET', `/api/budgets/${budget.id}`, undefined, owner.token)).data.revision, 3);
});

test('revocation blocks all private routes including already accepted queued retries', async t => {
  const f = await fixture(t);
  const owner = await f.setup(); const budget = await f.create(owner.token);
  const editor = await f.joinUser(owner, budget);
  const body = { operationId: 'fixture-editor-operation', expectedRevision: 1, payload, summary: 'Updated budget' };
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, body, editor.token)).status, 200);
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'other-user' }, editor.token)).status, 403);
  assert.equal((await f.request('DELETE', `/api/budgets/${budget.id}/members/${owner.user.id}`, {}, owner.token)).status, 400);
  assert.equal((await f.request('DELETE', `/api/budgets/${budget.id}/members/${editor.user.id}`, {}, owner.token)).status, 200);
  for (const path of ['', '/history', '/history/1', '/members']) {
    assert.equal((await f.request('GET', `/api/budgets/${budget.id}${path}`, undefined, editor.token)).status, 404);
  }
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, body, editor.token)).status, 404);
  assert.deepEqual((await f.request('GET', '/api/budgets', undefined, editor.token)).data.budgets, []);
});

test('recipient-bound invitations expire, cancel, and can be accepted only once', async t => {
  const f = await fixture(t, { inviteTtlMs: 1000 });
  const owner = await f.setup(); const budget = await f.create(owner.token);
  const invite = await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'invited-user' }, owner.token);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: invite.data.code, username: 'wrong-user', password })).status, 403);
  f.advance(1001);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: invite.data.code, username: 'invited-user', password })).status, 410);
  const cancelled = await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'invited-user' }, owner.token);
  assert.equal((await f.request('DELETE', `/api/budgets/${budget.id}/invites/${cancelled.data.invite.id}`, {}, owner.token)).status, 200);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: cancelled.data.code, username: 'invited-user', password })).status, 410);
  const editor = await f.joinUser(owner, budget, 'invited-user');
  const again = await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'invited-user' }, owner.token);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: again.data.code }, editor.token)).status, 200);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: again.data.code }, editor.token)).status, 410);
});

test('retained history restores as a new revision and rejects stale clients', async t => {
  const f = await fixture(t); const owner = await f.setup(); const budget = await f.create(owner.token);
  for (let revision = 1; revision < 55; revision++) {
    const r = await f.request('POST', `/api/budgets/${budget.id}/commits`, {
      operationId: `fixture-history-${revision}`, expectedRevision: revision, payload: { ciphertext: `fixture-${revision}` }, summary: 'Updated budget',
    }, owner.token);
    assert.equal(r.status, 200);
  }
  const history = await f.request('GET', `/api/budgets/${budget.id}/history`, undefined, owner.token);
  assert.equal(history.data.history.length, 50);
  assert.equal(history.data.history[0].revision, 55);
  assert.equal((await f.request('GET', `/api/budgets/${budget.id}/history/1`, undefined, owner.token)).status, 404);
  // A service cannot copy ciphertext authenticated to an old revision into a new revision.
  const old = await f.request('GET', `/api/budgets/${budget.id}/history/10`, undefined, owner.token);
  assert.deepEqual(old.data.payload, { ciphertext: 'fixture-9' });
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/restore`, {
    operationId: 'fixture-unsafe-restore', expectedRevision: 55, revision: 10,
  }, owner.token)).status, 404);
  const restore = { operationId: 'fixture-restore-1', expectedRevision: 55,
    payload: { ciphertext: 'fixture-9-resealed-at-56' }, summary: 'Restored budget' };
  const restored = await f.request('POST', `/api/budgets/${budget.id}/commits`, restore, owner.token);
  assert.equal(restored.status, 200); assert.equal(restored.data.revision, 56);
  assert.deepEqual(restored.data.payload, { ciphertext: 'fixture-9-resealed-at-56' });
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, restore, owner.token)).data.revision, 56);
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, {
    operationId: 'fixture-stale-after-restore', expectedRevision: 55, payload, summary: 'Updated budget',
  }, owner.token)).status, 409);
});

test('static delivery refuses hidden files, database files and traversal while applying restrictive headers', async t => {
  const f = await fixture(t);
  const index = await f.raw('/'); assert.equal(index.status, 200);
  assert.match(index.headers.get('content-security-policy'), /default-src 'self'/);
  assert.equal(index.headers.get('x-content-type-options'), 'nosniff');
  assert.equal((await f.raw('/secret.sqlite')).status, 404);
  assert.equal((await f.raw('/.env')).status, 404);
  assert.equal((await f.raw('/%2e%2e%2foutside.txt')).status, 404);
  assert.equal((await f.request('GET', '/health')).data.ok, true);
});

test('production refuses plaintext origins and credentials are throttled', async t => {
  assert.equal(typeof createBudgetServer, 'function');
  assert.throws(() => createBudgetServer({ dataDir: 'target/shared-server-tests/invalid', setupToken,
    publicOrigin: 'http://example.invalid' }), /HTTPS/);
  const f = await fixture(t); await f.setup();
  for (let i = 0; i < 10; i++) {
    assert.equal((await f.request('POST', '/api/login', { username: ownerName, password: 'incorrect-password' })).status, 401);
  }
  const limited = await f.request('POST', '/api/login', { username: ownerName, password });
  assert.equal(limited.status, 429);
  assert.ok(Number(limited.headers.get('retry-after')) > 0);
});

test('request bounds reject oversize and malformed writes without changing the acknowledged budget', async t => {
  const f = await fixture(t, { maxPayloadBytes: 1024 }); const owner = await f.setup(); const budget = await f.create(owner.token);
  const invalid = [
    { operationId: 'tiny', expectedRevision: 1, payload },
    { operationId: 'fixture-bad-revision', expectedRevision: 0, payload },
    { operationId: 'fixture-bad-payload', expectedRevision: 1, payload: 'plaintext' },
    { operationId: 'fixture-bad-summary', expectedRevision: 1, payload, summary: 'x'.repeat(201) },
  ];
  for (const input of invalid) assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, input, owner.token)).status, 400);
  const oversized = await f.request('POST', `/api/budgets/${budget.id}/commits`, {
    operationId: 'fixture-oversize-payload', expectedRevision: 1, payload: { ciphertext: 'x'.repeat(1024) }, summary: 'Updated budget',
  }, owner.token);
  assert.equal(oversized.status, 413);
  const oversizedBody = await f.request('POST', `/api/budgets/${budget.id}/commits`, {
    operationId: 'fixture-oversize-request', expectedRevision: 1, payload: { ciphertext: 'x'.repeat(70 * 1024) }, summary: 'Updated budget',
  }, owner.token);
  assert.equal(oversizedBody.status, 413);
  const current = await f.request('GET', `/api/budgets/${budget.id}`, undefined, owner.token);
  assert.equal(current.data.revision, 1); assert.deepEqual(current.data.payload, payload);
});

test('uninvited signed-in accounts cannot access another household and expired sessions stop working', async t => {
  const f = await fixture(t, { sessionTtlMs: 10000 });
  const owner = await f.setup(); const first = await f.create(owner.token); const second = await f.create(owner.token);
  const editor = await f.joinUser(owner, first);
  assert.equal((await f.request('GET', `/api/budgets/${second.id}`, undefined, editor.token)).status, 404);
  assert.equal((await f.request('POST', `/api/budgets/${second.id}/commits`, {
    operationId: 'fixture-uninvited-write', expectedRevision: 1, payload,
  }, editor.token)).status, 404);
  // A native token cannot be smuggled into a browser request or vice versa.
  assert.equal((await f.browser('GET', '/api/me', undefined, `budget_session=${owner.token}`)).status, 401);
  const login = await f.browser('POST', '/api/login', { username: ownerName, password });
  const cookieToken = /^budget_session=([^;]+)/.exec(login.headers.get('set-cookie'))[1];
  assert.equal((await f.request('GET', '/api/me', undefined, cookieToken)).status, 401);
  f.advance(10001);
  assert.equal((await f.request('GET', '/api/me', undefined, owner.token)).status, 401);
  assert.equal((await f.browser('GET', '/api/me', undefined, login.headers.get('set-cookie'))).status, 401);
});

test('revocation cancels invitations issued before removal and retry IDs remain author-bound', async t => {
  const f = await fixture(t); const owner = await f.setup(); const budget = await f.create(owner.token);
  const editor = await f.joinUser(owner, budget);
  const body = { operationId: 'fixture-author-bound-id', expectedRevision: 1, payload, summary: 'Updated budget' };
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, body, owner.token)).status, 200);
  const spoof = await f.request('POST', `/api/budgets/${budget.id}/commits`, body, editor.token);
  assert.equal(spoof.status, 409); assert.equal(spoof.data.error.code, 'operation_reused');
  const invite = await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: editor.user.username }, owner.token);
  await f.request('DELETE', `/api/budgets/${budget.id}/members/${editor.user.id}`, {}, owner.token);
  assert.equal((await f.request('POST', '/api/invites/accept', { code: invite.data.code }, editor.token)).status, 410);
  assert.equal((await f.request('GET', `/api/budgets/${budget.id}`, undefined, editor.token)).status, 404);
});

test('receipt quota rejects new writes while preserving old idempotent acknowledgements', async t => {
  const f = await fixture(t); const owner = await f.setup(); const budget = await f.create(owner.token);
  const body = { operationId: 'fixture-last-safe-operation', expectedRevision: 1, payload, summary: 'Updated budget' };
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/commits`, body, owner.token)).status, 200);
  // Seed only generated receipt metadata: exercise capacity without thousands of HTTP calls or huge payloads.
  const fixtureDb = new DatabaseSync(join(f.dir, 'data', 'budget.sqlite'));
  try {
    fixtureDb.prepare(`WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<9999)
      INSERT INTO operations(budget_id,operation_id,author_id,request_hash,result)
      SELECT ?, 'fixture-quota-'||x, ?, 'fixture-hash', '{}' FROM n`).run(budget.id, owner.user.id);
  } finally { fixtureDb.close(); }
  const exceeded = await f.request('POST', `/api/budgets/${budget.id}/commits`, {
    operationId: 'fixture-over-capacity', expectedRevision: 2, payload: { ciphertext: 'never-accepted' }, summary: 'Updated budget',
  }, owner.token);
  assert.equal(exceeded.status, 507); assert.equal(exceeded.data.error.code, 'storage_limit');
  const retry = await f.request('POST', `/api/budgets/${budget.id}/commits`, body, owner.token);
  assert.equal(retry.status, 200); assert.equal(retry.data.revision, 2);
  assert.equal((await f.request('GET', `/api/budgets/${budget.id}`, undefined, owner.token)).data.revision, 2);
});

test('outstanding invitation capacity is bounded and expired rows stop consuming it', async t => {
  const f = await fixture(t); const owner = await f.setup(); const budget = await f.create(owner.token);
  const fixtureDb = new DatabaseSync(join(f.dir, 'data', 'budget.sqlite'));
  try {
    fixtureDb.prepare(`WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<200)
      INSERT INTO invites(id,code_hash,budget_id,recipient,expires_at)
      SELECT 'fixture-invite-'||x, 'fixture-code-hash-'||x, ?, 'future-recipient', ? FROM n`).run(budget.id, Date.parse('2026-10-30T12:00:00Z'));
  } finally { fixtureDb.close(); }
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'next-recipient' }, owner.token)).status, 507);
  f.advance(86400001);
  assert.equal((await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient: 'next-recipient' }, owner.token)).status, 201);
});

test('invitation recheck rejects removal, cancellation and expiry during real asynchronous password hashing', async t => {
  for (const outcome of ['pruned', 'cancelled', 'expired']) {
    const f = await fixture(t, { inviteTtlMs: 1000 });
    const owner = await f.setup(); const budget = await f.create(owner.token);
    const recipient = `fixture-race-${outcome}`;
    const issued = await f.request('POST', `/api/budgets/${budget.id}/invites`, { recipient }, owner.token);
    assert.equal(issued.status, 201);
    const fixtureDb = new DatabaseSync(join(f.dir, 'data', 'budget.sqlite'));
    let changed = false;
    let armed = true;
    // Observe the real scrypt async resource rather than replacing the KDF or adding a production test hook.
    // Its initialization occurs after the first invite check. The microtask deterministically changes the
    // generated SQLite row before the asynchronous KDF completion can run its transactional recheck.
    const hook = createHook({ init(_id, type) {
      if (!armed || type !== 'SCRYPTREQUEST') return;
      armed = false;
      queueMicrotask(() => {
        if (outcome !== 'cancelled') f.advance(1001);
        if (outcome === 'pruned') fixtureDb.prepare('DELETE FROM invites WHERE id=?').run(issued.data.invite.id);
        if (outcome === 'cancelled') fixtureDb.prepare('UPDATE invites SET used=1 WHERE id=?').run(issued.data.invite.id);
        changed = true;
      });
    } });
    let result;
    try {
      hook.enable();
      result = await f.request('POST', '/api/invites/accept', { code: issued.data.code, username: recipient, password });
    } finally { hook.disable(); fixtureDb.close(); }
    assert.equal(changed, true, `${outcome}: fixture must change after the initial invite check`);
    assert.equal(result.status, 410, `${outcome}: unavailable invitation must return a stable client error`);
    assert.equal(result.data.error.code, 'invite_unavailable');
    assert.equal(result.data.token, undefined);
    const members = await f.request('GET', `/api/budgets/${budget.id}/members`, undefined, owner.token);
    assert.equal(members.data.members.length, 1);
  }
});
