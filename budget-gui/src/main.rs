#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Budget Calendar main window.
//!
//! Layout: header panel (menu row + toolbar), optional right summary panel, central panel with the
//! stats strip and the calendar. `theme.rs` holds the design tokens, `view.rs` the month view-model,
//! `calendar.rs` the painter-drawn grid.

mod calendar;
mod balance_editor;
mod date_watch;
mod document;
mod editor;
mod file_probe;
mod names;
mod payment_list;
mod refresh;
mod recovery;
mod reports;
mod theme;
mod undo;
mod view;
mod version;
mod shared_client;
mod sharing;

use budget_core::config::BudgetConfig;
use budget_core::filter::{Filter, Haystacks};
use budget_core::{bgt, BudgetDb, Cal, Field};
use eframe::egui::{self, Align, Align2, Button, Frame, Key, Layout, Margin, Modifiers, RichText, Sense, Stroke, StrokeKind, TextEdit, Vec2};
use egui_phosphor::regular as ph;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};


use theme::{alpha, tint, ts, Tokens, CAPTION, ICON, SECTION, SEMI, SP1, SP2, SP3, SP4, TITLE};
use view::{money, MonthView};
use editor::{PaymentAction, PaymentEditor, TransactionAction, TransactionEditor, TransactionScope};
use balance_editor::{BalanceAction, BalanceEditor};
use date_watch::DateWatch;
use file_probe::{FileProbes, FileStatus, TaskState};
use names::{MONTHS, MONTHS_LONG};

const SUMMARY_W: f32 = 292.0;
/// From this window width the summary panel fits beside a calendar that stays readable.
const SUMMARY_MIN_WINDOW: f32 = 1280.0;

/// The same artwork supplies the native window and the in-app brand mark.
fn app_icon() -> egui::IconData {
    let rgba = image::load_from_memory(include_bytes!("../assets/budget-calendar.png"))
        .expect("embedded application icon must be a valid PNG").into_rgba8();
    egui::IconData { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
}

/// The colour of a figure: negative when its printed text shows a minus sign, so the colour and the
/// sign a reader sees always agree (a value such as -0.7 prints "−" in whole-dollar mode).
fn sign_colour(t: &Tokens, v: f64, decimals: bool) -> egui::Color32 {
    if view::is_negative(v, decimals) { t.negative } else { t.positive }
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// `budget-gui [file.bgt] [--month yyyymm] [--screenshot out.png] [--size WxH] [--dark] [--today yyyymmdd] [--filter expr]`
#[derive(Default)]
struct Cli { file: Option<String>, month: Option<i32>, screenshot: Option<PathBuf>, size: Option<[f32; 2]>, dark: bool, today: Option<i32>, filter: Option<String> }

fn parse_cli() -> Cli { parse_cli_from(std::env::args().skip(1).collect()) }

/// The first positional argument (not a flag or a flag's value) is the budget file.
fn first_positional(args: &[String]) -> Option<String> {
    const VALUE_FLAGS: [&str; 5] = ["--month", "--screenshot", "--size", "--today", "--filter"];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if VALUE_FLAGS.contains(&a.as_str()) { i += 2; continue; }
        if !a.starts_with('-') { return Some(a.clone()); }
        i += 1;
    }
    None
}

fn parse_cli_from(args: Vec<String>) -> Cli {
    // `--flag value` and `--flag=value` both give the value.
    let val = |flag: &str| args.iter().enumerate().find_map(|(i, a)| {
        if a == flag { args.get(i + 1).cloned() } else { a.strip_prefix(flag)?.strip_prefix('=').map(str::to_string) }
    });
    Cli {
        file: first_positional(&args),
        month: val("--month").and_then(|m| m.parse().ok()),
        screenshot: val("--screenshot").map(PathBuf::from),
        size: val("--size").and_then(|s| { let (w, h) = s.split_once('x')?; Some([w.parse().ok()?, h.parse().ok()?]) }),
        dark: args.iter().any(|a| a == "--dark"),
        // Only a real calendar day: anything else would leave the grid and the engine on different dates.
        today: val("--today").and_then(|t| t.parse().ok()).filter(|&d| budget_core::date::is_calendar_date(d)),
        filter: val("--filter"),
    }
}

struct Toast { text: String, error: bool, until: Instant }

/// A persistent error. A load failure carries a title naming the file, the full
/// diagnostic log under "Details" and an "Open another…" recovery action.
#[derive(Clone, Debug, PartialEq)]
struct Banner {
    title: Option<String>, text: String, details: Option<String>, offer_open: bool,
    /// The budget whose backups a "Restore previous version" button lists.
    offer_restore: Option<PathBuf>,
}

impl Banner {
    fn plain(text: impl Into<String>) -> Banner { Banner { title: None, text: text.into(), details: None, offer_open: false, offer_restore: None } }
    fn load_failure(path: &Path, log: &str) -> Banner {
        let details = log.trim();
        let text = load_error_summary(log);
        Banner {
            title: Some(format!("Couldn't open {}", file_name(path))),
            details: (details.lines().count() > 1 || details != text).then(|| details.to_string()).filter(|d| !d.is_empty()),
            text, offer_open: true, offer_restore: None,
        }
    }
}

/// One readable sentence from a loader log such as
/// `(23): Invalid data for 'frequency'\nException: NumberFormatException: ""\nfailed`.
/// The line that caused the failure (the one before the first `Exception:`, or else
/// the first line) is used; `(N):` becomes `Line N:`, Java exception prefixes and the
/// lone trailing `failed` are dropped.
fn load_error_summary(log: &str) -> String {
    if log.contains("mismatched number of read lines") || log.contains("misformed end-of-data line") {
        return "Part of this budget file is missing, so it may have been damaged.".into();
    }
    let lines: Vec<&str> = log.lines().map(str::trim).filter(|l| !l.is_empty() && *l != "failed").collect();
    let exc = lines.iter().position(|l| l.starts_with("Exception:"));
    let head = match exc { Some(i) if i > 0 => lines[i - 1], _ => lines.first().copied().unwrap_or("") };
    let clean = |s: &str| -> String {
        let mut s = s.trim();
        if let Some(rest) = s.strip_prefix("Exception:") { s = rest.trim_start(); }
        // Drop a Java class prefix such as `NumberFormatException:` or `java.io.IOException:`.
        if let Some((cls, rest)) = s.split_once(':') {
            if cls.ends_with("Exception") && !cls.contains(' ') { s = rest.trim_start(); }
        }
        s.strip_suffix("failed").map(str::trim_end).unwrap_or(s).to_string()
    };
    let mut out = clean(head);
    if let Some(rest) = out.strip_prefix('(') {
        if let Some((n, tail)) = rest.split_once("):") {
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) { out = format!("Line {n}: {}", tail.trim_start()); }
        }
    }
    // "unable to open file." alone says nothing: add the exception's own message.
    if exc == Some(0) || (exc.is_some() && out.to_ascii_lowercase().starts_with("unable to open")) {
        let reason = clean(lines[exc.unwrap()]);
        if !reason.is_empty() && reason != "\"\"" && !out.contains(&reason) {
            out = if out.is_empty() { reason } else { format!("{}: {reason}", out.trim_end_matches('.')) };
        }
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => "The file could not be read.".to_string(),
    }
}

#[derive(Clone)]
enum PendingAction { Open(PathBuf), Close, New(PathBuf), CloseBudget, Shared(Box<shared_client::SharedDocument>), Restore { budget: PathBuf, backup: PathBuf } }

/// Adds `.bgt` unless the name already ends with it (any case). Only the end of the name counts:
/// `Household 2026.10` becomes `Household 2026.10.bgt`.
fn with_bgt_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bgt")) { return path; }
    let mut name = path.into_os_string();
    name.push(".bgt");
    PathBuf::from(name)
}
#[derive(Clone, Copy)]
struct DeleteTarget { id: i32, date: Option<i32> }

/// An encrypted budget that was read and waits for its password.
struct PasswordPrompt {
    path: PathBuf,
    /// The file as read: unlocking builds the budget from these, without reading the file again.
    bytes: Vec<u8>,
    typed: String,
    error: String,
    /// One authentication/build attempt. Polling and cancellation never block the UI.
    attempt: Option<Opening>,
    /// `path` is a backup being restored as a draft of this budget.
    restore_to: Option<PathBuf>,
    /// A failed attempt found backups to offer in place of this file.
    has_backup: bool,
}

/// Whether a budget that parsed can be trusted to be whole.
#[derive(Clone, Copy, PartialEq)]
enum Completeness {
    Complete,
    /// No end-of-data mark: it may have been cut short (see `bgt::Loaded::missing_end_marker`).
    MaybeCutShort { has_backup: bool },
}

/// What reading a budget found.
enum Loaded {
    Budget(Box<BudgetDb>, Vec<u8>, Completeness),
    /// A backup built as a draft of `budget`. `current` is what the budget file holds now.
    Restored { db: Box<BudgetDb>, budget: PathBuf, current: Option<Vec<u8>>, complete: bool },
    /// Encrypted and no password given: the bytes wait for the password prompt, with the budget
    /// they are a draft of when they come from a backup.
    Locked(Vec<u8>, Option<PathBuf>),
    /// The loader's log, and whether the budget has backups to offer.
    Failed(String, bool),
}

/// A budget that parsed but may be cut short. It is held here, unopened, until the user decides.
struct IncompleteBudget { path: PathBuf, password: Option<String>, db: Box<BudgetDb>, bytes: Vec<u8>, quiet: bool, has_backup: bool }

/// File > Restore previous version.
struct RestoreDialog {
    budget: PathBuf,
    /// The listing, read off the window thread (a share may stall).
    listing: Option<file_probe::Task<Vec<bgt::Backup>>>,
    backups: Vec<bgt::Backup>,
}

/// The password prompt's line. A recovery copy has a file name nobody has seen, so its budget is named.
fn encrypted_line(path: &Path, restore_to: Option<&Path>) -> String {
    match restore_to.filter(|_| recovery::is_copy(path)) {
        Some(budget) => format!("Unsaved work for {} is encrypted.", file_name(budget)),
        None => format!("{} is encrypted.", file_name(path)),
    }
}

/// `2026-10-30 14:05:09`, local time. Seconds tell apart quick saves.
fn backup_time(backup: &bgt::Backup) -> String {
    backup.modified.map_or_else(|| "Unknown date".to_string(), |time| chrono::DateTime::<chrono::Local>::from(time).format("%Y-%m-%d %H:%M:%S").to_string())
}

fn file_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} bytes"),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// What building a budget depends on besides the file.
#[derive(Clone, Copy, PartialEq)]
struct LoadSettings { today: i32, month: i32, future_months: i32, unlimited_history: bool }

/// Reads `path` (unless its bytes are given) and builds the budget. A background open runs this on
/// a worker thread, so a budget on an unreachable share never freezes the window.
fn load_budget(read: &file_probe::Reader, path: &Path, bytes: Option<Vec<u8>>, password: Option<&str>, s: LoadSettings) -> Loaded {
    let has_backup = || !bgt::list_backups(path).is_empty();
    let bytes = match bytes.map_or_else(|| read(path), Ok) {
        Ok(bytes) => bytes,
        Err(e) => return Loaded::Failed(e.to_string(), has_backup()),
    };
    let text = bgt::latin1_decode(&bytes);
    if password.is_none() && bgt::is_encrypted_text(&text) { return Loaded::Locked(bytes, None); }
    let cur = Cal::from_int(s.month);
    let mut cut_short = false;
    let built = bgt::load_str(&text, s.today, password).map_err(|l| l.log).and_then(|loaded| {
        cut_short = loaded.missing_end_marker;
        // A malformed file must surface as an error banner, never take the whole app down.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut db = BudgetDb::from_loaded(loaded, &cur, s.future_months, s.unlimited_history);
            // One "today" for the grid and the tentative scheduler (--today included).
            if db.today != s.today { db.today = s.today; db.rebuild_db(&cur, s.future_months, s.unlimited_history); }
            db
        })).map_err(|_| "This budget file could not be built: it contains values the schedule engine cannot handle.".to_string())
    });
    match built {
        Ok(mut db) => {
            db.file_name = Some(path.display().to_string());
            let completeness = if cut_short { Completeness::MaybeCutShort { has_backup: has_backup() } } else { Completeness::Complete };
            Loaded::Budget(Box::new(db), bytes, completeness)
        }
        Err(log) => Loaded::Failed(log, has_backup()),
    }
}

/// Builds the backup `backup` as a draft of `budget`, whose file is only read, never written.
fn load_backup(read: &file_probe::Reader, budget: &Path, backup: &Path, bytes: Option<Vec<u8>>, password: Option<&str>, s: LoadSettings) -> Loaded {
    let current = match read(budget) {
        Ok(current) => Some(current),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Loaded::Failed(format!("The budget file is unavailable: {e}"), false),
    };
    match load_budget(read, backup, bytes, password, s) {
        Loaded::Budget(db, _, completeness) => Loaded::Restored { db, budget: budget.to_path_buf(), current, complete: completeness == Completeness::Complete },
        Loaded::Locked(bytes, _) => Loaded::Locked(bytes, Some(budget.to_path_buf())),
        failed => failed,
    }
}

/// A budget being read and built on a worker thread.
struct Opening {
    path: PathBuf,
    password: Option<String>,
    /// The edit count when the open was decided: a later edit is never replaced unasked.
    edits: u64,
    /// Opened automatically (the last budget): no "Opened" notice.
    quiet: bool,
    settings: LoadSettings,
    task: file_probe::Task<Loaded>,
    /// The finished read, kept while an unsaved-changes prompt is showing.
    loaded: Option<Loaded>,
    /// `path` is a backup being restored as a draft of this budget.
    restore_to: Option<PathBuf>,
    /// The offer being answered, when `path` is a recovery copy: it comes back if the open cannot finish.
    offer: Option<recovery::Offer>,
}

/// The unsaved-changes prompt's answers.
#[derive(Clone, Copy)]
enum UnsavedChoice {
    /// Return to the editor (a draft) or Cancel: the pending open or close is dropped.
    Back,
    /// Discard the draft or the unsaved changes.
    Discard,
    Save,
    /// Save a separate copy (offered after a failed save).
    SaveCopy,
}

#[derive(Clone, Copy, PartialEq)]
enum DeleteChoice { Cancel, Delete }

/// How an open dialog counts for the modal checks.
#[derive(Clone, Copy, PartialEq)]
enum DialogKind {
    /// An editor holding unapplied changes.
    Draft,
    /// The keyboard shortcuts sheet, which F1 toggles while nothing else is open.
    Shortcuts,
    Other,
}

/// Every dialog that can cover the main window. `Dialogs::open` lists them without `..`, so a
/// new one does not compile until it says how it counts.
#[derive(Default)]
struct Dialogs {
    payment_editor: Option<PaymentEditor>,
    transaction_editor: Option<TransactionEditor>,
    balance_editor: Option<BalanceEditor>,
    /// The day's transaction list.
    day: Option<i32>,
    confirm_delete: Option<DeleteTarget>,
    /// An open or close waiting for the unsaved-changes prompt.
    pending: Option<PendingAction>,
    /// Why the last save failed; the unsaved-changes prompt shows this, not an unrelated banner.
    save_error: Option<String>,
    /// An encrypted budget waiting for its password.
    password: Option<PasswordPrompt>,
    /// A budget that may be cut short, waiting for the user's decision.
    incomplete: Option<IncompleteBudget>,
    restore: Option<RestoreDialog>,
    /// Unsaved work kept from a crash, waiting for Restore or Discard.
    recovery: Option<recovery::Offer>,
    about: bool,
    legend: bool,
    shortcuts: bool,
}

impl Dialogs {
    /// The kinds of the open dialogs.
    fn open(&self) -> impl Iterator<Item = DialogKind> {
        // `save_error` is the text of the unsaved-changes prompt, not a dialog of its own.
        let Dialogs { payment_editor, transaction_editor, balance_editor, day, confirm_delete, pending, password, incomplete, restore, recovery, about, legend, shortcuts, save_error: _ } = self;
        [
            (payment_editor.is_some(), DialogKind::Draft),
            (transaction_editor.is_some(), DialogKind::Draft),
            (balance_editor.is_some(), DialogKind::Draft),
            (day.is_some(), DialogKind::Other),
            (confirm_delete.is_some(), DialogKind::Other),
            (pending.is_some(), DialogKind::Other),
            (password.is_some(), DialogKind::Other),
            (incomplete.is_some(), DialogKind::Other),
            (restore.is_some(), DialogKind::Other),
            (recovery.is_some(), DialogKind::Other),
            (*about, DialogKind::Other),
            (*legend, DialogKind::Other),
            (*shortcuts, DialogKind::Shortcuts),
        ].into_iter().filter_map(|(open, kind)| open.then_some(kind))
    }

    /// An editor holds unapplied changes.
    fn has_draft(&self) -> bool { self.open().any(|kind| kind == DialogKind::Draft) }
    fn any(&self) -> bool { self.open().next().is_some() }
    fn any_but_shortcuts(&self) -> bool { self.open().any(|kind| kind != DialogKind::Shortcuts) }
}

/// Day dialog input state carried between frames.
#[derive(Clone, Copy, Default)]
struct DayDialogInput {
    /// Input time of the first frame the dialog was shown.
    opened_at: Option<f64>,
    /// A More menu was open at the end of the previous frame.
    menu_open: bool,
    /// The current press must not close the dialog when it is released on the backdrop.
    ignore_release: bool,
}

struct App {
    sharing: sharing::SharedUi,
    cfg: BudgetConfig,
    rc_path: PathBuf,
    /// Switches `today` when the local date changes (fixed with `--today`).
    date_watch: DateWatch,
    /// Background existence checks of remembered budgets.
    files: FileProbes,
    /// The last budget, opened once its background check finds it.
    auto_open: Option<PathBuf>,
    /// A budget being read and built in the background.
    opening: Option<Opening>,
    /// Counts applied edits, so an open finishing later never replaces newer ones unasked.
    edits: u64,
    /// The control that had keyboard focus when a dialog opened; it gets focus back on close.
    focus_return: Option<egui::Id>,
    /// Keeps a recovery copy of unsaved edits.
    journal: recovery::Journal,
    /// The steps Undo and Redo walk through.
    undo: undo::History,
    db: Option<BudgetDb>,
    file: Option<PathBuf>,
    password: Option<String>,
    saved_bytes: Option<Vec<u8>>,
    dirty: bool,
    dialogs: Dialogs,
    drag_month_hover: Option<(i32, Instant)>,
    show_payments: bool,
    payment_list: payment_list::PaymentList,
    allow_close: bool,
    /// First day of the viewed month.
    cur: Cal,
    today: i32,
    filter_text: String,
    filter: Option<Filter>,
    filter_error: Option<String>,
    /// Counts the budgets opened, so a cache never mistakes a new budget for the previous one.
    generation: u64,
    /// The month on screen without a search (rebuilt when the document, month or settings change)...
    month: Option<view::Month>,
    /// ...narrowed to the search text: what the calendar and summary draw.
    view: Option<MonthView>,
    /// The search text of every payment for the document revision it was made from.
    haystacks: Option<((u64, i32), Arc<Haystacks>)>,
    toast: Option<Toast>,
    /// Persistent error until dismissed.
    banner: Option<Banner>,
    /// `--screenshot <png>`: render a few frames, save the window, exit.
    screenshot: Option<PathBuf>,
    frames: u32,
    dark: bool,
    show_summary: bool,
    focus_filter: bool,
    title_dirty: bool,
    brand_texture: Option<egui::TextureHandle>,
    t: Tokens,
}

/// The pass in which a text input of the main window (a search box, the year box) last had keyboard focus.
const TEXT_FOCUS: &str = "text_input_focus";

/// Record that `response` is a text input holding the keyboard focus. egui's own
/// `wants_keyboard_input` is true for any focused widget, so a Tab-focused button would otherwise
/// switch off the month keys and `?`.
pub(crate) fn note_text_focus(ctx: &egui::Context, response: &egui::Response) {
    // Read the pass number first: `data_mut` holds the context lock, and asking the context inside it deadlocks.
    if response.has_focus() {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TEXT_FOCUS), pass));
    }
}

/// Whether a text input had the focus in this pass or the one before (the keys run before the widgets).
fn text_input_has_focus(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(TEXT_FOCUS))).is_some_and(|last| last + 1 >= pass)
}

impl App {
    fn new(cli: Cli) -> App {
        let rc_path = home_dir().join(".budgetrc");
        let cfg = BudgetConfig::load(&rc_path);
        // A simulated today also decides the month first shown (`--month=` still wins below).
        let mut cur = cli.today.map_or_else(Cal::today, Cal::from_int);
        cur.set_day_one();
        let t = Tokens::new(&cfg.colors, cli.dark);
        let mut app = App {
            sharing: sharing::SharedUi::default(),
            date_watch: if cli.today.is_some() { DateWatch::fixed() } else { DateWatch::system() },
            files: FileProbes::default(), auto_open: None, opening: None, edits: 0, focus_return: None,
            journal: recovery::Journal::new(recovery::app_dir().join("recovery")), undo: undo::History::default(),
            cfg, rc_path, db: None, file: None, password: None, cur, today: cli.today.unwrap_or_else(|| Cal::today().get()),
            saved_bytes: None, dirty: false, dialogs: Dialogs::default(), drag_month_hover: None,
            show_payments: false, payment_list: payment_list::PaymentList::default(), allow_close: false,
            filter_text: String::new(), filter: None, filter_error: None, generation: 0, month: None, view: None, haystacks: None,
            toast: None, banner: None,
            screenshot: cli.screenshot, frames: 0, dark: cli.dark, show_summary: true, focus_filter: false, title_dirty: true, brand_texture: None, t,
        };
        if app.screenshot.is_none() { app.journal.register(); }
        if let Some(m) = cli.month { app.cur = Cal::from_int(m); app.cur.set_day_one(); }
        if let Some(f) = cli.filter { app.filter_text = f; app.set_filter(); }
        if let Some(f) = cli.file {
            // A file named on the command line never falls back to the last-used budget.
            let p = resolve_path(Path::new(f.trim()));
            if p.is_file() { app.load_at_start(&p); }
            else {
                app.banner = Some(Banner { text: format!("No file was found at {}.", p.display()), details: None, ..Banner::load_failure(&p, "") });
                app.offer_recovery(&p, None);
            }
        } else if let Some(f) = app.cfg.last_file_name.clone() {
            // The last budget may sit on an unreachable share, whose check blocks for ~20 s: look for
            // it off the UI thread and open it when it turns up (see `finish_auto_open`).
            let p = resolve_path(Path::new(f.trim()));
            match app.files.wait(&p, Duration::from_millis(250)) {
                FileStatus::Present(_) => app.load_at_start(&p),
                FileStatus::Checking => app.auto_open = Some(p),
                FileStatus::Missing => app.offer_recovery(&p, None),
            }
        }
        app
    }

    /// Opens the last budget once its slow background check finds it, unless the user opened
    /// something else meanwhile.
    fn finish_auto_open(&mut self) {
        let Some(path) = self.auto_open.clone() else { return };
        match self.files.status(&path) {
            FileStatus::Checking => {}
            FileStatus::Present(_) if self.has_modal() => {}
            FileStatus::Present(_) => {
                self.auto_open = None;
                if self.db.is_none() { self.start_opening(&path, None, None, true); }
            }
            FileStatus::Missing => { self.auto_open = None; self.offer_recovery(&path, None); }
        }
    }

    /// The original's 20 s date-change timer (analysis/03 §11.4), switching without asking:
    /// the new day becomes today for the grid, the filters and the tentative scheduler.
    fn check_date(&mut self, ctx: &egui::Context) {
        if let Some(doc) = &self.sharing.document {
            ctx.request_repaint_after(Duration::from_secs(20));
            if let Ok(today) = shared_client::household_today(&doc.working.time_zone) {
                if today != self.today && !egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(ctx) { self.set_today(today); }
            }
            return;
        }
        let (date, next) = self.date_watch.poll(Instant::now());
        // Wake an idle window for the next check.
        if let Some(next) = next { ctx.request_repaint_after(next); }
        // A dragged transaction keeps the schedule it was picked up from; switch on a later check.
        if egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(ctx) { return; }
        if let Some(date) = date.filter(|d| *d != self.today) { self.set_today(date); }
    }

