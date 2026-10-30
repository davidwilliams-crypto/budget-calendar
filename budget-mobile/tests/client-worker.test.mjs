// Runs the real sw.js and the real stamp script against generated copies under target/.
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {cp,mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {createBudgetServer} from '../server/server.mjs';

const repository=fileURLToPath(new URL('../../',import.meta.url)),source=resolve(repository,'budget-mobile/public'),stampScript=resolve(repository,'scripts/stamp-mobile-worker.ps1');
const origin='https://budget.invalid';
async function copy(label){const directory=resolve(repository,`target/client-worker-${label}-${randomUUID()}`);await mkdir(directory,{recursive:true});await cp(source,directory,{recursive:true});return directory}
const stamp=(directory,...extra)=>execFileSync('powershell',['-NoProfile','-ExecutionPolicy','Bypass','-File',stampScript,'-PublicDirectory',directory,...extra],{encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
const cacheName=text=>/const CACHE='([^']+)'/.exec(text)[1];
// A browser with an HTTP cache: a plain fetch returns the stale body unless cache:'reload' is asked for.
function browser(directory){
 const seen=[],httpCache=new Map(),caches=new Map(),world={seen,caches,down:false},url=value=>new URL(value?.url??value,`${origin}/sw.js`).href;
 world.fetchAsset=async request=>{const href=url(request),mode=request?.cache;seen.push({href,cache:mode});if(world.down)throw new TypeError('Failed to fetch');const path=new URL(href).pathname,name=path==='/'?'index.html':path.slice(1);if(mode!=='reload'&&httpCache.has(href))return new Response(httpCache.get(href));const body=await readFile(join(directory,name));httpCache.set(href,body);return new Response(body)};
 const open=async name=>{if(!caches.has(name))caches.set(name,new Map());const entries=caches.get(name);return {addAll:async list=>{const fetched=await Promise.all(list.map(async r=>[url(r),await world.fetchAsset(r)]));for(const [k,v] of fetched)entries.set(k,v)},match:async r=>entries.get(url(r))?.clone()}};
 world.store={open,keys:async()=>[...caches.keys()],delete:async name=>caches.delete(name),match:async r=>{for(const name of caches.keys()){const hit=caches.get(name).get(url(r));if(hit)return hit.clone()}}};
 return world;
}
async function load(directory,world){
 const listeners={},self={location:new URL(`${origin}/sw.js`),registration:{scope:`${origin}/`},clients:{claim:async()=>{self.claimed=true}},skipWaiting:async()=>{self.skipped=true},addEventListener:(type,handler)=>{listeners[type]=handler}};
 class Req{constructor(path,init={}){this.url=new URL(path,`${origin}/sw.js`).href;this.cache=init.cache}}
 const text=await readFile(join(directory,'sw.js'),'utf8');vm.runInNewContext(text,{self,caches:world.store,fetch:world.fetchAsset,Request:Req,URL,Promise,console});
 const run=async(type,event={})=>{let done;await listeners[type]({...event,waitUntil:p=>{done=p}});await done};
 const get=async(path,mode='no-cors')=>{let response;listeners.fetch({request:{url:new URL(path,origin).href,method:'GET',mode},respondWith:r=>{response=r}});return await response};
 return {self,run,get,listeners,name:cacheName(text)};
}
test('installing fetches every shell file past the browser cache',async()=>{
 const directory=await copy('reload'),world=browser(directory),worker=await load(directory,world);await worker.run('install');
 assert.ok(world.seen.length>=12);assert.ok(world.seen.every(entry=>entry.cache==='reload'),JSON.stringify(world.seen.filter(entry=>entry.cache!=='reload')));
});
test('a new build reaches the phone instead of the old file, even inside the browser cache window',async()=>{
 const directory=await copy('bump');stamp(directory);const world=browser(directory),first=await load(directory,world);await first.run('install');await first.run('activate');
 const oldCode=await (await first.get('/app.mjs')).text();
 await writeFile(join(directory,'app.mjs'),oldCode+'\n// generated bump marker\n');const printed=stamp(directory),second=await load(directory,world);
 assert.notEqual(second.name,first.name);assert.match(printed,new RegExp(second.name));
 await second.run('install');
 assert.equal(second.self.skipped,undefined,'the new worker must wait for the person');
 assert.equal(await (await first.get('/app.mjs')).text(),oldCode,'the page keeps one consistent version until reload');
 await second.listeners.message({data:{type:'skip-waiting'}});assert.equal(second.self.skipped,true);
 await second.run('activate');assert.match(await (await second.get('/app.mjs')).text(),/generated bump marker/);assert.deepEqual([...world.caches.keys()],[second.name]);
});
test('offline boot works from the worker cache, for the page and the engine',async()=>{
 const directory=await copy('offline'),world=browser(directory),worker=await load(directory,world);await worker.run('install');await worker.run('activate');world.down=true;
 assert.match(await (await worker.get('/','navigate')).text(),/<!doctype html>/i);assert.ok((await (await worker.get('/budget_engine.wasm')).arrayBuffer()).byteLength>8);assert.ok((await (await worker.get('/status.mjs')).text()).length>0);
});
test('the hand-numbered cache of an older release cannot strand a page with no update prompt',async()=>{
 const directory=await copy('legacy'),world=browser(directory);world.caches.set('budget-household-0.8.3',new Map());const worker=await load(directory,world);await worker.run('install');
 assert.equal(worker.self.skipped,true);await worker.run('activate');assert.deepEqual([...world.caches.keys()],[worker.name]);
});
test('the cache name ignores line endings, and the check notices a stale stamp',async()=>{
 const directory=await copy('stamp');stamp(directory);const name=cacheName(await readFile(join(directory,'sw.js'),'utf8'));
 stamp(directory,'-Check');
 const css=join(directory,'styles.css'),text=(await readFile(css,'utf8')).replaceAll('\r\n','\n');
 await writeFile(css,text.replaceAll('\n','\r\n'));stamp(directory,'-Check');await writeFile(css,text);stamp(directory,'-Check');
 assert.equal(cacheName(await readFile(join(directory,'sw.js'),'utf8')),name);
 await writeFile(css,text+'/* changed */');assert.throws(()=>stamp(directory,'-Check'),/stamp-mobile-worker/);
});
test('every deployable file is precached, or stamping refuses',async()=>{
 const directory=await copy('shell');stamp(directory);await writeFile(join(directory,'forgotten.mjs'),'export {};\n');assert.throws(()=>stamp(directory),/forgotten\.mjs/);
 const listed=[...(await readFile(join(source,'sw.js'),'utf8')).matchAll(/'\.\/([^']+)'/g)].map(m=>m[1]);
 for(const name of await readdir(source))if(name!=='sw.js')assert.ok(listed.includes(name),name);
});
test('the worker and the page shell are revalidated; other assets may be cached',async()=>{
 const directory=await copy('headers'),data=resolve(directory,'..',`client-worker-data-${randomUUID()}`);await mkdir(data,{recursive:true});
 const service=createBudgetServer({dataDir:data,publicDir:directory,publicOrigin:'http://127.0.0.1:8796',development:true,setupToken:'fixture-worker-token-not-real-123456'});await new Promise(done=>service.server.listen(0,'127.0.0.1',done));
 try{const base=`http://127.0.0.1:${service.server.address().port}`,control=async path=>(await fetch(base+path)).headers.get('cache-control');for(const path of ['/','/index.html','/sw.js'])assert.equal(await control(path),'no-cache',path);assert.match(await control('/app.mjs'),/max-age/)}finally{await service.close()}
});
