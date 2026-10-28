//! Reports menu and range dialog (analysis/06 §3.5, §6–§11).
//!
//! The generators live in `budget_core::report`. This module picks the months, writes the result to
//! a temporary `bgt-report-*.html` (in the system temp folder, never next to the budget) and hands it
//! to Windows: the default browser, or for the two Excel variants the program registered for `.xls`
//! (else `.xlsx`). The original also wrote HTML for those and ran its `.xls` program on it (06 §1.12).
//! The files this run wrote are deleted when the app exits, as the original's were.
//!
//! The dialog state lives in egui memory rather than in `App`, so the main window only needs the
//! menu call, the dialog call and a keyboard guard.

use crate::editor::{dialog_escape, dialog_footer, field, form_error, form_section};
use crate::names::MONTHS_LONG;
use crate::theme::{self, ts, Tokens, CAPTION};
use budget_core::config::BudgetConfig;
use budget_core::report::{self, CalendarOptions, ReportColors, ReportMode};
use budget_core::stats::calc_statistics;
use budget_core::{BudgetDb, Cal, Field};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

/// The File menu's "&Print this month" (06 §3.5): the printed calendar of the month on screen.
pub const PRINT_THIS_MONTH: &str = "Print this month";
const NO_BUDGET: &str = "Open a budget to create reports";
const NO_SPREADSHEET: &str = "Needs a program that opens .xls files, such as Excel.";

/// The reports of the original "Income vs. Expenses" window's Actions menu (06 §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind { Print, ExcelReport, Comparative, MonthByMonth, MonthlyCalendar, MonthlyDetailed, MonthlyExcel, UnpaidPaid }

/// Where a finished report opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer { Browser, Spreadsheet }

/// Menu groups: the category tables first, then the transaction listings.
const GROUPS: [(&str, [ReportKind; 4]); 2] = [
    ("Income vs. Expenses", [ReportKind::Print, ReportKind::ExcelReport, ReportKind::Comparative, ReportKind::MonthByMonth]),
    ("Transactions", [ReportKind::MonthlyCalendar, ReportKind::MonthlyDetailed, ReportKind::MonthlyExcel, ReportKind::UnpaidPaid]),
];

impl ReportKind {
    /// The original's menu names.
    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Print => "Print…",
            Self::ExcelReport => "Excel Report…",
            Self::Comparative => "Comparative report to…",
            Self::MonthByMonth => "Month-by-Month report…",
            Self::MonthlyCalendar => "Print Monthly Calendar…",
            Self::MonthlyDetailed => "Monthly Detailed Report…",
            Self::MonthlyExcel => "Monthly Excel Report…",
            Self::UnpaidPaid => "Unpaid & paid Report…",
        }
    }
    fn title(self) -> &'static str {
        match self { Self::Comparative => "Comparative report", _ => self.menu_label().trim_end_matches('…') }
    }
    fn group(self) -> &'static str {
        GROUPS.iter().find(|(_, kinds)| kinds.contains(&self)).map_or("", |(group, _)| *group)
    }
    fn description(self) -> &'static str {
        match self {
            Self::Print => "Income and expenses by category, with shares and differences from plan.",
            Self::ExcelReport => "The income and expenses table with spreadsheet formulas.",
            Self::Comparative => "Two ranges of the same length side by side, with the differences.",
            Self::MonthByMonth => "Every category month by month, with totals and monthly averages.",
            Self::MonthlyCalendar => "A printable calendar for each month, with daily balances.",
            Self::MonthlyDetailed => "Every transaction with running balances and monthly totals.",
            Self::MonthlyExcel => "Every transaction with running balances, for a spreadsheet.",
            Self::UnpaidPaid => "Transactions marked Paid, then the unpaid ones, with subtotals.",
        }
    }
    pub fn viewer(self) -> Viewer {
        match self { Self::ExcelReport | Self::MonthlyExcel => Viewer::Spreadsheet, _ => Viewer::Browser }
    }
}

/// One report to produce. Months are day-1 calendars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub kind: ReportKind,
    pub from: Cal,
    pub to: Cal,
    /// Comparative report only: the first month of the range compared with the base range;
    /// it spans as many months as From–To (06 §6).
    pub other: Cal,
}

/// What reports need from the operating system; tests substitute one that launches nothing.
pub trait Shell {
    /// Folder for the report files.
    fn report_dir(&self) -> PathBuf;
    /// Whether a program is registered for `.xls` or `.xlsx` (the original disabled its Excel items otherwise).
    fn spreadsheet_available(&self) -> bool;
    fn open(&self, path: &Path, viewer: Viewer) -> Result<(), String>;
    /// A report file was written: remember it, to delete it when the app exits.
    fn created(&self, path: &Path);
}

/// Report files written by this run. The original's temporary reports were deleted when it exited
/// (`File.deleteOnExit`); only these exact files are, never anything else in the folder.
#[derive(Default)]
pub struct TempReports(std::sync::Mutex<Vec<PathBuf>>);

impl TempReports {
    const fn new() -> TempReports { TempReports(std::sync::Mutex::new(Vec::new())) }

    fn track(&self, path: &Path) { self.0.lock().unwrap_or_else(|e| e.into_inner()).push(path.to_path_buf()); }

    /// Deletes the tracked files (one already gone is skipped) and forgets them. Returns how many
    /// were deleted.
    fn delete_all(&self) -> usize {
        let paths = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        paths.iter().filter(|path| std::fs::remove_file(path).is_ok()).count()
    }
}

/// The reports this run wrote to the system's temporary folder.
static CREATED: TempReports = TempReports::new();

/// Deletes the report files this run wrote (when the app exits).
pub fn delete_created_reports() { CREATED.delete_all(); }

/// The real system: temp folder, default browser, `.xls` program.
struct System;

impl Shell for System {
    fn report_dir(&self) -> PathBuf { std::env::temp_dir() }
    fn spreadsheet_available(&self) -> bool { spreadsheet_class().is_some() }
    fn open(&self, path: &Path, viewer: Viewer) -> Result<(), String> {
        match viewer {
            Viewer::Browser => platform::open(path, None),
            Viewer::Spreadsheet => match spreadsheet_class() {
                Some(class) => platform::open(path, Some(class)),
                None => Err(NO_SPREADSHEET.into()),
            },
        }
    }
    fn created(&self, path: &Path) { CREATED.track(path); }
}

/// `.xls`, else `.xlsx`, whichever has a registered program (`Program.findProgram`), looked up once.
fn spreadsheet_class() -> Option<&'static str> {
    static CLASS: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *CLASS.get_or_init(|| [".xls", ".xlsx"].into_iter().find(|ext| platform::has_handler(ext)))
}

#[cfg(windows)]
mod platform {
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    /// `SHELLEXECUTEINFOW`.
    #[repr(C)]
    struct ShellExecuteInfo {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut c_void,
        verb: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
        inst_app: *mut c_void,
        id_list: *mut c_void,
        class: *const u16,
        hkey_class: *mut c_void,
        hot_key: u32,
        icon_or_monitor: *mut c_void,
        process: *mut c_void,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32;
    }
    #[link(name = "shlwapi")]
    extern "system" {
        fn AssocQueryStringW(flags: u32, kind: u32, assoc: *const u16, extra: *const u16, out: *mut u16, out_len: *mut u32) -> i32;
    }

