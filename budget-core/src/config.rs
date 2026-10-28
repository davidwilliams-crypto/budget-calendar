//! `.budgetrc`: Java `Properties` text file + the typed `BudgetConfig` (analysis/09).

use crate::date::Cal;
use std::path::Path;

// ---------------------------------------------------------------- Properties

/// Parse Java `Properties` text (`key=value`, `#`/`!` comments, `\` escapes, line continuations).
pub fn parse_properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut logical = String::new();
    let mut lines = text.lines().peekable();
    while let Some(raw) = lines.next() {
        let l = raw.trim_start_matches([' ', '\t', '\x0c']);
        if logical.is_empty() && (l.is_empty() || l.starts_with('#') || l.starts_with('!')) { continue; }
        // continuation: odd number of trailing backslashes
        let bs = l.chars().rev().take_while(|c| *c == '\\').count();
        if bs % 2 == 1 {
            logical.push_str(&l[..l.len() - 1]);
            continue;
        }
        logical.push_str(l);
        let line = std::mem::take(&mut logical);
        // split key / value
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut key = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() { key.push(c); key.push(chars[i + 1]); i += 2; continue; }
            if c == '=' || c == ':' || c == ' ' || c == '\t' { break; }
            key.push(c); i += 1;
        }
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') { i += 1; }
        if i < chars.len() && (chars[i] == '=' || chars[i] == ':') { i += 1; }
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') { i += 1; }
        let value: String = chars[i..].iter().collect();
        out.push((unescape(&key), unescape(&value)));
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' { out.push(c); continue; }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\x0c'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| it.next()).collect();
                out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('?'));
            }
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn escape(s: &str, is_key: bool) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        match c {
            ' ' if is_key || i == 0 => out.push_str("\\ "),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => { out.push('\\'); out.push(c); }
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `Properties.store(out, "")`: a `#` comment line, a date line, then `key=value` lines.
pub fn format_properties(props: &[(String, String)]) -> String {
    let mut s = format!("#\n#{}\n", Cal::today());
    for (k, v) in props {
        s.push_str(&escape(k, true));
        s.push('=');
        s.push_str(&escape(v, false));
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------- typed config

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme { Light, Dark }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStyle { Normal, Bold, Italic }

/// Default font recipe for a font preference; `scale` multiplies the system font height
/// (`h`, `s=0.9h`, `m=1.2h`, `l=1.5h`), `mono`/`system` pick the platform font name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontDefault { pub family: FontFamily, pub scale: f32, pub style: FontStyle }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFamily { Verdana, Mono, System }

/// `MONO`: Consolas on Windows, Monaco on Mac, Monospace on Linux, else Andale Mono.
pub fn mono_font_name() -> &'static str {
    if cfg!(target_os = "windows") { "Consolas" } else if cfg!(target_os = "macos") { "Monaco" } else if cfg!(target_os = "linux") { "Monospace" } else { "Andale Mono" }
}

macro_rules! colors {
    ($( $field:ident : $light:expr, $dark:expr ; )*) => {
        /// Field names are the `.budgetrc` keys on purpose (they double as the property names).
        #[allow(non_snake_case)]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct Colors { $( pub $field: u32, )* }
        impl Colors {
            pub fn theme(t: Theme) -> Colors {
                match t {
                    Theme::Light => Colors { $( $field: $light, )* },
                    Theme::Dark => Colors { $( $field: $dark, )* },
                }
            }
            fn entries(&self) -> Vec<(&'static str, u32)> { vec![ $( (stringify!($field), self.$field), )* ] }
            fn set(&mut self, key: &str, v: u32) -> bool {
                match key { $( stringify!($field) => { self.$field = v; true } )* _ => false }
            }
        }
    };
}

colors! {
    bgColorBalance: 0xe5f0ff, 0xeda6a6;
    bgColorMinBalance: 0xaed0ff, 0xf9c5ba;
    bgColorBelowMinBalance: 0xffff00, 0xffff00;
    bgColorMaxBalance: 0xdccce2, 0x84a8e2;
    bgColorCellOtherMonths: 0xcccccc, 0xa5a5a5;
    bgColorCell: 0xcce4ea, 0x7ca7ce;
    bgColorCellToday: 0xfdfdf4, 0xfff693;
    bgColorCellPast: 0xf4f4eb, 0xa6cceb;
    colorPositiveAmount: 0x0b077f, 0x003365;
    colorNegativeAmount: 0xff4c5e, 0xff4844;
    colorPosTranAmount: 0x000000, 0x000000;
    colorNegTranAmount: 0x8a0000, 0xff050d;
    colorNotes: 0xbc8f8f, 0xdfdfdf;
    bgColorNotes: 0xffffe0, 0x202020;
    bgColorHighlight: 0xd9d9d9, 0xfffe79;
    bgColorImportant: 0xffff00, 0xffff00;
    bgColorCellHighlight: 0xd9b3ff, 0xfafdca;
    bgColorPaid: 0xcadeb8, 0xff7f82;
    bgColorWindow: 0xffffff, 0x535253;
    bgColorDetails: 0xffffff, 0x000000;
    colorPastPayment: 0xff4c5e, 0xff4c5e;
    colorCalendarToday: 0xff4c5e, 0xff4c5e;
    colorFuturePayment: 0x4cb224, 0x4cb224;
    colorCalendarDay: 0x0b077f, 0x0b077f;
    colorCurrentMonth: 0x0018f4, 0x8080ff;
    colorDescText: 0x000000, 0x000000;
    bgColorCalendarHeader: 0x000000, 0x535353;
    colorCalendarHeader: 0xffffff, 0xfffaf2;
    bgColorDatePickerHeader: 0x000000, 0x000000;
    colorDatePickerHeader: 0xffffff, 0xffffff;
    colorDisabledPayment: 0x7c7c7c, 0x7c7c7c;
    graphTextColor: 0x000000, 0xffffff;
    graphGridColor: 0xcccccc, 0xcccccc;
    graphBarColor: 0x3684c9, 0x3662a1;
    graphNegativeBarColor: 0xff4c5e, 0xfb001a;
}

pub const FONT_KEYS: [(&str, FontDefault); 11] = [
    ("fontAmount", FontDefault { family: FontFamily::Mono, scale: 1.0, style: FontStyle::Italic }),
    ("fontCurMonthLink", FontDefault { family: FontFamily::Verdana, scale: 1.0, style: FontStyle::Bold }),
    ("fontTran", FontDefault { family: FontFamily::Verdana, scale: 1.0, style: FontStyle::Normal }),
    ("fontDate", FontDefault { family: FontFamily::Verdana, scale: 1.2, style: FontStyle::Bold }),
    ("fontGraphDayLabel", FontDefault { family: FontFamily::Verdana, scale: 0.9, style: FontStyle::Normal }),
    ("fontImportantTran", FontDefault { family: FontFamily::Verdana, scale: 1.0, style: FontStyle::Normal }),
    ("fontLabel", FontDefault { family: FontFamily::System, scale: 0.9, style: FontStyle::Bold }),
    ("fontMonth", FontDefault { family: FontFamily::Verdana, scale: 1.5, style: FontStyle::Normal }),
    ("fontNegTranAmount", FontDefault { family: FontFamily::Mono, scale: 1.0, style: FontStyle::Normal }),
    ("fontPosTranAmount", FontDefault { family: FontFamily::Mono, scale: 1.0, style: FontStyle::Normal }),
    ("fontPaidTran", FontDefault { family: FontFamily::Verdana, scale: 1.0, style: FontStyle::Normal }),
];

const IGNORED_KEYS: [&str; 5] = ["graphPastBarColor", "graphExtraMonthBarColor", "graphMinBarColor", "graphAvgLineColor", "detailsStatsOnDesc"];
pub const MAX_RECENT_FILES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct BudgetConfig {
    // plain fields
    pub regkey: Option<String>,
    pub last_file_name: Option<String>,
    pub lic_agree: bool,
    pub first_run: bool,
    pub first_run_jpackage: bool,
    pub start_folded: bool,
    pub window_maximized: bool,
    pub stat_height: i32,
    pub stat_width: i32,
    pub stat_x: i32,
    pub stat_y: i32,
    pub window_height: i32,
    pub window_pos_x: i32,
    pub window_pos_y: i32,
    pub window_width: i32,
    pub graph_preset_max: f64,
    pub graph_preset_min: f64,
    pub graph_type: i32,
    pub image_selection_zoom: i32,
    pub recent_file: Vec<String>,
    /// Collapsed state of the Preferences groups `ui`, `fnt`, `clr`, `grc`, `adv`.
    pub group_collapsed: [bool; 5],
    // ui
    pub first_day_of_week_is_sunday: bool,
    pub autosave: bool,
    pub one_button_mouse: bool,
    pub wheel_support: bool,
    pub show_decimals_in_cv: bool,
    pub preserve_color_on_print: bool,
    pub best_fit: bool,
    pub always_show_balance: bool,
    pub use_indefinite_end_mode: bool,
    pub encryption: bool,
    pub auto_date_switch: bool,
    pub unlimited_history: bool,
    pub future_n_months: i32,
    pub max_saving_n_months: i32,
    pub min_balance_4_max_saving: f64,
    pub graph_n_months: i32,
    pub currency_prefix: String,
    pub currency_suffix: String,
    /// Raw SWT `FontData` strings in `FONT_KEYS` order; `None` = platform default (`FONT_KEYS`).
    pub fonts: [Option<String>; 11],
    pub colors: Colors,
    // adv
    pub single_instance: bool,
    pub port: i32,
    /// Keys read from the file that nothing claimed (reported like the original's `skipped: k=v`).
    pub skipped: Vec<(String, String)>,
}

impl Default for BudgetConfig {
    fn default() -> BudgetConfig {
        BudgetConfig {
            regkey: None, last_file_name: None, lic_agree: false, first_run: false, first_run_jpackage: true,
            start_folded: false, window_maximized: false,
            stat_height: -1, stat_width: -1, stat_x: -1, stat_y: -1,
            window_height: -1, window_pos_x: -1, window_pos_y: -1, window_width: -1,
            graph_preset_max: 0.0, graph_preset_min: 0.0, graph_type: 2, image_selection_zoom: 175,
            recent_file: Vec::new(), group_collapsed: [false, false, false, false, true],
            first_day_of_week_is_sunday: true, autosave: true, one_button_mouse: false, wheel_support: true,
            show_decimals_in_cv: false, preserve_color_on_print: false, best_fit: true, always_show_balance: true,
            use_indefinite_end_mode: true, encryption: false, auto_date_switch: false, unlimited_history: false,
            future_n_months: 24, max_saving_n_months: 2, min_balance_4_max_saving: 100.0, graph_n_months: 6,
            currency_prefix: "$".into(), currency_suffix: String::new(),
            fonts: Default::default(), colors: Colors::theme(Theme::Light),
            single_instance: false, port: 57778, skipped: Vec::new(),
        }
    }
}

fn pbool(v: &str) -> bool { v == "true" || v == "1" }
fn conf_bool(v: &str) -> bool { let t = v.trim(); t == "1" || t.eq_ignore_ascii_case("true") }

impl BudgetConfig {
    pub fn apply_theme(&mut self, t: Theme) { self.colors = Colors::theme(t); }

    /// `Budget.valid_month_n`: `-1` kept, else clamped to `[1..12]`.
    pub fn valid_month_n(n: i32) -> i32 { if n == -1 { -1 } else { n.clamp(1, 12) } }

    /// `BudgetConfig.apply` validation; `Ok` when the values are acceptable.
    pub fn validate(&self) -> Result<(), Vec<(&'static str, &'static str)>> {
        let mut errs = Vec::new();
        if !(12..=120).contains(&self.future_n_months) {
            errs.push(("futureNMonths", "Invalid number of future months. Valid number is [12..120]."));
        }
        if Self::valid_month_n(self.graph_n_months) != self.graph_n_months {
            errs.push(("graphNMonths", "Invalid number of forecast months. Valid numbers are: -1, or [1..12]."));
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// `Budget.loadRc` post-load clamps.
    pub fn clamp_after_load(&mut self) {
        self.future_n_months = self.future_n_months.clamp(12, 180);
        self.graph_n_months = Self::valid_month_n(self.graph_n_months);
    }

    /// `Config.load` + BudgetConfig's unknown-key handler, applied on top of the defaults.
    pub fn from_properties(props: &[(String, String)]) -> BudgetConfig {
        let mut c = BudgetConfig::default();
        for (k, v) in props {
            c.set_key(k, v);
        }
        c.recent_file.retain(|f| !f.is_empty());
        c
    }

    fn set_key(&mut self, k: &str, v: &str) {
        if IGNORED_KEYS.contains(&k) { return; }
        let int = |cur: i32| v.trim().parse::<i32>().unwrap_or(cur);
        let dbl = |cur: f64| v.trim().parse::<f64>().unwrap_or(cur);
        match k {
            "regkey" => self.regkey = Some(v.to_string()),
            "lastFileName" => self.last_file_name = Some(v.to_string()),
            "licAgree" => self.lic_agree = pbool(v),
            "firstRun" => self.first_run = pbool(v),
            "firstRunJPACKAGE" => self.first_run_jpackage = pbool(v),
            "startFolded" => self.start_folded = pbool(v),
            "windowMaximized" => self.window_maximized = pbool(v),
            "statHeight" => self.stat_height = int(self.stat_height),
            "statWidth" => self.stat_width = int(self.stat_width),
            "statX" => self.stat_x = int(self.stat_x),
            "statY" => self.stat_y = int(self.stat_y),
            "windowHeight" => self.window_height = int(self.window_height),
            "windowPosX" => self.window_pos_x = int(self.window_pos_x),
            "windowPosY" => self.window_pos_y = int(self.window_pos_y),
            "windowWidth" => self.window_width = int(self.window_width),
            "graphPresetMax" => self.graph_preset_max = dbl(self.graph_preset_max),
            "graphPresetMin" => self.graph_preset_min = dbl(self.graph_preset_min),
            "graphType" => self.graph_type = int(self.graph_type),
            "image_selection_zoom" => self.image_selection_zoom = int(self.image_selection_zoom),
            "ui" => self.group_collapsed[0] = v == "true",
            "fnt" => self.group_collapsed[1] = v == "true",
            "clr" => self.group_collapsed[2] = v == "true",
            "grc" => self.group_collapsed[3] = v == "true",
            "adv" => self.group_collapsed[4] = v == "true",
            "firstDayOfWeekIsSunday" => self.first_day_of_week_is_sunday = conf_bool(v),
            "autosave" => self.autosave = conf_bool(v),
            "oneButtonMouse" => self.one_button_mouse = conf_bool(v),
            "wheelSupport" => self.wheel_support = conf_bool(v),
            "showDecimalsInCV" => self.show_decimals_in_cv = conf_bool(v),
            "preserveColorOnPrint" => self.preserve_color_on_print = conf_bool(v),
            "bestFit" => self.best_fit = conf_bool(v),
            "alwaysShowBalance" => self.always_show_balance = conf_bool(v),
            "useIndefiniteEndMode" => self.use_indefinite_end_mode = conf_bool(v),
            "encryption" => self.encryption = conf_bool(v),
            "autoDateSwitch" => self.auto_date_switch = conf_bool(v),
            "unlimitedHistory" => self.unlimited_history = conf_bool(v),
            "futureNMonths" => self.future_n_months = int(self.future_n_months),
            "maxSavingNMonths" => self.max_saving_n_months = int(self.max_saving_n_months),
            "minBalance4MaxSaving" => self.min_balance_4_max_saving = dbl(self.min_balance_4_max_saving),
            "graphNMonths" => self.graph_n_months = int(self.graph_n_months),
            "currencyPrefix" => self.currency_prefix = v.to_string(),
            "currencySuffix" => self.currency_suffix = v.to_string(),
            "singleInstance" => self.single_instance = conf_bool(v),
            "port" => self.port = int(self.port),
            // legacy keys handled by BudgetConfig.lambda$new$1
            "firstDayOfWeek" => self.first_day_of_week_is_sunday = v == "1",
            _ => {
                if let Some(i) = FONT_KEYS.iter().position(|(key, _)| *key == k) {
                    self.fonts[i] = Some(v.to_string());
                } else if self.colors.set(k, u32::from_str_radix(v.trim(), 16).unwrap_or(0)) {
                } else if let Some(idx) = k.strip_prefix("recentFile.").or_else(|| k.strip_prefix("recentFile")) {
                    if let Some(n) = idx.parse::<usize>().ok().filter(|&n| n < MAX_RECENT_FILES) {
                        if self.recent_file.len() <= n { self.recent_file.resize(n + 1, String::new()); }
                        self.recent_file[n] = v.to_string();
                    }
                } else {
                    self.skipped.push((k.to_string(), v.to_string()));
                }
            }
        }
    }

    /// `Config.save`: every persisted field in declaration order.
    pub fn to_properties(&self) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = Vec::new();
        let mut push = |k: &str, v: String| p.push((k.to_string(), v));
        if let Some(r) = &self.regkey { push("regkey", r.clone()); }
        if let Some(f) = &self.last_file_name { push("lastFileName", f.clone()); }
        push("licAgree", self.lic_agree.to_string());
        push("firstRun", self.first_run.to_string());
        push("firstRunJPACKAGE", self.first_run_jpackage.to_string());
        push("startFolded", self.start_folded.to_string());
        push("windowMaximized", self.window_maximized.to_string());
        push("statHeight", self.stat_height.to_string());
        push("statWidth", self.stat_width.to_string());
        push("statX", self.stat_x.to_string());
        push("statY", self.stat_y.to_string());
        push("windowHeight", self.window_height.to_string());
        push("windowPosX", self.window_pos_x.to_string());
        push("windowPosY", self.window_pos_y.to_string());
        push("windowWidth", self.window_width.to_string());
        push("graphPresetMax", crate::javafmt::java_double(self.graph_preset_max));
        push("graphPresetMin", crate::javafmt::java_double(self.graph_preset_min));
        push("graphType", self.graph_type.to_string());
        push("image_selection_zoom", self.image_selection_zoom.to_string());
        for (i, f) in self.recent_file.iter().enumerate() { push(&format!("recentFile.{i}"), f.clone()); }
        for (i, g) in ["ui", "fnt", "clr", "grc", "adv"].iter().enumerate() { push(g, self.group_collapsed[i].to_string()); }
        push("firstDayOfWeekIsSunday", self.first_day_of_week_is_sunday.to_string());
        push("autosave", self.autosave.to_string());
        push("oneButtonMouse", self.one_button_mouse.to_string());
        push("wheelSupport", self.wheel_support.to_string());
        push("showDecimalsInCV", self.show_decimals_in_cv.to_string());
        push("preserveColorOnPrint", self.preserve_color_on_print.to_string());
        push("bestFit", self.best_fit.to_string());
        push("alwaysShowBalance", self.always_show_balance.to_string());
        push("useIndefiniteEndMode", self.use_indefinite_end_mode.to_string());
        push("encryption", self.encryption.to_string());
        push("autoDateSwitch", self.auto_date_switch.to_string());
        push("unlimitedHistory", self.unlimited_history.to_string());
        push("futureNMonths", self.future_n_months.to_string());
        push("maxSavingNMonths", self.max_saving_n_months.to_string());
        push("minBalance4MaxSaving", crate::javafmt::java_double(self.min_balance_4_max_saving));
        push("graphNMonths", self.graph_n_months.to_string());
        push("currencyPrefix", self.currency_prefix.clone());
        push("currencySuffix", self.currency_suffix.clone());
        for (i, (k, _)) in FONT_KEYS.iter().enumerate() {
            if let Some(f) = &self.fonts[i] { push(k, f.clone()); }
        }
        for (k, v) in self.colors.entries() { push(k, format!("{v:06x}")); }
        push("singleInstance", self.single_instance.to_string());
        push("port", self.port.to_string());
        p
    }

    /// `Config.load`: a missing or unreadable file yields the defaults.
    pub fn load(path: &Path) -> BudgetConfig {
        match std::fs::read(path) {
            Ok(b) => { let mut c = BudgetConfig::from_properties(&parse_properties(&crate::bgt::latin1_decode(&b))); c.clamp_after_load(); c }
            Err(_) => BudgetConfig::default(),
        }
    }

    /// `Config.saveProperties`: copy to `.bak`, write, delete `.bak` on success / restore on failure.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bak = std::path::PathBuf::from(format!("{}.bak", path.display()));
        let had_old = path.exists();
        if had_old {
            std::fs::copy(path, &bak).map_err(|_| format!("unable to backup '{}' to '{}'", path.display(), bak.display()))?;
        }
        let text = format_properties(&self.to_properties());
        match std::fs::write(path, crate::bgt::latin1_encode(&text)) {
            Ok(()) => { if had_old { let _ = std::fs::remove_file(&bak); } Ok(()) }
            Err(e) => { if had_old { let _ = std::fs::rename(&bak, path); } Err(format!("Exception: {e}")) }
        }
    }

    /// `addRecentFile`: move/insert at the front, keep at most 5.
    pub fn add_recent_file(&mut self, name: &str) {
        self.recent_file.retain(|f| f != name);
        self.recent_file.insert(0, name.to_string());
        self.recent_file.truncate(MAX_RECENT_FILES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_syntax() {
        let p = parse_properties("#comment\n! other\nkey=value\n  spaced : v2\nmulti=a\\\n   b\nesc\\=k=x\\u0041\\n\nempty=\n");
        assert_eq!(p, vec![
            ("key".into(), "value".into()), ("spaced".into(), "v2".into()), ("multi".into(), "ab".into()),
            ("esc=k".into(), "xA\n".into()), ("empty".into(), String::new()),
        ]);
        let f = format_properties(&[("a b".into(), " x=y\u{e9}".into())]);
        assert!(f.ends_with("a\\ b=\\ x\\=y\\u00E9\n"));
        assert_eq!(parse_properties(&f), vec![("a b".into(), " x=y\u{e9}".into())]);
    }

    #[test]
    fn config_roundtrip_defaults_and_legacy() {
        let c = BudgetConfig::default();
        assert_eq!(c.colors.bgColorBalance, 0xe5f0ff);
        let props = c.to_properties();
        assert!(props.iter().any(|(k, v)| k == "bgColorBalance" && v == "e5f0ff"));
        assert!(props.iter().any(|(k, v)| k == "port" && v == "57778"));
        assert!(!props.iter().any(|(k, _)| k == "regkey"));
        let back = BudgetConfig::from_properties(&props);
        assert_eq!(back, c);
        let legacy = parse_properties("firstDayOfWeek=0\nrecentFile0=a.bgt\nrecentFile.2=c.bgt\nfutureNMonths=5\nunlimitedHistory=1\nbgColorCell=zz\ngraphMinBarColor=123456\nfoo=bar\nfontTran=1|Verdana|9.0|0|WINDOWS|1\n");
        let mut l = BudgetConfig::from_properties(&legacy);
        assert!(!l.first_day_of_week_is_sunday);
        assert_eq!(l.recent_file, vec!["a.bgt".to_string(), "c.bgt".to_string()]);
        let huge = BudgetConfig::from_properties(&parse_properties("recentFile.4000000000=x.bgt\nrecentFile.5=y.bgt\nrecentFile.1=b.bgt\n"));
        assert_eq!(huge.recent_file, vec!["b.bgt".to_string()]);
        assert!(l.unlimited_history);
        assert_eq!(l.colors.bgColorCell, 0);
        assert_eq!(l.skipped, vec![("foo".to_string(), "bar".to_string())]);
        assert_eq!(l.fonts[2].as_deref(), Some("1|Verdana|9.0|0|WINDOWS|1"));
        assert_eq!(l.future_n_months, 5);
        assert!(l.validate().is_err());
        l.clamp_after_load();
        assert_eq!(l.future_n_months, 12);
        l.apply_theme(Theme::Dark);
        assert_eq!(l.colors.bgColorWindow, 0x535253);
        l.add_recent_file("c.bgt");
        assert_eq!(l.recent_file[0], "c.bgt");
        assert_eq!(l.recent_file.len(), 2);
    }
}
