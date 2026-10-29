import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import test from 'node:test';
import {webcrypto} from 'node:crypto';
import {loadEngine} from '../../budget-mobile/public/engine.mjs';
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

test('password files interoperate between native and no-import WASM with trusted host entropy',async()=>{
  const snapshot=start(),password='long Unicode passphrase past the old fifty-six-byte boundary: café 🔐';
  const exported=native({action:'export',snapshot,today,password,entropy:{salt:Array(16).fill(0),nonce:Array(12).fill(0)}});
  assert.equal(exported.ok,true,exported.error);assert.ok(exported.result.text.startsWith('#db*sealed\n'));
  const envelope=JSON.parse(exported.result.text.split('\n')[1]);
  assert.notEqual(envelope.salt,Buffer.alloc(16).toString('base64'),'native callers cannot choose the salt');
  assert.notEqual(envelope.nonce,Buffer.alloc(12).toString('base64'),'native callers cannot choose the nonce');
  const importRequest=text=>({action:'import',text,password,id:'encrypted-copy',name:'Encrypted fixture',timeZone:'America/Halifax',today});
  assert.equal(portable(importRequest(exported.result.text)).ok,true);
  assert.equal(portable({action:'export',snapshot,today,password}).ok,false,'raw WASM must fail closed without fresh host entropy');
  const host={salt:[...webcrypto.getRandomValues(new Uint8Array(16))],nonce:[...webcrypto.getRandomValues(new Uint8Array(12))]};
  const raw=portable({action:'export',snapshot,today,password,entropy:host});assert.equal(raw.ok,true,raw.error);
  assert.equal(native(importRequest(raw.result.text)).ok,true);
  const wrapped=await loadEngine(`data:application/wasm;base64,${readFileSync(wasm).toString('base64')}`);
  const request={action:'export',snapshot,today,password,entropy:{salt:Array(16).fill(0),nonce:Array(12).fill(0)}};
  const a=wrapped(request),b=wrapped(request),ea=JSON.parse(a.text.split('\n')[1]),eb=JSON.parse(b.text.split('\n')[1]);
  assert.notEqual(ea.salt,eb.salt);assert.notEqual(ea.nonce,eb.nonce);assert.notEqual(ea.ciphertext,eb.ciphertext);
  assert.notEqual(ea.salt,Buffer.alloc(16).toString('base64'));assert.notEqual(ea.nonce,Buffer.alloc(12).toString('base64'));
  assert.equal(native(importRequest(a.text)).ok,true);
  assert.equal(portable({...importRequest(a.text),password:'wrong'}).ok,false);
  assert.deepEqual(request.entropy,{salt:Array(16).fill(0),nonce:Array(12).fill(0)},'wrapper must not mutate caller input');
});
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
