//! Toggl Track API v9 (`api.track.toggl.com/api/v9`).
//!
//! Auth: HTTP Basic with the API token as user name and `api_token` as
//! password. Running entries have `stop: null` and a negative `duration`.

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::client::{rfc3339, Credentials, Result, TogglClient};
use super::models::{Account, EntryTags, StartEntry, TimeEntry};
use super::Flavor;

/// Value sent as `created_with` when the app creates time entries.
pub const CREATED_WITH: &str = "tyoaikalaskuri";

/// Wire format of a Track time entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackTimeEntry {
    pub id: i64,
    pub workspace_id: i64,
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    pub start: DateTime<Utc>,
    #[serde(default)]
    pub stop: Option<DateTime<Utc>>,
    /// Negative for a running entry.
    pub duration: i64,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub billable: bool,
}

impl From<TrackTimeEntry> for TimeEntry {
    fn from(entry: TrackTimeEntry) -> Self {
        let running = entry.stop.is_none() || entry.duration < 0;
        Self {
            id: entry.id,
            workspace_id: entry.workspace_id,
            project_id: entry.project_id,
            task_id: entry.task_id,
            description: entry.description,
            start: entry.start,
            stop: if running { None } else { entry.stop },
            tags: entry.tags.map_or(EntryTags::None, EntryTags::Names),
            billable: entry.billable,
        }
    }
}

/// Body of `POST /workspaces/{wid}/time_entries`.
#[derive(Debug, Serialize)]
struct NewTimeEntry<'a> {
    created_with: &'static str,
    workspace_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<&'a [String]>,
    billable: bool,
    start: String,
    /// `-1` starts a running entry.
    duration: i64,
}

/// Body of `PUT /workspaces/{wid}/time_entries/{id}` when only the end
/// changes.
#[derive(Debug, Serialize)]
struct SetStop {
    stop: String,
    duration: i64,
}

#[derive(Debug, Deserialize)]
struct Me {
    #[serde(default)]
    fullname: String,
    default_workspace_id: i64,
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    id: i64,
    name: String,
}

impl TogglClient {
    pub(super) async fn track_account(&self, credentials: &Credentials) -> Result<Account> {
        let url = self.url(
            Flavor::Track,
            "/me",
            &[("with_related_data", "true".to_owned())],
        )?;
        let me: Me = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        let workspace_name = me
            .workspaces
            .iter()
            .find(|w| w.id == me.default_workspace_id)
            .map(|w| w.name.clone());
        Ok(Account {
            flavor: Flavor::Track,
            name: Some(me.fullname).filter(|n| !n.is_empty()),
            workspace_id: me.default_workspace_id,
            workspace_name,
            organization_id: None,
        })
    }

