export async function loadEngine(url='./budget_engine.wasm'){
 const response=await fetch(url);if(!response.ok)throw Error('The calculation engine is unavailable. Reconnect and reload.');
 const {instance}=await WebAssembly.instantiate(await response.arrayBuffer(),{});const e=instance.exports;const encoder=new TextEncoder(),decoder=new TextDecoder();
 return request=>{const bytes=encoder.encode(JSON.stringify(request));if(bytes.length>8*1024*1024)throw Error('Budget exceeds the engine limit.');const ptr=e.alloc(bytes.length);let result;try{new Uint8Array(e.memory.buffer,ptr,bytes.length).set(bytes);e.request(ptr,bytes.length);result=JSON.parse(decoder.decode(new Uint8Array(e.memory.buffer,e.result_ptr(),e.result_len())))}finally{e.dealloc(ptr,bytes.length)}if(!result.ok)throw Error(result.error||'The engine rejected this change.');return result.result};
}
export function householdToday(timeZone,now=new Date()){
 const parts=new Intl.DateTimeFormat('en-CA',{timeZone,year:'numeric',month:'2-digit',day:'2-digit'}).formatToParts(now);const get=k=>parts.find(p=>p.type===k).value;return Number(get('year')+get('month')+get('day'));
}
export function dateInput(value){const s=String(value);return `${s.slice(0,4)}-${s.slice(4,6)}-${s.slice(6,8)}`}
export function dateNumber(value){return Number(value.replaceAll('-',''))}
export function offsetDate(value,days){const d=new Date(`${dateInput(value)}T12:00:00Z`);d.setUTCDate(d.getUTCDate()+days);return Number(d.toISOString().slice(0,10).replaceAll('-',''))}
