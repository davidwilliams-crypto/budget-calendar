//! Port of `ca.mishell.misc.SimpleCalendar`: a y/m/d triple with the original's
//! normalisation rules. Dates travel as `i32` in `yyyymmdd` form.

use std::cmp::Ordering;
use std::fmt;

const DPM: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DPM_LEAP: [i32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DOY: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const DOY_LEAP: [i32; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

pub const SUNDAY: i32 = 0;
pub const SATURDAY: i32 = 6;

/// The years of the calendar the port works in.
pub const YEAR_MIN: i32 = 1000;
pub const YEAR_MAX: i32 = 9999;
/// 1 January 1000: the first date the calendar and the editors accept.
pub const DATE_MIN: i32 = YEAR_MIN * 10000 + 101;
/// 31 December 9999: the last representable date. Schedule walks stop after it, the loader
/// clamps stored dates to it and the editors accept nothing later. (The original's
/// `DATE_MAX = 99991131` was a sentinel, not a real date.)
pub const DATE_MAX: i32 = YEAR_MAX * 10000 + 1231;

/// Whether `v` is a real `yyyymmdd` date (already normalised) from `DATE_MIN` to `DATE_MAX`.
pub fn is_calendar_date(v: i32) -> bool {
    (DATE_MIN..=DATE_MAX).contains(&v) && Cal::from_int(v).get() == v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Day = 1,
    Month = 2,
    Year = 3,
    Week = 5,
    DayOfWeek = 6,
    NthWeekday = 7,
    LastWeekday = 8,
}

/// `m` is 0-based like the original; `get()` is 1-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cal {
    y: i32,
    m: i32,
    d: i32,
}

pub fn is_leap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn dpm(y: i32, m: i32) -> i32 {
    if is_leap(y) { DPM_LEAP[m as usize] } else { DPM[m as usize] }
}

fn doy_table(y: i32) -> &'static [i32; 12] {
    if is_leap(y) { &DOY_LEAP } else { &DOY }
}

