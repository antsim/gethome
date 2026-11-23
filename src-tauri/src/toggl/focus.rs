//! Toggl 2.0 API (`focus.toggl.com/api`).
//!
//! Auth: `Authorization: Bearer toggl_sk_…`. Time entries live under
//! `/organizations/{org}/workspaces/{ws}`; the organization and workspace are
//! resolved once per token from `GET /users/me/landing-workspace`.
//!
//! Differences from Toggl Track that matter here:
//! - entries have no `stop`; the end is `start + duration`, and a running
//!   entry has no `duration`;
//! - entries can be planned-only (no `start`) or of type `break`; neither
//!   counts as work;
//! - the timer is controlled with `tracking/start|stop|current`.

use chrono::{DateTime, Duration, Utc};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

use super::client::{rfc3339, Credentials, Result, TogglClient, TogglError};
use super::models::{Account, EntryTags, StartEntry, TimeEntry};
use super::Flavor;

const BREAK_TYPE: &str = "break";
const ACTIVITY_TYPE: &str = "activity";
/// How far back to look for the latest entry when resuming work.
const RECENT_DAYS: i64 = 30;
const RECENT_PAGE_SIZE: u32 = 20;

/// The organization and workspace whose entries the app reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusContext {
    pub organization_id: i64,
    pub workspace_id: i64,
}

#[derive(Debug, Deserialize)]
struct LandingWorkspace {
    organization_id: i64,
    workspace_id: i64,
}

#[derive(Debug, Deserialize)]
struct Page<T> {
    data: Vec<T>,
}

/// Wire format of a Toggl 2.0 time entry (the fields the app needs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusTimeEntry {
    pub id: i64,
    pub workspace_id: i64,
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    /// Absent for planned-only entries.
    #[serde(default)]
    pub start: Option<DateTime<Utc>>,
    /// Seconds; absent while the timer is running.
    #[serde(default)]
    pub duration: Option<i64>,
    /// Manual tag override; `None` inherits the task's tags.
    #[serde(default)]
    pub tag_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub billable: bool,
    /// `activity` or `break`.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl FocusTimeEntry {
    /// Maps a tracked work entry to the domain model. Planned-only entries,
    /// breaks and deleted entries return `None`.
    pub fn into_work_entry(self) -> Option<TimeEntry> {
        if self.deleted_at.is_some() || self.kind.as_deref() == Some(BREAK_TYPE) {
            return None;
        }
        let start = self.start?;
        Some(TimeEntry {
            id: self.id,
            workspace_id: self.workspace_id,
            project_id: self.project_id,
            task_id: self.task_id,
            description: self.description,
            start,
            stop: self.duration.map(|secs| start + Duration::seconds(secs)),
            tags: self.tag_ids.map_or(EntryTags::None, EntryTags::Ids),
            billable: self.billable,
        })
    }
}

/// Body of `POST …/tracking/start`.
#[derive(Debug, Serialize)]
struct StartTracking<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag_ids: Option<&'a [i64]>,
    billable: bool,
    start: String,
    #[serde(rename = "type")]
    kind: &'static str,
}

/// Body of `POST …/tracking/stop`.
#[derive(Debug, Serialize)]
struct StopTracking {
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<String>,
}

impl TogglClient {
    pub(super) async fn focus_account(&self, credentials: &Credentials) -> Result<Account> {
        let context = self.fetch_focus_context(credentials).await?;
        Ok(Account {
            flavor: Flavor::Focus,
            name: None,
            workspace_id: context.workspace_id,
            workspace_name: None,
            organization_id: Some(context.organization_id),
        })
    }

