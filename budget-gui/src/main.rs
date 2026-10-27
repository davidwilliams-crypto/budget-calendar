#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Budget Calendar main window: month navigation, quick stats, brief summary, calendar grid.

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::numfmt::format_frac2;
use budget_core::{bgt, stats, BudgetDb, Cal, Field, FLAG_IMPORTANT, FLAG_PAID};
use eframe::egui::{self, Color32, Frame, RichText, Stroke, Vec2};
use std::path::{Path, PathBuf};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn rgb(c: u32) -> Color32 { Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8) }

/// `#,##0` or `#,##0.00` depending on the "show decimals" preference.
fn money(v: f64, decimals: bool) -> String {
    if decimals { return format_frac2(v); }
    let s = format_frac2(v);
    s[..s.len() - 3].to_string()
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

struct TranRow { text: String, amount: String, fg: Color32, bg: Option<Color32>, amount_fg: Color32 }
struct CellData { day: i32, date: i32, in_month: bool, is_today: bool, rows: Vec<TranRow>, balance: Option<(String, Color32, Color32)>, has_note: bool, restorable: bool }
struct QuickStats { opening: f64, closing: f64, min: f64, max: f64, avg: f64, income: f64, expenses: f64 }
struct BriefRow { desc: String, amount: f64, pct: f64, diff: Option<f64>, bold: bool }
struct MonthView { key: (i32, String, i32), cells: Vec<CellData>, quick: QuickStats, brief: Vec<BriefRow> }

struct App {
    cfg: BudgetConfig,
    rc_path: PathBuf,
    db: Option<BudgetDb>,
    file: Option<PathBuf>,
    password: Option<String>,
    /// First day of the viewed month.
    cur: Cal,
    today: i32,
    filter_text: String,
    filter: Option<Filter>,
    view: Option<MonthView>,
    pw_prompt: Option<(PathBuf, String, String)>,
    status: String,
}

impl App {
    fn new() -> App {
        let rc_path = home_dir().join(".budgetrc");
        let cfg = BudgetConfig::load(&rc_path);
        let mut cur = Cal::today();
        cur.set_day_one();
        let mut app = App {
            cfg, rc_path, db: None, file: None, password: None, cur, today: Cal::today().get(),
            filter_text: String::new(), filter: None, view: None, pw_prompt: None, status: String::new(),
        };
        let start = std::env::args().nth(1).or_else(|| app.cfg.last_file_name.clone());
        if let Some(f) = start {
            let p = PathBuf::from(f.trim());
            if p.is_file() { app.open(&p); }
        }
        app
    }

    fn open(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) if bgt::is_encrypted_text(&bgt::latin1_decode(&bytes)) => {
                self.pw_prompt = Some((path.to_path_buf(), String::new(), String::new()));
            }
            Ok(_) => self.load(path, None),
            Err(e) => self.status = format!("Unable to open '{}' file\n{e}", path.display()),
        }
    }

    fn load(&mut self, path: &Path, password: Option<&str>) {
        match BudgetDb::load(path, password, &self.cur, self.cfg.future_n_months, self.cfg.unlimited_history) {
            Ok(db) => {
                self.db = Some(db);
                self.file = Some(path.to_path_buf());
                self.password = password.map(str::to_string);
                self.cfg.last_file_name = Some(path.display().to_string());
                self.cfg.add_recent_file(&path.display().to_string());
                let _ = self.cfg.save(&self.rc_path);
                self.view = None;
                self.status = format!("Loaded {}", path.display());
            }
            Err(log) => self.status = log,
        }
    }

    fn save(&mut self) {
        let (Some(db), Some(path)) = (&self.db, &self.file) else { return };
        self.status = match db.save(path, self.password.as_deref()) {
            Ok(()) => format!("Saved {}", path.display()),
            Err(e) => e,
        };
    }

    fn go(&mut self, months: i32) {
        self.cur.add(Field::Month, months);
        self.cur.set_day_one();
        if let Some(db) = &mut self.db {
            if !db.is_month_available(&self.cur) {
                db.rebuild_db(&self.cur, self.cfg.future_n_months, self.cfg.unlimited_history);
            }
        }
    }

    fn set_filter(&mut self) {
        let t = self.filter_text.trim().replace('&', "\u{ff06}");
        self.filter = if t.is_empty() { None } else { Filter::new(&t, self.today).ok() };
    }

    fn ensure_view(&mut self) {
        let Some(db) = &mut self.db else { self.view = None; return };
        let key = (self.cur.get(), self.filter_text.clone(), db.update_count);
        if self.view.as_ref().map_or(false, |v| v.key == key) { return; }
        let cfg = &self.cfg;
        let c = &cfg.colors;
        let decimals = cfg.show_decimals_in_cv;
        let filter = self.filter.as_ref();
        let n_days = self.cur.max_day();
        // month extremes first
        let mut closings = Vec::new();
        for d in 1..=n_days {
            let mut cal = self.cur;
            cal.set(Field::Day, d);
            if let Some(day) = db.get_day(&cal) { closings.push(day.closing_balance); }
        }
        let min = closings.iter().cloned().fold(f64::MAX, f64::min);
        let max = closings.iter().cloned().fold(f64::MIN, f64::max);
        let first_dow = self.cur.day_of_week();
        let lead = if cfg.first_day_of_week_is_sunday { first_dow } else { (first_dow + 6) % 7 };
        let mut cells = Vec::new();
        let total = lead + n_days;
        let trail = (7 - total % 7) % 7;
        for i in 0..(total + trail) {
            let mut cal = self.cur;
            cal.add(Field::Day, i - lead);
            let in_month = i >= lead && i < total;
            let date = cal.get();
            let mut cell = CellData { day: cal.day(), date, in_month, is_today: date == self.today, rows: Vec::new(), balance: None, has_note: false, restorable: false };
            if in_month {
                if let Some(day) = db.get_day(&cal) {
                    let trans: Vec<(usize, f64, char)> = day.transactions.iter().map(|t| (t.pmt, t.amount, t.suffix)).collect();
                    let closing = day.closing_balance;
                    let adjusted = day.closing_balance_adjusted;
                    cell.restorable = !day.restorable.is_empty();
                    cell.has_note = db.day_note(&cal).is_some();
                    for (pi, amount, suffix) in trans {
                        let p = &db.payments[pi];
                        if let Some(f) = filter { if !f.is_match(p, amount, date) { continue; } }
                        let flag = p.get_flag(date);
                        let hi = p.get_hilighting(date);
                        let hi = if hi == -1 { p.default_hilighting() } else { hi };
                        let bg = if p.disabled { None }
                            else if flag == FLAG_PAID { Some(rgb(c.bgColorPaid)) }
                            else if flag == FLAG_IMPORTANT { Some(rgb(c.bgColorImportant)) }
                            else if hi != -1 { Some(rgb(hi as u32)) }
                            else { None };
                        let mut text = p.short_description().to_string();
                        if p.get_note(date).is_some() { text.push_str(" ..."); }
                        let mut amt = money(amount, decimals);
                        if suffix != '\0' { amt.push(suffix); }
                        cell.rows.push(TranRow {
                            text,
                            amount: amt,
                            fg: rgb(if p.disabled { c.colorDisabledPayment } else { c.colorDescText }),
                            bg,
                            amount_fg: rgb(if p.disabled { c.colorDisabledPayment } else if amount < 0.0 { c.colorNegTranAmount } else { c.colorPosTranAmount }),
                        });
                    }
                    let bar = if closing == max { c.bgColorMaxBalance }
                        else if closing == min { c.bgColorMinBalance }
                        else if closing < cfg.min_balance_4_max_saving { c.bgColorBelowMinBalance }
                        else { c.bgColorBalance };
                    let mut txt = money(closing, decimals);
                    if adjusted { txt.push('\''); }
                    let fg = if closing < 0.0 { c.colorNegativeAmount } else { c.colorPositiveAmount };
                    cell.balance = Some((txt, rgb(bar), rgb(fg)));
                }
            }
            cells.push(cell);
        }
        let st = stats::calc_statistics(db, &self.cur, 1, false, filter);
        let mut first = self.cur;
        let mut last = self.cur;
        last.set_day_last();
        let opening = db.get_day(&mut first).map_or(0.0, |d| d.opening_balance);
        let closing = db.get_day(&last).map_or(0.0, |d| d.closing_balance);
        let avg = if closings.is_empty() { 0.0 } else { closings.iter().sum::<f64>() / closings.len() as f64 };
        let quick = QuickStats { opening, closing, min, max, avg, income: st.total_income, expenses: st.total_expense };
        // brief summary: rows >= 5% of total income, the rest as "Other..."
        let threshold = st.total_income.abs() * 0.05;
        let mut brief = vec![
            BriefRow { desc: "Disposable Income".into(), amount: st.total_income + st.total_expense, pct: 0.0, diff: None, bold: false },
            BriefRow { desc: "Total Income".into(), amount: st.total_income, pct: 100.0, diff: None, bold: true },
        ];
        let mut other = 0.0;
        for r in st.income.iter().chain(st.expense.iter()).filter(|r| !r.is_sub) {
            if r.amount.abs() >= threshold {
                let diff = r.amount - r.planned_amount;
                brief.push(BriefRow { desc: r.key.clone(), amount: r.amount, pct: if st.total_income != 0.0 { r.amount / st.total_income * 100.0 } else { 0.0 }, diff: if diff.abs() < 5.0 { None } else { Some(diff) }, bold: false });
            } else {
                other += r.amount;
            }
        }
        if other != 0.0 {
            brief.push(BriefRow { desc: "Other...".into(), amount: other, pct: if st.total_income != 0.0 { other / st.total_income * 100.0 } else { 0.0 }, diff: None, bold: false });
        }
        if st.total_income != 0.0 { brief[0].pct = brief[0].amount / st.total_income * 100.0; }
        self.view = Some(MonthView { key, cells, quick, brief });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open...").clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).pick_file() { self.open(&p); }
                }
                if ui.add_enabled(self.db.is_some(), egui::Button::new("Save")).clicked() { ui.close_menu(); self.save(); }
                if ui.add_enabled(self.db.is_some(), egui::Button::new("Save as...")).clicked() {
                    ui.close_menu();
                    if let Some(p) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).save_file() { self.file = Some(p); self.save(); }
                }
                ui.separator();
                let recent: Vec<String> = self.cfg.recent_file.clone();
                for r in recent {
                    if ui.button(&r).clicked() { ui.close_menu(); self.open(Path::new(&r)); }
                }
                ui.separator();
                if ui.button("Exit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    ui.close_menu();
                    self.status = "Budget Calendar (Rust port) — engine budget-core".into();
                }
            });
        });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let c = self.cfg.colors.clone();
        let decimals = true;
        let link_color = rgb(c.colorCurrentMonth);
        // month links row
        ui.horizontal(|ui| {
            let year = self.cur.year();
            let m0 = self.cur.month();
            let mut jump: Option<i32> = None;
            for i in 0..12 {
                let m = (m0 + i) % 12;
                let text = RichText::new(MONTHS[m as usize]).color(link_color);
                let text = if i == 0 { text.strong().underline() } else { text };
                if ui.add(egui::Label::new(text).sense(egui::Sense::click())).clicked() { jump = Some(i); }
            }
            if let Some(i) = jump { self.go(i); }
            ui.separator();
            if ui.button("◀").clicked() { self.go(-1); }
            ui.label(RichText::new(format!("{},{}", MONTHS_LONG[m0 as usize], year)).size(22.0).color(link_color));
            if ui.button("▶").clicked() { self.go(1); }
            ui.separator();
            ui.label("Filter:");
            let r = ui.add(egui::TextEdit::singleline(&mut self.filter_text).desired_width(220.0).hint_text("e.g. cat:utils <-100"));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.set_filter(); }
            if ui.button("Clear").clicked() { self.filter_text.clear(); self.set_filter(); }
        });
        self.ensure_view();
        let mut clicked: Option<String> = None;
        let Some(view) = &self.view else { return };
        ui.horizontal_top(|ui| {
            // quick stats
            egui::Grid::new("quick").num_columns(2).spacing([12.0, 2.0]).striped(true).show(ui, |ui| {
                let q = &view.quick;
                let row = |ui: &mut egui::Ui, label: &str, v: f64| {
                    ui.label(RichText::new(label).strong());
                    let col = if v < 0.0 { c.colorNegativeAmount } else { c.colorPositiveAmount };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(money(v, decimals)).monospace().color(rgb(col)));
                    });
                    ui.end_row();
                };
                row(ui, "Opening Balance:", q.opening);
                row(ui, "Closing Balance:", q.closing);
                row(ui, "Minimum Balance:", q.min);
                row(ui, "Avg Daily Balance:", q.avg);
                row(ui, "Maximum Balance:", q.max);
                row(ui, "Total Income:", q.income);
                row(ui, "Total Expenses:", q.expenses);
                row(ui, "", q.income + q.expenses);
            });
            ui.add_space(24.0);
            // brief summary
            ui.vertical(|ui| {
                ui.label(RichText::new("Brief Summary").strong());
                egui::Grid::new("brief").num_columns(4).spacing([14.0, 2.0]).striped(true).show(ui, |ui| {
                    for (h, _) in [("Description", 0), ("Amount", 1), ("%", 2), ("+/-", 3)] {
                        ui.label(RichText::new(h).strong().color(rgb(c.colorCalendarHeader)).background_color(rgb(c.bgColorCalendarHeader)));
                    }
                    ui.end_row();
                    for r in &view.brief {
                        let t = RichText::new(&r.desc);
                        let t = if r.bold { t.strong() } else { t };
                        if ui.add(egui::Label::new(t).sense(egui::Sense::click())).clicked() { clicked = Some(r.desc.clone()); }
                        let col = if r.amount < 0.0 { c.colorNegativeAmount } else { c.colorPositiveAmount };
                        ui.label(RichText::new(money(r.amount, false)).monospace().color(rgb(col)));
                        ui.label(RichText::new(format!("{:.0}", r.pct)).monospace());
                        ui.label(RichText::new(r.diff.map_or(String::new(), |d| money(d, false))).monospace());
                        ui.end_row();
                    }
                });
            });
        });
        if let Some(d) = clicked {
            self.filter_text = match d.as_str() {
                "Total Income" => ">0".into(),
                "Disposable Income" => String::new(),
                d if d.starts_with("   ") => format!("desc:{}", d.trim().replace(' ', "\\ ")),
                d => format!("cat:{}", d.replace(' ', "\\ ")),
            };
            self.set_filter();
        }
    }

    fn calendar(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &self.view else {
            ui.label("Open a .bgt file (File → Open...) to see the calendar.");
            return;
        };
        let c = self.cfg.colors.clone();
        let avail = ui.available_width();
        let cell_w = (avail - 6.0 * 4.0) / 7.0;
        let rows = view.cells.len() / 7;
        let cell_h = ((ui.available_height() - 30.0) / rows as f32).max(90.0);
        let sunday_first = self.cfg.first_day_of_week_is_sunday;
        egui::Grid::new("cal").num_columns(7).spacing([4.0, 4.0]).min_col_width(cell_w).max_col_width(cell_w).show(ui, |ui| {
            for i in 0..7 {
                let w = if sunday_first { i } else { (i + 1) % 7 };
                Frame::new().fill(rgb(c.bgColorCalendarHeader)).inner_margin(4).show(ui, |ui| {
                    ui.set_width(cell_w - 8.0);
                    ui.vertical_centered(|ui| { ui.label(RichText::new(WDAYS[w]).strong().size(16.0).color(rgb(c.colorCalendarHeader))); });
                });
            }
            ui.end_row();
            for (i, cell) in view.cells.iter().enumerate() {
                let bg = if !cell.in_month { c.bgColorCellOtherMonths }
                    else if cell.is_today { c.bgColorCellToday }
                    else if cell.date < self.today { c.bgColorCell }
                    else { c.bgColorCellPast };
                Frame::new().fill(rgb(bg)).stroke(Stroke::new(1.0_f32, Color32::from_gray(200))).inner_margin(3).show(ui, |ui| {
                    ui.set_min_size(Vec2::new(cell_w - 8.0, cell_h));
                    ui.set_max_width(cell_w - 8.0);
                    ui.vertical(|ui| {
                        let mut num = cell.day.to_string();
                        if cell.restorable { num.push('`'); }
                        if cell.has_note { num.push_str(" ..."); }
                        let num_col = if !cell.in_month { Color32::GRAY } else if cell.is_today { rgb(c.colorCalendarToday) } else { rgb(c.colorCalendarDay) };
                        ui.label(RichText::new(num).strong().size(15.0).color(num_col));
                        for r in &cell.rows {
                            ui.horizontal(|ui| {
                                let t = RichText::new(&r.text).color(r.fg);
                                let t = if let Some(bg) = r.bg { t.background_color(bg) } else { t };
                                ui.add(egui::Label::new(t).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(&r.amount).monospace().color(r.amount_fg));
                                });
                            });
                        }
                        if let Some((txt, bar, fg)) = &cell.balance {
                            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                Frame::new().fill(*bar).inner_margin(2).show(ui, |ui| {
                                    ui.set_width(cell_w - 14.0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(txt).monospace().italics().color(*fg));
                                    });
                                });
                            });
                        }
                    });
                });
                if i % 7 == 6 { ui.end_row(); }
            }
        });
    }

    fn password_prompt(&mut self, ctx: &egui::Context) {
        let Some((path, pw, err)) = &mut self.pw_prompt else { return };
        let mut done: Option<Option<String>> = None;
        egui::Window::new("Password").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("The budget file is password protected\nEnter the password:");
            let r = ui.add(egui::TextEdit::singleline(pw).password(true));
            r.request_focus();
            if !err.is_empty() { ui.colored_label(Color32::RED, err.as_str()); }
            ui.horizontal(|ui| {
                if ui.button("Ok").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) { done = Some(Some(pw.clone())); }
                if ui.button("Cancel").clicked() { done = Some(None); }
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
                    err.push_str("The password appears to be invalid!");
                }
            }
            Some(None) => self.pw_prompt = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::PageDown)) { self.go(1); }
        if ctx.input(|i| i.key_pressed(egui::Key::PageUp)) { self.go(-1); }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) { let mut t = Cal::today(); t.set_day_one(); self.cur = t; self.go(0); }
        self.password_prompt(ctx);
        let bg = rgb(self.cfg.colors.bgColorWindow);
        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::top("header").frame(Frame::default().fill(bg).inner_margin(8)).show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let title = match &self.file { Some(f) => format!("{} - Budget Calendar", f.display()), None => "Budget Calendar".into() };
            ui.horizontal(|ui| { ui.label(title); ui.separator(); ui.label(&self.status); });
        });
        egui::CentralPanel::default().frame(Frame::default().fill(bg).inner_margin(6)).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.calendar(ui));
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]).with_title("Budget Calendar"),
        ..Default::default()
    };
    eframe::run_native("Budget Calendar", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
