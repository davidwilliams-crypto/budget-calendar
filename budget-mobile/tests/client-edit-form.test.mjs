import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadEngine,dateInput} from '../public/engine.mjs';
import {paymentCommand,seriesAmount,dateMovePreview,paymentDates,balanceCommand,lastWeekdayLabel} from '../public/editor-scope.mjs';
import {upcomingEntries} from '../public/state.mjs';
// Generated fixtures against the shipped engine: these are the phone form's money rules.
const wasm=await readFile(new URL('../public/budget_engine.wasm',import.meta.url));
const request=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
const today=20261030;let operation=0;
const apply=(snapshot,command)=>request({action:'apply',snapshot,today,operationId:`form-fixture-${++operation}`,command});
const setup=payment=>apply(request({action:'new',id:'form-fixture',name:'Fixture',timeZone:'America/Halifax',today}),{type:'put_payment',payment});
const transactions=(snapshot,from,to)=>request({action:'forecast',snapshot,today,from,to}).days.flatMap(d=>d.transactions.map(t=>({date:d.date,...t})));
const dates=(snapshot,from,to)=>transactions(snapshot,from,to).map(t=>t.date);
// What the browser submits for the form as loaded for this entry on this date, plus the user's changes.
function submitted(record,shown,date,scope,changes={}){const p=record?.payment||{},data=new FormData(),amount=shown??p.amount??0,flags=p.flags??16,once=(flags&240)===48&&p.n_pays===1,values={kind:amount<=0?'expense':'income',amount:String(Math.abs(amount)),description:p.description??'',date:dateInput(date),category:p.category??'',note:'',repeat:once?'once':'repeat',frequency:String(p.frequency??1),freq_type:p.freq_type??'m',end_mode:String(flags&240),end_date:dateInput(p.end_date??date),n_pays:String(p.n_pays??1),loan_amount:String(p.loan_amount??0),grace_period_days:String(p.grace_period_days??0),...changes};if(scope)values.scope=scope;for(const [name,value] of Object.entries(values))if(value!==null)data.set(name,value);for(const bit of [1,2,4,8,256])if(flags&bit)data.set(`bit-${bit}`,'on');return data}
const edit=(snapshot,shown,date,scope,changes)=>{const record=snapshot.payments[0];return paymentCommand(submitted(record,shown,date,scope,changes),{id:record.id,original:record.payment,shown,date})};
const rent={amount:-1500,description:'Rent',start_date:20260101,end_date:20260101,frequency:1,freq_type:'m',flags:17};
const monthsBetween=(fromYear,fromMonth,toYear,toMonth)=>(toYear-fromYear)*12+toMonth-fromMonth+1;
const mortgage={amount:-2000,description:'Mortgage',start_date:20260131,end_date:20260131,frequency:1,freq_type:'m',flags:16};

