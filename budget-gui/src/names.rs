//! Month and weekday names shared by every window (English, as in the original). Index months
//! with `Cal::month()` (0 = January) and weekdays with `Cal::day_of_week()` (0 = Sunday).

use budget_core::Cal;

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
/// Full names; the first three or two letters give the short forms.
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// `October 27, 2026`.
pub fn long_date(date: i32) -> String {
    let day = Cal::from_int(date);
    format!("{} {}, {}", MONTHS_LONG[day.month() as usize], day.day(), day.year())
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_follow_the_calendar_indices() {
        let cal = budget_core::Cal::from_int(20261027); // a Tuesday in October
        assert_eq!(super::MONTHS[cal.month() as usize], "Oct");
        assert_eq!(super::MONTHS_LONG[cal.month() as usize], "October");
        assert_eq!(super::WEEKDAYS[cal.day_of_week() as usize], "Tuesday");
        assert_eq!(super::long_date(20261027), "October 27, 2026");
    }
}
