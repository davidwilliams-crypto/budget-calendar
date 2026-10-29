export class ApiError extends Error{constructor(status,data){super(data?.error?.message||`Service request failed (${status}).`);this.status=status;this.code=data?.error?.code;this.current=data?.current}}
export async function api(path,body,method=body===undefined?'GET':'POST'){
 const response=await fetch(`/api${path}`,{method,credentials:'same-origin',cache:'no-store',headers:body===undefined?{}:{'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});
 let data;try{data=await response.json()}catch{throw Error('The service returned an unreadable response. Your pending edits remain on this device.')}
 if(!response.ok)throw new ApiError(response.status,data);return data;
}
