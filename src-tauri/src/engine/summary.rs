//! Pure computation of the day summary: today's work grouped by task and
//! rounded to half hours for copying into a timesheet. No I/O; `now` is a
//! parameter.
//!
//! Rounding (PLAN.md, chapter 12):
//! 1. The day's total is the worked time rounded to the nearest half hour.
//! 2. Each task first gets its own time rounded down to a half hour. The
//!    half hours that are still missing from the total go to the tasks that
//!    lost the most in rounding (largest remainder method). Every task thus
//!    gets whole or half hours, and the tasks always add up to the total.
//! 3. Tasks that end up with zero hours are left out.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use super::calendar::DayBounds;
use super::state::DayState;

/// Granularity of booked time.
pub const BOOKING_UNIT_SECS: i64 = 30 * 60;

/// One task of the day: all entries with the same description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryTask {
    /// Trimmed description; empty when the entries have none.
    pub description: String,
    /// Actual tracked time today.
    pub worked_secs: i64,
    /// Rounded time to book, a positive multiple of 30.
    pub booked_minutes: i64,
}

/// The day's tasks, in the order they were first worked on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub date: NaiveDate,
    pub worked_secs: i64,
    /// Sum of `booked_minutes` of the tasks.
    pub booked_minutes: i64,
    pub tasks: Vec<SummaryTask>,
    /// Whether a timer is still running (the numbers keep changing).
    pub running: bool,
}

/// Builds the summary of the day containing `now`.
///
/// Like [`super::compute`], a `day` from an earlier date contributes only
/// its running entry.
pub fn day_summary(day: Option<&DayState>, now: DateTime<Utc>, tz: Tz) -> DaySummary {
    let today = DayBounds::containing(now, tz);
    let is_today = day.is_some_and(|d| d.date == today.date);

    // (description, start, secs) of every piece of work today.
    let mut pieces: Vec<(&str, DateTime<Utc>, i64)> = Vec::new();
    if let Some(day) = day.filter(|_| is_today) {
        for segment in &day.segments {
            pieces.push((
                description_key(segment.description.as_deref()),
                segment.start,
                today.overlap_secs(segment.start, segment.stop),
            ));
        }
    }
    let running = day.and_then(|d| d.running.as_ref());
    if let Some(entry) = running {
        pieces.push((
            description_key(entry.description.as_deref()),
            entry.start,
            today.overlap_secs(entry.start, now),
        ));
    }
    pieces.retain(|(_, _, secs)| *secs > 0);
    pieces.sort_by_key(|(_, start, _)| *start);

    let mut tasks: Vec<SummaryTask> = Vec::new();
    for (description, _, secs) in pieces {
        match tasks.iter_mut().find(|t| t.description == description) {
            Some(task) => task.worked_secs += secs,
            None => tasks.push(SummaryTask {
                description: description.to_owned(),
                worked_secs: secs,
                booked_minutes: 0,
            }),
        }
    }

    let worked_secs = tasks.iter().map(|t| t.worked_secs).sum();
    let units = distribute_units(&tasks, round_to_units(worked_secs));
    for (task, units) in tasks.iter_mut().zip(units) {
        task.booked_minutes = units * BOOKING_UNIT_SECS / 60;
    }
    tasks.retain(|t| t.booked_minutes > 0);

    DaySummary {
        date: today.date,
        worked_secs,
        booked_minutes: tasks.iter().map(|t| t.booked_minutes).sum(),
        tasks,
        running: running.is_some(),
    }
}

fn description_key(description: Option<&str>) -> &str {
    description.map_or("", str::trim)
}

/// Seconds rounded to the nearest whole unit; halves round up.
fn round_to_units(secs: i64) -> i64 {
    (secs + BOOKING_UNIT_SECS / 2) / BOOKING_UNIT_SECS
}

