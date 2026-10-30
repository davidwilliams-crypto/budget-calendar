// Integration runner, deliberately separate from fast node --test suites.
// All accounts, budgets, passwords and recovery keys below are generated fixtures.
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readFileSync} from 'node:fs';
import {spawn,spawnSync} from 'node:child_process';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createServer} from 'node:net';
import {createBudgetServer} from '../server/server.mjs';
import {generateRecoveryKey,importRecoveryKey,sealSnapshot,openSnapshot} from '../public/crypto.mjs';
import {readBackup} from '../public/backup.mjs';
import {householdToday} from '../public/engine.mjs';
// The Windows client is one of the three clients. It always runs; a missing Cargo stops the test rather than dropping the step.
const cargo=process.env.BUDGET_CARGO||'cargo';
if(spawnSync(cargo,['--version'],{windowsHide:true}).status!==0)throw Error(`The Windows client step cannot run because Cargo was not found (${cargo}). Set BUDGET_CARGO to cargo.exe.`);
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE_PATH?pathToFileURL(process.env.PLAYWRIGHT_MODULE_PATH).href:'playwright');
const root=resolve(import.meta.dirname,'../..'),directory=resolve(root,'target',`three-client-${Date.now()}`);
await mkdir(directory,{recursive:true});
const port=await new Promise(resolvePort=>{const s=createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolvePort(p))})});
const origin=`http://127.0.0.1:${port}`,setup='fixture-setup-token-not-real-123456',password='fixture-password-12345';
const service=createBudgetServer({dataDir:directory,publicDir:resolve(root,'budget-mobile/public'),publicOrigin:origin,development:true,setupToken:setup});
await new Promise(r=>service.server.listen(port,'127.0.0.1',r));
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'budget-mobile/public/budget_engine.wasm')),{});
function engine(value){const e=instance.exports,b=new TextEncoder().encode(JSON.stringify(value)),p=e.alloc(b.length);new Uint8Array(e.memory.buffer,p,b.length).set(b);e.request(p,b.length);e.dealloc(p,b.length);const result=JSON.parse(new TextDecoder().decode(new Uint8Array(e.memory.buffer,e.result_ptr(),e.result_len())));assert.equal(result.ok,true,result.error);return result.result;}
async function request(path,body,token,method){const response=await fetch(origin+'/api'+path,{method:method||(body?'POST':'GET'),headers:{'X-Budget-Client':'native',...(body?{'Content-Type':'application/json'}:{}),...(token?{Authorization:`Bearer ${token}`}:{})},body:body?JSON.stringify(body):undefined});const data=await response.json();if(!response.ok)throw Error(JSON.stringify({status:response.status,data}));return data;}
const results=[],errors=[];let browser;
try{
  const owner=await request('/setup',{setupToken:setup,username:'fixture-owner',password});
  const id=crypto.randomUUID(),recovery=generateRecoveryKey(),key=await importRecoveryKey(recovery),today=householdToday('America/Halifax');
  let snapshot=engine({action:'new',id,name:'Three-device fixture',timeZone:'America/Halifax',today});
  snapshot=engine({action:'apply',snapshot,today,operationId:crypto.randomUUID(),command:{type:'balance',date:today,amount:2000}});
  await request('/budgets',{id,name:snapshot.name,payload:await sealSnapshot(snapshot,key,id,1)},owner.token);
  const invitation=await request(`/budgets/${id}/invites`,{recipient:'fixture-wife'},owner.token);
  const wife=await request('/invites/accept',{code:invitation.code,username:'fixture-wife',password});
  browser=await chromium.launch({headless:true,...(process.env.BUDGET_CHROME?{executablePath:process.env.BUDGET_CHROME}:{})});
  const a=await browser.newContext({viewport:{width:390,height:844}}),b=await browser.newContext({viewport:{width:360,height:800}});
  const pa=await a.newPage(),pb=await b.newPage();
  for(const page of [pa,pb])page.on('pageerror',e=>errors.push(e.message));
  async function login(page,username){await page.goto(origin);await page.getByLabel('Username',{exact:true}).fill(username);await page.locator('input[name=password]').fill(password);await page.getByRole('button',{name:'Sign in',exact:false}).click();await page.getByLabel('Recovery key',{exact:true}).fill(recovery);await page.getByRole('button',{name:'Unlock budget',exact:true}).click();await page.getByRole('heading',{name:'Your day, at a glance.'}).waitFor();await page.evaluate(()=>navigator.serviceWorker.ready);}
  async function add(page,name,amount,kind='expense'){await page.getByRole('button',{name:'Add entry',exact:true}).click();await page.locator('select[name=kind]').selectOption(kind);await page.getByLabel('Amount',{exact:true}).fill(String(amount));await page.getByLabel('Description',{exact:true}).fill(name);await page.getByRole('button',{name:'Add entry on this device',exact:true}).click();await page.locator('.entry-title',{hasText:name}).waitFor();}
  await login(pa,'fixture-owner');await login(pb,'fixture-wife');results.push('separate owner/editor sign-in and device keys');
  await a.setOffline(true);await add(pa,'Offline A groceries',15);await pa.reload();await pa.locator('.entry-title',{hasText:'Offline A groceries'}).waitFor();results.push('offline edit survives reload');
  await add(pb,'Online B pay',100,'income');await pb.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
  await a.setOffline(false);await pa.getByRole('button',{name:'Review versions',exact:true}).waitFor({timeout:15000});await pa.getByRole('button',{name:'Review versions',exact:true}).click();
  await pa.getByRole('button',{name:'Reapply my edits to current household',exact:true}).click();await pa.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
  const current=await request(`/budgets/${id}`,undefined,owner.token);snapshot=await openSnapshot(current.payload,key,id,current.revision);
  assert.deepEqual(snapshot.payments.map(p=>p.payment.description).sort(),['Offline A groceries','Online B pay']);results.push('conflict preserves both users and reviewed reapply retains unrelated edits');
  const env={...process.env,BUDGET_TEST_ORIGIN:origin,BUDGET_TEST_ID:id,BUDGET_TEST_KEY:recovery};
  await new Promise((done,reject)=>{const child=spawn(cargo,['test','-p','budget-gui','shared_client::tests::native_service_interoperation','--offline','--','--ignored','--exact'],{cwd:root,env,windowsHide:true,stdio:['ignore','pipe','pipe']});let out='';child.stdout.on('data',d=>out+=d);child.stderr.on('data',d=>out+=d);child.on('error',reject);child.on('close',async code=>{await writeFile(resolve(directory,'native-test.log'),out);code===0?done():reject(Error('Native integration failed; see generated native-test.log'))})});
  const native=await request(`/budgets/${id}`,undefined,owner.token);assert.equal((await openSnapshot(native.payload,key,id,native.revision)).notes,'Windows fixture edit');results.push('Windows native client decrypts phone edits and commits a readable revision, exact retry deduplicated');
  // Wait for the editor's actual durable revision, not an earlier "Synced" label,
  // before making the independent backup/restore trial. Stale edits intentionally conflict.
  // Polled from here: waitForFunction treats the promise of an async predicate as already true, so it would not wait at all.
  const editorRevision=()=>pb.evaluate(async({account,id})=>{const {openStorage}=await import('/storage.mjs');return (await (await openStorage()).get(`state:${account}:${id}`))?.revision},{account:wife.user.id,id});
  const waitUntil=Date.now()+45000;while(await editorRevision()!==native.revision){assert.ok(Date.now()<waitUntil,`phone B never reached revision ${native.revision}`);await pb.waitForTimeout(100)}
  await pb.getByRole('button',{name:'Settings',exact:true}).click();const downloadPromise=pb.waitForEvent('download');await pb.getByRole('button',{name:'Export encrypted backup',exact:true}).click();const download=await downloadPromise;await download.saveAs(resolve(directory,'encrypted-backup.json'));const saved=JSON.parse(readFileSync(resolve(directory,'encrypted-backup.json'),'utf8'));const backed=await readBackup(saved,key,id);assert.equal(backed.snapshot.id,id);results.push('downloaded encrypted backup decrypts with household key');
  await pb.getByRole('button',{name:'Today',exact:true}).click();await add(pb,'Temporary after backup',5);await pb.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
  const beforeRestore=await request(`/budgets/${id}`,undefined,owner.token);assert.ok((await openSnapshot(beforeRestore.payload,key,id,beforeRestore.revision)).payments.some(p=>p.payment.description==='Temporary after backup'));
  await pb.getByRole('button',{name:'Settings',exact:true}).click();await pb.getByRole('button',{name:'Restore encrypted backup',exact:true}).click();await pb.locator('input[name=backup]').setInputFiles(resolve(directory,'encrypted-backup.json'));await pb.getByRole('button',{name:'Review backup',exact:true}).click();await pb.getByRole('button',{name:'Restore as a new revision',exact:true}).click();await pb.waitForFunction(()=>!document.querySelector('#sheet').open);await pb.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
  const restored=await request(`/budgets/${id}`,undefined,owner.token);assert.equal(restored.revision,beforeRestore.revision+1);assert.deepEqual(await openSnapshot(restored.payload,key,id,restored.revision),backed.snapshot);results.push('encrypted backup restores reviewed snapshot as a new authenticated revision');
  await pb.getByRole('button',{name:'Today',exact:true}).click();await b.setOffline(true);await add(pb,'Revoked offline entry',3);
  await request(`/budgets/${id}/members/${wife.user.id}`,{},owner.token,'DELETE');await b.setOffline(false);await pb.getByText('Access is no longer available.',{exact:false}).waitFor({timeout:15000});await pb.locator('.entry-title',{hasText:'Revoked offline entry'}).waitFor();results.push('revocation rejects queued write and preserves recoverable local work');
  for(const width of [320,390,1000]){await pa.setViewportSize({width,height:850});await pa.waitForTimeout(100);assert.ok(await pa.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`horizontal overflow at ${width}`)}
  await pa.setViewportSize({width:390,height:844});await pa.screenshot({path:resolve(directory,'household.png'),fullPage:true});assert.deepEqual(errors,[]);results.push('responsive layout and no page runtime errors');
  await writeFile(resolve(directory,'results.json'),JSON.stringify({results},null,2));console.log(JSON.stringify({results,directory},null,2));
}catch(error){if(browser)for(const context of browser.contexts())for(const page of context.pages()){await writeFile(resolve(directory,`failure-${browser.contexts().indexOf(context)}.txt`),await page.locator('body').innerText());}throw error}
finally{await browser?.close();await service.close()}
