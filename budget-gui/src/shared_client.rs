//! Shared-budget transport, authenticated encryption and recoverable device state.
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::{Aead, Payload, OsRng, rand_core::RngCore}};
use base64::{Engine, engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}};
use budget_core::sharing::{Snapshot, SharedPayment};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, io::{Read, Write}, path::Path, time::Duration};

const MAX_SNAPSHOT_BYTES: usize = 6 * 1024 * 1024 - 1024;
const MAX_DEVICE_BYTES: usize = 100 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct User { pub id: String, pub username: String }
#[derive(Clone, Serialize, Deserialize)]
pub struct Connection { pub server: String, pub token: String, pub user: User }
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all="camelCase")]
pub struct Remote { pub id: String, pub name: String, pub revision: u64, pub payload: Value, pub updated_at: String, pub role: String, pub summary: String }
#[derive(Clone, Serialize, Deserialize)]
pub struct Pending { pub operation_id: String, pub snapshot: Snapshot, pub request: Option<Value> }
#[derive(Clone, Serialize, Deserialize)]
pub struct SharedDocument {
    pub connection: Connection,
    pub recovery_key: String,
    pub baseline: Snapshot,
    pub working: Snapshot,
    pub revision: u64,
    pub pending: Vec<Pending>,
    pub conflict: Option<Remote>,
    pub role: String,
    pub last_sync: String,
}
#[derive(Debug)]
pub struct ServiceError { pub status: u16, pub message: String, pub current: Option<Remote> }
impl std::fmt::Display for ServiceError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"{}",self.message)} }
fn local_error(message: impl Into<String>) -> ServiceError { ServiceError { status:0,message:message.into(),current:None } }

pub fn normalize_server(server:&str)->Result<String,String>{
    let url=reqwest::Url::parse(server.trim()).map_err(|_|"Enter a complete HTTPS server address.")?;
    let loopback=matches!(url.host_str(),Some("localhost"|"127.0.0.1"|"[::1]"));
    if (url.scheme()!="https"&&!(url.scheme()=="http"&&loopback))||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some()||url.path()!="/" {
        return Err("Use an HTTPS server origin without a path, password or query. HTTP is allowed only on localhost for testing.".into());
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
pub struct Api { connection: Connection, client:reqwest::blocking::Client }
impl Api {
    pub fn new(connection:Connection)->Result<Self,ServiceError>{
        let mut connection=connection;connection.server=normalize_server(&connection.server).map_err(local_error)?;
        let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(20)).connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_|local_error("Unable to initialize secure networking."))?;
        Ok(Self{connection,client})
    }
    pub fn request(&self,method:&str,path:&str,body:Option<Value>)->Result<Value,ServiceError>{
        if !path.starts_with("/api/")||path.contains("..")||path.contains('?')||path.contains('#'){return Err(local_error("Invalid service request."))}
        let method=reqwest::Method::from_bytes(method.as_bytes()).map_err(|_|local_error("Invalid service method."))?;
        let mut request=self.client.request(method,format!("{}{}",self.connection.server,path)).header("X-Budget-Client","native");
        if !self.connection.token.is_empty(){request=request.bearer_auth(&self.connection.token)}
        if let Some(body)=body{request=request.json(&body)}
        let response=request.send().map_err(|_|local_error("Could not reach the sync service. Your pending changes remain on this device."))?;
        let status=response.status().as_u16();
        let mut bytes=Vec::new();response.take(12*1024*1024+1).read_to_end(&mut bytes).map_err(|_|local_error("Could not read the sync response."))?;
        if bytes.len()>12*1024*1024{return Err(local_error("The service response exceeds the supported limit."))}
        let value:Value=serde_json::from_slice(&bytes).map_err(|_|local_error("The sync service returned an unreadable response."))?;
        if !(200..300).contains(&status){return Err(ServiceError{status,message:value.pointer("/error/message").and_then(Value::as_str).unwrap_or("The sync request failed.").chars().take(500).collect(),current:value.get("current").and_then(|v|serde_json::from_value(v.clone()).ok())})}
        Ok(value)
    }
    pub fn login(server:&str,username:&str,password:&str)->Result<Connection,ServiceError>{
        let mut connection=Connection{server:normalize_server(server).map_err(local_error)?,token:String::new(),user:User{id:String::new(),username:String::new()}};
        let response=Self::new(connection.clone())?.request("POST","/api/login",Some(json!({"username":username,"password":password})))?;
        connection.token=response.get("token").and_then(Value::as_str).filter(|s|!s.is_empty()).ok_or_else(||local_error("The service did not return a native session."))?.into();
        connection.user=serde_json::from_value(response["user"].clone()).map_err(|_|local_error("Invalid account response."))?;Ok(connection)
    }
    pub fn remote(&self,id:&str)->Result<Remote,ServiceError>{
        validate_budget_id(id).map_err(local_error)?;
        serde_json::from_value(self.request("GET",&format!("/api/budgets/{id}"),None)?).map_err(|_|local_error("Invalid shared-budget response."))
    }
}
fn validate_budget_id(id:&str)->Result<(),String>{uuid::Uuid::parse_str(id).map(|_|()).map_err(|_|"Invalid shared budget identity.".into())}

