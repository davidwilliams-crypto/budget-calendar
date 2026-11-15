// An idle, visible phone checks the household's revision and downloads the full encrypted snapshot only when it changed.
// Generated accounts, keys and budgets only, under target/.
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {mkdir,readFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {createBudgetServer} from '../server/server.mjs';
import {generateRecoveryKey,importRecoveryKey,sealSnapshot} from '../public/crypto.mjs';
import {loadEngine,householdToday} from '../public/engine.mjs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE_PATH?pathToFileURL(process.env.PLAYWRIGHT_MODULE_PATH).href:'playwright');
const repository=fileURLToPath(new URL('../../',import.meta.url)),directory=resolve(repository,`target/client-poll-${Date.now()}`);
await mkdir(directory,{recursive:true});
const setup='fixture-poll-not-real-token-1234567',password='fixture-poll-password-12345';
const port=await new Promise(resolve=>{const server=createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port))})}),origin=`http://127.0.0.1:${port}`;
const service=createBudgetServer({dataDir:directory,publicDir:resolve(repository,'budget-mobile/public'),publicOrigin:origin,development:true,setupToken:setup});
await new Promise(resolve=>service.server.listen(port,'127.0.0.1',resolve));
let browser;
async function request(path,body,token){const response=await fetch(origin+'/api'+path,{method:body?'POST':'GET',headers:{'X-Budget-Client':'native',...(token?{Authorization:`Bearer ${token}`}:{}),...(body?{'Content-Type':'application/json'}:{})},body:body?JSON.stringify(body):undefined});const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data}
try{
 const owner=await request('/setup',{setupToken:setup,username:'poll-owner',password});
 const id=crypto.randomUUID(),recovery=generateRecoveryKey(),key=await importRecoveryKey(recovery),today=householdToday('America/Halifax');
 const wasm=await readFile(resolve(repository,'budget-mobile/public/budget_engine.wasm')),engine=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
 const first=engine({action:'new',id,name:'Poll fixture',timeZone:'America/Halifax',today});
 await request('/budgets',{id,name:'Poll fixture',payload:await sealSnapshot(first,key,id,1)},owner.token);
 browser=await chromium.launch({headless:true,...(process.env.BUDGET_CHROME?{executablePath:process.env.BUDGET_CHROME}:{})});
 const context=await browser.newContext({viewport:{width:390,height:844}}),page=await context.newPage(),errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 const snapshotPath=`/api/budgets/${id}`,revisionPath=`${snapshotPath}/revision`,seen={revision:0,full:0};
 const kind=r=>{if(r.method()!=='GET')return null;const path=new URL(r.url()).pathname;return path===revisionPath?'revision':path===snapshotPath?'full':null};
 page.on('request',r=>{const k=kind(r);if(k)seen[k]++});
 await page.goto(origin);await page.getByLabel('Username',{exact:true}).fill('poll-owner');await page.locator('input[name=password]').fill(password);await page.getByRole('button',{name:'Sign in',exact:false}).click();
 await page.getByRole('button',{name:'Use the recovery key instead'}).click();await page.getByLabel('Recovery key',{exact:true}).fill(recovery);await page.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await page.getByRole('heading',{name:'Your day, at a glance.'}).waitFor();
 assert.equal(await page.evaluate(()=>document.visibilityState),'visible');
 // One sync check: wait for whichever sync request the phone makes next to finish, while nudging the visible-tab tick
 // (the same path the 4 s interval runs). A tick that lands while a sync is still running does nothing, so keep nudging.
 async function syncCheck(){
  const done=page.waitForResponse(r=>kind(r.request())!==null,{timeout:20000});let waiting=true;done.finally(()=>{waiting=false}).catch(()=>{});
  while(waiting){await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await Promise.race([done.catch(()=>{}),new Promise(r=>setTimeout(r,250))])}
  return done;
 }
 const settled=async()=>{await page.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced',null,{timeout:20000})};
 await syncCheck();await settled();
 seen.revision=0;seen.full=0;
 for(let i=0;i<5;i++)await syncCheck();
 await page.waitForTimeout(300);
 const idle={...seen};console.log(`idle: ${idle.revision} revision poll(s), ${idle.full} full snapshot download(s)`);
 // The 4 s interval runs alongside, so one of the five checks may have been answered by a request sent just before counting began.
 assert.ok(idle.revision>=4,`an idle phone checks the revision on each sync (saw ${idle.revision})`);
 assert.equal(idle.full,0,`an idle phone with no remote change downloads no full snapshot (saw ${idle.full})`);
 assert.equal(await page.locator('.status strong').innerText(),'Synced');
 console.log('PASS: an idle visible phone polls the revision and downloads no snapshot while nothing changes.');

 // Another device commits a new balance for today; the phone must notice the new revision, fetch the snapshot and show it.
 const second=engine({action:'apply',snapshot:first,today,operationId:crypto.randomUUID(),command:{type:'balance',date:today,amount:4321.5}});
 const payload=await sealSnapshot(second,key,id,2);
 // Count from before the commit: an interval tick may already see the new revision while the commit reply is on its way here.
 seen.revision=0;seen.full=0;
 const fetched=page.waitForResponse(r=>kind(r.request())==='full',{timeout:20000});
 const accepted=await request(`/budgets/${id}/commits`,{operationId:crypto.randomUUID(),expectedRevision:1,payload,summary:'Updated budget'},owner.token);
 assert.equal(accepted.revision,2);
 while(!seen.full)await syncCheck();
 assert.equal((await fetched).status(),200);
 await page.locator('.balance-amount',{hasText:'4,321.50'}).waitFor({timeout:20000});
 const stored=await page.evaluate(async({accountId,id})=>{const {openStorage}=await import('/storage.mjs');return (await (await openStorage()).get(`state:${accountId}:${id}`))?.revision},{accountId:owner.user.id,id});
 assert.equal(stored,2);
 const changed={...seen};console.log(`after remote change: ${changed.revision} revision poll(s), ${changed.full} full snapshot download(s)`);
 assert.equal(changed.full,1,'one full snapshot download for one remote change');
 seen.revision=0;seen.full=0;
 for(let i=0;i<3;i++)await syncCheck();
 await page.waitForTimeout(300);
 console.log(`idle again: ${seen.revision} revision poll(s), ${seen.full} full snapshot download(s)`);
 assert.equal(seen.full,0,'once up to date, the phone goes back to revision polls only');
 // Sync now is the person asking: it always downloads the full snapshot, even when nothing changed.
 await page.getByRole('button',{name:'Settings',exact:true}).click();const syncNow=page.getByRole('button',{name:'Sync now',exact:true});await syncNow.waitFor();
 seen.revision=0;seen.full=0;
 // A tap that lands while an automatic sync is running is ignored, so tap again until the download starts.
 for(let tap=0;tap<20&&!seen.full;tap++){await syncNow.click();await page.waitForTimeout(500)}
 assert.ok(seen.full>0,'Sync now on an unchanged budget downloads the full snapshot');
 await settled();await page.waitForTimeout(300);
 console.log(`Sync now on an unchanged budget: ${seen.revision} revision poll(s), ${seen.full} full snapshot download(s)`);
 assert.equal(seen.full,1,'Sync now on an unchanged budget downloads the full snapshot once');
 assert.deepEqual(errors,[]);
 console.log('PASS: a change from another device is fetched once, shown on the phone, and polling returns to revision checks.');
}finally{await browser?.close();await service.close()}