    fn set_today(&mut self, date: i32) {
        // Someone looking at the current month keeps looking at the current month (31 Oct → 1 Nov).
        let follow = self.cur.get() / 100 == self.today / 100;
        self.today = date;
        if follow { self.cur = Cal::from_int(date); self.cur.set_day_one(); }
        // Relative filters such as range: are evaluated against today.
        if !self.filter_text.trim().is_empty() { self.set_filter(); }
        if let Some(db) = &mut self.db {
            db.today = date;
            db.rebuild_db(&self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
        }
        self.view = None;
    }

    fn notify(&mut self, text: impl Into<String>) {
        let text = text.into();
        let text = if self.sharing.document.is_some() && text.contains("Press Ctrl+S to save") {
            if self.dirty { "Change applied but not saved on this device. See the error message.".into() }
            else { "Saved on this device. Changes are queued for household sync.".into() }
        } else { text };
        self.toast = Some(Toast { text, error: false, until: Instant::now() + Duration::from_secs(4) });
    }
    /// A brief warning that needs no dismissing (unlike the persistent error banner).
    fn warn(&mut self, text: impl Into<String>) { self.toast = Some(Toast { text: text.into(), error: true, until: Instant::now() + Duration::from_secs(5) }); }
    fn fail(&mut self, text: impl Into<String>) { self.banner = Some(Banner::plain(text)); }
    fn fail_load(&mut self, path: &Path, log: &str, has_backup: bool) {
        self.banner = Some(Banner { offer_restore: has_backup.then(|| path.to_path_buf()), ..Banner::load_failure(path, log) });
    }

    fn open(&mut self, path: &Path) {
        // An explicit open replaces any pending automatic open of the last budget.
        self.auto_open = None;
        let path = &resolve_path(path);
        if self.dirty || self.has_draft() {
            self.dialogs.pending = Some(PendingAction::Open(path.to_path_buf()));
            return;
        }
        self.open_unchecked(path);
    }

    /// Opens `path` once any unsaved changes were dealt with. The file is read and built on a
    /// worker thread (an unreachable share must not freeze the window); `finish_opening` takes the
    /// result.
    fn open_unchecked(&mut self, path: &Path) { self.start_opening(path, None, None, false); }

    /// File > New budget: asks where the new budget is saved, then creates it.
    fn new_budget_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).set_file_name("budget.bgt");
        if let Some(path) = dialog.save_file() { self.start_new_budget(with_bgt_extension(path)); }
    }

    /// File > New budget, once a destination was chosen. Unsaved changes are asked about first;
    /// the file is written only when the new budget is created.
    fn start_new_budget(&mut self, path: PathBuf) {
        let path = resolve_path(&path);
        if path.exists() {
            self.fail(format!("{} already exists. Choose a new name for the new budget.", file_name(&path)));
            return;
        }
        self.auto_open = None;
        if self.dirty || self.has_draft() {
            self.dialogs.pending = Some(PendingAction::New(path));
            return;
        }
        self.create_budget(&path);
    }

    /// Writes an empty budget starting this month and opens it. An existing file is never replaced.
    fn create_budget(&mut self, path: &Path) {
        if path.exists() {
            self.fail(format!("{} already exists. Choose a new name for the new budget.", file_name(path)));
            return;
        }
        let mut cur = Cal::from_int(self.today);
        cur.set_day_one();
        let db = BudgetDb::new(&cur, self.cfg.future_n_months, self.cfg.unlimited_history);
        match document::save_checked(&db, path, None, None) {
            Ok(_) => self.open_unchecked(path),
            Err(e) => self.fail(e),
        }
    }

    /// File > Close budget: asks about unsaved changes first, then returns to the empty state.
    fn request_close_budget(&mut self) {
        if self.db.is_none() { return; }
        if self.dirty || self.has_draft() {
            self.dialogs.pending = Some(PendingAction::CloseBudget);
            return;
        }
        self.close_budget();
    }

    /// Drops the open budget and its editors; the window shows the empty state. The closed budget is
    /// not reopened at the next start.
    fn close_budget(&mut self) {
        self.journal.clear();
        self.sharing.disconnect();
        // A close requested while another budget is still being read wins over that open.
        self.opening = None;
        self.auto_open = None;
        self.db = None;
        self.file = None;
        self.password = None;
        self.saved_bytes = None;
        self.dirty = false;
        self.undo = undo::History::default();
        self.month = None;
        self.view = None;
        self.haystacks = None;
        self.dialogs.payment_editor = None;
        self.dialogs.transaction_editor = None;
        self.dialogs.balance_editor = None;
        self.dialogs.day = None;
        self.dialogs.confirm_delete = None;
        self.drag_month_hover = None;
        self.payment_list.selected = None;
        // The empty state has no Payments page: its toolbar and a pending search focus would be stale.
        self.show_payments = false;
        self.payment_list.focus_search = false;
        self.banner = None;
        self.dialogs.save_error = None;
        self.title_dirty = true;
        self.cfg.last_file_name = None;
        self.save_config();
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).pick_file() { self.open(&p); }
    }

    /// File > Restore previous version: lists the backups of `budget`. Nothing is read or changed
    /// until one is picked.
    fn open_restore(&mut self, budget: &Path) {
        let owned = budget.to_path_buf();
        let listing = self.files.spawn(move |_| bgt::list_backups(&owned));
        self.dialogs.restore = Some(RestoreDialog { budget: budget.to_path_buf(), listing: Some(listing), backups: Vec::new() });
    }

    /// Opens `backup` as a draft of `budget`, once any unsaved changes were dealt with.
    fn restore_backup(&mut self, budget: PathBuf, backup: PathBuf) {
        self.auto_open = None;
        if self.dirty || self.has_draft() {
            self.dialogs.pending = Some(PendingAction::Restore { budget, backup });
            return;
        }
        self.restore_unchecked(budget, &backup);
    }

    /// The backup is read and built in the background, like any open.
    fn restore_unchecked(&mut self, budget: PathBuf, backup: &Path) {
        self.opening = Some(self.opening_job(backup, None, None, false, Some(budget)));
    }

    fn load_settings(&self) -> LoadSettings {
        LoadSettings { today: self.today, month: self.cur.get(), future_months: self.cfg.future_n_months, unlimited_history: self.cfg.unlimited_history }
    }

    /// Reads (unless `bytes` are given) and builds `path` in the background. Opening the budget
    /// already being read changes nothing; another one replaces it.
    fn start_opening(&mut self, path: &Path, bytes: Option<Vec<u8>>, password: Option<String>, quiet: bool) {
        if bytes.is_none() {
            if let Some(opening) = self.opening.as_mut().filter(|o| o.path == path) {
                // Already being read. This request has passed the unsaved-changes prompt, so edits
                // made meanwhile must not make the finished open ask again.
                opening.edits = self.edits;
                return;
            }
        }
        self.opening = Some(self.opening_job(path, bytes, password, quiet, None));
    }

    fn opening_job(&self, path: &Path, bytes: Option<Vec<u8>>, password: Option<String>, quiet: bool, restore_to: Option<PathBuf>) -> Opening {
        let settings = self.load_settings();
        let (job_path, job_password, job_budget) = (path.to_path_buf(), password.clone(), restore_to.clone());
        let task = self.files.spawn(move |read| match job_budget {
            Some(budget) => load_backup(read, &budget, &job_path, bytes, job_password.as_deref(), settings),
            None => load_budget(read, &job_path, bytes, job_password.as_deref(), settings),
        });
        Opening { path: path.to_path_buf(), password, edits: self.edits, quiet, settings, task, loaded: None, restore_to, offer: None }
    }

    /// Takes a finished background open. Edits made while the file was read, or an editor opened
    /// meanwhile, are never replaced unasked: the open then waits for the unsaved-changes prompt,
    /// as any open does. A close or another open requested meanwhile wins. A finished read also
    /// waits, unapplied, while a prompt is showing, so the open this user asked for is not lost.
    fn finish_opening(&mut self) {
        let Some(opening) = &mut self.opening else { return };
        if opening.loaded.is_none() {
            opening.loaded = match opening.task.poll() {
                TaskState::Running => return,
                TaskState::Done(loaded) => Some(loaded),
                TaskState::Lost => Some(Loaded::Failed("The budget could not be read.".into(), false)),
            };
        }
        if self.dialogs.pending.is_some() { return; }
        let Some(Opening { path, password, edits, quiet, settings, loaded, restore_to, offer, .. }) = self.opening.take() else { return };
        let Some(mut loaded) = loaded else { return };
        if edits != self.edits || self.has_draft() {
            // A recovery copy is never reopened by name: the prompt could lead to removing it unread.
            if offer.is_some() { self.dialogs.recovery = offer; return; }
            self.dialogs.pending = Some(match restore_to {
                Some(budget) => PendingAction::Restore { budget, backup: path },
                None => PendingAction::Open(path),
            });
            return;
        }
        // Built around the month and date of the click: follow any change since, as a load now would.
        if let Loaded::Budget(db, ..) | Loaded::Restored { db, .. } = &mut loaded {
            if settings != self.load_settings() {
                db.today = self.today;
                db.rebuild_db(&self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
            }
        }
        self.apply_loaded(&path, password, loaded, quiet);
    }

    /// Reads and builds `path` on this thread: at startup, before the window is shown.
    fn load(&mut self, path: &Path, password: Option<&str>) {
        let path = &resolve_path(path);
        let read: file_probe::Reader = Arc::new(|path: &Path| std::fs::read(path));
        let loaded = load_budget(&read, path, None, password, self.load_settings());
        self.apply_loaded(path, password.map(str::to_string), loaded, false);
    }

    /// At startup: no "Opened" notice, but a warning about the budget stays.
    fn load_at_start(&mut self, path: &Path) {
        self.load(path, None);
        self.toast = self.toast.take().filter(|toast| toast.error);
    }

    fn apply_loaded(&mut self, path: &Path, password: Option<String>, loaded: Loaded, quiet: bool) {
        match loaded {
            // Never opened silently: it stays out of the window, and out of reach of Save, until the user decides.
            Loaded::Budget(db, bytes, Completeness::MaybeCutShort { has_backup }) => {
                // A save cut short by the crash is the file a recovery copy is most useful for.
                self.offer_recovery(path, None);
                self.dialogs.incomplete = Some(IncompleteBudget { path: path.to_path_buf(), password, db, bytes, quiet, has_backup });
            }
            Loaded::Budget(db, bytes, Completeness::Complete) => {
                let current = recovery::fingerprint(&bytes);
                self.adopt(*db, path, password, Some(bytes));
                self.dirty = false;
                if !quiet { self.notify(format!("Opened {}", file_name(path))); }
                self.offer_recovery(path, Some(current));
            }
            Loaded::Restored { mut db, budget, current, complete } => {
                // The same budget is open: its conflict check and its password stay as they are, so
                // saving an older version does not change who can open the file.
                let same = self.file.as_deref().is_some_and(|f| same_file(f, &budget));
                let baseline = if same { self.saved_bytes.clone().or_else(|| current.clone()) } else { current.clone() };
                let password = self.password.clone().filter(|_| same).or(password);
                let encrypted_now = current.is_some_and(|bytes| bgt::is_encrypted_text(&bgt::latin1_decode(&bytes)));
                let unprotected = encrypted_now && password.is_none();
                db.file_name = Some(budget.display().to_string());
                self.adopt(*db, &budget, password, baseline);
                self.dirty = true;
                self.edits += 1;
                // This draft writes to the same place as a copy nobody has answered for yet.
                if !recovery::is_copy(path) { self.offer_recovery(&budget, None); }
                self.journal_edit();
                if recovery::is_copy(path) { self.notify("Unsaved work restored. Your file is unchanged until you save."); }
                else if unprotected { self.warn("This earlier version has no password. Saving it would store your budget without one."); }
                else if !complete { self.warn("This earlier version may be incomplete too. Check it before you save."); }
                else { self.notify("Earlier version loaded. Your file is unchanged until you save."); }
            }
            Loaded::Locked(bytes, restore_to) => {
                self.dialogs.password = Some(PasswordPrompt { path: path.to_path_buf(), bytes, typed: String::new(), error: String::new(), attempt: None, restore_to, has_backup: false });
            }
            Loaded::Failed(log, has_backup) => { self.fail_load(path, &log, has_backup); self.offer_recovery(path, None); }
        }
    }

    /// Offers a recovery copy kept for `path`, whose file has the fingerprint `current`.
    fn offer_recovery(&mut self, path: &Path, current: Option<u64>) {
        if self.screenshot.is_some() { return; }
        match recovery::find(self.journal.dir(), path, current) {
            recovery::Found::Nothing => {}
            recovery::Found::Offer(offer) => self.dialogs.recovery = Some(offer),
            recovery::Found::Unreadable => self.warn("A recovery copy of this budget could not be read."),
        }
    }

    /// Restore loads the copy like an earlier version, as an unsaved draft of the budget.
    /// Discard removes it. Either way the budget file stays as it is.
    fn answer_recovery(&mut self, restore: bool) {
        let Some(offer) = self.dialogs.recovery.take() else { return };
        if restore {
            self.dialogs.incomplete = None;
            let password = self.password.clone().filter(|_| self.file.as_deref().is_some_and(|f| same_file(f, &offer.budget)));
            let mut job = self.opening_job(&offer.file, Some(offer.bytes.clone()), password, false, Some(offer.budget.clone()));
            job.offer = Some(offer);
            self.opening = Some(job);
        } else {
            self.journal.remove(&offer.budget);
            // A draft already on screen, from a backup, now has the copy to itself.
            if self.dirty { self.journal_edit(); }
        }
    }

    /// A recovery copy is being read. The window waits: an edit now would queue a copy over it.
    fn restoring_copy(&self) -> bool { self.opening.as_ref().is_some_and(|opening| recovery::is_copy(&opening.path)) }

    /// `budget` has a recovery copy the user has not seen through yet, so nothing may replace it.
    fn copy_is_waiting(&self, budget: &Path) -> bool {
        self.dialogs.recovery.as_ref().is_some_and(|offer| offer.budget == budget)
            || self.opening.as_ref().is_some_and(|opening| recovery::is_copy(&opening.path) && opening.restore_to.as_deref() == Some(budget))
            || self.dialogs.password.as_ref().is_some_and(|prompt| recovery::is_copy(&prompt.path) && prompt.restore_to.as_deref() == Some(budget))
    }

    /// The edits of `budget` are safe or given up on, so its copy goes, unless it holds other work.
    fn forget_copy(&mut self, budget: &Path) {
        if self.copy_is_waiting(budget) { self.journal.clear(); } else { self.journal.remove(budget); }
    }

    fn restoring_prompt(&mut self, ctx: &egui::Context) {
        egui::Modal::new(egui::Id::new("restoring")).frame(self.t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.horizontal(|ui| { ui.spinner(); ui.label("Restoring your unsaved work…"); });
        });
    }

    fn recovery_prompt(&mut self, ctx: &egui::Context) {
        let Some(offer) = &self.dialogs.recovery else { return };
        let t = self.t.clone();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("recovery")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading("Unsaved work found");
            ui.add_space(SP2);
            ui.label(RichText::new(file_name(&offer.budget)).text_style(ts(SEMI)));
            ui.add(egui::Label::new(RichText::new("Budget Calendar stopped before this work was saved, and kept a copy on this device. Restoring opens it as unsaved changes. Your file stays as it is until you save.").color(t.text_muted)).wrap());
            if offer.file_changed {
                ui.add_space(SP2);
                ui.add(egui::Label::new(RichText::new("The budget file has changed since this copy was started. Check both before you save.").color(t.negative)).wrap());
            }
            ui.add_space(SP4);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, format!("Restore unsaved work from {}", offer.written), &t).clicked() { choice = Some(true); }
                if theme::secondary_button(ui, "Discard", &t).clicked() { choice = Some(false); }
            });
        });
        if let Some(restore) = choice { self.answer_recovery(restore); }
    }

    /// Makes `db` the open budget at `path`, replacing the one in the window.
    fn adopt(&mut self, db: BudgetDb, path: &Path, password: Option<String>, baseline: Option<Vec<u8>>) {
        self.journal.clear();
        self.sharing.disconnect();
        self.db = Some(db);
        self.generation += 1;
        self.file = Some(path.to_path_buf());
        self.password = password;
        self.saved_bytes = baseline;
        self.dialogs.payment_editor = None;
        self.dialogs.transaction_editor = None;
        self.dialogs.balance_editor = None;
        // A pending confirmation names a payment of the replaced budget by id.
        self.dialogs.confirm_delete = None;
        self.drag_month_hover = None;
        self.payment_list.selected = None;
        self.dialogs.day = None;
        self.remember_file(path);
        self.view = None;
        self.banner = None;
        self.dialogs.save_error = None;
        self.title_dirty = true;
    }

    fn save(&mut self) -> bool {
        if self.sharing.document.is_some() { return self.save_shared(); }
        if self.db.is_some() && !self.dirty { return true; }
        let Some(path) = self.file.clone() else { return false };
        let expected = self.saved_bytes.clone();
        self.save_to(&path, expected.as_deref())
    }

    fn save_as(&mut self) -> bool {
        if self.sharing.document.is_some() { return self.export_shared_copy(); }
        let dialog = rfd::FileDialog::new().add_filter("Budget", &["bgt"])
            .set_file_name(self.file.as_deref().map(file_name).unwrap_or_else(|| "budget.bgt".into()));
        let Some(path) = dialog.save_file() else { return false };
        let path = with_bgt_extension(path);
        let expected = match self.save_as_baseline(&path) {
            Ok(expected) => expected,
            Err(e) => { self.dialogs.save_error = Some(e.clone()); self.fail(e); return false; }
        };
        self.save_to(&path, expected.as_deref())
    }

    /// The bytes a Save as destination must still hold: the open file keeps its
    /// opened snapshot however its path is spelled, so external changes are refused.
    fn save_as_baseline(&self, path: &Path) -> Result<Option<Vec<u8>>, String> {
        if self.file.as_deref().is_some_and(|f| same_file(f, path)) { return Ok(self.saved_bytes.clone()); }
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Unable to inspect the destination: {e}")),
        }
    }

    /// Record the open budget in .budgetrc, except in screenshot automation runs.
    fn remember_file(&mut self, path: &Path) {
        if self.screenshot.is_some() { return; }
        // The same budget may be spelled differently (drive letter or folder case): one entry only.
        let key = recent_key(path);
        self.cfg.recent_file.retain(|f| recent_key(Path::new(f)) != key);
        self.cfg.last_file_name = Some(path.display().to_string());
        self.cfg.add_recent_file(&path.display().to_string());
        let _ = self.cfg.save(&self.rc_path);
    }

    /// saveRc on exit: preferences and the last file survive a close without a budget save.
    fn save_config(&mut self) {
        if self.screenshot.is_some() { return; }
        if let Some(f) = &self.file { self.cfg.last_file_name = Some(f.display().to_string()); }
        let _ = self.cfg.save(&self.rc_path);
    }

    fn save_to(&mut self, path: &Path, expected: Option<&[u8]>) -> bool {
        if self.sharing.document.is_some() { self.fail("Use Export copy to save a shared budget as a standalone file."); return false; }
        let path = &resolve_path(path);
        let Some(db) = &self.db else { return false };
        match document::save_checked(db, path, self.password.as_deref(), expected) {
            Ok(bytes) => {
                // Save as leaves the edits safe under the new name, so neither name needs a copy.
                if let Some(previous) = self.file.replace(path.to_path_buf()) { self.forget_copy(&previous); }
                self.forget_copy(path);
                if let Some(db) = &mut self.db { db.file_name = Some(path.display().to_string()); }
                self.saved_bytes = Some(bytes);
                self.dirty = false;
                self.undo.mark_saved();
                // The edits just saved must not make a background open that is pending ask again.
                if let Some(opening) = &mut self.opening { opening.edits = self.edits; }
                self.title_dirty = true;
                self.banner = None;
                self.dialogs.save_error = None;
                self.remember_file(path);
                self.notify(format!("Saved {}", file_name(path)));
                true
            }
            Err(e) => { self.dialogs.save_error = Some(e.clone()); self.fail(e); false }
        }
    }

    fn go(&mut self, months: i32) {
        let mut target = self.cur;
        target.add(Field::Month, months);
        // The schedule covers years 1000..=9999: the arrows and keys stop there like the year box.
        if !(budget_core::date::YEAR_MIN..=budget_core::date::YEAR_MAX).contains(&target.year()) { return; }
        self.cur = target;
        self.cur.set_day_one();
        if let Some(db) = &mut self.db {
            if !db.is_month_available(&self.cur) {
                db.rebuild_db(&self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
            }
        }
    }

    fn go_today(&mut self) {
        self.cur = Cal::from_int(self.today);
        self.go(0);
    }

    fn set_filter(&mut self) {
        let t = self.filter_text.trim().to_string();
        self.filter_error = None;
        self.filter = if t.is_empty() { None } else {
            match Filter::new(&t, self.today) {
                // The original's "-" deleted-transactions grid is replaced by Restore in the day menu;
                // say so rather than silently searching without the "-".
                Ok(filter) if filter.deleted => {
                    self.filter_error = Some("A leading \u{2212} (deleted transactions) is not supported. Right-click a day showing the restore icon and choose Restore.".into());
                    None
                }
                Ok(filter) => Some(filter),
                Err(_) => { self.filter_error = Some("Finish entering the search filter, or clear it to show all transactions.".into()); None }
            }
        };
    }

    /// Restoring is offered only while no filter is applied (05 §1.5). Gate on the parsed
    /// filter, not the search text: a rejected leading "-" or unfinished search narrows nothing.
    fn can_restore(&self) -> bool { self.filter.is_none() }

    fn apply_filter(&mut self, text: &str) {
        // clicking the active filter again clears it
        self.filter_text = if self.filter_text == text { String::new() } else { text.to_string() };
        self.set_filter();
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark = dark;
        self.t = Tokens::new(&self.cfg.colors, dark);
        theme::apply(ctx, &self.t);
    }

    /// Brings the month view up to date. A keystroke in the search box reuses the month and the
    /// payments' prepared search text; only a document, month or setting change rebuilds them.
    fn ensure_view(&mut self) {
        let Some(db) = &mut self.db else { (self.view, self.month) = (None, None); return };
        let key = view::MonthKey { revision: (self.generation, db.update_count), month: self.cur.get(), today: self.today,
            decimals: self.cfg.show_decimals_in_cv, sunday_first: self.cfg.first_day_of_week_is_sunday,
            min_balance: self.cfg.min_balance_4_max_saving };
        if self.view.as_ref().is_some_and(|v| v.key.month == key && v.key.search == self.filter_text) { return; }
        if self.month.as_ref().is_none_or(|m| m.key != key) {
            self.month = Some(view::month(db, &self.cfg, &self.cur, self.today, key.clone()));
        }
        if let Some(filter) = &mut self.filter {
            if self.haystacks.as_ref().is_none_or(|(revision, _)| *revision != key.revision) {
                self.haystacks = Some((key.revision, Arc::new(Haystacks::new(&db.payments))));
            }
            filter.set_haystacks(self.haystacks.as_ref().map(|(_, hay)| hay.clone()));
        }
        let Some(month) = &self.month else { return };
        self.view = Some(view::view(month, db, &self.cur, self.filter.as_ref(), &self.filter_text));
    }

    // ---------- header ----------

    fn header(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        ui.spacing_mut().item_spacing.y = SP1;
        // Room for the file name: what is left after the brand, the status pill and the right-hand actions.
        // The "Unsaved changes" pill is 61 px wider than "Saved".
        let name_w = (ui.available_width() - 925.0 - if self.dirty { 62.0 } else { 0.0 }).clamp(0.0, 300.0);
        let brand = self.brand_texture.get_or_insert_with(|| {
            let icon = app_icon();
            ui.ctx().load_texture("budget-calendar-brand",
                egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba),
                egui::TextureOptions::LINEAR)
        }).id();
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            ui.painter().image(brand, mark, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            ui.add_space(SP1);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP2;
                // brand > file > actions: the title is the largest, the version chip the quietest.
                ui.label(RichText::new("Budget Calendar").size(18.0).family(egui::FontFamily::Name("semi".into())).color(t.text));
                if ui.add(Button::new(RichText::new(format!("v{}", version::VERSION)).text_style(ts(CAPTION)).color(t.text_muted)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Current version and change history").clicked() { self.dialogs.about = true; }
                // With no file open the slot stays empty.
                if let Some(path) = self.file.as_deref() {
                    let full = path.display().to_string();
                    ui.label(RichText::new(ph::FILE_TEXT).text_style(ts(ICON)).color(t.text_muted)).on_hover_text(&full);
                    if name_w >= 60.0 {
                        let label = ui.scope(|ui| {
                            ui.set_max_width(name_w);
                            ui.add(egui::Label::new(RichText::new(file_name(path)).text_style(ts(SEMI)).color(t.text)).truncate())
                        }).inner;
                        label.on_hover_text(&full);
                    }
                }
                if self.db.is_some() {
                    let (text, dot) = if self.dirty { ("Unsaved changes", t.important) }
                        else if self.sharing.document.as_ref().is_some_and(|d| d.conflict.is_some()) { ("Sync conflict", t.important) }
                        else if self.sharing.document.as_ref().is_some_and(|d| !d.pending.is_empty()) { ("Saved on device", t.important) }
                        else { ("Saved", t.positive) };
                    theme::status_pill(ui, text, dot, self.dirty, &t)
                        .on_hover_text(if self.dirty { "Press Ctrl+S to save your changes" } else { "Everything is saved to disk" });
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // With no budget there is nothing to add or save: the empty state offers Open… instead.
                if self.db.is_some() {
                    if theme::primary_button(ui, format!("{}  New payment", ph::PLUS), &t).on_hover_text("Add a one-time or repeating payment · Ctrl+N").clicked() { self.new_payment(self.today); }
                    let label = format!("{}  Save changes", ph::FLOPPY_DISK);
                    if self.dirty {
                        // Raised to the accent only while there is something to save.
                        if theme::accent_outline_button(ui, label, &t).on_hover_text("Save your budget · Ctrl+S").clicked() { self.save(); }
                    } else {
                        ui.add_enabled_ui(false, |ui| { theme::secondary_button(ui, label, &t).on_disabled_hover_text("Everything is saved"); });
                    }
                }
                ui.menu_button("File", |ui| {
                    ui.set_min_width(210.0);
                    if menu_item(ui, "Open budget…", "Ctrl+O").clicked() { ui.close_menu(); self.open_dialog(); }
                    if ui.button("New budget…").clicked() { ui.close_menu(); self.new_budget_dialog(); }
                    if ui.button("Shared budget…").clicked() { ui.close_menu(); self.sharing.open = true; }
                    if ui.add_enabled(self.db.is_some(), menu_btn(if self.sharing.document.is_some() { "Export copy…" } else { "Save as…" }, "")).clicked() { ui.close_menu(); self.save_as(); }
                    let restorable = self.file.clone().filter(|_| self.sharing.document.is_none());
                    if ui.add_enabled(restorable.is_some(), egui::Button::new("Restore previous version…")).clicked() {
                        ui.close_menu();
                        if let Some(path) = &restorable { self.open_restore(path); }
                    }
                    if ui.add_enabled(self.db.is_some(), egui::Button::new("Close budget")).clicked() { ui.close_menu(); self.request_close_budget(); }
                    ui.separator();
                    for recent in self.cfg.recent_file.clone() {
                        if ui.button(file_name(Path::new(&recent))).on_hover_text(&recent).clicked() { ui.close_menu(); self.open(Path::new(&recent)); }
                    }
                    ui.separator();
                    if ui.button("Close app").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button("Edit", |ui| {
                    ui.set_min_width(210.0);
                    for (back, shortcut) in [(true, "Ctrl+Z"), (false, "Ctrl+Y")] {
                        let (text, blocked) = self.history_item(back);
                        if ui.add_enabled(blocked.is_none(), menu_btn(&text, shortcut)).on_disabled_hover_text(blocked.unwrap_or_default()).clicked() {
                            ui.close_menu();
                            self.step_history(back);
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    let mut dark = self.dark;
                    if ui.checkbox(&mut dark, "Dark appearance").clicked() { ui.close_menu(); self.set_dark(ui.ctx(), dark); }
                    ui.checkbox(&mut self.show_summary, "Monthly breakdown on wide screens");
                    if ui.checkbox(&mut self.cfg.show_decimals_in_cv, "Show cents in calendar").clicked() { self.view = None; self.save_config(); }
                    ui.separator();
                    if ui.button("Calendar legend…").clicked() { self.dialogs.legend = true; ui.close_menu(); }
                    if ui.button("About and version history…").clicked() { self.dialogs.about = true; ui.close_menu(); }
                    ui.separator();
                    if menu_item(ui, "Keyboard shortcuts", "F1").clicked() { self.dialogs.shortcuts = true; ui.close_menu(); }
                });
                if let Some(result) = reports::menu(ui, &t, self.db.as_mut(), &self.cur, &self.cfg, self.generation) {
                    match result { Ok(done) => self.notify(done), Err(e) => self.fail(e) }
                }
            });
        });
        ui.add_space(6.0);
        self.toolbar(ui);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        let compact = ui.available_width() < 960.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP2;
            Frame::new().fill(t.bg).corner_radius(8).inner_margin(3).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let enabled = self.db.is_some();
                for (payments, icon, label) in [(false, ph::CALENDAR_BLANK, "Calendar"), (true, ph::LIST_BULLETS, "Payments")] {
                    let selected = self.show_payments == payments;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(108.0, 32.0), Sense::click());
                    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, selected, label));
                    let hovered = enabled && resp.hovered();
                    if selected {
                        ui.painter().rect(rect, 6, t.surface, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
                        // 2px accent underline marks the active segment.
                        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.min.x + 14.0, rect.max.y - 3.0), egui::pos2(rect.max.x - 14.0, rect.max.y - 1.0)), 1, t.accent);
                    } else if hovered {
                        ui.painter().rect_filled(rect, 6, t.surface_hover);
                    }
                    if resp.has_focus() { ui.painter().rect_stroke(rect.expand(1.0), 7, Stroke::new(2.0_f32, t.accent), StrokeKind::Outside); }
                    let color = if !enabled { t.text_muted.gamma_multiply(0.6) } else if selected { t.accent } else if hovered { t.text } else { t.text_muted };
                    ui.painter().text(rect.center() - Vec2::new(0.0, 1.0), Align2::CENTER_CENTER, format!("{icon}  {label}"), ts(SEMI).resolve(ui.style()), color);
                    if enabled && resp.clicked() {
                        self.show_payments = payments;
                        if payments { self.payment_list.focus_search = true; }
                    }
                }
            });
            if !self.show_payments {
                // The search box is added before the month controls so Tab reaches it first
                // (header → search → month → summary → calendar); it still sits at the right end.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(self.db.is_some(), |ui| self.filter_box(ui, &t, if compact { 218.0 } else { 278.0 }));
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| self.month_navigation(ui, &t, compact));
                });
            } else {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Your complete payment schedule").color(t.text_muted).text_style(ts(CAPTION)));
                });
            }
        });
    }

    fn month_navigation(&mut self, ui: &mut egui::Ui, t: &Tokens, compact: bool) {
        // Months move only through a budget: without one the controls stay visible but inert.
        let open = self.db.is_some();
        ui.add_space(if compact { SP1 } else { SP3 });
        ui.add_enabled_ui(open, |ui| {
            let previous = icon_button(ui, ph::CARET_LEFT).on_hover_text("Previous month (PgUp)\nHold a dragged payment here to move to this month");
            if previous.clicked() { self.go(-1); }
            self.drag_month_navigation(ui.ctx(), &previous, -1);
            ui.menu_button(RichText::new(format!("{} {}  {}", MONTHS_LONG[self.cur.month() as usize], self.cur.year(), ph::CARET_DOWN))
                .size(if compact { 17.0 } else { 20.0 }).family(egui::FontFamily::Name("semi".into())), |ui| {
                let mut year = self.cur.year();
                ui.horizontal(|ui| { ui.label("Year"); let r = ui.add(egui::DragValue::new(&mut year).range(budget_core::date::YEAR_MIN..=budget_core::date::YEAR_MAX).update_while_editing(false)); note_text_focus(ui.ctx(), &r); });
                if year != self.cur.year() { self.go((year - self.cur.year()) * 12); }
                egui::Grid::new("month_picker").num_columns(3).show(ui, |ui| {
                    for (month, label) in MONTHS.iter().enumerate() {
                        if ui.selectable_label(month as i32 == self.cur.month(), *label).clicked() {
                            self.go(month as i32 - self.cur.month()); ui.close_menu();
                        }
                        if month % 3 == 2 { ui.end_row(); }
                    }
                });
            });
            let next = icon_button(ui, ph::CARET_RIGHT).on_hover_text("Next month (PgDn)\nHold a dragged payment here to move to this month");
            if next.clicked() { self.go(1); }
            self.drag_month_navigation(ui.ctx(), &next, 1);
        });
        // Today is the call to action only with a budget open and another month on screen.
        let away = open && self.cur.get() / 100 != self.today / 100;
        if away {
            if theme::primary_button(ui, format!("{}  Today", ph::DOT_OUTLINE), t).on_hover_text("Jump to the current month (Home or T)").clicked() { self.go_today(); }
        } else if open {
            ui.add_enabled(false, Button::new(RichText::new("Today").color(t.text_muted)).min_size(Vec2::new(0.0, 32.0)))
                .on_disabled_hover_text("You are already viewing the current month");
        } else {
            // No budget: a plain secondary control that is off, not a blue button.
            ui.add_enabled_ui(false, |ui| theme::secondary_button(ui, format!("{}  Today", ph::DOT_OUTLINE), t))
                .inner.on_disabled_hover_text("Open a budget to move between months");
        }
    }

    /// "Showing 7 of 20 · cat:Auto ×": the active search above the grid, with a × that clears it.
    fn filter_chip(&mut self, ui: &mut egui::Ui, t: &Tokens, shown: usize, total: usize) {
        let mut clear = false;
        let chip = tint(t.surface, t.accent, 0.12);
        Frame::new().fill(chip).stroke(Stroke::new(1.0_f32, t.accent))
            .corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(10, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SP2;
                    ui.label(RichText::new(format!("Showing {shown} of {total} · {}", self.filter_text.trim())).text_style(ts(CAPTION)).color(t.text));
                    if ui.add(Button::new(RichText::new("×").color(theme::legible(t.accent, chip))).frame(false)).on_hover_text("Clear the search").clicked() { clear = true; }
                });
            });
        if clear { self.filter_text.clear(); self.set_filter(); }
    }

    fn filter_box(&mut self, ui: &mut egui::Ui, t: &Tokens, width: f32) {
        let active = !self.filter_text.trim().is_empty();
        let stroke = Stroke::new(1.0_f32, if self.filter_error.is_some() { t.negative } else if active { t.accent } else { t.border });
        Frame::new().fill(t.bg).stroke(stroke).corner_radius(7).inner_margin(Margin::symmetric(10, 3)).show(ui, |ui| {
            ui.set_width(width);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = SP2;
                ui.label(RichText::new(ph::MAGNIFYING_GLASS).text_style(ts(ICON)).color(t.text_muted));
                let r = ui.add(TextEdit::singleline(&mut self.filter_text).id_source("transaction_search").frame(false)
                    .hint_text("Search transactions").desired_width(width - 62.0));
                note_text_focus(ui.ctx(), &r);
                if self.focus_filter { r.request_focus(); self.focus_filter = false; }
                if r.changed() { self.set_filter(); }
                r.on_hover_text("Search descriptions, categories or amounts. Advanced filters such as cat:Auto and <-100 also work.");
                if active && ui.add(Button::new(RichText::new(ph::X).text_style(ts(ICON)).color(t.text_muted)).frame(false)).on_hover_text("Clear search").clicked() {
                    self.filter_text.clear(); self.set_filter();
                }
            });
        });
    }

    // ---------- body ----------

    fn stats(&self, ui: &mut egui::Ui, v: &MonthView) -> Option<calendar::Action> {
        let t = &self.t;
        let q = &v.quick;
        let compact = ui.available_width() < 960.0;
        let mut action = None;
        let tiles = [
            (if compact { "Opening" } else { "Opening balance" }, q.opening, t.text),
            ("Money in", q.income, t.positive),
            ("Money out", q.expenses.abs(), t.negative),
            (if compact { "Closing" } else { "Closing balance" }, q.closing, t.text),
        ];
        let width = ((ui.available_width() - SP2 * 3.0) / 4.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP2;
            ui.spacing_mut().interact_size.y = 22.0;
            for (index, (label, value, color)) in tiles.iter().enumerate() {
                // Closing balance is the hero tile: accent-tinted, accent border, larger value.
                let hero = index == 3;
                let card = if hero && v.below_min { t.card().stroke(Stroke::new(1.0_f32, t.low.1)) }
                    else if hero { t.card().fill(tint(t.surface, t.accent, 0.06)).stroke(Stroke::new(1.0_f32, t.accent)) }
                    else { t.card() };
                let delta = q.closing - q.opening;
                let response = card.inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
                    let size = Vec2::new((width - 20.0 - 2.0 * card.stroke.width).max(80.0), 22.0);
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    let p = ui.painter().with_clip_rect(rect);
                    let value_size = if hero { 19.0 } else { 17.0 };
                    let mut amount_job = egui::text::LayoutJob::simple_singleline(money(*value, true), egui::FontId::new(value_size, egui::FontFamily::Name("semi".into())), *color);
                    amount_job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() * 0.58);
                    let amount = p.layout_job(amount_job);
                    let amount_w = amount.size().x;
                    let amount_rect = egui::Rect::from_min_max(egui::pos2(rect.max.x - amount_w, rect.min.y), rect.max);
                    let mut name_right = (amount_rect.min.x - 8.0).max(rect.min.x);
                    let label_w = p.layout_no_wrap((*label).to_string(), ts(CAPTION).resolve(ui.style()), t.text_muted).size().x;
                    // Net change since opening, as an arrow + amount, when it fits beside the label.
                    if hero && delta.abs() >= 0.005 {
                        let (icon, col) = if delta > 0.0 { (ph::ARROW_UP, t.positive) } else { (ph::ARROW_DOWN, t.negative) };
                        let g = p.layout_no_wrap(format!("{icon} {}", money(delta.abs(), true)), ts(CAPTION).resolve(ui.style()), col);
                        if label_w + 10.0 + g.size().x <= name_right - rect.min.x {
                            let left = name_right - g.size().x;
                            p.galley(egui::pos2(left, rect.center().y - g.size().y / 2.0), g, col);
                            name_right = left - 8.0;
                        }
                    }
                    let name_rect = egui::Rect::from_min_max(rect.min, egui::pos2(name_right, rect.max.y));
                    let mut name = egui::text::LayoutJob::simple_singleline((*label).to_string(), ts(CAPTION).resolve(ui.style()), t.text_muted);
                    name.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width());
                    let name = p.layout_job(name);
                    p.with_clip_rect(name_rect).galley(egui::pos2(name_rect.min.x, rect.center().y - name.size().y / 2.0), name, t.text_muted);
                    p.with_clip_rect(amount_rect).galley(egui::pos2(rect.max.x - amount.size().x, rect.center().y - amount.size().y / 2.0), amount, *color);
                });
                if index == 1 || index == 2 {
                    let r = response.response.rect;
                    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(r.min.x, r.min.y + 6.0), egui::pos2(r.min.x + 3.0, r.max.y - 6.0)), 1.5, *color);
                }
                // Each tile is one Tab stop with its own id. Interacting through the frame's response
                // instead registered it twice (hover, then the context menu's click), so the tile
                // surrendered keyboard focus every frame and Tab restarted at the header.
                let tile = ui.interact(response.response.rect, ui.id().with(("summary_tile", index)), Sense::click());
                if tile.has_focus() {
                    ui.painter().rect_stroke(tile.rect.expand(1.0), theme::RADIUS + 1, Stroke::new(2.0_f32, t.accent), StrokeKind::Outside);
                }
                tile.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {}", money(*value, true))));
                if index == 0 || index == 3 {
                    let mut date = self.cur;
                    if index == 0 { date.add(Field::Day, -1); } else { date.set(Field::Day, date.max_day()); }
                    tile.context_menu(|ui| {
                        if ui.button("Edit balance").clicked() { action = Some(calendar::Action::EditBalance(date.get())); ui.close_menu(); }
                    });
                }
                // With cents hidden the calendar shows whole dollars; say so where the tile's cents are explained.
                let precision = if self.cfg.show_decimals_in_cv { String::new() } else { "\nCalendar shows whole dollars; View > Show cents".to_string() };
                tile.on_hover_text(format!("{label}: {}\nNet change: {}\nLowest balance: {}\nAverage balance: {}\nHighest balance: {}{precision}",
                    money(*value, true), money(q.closing - q.opening, true), money(q.min, true), money(q.avg, true), money(q.max, true)));
            }
        });
        if let Some(note) = q.corrections_note() { ui.label(RichText::new(note).text_style(ts(CAPTION)).color(t.text_muted)); }
        action
    }

    fn summary(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        let Some(v) = &self.view else { return };
        let active = self.filter_text.trim().to_string();
        let mut clicked: Option<String> = None;
        // The card wraps its rows; it does not stretch to the panel's full height.
        t.card().inner_margin(Margin::symmetric(12, 12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Summary").text_style(ts(SECTION)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Names the base of the % column (05 §13: income, or expenses when there is none).
                    ui.label(RichText::new(if v.shares_of_expenses { "% of expenses" } else { "% of income" }).text_style(ts(CAPTION)).color(t.text_muted));
                });
            });
            ui.add_space(SP1);
            let has_diff = v.brief.iter().any(|r| r.diff.is_some());
            let (aw, pw, dw): (f32, f32, f32) = (70.0, 44.0, if has_diff { 56.0 } else { 0.0 });
            // column headings
            {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                let p = ui.painter();
                let f = ts(CAPTION).resolve(ui.style());
                let cy = rect.center().y;
                p.text(rect.left_center(), Align2::LEFT_CENTER, "Category", f.clone(), t.text_muted);
                let mut x = rect.max.x;
                if has_diff { p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, "vs plan", f.clone(), t.text_muted); x -= dw; }
                p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, "%", f.clone(), t.text_muted); x -= pw;
                p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, "Amount", f, t.text_muted);
                p.hline(rect.x_range(), rect.max.y + 1.5, Stroke::new(1.0_f32, t.border));
            }
            ui.add_space(SP1);
            for row in &v.brief {
                let is_net = row.total && row.filter.is_empty();
                if is_net {
                    // The bottom line (disposable income) gets a rule coloured by its sign.
                    ui.add_space(SP1);
                    let r = ui.max_rect();
                    ui.painter().hline(r.x_range(), ui.cursor().min.y - 2.0, Stroke::new(1.5_f32, tint(t.surface, sign_colour(&t, row.amount, false), 0.7)));
                    ui.add_space(SP1);
                }
                let filterable = !row.filter.is_empty();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), if is_net { 26.0 } else { 30.0 }), if filterable { Sense::click() } else { Sense::hover() });
                let selected = !active.is_empty() && row.filter == active;
                let p = ui.painter();
                // The fill the row's text sits on: figures and description are nudged to read on it.
                let ground = if selected { tint(t.surface, t.accent, 0.12) } else if filterable && resp.hovered() { t.surface_hover } else { t.surface };
                if selected { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, ground); }
                else if filterable && resp.hovered() { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, t.surface_hover); }
                if filterable && resp.has_focus() { p.rect_stroke(rect.expand2(Vec2::new(6.0, 0.0)), 5, Stroke::new(2.0_f32, t.accent), StrokeKind::Inside); }
                // Text sits in the upper part of the row; the share bar owns the bottom 4px.
                let cy = if is_net { rect.center().y } else { rect.min.y + 11.0 };
                let fid = if row.total { ts(SEMI).resolve(ui.style()) } else { egui::TextStyle::Body.resolve(ui.style()) };
                let mut x = rect.max.x;
                if has_diff {
                    if let Some(d) = row.diff { p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(d, false), fid.clone(), theme::legible(sign_colour(&t, d, false), ground)); }
                    x -= dw;
                }
                if !row.outside { p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, view::share_cell(row.pct, is_net), fid.clone(), t.text_muted); }
                x -= pw;
                let ar = p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(row.amount, false), fid.clone(), theme::legible(sign_colour(&t, row.amount, false), ground));
                x -= aw.max(ar.width() + 8.0);
                let desc_col = theme::legible(if selected { t.accent } else { t.text }, ground);
                let desc_w = (x - rect.min.x).max(10.0);
                let mut job = egui::text::LayoutJob::simple_singleline(row.desc.clone(), fid, desc_col);
                job.wrap = egui::text::TextWrapping::truncate_at_width(desc_w);
                let g = p.layout_job(job);
                let gh = g.size().y;
                p.galley(egui::pos2(rect.min.x, cy - gh / 2.0), g, desc_col);
                // share of income: a 4px bar across the full row inside a faint track
                if !is_net && !row.outside {
                    let track = egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y - 6.0), Vec2::new(rect.width(), 4.0));
                    p.rect_filled(track, 2, tint(t.surface, t.border, 0.5));
                    let share = row.pct.map_or(0.0, |pct| (pct.abs() / 100.0).clamp(0.0, 1.0)) as f32;
                    if share > 0.0 {
                        let fill = egui::Rect::from_min_size(track.min, Vec2::new((track.width() * share).max(3.0), 4.0));
                        p.rect_filled(fill, 2, tint(t.surface, sign_colour(&t, row.amount, false), 0.75));
                    }
                }
                let resp = if row.filter.is_empty() { resp } else { resp.on_hover_cursor(egui::CursorIcon::PointingHand) };
                if !row.filter.is_empty() { resp.clone().on_hover_text(if selected { "Click to clear the filter" } else { "Click to filter the calendar" }); }
                if resp.clicked() { clicked = Some(row.filter.clone()); }
            }
            if !active.is_empty() {
                ui.add_space(SP2);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ph::FUNNEL).text_style(ts(ICON)).color(t.accent));
                    ui.label(RichText::new(format!("Filtered: {active}")).text_style(ts(CAPTION)).color(t.text_muted));
                    if ui.add(Button::new(RichText::new("Clear").text_style(ts(CAPTION)).color(t.accent)).frame(false)).clicked() { clicked = Some(String::new()); }
                });
            }
        });
        if let Some(f) = clicked {
            if f.is_empty() { self.filter_text.clear(); self.set_filter(); } else { self.apply_filter(&f); }
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        let h = ui.available_height();
        let mut to_open: Option<String> = None;
        let mut to_remove: Option<String> = None;
        ui.vertical_centered(|ui| {
            ui.add_space((h * 0.10).max(SP3));
            // A dashed accent outline invites a drop anywhere inside it.
            let width = 440.0_f32.min(ui.available_width() - SP3 * 2.0).max(240.0);
            let (zone, _) = ui.allocate_exact_size(Vec2::new(width, 236.0), Sense::hover());
            let hovering_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
            ui.painter().rect_filled(zone, theme::RADIUS, tint(t.bg, t.accent, if hovering_file { 0.12 } else { 0.04 }));
            let corners = [zone.left_top(), zone.right_top(), zone.right_bottom(), zone.left_bottom(), zone.left_top()];
            for shape in egui::Shape::dashed_line(&corners, Stroke::new(1.5_f32, tint(t.bg, t.accent, 0.6)), 7.0, 5.0) { ui.painter().add(shape); }
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(zone.shrink(SP3)).layout(Layout::top_down(Align::Center)));
            inner.add_space(SP1);
            let (circle, _) = inner.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            inner.painter().circle_filled(circle.center(), 28.0, tint(t.bg, t.accent, 0.16));
            inner.painter().text(circle.center(), Align2::CENTER_CENTER, ph::CALENDAR_DOTS, egui::FontId::new(30.0, egui::FontFamily::Proportional), t.accent);
            inner.label(RichText::new("Open a budget").text_style(ts(TITLE)));
            inner.label(RichText::new("Drop a .bgt file here, or choose one from your computer.").color(t.text_muted));
            inner.add_space(SP1);
            if theme::primary_button(&mut inner, format!("{}  Open…", ph::FOLDER_OPEN), &t).on_hover_text("Ctrl+O").clicked() { self.open_dialog(); }

            let recent: Vec<String> = self.cfg.recent_file.clone();
            if !recent.is_empty() {
                ui.add_space(SP3);
                ui.label(RichText::new("RECENT").text_style(ts(CAPTION)).color(t.text_muted));
                ui.add_space(SP1);
                for r in recent.iter().take(6) {
                    let path = Path::new(r);
                    // Answered by a background check; a share that does not respond must not freeze the window.
                    let status = self.files.status(path);
                    // So is reading it once clicked: the card says so until the budget opens.
                    let opening = self.opening.as_ref().is_some_and(|o| o.path == resolve_path(path));
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 52.0), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(r);
                    let p = ui.painter();
                    p.rect(rect, 8, if resp.hovered() { t.surface_hover } else { t.surface }, Stroke::new(1.0_f32, if resp.hovered() { tint(t.surface, t.accent, 0.6) } else { t.border }), StrokeKind::Inside);
                    if resp.has_focus() { p.rect_stroke(rect.expand(1.0), 9, Stroke::new(2.0_f32, t.accent), StrokeKind::Outside); }
                    let icon = match status { FileStatus::Present(_) => t.accent, FileStatus::Checking => t.text_muted, FileStatus::Missing => t.text_faint };
                    p.text(egui::pos2(rect.min.x + 14.0, rect.center().y), Align2::LEFT_CENTER, ph::FILE_TEXT, egui::FontId::new(22.0, egui::FontFamily::Proportional), icon);
                    let text_w = rect.width() - 62.0;
                    let mut name = egui::text::LayoutJob::simple_singleline(file_name(path), ts(SEMI).resolve(ui.style()), t.text);
                    name.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                    let g = ui.painter().layout_job(name);
                    ui.painter().galley(egui::pos2(rect.min.x + 48.0, rect.min.y + 9.0), g, t.text);
                    let sub = match status {
                        _ if opening => "Opening…".into(),
                        FileStatus::Checking => "Checking…".into(),
                        FileStatus::Present(Some(m)) => format!("Modified {}", ago(m)),
                        FileStatus::Present(None) => "Available".into(),
                        FileStatus::Missing => "File not found".into(),
                    };
                    let mut job = egui::text::LayoutJob::simple_singleline(sub, ts(CAPTION).resolve(ui.style()), t.text_muted);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(rect.min.x + 48.0, rect.min.y + 29.0), g, t.text_muted);
                    if resp.clicked() { to_open = Some(r.clone()); }
                    // A budget that moved or was deleted can be dropped from the list, or looked for again.
                    resp.context_menu(|ui| {
                        if ui.button("Remove from recent list").clicked() { to_remove = Some(r.clone()); ui.close_menu(); }
                        if ui.button("Check again").clicked() { self.files.forget(path); ui.close_menu(); }
                    });
                }
            }
        });
        if let Some(r) = to_remove {
            self.cfg.recent_file.retain(|f| *f != r);
            self.save_config();
        }
        if let Some(r) = to_open { self.open(Path::new(&r)); }
    }

    /// First-run card shown over the calendar while the budget has no payments.
    fn onboarding_card(&mut self, ctx: &egui::Context, area: egui::Rect) {
        let t = self.t.clone();
        let mut add = false;
        egui::Area::new(egui::Id::new("onboarding")).order(egui::Order::Middle).pivot(Align2::CENTER_CENTER)
            .fixed_pos(area.center()).show(ctx, |ui| {
                t.card().inner_margin(SP4).shadow(ctx.style().visuals.popup_shadow).show(ui, |ui| {
                    ui.set_width(380.0_f32.min((area.width() - SP4 * 2.0).max(240.0)));
                    ui.vertical_centered(|ui| {
                        let (circle, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
                        ui.painter().circle_filled(circle.center(), 28.0, tint(t.surface, t.accent, 0.16));
                        ui.painter().text(circle.center(), Align2::CENTER_CENTER, ph::PLUS_CIRCLE, egui::FontId::new(30.0, egui::FontFamily::Proportional), t.accent);
                        ui.label(RichText::new("Add your first payment").text_style(ts(TITLE)));
                        ui.add(egui::Label::new(RichText::new("Enter your salary and recurring bills once. Budget Calendar projects every future date and balance for you.").color(t.text_muted)).wrap());
                        ui.add_space(SP1);
                        // The card only shows over an open budget, so its one action is to add a payment.
                        if theme::primary_button(ui, format!("{}  New payment (Ctrl+N)", ph::PLUS), &t).clicked() { add = true; }
                    });
                    ui.add_space(SP2);
                    ui.separator();
                    for (n, text) in ["Add your salary", "Add recurring bills and loans", "Watch the balance day by day"].iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (c, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                            let disc = tint(t.surface, t.accent, 0.14);
                            ui.painter().circle_filled(c.center(), 10.0, disc);
                            ui.painter().text(c.center(), Align2::CENTER_CENTER, (n + 1).to_string(), ts(SEMI).resolve(ui.style()), theme::legible(t.accent, disc));
                            ui.label(RichText::new(*text).color(t.text));
                        });
                    }
                });
            });
        if add { self.new_payment(self.today); }
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        let Some(b) = self.banner.clone() else { return };
        let t = self.t.clone();
        let (mut dismiss, mut open, mut restore) = (false, false, false);
        Frame::new().fill(tint(t.surface, t.negative, 0.1)).stroke(Stroke::new(1.0_f32, tint(t.surface, t.negative, 0.4))).corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(ph::WARNING_CIRCLE).text_style(ts(ICON)).color(t.negative));
                // Close button on the far right; the message column takes the rest.
                let close_w = 24.0;
                let msg_w = (ui.available_width() - close_w - SP2).max(80.0);
                ui.allocate_ui_with_layout(Vec2::new(msg_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = SP1;
                    if let Some(title) = &b.title { ui.label(RichText::new(title).text_style(ts(SEMI)).color(t.text)); }
                    ui.add(egui::Label::new(RichText::new(&b.text).color(if b.title.is_some() { t.text_muted } else { t.text })).wrap());
                    if b.offer_restore.is_some() {
                        ui.add(egui::Label::new(RichText::new("Earlier versions of this budget are available.").color(t.text_muted)).wrap());
                        if theme::primary_button(ui, "Restore previous version…", &t).clicked() { restore = true; }
                    }
                    if b.offer_open && theme::secondary_button(ui, format!("{}  Open another…", ph::FOLDER_OPEN), &t).clicked() { open = true; }
                    if let Some(details) = &b.details {
                        egui::CollapsingHeader::new(RichText::new("Details").text_style(ts(CAPTION)).color(t.text_muted))
                            .id_salt("banner-details").default_open(false).show(ui, |ui| {
                                ui.add(egui::Label::new(RichText::new(details).monospace().size(11.5).color(t.text_muted)).wrap());
                            });
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.add(Button::new(RichText::new(ph::X).text_style(ts(ICON)).color(t.text_muted)).frame(false)).on_hover_text("Dismiss").clicked() { dismiss = true; }
                });
            });
        });
        ui.add_space(SP2);
        if dismiss {
            // Dismissing an unrelated message leaves the failed save the unsaved-changes prompt reports.
            if self.banner.as_ref().is_some_and(|b| self.dialogs.save_error.as_deref() == Some(b.text.as_str())) { self.dialogs.save_error = None; }
            self.banner = None;
        }
        if open { self.open_dialog(); }
        if restore { if let Some(path) = &b.offer_restore { self.open_restore(path); } }
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else { return };
        let now = Instant::now();
        if now >= toast.until { self.toast = None; return; }
        ctx.request_repaint_after(toast.until - now);
        let t = &self.t;
        let (bg, fg) = if t.dark { (t.surface_hover, t.text) } else { (t.text, t.bg) };
        egui::Area::new(egui::Id::new("toast")).anchor(Align2::RIGHT_BOTTOM, Vec2::new(-SP3, -SP3)).order(egui::Order::Foreground).show(ctx, |ui| {
            Frame::new().fill(bg).corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(14, 10)).shadow(ctx.style().visuals.popup_shadow).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let icon = if toast.error { ph::WARNING_CIRCLE } else { ph::CHECK_CIRCLE };
                    ui.label(RichText::new(icon).text_style(ts(ICON)).color(if toast.error { t.negative } else { t.positive }));
                    ui.label(RichText::new(&toast.text).color(fg));
                });
            });
        });
    }

    fn password_prompt(&mut self, ctx: &egui::Context) {
        if let Some(prompt) = &mut self.dialogs.password {
            if let Some(attempt) = &mut prompt.attempt {
                match attempt.task.poll() {
                    TaskState::Running => {},
                    TaskState::Done(loaded @ (Loaded::Budget(..) | Loaded::Restored { .. })) => {
                        attempt.loaded = Some(loaded);
                        // Reuse the authenticated result, including the original file bytes. The
                        // normal open path still guards against edits and changed load settings.
                        self.opening = prompt.attempt.take();
                        self.dialogs.password = None;
                        ctx.request_repaint();
                        return;
                    }
                    state => {
                        prompt.attempt = None;
                        prompt.error = "The password is incorrect or the encrypted file is damaged.".into();
                        if let TaskState::Done(Loaded::Failed(log, has_backup)) = state {
                            prompt.has_backup = has_backup;
                            // A damaged sealed file fails just as a wrong password does, and it is the
                            // file a copy is most useful for. The copy asks for its own password.
                            if prompt.restore_to.is_none() {
                                let (path, current) = (prompt.path.clone(), recovery::fingerprint(&prompt.bytes));
                                self.offer_recovery(&path, Some(current));
                                if self.dialogs.recovery.is_some() {
                                    self.fail_load(&path, &log, has_backup);
                                    self.dialogs.password = None;
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        let Some(PasswordPrompt { path, typed: pw, error: err, attempt, has_backup, restore_to, .. }) = &mut self.dialogs.password else { return };
        let busy = attempt.is_some();
        let offer_restore = *has_backup && !err.is_empty();
        let t = &self.t;
        let mut done: Option<Option<String>> = None;
        let mut restore = false;
        let line = encrypted_line(path, restore_to.as_deref());
        egui::Modal::new(egui::Id::new("password")).frame(Frame::new().fill(t.surface).stroke(Stroke::new(1.0_f32, t.border)).corner_radius(theme::RADIUS).inner_margin(SP4).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new("Password required").text_style(ts(TITLE)));
            ui.label(RichText::new(line).color(t.text_muted));
            ui.add_space(SP2);
            let r = ui.add_enabled(!busy, TextEdit::singleline(pw).password(true).desired_width(f32::INFINITY).hint_text("Password"));
            let enter = (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
            // Enter gives up focus; take it back whenever nothing else holds it.
            if !busy && ctx.memory(|m| m.focused()).is_none() { r.request_focus(); }
            if busy { ui.horizontal(|ui| { ui.spinner(); ui.label("Unlocking…"); }); }
            if !err.is_empty() { ui.label(RichText::new(err.as_str()).color(t.negative)); }
            if offer_restore && theme::secondary_button(ui, "Restore previous version…", t).clicked() { restore = true; }
            ui.add_space(SP2);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled_ui(!busy, |ui| theme::primary_button(ui, "Unlock", t)).inner.clicked() || (!busy && enter) { done = Some(Some(pw.clone())); }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) { done = Some(None); }
            });
        });
        match done {
            Some(Some(p)) => {
                if p.is_empty() { err.clear(); err.push_str("Empty password!"); return; }
                pw.clear();
                err.clear();
                // Keep the captured bytes for retries. The worker authenticates and builds once;
                // there is no preliminary KDF/decrypt on the window thread and no second read.
                let prompt = self.dialogs.password.as_ref().unwrap();
                let job = self.opening_job(&prompt.path, Some(prompt.bytes.clone()), Some(p), false, prompt.restore_to.clone());
                self.dialogs.password.as_mut().unwrap().attempt = Some(job);
            }
            Some(None) => self.dialogs.password = None,
            None => {}
        }
        if restore {
            if let Some(prompt) = self.dialogs.password.take() { self.open_restore(&prompt.path); }
        }
    }

    /// Asks what to do with a budget that may be cut short. Opening it is the user's decision, told
    /// what a save does; every other answer leaves the file as it is.
    fn incomplete_prompt(&mut self, ctx: &egui::Context) {
        #[derive(Clone, Copy)]
        enum Choice { Cancel, Open, Restore }
        let Some(prompt) = &self.dialogs.incomplete else { return };
        let t = self.t.clone();
        let (name, has_backup) = (file_name(&prompt.path), prompt.has_backup);
        let mut choice = None;
        egui::Modal::new(egui::Id::new("incomplete")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading("This budget may be incomplete");
            ui.add_space(SP2);
            ui.label(RichText::new(&name).text_style(ts(SEMI)));
            ui.add(egui::Label::new(RichText::new("This file does not end the way a finished save does. It may have been cut short by a crash or a sync, so some payments could be missing. Budgets from the original program can also look like this.").color(t.text_muted)).wrap());
            ui.add_space(SP2);
            ui.add(egui::Label::new(RichText::new(if has_backup { "You can restore an earlier version, or open this file as it is. If you open it and save, the file as it is now is kept as a backup." }
                else { "No earlier versions were found. If you open it and save, the file as it is now is kept as a backup." }).color(t.text_muted)).wrap());
            ui.add_space(SP4);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if has_backup {
                    if theme::primary_button(ui, "Restore previous version…", &t).clicked() { choice = Some(Choice::Restore); }
                    if theme::secondary_button(ui, "Open anyway", &t).clicked() { choice = Some(Choice::Open); }
                } else if theme::primary_button(ui, "Open anyway", &t).clicked() { choice = Some(Choice::Open); }
                if theme::secondary_button(ui, "Cancel", &t).clicked() { choice = Some(Choice::Cancel); }
            });
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { choice = Some(Choice::Cancel); }
        });
        let Some(choice) = choice else { return };
        let Some(IncompleteBudget { path, password, db, bytes, quiet, .. }) = self.dialogs.incomplete.take() else { return };
        match choice {
            Choice::Cancel => {}
            Choice::Open => self.apply_loaded(&path, password, Loaded::Budget(db, bytes, Completeness::Complete), quiet),
            Choice::Restore => self.open_restore(&path),
        }
    }

    fn restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialogs.restore else { return };
        if let Some(listing) = &dialog.listing {
            match listing.poll() {
                TaskState::Running => {}
                TaskState::Done(backups) => { dialog.backups = backups; dialog.listing = None; }
                TaskState::Lost => dialog.listing = None,
            }
        }
        let t = self.t.clone();
        let (mut close, mut picked) = (false, None);
        egui::Modal::new(egui::Id::new("restore")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("Restore previous version");
            ui.add_space(SP2);
            ui.label(RichText::new(file_name(&dialog.budget)).text_style(ts(SEMI)));
            ui.add(egui::Label::new(RichText::new("Each version is this budget as it was just before a save at that time. Opening one loads it as unsaved changes. Your current file stays as it is until you save.").color(t.text_muted)).wrap());
            ui.add_space(SP2);
            if dialog.listing.is_some() {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Looking for earlier versions…"); });
            } else if dialog.backups.is_empty() {
                ui.label("No earlier versions of this budget were found.");
            } else {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for backup in &dialog.backups {
                        ui.horizontal(|ui| {
                            ui.label(backup_time(backup));
                            ui.label(RichText::new(file_size(backup.size)).color(t.text_muted));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::secondary_button(ui, "Open", &t).on_hover_text(file_name(&backup.path)).clicked() { picked = Some(backup.path.clone()); }
                            });
                        });
                    }
                });
            }
            ui.add_space(SP4);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::secondary_button(ui, "Close", &t).clicked() { close = true; }
            });
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { close = true; }
        });
        if close || picked.is_some() {
            let budget = dialog.budget.clone();
            self.dialogs.restore = None;
            if let Some(backup) = picked { self.restore_backup(budget, backup); }
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        // A remembered budget still being located or read would be missing from the capture.
        if self.auto_open.is_some() || self.opening.is_some() { ctx.request_repaint(); return; }
        self.frames += 1;
        if self.frames == 6 { ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default())); }
        let shot = ctx.input(|i| i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        }));
        if let Some(img) = shot {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, raw) {
                Some(buf) => { if let Err(e) = buf.save(&path) { eprintln!("screenshot: {e}"); } }
                None => eprintln!("screenshot: bad buffer"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn has_draft(&self) -> bool { self.dialogs.has_draft() }

    fn has_modal(&self) -> bool { self.dialogs.any() || self.sharing.open || self.restoring_copy() }

    /// An edit in a test, which has no name to show.
    #[cfg(test)]
    fn changed(&mut self) -> bool { self.changed_as("edit") }

    /// After an applied edit, which Undo then offers as `label` ("edit Rent"). Returns whether the
    /// schedule had to be rebuilt.
    fn changed_as(&mut self, label: impl Into<String>) -> bool {
        let rebuilt = self.after_change();
        if let Some(db) = &self.db { self.undo.record(self.generation, label, db); }
        rebuilt
    }

    /// After the budget changed, by an edit or by Undo and Redo.
    fn after_change(&mut self) -> bool {
        // A save that failed while the budget was clean (Save as) says nothing about the edits to come.
        if !self.dirty { self.dialogs.save_error = None; }
        self.dirty = true;
        self.edits += 1;
        self.title_dirty = true;
        self.view = None;
        // The edit already rescheduled its payment in place; rebuild only where that would differ.
        let Some(db) = &mut self.db else { return false };
        let rebuilt = refresh::after_edit(db, &self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
        if self.sharing.document.is_some() { self.save_shared(); }
        self.journal_edit();
        rebuilt
    }

    /// Steps made before this frame belong to the budget they were made in.
    fn history_follow(&mut self) { self.undo.follow(self.generation, self.db.as_ref(), self.dirty); }

    /// The Edit menu's Undo (`back`) or Redo: its text, and why it cannot be used now.
    fn history_item(&self, back: bool) -> (String, Option<&'static str>) {
        let verb = if back { "Undo" } else { "Redo" };
        let label = if back { self.undo.undo_label() } else { self.undo.redo_label() };
        let Some(label) = label else { return (verb.into(), Some(if back { "Nothing to undo" } else { "Nothing to redo" })) };
        let conflict = self.sharing.document.as_ref().is_some_and(|d| d.conflict.is_some());
        (format!("{verb}: {label}"), conflict.then_some("Review the conflicting shared version first"))
    }

    /// Undo (`back`) or Redo. On a shared budget it is a new edit like any other, queued for the
    /// household. Elsewhere, arriving at the state last saved leaves nothing to save.
    fn step_history(&mut self, back: bool) {
        self.history_follow();
        let waiting = if back { self.undo.undo_label() } else { self.undo.redo_label() }.is_some();
        if !waiting || self.has_modal() || !self.shared_editable() { return; }
        let Some(step) = (if back { self.undo.undo() } else { self.undo.redo() }) else { return };
        let Some(db) = &mut self.db else { return };
        step.snapshot.restore(db);
        // Undo may bring back any payment, so the in-place shortcut of an edit does not apply.
        db.rebuild_db(&self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
        if self.payment_list.selected.is_some_and(|id| db.get_by_id(id).is_none()) { self.payment_list.selected = None; }
        self.after_change();
        let shared = self.sharing.document.is_some();
        if step.saved && !shared { self.back_at_saved(); }
        let done = format!("{}: {}.", if back { "Undone" } else { "Redone" }, step.label);
        self.notify(if shared && self.dirty { format!("{done} Not saved on this device. See the error message.") }
            else if shared { format!("{done} Queued for household sync.") }
            else if self.dirty { format!("{done} Press Ctrl+S to save.") }
            else { format!("{done} Back to your saved budget.") });
    }

    /// The budget is the file again: as after a save, nothing is left to keep or to ask about.
    fn back_at_saved(&mut self) {
        self.dirty = false;
        if let Some(path) = self.file.clone() { self.forget_copy(&path); }
        if let Some(opening) = &mut self.opening { opening.edits = self.edits; }
        if self.banner.as_ref().is_some_and(|banner| self.dialogs.save_error.as_deref() == Some(banner.text.as_str())) { self.banner = None; }
        self.dialogs.save_error = None;
    }

    /// Queues a recovery copy of the budget as it stands. The `autosave` setting (`.budgetrc`, on by
    /// default) turns these copies off. Shared budgets keep their edits in the protected cache instead;
    /// an edit that cannot reach it stays in memory only, with the error showing (see `save_shared`).
    fn journal_edit(&mut self) {
        if self.screenshot.is_some() || !self.cfg.autosave || self.sharing.document.is_some() { return; }
        if self.file.as_deref().is_some_and(|path| self.copy_is_waiting(path)) { return; }
        let (Some(db), Some(path)) = (&self.db, &self.file) else { return };
        let job = recovery::Job::new(db, path, self.password.as_deref(), self.saved_bytes.as_deref(), self.edits);
        self.journal.note(job);
    }

    /// A due recovery copy goes to its worker now, and an idle window wakes for the next one.
    fn journal_step(&mut self, ctx: &egui::Context) {
        if let Some(wait) = self.journal.tick(ctx, !ctx.input(|i| i.focused)) { ctx.request_repaint_after(wait); }
        if self.journal.take_failure() { self.warn("Unsaved work could not be copied for recovery. Save soon."); }
    }

    /// Unsaved changes were given up on: their recovery copy goes too.
    fn discard_unsaved(&mut self, action: PendingAction, ctx: &egui::Context) {
        if let Some(path) = self.file.clone() { self.forget_copy(&path); }
        self.perform_pending(action, ctx);
    }

    fn new_payment(&mut self, date: i32) {
        if !self.shared_editable() { return; }
        if self.db.is_some() && !self.has_draft() { self.dialogs.payment_editor = Some(PaymentEditor::new(date)); }
    }

    /// `from`: the calendar occurrence the whole payment is edited from, if any.
    fn edit_payment(&mut self, id: i32, from: Option<i32>) {
        if !self.shared_editable() { return; }
        if let Some(db) = &self.db {
            if let Some(i) = db.get_by_id(id) { self.dialogs.payment_editor = Some(PaymentEditor::edit_at(&db.payments[i], from)); }
        }
    }

    fn edit_transaction(&mut self, id: i32, date: i32, amount: f64) {
        if !self.shared_editable() { return; }
        if let Some(db) = &self.db {
            if let Some(i) = db.get_by_id(id) { self.dialogs.transaction_editor = Some(TransactionEditor::new(&db.payments[i], date, amount)); }
        }
    }

    /// The name of a payment, for the Undo item of an edit to it.
    fn payment_name(&self, id: i32) -> String {
        self.db.as_ref().and_then(|db| db.get_by_id(id).map(|i| db.payments[i].description.clone())).unwrap_or_default()
    }

    fn apply_transaction_edit(&mut self, id: i32, date: i32, amount: f64, note: &str, flag: char, scope: TransactionScope) {
        let name = self.payment_name(id);
        let Some(db) = &mut self.db else { return };
        // Under the future scope later occurrences are rewritten unless they already show the amount.
        let rewrites_future = scope == TransactionScope::Future && !document::future_already_shows(db, id, date, amount);
        let unchanged = match scope {
            TransactionScope::Occurrence => document::transaction_unchanged(db, id, date, amount, note, flag),
            TransactionScope::Future => document::future_unchanged(db, id, date, amount, note, flag),
        };
        if unchanged {
            self.dialogs.transaction_editor = None;
            self.notify("Nothing changed.");
            return;
        }
        let result = match scope {
            TransactionScope::Occurrence => document::change_transaction(db, id, date, amount, note, flag),
            TransactionScope::Future => document::change_future_transactions(db, id, date, amount, note, flag),
        };
        match result {
            Ok(()) => {
                self.dialogs.transaction_editor = None;
                self.changed_as(format!("edit {name}"));
                self.notify(if rewrites_future {
                    "This and future amounts updated. Press Ctrl+S to save."
                } else { "Transaction applied. Press Ctrl+S to save." });
            }
            Err(error) => { if let Some(editor) = &mut self.dialogs.transaction_editor { editor.set_error(error); } }
        }
    }

    fn edit_balance(&mut self, date: i32) {
        if !self.shared_editable() { return; }
        if self.has_draft() { return; }
        let Some(db) = &mut self.db else { return };
        let cal = Cal::from_int(date);
        if db.get_day(&cal).is_none() {
            db.rebuild_db(&cal, self.cfg.future_n_months, self.cfg.unlimited_history);
            self.view = None;
        }
        if let Some(day) = db.get_day(&cal) {
            self.dialogs.balance_editor = Some(BalanceEditor::new(date, day.closing_balance, day.closing_balance_adjusted));
        } else { self.fail("This date is outside the loaded budget calendar."); }
    }

    fn apply_balance_edit(&mut self, date: i32, amount: Option<f64>) {
        let Some(db) = &mut self.db else { return };
        match document::set_balance(db, date, amount) {
            Ok(changed) => {
                self.dialogs.balance_editor = None;
                if changed { self.changed_as(format!("change balance on {}", names::long_date(date))); self.notify("Balance updated. Press Ctrl+S to save."); }
            }
            Err(error) => { if let Some(editor) = &mut self.dialogs.balance_editor { editor.set_error(error); } }
        }
    }

    fn drag_month_navigation(&mut self, ctx: &egui::Context, response: &egui::Response, direction: i32) {
        if egui::DragAndDrop::payload::<calendar::DraggedTransaction>(ctx).is_none() { self.drag_month_hover = None; return; }
        if ctx.pointer_hover_pos().is_some_and(|position| response.rect.contains(position)) {
            let now = Instant::now();
            if self.drag_month_hover.map(|(d, _)| d) != Some(direction) { self.drag_month_hover = Some((direction, now)); }
            if self.drag_month_hover.is_some_and(|(_, started)| now.duration_since(started) >= Duration::from_millis(650)) {
                self.go(direction);
                self.drag_month_hover = Some((direction, now));
            }
            ctx.request_repaint_after(Duration::from_millis(30));
        } else if self.drag_month_hover.map(|(d, _)| d) == Some(direction) { self.drag_month_hover = None; }
    }

    fn calendar_action(&mut self, action: calendar::Action) {
        if !self.shared_editable() { return; }
        match action {
            calendar::Action::Filter(f) => self.apply_filter(&f),
            calendar::Action::Add(date) => self.new_payment(date),
            calendar::Action::Day(date) => self.dialogs.day = Some(date),
            calendar::Action::EditPayment(id) => self.edit_payment(id, None),
            calendar::Action::EditPaymentFrom { id, date } => self.edit_payment(id, Some(date)),
            calendar::Action::EditTransaction { id, date, amount } => self.edit_transaction(id, date, amount),
            calendar::Action::DeletePayment(id) => self.dialogs.confirm_delete = Some(DeleteTarget { id, date: None }),
            calendar::Action::DeleteTransaction { id, date } => self.dialogs.confirm_delete = Some(DeleteTarget { id, date: Some(date) }),
            calendar::Action::EditBalance(date) => self.edit_balance(date),
            calendar::Action::Restore { id, date } => {
                let name = self.payment_name(id);
                let Some(db) = &mut self.db else { return };
                match document::restore(db, id, date) {
                    Ok(()) => { self.changed_as(format!("restore {name}")); self.notify("Transaction restored. Press Ctrl+S to save."); }
                    Err(error) => self.fail(error),
                }
            }
            calendar::Action::MoveTransaction { id, from, to, amount } => {
                let name = self.payment_name(id);
                let Some(db) = &mut self.db else { return };
                match document::move_transaction(db, id, from, to, amount) {
                    Ok(true) => {
                        self.cur = Cal::from_int(to); self.cur.set_day_one();
                        self.changed_as(format!("move {name}"));
                        self.notify(format!("Moved to {}. Press Ctrl+S to save.", budget_core::date::sdate(to)));
                    }
                    Ok(false) => {},
                    Err(error) => self.fail(error),
                }
            }
        }
    }

    fn payments_panel(&mut self, ui: &mut egui::Ui) {
        let Some(db) = &self.db else { return };
        let revision = (self.generation, db.update_count);
        if let Some(action) = self.payment_list.show(ui, db, &self.t, self.today, revision) { self.calendar_action(action); }
    }

    fn day_window(&mut self, ctx: &egui::Context) {
        let state_id = egui::Id::new("day_transactions_input");
        let Some(date) = self.dialogs.day else { ctx.data_mut(|d| d.remove::<DayDialogInput>(state_id)); return };
        if self.has_draft() || self.dialogs.pending.is_some() || self.dialogs.confirm_delete.is_some() { return; }
        let can_restore = self.can_restore();
        let Some(db) = &mut self.db else { return };
        let transactions = db.get_day(&Cal::from_int(date)).map(|d| d.transactions.clone()).unwrap_or_default();
        let restorable = db.get_day(&Cal::from_int(date)).map(|d| d.restorable.clone()).unwrap_or_default();
        // A search lists what +N counted (the matching transactions) first, then the rest of the day.
        let (matching, others): (Vec<_>, Vec<_>) = match &self.filter {
            Some(filter) => transactions.into_iter().partition(|tr| filter.is_match(&db.payments[tr.pmt], tr.amount, date)),
            None => (transactions, Vec::new()),
        };
        let total = matching.len() + others.len();
        let summary = match (&self.filter, total) {
            (Some(_), _) => format!("{} of {total} transactions match the search", matching.len()),
            (None, 1) => "1 transaction".to_string(),
            (None, _) => format!("{total} transactions"),
        };
        let t = self.t.clone();
        let mut state: DayDialogInput = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
        let delay = ctx.options(|o| o.input_options.max_double_click_delay);
        let (time, pressed, since_click) = ctx.input(|i| (i.time, i.pointer.any_pressed(), i.pointer.time_since_last_click() as f64));
        let opened_at = *state.opened_at.get_or_insert(time);
        // A press that only dismisses an open More menu, or that is the second click of the
        // double-click that opened this dialog, must not close it when released on the backdrop.
        if pressed { state.ignore_release = state.menu_open || (since_click < delay && time - since_click <= opened_at + 1e-3); }
        let menu_was_open = state.menu_open;
        let mut menu_open = false;
        let mut open = true;
        let mut action = None;
        let response = egui::Modal::new(egui::Id::new("day_transactions")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.screen_rect().width() - 80.0));
            ui.heading(names::long_date(date));
            ui.label(RichText::new(summary).color(t.text_muted));
            ui.add_space(SP3);
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 250.0).max(140.0)).show(ui, |ui| {
                let mut row = |ui: &mut egui::Ui, tr: &budget_core::Transaction| {
                    let p = &db.payments[tr.pmt];
                    ui.push_id(p.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(money(tr.amount, true)).text_style(ts(SEMI)).color(sign_colour(&t, tr.amount, true)));
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add(egui::Label::new(RichText::new(&p.description).text_style(ts(SEMI))).truncate());
                                });
                            });
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Edit this date").clicked() { action = Some(calendar::Action::EditTransaction { id:p.id, date, amount:tr.amount }); }
                            ui.menu_button("More", |ui| {
                                // Runs only while the menu is shown (egui 0.31 returns the menu's
                                // inner response only on the frame it closes).
                                menu_open = true;
                                if ui.button("Edit whole payment").clicked() { action = Some(calendar::Action::EditPaymentFrom { id:p.id, date }); ui.close_menu(); }
                                if ui.button(RichText::new("Delete transaction").color(t.negative)).clicked() { action = Some(calendar::Action::DeleteTransaction { id:p.id, date }); ui.close_menu(); }
                            });
                        });
                        ui.add_space(SP2); ui.separator(); ui.add_space(SP2);
                    });
                };
                for tr in &matching { row(ui, tr); }
                if !others.is_empty() {
                    ui.label(RichText::new("Other transactions on this day").text_style(ts(SECTION)).color(t.text_muted));
                    ui.add_space(SP2);
                    for tr in &others { row(ui, tr); }
                }
                // Deleted or moved-away occurrences are not transactions any more; offer them for restoring.
                for pi in restorable {
                    let p = &db.payments[pi];
                    ui.push_id(("restorable", p.id), |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add_enabled(can_restore, Button::new("Restore")).on_disabled_hover_text("Clear the search filter to restore transactions.").clicked() {
                                    action = Some(calendar::Action::Restore { id: p.id, date });
                                }
                                ui.label(RichText::new("Deleted or moved").text_style(ts(CAPTION)).color(t.text_muted));
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                    ui.add(egui::Label::new(RichText::new(&p.description).color(t.text_muted)).truncate());
                                });
                            });
                        });
                        ui.add_space(SP2); ui.separator(); ui.add_space(SP2);
                    });
                }
            });
            ui.add_space(SP3);
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "Add payment on this date", &t).clicked() { action = Some(calendar::Action::Add(date)); }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| { if theme::primary_button(ui, "Done", &t).clicked() { open = false; } });
            });
            // egui closes an open More menu on Escape itself; that Escape must not also close the dialog.
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) && !menu_was_open { open = false; }
        });
        // Like the other read-only dialogs, a click on the dimmed backdrop closes it.
        if response.backdrop_response.clicked() && !state.ignore_release { open = false; }
        state.menu_open = menu_open;
        if open { ctx.data_mut(|d| d.insert_temp(state_id, state)); } else { self.dialogs.day = None; }
        if let Some(action) = action { self.calendar_action(action); }
    }

    /// `id`: the payment edited, or none for a new one.
    fn apply_payment_edit(&mut self, p: Box<budget_core::Payment>, id: Option<i32>, from: Option<i32>) {
        let start = p.start_date;
        if id.is_some_and(|id| document::payment_unchanged(self.db.as_ref().unwrap(), id, &p)) {
            self.dialogs.payment_editor = None;
            self.notify("Nothing changed.");
            return;
        }
        let label = format!("{} {}", if id.is_some() { "edit" } else { "add" }, p.description);
        match document::apply_payment(self.db.as_mut().unwrap(), *p, id, from) {
            Ok(applied) => {
                if id.is_none() {
                    // The calendar moves to the new payment's month, away from the day list.
                    self.cur = Cal::from_int(start); self.cur.set_day_one();
                    self.dialogs.day = None;
                }
                self.payment_list.selected = Some(applied);
                self.dialogs.payment_editor = None;
                self.changed_as(label);
                self.notify("Payment applied. Press Ctrl+S to save.");
            }
            Err(e) => { if let Some(editor) = &mut self.dialogs.payment_editor { editor.set_error(e); } },
        }
    }

    /// A confirmed delete of a payment, or of one date of it.
    fn apply_delete(&mut self, target: DeleteTarget) {
        let name = self.payment_name(target.id);
        match document::delete(self.db.as_mut().unwrap(), target.id, target.date) {
            Ok(()) => { self.dialogs.transaction_editor = None; self.dialogs.payment_editor = None; self.changed_as(format!("delete {name}")); self.notify("Deleted. Press Ctrl+S to save."); }
            Err(e) => self.fail(e),
        }
    }

    fn editors(&mut self, ctx: &egui::Context) {
        if self.dialogs.pending.is_some() || self.dialogs.confirm_delete.is_some() { return; }
        if let (Some(editor), Some(db)) = (&mut self.dialogs.payment_editor, &self.db) {
            let id = editor.payment_id;
            match editor.show(ctx, &self.t, db, self.cfg.first_day_of_week_is_sunday, self.today) {
                PaymentAction::None => {},
                PaymentAction::Cancel => self.dialogs.payment_editor = None,
                PaymentAction::Apply(p, from) => self.apply_payment_edit(p, id, from),
            }
        }
        if let Some(editor) = &mut self.dialogs.transaction_editor {
            let (id, date) = (editor.payment_id, editor.date);
            match editor.show(ctx, &self.t) {
                TransactionAction::None => {},
                TransactionAction::Cancel => self.dialogs.transaction_editor = None,
                TransactionAction::Delete => self.dialogs.confirm_delete = Some(DeleteTarget { id, date: Some(date) }),
                TransactionAction::Apply { amount, note, flag, scope } => self.apply_transaction_edit(id, date, amount, &note, flag, scope),
            }
        }
        if let Some(editor) = &mut self.dialogs.balance_editor {
            let date = editor.date;
            match editor.show(ctx, &self.t) {
                BalanceAction::None => {},
                BalanceAction::Cancel => self.dialogs.balance_editor = None,
                BalanceAction::Apply(amount) => self.apply_balance_edit(date, Some(amount)),
                BalanceAction::Remove => self.apply_balance_edit(date, None),
            }
        }
    }

    fn perform_pending(&mut self, action: PendingAction, ctx: &egui::Context) {
        self.dialogs.pending = None;
        match action {
            PendingAction::Open(path) => self.open_unchecked(&path),
            PendingAction::Close => { self.allow_close = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
            PendingAction::New(path) => self.create_budget(&path),
            PendingAction::CloseBudget => self.close_budget(),
            PendingAction::Shared(document) => self.activate_shared(*document),
            PendingAction::Restore { budget, backup } => self.restore_unchecked(budget, &backup),
        }
    }

    fn confirmations(&mut self, ctx: &egui::Context) {
        let t = self.t.clone();
        if let Some(action) = self.dialogs.pending.clone() {
            let draft = self.has_draft();
            let mut choice = None;
            egui::Modal::new(egui::Id::new("unsaved")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.heading(if draft { "Finish your edit" } else { "Unsaved changes" });
                ui.add_space(SP2);
                ui.label(RichText::new(if draft { "Return to the editor to apply your changes, or discard this draft before continuing." }
                    else { "Save your changes before leaving this budget." }).color(t.text_muted));
                if let Some(path) = &self.file { ui.label(RichText::new(file_name(path)).text_style(ts(SEMI))); }
                if let Some(error) = self.dialogs.save_error.as_ref().filter(|_| !draft) {
                    ui.add_space(SP2);
                    ui.add(egui::Label::new(RichText::new(error).color(t.negative)).wrap());
                    if !draft && theme::secondary_button(ui, "Save a separate copy…", &t).clicked() { choice = Some(UnsavedChoice::SaveCopy); }
                }
                ui.add_space(SP4);
                ui.horizontal(|ui| {
                    let discard = ui.add(Button::new(RichText::new(if draft { "Discard draft" } else { "Discard changes" }).color(t.negative)).frame(false));
                    theme::focus_ring(ui, &discard, &t);
                    if discard.clicked() { choice = Some(UnsavedChoice::Discard); }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if draft {
                            if theme::primary_button(ui, "Return to editor", &t).clicked() { choice = Some(UnsavedChoice::Back); }
                        } else {
                            if theme::primary_button(ui, "Save changes", &t).clicked() { choice = Some(UnsavedChoice::Save); }
                            if theme::secondary_button(ui, "Cancel", &t).clicked() { choice = Some(UnsavedChoice::Back); }
                        }
                    });
                });
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { choice = Some(UnsavedChoice::Back); }
            });
            match choice {
                Some(UnsavedChoice::Back) => self.dialogs.pending = None,
                Some(UnsavedChoice::Discard) if draft => {
                    self.dialogs.payment_editor = None; self.dialogs.transaction_editor = None; self.dialogs.balance_editor = None;
                    if !self.dirty { self.perform_pending(action, ctx); }
                }
                Some(UnsavedChoice::Discard) => self.discard_unsaved(action, ctx),
                Some(UnsavedChoice::Save) if self.save() => self.perform_pending(action, ctx),
                Some(UnsavedChoice::SaveCopy) if self.save_as() => self.perform_pending(action, ctx),
                Some(UnsavedChoice::Save | UnsavedChoice::SaveCopy) | None => {},
            }
            return;
        }
        if let Some(target) = self.dialogs.confirm_delete {
            let payment = self.db.as_ref().and_then(|db| db.get_by_id(target.id).map(|i| &db.payments[i]));
            let description = payment.map(|p| p.description.clone()).unwrap_or_default();
            let loan = payment.is_some_and(|p| p.end_mode() == budget_core::END_MODE_LOAN_PAID);
            let mut choice = None;
            egui::Modal::new(egui::Id::new("delete")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.heading(if target.date.is_some() { "Delete transaction" } else { "Delete payment" });
                ui.add_space(SP2);
                ui.label(RichText::new(&description).text_style(ts(SEMI)));
                let message = match target.date {
                    Some(date) if loan => format!("The installment on {} will be skipped. The loan payoff and future installments will be recalculated.", budget_core::date::sdate(date)),
                    Some(date) => format!("The transaction on {} will be skipped. Other scheduled dates stay unchanged.", budget_core::date::sdate(date)),
                    None => "This removes the payment and all its scheduled dates. The file changes when you save your budget.".into()
                };
                ui.add(egui::Label::new(RichText::new(message).color(t.text_muted)).wrap());
                ui.add_space(SP4);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::danger_button(ui, if target.date.is_some() { "Delete transaction" } else { "Delete payment" }, &t).clicked() { choice = Some(DeleteChoice::Delete); }
                    if theme::secondary_button(ui, "Cancel", &t).clicked() { choice = Some(DeleteChoice::Cancel); }
                });
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { choice = Some(DeleteChoice::Cancel); }
            });
            if choice.is_some() { self.dialogs.confirm_delete = None; }
            if choice == Some(DeleteChoice::Delete) { self.apply_delete(target); }
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bgt"))));
        if reports::is_open(ctx) {
            if dropped.is_some() { self.warn("Close the open dialog first, then drop the budget again."); }
            return;
        }
        // F1 toggles the shortcuts sheet; "?" opens it when no text box has focus.
        let dragging = egui::DragAndDrop::payload::<calendar::DraggedTransaction>(ctx).is_some();
        let help = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1))
            || (!text_input_has_focus(ctx) && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(s) if s == "?"))));
        if help && !dragging && !self.dialogs.any_but_shortcuts() { self.dialogs.shortcuts = !self.dialogs.shortcuts; }
        if self.has_modal() {
            // Dialogs keep their place (and any draft); say why the dropped budget did not open.
            if dropped.is_some() { self.warn("Close the open dialog first, then drop the budget again."); }
            return;
        }
        if dragging && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx); self.drag_month_hover = None; return;
        }
        // Only a text box swallows the plain keys: a Tab-focused tile, arrow or row must not disable them.
        let typing = text_input_has_focus(ctx);
        // The month keys move the calendar, which is hidden on the Payments page: they act only while it shows.
        // Without a budget the month controls are inert, and so are their keys.
        let calendar = !self.show_payments && self.db.is_some();
        let (o, s, f, n, p, pgdn, pgup, home) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::COMMAND, Key::O),
            i.consume_key(Modifiers::COMMAND, Key::S),
            i.consume_key(Modifiers::COMMAND, Key::F),
            i.consume_key(Modifiers::COMMAND, Key::N),
            i.consume_key(Modifiers::COMMAND, Key::P),
            // Alt+Right / Alt+Left mirror Page Down / Page Up and also work while the search box has focus.
            calendar && (i.consume_key(Modifiers::ALT, Key::ArrowRight) | (!typing && i.consume_key(Modifiers::NONE, Key::PageDown))),
            calendar && (i.consume_key(Modifiers::ALT, Key::ArrowLeft) | (!typing && i.consume_key(Modifiers::NONE, Key::PageUp))),
            calendar && ((!typing && i.consume_key(Modifiers::NONE, Key::Home)) | (!typing && i.consume_key(Modifiers::NONE, Key::T))),
        ));
        // A text box undoes its own typing. Ctrl+Shift+Z goes first: Ctrl+Z also matches it.
        let (redo, undo) = if typing { (false, false) } else { ctx.input_mut(|i| (
            i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) | i.consume_key(Modifiers::COMMAND, Key::Y),
            i.consume_key(Modifiers::COMMAND, Key::Z),
        )) };
        // Dialogs and view switches wait until a dragged transaction is dropped; month paging still works.
        if o && !dragging { self.open_dialog(); }
        if s { self.save(); }
        if f { if self.show_payments { self.payment_list.focus_search = true; } else { self.focus_filter = true; } }
        if n && !dragging { self.new_payment(self.today); }
        if undo && !dragging { self.step_history(true); }
        if redo && !dragging { self.step_history(false); }
        if p && !dragging && self.db.is_some() { self.show_payments = true; self.payment_list.focus_search = true; }
        if pgdn { self.go(1); }
        if pgup { self.go(-1); }
        if home { self.go_today(); }
        if let Some(p) = dropped { self.open(&p); }
    }


    /// The keyboard shortcuts sheet (F1 or ?).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.dialogs.shortcuts { return; }
        let t = self.t.clone();
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("shortcuts")).frame(t.card().inner_margin(SP4)).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 460.0));
            ui.label(RichText::new("Keyboard shortcuts").text_style(ts(TITLE)));
            ui.add_space(SP2);
            const ROWS: [(&str, &str); 16] = [
                ("Ctrl+N", "New payment"), ("Ctrl+S", "Save changes"), ("Ctrl+O", "Open a budget"),
                ("Ctrl+Z", "Undo"), ("Ctrl+Y", "Redo (also Ctrl+Shift+Z)"),
                ("Ctrl+F", "Search transactions"), ("Ctrl+P", "Payments list"),
                ("PgUp", "Previous month"), ("PgDn", "Next month"), ("Alt+Left", "Previous month (works while searching)"),
                ("Alt+Right", "Next month (works while searching)"), ("Home", "Jump to the current month"),
                ("T", "Jump to the current month"), ("Enter", "Apply the open editor"), ("Esc", "Cancel or close"),
                ("F1", "Show or hide this sheet"),
            ];
            egui::Grid::new("shortcut_grid").num_columns(2).spacing([SP3, 10.0]).show(ui, |ui| {
                for (combo, what) in ROWS {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (i, key) in combo.split('+').enumerate() {
                            if i > 0 { ui.label(RichText::new("+").text_style(ts(CAPTION)).color(t.text_faint)); }
                            theme::keycap(ui, key, &t);
                        }
                    });
                    ui.label(RichText::new(what).color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(SP3);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, "Close", &t).clicked() { close = true; }
            });
        });
        if close || response.should_close() { self.dialogs.shortcuts = false; }
    }

    /// What the small coloured markers on calendar rows mean (View > Calendar legend).
    fn legend(&mut self, ctx: &egui::Context) {
        let t = self.t.clone();
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("legend")).frame(t.card().inner_margin(SP4)).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 420.0));
            ui.label(RichText::new("Calendar legend").text_style(ts(TITLE)));
            ui.add_space(SP2);
            egui::Grid::new("legend_grid").num_columns(2).spacing([SP3, 10.0]).show(ui, |ui| {
                for (kind, what) in calendar::LEGEND {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 16.0), Sense::hover());
                    calendar::paint_marker(ui.painter(), rect.center(), *kind, &t);
                    ui.label(RichText::new(*what).color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(SP3);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, "Close", &t).clicked() { close = true; }
            });
        });
        if close || response.should_close() { self.dialogs.legend = false; }
    }

    fn about(&mut self, ctx: &egui::Context) {
        if !self.dialogs.about { return; }
        let t = &self.t;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("version_history")).frame(t.card()).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 650.0));
            ui.spacing_mut().item_spacing.y = SP2;
            ui.heading("Budget Calendar");
            ui.horizontal(|ui| {
                theme::badge(ui, &format!("v{}", version::VERSION), t.accent, t);
                ui.label(RichText::new(format!("Build {}", version::BUILD_ID)).text_style(ts(CAPTION)).color(t.text_muted));
            });
            ui.label(RichText::new("The version identifies this release. The build ID identifies the source used to build it.").color(t.text_muted));
            ui.separator();
            ui.label(RichText::new("Version history").text_style(ts(SECTION)).color(t.text));
            egui::ScrollArea::vertical().id_salt("version_history_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height((ctx.screen_rect().height() - 350.0).clamp(120.0, 440.0)).show(ui, |ui| {
                    for line in version::history().lines().map(str::trim).filter(|line| !line.is_empty()) {
                        if let Some(heading) = line.strip_prefix("## ") {
                            ui.add_space(SP2);
                            ui.label(RichText::new(heading).strong().color(t.text));
                        } else if let Some(heading) = line.strip_prefix("### ") {
                            ui.label(RichText::new(heading).strong().color(t.text_muted));
                        } else if !line.starts_with("# ") {
                            let text = line.strip_prefix("- ").map_or_else(|| line.to_string(), |bullet| format!("• {bullet}"));
                            ui.add(egui::Label::new(RichText::new(text).color(t.text_muted)).wrap());
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if theme::secondary_button(ui, "Copy version info", t).clicked() { ctx.copy_text(version::full_version()); }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::primary_button(ui, "Close", t).clicked() { close = true; }
                });
            });
        });
        if close || response.should_close() { self.dialogs.about = false; }
    }
}

