import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadEngine} from '../public/engine.mjs';
import {SyncState,undoStatus,undoMessages} from '../public/state.mjs';
// Generated fixtures against the shipped engine: an undo is a real edit, so it runs the real commands.
const wasm=await readFile(new URL('../public/budget_engine.wasm',import.meta.url));
const request=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
const today=20261030;let operation=0;
const apply=(snapshot,entry)=>request({action:'apply',snapshot,today,operationId:entry.operationId,command:entry.command});
const direct=(snapshot,command)=>apply(snapshot,{command,operationId:`undo-fixture-${++operation}`});
const blank=()=>request({action:'new',id:'undo-fixture',name:'Fixture',timeZone:'America/Halifax',today});
const rent={amount:-1500,description:'Rent',start_date:20260115,end_date:20260115,frequency:1,freq_type:'m',flags:16};
const gym={amount:-40,description:'Gym',start_date:20260103,end_date:20260103,frequency:1,freq_type:'m',flags:16};
const remote=(revision,snapshot)=>({revision,snapshot,updatedAt:'2026-10-30T12:00:00Z'});
function device(snapshot,saved=[]){return new SyncState(remote(1,snapshot),async value=>{saved.push(structuredClone(value))})}
async function edit(state,command,summary='Entry edited'){const operationId=`undo-fixture-${++operation}`,entry={command,operationId};await state.mutate(s=>apply(s,entry),entry,summary,operationId,{undoable:true});return operationId}
const undo=(state,id)=>state.undo(id,apply,`undo-fixture-${++operation}`);
const dated=(snapshot,from=20261001,to=20261231)=>request({action:'forecast',snapshot,today,from,to}).days.flatMap(d=>d.transactions.map(t=>[d.date,t.description,t.amount,t.flag,t.note]));
const seeded=()=>direct(direct(blank(),{type:'put_payment',payment:rent}),{type:'put_payment',payment:gym});
const idOf=(snapshot,description)=>snapshot.payments.find(p=>p.payment.description===description).id;
const plain=text=>assert.doesNotMatch(text,/tombstone|revision|occurrence|replay|Error|undefined/i);

