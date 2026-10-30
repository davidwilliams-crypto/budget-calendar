// What the status line says. Offline is a normal way to use the app, so it is always amber and never a failure.
const count=(n,word)=>`${n} ${word}${n===1?'':'s'}`;
export function syncAge(at,now=Date.now()){if(!at)return 'not synced yet on this phone';const minutes=Math.floor((now-at)/60000),hours=Math.floor(minutes/60),days=Math.floor(hours/24);return `last synced ${minutes<1?'just now':minutes<60?`${minutes} min ago`:hours<24?`${count(hours,'hour')} ago`:`${count(days,'day')} ago`}`}
export function syncStatus({conflict,pending,offline,syncError,checking,lastSync,updatedAt,now=Date.now()}){
 if(conflict)return {status:'Review conflict',kind:'conflict',detail:'Both versions kept'};
 if(offline)return {status:'Offline, changes saved on this phone',kind:'pending',detail:`${pending?`${count(pending,'change')} waiting`:'Nothing waiting'} · ${syncAge(lastSync,now)}`};
 if(pending)return {status:'Pending sync',kind:'pending',detail:`${pending} local ${pending===1?'change':'changes'}`};
 if(syncError)return {status:'Sync failed',kind:'failed',detail:'Local copy available'};
 if(checking)return {status:'Syncing',kind:'',detail:''};
 return {status:'Synced',kind:'',detail:updatedAt?new Date(updatedAt).toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'}):''};
}
// A reload must never land on an open form, or on a change the household has not received yet.
export const updateAllowed=({sheetOpen,signInForm,pending,conflict,busy})=>!sheetOpen&&!signInForm&&!pending&&!conflict&&!busy;