/// "3 days ago" style age of a file timestamp.
fn ago(time: std::time::SystemTime) -> String {
    let secs = time.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => plural(secs / 60, "minute"),
        3600..=86_399 => plural(secs / 3600, "hour"),
        86_400..=2_591_999 => plural(secs / 86_400, "day"),
        2_592_000..=31_535_999 => plural(secs / 2_592_000, "month"),
        _ => plural(secs / 31_536_000, "year"),
    }
}

/// Absolute spelling of a path, so .budgetrc entries work from any working directory.
fn resolve_path(p: &Path) -> PathBuf { std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()) }

/// The spelling-independent identity of a budget path: absolute, with the platform's separators,
/// and case-folded on Windows, where folder and drive letter case never matter. It is lexical
/// only, so a budget on an unreachable share is never touched.
fn recent_key(path: &Path) -> String {
    let text = resolve_path(path).display().to_string().replace('/', std::path::MAIN_SEPARATOR_STR);
    if cfg!(windows) { text.to_lowercase() } else { text }
}

/// Whether two spellings name the same existing file (case, `..`, relative forms).
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

fn file_name(p: &Path) -> String { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()) }

fn menu_btn(label: &str, shortcut: &str) -> Button<'static> { Button::new(label.to_string()).shortcut_text(shortcut.to_string()) }
fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> egui::Response { ui.add(menu_btn(label, shortcut)) }

