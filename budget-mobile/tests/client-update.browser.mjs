// Real browser, real worker, real server, on a generated copy of public/ under target/.
// Set PLAYWRIGHT_MODULE_PATH to an installed playwright index.mjs if needed.
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {resolve,join} from 'node:path';
import {cp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {createBudgetServer} from '../server/server.mjs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE_PATH?pathToFileURL(process.env.PLAYWRIGHT_MODULE_PATH).href:'playwright');
const repository=fileURLToPath(new URL('../../',import.meta.url)),origin='http://127.0.0.1:8797',fixture=resolve(repository,`target/client-update-${Date.now()}`),publicDir=join(fixture,'public'),data=join(fixture,'data'),token='fixture-update-token-not-real-123456';
await mkdir(data,{recursive:true});await cp(resolve(repository,'budget-mobile/public'),publicDir,{recursive:true});
const stamp=()=>execFileSync('powershell',['-NoProfile','-ExecutionPolicy','Bypass','-File',resolve(repository,'scripts/stamp-mobile-worker.ps1'),'-PublicDirectory',publicDir],{encoding:'utf8'}).trim();
stamp();
const service=createBudgetServer({dataDir:data,publicDir,publicOrigin:origin,development:true,setupToken:token});await new Promise(done=>service.server.listen(8797,'127.0.0.1',done));
let browser;
const bump=async marker=>{const file=join(publicDir,'app.mjs');await writeFile(file,(await readFile(file,'utf8'))+`\nglobalThis.buildMarker='${marker}';\n`);return stamp()};
const hunt=page=>page.evaluate(()=>navigator.serviceWorker.getRegistration().then(r=>r.update()));
try{
 browser=await chromium.launch({headless:true});
 const context=await browser.newContext({viewport:{width:390,height:844},deviceScaleFactor:2,hasTouch:true}),page=await context.newPage(),requests=[];
 const pageErrors=[];page.on('pageerror',e=>pageErrors.push(e.message));
 await page.goto(origin);await page.getByRole('tab',{name:'First setup'}).click();await page.locator('[name=setupToken]').fill(token);await page.locator('[name=username]').fill('update-fixture-owner');await page.locator('[name=password]').fill('fixture-password-12345');await page.getByRole('button',{name:'Create owner account'}).click();await page.getByRole('button',{name:'Create household budget',exact:true}).click();await page.getByLabel('Household name').fill('Update fixture');await page.getByLabel('Household passcode').fill('fixture passcode words');await page.getByLabel('Confirm passcode').fill('fixture passcode words');await page.getByRole('button',{name:'Create recovery key'}).click();await page.getByText('I saved the recovery key somewhere safe.').check();await page.getByRole('button',{name:'Create encrypted household'}).click();await page.getByRole('heading',{name:'Your day, at a glance.'}).waitFor();await page.evaluate(()=>navigator.serviceWorker.ready);await page.waitForFunction(()=>navigator.serviceWorker.controller);await page.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced');
 const bar=page.locator('#update-bar');assert.equal(await bar.isVisible(),false,'a first install is not an update');

 // A healthy phone keeps saying so: background checks every few seconds never replace it.
 for(let waited=0;waited<10000;waited+=250){const line=await page.locator('.status strong').innerText();assert.equal(line,'Synced',`after ${waited} ms`);await page.waitForTimeout(250)}

 // Losing the signal leaves a half-typed sign-in alone (the page still has its account here, so a second context signs out).
 const visitor=await browser.newContext({viewport:{width:320,height:640}}),signIn=await visitor.newPage();await signIn.goto(origin);await signIn.locator('[name=username]').fill('half-typed-name');await signIn.locator('[name=password]').fill('half-typed-password');await visitor.setOffline(true);await signIn.waitForTimeout(300);assert.equal(await signIn.locator('[name=username]').inputValue(),'half-typed-name');assert.equal(await signIn.locator('[name=password]').inputValue(),'half-typed-password');await visitor.close();

 // Offline: amber, no network, no raw error, even past the 4 second timer and across tabs.
 page.on('request',r=>{if(r.url().includes('/api/'))requests.push(r.url())});
 await context.setOffline(true);await page.locator('.status',{hasText:'Offline, changes saved on this phone'}).waitFor();requests.length=0;
 await page.getByRole('button',{name:'Add entry',exact:true}).click();await page.locator('[name=amount]').fill('12.34');await page.locator('input[name=description]').fill('Parking garage lunch');await page.getByRole('button',{name:'Add entry on this device'}).click();await page.locator('.entry-title',{hasText:'Parking garage lunch'}).waitFor();
 await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByRole('button',{name:'Today',exact:true}).click();await page.waitForTimeout(5000);
 const status=page.locator('.status'),shown=await status.innerText();assert.match(shown,/1 change waiting · last synced/);assert.deepEqual(requests,[],'no sync attempts while offline');assert.ok(await status.evaluate(e=>e.classList.contains('pending')&&!e.classList.contains('failed')));
 await page.screenshot({path:join(fixture,'offline.png')});
 // Signing out needs the service, so the prompt says so and a tap answers inside it.
 await page.getByRole('button',{name:'Settings',exact:true}).click();await page.getByRole('button',{name:'Sign out',exact:true}).click();await page.locator('#sheet',{hasText:'Signing out needs a connection'}).waitFor();await page.getByRole('button',{name:'Sign out & keep local work'}).click();await page.locator('#sheet .form-error',{hasText:'You are offline'}).waitFor();assert.equal(await page.locator('#sheet').evaluate(e=>e.open),true);await page.locator('.sheet-close').click();await page.getByRole('button',{name:'Today',exact:true}).click();
 assert.doesNotMatch(await page.locator('body').innerText(),/Failed to fetch|Sync failed/);assert.equal(await page.locator('.error-box').count(),0);
 await page.reload();await page.locator('.entry-title',{hasText:'Parking garage lunch'}).waitFor();assert.match(await status.innerText(),/Offline, changes saved on this phone/);await page.waitForTimeout(4500);assert.equal(await page.locator('.error-box').count(),0);
 await context.setOffline(false);await page.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced',null,{timeout:15000});

 // A new build: the prompt waits for a closed form and a quiet queue, and old code stays until then.
 await page.getByRole('button',{name:'Add entry',exact:true}).click();await page.locator('input[name=description]').fill('Half typed');
 const firstName=await bump('second');await hunt(page);await page.waitForFunction(()=>navigator.serviceWorker.getRegistration().then(r=>Boolean(r.waiting)));await page.waitForTimeout(500);
 assert.equal(await bar.isVisible(),false,'the prompt must not appear over an open form');assert.equal(await page.locator('input[name=description]').inputValue(),'Half typed');
 assert.equal(await page.evaluate(()=>globalThis.buildMarker),undefined);
 await page.locator('.sheet-close').click();await bar.waitFor();assert.match(await bar.innerText(),/Update ready/);
 await page.screenshot({path:join(fixture,'update-ready.png')});
 await page.reload();assert.equal(await page.evaluate(()=>globalThis.buildMarker),undefined,'a plain reload keeps one consistent build');await bar.waitFor();
 // A change waiting to sync holds the prompt back.
 await context.setOffline(true);await page.locator('.status',{hasText:'Offline'}).waitFor();await page.getByRole('button',{name:'Add entry',exact:true}).click();await page.locator('[name=amount]').fill('5');await page.locator('input[name=description]').fill('Queued before update');await page.getByRole('button',{name:'Add entry on this device'}).click();await page.locator('.entry-title',{hasText:'Queued before update'}).waitFor();await page.waitForTimeout(500);assert.equal(await bar.isVisible(),false,'the prompt must wait for a queued change');
 await context.setOffline(false);await page.waitForFunction(()=>document.querySelector('.status strong')?.textContent==='Synced',null,{timeout:15000});await bar.waitFor({timeout:6000});
 await Promise.all([page.waitForEvent('framenavigated',{timeout:10000}),bar.getByRole('button',{name:'Reload',exact:true}).click()]);await page.waitForFunction(()=>globalThis.buildMarker==='second',null,{timeout:10000});
 assert.deepEqual(await page.evaluate(()=>caches.keys()),[firstName]);await page.locator('.entry-title',{hasText:'Queued before update'}).waitFor();assert.equal(await bar.isVisible(),false);

 // The updated build still boots with no network.
 await context.setOffline(true);await page.reload();await page.locator('.entry-title',{hasText:'Queued before update'}).waitFor();assert.equal(await page.evaluate(()=>globalThis.buildMarker),'second');await context.setOffline(false);
 assert.deepEqual(pageErrors,[]);
 console.log(`PASS: offline stays amber with no requests; a new build waits for forms and queued changes, then reloads into new code; offline boot works. Screenshots in ${fixture}`);
}finally{await browser?.close();await service.close()}
