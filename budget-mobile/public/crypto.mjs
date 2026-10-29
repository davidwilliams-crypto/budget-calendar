// Client-only encryption. The server receives neither recovery keys nor plaintext budgets.
const encoder=new TextEncoder();
const decoder=new TextDecoder('utf-8',{fatal:true});
const MAX_BYTES=8*1024*1024;
function encode(bytes){let raw='';for(let i=0;i<bytes.length;i+=8192)raw+=String.fromCharCode(...bytes.subarray(i,i+8192));return btoa(raw);}
function decode(text){if(typeof text!=='string'||text.length>MAX_BYTES*2||!/^[A-Za-z0-9+/]*={0,2}$/.test(text))throw new Error('Invalid encrypted budget.');return Uint8Array.from(atob(text),c=>c.charCodeAt(0));}
function associatedData(id,revision){if(typeof id!=='string'||!id||id.length>200||!Number.isSafeInteger(revision)||revision<1)throw new Error('Invalid budget identity or revision.');return encoder.encode(`BudgetCalendar:1:${id}:${revision}`);}
export function generateRecoveryKey(){const bytes=crypto.getRandomValues(new Uint8Array(32));return 'BC1-'+encode(bytes).replaceAll('+','-').replaceAll('/','_').replaceAll('=','');}
export async function importRecoveryKey(text){
  const trimmed=typeof text==='string'?text.trim():'';
  if(!/^BC1-[A-Za-z0-9_-]{43}$/.test(trimmed))throw new Error('Enter the complete recovery key beginning BC1-.');
  const bytes=decode(trimmed.slice(4).replaceAll('-','+').replaceAll('_','/')+'=');
  if(bytes.length!==32)throw new Error('Invalid recovery key.');
  return crypto.subtle.importKey('raw',bytes,{name:'AES-GCM'},false,['encrypt','decrypt']);
}
export function assertSyncSnapshot(snapshot,budgetId){
  if(!snapshot||typeof snapshot!=='object'||Array.isArray(snapshot)||snapshot.id!==budgetId)throw new Error('Budget identity mismatch. Reload the household before retrying.');
  const bytes=encoder.encode(JSON.stringify(snapshot));
  if(bytes.length>6*1024*1024-1024)throw new Error('This edit exceeds the shared budget size limit. Reduce its size before saving; the previous budget remains available.');
  return bytes;
}
export async function sealSnapshot(snapshot,key,budgetId,revision){
  const additionalData=associatedData(budgetId,revision);
  const bytes=assertSyncSnapshot(snapshot,budgetId);
  const iv=crypto.getRandomValues(new Uint8Array(12));
  const encrypted=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData,tagLength:128},key,bytes);
  return {version:1,algorithm:'AES-256-GCM',iv:encode(iv),data:encode(new Uint8Array(encrypted))};
}
export async function openSnapshot(payload,key,budgetId,revision){
  const additionalData=associatedData(budgetId,revision);
  if(!payload||payload.version!==1||payload.algorithm!=='AES-256-GCM')throw new Error('Unsupported encrypted budget format.');
  const iv=decode(payload.iv),data=decode(payload.data);
  if(iv.length!==12||data.length<16||data.length>MAX_BYTES)throw new Error('Invalid encrypted budget.');
  try{
    const bytes=await crypto.subtle.decrypt({name:'AES-GCM',iv,additionalData,tagLength:128},key,data);
    const snapshot=JSON.parse(decoder.decode(bytes));
    if(!snapshot||typeof snapshot!=='object'||Array.isArray(snapshot)||snapshot.id!==budgetId)throw new Error('Budget identity mismatch.');
    return snapshot;
  }catch{throw new Error('Unable to unlock this budget. Check the recovery key; the budget may also be damaged or belong to another revision.');}
}
