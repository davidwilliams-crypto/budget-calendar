import {dateInput,dateNumber,offsetDate} from './engine.mjs';
// Disabling controls preserves their values while excluding them from FormData.
// A single date accepts only amount, note and status; series settings must remain
// unavailable until the user deliberately chooses a series scope, and a date's
// status stays out of series edits.
export function lastWeekdayLabel(value){return `Last ${new Intl.DateTimeFormat('en-CA',{weekday:'long',timeZone:'UTC'}).format(new Date(`${value}T12:00:00Z`))} of the month`}
export function bindEditorScope(form){
 const scope=form.elements.namedItem('scope'),date=form.elements.namedItem('date'),period=form.elements.namedItem('freq_type'),every=form.elements.namedItem('frequency'),flag=form.elements.namedItem('flag');
 const seriesFields=['description','category','date','repeat','frequency','freq_type','end_mode','end_date','n_pays','loan_amount','grace_period_days','disabled','bit-1','bit-2','bit-4','bit-8','bit-256'];
 const update=()=>{const occurrence=scope?.value==='one';for(const name of seriesFields){const field=form.elements.namedItem(name);if(field)field.disabled=occurrence}if(flag)flag.disabled=!occurrence;const label=lastWeekdayLabel(date.value&&date.value!==date.defaultValue?date.value:form.dataset.start);period.querySelector('option[value=L]').textContent=label;const ignored=period.value==='W';every.closest('label').hidden=ignored;if(ignored)every.disabled=true;const repeat=form.querySelector('[data-repeat-hint]');if(repeat)repeat.textContent=period.value==='L'?`The first date moves to the last ${label.split(' ')[1]} of its month.`:ignored?'Keeps the same week of the month, such as the second Tuesday.':'';const hint=form.querySelector('[data-scope-hint]');if(hint)hint.textContent=occurrence?'Only amount, note and paid status change, for this date alone. Choose another option to change description, category, date or how it repeats.':'Series settings can be changed. Paid status belongs to one date, so it stays as it is. Later dates continue as a new entry; whole-series changes apply to every date.'};
 for(const field of [scope,period,date])field?.addEventListener('change',update);update();
}
// An untouched amount box means the series amount stays, whatever this one date shows. Typing in it, even the same figure, sets it.
export function seriesAmount(entered,shown,original,typed=false){if(original===undefined||typed||Math.abs(entered)!==Math.abs(shown))return entered;return Math.sign(entered)===Math.sign(shown)?original:-original}
export function paymentCommand(data,{id,original={},shown,date,typed}){
 const record=id!=null,entered=Number(data.get('amount'))*(data.get('kind')==='expense'?-1:1),when=dateNumber(data.get('date')||dateInput(date)),scope=data.get('scope'),note=data.get('note').trim(),once=data.get('repeat')==='once',mode=once?48:Number(data.get('end_mode')),period=data.get('freq_type'),typedEvery=data.get('frequency');
 let flags=(original.flags||0)&~(240|1|2|4|8|256);for(const bit of [1,2,4,8,256])if(data.has(`bit-${bit}`))flags|=bit;flags|=mode;
 const payment={...original,amount:seriesAmount(entered,shown,original.amount,typed),description:data.get('description').trim(),start_date:record&&when===date?original.start_date:when,frequency:typedEvery===null?(original.freq_type===period?original.frequency:1):Number(typedEvery),freq_type:period,flags,n_pays:once?1:Number(data.get('n_pays')),end_date:dateNumber(data.get('end_date')),category:data.get('category').trim()||null,loan_amount:Number(data.get('loan_amount')),grace_period_days:Number(data.get('grace_period_days')),disabled:data.has('disabled'),flag_value:original.flag_value||'^'};
 if(note)payment.note=[...(original.note||[]).filter(n=>n.date!==when),{date:when,note}];else if(record)payment.note=(original.note||[]).filter(n=>n.date!==when);
 const command={type:'put_payment',payment};if(record){command.id=id;if(scope==='future')command.from=date}return command;
}
// One forecast reaches ten years, so an older series is read in spans.
export function paymentDates(snapshot,ids,from,to,engine,today){const dates=[];for(let start=from;start<=to;start=offsetDate(start,3651))dates.push(...engine({action:'forecast',snapshot,today,from:start,to:Math.min(offsetDate(start,3650),to)}).days.flatMap(d=>d.transactions.filter(t=>ids.includes(t.id)).map(()=>d.date)));return dates}
// Plain text for a real date move, from the dates before and after a trial save; null when no date moves.
export function dateMovePreview({scope,before,after,when,date,label}){
 const changed=before.some(d=>!after.includes(d))||after.some(d=>!before.includes(d));if(!changed)return null;
 const pivot=Math.min(date,when),next=list=>list.filter(d=>d>=pivot).slice(0,3).map(d=>label(d)).join(', ')||'none',moved=`Next dates become ${next(after)} (were ${next(before)}).`;
 if(scope==='future')return `${moved} Earlier dates stay as they are.`;
 const earlier=before.filter(d=>d<pivot&&!after.includes(d)),span=earlier.length>1?`${label(earlier[0])} to ${label(earlier.at(-1))}`:earlier.length?label(earlier[0]):'';
 return `${moved} ${earlier.length>1?`${earlier.length} earlier payments (${span}) come off the schedule, and past months will change.`:earlier.length?`1 earlier payment (${span}) comes off the schedule, and past months will change.`:'This changes every date, past and future.'}`;
}
// Null means the figure is unchanged, so nothing is saved. Blank never removes a balance.
export function balanceCommand(date,text,existing){const typed=String(text??'').trim();if(!typed)throw Error('Enter a balance. To take one off, use Remove this balance.');const amount=Number(typed);if(!Number.isFinite(amount))throw Error('Enter a valid balance.');return amount===existing?null:{type:'balance',date,amount}}
