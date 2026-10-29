import test from 'node:test';
import assert from 'node:assert/strict';
import {SyncState} from '../public/state.mjs';
const base={schemaVersion:1,payments:[{id:'a',amount:10},{id:'b',amount:20}]};
const remote=(revision,snapshot)=>({revision,snapshot,updatedAt:'2026-10-29T12:00:00Z'});
function durable(){let data; return {save:async value=>{data=structuredClone(value)},load:()=>structuredClone(data)}}
test('local edit survives restart and preserves exact operation request after lost acknowledgement',async()=>{
 const disk=durable();const state=new SyncState(remote(1,base),disk.save);await state.edit({...base,payments:[{id:'a',amount:15},{id:'b',amount:20}]},{type:'edit',id:'a'},'Entry edited','operation-0001');
 const req=await state.prepare(async(s,r)=>({encrypted:s,revision:r}));const restored=new SyncState(disk.load(),disk.save);assert.deepEqual(await restored.prepare(()=>{throw Error('must reuse encrypted bytes')}),req);assert.equal(restored.value.working.payments[0].amount,15);assert.equal(req.operationId,'operation-0001');
 await restored.ack(remote(2,req.payload.encrypted));assert.equal(restored.value.pending.length,0);assert.equal(restored.value.revision,2);
});
test('storage failure cannot expose an edit as saved',async()=>{
 const state=new SyncState(remote(1,base),async()=>{throw Error('disk full')});await assert.rejects(state.edit({...base,payments:[]},{type:'delete'},'Deleted','operation-0002'),/disk full/);assert.deepEqual(state.value.working,base);assert.equal(state.value.pending.length,0);
});
test('remote poll and conflict preserve pending work, baseline and competing remote',async()=>{
 const disk=durable();const state=new SyncState(remote(1,base),disk.save);await state.edit({...base,payments:[]},{type:'delete'},'Deleted','operation-0003');await state.receive(remote(2,{...base,payments:[{id:'a',amount:99}]}));assert.equal(state.value.working.payments.length,0);assert.deepEqual(state.value.baseline,base);assert.equal(state.value.conflict.snapshot.payments[0].amount,99);
});
test('explicit reapply executes edits against remote to retain unrelated remote changes',async()=>{
 const disk=durable();const state=new SyncState(remote(1,base),disk.save);await state.edit({...base,payments:[{id:'a',amount:15},{id:'b',amount:20}]},{id:'a',amount:15},'Edited','operation-0004');await state.receive(remote(2,{...base,payments:[{id:'a',amount:10},{id:'b',amount:99}]}));await state.reapply(async(s,c)=>({...s,payments:s.payments.map(p=>p.id===c.id?{...p,amount:c.amount}:p)}),'operation-0005');assert.deepEqual(state.value.working.payments,[{id:'a',amount:15},{id:'b',amount:99}]);assert.equal(state.value.revision,2);assert.equal(state.value.pending[0].operationId,'operation-0005');
});
test('edits queued during a request are not lost by acknowledgement',async()=>{
 const disk=durable();const state=new SyncState(remote(1,base),disk.save);await state.edit({...base,payments:[]},{type:'delete'},'Deleted','operation-0006');await state.prepare(async s=>({encrypted:s}));await state.edit({...base,payments:[{id:'new',amount:3}]},{type:'add'},'Added','operation-0007');await state.ack(remote(2,{...base,payments:[]}));assert.equal(state.value.working.payments[0].id,'new');assert.equal(state.value.pending.length,1);assert.equal((await state.prepare(async s=>s)).expectedRevision,2);
});
test('domain mutation queued behind a remote refresh uses its newly durable working state',async()=>{
 const disk=durable();const state=new SyncState(remote(1,base),disk.save);const receive=state.receive(remote(2,{...base,payments:[{id:'a',amount:10},{id:'b',amount:99}]}));const edit=state.mutate(snapshot=>({...snapshot,payments:[...snapshot.payments,{id:'c',amount:4}]}),{type:'add'},'Added','operation-0008');await Promise.all([receive,edit]);assert.deepEqual(state.value.working.payments,[{id:'a',amount:10},{id:'b',amount:99},{id:'c',amount:4}]);
});
