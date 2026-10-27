#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Budget Calendar main window.
//!
//! Layout: header panel (menu row + toolbar), optional right summary panel, central panel with the
//! stats strip and the calendar. `theme.rs` holds the design tokens, `view.rs` the month view-model,
//! `calendar.rs` the painter-drawn grid.

mod calendar;
mod theme;
mod view;

use budget_core::config::BudgetConfig;
use budget_core::filter::Filter;
use budget_core::{bgt, BudgetDb, Cal, Field};
use eframe::egui::{self, Align, Align2, Button, Color32, Frame, Key, Layout, Margin, Modifiers, RichText, Sense, Stroke, StrokeKind, TextEdit, Vec2};
use egui_phosphor::regular as ph;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use calendar::RichTextExt;
use theme::{alpha, tint, ts, Tokens, CAPTION, ICON, SECTION, SEMI, SP1, SP2, SP3, SP4, TITLE, VALUE};
use view::{money, MonthView};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const SUMMARY_W: f32 = 264.0;

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// `budget-gui [file.bgt] [--month yyyymm] [--screenshot out.png] [--size WxH] [--dark] [--today yyyymmdd] [--filter expr]`
#[derive(Default)]
struct Cli { file: Option<String>, month: Option<i32>, screenshot: Option<PathBuf>, size: Option<[f32; 2]>, dark: bool, today: Option<i32>, filter: Option<String> }

fn parse_cli() -> Cli {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let val = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
    Cli {
        file: args.iter().find(|a| a.ends_with(".bgt")).cloned(),
        month: val("--month").and_then(|m| m.parse().ok()),
        screenshot: val("--screenshot").map(PathBuf::from),
        size: val("--size").and_then(|s| { let (w, h) = s.split_once('x')?; Some([w.parse().ok()?, h.parse().ok()?]) }),
        dark: args.iter().any(|a| a == "--dark"),
        today: val("--today").and_then(|t| t.parse().ok()),
        filter: val("--filter"),
    }
}

