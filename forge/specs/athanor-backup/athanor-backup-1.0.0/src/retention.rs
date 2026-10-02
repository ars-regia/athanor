//! Which snapshots survive a prune.

use chrono::{Datelike, NaiveDateTime, Timelike};
use std::collections::HashSet;

/// How many distinct hours, days and ISO weeks keep their newest snapshot.
pub const HOURLY: usize = 24;
pub const DAILY: usize = 7;
pub const WEEKLY: usize = 4;

/// Marks the snapshots to keep, given their times sorted newest first.
///
/// This is the `keep-hourly`/`keep-daily`/`keep-weekly` rule of restic: the
/// newest snapshot of each of the [`HOURLY`] most recent hours that hold a snapshot is
/// kept, and likewise for [`DAILY`] days and [`WEEKLY`] ISO weeks; the union survives.
/// Counting hours that hold a snapshot rather than wall-clock hours means a machine that
/// stayed off for a week still has its last snapshots when it comes back.
pub fn keep(times_newest_first: &[NaiveDateTime]) -> Vec<bool> {
    let mut keep = vec![false; times_newest_first.len()];
    mark(&mut keep, times_newest_first, HOURLY, |t| {
        (t.year(), t.ordinal(), t.hour())
    });
    mark(&mut keep, times_newest_first, DAILY, |t| {
        (t.year(), t.ordinal(), 0)
    });
    mark(&mut keep, times_newest_first, WEEKLY, |t| {
        let week = t.iso_week();
        (week.year(), week.week(), 0)
    });
    keep
}

/// Marks the first snapshot of each new bucket until `limit` buckets have been seen.
fn mark(
    keep: &mut [bool],
    times_newest_first: &[NaiveDateTime],
    limit: usize,
    bucket: impl Fn(&NaiveDateTime) -> (i32, u32, u32),
) {
    let mut seen = HashSet::new();
    for (kept, time) in keep.iter_mut().zip(times_newest_first) {
        if seen.len() == limit {
            break;
        }
        if seen.insert(bucket(time)) {
            *kept = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .and_then(|date| date.and_hms_opt(hour, minute, 0))
            .expect("valid test time")
    }

    #[test]
    fn several_snapshots_in_one_hour_keep_only_the_newest() {
        let times: Vec<_> = (0..30).rev().map(|minute| at(23, 10, minute)).collect();
        let kept = keep(&times);
        assert!(kept[0]);
        assert_eq!(kept.iter().filter(|&&k| k).count(), 1);
    }

    #[test]
    fn three_days_of_hourly_snapshots_keep_a_day_of_hours_and_each_day() {
        // Monday 21 to Wednesday 23 September 2026: one ISO week, three days, 72 hours.
        let newest = at(23, 23, 0);
        let times: Vec<_> = (0..72)
            .map(|hours| newest - Duration::hours(hours))
            .collect();
        let kept = keep(&times);

        assert!(
            kept[..HOURLY].iter().all(|&k| k),
            "the newest 24 hours are kept"
        );
        // The newest snapshot of the 22nd and of the 21st, at 23:00 each.
        assert!(kept[24] && kept[48]);
        assert_eq!(kept.iter().filter(|&&k| k).count(), HOURLY + 2);
        assert!(!kept[71], "the oldest snapshot is pruned");
    }

    #[test]
    fn weeks_reach_further_back_than_days() {
        // Hourly snapshots for 40 days up to Wednesday 30 September 2026, 23:00.
        let newest = at(30, 23, 0);
        let times: Vec<_> = (0..40 * 24)
            .map(|hours| newest - Duration::hours(hours))
            .collect();
        let kept = keep(&times);
        let is_kept = |time: NaiveDateTime| times.iter().zip(&kept).any(|(&t, &k)| t == time && k);

        // 24 hours of the 30th, the 23:00 of the six days before it, and the newest of
        // the two ISO weeks before the one of the 21st (whose newest, the 27th, is a day).
        assert!(is_kept(at(24, 23, 0)) && !is_kept(at(23, 23, 0)));
        assert!(is_kept(at(20, 23, 0)) && is_kept(at(13, 23, 0)));
        assert!(!is_kept(at(6, 23, 0)));
        assert_eq!(kept.iter().filter(|&&k| k).count(), HOURLY + 6 + 2);
    }

    #[test]
    fn nothing_is_pruned_below_the_limits() {
        let times: Vec<_> = (0..5).rev().map(|hour| at(23, hour, 0)).collect();
        assert!(keep(&times).iter().all(|&k| k));
    }
}
