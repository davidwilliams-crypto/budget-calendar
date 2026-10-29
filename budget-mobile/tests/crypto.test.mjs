import test from 'node:test';
import assert from 'node:assert/strict';
import {generateRecoveryKey,importRecoveryKey,sealSnapshot,openSnapshot,assertSyncSnapshot} from '../public/crypto.mjs';

test('only the matching recovery key, budget and revision can open a snapshot',async()=>{
  const recovery=generateRecoveryKey();
  const key=await importRecoveryKey(recovery);
  assert.equal(key.extractable,false);
  const snapshot={schemaVersion:1,id:'fixture-budget',notes:'private fixture note'};
  const payload=await sealSnapshot(snapshot,key,'fixture-budget',7);
  assert.deepEqual(await openSnapshot(payload,key,'fixture-budget',7),snapshot);
  assert.ok(!JSON.stringify(payload).includes('private fixture'));
  await assert.rejects(openSnapshot(payload,key,'different-budget',7));
  await assert.rejects(openSnapshot(payload,key,'fixture-budget',8));
  await assert.rejects(openSnapshot(payload,await importRecoveryKey(generateRecoveryKey()),'fixture-budget',7));
});
test('tampering fails and repeated encryption uses fresh nonces',async()=>{
  const key=await importRecoveryKey(generateRecoveryKey());
  const a=await sealSnapshot({id:'fixture'},key,'fixture',1);
  const b=await sealSnapshot({id:'fixture'},key,'fixture',1);
  assert.notEqual(a.iv,b.iv);
  const changed={...a,data:(a.data[0]==='A'?'B':'A')+a.data.slice(1)};
  await assert.rejects(openSnapshot(changed,key,'fixture',1));
  await assert.rejects(openSnapshot({...a,version:2},key,'fixture',1));
});
test('bad recovery input and invalid revision fail before encryption',async()=>{
  await assert.rejects(importRecoveryKey('not a recovery key'));
  const key=await importRecoveryKey(generateRecoveryKey());
  await assert.rejects(sealSnapshot({},key,'fixture',0));
});

test('a snapshot can never be encrypted for a different household',async()=>{
  const key=await importRecoveryKey(generateRecoveryKey());
  await assert.rejects(sealSnapshot({id:'household-B'},key,'household-A',4),/identity/);
  await assert.rejects(sealSnapshot({},key,'household-A',4),/identity/);
});

test('preflight rejects oversized pending data before a client records it as saved',()=>{
  assert.throws(()=>assertSyncSnapshot({id:'fixture',notes:'x'.repeat(6*1024*1024)},'fixture'),/size limit/);
  const bytes=assertSyncSnapshot({id:'fixture',notes:'fits'},'fixture');
  assert.deepEqual(JSON.parse(new TextDecoder().decode(bytes)),{id:'fixture',notes:'fits'});
});