const edits={
 'a one-date change':s=>({type:'transaction',id:idOf(s,'Rent'),date:20261115,amount:-1600,note:'Late',flag:';',scope:'one'}),
 'a whole-series change':s=>({type:'put_payment',id:idOf(s,'Rent'),payment:{...s.payments.find(p=>p.id===idOf(s,'Rent')).payment,amount:-1700}}),
 'a change from a date onward':s=>({type:'put_payment',id:idOf(s,'Rent'),from:20261115,payment:{...s.payments.find(p=>p.id===idOf(s,'Rent')).payment,amount:-1700}}),
 'a new entry':()=>({type:'put_payment',payment:{...gym,description:'Dentist',amount:-90}}),
 'a one-date delete':s=>({type:'delete',id:idOf(s,'Rent'),date:20261115}),
 'a moved date':s=>({type:'move',id:idOf(s,'Rent'),from:20261115,to:20261118,amount:-1500}),
 'a new balance':()=>({type:'balance',date:20261030,amount:1500.25}),
 'a new day note':()=>({type:'day_note',date:20261030,note:'Call the bank'})
};
for(const [name,command] of Object.entries(edits))test(`undo puts back ${name} as a new pending edit`,async()=>{
 const start=seeded(),state=device(start),before=dated(start),id=await edit(state,command(start));assert.notDeepEqual(dated(state.value.working).concat(state.value.working.adjustments.opt??[],state.value.working.adjustments.note??[]),before.concat(start.adjustments.opt??[],start.adjustments.note??[]));
 assert.equal(undoStatus(state.value,id),'ready');const result=await undo(state,id);assert.equal(result.ok,true,result.message);
 assert.deepEqual(dated(state.value.working),before);assert.deepEqual(state.value.working.adjustments,start.adjustments);
 assert.equal(state.value.pending.length,2,'the original edit stays in the queue and the undo follows it');assert.equal(state.value.revision,1,'nothing about the shared history is touched');assert.deepEqual(state.value.baseline,start);
 assert.match(state.value.pending[1].summary,/^Undo: /);assert.equal(undoStatus(state.value,id),'undone');
});
test('a whole-series delete comes back under a new identity, so the deleted one is not revived',async()=>{
 const start=seeded(),state=device(start),old=idOf(start,'Rent'),before=dated(start),id=await edit(state,{type:'delete',id:old},'Entry deleted');assert.equal((await undo(state,id)).ok,true);
 assert.deepEqual(dated(state.value.working),before);assert.ok(state.value.working.tombstones.includes(old));assert.notEqual(idOf(state.value.working,'Rent'),old);
});
test('a one-date status and note survive an undo of a later series edit',async()=>{
 const seed=seeded(),start=direct(seed,{type:'transaction',id:idOf(seed,'Rent'),date:20261115,amount:-1725.5,note:'Paid late',flag:';',scope:'one'}),state=device(start),before=dated(start),record=start.payments.find(p=>p.payment.description==='Rent');
 const id=await edit(state,{type:'put_payment',id:record.id,payment:{...record.payment,category:'Housing'}});assert.equal((await undo(state,id)).ok,true);assert.deepEqual(dated(state.value.working),before);assert.deepEqual(state.value.working.payments,start.payments);
});
test('schedules of every kind come back exactly',async()=>{
 const kinds={'a last-weekday series':{...rent,description:'Last',freq_type:'L',start_date:20261127,end_date:20261127},'a same-weekday series':{...rent,description:'Second',freq_type:'W',start_date:20261110,end_date:20261110},'a weekend-shifted series':{...rent,description:'Shifted',flags:17,start_date:20260101,end_date:20260101},'a fixed count':{...rent,description:'Count',flags:48,n_pays:6,end_date:20260615},'an end date':{...rent,description:'Ending',flags:32,end_date:20270115},'a tentative estimate':{...rent,description:'Estimate',flags:18},'a loan':{...rent,description:'Loan',flags:64,loan_amount:9000},'a switched-off series':{...rent,description:'Off',disabled:true}};
 for(const [name,payment] of Object.entries(kinds)){const start=direct(blank(),{type:'put_payment',payment}),record=start.payments[0],state=device(start),changed=await edit(state,{type:'put_payment',id:record.id,payment:{...record.payment,category:'Moved',amount:record.payment.amount-1}});const result=await undo(state,changed);assert.equal(result.ok,true,`${name}: ${result.message}`);assert.deepEqual(state.value.working.payments,start.payments,name);
  const gone=device(start),deleted=await edit(gone,{type:'delete',id:record.id});assert.equal((await undo(gone,deleted)).ok,true,name);assert.deepEqual({...gone.value.working.payments[0].payment,id:0},{...record.payment,id:0},name)}
});
test('a day note and a balance on the same date are undone separately',async()=>{
 const start=seeded(),state=device(start),note=await edit(state,{type:'day_note',date:20261030,note:'Kept'}),balance=await edit(state,{type:'balance',date:20261030,amount:900});
 assert.equal((await undo(state,balance)).ok,true);assert.deepEqual(state.value.working.adjustments.note.map(n=>n.note),['Kept']);assert.equal(undoStatus(state.value,note),'ready');
});
test('undo re-writes a changed balance and day note to their earlier values',async()=>{
 const start=direct(direct(seeded(),{type:'balance',date:20261030,amount:100}),{type:'day_note',date:20261030,note:'Old'}),state=device(start),a=await edit(state,{type:'balance',date:20261030,amount:200}),b=await edit(state,{type:'day_note',date:20261030,note:null});
 await undo(state,b);await undo(state,a);assert.deepEqual(state.value.working.adjustments,start.adjustments);
});
test('a partner\'s newer change to the entry blocks the undo and nothing is written',async()=>{
 const start=seeded(),state=device(start),rentId=idOf(start,'Rent'),id=await edit(state,{type:'transaction',id:rentId,date:20261115,amount:-1600,note:'',flag:'^',scope:'one'});
 const mine=state.value.working;await state.prepare(async s=>s);await state.ack(remote(2,mine));const partner=direct(mine,{type:'transaction',id:rentId,date:20261115,amount:-1999,note:'Partner',flag:'^',scope:'one'});await state.receive(remote(3,partner));
 assert.equal(undoStatus(state.value,id),'changed');const result=await undo(state,id);assert.equal(result.ok,false);assert.equal(result.message,undoMessages.changed);plain(result.message);assert.deepEqual(state.value.working,partner);assert.equal(state.value.pending.length,0);
});
test('a partner\'s delete of the entry blocks the undo',async()=>{
 const start=seeded(),state=device(start),rentId=idOf(start,'Rent'),id=await edit(state,{type:'transaction',id:rentId,date:20261115,amount:-1600,note:'',flag:'^',scope:'one'}),mine=state.value.working;await state.prepare(async s=>s);await state.ack(remote(2,mine));await state.receive(remote(3,direct(mine,{type:'delete',id:rentId})));
 assert.equal((await undo(state,id)).ok,false);assert.equal(state.value.working.payments.some(p=>p.id===rentId),false);
});
test('a partner\'s edit to another entry leaves the undo available and is kept',async()=>{
 const start=seeded(),state=device(start),id=await edit(state,{type:'transaction',id:idOf(start,'Rent'),date:20261115,amount:-1600,note:'',flag:'^',scope:'one'}),mine=state.value.working;await state.prepare(async s=>s);await state.ack(remote(2,mine));const gymId=idOf(start,'Gym'),record=mine.payments.find(p=>p.id===gymId),partner=direct(mine,{type:'put_payment',id:gymId,payment:{...record.payment,amount:-55}});await state.receive(remote(3,partner));
 assert.equal((await undo(state,id)).ok,true);assert.equal(state.value.working.payments.find(p=>p.id===gymId).payment.amount,-55);assert.deepEqual(state.value.working.payments.find(p=>p.id===idOf(start,'Rent')).payment,start.payments.find(p=>p.id===idOf(start,'Rent')).payment);assert.equal(state.value.revision,3);
});
test('a newer edit of your own to the entry has to be undone first',async()=>{
 const start=seeded(),state=device(start),rentId=idOf(start,'Rent'),first=await edit(state,{type:'transaction',id:rentId,date:20261115,amount:-1600,note:'',flag:'^',scope:'one'}),second=await edit(state,{type:'transaction',id:rentId,date:20261215,amount:-1650,note:'',flag:'^',scope:'one'});
 assert.equal(undoStatus(state.value,first),'newer');const blocked=await undo(state,first);assert.equal(blocked.message,undoMessages.newer);plain(blocked.message);
 assert.equal((await undo(state,second)).ok,true);assert.equal(undoStatus(state.value,first),'ready');assert.equal((await undo(state,first)).ok,true);assert.deepEqual(state.value.working.payments,start.payments);
});
test('undo cannot run twice, cannot run during a conflict, and only the last few edits are kept',async()=>{
 const start=seeded(),state=device(start);let ids=[];for(let n=0;n<7;n++)ids.push(await edit(state,{type:'day_note',date:20261101+n,note:`Note ${n}`}));
 assert.equal(state.value.undo.length,5);assert.equal(undoStatus(state.value,ids[0]),'gone');assert.equal((await undo(state,ids[0])).message,undoMessages.gone);
 assert.equal((await undo(state,ids[6])).ok,true);const again=await undo(state,ids[6]);assert.equal(again.ok,false);assert.equal(again.message,undoMessages.undone);
 await state.receive(remote(2,seeded()));const pendingCount=state.value.pending.length,blocked=await undo(state,ids[5]);assert.equal(blocked.ok,false);assert.equal(blocked.message,undoMessages.conflict);assert.equal(state.value.pending.length,pendingCount);
 for(const text of Object.values(undoMessages))plain(text);
});
test('undo works offline, and the list and the undo survive a restart',async()=>{
 const saved=[],start=seeded(),state=device(start,saved),id=await edit(state,{type:'day_note',date:20261030,note:'Offline'}),restarted=new SyncState(saved.at(-1),async value=>{saved.push(structuredClone(value))});
 assert.equal(undoStatus(restarted.value,id),'ready');assert.equal((await undo(restarted,id)).ok,true);const again=new SyncState(saved.at(-1),async()=>{});assert.equal(undoStatus(again.value,id),'undone');assert.equal(again.value.pending.length,2);
});
test('a device saved before undo existed opens with an empty list',()=>{const {undo:_,...older}=device(seeded()).value;assert.deepEqual(new SyncState(older,async()=>{}).value.undo,[])});
test('a storage failure leaves the entry undoable',async()=>{
 const start=seeded();let broken=false;const state=new SyncState(remote(1,start),async()=>{if(broken)throw Error('disk full')}),id=await edit(state,{type:'day_note',date:20261030,note:'Kept'});broken=true;await assert.rejects(undo(state,id),/disk full/);assert.equal(state.value.pending.length,1);assert.equal(undoStatus(state.value,id),'ready');
});
test('an undo the engine refuses says so plainly and changes nothing',async()=>{
 const start=seeded(),state=device(start),id=await edit(state,{type:'day_note',date:20261030,note:'Kept'}),result=await state.undo(id,()=>{throw Error('panic: raw detail')},'undo-fixture-refused');assert.equal(result.ok,false);assert.equal(result.message,undoMessages.failed);assert.doesNotMatch(result.message,/panic/);assert.equal(state.value.pending.length,1);assert.equal(undoStatus(state.value,id),'ready');
});
test('an edit that changes nothing offers no undo, and only marked saves are listed',async()=>{
 const start=seeded(),state=device(start),same=await edit(state,{type:'day_note',date:20261030,note:null});assert.equal(undoStatus(state.value,same),'gone');await state.edit(start,{replaceSnapshot:start},'Restore');assert.equal(state.value.undo.length,0);
});
test('reviewing a conflict keeps an undo only while the entry still matches',async()=>{
 const start=seeded(),rentId=idOf(start,'Rent'),state=device(start),id=await edit(state,{type:'transaction',id:rentId,date:20261115,amount:-1600,note:'',flag:'^',scope:'one'});await undo(state,id);const record=start.payments.find(p=>p.id===idOf(start,'Gym'));
 await state.receive(remote(2,direct(start,{type:'put_payment',id:record.id,payment:{...record.payment,amount:-55}})));assert.deepEqual((await state.reapply(apply,'undo-fixture-review')).skipped,[]);
 assert.equal(state.value.working.payments.find(p=>p.id===rentId).payment.opt?.length??0,0,'the edit and its undo both landed');assert.equal(state.value.working.payments.find(p=>p.id===record.id).payment.amount,-55);
});
test('reviewing a conflict never lets an undo overwrite a partner\'s change to that entry',async()=>{
 const start=seeded(),rentId=idOf(start,'Rent'),state=device(start),id=await edit(state,{type:'transaction',id:rentId,date:20261115,amount:-1600,note:'',flag:'^',scope:'one'}),mine=state.value.working;await state.prepare(async s=>s);await state.ack(remote(2,mine));
 assert.equal((await undo(state,id)).ok,true);const partner=direct(mine,{type:'transaction',id:rentId,date:20261215,amount:-1999,note:'Partner',flag:'^',scope:'one'});await state.receive(remote(3,partner));assert.ok(state.value.conflict);
 const {skipped}=await state.reapply(apply,'undo-fixture-review');assert.deepEqual(skipped,['Undo: Entry edited']);assert.deepEqual(state.value.working,partner);assert.equal(state.value.pending.length,0,'nothing is left to send');assert.equal(undoStatus(state.value,id),'changed');
});
test('throwing away pending edits takes their undo along, and frees an undo that was thrown away',async()=>{
 const start=seeded(),state=device(start),kept=await edit(state,{type:'day_note',date:20261101,note:'Kept'}),mine=state.value.working;await state.prepare(async s=>s);await state.ack(remote(2,mine));const dropped=await edit(state,{type:'day_note',date:20261102,note:'Dropped'});await undo(state,kept);assert.equal(undoStatus(state.value,kept),'undone');
 await state.receive(remote(3,{...mine,notes:'Partner'}));await state.discard();assert.equal(undoStatus(state.value,dropped),'gone');assert.equal(undoStatus(state.value,kept),'ready');
});
