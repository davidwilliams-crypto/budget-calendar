//! Native sharing UI. Network jobs never replace the live document; only receipts cross threads.
use super::*;
use crate::shared_client::{self as client, Api, Connection, Remote, SharedDocument};
use budget_core::sharing::{snapshot_from_db_with_operation, snapshot_to_db};
use serde_json::{json, Value};
use std::sync::mpsc::{self, Receiver};

mod panel;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Discarded copies kept per household. The oldest goes first, which also bounds their size.
const DISCARDED_KEPT: usize = 5;

pub(crate) struct SharedUi {
    pub document: Option<SharedDocument>,
    pub open: bool,
    pub error: Option<String>,
    cache: PathBuf,
    /// The OS handle denies all other opens of the cache-directory lock for this window's lifetime.
    /// It is acquired only by an explicit sharing operation and retained even after Close budget.
    cache_lock: Option<std::fs::File>,
    server: String,
    username: String,
    password: String,
    connection: Option<Connection>,
    budgets: Vec<Value>,
    key: String,
    /// Unlock input: the household passcode, or the recovery key when `unlock_use_key` is set.
    unlock_passcode: String,
    unlock_use_key: bool,
    passcode_a: String,
    passcode_b: String,
    passcode_dialog: bool,
    name: String,
    zone: String,
    saved_key: bool,
    creating: bool,
    job: Option<Receiver<Event>>,
    last_poll: Instant,
    deferred: Option<Remote>,
    staged: Option<SharedDocument>,
    preview: Option<panel::ConflictPreview>,
    /// "Use current shared version" was clicked: how many changes the confirmation names.
    confirm_discard: Option<usize>,
    /// This household's discarded copies, newest first, read once per household.
    discarded: Vec<Discarded>,
    discarded_for: Option<String>,
    recover: Option<usize>,
}

/// What "Use current shared version" replaced, in the protected cache. Keeping the version it
/// started from allows the same reapply that Keep mine does, later and on any shared version.
#[derive(serde::Serialize, serde::Deserialize)]
struct DiscardedWork {
    id: String,
    discarded_at: String,
    changes: usize,
    baseline: budget_core::sharing::Snapshot,
    local: budget_core::sharing::Snapshot,
}

/// A discarded copy as the dialog lists it.
struct Discarded { path: PathBuf, discarded_at: String, changes: usize }

fn changes_text(count: usize) -> String { format!("{count} unsynced change{}", if count == 1 { "" } else { "s" }) }

enum Event {
    SignedIn(Result<(Connection, Vec<Value>), String>),
    Opened(Result<(SharedDocument, Option<String>), String>),
    Passcode(Result<String, String>),
    Sync { id: String, operation: Option<String>, result: Result<Remote, client::ServiceError> },
}

/// A creation is a durable operation before it is a shared document. Keep its exact encrypted
/// request and identity separately from every existing household cache until the server confirms it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CreationCandidate {
    connection: Connection,
    recovery_key: String,
    /// Empty when the household is created before this release or without a passcode.
    #[serde(default)]
    passcode: String,
    snapshot: budget_core::sharing::Snapshot,
    request: Value,
}
impl CreationCandidate {
    fn new(connection: Connection, recovery_key: String, passcode: String, snapshot: budget_core::sharing::Snapshot) -> Result<Self, String> {
        let request = json!({"id":snapshot.id,"name":snapshot.name,"payload":client::seal(&snapshot, &recovery_key, 1)?});
        let mut candidate = Self { connection, recovery_key, passcode, snapshot, request };
        candidate.validate()?;
        Ok(candidate)
    }
    fn validate(&mut self) -> Result<(), String> {
        uuid::Uuid::parse_str(&self.snapshot.id).map_err(|_| "Invalid pending household identity.")?;
        self.connection.server = client::normalize_server(&self.connection.server)?;
        if self.request["id"].as_str() != Some(self.snapshot.id.as_str()) || self.request["name"].as_str() != Some(self.snapshot.name.as_str()) {
            return Err("Pending household request does not match its protected snapshot.".into());
        }
        let remote = Remote { id: self.snapshot.id.clone(), name: self.snapshot.name.clone(), revision: 1,
            payload: self.request["payload"].clone(), updated_at: String::new(), role: "owner".into(), summary: String::new() };
        if client::unlock(&remote, &self.recovery_key)? != self.snapshot { return Err("Pending household contents differ from their protected request.".into()); }
        Ok(())
    }
}