    pub(super) async fn track_time_entries(
        &self,
        credentials: &Credentials,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TimeEntry>> {
        let url = self.url(
            Flavor::Track,
            "/me/time_entries",
            &[("start_date", rfc3339(start)), ("end_date", rfc3339(end))],
        )?;
        let entries: Vec<TrackTimeEntry> = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        Ok(entries.into_iter().map(TimeEntry::from).collect())
    }

    pub(super) async fn track_recent_time_entries(
        &self,
        credentials: &Credentials,
    ) -> Result<Vec<TimeEntry>> {
        let url = self.url(Flavor::Track, "/me/time_entries", &[])?;
        let entries: Vec<TrackTimeEntry> = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        Ok(entries.into_iter().map(TimeEntry::from).collect())
    }

    pub(super) async fn track_current_time_entry(
        &self,
        credentials: &Credentials,
    ) -> Result<Option<TimeEntry>> {
        let url = self.url(Flavor::Track, "/me/time_entries/current", &[])?;
        let entry: Option<TrackTimeEntry> = self
            .json(credentials, Method::GET, url, None::<&()>)
            .await?;
        Ok(entry.map(TimeEntry::from))
    }

    pub(super) async fn track_start(
        &self,
        credentials: &Credentials,
        entry: &StartEntry,
    ) -> Result<TimeEntry> {
        let url = self.url(
            Flavor::Track,
            &format!("/workspaces/{}/time_entries", entry.workspace_id),
            &[],
        )?;
        let tags = match &entry.tags {
            EntryTags::Names(names) => Some(names.as_slice()),
            EntryTags::None | EntryTags::Ids(_) => None,
        };
        let body = NewTimeEntry {
            created_with: CREATED_WITH,
            workspace_id: entry.workspace_id,
            description: entry.description.as_deref(),
            project_id: entry.project_id,
            task_id: entry.task_id,
            tags,
            billable: entry.billable,
            start: rfc3339(entry.start),
            duration: -1,
        };
        let created: TrackTimeEntry = self
            .json(credentials, Method::POST, url, Some(&body))
            .await?;
        Ok(created.into())
    }

    pub(super) async fn track_stop(
        &self,
        credentials: &Credentials,
        workspace_id: i64,
        entry_id: i64,
    ) -> Result<()> {
        let url = self.url(
            Flavor::Track,
            &format!("/workspaces/{workspace_id}/time_entries/{entry_id}/stop"),
            &[],
        )?;
        self.send(credentials, Method::PATCH, url, None::<&()>)
            .await
            .map(drop)
    }

    /// `PUT /workspaces/{wid}/time_entries/{id}` with a new end time. Toggl
    /// requires `stop` and `duration` to agree, so both are sent.
    pub(super) async fn track_set_stop(
        &self,
        credentials: &Credentials,
        workspace_id: i64,
        entry_id: i64,
        start: DateTime<Utc>,
        stop: DateTime<Utc>,
    ) -> Result<()> {
        let url = self.url(
            Flavor::Track,
            &format!("/workspaces/{workspace_id}/time_entries/{entry_id}"),
            &[],
        )?;
        let body = SetStop {
            stop: rfc3339(stop),
            duration: (stop - start).num_seconds().max(0),
        };
        self.send(credentials, Method::PUT, url, Some(&body))
            .await
            .map(drop)
    }
}

/// Track wire JSON for a domain entry. Used by tests to build mock responses.
#[cfg(test)]
pub(crate) fn to_json(entry: &TimeEntry) -> serde_json::Value {
    let wire = TrackTimeEntry {
        id: entry.id,
        workspace_id: entry.workspace_id,
        project_id: entry.project_id,
        task_id: entry.task_id,
        description: entry.description.clone(),
        start: entry.start,
        stop: entry.stop,
        duration: entry
            .stop
            .map_or(-1, |stop| (stop - entry.start).num_seconds()),
        tags: match &entry.tags {
            EntryTags::Names(names) => Some(names.clone()),
            _ => None,
        },
        billable: entry.billable,
    };
    serde_json::to_value(wire).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toggl::client::testing::{client, system_clock};
    use wiremock::matchers::{basic_auth, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn maps_running_entry() {
        let json = r#"{
            "id": 1, "workspace_id": 2, "project_id": null, "description": "Koodaus",
            "start": "2026-03-02T07:00:00+00:00", "stop": null, "duration": -1772434800,
            "tags": ["a"], "billable": false, "at": "2026-03-02T07:00:00+00:00"
        }"#;
        let entry: TimeEntry = serde_json::from_str::<TrackTimeEntry>(json)
            .expect("valid entry")
            .into();
        assert!(entry.is_running());
        assert_eq!(entry.description.as_deref(), Some("Koodaus"));
        assert_eq!(entry.tags, EntryTags::Names(vec!["a".into()]));
    }

    #[tokio::test]
    async fn uses_basic_auth_and_parses_empty_current_entry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me/time_entries/current"))
            .and(basic_auth("secret-token", "api_token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("null"))
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server, "secret-token", system_clock());
        assert_eq!(client.current_time_entry().await.expect("ok"), None);
        assert_eq!(client.requests_in_last_hour(), 1);
    }

    #[tokio::test]
    async fn passes_date_range_as_rfc3339() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me/time_entries"))
            .and(query_param("start_date", "2026-03-01T22:00:00Z"))
            .and(query_param("end_date", "2026-03-02T22:00:00Z"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server, "secret-token", system_clock());
        let start = "2026-03-01T22:00:00Z".parse().expect("date");
        let end = "2026-03-02T22:00:00Z".parse().expect("date");
        assert!(client
            .time_entries(start, end)
            .await
            .expect("ok")
            .is_empty());
    }

    #[tokio::test]
    async fn reads_account_with_default_workspace_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .and(query_param("with_related_data", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 1, "fullname": "Testi", "default_workspace_id": 7,
                "workspaces": [{"id": 3, "name": "Muu"}, {"id": 7, "name": "Oma"}]
            })))
            .mount(&server)
            .await;

        let client = client(&server, "secret-token", system_clock());
        let account = client.account().await.expect("account");
        assert_eq!(account.flavor, Flavor::Track);
        assert_eq!(account.name.as_deref(), Some("Testi"));
        assert_eq!(account.workspace_name.as_deref(), Some("Oma"));
    }
}