fn icon_button(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(Button::new(RichText::new(icon).text_style(ts(ICON))).min_size(Vec2::new(28.0, 28.0)))
}

impl App {
    fn render(&mut self, ctx: &egui::Context) {
        let (modal_before, focus_before) = (self.has_modal(), ctx.memory(|m| m.focused()));
        self.files.wake(ctx);
        // A mouse release in another application never reaches egui: losing the focus cancels a drag,
        // or the payload would linger and swallow the next click as a drop.
        if !ctx.input(|i| i.focused) && egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(ctx) {
            egui::DragAndDrop::clear_payload(ctx);
            self.drag_month_hover = None;
        }
        self.finish_auto_open();
        self.poll_shared(ctx);
        self.journal_step(ctx);
        // A dragged transaction belongs to the budget it was picked up from: the swap waits for the drop.
        if !egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(ctx) { self.finish_opening(); }
        self.check_date(ctx);
        self.screenshot_step(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.allow_close && (self.dirty || self.has_draft()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                // The newest explicit intent wins over a deferred Open.
                self.dialogs.pending = Some(PendingAction::Close);
            } else {
                self.save_config();
                // The app is closing: its temporary report files go with it, as in the original.
                reports::delete_created_reports();
            }
        }
        if self.title_dirty {
            self.title_dirty = false;
            let name = self.file.as_deref().map(file_name).or_else(|| self.sharing.document.as_ref().map(|d| format!("{} (shared)", d.working.name)));
            let title = version::window_title(name.as_deref(), self.dirty);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        self.history_follow();
        self.keys(ctx);
        self.ensure_view();
        let t = self.t.clone();
        egui::TopBottomPanel::top("header").show_separator_line(false)
            .frame(Frame::new().fill(t.surface).inner_margin(Margin { left: 16, right: 16, top: 6, bottom: 6 }))
            .show(ctx, |ui| {
                self.header(ui);
                let r = ui.max_rect();
                ui.painter().hline(egui::Rangef::new(r.min.x - 16.0, r.max.x + 16.0), r.max.y + 5.5, Stroke::new(1.0_f32, t.border));
            });
        let has_db = self.db.is_some();
        // The footer is added before the summary panel so it spans the full window width
        // (egui gives the full width to whichever panel is added first).
        egui::TopBottomPanel::bottom("status").show_separator_line(true)
            .frame(Frame::new().fill(t.surface).inner_margin(Margin::symmetric(16, 3))).show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 16.0;
                ui.horizontal(|ui| {
                    if let Some(db) = &self.db {
                        ui.label(RichText::new(format!("{} {}", db.size(), payment_list::payments_noun(db.size()))).text_style(ts(CAPTION)).color(t.text_muted));
                        if self.filter.is_some() && !self.show_payments {
                            // The count of a search takes the accent, so a narrowed grid is not missed.
                            if let Some(view) = &self.view { ui.label(RichText::new(format!("·  {} of {} transactions", view.shown, view.total)).text_style(ts(CAPTION)).color(t.accent)); }
                        }
                        ui.label(RichText::new("·").color(t.text_faint));
                        let saved = self.sharing.status(self.dirty).unwrap_or_else(|| if self.dirty { "Changes waiting to be saved".into() } else { "All changes saved".into() });
                        ui.label(RichText::new(saved).text_style(ts(CAPTION)).color(if self.dirty { theme::legible(t.important, t.surface) } else { t.text_muted }));
                        // The calendar's gestures have no other cue; the wide window names all three.
                        let crowded = ctx.screen_rect().width() < 1000.0 && (self.dirty || self.filter.is_some());
                        if !self.show_payments && !crowded {
                            let hint = if ctx.screen_rect().width() >= 1200.0 { "Double-click a day to add · Drag a payment to move · Right-click for more" }
                                else { "Double-click a day to add · Right-click for more" };
                            ui.label(RichText::new("·").color(t.text_faint));
                            ui.label(RichText::new(hint).text_style(ts(CAPTION)).color(t.text_muted));
                        }
                    } else if self.opening.is_none() { ui.label(RichText::new("Open a budget to get started").text_style(ts(CAPTION)).color(t.text_muted)); }
                    if let Some(opening) = &self.opening {
                        if self.db.is_some() { ui.label(RichText::new("·").color(t.text_faint)); }
                        ui.label(RichText::new(format!("Opening {}…", file_name(&opening.path))).text_style(ts(CAPTION)).color(t.text_muted));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Added right to left: label first, then the keycaps from the last key to the first.
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (i, (combo, label)) in [("F1", "Shortcuts"), ("Ctrl+F", "Search"), ("Ctrl+S", "Save")].iter().enumerate() {
                            if i > 0 { ui.add_space(SP2); }
                            ui.label(RichText::new(*label).text_style(ts(CAPTION)).color(t.text_muted));
                            let keys: Vec<&str> = combo.split('+').collect();
                            for key in keys.iter().rev() { theme::keycap(ui, key, &t); }
                        }
                    });
                });
            });
        if has_db && !self.show_payments && self.show_summary && ctx.screen_rect().width() >= SUMMARY_MIN_WINDOW {
            egui::SidePanel::right("summary").resizable(false).exact_width(SUMMARY_W).show_separator_line(false)
                .frame(Frame::new().fill(t.bg).inner_margin(Margin { left: 0, right: 8, top: 8, bottom: 8 }))
                .show(ctx, |ui| self.summary(ui));
        }
        let mut onboarding: Option<egui::Rect> = None;
        egui::CentralPanel::default().frame(Frame::new().fill(t.bg).inner_margin(8)).show(ctx, |ui| {
            // One rhythm: 8px between header, summary strip, grid and footer.
            ui.spacing_mut().item_spacing.y = SP2;
            self.banner(ui);
            if !has_db { self.empty_state(ui); return; }
            if self.show_payments { self.payments_panel(ui); return; }
            if let Some(error) = &self.filter_error { ui.colored_label(t.negative, error); }
            let Some(view) = self.view.take() else { return };
            let balance_action = self.stats(ui, &view);
            // A search hides rows, so the chip says how many are shown and offers to clear the search.
            if view.filtered { self.filter_chip(ui, &t, view.shown, view.total); }
            let grid_top = ui.cursor().min.y;
            let clicked = calendar::show(ui, &view, &t, self.cfg.first_day_of_week_is_sunday, self.today, self.cfg.show_decimals_in_cv);
            let grid_area = egui::Rect::from_min_max(egui::pos2(ui.max_rect().min.x, grid_top), ui.max_rect().max);
            self.view = Some(view);
            if self.db.as_ref().is_some_and(|d| d.payments.is_empty()) && !self.has_modal() { onboarding = Some(grid_area); }
            if let Some(action) = balance_action.or(clicked) { self.calendar_action(action); }
        });
        if let Some(area) = onboarding { self.onboarding_card(ctx, area); }
        if self.dialogs.about {
            self.about(ctx);
        } else if self.dialogs.legend {
            self.legend(ctx);
        } else if self.dialogs.shortcuts && !self.dialogs.any_but_shortcuts() {
            self.shortcuts(ctx);
        } else if self.dialogs.password.is_some() {
            // The password prompt is topmost: Escape and clicks reach it, not a dialog behind it.
            self.password_prompt(ctx);
        } else if self.dialogs.recovery.is_some() {
            self.recovery_prompt(ctx);
        } else if self.dialogs.incomplete.is_some() {
            self.incomplete_prompt(ctx);
        } else if self.dialogs.restore.is_some() {
            self.restore_dialog(ctx);
        } else if self.restoring_copy() && self.dialogs.pending.is_none() {
            self.restoring_prompt(ctx);
        } else {
            self.day_window(ctx);
            self.editors(ctx);
            self.confirmations(ctx);
        }
        // The report range dialog waits behind a save/close prompt (Reports is unreachable under other dialogs).
        if !self.has_modal() {
            if let Some(result) = reports::dialog(ctx, &t, self.db.as_mut(), &self.cfg, self.generation) {
                match result { Ok(done) => self.notify(done), Err(e) => self.fail(e) }
            }
        }
        // drag-over hint; a dialog does not accept drops (`keys` explains a drop made anyway)
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) && !self.has_modal() && !reports::is_open(ctx) {
            let r = ctx.screen_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
            p.rect(r.shrink(SP3), theme::RADIUS, alpha(t.accent, 10), Stroke::new(2.0_f32, t.accent), StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, "Drop to open", ts(TITLE).resolve(&ctx.style()), t.accent);
        }
        self.toast(ctx);
        self.shared_panel(ctx);
        // Keyboard focus goes back to the control that opened a dialog (e.g. a row opened with Enter).
        match (modal_before, self.has_modal()) {
            (false, true) => self.focus_return = focus_before,
            // A menu that opened the dialog is gone by now, and AccessKit crashes on a focused widget that
            // is not in the frame. egui keeps a fresh request for one frame, so check here.
            (true, false) => if let Some(id) = self.focus_return.take().filter(|id| ctx.viewport(|v| v.this_pass.widgets.contains(*id))) { ctx.memory_mut(|m| m.request_focus(id)); },
            _ => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) { self.render(ctx); }
}

