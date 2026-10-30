export class ApiError extends Error{constructor(status,data){super(data?.error?.message||`Service request failed (${status}).`);this.status=status;this.code=data?.error?.code;this.current=data?.current}}
export class OfflineError extends Error{constructor(){super('You are offline. Reconnect and try again.')}}
// False once a request cannot reach the service, even if the phone believes it has a connection. Any reply sets it back.
export const connection={reachable:true};
export const isOffline=()=>navigator.onLine===false||!connection.reachable;
export async function api(path,body,method=body===undefined?'GET':'POST'){
 if(navigator.onLine===false)throw new OfflineError();
 let response;try{response=await fetch(`/api${path}`,{method,credentials:'same-origin',cache:'no-store',headers:body===undefined?{}:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)})}catch{connection.reachable=false;throw new OfflineError()}
 connection.reachable=true;
 let data;try{data=await response.json()}catch{throw Error('The service returned an unreadable response. Your pending edits remain on this device.')}
 if(!response.ok)throw new ApiError(response.status,data);return data;
}