pub fn generate_recovery_key()->String { let mut key=[0u8;32];OsRng.fill_bytes(&mut key);format!("BC1-{}",URL_SAFE_NO_PAD.encode(key)) }
fn key_bytes(text:&str)->Result<Vec<u8>,String>{
    let raw=text.trim().strip_prefix("BC1-").ok_or("Enter the complete recovery key beginning BC1-.")?;
    let bytes=URL_SAFE_NO_PAD.decode(raw).map_err(|_|"Invalid recovery key.")?;
    if bytes.len()!=32{return Err("Invalid recovery key.".into())}Ok(bytes)
}
fn aad(id:&str,revision:u64)->Result<Vec<u8>,String>{if id.is_empty()||id.len()>200||revision==0||revision>9_007_199_254_740_991{return Err("Invalid budget identity or revision.".into())}Ok(format!("BudgetCalendar:1:{id}:{revision}").into_bytes())}
fn encode_snapshot(snapshot: &Snapshot) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(snapshot).map_err(|_| "Unable to encode the shared budget.")?;
    if bytes.len() > MAX_SNAPSHOT_BYTES { return Err("This budget exceeds the encrypted synchronization size limit.".into()); }
    Ok(bytes)
}
pub fn seal(snapshot:&Snapshot,key:&str,revision:u64)->Result<Value,String>{
    let key=key_bytes(key)?;let cipher=Aes256Gcm::new_from_slice(&key).map_err(|_|"Invalid recovery key.")?;
    let bytes=encode_snapshot(snapshot)?;
    let mut iv=[0u8;12];OsRng.fill_bytes(&mut iv);
    let data=cipher.encrypt(Nonce::from_slice(&iv),Payload{msg:&bytes,aad:&aad(&snapshot.id,revision)?}).map_err(|_|"Unable to encrypt the shared budget.")?;
    Ok(json!({"version":1,"algorithm":"AES-256-GCM","iv":STANDARD.encode(iv),"data":STANDARD.encode(data)}))
}
pub fn unlock(remote:&Remote,key:&str)->Result<Snapshot,String>{
    let payload=&remote.payload;if payload["version"]!=1||payload["algorithm"]!="AES-256-GCM"{return Err("Unsupported encrypted shared budget.".into())}
    let iv=STANDARD.decode(payload["iv"].as_str().ok_or("Invalid encryption nonce.")?).map_err(|_|"Invalid encryption nonce.")?;
    let encoded=payload["data"].as_str().filter(|s|s.len()<=12*1024*1024).ok_or("Invalid encrypted budget.")?;
    let data=STANDARD.decode(encoded).map_err(|_|"Invalid encrypted budget.")?;
    if iv.len()!=12||data.len()<16{return Err("Invalid encrypted budget.".into())}
    let key=key_bytes(key)?;let cipher=Aes256Gcm::new_from_slice(&key).map_err(|_|"Invalid recovery key.")?;
    let bytes=cipher.decrypt(Nonce::from_slice(&iv),Payload{msg:&data,aad:&aad(&remote.id,remote.revision)?}).map_err(|_|"Unable to unlock this budget. Check its recovery key and revision.")?;
    let snapshot:Snapshot=serde_json::from_slice(&bytes).map_err(|_|"The decrypted budget has an unsupported format.")?;
    if snapshot.id!=remote.id{return Err("Budget identity mismatch.".into())}budget_core::sharing::validate_snapshot(&snapshot)?;Ok(snapshot)
}
pub fn household_today(time_zone:&str)->Result<i32,String>{use chrono::Datelike;let zone:chrono_tz::Tz=time_zone.parse().map_err(|_|"Unknown household time zone.")?;let now=chrono::Utc::now().with_timezone(&zone);Ok(now.year()*10000+now.month() as i32*100+now.day() as i32)}