impl Cal {
    /// `SimpleCalendar(int)`: `yyyy` -> 1 January, `yyyymm` -> day 1, else `yyyymmdd`, all normalised.
    pub fn from_int(v: i32) -> Cal {
        // i64 + saturation like `get()`: an absurd value must not overflow the short-form scaling.
        let v = v as i64;
        let v = if v < 10000 { v * 10000 + 101 } else if v < 1000000 { v * 100 + 1 } else { v };
        let mut c = Cal { y: 0, m: 0, d: 1 };
        c.set_int(v.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        c
    }

    /// `SimpleCalendar(y, m0, d)`.
    pub fn ymd(y: i32, m0: i32, d: i32) -> Cal {
        let mut c = Cal { y: 0, m: 0, d: 1 };
        c.set_ymd(y, m0, d);
        c
    }

    pub fn get(&self) -> i32 {
        // Saturate rather than overflow for absurd years (callers compare against `DATE_MAX`).
        (self.y as i64 * 10000 + (self.m as i64 + 1) * 100 + self.d as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    pub fn year(&self) -> i32 { self.y }
    /// 0-based month.
    pub fn month(&self) -> i32 { self.m }
    pub fn day(&self) -> i32 { self.d }
    /// `getYm()` = `y*100 + m0`.
    pub fn ym(&self) -> i32 { self.y * 100 + self.m }

    pub fn set_int(&mut self, v: i32) -> &mut Cal {
        self.set_ymd(v / 10000, (v / 100) % 100 - 1, v % 100)
    }

    pub fn set_cal(&mut self, o: &Cal) -> &mut Cal {
        self.set_int(o.get())
    }

    pub fn set_ymd(&mut self, y: i32, m: i32, d: i32) -> &mut Cal {
        self.y = y;
        if (0..=11).contains(&m) {
            self.m = m;
            if (1..=28).contains(&d) { self.d = d; } else { self.set(Field::Day, d); }
        } else {
            self.set(Field::Month, m);
            self.set(Field::Day, d);
        }
        self
    }

    fn adjust_day(&mut self) {
        let n = dpm(self.y, self.m);
        if self.d > n { self.d = n; }
    }

    fn adjust_month(&mut self) {
        if self.m < 0 {
            self.y += (self.m + 1) / 12 - 1;
            self.m %= 12;
            if self.m < 0 { self.m += 12; }
        } else if self.m > 11 {
            self.y += self.m / 12;
            self.m %= 12;
        }
        self.adjust_day();
    }

    pub fn set(&mut self, f: Field, v: i32) -> &mut Cal {
        match f {
            Field::Year => { self.y = v; self.adjust_day(); }
            Field::Month => { self.m = v; self.adjust_month(); }
            Field::Day if !(-400..=400).contains(&v) => {
                // Large steps go through a day number so they cost O(1) instead of a month-by-month walk.
                let (y, m, d) = civil_from_days(days_from_civil(self.y as i64, self.m as i64 + 1, 1) + v as i64 - 1);
                self.y = y.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                self.m = m as i32 - 1;
                self.d = d as i32;
            }
            Field::Day => {
                self.d = v;
                if self.d < 1 {
                    loop {
                        self.m = if self.m == 0 { 11 } else { self.m - 1 };
                        if self.m == 11 { self.y -= 1; }
                        self.d += dpm(self.y, self.m);
                        if self.d >= 1 { break; }
                    }
                } else if self.d > 28 {
                    loop {
                        let n = dpm(self.y, self.m);
                        if self.d <= n { break; }
                        self.d -= n;
                        self.m = if self.m == 11 { 0 } else { self.m + 1 };
                        if self.m == 0 { self.y += 1; }
                    }
                }
            }
            Field::DayOfWeek => { let w = self.day_of_week(); self.add(Field::Day, v - w); }
            Field::NthWeekday => {
                let w = self.day_of_week();
                let n = self.week_of_month();
                self.d = 1;
                self.m = v;
                self.adjust_month();
                let w1 = self.day_of_week();
                self.d = if w1 <= w { 1 + w - w1 } else { 8 - w1 + w };
                self.d += n * 7;
                // Months without an n-th occurrence fall back to the last matching weekday
                // (deliberate deviation: the original clamped to month end, a different weekday).
                let max = self.max_day();
                while self.d > max { self.d -= 7; }
            }
            Field::LastWeekday => {
                let w = self.day_of_week();
                self.d = 1;
                self.m = v;
                self.adjust_month();
                self.d = self.max_day();
                let wl = self.day_of_week();
                self.d -= if wl >= w { wl - w } else { 7 - (w - wl) };
            }
            Field::Week => panic!("Invalid field value 5"),
        }
        self
    }

    pub fn add(&mut self, f: Field, n: i32) -> &mut Cal {
        match f {
            Field::Day => self.set(Field::Day, self.d + n),
            Field::Week => self.set(Field::Day, self.d + 7 * n),
            Field::Month => self.set(Field::Month, self.m + n),
            Field::Year => self.set(Field::Year, self.y + n),
            Field::NthWeekday => self.set(Field::NthWeekday, self.m + n),
            Field::LastWeekday => self.set(Field::LastWeekday, self.m + n),
            Field::DayOfWeek => panic!("Invalid field value 6"),
        }
    }

    pub fn add_days(&mut self, n: i32) -> &mut Cal { self.add(Field::Day, n) }
    pub fn add_months(&mut self, n: i32) -> &mut Cal { self.add(Field::Month, n) }
    pub fn next_day(&mut self) -> &mut Cal { if self.d < 28 { self.d += 1; self } else { self.add(Field::Day, 1) } }
    pub fn prev_day(&mut self) -> &mut Cal { if self.d > 1 { self.d -= 1; self } else { self.add(Field::Day, -1) } }
    pub fn set_day_one(&mut self) -> &mut Cal { self.d = 1; self }
    pub fn set_day_last(&mut self) -> &mut Cal { self.d = self.max_day(); self }

    pub fn max_day(&self) -> i32 { dpm(self.y, self.m) }
    pub fn max_day_of_year(&self) -> i32 { if is_leap(self.y) { 366 } else { 365 } }
    pub fn day_of_year(&self) -> i32 { self.d + doy_table(self.y)[self.m as usize] }
    /// 0 = Sunday .. 6 = Saturday.
    pub fn day_of_week(&self) -> i32 {
        let y1 = self.y - 1;
        (y1 + y1 / 4 - y1 / 100 + y1 / 400 + self.day_of_year()) % 7
    }
    pub fn week_of_month(&self) -> i32 { (self.d - 1) / 7 }
    pub fn is_weekend(&self) -> bool { matches!(self.day_of_week(), SUNDAY | SATURDAY) }
    pub fn is_last_weekday(&self) -> bool { self.d > self.max_day() - 7 }

    /// Days from `a` to `b` (negative when `a > b`), in constant time: validating a daily or weekly
    /// payment measures from its start to the year 9999, which used to walk every year in between.
    pub fn diff(a: &Cal, b: &Cal) -> i32 { (b.day_number() - a.day_number()) as i32 }
    fn day_number(&self) -> i64 { days_from_civil(self.y as i64, self.m as i64 + 1, self.d as i64) }
    pub fn diff_int(a: i32, b: i32) -> i32 { Cal::diff(&Cal::from_int(a), &Cal::from_int(b)) }
    pub fn months_diff(a: &Cal, b: &Cal) -> i32 { (b.y * 12 + b.m) - (a.y * 12 + a.m) }

    fn order_key(&self) -> i64 { self.y as i64 * 10000 + self.m as i64 * 100 + self.d as i64 }
    pub fn lt(&self, o: &Cal) -> bool { self.order_key() < o.order_key() }
    pub fn gt(&self, o: &Cal) -> bool { self.order_key() > o.order_key() }
    pub fn le(&self, o: &Cal) -> bool { !self.gt(o) }
    pub fn ge(&self, o: &Cal) -> bool { !self.lt(o) }
    pub fn in_range(&self, a: &Cal, b: &Cal) -> bool { self.ge(a) && self.le(b) }

    /// `getYMdurationString`: 14 -> "1y 2m", 12 -> "1y ", 0 -> "".
    pub fn ym_duration_string(months: i32) -> String {
        let mut s = String::new();
        if months > 11 { s.push_str(&format!("{}y ", months / 12)); }
        if months % 12 != 0 { s.push_str(&format!("{}m", months % 12)); }
        s
    }

    /// Today's date from the system clock, in the local time zone.
    pub fn today() -> Cal {
        #[cfg(windows)]
        {
            #[repr(C)]
            struct SystemTime { y: u16, m: u16, dow: u16, d: u16, h: u16, mi: u16, s: u16, ms: u16 }
            extern "system" { fn GetLocalTime(t: *mut SystemTime); }
            let mut t = SystemTime { y: 0, m: 0, dow: 0, d: 0, h: 0, mi: 0, s: 0, ms: 0 };
            unsafe { GetLocalTime(&mut t) };
            if t.y > 0 && (1..=12).contains(&t.m) && t.d > 0 {
                return Cal::ymd(t.y as i32, t.m as i32 - 1, t.d as i32);
            }
        }
        Cal::today_utc()
    }

    /// Today's date in UTC (fallback when no local time is available).
    pub fn today_utc() -> Cal {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (y, m, d) = civil_from_days(secs.div_euclid(86400));
        Cal::ymd(y as i32, m as i32 - 1, d as i32)
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (`m` 1-based; H. Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil`: `(year, 1-based month, day)`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Default for Cal {
    fn default() -> Cal { Cal::from_int(20000101) }
}

impl PartialOrd for Cal {
    fn partial_cmp(&self, o: &Cal) -> Option<Ordering> { Some(self.cmp(o)) }
}
impl Ord for Cal {
    fn cmp(&self, o: &Cal) -> Ordering { self.order_key().cmp(&o.order_key()) }
}

impl fmt::Display for Cal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m + 1, self.d)
    }
}

/// `sDate(int)`: `yyyy-MM-dd` of an already-normalised int date.
pub fn sdate(v: i32) -> String {
    format!("{:04}-{:02}-{:02}", v / 10000, (v / 100) % 100, v % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_and_weekday() {
        assert_eq!(Cal::from_int(20071232).get(), 20080101);
        assert_eq!(Cal::from_int(2007).get(), 20070101);
        assert_eq!(Cal::from_int(200702).get(), 20070201);
        assert_eq!(Cal::from_int(20000101).day_of_week(), SATURDAY);
        assert_eq!(Cal::from_int(20040807).day_of_week(), SATURDAY);
        assert_eq!(Cal::from_int(20061218).day_of_week(), 1);
        let mut c = Cal::from_int(20040131);
        c.add(Field::Month, 1);
        assert_eq!(c.get(), 20040229);
        let mut c = Cal::from_int(20040131);
        c.add(Field::Day, -31);
        assert_eq!(c.get(), 20031231);
        assert_eq!(Cal::diff_int(20031231, 20040101), 1);
        assert_eq!(Cal::diff_int(20040101, 20031231), -1);
        assert_eq!(Cal::diff_int(20040101, 20050101), 366);
    }

    #[test]
    fn nth_and_last_weekday() {
        // 3rd Sunday: 2004-08-15 -> 2004-09-19
        let mut c = Cal::from_int(20040815);
        c.add(Field::NthWeekday, 1);
        assert_eq!(c.get(), 20040919);
        // last Sunday of Aug 2004 is the 29th -> last Sunday of Sep 2004 is the 26th
        let mut c = Cal::from_int(20040829);
        c.add(Field::LastWeekday, 1);
        assert_eq!(c.get(), 20040926);
        assert_eq!(Cal::ym_duration_string(14), "1y 2m");
    }

    #[test]
    fn fifth_weekday_falls_back_to_last_matching_weekday() {
        // 2024-01-29 is the 5th Monday; May 2024 has only four Mondays.
        let mut c = Cal::from_int(20240129);
        c.add(Field::NthWeekday, 4);
        assert_eq!(c.get(), 20240527);
        assert_eq!(c.day_of_week(), 1);
    }

    #[test]
    fn large_day_steps_match_the_month_walk() {
        for start in [20240131, 20000229, 19991231, 20260315] {
            for n in [-100_000, -36_525, -401, 401, 1_000, 36_525, 100_000] {
                let mut fast = Cal::from_int(start);
                fast.add(Field::Day, n);
                let mut slow = Cal::from_int(start);
                let step = if n > 0 { 300 } else { -300 };
                let mut left = n;
                while left != 0 { let s = if left.abs() < 300 { left } else { step }; slow.add(Field::Day, s); left -= s; }
                assert_eq!(fast.get(), slow.get(), "{start} {n}");
                assert_eq!(Cal::diff(&Cal::from_int(start), &fast), n);
            }
        }
    }

    #[test]
    fn day_differences_match_a_year_by_year_count() {
        // The walk `diff` used before it became arithmetic.
        fn walked(a: &Cal, b: &Cal) -> i32 {
            let (a, b, sign) = if a.order_key() <= b.order_key() { (a, b, 1) } else { (b, a, -1) };
            if a.y == b.y { return (b.day_of_year() - a.day_of_year()) * sign; }
            let mut n = a.max_day_of_year() - a.day_of_year() + b.day_of_year();
            for y in (a.y + 1)..b.y { n += if is_leap(y) { 366 } else { 365 }; }
            n * sign
        }
        let dates = [DATE_MIN, 10000228, 15821015, 18991231, 19000228, 19000301, 19991231, 20000229, 20000301, 20231231,
            20240101, 20240229, 20240301, 20241231, 20260315, 20261110, 21000228, 21000301, 24000229, 99990101, DATE_MAX];
        for a in dates {
            for b in dates {
                let (a, b) = (Cal::from_int(a), Cal::from_int(b));
                assert_eq!(Cal::diff(&a, &b), walked(&a, &b), "{} to {}", a.get(), b.get());
            }
        }
        assert_eq!(Cal::diff_int(DATE_MIN, DATE_MAX), 3_287_181);
    }

    #[test]
    fn calendar_bounds_are_real_dates() {
        assert_eq!((DATE_MIN, DATE_MAX), (10000101, 99991231));
        assert!(is_calendar_date(DATE_MIN) && is_calendar_date(DATE_MAX));
        for v in [DATE_MIN - 1, 9991231, DATE_MAX + 1, 100000101, 20260229, 20261301, 0, -1] {
            assert!(!is_calendar_date(v), "{v}");
        }
    }

    #[test]
    fn today_is_plausible() {
        assert!(Cal::today().get() >= 20260101);
    }
}
