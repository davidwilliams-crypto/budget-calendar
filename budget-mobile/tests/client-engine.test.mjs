import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadEngine,householdToday} from '../public/engine.mjs';
// Exercise the shipped ABI, not a JavaScript scheduling substitute.
test('WASM client calls Rust for month-boundary recurrence, paid status and stable offline replay',async()=>{
 const wasm=await readFile(new URL('../public/budget_engine.wasm',import.meta.url));
 const request=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
 const today=20261029;let snapshot=request({action:'new',id:'fixture-household',name:'Fixture household',timeZone:'America/Halifax',today});
 snapshot=request({action:'apply',snapshot,today,operationId:'fixture-add-001',command:{type:'put_payment',payment:{amount:-100,description:'Fixture monthly bill',start_date:20261031,end_date:20271031,frequency:1,freq_type:'m',flags:16}}});
 const id=snapshot.payments[0].id;assert.ok(id.includes('fixture-add-001'));
 const dates=request({action:'forecast',snapshot,today,from:20261029,to:20261201}).days.flatMap(d=>d.transactions.map(t=>d.date));assert.deepEqual(dates,[20261031,20261130]);
 snapshot=request({action:'apply',snapshot,today,operationId:'fixture-paid-001',command:{type:'transaction',id,date:20261031,amount:-100,note:'Paid fixture',flag:';',scope:'one'}});
 const tx=request({action:'forecast',snapshot,today,from:20261031,to:20261031}).days[0].transactions[0];assert.equal(tx.flag,';');assert.equal(tx.note,'Paid fixture');
 assert.throws(()=>request({action:'apply',snapshot,today,operationId:'fixture-invalid',command:{type:'delete',id:'missing'}}));assert.equal(snapshot.payments.length,1);
});
test('household today uses configured zone at midnight and ignores device zone',()=>{
 const instant=new Date('2026-11-01T02:30:00Z');assert.equal(householdToday('America/Halifax',instant),20261031);assert.equal(householdToday('Europe/London',instant),20261101);
});