    pub(super) async fn focus_time_entries(
        &self,
        credentials: &Credentials,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TimeEntry>> {
        let context = self.focus_context(credentials).await?;
        let url = self.url(
            Flavor::Focus,
            &entries_path(&context, "/time-entries/stream"),
            &[
                ("date_from", rfc3339(start)),
                ("date_to", rfc3339(end)),
                ("include_taskless", "true".to_owned()),
            ],
        )?;
        let entries: Vec<FocusTimeEntry> = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        Ok(work_entries(entries))
    }

    pub(super) async fn focus_recent_time_entries(
        &self,
        credentials: &Credentials,
    ) -> Result<Vec<TimeEntry>> {
        let context = self.focus_context(credentials).await?;
        let now = self.clock.now();
        let url = self.url(
            Flavor::Focus,
            &entries_path(&context, "/time-entries"),
            &[
                ("date_from", rfc3339(now - Duration::days(RECENT_DAYS))),
                ("date_to", rfc3339(now)),
                ("include_taskless", "true".to_owned()),
                ("order_by", "-start".to_owned()),
                ("per_page", RECENT_PAGE_SIZE.to_string()),
            ],
        )?;
        let page: Page<FocusTimeEntry> = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        Ok(work_entries(page.data))
    }

    pub(super) async fn focus_current_time_entry(
        &self,
        credentials: &Credentials,
    ) -> Result<Option<TimeEntry>> {
        let context = self.focus_context(credentials).await?;
        let url = self.url(
            Flavor::Focus,
            &entries_path(&context, "/tracking/current"),
            &[],
        )?;
        let response = self
            .send(credentials, Method::GET, url, None::<&()>)
            .await?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let entry: FocusTimeEntry = response.json().await.map_err(TogglError::Decode)?;
        // A running break is not work.
        Ok(entry.into_work_entry())
    }

    pub(super) async fn focus_start(
        &self,
        credentials: &Credentials,
        entry: &StartEntry,
    ) -> Result<TimeEntry> {
        let context = self.focus_context(credentials).await?;
        let url = self.url(
            Flavor::Focus,
            &entries_path(&context, "/tracking/start"),
            &[],
        )?;
        let tag_ids = match &entry.tags {
            EntryTags::Ids(ids) => Some(ids.as_slice()),
            EntryTags::None | EntryTags::Names(_) => None,
        };
        let body = StartTracking {
            description: entry.description.as_deref(),
            project_id: entry.project_id,
            task_id: entry.task_id,
            tag_ids,
            billable: entry.billable,
            start: rfc3339(entry.start),
            kind: ACTIVITY_TYPE,
        };
        let created: FocusTimeEntry = self
            .json(credentials, Method::POST, url, Some(&body))
            .await?;
        let id = created.id;
        let workspace_id = created.workspace_id;
        // The response is a running activity; fall back to the request if
        // the server omitted fields.
        Ok(created.into_work_entry().unwrap_or_else(|| TimeEntry {
            id,
            workspace_id,
            project_id: entry.project_id,
            task_id: entry.task_id,
            description: entry.description.clone(),
            start: entry.start,
            stop: None,
            tags: entry.tags.clone(),
            billable: entry.billable,
        }))
    }

    /// `POST …/tracking/stop`. With `end`, the entry ends then (possibly in
    /// the future) instead of now.
    pub(super) async fn focus_stop(
        &self,
        credentials: &Credentials,
        end: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let context = self.focus_context(credentials).await?;
        let url = self.url(
            Flavor::Focus,
            &entries_path(&context, "/tracking/stop"),
            &[],
        )?;
        let body = StopTracking {
            end: end.map(rfc3339),
        };
        match self.send(credentials, Method::POST, url, Some(&body)).await {
            Ok(_) => Ok(()),
            // 404: not tracking, i.e. already stopped elsewhere.
            Err(TogglError::Status { status: 404, .. }) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// The cached organization and workspace, resolving it on first use.
    async fn focus_context(&self, credentials: &Credentials) -> Result<FocusContext> {
        let cached = *self.focus_context.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = cached {
            return Ok(context);
        }
        let context = self.fetch_focus_context(credentials).await?;
        *self.focus_context.lock().unwrap_or_else(|e| e.into_inner()) = Some(context);
        Ok(context)
    }

    async fn fetch_focus_context(&self, credentials: &Credentials) -> Result<FocusContext> {
        let url = self.url(Flavor::Focus, "/users/me/landing-workspace", &[])?;
        let response = self
            .send(credentials, Method::GET, url, None::<&()>)
            .await?;
        if response.status() == StatusCode::NO_CONTENT {
            return Err(TogglError::NoWorkspace);
        }
        let landing: LandingWorkspace = response.json().await.map_err(TogglError::Decode)?;
        Ok(FocusContext {
            organization_id: landing.organization_id,
            workspace_id: landing.workspace_id,
        })
    }
}

fn entries_path(context: &FocusContext, suffix: &str) -> String {
    format!(
        "/organizations/{}/workspaces/{}{suffix}",
        context.organization_id, context.workspace_id
    )
}

fn work_entries(entries: Vec<FocusTimeEntry>) -> Vec<TimeEntry> {
    entries
        .into_iter()
        .filter_map(FocusTimeEntry::into_work_entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toggl::client::testing::{client, system_clock};
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN: &str = "toggl_sk_test";
    const BASE: &str = "/organizations/10/workspaces/20";

    async fn server_with_landing() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/landing-workspace"))
            .and(header("authorization", "Bearer toggl_sk_test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "organization_id": 10, "workspace_id": 20, "reason": "default"
            })))
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn maps_entries_and_skips_breaks_and_planned() {
        let entries: Vec<FocusTimeEntry> = serde_json::from_value(json!([
            {"id": 1, "workspace_id": 20, "start": "2026-03-02T07:00:00Z", "duration": 3600,
             "type": "activity", "tag_ids": [5]},
            {"id": 2, "workspace_id": 20, "start": "2026-03-02T08:00:00Z", "duration": 600,
             "type": "break"},
            {"id": 3, "workspace_id": 20, "planned_start": "2026-03-02T09:00:00Z",
             "planned_duration": 3600},
            {"id": 4, "workspace_id": 20, "start": "2026-03-02T09:00:00Z"}
        ]))
        .expect("valid json");
        let mapped = work_entries(entries);
        assert_eq!(mapped.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 4]);
        assert_eq!(
            mapped[0].stop,
            Some("2026-03-02T08:00:00Z".parse().expect("date"))
        );
        assert_eq!(mapped[0].tags, EntryTags::Ids(vec![5]));
        assert!(mapped[1].is_running());
    }

    #[tokio::test]
    async fn resolves_workspace_once_and_lists_entries() {
        let server = server_with_landing().await;
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/time-entries/stream")))
            .and(query_param("date_from", "2026-03-01T22:00:00Z"))
            .and(query_param("include_taskless", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id": 1, "workspace_id": 20, "start": "2026-03-02T07:00:00Z", "duration": 60}
            ])))
            .expect(2)
            .mount(&server)
            .await;

