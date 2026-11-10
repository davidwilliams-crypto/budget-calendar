// CACHE is written by scripts/stamp-mobile-worker.ps1 from a hash of public/. Never edit it by hand: a phone only finds a new build when these bytes change.
const CACHE='budget-household-d59178bad3555de7';
const SHELL=['./','./index.html','./styles.css','./app.mjs','./backup.mjs','./api.mjs','./status.mjs','./engine.mjs','./editor-scope.mjs','./state.mjs','./storage.mjs','./writer-lock.mjs','./crypto.mjs','./budget_engine.wasm','./manifest.webmanifest','./icon-192.png','./icon-512.png'];
const cached=request=>caches.open(CACHE).then(cache=>cache.match(request));
// Pages from before stamped names have no update prompt, so a new worker cannot wait for them. Their code is already in memory, and an open form is unaffected.
const replacesUnstamped=()=>caches.keys().then(keys=>{if(keys.some(k=>k.startsWith('budget-household-')&&k.includes('.')))return self.skipWaiting()});
self.addEventListener('install',event=>event.waitUntil(caches.open(CACHE).then(cache=>cache.addAll(SHELL.map(path=>new Request(path,{cache:'reload'})))).then(replacesUnstamped)));
self.addEventListener('activate',event=>event.waitUntil(caches.keys().then(keys=>Promise.all(keys.filter(k=>k.startsWith('budget-household-')&&k!==CACHE).map(k=>caches.delete(k)))).then(()=>self.clients.claim())));
self.addEventListener('message',event=>{if(event.data?.type==='skip-waiting')self.skipWaiting()});
// One worker serves one build: the page and its modules always come from the same cache, so they cannot mix versions.
self.addEventListener('fetch',event=>{const url=new URL(event.request.url);if(event.request.method!=='GET'||url.origin!==self.location.origin||url.pathname.startsWith('/api/')||url.pathname==='/health')return;if(event.request.mode==='navigate'){event.respondWith(cached('./index.html').then(hit=>hit||fetch(event.request)));return}if(SHELL.some(path=>new URL(path,self.registration.scope).href===url.href))event.respondWith(cached(event.request).then(hit=>hit||fetch(event.request)))});