    const SEE_MASK_CLASSNAME: u32 = 0x1;
    /// Report failures to the caller (the app's banner) instead of a Windows dialog.
    const SEE_MASK_FLAG_NO_UI: u32 = 0x400;
    const SW_SHOWNORMAL: i32 = 1;
    const ASSOCF_NOTRUNCATE: u32 = 0x20;
    /// Fail for an unregistered type instead of answering with the "Open with" picker.
    const ASSOCF_INIT_IGNOREUNKNOWN: u32 = 0x400;
    const ASSOCSTR_EXECUTABLE: u32 = 2;

    fn wide(s: &OsStr) -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() }

    #[cfg(test)]
    pub const INFO_SIZE: usize = std::mem::size_of::<ShellExecuteInfo>();

    pub fn has_handler(ext: &str) -> bool {
        let assoc = wide(OsStr::new(ext));
        let mut out = [0u16; 1024];
        let mut len = out.len() as u32;
        // SAFETY: `assoc` is NUL-terminated and `out`/`len` describe a writable buffer of `len` units.
        let hr = unsafe { AssocQueryStringW(ASSOCF_NOTRUNCATE | ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_EXECUTABLE, assoc.as_ptr(), null(), out.as_mut_ptr(), &mut len) };
        hr == 0 && len > 1
    }

    /// Opens `path` with its default program, or with the program of file class `class` (e.g. `.xls`).
    /// No command line is built, so spaces, `&` or `%` in the path need no quoting.
    pub fn open(path: &Path, class: Option<&str>) -> Result<(), String> {
        let file = wide(path.as_os_str());
        let class = class.map(|c| wide(OsStr::new(c)));
        let mut info = ShellExecuteInfo {
            cb_size: std::mem::size_of::<ShellExecuteInfo>() as u32,
            f_mask: SEE_MASK_FLAG_NO_UI | if class.is_some() { SEE_MASK_CLASSNAME } else { 0 },
            hwnd: null_mut(), verb: null(), file: file.as_ptr(), parameters: null(), directory: null(), show: SW_SHOWNORMAL,
            inst_app: null_mut(), id_list: null_mut(), class: class.as_ref().map_or(null(), |c| c.as_ptr()),
            hkey_class: null_mut(), hot_key: 0, icon_or_monitor: null_mut(), process: null_mut(),
        };
        // SAFETY: `info` is a correctly sized SHELLEXECUTEINFOW whose strings outlive the call.
        if unsafe { ShellExecuteExW(&mut info) } != 0 { Ok(()) } else { Err(std::io::Error::last_os_error().to_string()) }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;
    pub fn has_handler(_ext: &str) -> bool { false }
    pub fn open(path: &Path, _class: Option<&str>) -> Result<(), String> {
        let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(program).arg(path).spawn().map(drop).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Months and validation
// ---------------------------------------------------------------------------------------------

fn first_of(c: &Cal) -> Cal { let mut m = *c; m.set_day_one(); m }
fn shift(c: Cal, months: i32) -> Cal { let mut m = c; m.add(Field::Month, months); m }
fn clamp(c: Cal, min: Cal, max: Cal) -> Cal { if c.ym() < min.ym() { min } else if c.ym() > max.ym() { max } else { c } }
fn month_name(c: &Cal) -> String { format!("{} {}", MONTHS_LONG[c.month() as usize], c.year()) }
fn long_date(c: &Cal) -> String { format!("{} {} {}", c.day(), MONTHS_LONG[c.month() as usize], c.year()) }
/// `March 2026`, or `March 2026 to May 2026 (3 months)`.
fn span(a: &Cal, b: &Cal) -> String {
    let n = Cal::months_diff(a, b) + 1;
    if n <= 1 { month_name(a) } else { format!("{} to {} ({n} months)", month_name(a), month_name(b)) }
}

/// The months the open budget has a schedule for (the original's `minDate`..`maxDate`).
pub fn months_available(db: &BudgetDb) -> (Cal, Cal) { (first_of(&db.start_date), first_of(&db.end_date)) }

/// The original's range rules (06 §3.5, §6, §7, §11), with a reason a person can act on.
pub fn validate(req: &Request, min: Cal, max: Cal) -> Result<(), String> {
    if req.from.ym() > req.to.ym() {
        return Err(format!("From ({}) is after To ({}). Choose a From month on or before the To month.", month_name(&req.from), month_name(&req.to)));
    }
    if req.from.ym() < min.ym() || req.to.ym() > max.ym() {
        return Err(format!("Choose months from {} to {}; this budget has no schedule outside that period.", month_name(&min), month_name(&max)));
    }
    if req.kind == ReportKind::MonthByMonth && req.from == req.to {
        return Err("A Month-by-Month report needs at least two months. Choose a later To month.".into());
    }
    if req.kind == ReportKind::Comparative {
        let d = Cal::months_diff(&req.from, &req.to);
        let end = shift(req.other, d);
        if req.other.ym() < min.ym() || end.ym() > max.ym() {
            return Err(format!("The comparison ({}) must fit between {} and {}.", span(&req.other, &end), month_name(&min), month_name(&max)));
        }
        // The original offers no start month whose window would overlap the base range.
        if (shift(req.from, -d).ym()..=req.to.ym()).contains(&req.other.ym()) {
            return Err(format!("The comparison ({}) overlaps {}. Choose months that end before {} or start after {}.",
                span(&req.other, &end), span(&req.from, &req.to), month_name(&req.from), month_name(&req.to)));
        }
    }
    Ok(())
}

/// What the report will actually contain, spelled out so a range is never a surprise.
pub fn covers(req: &Request) -> Option<String> {
    if req.from.ym() > req.to.ym() { return None; }
    Some(match req.kind {
        ReportKind::UnpaidPaid => {
            let mut end = req.to;
            end.set_day_last();
            format!("Covers {} to {}.", long_date(&req.from), long_date(&end))
        }
        ReportKind::Comparative => {
            let end = shift(req.other, Cal::months_diff(&req.from, &req.to));
            format!("Compares {} with {}.", span(&req.from, &req.to), span(&req.other, &end))
        }
        _ => format!("Covers {}.", span(&req.from, &req.to)),
    })
}

// ---------------------------------------------------------------------------------------------
// Generating and opening
// ---------------------------------------------------------------------------------------------

/// A 1×1 GIF of one colour: the stretched bar of the stats graphs (`blue_g.gif` / `red_g.gif`).
fn solid_gif(rgb: u32) -> Vec<u8> {
    let mut gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    gif.extend_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0, 0, 0]);
    gif.extend_from_slice(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b");
    gif
}

/// A transparent pixel for payment pictures: the original's `images/` folder is not part of the port.
const TRANSPARENT_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";

/// The day-note marker of the Monthly Detailed report (`icons/note.png`): a small ruled note.
fn note_icon() -> &'static str {
    static URI: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URI.get_or_init(|| {
        let (paper, edge, line) = (image::Rgba([0xff, 0xf1, 0xb8, 0xff]), image::Rgba([0xb4, 0x8a, 0x1e, 0xff]), image::Rgba([0x8a, 0x6a, 0x16, 0xff]));
        let img = image::RgbaImage::from_fn(12, 12, |x, y| {
            if !(1..=10).contains(&x) { image::Rgba([0, 0, 0, 0]) }
            else if x == 1 || x == 10 || y == 0 || y == 11 { edge }
            else if matches!(y, 3 | 6 | 9) && (3..=8).contains(&x) { line }
            else { paper }
        });
        let mut png = std::io::Cursor::new(Vec::new());
        match image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png) {
            Ok(()) => report::base64_data_uri(png.get_ref()),
            Err(_) => report::base64_data_uri(TRANSPARENT_GIF),
        }
    })
}

