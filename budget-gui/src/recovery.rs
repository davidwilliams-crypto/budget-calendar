//! Crash recovery for standalone budgets.
//!
//! After an edit a worker thread writes the whole budget to a per-user folder, so a panic, power
//! cut or Windows restart costs a few seconds of work at most. A copy is sealed with the budget's
//! password when it has one, and always wrapped with the user-bound key of the shared cache. Shared
//! budgets are not journaled: an edit that reaches their protected cache needs no copy, and one that
//! cannot reach it stays in memory only, with the error showing, until a later save succeeds.
//!
//! Writing and removing share one queue, so a save can never be undone by a write still running.
use crate::shared_client;
use budget_core::{bgt, BudgetDb, Payment};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long an edit can stay uncopied. Further edits do not extend it, so steady typing is covered.
const DELAY: Duration = Duration::from_secs(5);
/// The next attempt after a write failed.
const RETRY: Duration = Duration::from_secs(30);
const COPY_VERSION: u32 = 1;
const EXTENSION: &str = "recovery";
const MAX_LOG_BYTES: u64 = 64 * 1024;

/// The per-user application data folder. It follows the profile folder rather than `LOCALAPPDATA`,
/// so a run with a redirected profile stays away from the real one.
pub fn app_dir() -> PathBuf { crate::home_dir().join("AppData").join("Local").join("BudgetCalendar") }

/// FNV-1a: names a copy after its budget and tells whether a file is the one a copy started from.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

fn copy_path(dir: &Path, budget: &Path) -> PathBuf {
    dir.join(format!("{:016x}.{EXTENSION}", fingerprint(crate::recent_key(budget).as_bytes())))
}

fn owner_path(copy: &Path) -> PathBuf { copy.with_extension("owner") }

/// Whether `path` is a recovery copy, not a budget or one of its backups.
pub fn is_copy(path: &Path) -> bool { path.extension().is_some_and(|e| e == EXTENSION) }

/// Everything needed to write one copy, taken from the budget right after an edit.
#[derive(Clone)]
pub struct Job {
    path: PathBuf,
    payments: Vec<Payment>,
    adjustments: Payment,
    notes: String,
    password: Option<String>,
    base: Option<u64>,
    edits: u64,
}

impl Job {
    /// `None` for a file that is sealed while the draft has no password (an unsealed backup being
    /// restored): a copy would be less protected than the budget, so none is written.
    pub fn new(db: &BudgetDb, path: &Path, password: Option<&str>, saved_bytes: Option<&[u8]>, edits: u64) -> Option<Job> {
        if password.is_none() && saved_bytes.is_some_and(|bytes| bgt::is_encrypted_text(&bgt::latin1_decode(bytes))) { return None; }
        Some(Job { path: path.to_path_buf(), payments: db.payments.clone(), adjustments: db.adjustments.clone(), notes: db.notes.clone(),
            password: password.map(str::to_string), base: saved_bytes.map(fingerprint), edits })
    }
}

/// What is stored, inside the user-bound protection. `text` is what a save would write, so for a
/// sealed budget it is already sealed with the budget's password.
#[derive(Serialize, Deserialize)]
struct RecoveryCopy { version: u32, key: String, path: String, written_ms: u64, base: Option<u64>, text: String }

fn millis(time: SystemTime) -> u64 { time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }

fn write(dir: &Path, job: &Job) -> Result<(), String> {
    let text = bgt::save_str(&job.payments, &job.adjustments, &job.notes, job.password.as_deref())?;
    let copy = RecoveryCopy { version: COPY_VERSION, key: crate::recent_key(&job.path), path: job.path.display().to_string(),
        written_ms: millis(SystemTime::now()), base: job.base, text };
    shared_client::save_protected(&copy_path(dir, &job.path), &copy)
}

/// Marks a copy as belonging to a window that is still running. The operating system releases it
/// however the window ends, so a copy without a holder was left behind by a crash.
#[cfg(windows)]
fn hold_owner(path: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).open(path).ok()
}
#[cfg(not(windows))]
fn hold_owner(_path: &Path) -> Option<std::fs::File> { None }

#[cfg(windows)]
fn owner_is_held(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).share_mode(0).open(path).is_err_and(|e| matches!(e.raw_os_error(), Some(32 | 33)))
}
#[cfg(not(windows))]
fn owner_is_held(_path: &Path) -> bool { false }