impl SharedDocument {
    pub fn open(connection:Connection,recovery_key:String,remote:Remote)->Result<Self,String>{let snapshot=unlock(&remote,&recovery_key)?;Ok(Self{connection,recovery_key,baseline:snapshot.clone(),working:snapshot,revision:remote.revision,pending:vec![],conflict:None,role:remote.role,last_sync:remote.updated_at})}
    pub fn capture(&mut self,snapshot:Snapshot)->Result<(),String>{
        if self.conflict.is_some(){return Err("Review the conflicting shared version before editing.".into())}
        if snapshot==self.working{return Ok(())}budget_core::sharing::validate_snapshot(&snapshot)?;
        if snapshot.id!=self.working.id{return Err("Cannot replace a shared budget with another budget.".into())}
        encode_snapshot(&snapshot)?;
        self.pending.push(Pending{operation_id:uuid::Uuid::new_v4().to_string(),snapshot:snapshot.clone(),request:None});self.working=snapshot;Ok(())
    }
    pub fn prepare(&mut self)->Result<Option<Value>,String>{
        if self.conflict.is_some(){return Ok(None)}
        let Some(first)=self.pending.first_mut()else{return Ok(None)};
        if first.request.is_none(){first.request=Some(json!({"operationId":first.operation_id,"expectedRevision":self.revision,"payload":seal(&first.snapshot,&self.recovery_key,self.revision+1)?,"summary":"Updated shared budget"}));}
        Ok(first.request.clone())
    }
    pub fn acknowledge(&mut self,remote:Remote,operation_id:&str)->Result<(),String>{
        if self.pending.first().is_none_or(|p|p.operation_id!=operation_id){return Err("Acknowledgement does not match the pending change.".into())}
        if remote.revision!=self.revision+1{return Err("Unexpected acknowledgement revision.".into())}
        let snapshot=unlock(&remote,&self.recovery_key)?;
        if self.pending[0].snapshot!=snapshot{return Err("Acknowledged contents differ from the pending change.".into())}
        self.pending.remove(0);self.baseline=snapshot;self.revision=remote.revision;self.last_sync=remote.updated_at;self.role=remote.role;
        if self.pending.is_empty(){self.working=self.baseline.clone()}Ok(())
    }
    pub fn receive(&mut self,remote:Remote)->Result<bool,String>{
        if remote.revision<=self.revision{return Ok(false)}
        let snapshot=unlock(&remote,&self.recovery_key)?;
        if !self.pending.is_empty(){self.conflict=Some(remote);return Ok(false)}
        self.revision=remote.revision;self.baseline=snapshot.clone();self.working=snapshot;self.last_sync=remote.updated_at;self.role=remote.role;Ok(true)
    }
    pub fn resolve(&mut self,keep_local:bool)->Result<(),String>{
        let remote=self.conflict.clone().ok_or("There is no conflict to review.")?;let current=unlock(&remote,&self.recovery_key)?;
        let merged=if keep_local{merge_snapshots(&self.baseline,&self.working,&current,true)?}else{current.clone()};
        // Combining two valid device snapshots can exceed the transport limit. Preserve the
        // unresolved local queue and both versions if that merged result cannot be synchronized.
        encode_snapshot(&merged)?;
        self.revision=remote.revision;self.baseline=current.clone();self.working=current;self.last_sync=remote.updated_at;self.role=remote.role;self.pending.clear();self.conflict=None;
        self.capture(merged)
    }
}

