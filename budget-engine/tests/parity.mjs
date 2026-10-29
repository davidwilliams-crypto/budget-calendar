import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const executable=process.env.BUDGET_ENGINE_NATIVE||path.join(root,'target/debug/budget-engine-cli.exe');
const wasm=process.env.BUDGET_ENGINE_WASM||path.join(root,'target/wasm32-unknown-unknown/release/budget_engine.wasm');
const {instance}=await WebAssembly.instantiate(readFileSync(wasm),{});
const engine=instance.exports;
function native(request){
  const p=spawnSync(executable,[],{input:JSON.stringify(request)+'\n',encoding:'utf8',windowsHide:true});
  assert.equal(p.status,0,p.error?.message||p.stderr);return JSON.parse(p.stdout.trim());
}
function portable(request){
  const bytes=new TextEncoder().encode(JSON.stringify(request));const ptr=engine.alloc(bytes.length);assert.notEqual(ptr,0);
  new Uint8Array(engine.memory.buffer,ptr,bytes.length).set(bytes);engine.request(ptr,bytes.length);engine.dealloc(ptr,bytes.length);
  return JSON.parse(new TextDecoder().decode(new Uint8Array(engine.memory.buffer,engine.result_ptr(),engine.result_len())));
}
function same(request){const a=native(request);const b=portable(request);assert.deepEqual(b,a,JSON.stringify(request));return a;}
const today=20261029;
function start(){return same({action:'new',id:'parity-household',name:'Generated fixture',timeZone:'America/Halifax',today}).result;}
function apply(snapshot,command,operationId){const result=same({action:'apply',snapshot,today,command,operationId});assert.equal(result.ok,true,result.error);return result.result;}
test('native/raw WASM parity: calendar boundaries, checkpoint history, split, move, paid, export/import',()=>{
  let snapshot=start();
  snapshot=apply(snapshot,{type:'put_payment',payment:{description:'Month end',amount:-100,start_date:20260131,end_date:20260131,flags:16,frequency:1,freq_type:'m'}},'add-month-end');
  snapshot=apply(snapshot,{type:'put_payment',payment:{description:'Tentative',amount:-30,start_date:20261001,end_date:20261001,flags:18,frequency:1,freq_type:'m'}},'add-tentative');
  snapshot=apply(snapshot,{type:'balance',date:20260801,amount:2000},'opening');
  snapshot=apply(snapshot,{type:'day_note',date:20261029,note:'UTF-8: café ☕'},'day-note');
  const monthly=snapshot.payments[0].id;const tentative=snapshot.payments[1].id;
  snapshot=apply(snapshot,{type:'transaction',id:tentative,date:today,amount:-30,note:'Paid',flag:';',scope:'one'},'paid');
  snapshot=apply(snapshot,{type:'transaction',id:monthly,date:20261031,amount:-120,note:'Future',flag:'#',scope:'future'},'future-split');
  let prediction=same({action:'forecast',snapshot,today:20261030,from:20261029,to:20270228});assert.equal(prediction.ok,true,prediction.error);
  assert.equal(prediction.result.days[0].transactions.find(t=>t.id===tentative).flag,';');
  const split=snapshot.payments.find(p=>p.id!==monthly&&p.id!==tentative).id;
  snapshot=apply(snapshot,{type:'move',id:split,from:20261130,to:20261128,amount:-120},'move');
  snapshot=apply(snapshot,{type:'delete',id:split,date:20261231},'skip');
  snapshot=apply(snapshot,{type:'restore',id:split,date:20261231},'restore');
  prediction=same({action:'forecast',snapshot,today,from:20261029,to:20270228});assert.equal(prediction.ok,true,prediction.error);
  const exported=same({action:'export',snapshot,today});assert.equal(exported.ok,true,exported.error);
  const imported=same({action:'import',text:exported.result.text,id:'import-copy',name:'Import fixture',timeZone:'America/Halifax',today});assert.equal(imported.ok,true,imported.error);
  assert.equal(imported.result.payments.length,3);
});
test('native/raw WASM parity: leap days, weekends and loan payoff',()=>{
  let snapshot=start();
  for(const [i,payment] of [
    {description:'Leap',amount:500,start_date:20240229,end_date:20240229,flags:16,freq_type:'y'},
    {description:'Early weekend',amount:-40,start_date:20261101,end_date:20261101,flags:21,freq_type:'m'},
    {description:'Loan',amount:-100,start_date:20261001,end_date:20261001,flags:64,loan_amount:250,freq_type:'m'}
  ].entries()) snapshot=apply(snapshot,{type:'put_payment',payment},`schedule-${i}`);
  const result=same({action:'forecast',snapshot,today,from:20261001,to:20270301});assert.equal(result.ok,true,result.error);
  assert.equal(result.result.days.find(d=>d.date===20261201).transactions.find(t=>t.description==='Loan').amount,-50);
});
test('native/raw WASM parity: invalid commands, duplicate IDs, bad dates and oversized money',()=>{
  const snapshot=start();
  for(const command of [
    {type:'balance',date:20260230,amount:1},
    {type:'balance',date:20261029,amount:1e90},
    {type:'put_payment',payment:{description:'Bad',amount:-1,start_date:20261001,end_date:20261001,frequency:0}},
    {type:'put_payment',payment:{description:'Bad',amount:-1,start_date:20261001,end_date:20261001,flags:1024}},
  ])assert.equal(same({action:'apply',snapshot,today,operationId:'invalid-op',command}).ok,false);
  assert.equal(same({action:'forecast',snapshot,today:20260230,from:20261001,to:20261002}).ok,false);
  assert.equal(same({action:'validate',snapshot:{...snapshot,schemaVersion:99}}).ok,false);
});
test('independent offline additions retain distinct IDs after reviewed reapply to the current baseline',()=>{
  const baseline=start();
  const command=description=>({type:'put_payment',payment:{description,amount:-10,start_date:today,end_date:today}});
  const a=apply(baseline,command('Device A'),'offline-a-op');
  const b=apply(baseline,command('Device B'),'offline-b-op');
  assert.notEqual(a.payments[0].id,b.payments[0].id);
  // Whole-budget revision conflict is reviewed first; reapply B onto acknowledged A.
  const reviewed=apply(a,command('Device B'),'offline-b-op');
  assert.equal(reviewed.payments[0].id,a.payments[0].id);
  assert.equal(reviewed.payments[1].id,b.payments[0].id);
  assert.notEqual(reviewed.payments[0].payment.id,reviewed.payments[1].payment.id);
  assert.equal(same({action:'validate',snapshot:reviewed}).ok,true);
});