struct Toast { text: String, error: bool, until: Instant }

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
    toast: Option<Toast>,
    /// Persistent error until dismissed.
    banner: Option<String>,
    /// `--screenshot <png>`: render a few frames, save the window, exit.
    screenshot: Option<PathBuf>,
    frames: u32,
    dark: bool,
    show_summary: bool,
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
            filter_text: String::new(), filter: None, view: None, pw_prompt: None, toast: None, banner: None,
            screenshot: cli.screenshot, frames: 0, dark: cli.dark, show_summary: true, focus_filter: false, title_dirty: true, t,
        };
        if let Some(m) = cli.month { app.cur = Cal::from_int(m); app.cur.set_day_one(); }
        if let Some(f) = cli.filter { app.filter_text = f; app.set_filter(); }
        let start = cli.file.or_else(|| app.cfg.last_file_name.clone());
        if let Some(f) = start {
            let p = PathBuf::from(f.trim());
            if p.is_file() { app.open(&p); app.toast = None; }
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>) { self.toast = Some(Toast { text: text.into(), error: false, until: Instant::now() + Duration::from_secs(4) }); }
    fn fail(&mut self, text: impl Into<String>) { self.banner = Some(text.into()); }

    fn open(&mut self, path: &Path) {
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
        match BudgetDb::load(path, password, &self.cur, self.cfg.future_n_months, self.cfg.unlimited_history) {
            Ok(db) => {
                self.db = Some(db);
                self.file = Some(path.to_path_buf());
                self.password = password.map(str::to_string);
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

    fn save(&mut self) {
        let (Some(db), Some(path)) = (&self.db, &self.file) else { return };
        match db.save(path, self.password.as_deref()) {
            Ok(()) => { let n = file_name(path); self.notify(format!("Saved {n}")); }
            Err(e) => self.fail(e),
        }
    }

    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Budget", &["bgt"]).save_file() { self.file = Some(p); self.title_dirty = true; self.save(); }
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

    fn go_today(&mut self) {
        self.cur = Cal::from_int(self.today);
        self.go(0);
    }

    fn set_filter(&mut self) {
        let t = self.filter_text.trim().replace('&', "\u{ff06}");
        self.filter = if t.is_empty() { None } else { Filter::new(&t, self.today).ok() };
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
        egui::menu::bar(ui, |ui| {
            ui.spacing_mut().button_padding = Vec2::new(8.0, 3.0);
            ui.menu_button("File", |ui| {
                ui.set_min_width(220.0);
                if menu_item(ui, "Open…", "Ctrl+O").clicked() { ui.close_menu(); self.open_dialog(); }
                if ui.add_enabled(self.db.is_some(), menu_btn("Save", "Ctrl+S")).clicked() { ui.close_menu(); self.save(); }
                if ui.add_enabled(self.db.is_some(), menu_btn("Save as…", "")).clicked() { ui.close_menu(); self.save_as(); }
                let recent: Vec<String> = self.cfg.recent_file.clone();
                if !recent.is_empty() {
                    ui.separator();
                    ui.label(RichTextExt::faint("Recent", ui));
                    for r in recent {
                        if ui.button(file_name(Path::new(&r))).on_hover_text(&r).clicked() { ui.close_menu(); self.open(Path::new(&r)); }
                    }
                }
                ui.separator();
                if ui.button("Exit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
            });
            ui.menu_button("View", |ui| {
                ui.set_min_width(200.0);
                if ui.checkbox(&mut self.show_summary, "Summary panel").clicked() { ui.close_menu(); }
                let mut dark = self.dark;
                if ui.checkbox(&mut dark, "Dark theme").clicked() { ui.close_menu(); self.set_dark(ui.ctx(), dark); }
                ui.separator();
                if menu_item(ui, "Today", "Home").clicked() { ui.close_menu(); self.go_today(); }
                if menu_item(ui, "Previous month", "PgUp").clicked() { ui.close_menu(); self.go(-1); }
                if menu_item(ui, "Next month", "PgDn").clicked() { ui.close_menu(); self.go(1); }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() { ui.close_menu(); self.notify("Budget Calendar — Rust port on the budget-core engine"); }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("PgUp · PgDn  month      Home  today      Ctrl+F  filter").text_style(ts(CAPTION)).color(t.text_faint));
                if let Some(f) = &self.file {
                    ui.add_space(SP3);
                    ui.label(RichText::new(file_name(f)).text_style(ts(CAPTION)).color(t.text_muted)).on_hover_text(f.display().to_string());
                }
            });
        });
        ui.add_space(SP1);
        self.toolbar(ui);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let t = self.t.clone();
        const RIGHT_W: f32 = 364.0;
        const CHIPS_W: f32 = 440.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SP1;
            if icon_button(ui, ph::CARET_LEFT).on_hover_text("Previous month  (PgUp)").clicked() { self.go(-1); }
            let title = format!("{} {}", MONTHS_LONG[self.cur.month() as usize], self.cur.year());
            ui.allocate_ui_with_layout(Vec2::new(172.0, 28.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                ui.label(RichText::new(title).text_style(ts(TITLE)).color(t.text));
            });
            if icon_button(ui, ph::CARET_RIGHT).on_hover_text("Next month  (PgDn)").clicked() { self.go(1); }
            ui.add_space(SP2);
            if ui.button("Today").on_hover_text("Home").clicked() { self.go_today(); }
            if ui.available_width() >= RIGHT_W + CHIPS_W {
                ui.add_space(SP3);
                vsep(ui, &t);
                ui.add_space(SP2);
                ui.spacing_mut().button_padding = Vec2::new(5.0, 3.0);
                let m0 = self.cur.month();
                let mut jump = None;
                for i in 0..12 {
                    let m = (m0 + i) % 12;
                    let (fill, col) = if i == 0 { (t.accent, Color32::WHITE) } else { (Color32::TRANSPARENT, t.text_muted) };
                    let label = RichText::new(MONTHS[m as usize]).family(egui::FontFamily::Name("semi".into())).size(12.0).color(col);
                    let b = Button::new(label).fill(fill).stroke(Stroke::NONE).corner_radius(11).min_size(Vec2::new(0.0, 22.0));
                    if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { jump = Some(i); }
                }
                if let Some(i) = jump { self.go(i); }
            }
            let slack = (ui.available_width() - RIGHT_W).max(0.0);
            ui.add_space(slack);
            ui.spacing_mut().item_spacing.x = SP2;
            let has_db = self.db.is_some();
            ui.add_enabled_ui(has_db, |ui| self.filter_box(ui, &t));
            if ui.button(format!("{}  Open", ph::FOLDER_OPEN)).on_hover_text("Ctrl+O").clicked() { self.open_dialog(); }
            if ui.add_enabled(self.db.is_some(), Button::new(format!("{}  Save", ph::FLOPPY_DISK))).on_hover_text("Ctrl+S").clicked() { self.save(); }
        });
    }

    fn filter_box(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let active = self.filter.is_some();
        let stroke = Stroke::new(1.0_f32, if active { t.accent } else { t.border });
        Frame::new().fill(t.surface).stroke(stroke).corner_radius(6).inner_margin(Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_width(200.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SP1;
                ui.label(RichText::new(if active { ph::FUNNEL } else { ph::MAGNIFYING_GLASS }).text_style(ts(ICON)).color(if active { t.accent } else { t.text_muted }));
                let mut w = 200.0 - 18.0 - 22.0;
                let count = match (active, &self.view) { (true, Some(v)) => Some(format!("{}/{}", v.shown, v.total)), _ => None };
                if count.is_some() { w -= 34.0; }
                let r = ui.add(TextEdit::singleline(&mut self.filter_text).frame(false).hint_text("Filter  \u{b7}  cat:Auto  <-100").desired_width(w));
                if self.focus_filter { r.request_focus(); self.focus_filter = false; }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) { self.set_filter(); }
                if let Some(c) = count {
                    ui.label(RichText::new(c).text_style(ts(CAPTION)).color(t.text_muted)).on_hover_text("Transactions shown this month");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !self.filter_text.is_empty() {
                        let x = Button::new(RichText::new(ph::X).text_style(ts(ICON)).color(t.text_muted)).frame(false);
                        if ui.add(x).on_hover_text("Clear filter").clicked() { self.filter_text.clear(); self.set_filter(); }
                    }
                });
            });
        });
    }

    // ---------- body ----------

    fn stats(&self, ui: &mut egui::Ui, v: &MonthView) {
        let t = &self.t;
        let q = &v.quick;
        let net = q.income + q.expenses;
        let tiles: [(&str, f64, Color32, Option<Color32>); 8] = [
            ("Opening", q.opening, t.text, None),
            ("Closing", q.closing, t.text, None),
            ("Income", q.income, t.positive, None),
            ("Expenses", q.expenses, t.negative, None),
            ("Net", net, t.signed(net), None),
            ("Lowest", q.min, if v.below_min { t.low.1 } else { t.text }, Some(if v.below_min { t.low.1 } else { t.min.1 })),
            ("Average", q.avg, t.text, None),
            ("Highest", q.max, t.text, Some(t.max.1)),
        ];
        Frame::new().fill(t.surface).stroke(Stroke::new(1.0_f32, t.border)).corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let n = tiles.len() as f32;
            let gap = SP3;
            let w = ((ui.available_width() - gap * (n - 1.0)) / n).floor();
            let decimals = w >= 104.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (i, (label, value, col, dot)) in tiles.iter().enumerate() {
                    if i > 0 {
                        let x = ui.cursor().min.x - gap / 2.0;
                        ui.painter().vline(x, ui.cursor().y_range().shrink(0.0), Stroke::new(1.0_f32, t.border));
                    }
                    ui.allocate_ui_with_layout(Vec2::new(w, 40.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = SP1;
                            if let Some(d) = dot { ui.painter().circle_filled(ui.cursor().min + Vec2::new(4.0, 8.0), 4.0, *d); ui.add_space(10.0); }
                            ui.label(RichText::new(*label).text_style(ts(CAPTION)).color(t.text_muted));
                        });
                        ui.label(RichText::new(money(*value, decimals)).text_style(ts(VALUE)).color(*col));
                    });
                }
            });
        });
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
                if is_net { ui.add_space(SP1); let r = ui.max_rect(); ui.painter().hline(r.x_range(), ui.cursor().min.y - 2.0, Stroke::new(1.0_f32, t.border)); ui.add_space(SP1); }
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
                let selected = !active.is_empty() && row.filter == active;
                let p = ui.painter();
                if selected { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, tint(t.surface, t.accent, 0.12)); }
                else if resp.hovered() { p.rect_filled(rect.expand2(Vec2::new(6.0, 0.0)), 5, t.surface_hover); }
                let cy = rect.center().y;
                let fid = if row.total { ts(SEMI).resolve(ui.style()) } else { egui::TextStyle::Body.resolve(ui.style()) };
                let mut x = rect.max.x;
                if has_diff {
                    if let Some(d) = row.diff { p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(d, false), fid.clone(), t.signed(d)); }
                    x -= dw;
                }
                p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, format!("{:.0}", row.pct), fid.clone(), t.text_muted);
                x -= pw;
                let ar = p.text(egui::pos2(x, cy), Align2::RIGHT_CENTER, money(row.amount, false), fid.clone(), t.signed(row.amount));
                x -= aw.max(ar.width() + 8.0);
                let desc_col = if selected { t.accent } else { t.text };
                let desc_w = (x - rect.min.x).max(10.0);
                let mut job = egui::text::LayoutJob::simple_singleline(row.desc.clone(), fid, desc_col);
                job.wrap = egui::text::TextWrapping::truncate_at_width(desc_w);
                let g = p.layout_job(job);
                let gh = g.size().y;
                p.galley(egui::pos2(rect.min.x, cy - 2.0 - gh / 2.0), g, desc_col);
                // share of income as a hairline bar under the name
                let share = (row.pct.abs() / 100.0).clamp(0.0, 1.0) as f32;
                if share > 0.0 && !is_net {
                    let bar = egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.max.y - 4.0), Vec2::new((desc_w * share).max(2.0), 2.0));
                    p.rect_filled(bar, 1, tint(t.surface, t.signed(row.amount), 0.6));
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
        ui.vertical_centered(|ui| {
            ui.add_space((h * 0.22).max(SP4));
            ui.label(RichText::new(ph::CALENDAR_DOTS).size(56.0).color(t.text_faint));
            ui.add_space(SP2);
            ui.label(RichText::new("Open a budget").text_style(ts(TITLE)));
            ui.add_space(SP1);
            ui.label(RichText::new("Choose a .bgt file, or drop one anywhere on this window.").color(t.text_muted));
            ui.add_space(SP3);
            let b = Button::new(RichText::new(format!("{}  Open…", ph::FOLDER_OPEN)).color(Color32::WHITE).text_style(ts(SEMI))).fill(t.accent).stroke(Stroke::NONE).min_size(Vec2::new(150.0, 34.0));
            if ui.add(b).on_hover_text("Ctrl+O").clicked() { self.open_dialog(); }
            let recent: Vec<String> = self.cfg.recent_file.clone();
            if !recent.is_empty() {
                ui.add_space(SP4);
                ui.label(RichText::new("Recent").text_style(ts(CAPTION)).color(t.text_faint));
                ui.add_space(SP1);
                for r in recent {
                    let lbl = RichText::new(format!("{}  {}", ph::FILE_TEXT, file_name(Path::new(&r)))).color(t.accent);
                    if ui.add(Button::new(lbl).frame(false)).on_hover_text(&r).clicked() { self.open(Path::new(&r)); }
                }
            }
        });
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
                let unlock = Button::new(RichText::new("Unlock").color(Color32::WHITE)).fill(t.accent).stroke(Stroke::NONE);
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

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (o, s, f, pgdn, pgup, home) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::COMMAND, Key::O),
            i.consume_key(Modifiers::COMMAND, Key::S),
            i.consume_key(Modifiers::COMMAND, Key::F),
            !typing && i.consume_key(Modifiers::NONE, Key::PageDown),
            !typing && i.consume_key(Modifiers::NONE, Key::PageUp),
            !typing && i.consume_key(Modifiers::NONE, Key::Home),
        ));
        if o { self.open_dialog(); }
        if s { self.save(); }
        if f { self.focus_filter = true; }
        if pgdn { self.go(1); }
        if pgup { self.go(-1); }
        if home { self.go_today(); }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).find(|p| p.extension().map_or(false, |e| e.eq_ignore_ascii_case("bgt"))));
        if let Some(p) = dropped { self.open(&p); }
    }
}