/// Apply only the locally changed records to the latest shared version. Conflicts require explicit review.
pub fn merge_snapshots(base:&Snapshot,local:&Snapshot,remote:&Snapshot,reviewed:bool)->Result<Snapshot,String>{
    if base.id!=local.id||base.id!=remote.id{return Err("Cannot merge different budgets.".into())}
    let mut merged=remote.clone();
    let old:BTreeMap<_,_>=base.payments.iter().map(|p|(p.id.clone(),p)).collect();
    let mine:BTreeMap<_,_>=local.payments.iter().map(|p|(p.id.clone(),p)).collect();
    let theirs:BTreeMap<_,_>=remote.payments.iter().map(|p|(p.id.clone(),p)).collect();
    let keys:BTreeSet<_>=old.keys().chain(mine.keys()).cloned().collect();
    for id in keys{let before=old.get(&id);let after=mine.get(&id);if before==after{continue}if !reviewed&&theirs.get(&id)!=before&&theirs.get(&id)!=after{return Err(format!("Both devices changed payment {id}. Review both versions."))}
        merged.payments.retain(|p|p.id!=id);if let Some(record)=after{merged.payments.push((**record).clone())}
    }
    macro_rules! field {($name:ident)=>{if local.$name!=base.$name{if !reviewed&&remote.$name!=base.$name&&remote.$name!=local.$name{return Err(concat!("Both devices changed ",stringify!($name),". Review both versions.").into())}merged.$name=local.$name.clone();}}}
    merged.adjustments=merge_adjustments(&base.adjustments,&local.adjustments,&remote.adjustments,reviewed)?;
    field!(notes);field!(name);field!(time_zone);
    let live:BTreeSet<_>=merged.payments.iter().map(|p|p.id.clone()).collect();
    merged.tombstones=remote.tombstones.iter().chain(&local.tombstones).filter(|id|!live.contains(*id)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
    // Concurrent creates can have equal native numeric IDs; stable IDs remain authoritative.
    let mut used=BTreeSet::new();let mut next=local.next_id.max(remote.next_id);
    for SharedPayment{payment,..} in &mut merged.payments{if !used.insert(payment.id){payment.id=next;next=next.checked_add(1).ok_or("Payment identity limit reached.")?;used.insert(payment.id);}}
    merged.next_id=next.max(merged.payments.iter().map(|p|p.payment.id).max().unwrap_or(0)+1);
    budget_core::sharing::validate_snapshot(&merged)?;Ok(merged)
}

/// Adjustment metadata has independent identities by list kind and date. Duplicate entries at
/// one date remain an ordered group because legacy files can depend on the first matching entry.
fn merge_dated<T: Clone + PartialEq>(base: &[T], local: &[T], remote: &[T], date: fn(&T) -> i32, label: &str, reviewed: bool) -> Result<Vec<T>, String> {
    fn groups<T>(entries: &[T], date: fn(&T) -> i32) -> BTreeMap<i32, Vec<&T>> {
        let mut grouped = BTreeMap::<_, Vec<_>>::new();
        for entry in entries { grouped.entry(date(entry)).or_default().push(entry); }
        grouped
    }
    let old = groups(base, date); let mine = groups(local, date); let theirs = groups(remote, date);
    let mut changed = BTreeSet::new();
    for key in old.keys().chain(mine.keys()) {
        let before = old.get(key); let after = mine.get(key);
        if before == after { continue; }
        if !reviewed && theirs.get(key) != before && theirs.get(key) != after {
            return Err(format!("Both devices changed {label} on {key}. Review both versions."));
        }
        changed.insert(*key);
    }
    let mut remaining: BTreeMap<_, _> = theirs.iter().map(|(key, entries)| (*key, entries.len())).collect();
    let mut used = BTreeMap::<i32, usize>::new();
    let mut merged = Vec::new();
    for entry in remote {
        let key = date(entry);
        if !changed.contains(&key) { merged.push(entry.clone()); continue; }
        let desired = mine.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let cursor = used.entry(key).or_default();
        // Replace existing slots in place, preserving interleaved unrelated dates and duplicate order.
        if *cursor < desired.len() { merged.push((*desired[*cursor]).clone()); *cursor += 1; }
        let left = remaining.get_mut(&key).unwrap(); *left -= 1;
        if *left == 0 {
            merged.extend(desired[*cursor..].iter().map(|entry| (**entry).clone()));
            *cursor = desired.len();
        }
    }
    // Newly introduced dates retain their complete local ordering, including duplicate entries.
    for entry in local {
        let key = date(entry);
        if changed.contains(&key) && !theirs.contains_key(&key) { merged.push(entry.clone()); }
    }
    Ok(merged)
}

fn merge_adjustments(base: &budget_core::Payment, local: &budget_core::Payment, remote: &budget_core::Payment, reviewed: bool) -> Result<budget_core::Payment, String> {
    let mut merged = remote.clone();
    macro_rules! field { ($($name:ident),+ $(,)?) => { $(
        if local.$name != base.$name {
            if !reviewed && remote.$name != base.$name && remote.$name != local.$name {
                return Err(concat!("Both devices changed adjustment ", stringify!($name), ". Review both versions.").into());
            }
            merged.$name = local.$name.clone();
        }
    )+ }; }
    field!(id, amount, description, start_date, frequency, freq_type, n_pays, end_date, image_path, flags,
        disabled, category, loan_amount, flag_value, grace_period_days);
    merged.opt = merge_dated(&base.opt, &local.opt, &remote.opt, |p| p.date, "balance checkpoints", reviewed)?;
    merged.note = merge_dated(&base.note, &local.note, &remote.note, |p| p.date, "day notes", reviewed)?;
    merged.flag = merge_dated(&base.flag, &local.flag, &remote.flag, |p| p.date, "day flags", reviewed)?;
    merged.hilighting = merge_dated(&base.hilighting, &local.hilighting, &remote.hilighting, |p| p.date, "day highlights", reviewed)?;
    Ok(merged)
}

/// Windows protects the complete offline document and session using the current user's DPAPI key.
pub fn save_device(path:&Path,document:&SharedDocument)->Result<(),String>{
    let bytes=serde_json::to_vec(document).map_err(|_|"Unable to encode device state.")?;let encrypted=protect(&bytes,false)?;
    write_protected_bytes(path,&encrypted,MAX_DEVICE_BYTES).map_err(|e|format!("Changes could not be saved to this device: {e}"))
}
pub fn load_device(path:&Path)->Result<SharedDocument,String>{let bytes=std::fs::read(path).map_err(|e|format!("Unable to read the shared device cache: {e}"))?;if bytes.len()>MAX_DEVICE_BYTES{return Err("Shared device cache exceeds the supported limit.".into())}let plaintext=protect(&bytes,true)?;let doc:SharedDocument=serde_json::from_slice(&plaintext).map_err(|_|"The shared device cache is damaged.")?;budget_core::sharing::validate_snapshot(&doc.working)?;budget_core::sharing::validate_snapshot(&doc.baseline)?;normalize_server(&doc.connection.server)?;key_bytes(&doc.recovery_key)?;Ok(doc)}
#[cfg(windows)]
fn protect(bytes:&[u8],decrypt:bool)->Result<Vec<u8>,String>{
    #[repr(C)]struct Blob{length:u32,data:*mut u8}
    #[link(name="crypt32")]unsafe extern "system"{fn CryptProtectData(input:*const Blob,description:*const u16,entropy:*const Blob,reserved:*mut std::ffi::c_void,prompt:*mut std::ffi::c_void,flags:u32,output:*mut Blob)->i32;fn CryptUnprotectData(input:*const Blob,description:*mut *mut u16,entropy:*const Blob,reserved:*mut std::ffi::c_void,prompt:*mut std::ffi::c_void,flags:u32,output:*mut Blob)->i32;}
    #[link(name="kernel32")]unsafe extern "system"{fn LocalFree(memory:*mut std::ffi::c_void)->*mut std::ffi::c_void;}
    let input=Blob{length:u32::try_from(bytes.len()).map_err(|_|"Device cache too large.")?,data:bytes.as_ptr() as *mut u8};let mut output=Blob{length:0,data:std::ptr::null_mut()};
    let ok=unsafe{if decrypt{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null_mut(),1,&mut output)}else{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null_mut(),1,&mut output)}};
    if ok==0{return Err("Windows could not protect or unlock this device's shared-budget cache.".into())}
    let result=unsafe{std::slice::from_raw_parts(output.data,output.length as usize).to_vec()};unsafe{LocalFree(output.data.cast());}Ok(result)
}
#[cfg(not(windows))]
fn protect(_bytes:&[u8],_decrypt:bool)->Result<Vec<u8>,String>{Err("Secure native device storage is available on Windows.".into())}