/// A copy kept for the budget just opened, for the user to restore or discard.
pub struct Offer {
    pub budget: PathBuf,
    pub file: PathBuf,
    pub bytes: Vec<u8>,
    pub written: String,
    /// The budget file is no longer the one the copy started from.
    pub file_changed: bool,
}

/// What to do about a copy, from what is known about it and about the budget file.
#[derive(Debug, PartialEq)]
enum Decision {
    Ignore,
    Offer { file_changed: bool },
}

/// `current` is the fingerprint of the budget file, `None` when it is missing or damaged.
/// A file that moved on does not replace the copy: being older says nothing about edits the file
/// never received, as after a sync from another computer. Only a file that already holds exactly
/// this work, left by a crash between a save and removing the copy, has nothing to offer. A sealed
/// copy never matches, because each sealing differs, so it is offered with the warning.
fn decide(copy: &RecoveryCopy, key: &str, current: Option<u64>) -> Decision {
    if copy.key != key { return Decision::Ignore; }
    let Some(current) = current else { return Decision::Offer { file_changed: false } };
    if copy.base == Some(current) { return Decision::Offer { file_changed: false }; }
    if fingerprint(&bgt::latin1_encode(&copy.text)) == current { Decision::Ignore } else { Decision::Offer { file_changed: true } }
}

/// The result of looking for a copy of a budget.
pub enum Found { Nothing, Offer(Offer), Unreadable }

/// Looks for a copy of `budget`, whose file has the fingerprint `current`. Most opens stop at the
/// first check, in the local folder, without touching a budget that may sit on a slow share.
pub fn find(dir: &Path, budget: &Path, current: Option<u64>) -> Found {
    let file = copy_path(dir, budget);
    if !file.is_file() || owner_is_held(&owner_path(&file)) { return Found::Nothing; }
    let Ok(copy) = shared_client::load_protected::<RecoveryCopy>(&file) else { return Found::Unreadable };
    if copy.version != COPY_VERSION { return Found::Unreadable; }
    match decide(&copy, &crate::recent_key(budget), current) {
        Decision::Ignore => Found::Nothing,
        Decision::Offer { file_changed } => {
            let written = chrono::DateTime::<chrono::Local>::from(UNIX_EPOCH + Duration::from_millis(copy.written_ms));
            Found::Offer(Offer { budget: budget.to_path_buf(), bytes: bgt::latin1_encode(&copy.text), file,
                written: written.format("%Y-%m-%d %H:%M:%S").to_string(), file_changed })
        }
    }
}

struct Shared {
    dir: PathBuf,
    /// The newest edit. The panic hook reads this as well, because it cannot reach the application.
    latest: Mutex<Option<Job>>,
    written: AtomicU64,
    failed: AtomicBool,
    waker: Mutex<Option<egui::Context>>,
}

impl Shared {
    /// Writes the newest edit unless that is already done. Returns whether a write failed.
    fn write_latest(&self, latest: Option<Job>) -> bool {
        let Some(job) = latest.filter(|job| job.edits != self.written.load(Ordering::SeqCst)) else { return false };
        let result = write(&self.dir, &job);
        if result.is_ok() { self.written.store(job.edits, Ordering::SeqCst); }
        result.is_err()
    }
    fn newest(&self) -> Option<Job> { self.latest.lock().ok().and_then(|job| job.clone()) }
    /// The edit count of the newest edit, read in place: `tick` asks on every frame, and cloning the
    /// job to answer would copy every payment of the budget each time.
    fn newest_edits(&self) -> Option<u64> { self.latest.lock().ok()?.as_ref().map(|job| job.edits) }
}

enum Command { Flush, Remove(PathBuf) }

type Owner = Option<(PathBuf, Option<std::fs::File>)>;

fn release(owner: &mut Owner) {
    if let Some((path, file)) = owner.take() { drop(file); let _ = std::fs::remove_file(path); }
}