/// The report pictures, embedded as `data:` URIs (06 §1.9) so the temporary file is self-contained.
fn report_image(path: &str, cfg: &BudgetConfig) -> Option<String> {
    Some(match path {
        "icons/blue_g.gif" => report::base64_data_uri(&solid_gif(cfg.colors.graphBarColor)),
        "icons/red_g.gif" => report::base64_data_uri(&solid_gif(cfg.colors.graphNegativeBarColor)),
        "icons/note.png" => note_icon().to_string(),
        _ => report::base64_data_uri(TRANSPARENT_GIF),
    })
}

fn calendar_options(cfg: &BudgetConfig) -> CalendarOptions {
    CalendarOptions {
        show_decimals: cfg.show_decimals_in_cv,
        preserve_color_on_print: cfg.preserve_color_on_print,
        first_day_sunday: cfg.first_day_of_week_is_sunday,
        currency_prefix: cfg.currency_prefix.clone(),
        currency_suffix: cfg.currency_suffix.clone(),
        colors: ReportColors { bg_paid: cfg.colors.bgColorPaid, bg_important: cfg.colors.bgColorImportant },
    }
}

/// The report document. Ranges follow 06 §11: the Stats tables use `calcStatistics(From, months)`,
/// Unpaid & paid runs from the 1st of From to the last day of To, and every other report includes
/// the To month. The original's Monthly Detailed / Excel reports left the To month out unless it
/// equalled From (06 §8); here the generator is given the month after To, so they include it too.
pub fn generate(db: &mut BudgetDb, cfg: &BudgetConfig, req: &Request) -> Result<String, String> {
    let images = |path: &str| report_image(path, cfg);
    let n = Cal::months_diff(&req.from, &req.to) + 1;
    let stats = |db: &mut BudgetDb, start: &Cal| calc_statistics(db, start, n, true, None);
    Ok(match req.kind {
        ReportKind::Print => report::generate_stats_report(&stats(db, &req.from), &images),
        ReportKind::ExcelReport => report::generate_excel_stats_report(&stats(db, &req.from)),
        ReportKind::Comparative => {
            let base = stats(db, &req.from);
            let other = stats(db, &req.other);
            report::generate_comparison_report(&base, &other, &images)
        }
        ReportKind::MonthByMonth => {
            let st = stats(db, &req.from);
            report::generate_month_by_month_report(db, &st, cfg.show_decimals_in_cv)?
        }
        ReportKind::MonthlyCalendar => report::generate_monthly_calendar(db, &req.from, &req.to, &calendar_options(cfg), &images),
        ReportKind::MonthlyDetailed => report::generate_report(db, &req.from, &shift(req.to, 1), ReportMode::Html, &images),
        ReportKind::MonthlyExcel => report::generate_report(db, &req.from, &shift(req.to, 1), ReportMode::Excel, &images),
        ReportKind::UnpaidPaid => {
            let mut end = req.to;
            end.set_day_last();
            report::generate_paid_unpaid_report(db, &req.from, &end, &images)
        }
    })
}

