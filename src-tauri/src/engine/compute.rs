//! Pure computation of what the UI shows. No I/O; `now` is a parameter.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use super::calendar::DayBounds;
use super::state::{DayState, SyncStatus, SyncWarning, TimerStatus};
use crate::settings::WeeklyTargets;

/// Pausing with less than this much of the target left stops the timer at
/// the moment the target is reached instead of now. See
/// [`day_completion_stop`].
pub const FINISH_DAY_WINDOW_SECS: i64 = 15 * 60;

/// Sent to the UI as the `timer-state` event once per second.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerViewModel {
    pub status: TimerStatus,
    /// Negative means overtime.
    pub remaining_secs: i64,
    pub worked_secs: i64,
    pub target_secs: i64,
    pub break_secs: i64,
    /// Estimated finish time; only while working and before the target.
    pub eta: Option<DateTime<Utc>>,
    /// Whether a timer is running in Toggl.
    pub running: bool,
    pub last_synced: Option<DateTime<Utc>>,
    pub sync_warning: SyncWarning,
}

/// Computes the view for `now`.
///
/// `day` is the last known state. If it belongs to an earlier date (the day
/// changed before the next sync), only the running entry carries over.
pub fn compute(
    day: Option<&DayState>,
    sync: &SyncStatus,
    now: DateTime<Utc>,
    tz: Tz,
    targets: &WeeklyTargets,
) -> TimerViewModel {
    let today = DayBounds::containing(now, tz);
    let target_secs = targets.secs_for(today.date);

    let is_today = day.is_some_and(|d| d.date == today.date);
    let (completed_secs, has_entries, last_stop) = match day {
        Some(d) if is_today => (d.completed_secs, d.has_entries, d.last_stop),
        _ => (0, false, None),
    };
    let running_start = day.and_then(|d| d.running.as_ref()).map(|r| r.start);

    let running_secs = running_start.map_or(0, |start| today.overlap_secs(start, now));
    let worked_secs = completed_secs + running_secs;
    let remaining_secs = target_secs - worked_secs;
    let running = running_start.is_some();
    let has_activity = running || has_entries;

    let status = if has_activity && remaining_secs <= 0 {
        TimerStatus::Done
    } else if running {
        TimerStatus::Working
    } else if has_entries {
        TimerStatus::OnBreak
    } else {
        TimerStatus::NotStarted
    };

    let break_secs = match (status, last_stop) {
        (TimerStatus::OnBreak, Some(stop)) => (now - stop).num_seconds().max(0),
        _ => 0,
    };
    let eta = (status == TimerStatus::Working).then(|| now + Duration::seconds(remaining_secs));

    TimerViewModel {
        status,
        remaining_secs,
        worked_secs,
        target_secs,
        break_secs,
        eta,
        running,
        last_synced: sync.last_synced,
        sync_warning: sync.warning,
    }
}

