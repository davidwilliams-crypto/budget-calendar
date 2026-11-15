// Recovery-key display and history review regressions use only generated accounts and budgets under target/.
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {mkdir,readFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {createBudgetServer} from '../server/server.mjs';
import {generateRecoveryKey,importRecoveryKey,sealSnapshot} from '../public/crypto.mjs';
import {loadEngine,householdToday} from '../public/engine.mjs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE_PATH?pathToFileURL(process.env.PLAYWRIGHT_MODULE_PATH).href:'playwright');
const repository=fileURLToPath(new URL('../../',import.meta.url)),directory=resolve(repository,`target/client-recovery-${Date.now()}`);
await mkdir(directory,{recursive:true});
const setup='fixture-recovery-not-real-token-123456',password='fixture-recovery-password-12345';
const port=await new Promise(resolve=>{const server=createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port))})}),origin=`http://127.0.0.1:${port}`;
const service=createBudgetServer({dataDir:directory,publicDir:resolve(repository,'budget-mobile/public'),publicOrigin:origin,development:true,setupToken:setup});
await new Promise(resolve=>service.server.listen(port,'127.0.0.1',resolve));
let browser;
async function request(path,body,token){const response=await fetch(origin+'/api'+path,{method:body?'POST':'GET',headers:{'X-Budget-Client':'native',...(token?{Authorization:`Bearer ${token}`}:{}),...(body?{'Content-Type':'application/json'}:{})},body:body?JSON.stringify(body):undefined});const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data}
const synced=page=>page.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
async function shownKey(page){
 // Opening a household finishes with a sync; give it a few renders before judging.
 for(let attempt=0;attempt<20;attempt++){await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByRole('heading',{name:'Passcode, recovery & backups'}).waitFor();if(await page.getByRole('button',{name:'Show recovery key',exact:true}).count())break;await page.waitForTimeout(100)}
 assert.equal(await page.getByRole('button',{name:'Show recovery key',exact:true}).count(),1,'a device that just unlocked or created this household must offer its recovery key');
 await page.getByRole('button',{name:'Show recovery key',exact:true}).click();const text=await page.locator('#sheet .key').innerText();await page.getByRole('button',{name:'Close dialog',exact:true}).click();return text;
}
try{
 const owner=await request('/setup',{setupToken:setup,username:'recovery-owner',password});
 const id=crypto.randomUUID(),recovery=generateRecoveryKey(),key=await importRecoveryKey(recovery),today=householdToday('America/Halifax');
 const wasm=await readFile(resolve(repository,'budget-mobile/public/budget_engine.wasm')),engine=await loadEngine(`data:application/wasm;base64,${wasm.toString('base64')}`);
 const snapshot=engine({action:'new',id,name:'Recovery fixture',timeZone:'America/Halifax',today});
 await request('/budgets',{id,name:'Recovery fixture',payload:await sealSnapshot(snapshot,key,id,1)},owner.token);
 // A member holding the key commits a snapshot whose fields are markup, then a valid one on top.
 const hostile={...snapshot,payments:{length:'<b id="injected-by-history">fixture</b>'}};
 await request(`/budgets/${id}/commits`,{operationId:'fixture-hostile-history',expectedRevision:1,payload:await sealSnapshot(hostile,key,id,2),summary:'Updated budget'},owner.token);
 await request(`/budgets/${id}/commits`,{operationId:'fixture-valid-after-hostile',expectedRevision:2,payload:await sealSnapshot(snapshot,key,id,3),summary:'Updated budget'},owner.token);
 browser=await chromium.launch({headless:true,...(process.env.BUDGET_CHROME?{executablePath:process.env.BUDGET_CHROME}:{})});
 const page=await browser.newPage({viewport:{width:390,height:844}}),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin);await page.getByLabel('Username',{exact:true}).fill('recovery-owner');await page.locator('input[name=password]').fill(password);await page.getByRole('button',{name:'Sign in',exact:false}).click();
 await page.getByRole('button',{name:'Use the recovery key instead'}).click();await page.getByLabel('Recovery key',{exact:true}).fill(recovery);await page.getByRole('button',{name:'Unlock budget',exact:true}).click();
 await page.locator('.brand strong',{hasText:'Recovery fixture'}).waitFor();await synced(page);
 assert.equal(await shownKey(page),recovery);
 console.log('PASS: unlocking with the recovery key keeps it available to show and to re-wrap the passcode.');

 await page.getByRole('button',{name:'Create another household',exact:true}).click();await page.getByLabel('Household name').fill('Created fixture');await page.getByLabel('Household passcode').fill('fixture created passcode words');await page.getByLabel('Confirm passcode').fill('fixture created passcode words');
 await page.getByRole('button',{name:'Create recovery key'}).click();const created=await page.locator('#sheet .key').innerText();await page.getByText('I saved the recovery key somewhere safe.').check();await page.getByRole('button',{name:'Create encrypted household'}).click();
 await page.locator('.brand strong',{hasText:'Created fixture'}).waitFor();await synced(page);
 assert.notEqual(created,recovery);assert.equal(await shownKey(page),created);
 console.log('PASS: a newly created household keeps its recovery key available on the creating device.');

 await page.locator('#budget-picker').selectOption(id);await page.locator('.brand strong',{hasText:'Recovery fixture'}).waitFor();await synced(page);
 await page.getByRole('button',{name:'Activity',exact:true}).click();await page.locator('[data-action="history-review"][data-revision="2"]').click();
 await page.getByRole('heading',{name:'Snapshot · revision 2'}).waitFor();
 assert.equal(await page.locator('#injected-by-history').count(),0,'decrypted snapshot fields must never become markup');
 assert.match(await page.locator('#sheet .lead').innerText(),/<b id="injected-by-history">fixture<\/b>/);
 assert.deepEqual(errors,[]);console.log('PASS: reviewing a retained snapshot shows its contents as text.');
}finally{await browser?.close();await service.close()}