/// `File.createTempFile("bgt-report-", ".html")`: a new, uniquely named file in `dir`.
fn write_report(dir: &Path, html: &str) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64);
    loop {
        let path = dir.join(format!("bgt-report-{}{}{}.html", std::process::id(), stamp, NEXT.fetch_add(1, Ordering::Relaxed)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(html.as_bytes()).and_then(|()| file.flush()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Generates, writes and opens one report: `Ok` is a confirmation, `Err` a message for the banner.
pub fn run(db: &mut BudgetDb, cfg: &BudgetConfig, req: &Request, shell: &dyn Shell) -> Result<String, String> {
    run_named(db, cfg, req, req.kind.title(), shell)
}

fn run_named(db: &mut BudgetDb, cfg: &BudgetConfig, req: &Request, name: &str, shell: &dyn Shell) -> Result<String, String> {
    // A generator fault must not close the app and lose unsaved edits.
    let html = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| generate(db, cfg, req)))
        .unwrap_or_else(|_| Err("an internal error stopped the report.".into()))
        .map_err(|e| format!("Couldn't create \u{201c}{name}\u{201d}: {e}"))?;
    let path = write_report(&shell.report_dir(), &html)
        .map_err(|e| format!("Couldn't save \u{201c}{name}\u{201d} in the temporary folder: {e}"))?;
    shell.created(&path);
    shell.open(&path, req.kind.viewer())
        .map_err(|e| format!("Couldn't open \u{201c}{name}\u{201d}: {e} The report was saved as {}.", path.display()))?;
    Ok(match req.kind.viewer() {
        Viewer::Browser => format!("Opened \u{201c}{name}\u{201d} in your browser"),
        Viewer::Spreadsheet => format!("Opened \u{201c}{name}\u{201d} in your spreadsheet program"),
    })
}

fn print_this_month(db: &mut BudgetDb, cfg: &BudgetConfig, cur: &Cal, shell: &dyn Shell) -> Result<String, String> {
    let month = first_of(cur);
    run_named(db, cfg, &Request { kind: ReportKind::MonthlyCalendar, from: month, to: month, other: month }, PRINT_THIS_MONTH, shell)
}

// ---------------------------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------------------------

/// The header's Reports menu, disabled while no budget is open. "Print this month" runs at once
/// (its result is returned); the other items open the range dialog shown by [`dialog`].
/// `budget` identifies the open budget (the app's load generation): the range dialog belongs to
/// it and closes if another budget replaces it, e.g. when a background open finishes.
pub fn menu(ui: &mut egui::Ui, t: &Tokens, db: Option<&mut BudgetDb>, cur: &Cal, cfg: &BudgetConfig, budget: u64) -> Option<Result<String, String>> {
    let was_open = is_open(ui.ctx());
    let outcome = menu_with(ui, t, db, cur, cfg, &System);
    if !was_open && is_open(ui.ctx()) { ui.ctx().data_mut(|d| d.insert_temp(budget_id(), budget)); }
    outcome
}

fn menu_with(ui: &mut egui::Ui, t: &Tokens, db: Option<&mut BudgetDb>, cur: &Cal, cfg: &BudgetConfig, shell: &dyn Shell) -> Option<Result<String, String>> {
    // Some(None) = Print this month, Some(Some(kind)) = a report that asks for its months.
    let mut chosen: Option<Option<ReportKind>> = None;
    let button = ui.add_enabled_ui(db.is_some(), |ui| ui.menu_button("Reports", |ui| {
        ui.set_min_width(240.0);
        if ui.button(PRINT_THIS_MONTH).on_hover_text("A printable calendar of the month on screen").clicked() {
            chosen = Some(None);
            ui.close_menu();
        }
        let spreadsheet = shell.spreadsheet_available();
        for (group, kinds) in GROUPS {
            ui.separator();
            ui.label(RichText::new(group).text_style(ts(CAPTION)).color(t.text_muted));
            for kind in kinds {
                let enabled = kind.viewer() == Viewer::Browser || spreadsheet;
                let item = ui.add_enabled(enabled, egui::Button::new(kind.menu_label()))
                    .on_hover_text(kind.description()).on_disabled_hover_text(NO_SPREADSHEET);
                if item.clicked() {
                    chosen = Some(Some(kind));
                    ui.close_menu();
                }
            }
        }
    })).inner.response;
    button.on_disabled_hover_text(NO_BUDGET);
    let db = db?;
    match chosen? {
        None => Some(print_this_month(db, cfg, cur, shell)),
        Some(kind) => {
            let (min, max) = months_available(db);
            ui.ctx().data_mut(|d| d.insert_temp(draft_id(), Draft::new(kind, cur, min, max)));
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Range dialog
// ---------------------------------------------------------------------------------------------

fn draft_id() -> egui::Id { egui::Id::new("report_range_draft") }
fn budget_id() -> egui::Id { draft_id().with("budget") }

fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| { d.remove::<Draft>(draft_id()); d.remove::<u64>(budget_id()); });
}

/// Whether the range dialog is showing (the main window then ignores its own shortcuts).
pub fn is_open(ctx: &egui::Context) -> bool { ctx.data(|d| d.get_temp::<Draft>(draft_id()).is_some()) }

#[derive(Clone, Debug)]
struct Draft {
    kind: ReportKind,
    from: Cal,
    to: Cal,
    other: Cal,
    /// The comparison start was picked by hand; until then it follows the base range.
    other_chosen: bool,
    /// The month on screen when the dialog opened.
    cur: Cal,
    min: Cal,
    max: Cal,
    error: Option<String>,
}

#[derive(Debug, PartialEq)]
enum DraftAction { None, Cancel, Create }

impl Draft {
    /// Like the original Stats window, the range starts at the month on screen (06 §3.1); a
    /// Month-by-Month report needs more than one month, so it starts with the last 12.
    fn new(kind: ReportKind, cur: &Cal, min: Cal, max: Cal) -> Draft {
        let cur = clamp(first_of(cur), min, max);
        let mut draft = Draft { kind, from: cur, to: cur, other: cur, other_chosen: false, cur, min, max, error: None };
        if kind == ReportKind::MonthByMonth { draft.cover(shift(cur, -11), cur); }
        draft.other = draft.previous_period();
        draft
    }

    fn request(&self) -> Request { Request { kind: self.kind, from: self.from, to: self.to, other: self.other } }

    /// Chooses From/To so that the report covers `first..=last`.
    fn cover(&mut self, first: Cal, last: Cal) {
        self.from = clamp(first, self.min, self.max);
        self.to = clamp(last, self.min, self.max);
    }

    /// The equally long range just before the base range, or just after it when the budget starts too late.
    fn previous_period(&self) -> Cal {
        let n = Cal::months_diff(&self.from, &self.to).max(0) + 1;
        let before = shift(self.from, -n);
        if before.ym() >= self.min.ym() { before } else { shift(self.to, 1) }
    }

    fn show(&mut self, ctx: &egui::Context, t: &Tokens) -> DraftAction {
        if dialog_escape(ctx) { return DraftAction::Cancel; }
        let mut action = DraftAction::None;
        let before = (self.from, self.to, self.other);
        egui::Modal::new(egui::Id::new("report_range")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(460.0_f32.min((ctx.screen_rect().width() - 96.0).max(260.0)));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
            ui.heading(self.kind.title());
            ui.horizontal_wrapped(|ui| {
                theme::badge(ui, self.kind.group(), t.accent, t);
                let opens = match self.kind.viewer() { Viewer::Browser => "Opens in your browser", Viewer::Spreadsheet => "Opens in your spreadsheet program" };
                ui.label(RichText::new(opens).color(t.text_muted));
            });
            note(ui, self.kind.description(), t);
            // The fields scroll in compact windows; the error and the footer stay pinned.
            let reserved = if self.error.is_some() { 325.0 } else { 232.0 };
            egui::ScrollArea::vertical().id_salt("report_range_scroll")
                .max_height((ctx.screen_rect().height() - reserved).max(140.0))
                .show(ui, |ui| self.fields(ui, t));
            if let Some(error) = &self.error { form_error(ui, error, t); }
            ui.add_space(4.0);
            ui.separator();
            let (create, cancel, _) = dialog_footer(ui, "Create report", None, t);
            if cancel { action = DraftAction::Cancel; } else if create { action = DraftAction::Create; }
        });
        if (self.from, self.to) != (before.0, before.1) && !self.other_chosen { self.other = self.previous_period(); }
        if (self.from, self.to, self.other) != before { self.error = None; }
        // Enter creates the report, as Enter applies the other dialogs.
        if action == DraftAction::None && !ctx.memory(|m| m.any_popup_open()) && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = DraftAction::Create;
        }
        action
    }

    /// The month choosers, range shortcuts and what the report will cover.
    fn fields(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let (min, max) = (self.min, self.max);
        let comparative = self.kind == ReportKind::Comparative;
        form_section(ui, if comparative { "Base range" } else { "Months" }, t, |ui| {
            ui.columns(2, |cols| {
                month_field(&mut cols[0], t, "From", "report_from", &mut self.from, min, max);
                month_field(&mut cols[1], t, "To", "report_to", &mut self.to, min, max);
            });
            ui.horizontal(|ui| {
                if self.kind != ReportKind::MonthByMonth && shortcut(ui, "This month", t) { self.cover(self.cur, self.cur); }
                if shortcut(ui, "Last 12 months", t) { self.cover(shift(self.cur, -11), self.cur); }
            });
            if !comparative {
                if let Some(text) = covers(&self.request()) { note(ui, &text, t); }
            }
        });
        if !comparative { return; }
        let end = shift(self.other, Cal::months_diff(&self.from, &self.to).max(0));
        form_section(ui, "Compare with", t, |ui| {
            ui.columns(2, |cols| {
                if month_field(&mut cols[0], t, "From", "report_other", &mut self.other, min, max) { self.other_chosen = true; }
                field(&mut cols[1], "To", t, |ui, label| {
                    ui.add_enabled_ui(false, |ui| {
                        egui::ComboBox::from_id_salt("report_other_to").width(ui.available_width())
                            .selected_text(month_name(&end)).show_ui(ui, |_| {}).response
                    }).inner.labelled_by(label).on_disabled_hover_text("As many months as the base range");
                });
            });
            ui.horizontal(|ui| {
                if shortcut(ui, "Previous period", t) { self.other = self.previous_period(); self.other_chosen = true; }
                if shortcut(ui, "A year earlier", t) { self.other = shift(self.from, -12); self.other_chosen = true; }
            });
            if let Some(text) = covers(&self.request()) { note(ui, &text, t); }
        });
    }
}

fn note(ui: &mut egui::Ui, text: &str, t: &Tokens) {
    ui.add(egui::Label::new(RichText::new(text).color(t.text_muted)).wrap());
}

/// A quiet accent text button that fills in a common range.
fn shortcut(ui: &mut egui::Ui, text: &str, t: &Tokens) -> bool {
    ui.add(egui::Button::new(RichText::new(text).color(t.accent)).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Month and year choosers under one caption; months the budget has no schedule for are disabled.
/// Returns whether the value changed.
fn month_field(ui: &mut egui::Ui, t: &Tokens, label: &str, id: &str, value: &mut Cal, min: Cal, max: Cal) -> bool {
    let before = *value;
    field(ui, label, t, |ui, label| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let width = ui.available_width();
            let month_w = ((width - 6.0) * 0.6).floor();
            egui::ComboBox::from_id_salt((id, "month")).width(month_w).selected_text(MONTHS_LONG[value.month() as usize]).show_ui(ui, |ui| {
                for (m, name) in MONTHS_LONG.iter().enumerate() {
                    let c = Cal::ymd(value.year(), m as i32, 1);
                    let available = (min.ym()..=max.ym()).contains(&c.ym());
                    if ui.add_enabled(available, egui::SelectableLabel::new(m as i32 == value.month(), *name)).clicked() { *value = c; }
                }
            }).response.labelled_by(label);
            egui::ComboBox::from_id_salt((id, "year")).width(width - month_w - 6.0).selected_text(value.year().to_string()).show_ui(ui, |ui| {
                for y in min.year()..=max.year() {
                    if ui.selectable_label(y == value.year(), y.to_string()).clicked() { *value = clamp(Cal::ymd(y, value.month(), 1), min, max); }
                }
            }).response.labelled_by(label);
        });
    });
    *value != before
}

