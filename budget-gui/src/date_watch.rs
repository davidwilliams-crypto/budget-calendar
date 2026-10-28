//! Notices when the local date changes while the app is open (analysis/03 §11.4: the original
//! checks every 20 s and switches to the new day).

use std::time::{Duration, Instant};

/// Reads "today" (`yyyymmdd`) at most once per [`DateWatch::INTERVAL`].
pub struct DateWatch {
    /// `None` keeps the date fixed (`--today` on the command line).
    clock: Option<Box<dyn Fn() -> i32>>,
    next_check: Instant,
}

impl DateWatch {
    /// The original's `display.timerExec(20000, …)` period.
    pub const INTERVAL: Duration = Duration::from_secs(20);

    pub fn new(clock: Option<Box<dyn Fn() -> i32>>, first_check: Instant) -> DateWatch {
        DateWatch { clock, next_check: first_check }
    }

    /// The local system date, first checked one interval from now.
    pub fn system() -> DateWatch {
        DateWatch::new(Some(Box::new(|| budget_core::Cal::today().get())), Instant::now() + Self::INTERVAL)
    }

    /// A date that never changes.
    pub fn fixed() -> DateWatch { DateWatch::new(None, Instant::now()) }

    /// Reads the clock when a check is due. Also returns how long until the next check, so an idle
    /// window can ask to be woken for it; `None` when the date is fixed.
    pub fn poll(&mut self, now: Instant) -> (Option<i32>, Option<Duration>) {
        let Some(clock) = &self.clock else { return (None, None) };
        let mut date = None;
        if now >= self.next_check {
            date = Some(clock());
            self.next_check = now + Self::INTERVAL;
        }
        (date, Some(self.next_check.saturating_duration_since(now)))
    }

    /// Make the next poll read the clock (tests).
    #[cfg(test)]
    pub fn expire(&mut self) { self.next_check = Instant::now(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn the_clock_is_read_once_per_interval_and_never_when_fixed() {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        let start = Instant::now();
        let mut watch = DateWatch::new(Some(Box::new(move || { counter.fetch_add(1, Ordering::Relaxed); 20261028 })), start);
        assert_eq!(watch.poll(start), (Some(20261028), Some(DateWatch::INTERVAL)));
        let later = start + Duration::from_secs(5);
        assert_eq!(watch.poll(later), (None, Some(Duration::from_secs(15))), "no clock read between checks");
        assert_eq!(watch.poll(start + DateWatch::INTERVAL).0, Some(20261028));
        assert_eq!(reads.load(Ordering::Relaxed), 2);
        assert_eq!(DateWatch::fixed().poll(start + Duration::from_secs(3600)), (None, None));
    }
}