/// Protect an auxiliary recovery record with the same user-bound key as the device document.
pub fn save_protected<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Unable to encode protected recovery state.")?;
    let encrypted = protect(&bytes, false)?;
    write_protected_bytes(path, &encrypted, MAX_DEVICE_BYTES).map_err(|e| format!("Unable to save protected recovery state: {e}"))
}
/// Check the actual encrypted length, including DPAPI overhead, before any filesystem mutation.
fn write_protected_bytes(path: &Path, encrypted: &[u8], limit: usize) -> Result<(), String> {
    if encrypted.len() > limit { return Err("Protected device state exceeds the supported size limit. The previous cache has been kept unchanged.".into()); }
    let parent = path.parent().ok_or("Invalid protected recovery location.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Unable to create protected recovery storage: {e}"))?;
    let temporary = parent.join(format!(".recovery-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(encrypted).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}
pub fn load_protected<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Unable to read protected recovery state: {e}"))?;
    if bytes.len() > MAX_DEVICE_BYTES { return Err("Protected recovery state exceeds the supported limit.".into()); }
    let plaintext = protect(&bytes, true)?;
    serde_json::from_slice(&plaintext).map_err(|_| "The protected recovery state is damaged.".into())
}

#[cfg(test)]mod tests{
    use super::*;
    fn fixture()->Snapshot{serde_json::from_value(serde_json::from_str::<Value>(&budget_core::sharing::request_json(&json!({"action":"new","id":"94dc206f-8f43-4e28-941b-a9bd4c4847da","name":"Fixture","timeZone":"America/Halifax","today":20261029}).to_string())).unwrap()["result"].clone()).unwrap()}
    fn remote(s:&Snapshot,key:&str,revision:u64)->Remote{Remote{id:s.id.clone(),name:s.name.clone(),revision,payload:seal(s,key,revision).unwrap(),updated_at:"fixture".into(),role:"owner".into(),summary:"Fixture".into()}}
    fn connection()->Connection{Connection{server:"http://127.0.0.1:8787".into(),token:"fixture-session".into(),user:User{id:"fixture".into(),username:"fixture".into()}}}
    #[test]fn encryption_binds_key_budget_revision_and_detects_tampering(){let s=fixture();let key=generate_recovery_key();let mut r=remote(&s,&key,1);assert_eq!(unlock(&r,&key).unwrap(),s);assert!(unlock(&r,&generate_recovery_key()).is_err());r.revision=2;assert!(unlock(&r,&key).is_err());r.revision=1;r.payload["data"]=json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAA");assert!(unlock(&r,&key).is_err());}
    #[test]fn native_transport_rejects_insecure_or_credential_bearing_origins(){assert!(normalize_server("https://example.com").is_ok());assert!(normalize_server("http://127.0.0.1:8787").is_ok());for input in ["http://example.com","https://user:password@example.com","https://example.com/api","https://example.com/?token=x"]{assert!(normalize_server(input).is_err(),"{input}")}}
    #[test]fn pending_request_survives_serialization_and_later_edits_survive_ack(){let s=fixture();let key=generate_recovery_key();let mut d=SharedDocument::open(connection(),key.clone(),remote(&s,&key,1)).unwrap();let mut a=s.clone();a.notes="first".into();d.capture(a.clone()).unwrap();let first=d.prepare().unwrap().unwrap();let mut b=a.clone();b.notes="second".into();d.capture(b.clone()).unwrap();let mut d:SharedDocument=serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();assert_eq!(d.prepare().unwrap().unwrap(),first);d.acknowledge(remote(&a,&key,2),first["operationId"].as_str().unwrap()).unwrap();assert_eq!(d.working,b);assert_eq!(d.pending.len(),1);assert_eq!(d.prepare().unwrap().unwrap()["expectedRevision"],2);}
    #[test]
    fn an_unsyncable_snapshot_is_refused_before_changing_the_working_copy_or_pending_request() {
        let s = fixture(); let key = generate_recovery_key();
        let mut document = SharedDocument::open(connection(), key.clone(), remote(&s, &key, 1)).unwrap();
        let mut queued = s; queued.notes = "Already saved pending change".into(); document.capture(queued).unwrap(); document.prepare().unwrap();
        let before = serde_json::to_value(&document).unwrap();
        let mut too_large = document.working.clone();
        too_large.adjustments.note = (0..100).map(|_| budget_core::model::Note { date: 20261029, note: "x".repeat(65536) }).collect();
        budget_core::sharing::validate_snapshot(&too_large).unwrap();
        assert!(document.capture(too_large).unwrap_err().contains("synchronization size limit"));
        assert_eq!(serde_json::to_value(&document).unwrap(), before);
    }
    #[cfg(windows)]
    #[test]
    fn protected_size_limit_refuses_before_replacing_or_creating_any_cache() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("target/shared-size-tests").join(uuid::Uuid::new_v4().to_string());
        let path = dir.join("cache.enc"); let old = json!({"pending":["old generated fixture"]});
        save_protected(&path, &old).unwrap(); let saved = std::fs::read(&path).unwrap();
        let next = json!({"pending":["x".repeat(2048)]});
        let encrypted = protect(&serde_json::to_vec(&next).unwrap(), false).unwrap();
        let limit = encrypted.len() - 1;
        assert!(write_protected_bytes(&path, &encrypted, limit).unwrap_err().contains("size limit"));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert_eq!(load_protected::<Value>(&path).unwrap(), old);
        let never_created = dir.join("not-created/cache.enc");
        assert!(write_protected_bytes(&never_created, &encrypted, limit).is_err());
        assert!(!never_created.parent().unwrap().exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "a refused cache leaves no temporary files");
        write_protected_bytes(&path, &encrypted, encrypted.len()).unwrap();
        assert_eq!(load_protected::<Value>(&path).unwrap(), next, "the exact supported boundary remains readable");
    }
    #[test]
    fn oversized_conflict_merge_preserves_the_unresolved_local_queue() {
        let base = fixture(); let key = generate_recovery_key();
        let mut document = SharedDocument::open(connection(), key.clone(), remote(&base, &key, 1)).unwrap();
        let mut local = base.clone(); let mut theirs = base.clone();
        local.adjustments.note = (0..50).map(|_| budget_core::model::Note { date: 20261029, note: "a".repeat(65536) }).collect();
        theirs.adjustments.note = (0..50).map(|_| budget_core::model::Note { date: 20261030, note: "b".repeat(65536) }).collect();
        document.capture(local.clone()).unwrap();
        document.receive(remote(&theirs, &key, 2)).unwrap();
        let operation = document.pending[0].operation_id.clone();
        assert!(document.resolve(true).unwrap_err().contains("synchronization size limit"));
        assert_eq!(document.working, local); assert_eq!(document.baseline, base);
        assert_eq!(document.pending.len(), 1); assert_eq!(document.pending[0].operation_id, operation);
        assert_eq!(document.revision, 1); assert_eq!(document.conflict.as_ref().unwrap().revision, 2);
    }
    #[test]fn remote_update_cannot_replace_pending_work(){let s=fixture();let key=generate_recovery_key();let mut d=SharedDocument::open(connection(),key.clone(),remote(&s,&key,1)).unwrap();let mut mine=s.clone();mine.notes="mine".into();d.capture(mine.clone()).unwrap();let mut theirs=s.clone();theirs.name="Theirs".into();assert!(!d.receive(remote(&theirs,&key,2)).unwrap());assert_eq!(d.working,mine);d.resolve(true).unwrap();assert_eq!(d.working.notes,"mine");assert_eq!(d.working.name,"Theirs");assert_eq!(d.pending.len(),1);}
    #[test]fn same_field_conflict_requires_review(){let s=fixture();let mut a=s.clone();a.notes="a".into();let mut b=s.clone();b.notes="b".into();assert!(merge_snapshots(&s,&a,&b,false).is_err());assert_eq!(merge_snapshots(&s,&a,&b,true).unwrap().notes,"a");}
    #[test]
    fn keeping_local_day_note_preserves_remote_day_balances_and_notes() {
        let base = fixture(); let mut local = base.clone(); let mut remote = base.clone();
        local.adjustments.set_note(20261029, Some("Local grocery note"));
        remote.adjustments.set_custom_amount(20261030, 1250.0);
        remote.adjustments.set_note(20261030, Some("Remote payday note"));
        let merged = merge_snapshots(&base, &local, &remote, true).unwrap();
        assert_eq!(merged.adjustments.get_note(20261029).as_deref(), Some("Local grocery note"));
        assert_eq!(merged.adjustments.get_note(20261030).as_deref(), Some("Remote payday note"));
        assert_eq!(merged.adjustments.option_by_date(20261030).unwrap().amount, 1250.0);
    }
    #[test]
    fn independent_balance_and_day_note_on_same_date_merge_without_review() {
        let base = fixture(); let mut local = base.clone(); let mut remote = base.clone();
        local.adjustments.set_note(20261029, Some("Local note"));
        remote.adjustments.set_custom_amount(20261029, 450.0);
        remote.adjustments.set_flag(20261029, '#');
        remote.adjustments.set_hilighting(20261029, 0x123456);
        let merged = merge_snapshots(&base, &local, &remote, false).unwrap();
        assert_eq!(merged.adjustments.get_note(20261029).as_deref(), Some("Local note"));
        assert_eq!(merged.adjustments.option_by_date(20261029).unwrap().amount, 450.0);
        assert_eq!(merged.adjustments.get_flag(20261029), '#');
        assert_eq!(merged.adjustments.get_hilighting(20261029), 0x123456);
    }
    #[test]
    fn date_metadata_merge_retains_deletions_duplicate_order_and_other_dates() {
        use budget_core::model::{Note, PaymentOption};
        let mut base = fixture();
        base.adjustments.opt = vec![PaymentOption { date: 20261029, amount: 100.0, deleted: false }, PaymentOption { date: 20261030, amount: 200.0, deleted: false }, PaymentOption { date: 20261029, amount: 300.0, deleted: false }];
        base.adjustments.note = vec![Note { date: 20261029, note: "first".into() }, Note { date: 20261030, note: "other date".into() }, Note { date: 20261029, note: "duplicate".into() }];
        let mut local = base.clone(); let mut remote = base.clone();
        local.adjustments.note.retain(|n| n.date != 20261029);
        local.adjustments.opt[0].amount = 125.0;
        remote.adjustments.opt[1].amount = 250.0;
        remote.adjustments.note[1].note = "Remote other date".into();
        let merged = merge_snapshots(&base, &local, &remote, false).unwrap();
        assert_eq!(merged.adjustments.opt.iter().map(|p| (p.date, p.amount)).collect::<Vec<_>>(), vec![(20261029, 125.0), (20261030, 250.0), (20261029, 300.0)]);
        assert_eq!(merged.adjustments.note, vec![Note { date: 20261030, note: "Remote other date".into() }]);
    }
    #[test]
    fn same_date_note_edit_versus_delete_requires_review_and_retains_other_dates() {
        let mut base = fixture(); base.adjustments.set_note(20261029, Some("Original"));
        let mut local = base.clone(); let mut remote = base.clone();
        local.adjustments.set_note(20261029, None);
        remote.adjustments.set_note(20261029, Some("Edited remotely")); remote.adjustments.set_note(20261030, Some("Keep this"));
        assert!(merge_snapshots(&base, &local, &remote, false).is_err());
        let merged = merge_snapshots(&base, &local, &remote, true).unwrap();
        assert!(merged.adjustments.get_note(20261029).is_none());
        assert_eq!(merged.adjustments.get_note(20261030).as_deref(), Some("Keep this"));
    }
    #[test]
    #[ignore="Run by the generated three-client HTTP integration fixture"]
    fn native_service_interoperation(){
        let server=std::env::var("BUDGET_TEST_ORIGIN").expect("isolated fixture server");
        assert!(server.starts_with("http://127.0.0.1:"),"fixture server must be loopback");
        let id=std::env::var("BUDGET_TEST_ID").unwrap();let key=std::env::var("BUDGET_TEST_KEY").unwrap();
        let connection=Api::login(&server,"fixture-owner","fixture-password-12345").unwrap();
        let api=Api::new(connection.clone()).unwrap();let remote=api.remote(&id).unwrap();
        let mut document=SharedDocument::open(connection,key,remote).unwrap();
        assert!(document.working.payments.iter().any(|p|p.payment.description=="Offline A groceries"));
        assert!(document.working.payments.iter().any(|p|p.payment.description=="Online B pay"));
        let mut next=document.working.clone();next.notes="Windows fixture edit".into();document.capture(next).unwrap();
        let request=document.prepare().unwrap().unwrap();let operation=request["operationId"].as_str().unwrap().to_string();
        let remote:Remote=serde_json::from_value(api.request("POST",&format!("/api/budgets/{id}/commits"),Some(request.clone())).unwrap()).unwrap();
        document.acknowledge(remote,&operation).unwrap();assert!(document.pending.is_empty());
        let repeated:Remote=serde_json::from_value(api.request("POST",&format!("/api/budgets/{id}/commits"),Some(request)).unwrap()).unwrap();
        assert_eq!(repeated.revision,document.revision);assert_eq!(unlock(&api.remote(&id).unwrap(),&document.recovery_key).unwrap().notes,"Windows fixture edit");
    }
    #[cfg(windows)]#[test]fn device_cache_is_protected_and_reopens(){let s=fixture();let key=generate_recovery_key();let d=SharedDocument::open(connection(),key.clone(),remote(&s,&key,1)).unwrap();let dir=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("target/shared-device-tests").join(uuid::Uuid::new_v4().to_string());let path=dir.join("state.enc");save_device(&path,&d).unwrap();assert_eq!(load_device(&path).unwrap().working,s);assert!(!String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("fixture-session"));save_device(&path,&d).unwrap();}
}
