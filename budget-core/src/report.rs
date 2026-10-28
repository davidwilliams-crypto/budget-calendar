//! HTML / Excel report generators (analysis/06-stats-window-and-reports.md §1, §4–§10).
//! Every generator returns the complete document as a `String`; the caller writes the file and
//! opens it. Output matches the original byte for byte, quirks included (search `// quirk:`).
//!
//! Images: the generators take an [`ImageResolver`] that maps a relative image path
//! (`icons/note.png`, `images/pig.gif`) to a `data:;base64,...` URI (see [`base64_data_uri`]);
//! `None` leaves the original `file://` URL in place, exactly like `embeddedSRC` on a missing file.

use crate::date::{Cal, Field};
use crate::db::BudgetDb;
use crate::model::*;
use crate::numfmt::{format_frac2, group_digits, sdate_dmy};
use crate::stats::{calc_statistics, Row, Stats, INDENT, UNCATEGORIZED_EXPENSES, UNCATEGORIZED_INCOME};
use std::collections::HashMap;

/// Maps a relative image path to the URL to embed (normally a `data:` URI), `None` if missing.
pub type ImageResolver<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportMode { Html, Excel }

/// Theme colours the reports need (light-theme defaults from analysis/09, `0xRRGGBB`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportColors {
    pub bg_paid: u32,
    pub bg_important: u32,
}

impl Default for ReportColors {
    fn default() -> ReportColors { ReportColors { bg_paid: 0xcadeb8, bg_important: 0xffff00 } }
}

/// Options of the printed monthly calendar (`saveHTML`).
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarOptions {
    /// `cfg.showDecimalsInCV`.
    pub show_decimals: bool,
    /// `cfg.preserveColorOnPrint`.
    pub preserve_color_on_print: bool,
    /// `cfg.firstDayOfWeekIsSunday`.
    pub first_day_sunday: bool,
    /// `cfg.currencyPrefix` / `cfg.currencySuffix`: accepted for API parity; the original
    /// `saveHTML` never prints them (amounts go through `amount2html` only).
    pub currency_prefix: String,
    pub currency_suffix: String,
    pub colors: ReportColors,
}

impl Default for CalendarOptions {
    fn default() -> CalendarOptions {
        CalendarOptions {
            show_decimals: false,
            preserve_color_on_print: false,
            first_day_sunday: true,
            currency_prefix: "$".into(),
            currency_suffix: String::new(),
            colors: ReportColors::default(),
        }
    }
}

const NBSP3: &str = "&nbsp;&nbsp;&nbsp;";
const NBSP4: &str = "&nbsp;&nbsp;&nbsp;&nbsp;";
pub const NORMAL_COLORS: [&str; 2] = ["black", "red"];
const DISABLED_COLORS: [&str; 2] = ["#C0C0C0", "#FF8080"];
const MONTHS3: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_FULL: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WDAY_NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const STYLE_SEP: &str = " style='padding-top: 1em; padding-bottom: 1em; border-top: 1px solid black;'";
const REPORT_CSS: &str = "<style type=\"text/css\">\nBODY { font: 10px Arial, Helvetica, sans-serif; }\nP, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }\n";

// ---------------------------------------------------------------------------------------------
// §1 shared helpers
// ---------------------------------------------------------------------------------------------

