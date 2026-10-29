// One tab owns the device cache for its entire document lifetime. Acquiring this
// origin-scoped lock must precede IndexedDB opening, session caching or syncing.
// The browser releases it automatically when the owning document is destroyed.
export function claimWriterLock(locks=globalThis.navigator?.locks){
 if(!locks?.request)throw Error('Web Locks are unavailable. Use an up-to-date browser with Web Locks support to protect pending changes.');
 return new Promise((resolve,reject)=>{
  locks.request('budget-calendar-device-writer-v1',{mode:'exclusive',ifAvailable:true},async lock=>{
   if(!lock){resolve(null);return}
   let release;const lifetime=new Promise(done=>{release=done});resolve({release});await lifetime;
  }).catch(reject);
 });
}