/// When the timer is paused at `now`, the time it should be stopped at so
/// that the day's target is exactly reached.
///
/// Returns `Some` only while a timer is running and less than
/// [`FINISH_DAY_WINDOW_SECS`] of the target is left: the user is wrapping up
/// a bit early and wants the day to be full. Otherwise the timer stops now.
pub fn day_completion_stop(
    day: Option<&DayState>,
    now: DateTime<Utc>,
    tz: Tz,
    targets: &WeeklyTargets,
) -> Option<DateTime<Utc>> {
    let vm = compute(day, &SyncStatus::default(), now, tz, targets);
    let wrapping_up =
        vm.status == TimerStatus::Working && vm.remaining_secs < FINISH_DAY_WINDOW_SECS;
    wrapping_up.then(|| now + Duration::seconds(vm.remaining_secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::state::tests::entry;
    use chrono::{NaiveDate, TimeZone};
    use chrono_tz::Europe::Helsinki;

    const TARGET: i64 = 7 * 3600 + 30 * 60;

    fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Helsinki
            .with_ymd_and_hms(2026, 1, d, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn day_of(date: (i32, u32, u32), entries: &[crate::toggl::TimeEntry]) -> DayState {
        let date = NaiveDate::from_ymd_opt(date.0, date.1, date.2).expect("date");
        DayState::from_entries(&DayBounds::for_date(date, Helsinki), entries)
    }

    fn view(day: &DayState, now: DateTime<Utc>) -> TimerViewModel {
        compute(
            Some(day),
            &SyncStatus::default(),
            now,
            Helsinki,
            &WeeklyTargets::default(),
        )
    }

    #[test]
    fn no_entries_is_not_started() {
        let vm = view(&day_of((2026, 1, 5), &[]), at(5, 9, 0));
        assert_eq!(vm.status, TimerStatus::NotStarted);
        assert_eq!(vm.remaining_secs, TARGET);
        assert!(!vm.running);
    }

    #[test]
    fn no_sync_yet_is_not_started() {
        let vm = compute(
            None,
            &SyncStatus::default(),
            at(5, 9, 0),
            Helsinki,
            &WeeklyTargets::default(),
        );
        assert_eq!(vm.status, TimerStatus::NotStarted);
        assert_eq!(vm.remaining_secs, TARGET);
    }

    #[test]
    fn running_entry_counts_down_every_second() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), None)]);
        let first = view(&day, at(5, 9, 0));
        let second = view(&day, at(5, 9, 0) + Duration::seconds(1));
        assert_eq!(first.status, TimerStatus::Working);
        assert_eq!(first.remaining_secs - second.remaining_secs, 1);
        assert_eq!(first.eta, Some(at(5, 15, 30)));
    }

    #[test]
    fn break_freezes_remaining_and_counts_break() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), Some(at(5, 11, 0)))]);
        let first = view(&day, at(5, 11, 10));
        let later = view(&day, at(5, 11, 40));
        assert_eq!(first.status, TimerStatus::OnBreak);
        assert_eq!(first.remaining_secs, later.remaining_secs);
        assert_eq!(first.break_secs, 600);
        assert_eq!(later.break_secs, 2400);
        assert_eq!(first.eta, None);
    }

    #[test]
    fn overtime_is_done_with_negative_remaining() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), None)]);
        let vm = view(&day, at(5, 15, 42));
        assert_eq!(vm.status, TimerStatus::Done);
        assert_eq!(vm.remaining_secs, -12 * 60);
        assert!(vm.running);
        assert_eq!(vm.eta, None);
    }

    #[test]
    fn running_entry_from_yesterday_is_clipped_to_today() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 23, 0), None)]);
        // The stored state is from Monday; it is now Tuesday 01:00.
        let vm = view(&day, at(6, 1, 0));
        assert_eq!(vm.worked_secs, 3600);
        assert_eq!(vm.status, TimerStatus::Working);
    }

    #[test]
    fn stale_day_without_running_entry_starts_fresh() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), Some(at(5, 16, 0)))]);
        let vm = view(&day, at(6, 8, 0));
        assert_eq!(vm.status, TimerStatus::NotStarted);
        assert_eq!(vm.worked_secs, 0);
    }

    #[test]
    fn dst_day_does_not_break_computation() {
        // 2026-03-29 in Helsinki is 23 hours long (clocks jump 03:00 -> 04:00).
        let start = Helsinki.with_ymd_and_hms(2026, 3, 29, 2, 0, 0).unwrap();
        let stop = Helsinki.with_ymd_and_hms(2026, 3, 29, 5, 0, 0).unwrap();
        let day = day_of(
            (2026, 3, 29),
            &[entry(
                1,
                start.with_timezone(&Utc),
                Some(stop.with_timezone(&Utc)),
            )],
        );
        let mut targets = WeeklyTargets::default();
        targets.0[6] = 450; // Sunday
        let now = Helsinki.with_ymd_and_hms(2026, 3, 29, 12, 0, 0).unwrap();
        let vm = compute(
            Some(&day),
            &SyncStatus::default(),
            now.with_timezone(&Utc),
            Helsinki,
            &targets,
        );
        // 02:00 -> 05:00 local is only two real hours on this day.
        assert_eq!(vm.worked_secs, 2 * 3600);
        assert_eq!(vm.remaining_secs, TARGET - 2 * 3600);
    }

    #[test]
    fn uses_weekday_specific_target() {
        let mut targets = WeeklyTargets::default();
        targets.0[4] = 360; // Friday 6 h
        let friday = at(9, 9, 0);
        let vm = compute(None, &SyncStatus::default(), friday, Helsinki, &targets);
        assert_eq!(vm.target_secs, 6 * 3600);
    }

    #[test]
    fn work_on_zero_target_day_is_overtime() {
        let day = day_of((2026, 1, 10), &[entry(1, at(10, 10, 0), None)]);
        let vm = view(&day, at(10, 10, 30));
        assert_eq!(vm.target_secs, 0);
        assert_eq!(vm.status, TimerStatus::Done);
        assert_eq!(vm.remaining_secs, -1800);
    }

    #[test]
    fn passes_sync_status_through() {
        let sync = SyncStatus {
            last_synced: Some(at(5, 8, 0)),
            warning: SyncWarning::Offline,
        };
        let vm = compute(
            None,
            &sync,
            at(5, 9, 0),
            Helsinki,
            &WeeklyTargets::default(),
        );
        assert_eq!(vm.sync_warning, SyncWarning::Offline);
        assert_eq!(vm.last_synced, Some(at(5, 8, 0)));
    }

    fn completion_stop(day: &DayState, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        day_completion_stop(Some(day), now, Helsinki, &WeeklyTargets::default())
    }

    #[test]
    fn pause_near_the_end_stops_when_the_target_is_reached() {
        // 7:30 target, started 08:00: the day is full at 15:30.
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), None)]);
        assert_eq!(completion_stop(&day, at(5, 15, 16)), Some(at(5, 15, 30)));
        assert_eq!(
            completion_stop(&day, at(5, 15, 15) + Duration::seconds(1)),
            Some(at(5, 15, 30))
        );
    }

    #[test]
    fn pause_with_fifteen_minutes_or_more_left_stops_now() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), None)]);
        assert_eq!(completion_stop(&day, at(5, 15, 15)), None);
        assert_eq!(completion_stop(&day, at(5, 12, 0)), None);
    }

    #[test]
    fn pause_after_the_target_stops_now() {
        let day = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), None)]);
        assert_eq!(completion_stop(&day, at(5, 15, 30)), None);
        assert_eq!(completion_stop(&day, at(5, 16, 0)), None);
    }

    #[test]
    fn completion_stop_counts_earlier_entries() {
        let day = day_of(
            (2026, 1, 5),
            &[
                entry(1, at(5, 8, 0), Some(at(5, 12, 0))),
                entry(2, at(5, 12, 30), None),
            ],
        );
        // 4 h before lunch + 3 h 20 min after it: 10 min left at 15:50.
        assert_eq!(completion_stop(&day, at(5, 15, 50)), Some(at(5, 16, 0)));
    }

    #[test]
    fn completion_stop_needs_a_running_timer_and_a_target() {
        let on_break = day_of((2026, 1, 5), &[entry(1, at(5, 8, 0), Some(at(5, 15, 25)))]);
        assert_eq!(completion_stop(&on_break, at(5, 15, 26)), None);

        // Saturday has a zero target: any work is overtime.
        let saturday = day_of((2026, 1, 10), &[entry(1, at(10, 10, 0), None)]);
        assert_eq!(completion_stop(&saturday, at(10, 10, 5)), None);
    }
}