/// `DecimalFormat` with grouping on and exactly `n` fraction digits (`-1,234.5`). Unlike
/// `numfmt::format_frac0`, a negative value that rounds to zero keeps its sign (`-0`), as Java's
/// `DecimalFormat` prints it in the reports.
fn frac(x: f64, n: usize) -> String {
    let s = format!("{:.*}", n, x.abs());
    let (int_part, fr) = s.split_once('.').unwrap_or((s.as_str(), ""));
    let grouped = group_digits(int_part);
    let sign = if x < 0.0 { "-" } else { "" };
    if n == 0 { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{fr}") }
}

/// `amount2html(v, cssClass, colors, decimals)`: `<FONT color=red>-1,234<small>.50</small></font>`.
pub fn amount2html(v: f64, css_class: Option<&str>, colors: [&str; 2], decimals: bool) -> String {
    let v = if v == 0.0 { 0.0 } else { v }; // -0.0 -> 0.0
    let color = if v < 0.0 { colors[1] } else { colors[0] };
    let open = match css_class { None => ">".to_string(), Some(c) => format!(" class='{c}'>") };
    let num = if decimals { format_frac2(v) } else { frac(v, 0) };
    let s = format!("<FONT color={color}{open}{num}");
    match s.find('.') {
        Some(i) => format!("{}<small>{}</small></font>", &s[..i], &s[i..]),
        None => s + "</font>",
    }
}

/// `f2html(v)`: black/red, always two decimals.
pub fn f2html(v: f64) -> String { amount2html(v, None, NORMAL_COLORS, true) }

/// `f2html_no_zero(v)`: `""` when `|v| < 1.0` (integer division of cents), else `amount2html`.
pub fn f2html_no_zero(v: f64, show_decimals: bool) -> String {
    if ((v * 100.0) as i32) / 100 == 0 { String::new() } else { amount2html(v, None, NORMAL_COLORS, show_decimals) }
}

/// `escape_html_text`: `& < > " '` escaped, code points > 255 as `&#x{hex};`.
pub fn escape_html_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if (c as u32) > 255 => out.push_str(&format!("&#x{:x};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `getDesc(desc)`: the text before the first `;`.
/// Quirks kept: the `calendarDescriptionMaxLen` truncation (`...`) is dead code because that field is
/// always `-1`, and a stats key passed in keeps its leading 3-space indent (callers rely on it).
pub fn get_desc(desc: &str) -> &str { desc.split(';').next().unwrap_or("") }

/// `get_normalized_note(note, mode, leadingBR)`. The note text is HTML-escaped (the original wrote it raw).
pub fn get_normalized_note(note: Option<&str>, mode: ReportMode, leading_br: bool) -> String {
    let Some(note) = note else { return String::new() };
    let note = escape_html_text(note);
    match mode {
        ReportMode::Excel => match note.split_once('\n') {
            Some((first, _)) => format!("  (Note: {first}...)"),
            None => format!("  (Note: {note})"),
        },
        ReportMode::Html => format!("{}<FONT COLOR=gray>Note: <I>{note}</I></FONT>", if leading_br { "<BR>" } else { "" }).replace('\n', "<BR>"),
    }
}

/// `html_string(pmt, text)`: gray when the payment is disabled.
pub fn html_string(pmt: &Payment, text: &str) -> String {
    if pmt.disabled { format!("<FONT color=gray>{text}</FONT>") } else { text.to_string() }
}

/// `getExcelNumberStyle()` (note the trailing space).
pub fn excel_number_style() -> &'static str { "STYLE=\"vnd.ms-excel.numberformat:###0.00\" " }

/// `repeat(s, n)`.
pub fn repeat(s: &str, n: i32) -> String { if n <= 0 { String::new() } else { s.repeat(n as usize) } }

/// `html_merge_styles(style, extra)`: appends `extra` inside an existing ` style='...;'` attribute.
pub fn html_merge_styles(style: &str, extra: &str) -> String {
    if style.trim().is_empty() { format!(" style='{extra}'") } else { style.replace(";'", &format!(";{extra}'")) }
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// What `embeddedSRC` returns for an existing file: `data:;base64,...` (no MIME type).
pub fn base64_data_uri(bytes: &[u8]) -> String { format!("data:;base64,{}", base64(bytes)) }

/// `embeddedSRC(path, isExcel)`: in HTML mode a `file://` path the resolver knows becomes its
/// embedded URI; otherwise the path is returned unchanged.
pub fn embedded_src(path: &str, is_excel: bool, images: ImageResolver) -> String {
    if !is_excel {
        if let Some(rel) = path.strip_prefix("file://") {
            if let Some(url) = images(rel) { return url; }
        }
    }
    path.to_string()
}

/// `genRepImage(value, totalIncome, totalExpense, width)`.
pub fn gen_rep_image(value: f64, total_income: f64, total_expense: f64, width: i32, images: ImageResolver) -> String {
    let mut max = total_income.abs().max(total_expense.abs());
    if max == 0.0 { max = 1.0; }
    let gif = if value < 0.0 { "red_g.gif" } else { "blue_g.gif" };
    let src = embedded_src(&format!("file://icons/{gif}"), false, images);
    format!("<img src=\"{src}\" height=10 width={}>\n", (width as f64 * value.abs() / max) as i32)
}

/// `UnicodishBufferedWriter.str2unicodish`: every UTF-16 unit >= 128 and `,` `\n` `\r` become `&#xhhhh;`.
pub fn unicodish(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for u in s.encode_utf16() {
        if u >= 128 || u == 44 || u == 10 || u == 13 { out.push_str(&format!("&#x{u:04x};")); }
        else { out.push(u as u8 as char); }
    }
    out
}

/// `UnicodishBufferedWriter`: `write` escapes (see [`unicodish`]), `write_raw` does not.
#[derive(Default)]
pub struct UnicodishWriter { pub out: String }

impl UnicodishWriter {
    pub fn write(&mut self, s: &str) { self.out.push_str(&unicodish(s)); }
    pub fn write_raw(&mut self, s: &str) { self.out.push_str(s); }
}

fn sdate(c: &Cal) -> String { sdate_dmy(c.get()) }
/// `sdate_MY`: `December-2021`.
fn sdate_my(c: &Cal) -> String { format!("{}-{}", MONTHS_FULL[c.month() as usize], c.year()) }
/// `SimpleCalendar.toString("MMM, yyyy")`: `Dec, 2021`.
fn mmm_yyyy(c: &Cal) -> String { format!("{}, {}", MONTHS3[c.month() as usize], c.year()) }

/// `Stats.get_range_string()`.
pub fn range_string(st: &Stats) -> String {
    let d = st.duration_months();
    if d == 0 { mmm_yyyy(&st.start) } else { format!("{} - {} ({}m)", mmm_yyyy(&st.start), mmm_yyyy(&st.end), d + 1) }
}

/// Entry description of the stats rows: escaped `getDesc(key)` with `nbsp4` in front of leaves.
fn entry_desc(key: &str) -> String {
    let d = escape_html_text(get_desc(key));
    if d.starts_with(INDENT) { format!("{NBSP4}{d}") } else { d }
}

/// Number of leaf rows that follow the category at `i` (= `sub_amounts.size()`).
fn sub_count(rows: &[Row], i: usize) -> usize { rows[i + 1..].iter().take_while(|r| r.is_sub).count() }

fn flag_icon(flag: char) -> &'static str {
    match flag {
        FLAG_PAID => "<font color=green>&#x2714</font> ",
        FLAG_IMPORTANT => "<font color=red>&#x26A0</font> ",
        _ => "",
    }
}

fn bgcolor(shade: bool) -> String { format!("BGCOLOR=\"#{}\"", if shade { "eeeeee" } else { "ffffff" }) }

// ---------------------------------------------------------------------------------------------
// §4 Print -> generateStatsReport
// ---------------------------------------------------------------------------------------------

// Mirrors the original's addRepRow parameter list; a wrapper struct would only obscure the mapping.
#[allow(clippy::too_many_arguments)]
fn add_rep_row(w: &mut UnicodishWriter, st: &Stats, shade: &mut bool, desc: &str, amount: f64, indented: bool, diff: f64, images: ImageResolver) {
    let pct = if st.total_income != 0.0 { frac(amount / st.total_income * 100.0, 1) }
        else if st.total_expense != 0.0 { frac(amount / st.total_expense * 100.0, 1) }
        else { "?".to_string() };
    let bg = if *shade { "EEeeEE" } else { "FFffFF" };
    w.write(&format!("<TR bgcolor={bg}><TD>{}{desc}<TD ALIGN=RIGHT>{NBSP3}{}<TD ALIGN=RIGHT>{NBSP3}{pct}<TD ALIGN=RIGHT>{NBSP3}{}",
        if indented { NBSP3 } else { "" }, format_frac2(amount), if diff.abs() > 5.0 { format_frac2(diff) } else { String::new() }));
    *shade = !*shade;
    w.write(&format!("<TD>{}", gen_rep_image(amount, st.total_income, st.total_expense, 300, images)));
}

/// "Print" in the Stats window (§4).
pub fn generate_stats_report(st: &Stats, images: ImageResolver) -> String {
    let mut w = UnicodishWriter::default();
    let mut shade = false;
    w.write(&format!("<HTML>\n<HEAD>\n\t<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n{REPORT_CSS}</style>\n"));
    w.write("<TABLE BORDER=0 CELLSPACING=2>\n");
    w.write(&format!("<CAPTION>Budget from <B>{}</B> to <B>{}</B></CAPTION>\n", sdate(&st.start), sdate(&st.end)));
    w.write("<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B><TD><B>Graph</B>");
    let (ti, te, pti, pte) = (st.total_income, st.total_expense, st.total_planned_income, st.total_planned_expense);
    add_rep_row(&mut w, st, &mut shade, "Disposable Income", ti + te, false, (ti + te) - (pti + pte), images);
    add_rep_row(&mut w, st, &mut shade, "Total Income", ti, false, ti - pti, images);
    for r in &st.income { add_rep_row(&mut w, st, &mut shade, &entry_desc(&r.key), r.amount, true, r.amount - r.planned_amount, images); }
    add_rep_row(&mut w, st, &mut shade, "Total Expenses", te, false, te - pte, images);
    for r in &st.expense { add_rep_row(&mut w, st, &mut shade, &entry_desc(&r.key), r.amount, true, r.amount - r.planned_amount, images); }
    w.write("</TABLE></HTML>"); // quirk: no </BODY>
    w.out
}

// ---------------------------------------------------------------------------------------------
// §5 Excel Report -> generateExcelStatsReport
// ---------------------------------------------------------------------------------------------

/// `getStatFormula(col, firstRow, vec)`: `FORMULA="=+B4+B7" ` summing the category rows.
fn stat_formula(col: char, first_row: usize, rows: &[Row]) -> String {
    let mut s = String::from(" FORMULA=\"=");
    let mut found = false;
    for (i, r) in rows.iter().enumerate() {
        if !r.key.starts_with(INDENT) { s.push_str(&format!("+{col}{}", first_row + i)); found = true; }
    }
    if !found { s.push_str("0.0"); }
    s + "\" "
}

fn excel_entries(w: &mut UnicodishWriter, rows: &[Row], r: &mut usize, den: &str) {
    let s = excel_number_style();
    for (i, row) in rows.iter().enumerate() {
        let mut desc = escape_html_text(get_desc(&row.key));
        let mut sum = String::new();
        if desc.starts_with(INDENT) { desc = format!("{NBSP4}{desc}"); }
        else { sum = format!(" FORMULA='=SUM(B{}..B{})'", *r + 1, *r + sub_count(rows, i)); }
        w.write(&format!("<TR><TD>{NBSP3}{desc}<TD ALIGN=RIGHT {s}{sum}>{}<TD ALIGN=RIGHT {s}FORMULA='=B{r}/{den}*100.0'>?\n", format_frac2(row.amount)));
        if row.amount - row.planned_amount != 0.0 {
            w.write(&format!("<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{}</FONT>\n", format_frac2(row.amount - row.planned_amount)));
        }
        *r += 1;
    }
}

/// "Excel Report" in the Stats window (§5). Amount cells hold `0.001` / `?` placeholders for Excel formulas.
pub fn generate_excel_stats_report(st: &Stats) -> String {
    let s = excel_number_style();
    let mut w = UnicodishWriter::default();
    w.write(&format!("<HTML>\n<HEAD>\n\t<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n{REPORT_CSS}</style>\n"));
    w.write("<TABLE BORDER=0 CELLSPACING=2>\n");
    w.write(&format!("<CAPTION>Budget from <B>{}</B> to <B>{}</B></CAPTION>\n", sdate(&st.start), sdate(&st.end)));
    w.write("<TR bgcolor=eeEEee><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B>");
    let den = if st.total_income != 0.0 { "B3" } else { "B2" };
    w.write(&format!("<TR><TD><B>Disposable Income:</B><TD ALIGN=RIGHT {s} FORMULA='=B3+B{}'>0.001<TD ALIGN=RIGHT {s} FORMULA='=B2/{den}*100'>?\n<TD ALIGN=RIGHT>\n", st.income.len() + 4));
    w.write(&format!("<TR><TD><B>Total Income:</B><TD ALIGN=RIGHT {s}{}>0.001<TD ALIGN=RIGHT {s}>100.00\n<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{}</FONT>\n",
        stat_formula('B', 4, &st.income), format_frac2(st.total_income - st.total_planned_income)));
    let mut r = 4;
    excel_entries(&mut w, &st.income, &mut r, den);
    w.write(&format!("<TR><TD><B>Total Expenses:</B><TD ALIGN=RIGHT {s}{}>0.001<TD ALIGN=RIGHT {s}FORMULA='=B{r}/{den}*100.0'>?\n<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>{}</FONT>\n",
        stat_formula('B', r + 1, &st.expense), format_frac2(st.total_expense - st.total_planned_expense)));
    r += 1;
    excel_entries(&mut w, &st.expense, &mut r, den);
    w.write("</TABLE></HTML>"); // quirk: no </BODY>
    w.out
}

// ---------------------------------------------------------------------------------------------
// §6 Comparative report
// ---------------------------------------------------------------------------------------------

/// `Budget$comp_entry`: `v1` = base amount, `v2` = other amount.
#[derive(Clone, Debug, PartialEq)]
pub struct CompEntry { pub key: String, pub v1: f64, pub v2: f64 }

/// `comp_entry_from(baseVec, otherVec)`: base entries in order, then unmatched other entries appended;
/// entries with both amounts zero are dropped. Matching is by raw key (leaves keep their indent).
pub fn comp_entry_from(base: &[Row], other: &[Row]) -> Vec<CompEntry> {
    let mut v: Vec<CompEntry> = base.iter().map(|r| CompEntry { key: r.key.clone(), v1: r.amount, v2: 0.0 }).collect();
    for r in other {
        match v.iter_mut().find(|e| e.key == r.key) {
            Some(e) => e.v2 = r.amount,
            None => v.push(CompEntry { key: r.key.clone(), v1: 0.0, v2: r.amount }),
        }
    }
    v.retain(|e| e.v1 != 0.0 || e.v2 != 0.0);
    v
}

// Mirrors the original's addCompRow parameter list; a wrapper struct would only obscure the mapping.
#[allow(clippy::too_many_arguments)]
fn add_comp_row(w: &mut UnicodishWriter, base: &Stats, shade: &mut bool, desc: &str, v1: f64, indented: bool, v2: f64, images: ImageResolver) {
    // Cents as i64: an i32 saturates at 21,474,836.47.
    let v1 = ((v1 * 100.0) as i64) as f64 / 100.0;
    let v2 = ((v2 * 100.0) as i64) as f64 / 100.0;
    let diff = v1 - v2;
    let pct = if v1 == 0.0 || diff == 0.0 { f64::INFINITY } else { diff / v1 * 100.0 };
    let bg = if *shade { "EEeeEE" } else { "FFffFF" };
    let diff_cell = if diff.abs() > 5.0 { f2html(diff) } else { String::new() };
    let pct_cell = if diff.abs() > 5.0 && pct.is_finite() { format!("{}{}", if pct > 0.0 { "+" } else { "" }, frac(pct, 0)) } else { String::new() };
    w.write(&format!("<TR bgcolor={bg}><TD>{}{desc}<TD ALIGN=RIGHT>{NBSP3}{}<TD ALIGN=RIGHT>{NBSP3}{}<TD ALIGN=RIGHT>{NBSP3}{diff_cell}<TD ALIGN=RIGHT>{NBSP3}{pct_cell}",
        if indented { NBSP3 } else { "" }, f2html(v1), f2html(v2)));
    *shade = !*shade;
    // quirk: the bar shows the *other* value scaled against the *base* totals
    w.write(&format!("<TD>{}", gen_rep_image(v2, base.total_income, base.total_expense, 300, images)));
}

/// "Comparative report to..." (§6): `other` must span the same number of months as `base`
/// (`calc_statistics(month, base.duration_months() + 1, true)`).
pub fn generate_comparison_report(base: &Stats, other: &Stats, images: ImageResolver) -> String {
    let mut w = UnicodishWriter::default();
    let mut shade = false;
    w.write(&format!("<HTML>\n<HEAD>\n<TITLE>Budget Comparison Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n{REPORT_CSS}</style>\n"));
    w.write("<TABLE BORDER=0 CELLSPACING=2>\n");
    let (rb, ro) = (range_string(base), range_string(other));
    w.write(&format!("<CAPTION>Stats comparison of two ranges <B>{rb}</B> vs <B>{ro}</B></CAPTION>\n"));
    w.write(&format!("<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=CENTER><B>{rb}<br>(base)<br>amount</B><TD ALIGN=CENTER><B>{ro}<br>(other)<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>%</B><TD><B>Graph</B>"));
    add_comp_row(&mut w, base, &mut shade, "Disposable Income", base.total_income + base.total_expense, false, other.total_income + other.total_expense, images);
    add_comp_row(&mut w, base, &mut shade, "Total Income", base.total_income, false, other.total_income, images);
    for e in comp_entry_from(&base.income, &other.income) { add_comp_row(&mut w, base, &mut shade, &entry_desc(&e.key), e.v1, true, e.v2, images); }
    add_comp_row(&mut w, base, &mut shade, "Total Expenses", base.total_expense, false, other.total_expense, images);
    for e in comp_entry_from(&base.expense, &other.expense) { add_comp_row(&mut w, base, &mut shade, &entry_desc(&e.key), e.v1, true, e.v2, images); }
    w.write("</TABLE></HTML>"); // quirk: no </BODY>
    w.out
}

// ---------------------------------------------------------------------------------------------
// §7 Month-by-Month report
// ---------------------------------------------------------------------------------------------

/// `Budget$month_by_month_entry`. `category == None` means an aggregate row (category or one of the
/// three totals); `special` marks Disposable Income / Total Income / Total Expenses.
#[derive(Clone, Debug, PartialEq)]
pub struct MbmEntry {
    pub category: Option<String>,
    pub description: String,
    pub total: f64,
    pub amounts: Vec<f64>,
    pub style: &'static str,
    pub special: bool,
}

impl MbmEntry {
    fn new(category: Option<&str>, description: &str, special: bool) -> MbmEntry {
        MbmEntry { category: category.map(str::to_string), description: description.to_string(), total: 0.0, amounts: Vec::new(), style: if special { STYLE_SEP } else { "" }, special }
    }
    pub fn is_aggregate(&self) -> bool { self.category.is_none() }
}

/// `Stats.get_flat_map()`: `(category, key) -> amount` for every category and leaf.
fn flat_map(st: &Stats) -> Vec<((Option<String>, String), f64)> {
    let mut v = Vec::new();
    for a in &st.top {
        v.push(((None, a.key.clone()), a.amount));
        for s in &a.sub_amounts { v.push(((Some(a.key.clone()), s.key.clone()), s.amount)); }
    }
    v
}

/// `get_month_by_month_entries(stats)`; `Err` carries the original's consistency `RuntimeException` text.
pub fn month_by_month_entries(db: &mut BudgetDb, st: &Stats) -> Result<Vec<MbmEntry>, String> {
    let m = (st.duration_months() + 1) as usize;
    let monthly: Vec<Stats> = (0..m).map(|i| { let mut c = st.start; c.add(Field::Month, i as i32); calc_statistics(db, &c, 1, true, None) }).collect();
    let flat: Vec<HashMap<(Option<String>, String), f64>> = monthly.iter().map(|s| flat_map(s).into_iter().collect()).collect();

    let mut list: Vec<MbmEntry> = Vec::new();
    let mut disposable = MbmEntry::new(None, "Disposable Income", true);
    for s in &monthly { let v = s.total_income + s.total_expense; disposable.amounts.push(v); disposable.total += v; }
    list.push(disposable);
    let has_income = !st.income.is_empty();
    let has_expense = !st.expense.is_empty();
    if has_income { list.push(MbmEntry::new(None, "Total Income", true)); }
    let mut cat: Option<String> = None;
    for (idx, row) in st.income.iter().chain(st.expense.iter()).enumerate() {
        if has_expense && idx == st.income.len() { list.push(MbmEntry::new(None, "Total Expenses", true)); }
        if !row.is_sub {
            cat = Some(row.key.clone());
            let mut e = MbmEntry::new(None, &row.key, false);
            e.amounts = vec![0.0; m];
            list.push(e);
        } else {
            let mut e = MbmEntry::new(cat.as_deref(), &row.key, false);
            for f in &flat {
                let key = (cat.clone(), row.key.clone());
                let mut v = f.get(&key);
                if v.is_none() {
                    let alt = match cat.as_deref() {
                        Some(UNCATEGORIZED_INCOME) => Some(UNCATEGORIZED_EXPENSES),
                        Some(UNCATEGORIZED_EXPENSES) => Some(UNCATEGORIZED_INCOME),
                        _ => None,
                    };
                    v = f.get(&(alt.map(str::to_string), row.key.clone()));
                }
                let a = v.copied().unwrap_or(0.0);
                e.amounts.push(a);
                e.total += a;
            }
            list.push(e);
        }
    }
    // category totals: sum of the leaves that follow, per month
    for i in 0..list.len() {
        if list[i].special || !list[i].is_aggregate() { continue; }
        list[i].total = 0.0;
        for month in 0..m {
            let sum: f64 = list[i + 1..].iter().take_while(|e| !e.is_aggregate()).map(|e| e.amounts[month]).sum();
            list[i].amounts[month] = sum;
            list[i].total += sum;
        }
    }
    // Total Income / Total Expenses: sum of the aggregate rows between them
    let ti = list.iter().position(|e| e.special && e.description == "Total Income");
    let te = list.iter().position(|e| e.special && e.description == "Total Expenses");
    if let Some(ti) = ti {
        let end = te.unwrap_or(list.len());
        for month in 0..m {
            let sum: f64 = list[ti + 1..end].iter().filter(|e| e.is_aggregate()).map(|e| e.amounts[month]).sum();
            list[ti].total += sum;
            list[ti].amounts.push(sum);
        }
    }
    if let Some(te) = te {
        let end = list.len();
        for month in 0..m {
            let sum: f64 = list[te + 1..end].iter().filter(|e| e.is_aggregate()).map(|e| e.amounts[month]).sum();
            list[te].total += sum;
            list[te].amounts.push(sum);
        }
    }
    let all_zero = |e: &MbmEntry| e.amounts.iter().all(|a| *a == 0.0);
    list.retain(|e| e.special || e.is_aggregate() || !all_zero(e));
    let mut drop = vec![false; list.len()];
    for i in 0..list.len() {
        let e = &list[i];
        if e.special || !e.is_aggregate() || !all_zero(e) { continue; }
        if i + 1 == list.len() || list[i + 1].is_aggregate() { drop[i] = true; }
    }
    let mut k = 0;
    list.retain(|_| { k += 1; !drop[k - 1] });

    // consistency checks
    let by_key: HashMap<(Option<String>, String), &MbmEntry> = list.iter().filter(|e| !e.special).map(|e| ((e.category.clone(), e.description.clone()), e)).collect();
    for (key, amount) in flat_map(st) {
        match by_key.get(&key) {
            None => if amount != 0.0 { return Err(format!("missing '{}' in the resultant table", key.1)); },
            Some(e) => if !amounts_match(e.total, amount) {
                return Err(format!("'{}' amounts do NOT match: base_stats={:.2}, mbm={:.2}", e.description, amount, e.total));
            },
        }
    }
    if let Some(e) = list.iter().find(|e| e.special && e.description == "Total Income") {
        if !amounts_match(st.total_income, e.total) { return Err(format!("base stats income doesn't match! got={:.2}, expected={:.2}", e.total, st.total_income)); }
    }
    if let Some(e) = list.iter().find(|e| e.special && e.description == "Total Expenses") {
        if !amounts_match(st.total_expense, e.total) { return Err(format!("base stats expenses doesn't match! got={:.2}, expected={:.2}", e.total, st.total_expense)); }
    }
    Ok(list)
}

/// Totals summed in a different order differ by float noise; compare to half a cent.
fn amounts_match(a: f64, b: f64) -> bool { (a - b).abs() < 0.005 }

/// Java `String.format("%+,.0f", v)`.
fn signed0(v: f64) -> String { let s = frac(v, 0); if s.starts_with('-') { s } else { format!("+{s}") } }

/// "Month-by-Month report" (§7). `Err` for a single-month range (the dialog's message) or a consistency failure.
pub fn generate_month_by_month_report(db: &mut BudgetDb, st: &Stats, show_decimals: bool) -> Result<String, String> {
    if st.duration_months() == 0 { return Err("Please select more than one month in the Full Stats dialog.".into()); }
    let m = (st.duration_months() + 1) as usize;
    let mut w = UnicodishWriter::default();
    w.write_raw(&format!("<HTML>\n<HEAD>\n<TITLE>Budget Month-by-Month Comparison Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n{REPORT_CSS}table tr:hover td {{ background: #ffffc5; cursor: pointer; }}\n</style>\n"));
    w.write("<TABLE BORDER=0 CELLSPACING=2>\n");
    w.write(&format!("<CAPTION>Month-By-Month report <B>{}</B></CAPTION>\n", range_string(st)));
    w.write("<TR bgcolor=ddDDdd><TD><B>Description</B>");
    for i in 0..m {
        let mut c = st.start;
        c.add(Field::Month, i as i32);
        w.write(&format!("<TD ALIGN=CENTER><B>{}<br>amount</B>", mmm_yyyy(&c)));
    }
    w.write("<TD ALIGN=CENTER><B>Total<br>amount</B><TD ALIGN=CENTER><B>Avg/month<br>amount</B>");
    let mut shade = true;
    for e in month_by_month_entries(db, st)? {
        let mut desc = escape_html_text(get_desc(&e.description));
        let (bold, span_style) = if desc.starts_with(INDENT) {
            desc = format!("{NBSP4}{desc}");
            ("", " style='font-size: 10px'")
        } else { ("<B>", "") }; // quirk: the <B> is never closed
        shade = !shade;
        w.write_raw("\n");
        w.write(&format!("<TR bgcolor={}><TD{}>{bold}{desc}", if shade { "eeEEee" } else { "ffFFff" }, e.style));
        let mut prev = 0.0;
        for (i, a) in e.amounts.iter().enumerate() {
            let tip = if i == 0 { "base amount".to_string() } else {
                format!("{}{}", signed0(a - prev), if prev != 0.0 { format!(" ({}%)", signed0((a / prev - 1.0) * 100.0)) } else { String::new() })
            };
            w.write(&format!("<TD align=right{}><span title='{tip}'{span_style}>{bold}{}</span>", e.style, f2html_no_zero(*a, show_decimals)));
            prev = *a;
        }
        w.write(&format!("<TD align=right{}><span{span_style}>{NBSP3}{bold}{}</span>", html_merge_styles(e.style, "border-left: 1px solid black; border-right: 1px solid black;"), f2html_no_zero(e.total, show_decimals)));
        w.write(&format!("<TD align=right{}><span style='font-size: 11px'>{}</span>", e.style, f2html_no_zero(e.total / m as f64, show_decimals)));
    }
    w.write("</TABLE></HTML>"); // quirk: no </BODY>
    Ok(w.out)
}

// ---------------------------------------------------------------------------------------------
// §8 Monthly Detailed / Monthly Excel report -> generateReport
// ---------------------------------------------------------------------------------------------

/// `generateReport(start, end, path, mode)`. Columns: A Date, B image, C Category, D Payee,
/// E positive amount, F negative amount, G balance (Excel `FORMULA=` attributes refer to them).
pub fn generate_report(db: &mut BudgetDb, start: &Cal, end: &Cal, mode: ReportMode, images: ImageResolver) -> String {
    let is_excel = mode == ReportMode::Excel;
    let num_attr = format!(" LOTUS_FORMAT=\"Fixed;2\" {}", if is_excel { excel_number_style() } else { "" });
    let fmt = |x: f64| if is_excel { format!("{x:.2}") } else { format_frac2(x) }; // Excel: grouping off
    let num = |x: f64| if is_excel { fmt(x) } else { f2html(x) };
    // quirk: the Excel red font is never closed
    let bal = |x: f64| if is_excel { format!("{}{}", if x < 0.0 { "<FONT COLOR=ff0000>" } else { "" }, fmt(x)) } else { f2html(x) };
    let mut first_data_row = 3usize;
    let mut row = 0usize;
    let mut m = *start;
    m.set_day_one();
    let mut w = UnicodishWriter::default();
    w.write("<HTML>\n<HEAD>\n<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n");
    loop {
        let (mut income, mut expenses) = (0.0, 0.0);
        let mut d = m;
        d.add(Field::Day, -1);
        let mut balance = db.get_day(&d).map_or(0.0, |x| x.closing_balance);
        d.add(Field::Day, 1);
        let mut last = m;
        last.set_day_last();
        let open_formula = if row == 0 { String::new() } else { format!(" FORMULA=\"=G{}\"", row - 1) };
        if !is_excel || row == 0 {
            w.write(&format!("<TABLE BORDER=0 CELLSPACING=2><CAPTION>Budget from <B>{}</B> to <B>{}</B></CAPTION>\n", sdate(&d), sdate(&last)));
        } else {
            w.write(&format!("<TR><TR><TR><TD COLSPAN=7><BR><CENTER>Budget from <B>{}</B> to <B>{}</B></CENTER></TR>\n", sdate(&d), sdate(&last)));
            row += 2;
        }
        w.write(&format!("<TR bgcolor=EEEEEE><TD><B>Date</B><TD><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2><B>Amount</B><TD><B>Balance</B>\n<TR bgcolor=ffffff><TD ALIGN=CENTER><TD><TD ALIGN=LEFT><TD ALIGN=LEFT><B>Opening balance{}</B>\n<TD ALIGN=RIGHT><TD ALIGN=RIGHT><TD ALIGN=RIGHT{num_attr}{open_formula}><B>{}</B></TR>\n",
            repeat("&nbsp;", 30), if is_excel { fmt(balance) } else { f2html(balance) }));
        row += if is_excel { 3 } else { 4 };
        let mut shade = true;
        while d.le(&last) {
            let mut show_date = true;
            let day = db.get_day(&d).cloned().unwrap_or_default();
            let date_int = d.get();
            let date_cell = |show: bool| if show { sdate(&d) } else { String::new() };
            if let Some(note) = db.day_note(&d) {
                let note = get_normalized_note(Some(&note), mode, false);
                let formula = format!(" FORMULA=\"=G{}+E{row}+F{row}\"", row - 1);
                let c = bgcolor(shade);
                let img = format!("<IMG SRC=\"{}\">", embedded_src("file://icons/note.png", is_excel, images));
                w.write(&format!("<TR {c}><TD ALIGN=CENTER {c}>{}<TD WIDTH=18>{img}<TD ALIGN=LEFT {c}><TD ALIGN=LEFT {c}>{note}<TD ALIGN=RIGHT {c}{num_attr}><TD ALIGN=RIGHT {c}{num_attr}><TD ALIGN=RIGHT {c}{num_attr} {formula}>{}</TR>\n",
                    date_cell(show_date), bal(balance)));
                row += 1;
                shade = !shade;
                show_date = false;
            }
            for t in &day.transactions {
                let pmt = &db.payments[t.pmt];
                if !pmt.disabled { balance += t.amount; }
                let img = match &pmt.image_path {
                    Some(p) if !p.is_empty() => format!("<IMG SRC=\"{}\">", embedded_src(&format!("file://images/{p}"), is_excel, images)),
                    _ => String::new(),
                };
                let formula = format!(" FORMULA=\"=G{}{}\"", row - 1, if pmt.disabled { String::new() } else { format!("+E{row}+F{row}") });
                let c = bgcolor(shade);
                let note = get_normalized_note(pmt.get_note(date_int).as_deref(), mode, true);
                let amt = |x: f64| if pmt.disabled { amount2html(x, None, ["gray", "gray"], true) } else { num(x) };
                let pos = if t.amount >= 0.0 { amt(t.amount) } else { String::new() };
                let neg = if t.amount < 0.0 { amt(t.amount) } else { String::new() };
                let bal_cell = if !is_excel && pmt.disabled { "<TD>".to_string() } else { format!("<TD ALIGN=RIGHT {c}{num_attr} {formula}>{}", bal(balance)) };
                w.write(&format!("<TR {c}><TD ALIGN=CENTER {c}>{}<TD WIDTH=18>{img}<TD ALIGN=LEFT {c}>{}<TD ALIGN=LEFT {c}>{}{}{note}<TD ALIGN=RIGHT {c}{num_attr}>{pos}<TD ALIGN=RIGHT {c}{num_attr}>{neg}{bal_cell}</TR>\n",
                    date_cell(show_date), html_string(pmt, &escape_html_text(pmt.category.as_deref().unwrap_or(""))), flag_icon(pmt.get_flag(date_int)), html_string(pmt, &escape_html_text(&pmt.description))));
                row += 1;
                shade = !shade;
                if !pmt.disabled { if t.amount < 0.0 { expenses += t.amount; } else { income += t.amount; } }
                show_date = false;
            }
            if balance != day.closing_balance {
                let diff = day.closing_balance - balance;
                balance = day.closing_balance;
                let formula = format!(" FORMULA=\"=G{}+E{row}+F{row}\"", row - 1);
                let c = bgcolor(shade);
                w.write(&format!("<TR {c}><TD ALIGN=CENTER {c}>{}<TD WIDTH=18><TD><TD ALIGN=RIGHT {c}><I>Balance adjustment</I><TD ALIGN=RIGHT {c}{num_attr}><I>{}</I><TD ALIGN=RIGHT {c}{num_attr}><I>{}</I><TD ALIGN=RIGHT {c}{num_attr} {formula}><I>{}</I></TR>\n",
                    date_cell(show_date), if diff >= 0.0 { num(diff) } else { String::new() }, if diff < 0.0 { num(diff) } else { String::new() }, bal(balance)));
                row += 1;
                shade = !shade;
                if diff < 0.0 { expenses += diff; } else { income += diff; }
            }
            d.add(Field::Day, 1);
        }
        let bg1 = if shade { "eeeeee" } else { "ffffff" };
        let bg2 = if !shade { "eeeeee" } else { "ffffff" };
        // The original printed the income total in this cell in HTML mode; the port shows expenses.
        let expenses_cell = if is_excel { fmt(expenses) } else { f2html(expenses) };
        w.write(&format!("<TR bgcolor={bg1}><TD><TD COLSPAN=3 ALIGN=RIGHT><B>Totals:</B><TD ALIGN=RIGHT {num_attr} FORMULA=\"=@sum(E{first_data_row}..E{})\"><B>{}</B><TD ALIGN=RIGHT {num_attr} FORMULA=\"=@sum(F{first_data_row}..F{})\"><B>{expenses_cell}</B><TD ALIGN=RIGHT {num_attr} FORMULA=\"=E{row}+F{row}\"><B>{}</B></TR>\n<TR bgcolor={bg2}><TD><TD><TD COLSPAN=2 ALIGN=RIGHT><B>Closing balance:</B><TD ALIGN=RIGHT><B></B><TD ALIGN=RIGHT><B></B><TD ALIGN=RIGHT {num_attr} FORMULA=\"=G{}\"><B>{}</B></TR>\n",
            row - 1, num(income), row - 1, num(income + expenses), row - 1, bal(balance)));
        if !is_excel { w.write("</TABLE>\n<P><BR></P>"); }
        row += 2;
        first_data_row = row + 4; // quirk: from the 2nd month on this points at the opening-balance row
        m.add(Field::Month, 1);
        // quirk: the "To" month is excluded (loop continues only while m < end)
        if !m.lt(end) { break; }
    }
    w.write("</BODY>\n</HTML>\n");
    w.out
}

// ---------------------------------------------------------------------------------------------
// §9 Unpaid & paid report
// ---------------------------------------------------------------------------------------------

struct TranSum { income: f64, expenses: f64, amount: f64 }

impl TranSum {
    fn of(list: &[(i32, usize, f64)]) -> TranSum {
        let (mut income, mut expenses) = (0.0, 0.0);
        for &(_, _, a) in list { if a < 0.0 { expenses += a; } else { income += a; } }
        TranSum { income, expenses, amount: income + expenses }
    }
    fn cells(&self) -> String {
        let c = |x| amount2html(x, None, NORMAL_COLORS, true);
        format!("<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}</TR><TR></TR>", c(self.income), c(self.expenses), c(self.amount))
    }
}

/// `generatePaidUnpaidReport(start, end, path)` (§9), day-granular range, disabled payments excluded.
pub fn generate_paid_unpaid_report(db: &mut BudgetDb, start: &Cal, end: &Cal, images: ImageResolver) -> String {
    let (mut paid, mut unpaid, mut important): (Vec<(i32, usize, f64)>, Vec<_>, Vec<_>) = (Vec::new(), Vec::new(), Vec::new());
    let mut c = *start;
    while c.le(end) {
        let trans: Vec<(usize, f64)> = db.get_day(&c).map_or(Vec::new(), |d| d.transactions.iter().map(|t| (t.pmt, t.amount)).collect());
        for (pi, amount) in trans {
            let p = &db.payments[pi];
            if p.disabled { continue; }
            let e = (c.get(), pi, amount);
            match p.get_flag(c.get()) {
                FLAG_PAID => paid.push(e),
                f => { unpaid.push(e); if f == FLAG_IMPORTANT { important.push(e); } }
            }
        }
        c.add(Field::Day, 1);
    }
    let mut w = UnicodishWriter::default();
    w.write("<HTML>\n<HEAD><TITLE>Budget Report</TITLE></HEAD><BODY bgcolor=ffffff>\n");
    w.write(&format!("<TABLE BORDER=0 CELLSPACING=2><CAPTION>Paid/Unpaid Report from <B>{}</B> to <B>{}</B></CAPTION>\n", sdate(start), sdate(end)));
    w.write("<TR bgcolor=EEEEEE><TD><B>Date</B><TD><!--image--><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2 ALIGN=CENTER><B>Amount</B></TR>\n");
    let mut shade = false; // quirk: not reset between sections (only lastDate is)
    let mut write_rows = |w: &mut UnicodishWriter, list: &[(i32, usize, f64)]| {
        let mut last_date = 0;
        for &(date, pi, amount) in list {
            let p = &db.payments[pi];
            let c = bgcolor(shade);
            let img = match &p.image_path {
                Some(path) if !path.is_empty() => format!("<IMG SRC=\"{}\">", embedded_src(&format!("file://images/{path}"), false, images)),
                _ => String::new(),
            };
            let amt = amount2html(amount, None, NORMAL_COLORS, true);
            w.write(&format!("<TR {c}><TD ALIGN=CENTER {c}>{}<TD WIDTH=18>{img}<TD ALIGN=LEFT {c}>{}<TD ALIGN=LEFT {c}>{}{}{}<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}</TR>\n",
                if date != last_date { sdate_dmy(date) } else { String::new() },
                escape_html_text(p.category.as_deref().unwrap_or("")),
                flag_icon(p.get_flag(date)), escape_html_text(get_desc(&p.description)),
                get_normalized_note(p.get_note(date).as_deref(), ReportMode::Html, true),
                if amount >= 0.0 { amt.clone() } else { String::new() }, if amount < 0.0 { amt } else { String::new() }));
            shade = !shade;
            last_date = date;
        }
    };
    const NO_ENTRIES: &str = "<TR bgcolor=ffffff><TD><TD><TD COLSPAN=4><FONT COLOR=gray>No entries!</TR><TR></TR>";
    w.write("<TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** PAID ***</FONT></TR>"); // quirk: bgcolor=#FCF#CF literal
    if paid.is_empty() { w.write(NO_ENTRIES); } else {
        write_rows(&mut w, &paid);
        w.write(&format!("<TR><TD COLSPAN=4 ALIGN=RIGHT>PAID SUBTOTALS:{}", TranSum::of(&paid).cells()));
    }
    w.write("<TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** UNPAID ***</FONT></TR>");
    if unpaid.is_empty() { w.write(NO_ENTRIES); } else {
        write_rows(&mut w, &unpaid);
        w.write(&format!("<TR><TD COLSPAN=4 ALIGN=RIGHT>UNPAID SUBTOTALS:{}", TranSum::of(&unpaid).cells()));
        if !important.is_empty() {
            w.write(&format!("<TR><TD COLSPAN=4 ALIGN=RIGHT><font color=red>&#x26A0</font> IMPORTANT SUBTOTALS:{}", TranSum::of(&important).cells()));
        }
    }
    w.write("</BODY>\n</HTML>\n"); // quirk: no </TABLE>
    w.out
}

// ---------------------------------------------------------------------------------------------
// §10 Printed monthly calendar -> saveHTML
// ---------------------------------------------------------------------------------------------

/// The `$HTML_STYLE` head constant (verbatim, including the unclosed `@media print{`).
pub const CALENDAR_HEAD: &str = "<HTML>\n<HEAD>\n<TITLE>Budget Report</TITLE>\n<style type='text/css'>\ntable { font-size:12px; border-width:0px; border-collapse: collapse; }\nth { white-space: nowrap; font-size:13px; font-family: Helvetica; background-color:#f0f0f3; border-width: 1px;padding: 8px; border-style: solid; border-color: #729ea5; text-align:center; }\ntd.day { white-space: nowrap; text-align: left; font-size: 17px; font-family: Terminus,Consolas,Profont,'Andale Mono'; font-weight: bold; border-width: 1px; border-bottom: 0px; padding: 8px; border-style: solid; border-color: #729ea5; }\ntd.bal { white-space: nowrap; text-align: right; font-size: 13px; font-family: Terminus,Consolas,Profont,'Andale Mono'; font-weight: bold; font-style:oblique; border-width: 1px; border-top: 0px; padding: 8px;border-style: solid; border-color: #729ea5;}\ntd.dsc { white-space: nowrap; font-size:12px; font-family: Helvetica; border-width: 0px; text-align: left; padding-top: 5px; }\ntd.amt { white-space: nowrap; font-size:12px; font-family: Terminus,Consolas,Profont,'Andale Mono'; border-width: 0px; text-align: right; padding-top: 5px; }\n@media print{ .noprint{  display:none; }</style>\n</HEAD>\n<BODY bgcolor=ffffff>\n<input class='noprint' type='checkbox' onchange=\"var v = (this.checked ? 'hidden' : 'visible'); [].forEach.call(document.querySelectorAll('.bamt'), function (el){el.style.visibility = v;});\"><span class='noprint'>hide running balance</span></input>\n";

/// `isDarkColor`: r < 10 && g < 10, or HSB brightness (max component / 255) < 0.7.
fn is_dark_color(rgb: u32) -> bool {
    let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
    (r < 10 && g < 10) || (r.max(g).max(b) as f32 / 255.0) < 0.7
}

/// `get_html_span_color(t, d)` -> (background, foreground).
fn html_span_color(pmt: &Payment, date: i32, opts: &CalendarOptions) -> (String, String) {
    if !opts.preserve_color_on_print {
        return ("white".into(), (if pmt.disabled { "#C0C0C0" } else { "black" }).into());
    }
    let mut bg: u32 = 0xffffff;
    let mut fg: u32 = if pmt.disabled { 0xc0c0c0 } else { 0 };
    match pmt.get_flag(date) {
        FLAG_PAID => bg = opts.colors.bg_paid,
        FLAG_IMPORTANT => bg = opts.colors.bg_important,
        _ => {}
    }
    let mut hi = pmt.get_hilighting(date);
    if hi == -1 { hi = pmt.default_hilighting(); }
    if hi != -1 {
        bg = hi as u32 & 0xffffff;
        if is_dark_color(bg) { fg = 0xffffff; }
    }
    (format!("#{bg:06x}"), format!("#{fg:06x}"))
}

/// `saveHTML(path, start, end)` (§10): "Print Monthly Calendar" / "Print this month" / CLI `-r`.
/// Months `start..=end` (both are day-1 calendars in the original; the last month prints only when
/// `end` is one, exactly like `v8.eq(p2)`).
pub fn generate_monthly_calendar(db: &mut BudgetDb, start: &Cal, end: &Cal, opts: &CalendarOptions, images: ImageResolver) -> String {
    let fmt = |x: f64| if opts.show_decimals { format_frac2(x) } else { frac(x, 0) };
    let first_dow = if opts.first_day_sunday { 0 } else { 1 };
    let last_dow = first_dow + 6;
    let dow = |c: &Cal| { let w = c.day_of_week(); if first_dow != 0 && w == 0 { 7 } else { w } };
    let mut out = String::from(CALENDAR_HEAD);
    let mut m = *start;
    m.set_day_one();
    loop {
        let month = m.month();
        let mut d = m;
        let mut e = m;
        e.set_day_last();
        out.push_str(&format!("<P ALIGN=CENTER><FONT size=+2><B>{}</B></FONT></P>\n<TABLE BORDER=1 CELLSPACING=2 WIDTH=\"100%\" >\n<TR>", sdate_my(&m)));
        for i in 0..7 { out.push_str(&format!("<TH>{}", WDAY_NAMES[((first_dow + i) % 7) as usize])); }
        while dow(&d) != first_dow { d.add(Field::Day, -1); }
        while dow(&e) != last_dow { e.add(Field::Day, 1); }
        e.add(Field::Day, 1);
        let (mut expenses, mut income) = (0.0, 0.0); // expenses accumulated as a positive magnitude
        while d.lt(&e) {
            if dow(&d) == first_dow { out.push_str("<TR>"); }
            if month != d.month() {
                out.push_str("<TD class='day' WIDTH=\"14%\">&nbsp;");
            } else {
                out.push_str(&format!("<TD class='day' VALIGN=TOP WIDTH=\"14%\">{}<BR>\n", d.day()));
                let day = db.get_day(&d).cloned().unwrap_or_default();
                if day.adjustment_diff < 0.0 { expenses -= day.adjustment_diff; } else { income += day.adjustment_diff; }
                out.push_str("<TABLE BORDER=0 CELLSPACING=1 WIDTH=\"100%\">\n");
                for t in &day.transactions {
                    let pmt = &db.payments[t.pmt];
                    if !pmt.disabled { if t.amount < 0.0 { expenses -= t.amount; } else { income += t.amount; } }
                    let img = match &pmt.image_path {
                        Some(p) if !p.is_empty() => format!("<IMG WIDTH=\"12\" HEIGHT=\"12\" SRC=\"{}\">", embedded_src(&format!("file://images/{p}"), false, images)),
                        _ => String::new(),
                    };
                    let (bg, fg) = html_span_color(pmt, d.get(), opts);
                    out.push_str(&format!("<TR><TD class='dsc'>{img}<SPAN style='background-color: {bg};'><FONT COLOR={fg}>{}</FONT></SPAN><TD class='amt'>{}{}</TR>\n",
                        unicodish(&escape_html_text(get_desc(&pmt.description))),
                        amount2html(t.amount, Some("tamt"), if pmt.disabled { DISABLED_COLORS } else { NORMAL_COLORS }, opts.show_decimals),
                        if t.amount != pmt.amount { "'" } else { "&nbsp;" }));
                }
                out.push_str("</TABLE>\n");
            }
            if dow(&d) == last_dow {
                out.push_str("</TR><TR>\n");
                d.add(Field::Day, -6);
                for _ in 0..7 {
                    if month != d.month() {
                        out.push_str("<TD class='bal' WIDTH=\"14%\">&nbsp;");
                    } else {
                        let day = db.get_day(&d).cloned().unwrap_or_default();
                        out.push_str(&format!("<TD class='bal'>{}{}\n",
                            amount2html(day.closing_balance_display(opts.show_decimals), Some("bamt"), NORMAL_COLORS, opts.show_decimals),
                            if day.closing_balance_adjusted { "'" } else { "&nbsp;" }));
                    }
                    d.add(Field::Day, 1);
                }
                d.add(Field::Day, -1);
                out.push_str("</TR>\n");
            }
            d.add(Field::Day, 1);
        }
        out.push_str(&format!("</TR>\n</TABLE>\n<P class='bal' ALIGN=CENTER><B>Totals:</B> Income = <B>{}</B>, Expenses = <B>{}</B> ({})</P>\n<BR><BR>", fmt(income), fmt(expenses), fmt(income - expenses)));
        m.add(Field::Month, 1);
        if !(m.lt(end) || m == *end) { break; }
    }
    out.push_str("</BODY>\n</HTML>\n");
    out
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bgt;
    use std::path::Path;

    const DEC: i32 = 20211201;

    fn sample() -> BudgetDb {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference/sample.bgt");
        let text = bgt::latin1_decode(&std::fs::read(p).unwrap());
        let l = bgt::load_str(&text, DEC, None).unwrap_or_else(|l| panic!("{}", l.log));
        BudgetDb::from_loaded(l, &Cal::from_int(DEC), 12, false)
    }

    fn no_images(_: &str) -> Option<String> { None }

    #[test]
    fn mbm_consistency_check_tolerates_float_noise() {
        // one ulp above 40874.0 differs by one ceil() unit but is the same amount
        assert!(amounts_match(f64::from_bits(40874.0f64.to_bits() + 1), 40874.0));
        assert!(amounts_match(-2732.0000000000005, -2732.0));
        assert!(!amounts_match(10.0, 10.01));
    }

    /// Transactions of Dec 2021 as `(date, pmt index, amount)` in report order.
    fn december(db: &mut BudgetDb) -> Vec<(i32, usize, f64)> {
        let mut v = Vec::new();
        let mut c = Cal::from_int(DEC);
        while c.month() == 11 {
            if let Some(d) = db.get_day(&c) { for t in &d.transactions { v.push((c.get(), t.pmt, t.amount)); } }
            c.add(Field::Day, 1);
        }
        v
    }

    #[test]
    fn shared_helpers_match_spec() {
        assert_eq!(amount2html(-1234.5, None, NORMAL_COLORS, true), "<FONT color=red>-1,234<small>.50</small></font>");
        assert_eq!(amount2html(100.0, Some("tamt"), NORMAL_COLORS, false), "<FONT color=black class='tamt'>100</font>");
        assert_eq!(amount2html(-0.0, None, NORMAL_COLORS, true), "<FONT color=black>0<small>.00</small></font>");
        assert_eq!(f2html_no_zero(0.99, true), "");
        assert_eq!(f2html_no_zero(-1.5, false), "<FONT color=red>-2</font>");
        assert_eq!(escape_html_text("a<b & \"c\" 'd' \u{e9} \u{2603}"), "a&lt;b &amp; &quot;c&quot; &#39;d&#39; \u{e9} &#x2603;");
        assert_eq!(escape_html_text(""), "");
        assert_eq!(get_desc("Rent;extra;more"), "Rent");
        assert_eq!(get_desc("   Salary"), "   Salary");
        assert_eq!(get_normalized_note(None, ReportMode::Html, true), "");
        assert_eq!(get_normalized_note(Some("a\nb"), ReportMode::Excel, false), "  (Note: a...)");
        assert_eq!(get_normalized_note(Some("a"), ReportMode::Excel, false), "  (Note: a)");
        assert_eq!(get_normalized_note(Some("a\nb"), ReportMode::Html, true), "<BR><FONT COLOR=gray>Note: <I>a<BR>b</I></FONT>");
        let mut p = Payment::new(DEC);
        assert_eq!(html_string(&p, "x"), "x");
        p.disabled = true;
        assert_eq!(html_string(&p, "x"), "<FONT color=gray>x</FONT>");
        assert_eq!(excel_number_style(), "STYLE=\"vnd.ms-excel.numberformat:###0.00\" ");
        assert_eq!(repeat("ab", 2), "abab");
        assert_eq!(repeat("ab", 0), "");
        assert_eq!(html_merge_styles("  ", "x"), " style='x'");
        assert_eq!(html_merge_styles(STYLE_SEP, "b;"), " style='padding-top: 1em; padding-bottom: 1em; border-top: 1px solid black;b;'");
        assert_eq!(unicodish("a,b\n\r\u{e9}\u{1F600}"), "a&#x002c;b&#x000a;&#x000d;&#x00e9;&#xd83d;&#xde00;");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64_data_uri(b"GIF"), "data:;base64,R0lG");
        let resolver = |p: &str| if p == "icons/red_g.gif" { Some(base64_data_uri(b"GIF")) } else { None };
        assert_eq!(embedded_src("file://icons/red_g.gif", false, &resolver), "data:;base64,R0lG");
        assert_eq!(embedded_src("file://icons/red_g.gif", true, &resolver), "file://icons/red_g.gif");
        assert_eq!(embedded_src("file://icons/other.gif", false, &resolver), "file://icons/other.gif");
        assert_eq!(gen_rep_image(-50.0, 100.0, -200.0, 300, &no_images), "<img src=\"file://icons/red_g.gif\" height=10 width=75>\n");
        assert_eq!(gen_rep_image(50.0, 0.0, 0.0, 300, &resolver), "<img src=\"file://icons/blue_g.gif\" height=10 width=15000>\n");
        assert_eq!(frac(1234.56, 1), "1,234.6");
        assert_eq!(frac(-0.4, 0), "-0");
        assert_eq!(signed0(12.0), "+12");
        assert_eq!(signed0(-1200.0), "-1,200");
        assert!(is_dark_color(0x000080) && is_dark_color(0x050500) && !is_dark_color(0xffff00));
    }

    #[test]
    fn stats_report_dec_2021() {
        let mut db = sample();
        let st = calc_statistics(&mut db, &Cal::from_int(DEC), 1, true, None);
        let html = generate_stats_report(&st, &no_images);
        let head = "<HTML>\n<HEAD>\n\t<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n<style type=\"text/css\">\nBODY { font: 10px Arial, Helvetica, sans-serif; }\nP, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }\n</style>\n<TABLE BORDER=0 CELLSPACING=2>\n<CAPTION>Budget from <B>01-Dec-2021</B> to <B>31-Dec-2021</B></CAPTION>\n<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B><TD><B>Graph</B>";
        assert!(html.starts_with(&unicodish(head)));
        // Disposable Income = 3000 - 3030 = -30, -1.0% of income, planned diff 0 -> empty
        assert!(html.contains(&unicodish("<TR bgcolor=FFffFF><TD>Disposable Income<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;-30.00<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;-1.0<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;<TD><img src=\"file://icons/red_g.gif\" height=10 width=2>\n")));
        assert!(html.contains(&unicodish("<TR bgcolor=EEeeEE><TD>Total Income<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;3,000.00<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;100.0<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;<TD><img src=\"file://icons/blue_g.gif\" height=10 width=297>\n")));
        // leaf: nbsp3 (indented) + nbsp4 + the key's own 3 spaces
        assert!(html.contains(&unicodish("<TD>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;   Salary<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;3,000.00<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;100.0")));
        assert!(html.contains(&unicodish("<TD>Total Expenses<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;-3,030.00<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;-101.0")));
        // row 6 (Disposable, Total Income, Uncat-Income, Salary, Total Expenses, ...) -> shaded
        assert!(html.contains("<TR bgcolor=EEeeEE><TD>&nbsp;&nbsp;&nbsp;(Uncategorized-Expenses)<TD ALIGN=RIGHT>"));
        assert!(html.ends_with("</TABLE></HTML>"));
        // embedded image
        let resolver = |p: &str| Some(format!("data:;base64,{p}"));
        // (the data URI's comma is escaped too: it is written through the unicodish writer, as in the original)
        assert!(generate_stats_report(&st, &resolver).contains(&unicodish("<img src=\"data:;base64,icons/blue_g.gif\" height=10 width=297>")));
    }

    #[test]
    fn excel_stats_report_dec_2021() {
        let mut db = sample();
        let st = calc_statistics(&mut db, &Cal::from_int(DEC), 1, true, None);
        let html = generate_excel_stats_report(&st);
        let s = excel_number_style();
        assert!(html.starts_with(&unicodish("<HTML>\n<HEAD>\n\t<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n<style type=\"text/css\">\nBODY { font: 10px Arial, Helvetica, sans-serif; }\nP, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }\n</style>\n<TABLE BORDER=0 CELLSPACING=2>\n<CAPTION>Budget from <B>01-Dec-2021</B> to <B>31-Dec-2021</B></CAPTION>\n<TR bgcolor=eeEEee><TD><B>Description</B><TD ALIGN=RIGHT><B>Amount</B><TD ALIGN=RIGHT><B>%</B><TD ALIGN=RIGHT><B>&#43;&#47;&#45;</B>")));
        // incomeVec = [(Uncategorized-Income), Salary] -> Total Expenses on row 6
        assert!(html.contains(&unicodish(&format!("<TR><TD><B>Disposable Income:</B><TD ALIGN=RIGHT {s} FORMULA='=B3+B6'>0.001<TD ALIGN=RIGHT {s} FORMULA='=B2/B3*100'>?\n<TD ALIGN=RIGHT>\n"))));
        assert!(html.contains(&unicodish(&format!("<TR><TD><B>Total Income:</B><TD ALIGN=RIGHT {s} FORMULA=\"=+B4\" >0.001<TD ALIGN=RIGHT {s}>100.00\n<TD ALIGN=RIGHT><FONT COLOR=aaAAaa>0.00</FONT>\n"))));
        assert!(html.contains(&unicodish(&format!("<TR><TD>&nbsp;&nbsp;&nbsp;(Uncategorized-Income)<TD ALIGN=RIGHT {s} FORMULA='=SUM(B5..B5)'>3,000.00<TD ALIGN=RIGHT {s}FORMULA='=B4/B3*100.0'>?\n"))));
        assert!(html.contains(&unicodish(&format!("<TR><TD>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;   Salary<TD ALIGN=RIGHT {s}>3,000.00<TD ALIGN=RIGHT {s}FORMULA='=B5/B3*100.0'>?\n"))));
        // expenseVec = [Uncat-Exp(7) + 5 leaves, Auto(13) + 3, Utilities(17) + 2]
        assert!(html.contains(&unicodish(&format!("<TR><TD><B>Total Expenses:</B><TD ALIGN=RIGHT {s} FORMULA=\"=+B7+B13+B17\" >0.001<TD ALIGN=RIGHT {s}FORMULA='=B6/B3*100.0'>?\n"))));
        assert!(html.contains(&unicodish(&format!("(Uncategorized-Expenses)<TD ALIGN=RIGHT {s} FORMULA='=SUM(B8..B12)'>-2,300.00"))));
        assert!(html.ends_with("</TABLE></HTML>"));
    }

    #[test]
    fn comparison_report_and_comp_entry_merge() {
        let row = |key: &str, amount: f64, is_sub: bool| Row { key: key.into(), amount, planned_amount: 0.0, pmt: None, is_sub };
        let merged = comp_entry_from(&[row("a", 10.0, false), row("   b", 0.0, true), row("z", 0.0, false)], &[row("   b", 5.0, true), row("c", 7.0, false)]);
        assert_eq!(merged, vec![
            CompEntry { key: "a".into(), v1: 10.0, v2: 0.0 },
            CompEntry { key: "   b".into(), v1: 0.0, v2: 5.0 },
            CompEntry { key: "c".into(), v1: 0.0, v2: 7.0 },
        ]);
        let mut db = sample();
        let base = calc_statistics(&mut db, &Cal::from_int(DEC), 1, true, None);
        let other = calc_statistics(&mut db, &Cal::from_int(20211101), 1, true, None);
        let html = generate_comparison_report(&base, &other, &no_images);
        assert!(html.starts_with(&unicodish("<HTML>\n<HEAD>\n<TITLE>Budget Comparison Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n<style type=\"text/css\">\nBODY { font: 10px Arial, Helvetica, sans-serif; }\nP, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }\n</style>\n<TABLE BORDER=0 CELLSPACING=2>\n<CAPTION>Stats comparison of two ranges <B>Dec, 2021</B> vs <B>Nov, 2021</B></CAPTION>\n<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=CENTER><B>Dec, 2021<br>(base)<br>amount</B><TD ALIGN=CENTER><B>Nov, 2021<br>(other)<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>amount</B><TD ALIGN=CENTER><B>&#43;&#47;&#45;<br>%</B><TD><B>Graph</B>")));
        // Total Income: 3,000 (Dec, 2 pay days) vs 3,000 (Nov) -> no diff cells; bar = other/max(|base totals|) = 3000/3030
        assert_eq!(other.total_income, 3000.0);
        assert!(html.contains(&unicodish(&format!("<TR bgcolor=EEeeEE><TD>Total Income<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;{0}<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;{0}<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;<TD><img src=\"file://icons/blue_g.gif\" height=10 width=297>\n", f2html(3000.0)))));
        assert!(html.contains(&unicodish(&format!("<TD>&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;   Rent<TD ALIGN=RIGHT>&nbsp;&nbsp;&nbsp;{}", f2html(-1100.0)))));
        assert_eq!(range_string(&calc_statistics(&mut db, &Cal::from_int(DEC), 3, true, None)), "Dec, 2021 - Feb, 2022 (3m)");
        assert!(html.ends_with("</TABLE></HTML>"));
    }

    #[test]
    fn month_by_month_report_dec_2021_to_feb_2022() {
        let mut db = sample();
        let single = calc_statistics(&mut db, &Cal::from_int(DEC), 1, true, None);
        assert_eq!(generate_month_by_month_report(&mut db, &single, false).unwrap_err(), "Please select more than one month in the Full Stats dialog.");
        let st = calc_statistics(&mut db, &Cal::from_int(DEC), 3, true, None);
        let entries = month_by_month_entries(&mut db, &st).unwrap();
        assert_eq!(entries[0].description, "Disposable Income");
        assert_eq!(entries[0].amounts.len(), 3);
        assert_eq!(entries[0].amounts[0], -30.0);
        assert_eq!(entries[1].description, "Total Income");
        assert_eq!(entries[1].amounts[0], 3000.0);
        assert_eq!(entries[1].total, st.total_income);
        let te = entries.iter().find(|e| e.description == "Total Expenses").unwrap();
        assert_eq!(te.total, st.total_expense);
        let auto = entries.iter().find(|e| e.description == "Auto").unwrap();
        assert!(auto.is_aggregate() && !auto.special);
        assert_eq!(auto.amounts[0], -600.0);
        let gas = entries.iter().find(|e| e.description == "   Gasoline").unwrap();
        assert_eq!(gas.category.as_deref(), Some("Auto"));
        assert_eq!(gas.amounts[0], -120.0);
        let html = generate_month_by_month_report(&mut db, &st, false).unwrap();
        assert!(html.starts_with("<HTML>\n<HEAD>\n<TITLE>Budget Month-by-Month Comparison Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n<style type=\"text/css\">\nBODY { font: 10px Arial, Helvetica, sans-serif; }\nP, TD, TH, UL, DIV, .yspgen { font: 12px Arial, Helvetica, sans-serif; }\ntable tr:hover td { background: #ffffc5; cursor: pointer; }\n</style>\n"));
        assert!(html.contains(&(unicodish("<CAPTION>Month-By-Month report <B>Dec, 2021 - Feb, 2022 (3m)</B></CAPTION>\n<TR bgcolor=ddDDdd><TD><B>Description</B><TD ALIGN=CENTER><B>Dec, 2021<br>amount</B><TD ALIGN=CENTER><B>Jan, 2022<br>amount</B><TD ALIGN=CENTER><B>Feb, 2022<br>amount</B><TD ALIGN=CENTER><B>Total<br>amount</B><TD ALIGN=CENTER><B>Avg/month<br>amount</B>") + "\n<TR bgcolor=ffFFff>")));
        assert!(html.contains(&format!("\n<TR bgcolor=ffFFff><TD{STYLE_SEP}><B>Disposable Income<TD align=right{STYLE_SEP}><span title='base amount'><B>{}</span>", f2html_no_zero(-30.0, false))));
        for cat in ["(Uncategorized-Income)", "Total Income", "Total Expenses", "(Uncategorized-Expenses)", "Auto", "Utilities"] {
            assert!(html.contains(&format!("><B>{cat}<TD align=right")), "{cat}");
        }
        for leaf in ["Salary", "Rent", "Savings", "Groceries", "Visa Pmt", "TV", "Car Loan", "Gasoline", "Car Insurance", "Hydro", "Mobile"] {
            assert!(html.contains(&format!("><TD>&nbsp;&nbsp;&nbsp;&nbsp;   {leaf}<TD align=right><span title='base amount' style='font-size: 10px'>")), "{leaf}");
        }
        assert!(html.contains("<TD align=right style='border-left: 1px solid black; border-right: 1px solid black;'><span style='font-size: 10px'>&nbsp;&nbsp;&nbsp;"));
        assert!(html.ends_with("</TABLE></HTML>"));
    }

    #[test]
    fn monthly_detailed_report_dec_2021() {
        let mut db = sample();
        let trans = december(&mut db);
        assert!(!trans.is_empty());
        let (start, end) = (Cal::from_int(DEC), Cal::from_int(DEC));
        let excel = generate_report(&mut db, &start, &end, ReportMode::Excel, &no_images);
        assert!(excel.starts_with(&unicodish("<HTML>\n<HEAD>\n<TITLE>Budget Report</TITLE>\n</HEAD>\n<BODY bgcolor=ffffff>\n<TABLE BORDER=0 CELLSPACING=2><CAPTION>Budget from <B>01-Dec-2021</B> to <B>31-Dec-2021</B></CAPTION>\n<TR bgcolor=EEEEEE><TD><B>Date</B><TD><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2><B>Amount</B><TD><B>Balance</B>\n<TR bgcolor=ffffff><TD ALIGN=CENTER><TD><TD ALIGN=LEFT><TD ALIGN=LEFT><B>Opening balance")));
        assert!(excel.contains(&unicodish(&format!("<B>Opening balance{}</B>\n<TD ALIGN=RIGHT><TD ALIGN=RIGHT><TD ALIGN=RIGHT LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\" ><B>", repeat("&nbsp;", 30)))));
        // one row per transaction, rows 3.., each with =G{r-1}+E{r}+F{r}
        for (i, (date, pi, amount)) in trans.iter().enumerate() {
            let r = i + 3;
            let formula = format!(" FORMULA=\"=G{}+E{r}+F{r}\"", r - 1);
            let cell = if *amount < 0.0 { format!("<TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\" ><TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\" >{amount:.2}<TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  {formula}>", if i % 2 == 0 { "eeeeee" } else { "ffffff" }) }
                       else { format!("<TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\" >{amount:.2}<TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\" ><TD ALIGN=RIGHT BGCOLOR=\"#{0}\" LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  {formula}>", if i % 2 == 0 { "eeeeee" } else { "ffffff" }) };
            assert!(excel.contains(&unicodish(&cell)), "row {r}: {cell}");
            let date_cell = if i == 0 || trans[i - 1].0 != *date { sdate_dmy(*date) } else { String::new() };
            assert!(excel.contains(&unicodish(&format!("<TD ALIGN=CENTER BGCOLOR=\"#{}\">{date_cell}<TD WIDTH=18>", if i % 2 == 0 { "eeeeee" } else { "ffffff" }))));
            assert!(excel.contains(&unicodish(&db.payments[*pi].description)));
        }
        assert_eq!(excel.matches("+E").count(), trans.len());
        let last = trans.len() + 2;
        assert!(excel.contains(&unicodish(&format!("<B>Totals:</B><TD ALIGN=RIGHT  LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  FORMULA=\"=@sum(E3..E{last})\"><B>3000.00</B><TD ALIGN=RIGHT  LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  FORMULA=\"=@sum(F3..F{last})\"><B>-3030.00</B><TD ALIGN=RIGHT  LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  FORMULA=\"=E{}+F{}\"><B>-30.00</B></TR>\n", last + 1, last + 1))));
        assert!(excel.contains("<font color=red>&#x26A0</font> Hydro"));
        assert!(!excel.contains("</TABLE>"));
        assert!(excel.ends_with(&unicodish("</BODY>\n</HTML>\n")));

        // HTML mode: the Expenses total cell shows the expenses total (the original repeated income)
        // (HTML mode counts the opening balance as 4 rows, so the data rows are 4..=n+3 while firstDataRow stays 3)
        let html = generate_report(&mut db, &start, &end, ReportMode::Html, &no_images);
        let lh = last + 1;
        assert!(html.contains(&unicodish(&format!("FORMULA=\"=@sum(E3..E{lh})\"><B>{0}</B><TD ALIGN=RIGHT  LOTUS_FORMAT=\"Fixed;2\"  FORMULA=\"=@sum(F3..F{lh})\"><B>{3}</B><TD ALIGN=RIGHT  LOTUS_FORMAT=\"Fixed;2\"  FORMULA=\"=E{1}+F{1}\"><B>{2}</B>", f2html(3000.0), lh + 1, f2html(-30.0), f2html(-3030.0)))));
        assert!(html.contains(&unicodish(" FORMULA=\"=G3+E4+F4\"")) && !html.contains(&unicodish(" FORMULA=\"=G2+E3+F3\"")));
        assert!(html.contains(&unicodish("</TABLE>\n<P><BR></P>")));
        assert_eq!(html.matches("<CAPTION>").count(), 1);
        // quirk: the "To" month is excluded -> Dec..Feb prints Dec and Jan only
        let two = generate_report(&mut db, &start, &Cal::from_int(20220201), ReportMode::Excel, &no_images);
        assert_eq!(two.matches("Budget from <B>").count(), 2);
        assert!(two.contains(&unicodish("<TR><TR><TR><TD COLSPAN=7><BR><CENTER>Budget from <B>01-Jan-2022</B> to <B>31-Jan-2022</B></CENTER></TR>\n")));
        // month 1 closes with =G{last}; month 2's opening formula is computed before its 2 caption rows are counted
        assert!(two.contains(&unicodish(&format!("FORMULA=\"=G{last}\"><B>"))));
        assert!(two.contains(&unicodish(&format!("<TD ALIGN=RIGHT LOTUS_FORMAT=\"Fixed;2\" STYLE=\"vnd.ms-excel.numberformat:###0.00\"  FORMULA=\"=G{}\"><B>", last + 2))));
    }

    #[test]
    fn paid_unpaid_report_dec_2021() {
        let mut db = sample();
        let trans = december(&mut db);
        let html = generate_paid_unpaid_report(&mut db, &Cal::from_int(DEC), &Cal::from_int(20211231), &no_images);
        assert!(html.starts_with(&unicodish("<HTML>\n<HEAD><TITLE>Budget Report</TITLE></HEAD><BODY bgcolor=ffffff>\n<TABLE BORDER=0 CELLSPACING=2><CAPTION>Paid/Unpaid Report from <B>01-Dec-2021</B> to <B>31-Dec-2021</B></CAPTION>\n<TR bgcolor=EEEEEE><TD><B>Date</B><TD><!--image--><TD><B>Category</B><TD><B>Payee</B><TD COLSPAN=2 ALIGN=CENTER><B>Amount</B></TR>\n<TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** PAID ***</FONT></TR><TR bgcolor=ffffff><TD><TD><TD COLSPAN=4><FONT COLOR=gray>No entries!</TR><TR></TR><TR bgcolor=#FCF#CF><TD COLSPAN=6 ALIGN=CENTER><FONT SIZE=+2>*** UNPAID ***</FONT></TR>")));
        let c = |x| amount2html(x, None, NORMAL_COLORS, true);
        assert!(html.contains(&unicodish(&format!("<TR><TD COLSPAN=4 ALIGN=RIGHT>UNPAID SUBTOTALS:<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}</TR><TR></TR>", c(3000.0), c(-3030.0), c(-30.0)))));
        // Hydro's default flag is IMPORTANT (`-100.0#`)
        assert!(html.contains(&unicodish(&format!("<TR><TD COLSPAN=4 ALIGN=RIGHT><font color=red>&#x26A0</font> IMPORTANT SUBTOTALS:<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}<TD ALIGN=RIGHT>{}</TR><TR></TR></BODY>\n</HTML>\n", c(0.0), c(-100.0), c(-100.0)))));
        let hydro = trans.iter().position(|(_, pi, _)| db.payments[*pi].description == "Hydro").unwrap();
        let bg = if hydro % 2 == 0 { "ffffff" } else { "eeeeee" };
        assert!(html.contains(&unicodish(&format!("<TD WIDTH=18><IMG SRC=\"file://images/power1.png\"><TD ALIGN=LEFT BGCOLOR=\"#{bg}\">Utilities<TD ALIGN=LEFT BGCOLOR=\"#{bg}\"><font color=red>&#x26A0</font> Hydro<TD ALIGN=RIGHT><TD ALIGN=RIGHT>{}</TR>\n", c(-100.0)))));
        assert_eq!(html.matches("<TD WIDTH=18>").count(), trans.len());
        // date printed once per day, first row white
        let first = trans[0];
        assert!(html.contains(&unicodish(&format!("<TR BGCOLOR=\"#ffffff\"><TD ALIGN=CENTER BGCOLOR=\"#ffffff\">{}<TD WIDTH=18>", sdate_dmy(first.0)))));
        assert!(!html.contains("</TABLE>"));
    }

    /// One October 2026 payment whose description, category and note carry markup, paid on the 5th.
    fn markup_db() -> BudgetDb {
        let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
        let mut p = Payment::new(20261005);
        p.description = "Fish & <Chips> \"x\"".into();
        p.category = Some("Food<Out>".into());
        p.amount = -12.5;
        db.add(p);
        db.payments[0].set_note(20261005, Some("<b>bold</b>"));
        db.payments[0].set_flag(20261005, FLAG_PAID);
        db
    }

    #[test]
    fn user_text_is_escaped_in_every_report() {
        let mut db = markup_db();
        let (oct, last) = (Cal::from_int(20261001), Cal::from_int(20261031));
        let texts = ["Fish &amp; &lt;Chips&gt; &quot;x&quot;", "Food&lt;Out&gt;", "&lt;b&gt;bold&lt;/b&gt;"];
        let raw = ["<Chips>", "Food<Out>", "<b>bold</b>", "Fish & <"];
        let check = |name: &str, html: &str, texts: &[&str]| {
            for t in texts { assert!(html.contains(t), "{name} lacks {t}"); }
            for r in raw { assert!(!html.contains(r), "{name} writes {r} raw"); }
        };
        for mode in [ReportMode::Html, ReportMode::Excel] {
            check(&format!("detailed {mode:?}"), &generate_report(&mut db, &oct, &oct, mode, &no_images), &texts);
        }
        check("paid/unpaid", &generate_paid_unpaid_report(&mut db, &oct, &last, &no_images), &texts);
        let calendar = generate_monthly_calendar(&mut db, &oct, &oct, &CalendarOptions::default(), &no_images);
        check("calendar", &calendar, &texts[..1]);
        // the statistics reports escape the same text
        let st = calc_statistics(&mut db, &oct, 1, true, None);
        check("stats print", &generate_stats_report(&st, &no_images), &texts[..1]);
        check("stats excel", &generate_excel_stats_report(&st), &texts[..1]);
        check("comparison", &generate_comparison_report(&st, &st, &no_images), &texts[..1]);
        let range = crate::stats::calc_statistics_exact(&mut db, &oct, &Cal::from_int(20261130), true, None);
        check("month by month", &generate_month_by_month_report(&mut db, &range, false).unwrap(), &texts[..1]);
    }

    #[test]
    fn comparison_cents_do_not_saturate_above_the_i32_range() {
        // 25,000,000.00 * 100 is beyond i32::MAX: the cents used to saturate at 21,474,836.47.
        let salary = Row { key: "   Salary".into(), amount: 25e6, planned_amount: 0.0, pmt: None, is_sub: true };
        let base = Stats { total_income: 25e6, income: vec![salary], ..Default::default() };
        let html = generate_comparison_report(&base, &Stats::default(), &no_images);
        assert!(html.contains(&unicodish(&f2html(25_000_000.0))), "{html}");
        assert!(!html.contains(&unicodish("21,474,836")));
    }

    #[test]
    fn monthly_calendar_dec_2021() {
        let mut db = sample();
        let (start, end) = (Cal::from_int(DEC), Cal::from_int(DEC));
        let opts = CalendarOptions { show_decimals: true, ..Default::default() };
        let html = generate_monthly_calendar(&mut db, &start, &end, &opts, &no_images);
        assert!(html.starts_with(CALENDAR_HEAD));
        assert!(html.contains("@media print{ .noprint{  display:none; }</style>"));
        assert!(html.contains("<P ALIGN=CENTER><FONT size=+2><B>December-2021</B></FONT></P>\n<TABLE BORDER=1 CELLSPACING=2 WIDTH=\"100%\" >\n<TR><TH>Sunday<TH>Monday<TH>Tuesday<TH>Wednesday<TH>Thursday<TH>Friday<TH>Saturday<TR><TD class='day' WIDTH=\"14%\">&nbsp;<TD class='day' WIDTH=\"14%\">&nbsp;<TD class='day' WIDTH=\"14%\">&nbsp;<TD class='day' VALIGN=TOP WIDTH=\"14%\">1<BR>\n<TABLE BORDER=0 CELLSPACING=1 WIDTH=\"100%\">\n"));
        assert!(html.contains("<P class='bal' ALIGN=CENTER><B>Totals:</B> Income = <B>3,000.00</B>, Expenses = <B>3,030.00</B> (-30.00)</P>\n<BR><BR></BODY>\n</HTML>\n"));
        assert!(html.contains("<TR><TD class='dsc'><IMG WIDTH=\"12\" HEIGHT=\"12\" SRC=\"file://images/power1.png\"><SPAN style='background-color: white;'><FONT COLOR=black>Hydro</FONT></SPAN><TD class='amt'><FONT color=red class='tamt'>-100<small>.00</small></font>&nbsp;</TR>\n"));
        assert!(html.contains("<TD class='bal'><FONT color="));
        assert!(html.contains("</TR><TR>\n<TD class='bal' WIDTH=\"14%\">&nbsp;<TD class='bal' WIDTH=\"14%\">&nbsp;<TD class='bal' WIDTH=\"14%\">&nbsp;<TD class='bal'>"));
        assert_eq!(html.matches("<TD class='day'").count(), 35);

        let opts = CalendarOptions { preserve_color_on_print: true, first_day_sunday: false, ..Default::default() };
        let html = generate_monthly_calendar(&mut db, &start, &end, &opts, &no_images);
        assert!(html.contains("<TR><TH>Monday<TH>Tuesday<TH>Wednesday<TH>Thursday<TH>Friday<TH>Saturday<TH>Sunday"));
        assert!(html.contains("Income = <B>3,000</B>, Expenses = <B>3,030</B> (-30)</P>"));
        assert!(html.contains("<SPAN style='background-color: #ffff00;'><FONT COLOR=#000000>Hydro</FONT></SPAN><TD class='amt'><FONT color=red class='tamt'>-100</font>&nbsp;"));
        assert!(html.contains("<SPAN style='background-color: #ffffff;'><FONT COLOR=#000000>Rent</FONT></SPAN>"));
        // Dec 2021 starts on a Wednesday: two leading blanks with a Monday week; 5 weeks
        assert!(html.contains("<TR><TD class='day' WIDTH=\"14%\">&nbsp;<TD class='day' WIDTH=\"14%\">&nbsp;<TD class='day' VALIGN=TOP WIDTH=\"14%\">1<BR>"));
        assert_eq!(html.matches("<TD class='day'").count(), 35);
        // two months inclusive
        let two = generate_monthly_calendar(&mut db, &start, &Cal::from_int(20220101), &opts, &no_images);
        assert!(two.contains("<B>December-2021</B>") && two.contains("<B>January-2022</B>"));
        assert_eq!(two.matches("<B>Totals:</B>").count(), 2);
    }
}
