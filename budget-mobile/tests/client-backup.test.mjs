import test from 'node:test';
import assert from 'node:assert/strict';
import {generateRecoveryKey,importRecoveryKey,openSnapshot} from '../public/crypto.mjs';
import {makeBackup,readBackup} from '../public/backup.mjs';
test('encrypted backup restores the local snapshot and retains a recoverable snapshot including pending edits',async()=>{
 const key=await importRecoveryKey(generateRecoveryKey());const snapshot={id:'backup-fixture',schemaVersion:1,payments:[{id:'entry-new',payment:{amount:-12}}]};const localState={revision:4,baseline:{...snapshot,payments:[]},working:snapshot,pending:[{operationId:'pending-001',snapshot,commands:[{operationId:'pending-001',command:{type:'put_payment'}}]}],conflict:null};
 const backup=await makeBackup(localState,key,'backup-fixture','2026-10-29T12:00:00Z');const restored=await readBackup(backup,key,'backup-fixture');assert.deepEqual(restored.snapshot,snapshot);assert.equal(restored.sync.pendingCount,1);assert.equal(backup.revision,4);assert.ok(!JSON.stringify(backup).includes('entry-new'));
 await assert.rejects(readBackup({...backup,budgetId:'other'},key,'backup-fixture'),/household/);await assert.rejects(openSnapshot(backup.payload,key,'backup-fixture',5),/unlock/);
});

test('200 pending snapshots produce one compact recoverable encrypted working snapshot',async()=>{
 const key=await importRecoveryKey(generateRecoveryKey()),snapshot={id:'pending-large',schemaVersion:1,payments:[],notes:'x'.repeat(32768)};const localState={revision:7,baseline:snapshot,working:snapshot,pending:Array.from({length:200},(_,i)=>({operationId:`pending-${i}`,snapshot,commands:[{operationId:`pending-${i}`,command:{type:'notes',notes:snapshot.notes}}]})),conflict:null};
 const backup=await makeBackup(localState,key,snapshot.id),restored=await readBackup(backup,key,snapshot.id);assert.deepEqual(restored.snapshot,snapshot);assert.equal(restored.sync.pendingCount,200);assert.ok(JSON.stringify(backup).length<60000,'backup must not duplicate the pending snapshots');assert.equal(restored.localState,undefined);
});
test('acknowledged 2 MiB budget exports without triplicating its snapshot',async()=>{
 const key=await importRecoveryKey(generateRecoveryKey()),snapshot={id:'ack-large',schemaVersion:1,payments:[],notes:'x'.repeat(2*1024*1024)};const backup=await makeBackup({revision:9,baseline:snapshot,working:snapshot,pending:[],conflict:null},key,snapshot.id),restored=await readBackup(backup,key,snapshot.id);assert.deepEqual(restored.snapshot,snapshot);assert.equal(restored.sync.pendingCount,0);assert.ok(JSON.stringify(backup).length<3*1024*1024);
});
