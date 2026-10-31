// Fresh-device unlock regressions use only generated accounts and budgets under target/.
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {mkdir,readFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {createBudgetServer} from '../server/server.mjs';
import {generateRecoveryKey,importRecoveryKey,sealSnapshot} from '../public/crypto.mjs';
import {loadEngine,householdToday} from '../public/engine.mjs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE_PATH?pathToFileURL(process.env.PLAYWRIGHT_MODULE_PATH).href:'playwright');
const repository=fileURLToPath(new URL('../../',import.meta.url)),directory=resolve(repository,`target/client-unlock-${Date.now()}`);
await mkdir(directory,{recursive:true});
const setup='fixture-unlock-not-real-token-123456',password='fixture-unlock-password-12345';
const port=await new Promise(resolve=>{const server=createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port))})}),origin=`http://127.0.0.1:${port}`;
const service=createBudgetServer({dataDir:directory,publicDir:resolve(repository,'budget-mobile/public'),publicOrigin:origin,development:true,setupToken:setup});
await new Promise(resolve=>service.server.listen(port,'127.0.0.1',resolve));
let browser;
async function request(path,body,token){const response=await fetch(origin+'/api'+path,{method:body?'POST':'GET',headers:{'X-Budget-Client':'native',...(token?{Authorization:`Bearer ${token}`}:{}) ,...(body?{'Content-Type':'application/json'}:{})},body:body?JSON.stringify(body):undefined});const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data}
async function login(page,username){await page.goto(origin);await page.getByLabel('Username',{exact:true}).fill(username);await page.locator('input[name=password]').fill(password);await page.getByRole('button',{name:'Sign in',exact:false}).click();await page.getByLabel('Household passcode').waitFor()}
async function useKey(page){const already=await page.getByLabel('Recovery key',{exact:true}).count()>0;if(!already){await page.getByRole('button',{name:'Use the recovery key instead'}).click()}await page.getByLabel('Recovery key',{exact:true}).waitFor()}
async function stored(page,accountId,aid,bid){return page.evaluate(async({accountId,aid,bid})=>{const {openStorage}=await import('/storage.mjs'),storage=await openStorage();return {selected:await storage.get(`selection:${accountId}`),keyA:Boolean(await storage.get(`key:${accountId}:${aid}`)),keyB:Boolean(await storage.get(`key:${accountId}:${bid}`)),stateA:Boolean(await storage.get(`state:${accountId}:${aid}`)),stateB:Boolean(await storage.get(`state:${accountId}:${bid}`))}},{accountId,aid,bid})}
try{
 const owner=await request('/setup',{setupToken:setup,username:'unlock-owner',password});
 const aid=crypto.randomUUID(),bid=crypto.randomUUID(),recoveryA=generateRecoveryKey(),recoveryB=generateRecoveryKey(),keyA=await importRecoveryKey(recoveryA),keyB=await importRecoveryKey(recoveryB),today=householdToday('America/Halifax');
 const wasm=await readFile(resolve(repository,'budget-mobile/public/budget_engine.wasm')),engine=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
 for(const [id,name,key]of[[aid,'A empty fixture',keyA],[bid,'B daily fixture',keyB]]){const snapshot=engine({action:'new',id,name,timeZone:'America/Halifax',today});await request('/budgets',{id,name,payload:await sealSnapshot(snapshot,key,id,1)},owner.token)}
 const invitation=await request(`/budgets/${bid}/invites`,{recipient:'unlock-guest'},owner.token),guest=await request('/invites/accept',{code:invitation.code,username:'unlock-guest',password});
 browser=await chromium.launch({headless:true,...(process.env.BUDGET_CHROME?{executablePath:process.env.BUDGET_CHROME}:{})});
 const context=await browser.newContext({viewport:{width:390,height:844}}),page=await context.newPage(),errors=[];
 page.on('pageerror',error=>errors.push(error.message));await login(page,'unlock-owner');
 const picker=page.getByLabel('Household',{exact:true});
 await picker.waitFor({timeout:3000});assert.equal(await picker.inputValue(),aid);
 assert.deepEqual(await picker.locator('option').allTextContents(),['A empty fixture','B daily fixture']);
 await picker.selectOption(bid);await useKey(page);await page.getByLabel('Recovery key',{exact:true}).fill(recoveryA);await page.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await page.waitForFunction(()=>Boolean(document.querySelector('#sheet .form-error')?.textContent));
 assert.deepEqual(await stored(page,owner.user.id,aid,bid),{selected:undefined,keyA:false,keyB:false,stateA:false,stateB:false});
 assert.equal(await picker.inputValue(),bid);
 await useKey(page);await page.getByLabel('Recovery key',{exact:true}).fill(recoveryB);await page.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await page.locator('.brand strong',{hasText:'B daily fixture'}).waitFor();
 assert.deepEqual(await stored(page,owner.user.id,aid,bid),{selected:bid,keyA:false,keyB:true,stateA:false,stateB:true});
 await page.reload();await page.locator('.brand strong',{hasText:'B daily fixture'}).waitFor();assert.equal(await page.locator('dialog[open]').count(),0);
 console.log('PASS: a fresh owner device chooses the second household, rejects the wrong key without saving it, and reopens that household after reload.');

 // Passcode path: the household's owner sets a passcode, then a fresh member device unlocks with
 // only that passcode — the recovery key is never typed, and a wrong passcode stores nothing.
 const passcode='quiet harbour tin kettle';
 const wrap=engine({action:'passcode_wrap',recoveryKey:recoveryA,passcode,budgetId:aid,entropy:{salt:[...crypto.getRandomValues(new Uint8Array(16))],nonce:[...crypto.getRandomValues(new Uint8Array(12))]}});
 const saved=await (async()=>{const response=await fetch(origin+`/api/budgets/${aid}/passcode`,{method:'PUT',headers:{'X-Budget-Client':'native','Content-Type':'application/json',Authorization:`Bearer ${owner.token}`},body:JSON.stringify({slot:wrap.slot})});const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data})();assert.equal(saved.ok,true);
 const invite2=await request(`/budgets/${aid}/invites`,{recipient:'passcode-guest'},owner.token);
 const guest2=await request('/invites/accept',{code:invite2.code,username:'passcode-guest',password});
 const third=await browser.newContext({viewport:{width:390,height:844}}),phone=await third.newPage();phone.on('pageerror',error=>errors.push(error.message));
 await phone.goto(origin);await phone.getByLabel('Username',{exact:true}).fill('passcode-guest');await phone.locator('input[name=password]').fill(password);await phone.getByRole('button',{name:'Sign in',exact:false}).click();
 await phone.getByLabel('Household passcode').waitFor();assert.deepEqual(await phone.getByLabel('Household',{exact:true}).locator('option').allTextContents(),['A empty fixture']);
 await phone.getByLabel('Household passcode').fill('wrong guess entirely');await phone.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await phone.waitForFunction(()=>Boolean(document.querySelector('#sheet .form-error')?.textContent));
 assert.deepEqual(await stored(phone,guest2.user.id,aid,bid),{selected:undefined,keyA:false,keyB:false,stateA:false,stateB:false},'a wrong passcode must save nothing');
 await phone.getByLabel('Household passcode').fill(passcode);await phone.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await phone.locator('.brand strong',{hasText:'A empty fixture'}).waitFor();
 assert.deepEqual(await stored(phone,guest2.user.id,aid,bid),{selected:aid,keyA:true,keyB:false,stateA:true,stateB:false});
 console.log('PASS: a fresh member device unlocks with the household passcode alone; a wrong passcode saves nothing.');

 // A late unlock response from another account must not save its key or reopen its household.
 const other=await browser.newContext({viewport:{width:390,height:844}}),second=await other.newPage();second.on('pageerror',error=>errors.push(error.message));await login(second,'unlock-owner');
 await second.getByLabel('Household',{exact:true}).selectOption(bid);
 let release,reached;const held=new Promise(resolve=>{reached=resolve}),wait=new Promise(resolve=>{release=resolve});let once=true;
 await second.route(origin+`/api/budgets/${bid}`,async route=>{
  if(once&&route.request().method()==='GET'){
   once=false;const response=await route.fetch();assert.equal(response.status(),200,'the held owner response must have succeeded before logout');
   reached();await wait;await route.fulfill({response});return;
  }
  await route.continue();
 });
 await useKey(second);await second.getByLabel('Recovery key',{exact:true}).fill(recoveryB);await second.getByRole('button',{name:'Unlock budget',exact:true}).click();await held;
 await second.getByRole('button',{name:'Close dialog',exact:true}).click();await second.getByRole('button',{name:'Sign out',exact:true}).click();
 await second.getByLabel('Username',{exact:true}).fill('unlock-guest');await second.locator('input[name=password]').fill(password);await second.getByRole('button',{name:'Sign in',exact:false}).click();
 await second.getByLabel('Household',{exact:true}).waitFor();assert.deepEqual(await second.getByLabel('Household',{exact:true}).locator('option').allTextContents(),['B daily fixture']);
 await useKey(second);await second.getByLabel('Recovery key',{exact:true}).fill(recoveryB);await second.getByRole('button',{name:'Unlock budget',exact:true}).click();await second.locator('.brand strong',{hasText:'B daily fixture'}).waitFor();
 const completed=second.waitForResponse(response=>response.url()===origin+`/api/budgets/${bid}`);release();await completed;await second.waitForTimeout(100);
 assert.deepEqual(await stored(second,owner.user.id,aid,bid),{selected:undefined,keyA:false,keyB:false,stateA:false,stateB:false});
 assert.equal((await stored(second,guest.user.id,aid,bid)).selected,bid);assert.equal(await second.locator('.brand strong').innerText(),'B daily fixture');
 assert.deepEqual(errors,[]);console.log('PASS: one-household unlock names its destination and a delayed unlock cannot cross an account change.');
}finally{await browser?.close();await service.close()}