fn run(shared: &Shared, commands: Receiver<Command>) {
    let mut owner: Owner = None;
    for command in commands {
        match command {
            Command::Flush => {
                let latest = shared.newest();
                if let Some(job) = &latest {
                    let path = owner_path(&copy_path(&shared.dir, &job.path));
                    if owner.as_ref().is_none_or(|(held, _)| *held != path) {
                        release(&mut owner);
                        let _ = std::fs::create_dir_all(&shared.dir);
                        owner = Some((path.clone(), hold_owner(&path)));
                    }
                }
                if shared.write_latest(latest) {
                    shared.failed.store(true, Ordering::SeqCst);
                    if let Some(ctx) = shared.waker.lock().ok().and_then(|ctx| ctx.clone()) { ctx.request_repaint(); }
                }
            }
            Command::Remove(budget) => {
                let file = copy_path(&shared.dir, &budget);
                let holder = owner_path(&file);
                if owner.as_ref().is_some_and(|(held, _)| *held == holder) { release(&mut owner); }
                // Another window is still using this copy for edits of its own.
                if !owner_is_held(&holder) { let _ = std::fs::remove_file(file); }
            }
        }
    }
    release(&mut owner);
}

/// The budget's recovery copy: when it is due, and the worker that writes it.
pub struct Journal {
    shared: Arc<Shared>,
    worker: Option<(Sender<Command>, JoinHandle<()>)>,
    delay: Duration,
    /// How long to wait after a failed write.
    retry: Duration,
    /// When the oldest edit without a copy was made.
    first_unwritten: Option<Instant>,
    /// The edit count last handed to the worker.
    sent: Option<u64>,
    /// No new attempt before this, after a failure.
    hold_until: Option<Instant>,
    failure: bool,
}

impl Journal {
    pub fn new(dir: PathBuf) -> Journal {
        let shared = Shared { dir, latest: Mutex::new(None), written: AtomicU64::new(0), failed: AtomicBool::new(false), waker: Mutex::new(None) };
        Journal { shared: Arc::new(shared), worker: None, delay: DELAY, retry: RETRY, first_unwritten: None, sent: None, hold_until: None, failure: false }
    }

    pub fn dir(&self) -> &Path { &self.shared.dir }

    /// Lets the panic hook write this journal's newest edit.
    pub fn register(&self) { if let Ok(mut target) = PANIC_TARGET.lock() { *target = Some(self.shared.clone()); } }

    fn send(&mut self, command: Command) {
        if self.worker.is_none() {
            let (send, receive) = channel();
            let shared = self.shared.clone();
            if let Ok(handle) = std::thread::Builder::new().name("recovery-journal".into()).spawn(move || run(&shared, receive)) {
                self.worker = Some((send, handle));
            }
        }
        match (&self.worker, command) {
            (Some((worker, _)), command) => { let _ = worker.send(command); }
            // Without a worker the window does the work: a pause is better than a lost copy.
            (None, Command::Flush) => if self.shared.write_latest(self.shared.newest()) { self.shared.failed.store(true, Ordering::SeqCst); },
            (None, Command::Remove(budget)) => { let _ = std::fs::remove_file(copy_path(&self.shared.dir, &budget)); }
        }
    }

    /// The budget was edited. `None` means this draft must have no copy.
    pub fn note(&mut self, job: Option<Job>) {
        if let Ok(mut latest) = self.shared.latest.lock() { *latest = job; }
        self.first_unwritten.get_or_insert_with(Instant::now);
    }

    /// Forgets the open budget's edits. Any copy on disk stays.
    pub fn clear(&mut self) {
        if let Ok(mut latest) = self.shared.latest.lock() { *latest = None; }
        self.first_unwritten = None;
    }

    /// The edits were saved or discarded, so the copy of `budget` has no more use.
    pub fn remove(&mut self, budget: &Path) {
        self.clear();
        self.send(Command::Remove(budget.to_path_buf()));
    }

    /// Hands a due copy to the worker. Returns how long until the next one is due, if one is waiting.
    pub fn tick(&mut self, ctx: &egui::Context, unfocused: bool) -> Option<Duration> {
        if let Ok(mut waker) = self.shared.waker.lock() { if waker.is_none() { *waker = Some(ctx.clone()); } }
        let now = Instant::now();
        if self.shared.failed.swap(false, Ordering::SeqCst) {
            self.sent = None; self.failure = true; self.hold_until = Some(now + self.retry);
            self.first_unwritten.get_or_insert(now);
        }
        let latest = self.shared.newest_edits()?;
        if self.shared.written.load(Ordering::SeqCst) == latest || self.sent == Some(latest) { return None; }
        if let Some(until) = self.hold_until.filter(|until| *until > now) { return Some(until - now); }
        let due = self.first_unwritten.unwrap_or(now) + self.delay;
        if now < due && !unfocused { return Some(due - now); }
        self.sent = Some(latest); self.first_unwritten = None; self.hold_until = None;
        self.send(Command::Flush);
        None
    }

