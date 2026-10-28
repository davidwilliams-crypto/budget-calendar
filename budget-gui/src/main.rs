#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Budget Calendar main window.
//!
//! Layout: header panel (menu row + toolbar), optional right summary panel, central panel with the
//! stats strip and the calendar. `theme.rs` holds the design tokens, `view.rs` the month view-model,
//! `calendar.rs` the painter-drawn grid.

mod calendar;
mod balance_editor;
mod document;
mod editor;
mod payment_list;
mod theme;
mod view;
mod version;

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::{bgt, BudgetDb, Cal, Field};
use eframe::egui::{self, Align, Align2, Button, Frame, Key, Layout, Margin, Modifiers, RichText, Sense, Stroke, StrokeKind, TextEdit, Vec2};
use egui_phosphor::regular as ph;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};


use theme::{alpha, tint, ts, Tokens, CAPTION, ICON, SECTION, SEMI, SP1, SP2, SP3, SP4, TITLE};
use view::{money, MonthView};
use editor::{PaymentAction, PaymentEditor, TransactionAction, TransactionEditor, TransactionScope};
use balance_editor::{BalanceAction, BalanceEditor};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const SUMMARY_W: f32 = 292.0;

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
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
    Cli {
        file: first_positional(&args),
        month: val("--month").and_then(|m| m.parse().ok()),
        screenshot: val("--screenshot").map(PathBuf::from),
        size: val("--size").and_then(|s| { let (w, h) = s.split_once('x')?; Some([w.parse().ok()?, h.parse().ok()?]) }),
        dark: args.iter().any(|a| a == "--dark"),
        today: val("--today").and_then(|t| t.parse().ok()),
        filter: val("--filter"),
    }
}

struct Toast { text: String, error: bool, until: Instant }

#[derive(Clone)]
enum PendingAction { Open(PathBuf), Close }
#[derive(Clone, Copy)]
struct DeleteTarget { id: i32, date: Option<i32> }

struct App {
    cfg: BudgetConfig,
    rc_path: PathBuf,
    db: Option<BudgetDb>,
    file: Option<PathBuf>,
    password: Option<String>,
    saved_bytes: Option<Vec<u8>>,
    dirty: bool,
    payment_editor: Option<PaymentEditor>,
    transaction_editor: Option<TransactionEditor>,
    balance_editor: Option<BalanceEditor>,
    drag_month_hover: Option<(i32, Instant)>,
    show_payments: bool,
    payment_list: payment_list::PaymentList,
    day_dialog: Option<i32>,
    confirm_delete: Option<DeleteTarget>,
    pending_action: Option<PendingAction>,
    allow_close: bool,
    /// First day of the viewed month.
    cur: Cal,
    today: i32,
    filter_text: String,
    filter: Option<Filter>,
    filter_error: Option<String>,
    view: Option<MonthView>,
    pw_prompt: Option<(PathBuf, String, String)>,
    toast: Option<Toast>,
    /// Persistent error until dismissed.
    banner: Option<String>,
    /// `--screenshot <png>`: render a few frames, save the window, exit.
    screenshot: Option<PathBuf>,
    frames: u32,
    dark: bool,
    show_summary: bool,
    show_about: bool,
    show_legend: bool,
    show_shortcuts: bool,
    focus_filter: bool,
    title_dirty: bool,
    t: Tokens,
}