/// Splits `total` units between the tasks by the largest remainder method.
/// Ties go to the task with more time, then to the earlier one.
fn distribute_units(tasks: &[SummaryTask], total: i64) -> Vec<i64> {
    let mut units: Vec<i64> = tasks
        .iter()
        .map(|t| t.worked_secs / BOOKING_UNIT_SECS)
        .collect();
    let mut left = total - units.iter().sum::<i64>();

    let mut order: Vec<usize> = (0..tasks.len()).collect();
    order.sort_by_key(|&i| {
        let task = &tasks[i];
        (
            std::cmp::Reverse(task.worked_secs % BOOKING_UNIT_SECS),
            std::cmp::Reverse(task.worked_secs),
            i,
        )
    });
    // `left` never exceeds the task count (the total is within half a unit
    // of the sum), but guard anyway so that the loop is obviously finite.
    for &i in order.iter().cycle().take(order.len() * 2) {
        if left <= 0 {
            break;
        }
        units[i] += 1;
        left -= 1;
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::state::tests::entry;
    use crate::engine::state::EntryTemplate;
    use crate::toggl::TimeEntry;
    use chrono::{Duration, TimeZone};
    use chrono_tz::Europe::Helsinki;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Helsinki
            .with_ymd_and_hms(2026, 1, 5, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn task(id: i64, name: &str, start: DateTime<Utc>, minutes: i64) -> TimeEntry {
        TimeEntry {
            description: Some(name.into()),
            ..entry(id, start, Some(start + Duration::minutes(minutes)))
        }
    }

    fn summary_of(entries: &[TimeEntry], now: DateTime<Utc>) -> DaySummary {
        let day = DayState::from_entries(&DayBounds::containing(now, Helsinki), entries);
        day_summary(Some(&day), now, Helsinki)
    }

    fn booked(summary: &DaySummary) -> Vec<(&str, i64)> {
        summary
            .tasks
            .iter()
            .map(|t| (t.description.as_str(), t.booked_minutes))
            .collect()
    }

    #[test]
    fn full_day_is_split_into_half_hours_that_add_up() {
        let summary = summary_of(
            &[
                task(1, "Koodaus", at(8, 0), 220),
                task(2, "Code review", at(11, 40), 130),
                task(3, "Palaveri", at(13, 50), 65),
                task(4, "Sähköpostit", at(14, 55), 35),
            ],
            at(16, 0),
        );
        assert_eq!(summary.worked_secs, 450 * 60);
        assert_eq!(summary.booked_minutes, 450);
        assert_eq!(
            booked(&summary),
            [
                ("Koodaus", 240),
                ("Code review", 120),
                ("Palaveri", 60),
                ("Sähköpostit", 30)
            ]
        );
    }

    #[test]
    fn total_rounds_to_the_nearest_half_hour() {
        // 7:10 → 7:00
        let down = summary_of(&[task(1, "A", at(8, 0), 430)], at(16, 0));
        assert_eq!(down.booked_minutes, 420);
        // 7:20 → 7:30
        let up = summary_of(&[task(1, "A", at(8, 0), 440)], at(16, 0));
        assert_eq!(up.booked_minutes, 450);
        // 8:10 of overtime → 8:00
        let over = summary_of(&[task(1, "A", at(8, 0), 490)], at(17, 0));
        assert_eq!(over.booked_minutes, 480);
        // Exactly a quarter rounds up.
        let half = summary_of(&[task(1, "A", at(8, 0), 15)], at(9, 0));
        assert_eq!(half.booked_minutes, 30);
    }

    #[test]
    fn entries_with_the_same_description_are_merged() {
        let summary = summary_of(
            &[
                task(1, "Koodaus", at(8, 0), 60),
                task(2, "Palaveri", at(9, 0), 60),
                task(3, " Koodaus ", at(10, 0), 60),
            ],
            at(12, 0),
        );
        assert_eq!(booked(&summary), [("Koodaus", 120), ("Palaveri", 60)]);
        assert_eq!(summary.tasks[0].worked_secs, 7200);
    }

    #[test]
    fn short_tasks_can_drop_out() {
        let summary = summary_of(
            &[
                task(1, "Koodaus", at(8, 0), 60),
                task(2, "Kahvi", at(9, 0), 10),
            ],
            at(12, 0),
        );
        // 1:10 → 1:00, all of it to the longer task.
        assert_eq!(booked(&summary), [("Koodaus", 60)]);
        assert_eq!(summary.booked_minutes, 60);
        assert_eq!(summary.worked_secs, 70 * 60);
    }

    #[test]
    fn missing_description_is_its_own_task() {
        let mut blank = task(1, "", at(8, 0), 60);
        blank.description = None;
        let summary = summary_of(&[blank, task(2, "  ", at(9, 0), 60)], at(12, 0));
        assert_eq!(booked(&summary), [("", 120)]);
    }

    #[test]
    fn ties_go_to_the_longer_task() {
        // Both lose 20 min in rounding; one extra half hour is available.
        let summary = summary_of(
            &[task(1, "A", at(8, 0), 50), task(2, "B", at(9, 0), 80)],
            at(12, 0),
        );
        // 2:10 → 2:00. A rounds down to 0:30 and B to 1:00; B is longer.
        assert_eq!(booked(&summary), [("A", 30), ("B", 90)]);
    }

    #[test]
    fn running_entry_counts_up_to_now() {
        let mut day = DayState::from_entries(
            &DayBounds::containing(at(8, 0), Helsinki),
            &[task(1, "Koodaus", at(8, 0), 120)],
        );
        let template = EntryTemplate {
            description: Some("Palaveri".into()),
            ..EntryTemplate::blank(1)
        };
        day.start_running(at(10, 0), template);
        let summary = day_summary(Some(&day), at(11, 0), Helsinki);
        assert!(summary.running);
        assert_eq!(booked(&summary), [("Koodaus", 120), ("Palaveri", 60)]);
    }

    #[test]
    fn entry_crossing_midnight_is_clipped() {
        let day = DayBounds::containing(at(8, 0), Helsinki);
        let summary = summary_of(
            &[task(1, "Yövuoro", day.start - Duration::hours(1), 120)],
            at(8, 0),
        );
        assert_eq!(summary.worked_secs, 3600);
        assert_eq!(booked(&summary), [("Yövuoro", 60)]);
    }

    #[test]
    fn stale_day_without_running_entry_is_empty() {
        let day = DayState::from_entries(
            &DayBounds::containing(at(8, 0), Helsinki),
            &[task(1, "A", at(8, 0), 60)],
        );
        let tomorrow = at(8, 0) + Duration::days(1);
        let summary = day_summary(Some(&day), tomorrow, Helsinki);
        assert!(summary.tasks.is_empty());
        assert_eq!(summary.booked_minutes, 0);
        assert_eq!(summary.date, tomorrow.with_timezone(&Helsinki).date_naive());
    }

    #[test]
    fn many_tasks_always_add_up_to_the_total() {
        for minutes in 1..=120 {
            let entries: Vec<TimeEntry> = (0..7)
                .map(|i| {
                    task(
                        i,
                        &format!("T{i}"),
                        at(8, 0) + Duration::minutes(i * 130),
                        minutes + i * 7,
                    )
                })
                .collect();
            let summary = summary_of(&entries, at(23, 59));
            assert_eq!(
                summary.booked_minutes,
                round_to_units(summary.worked_secs) * 30
            );
            assert!(summary.tasks.iter().all(|t| t.booked_minutes % 30 == 0));
        }
    }
}