        let client = client(&server, TOKEN, system_clock());
        let start = "2026-03-01T22:00:00Z".parse().expect("date");
        let end = "2026-03-02T22:00:00Z".parse().expect("date");
        assert_eq!(client.time_entries(start, end).await.expect("ok").len(), 1);
        client.time_entries(start, end).await.expect("ok");
        // landing-workspace + two list calls.
        assert_eq!(client.requests_in_last_hour(), 3);
    }

    #[tokio::test]
    async fn no_content_means_no_running_timer() {
        let server = server_with_landing().await;
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/tracking/current")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = client(&server, TOKEN, system_clock());
        assert_eq!(client.current_time_entry().await.expect("ok"), None);
    }

    #[tokio::test]
    async fn running_break_is_not_work() {
        let server = server_with_landing().await;
        Mock::given(method("GET"))
            .and(path(format!("{BASE}/tracking/current")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 9, "workspace_id": 20, "start": "2026-03-02T07:00:00Z", "type": "break"
            })))
            .mount(&server)
            .await;

        let client = client(&server, TOKEN, system_clock());
        assert_eq!(client.current_time_entry().await.expect("ok"), None);
    }

    #[tokio::test]
    async fn starts_and_stops_tracking() {
        let server = server_with_landing().await;
        Mock::given(method("POST"))
            .and(path(format!("{BASE}/tracking/start")))
            .and(body_partial_json(json!({
                "description": "Koodaus", "project_id": 3, "tag_ids": [5], "type": "activity"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 77, "workspace_id": 20, "start": "2026-03-02T07:00:00Z",
                "description": "Koodaus", "project_id": 3, "type": "activity"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("{BASE}/tracking/stop")))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server, TOKEN, system_clock());
        let started = client
            .start_time_entry(&StartEntry {
                workspace_id: 20,
                project_id: Some(3),
                task_id: None,
                description: Some("Koodaus".into()),
                tags: EntryTags::Ids(vec![5]),
                billable: false,
                start: "2026-03-02T07:00:00Z".parse().expect("date"),
            })
            .await
            .expect("started");
        assert_eq!(started.id, 77);
        assert!(started.is_running());
        // 404 "not tracking" counts as already stopped.
        client.stop_time_entry(20, 77).await.expect("stopped");
    }

    #[tokio::test]
    async fn account_without_workspace_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/landing-workspace"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = client(&server, TOKEN, system_clock());
        assert!(matches!(
            client.verify_token(TOKEN).await,
            Err(TogglError::NoWorkspace)
        ));
    }

    #[tokio::test]
    async fn verifies_key_and_reports_workspace() {
        let server = server_with_landing().await;
        let client = client(&server, TOKEN, system_clock());
        let account = client.verify_token(TOKEN).await.expect("account");
        assert_eq!(account.flavor, Flavor::Focus);
        assert_eq!(account.organization_id, Some(10));
        assert_eq!(account.workspace_id, 20);
    }
}
