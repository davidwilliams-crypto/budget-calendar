import test from 'node:test';
import assert from 'node:assert/strict';
import {api,ApiError,OfflineError,connection,isOffline} from '../public/api.mjs';
import {syncStatus,syncAge,updateAllowed} from '../public/status.mjs';

const now=Date.UTC(2026,9,30,12);
const base={conflict:false,pending:0,offline:false,syncError:'',checking:false,lastSync:now-5*60000,updatedAt:'2026-10-30T11:00:00Z',now};
function online(value){Object.defineProperty(globalThis,'navigator',{value:{onLine:value},configurable:true})}
function respond(result){const calls=[];globalThis.fetch=async(...args)=>{calls.push(args);if(result instanceof Error)throw result;return result};return calls}
const reset=()=>{online(true);connection.reachable=true};

test('offline is amber, plain and says what is waiting',()=>{
 assert.deepEqual(syncStatus({...base,offline:true,pending:3}),{status:'Offline, changes saved on this phone',kind:'pending',detail:'3 changes waiting · last synced 5 min ago'});
 assert.equal(syncStatus({...base,offline:true,pending:1}).detail,'1 change waiting · last synced 5 min ago');
 assert.deepEqual(syncStatus({...base,offline:true}),{status:'Offline, changes saved on this phone',kind:'pending',detail:'Nothing waiting · last synced 5 min ago'});
});
test('offline outranks an old sync error, so it is never red',()=>{
 const shown=syncStatus({...base,offline:true,pending:1,syncError:'Failed to fetch'});
 assert.equal(shown.kind,'pending');assert.doesNotMatch(JSON.stringify(shown),/failed|fetch/i);
});
test('real errors and conflicts stay distinct from offline',()=>{
 assert.equal(syncStatus({...base,syncError:'Service request failed (500).'}).kind,'failed');
 assert.equal(syncStatus({...base,offline:true,conflict:true}).kind,'conflict');
 assert.deepEqual(syncStatus({...base,pending:2}),{status:'Pending sync',kind:'pending',detail:'2 local changes'});
 assert.equal(syncStatus({...base,checking:true}).status,'Syncing');
 assert.equal(syncStatus(base).status,'Synced');
});
test('last sync age reads naturally',()=>{
 assert.equal(syncAge(now-10000,now),'last synced just now');
 assert.equal(syncAge(now-59*60000,now),'last synced 59 min ago');
 assert.equal(syncAge(now-3600000,now),'last synced 1 hour ago');
 assert.equal(syncAge(now-5*3600000,now),'last synced 5 hours ago');
 assert.equal(syncAge(now-86400000,now),'last synced 1 day ago');
 assert.equal(syncAge(now-3*86400000,now),'last synced 3 days ago');
 assert.equal(syncAge(undefined,now),'not synced yet on this phone');
});
test('the update prompt waits for forms, queued saves and conflicts',()=>{
 const clear={sheetOpen:false,signInForm:false,pending:0,conflict:false,busy:false};
 assert.equal(updateAllowed(clear),true);
 for(const blocked of [{sheetOpen:true},{signInForm:true},{pending:1},{conflict:true},{busy:true}])assert.equal(updateAllowed({...clear,...blocked}),false,JSON.stringify(blocked));
});
test('a browser network failure becomes a typed offline error with plain words',async()=>{
 reset();respond(new TypeError('Failed to fetch'));
 await assert.rejects(api('/me'),error=>error instanceof OfflineError&&!(error instanceof ApiError)&&!/fetch/i.test(error.message));
 assert.equal(connection.reachable,false);assert.equal(isOffline(),true);
});
test('no request is attempted while the phone says it is offline',async()=>{
 reset();online(false);const calls=respond(new Response('{}'));
 await assert.rejects(api('/me'),OfflineError);assert.equal(calls.length,0);assert.equal(isOffline(),true);
 reset();
});
test('any reply from the service, even an error, means it is reachable',async()=>{
 reset();connection.reachable=false;respond(new Response(JSON.stringify({error:{code:'broken',message:'Service broke'}}),{status:500}));
 await assert.rejects(api('/me'),error=>error instanceof ApiError&&error.status===500&&error.message==='Service broke');
 assert.equal(connection.reachable,true);assert.equal(isOffline(),false);
});
test('an unreadable reply is a real error, not offline',async()=>{
 reset();respond(new Response('<html>Bad gateway</html>',{status:502}));
 await assert.rejects(api('/me'),error=>!(error instanceof OfflineError)&&/unreadable/.test(error.message));
 assert.equal(isOffline(),false);
});
