//! The engine's model of the current day, built from Toggl time entries.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

use super::calendar::DayBounds;
use crate::toggl::{EntryTags, StartEntry, TimeEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TimerStatus {
    /// No time entries today.
    NotStarted,
    /// A timer is running and the target has not been reached.
    Working,
    /// No timer is running, but there are entries today.
    OnBreak,
    /// The target has been reached (a running timer means overtime).
    Done,
}

/// The currently running time entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningEntry {
    /// `None` while an optimistic start is waiting for Toggl's response.
    pub id: Option<i64>,
    pub workspace_id: i64,
    pub start: DateTime<Utc>,
    pub description: Option<String>,
}

/// A finished time entry that overlaps the day. Used for the day summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaySegment {
    pub description: Option<String>,
    pub start: DateTime<Utc>,
    pub stop: DateTime<Utc>,
}

/// The fields copied from an earlier entry when work is resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTemplate {
    pub workspace_id: i64,
    pub project_id: Option<i64>,
    pub task_id: Option<i64>,
    pub description: Option<String>,
    pub tags: EntryTags,
    pub billable: bool,
}

impl EntryTemplate {
    /// A template with no description or project.
    pub fn blank(workspace_id: i64) -> Self {
        Self {
            workspace_id,
            project_id: None,
            task_id: None,
            description: None,
            tags: EntryTags::None,
            billable: false,
        }
    }

    /// The template of the entry that started last.
    pub fn latest_of(entries: &[TimeEntry]) -> Option<Self> {
        entries.iter().max_by_key(|e| e.start).map(Self::from)
    }

    /// A request that starts a running entry at `start`.
    pub fn to_start_entry(&self, start: DateTime<Utc>) -> StartEntry {
        StartEntry {
            workspace_id: self.workspace_id,
            project_id: self.project_id,
            task_id: self.task_id,
            description: self.description.clone(),
            tags: self.tags.clone(),
            billable: self.billable,
            start,
        }
    }
}

impl From<&TimeEntry> for EntryTemplate {
    fn from(entry: &TimeEntry) -> Self {
        Self {
            workspace_id: entry.workspace_id,
            project_id: entry.project_id,
            task_id: entry.task_id,
            description: entry.description.clone(),
            tags: entry.tags.clone(),
            billable: entry.billable,
        }
    }
}

/// Summary of one local day's time entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayState {
    pub date: NaiveDate,
    /// Sum of finished entries, clipped to this day.
    pub completed_secs: i64,
    /// Whether any entry (finished or running) overlaps this day.
    pub has_entries: bool,
    pub running: Option<RunningEntry>,
    /// End of the latest finished entry today; the start of the current break.
    pub last_stop: Option<DateTime<Utc>>,
    /// The most recently started entry, used when resuming work.
    pub latest_entry: Option<EntryTemplate>,
    /// Finished entries that overlap this day (unclipped).
    pub segments: Vec<DaySegment>,
}

impl DayState {
    pub fn empty(date: NaiveDate) -> Self {
        Self {
            date,
            completed_secs: 0,
            has_entries: false,
            running: None,
            last_stop: None,
            latest_entry: None,
            segments: Vec::new(),
        }
    }

    /// Builds the day from entries. `entries` may contain entries from other
    /// days (e.g. one that started yesterday and crossed midnight); only the
    /// part within `day` counts.
    pub fn from_entries(day: &DayBounds, entries: &[TimeEntry]) -> Self {
        let mut state = Self::empty(day.date);
        state.latest_entry = EntryTemplate::latest_of(entries);

        for entry in entries {
            if entry.is_running() {
                // Toggl allows only one running entry; keep the latest.
                if state.running.as_ref().is_none_or(|r| r.start < entry.start) {
                    state.running = Some(RunningEntry {
                        id: Some(entry.id),
                        workspace_id: entry.workspace_id,
                        start: entry.start,
                        description: entry.description.clone(),
                    });
                }
                if entry.start < day.end {
                    state.has_entries = true;
                }
                continue;
            }

            let Some(stop) = entry.stop else { continue };
            let secs = day.overlap_secs(entry.start, stop);
            if secs > 0 {
                state.completed_secs += secs;
                state.has_entries = true;
                state.last_stop = state.last_stop.max(Some(stop.min(day.end)));
                state.segments.push(DaySegment {
                    description: entry.description.clone(),
                    start: entry.start,
                    stop,
                });
            }
        }
        state
    }