    /// Whether a write failed since the last call.
    pub fn take_failure(&mut self) -> bool { std::mem::take(&mut self.failure) }
}

impl Drop for Journal {
    /// Finishes the queue, so a save just before closing also removes its copy.
    fn drop(&mut self) {
        if let Some((send, handle)) = self.worker.take() { drop(send); let _ = handle.join(); }
    }
}

static PANIC_TARGET: Mutex<Option<Arc<Shared>>> = Mutex::new(None);
static IN_PANIC_HOOK: AtomicBool = AtomicBool::new(false);

/// One line for the log. It takes no panic message: those can quote values from the budget.
fn crash_line(now: chrono::DateTime<chrono::Local>, thread: Option<&str>, location: Option<&std::panic::Location>) -> String {
    let place = location.map_or_else(|| "an unknown place".to_string(), |at| format!("{}:{}", at.file(), at.line()));
    format!("{} Budget Calendar {} ({}) stopped at {place} on thread {}\n", now.format("%Y-%m-%d %H:%M:%S"), crate::version::VERSION, crate::version::BUILD_ID, thread.unwrap_or("unnamed"))
}

fn append_log(dir: &Path, line: &str) {
    use std::io::Write;
    if std::fs::create_dir_all(dir).is_err() { return; }
    let path = dir.join("crash.log");
    let full = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let file = if full { std::fs::File::create(path) } else { std::fs::OpenOptions::new().create(true).append(true).open(path) };
    if let Ok(mut file) = file { let _ = file.write_all(line.as_bytes()); }
}