/// The created household, and a warning to show when its passcode could not be stored.
fn submit_creation(mut candidate: CreationCandidate) -> Result<(SharedDocument, Option<String>), String> {
    candidate.validate()?;
    let api = Api::new(candidate.connection.clone()).map_err(|e| e.to_string())?;
    let posted = api.request("POST", "/api/budgets", Some(candidate.request)).and_then(|v| serde_json::from_value::<Remote>(v)
        .map_err(|_| client::ServiceError { status: 0, message: "Invalid household creation response.".into(), current: None }));
    // Creation is keyed by the persisted UUID. A timeout or duplicate response is recovered by
    // fetching that same identity, including after the app or network restarts.
    let remote = match posted {
        Ok(remote) => remote,
        Err(error) => api.remote(&candidate.snapshot.id).map_err(|_| format!("{error} The pending household and recovery key remain protected on this device. Use Resume pending household creation to retry."))?,
    };
    let document = SharedDocument::open(candidate.connection, candidate.recovery_key, remote)?;
    if !candidate.passcode.is_empty() {
        // The household exists and this device holds its key, so a failed passcode upload is
        // recoverable from Shared budget; the household itself opens either way.
        let id = document.working.id.clone();
        match client::wrap_key_with_passcode(&document.recovery_key, &candidate.passcode, &id)
            .and_then(|slot| Api::new(document.connection.clone()).and_then(|api| api.set_passcode(&id, &slot)).map_err(|e| e.to_string())) {
            Ok(()) => {}
            Err(e) => return Ok((document, Some(format!("{e} The household was created; set its passcode from Shared budget.")))),
        }
    }
    Ok((document, None))
}
impl Default for SharedUi {
    fn default() -> Self { Self {
        document: None, open: false, error: None,
        cache: home_dir().join(".budget-calendar/shared.enc"), cache_lock: None, server: String::new(), username: String::new(), password: String::new(),
        connection: None, budgets: vec![], key: String::new(), unlock_passcode: String::new(), unlock_use_key: false,
        passcode_a: String::new(), passcode_b: String::new(), passcode_dialog: false, name: "Household".into(), zone: "America/Halifax".into(),
        saved_key: false, creating: false, job: None, last_poll: Instant::now(), deferred: None, staged: None, preview: None,
        confirm_discard: None, discarded: vec![], discarded_for: None, recover: None,
    } }
}
impl SharedUi {
    fn ensure_cache_lock(&mut self) -> Result<(), String> {
        if self.cache_lock.is_some() { return Ok(()); }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let parent = self.cache.parent().ok_or("Invalid shared cache directory.")?;
            std::fs::create_dir_all(parent).map_err(|e| format!("Unable to create the shared cache directory: {e}"))?;
            let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).share_mode(0)
                .open(parent.join("shared-session.lock"))
                .map_err(|e| if matches!(e.raw_os_error(), Some(32 | 33)) {
                    "The shared budget cache is already open in another Windows window. Close that window before opening a shared budget here.".into()
                } else { format!("Unable to lock the shared budget cache safely: {e}") })?;
            self.cache_lock = Some(lock);
            Ok(())
        }
        #[cfg(not(windows))]
        { Err("Secure shared cache locking is available on Windows.".into()) }
    }
    fn load_cached(&mut self) -> Result<SharedDocument, String> {
        self.ensure_cache_lock()?;
        client::load_device(&self.cache)
    }
    fn load_creation(&mut self) -> Result<CreationCandidate, String> {
        self.ensure_cache_lock()?;
        let mut candidate: CreationCandidate = client::load_protected(&self.creation_path())?;
        candidate.validate()?;
        Ok(candidate)
    }
    pub fn disconnect(&mut self) {
        self.document = None; self.job = None; self.deferred = None; self.staged = None; self.preview = None; self.open = false; self.passcode_dialog = false;
        self.confirm_discard = None; self.discarded.clear(); self.discarded_for = None; self.recover = None;
    }
    pub fn status(&self, dirty: bool) -> Option<String> {
        let d = self.document.as_ref()?;
        Some(if dirty { "Shared · not saved on device".into() }
        else if d.conflict.is_some() { "Shared · conflict needs review".into() }
        else if self.error.is_some() { "Shared · sync needs attention".into() }
        else if !d.pending.is_empty() { format!("Saved on device · {} pending", d.pending.len()) }
        else { "Shared · up to date".into() })
    }
    fn spawn(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Event + Send + 'static) {
        let (send, receive) = mpsc::channel();
        self.job = Some(receive);
        let completion = ctx.clone();
        if std::thread::Builder::new().name("household-sync".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Event::Opened(Err("The shared-budget worker stopped. Retry from Shared budget.".into())));
            if send.send(result).is_ok() { completion.request_repaint(); }
        }).is_err() {
            self.job = None;
            self.error = Some("The shared-budget worker could not start. Retry shortly.".into());
            self.last_poll = Instant::now();
            ctx.request_repaint_after(POLL_INTERVAL);
        }
    }
    /// Retain a separate protected copy per household so opening another cannot erase queued work.
    fn persist(&mut self, document: &SharedDocument) -> Result<(), String> {
        self.ensure_cache_lock()?;
        let id = uuid::Uuid::parse_str(&document.working.id).map_err(|_| "Invalid shared budget identity.")?;
        let sibling = self.cache.with_file_name(format!("shared-{id}.enc"));
        client::save_device(&sibling, document)?;
        client::save_device(&self.cache, document)
    }
    fn creation_path(&self) -> PathBuf { self.cache.with_file_name("shared-creation.enc") }
    /// One file per discard, oldest first: a damaged copy cannot take the others with it.
    fn discarded_files(&self, id: &str) -> Vec<PathBuf> {
        let prefix = format!("shared-discarded-{id}-");
        let Some(entries) = self.cache.parent().and_then(|dir| std::fs::read_dir(dir).ok()) else { return vec![] };
        let mut files: Vec<_> = entries.flatten().map(|entry| entry.path())
            .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(&prefix))).collect();
        files.sort();
        files
    }
    /// Stores a protected copy of this device's changes before they are replaced.
    fn keep_discarded(&mut self, document: &SharedDocument) -> Result<PathBuf, String> {
        self.ensure_cache_lock()?;
        let id = &document.working.id;
        let now = chrono::Local::now();
        let work = DiscardedWork { id: id.clone(), discarded_at: now.format("%Y-%m-%d %H:%M").to_string(), changes: document.pending.len(),
            baseline: document.baseline.clone(), local: document.working.clone() };
        let path = self.cache.with_file_name(format!("shared-discarded-{id}-{:020}-{}.enc", now.timestamp_millis(), uuid::Uuid::new_v4()));
        client::save_protected(&path, &work)?;
        let files = self.discarded_files(id);
        for old in &files[..files.len().saturating_sub(DISCARDED_KEPT)] { let _ = std::fs::remove_file(old); }
        Ok(path)
    }
    /// Reads the open household's discarded copies once, not on every repaint.
    fn load_discarded(&mut self) {
        let Some(id) = self.document.as_ref().map(|d| d.working.id.clone()) else { return };
        if self.discarded_for.as_deref() == Some(id.as_str()) { return; }
        self.discarded.clear();
        for path in self.discarded_files(&id).into_iter().rev() {
            match client::load_protected::<DiscardedWork>(&path) {
                Ok(work) if work.id == id => self.discarded.push(Discarded { path, discarded_at: work.discarded_at, changes: work.changes }),
                _ => self.error = Some("A copy of discarded changes could not be read. It was left unchanged.".into()),
            }
        }
        self.discarded_for = Some(id);
    }
    fn persist_creation(&mut self, candidate: &CreationCandidate) -> Result<(), String> {
        self.ensure_cache_lock()?;
        let mut candidate = candidate.clone();
        candidate.validate()?;
        let path = self.creation_path();
        if path.exists() {
            let mut old: CreationCandidate = client::load_protected(&path)?;
            old.validate()?;
            if old.snapshot.id != candidate.snapshot.id || !client::same_server(&old.connection.server, &candidate.connection.server) {
                return Err("A previous household creation is pending on this device. Resume it before starting another household.".into());
            }
        }
        client::save_protected(&path, &candidate)
    }
}
impl App {
    pub(super) fn shared_editable(&mut self) -> bool {
        if self.sharing.document.as_ref().is_some_and(|d| d.conflict.is_some()) {
            self.sharing.open = true;
            self.sharing.error = Some("Review the conflicting shared version before editing.".into());
            return false;
        }
        true
    }
    pub(super) fn save_shared(&mut self) -> bool {
        let Some(mut doc) = self.sharing.document.clone() else { return false };
        let result = (|| {
            let db = self.db.as_ref().ok_or("There is no budget to save.")?;
            let s = snapshot_from_db_with_operation(db, &doc.working.id, &doc.working.name, &doc.working.time_zone,
                Some(&doc.working), &uuid::Uuid::new_v4().to_string())?;
            doc.capture(s)?;
            self.sharing.persist(&doc)
        })();
        // Even a storage failure retains the edit in memory and leaves the unsaved guard active.
        self.sharing.preview = None;
        self.sharing.document = Some(doc);
        match result {
            Ok(()) => { self.dirty = false; self.dialogs.save_error = None; self.title_dirty = true; true }
            Err(e) => { self.dirty = true; self.sharing.error = Some(e.clone()); self.dialogs.save_error = Some(e.clone()); self.fail(e); false }
        }
    }
    pub(super) fn export_shared_copy(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).set_file_name("household-copy.bgt").save_file() else { return false };
        self.export_shared_to(&with_bgt_extension(path))
    }
    fn export_shared_to(&mut self, path: &Path) -> bool {
        let Some(db) = self.db.as_ref() else { return false };
        let expected = match std::fs::read(path) { Ok(b) => Some(b), Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => { self.fail(format!("Unable to inspect export destination: {e}")); return false } };
        match document::save_checked(db, path, None, expected.as_deref()) {
            Ok(_) => { self.notify("Exported a standalone copy. Shared budget remains open."); true }
            Err(e) => { self.fail(e); false }
        }
    }
    pub(super) fn request_shared(&mut self, mut doc: SharedDocument, ctx: &egui::Context) {
        if let Err(e) = self.sharing.ensure_cache_lock() { self.sharing.error = Some(e); self.sharing.open = true; return; }
        // An explicit Open also consults that household's protected cache. A fresh server read
        // after restart must never overwrite offline changes that were saved before shutdown.
        let cached = if self.sharing.document.as_ref().is_none_or(|d| d.working.id != doc.working.id) {
            let id = match uuid::Uuid::parse_str(&doc.working.id) { Ok(id) => id, Err(_) => { self.sharing.error = Some("Invalid shared budget identity.".into()); self.sharing.open = true; return; } };
            let path = self.sharing.cache.with_file_name(format!("shared-{id}.enc"));
            if path.exists() {
                match client::load_device(&path) {
                    Ok(d) => Some(d),
                    Err(e) => { self.sharing.error = Some(format!("The existing household cache could not be opened; it was kept unchanged. {e}")); self.sharing.open = true; return; }
                }
            } else { None }
        } else { None };
        if let Some(current) = self.sharing.document.as_ref().filter(|d| d.working.id == doc.working.id).or(cached.as_ref()) {
            if !client::same_server(&current.connection.server, &doc.connection.server) {
                self.sharing.error = Some("A protected cache with this household identity belongs to another server. It has been kept unchanged.".into());
                self.sharing.open = true;
                return;
            }
            if current.working.id == doc.working.id && client::same_server(&current.connection.server, &doc.connection.server) {
                if !current.pending.is_empty() && current.connection.user.id != doc.connection.user.id {
                    self.sharing.error = Some("This device has pending edits for a different account. Open its cached budget and export a backup, or sign in with that account to synchronize it first.".into());
                    self.sharing.open = true;
                    return;
                }
                // Reopening the household must not turn a new remote baseline into a local overwrite.
                let mut retained = current.clone();
                retained.connection = doc.connection.clone();
                if doc.revision > retained.revision {
                    let remote = Remote { id: doc.working.id.clone(), name: doc.working.name.clone(), revision: doc.revision,
                        payload: match client::seal(&doc.working, &retained.recovery_key, doc.revision) { Ok(p) => p, Err(e) => { self.sharing.error = Some(e); return; } },
                        updated_at: doc.last_sync.clone(), role: doc.role.clone(), summary: "Reopened shared version".into() };
                    if let Err(e) = retained.receive(remote) { self.sharing.error = Some(e); return; }
                }
                doc = retained;
            }
        }
        if self.dirty || self.has_draft() {
            self.dialogs.pending = Some(PendingAction::Shared(Box::new(doc)));
        } else { self.perform_pending(PendingAction::Shared(Box::new(doc)), ctx); }
    }
    pub(super) fn activate_shared(&mut self, doc: SharedDocument) {
        if let Err(e) = self.sharing.ensure_cache_lock() { self.sharing.error = Some(e.clone()); self.fail(e); return; }
        let result = (|| {
            let today = client::household_today(&doc.working.time_zone)?;
            let db = snapshot_to_db(&doc.working, today, self.cur.get(), self.cfg.future_n_months)?;
            self.sharing.persist(&doc)?;
            Ok::<_, String>((today, db))
        })();
        match result {
            Ok((today, db)) => {
                let created_id = doc.working.id.clone();
                let created_server = doc.connection.server.clone();
                self.opening = None; self.auto_open = None;
                self.sharing.disconnect();
                self.sharing.server = doc.connection.server.clone(); self.sharing.username = doc.connection.user.username.clone();
                self.sharing.connection = Some(doc.connection.clone()); self.sharing.document = Some(doc);
                self.sharing.key.clear(); self.sharing.password.clear(); self.sharing.saved_key = false; self.sharing.creating = false;
                self.sharing.error = None;
                self.file = None; self.saved_bytes = None; self.password = None;
                self.cfg.last_file_name = None;
                self.db = Some(db); self.today = today; self.dirty = false;
                self.dialogs = Dialogs::default(); self.payment_list.selected = None;
                self.generation += 1; self.month = None; self.view = None; self.haystacks = None; self.title_dirty = true;
                let candidate_path = self.sharing.creation_path();
                if let Ok(candidate) = client::load_protected::<CreationCandidate>(&candidate_path) {
                    if candidate.snapshot.id == created_id && client::same_server(&candidate.connection.server, &created_server) {
                        // The normal protected document is durable before its recovery candidate is removed.
                        let _ = std::fs::remove_file(candidate_path);
                    }
                }
                self.notify("Shared budget opened. Applied changes save to this device automatically.");
            }
            Err(e) => self.fail(e),
        }
    }
    fn shared_can_apply(&self, ctx: &egui::Context) -> bool {
        !self.has_modal() && !self.has_draft() && !self.dirty && self.opening.is_none() && !reports::is_open(ctx)
            && !egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(ctx)
    }
    fn refresh_shared_db(&mut self) -> Result<(), String> {
        let doc = self.sharing.document.as_ref().ok_or("No shared budget is open.")?;
        let today = client::household_today(&doc.working.time_zone)?;
        let db = snapshot_to_db(&doc.working, today, self.cur.get(), self.cfg.future_n_months)?;
        self.db = Some(db); self.today = today; self.generation += 1;
        self.month = None; self.view = None; self.haystacks = None; self.title_dirty = true;
        Ok(())
    }
    pub(super) fn poll_shared(&mut self, ctx: &egui::Context) {
        let event = self.sharing.job.as_ref().and_then(|r| match r.try_recv() {
            Ok(v) => Some(v), Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => { Some(Event::Opened(Err("The shared-budget worker stopped. Retry from Shared budget.".into()))) }
        });
        if let Some(event) = event {
            self.sharing.job = None;
            self.sharing.last_poll = Instant::now();
            match event {
                Event::SignedIn(result) => match result {
                    Ok((connection, budgets)) => {
                        // Reauthentication changes credentials only, retaining every queued request.
                        let mut changed = self.sharing.document.clone();
                        if let Some(d) = &mut changed {
                            if client::same_server(&d.connection.server, &connection.server) && d.connection.user.id == connection.user.id {
                                d.connection = connection.clone();
                                match self.sharing.persist(d) { Ok(()) => self.sharing.document = changed, Err(e) => self.sharing.error = Some(e) }
                            }
                        }
                        self.sharing.connection = Some(connection); self.sharing.budgets = budgets;
                    }
                    Err(e) => self.sharing.error = Some(e),
                },
                Event::Opened(result) => match result {
                    Ok((doc, warning)) => {
                        if let Some(warning) = warning { self.sharing.error = Some(warning); }
                        self.sharing.staged = Some(doc);
                    }
                    Err(e) => self.sharing.error = Some(e),
                },
                Event::Passcode(result) => match result {
                    Ok(message) => { self.sharing.error = None; self.notify(message); }
                    Err(e) => self.sharing.error = Some(e),
                },
                Event::Sync { id, operation, result } => {
                    if self.sharing.document.as_ref().is_some_and(|d| d.working.id == id) {
                        match result {
                            Ok(remote) => {
                                if let Some(operation) = operation {
                                    let mut current = self.sharing.document.clone().unwrap();
                                    let result = current.acknowledge(remote, &operation).and_then(|_| self.sharing.persist(&current));
                                    // A receipt is applied to CURRENT state; edits made during POST survive.
                                    self.sharing.preview = None;
                                    self.sharing.document = Some(current);
                                    match result { Ok(()) => self.sharing.error = None, Err(e) => { self.dirty = true; self.sharing.error = Some(e); } }
                                } else {
                                    // The ordinary GET usually returns the revision we already have.
                                    // Drop it before cloning, decrypting or flushing either device cache,
                                    // including while a modal keeps newer revisions deferred.
                                    if self.sharing.document.as_ref().is_some_and(|d| remote.revision > d.revision)
                                        && self.sharing.deferred.as_ref().is_none_or(|old| remote.revision > old.revision) {
                                        self.sharing.deferred = Some(remote);
                                    }
                                    self.sharing.error = None;
                                }
                            }
                            Err(e) => {
                                if let Some(remote) = e.current { self.sharing.deferred = Some(remote); }
                                self.sharing.error = Some(if e.status == 401 { "Session expired. Sign in again; pending changes are kept.".into() } else { e.message });
                            }
                        }
                    }
                }
            }
        }
        if let Some(doc) = self.sharing.staged.take() {
            self.sharing.open = false;
            self.request_shared(doc, ctx);
        }
        if self.shared_can_apply(ctx) {
            if let Some(remote) = self.sharing.deferred.take() {
                if self.sharing.document.as_ref().is_some_and(|d| remote.revision > d.revision) {
                    let mut current = self.sharing.document.clone().unwrap();
                    let result = current.receive(remote).and_then(|changed| { self.sharing.persist(&current)?; Ok(changed) });
                    match result {
                        Ok(changed) => {
                            let conflict = current.conflict.is_some();
                            self.sharing.preview = None;
                            self.sharing.document = Some(current);
                            if changed { if let Err(e) = self.refresh_shared_db() { self.sharing.error = Some(e); } }
                            if conflict { self.sharing.open = true; }
                        }
                        Err(e) => self.sharing.error = Some(e),
                    }
                }
            }
        }
        // Workers wake the window on completion. An idle household needs just its next
        // deadline, not four redraws per second (or a full document clone on every GET).
        if self.sharing.job.is_some() || self.dirty { return; }
        let Some(current) = self.sharing.document.as_ref() else { return };
        if current.conflict.is_some() { return; }
        let remaining = POLL_INTERVAL.saturating_sub(self.sharing.last_poll.elapsed());
        if !remaining.is_zero() { ctx.request_repaint_after(remaining); return; }
        let id = current.working.id.clone(); let connection = current.connection.clone();
        let prepared = if let Some(first) = current.pending.first() {
            if let Some(request) = &first.request {
                // A saved immutable request may be retried without re-encryption or another flush.
                Some(request.clone())
            } else {
                let mut current = current.clone();
                match current.prepare().and_then(|request| { self.sharing.persist(&current)?; Ok(request) }) {
                    Ok(request) => { self.sharing.document = Some(current); request }
                    Err(e) => {
                        self.sharing.error = Some(e); self.sharing.last_poll = Instant::now();
                        ctx.request_repaint_after(POLL_INTERVAL); return;
                    }
                }
            }
        } else { None };
        self.sharing.last_poll = Instant::now();
        self.sharing.spawn(ctx, move || {
            let operation = prepared.as_ref().and_then(|v| v["operationId"].as_str()).map(str::to_string);
            let result = Api::new(connection).and_then(|api| if let Some(body) = prepared {
                api.request("POST", &format!("/api/budgets/{id}/commits"), Some(body)).and_then(|v| serde_json::from_value(v).map_err(|_| client::ServiceError { status: 0, message: "Invalid commit receipt.".into(), current: None }))
            } else { api.remote(&id) });
            Event::Sync { id, operation, result }
        });
    }
    pub(super) fn shared_panel(&mut self, ctx: &egui::Context) {
        if !self.sharing.open { return; }
        self.sharing.load_discarded();
        let command = self.shared_panel_contents(ctx);
        match command {
            Some("copy-key") => {
                if let Some(doc) = &self.sharing.document {
                    ctx.copy_text(doc.recovery_key.clone());
                    self.notify("Recovery key copied.");
                }
            }
            Some("login") => {
                let server = self.sharing.server.clone(); let username = self.sharing.username.clone(); let password = std::mem::take(&mut self.sharing.password);
                self.sharing.error = None;
                self.sharing.spawn(ctx, move || Event::SignedIn((|| {
                    let connection = Api::login(&server, &username, &password).map_err(|e| e.to_string())?;
                    let list = Api::new(connection.clone()).map_err(|e| e.to_string())?.request("GET", "/api/budgets", None).map_err(|e| e.to_string())?;
                    Ok((connection, list["budgets"].as_array().cloned().unwrap_or_default()))
                })()));
            }
            Some("open") => {
                if let Err(e) = self.sharing.ensure_cache_lock() { self.sharing.error = Some(e); return; }
                let connection = self.sharing.connection.clone().unwrap(); let id = self.sharing.name.clone();
                let use_key = self.sharing.unlock_use_key;
                let key = self.sharing.key.trim().to_string();
                let passcode = self.sharing.unlock_passcode.trim().to_string();
                self.sharing.spawn(ctx, move || Event::Opened((|| {
                    let api = Api::new(connection.clone()).map_err(|e| e.to_string())?;
                    let remote = api.remote(&id).map_err(|e| e.to_string())?;
                    let key_text = if use_key { key } else {
                        let slot = api.passcode_slot(&id).map_err(|e| e.to_string())?
                            .ok_or("This household has no passcode yet. Ask its owner to set one, or unlock with its recovery key.")?;
                        client::unlock_with_passcode(&slot, &passcode, &id)?
                    };
                    Ok((SharedDocument::open(connection, key_text, remote)?, None))
                })()));
            }
            Some("cache") => {
                match self.sharing.load_cached() {
                    Ok(doc) => { self.sharing.open = false; self.request_shared(doc, ctx); }
                    Err(e) => self.sharing.error = Some(e),
                }
            }
            Some("prepare") => { self.sharing.creating = true; self.sharing.key = client::generate_recovery_key(); self.sharing.name = self.file.as_deref().map(file_name).unwrap_or("Household".into()); self.sharing.saved_key = false; }
            Some("create") => {
                let result = self.db.as_ref().ok_or("No budget is open.".into()).and_then(|db| {
                    client::household_today(&self.sharing.zone)?;
                    snapshot_from_db_with_operation(db, &uuid::Uuid::new_v4().to_string(), &self.sharing.name, &self.sharing.zone, None, &uuid::Uuid::new_v4().to_string())
                });
                match result {
                    Err(e) => self.sharing.error = Some(e),
                    Ok(snapshot) => {
                        let connection = self.sharing.connection.clone().unwrap(); let key = self.sharing.key.clone();
                        let passcode = self.sharing.passcode_a.trim().to_string();
                        if passcode != self.sharing.passcode_b.trim() { self.sharing.error = Some("The two passcodes do not match.".into()); return; }
                        if let Err(e) = budget_core::passcode::check_passcode(&passcode) { self.sharing.error = Some(e); return; }
                        // The explicit upload button is the only creation path.
                        match CreationCandidate::new(connection, key, passcode, snapshot).and_then(|candidate| { self.sharing.persist_creation(&candidate)?; Ok(candidate) }) {
                            Ok(candidate) => self.sharing.spawn(ctx, move || Event::Opened(submit_creation(candidate))),
                            Err(e) => self.sharing.error = Some(e),
                        }
                    }
                }
            }
            Some("resume-create") => {
                let result = (|| {
                    let mut candidate = self.sharing.load_creation()?;
                    if let Some(connection) = &self.sharing.connection {
                        if client::same_server(&candidate.connection.server, &connection.server) && candidate.connection.user.id == connection.user.id {
                            candidate.connection = connection.clone();
                        } else { return Err("Sign in with the account and server used for the pending household creation.".into()); }
                    }
                    self.sharing.persist_creation(&candidate)?;
                    Ok::<_, String>(candidate)
                })();
                match result {
                    Ok(candidate) => {
                        self.sharing.key = candidate.recovery_key.clone(); self.sharing.server = candidate.connection.server.clone();
                        self.sharing.username = candidate.connection.user.username.clone(); self.sharing.error = None;
                        self.sharing.spawn(ctx, move || Event::Opened(submit_creation(candidate)));
                    }
                    Err(e) => self.sharing.error = Some(e),
                }
            }
            Some("passcode-cancel") => {}
            Some("passcode-open") => {
                self.sharing.passcode_a.clear(); self.sharing.passcode_b.clear(); self.sharing.passcode_dialog = true;
            }
            Some("passcode-save") => {
                self.sharing.passcode_dialog = false;
                let Some(doc) = self.sharing.document.clone() else { return };
                let passcode = self.sharing.passcode_a.trim().to_string();
                if passcode != self.sharing.passcode_b.trim() { self.sharing.error = Some("The two passcodes do not match.".into()); return; }
                match budget_core::passcode::check_passcode(&passcode).and_then(|()| client::wrap_key_with_passcode(&doc.recovery_key, &passcode, &doc.working.id)) {
                    Ok(slot) => {
                        let id = doc.working.id.clone();
                        self.sharing.spawn(ctx, move || Event::Passcode((|| {
                            Api::new(doc.connection.clone()).map_err(|e| e.to_string())?
                                .set_passcode(&id, &slot).map_err(|e| e.to_string())?;
                            Ok("Household passcode saved. Members can unlock new devices with it.".to_string())
                        })()));
                    }
                    Err(e) => self.sharing.error = Some(e),
                }
            }
            Some("passcode-remove") => {
                let Some(doc) = self.sharing.document.clone() else { return };
                let id = doc.working.id.clone();
                self.sharing.spawn(ctx, move || Event::Passcode((|| {
                    Api::new(doc.connection.clone()).map_err(|e| e.to_string())?
                        .remove_passcode(&id).map_err(|e| e.to_string())?;
                    Ok("Household passcode removed. Keep the recovery key safe.".to_string())
                })()));
            }
            Some("sync") => self.sharing.last_poll = Instant::now() - Duration::from_secs(5),
            Some("export") => { self.export_shared_copy(); }
            Some(choice @ ("mine" | "theirs")) => self.resolve_shared(choice == "mine"),
            Some("discard") => self.discard_shared(),
            Some("keep-discard") => self.sharing.confirm_discard = None,
            Some("recover") => if let Some(index) = self.sharing.recover.take() { self.recover_discarded(index); },
            _ => {}
        }
    }
    /// An edit the closed queue refused is still in the budget. Queue it, so it counts as one of
    /// this device's changes and the failed save that flagged it cannot block the resolution.
    fn queue_stranded_edit(&mut self) -> Result<(), String> {
        if !self.dirty { return Ok(()); }
        let Some(mut doc) = self.sharing.document.clone() else { return Ok(()) };
        let db = self.db.as_ref().ok_or("There is no budget to save.")?;
        let snapshot = snapshot_from_db_with_operation(db, &doc.working.id, &doc.working.name, &doc.working.time_zone,
            Some(&doc.working), &uuid::Uuid::new_v4().to_string())?;
        doc.capture_unresolved(snapshot)?;
        self.sharing.persist(&doc)?;
        self.sharing.document = Some(doc);
        self.sharing.preview = None;
        if self.banner.as_ref().is_some_and(|banner| self.dialogs.save_error.as_deref() == Some(banner.text.as_str())) { self.banner = None; }
        self.dirty = false; self.dialogs.save_error = None; self.title_dirty = true;
        Ok(())
    }
    fn resolve_shared(&mut self, keep_local: bool) {
        if self.has_draft() { self.sharing.error = Some("Finish your open editor before resolving the shared conflict.".into()); return; }
        if let Err(e) = self.queue_stranded_edit() { self.sharing.error = Some(e); return; }
        let Some(doc) = self.sharing.document.clone() else { return };
        // Replacing this device's changes asks first; the discard itself keeps a copy.
        if !keep_local && !doc.pending.is_empty() { self.sharing.confirm_discard = Some(doc.pending.len()); return; }
        if let Err(e) = self.apply_resolution(doc, keep_local) { self.sharing.error = Some(e); }
    }
    fn apply_resolution(&mut self, mut doc: SharedDocument, keep_local: bool) -> Result<(), String> {
        doc.resolve(keep_local).and_then(|_| self.sharing.persist(&doc))?;
        self.sharing.document = Some(doc);
        self.sharing.confirm_discard = None;
        self.refresh_shared_db()?;
        self.sharing.error = None; self.sharing.open = false;
        Ok(())
    }
    fn discard_shared(&mut self) {
        self.sharing.confirm_discard = None;
        if self.has_draft() || self.dirty { self.sharing.error = Some("Finish your open editor before resolving the shared conflict.".into()); return; }
        let Some(doc) = self.sharing.document.clone() else { return };
        // Nothing is replaced until its copy is on disk.
        let copy = match self.sharing.keep_discarded(&doc) { Ok(copy) => copy, Err(e) => { self.sharing.error = Some(e); return; } };
        match self.apply_resolution(doc, false) {
            Ok(()) => self.notify("Using the current shared version. A copy of your discarded changes is kept under Shared budget."),
            Err(e) => {
                if self.sharing.document.as_ref().is_some_and(|d| d.conflict.is_some()) { let _ = std::fs::remove_file(copy); }
                self.sharing.error = Some(e);
            }
        }
        self.sharing.discarded_for = None;
    }
    /// Reapplies a discarded copy to the current shared version, as new queued changes.
    fn recover_discarded(&mut self, index: usize) {
        let result = (|| {
            if self.has_draft() || self.dirty { return Err("Finish your open editor before getting these changes back.".to_string()); }
            let mut doc = self.sharing.document.clone().ok_or("No shared budget is open.")?;
            if doc.conflict.is_some() { return Err("Resolve the conflict before getting these changes back.".into()); }
            let path = self.sharing.discarded.get(index).ok_or("That copy is no longer available.")?.path.clone();
            let work: DiscardedWork = client::load_protected(&path)?;
            if work.id != doc.working.id { return Err("That copy belongs to another household.".into()); }
            doc.capture(client::merge_snapshots(&work.baseline, &work.local, &doc.working, true)?)?;
            self.sharing.persist(&doc)?;
            self.sharing.document = Some(doc);
            // The changes are durable in the queue, so the copy has done its job.
            let _ = std::fs::remove_file(path);
            self.refresh_shared_db()
        })();
        self.sharing.discarded_for = None;
        match result {
            Ok(()) => { self.sharing.error = None; self.notify("Your changes are back and queued for household sync."); }
            Err(e) => self.sharing.error = Some(e),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (App, PathBuf) {
        let (mut app, dir) = crate::lifecycle_tests::fixture();
        app.sharing.cache = dir.join("shared.enc");
        let s = snapshot_from_db_with_operation(app.db.as_ref().unwrap(), &uuid::Uuid::new_v4().to_string(), "Fixture household", "America/Halifax", None, &uuid::Uuid::new_v4().to_string()).unwrap();
        let key = client::generate_recovery_key();
        let doc = SharedDocument::open(Connection { server: "http://127.0.0.1:8787".into(), token: "fixture-session".into(), user: client::User { id: "fixture".into(), username: "fixture".into() } }, key.clone(), remote(&s, &key, 1)).unwrap();
        app.activate_shared(doc);
        assert!(app.sharing.document.is_some(), "{:?}", app.banner.as_ref().map(|b| &b.text));
        (app, dir)
    }
    fn remote(s: &budget_core::sharing::Snapshot, key: &str, revision: u64) -> Remote {
        Remote { id: s.id.clone(), name: s.name.clone(), revision, payload: client::seal(s, key, revision).unwrap(), updated_at: format!("fixture-{revision}"), role: "owner".into(), summary: "Fixture update".into() }
    }
    fn deliver(app: &mut App, event: Event) {
        let (send, recv) = mpsc::channel(); send.send(event).unwrap();
        app.sharing.job = Some(recv);
        app.poll_shared(&egui::Context::default());
    }
    #[cfg(windows)]
    #[test]
    fn idle_poll_does_not_touch_the_protected_cache() {
        use std::os::windows::fs::OpenOptionsExt;
        let (mut app, _dir) = setup();
        let before = std::fs::read(&app.sharing.cache).unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&app.sharing.cache).unwrap();
        // Invalid local fixture origin makes the background worker finish without network I/O.
        app.sharing.document.as_mut().unwrap().connection.server = "fixture-not-a-url".into();
        app.sharing.last_poll = Instant::now() - Duration::from_secs(6);
        app.poll_shared(&egui::Context::default());
        assert!(app.sharing.error.is_none(), "an idle read must not try to rewrite a locked cache: {:?}", app.sharing.error);
        assert!(app.sharing.job.is_some(), "the background read must be admitted without cache writes");
        assert_eq!(std::fs::read(&app.sharing.cache).unwrap(), before);
        drop(held);
    }
    #[cfg(windows)]
    #[test]
    fn unchanged_receipt_does_not_write_or_defer_a_budget() {
        use std::os::windows::fs::OpenOptionsExt;
        let (mut app, _dir) = setup();
        let generation = app.generation;
        let doc = app.sharing.document.as_ref().unwrap();
        let same = remote(&doc.working, &doc.recovery_key, doc.revision);
        let id = doc.working.id.clone();
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&app.sharing.cache).unwrap();
        deliver(&mut app, Event::Sync { id:id.clone(), operation:None, result:Ok(same.clone()) });
        assert!(app.sharing.error.is_none(), "unchanged data must not be persisted: {:?}", app.sharing.error);
        assert_eq!(app.generation, generation);
        app.sharing.open = true;
        deliver(&mut app, Event::Sync { id, operation:None, result:Ok(same) });
        assert!(app.sharing.deferred.is_none(), "an unchanged full payload must not wait behind the dialog");
        drop(held);
    }
    #[test]
    fn idle_shared_window_sleeps_until_its_next_poll_deadline() {
        let (mut app, _dir) = setup();
        let ctx = egui::Context::default();
        let mut output = ctx.run(egui::RawInput::default(), |ctx| app.poll_shared(ctx));
        for _ in 0..3 { output = ctx.run(egui::RawInput::default(), |ctx| app.poll_shared(ctx)); }
        assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(4),
            "idle sync must not redraw four times each second: {:?}", output.viewport_output[&egui::ViewportId::ROOT].repaint_delay);
    }
    #[test]
    fn completion_wakes_a_shared_window_without_periodic_redraws() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let mut sharing = SharedUi::default();
        let ctx = egui::Context::default();
        for _ in 0..3 { let _ = ctx.run(egui::RawInput::default(), |_| {}); }
        let awakened = Arc::new(AtomicBool::new(false));
        let observed = awakened.clone();
        ctx.set_request_repaint_callback(move |_| { observed.store(true, Ordering::SeqCst); });
        sharing.spawn(&ctx, || Event::Opened(Err("generated fixture result".into())));
        let deadline = Instant::now() + Duration::from_millis(250);
        while !awakened.load(Ordering::SeqCst) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(1)); }
        assert!(awakened.load(Ordering::SeqCst), "the completed worker must wake an otherwise idle window");
    }
    #[test]
    fn newer_conflict_is_still_durable_and_never_replaces_local_edits() {
        let (mut app, _dir) = setup();
        app.db.as_mut().unwrap().notes = "Saved local fixture edit".into();
        app.changed();
        let doc = app.sharing.document.as_ref().unwrap();
        let mut newer = doc.baseline.clone(); newer.notes = "Other device fixture edit".into();
        let receipt = remote(&newer, &doc.recovery_key, 2);
        let id = doc.working.id.clone();
        deliver(&mut app, Event::Sync { id, operation:None, result:Ok(receipt) });
        let persisted = client::load_device(&app.sharing.cache).unwrap();
        assert_eq!(persisted.working.notes, "Saved local fixture edit");
        assert_eq!(persisted.pending.len(), 1);
        assert_eq!(persisted.conflict.as_ref().unwrap().revision, 2);
        assert!(app.sharing.open && !app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, "Saved local fixture edit");
    }
    #[cfg(windows)]
    #[test]
    fn durable_commit_retry_does_not_rewrite_or_change_its_request() {
        use std::os::windows::fs::OpenOptionsExt;
        let (mut app, _dir) = setup();
        app.db.as_mut().unwrap().notes = "Queued fixture edit".into(); app.changed();
        let mut doc = app.sharing.document.clone().unwrap();
        doc.connection.server = "fixture-not-a-url".into();
        let exact = doc.prepare().unwrap().unwrap();
        app.sharing.persist(&doc).unwrap();
        app.sharing.document = Some(doc);
        let before = std::fs::read(&app.sharing.cache).unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&app.sharing.cache).unwrap();
        app.sharing.last_poll = Instant::now() - Duration::from_secs(6);
        app.poll_shared(&egui::Context::default());
        assert!(app.sharing.error.is_none(), "a durable retry must not rewrite its request: {:?}", app.sharing.error);
        assert!(app.sharing.job.is_some());
        assert_eq!(app.sharing.document.as_ref().unwrap().pending[0].request.as_ref(), Some(&exact));
        assert_eq!(std::fs::read(&app.sharing.cache).unwrap(), before);
        drop(held);
    }
    #[test]
    fn sharing_links_open_only_canonical_origins_in_every_panel_state() {
        fn click_link(app: &mut App, label: &str) -> Option<String> {
            fn find_text(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
                match shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos + text.galley.mesh_bounds.center().to_vec2()),
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find_text(shape, label)),
                    _ => None,
                }
            }
            let ctx = egui::Context::default();
            theme::install_fonts(&ctx); theme::apply(&ctx, &app.t);
            let frame = |app: &mut App, events| ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 1000.0))),
                events, ..Default::default()
            }, |ctx| app.shared_panel(ctx));
            let mut output = frame(app, vec![]);
            for _ in 0..2 { output = frame(app, vec![]); }
            let pointer = output.shapes.iter().find_map(|shape| find_text(&shape.shape, label))?;
            let mut opened = None;
            for pressed in [true, false] {
                output = frame(app, vec![egui::Event::PointerMoved(pointer), egui::Event::PointerButton {
                    pos: pointer, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE
                }]);
                for command in output.platform_output.commands {
                    if let egui::OutputCommand::OpenUrl(url) = command { opened = Some(url.url); }
                }
            }
            opened
        }
        let (mut app, _dir) = setup(); app.sharing.open = true;
        for (raw, expected) in [
            (" HTTPS://Example.COM:443/ ", Some("https://example.com")),
            ("HTTP://LOCALHOST:8787/", Some("http://localhost:8787")),
            ("https://example.com/&calc.exe", None),
            ("https://example.com/|calc.exe", None),
            ("https://example.com/%PATH%", None),
            ("https://example.com/\"&calc.exe", None),
            ("https://example.com\0", None),
            ("https://exam\nple.com", None),
            ("https://user@example.com", None),
            ("file:///C:/Windows/System32/calc.exe", None),
        ] {
            for state in 0..3 {
                let doc = app.sharing.document.clone(); let connection = app.sharing.connection.clone();
                if state == 0 { app.sharing.document.as_mut().unwrap().connection.server = raw.into(); }
                else if state == 1 { app.sharing.document = None; app.sharing.connection.as_mut().unwrap().server = raw.into(); }
                else { app.sharing.document = None; app.sharing.connection = None; app.sharing.server = raw.into(); }
                let label = match state {
                    0 => "Open mobile app · invitations, members and history",
                    1 => "Open mobile app to set up an account or accept an invitation",
                    _ => "Open mobile app for account setup / invitation",
                };
                assert_eq!(click_link(&mut app, label).as_deref(), expected, "panel state {state}, server {raw:?}");
                app.sharing.document = doc; app.sharing.connection = connection;
            }
        }
    }
    #[test]
    fn pending_creation_normalizes_reopened_origins_before_identity_checks() {
        let (mut app, _dir) = setup();
        let mut candidate = creation(&app);
        candidate.connection.server = " HTTPS://Example.COM:443/ ".into();
        client::save_protected(&app.sharing.creation_path(), &candidate).unwrap();
        let reopened = app.sharing.load_creation().unwrap();
        assert_eq!(reopened.connection.server, "https://example.com");
        candidate.connection.server = "https://example.com".into();
        app.sharing.persist_creation(&candidate).unwrap();
        assert_eq!(app.sharing.load_creation().unwrap().connection.server, "https://example.com");
    }
    #[test]
    fn session_refresh_matches_canonical_origins_and_retains_pending_work() {
        let (mut app, _dir) = setup();
        app.db.as_mut().unwrap().notes = "Pending offline work".into(); app.changed();
        let mut connection = app.sharing.document.as_ref().unwrap().connection.clone();
        connection.server = "HTTP://127.0.0.1:8787/".into();
        connection.token = "refreshed-fixture-session".into();
        deliver(&mut app, Event::SignedIn(Ok((connection.clone(), vec![]))));
        let stored = client::load_device(&app.sharing.cache).unwrap();
        assert_eq!(stored.connection.token, "refreshed-fixture-session");
        assert_eq!(stored.connection.server, "http://127.0.0.1:8787");
        assert_eq!(stored.working.notes, "Pending offline work");
        assert_eq!(stored.pending.len(), 1);
        connection.server = "https://another.example".into(); connection.token = "foreign-fixture-session".into();
        deliver(&mut app, Event::SignedIn(Ok((connection, vec![]))));
        let stored = client::load_device(&app.sharing.cache).unwrap();
        assert_eq!(stored.connection.token, "refreshed-fixture-session");
        assert_eq!(stored.pending.len(), 1);
        assert_eq!(stored.working.notes, "Pending offline work");
    }
    #[test]
    fn recovery_key_copy_is_explicit_uses_active_household_and_preserves_budget() {
        let (mut app, dir) = setup();
        app.db.as_mut().unwrap().notes = "Pending fixture work".into(); app.changed();
        let key = app.sharing.document.as_ref().unwrap().recovery_key.clone();
        let before = serde_json::to_value(app.sharing.document.as_ref().unwrap()).unwrap();
        let cache_before = std::fs::read(dir.join("shared.enc")).unwrap();
        // The sign-in/unlock fields can refer to a different household or account.
        // Copy must always use the identity shown in the active document section.
        app.sharing.key = "unrelated-unlock-key".into();
        app.sharing.connection.as_mut().unwrap().user.id = "another-account".into();
        app.sharing.open = true;
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        let frame = |app: &mut App, events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 700.0))),
            events, ..Default::default()
        }, |ctx| app.shared_panel(ctx));
        fn text_shapes(output: &egui::FullOutput) -> Vec<egui::epaint::TextShape> {
            fn visit(shape: &egui::Shape, out: &mut Vec<egui::epaint::TextShape>) {
                match shape {
                    egui::Shape::Text(text) => out.push(text.clone()),
                    egui::Shape::Vec(shapes) => for shape in shapes { visit(shape, out); },
                    _ => {}
                }
            }
            let mut out = Vec::new();
            for shape in &output.shapes { visit(&shape.shape, &mut out); }
            out
        }
        let mut output = frame(&mut app, vec![]);
        for _ in 0..2 {
            assert!(output.platform_output.commands.iter().all(|command| !matches!(command, egui::OutputCommand::CopyText(_))),
                "opening the panel must not overwrite the clipboard");
            output = frame(&mut app, vec![]);
        }
        let texts = text_shapes(&output);
        assert!(texts.iter().all(|text| !text.galley.job.text.contains(&key)), "the recovery key must not be displayed");
        let button = texts.iter().find(|text| text.galley.job.text == "Copy recovery key")
            .expect("an open shared budget must offer recovery-key retrieval");
        let pointer = button.pos + button.galley.mesh_bounds.center().to_vec2();
        let mut copied = Vec::new();
        for pressed in [true, false] {
            output = frame(&mut app, vec![egui::Event::PointerMoved(pointer), egui::Event::PointerButton {
                pos: pointer, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE
            }]);
            copied.extend(output.platform_output.commands.iter().filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()), _ => None
            }));
            assert!(text_shapes(&output).iter().all(|text| !text.galley.job.text.contains(&key)));
        }
        assert_eq!(copied, vec![key.clone()], "an explicit click must copy only the active household's key");
        assert!(app.toast.as_ref().is_some_and(|toast| toast.text == "Recovery key copied."));
        assert_eq!(serde_json::to_value(app.sharing.document.as_ref().unwrap()).unwrap(), before);
        assert_eq!(std::fs::read(dir.join("shared.enc")).unwrap(), cache_before);
        assert_eq!(app.sharing.key, "unrelated-unlock-key");
        app.close_budget();
        app.sharing.open = true;
        output = frame(&mut app, vec![]);
        assert!(text_shapes(&output).iter().all(|text| text.galley.job.text != "Copy recovery key" && !text.galley.job.text.contains(&key)),
            "closing the household must remove its recovery-key control");
        assert!(output.platform_output.commands.iter().all(|command| !matches!(command, egui::OutputCommand::CopyText(_))));
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn applied_changes_are_protected_before_saved_and_stable_ids_survive_edits() {
        let (mut app, dir) = setup();
        let before = app.sharing.document.as_ref().unwrap().working.payments[0].id.clone();
        app.db.as_mut().unwrap().payments[0].description = "Changed fixture".into(); app.changed();
        assert!(!app.dirty);
        let stored = client::load_device(&dir.join("shared.enc")).unwrap();
        assert_eq!(stored.pending.len(), 1);
        assert_eq!(stored.working.payments[0].id, before);
        assert_eq!(stored.working.payments[0].payment.description, "Changed fixture");
        assert!(!String::from_utf8_lossy(&std::fs::read(dir.join("shared.enc")).unwrap()).contains("fixture-session"));
        app.close_budget();
        assert!(client::load_device(&dir.join("shared.enc")).unwrap().pending.len() == 1);
    }
    #[test]
    fn cache_failure_leaves_unsaved_guard_and_in_memory_pending_work() {
        let (mut app, dir) = setup();
        let blocker = dir.join("not-a-directory"); std::fs::write(&blocker, b"fixture").unwrap();
        app.sharing.cache = blocker.join("shared.enc");
        app.db.as_mut().unwrap().notes = "Must remain in memory".into(); app.changed();
        assert!(app.dirty && app.dialogs.save_error.is_some());
        assert_eq!(app.sharing.document.as_ref().unwrap().working.notes, "Must remain in memory");
        assert_eq!(app.sharing.document.as_ref().unwrap().pending.len(), 1);
        app.request_close_budget();
        assert!(matches!(app.dialogs.pending, Some(PendingAction::CloseBudget)));
        app.sharing.cache = dir.join("shared.enc"); assert!(app.save_shared()); assert!(!app.dirty);
    }
    #[test]
    fn shared_save_cannot_switch_files_and_export_keeps_mode_and_queue() {
        let (mut app, dir) = setup();
        app.db.as_mut().unwrap().notes = "Queued".into(); app.changed();
        let path = dir.join("copy.bgt");
        assert!(!app.save_to(&path, None)); assert!(!path.exists());
        assert!(app.export_shared_to(&path));
        assert!(app.file.is_none() && app.sharing.document.is_some());
        assert_eq!(app.sharing.document.as_ref().unwrap().pending.len(), 1);
        assert!(path.exists());
    }
    #[test]
    fn opening_shared_budget_preserves_an_unapplied_draft() {
        let (mut app, _dir) = setup(); let doc = app.sharing.document.clone().unwrap();
        app.new_payment(20261029);
        app.request_shared(doc, &egui::Context::default());
        assert!(app.dialogs.payment_editor.is_some());
        assert!(matches!(app.dialogs.pending, Some(PendingAction::Shared(_))));
    }
    #[test]
    fn receipt_applies_to_current_document_and_keeps_edits_made_during_post() {
        let (mut app, _dir) = setup();
        app.db.as_mut().unwrap().notes = "First".into(); app.changed();
        let mut doc = app.sharing.document.clone().unwrap();
        let request = doc.prepare().unwrap().unwrap();
        let receipt = remote(&doc.working, &doc.recovery_key, 2);
        let id = doc.working.id.clone(); app.sharing.document = Some(doc);
        app.db.as_mut().unwrap().notes = "Second".into(); app.changed();
        deliver(&mut app, Event::Sync { id, operation: Some(request["operationId"].as_str().unwrap().into()), result: Ok(receipt) });
        let current = app.sharing.document.as_ref().unwrap();
        assert_eq!(current.revision, 2); assert_eq!(current.working.notes, "Second");
        assert_eq!(current.pending.len(), 1);
        assert_eq!(client::load_device(&app.sharing.cache).unwrap().working.notes, "Second");
    }
    #[test]
    fn remote_update_waits_for_draft_and_is_applied_afterward() {
        let (mut app, _dir) = setup();
        let doc = app.sharing.document.as_ref().unwrap(); let mut next = doc.working.clone(); next.notes = "Remote".into();
        let r = remote(&next, &doc.recovery_key, 2); let id = doc.working.id.clone();
        app.new_payment(20261029);
        deliver(&mut app, Event::Sync { id, operation: None, result: Ok(r) });
        assert!(app.sharing.deferred.is_some()); assert!(app.dialogs.payment_editor.is_some());
        assert_ne!(app.db.as_ref().unwrap().notes, "Remote");
        app.dialogs.payment_editor = None; app.poll_shared(&egui::Context::default());
        assert_eq!(app.db.as_ref().unwrap().notes, "Remote"); assert!(app.sharing.deferred.is_none());
    }
    #[test]
    fn conflict_and_reopening_never_replace_queued_edits() {
        let (mut app, _dir) = setup(); let mut fresh = app.sharing.document.clone().unwrap();
        app.db.as_mut().unwrap().notes = "My offline change".into(); app.changed();
        fresh.working.name = "Remote household name".into(); fresh.baseline = fresh.working.clone(); fresh.revision = 2;
        app.request_shared(fresh, &egui::Context::default());
        let doc = app.sharing.document.as_ref().unwrap();
        assert_eq!(doc.working.notes, "My offline change"); assert_eq!(doc.pending.len(), 1); assert!(doc.conflict.is_some());
        assert!(!app.shared_editable()); assert!(app.sharing.open);
    }
    /// Two devices edited, so the household waits for a choice. Returns the other device's notes.
    fn conflicted(app: &mut App) -> String {
        app.db.as_mut().unwrap().notes = "My offline change".into(); app.changed();
        let doc = app.sharing.document.as_ref().unwrap();
        let mut newer = doc.baseline.clone(); newer.notes = "Other device change".into();
        let receipt = remote(&newer, &doc.recovery_key, 2); let id = doc.working.id.clone();
        deliver(app, Event::Sync { id, operation: None, result: Ok(receipt) });
        assert!(app.sharing.document.as_ref().unwrap().conflict.is_some());
        app.sharing.open = false;
        newer.notes
    }
    #[test]
    fn ctrl_s_during_a_conflict_leaves_it_resolvable() {
        let (mut app, _dir) = setup();
        let theirs = conflicted(&mut app);
        assert!(app.save(), "nothing new needs saving, so Ctrl+S has nothing to refuse");
        assert!(!app.dirty); assert!(app.banner.is_none());
        app.resolve_shared(false);
        app.discard_shared();
        let doc = app.sharing.document.as_ref().unwrap();
        assert!(doc.conflict.is_none(), "{:?}", app.sharing.error);
        assert_eq!(app.db.as_ref().unwrap().notes, theirs);
    }
    #[test]
    fn an_edit_stranded_by_a_conflict_is_kept_and_does_not_block_resolving_it() {
        let (mut app, _dir) = setup();
        conflicted(&mut app);
        // An editor that was already open can apply after the conflict arrived.
        app.db.as_mut().unwrap().adjustments.set_note(20261029, Some("Applied during the conflict"));
        app.changed();
        assert!(app.dirty, "the queue is closed, so this edit is not saved yet");
        app.resolve_shared(true);
        let doc = app.sharing.document.as_ref().unwrap();
        assert!(doc.conflict.is_none(), "{:?}", app.sharing.error);
        assert_eq!(doc.working.adjustments.get_note(20261029).as_deref(), Some("Applied during the conflict"));
        assert_eq!(doc.working.notes, "My offline change");
        assert!(!app.dirty);
        assert_eq!(client::load_device(&app.sharing.cache).unwrap().working.adjustments.get_note(20261029).as_deref(), Some("Applied during the conflict"));
    }
    #[test]
    fn using_the_shared_version_asks_first_keeps_a_protected_copy_and_can_be_undone() {
        let (mut app, dir) = setup();
        let theirs = conflicted(&mut app);
        app.resolve_shared(false);
        let doc = app.sharing.document.as_ref().unwrap();
        assert_eq!(app.sharing.confirm_discard, Some(1));
        assert!(doc.conflict.is_some() && doc.pending.len() == 1, "nothing changes until the user confirms");
        assert_eq!(app.db.as_ref().unwrap().notes, "My offline change");

        app.discard_shared();
        let doc = app.sharing.document.as_ref().unwrap();
        assert!(doc.conflict.is_none() && doc.pending.is_empty());
        assert_eq!(app.db.as_ref().unwrap().notes, theirs);
        let id = doc.working.id.clone();
        let files = app.sharing.discarded_files(&id);
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with(&dir), "the copy sits with the protected cache");
        assert!(!String::from_utf8_lossy(&std::fs::read(&files[0]).unwrap()).contains("My offline change"));

        // A later session finds it.
        app.sharing.discarded_for = None;
        app.sharing.load_discarded();
        assert_eq!(app.sharing.discarded.len(), 1);
        assert_eq!(app.sharing.discarded[0].changes, 1);

        // The household moved on meanwhile: that unrelated change stays.
        app.db.as_mut().unwrap().adjustments.set_note(20261029, Some("Newer shared note")); app.changed();
        app.recover_discarded(0);
        assert!(app.sharing.error.is_none(), "{:?}", app.sharing.error);
        let doc = app.sharing.document.as_ref().unwrap();
        assert_eq!(doc.working.notes, "My offline change");
        assert_eq!(doc.working.adjustments.get_note(20261029).as_deref(), Some("Newer shared note"));
        assert_eq!(app.db.as_ref().unwrap().notes, "My offline change");
        assert!(!doc.pending.is_empty() && !app.dirty);
        assert_eq!(client::load_device(&app.sharing.cache).unwrap().working.notes, "My offline change");
        assert!(app.sharing.discarded_files(&id).is_empty(), "a recovered copy is not offered twice");
    }
    #[test]
    fn a_copy_that_cannot_be_written_leaves_the_conflict_and_the_changes_alone() {
        let (mut app, dir) = setup();
        conflicted(&mut app);
        app.resolve_shared(false);
        let blocker = dir.join("not-a-directory"); std::fs::write(&blocker, b"fixture").unwrap();
        let cache = std::mem::replace(&mut app.sharing.cache, blocker.join("shared.enc"));
        app.discard_shared();
        app.sharing.cache = cache;
        let doc = app.sharing.document.as_ref().unwrap();
        assert!(app.sharing.error.is_some());
        assert!(doc.conflict.is_some()); assert_eq!(doc.pending.len(), 1);
        assert_eq!(doc.working.notes, "My offline change");
    }
    #[test]
    fn only_the_newest_discarded_copies_are_kept() {
        let (mut app, _dir) = setup();
        let doc = app.sharing.document.clone().unwrap();
        for _ in 0..DISCARDED_KEPT + 2 { app.sharing.keep_discarded(&doc).unwrap(); std::thread::sleep(Duration::from_millis(2)); }
        assert_eq!(app.sharing.discarded_files(&doc.working.id).len(), DISCARDED_KEPT);
    }
    #[test]
    fn credential_refresh_retains_queue_and_persists_new_session_only_for_same_account() {
        let (mut app, _dir) = setup();
        app.db.as_mut().unwrap().notes = "Offline".into(); app.changed();
        let mut connection = app.sharing.document.as_ref().unwrap().connection.clone(); connection.token = "new-fixture-session".into();
        deliver(&mut app, Event::SignedIn(Ok((connection, vec![]))));
        let stored = client::load_device(&app.sharing.cache).unwrap();
        assert_eq!(stored.connection.token, "new-fixture-session"); assert_eq!(stored.pending.len(), 1); assert_eq!(stored.working.notes, "Offline");
    }
    #[test]
    fn fresh_server_open_after_close_recovers_pending_cache_instead_of_overwriting_it() {
        let (mut app, _dir) = setup(); let from_server = app.sharing.document.clone().unwrap();
        app.db.as_mut().unwrap().notes = "Offline before closing".into(); app.changed();
        app.close_budget();
        app.request_shared(from_server, &egui::Context::default());
        let recovered = app.sharing.document.as_ref().unwrap();
        assert_eq!(recovered.working.notes, "Offline before closing"); assert_eq!(recovered.pending.len(), 1);
    }
    #[test]
    fn shared_household_clock_wins_over_local_date_watch() {
        let (mut app, _dir) = setup();
        app.today = 20000101;
        app.check_date(&egui::Context::default());
        assert_eq!(app.today, client::household_today("America/Halifax").unwrap());
        assert_eq!(app.db.as_ref().unwrap().today, app.today);
    }
    #[cfg(windows)]
    #[test]
    fn a_second_window_cannot_replace_the_first_windows_pending_cache() {
        let (mut first, dir) = setup();
        let stale = first.sharing.document.clone().unwrap();
        first.db.as_mut().unwrap().notes = "First window pending change".into(); first.changed();
        let mut second = SharedUi::default(); second.cache = dir.join("shared.enc");
        let error = second.persist(&stale).unwrap_err();
        assert!(error.contains("another Windows window"));
        assert!(second.load_cached().err().unwrap().contains("another Windows window"));
        assert!(second.load_creation().err().unwrap().contains("another Windows window"));
        let candidate = creation(&first);
        assert!(second.persist_creation(&candidate).unwrap_err().contains("another Windows window"));
        assert!(second.cache_lock.is_none());
        assert_eq!(client::load_device(&dir.join("shared.enc")).unwrap().working.notes, "First window pending change");
        first.close_budget();
        assert!(second.ensure_cache_lock().unwrap_err().contains("another Windows window"), "Close budget retains the lifetime lock while workers may still finish");
        drop(first);
        let recovered = second.load_cached().unwrap();
        assert_eq!(recovered.working.notes, "First window pending change");
        assert_eq!(recovered.pending.len(), 1);
        assert!(second.cache_lock.is_some());
    }
    #[cfg(windows)]
    #[test]
    fn opening_shared_in_a_second_window_preserves_its_standalone_work() {
        let (first, dir) = setup();
        let doc = first.sharing.document.clone().unwrap();
        let (mut second, _second_dir) = crate::lifecycle_tests::fixture();
        second.sharing.cache = dir.join("shared.enc");
        let original_file = second.file.clone();
        let original_payment = second.db.as_ref().unwrap().payments[0].clone();
        assert!(second.sharing.cache_lock.is_none(), "standalone startup does not touch or lock the shared cache");
        second.request_shared(doc.clone(), &egui::Context::default());
        assert!(second.dialogs.pending.is_none());
        assert!(second.sharing.document.is_none());
        assert!(second.sharing.error.as_ref().unwrap().contains("another Windows window"));
        second.activate_shared(doc);
        assert!(second.sharing.document.is_none());
        assert_eq!(second.file, original_file);
        assert_eq!(second.db.as_ref().unwrap().payments[0], original_payment);
        assert!(second.dirty);
    }
    /// Serves the household-creation and passcode routes, recording the stored passcode slot.
    fn creation_server(created_response: String, passcode_status: (String, String)) -> (String, std::sync::mpsc::Receiver<Value>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (send, received) = mpsc::channel();
        let (status_line, body) = passcode_status;
        std::thread::spawn(move || {
            for turn in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes = Vec::new(); let mut buffer = [0u8; 4096];
                let (header_end, content_length) = loop {
                    let count = stream.read(&mut buffer).unwrap(); assert!(count > 0); bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let len = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                        break (end + 4, len);
                    }
                };
                while bytes.len() < header_end + content_length { let count = stream.read(&mut buffer).unwrap(); assert!(count > 0); bytes.extend_from_slice(&buffer[..count]); }
                let header = String::from_utf8_lossy(&bytes[..header_end]).to_string();
                let (status, response_body) = if header.starts_with("POST /api/budgets") {
                    ("201 Created".to_string(), created_response.clone())
                } else {
                    assert!(header.starts_with("PUT /api/budgets/") && header.contains("/passcode "), "unexpected request: {header}");
                    let stored: Value = serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap();
                    let _ = send.send(stored["slot"].clone());
                    (status_line.to_string(), body.clone())
                };
                let _ = turn;
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}", response_body.len()).unwrap();
            }
        });
        (origin, received)
    }
    fn passcode_fixture_origin(candidate: &CreationCandidate) -> String {
        serde_json::to_string(&Remote { id: candidate.snapshot.id.clone(), name: candidate.snapshot.name.clone(), revision: 1,
            payload: client::seal(&candidate.snapshot, &candidate.recovery_key, 1).unwrap(), updated_at: "fixture".into(),
            role: "owner".into(), summary: "Created".into() }).unwrap()
    }

    fn creation_with_passcode(app: &App, passcode: &str) -> CreationCandidate {
        let doc = app.sharing.document.clone().unwrap();
        let mut snapshot = doc.working.clone(); snapshot.id = uuid::Uuid::new_v4().to_string();
        CreationCandidate::new(doc.connection.clone(), client::generate_recovery_key(), passcode.to_string(), snapshot).unwrap()
    }

    #[test]
    fn creating_with_a_passcode_stores_its_slot_and_opens_without_warning() {
        let (mut app, _dir) = setup();
        let mut candidate = creation_with_passcode(&app, "copper river lantern");
        let created = passcode_fixture_origin(&candidate);
        let (origin, stored_slot) = creation_server(created, ("200 OK".to_string(), json!({"ok":true}).to_string()));
        candidate.connection.server = origin;
        app.sharing.persist_creation(&candidate).unwrap();
        let stored: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        let (document, warning) = submit_creation(stored).unwrap();
        assert!(warning.is_none());
        let slot: budget_core::passcode::PasscodeSlot = serde_json::from_value(stored_slot.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
        assert_eq!(client::unlock_with_passcode(&slot, "copper river lantern", &document.working.id).unwrap(), document.recovery_key);
    }

    #[test]
    fn a_failed_passcode_upload_opens_the_household_and_reports_the_remedy() {
        let (mut app, _dir) = setup();
        let mut candidate = creation_with_passcode(&app, "copper river lantern");
        let created = passcode_fixture_origin(&candidate);
        let (origin, stored_slot) = creation_server(created, ("500 Internal Server Error".to_string(), json!({"error":{"message":"fixture refused"}}).to_string()));
        candidate.connection.server = origin;
        app.sharing.persist_creation(&candidate).unwrap();
        let stored: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        let (_document, warning) = submit_creation(stored).unwrap();
        let warning = warning.expect("the household opens but the failure is named");
        assert!(warning.contains("fixture refused") && warning.contains("set its passcode from Shared budget"), "{warning}");
        assert!(stored_slot.recv_timeout(Duration::from_secs(5)).is_ok(), "the slot was still sent");
    }

    fn creation(app: &App) -> CreationCandidate {
        let doc = app.sharing.document.as_ref().unwrap();
        let mut snapshot = doc.working.clone(); snapshot.id = uuid::Uuid::new_v4().to_string();
        CreationCandidate::new(doc.connection.clone(), client::generate_recovery_key(), String::new(), snapshot).unwrap()
    }
    #[test]
    fn creation_candidate_is_protected_durable_and_does_not_replace_an_open_household_cache() {
        let (mut app, _dir) = setup(); let current = app.sharing.document.as_ref().unwrap().working.clone();
        let candidate = creation(&app);
        app.sharing.persist_creation(&candidate).unwrap();
        let mut loaded: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.request, candidate.request); assert_eq!(loaded.recovery_key, candidate.recovery_key);
        assert_eq!(client::load_device(&app.sharing.cache).unwrap().working, current);
        assert_eq!(app.sharing.document.as_ref().unwrap().working, current);
        let bytes = std::fs::read(app.sharing.creation_path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(&candidate.recovery_key));
        assert!(!app.sharing.persist_creation(&creation(&app)).is_ok(), "a second click cannot overwrite the first pending identity");
        app.close_budget();
        let after_restart: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        assert_eq!(after_restart.request, candidate.request);
    }
    #[test]
    fn accepted_creation_with_lost_response_and_retry_recovers_one_persisted_identity() {
        use std::io::{Read, Write};
        let (mut app, _dir) = setup(); let mut candidate = creation(&app);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        candidate.connection.server = format!("http://{}", listener.local_addr().unwrap());
        app.sharing.persist_creation(&candidate).unwrap();
        let expected = candidate.request.clone();
        let response = serde_json::to_string(&remote(&candidate.snapshot, &candidate.recovery_key, 1)).unwrap();
        let server = std::thread::spawn(move || {
            for turn in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes = Vec::new(); let mut buffer = [0; 4096];
                let (header_end, content_length) = loop {
                    let count = stream.read(&mut buffer).unwrap(); assert!(count > 0); bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let len = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                        break (end + 4, len);
                    }
                };
                while bytes.len() < header_end + content_length { let count = stream.read(&mut buffer).unwrap(); assert!(count > 0); bytes.extend_from_slice(&buffer[..count]); }
                let header = String::from_utf8_lossy(&bytes[..header_end]);
                let (status, body) = if turn % 2 == 0 {
                    assert!(header.starts_with("POST /api/budgets "));
                    assert_eq!(serde_json::from_slice::<Value>(&bytes[header_end..header_end + content_length]).unwrap(), expected);
                    (if turn == 0 { "500 Internal Server Error" } else { "409 Conflict" }, "{\"error\":{\"message\":\"Accepted response was lost\"}}".to_string())
                } else { assert!(header.starts_with("GET /api/budgets/")); ("200 OK", response.clone()) };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let stored: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        let (first, first_warning) = submit_creation(stored).unwrap();
        let restarted: CreationCandidate = client::load_protected(&app.sharing.creation_path()).unwrap();
        let (second, second_warning) = submit_creation(restarted).unwrap();
        server.join().unwrap();
        assert!(first_warning.is_none() && second_warning.is_none());
        assert_eq!(first.working.id, candidate.snapshot.id); assert_eq!(second.working.id, first.working.id);
        assert_eq!(first.recovery_key, second.recovery_key); assert_eq!(first.revision, 1); assert_eq!(second.revision, 1);
    }
}