    /// Optimistically marks the running entry as stopped at `at`.
    /// Returns the entry that was stopped.
    pub fn stop_running(&mut self, at: DateTime<Utc>, day: &DayBounds) -> Option<RunningEntry> {
        let running = self.running.take()?;
        self.completed_secs += day.overlap_secs(running.start, at);
        self.last_stop = Some(at);
        self.has_entries = true;
        self.segments.push(DaySegment {
            description: running.description.clone(),
            start: running.start,
            stop: at,
        });
        Some(running)
    }

    /// Optimistically moves the end of the latest finished entry from `from`
    /// back to `to` (used when a future stop time is withdrawn).
    pub fn move_last_stop(&mut self, from: DateTime<Utc>, to: DateTime<Utc>, day: &DayBounds) {
        self.completed_secs -= day.overlap_secs(to, from);
        if self.last_stop == Some(from) {
            self.last_stop = Some(to);
        }
        if let Some(segment) = self.segments.iter_mut().rev().find(|s| s.stop == from) {
            segment.stop = to;
        }
    }

    /// Optimistically marks a new entry as running from `at`.
    pub fn start_running(&mut self, at: DateTime<Utc>, template: EntryTemplate) {
        self.running = Some(RunningEntry {
            id: None,
            workspace_id: template.workspace_id,
            start: at,
            description: template.description.clone(),
        });
        self.latest_entry = Some(template);
        self.has_entries = true;
    }
}

/// Problems talking to Toggl, shown as a warning in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncWarning {
    #[default]
    None,
    /// HTTP 402/429: Toggl asked us to slow down.
    RateLimited,
    /// Network error or Toggl unavailable.
    Offline,
    /// The API token was rejected; polling is paused until it changes.
    AuthError,
    /// No API token has been configured yet.
    NoToken,
}

/// Health of the connection to Toggl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncStatus {
    pub last_synced: Option<DateTime<Utc>>,
    pub warning: SyncWarning,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use chrono_tz::Europe::Helsinki;

    /// Builds a finished entry, or a running one when `stop` is `None`.
    pub(crate) fn entry(id: i64, start: DateTime<Utc>, stop: Option<DateTime<Utc>>) -> TimeEntry {
        TimeEntry {
            id,
            workspace_id: 1,
            project_id: Some(10),
            task_id: None,
            description: Some(format!("entry {id}")),
            start,
            stop,
            tags: EntryTags::None,
            billable: false,
        }
    }

    fn monday() -> DayBounds {
        DayBounds::for_date(NaiveDate::from_ymd_opt(2026, 1, 5).expect("date"), Helsinki)
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Helsinki
            .with_ymd_and_hms(2026, 1, 5, h, m, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn sums_finished_entries_and_tracks_running_one() {
        let entries = [
            entry(1, at(8, 0), Some(at(11, 0))),
            entry(2, at(11, 30), Some(at(12, 0))),
            entry(3, at(12, 30), None),
        ];
        let state = DayState::from_entries(&monday(), &entries);
        assert_eq!(state.completed_secs, 3 * 3600 + 1800);
        assert_eq!(state.last_stop, Some(at(12, 0)));
        assert_eq!(state.running.as_ref().and_then(|r| r.id), Some(3));
        assert_eq!(
            state.latest_entry.and_then(|t| t.description),
            Some("entry 3".into())
        );
    }

    #[test]
    fn clips_entry_that_crosses_midnight() {
        let day = monday();
        let entries = [entry(
            1,
            day.start - Duration::hours(1),
            Some(day.start + Duration::hours(1)),
        )];
        let state = DayState::from_entries(&day, &entries);
        assert_eq!(state.completed_secs, 3600);
        assert!(state.has_entries);
    }

    #[test]
    fn ignores_entries_from_yesterday_only() {
        let day = monday();
        let entries = [entry(
            1,
            day.start - Duration::hours(3),
            Some(day.start - Duration::hours(2)),
        )];
        let state = DayState::from_entries(&day, &entries);
        assert!(!state.has_entries);
        assert_eq!(state.completed_secs, 0);
        assert!(state.segments.is_empty());
        // Yesterday's entry is still a valid template for resuming.
        assert!(state.latest_entry.is_some());
    }

    #[test]
    fn optimistic_stop_and_start() {
        let day = monday();
        let mut state = DayState::from_entries(&day, &[entry(1, at(8, 0), None)]);
        let stopped = state.stop_running(at(9, 0), &day).expect("was running");
        assert_eq!(stopped.id, Some(1));
        assert_eq!(state.completed_secs, 3600);
        assert_eq!(state.last_stop, Some(at(9, 0)));
        assert_eq!(state.segments.len(), 1);
        assert_eq!(state.segments[0].description.as_deref(), Some("entry 1"));

        state.start_running(at(9, 15), EntryTemplate::blank(1));
        assert_eq!(state.running.as_ref().map(|r| r.start), Some(at(9, 15)));
    }
}