fn file_name(p: &Path) -> String { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()) }

fn menu_btn(label: &str, shortcut: &str) -> Button<'static> { Button::new(label.to_string()).shortcut_text(shortcut.to_string()) }
fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> egui::Response { ui.add(menu_btn(label, shortcut)) }

fn icon_button(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(Button::new(RichText::new(icon).text_style(ts(ICON))).min_size(Vec2::new(28.0, 28.0)))
}

fn vsep(ui: &mut egui::Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, t.border));
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.screenshot_step(ctx);
        if self.title_dirty {
            self.title_dirty = false;
            let title = match &self.file { Some(f) => format!("{} — Budget Calendar", file_name(f)), None => "Budget Calendar".into() };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        self.keys(ctx);
        self.password_prompt(ctx);
        self.ensure_view();
        let t = self.t.clone();
        egui::TopBottomPanel::top("header").show_separator_line(false)
            .frame(Frame::new().fill(t.surface).inner_margin(Margin { left: 16, right: 16, top: 6, bottom: 10 }))
            .show(ctx, |ui| {
                self.header(ui);
                let r = ui.max_rect();
                ui.painter().hline(egui::Rangef::new(r.min.x - 16.0, r.max.x + 16.0), r.max.y + 9.5, Stroke::new(1.0_f32, t.border));
            });
        let has_db = self.db.is_some();
        if has_db && self.show_summary && ctx.screen_rect().width() >= 1100.0 {
            egui::SidePanel::right("summary").resizable(false).exact_width(SUMMARY_W).show_separator_line(false)
                .frame(Frame::new().fill(t.bg).inner_margin(Margin { left: 0, right: 16, top: 16, bottom: 16 }))
                .show(ctx, |ui| self.summary(ui));
        }
        egui::CentralPanel::default().frame(Frame::new().fill(t.bg).inner_margin(SP3)).show(ctx, |ui| {
            self.banner(ui);
            if !has_db { self.empty_state(ui); return; }
            let Some(view) = self.view.take() else { return };
            self.stats(ui, &view);
            ui.add_space(SP2 + SP1);
            let clicked = calendar::show(ui, &view, &t, self.cfg.first_day_of_week_is_sunday, self.today, self.cfg.show_decimals_in_cv);
            self.view = Some(view);
            if let Some(f) = clicked { self.apply_filter(&f); }
        });
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

fn main() -> eframe::Result {
    let cli = parse_cli();
    let size = cli.size.unwrap_or([1200.0, 800.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size).with_min_inner_size([900.0, 600.0]).with_title("Budget Calendar"),
        ..Default::default()
    };
    eframe::run_native("Budget Calendar", options, Box::new(|cc| {
        theme::install_fonts(&cc.egui_ctx);
        let app = App::new(cli);
        theme::apply(&cc.egui_ctx, &app.t);
        Ok(Box::new(app))
    }))
}