/// Writes a short log and tries to keep the newest edit. It never waits for a lock another thread
/// may hold while stopping, and it passes on to the previous hook either way.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !IN_PANIC_HOOK.swap(true, Ordering::SeqCst) {
            append_log(&app_dir(), &crash_line(chrono::Local::now(), std::thread::current().name(), info.location()));
            let target = PANIC_TARGET.try_lock().ok().and_then(|target| target.clone());
            if let Some(shared) = target {
                let latest = shared.latest.try_lock().ok().and_then(|job| job.clone());
                shared.write_latest(latest);
            }
            IN_PANIC_HOOK.store(false, Ordering::SeqCst);
        }
        previous(info);
    }));
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::lifecycle_tests::fixture;
    use crate::{document, App, PendingAction};

    const EDIT: &str = "Fixture note written after the last save";

    fn copy_at(budget: &Path, written: SystemTime, base: Option<u64>) -> RecoveryCopy {
        RecoveryCopy { version: COPY_VERSION, key: crate::recent_key(budget), path: budget.display().to_string(), written_ms: millis(written), base, text: String::new() }
    }

    #[test]
    fn the_restore_decision_offers_what_is_newer_and_leaves_the_rest() {
        let budget = Path::new("target/recovery/home.bgt");
        let key = crate::recent_key(budget);
        let then = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let copy = copy_at(budget, then, Some(1));
        let offer = |file_changed| Decision::Offer { file_changed };
        // The file is the one the copy started from.
        assert_eq!(decide(&copy, &key, Some(1)), offer(false));
        // The file moved on: the copy still holds edits it never received, and the user is told.
        assert_eq!(decide(&copy, &key, Some(2)), offer(true));
        // The file already holds exactly this work.
        assert_eq!(decide(&copy, &key, Some(fingerprint(b""))), Decision::Ignore);
        // A missing or damaged file has nothing to compare.
        assert_eq!(decide(&copy, &key, None), offer(false));
        // Another budget whose name hashed alike.
        assert_eq!(decide(&copy, "another budget", Some(1)), Decision::Ignore);
    }

    /// A budget saved on disk, open and clean in the app.
    fn opened(password: Option<&str>) -> (App, PathBuf, PathBuf) {
        let (mut app, dir) = fixture();
        let path = dir.join("household.bgt");
        document::save_checked(app.db.as_ref().unwrap(), &path, password, None).unwrap();
        app.load(&path, password);
        assert_eq!(app.file.as_deref(), Some(path.as_path()), "{:?}", app.banner);
        assert!(!app.dirty);
        (app, dir, path)
    }

    fn edit(app: &mut App) {
        app.db.as_mut().unwrap().notes = EDIT.into();
        app.changed();
    }

    fn gui(app: &App) -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx, &app.t);
        ctx
    }

    fn frame(app: &mut App, ctx: &egui::Context, focused: bool) -> egui::FullOutput {
        ctx.run(egui::RawInput { focused, ..Default::default() }, |ctx| app.render(ctx))
    }

    fn wait(app: &mut App, ctx: &egui::Context, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
            frame(app, ctx, true);
        }
    }

    fn copy_of(app: &App, path: &Path) -> PathBuf { copy_path(app.journal.dir(), path) }

    /// The state a crash leaves: the edit's copy on disk, and no window holding it.
    fn crashed(mut app: App) {
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| app.journal.shared.written.load(Ordering::SeqCst) == app.edits);
        app.journal.clear();
        drop(app);
    }

    fn reopen(dir: &Path, path: &Path, password: Option<&str>) -> (App, PathBuf) {
        let (mut app, scratch) = fixture();
        app.journal = Journal::new(dir.join("recovery"));
        app.dirty = false;
        app.load(path, password);
        (app, scratch)
    }

    #[test]
    fn an_edit_is_copied_after_the_delay_or_on_focus_loss_and_not_before() {
        let (mut app, _dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        let output = frame(&mut app, &ctx, true);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!copy_of(&app, &path).exists(), "still within the delay");
        let wake = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(wake <= DELAY, "an idle window must wake for the copy: {wake:?}");
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
        let (mut app, _dir, path) = opened(None);
        app.journal.delay = Duration::from_millis(40);
        edit(&mut app);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
    }

    #[test]
    fn saving_removes_the_copy_and_a_write_already_queued_cannot_bring_it_back() {
        let (mut app, _dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        assert!(app.save());
        wait(&mut app, &ctx, |app| !copy_of(app, &path).exists());
        for _ in 0..3 { frame(&mut app, &ctx, false); }
        let file = copy_of(&app, &path);
        drop(app);
        assert!(!file.exists());
    }

    #[test]
    fn discarding_changes_removes_the_copy() {
        let (mut app, _dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
        let file = copy_of(&app, &path);
        app.discard_unsaved(PendingAction::CloseBudget, &ctx);
        assert!(app.db.is_none());
        drop(app);
        assert!(!file.exists());
    }

    #[test]
    fn a_plain_budget_is_restored_as_an_unsaved_draft_and_never_written_to_the_file() {
        let (app, dir, path) = opened(None);
        let before = std::fs::read(&path).unwrap();
        let file = copy_of(&app, &path);
        crashed(app);
        assert!(!String::from_utf8_lossy(&std::fs::read(file).unwrap()).contains(EDIT), "the copy is wrapped even for a plain budget");

        let (mut app, _scratch) = reopen(&dir, &path, None);
        assert!(app.db.as_ref().unwrap().notes.is_empty(), "the file opens as it is until the user decides");
        assert!(!app.dialogs.recovery.as_ref().expect("the copy is offered").file_changed);
        app.answer_recovery(true);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
        assert_eq!(std::fs::read(&path).unwrap(), before, "restoring leaves the file alone");
        assert!(app.toast.as_ref().is_some_and(|toast| toast.text.starts_with("Unsaved work restored")));
        assert_eq!(app.file.as_deref(), Some(path.as_path()));

        assert!(app.save());
        assert_ne!(std::fs::read(&path).unwrap(), before, "saving is what changes the file");
        wait(&mut app, &ctx, |app| !copy_of(app, &path).exists());
    }

    #[test]
    fn discarding_the_offer_keeps_the_file_and_removes_the_copy() {
        let (app, dir, path) = opened(None);
        let before = std::fs::read(&path).unwrap();
        crashed(app);
        let (mut app, _scratch) = reopen(&dir, &path, None);
        assert!(app.dialogs.recovery.is_some());
        app.answer_recovery(false);
        assert!(app.dialogs.recovery.is_none());
        assert!(!app.dirty);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| !copy_of(app, &path).exists());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_sealed_budget_gets_a_sealed_copy_and_restores_with_its_password() {
        let password = "fixture password";
        let (app, dir, path) = opened(Some(password));
        let file = copy_of(&app, &path);
        crashed(app);
        let copy: RecoveryCopy = shared_client::load_protected(&file).unwrap();
        assert!(bgt::is_encrypted_text(&copy.text), "unwrapping the copy still leaves it sealed");
        assert!(!copy.text.contains(EDIT));
        let (mut app, _scratch) = reopen(&dir, &path, Some(password));
        app.answer_recovery(true);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
        assert_eq!(app.password.as_deref(), Some(password), "a save stays sealed");
    }

    #[test]
    fn a_sealed_file_open_without_a_password_is_never_copied() {
        let (mut app, _dir, path) = opened(Some("fixture password"));
        app.password = None; // an unsealed earlier version restored over a sealed file
        app.journal.delay = Duration::ZERO;
        edit(&mut app);
        let ctx = gui(&app);
        for _ in 0..5 { frame(&mut app, &ctx, false); std::thread::sleep(Duration::from_millis(5)); }
        assert!(app.journal.shared.newest().is_none());
        assert!(!copy_of(&app, &path).exists());
    }

    #[test]
    fn the_autosave_setting_turns_copies_off() {
        let (mut app, _dir, path) = opened(None);
        app.cfg.autosave = false;
        app.journal.delay = Duration::ZERO;
        edit(&mut app);
        let ctx = gui(&app);
        for _ in 0..5 { frame(&mut app, &ctx, false); std::thread::sleep(Duration::from_millis(5)); }
        assert!(!copy_of(&app, &path).exists());
    }

    #[test]
    fn a_copy_of_a_file_that_moved_on_is_offered_with_the_warning_and_one_in_use_elsewhere_is_not() {
        let (app, dir, path) = opened(None);
        crashed(app);
        std::thread::sleep(Duration::from_millis(20));
        let (other, _other_dir) = fixture();
        let mut newer = other.db.clone().unwrap();
        newer.notes = "Saved on another computer".into();
        document::save_checked(&newer, &path, None, None).unwrap();
        let (app, _scratch) = reopen(&dir, &path, None);
        assert!(app.dialogs.recovery.as_ref().expect("the unsaved work is still offered").file_changed);

        let (mut app, _dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
        assert!(matches!(find(app.journal.dir(), &path, None), Found::Nothing), "a window is still using this copy");
    }

    #[test]
    fn a_copy_left_by_a_crash_right_after_a_save_is_not_offered() {
        let (mut app, dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
        document::save_checked(app.db.as_ref().unwrap(), &path, None, None).unwrap();
        app.journal.clear();
        drop(app);
        let (app, _scratch) = reopen(&dir, &path, None);
        assert!(app.dialogs.recovery.is_none());
    }

    #[test]
    fn the_prompt_for_a_sealed_copy_names_the_budget() {
        let budget = Path::new("target/recovery/Rent.bgt");
        assert_eq!(crate::encrypted_line(Path::new("target/recovery/0123456789abcdef.recovery"), Some(budget)), "Unsaved work for Rent.bgt is encrypted.");
        assert_eq!(crate::encrypted_line(budget, None), "Rent.bgt is encrypted.");
    }

    #[test]
    fn the_window_is_held_while_a_copy_is_read() {
        let (app, dir, path) = opened(None);
        crashed(app);
        let (mut app, _scratch) = reopen(&dir, &path, None);
        app.answer_recovery(true);
        assert!(app.has_modal(), "nothing can be edited until the copy is in");
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.dirty);
        assert!(!app.has_modal());
    }

    #[test]
    fn a_missing_budget_still_offers_its_copy_and_saving_recreates_it() {
        let (app, dir, path) = opened(None);
        crashed(app);
        std::fs::remove_file(&path).unwrap();
        let (mut app, _scratch) = reopen(&dir, &path, None);
        assert!(app.banner.is_some(), "the failed open still says so");
        assert!(app.dialogs.recovery.is_some());
        app.answer_recovery(true);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.file.as_deref() == Some(path.as_path()) && app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
        assert!(app.save());
        assert!(path.exists());
    }

    #[test]
    fn a_budget_cut_short_by_the_crash_offers_its_copy_first() {
        let (app, dir, path) = opened(None);
        crashed(app);
        let bytes = std::fs::read(&path).unwrap();
        let end = String::from_utf8_lossy(&bytes).rfind("## END").unwrap();
        std::fs::write(&path, &bytes[..end]).unwrap();
        let (mut app, _scratch) = reopen(&dir, &path, None);
        assert!(app.dialogs.incomplete.is_some() && app.dialogs.recovery.is_some());
        app.answer_recovery(true);
        assert!(app.dialogs.incomplete.is_none());
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
        assert_eq!(std::fs::read(&path).unwrap(), &bytes[..end], "the damaged file stays until a save");
    }

    #[test]
    fn a_failed_write_warns_and_keeps_the_edit_for_a_retry() {
        let (mut app, dir, _path) = opened(None);
        std::fs::write(dir.join("blocked"), b"not a folder").unwrap();
        let mut blocked = Journal::new(dir.join("blocked").join("recovery"));
        blocked.delay = Duration::ZERO;
        app.journal = blocked;
        edit(&mut app);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.toast.as_ref().is_some_and(|toast| toast.error));
        assert!(app.toast.as_ref().unwrap().text.contains("Save soon"));
        assert!(app.dirty);
        assert!(app.journal.shared.newest().is_some());
    }

    #[test]
    fn a_failed_write_is_tried_again_while_the_window_keeps_focus() {
        let (mut app, dir, path) = opened(None);
        std::fs::write(dir.join("blocked"), b"not a folder").unwrap();
        let mut blocked = Journal::new(dir.join("blocked").join("recovery"));
        blocked.delay = Duration::from_millis(200);
        blocked.retry = Duration::from_millis(50);
        app.journal = blocked;
        edit(&mut app);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.toast.as_ref().is_some_and(|toast| toast.error));
        std::fs::remove_file(dir.join("blocked")).unwrap();
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
    }

    /// Answers the password prompt as its Unlock button does.
    fn try_password(app: &mut App, ctx: &egui::Context, password: &str) {
        let prompt = app.dialogs.password.as_ref().unwrap();
        let job = app.opening_job(&prompt.path, Some(prompt.bytes.clone()), Some(password.into()), false, prompt.restore_to.clone());
        app.dialogs.password.as_mut().unwrap().attempt = Some(job);
        wait(app, ctx, |app| app.dialogs.password.as_ref().is_none_or(|prompt| prompt.attempt.is_none()));
    }

    #[test]
    fn a_damaged_sealed_budget_offers_its_copy_once_the_password_fails() {
        let password = "fixture password";
        let (app, dir, path) = opened(Some(password));
        crashed(app);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        let (mut app, _scratch) = reopen(&dir, &path, None);
        let ctx = gui(&app);
        assert!(app.dialogs.password.is_some());
        try_password(&mut app, &ctx, password);
        assert!(app.dialogs.password.is_none() && app.dialogs.recovery.is_some(), "the copy is offered in place of the prompt");
        assert!(app.banner.is_some(), "the failed open still says so");
        app.answer_recovery(true);
        wait(&mut app, &ctx, |app| app.dialogs.password.is_some());
        assert_eq!(app.dialogs.password.as_ref().unwrap().restore_to.as_deref(), Some(path.as_path()));
        try_password(&mut app, &ctx, password);
        wait(&mut app, &ctx, |app| app.dirty);
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
        assert_eq!(app.password.as_deref(), Some(password));
        assert_eq!(std::fs::read(&path).unwrap(), &bytes[..bytes.len() / 2], "the damaged file stays until a save");
    }

    #[test]
    fn a_wrong_password_with_no_copy_keeps_the_prompt() {
        let (_app, _dir, path) = opened(Some("fixture password"));
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.load(&path, None);
        let ctx = gui(&app);
        try_password(&mut app, &ctx, "not the password");
        assert!(app.dialogs.password.as_ref().is_some_and(|prompt| !prompt.error.is_empty()));
        assert!(app.dialogs.recovery.is_none());
        drop(dir);
    }

    #[test]
    fn restoring_a_backup_never_replaces_a_copy_not_yet_answered() {
        let (app, dir, path) = opened(None);
        let file = copy_of(&app, &path);
        crashed(app);
        let backup = dir.join("household.bgt.bak");
        std::fs::copy(&path, &backup).unwrap();
        let before = std::fs::read(&file).unwrap();
        let (mut app, _scratch) = reopen(&dir, &path, None);
        app.journal.delay = Duration::ZERO;
        app.dialogs.recovery = None; // the state a damaged file reaches by way of Restore previous version
        app.restore_backup(path.clone(), backup);
        let ctx = gui(&app);
        wait(&mut app, &ctx, |app| app.dirty);
        for _ in 0..5 { frame(&mut app, &ctx, false); std::thread::sleep(Duration::from_millis(5)); }
        assert!(app.dialogs.recovery.is_some(), "the copy is asked about first");
        assert_eq!(std::fs::read(&file).unwrap(), before);
        // Answered, the draft on screen gets its own copy.
        app.answer_recovery(false);
        assert!(app.dirty && app.journal.shared.newest().is_some());
    }

    #[test]
    fn an_edit_while_the_copy_is_read_puts_the_offer_back() {
        let (app, dir, path) = opened(None);
        let file = copy_of(&app, &path);
        crashed(app);
        let (mut app, _scratch) = reopen(&dir, &path, None);
        let ctx = gui(&app);
        app.answer_recovery(true);
        edit(&mut app);
        wait(&mut app, &ctx, |app| app.opening.is_none());
        assert!(app.dialogs.pending.is_none(), "no unsaved-changes prompt can reach the copy");
        assert!(app.dialogs.recovery.is_some(), "the copy is offered again");
        assert!(file.exists());
        app.answer_recovery(true);
        wait(&mut app, &ctx, |app| app.opening.is_none());
        assert_eq!(app.db.as_ref().unwrap().notes, EDIT);
    }

    #[test]
    fn discarding_changes_while_the_copy_is_read_keeps_the_copy() {
        let (app, dir, path) = opened(None);
        let file = copy_of(&app, &path);
        crashed(app);
        let (mut app, _scratch) = reopen(&dir, &path, None);
        let ctx = gui(&app);
        app.answer_recovery(true);
        app.dirty = true;
        app.discard_unsaved(PendingAction::CloseBudget, &ctx);
        drop(app);
        assert!(file.exists());
    }

    #[test]
    fn a_warning_about_a_copy_survives_the_start_of_the_window() {
        let (app, dir, path) = opened(None);
        let file = copy_of(&app, &path);
        drop(app);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"not a copy").unwrap();
        let (mut app, _scratch) = fixture();
        app.journal = Journal::new(dir.join("recovery"));
        app.dirty = false;
        app.load_at_start(&path);
        assert!(app.toast.as_ref().is_some_and(|toast| toast.error && toast.text.contains("could not be read")));
        let (mut app, _dir, path) = opened(None);
        app.load_at_start(&path);
        assert!(app.toast.is_none(), "the Opened notice stays away at the start");
    }

    #[test]
    fn a_save_in_another_window_leaves_this_windows_copy() {
        let (mut app, _dir, path) = opened(None);
        let ctx = gui(&app);
        edit(&mut app);
        frame(&mut app, &ctx, false);
        wait(&mut app, &ctx, |app| copy_of(app, &path).exists());
        let mut other = Journal::new(app.journal.dir().to_path_buf());
        other.remove(&path);
        drop(other);
        assert!(copy_of(&app, &path).exists(), "another window is still using this copy");
    }

    #[test]
    fn the_panic_path_writes_the_newest_edit_and_its_log_holds_no_budget_data() {
        let (mut app, _dir, path) = opened(None);
        edit(&mut app);
        let latest = app.journal.shared.newest();
        assert!(!app.journal.shared.write_latest(latest.clone()));
        let copy: RecoveryCopy = shared_client::load_protected(&copy_of(&app, &path)).unwrap();
        assert!(copy.text.contains(EDIT));
        // A second panic finds it written and leaves the file alone.
        let before = std::fs::read(copy_of(&app, &path)).unwrap();
        assert!(!app.journal.shared.write_latest(latest));
        assert_eq!(std::fs::read(copy_of(&app, &path)).unwrap(), before);

        let line = crash_line(chrono::Local::now(), Some("main"), Some(std::panic::Location::caller()));
        assert!(line.contains("recovery.rs:") && line.contains(crate::version::VERSION) && line.ends_with("on thread main\n"), "{line}");
        let dir = app.journal.dir().with_file_name("log-test");
        append_log(&dir, &line);
        append_log(&dir, &line);
        assert_eq!(std::fs::read_to_string(dir.join("crash.log")).unwrap().lines().count(), 2);
        std::fs::write(dir.join("crash.log"), vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append_log(&dir, &line);
        assert_eq!(std::fs::read_to_string(dir.join("crash.log")).unwrap(), line, "the log cannot grow without limit");
    }
}