impl App {
    fn new(cli: Cli) -> App {
        let rc_path = home_dir().join(".budgetrc");
        let cfg = BudgetConfig::load(&rc_path);
        let mut cur = Cal::today();
        cur.set_day_one();
        let t = Tokens::new(&cfg.colors, cli.dark);
        let mut app = App {
            cfg, rc_path, db: None, file: None, password: None, cur, today: cli.today.unwrap_or_else(|| Cal::today().get()),
            saved_bytes: None, dirty: false, payment_editor: None, transaction_editor: None,
            balance_editor: None, drag_month_hover: None,
            show_payments: false, payment_list: payment_list::PaymentList::default(),
            day_dialog: None, confirm_delete: None, pending_action: None, allow_close: false,
            filter_text: String::new(), filter: None, filter_error: None, view: None, pw_prompt: None, toast: None, banner: None,
            screenshot: cli.screenshot, frames: 0, dark: cli.dark, show_summary: true, show_about: false, show_legend: false, show_shortcuts: false, focus_filter: false, title_dirty: true, t,
        };
        if let Some(m) = cli.month { app.cur = Cal::from_int(m); app.cur.set_day_one(); }
        if let Some(f) = cli.filter { app.filter_text = f; app.set_filter(); }
        if let Some(f) = cli.file {
            // A file named on the command line never falls back to the last-used budget.
            let p = PathBuf::from(f.trim());
            if p.is_file() { app.open(&p); app.toast = None; }
            else { app.banner = Some(format!("Cannot open {}: file not found.", p.display())); }
        } else if let Some(f) = app.cfg.last_file_name.clone() {
            let p = PathBuf::from(f.trim());
            if p.is_file() { app.open(&p); app.toast = None; }
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>) { self.toast = Some(Toast { text: text.into(), error: false, until: Instant::now() + Duration::from_secs(4) }); }
    fn fail(&mut self, text: impl Into<String>) { self.banner = Some(text.into()); }

    fn open(&mut self, path: &Path) {
        if self.dirty || self.has_draft() {
            self.pending_action = Some(PendingAction::Open(path.to_path_buf()));
            return;
        }
        self.open_unchecked(path);
    }

    fn open_unchecked(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) if bgt::is_encrypted_text(&bgt::latin1_decode(&bytes)) => {
                self.pw_prompt = Some((path.to_path_buf(), String::new(), String::new()));
            }
            Ok(_) => self.load(path, None),
            Err(e) => self.fail(format!("Unable to open '{}': {e}", path.display())),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).pick_file() { self.open(&p); }
    }

    fn load(&mut self, path: &Path, password: Option<&str>) {
        let parsed = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
            let loaded = bgt::load_str(&bgt::latin1_decode(&bytes), self.today, password).map_err(|l| l.log)?;
            // A malformed file must surface as an error banner, never take the whole app down.
            let (cur, months, unlimited) = (self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
            let mut db = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| document::from_loaded(loaded, &cur, months, unlimited)))
                .map_err(|_| "This budget file could not be built: it contains values the schedule engine cannot handle.".to_string())?;
            db.file_name = Some(path.display().to_string());
            Ok((db, bytes))
        });
        match parsed {
            Ok((db, bytes)) => {
                self.db = Some(db);
                self.file = Some(path.to_path_buf());
                self.password = password.map(str::to_string);
                self.saved_bytes = Some(bytes);
                self.dirty = false;
                self.payment_editor = None;
                self.transaction_editor = None;
                self.balance_editor = None;
                self.drag_month_hover = None;
                self.payment_list.selected = None;
                self.day_dialog = None;
                self.cfg.last_file_name = Some(path.display().to_string());
                self.cfg.add_recent_file(&path.display().to_string());
                let _ = self.cfg.save(&self.rc_path);
                self.view = None;
                self.banner = None;
                self.title_dirty = true;
                self.notify(format!("Opened {}", file_name(path)));
            }
            Err(log) => self.fail(log),
        }
    }

    fn save(&mut self) -> bool {
        if self.db.is_some() && !self.dirty { return true; }
        let Some(path) = self.file.clone() else { return false };
        let expected = self.saved_bytes.clone();
        self.save_to(&path, expected.as_deref())
    }

    fn save_as(&mut self) -> bool {
        let dialog = rfd::FileDialog::new().add_filter("Budget", &["bgt"])
            .set_file_name(self.file.as_deref().map(file_name).unwrap_or_else(|| "budget.bgt".into()));
        let Some(mut path) = dialog.save_file() else { return false };
        if path.extension().is_none() { path.set_extension("bgt"); }
        let expected = if self.file.as_deref() == Some(path.as_path()) {
            self.saved_bytes.clone()
        } else {
            match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => { self.fail(format!("Unable to inspect the destination: {e}")); return false; }
            }
        };
        self.save_to(&path, expected.as_deref())
    }

    fn save_to(&mut self, path: &Path, expected: Option<&[u8]>) -> bool {
        let Some(db) = &self.db else { return false };
        match document::save_checked(db, path, self.password.as_deref(), expected) {
            Ok(bytes) => {
                self.file = Some(path.to_path_buf());
                if let Some(db) = &mut self.db { db.file_name = Some(path.display().to_string()); }
                self.saved_bytes = Some(bytes);
                self.dirty = false;
                self.title_dirty = true;
                self.banner = None;
                self.cfg.last_file_name = Some(path.display().to_string());
                self.cfg.add_recent_file(&path.display().to_string());
                let _ = self.cfg.save(&self.rc_path);
                self.notify(format!("Saved {}", file_name(path)));
                true
            }
            Err(e) => { self.fail(e); false }
        }
    }

    fn go(&mut self, months: i32) {
        self.cur.add(Field::Month, months);
        self.cur.set_day_one();
        if let Some(db) = &mut self.db {
            if !db.is_month_available(&self.cur) {
                document::rebuild(db, &self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
            }
        }
    }

    fn go_today(&mut self) {
        self.cur = Cal::from_int(self.today);
        self.go(0);
    }

    fn set_filter(&mut self) {
        let t = self.filter_text.trim().replace('&', "\u{ff06}");
        self.filter_error = None;
        self.filter = if t.is_empty() { None } else {
            match Filter::new(&t, self.today) {
                Ok(filter) => Some(filter),
                Err(_) => { self.filter_error = Some("Finish entering the search filter, or clear it to show all transactions.".into()); None }
            }
        };
    }

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

    fn ensure_view(&mut self) {
        let Some(db) = &mut self.db else { self.view = None; return };
        let key = (self.cur.get(), self.filter_text.clone(), db.update_count as u64);
        if self.view.as_ref().map_or(false, |v| v.key == key) { return; }
        self.view = Some(view::build(db, &self.cfg, &self.cur, self.today, self.filter.as_ref(), key));
    }

    // ---------- header ----------

    fn header(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        ui.spacing_mut().item_spacing.y = SP1;
        // Room for the file name: what is left after the brand, the status pill and the right-hand actions.
        let name_w = (ui.available_width() - 880.0).clamp(0.0, 300.0);
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            ui.painter().rect_filled(mark, 8, t.accent);
            ui.painter().text(mark.center(), Align2::CENTER_CENTER, ph::CALENDAR_DOTS,
                egui::FontId::new(22.0, egui::FontFamily::Proportional), theme::on(t.accent));
            ui.add_space(SP1);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP2;
                // brand > file > actions: the title is the largest, the version chip the quietest.
                ui.label(RichText::new("Budget Calendar").size(18.0).family(egui::FontFamily::Name("semi".into())).color(t.text));
                if ui.add(Button::new(RichText::new(format!("v{}", version::VERSION)).text_style(ts(CAPTION)).color(t.text_muted)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Current version and change history").clicked() { self.show_about = true; }
                match self.file.as_deref() {
                    Some(path) => {
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
                    None if name_w >= 60.0 => {
                        ui.add_sized(Vec2::new(name_w, 20.0), egui::Label::new(RichText::new("Your money, one day at a time").text_style(ts(CAPTION)).color(t.text_muted)).truncate());
                    }
                    None => {}
                }
                if self.db.is_some() {
                    let (text, dot) = if self.dirty { ("Unsaved changes", t.important) } else { ("Saved", t.positive) };
                    theme::status_pill(ui, text, dot, self.dirty, &t)
                        .on_hover_text(if self.dirty { "Press Ctrl+S to save your changes" } else { "Everything is saved to disk" });
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(self.db.is_some(), |ui| {
                    if theme::primary_button(ui, format!("{}  New payment", ph::PLUS), &t).on_hover_text("Add a one-time or repeating payment · Ctrl+N").clicked() { self.new_payment(self.today); }
                    ui.add_enabled_ui(self.dirty, |ui| {
                        if theme::secondary_button(ui, format!("{}  Save changes", ph::FLOPPY_DISK), &t).on_hover_text("Save your budget · Ctrl+S").clicked() { self.save(); }
                    });
                });
                ui.menu_button("File", |ui| {
                    ui.set_min_width(210.0);
                    if menu_item(ui, "Open budget…", "Ctrl+O").clicked() { ui.close_menu(); self.open_dialog(); }
                    if ui.add_enabled(self.db.is_some(), menu_btn("Save as…", "")).clicked() { ui.close_menu(); self.save_as(); }
                    ui.separator();
                    for recent in self.cfg.recent_file.clone() {
                        if ui.button(file_name(Path::new(&recent))).on_hover_text(&recent).clicked() { ui.close_menu(); self.open(Path::new(&recent)); }
                    }
                    ui.separator();
                    if ui.button("Close app").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button("View", |ui| {
                    let mut dark = self.dark;
                    if ui.checkbox(&mut dark, "Dark appearance").clicked() { ui.close_menu(); self.set_dark(ui.ctx(), dark); }
                    ui.checkbox(&mut self.show_summary, "Monthly breakdown on wide screens");
                    if ui.checkbox(&mut self.cfg.show_decimals_in_cv, "Show cents in calendar").clicked() { self.view = None; }
                    ui.separator();
                    if ui.button("Calendar legend…").clicked() { self.show_legend = true; ui.close_menu(); }
                    if ui.button("About and version history…").clicked() { self.show_about = true; ui.close_menu(); }
                    ui.separator();
                    if menu_item(ui, "Keyboard shortcuts", "F1").clicked() { self.show_shortcuts = true; ui.close_menu(); }
                });
            });
        });
        ui.add_space(6.0);
        self.toolbar(ui);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        let compact = ui.available_width() < 1000.0;
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
                ui.add_space(if compact { SP1 } else { SP3 });
                let previous = icon_button(ui, ph::CARET_LEFT).on_hover_text("Previous month (PgUp)\nHold a dragged payment here to move to this month");
                if previous.clicked() { self.go(-1); }
                self.drag_month_navigation(ui.ctx(), &previous, -1);
                ui.menu_button(RichText::new(format!("{} {}  {}", MONTHS_LONG[self.cur.month() as usize], self.cur.year(), ph::CARET_DOWN))
                    .size(if compact { 17.0 } else { 20.0 }).family(egui::FontFamily::Name("semi".into())), |ui| {
                    let mut year = self.cur.year();
                    ui.horizontal(|ui| { ui.label("Year"); ui.add(egui::DragValue::new(&mut year).range(1000..=9999)); });
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
                // Today is a call to action only when another month is on screen.
                let away = self.cur.get() / 100 != self.today / 100;
                if away {
                    if theme::primary_button(ui, format!("{}  Today", ph::DOT_OUTLINE), &t).on_hover_text("Jump to the current month (Home or T)").clicked() { self.go_today(); }
                } else {
                    ui.add_enabled(false, Button::new(RichText::new("Today").color(t.text_muted)).min_size(Vec2::new(0.0, 32.0)))
                        .on_disabled_hover_text("You are already viewing the current month");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(self.db.is_some(), |ui| self.filter_box(ui, &t, if compact { 218.0 } else { 278.0 }));
                });
            } else {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Your complete payment schedule").color(t.text_muted).text_style(ts(CAPTION)));
                });
            }
        });
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
        let compact = ui.available_width() < 1000.0;
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
                response.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {}", money(*value, true))));
                if index == 0 || index == 3 {
                    let mut date = self.cur;
                    if index == 0 { date.add(Field::Day, -1); } else { date.set(Field::Day, date.max_day()); }
                    response.response.context_menu(|ui| {
                        if ui.button("Edit balance").clicked() { action = Some(calendar::Action::EditBalance(date.get())); ui.close_menu(); }
                    });
                }
                response.response.on_hover_text(format!("{label}: {}\nNet change: {}\nLowest balance: {}\nAverage balance: {}\nHighest balance: {}",
                    money(*value, true), money(q.income + q.expenses, true), money(q.min, true), money(q.avg, true), money(q.max, true)));
            }
        });
        action
    }

    fn summary(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        let Some(v) = &self.view else { return };
        let active = self.filter_text.trim().to_string();
        let mut clicked: Option<String> = None;
        t.card().inner_margin(Margin::symmetric(12, 12)).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Summary").text_style(ts(SECTION)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("share of income").text_style(ts(CAPTION)).color(t.text_faint));
                });
            });
            ui.add_space(SP1);
            let has_diff = v.brief.iter().any(|r| r.diff.is_some());
            let (aw, pw, dw): (f32, f32, f32) = (70.0, 36.0, if has_diff { 56.0 } else { 0.0 });
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
                    ui.painter().hline(r.x_range(), ui.cursor().min.y - 2.0, Stroke::new(1.5_f32, tint(t.surface, t.signed(row.amount), 0.7)));
                    ui.add_space(SP1);
                }
                let filterable = !row.filter.is_empty();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), if is_net { 26.0 } else { 30.0 }), if filterable { Sense::click() } else { Sense::hover() });
                let selected = !active.is_empty() && row.filter == active;
                let p = ui.painter();
                if selected { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, tint(t.surface, t.accent, 0.12)); }
                else if filterable && resp.hovered() { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, t.surface_hover); }
                // Text sits in the upper part of the row; the share bar owns the bottom 4px.
                let cy = if is_net { rect.center().y } else { rect.min.y + 11.0 };
                let fid = if row.total { ts(SEMI).resolve(ui.style()) } else { egui::TextStyle::Body.resolve(ui.style()) };
                let mut x = rect.max.x;
                if has_diff {
                    if let Some(d) = row.diff { p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(d, false), fid.clone(), t.signed(d)); }
                    x -= dw;
                }
                p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, view::pct_text(row.pct), fid.clone(), t.text_muted);
                x -= pw;
                let ar = p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(row.amount, false), fid.clone(), t.signed(row.amount));
                x -= aw.max(ar.width() + 8.0);
                let desc_col = if selected { t.accent } else { t.text };
                let desc_w = (x - rect.min.x).max(10.0);
                let mut job = egui::text::LayoutJob::simple_singleline(row.desc.clone(), fid, desc_col);
                job.wrap = egui::text::TextWrapping::truncate_at_width(desc_w);
                let g = p.layout_job(job);
                let gh = g.size().y;
                p.galley(egui::pos2(rect.min.x, cy - gh / 2.0), g, desc_col);
                // share of income: a 4px bar across the full row inside a faint track
                if !is_net {
                    let track = egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y - 6.0), Vec2::new(rect.width(), 4.0));
                    p.rect_filled(track, 2, tint(t.surface, t.border, 0.5));
                    let share = (row.pct.abs() / 100.0).clamp(0.0, 1.0) as f32;
                    if share > 0.0 {
                        let fill = egui::Rect::from_min_size(track.min, Vec2::new((track.width() * share).max(3.0), 4.0));
                        p.rect_filled(fill, 2, tint(t.surface, t.signed(row.amount), 0.75));
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
                    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 52.0), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(r);
                    let p = ui.painter();
                    p.rect(rect, 8, if resp.hovered() { t.surface_hover } else { t.surface }, Stroke::new(1.0_f32, if resp.hovered() { tint(t.surface, t.accent, 0.6) } else { t.border }), StrokeKind::Inside);
                    if resp.has_focus() { p.rect_stroke(rect.expand(1.0), 9, Stroke::new(2.0_f32, t.accent), StrokeKind::Outside); }
                    p.text(egui::pos2(rect.min.x + 14.0, rect.center().y), Align2::LEFT_CENTER, ph::FILE_TEXT, egui::FontId::new(22.0, egui::FontFamily::Proportional), if modified.is_some() { t.accent } else { t.text_faint });
                    let text_w = rect.width() - 62.0;
                    let mut name = egui::text::LayoutJob::simple_singleline(file_name(path), ts(SEMI).resolve(ui.style()), t.text);
                    name.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                    let g = ui.painter().layout_job(name);
                    ui.painter().galley(egui::pos2(rect.min.x + 48.0, rect.min.y + 9.0), g, t.text);
                    let sub = match modified { Some(m) => format!("Modified {}", ago(m)), None => "File not found".into() };
                    let mut job = egui::text::LayoutJob::simple_singleline(sub, ts(CAPTION).resolve(ui.style()), t.text_muted);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(rect.min.x + 48.0, rect.min.y + 29.0), g, t.text_muted);
                    if resp.clicked() { to_open = Some(r.clone()); }
                }
            }
        });
        if let Some(r) = to_open { self.open(Path::new(&r)); }
    }

    /// First-run card shown over the calendar while the budget has no payments.
    fn onboarding_card(&mut self, ctx: &egui::Context, area: egui::Rect) {
        let t = self.t.clone();
        let (mut add, mut open) = (false, false);
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
                        ui.horizontal(|ui| {
                            let w = ui.available_width();
                            ui.add_space(((w - 330.0) / 2.0).max(0.0));
                            if theme::primary_button(ui, format!("{}  New payment (Ctrl+N)", ph::PLUS), &t).clicked() { add = true; }
                            if theme::secondary_button(ui, "Open a budget", &t).clicked() { open = true; }
                        });
                    });
                    ui.add_space(SP2);
                    ui.separator();
                    for (n, text) in ["Add your salary", "Add recurring bills and loans", "Watch the balance day by day"].iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (c, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                            ui.painter().circle_filled(c.center(), 10.0, tint(t.surface, t.accent, 0.14));
                            ui.painter().text(c.center(), Align2::CENTER_CENTER, (n + 1).to_string(), ts(SEMI).resolve(ui.style()), t.accent);
                            ui.label(RichText::new(*text).color(t.text));
                        });
                    }
                });
            });
        if add { self.new_payment(self.today); }
        if open { self.open_dialog(); }
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        let Some(msg) = self.banner.clone() else { return };
        let t = self.t.clone();
        Frame::new().fill(tint(t.surface, t.negative, 0.1)).stroke(Stroke::new(1.0_f32, tint(t.surface, t.negative, 0.4))).corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(ph::WARNING_CIRCLE).text_style(ts(ICON)).color(t.negative));
                ui.add(egui::Label::new(RichText::new(msg).color(t.text)).wrap());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::new(RichText::new(ph::X).text_style(ts(ICON)).color(t.text_muted)).frame(false)).clicked() { self.banner = None; }
                });
            });
        });
        ui.add_space(SP2);
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
        let Some((path, pw, err)) = &mut self.pw_prompt else { return };
        let t = &self.t;
        let mut done: Option<Option<String>> = None;
        let name = file_name(path);
        egui::Modal::new(egui::Id::new("password")).frame(Frame::new().fill(t.surface).stroke(Stroke::new(1.0_f32, t.border)).corner_radius(theme::RADIUS).inner_margin(SP4).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new("Password required").text_style(ts(TITLE)));
            ui.label(RichText::new(format!("{name} is encrypted.")).color(t.text_muted));
            ui.add_space(SP2);
            let r = ui.add(TextEdit::singleline(pw).password(true).desired_width(f32::INFINITY).hint_text("Password"));
            if ctx.memory(|m| m.focused()) != Some(r.id) && pw.is_empty() { r.request_focus(); }
            if !err.is_empty() { ui.label(RichText::new(err.as_str()).color(t.negative)); }
            ui.add_space(SP2);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let unlock = Button::new(RichText::new("Unlock").color(theme::on(t.accent))).fill(t.accent).stroke(Stroke::NONE);
                if ui.add(unlock).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) { done = Some(Some(pw.clone())); }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) { done = Some(None); }
            });
        });
        match done {
            Some(Some(p)) => {
                if p.is_empty() { err.clear(); err.push_str("Empty password!"); return; }
                let path = path.clone();
                let text = std::fs::read(&path).map(|b| bgt::latin1_decode(&b)).unwrap_or_default();
                if bgt::is_decryptable(&text, &p) {
                    self.pw_prompt = None;
                    self.load(&path, Some(&p));
                } else {
                    err.clear();
                    err.push_str("The password appears to be invalid.");
                }
            }
            Some(None) => self.pw_prompt = None,
            None => {}
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
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

    fn has_draft(&self) -> bool { self.payment_editor.is_some() || self.transaction_editor.is_some() || self.balance_editor.is_some() }

    fn has_modal(&self) -> bool { self.show_about || self.show_legend || self.show_shortcuts ||self.has_draft() || self.confirm_delete.is_some() || self.pending_action.is_some() || self.pw_prompt.is_some() || self.day_dialog.is_some() }

    fn changed(&mut self) {
        self.dirty = true;
        self.title_dirty = true;
        self.view = None;
        if let Some(db) = &mut self.db { document::rebuild(db, &self.cur, self.cfg.future_n_months, self.cfg.unlimited_history); }
    }

    fn new_payment(&mut self, date: i32) {
        if self.db.is_some() && !self.has_draft() { self.payment_editor = Some(PaymentEditor::new(date)); }
    }

    fn edit_payment(&mut self, id: i32) {
        if let Some(db) = &self.db {
            if let Some(i) = db.get_by_id(id) { self.payment_editor = Some(PaymentEditor::edit(&db.payments[i])); }
        }
    }

    fn edit_transaction(&mut self, id: i32, date: i32, amount: f64) {
        if let Some(db) = &self.db {
            if let Some(i) = db.get_by_id(id) { self.transaction_editor = Some(TransactionEditor::new(&db.payments[i], date, amount)); }
        }
    }

    fn apply_transaction_edit(&mut self, id: i32, date: i32, amount: f64, note: &str, flag: char, scope: TransactionScope) {
        let Some(db) = &mut self.db else { return };
        let result = match scope {
            TransactionScope::Occurrence => document::change_transaction(db, id, date, amount, note, flag),
            TransactionScope::Future => document::change_future_transactions(db, id, date, amount, note, flag),
        };
        match result {
            Ok(()) => {
                self.transaction_editor = None;
                self.day_dialog = None;
                self.changed();
                self.notify(if scope == TransactionScope::Future {
                    "This and future amounts updated. Press Ctrl+S to save."
                } else { "Transaction applied. Press Ctrl+S to save." });
            }
            Err(error) => { if let Some(editor) = &mut self.transaction_editor { editor.set_error(error); } }
        }
    }

    fn edit_balance(&mut self, date: i32) {
        if self.has_draft() { return; }
        let Some(db) = &mut self.db else { return };
        let cal = Cal::from_int(date);
        if db.get_day(&cal).is_none() {
            document::rebuild(db, &cal, self.cfg.future_n_months, self.cfg.unlimited_history);
            self.view = None;
        }
        if let Some(day) = db.get_day(&cal) {
            self.balance_editor = Some(BalanceEditor::new(date, day.closing_balance, day.closing_balance_adjusted));
        } else { self.fail("This date is outside the loaded budget calendar."); }
    }

    fn apply_balance_edit(&mut self, date: i32, amount: Option<f64>) {
        let Some(db) = &mut self.db else { return };
        match document::set_balance(db, date, amount) {
            Ok(changed) => {
                self.balance_editor = None;
                if changed { self.changed(); self.notify("Balance updated. Press Ctrl+S to save."); }
            }
            Err(error) => { if let Some(editor) = &mut self.balance_editor { editor.set_error(error); } }
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
        match action {
            calendar::Action::Filter(f) => self.apply_filter(&f),
            calendar::Action::Add(date) => self.new_payment(date),
            calendar::Action::Day(date) => self.day_dialog = Some(date),
            calendar::Action::EditPayment(id) => self.edit_payment(id),
            calendar::Action::EditTransaction { id, date, amount } => self.edit_transaction(id, date, amount),
            calendar::Action::DeletePayment(id) => self.confirm_delete = Some(DeleteTarget { id, date: None }),
            calendar::Action::DeleteTransaction { id, date } => self.confirm_delete = Some(DeleteTarget { id, date: Some(date) }),
            calendar::Action::EditBalance(date) => self.edit_balance(date),
            calendar::Action::MoveTransaction { id, from, to, amount } => {
                let Some(db) = &mut self.db else { return };
                match document::move_transaction(db, id, from, to, amount) {
                    Ok(true) => {
                        self.cur = Cal::from_int(to); self.cur.set_day_one();
                        self.changed();
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
        if let Some(action) = self.payment_list.show(ui, db, &self.t, self.today) { self.calendar_action(action); }
    }

    fn day_window(&mut self, ctx: &egui::Context) {
        if self.has_draft() || self.pending_action.is_some() || self.confirm_delete.is_some() { return; }
        let Some(date) = self.day_dialog else { return };
        let Some(db) = &mut self.db else { return };
        let transactions = db.get_day(&Cal::from_int(date)).map(|d| d.transactions.clone()).unwrap_or_default();
        let t = self.t.clone();
        let mut open = true;
        let mut action = None;
        egui::Modal::new(egui::Id::new("day_transactions")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.screen_rect().width() - 80.0));
            let cal = Cal::from_int(date);
            ui.heading(format!("{} {}, {}", MONTHS_LONG[cal.month() as usize], cal.day(), cal.year()));
            ui.label(RichText::new(format!("{} transactions", transactions.len())).color(t.text_muted));
            ui.add_space(SP3);
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 250.0).max(140.0)).show(ui, |ui| {
                for tr in transactions {
                    let p = &db.payments[tr.pmt];
                    ui.push_id(p.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&p.description).text_style(ts(SEMI))).truncate());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(money(tr.amount, true)).text_style(ts(SEMI)).color(t.signed(tr.amount)));
                            });
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Edit this date").clicked() { action = Some(calendar::Action::EditTransaction { id:p.id, date, amount:tr.amount }); }
                            ui.menu_button("More", |ui| {
                                if ui.button("Edit whole payment").clicked() { action = Some(calendar::Action::EditPayment(p.id)); ui.close_menu(); }
                                if ui.button(RichText::new("Delete transaction").color(t.negative)).clicked() { action = Some(calendar::Action::DeleteTransaction { id:p.id, date }); ui.close_menu(); }
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
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { open = false; }
        });
        if !open { self.day_dialog = None; }
        if let Some(action) = action { self.calendar_action(action); }
    }

    fn editors(&mut self, ctx: &egui::Context) {
        if self.pending_action.is_some() || self.confirm_delete.is_some() { return; }
        if let (Some(editor), Some(db)) = (&mut self.payment_editor, &self.db) {
            let id = editor.payment_id;
            match editor.show(ctx, &self.t, db) {
                PaymentAction::None => {},
                PaymentAction::Cancel => self.payment_editor = None,
                PaymentAction::Apply(p) => {
                    let start = p.start_date;
                    match document::put_payment(self.db.as_mut().unwrap(), p, id) {
                        Ok(id) => {
                            if self.payment_editor.as_ref().is_some_and(|e| e.payment_id.is_none()) {
                                self.cur = Cal::from_int(start); self.cur.set_day_one();
                            }
                            self.payment_list.selected = Some(id);
                            self.payment_editor = None;
                            self.day_dialog = None;
                            self.changed();
                            self.notify("Payment applied. Press Ctrl+S to save.");
                        }
                        Err(e) => { if let Some(editor) = &mut self.payment_editor { editor.set_error(e); } },
                    }
                }
            }
        }
        if let Some(editor) = &mut self.transaction_editor {
            let (id, date) = (editor.payment_id, editor.date);
            match editor.show(ctx, &self.t) {
                TransactionAction::None => {},
                TransactionAction::Cancel => self.transaction_editor = None,
                TransactionAction::Delete => self.confirm_delete = Some(DeleteTarget { id, date: Some(date) }),
                TransactionAction::Apply { amount, note, flag, scope } => self.apply_transaction_edit(id, date, amount, &note, flag, scope),
            }
        }
        if let Some(editor) = &mut self.balance_editor {
            let date = editor.date;
            match editor.show(ctx, &self.t) {
                BalanceAction::None => {},
                BalanceAction::Cancel => self.balance_editor = None,
                BalanceAction::Apply(amount) => self.apply_balance_edit(date, Some(amount)),
                BalanceAction::Remove => self.apply_balance_edit(date, None),
            }
        }
    }

    fn perform_pending(&mut self, action: PendingAction, ctx: &egui::Context) {
        self.pending_action = None;
        match action {
            PendingAction::Open(path) => self.open_unchecked(&path),
            PendingAction::Close => { self.allow_close = true; ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        }
    }

    fn confirmations(&mut self, ctx: &egui::Context) {
        let t = self.t.clone();
        if let Some(action) = self.pending_action.clone() {
            let draft = self.has_draft();
            let mut choice = 0;
            egui::Modal::new(egui::Id::new("unsaved")).frame(t.card().inner_margin(24)).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.heading(if draft { "Finish your edit" } else { "Unsaved changes" });
                ui.add_space(SP2);
                ui.label(RichText::new(if draft { "Return to the editor to apply your changes, or discard this draft before continuing." }
                    else { "Save your changes before leaving this budget." }).color(t.text_muted));
                if let Some(path) = &self.file { ui.label(RichText::new(file_name(path)).text_style(ts(SEMI))); }
                if let Some(error) = &self.banner {
                    ui.add_space(SP2);
                    ui.add(egui::Label::new(RichText::new(error).color(t.negative)).wrap());
                    if !draft && theme::secondary_button(ui, "Save a separate copy…", &t).clicked() { choice = 4; }
                }
                ui.add_space(SP4);
                ui.horizontal(|ui| {
                    if draft {
                        if ui.add(Button::new(RichText::new("Discard draft").color(t.negative)).frame(false)).clicked() { choice = 2; }
                    } else if ui.add(Button::new(RichText::new("Discard changes").color(t.negative)).frame(false)).clicked() { choice = 2; }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if draft {
                            if theme::primary_button(ui, "Return to editor", &t).clicked() { choice = 1; }
                        } else {
                            if theme::primary_button(ui, "Save changes", &t).clicked() { choice = 3; }
                            if theme::secondary_button(ui, "Cancel", &t).clicked() { choice = 1; }
                        }
                    });
                });
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { choice = 1; }
            });
            match choice {
                1 => self.pending_action = None,
                2 if draft => {
                    self.payment_editor = None; self.transaction_editor = None; self.balance_editor = None;
                    if !self.dirty { self.perform_pending(action, ctx); }
                }
                2 => self.perform_pending(action, ctx),
                3 => { if self.save() { self.perform_pending(action, ctx); } },
                4 => { if self.save_as() { self.perform_pending(action, ctx); } },
                _ => {},
            }
            return;
        }
        if let Some(target) = self.confirm_delete {
            let payment = self.db.as_ref().and_then(|db| db.get_by_id(target.id).map(|i| &db.payments[i]));
            let description = payment.map(|p| p.description.clone()).unwrap_or_default();
            let loan = payment.is_some_and(|p| p.end_mode() == budget_core::END_MODE_LOAN_PAID);
            let mut choice = 0;
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
                    if theme::danger_button(ui, if target.date.is_some() { "Delete transaction" } else { "Delete payment" }, &t).clicked() { choice = 2; }
                    if theme::secondary_button(ui, "Cancel", &t).clicked() { choice = 1; }
                });
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) { choice = 1; }
            });
            if choice > 0 { self.confirm_delete = None; }
            if choice == 2 {
                match document::delete(self.db.as_mut().unwrap(), target.id, target.date) {
                    Ok(()) => { self.transaction_editor = None; self.payment_editor = None; self.changed(); self.notify("Deleted. Press Ctrl+S to save."); }
                    Err(e) => self.fail(e),
                }
            }
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // F1 toggles the shortcuts sheet; "?" opens it when no text box has focus.
        let help = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1))
            || (!ctx.wants_keyboard_input() && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(s) if s == "?"))));
        if help && !self.has_modal_other_than_shortcuts() { self.show_shortcuts = !self.show_shortcuts; }
        if self.has_modal() { return; }
        if egui::DragAndDrop::payload::<calendar::DraggedTransaction>(ctx).is_some()
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx); self.drag_month_hover = None; return;
        }
        let typing = ctx.wants_keyboard_input();
        let (o, s, f, n, p, pgdn, pgup, home) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::COMMAND, Key::O),
            i.consume_key(Modifiers::COMMAND, Key::S),
            i.consume_key(Modifiers::COMMAND, Key::F),
            i.consume_key(Modifiers::COMMAND, Key::N),
            i.consume_key(Modifiers::COMMAND, Key::P),
            // Alt+Right / Alt+Left mirror Page Down / Page Up and also work while the search box has focus.
            i.consume_key(Modifiers::ALT, Key::ArrowRight) | (!typing && i.consume_key(Modifiers::NONE, Key::PageDown)),
            i.consume_key(Modifiers::ALT, Key::ArrowLeft) | (!typing && i.consume_key(Modifiers::NONE, Key::PageUp)),
            (!typing && i.consume_key(Modifiers::NONE, Key::Home)) | (!typing && i.consume_key(Modifiers::NONE, Key::T)),
        ));
        if o { self.open_dialog(); }
        if s { self.save(); }
        if f { if self.show_payments { self.payment_list.focus_search = true; } else { self.focus_filter = true; } }
        if n { self.new_payment(self.today); }
        if p && self.db.is_some() { self.show_payments = true; self.payment_list.focus_search = true; }
        if pgdn { self.go(1); }
        if pgup { self.go(-1); }
        if home { self.go_today(); }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).find(|p| p.extension().map_or(false, |e| e.eq_ignore_ascii_case("bgt"))));
        if let Some(p) = dropped { self.open(&p); }
    }

    fn has_modal_other_than_shortcuts(&self) -> bool { self.show_about || self.show_legend || self.has_draft() || self.confirm_delete.is_some() || self.pending_action.is_some() || self.pw_prompt.is_some() || self.day_dialog.is_some() }

    /// The keyboard shortcuts sheet (F1 or ?).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts { return; }
        let t = self.t.clone();
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("shortcuts")).frame(t.card().inner_margin(SP4)).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 460.0));
            ui.label(RichText::new("Keyboard shortcuts").text_style(ts(TITLE)));
            ui.add_space(SP2);
            const ROWS: [(&str, &str); 14] = [
                ("Ctrl+N", "New payment"), ("Ctrl+S", "Save changes"), ("Ctrl+O", "Open a budget"),
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
        if close || response.should_close() { self.show_shortcuts = false; }
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
        if close || response.should_close() { self.show_legend = false; }
    }

    fn about(&mut self, ctx: &egui::Context) {
        if !self.show_about { return; }
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
        if close || response.should_close() { self.show_about = false; }
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

fn file_name(p: &Path) -> String { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()) }

