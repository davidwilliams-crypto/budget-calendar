export async function openStorage(){
 const db=await new Promise((resolve,reject)=>{const r=indexedDB.open('budget-household-v1',1);r.onupgradeneeded=()=>r.result.createObjectStore('records');r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error)});
 function transact(mode,action){return new Promise((resolve,reject)=>{const tx=db.transaction('records',mode);let value;try{const request=action(tx.objectStore('records'));request.onsuccess=()=>{value=request.result}}catch(e){reject(e);return}tx.oncomplete=()=>resolve(value);tx.onabort=tx.onerror=()=>reject(tx.error||Error('Device storage could not save your change.'))})}
 return {get:key=>transact('readonly',s=>s.get(key)),put:(key,value)=>transact('readwrite',s=>s.put(value,key)),remove:key=>transact('readwrite',s=>s.delete(key))};
}