/// Print to stdout; in a release (GUI-subsystem) build attach to the parent console first when
/// stdout is not redirected, so `--version` is visible in cmd/PowerShell.
fn print_cli(info: &str) {
    #[cfg(windows)]
    {
        use std::io::Write;
        extern "system" {
            fn GetStdHandle(n: u32) -> isize;
            fn AttachConsole(pid: u32) -> i32;
        }
        let h = unsafe { GetStdHandle(-11i32 as u32) };
        if h == 0 || h == -1 {
            if unsafe { AttachConsole(u32::MAX) } != 0 {
                if let Ok(mut con) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                    let _ = writeln!(con, "{info}");
                    return;
                }
            }
            return;
        }
    }
    println!("{info}");
}

fn main() -> eframe::Result {
    if let Some(info) = version::cli_output(&std::env::args().skip(1).collect::<Vec<_>>()) {
        print_cli(&info);
        return Ok(());
    }
    recovery::install_panic_hook();
    let cli = parse_cli();
    let size = cli.size.unwrap_or([1280.0, 840.0]);
    let maximized = cli.size.is_none();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size).with_min_inner_size([900.0, 600.0]).with_maximized(maximized).with_title(version::window_title(None, false))
            .with_icon(app_icon()),
        ..Default::default()
    };
    let result = eframe::run_native("Budget Calendar", options, Box::new(|cc| {
        theme::install_fonts(&cc.egui_ctx);
        let app = App::new(cli);
        theme::apply(&cc.egui_ctx, &app.t);
        Ok(Box::new(app))
    }));
    // However the window ended, this run's report files do not outlive it.
    reports::delete_created_reports();
    result
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    fn a(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }
    #[test]
    fn file_argument_is_case_insensitive_and_skips_flag_values() {
        assert_eq!(parse_cli_from(a(&["Home.BGT"])).file.as_deref(), Some("Home.BGT"));
        assert_eq!(parse_cli_from(a(&["--screenshot", "x.png", "--dark", "Plan.Bgt"])).file.as_deref(), Some("Plan.Bgt"));
        assert_eq!(parse_cli_from(a(&["--filter", "x.bgt"])).file, None);
        assert_eq!(parse_cli_from(a(&["--dark"])).file, None);
    }

    #[test]
    fn value_flags_accept_the_equals_form_as_well_as_the_separate_form() {
        let cli = parse_cli_from(a(&["--month=202610", "--today=20261028", "--size=1200x800", "--filter=cat:Auto", "Home.bgt"]));
        assert_eq!((cli.month, cli.today, cli.size, cli.filter.as_deref(), cli.file.as_deref()),
            (Some(202610), Some(20261028), Some([1200.0, 800.0]), Some("cat:Auto"), Some("Home.bgt")));
        assert_eq!(parse_cli_from(a(&["--screenshot=shot.png", "--dark"])).screenshot, Some(PathBuf::from("shot.png")));
        // A flag without its value stays unset rather than taking the next flag.
        assert_eq!(parse_cli_from(a(&["--month", "--dark"])).month, None);
    }
}

#[cfg(test)]
mod banner_tests {
    use super::*;

    #[test]
    fn load_failure_reads_as_a_sentence_and_keeps_the_log_as_details() {
        let log = "(23): Invalid data for 'frequency'\nException: NumberFormatException: \"\"\nfailed";
        assert_eq!(load_error_summary(log), "Line 23: Invalid data for 'frequency'");
        let b = Banner::load_failure(Path::new("target/x/Home.bgt"), log);
        assert_eq!(b.title.as_deref(), Some("Couldn't open Home.bgt"));
        assert_eq!(b.details.as_deref(), Some(log));
        assert!(b.offer_open);
    }

    #[test]
    fn load_failure_summary_handles_warnings_and_generic_heads() {
        // An earlier range warning is not the cause; the line before the exception is.
        let log = "(5): frequency 9 out of range, using 1\n(23): Invalid data for 'amount'\nException: NumberFormatException: abc\nfailed";
        assert_eq!(load_error_summary(log), "Line 23: Invalid data for 'amount'");
        assert_eq!(load_error_summary("unable to open file.\nException: wrong password\nfailed"), "Unable to open file: wrong password");
        assert_eq!(load_error_summary("Exception: java.io.IOException: disk error\nfailed"), "Disk error");
        for damaged in ["mismatched number of read lines! line_no=3, file_line_no=4\nfailed", "misformed end-of-data line: line_no=3, text:'x'\nfailed"] {
            assert_eq!(load_error_summary(damaged), "Part of this budget file is missing, so it may have been damaged.");
        }
        assert_eq!(load_error_summary("The system cannot find the file specified. (os error 2)"), "The system cannot find the file specified. (os error 2)");
        assert_eq!(load_error_summary(""), "The file could not be read.");
        // A single-line error adds no Details section.
        assert_eq!(Banner::load_failure(Path::new("a.bgt"), "Access is denied.").details, None);
        assert_eq!(Banner::plain("x").title, None);
    }
}

#[cfg(test)]
mod dialog_tests {
    use super::*;