test('a metadata edit from a date shifted off a weekend keeps the schedule',()=>{
 const snapshot=setup(rent);const before=dates(snapshot,20261001,20270331);assert.ok(before.includes(20261102),'Nov 1 2026 is a Sunday, so rent shows on Monday the 2nd');
 const command=edit(snapshot,-1500,20261102,'future',{category:'Housing'});assert.equal(command.payment.start_date,20260101);assert.equal(command.from,20261102);
 assert.deepEqual(dates(apply(snapshot,command),20261001,20270331),before);
});
test('a metadata edit from a short month keeps the month end',()=>{
 const snapshot=setup(mortgage);const before=dates(snapshot,20270101,20270630);assert.deepEqual(before,[20270131,20270228,20270331,20270430,20270531,20270630]);
 assert.deepEqual(dates(apply(snapshot,edit(snapshot,-2000,20270228,'future',{category:'Housing'})),20270101,20270630),before);
});
test('a whole-series edit keeps the start unless the date box changed, and a real change is sent',()=>{
 const snapshot=setup({...rent,flags:16,start_date:20260115,end_date:20260115});
 assert.equal(edit(snapshot,-1500,20261115,'all',{category:'Housing'}).payment.start_date,20260115);
 assert.equal(edit(snapshot,-1500,20261115,'all',{date:'2026-11-17'}).payment.start_date,20261117);
 const future=edit(snapshot,-1500,20261115,'future',{date:'2026-11-17'});assert.equal(future.payment.start_date,20261117);assert.equal(future.from,20261115);
 assert.deepEqual(dates(apply(snapshot,future),20261001,20270131),[20261015,20261117,20261217,20270117]);
});
test('a series edit never turns one date\'s status into the series default',()=>{
 let snapshot=setup({...rent,flags:16,start_date:20260115,end_date:20260115});const id=snapshot.payments[0].id;
 snapshot=apply(snapshot,{type:'transaction',id,date:20261115,amount:-1500,note:'',flag:';',scope:'one'});
 for(const scope of ['all','future']){const command=edit(snapshot,-1500,20261115,scope,{description:'Rent fixed',flag:';'});assert.equal(command.payment.flag_value,'^');const paid=transactions(apply(snapshot,command),20260101,20261231).filter(t=>t.flag===';').map(t=>t.date);assert.deepEqual(paid,[20261115],scope)}
 const legacy=setup({...rent,flag_value:'#'});assert.equal(edit(legacy,-1500,20261102,'all',{category:'Housing'}).payment.flag_value,'#');
});
test('a series edit keeps the series amount unless the amount was changed',()=>{
 let snapshot=setup({...rent,flags:16,start_date:20260115,end_date:20260115});const id=snapshot.payments[0].id;
 snapshot=apply(snapshot,{type:'transaction',id,date:20261115,amount:-1725.5,note:'',flag:'^',scope:'one'});
 for(const scope of ['all','future']){
  assert.equal(edit(snapshot,-1725.5,20261115,scope,{category:'Housing'}).payment.amount,-1500,scope);
  assert.equal(edit(snapshot,-1725.5,20261115,scope,{amount:'1600'}).payment.amount,-1600,scope);
  assert.equal(edit(snapshot,-1725.5,20261115,scope,{kind:'income'}).payment.amount,1500,scope);
 }
 assert.deepEqual([seriesAmount(-40,undefined,undefined),seriesAmount(-50,-50,-40),seriesAmount(50,-50,-40),seriesAmount(-60,-50,-40)],[-40,-40,40,-60]);
 const all=transactions(apply(snapshot,edit(snapshot,-1725.5,20261115,'all',{category:'Housing'})),20261001,20261231);assert.deepEqual(all.map(t=>t.amount),[-1500,-1725.5,-1500]);
});
test('typing the amount a date already shows makes it the series amount',()=>{
 let snapshot=setup({...rent,flags:16,start_date:20260115,end_date:20260115});const record=snapshot.payments[0];
 snapshot=apply(snapshot,{type:'transaction',id:record.id,date:20261115,amount:-1725.5,note:'',flag:'^',scope:'one'});
 const typed=scope=>paymentCommand(submitted(snapshot.payments[0],-1725.5,20261115,scope),{id:record.id,original:snapshot.payments[0].payment,shown:-1725.5,date:20261115,typed:true});
 assert.equal(typed('all').payment.amount,-1725.5);assert.equal(typed('future').payment.amount,-1725.5);
 assert.equal(seriesAmount(-50,-50,-40,true),-50);
});
test('the date preview counts a series older than one forecast can reach',()=>{
 const snapshot=setup({...rent,flags:16,start_date:20120101,end_date:20120101}),record=snapshot.payments[0];
 const from=20120101,to=20280101,seen=paymentDates(snapshot,[record.id],from,to,request,today);
 assert.equal(seen.length,monthsBetween(2012,1,2028,1));assert.equal(seen[0],20120101);assert.equal(seen.at(-1),20280101);
});
test('a new entry uses the typed values, with no series status to carry',()=>{
 const data=submitted(null,null,20261103,undefined,{amount:'12.34',description:'New bill',repeat:'once'});const command=paymentCommand(data,{date:20261103});
 assert.deepEqual([command.id,command.from,command.payment.amount,command.payment.start_date,command.payment.flag_value,command.payment.n_pays],[undefined,undefined,-12.34,20261103,'^',1]);
 const income=paymentCommand(submitted(null,null,20261103,undefined,{amount:'5',kind:'income',description:'Paid'}),{date:20261103});assert.equal(income.payment.amount,5);
});
test('the same weekday repeat keeps a stored interval and defaults a new one, since the engine ignores it',()=>{
 const stored=setup({...rent,flags:16,freq_type:'W',frequency:3,start_date:20261110,end_date:20261110});
 assert.equal(edit(stored,-1500,20261110,'all',{frequency:null,category:'Housing'}).payment.frequency,3);
 const changed=setup({...rent,flags:16,frequency:2,start_date:20261110,end_date:20261110});
 assert.equal(edit(changed,-1500,20261110,'all',{frequency:null,freq_type:'W'}).payment.frequency,1);
});
test('labels say what the engine does',()=>{
 assert.equal(lastWeekdayLabel('2026-11-06'),'Last Friday of the month');assert.equal(lastWeekdayLabel('2026-11-03'),'Last Tuesday of the month');
 const snapshot=setup({...rent,flags:16,freq_type:'L',start_date:20261127,end_date:20261127});assert.deepEqual(dates(snapshot,20261101,20270131),[20261127,20261225,20270129]);
});
test('a date move is described in plain words and warns about earlier payments',()=>{
 const label=n=>({20260115:'Jan 15',20261015:'Oct 15',20261115:'Nov 15',20261215:'Dec 15',20261117:'Nov 17',20261217:'Dec 17'}[n]??String(n));
 assert.equal(dateMovePreview({scope:'all',before:[20261115],after:[20261115],when:20261117,date:20261115,label}),null);
 const all=dateMovePreview({scope:'all',before:[20260115,20261015,20261115,20261215],after:[20261117,20261217],when:20261117,date:20261115,label});
 assert.match(all,/Nov 17, Dec 17/);assert.match(all,/were Nov 15, Dec 15/);assert.match(all,/2 earlier payments \(Jan 15 to Oct 15\)/);
 const one=dateMovePreview({scope:'all',before:[20261015,20261115],after:[20261117],when:20261117,date:20261115,label});assert.match(one,/1 earlier payment \(Oct 15\) comes off/);
 const future=dateMovePreview({scope:'future',before:[20260115,20261115,20261215],after:[20260115,20261117,20261217],when:20261117,date:20261115,label});
 assert.match(future,/Nov 17, Dec 17/);assert.match(future,/Earlier dates stay as they are/);assert.doesNotMatch(future,/come off/);
 for(const text of [all,one,future])assert.doesNotMatch(text,/occurrence|revision|tombstone|replay/i);
});
test('a moved whole series shows what really changes, before anything is saved',()=>{
 const snapshot=setup({...rent,flags:16,start_date:20260115,end_date:20260115});const command=edit(snapshot,-1500,20261115,'all',{date:'2026-11-17'}),before=dates(snapshot,20260101,20270131),after=dates(apply(snapshot,command),20260101,20270131);
 assert.equal(before.length,13);assert.deepEqual(after,[20261117,20261217,20270117]);
 assert.match(dateMovePreview({scope:'all',before,after,when:20261117,date:20261115,label:String}),/10 earlier payments \(20260115 to 20261015\)/);
 let calls=[];dateMovePreview({scope:'all',before,after,when:20261117,date:20261115,label:(...args)=>{calls.push(args.length);return 'x'}});assert.ok(calls.length&&calls.every(n=>n===1),'a date label takes the date alone');
});
test('a one-time entry moves to the typed date',()=>{
 const snapshot=setup({amount:-20,description:'Dentist',start_date:20261110,end_date:20261110,n_pays:1,frequency:1,freq_type:'m',flags:48});
 assert.deepEqual(dates(apply(snapshot,edit(snapshot,-20,20261110,'all',{date:'2026-11-12'})),20261101,20261231),[20261112]);
});
test('saving the balance box unchanged changes nothing, and only Remove takes a balance off',()=>{
 let snapshot=setup({...rent,flags:16});const day=()=>request({action:'forecast',snapshot,today,from:20261030,to:20261030}).days[0];
 assert.equal(day().adjusted,false);snapshot=apply(snapshot,balanceCommand(20261030,'1500.25',null));
 const existing=day().adjusted?day().closingBalance:null;assert.equal(existing,1500.25);
 assert.equal(balanceCommand(20261030,String(existing),existing),null);assert.equal(balanceCommand(20261030,' 1500.25 ',existing),null);
 assert.throws(()=>balanceCommand(20261030,'',existing),/Enter a balance/);assert.throws(()=>balanceCommand(20261030,'  ',null),/Enter a balance/);
 assert.deepEqual(balanceCommand(20261030,'1600',existing),{type:'balance',date:20261030,amount:1600});
 assert.deepEqual(balanceCommand(20261030,'0',existing),{type:'balance',date:20261030,amount:0});
 snapshot=apply(snapshot,{type:'balance',date:20261030,amount:null});assert.equal(day().adjusted,false);
});
test('tiles and Coming up leave out disabled payments and agree with the balance',()=>{
 let snapshot=setup({amount:-100,description:'Kept',start_date:20261101,end_date:20261101,frequency:1,freq_type:'m',flags:16});
 snapshot=apply(snapshot,{type:'put_payment',payment:{amount:-900,description:'Switched off',start_date:20261102,end_date:20261102,frequency:1,freq_type:'m',flags:16,disabled:true}});
 snapshot=apply(snapshot,{type:'put_payment',payment:{amount:250,description:'Pay',start_date:20261103,end_date:20261103,frequency:1,freq_type:'m',flags:16}});
 const data=request({action:'forecast',snapshot,today,from:20261030,to:20261129}),entries=upcomingEntries(data);
 assert.deepEqual(entries.map(({t,date})=>[date,t.description]),[[20261101,'Kept'],[20261103,'Pay']]);assert.deepEqual(data.totals,{moneyIn:250,moneyOut:100});
 assert.equal(data.days.at(-1).closingBalance-data.days[0].openingBalance,data.totals.moneyIn-data.totals.moneyOut);
});