/// Shows the range dialog while one is open. Returns the outcome once a report was created
/// (`Ok` confirmation or `Err` for the banner); validation problems stay in the dialog.
pub fn dialog(ctx: &egui::Context, t: &Tokens, db: Option<&mut BudgetDb>, cfg: &BudgetConfig, budget: u64) -> Option<Result<String, String>> {
    if !belongs_to(ctx, budget) { close(ctx); return None; }
    dialog_with(ctx, t, db, cfg, &System)
}

/// Whether a showing range dialog was opened for `budget` (its months come from that budget).
fn belongs_to(ctx: &egui::Context, budget: u64) -> bool {
    !is_open(ctx) || ctx.data(|d| d.get_temp::<u64>(budget_id())) == Some(budget)
}

fn dialog_with(ctx: &egui::Context, t: &Tokens, db: Option<&mut BudgetDb>, cfg: &BudgetConfig, shell: &dyn Shell) -> Option<Result<String, String>> {
    let mut draft = ctx.data(|d| d.get_temp::<Draft>(draft_id()))?;
    let Some(db) = db else { close(ctx); return None };
    match draft.show(ctx, t) {
        DraftAction::None => {}
        DraftAction::Cancel => { close(ctx); return None; }
        DraftAction::Create => {
            let req = draft.request();
            match validate(&req, draft.min, draft.max) {
                Ok(()) => { close(ctx); return Some(run(db, cfg, &req, shell)); }
                Err(e) => draft.error = Some(e),
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget_core::{Payment, END_MODE_INDEFINITE, FLAG_IMPORTANT, FLAG_PAID};
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Records what would have been opened; launches nothing.
    struct FakeShell { dir: PathBuf, spreadsheet: bool, opened: RefCell<Vec<(PathBuf, Viewer)>>, created: TempReports }

    impl Shell for FakeShell {
        fn report_dir(&self) -> PathBuf { self.dir.clone() }
        fn spreadsheet_available(&self) -> bool { self.spreadsheet }
        fn open(&self, path: &Path, viewer: Viewer) -> Result<(), String> {
            self.opened.borrow_mut().push((path.to_path_buf(), viewer));
            Ok(())
        }
        fn created(&self, path: &Path) { self.created.track(path); }
    }

    impl Drop for FakeShell {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
    }

    /// A shell writing under `target/report-tests/`, one folder per test.
    fn shell(spreadsheet: bool) -> FakeShell {
        let dir = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/report-tests")).unwrap()
            .join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        FakeShell { dir, spreadsheet, opened: RefCell::new(Vec::new()), created: TempReports::default() }
    }

    fn files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir).map(|entries| entries.map(|e| e.unwrap().path()).collect()).unwrap_or_default()
    }

    fn payment(description: &str, category: Option<&str>, amount: f64, start: i32, freq: char) -> Payment {
        let mut p = Payment::new(start);
        p.description = description.into();
        p.category = category.map(str::to_string);
        p.amount = amount;
        p.freq_type = freq;
        p.set_end_mode(END_MODE_INDEFINITE);
        p
    }

    fn month(v: i32) -> Cal { Cal::from_int(v) }

    /// A generated budget scheduled from April 2026 to December 2027: categories, an uncategorized
    /// payment with a picture, a disabled payment, flags, notes and a balance adjustment.
    fn fixture() -> BudgetDb {
        let mut db = BudgetDb::new(&month(20260601), 12, false);
        let mut salary = payment("Salary", Some("Work"), 3200.0, 20260415, 'm');
        salary.set_flag(20260615, FLAG_PAID);
        db.add(salary);
        let mut rent = payment("Rent", Some("Home"), -1450.0, 20260401, 'm');
        rent.set_note(20260601, Some("Paid by transfer"));
        rent.set_flag(20260601, FLAG_IMPORTANT);
        db.add(rent);
        db.add(payment("Groceries, market", Some("Food"), -85.25, 20260403, 'w'));
        let mut gym = payment("Gym", None, -40.0, 20260410, 'm');
        gym.image_path = Some("gym.png".into());
        db.add(gym);
        let mut old = payment("Old plan", Some("Home"), -10.0, 20260405, 'm');
        old.disabled = true;
        db.add(old);
        db.set_day_note(&month(20260610), Some("Check the bank"));
        db.update_adjustment(&month(20260620), 2500.0);
        db
    }

    fn req(kind: ReportKind, from: i32, to: i32, other: i32) -> Request {
        Request { kind, from: month(from), to: month(to), other: month(other) }
    }

    #[test]
    fn every_report_type_writes_a_non_empty_temp_report_and_opens_it_in_the_right_program() {
        let cfg = BudgetConfig::default();
        let shell = shell(true);
        let cases = [
            (req(ReportKind::Print, 20260601, 20260701, 0), "Budget from"),
            (req(ReportKind::ExcelReport, 20260601, 20260701, 0), "FORMULA="),
            (req(ReportKind::Comparative, 20260701, 20260701, 20260601), "Budget Comparison Report"),
            (req(ReportKind::MonthByMonth, 20260401, 20260701, 0), "Month-By-Month report"),
            (req(ReportKind::MonthlyCalendar, 20260601, 20260601, 0), "hide running balance"),
            (req(ReportKind::MonthlyDetailed, 20260601, 20260701, 0), "Opening balance"),
            (req(ReportKind::MonthlyExcel, 20260601, 20260701, 0), "vnd.ms-excel.numberformat"),
            (req(ReportKind::UnpaidPaid, 20260601, 20260601, 0), "*** UNPAID ***"),
        ];
        let mut db = fixture();
        let (min, max) = months_available(&db);
        assert_eq!((min.get(), max.get()), (20260401, 20271201));
        for (request, marker) in cases {
            assert_eq!(validate(&request, min, max), Ok(()), "{request:?}");
            let done = run(&mut db, &cfg, &request, &shell).unwrap_or_else(|e| panic!("{:?}: {e}", request.kind));
            assert!(done.contains(request.kind.title()), "{done}");
            let (path, viewer) = shell.opened.borrow().last().cloned().unwrap();
            assert_eq!(viewer, request.kind.viewer());
            assert_eq!(path.parent(), Some(shell.dir.as_path()), "reports go to the report folder only");
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("bgt-report-") && name.ends_with(".html"), "{name}");
            let html = std::fs::read_to_string(&path).unwrap();
            assert!(html.len() > 300 && html.contains(marker), "{:?} lacks {marker}:\n{html}", request.kind);
            assert!(html.contains("Rent") || html.contains("Salary"), "{:?} lists no payment", request.kind);
        }
        assert_eq!(files(&shell.dir).len(), cases.len(), "one new file per report");
        assert_eq!(shell.opened.borrow().iter().filter(|(_, v)| *v == Viewer::Spreadsheet).count(), 2);
    }

    #[test]
    fn report_pictures_are_embedded_so_the_temp_file_stands_alone() {
        let cfg = BudgetConfig::default();
        let mut db = fixture();
        let stats = generate(&mut db, &cfg, &req(ReportKind::Print, 20260601, 20260601, 0)).unwrap();
        assert!(stats.contains("<img src=\"data:;base64") && !stats.contains("file://icons"), "{stats}");
        let detailed = generate(&mut db, &cfg, &req(ReportKind::MonthlyDetailed, 20260601, 20260601, 0)).unwrap();
        assert!(detailed.contains("Check the bank") && detailed.contains(&report::unicodish(note_icon())), "the day note keeps its icon");
        assert!(!detailed.contains("file://"), "payment pictures must not point at a missing folder");
        // The bars take the graph colours of .budgetrc.
        assert_eq!(&solid_gif(0x3684c9)[13..16], &[0x36, 0x84, 0xc9]);
    }

    #[test]
    fn print_this_month_prints_the_month_on_screen_in_the_browser() {
        let cfg = BudgetConfig::default();
        let shell = shell(false);
        let mut db = fixture();
        let done = print_this_month(&mut db, &cfg, &month(20260715), &shell).unwrap();
        assert!(done.contains(PRINT_THIS_MONTH) && done.contains("browser"), "{done}");
        let (path, viewer) = shell.opened.borrow()[0].clone();
        assert_eq!(viewer, Viewer::Browser);
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.contains("<B>July-2026</B>") && !html.contains("June-2026") && !html.contains("August-2026"));
    }

    #[test]
    fn range_rules_follow_the_original_and_explain_rejections() {
        let (min, max) = (month(20260401), month(20271201));
        let after = validate(&req(ReportKind::Print, 20260701, 20260601, 0), min, max).unwrap_err();
        assert!(after.contains("From (July 2026) is after To (June 2026)"), "{after}");
        for kind in [ReportKind::MonthlyDetailed, ReportKind::UnpaidPaid, ReportKind::MonthlyCalendar] {
            assert!(validate(&req(kind, 20260801, 20260601, 0), min, max).is_err(), "{kind:?}");
        }
        let early = validate(&req(ReportKind::Print, 20260301, 20260601, 0), min, max).unwrap_err();
        assert!(early.contains("April 2026 to December 2027"), "{early}");
        let single = validate(&req(ReportKind::MonthByMonth, 20260601, 20260601, 0), min, max).unwrap_err();
        assert!(single.contains("at least two months"), "{single}");
        assert_eq!(validate(&req(ReportKind::MonthByMonth, 20260501, 20260601, 0), min, max), Ok(()));
        // Comparative: a start whose window overlaps the base range is refused (06 §6).
        let overlap = validate(&req(ReportKind::Comparative, 20260601, 20260801, 20260501), min, max).unwrap_err();
        assert!(overlap.contains("overlaps June 2026 to August 2026 (3 months)"), "{overlap}");
        assert_eq!(validate(&req(ReportKind::Comparative, 20260701, 20260901, 20260401), min, max), Ok(()));
        assert_eq!(validate(&req(ReportKind::Comparative, 20260701, 20260901, 20261001), min, max), Ok(()));
        assert!(validate(&req(ReportKind::Comparative, 20260701, 20260901, 20271101), min, max).unwrap_err().contains("must fit"));
    }

    #[test]
    fn covered_months_are_spelled_out_and_include_the_to_month() {
        assert_eq!(covers(&req(ReportKind::Print, 20260601, 20260601, 0)).unwrap(), "Covers June 2026.");
        assert_eq!(covers(&req(ReportKind::MonthlyCalendar, 20260601, 20260801, 0)).unwrap(), "Covers June 2026 to August 2026 (3 months).");
        assert_eq!(covers(&req(ReportKind::MonthlyDetailed, 20260601, 20260801, 0)).unwrap(), "Covers June 2026 to August 2026 (3 months).");
        assert_eq!(covers(&req(ReportKind::MonthlyExcel, 20260601, 20260601, 0)).unwrap(), "Covers June 2026.");
        assert_eq!(covers(&req(ReportKind::UnpaidPaid, 20260601, 20260701, 0)).unwrap(), "Covers 1 June 2026 to 31 July 2026.");
        assert_eq!(covers(&req(ReportKind::Comparative, 20260601, 20260701, 20260401)).unwrap(),
            "Compares June 2026 to July 2026 (2 months) with April 2026 to May 2026 (2 months).");
        assert_eq!(covers(&req(ReportKind::Print, 20260701, 20260601, 0)), None);
        // Unlike the original, the Monthly Detailed and Excel reports include the To month, and a
        // one-month range still produces exactly that month.
        let mut db = fixture();
        for kind in [ReportKind::MonthlyDetailed, ReportKind::MonthlyExcel] {
            let html = generate(&mut db, &BudgetConfig::default(), &req(kind, 20260601, 20260801, 0)).unwrap();
            assert!(html.contains("01-Aug-2026") && html.contains("31-Aug-2026") && !html.contains("01-Sep-2026"), "{kind:?}");
            let html = generate(&mut db, &BudgetConfig::default(), &req(kind, 20260601, 20260601, 0)).unwrap();
            assert!(html.contains("01-Jun-2026") && !html.contains("01-Jul-2026"), "{kind:?}");
        }
    }

    #[test]
    fn the_dialog_starts_at_the_month_on_screen_like_the_original() {
        let (min, max) = (month(20260401), month(20271201));
        let d = Draft::new(ReportKind::Print, &month(20260615), min, max);
        assert_eq!((d.from.get(), d.to.get()), (20260601, 20260601));
        // Month-by-Month: the last 12 months, limited to what the budget covers.
        let d = Draft::new(ReportKind::MonthByMonth, &month(20270615), min, max);
        assert_eq!((d.from.get(), d.to.get()), (20260701, 20270601));
        let d = Draft::new(ReportKind::MonthByMonth, &month(20260615), min, max);
        assert_eq!((d.from.get(), d.to.get()), (20260401, 20260601));
        // Comparative: the previous period, or the next one when the budget starts too late.
        let d = Draft::new(ReportKind::Comparative, &month(20260615), min, max);
        assert_eq!(d.other.get(), 20260501);
        assert_eq!(validate(&d.request(), min, max), Ok(()));
        let d = Draft::new(ReportKind::Comparative, &month(20260415), min, max);
        assert_eq!(d.other.get(), 20260501);
        // "Last 12 months" ends at the month on screen for every report.
        let mut d = Draft::new(ReportKind::MonthlyDetailed, &month(20270615), min, max);
        d.cover(shift(d.cur, -11), d.cur);
        assert_eq!((d.from.get(), d.to.get()), (20260701, 20270601));
        assert!(covers(&d.request()).unwrap().starts_with("Covers July 2026 to June 2027 (12 months)."));
    }

    #[test]
    fn the_range_dialog_closes_when_another_budget_replaces_its_own() {
        let (ctx, t) = gui();
        let mut db = fixture();
        let (min, max) = months_available(&db);
        ctx.data_mut(|d| { d.insert_temp(draft_id(), Draft::new(ReportKind::Print, &month(20260601), min, max)); d.insert_temp(budget_id(), 1_u64); });
        let _ = ctx.run(input(Vec::new()), |ctx| { dialog(ctx, &t, Some(&mut db), &BudgetConfig::default(), 1); });
        assert!(is_open(&ctx), "still the budget it was opened for");
        let _ = ctx.run(input(Vec::new()), |ctx| { assert!(dialog(ctx, &t, Some(&mut db), &BudgetConfig::default(), 2).is_none()); });
        assert!(!is_open(&ctx), "a background open replaced the budget");
    }

    // ---------- UI ----------

    fn gui() -> (egui::Context, Tokens) {
        let ctx = egui::Context::default();
        let t = Tokens::new(&BudgetConfig::default().colors, false);
        theme::install_fonts(&ctx);
        theme::apply(&ctx, &t);
        (ctx, t)
    }

    /// The app's minimum window size.
    fn input(events: Vec<egui::Event>) -> egui::RawInput { sized(egui::vec2(900.0, 600.0), events) }

    fn sized(size: egui::Vec2, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() }
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
        fn visit(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(text) => out.push((text.galley.job.text.clone(), text.pos + text.galley.mesh_bounds.center().to_vec2())),
                egui::Shape::Vec(shapes) => for shape in shapes { visit(shape, out); },
                _ => {},
            }
        }
        let mut out = Vec::new();
        for shape in &output.shapes { visit(&shape.shape, &mut out); }
        out
    }

    fn click(at: egui::Pos2) -> [Vec<egui::Event>; 2] {
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        [vec![egui::Event::PointerMoved(at), press(true)], vec![press(false)]]
    }

    /// Renders the menu in a bare window, clicks "Reports", then clicks `item` if given.
    /// Returns the texts shown while the menu was open.
    fn use_menu(ctx: &egui::Context, t: &Tokens, mut db: Option<&mut BudgetDb>, shell: &FakeShell, item: Option<&str>) -> (Vec<String>, Option<Result<String, String>>) {
        let cfg = BudgetConfig::default();
        let cur = month(20260601);
        let mut outcome = None;
        let frame = |events: Vec<egui::Event>, mut db: Option<&mut BudgetDb>, outcome: &mut Option<Result<String, String>>| {
            ctx.run(input(events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(o) = menu_with(ui, t, db.as_deref_mut(), &cur, &cfg, shell) { *outcome = Some(o); }
                });
            })
        };
        let mut shown = texts(&frame(Vec::new(), db.as_deref_mut(), &mut outcome));
        let at = |shown: &[(String, egui::Pos2)], label: &str| shown.iter().find(|(text, _)| text == label).map(|(_, pos)| *pos)
            .unwrap_or_else(|| panic!("{label} is not shown: {:?}", shown.iter().map(|(text, _)| text).collect::<Vec<_>>()));
        for events in click(at(&shown, "Reports")) { frame(events, db.as_deref_mut(), &mut outcome); }
        shown = texts(&frame(Vec::new(), db.as_deref_mut(), &mut outcome));
        let menu: Vec<String> = shown.iter().map(|(text, _)| text.clone()).collect();
        if let Some(item) = item {
            for events in click(at(&shown, item)) { frame(events, db.as_deref_mut(), &mut outcome); }
        }
        (menu, outcome)
    }

    #[test]
    fn the_reports_menu_lists_every_report_under_the_original_names() {
        let (ctx, t) = gui();
        let shell = shell(true);
        let mut db = fixture();
        let (menu, _) = use_menu(&ctx, &t, Some(&mut db), &shell, None);
        for label in [PRINT_THIS_MONTH, "Income vs. Expenses", "Transactions"].into_iter().chain(GROUPS.iter().flat_map(|(_, kinds)| kinds.iter().map(|k| k.menu_label()))) {
            assert!(menu.iter().any(|text| text == label), "{label} missing from {menu:?}");
        }
        assert_eq!(GROUPS.iter().flat_map(|(_, kinds)| kinds).count(), 8, "every generator has an entry");
        assert!(shell.opened.borrow().is_empty() && !is_open(&ctx), "opening the menu creates nothing");
    }

    #[test]
    fn menu_items_open_the_dialog_or_print_at_once_and_wait_for_a_budget() {
        let shell = shell(true);
        let mut db = fixture();
        let (ctx, t) = gui();
        let (_, outcome) = use_menu(&ctx, &t, Some(&mut db), &shell, Some("Excel Report…"));
        assert!(outcome.is_none() && shell.opened.borrow().is_empty());
        let draft = ctx.data(|d| d.get_temp::<Draft>(draft_id())).expect("the item opens the range dialog");
        assert_eq!((draft.kind, draft.from.get(), draft.to.get()), (ReportKind::ExcelReport, 20260601, 20260601));

        let (ctx, t) = gui();
        let (_, outcome) = use_menu(&ctx, &t, Some(&mut db), &shell, Some(PRINT_THIS_MONTH));
        assert!(outcome.unwrap().is_ok() && !is_open(&ctx));
        assert_eq!(shell.opened.borrow().len(), 1);

        // Without a spreadsheet program the Excel items are disabled (06 §1.12).
        let (ctx, t) = gui();
        let no_excel = self::shell(false);
        let (_, outcome) = use_menu(&ctx, &t, Some(&mut db), &no_excel, Some("Monthly Excel Report…"));
        assert!(outcome.is_none() && !is_open(&ctx));

        // Without a budget the menu does not open.
        let (ctx, t) = gui();
        let (shown, outcome) = use_menu(&ctx, &t, None, &shell, None);
        assert!(shown.iter().any(|text| text == "Reports"));
        assert!(!shown.iter().any(|text| text == PRINT_THIS_MONTH || text == "Print…"), "no report item may be reachable without a budget: {shown:?}");
        assert!(outcome.is_none() && !is_open(&ctx) && shell.opened.borrow().len() == 1);
    }

    fn enter() -> egui::Event {
        egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn creating_with_from_after_to_is_refused_in_the_dialog_and_writes_nothing() {
        let (ctx, t) = gui();
        let cfg = BudgetConfig::default();
        let shell = shell(true);
        let mut db = fixture();
        let (min, max) = months_available(&db);
        let mut draft = Draft::new(ReportKind::MonthlyDetailed, &month(20260601), min, max);
        draft.from = month(20260801);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        let mut outcome = None;
        let _ = ctx.run(input(vec![enter()]), |ctx| outcome = dialog_with(ctx, &t, Some(&mut db), &cfg, &shell));
        assert!(outcome.is_none() && is_open(&ctx), "the dialog stays open");
        let error = ctx.data(|d| d.get_temp::<Draft>(draft_id())).unwrap().error.unwrap();
        assert!(error.contains("From (August 2026) is after To (June 2026)"), "{error}");
        assert!(shell.opened.borrow().is_empty() && files(&shell.dir).is_empty(), "nothing is written or opened");
        // The message is shown in the dialog.
        let output = ctx.run(input(Vec::new()), |ctx| { dialog_with(ctx, &t, Some(&mut db), &cfg, &shell); });
        assert!(texts(&output).iter().any(|(text, _)| text == &error));

        // Fixing the range and pressing Enter creates the report and closes the dialog.
        let mut fixed = ctx.data(|d| d.get_temp::<Draft>(draft_id())).unwrap();
        fixed.to = month(20260901);
        ctx.data_mut(|d| d.insert_temp(draft_id(), fixed));
        let _ = ctx.run(input(vec![enter()]), |ctx| outcome = dialog_with(ctx, &t, Some(&mut db), &cfg, &shell));
        assert!(outcome.unwrap().is_ok() && !is_open(&ctx));
        assert_eq!(files(&shell.dir).len(), 1);
    }

    #[test]
    fn escape_closes_the_dialog_without_a_report() {
        let (ctx, t) = gui();
        let cfg = BudgetConfig::default();
        let shell = shell(true);
        let mut db = fixture();
        let (min, max) = months_available(&db);
        ctx.data_mut(|d| d.insert_temp(draft_id(), Draft::new(ReportKind::Comparative, &month(20260601), min, max)));
        // The first frame only sizes the new dialog; the scroll area settles on the next.
        let large = egui::vec2(1280.0, 840.0);
        let mut output = ctx.run(sized(large, Vec::new()), |ctx| { dialog_with(ctx, &t, Some(&mut db), &cfg, &shell); });
        for _ in 0..2 { output = ctx.run(sized(large, Vec::new()), |ctx| { dialog_with(ctx, &t, Some(&mut db), &cfg, &shell); }); }
        let shown: Vec<String> = texts(&output).into_iter().map(|(text, _)| text).collect();
        for label in ["Comparative report", "Base range", "Compare with", "Create report", "Cancel", "Compares June 2026 with May 2026."] {
            assert!(shown.iter().any(|text| text == label), "{label} missing from {shown:?}");
        }
        let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let mut outcome = None;
        let _ = ctx.run(input(vec![escape]), |ctx| outcome = dialog_with(ctx, &t, Some(&mut db), &cfg, &shell));
        assert!(outcome.is_none() && !is_open(&ctx) && shell.opened.borrow().is_empty());
    }

    #[test]
    fn the_tallest_dialog_fits_the_smallest_window_and_keeps_footer_and_error_in_view() {
        let cfg = BudgetConfig::default();
        let shell = shell(true);
        let mut db = fixture();
        let (min, max) = months_available(&db);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let mut overlapping = Draft::new(ReportKind::Comparative, &month(20260601), min, max);
        overlapping.other = overlapping.from;
        overlapping.error = validate(&overlapping.request(), min, max).err();
        let error = overlapping.error.clone().unwrap();
        for (draft, must_show) in [
            (Draft::new(ReportKind::Comparative, &month(20260601), min, max), vec!["Comparative report", "Compares June 2026 with May 2026.", "Cancel", "Create report"]),
            (overlapping, vec![error.as_str(), "Cancel", "Create report"]),
        ] {
            let (ctx, t) = gui();
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
            let mut output = ctx.run(input(Vec::new()), |ctx| { dialog_with(ctx, &t, Some(&mut db), &cfg, &shell); });
            for _ in 0..2 { output = ctx.run(input(Vec::new()), |ctx| { dialog_with(ctx, &t, Some(&mut db), &cfg, &shell); }); }
            let shown = texts(&output);
            for label in must_show {
                let (_, at) = shown.iter().find(|(text, _)| text == label).unwrap_or_else(|| panic!("{label} is not shown at 900×600"));
                assert!(screen.contains(*at), "{label} is off screen at {at:?}");
            }
        }
    }

    #[test]
    fn system_reports_go_to_the_temp_folder() {
        assert_eq!(System.report_dir(), std::env::temp_dir());
    }

    #[test]
    fn the_reports_this_run_wrote_are_deleted_at_exit_and_nothing_else() {
        let cfg = BudgetConfig::default();
        let shell = shell(false);
        let mut db = fixture();
        run(&mut db, &cfg, &req(ReportKind::Print, 20260601, 20260601, 0), &shell).unwrap();
        print_this_month(&mut db, &cfg, &month(20260601), &shell).unwrap();
        // Another run's report and an unrelated file in the same folder stay.
        let (other, notes) = (shell.dir.join("bgt-report-other-run.html"), shell.dir.join("notes.txt"));
        std::fs::write(&other, "x").unwrap();
        std::fs::write(&notes, "x").unwrap();
        let written: Vec<PathBuf> = shell.opened.borrow().iter().map(|(path, _)| path.clone()).collect();
        assert!(written.iter().all(|path| path.exists()));
        assert_eq!(shell.created.delete_all(), 2);
        assert!(written.iter().all(|path| !path.exists()));
        let mut left = files(&shell.dir);
        left.sort();
        assert_eq!(left, [other, notes]);
        assert_eq!(shell.created.delete_all(), 0, "each file is deleted once");
    }

    #[test]
    fn reports_written_to_the_system_temp_folder_are_deleted_at_exit() {
        // The real shell tracks what this run wrote; the exit clean-up deletes exactly that.
        let dir = shell(false).dir.with_extension("system");
        std::fs::create_dir_all(&dir).unwrap();
        let (written, kept) = (dir.join("bgt-report-1.html"), dir.join("bgt-report-2.html"));
        std::fs::write(&written, "x").unwrap();
        std::fs::write(&kept, "x").unwrap();
        System.created(&written);
        delete_created_reports();
        assert!(!written.exists() && kept.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Checks the Windows binding without launching anything: the struct matches SHELLEXECUTEINFOW,
    /// the association lookup answers "no program" for an unknown file type, and opening a missing
    /// file fails with "file not found" (a malformed struct would fail with "invalid parameter").
    #[cfg(windows)]
    #[test]
    fn windows_shell_binding_matches_the_api() {
        assert_eq!(platform::INFO_SIZE, if cfg!(target_pointer_width = "64") { 112 } else { 60 });
        assert!(!platform::has_handler(".bgt-report-test-unregistered"));
        let shell = shell(false);
        let missing = shell.dir.join("bgt-report-missing.html");
        assert!(!missing.exists());
        let error = platform::open(&missing, None).unwrap_err();
        assert!(error.contains("os error 2") || error.contains("os error 3"), "{error}");
    }
}