    #[test]
    fn each_dialog_counts_as_a_draft_the_shortcuts_sheet_or_another_modal() {
        type Open = fn(&mut Dialogs);
        let open: [(&str, Open); 13] = [
            ("payment editor", |d| d.payment_editor = Some(PaymentEditor::new(20261027))),
            ("transaction editor", |d| d.transaction_editor = Some(TransactionEditor::new(&budget_core::Payment::new(20261027), 20261027, -1.0))),
            ("balance editor", |d| d.balance_editor = Some(BalanceEditor::new(20261027, 0.0, false))),
            ("day", |d| d.day = Some(20261027)),
            ("delete", |d| d.confirm_delete = Some(DeleteTarget { id: 1, date: None })),
            ("pending", |d| d.pending = Some(PendingAction::Close)),
            ("password", |d| d.password = Some(PasswordPrompt { path: PathBuf::from("x.bgt"), bytes: Vec::new(), typed: String::new(), error: String::new(), attempt: None, restore_to: None, has_backup: false })),
            ("incomplete", |d| d.incomplete = Some(IncompleteBudget { path: PathBuf::from("x.bgt"), password: None, db: Box::new(BudgetDb::new(&Cal::from_int(20261001), 12, false)), bytes: Vec::new(), quiet: false, has_backup: false })),
            ("restore", |d| d.restore = Some(RestoreDialog { budget: PathBuf::from("x.bgt"), listing: None, backups: Vec::new() })),
            ("recovery", |d| d.recovery = Some(recovery::Offer { budget: PathBuf::from("x.bgt"), file: PathBuf::from("x.recovery"), bytes: Vec::new(), written: String::new(), file_changed: false })),
            ("about", |d| d.about = true),
            ("legend", |d| d.legend = true),
            ("shortcuts", |d| d.shortcuts = true),
        ];
        let none = Dialogs::default();
        assert!(!none.any() && !none.any_but_shortcuts() && !none.has_draft());
        for (name, set) in open {
            let mut d = Dialogs::default();
            set(&mut d);
            let draft = name.ends_with("editor");
            let shortcuts = name == "shortcuts";
            assert_eq!((d.any(), d.any_but_shortcuts(), d.has_draft()), (true, !shortcuts, draft), "{name}");
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn fixture() -> (App, PathBuf) {
        let dir = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/lifecycle-tests")).unwrap()
            .join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = BudgetConfig::default();
        let t = Tokens::new(&cfg.colors, false);
        let cur = Cal::from_int(20261001);
        let mut db = BudgetDb::new(&cur, 12, false);
        let mut p = budget_core::Payment::new(20261027); p.description = "Fixture payment".into(); p.amount = -12.0;
        db.add(p);
        let app = App {
            sharing: sharing::SharedUi::default(),
            date_watch: DateWatch::fixed(), files: FileProbes::default(), auto_open: None, opening: None, edits: 0, focus_return: None,
            journal: recovery::Journal::new(dir.join("recovery")), undo: undo::History::default(),
            cfg, rc_path: dir.join(".budgetrc"), db: Some(db), file: Some(dir.join("current.bgt")), password: None,
            saved_bytes: Some(b"previous budget".to_vec()), dirty: true, dialogs: Dialogs::default(), drag_month_hover: None,
            show_payments: false, payment_list: payment_list::PaymentList::default(), allow_close: false, cur, today: 20261027,
            filter_text: String::new(), filter: None, filter_error: None, generation: 0, month: None, view: None, haystacks: None,
            toast: None, banner: None,
            screenshot: None, frames: 0, dark: false, show_summary: true, focus_filter: false, title_dirty: true, brand_texture: None, t,
        };
        (app, dir)
    }

    #[test]
    fn failed_save_as_keeps_current_path_unsaved_state_and_snapshot() {
        let (mut app, dir) = fixture();
        let original = app.file.clone();
        let snapshot = app.saved_bytes.clone();
        assert!(!app.save_to(&dir.join("missing/other.bgt"), None));
        assert_eq!(app.file, original);
        assert_eq!(app.saved_bytes, snapshot);
        assert!(app.dirty);
        assert!(app.banner.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disk_conflict_keeps_edits_and_does_not_overwrite_other_app() {
        let (mut app, dir) = fixture();
        std::fs::write(app.file.as_ref().unwrap(), b"other app changed the budget").unwrap();
        assert!(!app.save());
        assert!(app.dirty);
        assert_eq!(std::fs::read(app.file.as_ref().unwrap()).unwrap(), b"other app changed the budget");
        assert_eq!(app.db.as_ref().unwrap().payments[0].amount, -12.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_a_file_with_unsaved_changes_defers_and_preserves_current_document() {
        let (mut app, dir) = fixture();
        app.open(&dir.join("other.bgt"));
        assert!(matches!(app.dialogs.pending, Some(PendingAction::Open(_))));
        assert_eq!(app.file.as_deref(), Some(dir.join("current.bgt").as_path()));
        assert_eq!(app.db.as_ref().unwrap().payments[0].amount, -12.0);
        assert!(app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_a_file_with_only_an_unapplied_draft_also_defers() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.dialogs.payment_editor = Some(PaymentEditor::new(20261027));
        app.open(&dir.join("other.bgt"));
        assert!(app.dialogs.pending.is_some());
        assert!(app.dialogs.payment_editor.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_save_as_switches_path_and_clears_dirty_only_after_write() {
        let (mut app, dir) = fixture();
        let path = dir.join("saved.bgt");
        assert!(app.save_to(&path, None));
        assert!(!app.dirty);
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert_eq!(app.saved_bytes.as_ref().unwrap(), &std::fs::read(path).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_search_is_explained_and_a_valid_search_clears_the_error() {
        let (mut app, dir) = fixture();
        app.filter_text = "~".into();
        app.set_filter();
        assert!(app.filter_error.is_some());
        app.filter_text = "fixture".into();
        app.set_filter();
        assert!(app.filter_error.is_none());
        let payment = &app.db.as_ref().unwrap().payments[0];
        assert!(app.filter.as_ref().unwrap().is_match(payment, -12.0, 20261027));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_a_clean_document_does_not_rewrite_or_create_a_backup() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = app.file.clone().unwrap();
        std::fs::write(&path, b"previous budget").unwrap();
        assert!(app.save());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous budget");
        assert!(!dir.join("current.bgt.bak").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn balance_draft_is_protected_before_opening_another_budget() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.edit_balance(20261027);
        assert_eq!(app.dialogs.balance_editor.as_ref().unwrap().date, 20261027);
        app.open(&dir.join("other.bgt"));
        assert!(app.dialogs.pending.is_some());
        assert!(app.dialogs.balance_editor.is_some());
        assert!(!app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn applying_a_balance_marks_unsaved_and_saves_the_adjustment() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.title_dirty = false;
        app.edit_balance(20261027);
        app.apply_balance_edit(20261027, Some(1234.56));
        assert!(app.dialogs.balance_editor.is_none());
        assert!(app.dirty && app.title_dirty);
        assert_eq!(app.db.as_mut().unwrap().get_day(&Cal::from_int(20261027)).unwrap().closing_balance, 1234.56);
        assert_eq!(app.db.as_ref().unwrap().payments[0].amount, -12.0);
        let saved = dir.join("balance.bgt");
        assert!(app.save_to(&saved, None));
        let loaded = bgt::load_file(&saved, 20261027, None).unwrap();
        assert!(loaded.adjustments.unwrap().opt.iter().any(|o| o.date == 20261027 && o.amount == 1234.56));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_balance_keeps_the_editor_and_does_not_mark_the_budget_dirty() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let before = app.db.as_ref().unwrap().to_text(None).unwrap();
        app.edit_balance(20261027);
        app.apply_balance_edit(20261027, Some(f64::NAN));
        assert!(app.dialogs.balance_editor.is_some());
        assert!(!app.dirty);
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_deleted_occurrence_can_be_restored_and_the_deleted_filter_mode_is_explained() {
        let (mut app, dir) = fixture();
        let id = app.db.as_ref().unwrap().payments[0].id;
        document::delete(app.db.as_mut().unwrap(), id, Some(20261027)).unwrap();
        app.changed();
        assert!(app.db.as_mut().unwrap().get_day(&Cal::from_int(20261027)).unwrap().transactions.is_empty());
        app.dirty = false;
        app.calendar_action(calendar::Action::Restore { id, date: 20261027 });
        assert!(app.dirty);
        assert_eq!(app.db.as_mut().unwrap().get_day(&Cal::from_int(20261027)).unwrap().transactions[0].amount, -12.0);
        assert!(app.db.as_ref().unwrap().payments[0].opt.is_empty());
        app.filter_text = "-fixture".into();
        app.set_filter();
        assert!(app.filter.is_none() && app.filter_error.as_deref().is_some_and(|e| e.contains("Restore")));
        // The hint points to Restore, so Restore must stay available while that text is in the box.
        assert!(app.can_restore());
        app.ensure_view();
        assert!(!app.view.as_ref().unwrap().filtered);
        app.filter_text = "fixture".into();
        app.set_filter();
        assert!(!app.can_restore());
        app.ensure_view();
        assert!(app.view.as_ref().unwrap().filtered);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn successful_drag_move_switches_to_the_destination_month_and_marks_unsaved() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let id = app.db.as_ref().unwrap().payments[0].id;
        app.calendar_action(calendar::Action::MoveTransaction { id, from:20261027, to:20261102, amount:-12.0 });
        assert_eq!(app.cur.get(), 20261101);
        assert!(app.dirty);
        assert_eq!(app.db.as_mut().unwrap().get_day(&Cal::from_int(20261102)).unwrap().transactions[0].amount, -12.0);
        let before = app.db.as_ref().unwrap().to_text(None).unwrap();
        app.dirty = false;
        app.calendar_action(calendar::Action::MoveTransaction { id:99999, from:20261102, to:20261103, amount:-12.0 });
        assert!(!app.dirty);
        assert!(app.banner.is_some());
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn holding_a_drag_on_the_next_month_control_navigates_without_editing_the_budget() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
            events: vec![egui::Event::PointerMoved(egui::pos2(110.0, 110.0))],
            ..Default::default()
        };
        app.drag_month_hover = Some((1, Instant::now() - Duration::from_millis(800)));
        let _ = ctx.run(input, |ctx| {
            egui::DragAndDrop::set_payload(ctx, calendar::DraggedTransaction {id:1,date:20261027,amount:-12.0});
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.allocate_rect(egui::Rect::from_min_size(egui::pos2(100.0, 100.0), Vec2::new(40.0, 30.0)), Sense::click());
                app.drag_month_navigation(ctx, &response, 1);
            });
        });
        assert_eq!(app.cur.get(), 20261101);
        assert!(!app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn transaction_amount_scope_reaches_the_schedule_and_survives_saving() {
        for future in [false, true] {
            let (mut app, dir) = fixture();
            let db = app.db.as_mut().unwrap();
            let id = db.payments[0].id;
            db.payments[0].start_date = 20260927;
            db.payments[0].amount = -150.0;
            db.payments[0].set_end_mode(budget_core::END_MODE_INDEFINITE);
            db.rebuild_db(&app.cur, 12, false);
            app.dirty = false;
            app.edit_transaction(id, 20261027, -150.0);
            let scope = if future { TransactionScope::Future } else { TransactionScope::Occurrence };
            app.apply_transaction_edit(id, 20261027, -250.0, "This date only", budget_core::FLAG_PAID, scope);
            assert!(app.dirty);
            assert!(app.dialogs.transaction_editor.is_none());
            let saved = dir.join("scoped-amount.bgt");
            assert!(app.save_to(&saved, None));
            assert!(!app.dirty);
            let loaded = bgt::load_file(&saved, app.today, None).unwrap();
            let mut restored = BudgetDb::from_loaded(loaded, &app.cur, 12, true);
            for (date, expected) in [(20260927, -150.0), (20261027, -250.0),
                (20261127, if future { -250.0 } else { -150.0 }),
                (20261227, if future { -250.0 } else { -150.0 })] {
                let actual = restored.get_day(&Cal::from_int(date)).unwrap().transactions[0].amount;
                assert_eq!(actual, expected, "scope {scope:?}, date {date}");
            }
            // The future scope splits the payment (04 §18), so the selected date's note lives on
            // whichever record holds that occurrence.
            assert_eq!(restored.payments.len(), if future { 2 } else { 1 });
            let note = |date| restored.payments.iter().find_map(|p| p.get_note(date));
            assert_eq!(note(20261027).as_deref(), Some("This date only"));
            assert!(note(20261127).is_none());
            assert!(restored.payments.iter().all(|p| p.get_flag(20261127) == budget_core::FLAG_NONE));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn rejected_future_amount_keeps_the_transaction_draft_and_document_unchanged() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let id = app.db.as_ref().unwrap().payments[0].id;
        let before = app.db.as_ref().unwrap().to_text(None).unwrap();
        app.edit_transaction(id, 20261027, -12.0);
        app.apply_transaction_edit(id, 20261027, f64::NAN, "draft", budget_core::FLAG_PAID, TransactionScope::Future);
        assert!(!app.dirty);
        assert!(app.dialogs.transaction_editor.is_some());
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn dense_fixture() -> (App, PathBuf) {
        let (mut app, dir) = fixture();
        let mut db = BudgetDb::new(&app.cur, 12, false);
        for (date, count) in [(20261002, 4), (20261008, 5), (20261016, 5), (20261022, 5), (20261030, 4)] {
            for number in 0..count {
                let mut payment = budget_core::Payment::new(date);
                payment.description = format!("Bill {}-{number}", date % 100);
                payment.amount = -150.0 - number as f64;
                db.add(payment);
            }
        }
        app.db = Some(db);
        app.dirty = false;
        app.title_dirty = false;
        (app, dir)
    }

    fn rendered_text(output: &egui::FullOutput) -> Vec<(egui::epaint::TextShape, egui::Rect)> {
        fn visit(shape: &egui::Shape, clip: egui::Rect, texts: &mut Vec<(egui::epaint::TextShape, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => texts.push((text.clone(), clip)),
                egui::Shape::Vec(shapes) => for shape in shapes { visit(shape, clip, texts); },
                _ => {},
            }
        }
        let mut texts = Vec::new();
        for shape in &output.shapes { visit(&shape.shape, shape.clip_rect, &mut texts); }
        texts
    }

    #[test]
    fn full_app_shows_the_normal_months_busy_day_lists() {
        let (mut app, dir) = dense_fixture();
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        let mut texts = Vec::new();
        let mut large_rects = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1200.0, 800.0))),
                ..Default::default()
            }, |ctx| app.render(ctx));
            texts = rendered_text(&output);
            large_rects = output.shapes.iter().filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.rect.width() > 1000.0 && r.rect.height() > 100.0 => Some(r.rect),
                _ => None,
            }).collect::<Vec<_>>();
        }
        for (date, count) in [(2, 4), (8, 5), (16, 5), (22, 5), (30, 4)] {
            for number in 0..count {
                let label = format!("Bill {date}-{number}");
                let (shape, clip) = texts.iter().find(|(text, _)| text.galley.job.text == label)
                    .unwrap_or_else(|| panic!("the full app must show {label} directly in the calendar; shown {:?}; view {} transactions; large rects {large_rects:?}", texts.iter().filter(|(text, _)| text.galley.job.text.starts_with("Bill")).map(|(text, _)| &text.galley.job.text).collect::<Vec<_>>(), app.view.as_ref().unwrap().shown));
                let ink = shape.galley.mesh_bounds.translate(shape.pos.to_vec2());
                assert!(clip.expand(0.01).contains_rect(ink), "{label} is clipped: {ink:?} in {clip:?}");
            }
        }
        assert!(!texts.iter().any(|(text, _)| text.galley.job.text.starts_with('+')),
            "the normal busy days must not need an overflow popup");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_full_app_shows_multiple_payments_with_amounts() {
        let (mut app, dir) = dense_fixture();
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                ..Default::default()
            }, |ctx| app.render(ctx));
            texts = rendered_text(&output);
        }
        for date in [2, 8, 16, 22, 30] {
            assert!(texts.iter().filter(|(text, _)| text.galley.job.text.starts_with(&format!("Bill {date}-"))).count() >= 2,
                "the compact full app must show at least two payments on busy day {date}");
        }
        let amounts = ["−150", "−151", "−152", "−153", "−154"];
        for (name, _) in texts.iter().filter(|(text, _)| text.galley.job.text.starts_with("Bill ")) {
            let number: usize = name.galley.job.text.rsplit('-').next().unwrap().parse().unwrap();
            let name_y = name.pos.y + name.galley.mesh_bounds.center().y;
            assert!(texts.iter().any(|(amount, _)| amount.galley.job.text.starts_with(amounts[number]) &&
                (amount.pos.y + amount.galley.mesh_bounds.center().y - name_y).abs() <= 1.0),
                "{} must have its amount on the same line", name.galley.job.text);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_summary_values_have_explicit_ellipsis_and_exact_hover_text() {
        let (mut app, dir) = dense_fixture();
        app.ensure_view();
        app.view.as_mut().unwrap().quick.opening = -123_456_789.12;
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let mut texts = Vec::new();
        for time in [0.0, 0.1, 0.2] {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                time: Some(time), ..Default::default()
            }, |ctx| app.render(ctx));
            texts = rendered_text(&output);
        }
        let (amount, clip) = texts.iter().find(|(text, _)| text.galley.job.text == "−123,456,789.12").unwrap();
        assert!(amount.galley.elided, "an oversized value must show an ellipsis rather than silently lose leading digits");
        let ink = amount.galley.mesh_bounds.translate(amount.pos.to_vec2());
        assert!(clip.expand(0.01).contains_rect(ink));
        let pointer = clip.center();
        for time in [1.0, 1.1, 1.2] {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                events: vec![egui::Event::PointerMoved(pointer)], time: Some(time), ..Default::default()
            }, |ctx| app.render(ctx));
            texts = rendered_text(&output);
        }
        assert!(texts.iter().any(|(text, _)| text.galley.job.text.contains("Opening: −123,456,789.12")),
            "hover must expose the complete signed summary value");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn visible_version_opens_history_and_is_in_the_native_title() {
        let (mut app, dir) = dense_fixture();
        app.title_dirty = true;
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        let mut texts = Vec::new();
        let mut version_in_title = false;
        for _ in 0..3 {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                ..Default::default()
            }, |ctx| app.render(ctx));
            version_in_title |= output.viewport_output.values().flat_map(|viewport| &viewport.commands).any(|command|
                matches!(command, egui::ViewportCommand::Title(title) if title.contains(&format!("v{}", version::VERSION)) && title.contains("current.bgt")));
            texts = rendered_text(&output);
        }
        let (badge, _) = texts.iter().find(|(text, _)| text.galley.job.text == format!("v{}", version::VERSION))
            .expect("the current app version must be visible without opening a menu");
        assert!(version_in_title, "the native app title must identify the running version");
        let pointer = badge.pos + badge.galley.mesh_bounds.center().to_vec2();
        for pressed in [true, false] {
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                events: vec![egui::Event::PointerMoved(pointer), egui::Event::PointerButton {
                    pos: pointer, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE
                }], ..Default::default()
            }, |ctx| app.render(ctx));
        }
        assert!(app.dialogs.about, "the version button must open the history dialog");
        let output = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
            ..Default::default()
        }, |ctx| app.render(ctx));
        texts = rendered_text(&output);
        assert!(texts.iter().any(|(text, _)| text.galley.job.text == "Version history"),
            "clicking the visible version must open the version history");
        assert!(!app.dirty, "viewing versions must not edit the budget");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_history_fits_compact_windows_copies_identity_and_preserves_a_draft() {
        let (mut app, dir) = dense_fixture();
        app.edit_balance(20261027);
        app.dialogs.about = true;
        let before = app.db.as_ref().unwrap().to_text(None).unwrap();
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0));
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| app.render(ctx));
            texts = rendered_text(&output);
        }
        for label in ["Version history", "Copy version info", "Close"] {
            let (text, clip) = texts.iter().find(|(text, _)| text.galley.job.text == label).unwrap();
            let ink = text.galley.mesh_bounds.translate(text.pos.to_vec2());
            assert!(screen.contains_rect(ink) && clip.expand(0.01).contains_rect(ink), "{label} must remain visible in the compact dialog");
        }
        assert!(!texts.iter().any(|(text, _)| text.galley.job.text == "Edit balance"),
            "the underlying editor must not process the history dialog's input");
        let (copy, _) = texts.iter().find(|(text, _)| text.galley.job.text == "Copy version info").unwrap();
        let pointer = copy.pos + copy.galley.mesh_bounds.center().to_vec2();
        let mut copied = false;
        for pressed in [true, false] {
            let output = ctx.run(egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(pointer), egui::Event::PointerButton {
                    pos: pointer, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE
                }], ..Default::default()
            }, |ctx| app.render(ctx));
            copied |= output.platform_output.commands.iter().any(|command|
                matches!(command, egui::OutputCommand::CopyText(text) if text.contains(version::VERSION) && text.contains(version::BUILD_ID)));
        }
        assert!(copied, "copy must contain the current version and exact build identity");
        let _ = ctx.run(egui::RawInput {
            screen_rect: Some(screen), events: vec![egui::Event::Key {
                key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE
            }], ..Default::default()
        }, |ctx| app.render(ctx));
        assert!(!app.dialogs.about);
        assert!(app.dialogs.balance_editor.is_some(), "closing history must retain an unfinished balance edit");
        assert!(!app.dirty);
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The Windows AccessKit adapter panics when an update names a focused node it does not contain.
    fn assert_focus_is_in_the_tree(output: &egui::FullOutput) {
        let update = output.platform_output.accesskit_update.as_ref().expect("AccessKit output is switched on");
        assert!(update.nodes.iter().any(|(id, _)| *id == update.focus), "focused node {:?} is not in the update", update.focus);
    }

    #[test]
    fn closing_a_dialog_opened_from_a_menu_item_does_not_focus_the_vanished_item() {
        let (mut app, dir) = fixture();
        let ctx = gui(&app);
        ctx.enable_accesskit();
        frame(&mut app, &ctx, vec![], false);
        // The menu that opened the dialog closed with it, so the remembered control no longer exists.
        app.focus_return = Some(egui::Id::new("menu item that closed with its menu"));
        app.dialogs.legend = true;
        for _ in 0..2 { assert_focus_is_in_the_tree(&frame(&mut app, &ctx, vec![], false)); }
        let output = frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        assert!(!app.dialogs.legend, "Escape closes the legend");
        assert_focus_is_in_the_tree(&output);
        assert_focus_is_in_the_tree(&frame(&mut app, &ctx, vec![], false));
        assert_eq!(app.focus_return, None);
        assert_eq!(ctx.memory(|m| m.focused()), None);
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closing_a_dialog_still_returns_focus_to_a_control_that_exists() {
        let (mut app, dir) = fixture();
        let ctx = gui(&app);
        ctx.enable_accesskit();
        for _ in 0..2 { frame(&mut app, &ctx, vec![], false); }
        frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::NONE)], false);
        let control = ctx.memory(|m| m.focused()).expect("Tab focuses a control");
        app.focus_return = Some(control);
        app.dialogs.legend = true;
        frame(&mut app, &ctx, vec![], false);
        let output = frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        assert_focus_is_in_the_tree(&output);
        assert_eq!(ctx.memory(|m| m.focused()), Some(control));
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>, close: bool) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 840.0))),
            events, ..Default::default()
        };
        if close {
            input.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { events: vec![egui::ViewportEvent::Close], ..Default::default() });
        }
        ctx.run(input, |ctx| app.render(ctx))
    }

    /// Renders frames until `done` holds (a background open finishing), for up to ten seconds.
    fn settle(app: &mut App, ctx: &egui::Context, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(app) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            frame(app, ctx, vec![], false);
        }
        assert!(done(app), "timed out waiting for the background open");
    }

    fn gui(app: &App) -> egui::Context {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &app.t);
        ctx
    }

    /// The same file spelled relative to the test process's working directory.
    fn relative(path: &Path) -> PathBuf {
        let cwd = std::env::current_dir().unwrap();
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(std::fs::canonicalize(&cwd).unwrap(), std::fs::canonicalize(base).unwrap());
        let root = std::path::absolute(base.join("..")).unwrap();
        PathBuf::from("..").join(path.strip_prefix(root).unwrap())
    }

    #[test]
    fn loaded_budget_uses_the_app_today_for_tentative_scheduling() {
        let (mut app, dir) = fixture();
        app.today = 20270115;
        let path = dir.join("today.bgt");
        assert!(app.save_to(&path, None));
        app.load(&path, None);
        assert_eq!(app.db.as_ref().unwrap().today, 20270115);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relative_paths_are_remembered_as_absolute_paths() {
        let (mut app, dir) = fixture();
        let path = dir.join("rel.bgt");
        assert!(app.save_to(&path, None));
        app.cfg.recent_file.clear();
        app.load(&relative(&path), None);
        assert!(app.file.as_ref().unwrap().is_absolute());
        assert!(Path::new(app.cfg.last_file_name.as_deref().unwrap()).is_absolute());
        assert!(Path::new(&app.cfg.recent_file[0]).is_absolute());
        app.load(&path, None);
        assert_eq!(app.cfg.recent_file.len(), 1, "one file must not be listed under two spellings");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_as_to_another_spelling_of_the_open_file_keeps_the_conflict_check() {
        let (mut app, dir) = fixture();
        let path = app.file.clone().unwrap();
        std::fs::write(&path, b"previous budget").unwrap();
        app.file = Some(relative(&path));
        std::fs::write(&path, b"other app changed the budget").unwrap();
        let expected = app.save_as_baseline(&path).unwrap();
        assert!(!app.save_to(&path, expected.as_deref()));
        assert_eq!(std::fs::read(&path).unwrap(), b"other app changed the budget");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn close_request_supersedes_a_pending_open() {
        let (mut app, dir) = fixture();
        app.dialogs.pending = Some(PendingAction::Open(dir.join("other.bgt")));
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], true);
        assert!(matches!(app.dialogs.pending, Some(PendingAction::Close)));
        assert!(!app.allow_close);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn password_unlock_runs_in_background_and_cancel_preserves_the_open_budget() {
        let (mut app, dir) = fixture();
        let previous = app.file.clone();
        let path = dir.join("cancel-unlock.bgt");
        let text = app.db.as_ref().unwrap().to_text(Some("secret")).unwrap();
        bgt::save_file(&path, &text).unwrap();
        app.dirty = false;
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.password.is_some());
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![egui::Event::Text("secret".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        assert!(app.dialogs.password.is_some(), "unlock must return with a cancellable password job, without decrypting on the UI thread");
        frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        assert!(app.dialogs.password.is_none() && app.opening.is_none());
        assert_eq!(app.file, previous, "cancel must preserve the open document");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejected_password_keeps_the_field_usable_from_the_keyboard() {
        let (mut app, dir) = fixture();
        let path = dir.join("secret.bgt");
        let text = app.db.as_ref().unwrap().to_text(Some("secret")).unwrap();
        bgt::save_file(&path, &text).unwrap();
        app.dirty = false;
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.password.is_some());
        // The file was read once, when it was opened: unlocking works from what was read.
        std::fs::remove_file(&path).unwrap();
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![egui::Event::Text("wrong".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        settle(&mut app, &ctx, |app| app.dialogs.password.as_ref().is_some_and(|p| !p.error.is_empty()));
        assert_eq!(app.dialogs.password.as_ref().unwrap().error, "The password is incorrect or the encrypted file is damaged.");
        frame(&mut app, &ctx, vec![], false);
        assert!(ctx.memory(|m| m.focused()).is_some(), "the password field must regain focus");
        frame(&mut app, &ctx, vec![egui::Event::Text("secret".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        settle(&mut app, &ctx, |app| app.dialogs.password.is_none());
        assert!(app.dialogs.password.is_none(), "retyping and Enter must unlock");
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert_eq!(app.file.as_deref().map(file_name).as_deref(), Some("secret.bgt"));
        assert_eq!(app.password.as_deref(), Some("secret"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typing_a_search_reuses_the_month_and_an_edit_or_another_budget_rebuilds_it() {
        let (mut app, dir) = fixture();
        app.ensure_view();
        let row = |app: &App| app.view.as_ref().unwrap().cells.iter().flat_map(|c| &c.rows).next().cloned();
        let before = row(&app).unwrap();
        for text in ["fix", "fixture", "fixture zz"] {
            app.filter_text = text.into();
            app.set_filter();
            app.ensure_view();
            assert!(app.view.as_ref().unwrap().filtered);
        }
        assert!(row(&app).is_none(), "\"fixture zz\" matches nothing");
        app.filter_text = "fixture".into();
        app.set_filter();
        app.ensure_view();
        assert!(std::rc::Rc::ptr_eq(&before, &row(&app).unwrap()), "keystrokes reuse the month's rows instead of rebuilding them");
        let id = app.db.as_ref().unwrap().payments[0].id;
        document::change_transaction(app.db.as_mut().unwrap(), id, 20261027, -13.0, "", budget_core::FLAG_NONE).unwrap();
        app.changed();
        app.ensure_view();
        let edited = row(&app).unwrap();
        assert!(!std::rc::Rc::ptr_eq(&before, &edited) && edited.value == -13.0, "an edit rebuilds the month");
        // Another budget is never served from the previous one's month, even at the same engine count.
        let count = app.db.as_ref().unwrap().update_count;
        let mut other = BudgetDb::new(&app.cur, 12, false);
        let mut p = budget_core::Payment::new(20261027); p.description = "Fixture other".into(); p.amount = -99.0;
        other.add(p);
        let path = dir.join("other.bgt");
        bgt::save_file(&path, &other.to_text(None).unwrap()).unwrap();
        app.dirty = false;
        app.load(&path, None);
        app.db.as_mut().unwrap().update_count = count;
        app.ensure_view();
        assert_eq!(row(&app).unwrap().value, -99.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clicking_a_description_with_an_ampersand_filters_to_it() {
        let (mut app, dir) = fixture();
        app.db.as_mut().unwrap().payments[0].description = "AT&T".into();
        app.apply_filter("desc:AT&T");
        app.ensure_view();
        assert_eq!(app.view.as_ref().unwrap().shown, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn view_preferences_are_written_when_the_app_closes() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.cfg.show_decimals_in_cv = true;
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], true);
        let saved = BudgetConfig::load(&app.rc_path);
        assert!(saved.show_decimals_in_cv);
        assert_eq!(saved.last_file_name.as_deref(), Some(app.file.as_ref().unwrap().display().to_string().as_str()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn screenshot_mode_leaves_the_user_configuration_alone() {
        let (mut app, dir) = fixture();
        let path = dir.join("shot.bgt");
        bgt::save_file(&path, &app.db.as_ref().unwrap().to_text(None).unwrap()).unwrap();
        app.screenshot = Some(dir.join("shot.png"));
        app.dirty = false;
        app.load(&path, None);
        assert!(app.db.is_some() && app.file.as_deref() == Some(path.as_path()));
        assert!(!app.rc_path.exists(), "automation runs must not rewrite .budgetrc");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn editing_from_the_day_dialog_returns_to_the_day_list() {
        let (mut app, dir) = fixture();
        let id = app.db.as_ref().unwrap().payments[0].id;
        app.dialogs.day = Some(20261027);
        app.edit_transaction(id, 20261027, -12.0);
        app.apply_transaction_edit(id, 20261027, -20.0, "", budget_core::FLAG_NONE, TransactionScope::Occurrence);
        assert!(app.dialogs.transaction_editor.is_none());
        assert_eq!(app.dialogs.day, Some(20261027));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dialog_shortcuts_are_ignored_during_a_drag() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        egui::DragAndDrop::set_payload(&ctx, calendar::DraggedTransaction { id: 1, date: 20261027, amount: -12.0 });
        frame(&mut app, &ctx, vec![key(Key::N, Modifiers::COMMAND), key(Key::F1, Modifiers::NONE), key(Key::P, Modifiers::COMMAND)], false);
        assert!(app.dialogs.payment_editor.is_none());
        assert!(!app.dialogs.shortcuts);
        assert!(!app.show_payments);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typing_a_year_does_not_navigate_until_it_is_committed() {
        let (mut app, dir) = dense_fixture();
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..2 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let click = |app: &mut App, pos: egui::Pos2| -> Vec<(egui::epaint::TextShape, egui::Rect)> {
            let mut out = Vec::new();
            for pressed in [true, false] {
                out = rendered_text(&frame(app, &ctx, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton {
                    pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }], false));
            }
            for _ in 0..2 { out = rendered_text(&frame(app, &ctx, vec![], false)); }
            out
        };
        let center = |texts: &Vec<(egui::epaint::TextShape, egui::Rect)>, pred: &dyn Fn(&str) -> bool| {
            let (t, _) = texts.iter().find(|(t, _)| pred(&t.galley.job.text)).unwrap();
            t.pos + t.galley.mesh_bounds.center().to_vec2()
        };
        let month = center(&texts, &|s| s.starts_with("October 2026"));
        texts = click(&mut app, month);
        let year = center(&texts, &|s| s == "2026");
        click(&mut app, year);
        frame(&mut app, &ctx, vec![key(Key::A, Modifiers::COMMAND), egui::Event::Text("2".into())], false);
        frame(&mut app, &ctx, vec![], false);
        assert_eq!(app.cur.get(), 20261001, "a partially typed year must not move the calendar");
        frame(&mut app, &ctx, vec![egui::Event::Text("027".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        frame(&mut app, &ctx, vec![], false);
        assert_eq!(app.cur.get(), 20271001);
        std::fs::remove_dir_all(dir).unwrap();
    }

    type Texts = Vec<(egui::epaint::TextShape, egui::Rect)>;

    fn text_center(texts: &Texts, pred: impl Fn(&str) -> bool) -> Option<egui::Pos2> {
        texts.iter().find(|(t, _)| pred(&t.galley.job.text)).map(|(t, _)| t.pos + t.galley.mesh_bounds.center().to_vec2())
    }

    fn click_at(app: &mut App, ctx: &egui::Context, pos: egui::Pos2) -> Texts {
        frame(app, ctx, vec![egui::Event::PointerMoved(pos)], false);
        for pressed in [true, false] {
            frame(app, ctx, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }], false);
        }
        rendered_text(&frame(app, ctx, vec![], false))
    }

    /// The rectangle of the widget that has keyboard focus.
    fn focused_rect(ctx: &egui::Context) -> Option<(egui::Id, egui::Rect)> {
        let id = ctx.memory(|m| m.focused())?;
        Some((id, ctx.read_response(id)?.rect))
    }

    #[test]
    fn tab_walks_header_search_month_tiles_and_calendar_rows_and_shift_tab_goes_back() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        // This walk is of the calendar page alone; the summary panel, added first, is walked before it.
        app.show_summary = false;
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let landmarks = [
            ("search", text_center(&texts, |s| s == "Search transactions").unwrap()),
            ("month", text_center(&texts, |s| s.starts_with("October 2026")).unwrap()),
            ("opening", text_center(&texts, |s| s == "Opening balance").unwrap()),
            ("closing", text_center(&texts, |s| s == "Closing balance").unwrap()),
            ("row", text_center(&texts, |s| s == "Fixture payment").unwrap()),
        ];
        // Each key press is followed by an idle frame, as when a person presses Tab repeatedly.
        let mut order: Vec<(egui::Id, Option<&str>)> = Vec::new();
        for _ in 0..60 {
            frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::NONE)], false);
            frame(&mut app, &ctx, vec![], false);
            let (id, rect) = focused_rect(&ctx).unwrap_or_else(|| panic!("Tab left nothing focused after {:?}", order.iter().map(|(_, n)| n).collect::<Vec<_>>()));
            order.push((id, landmarks.iter().find(|(_, pos)| rect.contains(*pos)).map(|(name, _)| *name)));
            if order.len() > 1 && order[0].0 == id { break; }
        }
        let first = |name: &str| order.iter().position(|(_, n)| *n == Some(name))
            .unwrap_or_else(|| panic!("Tab never reached {name}: {:?}", order.iter().map(|(_, n)| n).collect::<Vec<_>>()));
        let steps: Vec<usize> = landmarks.iter().map(|(name, _)| first(name)).collect();
        assert!(steps.windows(2).all(|w| w[0] < w[1]), "header → search → month → tiles → rows, got steps {steps:?}");
        assert_eq!(order.last().unwrap().0, order[0].0, "after the calendar Tab wraps around to the header");
        // Walk back to the row, then Shift+Tab returns to the closing balance tile.
        while focused_rect(&ctx).and_then(|(_, r)| r.contains(landmarks[4].1).then_some(())).is_none() {
            frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::SHIFT)], false);
            frame(&mut app, &ctx, vec![], false);
        }
        let (row_id, row_rect) = focused_rect(&ctx).unwrap();
        let ring = frame(&mut app, &ctx, vec![], false).shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Rect(r) if r.stroke.width >= 2.0 && r.stroke.color == app.t.accent && r.rect == row_rect));
        assert!(ring, "a focused row shows a 2px accent focus ring inside its unchanged rectangle");
        frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::SHIFT)], false);
        frame(&mut app, &ctx, vec![], false);
        assert!(focused_rect(&ctx).unwrap().1.contains(landmarks[3].1), "Shift+Tab from the first row goes back to the summary tiles");
        // Back on the row: Enter opens this date's editor, and closing it returns focus to the row.
        frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::NONE)], false);
        frame(&mut app, &ctx, vec![], false);
        assert_eq!(focused_rect(&ctx).unwrap().0, row_id);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        let editor = app.dialogs.transaction_editor.as_ref().expect("Enter on a focused row opens the transaction editor");
        assert_eq!((editor.payment_id, editor.date), (app.db.as_ref().unwrap().payments[0].id, 20261027));
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        frame(&mut app, &ctx, vec![], false);
        assert!(app.dialogs.transaction_editor.is_none());
        assert_eq!(ctx.memory(|m| m.focused()), Some(row_id), "closing the editor returns focus to the row");
        frame(&mut app, &ctx, vec![key(Key::Space, Modifiers::NONE)], false);
        assert!(app.dialogs.transaction_editor.is_some(), "Space opens the editor too");
        assert!(!app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn filtered_day_dialog_lists_matching_transactions_before_the_others() {
        let (mut app, dir) = dense_fixture();
        let ctx = gui(&app);
        // Bill 8-3 and 8-4 (−153, −154) match; 8-0..8-2 do not.
        app.filter_text = "<-152.5".into();
        app.set_filter();
        app.dialogs.day = Some(20261008);
        // Tall enough that the settled dialog lists all five without scrolling.
        let tall = |app: &mut App| rendered_text(&ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 1400.0))), ..Default::default() }, |ctx| app.render(ctx)));
        let mut texts = Vec::new();
        for _ in 0..12 { texts = tall(&mut app); }
        // The dialog names payments in the semibold face; the calendar uses the body face.
        let semi = egui::FontFamily::Name("semi".into());
        let dialog_y = |name: &str| texts.iter().find(|(t, _)| t.galley.job.text == name && t.galley.job.sections[0].format.font_id.family == semi)
            .map(|(t, _)| t.pos.y).unwrap_or_else(|| panic!("the day dialog must list {name}"));
        let other = text_center(&texts, |s| s == "Other transactions on this day").expect("non-matching transactions get their own section").y;
        assert!(texts.iter().any(|(t, _)| t.galley.job.text == "2 of 5 transactions match the search"));
        for name in ["Bill 8-3", "Bill 8-4"] { assert!(dialog_y(name) < other, "{name} matches and is listed first"); }
        for name in ["Bill 8-0", "Bill 8-1", "Bill 8-2"] { assert!(dialog_y(name) > other, "{name} is listed under the other transactions"); }
        app.filter_text.clear();
        app.set_filter();
        for _ in 0..2 { texts = tall(&mut app); }
        assert!(texts.iter().any(|(t, _)| t.galley.job.text == "5 transactions"));
        assert!(!texts.iter().any(|(t, _)| t.galley.job.text == "Other transactions on this day"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn escape_closes_the_day_dialog_menu_first_and_the_backdrop_closes_the_dialog() {
        let (mut app, dir) = fixture();
        let ctx = gui(&app);
        app.dialogs.day = Some(20261027);
        let mut texts = Vec::new();
        for _ in 0..2 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let more = text_center(&texts, |s| s == "More").expect("the day dialog offers More");
        texts = click_at(&mut app, &ctx, more);
        assert!(texts.iter().any(|(t, _)| t.galley.job.text == "Edit whole payment"), "More opens its menu");
        frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(!texts.iter().any(|(t, _)| t.galley.job.text == "Edit whole payment"), "Escape closes the menu");
        assert_eq!(app.dialogs.day, Some(20261027), "the first Escape closes only the menu");
        frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        assert_eq!(app.dialogs.day, None, "a second Escape closes the dialog");
        // A press on the backdrop while the menu is open dismisses only the menu, as Escape does.
        app.dialogs.day = Some(20261027);
        frame(&mut app, &ctx, vec![], false);
        click_at(&mut app, &ctx, more);
        let backdrop = egui::pos2(20.0, 420.0);
        texts = click_at(&mut app, &ctx, backdrop);
        assert!(!texts.iter().any(|(t, _)| t.galley.job.text == "Edit whole payment"));
        assert_eq!(app.dialogs.day, Some(20261027));
        click_at(&mut app, &ctx, backdrop);
        assert_eq!(app.dialogs.day, None, "a backdrop click closes the day dialog, like the other read-only dialogs");
        app.dialogs.legend = true;
        frame(&mut app, &ctx, vec![], false);
        click_at(&mut app, &ctx, backdrop);
        assert!(!app.dialogs.legend, "the legend closes the same way");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn double_clicking_plus_n_leaves_the_day_dialog_open() {
        let (mut app, dir) = fixture();
        let db = app.db.as_mut().unwrap();
        for n in 0..60 {
            let mut p = budget_core::Payment::new(20261004);
            p.description = format!("Busy {n}"); p.amount = -1.0 - n as f64;
            db.add(p);
        }
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let more = text_center(&texts, |s| s.len() > 1 && s.starts_with('+') && s[1..].chars().all(|c| c.is_ascii_digit())).expect("the busy day shows +N");
        assert!(more.x < 300.0, "+N must lie outside the centred dialog for this test: {more:?}");
        let button = |pressed| egui::Event::PointerButton { pos: more, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(more)], false);
        for pressed in [true, false] { frame(&mut app, &ctx, vec![button(pressed)], false); }
        // The second press follows a few frames later (each test frame is 1/60 s; the double-click window is 0.3 s).
        for _ in 0..4 { frame(&mut app, &ctx, vec![], false); }
        assert_eq!(app.dialogs.day, Some(20261004), "the first click opens the day dialog");
        for pressed in [true, false] { frame(&mut app, &ctx, vec![button(pressed)], false); }
        frame(&mut app, &ctx, vec![], false);
        assert_eq!(app.dialogs.day, Some(20261004), "the double-click's second click lands on the backdrop but must not close the dialog it opened");
        for _ in 0..30 { frame(&mut app, &ctx, vec![], false); }
        click_at(&mut app, &ctx, more);
        assert_eq!(app.dialogs.day, None, "a later backdrop click closes it");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_dropped_while_a_dialog_is_open_is_explained_not_silently_ignored() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 840.0));
        let dropped = dir.join("other.bgt");
        let hover = |app: &mut App| rendered_text(&ctx.run(egui::RawInput { screen_rect: Some(screen),
            hovered_files: vec![egui::HoveredFile { path: Some(dropped.clone()), ..Default::default() }], ..Default::default() }, |ctx| app.render(ctx)));
        assert!(hover(&mut app).iter().any(|(t, _)| t.galley.job.text == "Drop to open"), "without a dialog the drop target is shown");
        app.dialogs.legend = true;
        assert!(!hover(&mut app).iter().any(|(t, _)| t.galley.job.text == "Drop to open"), "no drop target while a dialog is open");
        let _ = ctx.run(egui::RawInput { screen_rect: Some(screen),
            dropped_files: vec![egui::DroppedFile { path: Some(dropped.clone()), ..Default::default() }], ..Default::default() }, |ctx| app.render(ctx));
        assert!(app.toast.as_ref().is_some_and(|t| t.error && t.text.contains("Close the open dialog")), "the drop is explained");
        assert!(app.dialogs.legend && app.dialogs.pending.is_none() && app.dialogs.password.is_none());
        assert_eq!(app.file.as_deref(), Some(dir.join("current.bgt").as_path()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_local_date_becomes_today_while_the_app_is_open() {
        use std::sync::atomic::AtomicI32;
        use std::sync::Arc;
        let (mut app, dir) = fixture();
        app.dirty = false;
        let clock = Arc::new(AtomicI32::new(20261027));
        let source = clock.clone();
        app.date_watch = DateWatch::new(Some(Box::new(move || source.load(Ordering::Relaxed))), Instant::now());
        let ctx = gui(&app);
        let wake = |output: &egui::FullOutput| output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        let mut output = frame(&mut app, &ctx, vec![], false);
        for _ in 0..3 { output = frame(&mut app, &ctx, vec![], false); }
        assert_eq!(app.today, 20261027);
        assert!(wake(&output) <= DateWatch::INTERVAL, "an idle window wakes for the next date check: {:?}", wake(&output));
        // Midnight passes: the grid and the scheduler's today follow without any input.
        clock.store(20261028, Ordering::Relaxed);
        app.date_watch.expire();
        frame(&mut app, &ctx, vec![], false);
        assert_eq!((app.today, app.db.as_ref().unwrap().today), (20261028, 20261028));
        let today: Vec<i32> = app.view.as_ref().unwrap().cells.iter().filter(|c| c.is_today).map(|c| c.date).collect();
        assert_eq!(today, [20261028], "the grid marks the new day");
        assert!(!app.dirty, "a new day is not an edit");
        // Someone looking at the current month moves into the next one with it...
        clock.store(20261101, Ordering::Relaxed);
        app.date_watch.expire();
        frame(&mut app, &ctx, vec![], false);
        assert_eq!((app.today, app.cur.get()), (20261101, 20261101));
        // ...while someone browsing another month stays there.
        app.go(2);
        clock.store(20261102, Ordering::Relaxed);
        app.date_watch.expire();
        frame(&mut app, &ctx, vec![], false);
        assert_eq!((app.today, app.cur.get()), (20261102, 20270101));
        // `--today` keeps the date fixed and needs no wake-ups.
        app.date_watch = DateWatch::fixed();
        clock.store(20261103, Ordering::Relaxed);
        for _ in 0..3 { output = frame(&mut app, &ctx, vec![], false); }
        assert_eq!(app.today, 20261102);
        assert!(wake(&output) > DateWatch::INTERVAL, "{:?}", wake(&output));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recent_budgets_on_a_stalled_share_never_block_the_window() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        app.cfg.recent_file = vec![r"\\stalled-server\budgets\Home.bgt".to_string()];
        let (gate, probes) = file_probe::tests::Gate::probes(FileStatus::Present(Some(std::time::SystemTime::now())));
        app.files = probes;
        let ctx = gui(&app);
        let started = Instant::now();
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        assert!(started.elapsed() < Duration::from_secs(2), "frames must not wait for the file check");
        let shown = |texts: &Texts, s: &str| texts.iter().any(|(t, _)| t.galley.job.text == s);
        assert!(shown(&texts, "Home.bgt") && shown(&texts, "Checking…"), "a neutral placeholder until the answer arrives");
        assert!(!shown(&texts, "File not found"));
        gate.open();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !shown(&texts, "Modified just now") && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        }
        assert!(shown(&texts, "Modified just now"));
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1, "each path is checked once, not every frame");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clicking_a_recent_budget_on_a_stalled_share_opens_it_without_blocking_the_window() {
        let (mut app, dir) = fixture();
        let path = dir.join("Home.bgt");
        bgt::save_file(&path, &app.db.as_ref().unwrap().to_text(None).unwrap()).unwrap();
        (app.db, app.file, app.dirty) = (None, None, false);
        app.cfg.recent_file = vec![path.display().to_string()];
        let (gate, probes) = file_probe::tests::Gate::reads();
        app.files = probes;
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let card = text_center(&texts, |s| s == "Home.bgt").expect("the recent budget has a card");
        // Pressed where the pointer arrives, before the card's tooltip can cover it.
        let click = |app: &mut App| for (events, pressed) in [(vec![egui::Event::PointerMoved(card)], true), (Vec::new(), false)] {
            let button = egui::Event::PointerButton { pos: card, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            frame(app, &ctx, events.into_iter().chain([button]).collect(), false);
        };
        let started = Instant::now();
        click(&mut app);
        for _ in 0..2 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        assert!(started.elapsed() < Duration::from_secs(2), "frames must not wait for the read");
        let shown = |texts: &Texts, s: &str| texts.iter().any(|(t, _)| t.galley.job.text == s);
        assert!(app.db.is_none() && shown(&texts, "Opening…") && shown(&texts, "Opening Home.bgt…"), "the card and the status bar say it is opening");
        click(&mut app);
        gate.open();
        settle(&mut app, &ctx, |app| app.db.is_some());
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert!(app.toast.as_ref().is_some_and(|t| t.text == "Opened Home.bgt"));
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1, "a second click on the opening card does not read it again");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A budget saved beside the fixture with one payment, read through a gate the test opens.
    fn other_budget(app: &mut App, dir: &Path) -> (PathBuf, file_probe::tests::Gate) {
        let other = dir.join("other.bgt");
        let mut db = BudgetDb::new(&app.cur, 12, false);
        let mut p = budget_core::Payment::new(20261027); p.description = "Other payment".into(); p.amount = -5.0;
        db.add(p);
        bgt::save_file(&other, &db.to_text(None).unwrap()).unwrap();
        let (gate, probes) = file_probe::tests::Gate::reads();
        app.files = probes;
        (other, gate)
    }

    #[test]
    fn a_budget_opened_in_the_background_closes_dialogs_about_the_replaced_one() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let (other, gate) = other_budget(&mut app, &dir);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.open(&other);
        // While it is read, ask to delete a payment of the current budget.
        let id = app.db.as_ref().unwrap().payments[0].id;
        app.dialogs.confirm_delete = Some(DeleteTarget { id, date: None });
        frame(&mut app, &ctx, vec![], false);
        gate.open();
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert_eq!(app.file.as_deref(), Some(other.as_path()));
        assert!(app.dialogs.confirm_delete.is_none(), "the confirmation named a payment of the replaced budget");
        assert_eq!(app.db.as_ref().unwrap().payments.len(), 1, "nothing was deleted");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asking_again_for_the_budget_being_read_answers_the_prompt_once() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let (other, gate) = other_budget(&mut app, &dir);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.open(&other);
        let id = app.db.as_ref().unwrap().payments[0].id;
        document::change_transaction(app.db.as_mut().unwrap(), id, 20261027, -20.0, "", budget_core::FLAG_NONE).unwrap();
        app.changed();
        app.open(&other);
        assert!(matches!(&app.dialogs.pending, Some(PendingAction::Open(p)) if *p == other), "the edit is asked about");
        app.dialogs.pending = None; // Discard changes
        app.perform_pending(PendingAction::Open(other.clone()), &ctx);
        gate.open();
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert!(app.dialogs.pending.is_none(), "the answered prompt is not shown again");
        assert_eq!(app.file.as_deref(), Some(other.as_path()));
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1, "the budget is read once");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_edit_made_while_a_budget_is_read_is_kept_until_the_prompt_is_answered() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let other = dir.join("other.bgt");
        let mut db = BudgetDb::new(&app.cur, 12, false);
        let mut p = budget_core::Payment::new(20261027); p.description = "Other payment".into(); p.amount = -5.0;
        db.add(p);
        bgt::save_file(&other, &db.to_text(None).unwrap()).unwrap();
        let (gate, probes) = file_probe::tests::Gate::reads();
        app.files = probes;
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.open(&other);
        assert!(app.opening.is_some() && app.dialogs.pending.is_none(), "a clean budget opens without asking");
        let id = app.db.as_ref().unwrap().payments[0].id;
        document::change_transaction(app.db.as_mut().unwrap(), id, 20261027, -20.0, "", budget_core::FLAG_NONE).unwrap();
        app.changed();
        gate.open();
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert!(matches!(&app.dialogs.pending, Some(PendingAction::Open(p)) if *p == other), "the finished open asks first");
        assert_eq!(app.file.as_deref(), Some(dir.join("current.bgt").as_path()));
        assert!(app.dirty);
        assert_eq!(app.db.as_mut().unwrap().get_day(&Cal::from_int(20261027)).unwrap().transactions[0].amount, -20.0);
        // Discard changes: the budget opens.
        app.dialogs.pending = None;
        app.perform_pending(PendingAction::Open(other.clone()), &ctx);
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert_eq!(app.file.as_deref(), Some(other.as_path()));
        assert!(!app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_last_budget_on_a_slow_share_opens_when_its_check_returns() {
        let (mut app, dir) = fixture();
        let path = dir.join("last.bgt");
        bgt::save_file(&path, &app.db.as_ref().unwrap().to_text(None).unwrap()).unwrap();
        (app.db, app.file, app.dirty) = (None, None, false);
        let (gate, probes) = file_probe::tests::Gate::probes(FileStatus::Present(None));
        app.files = probes;
        app.auto_open = Some(path.clone());
        let ctx = gui(&app);
        for _ in 0..2 { frame(&mut app, &ctx, vec![], false); }
        assert!(app.db.is_none() && app.auto_open.is_some(), "the window stays usable while the share is checked");
        gate.open();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.db.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            frame(&mut app, &ctx, vec![], false);
        }
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert!(app.auto_open.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // ---------- review round 3: lifecycle, navigation, layout and recent files ----------

    fn shown(texts: &Texts, s: &str) -> bool { texts.iter().any(|(t, _)| t.galley.job.text == s) }

    /// The painted extent of a rendered text.
    fn ink(text: &egui::epaint::TextShape) -> egui::Rect { text.galley.mesh_bounds.translate(text.pos.to_vec2()) }

    fn window(width: f32, height: f32) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, height))), ..Default::default() }
    }

    fn secondary_click(app: &mut App, ctx: &egui::Context, pos: egui::Pos2) -> Texts {
        for (events, pressed) in [(vec![egui::Event::PointerMoved(pos)], true), (Vec::new(), false)] {
            let button = egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
            frame(app, ctx, events.into_iter().chain([button]).collect(), false);
        }
        // A new popup is laid out over its first frame and shown from the next.
        frame(app, ctx, vec![], false);
        rendered_text(&frame(app, ctx, vec![], false))
    }

    #[test]
    fn a_new_budget_is_written_empty_opens_cleanly_and_never_replaces_a_file() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        let path = dir.join("New plan.bgt");
        app.start_new_budget(path.clone());
        settle(&mut app, &ctx, |app| app.file.as_deref() == Some(path.as_path()) && app.db.is_some());
        assert!(app.db.as_ref().unwrap().payments.is_empty(), "a new budget has no payments");
        assert!(!app.dirty && app.banner.is_none());
        let written = std::fs::read(&path).unwrap();
        app.start_new_budget(path.clone());
        assert!(app.banner.as_ref().is_some_and(|b| b.text.contains("already exists")), "an existing file is never replaced");
        assert_eq!(std::fs::read(&path).unwrap(), written);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_budget_with_unsaved_edits_asks_before_anything_is_written() {
        let (mut app, dir) = fixture();
        let path = dir.join("Second.bgt");
        app.start_new_budget(path.clone());
        assert!(matches!(&app.dialogs.pending, Some(PendingAction::New(p)) if *p == path), "the unsaved changes are asked about first");
        assert!(!path.exists() && app.db.is_some() && app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn close_budget_with_unsaved_edits_asks_first_and_closes_once_discarded() {
        let (mut app, dir) = fixture();
        app.request_close_budget();
        assert!(matches!(app.dialogs.pending, Some(PendingAction::CloseBudget)), "unsaved edits are asked about");
        assert!(app.db.is_some() && app.dirty);
        let ctx = gui(&app);
        app.perform_pending(PendingAction::CloseBudget, &ctx);
        assert!(app.db.is_none() && app.file.is_none() && !app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn close_budget_without_edits_returns_to_the_empty_state() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.request_close_budget();
        assert!(app.dialogs.pending.is_none() && app.db.is_none() && app.file.is_none());
        let ctx = gui(&app);
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(shown(&texts, "Open a budget"), "the empty state is back");
        assert!(app.cfg.last_file_name.is_none(), "a closed budget is not reopened at the next start");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_file_menu_offers_new_and_close_budget() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let file = text_center(&texts, |s| s == "File").expect("the File menu");
        let texts = click_at(&mut app, &ctx, file);
        assert!(shown(&texts, "New budget…") && shown(&texts, "Close budget"), "File lists New budget and Close budget");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A budget file named `name` holding one payment, saved `versions` times so it has backups.
    fn saved_budget(dir: &Path, name: &str, versions: &[&str], password: Option<&str>) -> PathBuf {
        let path = dir.join(name);
        for version in versions {
            let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
            let mut p = budget_core::Payment::new(20261027); p.description = version.to_string(); p.amount = -12.0;
            db.add(p);
            bgt::save_file(&path, &db.to_text(password).unwrap()).unwrap();
        }
        path
    }
    /// The same file with its end-of-data line cut off.
    fn cut_short(path: &Path) -> Vec<u8> {
        let text = std::fs::read_to_string(path).unwrap();
        let cut = text[..text.rfind("## END").unwrap()].as_bytes().to_vec();
        std::fs::write(path, &cut).unwrap();
        cut
    }
    fn first_payment(app: &App) -> &str { &app.db.as_ref().unwrap().payments[0].description }

    #[test]
    fn a_budget_cut_short_is_not_opened_until_the_user_decides_and_save_cannot_touch_it() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let previous = app.file.clone();
        let path = saved_budget(&dir, "cut.bgt", &["first", "second"], None);
        let cut = cut_short(&path);
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.incomplete.is_some());
        assert_eq!(app.file, previous, "held back: the open budget stays");
        assert!(app.dialogs.incomplete.as_ref().unwrap().has_backup);
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(shown(&texts, "This budget may be incomplete") && shown(&texts, "Restore previous version…"));
        app.save();
        assert_eq!(std::fs::read(&path).unwrap(), cut);
        // Cancel leaves everything as it was.
        frame(&mut app, &ctx, vec![key(Key::Escape, Modifiers::NONE)], false);
        assert!(app.dialogs.incomplete.is_none());
        assert_eq!(app.file, previous);
        assert_eq!(std::fs::read(&path).unwrap(), cut);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_a_cut_budget_anyway_then_saving_keeps_the_file_as_it_was_in_a_backup() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "only.bgt", &["only"], None);
        let cut = cut_short(&path);
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.incomplete.is_some());
        assert!(!app.dialogs.incomplete.as_ref().unwrap().has_backup, "nothing to offer yet");
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        let texts = click_at(&mut app, &ctx, text_center(&texts, |s| s == "Open anyway").expect("Open anyway"));
        assert!(!shown(&texts, "Restore previous version…"));
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert!(!app.dirty);
        app.changed();
        assert!(app.save());
        assert_eq!(std::fs::read(dir.join("only.bgt.bak")).unwrap(), cut);
        assert!(!bgt::load_file(&path, 20261030, None).unwrap().missing_end_marker);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_budget_file_is_refused_and_offers_a_backup_only_when_there_is_one() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let previous = app.file.clone();
        let ctx = gui(&app);
        let bare = dir.join("bare.bgt");
        std::fs::write(&bare, b"").unwrap();
        app.open(&bare);
        settle(&mut app, &ctx, |app| app.banner.is_some());
        let banner = app.banner.clone().unwrap();
        assert_eq!(banner.text, "This file is empty, so it holds no budget.");
        assert_eq!(banner.offer_restore, None);

        let path = saved_budget(&dir, "emptied.bgt", &["first", "second"], None);
        std::fs::write(&path, b"").unwrap();
        app.banner = None;
        app.open(&path);
        settle(&mut app, &ctx, |app| app.banner.is_some());
        assert_eq!(app.banner.as_ref().unwrap().offer_restore.as_deref(), Some(path.as_path()));
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(shown(&texts, "Restore previous version…"));
        click_at(&mut app, &ctx, text_center(&texts, |s| s == "Restore previous version…").unwrap());
        assert!(app.dialogs.restore.is_some(), "the offer opens the list");
        assert_eq!(app.file, previous);
        assert!(app.save());
        assert_eq!(std::fs::read(&path).unwrap(), b"", "the refused file is not written");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_line_reads_as_damage_and_offers_a_backup() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "gap.bgt", &["first", "second"], None);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.lines().filter(|l| !l.contains("second")).collect::<Vec<_>>().join("\n")).unwrap();
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.banner.is_some());
        let banner = app.banner.as_ref().unwrap();
        assert_eq!(banner.text, "Part of this budget file is missing, so it may have been damaged.");
        assert!(banner.offer_restore.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_file_menu_lists_this_budgets_backups_with_date_and_size() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "home.bgt", &["first", "second", "third"], None);
        app.load(&path, None);
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        let texts_open = click_at(&mut app, &ctx, text_center(&texts, |s| s == "File").unwrap());
        let texts = click_at(&mut app, &ctx, text_center(&texts_open, |s| s == "Restore previous version…").expect("the menu item"));
        assert!(app.dialogs.restore.is_some());
        let _ = texts;
        settle(&mut app, &ctx, |app| app.dialogs.restore.as_ref().is_some_and(|d| d.listing.is_none()));
        let backups = bgt::list_backups(&path);
        assert_eq!(backups.len(), 2);
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        for backup in &backups { assert!(shown(&texts, &backup_time(backup)), "{}", backup_time(backup)); }
        assert!(shown(&texts, &file_size(backups[0].size)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_restored_version_is_an_unsaved_draft_until_saved() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "home.bgt", &["first", "second", "third"], None);
        let current = std::fs::read(&path).unwrap();
        app.load(&path, None);
        let ctx = gui(&app);
        app.restore_backup(path.clone(), dir.join("home.bgt.bak"));
        settle(&mut app, &ctx, |app| app.opening.is_none() && app.dirty);
        assert_eq!(first_payment(&app), "first");
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert_eq!(std::fs::read(&path).unwrap(), current, "nothing is written until the user saves");
        assert_eq!(app.db.as_ref().unwrap().file_name.as_deref(), Some(path.display().to_string().as_str()));
        // Leaving asks, like any unsaved change.
        app.request_close_budget();
        assert!(matches!(app.dialogs.pending, Some(PendingAction::CloseBudget)));
        app.dialogs.pending = None;
        assert!(app.save());
        assert_eq!(bgt::load_file(&path, 20261030, None).unwrap().payments[0].description, "first");
        assert!(!app.dirty);
        assert!(bgt::list_backups(&path).len() == 3, "the version it replaced is a backup too");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_over_a_budget_that_changed_on_disk_is_refused_at_save() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "home.bgt", &["first", "second"], None);
        app.load(&path, None);
        let ctx = gui(&app);
        std::fs::write(&path, b"another program's work").unwrap();
        app.restore_backup(path.clone(), dir.join("home.bgt.bak"));
        settle(&mut app, &ctx, |app| app.opening.is_none() && app.dirty);
        assert!(!app.save());
        assert_eq!(std::fs::read(&path).unwrap(), b"another program's work");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_with_unsaved_edits_asks_first_and_keeps_them_until_answered() {
        let (mut app, dir) = fixture();
        let path = saved_budget(&dir, "home.bgt", &["first", "second"], None);
        let ctx = gui(&app);
        let backup = dir.join("home.bgt.bak");
        assert!(app.dirty);
        app.restore_backup(path.clone(), backup.clone());
        assert!(matches!(&app.dialogs.pending, Some(PendingAction::Restore { backup: b, .. }) if *b == backup));
        assert!(app.opening.is_none());
        assert_eq!(first_payment(&app), "Fixture payment");
        // Discard changes: the budget is still dirty when the prompt carries out the restore.
        let action = app.dialogs.pending.clone().unwrap();
        app.perform_pending(action, &ctx);
        assert!(app.dialogs.pending.is_none(), "answered once");
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert_eq!(first_payment(&app), "first");
        assert!(app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cut_budget_can_be_replaced_by_a_backup_and_the_cut_file_is_kept() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "home.bgt", &["first", "second"], None);
        let cut = cut_short(&path);
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.incomplete.is_some());
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        click_at(&mut app, &ctx, text_center(&texts, |s| s == "Restore previous version…").unwrap());
        assert!(app.dialogs.incomplete.is_none() && app.dialogs.restore.is_some());
        app.dialogs.restore = None;
        app.restore_backup(path.clone(), dir.join("home.bgt.bak"));
        settle(&mut app, &ctx, |app| app.opening.is_none() && app.dirty);
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert_eq!(app.saved_bytes.as_deref(), Some(cut.as_slice()), "the file as it is now is the baseline");
        assert!(app.save());
        assert_eq!(std::fs::read(dir.join("home.bgt.bak.1")).unwrap(), cut);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sealed_backup_asks_for_its_password_and_stays_a_draft() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "sealed.bgt", &["first", "second"], Some("secret"));
        let current = std::fs::read(&path).unwrap();
        let ctx = gui(&app);
        app.restore_backup(path.clone(), dir.join("sealed.bgt.bak"));
        settle(&mut app, &ctx, |app| app.dialogs.password.is_some());
        assert_eq!(app.dialogs.password.as_ref().unwrap().restore_to.as_deref(), Some(path.as_path()));
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![egui::Event::Text("wrong".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        settle(&mut app, &ctx, |app| app.dialogs.password.as_ref().is_some_and(|p| !p.error.is_empty()));
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![egui::Event::Text("secret".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        settle(&mut app, &ctx, |app| app.dialogs.password.is_none() && app.opening.is_none() && app.dirty);
        assert_eq!(first_payment(&app), "first");
        assert_eq!(app.file.as_deref(), Some(path.as_path()));
        assert_eq!(app.password.as_deref(), Some("secret"), "saved, it stays sealed");
        assert_eq!(std::fs::read(&path).unwrap(), current);
        assert!(app.save());
        assert!(bgt::is_decryptable(&bgt::latin1_decode(&std::fs::read(&path).unwrap()), "secret"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A password changed since the backup: the older version takes the file's current password.
    #[test]
    fn an_older_version_of_an_open_sealed_budget_keeps_the_current_password() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let plain = saved_budget(&dir, "home.bgt", &["no password yet"], None);
        let path = saved_budget(&dir, "home.bgt", &["sealed now"], Some("secret"));
        assert_eq!(plain, path);
        let ctx = gui(&app);
        app.load(&path, Some("secret"));
        app.restore_backup(path.clone(), dir.join("home.bgt.bak"));
        settle(&mut app, &ctx, |app| app.opening.is_none() && app.dirty);
        assert_eq!(first_payment(&app), "no password yet");
        assert_eq!(app.password.as_deref(), Some("secret"));
        assert!(app.save());
        assert!(bgt::is_encrypted_text(&bgt::latin1_decode(&std::fs::read(&path).unwrap())));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Unopened and sealed, with a plain backup: the app cannot keep the seal, so it says so.
    #[test]
    fn a_plain_backup_of_a_sealed_file_warns_before_a_save_drops_the_password() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        saved_budget(&dir, "home.bgt", &["no password yet"], None);
        let path = saved_budget(&dir, "home.bgt", &["sealed now"], Some("secret"));
        let ctx = gui(&app);
        app.restore_backup(path.clone(), dir.join("home.bgt.bak"));
        settle(&mut app, &ctx, |app| app.opening.is_none() && app.dirty);
        let toast = app.toast.as_ref().unwrap();
        assert!(toast.error && toast.text.contains("no password"), "{}", toast.text);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_sealed_budget_offers_its_backups_from_the_password_prompt() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let path = saved_budget(&dir, "sealed.bgt", &["first", "second"], Some("secret"));
        let text = std::fs::read(&path).unwrap();
        std::fs::write(&path, &text[..text.len() / 2]).unwrap();
        let ctx = gui(&app);
        app.open(&path);
        settle(&mut app, &ctx, |app| app.dialogs.password.is_some());
        frame(&mut app, &ctx, vec![], false);
        frame(&mut app, &ctx, vec![egui::Event::Text("secret".into())], false);
        frame(&mut app, &ctx, vec![key(Key::Enter, Modifiers::NONE)], false);
        settle(&mut app, &ctx, |app| app.dialogs.password.as_ref().is_some_and(|p| p.has_backup));
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        click_at(&mut app, &ctx, text_center(&texts, |s| s == "Restore previous version…").expect("the offer"));
        assert!(app.dialogs.password.is_none());
        assert_eq!(app.dialogs.restore.as_ref().unwrap().budget, path);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sizes_read_as_bytes_kb_and_mb() {
        assert_eq!(file_size(0), "0 bytes");
        assert_eq!(file_size(1023), "1023 bytes");
        assert_eq!(file_size(1536), "1.5 KB");
        assert_eq!(file_size(3 * 1_048_576), "3.0 MB");
    }

    #[test]
    fn a_save_as_name_gets_bgt_unless_it_already_ends_with_it() {
        assert_eq!(with_bgt_extension(PathBuf::from("Household 2026.10")), PathBuf::from("Household 2026.10.bgt"));
        assert_eq!(with_bgt_extension(PathBuf::from("Plan.BGT")), PathBuf::from("Plan.BGT"));
        assert_eq!(with_bgt_extension(PathBuf::from("notes.txt")), PathBuf::from("notes.txt.bgt"));
        assert_eq!(with_bgt_extension(PathBuf::from("dir.v2/Plan")), PathBuf::from("dir.v2/Plan.bgt"));
    }

    #[test]
    #[cfg(windows)]
    fn a_budget_spelled_with_other_case_is_listed_once_in_the_recent_list() {
        let (mut app, dir) = fixture();
        let path = dir.join("Case.bgt");
        assert!(app.save_to(&path, None));
        // Windows names the same file in any case: the drive letter and folder case differ here.
        app.cfg.recent_file = vec![path.display().to_string().to_uppercase()];
        app.load(&path, None);
        assert_eq!(app.cfg.recent_file, vec![path.display().to_string()], "one entry for one budget");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_a_budget_the_month_controls_do_nothing_and_today_is_not_a_call_to_action() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        app.cur = Cal::from_int(20260901);
        let ctx = gui(&app);
        let accent = app.t.accent;
        let mut output = frame(&mut app, &ctx, vec![], false);
        for _ in 0..2 { output = frame(&mut app, &ctx, vec![], false); }
        let texts = rendered_text(&output);
        let today = text_center(&texts, |s| s.ends_with("Today")).expect("the Today control");
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == accent && r.rect.contains(today))),
            "Today is not the accent call to action without a budget");
        let next = text_center(&texts, |s| s == ph::CARET_RIGHT).expect("the next-month arrow");
        let title = text_center(&texts, |s| s.starts_with("September 2026")).expect("the month title");
        click_at(&mut app, &ctx, next);
        assert_eq!(app.cur.get(), 20260901, "the next-month arrow is disabled without a budget");
        let texts = click_at(&mut app, &ctx, title);
        assert!(!shown(&texts, "Year"), "the month chooser stays closed without a budget");
        click_at(&mut app, &ctx, today);
        assert_eq!(app.cur.get(), 20260901, "Today does nothing without a budget");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_active_search_shows_a_clearable_chip_and_an_accent_footer_count() {
        let (mut app, dir) = dense_fixture();
        let ctx = gui(&app);
        app.filter_text = "Bill 8".into();
        app.set_filter();
        let mut texts = Vec::new();
        for _ in 0..3 { texts = rendered_text(&frame(&mut app, &ctx, vec![], false)); }
        assert!(shown(&texts, "Showing 5 of 23 · Bill 8"), "the chip says what is shown: {:?}", texts.iter().map(|(t, _)| &t.galley.job.text).collect::<Vec<_>>());
        let (count, _) = texts.iter().find(|(t, _)| t.galley.job.text.contains("5 of 23 transactions")).expect("the footer count");
        assert_eq!(count.galley.job.sections[0].format.color, app.t.accent, "the footer count takes the accent colour");
        let clear = text_center(&texts, |s| s == "×").expect("the chip's clear button");
        click_at(&mut app, &ctx, clear);
        assert!(app.filter_text.is_empty() && app.filter.is_none(), "the chip's × clears the search");
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(!texts.iter().any(|(t, _)| t.galley.job.text.starts_with("Showing ")), "the chip is gone once cleared");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_filter_chip_leaves_the_last_week_of_the_month_on_screen_at_the_smallest_window() {
        let (mut app, dir) = dense_fixture();
        let ctx = gui(&app);
        app.filter_text = "Bill 8".into();
        app.set_filter();
        let mut output = ctx.run(window(900.0, 600.0), |ctx| app.render(ctx));
        for _ in 0..2 { output = ctx.run(window(900.0, 600.0), |ctx| app.render(ctx)); }
        let texts = rendered_text(&output);
        assert!(shown(&texts, "Showing 5 of 23 · Bill 8"), "the chip is shown");
        // October 2026 ends on the 31st in the fifth week: its day number must stay above the footer.
        let last_day = texts.iter().find(|(t, _)| t.galley.job.text == "31").map(|(t, _)| ink(t)).expect("the 31st is drawn");
        assert!(last_day.max.y <= 600.0 - 30.0, "the 31st sits above the footer: {last_day:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_first_payment_card_is_opaque_centred_and_offers_no_open_button() {
        let (mut app, dir) = fixture();
        app.db = Some(BudgetDb::new(&app.cur, 12, false));
        app.dirty = false;
        let ctx = gui(&app);
        let mut output = frame(&mut app, &ctx, vec![], false);
        for _ in 0..2 { output = frame(&mut app, &ctx, vec![], false); }
        let texts = rendered_text(&output);
        let title = text_center(&texts, |s| s == "Add your first payment").expect("the first-payment card");
        assert!(!shown(&texts, "Open a budget"), "a budget is open, so the card does not offer to open one");
        let button = text_center(&texts, |s| s.ends_with("New payment (Ctrl+N)")).expect("the card's button");
        assert!((title.x - button.x).abs() < 2.0, "the button sits centred under the card's title: {title:?} {button:?}");
        let card = output.shapes.iter().filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect.contains(title) && r.rect.width() >= 300.0 => Some((r.rect, r.fill)),
            _ => None,
        }).min_by(|a, b| a.0.area().total_cmp(&b.0.area())).expect("the card background");
        assert_eq!(card.1.a(), 255, "the card is opaque in a settled frame: {:?}", card.1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_footer_names_the_calendar_gestures_and_its_texts_never_overlap() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        for (width, hint) in [
            (1400.0, "Double-click a day to add · Drag a payment to move · Right-click for more"),
            (900.0, "Double-click a day to add · Right-click for more"),
        ] {
            let mut output = ctx.run(window(width, 600.0), |ctx| app.render(ctx));
            for _ in 0..2 { output = ctx.run(window(width, 600.0), |ctx| app.render(ctx)); }
            let texts = rendered_text(&output);
            assert!(shown(&texts, hint), "{width} px: the footer names the gestures; shown {:?}", texts.iter().map(|(t, _)| &t.galley.job.text).collect::<Vec<_>>());
            // Empty texts (spacers) paint nothing; only the visible footer texts are compared.
            let footer: Vec<(String, egui::Rect)> = texts.iter().filter(|(t, _)| !t.galley.job.text.is_empty()).map(|(t, _)| (t.galley.job.text.clone(), ink(t)))
                .filter(|(_, r)| r.min.y > 580.0).collect();
            for (i, (name, a)) in footer.iter().enumerate() {
                assert!(a.max.x <= width, "{name} is off the window at {width} px: {a:?}");
                for (other, b) in &footer[i + 1..] {
                    let overlap = a.intersect(*b);
                    assert!(!(overlap.width() > 0.5 && overlap.height() > 0.5), "{name} overlaps {other} at {width} px: {a:?} {b:?}");
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_summary_card_is_no_taller_than_its_content() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        let mut output = ctx.run(window(1600.0, 900.0), |ctx| app.render(ctx));
        for _ in 0..2 { output = ctx.run(window(1600.0, 900.0), |ctx| app.render(ctx)); }
        let title = text_center(&rendered_text(&output), |s| s == "Summary").expect("the summary card");
        let card = output.shapes.iter().filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect.contains(title) && r.rect.width() > 200.0 => Some(r.rect),
            _ => None,
        }).min_by(|a, b| a.area().total_cmp(&b.area())).expect("the card background");
        assert!(card.height() < 600.0, "the card wraps its content instead of filling the 900 px panel: {card:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_empty_header_shows_no_tagline_in_the_file_name_slot() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        let ctx = gui(&app);
        let texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
        assert!(!texts.iter().any(|(t, _)| t.galley.job.text.contains("one day at a time")), "no tagline floats in the header");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn summary_tiles_say_the_calendar_shows_whole_dollars_while_cents_are_hidden() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.cfg.show_decimals_in_cv = false;
        let ctx = gui(&app);
        ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let note = "Calendar shows whole dollars; View > Show cents";
        let mut texts = Vec::new();
        for time in [0.0, 0.1, 0.2] {
            texts = rendered_text(&ctx.run(egui::RawInput { time: Some(time), ..window(1280.0, 840.0) }, |ctx| app.render(ctx)));
        }
        let closing = text_center(&texts, |s| s == "Closing balance").expect("the closing balance tile");
        for time in [1.0, 1.1, 1.2] {
            texts = rendered_text(&ctx.run(egui::RawInput { time: Some(time), events: vec![egui::Event::PointerMoved(closing)], ..window(1280.0, 840.0) }, |ctx| app.render(ctx)));
        }
        assert!(texts.iter().any(|(t, _)| t.galley.job.text.contains(note)), "the tile's tooltip explains the whole dollars");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A fixture with history: a pay since January 2025 and two checkpoints, viewing October 2026.
    fn reconciled(app: &mut App) {
        let db = app.db.as_mut().unwrap();
        let mut pay = budget_core::Payment::new(20250115);
        pay.description = "Pay".into();
        pay.amount = 1000.0;
        pay.set_end_mode(budget_core::END_MODE_INDEFINITE);
        db.add(pay);
        db.adjustments.set_custom_amount(20260620, 2500.0);
        db.adjustments.set_custom_amount(20260101, 7000.0);
        db.rebuild_db(&app.cur, 12, false);
        app.dirty = false;
    }

    #[test]
    fn paging_back_to_the_first_loaded_month_shows_real_balances() {
        let (mut app, dir) = fixture();
        reconciled(&mut app);
        assert_eq!((app.cur.get(), app.db.as_ref().unwrap().start_date.get()), (20261001, 20260601));
        let ctx = gui(&app);
        // PgUp to June, the month the window starts in.
        for _ in 0..4 { app.go(-1); }
        assert_eq!(app.cur.get(), 20260601);
        frame(&mut app, &ctx, vec![], false);
        let q = app.view.as_ref().unwrap().quick.clone();
        assert!(q.opening > 7000.0, "June opens with May's closing balance, not 0: {}", q.opening);
        assert!(q.income < 2000.0, "the bank balance is not income: {}", q.income);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn balance_corrections_are_explained_and_the_summary_shows_at_1280_wide() {
        let (mut app, dir) = fixture();
        reconciled(&mut app);
        app.go(-4);
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for time in [0.0, 0.1, 0.2] {
            texts = rendered_text(&ctx.run(egui::RawInput { time: Some(time), ..window(1280.0, 800.0) }, |ctx| app.render(ctx)));
        }
        let shown = |texts: &[(egui::epaint::TextShape, egui::Rect)], text: &str| texts.iter().any(|(t, _)| t.galley.job.text.contains(text));
        assert!(shown(&texts, "Summary") && shown(&texts, "Disposable income"), "the summary panel fits at 1280");
        assert!(shown(&texts, "outside Money in and Money out"), "the tiles say why they do not add up");
        // Narrower than the panel, the line under the tiles still explains it.
        for time in [1.0, 1.1, 1.2] {
            texts = rendered_text(&ctx.run(egui::RawInput { time: Some(time), ..window(1100.0, 800.0) }, |ctx| app.render(ctx)));
        }
        assert!(!shown(&texts, "Disposable income"));
        assert!(shown(&texts, "Balance corrections"), "the correction is named at any width");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn month_keys_act_on_the_calendar_only_while_the_calendar_page_shows() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        // Two months away from today (20261027), so Home and T have somewhere to go.
        app.go(2);
        assert_eq!(app.cur.get(), 20261201);
        app.show_payments = true;
        for (name, key_event) in [
            ("PageDown", key(Key::PageDown, Modifiers::NONE)), ("PageUp", key(Key::PageUp, Modifiers::NONE)),
            ("Home", key(Key::Home, Modifiers::NONE)), ("T", key(Key::T, Modifiers::NONE)),
            ("Alt+Right", key(Key::ArrowRight, Modifiers::ALT)), ("Alt+Left", key(Key::ArrowLeft, Modifiers::ALT)),
        ] {
            frame(&mut app, &ctx, vec![key_event], false);
            assert_eq!(app.cur.get(), 20261201, "{name} must not move the calendar that is hidden behind the Payments page");
        }
        app.show_payments = false;
        frame(&mut app, &ctx, vec![key(Key::PageDown, Modifiers::NONE)], false);
        assert_eq!(app.cur.get(), 20270101, "the calendar page still pages with PageDown");
        frame(&mut app, &ctx, vec![key(Key::Home, Modifiers::NONE)], false);
        assert_eq!(app.cur.get(), 20261001, "Home jumps to the current month on the calendar page");
        app.go(3);
        frame(&mut app, &ctx, vec![key(Key::T, Modifiers::NONE)], false);
        assert_eq!(app.cur.get(), 20261001, "T jumps to the current month on the calendar page");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn month_keys_work_while_a_button_has_focus_but_not_while_typing_in_a_search_box() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        for _ in 0..3 { frame(&mut app, &ctx, vec![], false); }
        // Tab lands on a header control: the keys must still page the calendar.
        frame(&mut app, &ctx, vec![key(Key::Tab, Modifiers::NONE)], false);
        frame(&mut app, &ctx, vec![], false);
        assert!(ctx.memory(|m| m.focused()).is_some(), "Tab focuses a control");
        frame(&mut app, &ctx, vec![key(Key::PageDown, Modifiers::NONE)], false);
        assert_eq!(app.cur.get(), 20261101, "PageDown pages the calendar while a button has focus");
        // In the search box they are typing, not navigation.
        app.focus_filter = true;
        for _ in 0..3 { frame(&mut app, &ctx, vec![], false); }
        frame(&mut app, &ctx, vec![key(Key::PageDown, Modifiers::NONE)], false);
        assert_eq!(app.cur.get(), 20261101, "PageDown does not page while the search box has focus");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn month_keys_do_nothing_without_a_budget() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.go(2); // away from today, so Home and T would have somewhere to go
        let before = app.cur.get();
        assert_ne!(before, 20261001);
        for key_event in [key(Key::PageDown, Modifiers::NONE), key(Key::PageUp, Modifiers::NONE), key(Key::ArrowRight, Modifiers::ALT), key(Key::Home, Modifiers::NONE)] {
            frame(&mut app, &ctx, vec![key_event], false);
            assert_eq!(app.cur.get(), before);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn month_navigation_stops_at_the_first_and_last_supported_year() {
        let (mut app, dir) = fixture();
        app.cur = Cal::from_int(99991201);
        app.go(1);
        assert_eq!(app.cur.get(), 99991201, "no month after December 9999");
        app.go(-1);
        assert_eq!(app.cur.get(), 99991101);
        app.cur = Cal::from_int(10000101);
        app.go(-1);
        assert_eq!(app.cur.get(), 10000101, "no month before January 1000");
        app.go(12);
        assert_eq!(app.cur.get(), 10010101);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_unsaved_prompt_offers_a_copy_only_after_a_failed_save_not_for_unrelated_banners() {
        let (mut app, dir) = fixture();
        app.dirty = true;
        app.fail("Line 23: Invalid data for 'frequency'");
        app.dialogs.pending = Some(PendingAction::Close);
        let ctx = gui(&app);
        let mut output = frame(&mut app, &ctx, vec![], false);
        for _ in 0..3 { output = frame(&mut app, &ctx, vec![], false); }
        let texts = rendered_text(&output);
        assert!(shown(&texts, "Discard changes"), "the prompt is on screen");
        assert!(!shown(&texts, "Save a separate copy\u{2026}"), "a load error is not a failed save");
        app.dialogs.save_error = Some("Unable to write the budget".into());
        let mut output = frame(&mut app, &ctx, vec![], false);
        for _ in 0..3 { output = frame(&mut app, &ctx, vec![], false); }
        let texts = rendered_text(&output);
        assert!(shown(&texts, "Save a separate copy\u{2026}"), "{:?}", texts.iter().map(|(t, _)| t.galley.job.text.clone()).collect::<Vec<_>>());
        assert!(texts.iter().any(|(t, _)| t.galley.job.text.contains("Unable to write the budget")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closing_the_budget_cancels_an_open_still_being_read() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let (other, _gate) = other_budget(&mut app, &dir);
        app.open(&other);
        assert!(app.opening.is_some(), "the read is in flight");
        app.close_budget();
        assert!(app.opening.is_none() && app.db.is_none(), "the close wins");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn losing_the_window_focus_cancels_a_drag() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        egui::DragAndDrop::set_payload(&ctx, calendar::DraggedTransaction { id: 1, date: 20261027, amount: -12.0 });
        frame(&mut app, &ctx, vec![], false);
        assert!(egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(&ctx), "a drag survives ordinary frames");
        let blurred = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1280.0, 840.0))), focused: false, ..Default::default() };
        let _ = ctx.run(blurred, |ctx| app.render(ctx));
        assert!(!egui::DragAndDrop::has_payload_of_type::<calendar::DraggedTransaction>(&ctx), "focus loss cancels it");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn applying_an_untouched_occurrence_changes_nothing_and_leaves_the_budget_clean() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let id = app.db.as_ref().unwrap().payments[0].id;
        let flag = app.db.as_ref().unwrap().payments[0].get_flag(20261027);
        let before = app.db.as_ref().unwrap().to_text(None).unwrap();
        for (note, scope) in [("", TransactionScope::Occurrence), ("   ", TransactionScope::Occurrence), ("", TransactionScope::Future)] {
            app.edit_transaction(id, 20261027, -12.0);
            app.apply_transaction_edit(id, 20261027, -12.0, note, flag, scope);
            assert!(app.dialogs.transaction_editor.is_none(), "the editor closes");
            assert!(!app.dirty, "an untouched apply leaves the budget clean (note {note:?}, {scope:?})");
            assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before, "and writes no status entry");
        }
        // A real change still applies.
        app.edit_transaction(id, 20261027, -12.0);
        app.apply_transaction_edit(id, 20261027, -13.0, "", flag, TransactionScope::Occurrence);
        assert!(app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_save_records_why_and_a_good_one_forgets_it() {
        let (mut app, dir) = fixture();
        let missing = dir.join("no-such-folder").join("x.bgt");
        assert!(!app.save_to(&missing, None));
        assert!(app.dialogs.save_error.is_some() && app.banner.is_some(), "the failure is kept for the prompt");
        app.dirty = true;
        assert!(app.save_to(&dir.join("saved.bgt"), None));
        assert!(app.dialogs.save_error.is_none() && app.banner.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closing_the_budget_on_the_payments_page_returns_to_the_calendar_toolbar() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.show_payments = true;
        app.payment_list.focus_search = true;
        app.close_budget();
        assert!(!app.show_payments && !app.payment_list.focus_search);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_save_as_on_a_clean_budget_is_forgotten_by_the_next_edit_but_not_by_more_edits() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.dialogs.save_error = Some("Unable to write the budget".into());
        app.changed();
        assert!(app.dialogs.save_error.is_none(), "the edit starts a new unsaved state");
        // A failed Ctrl+S on a dirty budget still stands after further edits.
        app.dialogs.save_error = Some("Unable to write the budget".into());
        app.changed();
        assert!(app.dialogs.save_error.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn today_must_be_a_real_calendar_day() {
        for bad in ["--today=20260231", "--today=202610", "--today=5", "--today=abc"] {
            assert_eq!(parse_cli_from(vec![bad.to_string()]).today, None, "{bad}");
        }
        assert_eq!(parse_cli_from(vec!["--today=20240229".to_string()]).today, Some(20240229));
    }

    #[test]
    fn a_background_open_finished_while_a_close_prompt_waits_is_kept_until_the_prompt_is_answered() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let (other, gate) = other_budget(&mut app, &dir);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.open(&other);
        app.dialogs.pending = Some(PendingAction::Close);
        gate.open();
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(5));
            frame(&mut app, &ctx, vec![], false);
        }
        assert!(app.opening.is_some(), "the finished open is kept while the prompt waits, not dropped");
        assert!(matches!(app.dialogs.pending, Some(PendingAction::Close)));
        assert_eq!(app.file.as_deref(), Some(dir.join("current.bgt").as_path()));
        // Cancelling the close prompt lets the open this user asked for proceed.
        app.dialogs.pending = None;
        settle(&mut app, &ctx, |app| app.file.as_deref() == Some(other.as_path()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_during_a_background_open_does_not_ask_again_when_the_open_finishes() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        let (other, gate) = other_budget(&mut app, &dir);
        let ctx = gui(&app);
        frame(&mut app, &ctx, vec![], false);
        app.open(&other);
        let id = app.db.as_ref().unwrap().payments[0].id;
        document::change_transaction(app.db.as_mut().unwrap(), id, 20261027, -20.0, "", budget_core::FLAG_NONE).unwrap();
        app.changed();
        assert!(app.save(), "the edited budget saves while the other one is read");
        gate.open();
        settle(&mut app, &ctx, |app| app.opening.is_none());
        assert!(app.dialogs.pending.is_none(), "the saved edit is not asked about again");
        assert_eq!(app.file.as_deref(), Some(other.as_path()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recent_budget_whose_file_is_gone_can_be_removed_from_the_list() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        let missing = dir.join("Gone.bgt");
        app.cfg.recent_file = vec![missing.display().to_string()];
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..200 {
            texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
            if shown(&texts, "File not found") { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(shown(&texts, "File not found"), "the missing budget is reported");
        let card = text_center(&texts, |s| s == "Gone.bgt").expect("the recent card");
        let texts = secondary_click(&mut app, &ctx, card);
        let remove = text_center(&texts, |s| s == "Remove from recent list").expect("the card's menu offers removal");
        click_at(&mut app, &ctx, remove);
        assert!(app.cfg.recent_file.is_empty(), "the entry is removed from the recent list");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_recent_budget_that_reappears_is_checked_again_from_its_menu() {
        let (mut app, dir) = fixture();
        (app.db, app.file, app.dirty) = (None, None, false);
        let back = dir.join("Back.bgt");
        app.cfg.recent_file = vec![back.display().to_string()];
        let ctx = gui(&app);
        let mut texts = Vec::new();
        for _ in 0..200 {
            texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
            if shown(&texts, "File not found") { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(&back, b"restored").unwrap();
        let card = text_center(&texts, |s| s == "Back.bgt").expect("the recent card");
        let texts = secondary_click(&mut app, &ctx, card);
        let again = text_center(&texts, |s| s == "Check again").expect("the card's menu offers a new check");
        click_at(&mut app, &ctx, again);
        let mut texts = Vec::new();
        for _ in 0..200 {
            texts = rendered_text(&frame(&mut app, &ctx, vec![], false));
            if texts.iter().any(|(t, _)| t.galley.job.text.starts_with("Modified ")) { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(texts.iter().any(|(t, _)| t.galley.job.text.starts_with("Modified ")), "the card now shows the file it found");
        assert!(!shown(&texts, "File not found"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    // ---------- performance harness (ignored; run with `--release -- --ignored perf --nocapture`) ----------

    /// A generated 3000-payment budget: monthly, weekly, fortnightly and yearly payments started
    /// 2019–2026, a third of them ending, every fifth with a dated amount, note and flag, plus a
    /// balance adjustment.
    pub(crate) fn perf_db(cur: &Cal) -> BudgetDb {
        const KINDS: [&str; 8] = ["Salary", "Groceries", "Gas", "Netflix", "Hydro", "Rent", "Visa", "Insurance"];
        const CATS: [&str; 19] = ["Loans", "Phone", "Utilities", "Auto", "Housing", "Pets", "Internet", "Clothes", "Gifts",
            "Food", "Tax", "Fun", "School", "Misc", "Travel", "Health", "Savings", "Kids", "Subscriptions"];
        let mut seed: u64 = 42;
        let mut next = move |n: u32| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) as u32) % n
        };
        let mut db = BudgetDb::new(cur, 12, false);
        db.adjustments.set_custom_amount(20260101, 5000.0);
        for n in 0..3000 {
            let mut p = budget_core::Payment::new(20261027);
            p.id = n + 1;
            // Numbered out of order, so the list is not already sorted by name.
            p.description = format!("Payment {:04} {}", (n * 1847) % 3000, KINDS[next(8) as usize]);
            p.category = Some(CATS[next(19) as usize].to_string());
            p.amount = next(4000) as f64 - 2400.0;
            p.start_date = (2019 + next(8) as i32) * 10000 + (1 + next(12) as i32) * 100 + 1 + next(28) as i32;
            let (freq, every) = match next(10) { 0..=4 => ('m', 1), 5 | 6 => ('w', 2), 7 | 8 => ('w', 1), _ => ('y', 1) };
            (p.freq_type, p.frequency) = (freq, every);
            if next(3) == 0 {
                p.set_end_mode(budget_core::END_MODE_DATE);
                p.end_date = p.start_date + 20000;
            } else {
                p.set_end_mode(budget_core::END_MODE_INDEFINITE);
            }
            if n % 5 == 0 {
                p.set_custom_amount(20261015, -42.0);
                p.set_note(20261015, Some("a note"));
                p.set_flag(20261015, budget_core::FLAG_PAID);
            }
            db.payments.push(p);
        }
        db.rebuild_db(cur, 12, false);
        db
    }

    fn perf_app() -> (App, PathBuf) {
        let (mut app, dir) = fixture();
        app.db = Some(perf_db(&app.cur));
        (app.dirty, app.title_dirty) = (false, false);
        (app, dir)
    }

    fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1000.0 }

    fn report(name: &str, mut samples: Vec<f64>) {
        samples.sort_by(f64::total_cmp);
        let median = samples[samples.len() / 2];
        println!("PERF {name}: median {median:.2} ms, max {:.2} ms, min {:.2} ms (n={})", samples[samples.len() - 1], samples[0], samples.len());
    }

    fn wide_frame(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
        ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1920.0, 1080.0))), ..Default::default() }, |ctx| app.render(ctx))
    }

    #[test]
    #[ignore]
    fn perf_typing_in_the_search_box() {
        let (mut app, dir) = perf_app();
        let ctx = gui(&app);
        for _ in 0..3 { wide_frame(&mut app, &ctx); }
        let text = "payment 01 gas";
        let (mut view, mut frames) = (Vec::new(), Vec::new());
        for round in 0..3 {
            for end in 1..=text.len() {
                app.filter_text = text[..end].to_string();
                app.set_filter();
                let started = Instant::now();
                app.ensure_view();
                if round > 0 { view.push(ms(started.elapsed())); }
                // The same keystroke rendered as a whole frame (filter parse, view, panels, grid).
                app.filter_text = text[..end].to_string();
                app.view = None;
                let started = Instant::now();
                app.set_filter();
                wide_frame(&mut app, &ctx);
                if round > 0 { frames.push(ms(started.elapsed())); }
            }
        }
        report("keystroke: month view rebuild", view);
        report("keystroke: full frame", frames);
        {
            let mut f = Filter::new("payment 01 g", app.today).unwrap();
            f.set_haystacks(Some(Arc::new(Haystacks::new(&app.db.as_ref().unwrap().payments))));
            let db = app.db.as_mut().unwrap();
            let mut n = 0;
            let started = Instant::now();
            for date in 20261001..=20261031 {
                let trans = db.get_day(&Cal::from_int(date)).unwrap().transactions.clone();
                for t in trans { n += 1; std::hint::black_box(f.is_match(&db.payments[t.pmt], t.amount, date)); }
            }
            println!("PERF {n} is_match: {:.2} ms", ms(started.elapsed()));
            let started = Instant::now();
            std::hint::black_box(budget_core::stats::calc_statistics(db, &app.cur, 1, false, Some(&f)));
            println!("PERF filtered stats: {:.2} ms", ms(started.elapsed()));
            let started = Instant::now();
            std::hint::black_box(budget_core::stats::calc_statistics(db, &app.cur, 1, false, None));
            println!("PERF unfiltered stats: {:.2} ms", ms(started.elapsed()));
        }
        let mut idle = Vec::new();
        for _ in 0..20 { let started = Instant::now(); wide_frame(&mut app, &ctx); idle.push(ms(started.elapsed())); }
        report("idle frame with a search", idle);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore]
    fn perf_editing_a_transaction() {
        let (mut app, dir) = perf_app();
        let ctx = gui(&app);
        for _ in 0..2 { wide_frame(&mut app, &ctx); }
        let (mut fast, mut slow) = (Vec::new(), Vec::new());
        for k in 0..32 {
            let db = app.db.as_mut().unwrap();
            // A payment shown this month: edit that occurrence's amount.
            let (pmt, date, amount) = (20261001..=20261028).find_map(|date| {
                db.get_day(&Cal::from_int(date)).and_then(|d| d.transactions.get(k % d.transactions.len().max(1)).map(|t| (t.pmt, date, t.amount)))
            }).unwrap();
            let id = db.payments[pmt].id;
            document::change_transaction(db, id, date, amount - 1.0, "", budget_core::FLAG_NONE).unwrap();
            let started = Instant::now();
            let rebuilt = app.changed();
            let changed_ms = ms(started.elapsed());
            app.ensure_view();
            let total_ms = ms(started.elapsed());
            if k > 1 {
                (if rebuilt { &mut slow } else { &mut fast }).push((changed_ms, total_ms));
            }
        }
        // Every fifth generated payment has an equal -42.00 dated amount on 15 October, and a few
        // others share amounts by chance: editing those still rebuilds, to keep their order.
        for (name, samples) in [("updated in place", fast), ("rebuilt (equal amounts on a day)", slow)] {
            if samples.is_empty() { println!("PERF edit {name}: none"); continue; }
            report(&format!("edit {name}: App::changed"), samples.iter().map(|s| s.0).collect());
            report(&format!("edit {name}: App::changed + month view"), samples.iter().map(|s| s.1).collect());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore]
    fn perf_payment_list_frames() {
        let (mut app, dir) = perf_app();
        app.show_payments = true;
        let ctx = gui(&app);
        for _ in 0..3 { wide_frame(&mut app, &ctx); }
        let rows = rendered_text(&wide_frame(&mut app, &ctx)).iter().filter(|(t, _)| t.galley.job.text.starts_with("Payment ")).count();
        assert!(rows >= 10, "the list shows its rows ({rows})");
        let mut idle = Vec::new();
        for _ in 0..20 { let started = Instant::now(); wide_frame(&mut app, &ctx); idle.push(ms(started.elapsed())); }
        report("payment list: idle frame", idle);
        let mut typing = Vec::new();
        for (round, end) in (0..2).flat_map(|r| (1..=7).map(move |e| (r, e))) {
            app.payment_list.search = "payment"[..end].to_string();
            let started = Instant::now();
            wide_frame(&mut app, &ctx);
            if round > 0 { typing.push(ms(started.elapsed())); }
        }
        report("payment list: search keystroke frame", typing);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