fn menu_btn(label: &str, shortcut: &str) -> Button<'static> { Button::new(label.to_string()).shortcut_text(shortcut.to_string()) }
fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> egui::Response { ui.add(menu_btn(label, shortcut)) }

fn icon_button(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(Button::new(RichText::new(icon).text_style(ts(ICON))).min_size(Vec2::new(28.0, 28.0)))
}

impl App {
    fn render(&mut self, ctx: &egui::Context) {
        self.screenshot_step(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && (self.dirty || self.has_draft()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.pending_action.is_none() { self.pending_action = Some(PendingAction::Close); }
        }
        if self.title_dirty {
            self.title_dirty = false;
            let name = self.file.as_deref().map(file_name);
            let title = version::window_title(name.as_deref(), self.dirty);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
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
        if has_db && !self.show_payments && self.show_summary && ctx.screen_rect().width() >= 1480.0 {
            egui::SidePanel::right("summary").resizable(false).exact_width(SUMMARY_W).show_separator_line(false)
                .frame(Frame::new().fill(t.bg).inner_margin(Margin { left: 0, right: 8, top: 8, bottom: 8 }))
                .show(ctx, |ui| self.summary(ui));
        }
        egui::TopBottomPanel::bottom("status").show_separator_line(true)
            .frame(Frame::new().fill(t.surface).inner_margin(Margin::symmetric(16, 3))).show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 16.0;
                ui.horizontal(|ui| {
                    if let Some(db) = &self.db {
                        ui.label(RichText::new(format!("{} {}", db.size(), payment_list::payments_noun(db.size() as usize))).text_style(ts(CAPTION)).color(t.text_muted));
                        if self.filter.is_some() && !self.show_payments {
                            if let Some(view) = &self.view { ui.label(RichText::new(format!("·  {} of {} transactions", view.shown, view.total)).text_style(ts(CAPTION)).color(t.text_muted)); }
                        }
                        ui.label(RichText::new("·").color(t.text_faint));
                        ui.label(RichText::new(if self.dirty { "Changes waiting to be saved" } else { "All changes saved" }).text_style(ts(CAPTION)).color(if self.dirty { t.important } else { t.text_muted }));
                    } else { ui.label(RichText::new("Open a budget to get started").text_style(ts(CAPTION)).color(t.text_muted)); }
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
            let grid_top = ui.cursor().min.y;
            let clicked = calendar::show(ui, &view, &t, self.cfg.first_day_of_week_is_sunday, self.today, self.cfg.show_decimals_in_cv);
            let grid_area = egui::Rect::from_min_max(egui::pos2(ui.max_rect().min.x, grid_top), ui.max_rect().max);
            self.view = Some(view);
            if self.db.as_ref().is_some_and(|d| d.payments.is_empty()) && !self.has_modal() { onboarding = Some(grid_area); }
            if let Some(action) = balance_action.or(clicked) { self.calendar_action(action); }
        });
        if let Some(area) = onboarding { self.onboarding_card(ctx, area); }
        if self.show_about {
            self.about(ctx);
        } else if self.show_legend {
            self.legend(ctx);
        } else if self.show_shortcuts && !self.has_modal_other_than_shortcuts() {
            self.shortcuts(ctx);
        } else {
            self.day_window(ctx);
            self.editors(ctx);
            self.confirmations(ctx);
            self.password_prompt(ctx);
        }
        // drag-over hint
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let r = ctx.screen_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
            p.rect(r.shrink(SP3), theme::RADIUS, alpha(t.accent, 10), Stroke::new(2.0_f32, t.accent), StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, "Drop to open", ts(TITLE).resolve(&ctx.style()), t.accent);
        }
        self.toast(ctx);
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
    let cli = parse_cli();
    let size = cli.size.unwrap_or([1280.0, 840.0]);
    let maximized = cli.size.is_none();
    let icon = image::load_from_memory(include_bytes!("../assets/budget-calendar.png")).ok().map(|img| {
        let rgba = img.into_rgba8();
        egui::IconData { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size).with_min_inner_size([900.0, 600.0]).with_maximized(maximized).with_title(version::window_title(None, false))
            .with_icon(icon.unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native("Budget Calendar", options, Box::new(|cc| {
        theme::install_fonts(&cc.egui_ctx);
        let app = App::new(cli);
        theme::apply(&cc.egui_ctx, &app.t);
        Ok(Box::new(app))
    }))
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
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> (App, PathBuf) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/lifecycle-tests")
            .join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = BudgetConfig::default();
        let t = Tokens::new(&cfg.colors, false);
        let cur = Cal::from_int(20261001);
        let mut db = BudgetDb::new(&cur, 12, false);
        let mut p = budget_core::Payment::new(20261027); p.description = "Fixture payment".into(); p.amount = -12.0;
        db.add(p);
        let app = App {
            cfg, rc_path: dir.join(".budgetrc"), db: Some(db), file: Some(dir.join("current.bgt")), password: None,
            saved_bytes: Some(b"previous budget".to_vec()), dirty: true, payment_editor: None, transaction_editor: None,
            balance_editor: None, drag_month_hover: None,
            show_payments: false, payment_list: payment_list::PaymentList::default(), day_dialog: None,
            confirm_delete: None, pending_action: None, allow_close: false, cur, today: 20261027,
            filter_text: String::new(), filter: None, filter_error: None, view: None, pw_prompt: None, toast: None, banner: None,
            screenshot: None, frames: 0, dark: false, show_summary: true, show_about: false, show_legend: false, show_shortcuts: false, focus_filter: false, title_dirty: true, t,
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
        assert!(matches!(app.pending_action, Some(PendingAction::Open(_))));
        assert_eq!(app.file.as_deref(), Some(dir.join("current.bgt").as_path()));
        assert_eq!(app.db.as_ref().unwrap().payments[0].amount, -12.0);
        assert!(app.dirty);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_a_file_with_only_an_unapplied_draft_also_defers() {
        let (mut app, dir) = fixture();
        app.dirty = false;
        app.payment_editor = Some(PaymentEditor::new(20261027));
        app.open(&dir.join("other.bgt"));
        assert!(app.pending_action.is_some());
        assert!(app.payment_editor.is_some());
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
        assert_eq!(app.balance_editor.as_ref().unwrap().date, 20261027);
        app.open(&dir.join("other.bgt"));
        assert!(app.pending_action.is_some());
        assert!(app.balance_editor.is_some());
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
        assert!(app.balance_editor.is_none());
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
        assert!(app.balance_editor.is_some());
        assert!(!app.dirty);
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
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
            document::rebuild(db, &app.cur, 12, false);
            app.dirty = false;
            app.edit_transaction(id, 20261027, -150.0);
            let scope = if future { TransactionScope::Future } else { TransactionScope::Occurrence };
            app.apply_transaction_edit(id, 20261027, -250.0, "This date only", budget_core::FLAG_PAID, scope);
            assert!(app.dirty);
            assert!(app.transaction_editor.is_none());
            let saved = dir.join("scoped-amount.bgt");
            assert!(app.save_to(&saved, None));
            assert!(!app.dirty);
            let loaded = bgt::load_file(&saved, app.today, None).unwrap();
            let mut restored = document::from_loaded(loaded, &app.cur, 12, true);
            for (date, expected) in [(20260927, -150.0), (20261027, -250.0),
                (20261127, if future { -250.0 } else { -150.0 }),
                (20261227, if future { -250.0 } else { -150.0 })] {
                let actual = restored.get_day(&Cal::from_int(date)).unwrap().transactions[0].amount;
                assert_eq!(actual, expected, "scope {scope:?}, date {date}");
            }
            assert_eq!(restored.payments[0].get_note(20261027).as_deref(), Some("This date only"));
            assert!(restored.payments[0].get_note(20261127).is_none());
            assert_eq!(restored.payments[0].get_flag(20261127), budget_core::FLAG_NONE);
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
        assert!(app.transaction_editor.is_some());
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
        assert!(app.show_about, "the version button must open the history dialog");
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
        app.show_about = true;
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
        assert!(!app.show_about);
        assert!(app.balance_editor.is_some(), "closing history must retain an unfinished balance edit");
        assert!(!app.dirty);
        assert_eq!(app.db.as_ref().unwrap().to_text(None).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
