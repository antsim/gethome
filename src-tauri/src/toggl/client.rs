//! HTTP client for Toggl: authentication, rate budget, error mapping and the
//! product-independent public API. The product-specific endpoints live in
//! [`super::track`] and [`super::focus`].

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration as StdDuration;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Method, Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::focus::FocusContext;
use super::models::{Account, StartEntry, TimeEntry};
use super::rate_budget::RateBudget;
use crate::clock::Clock;

pub const TRACK_BASE_URL: &str = "https://api.track.toggl.com/api/v9";
pub const FOCUS_BASE_URL: &str = "https://focus.toggl.com/api";
const REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(15);
const USER_AGENT: &str = concat!("tyoaikalaskuri/", env!("CARGO_PKG_VERSION"));
/// Toggl 2.0 API keys start with this prefix.
const FOCUS_KEY_PREFIX: &str = "toggl_sk_";
const MAX_ERROR_DETAIL: usize = 200;

/// Which Toggl product (and API) a token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Flavor {
    /// Toggl Track, API v9.
    #[default]
    Track,
    /// Toggl 2.0 (`focus.toggl.com`).
    Focus,
}

impl Flavor {
    pub fn detect(token: &str) -> Self {
        if token.starts_with(FOCUS_KEY_PREFIX) {
            Self::Focus
        } else {
            Self::Track
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TogglError {
    #[error("no API token configured")]
    MissingToken,
    #[error("API token was rejected (HTTP 401/403)")]
    Unauthorized,
    #[error("request quota exceeded (HTTP 402)")]
    QuotaExceeded,
    #[error("rate limited (HTTP 429)")]
    RateLimited,
    #[error("network error: {0}")]
    Network(#[source] reqwest::Error),
    #[error("unexpected HTTP status {status}: {}", detail.as_deref().unwrap_or("-"))]
    Status { status: u16, detail: Option<String> },
    #[error("invalid response from Toggl: {0}")]
    Decode(#[source] reqwest::Error),
    #[error("invalid request URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("the Toggl account has no accessible workspace")]
    NoWorkspace,
    #[error("not supported by this Toggl product: {0}")]
    Unsupported(&'static str),
}

impl TogglError {
    /// Whether the error means Toggl asked us to slow down.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::QuotaExceeded | Self::RateLimited)
    }
}

pub type Result<T> = std::result::Result<T, TogglError>;

/// A token together with the API it belongs to.
#[derive(Clone)]
pub(super) struct Credentials {
    pub token: String,
    pub flavor: Flavor,
}

impl Credentials {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            flavor: Flavor::detect(token),
        }
    }
}

/// Thread-safe Toggl API client. Cheap to share behind an `Arc`.
pub struct TogglClient {
    http: reqwest::Client,
    track_base: String,
    focus_base: String,
    credentials: RwLock<Option<Credentials>>,
    /// Toggl 2.0 organization and workspace, resolved once per token.
    pub(super) focus_context: Mutex<Option<FocusContext>>,
    budget: Mutex<RateBudget>,
    pub(super) clock: Arc<dyn Clock>,
}

impl TogglClient {
    /// A client for the production Toggl APIs.
    pub fn new(clock: Arc<dyn Clock>) -> Result<Self> {
        Self::with_base_urls(TRACK_BASE_URL, FOCUS_BASE_URL, clock)
    }

    /// A client with custom base URLs (used by tests).
    pub fn with_base_urls(
        track_base: impl Into<String>,
        focus_base: impl Into<String>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(TogglError::Network)?;
        Ok(Self {
            http,
            track_base: track_base.into().trim_end_matches('/').to_owned(),
            focus_base: focus_base.into().trim_end_matches('/').to_owned(),
            credentials: RwLock::new(None),
            focus_context: Mutex::new(None),
            budget: Mutex::new(RateBudget::default()),
            clock,
        })
    }

    /// Replaces the API token used for subsequent requests.
    pub fn set_token(&self, token: Option<String>) {
        *self.credentials.write().unwrap_or_else(|e| e.into_inner()) =
            token.as_deref().map(Credentials::new);
        *self.focus_context.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn has_token(&self) -> bool {
        self.credentials
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Requests made during the last hour (sliding window).
    pub fn requests_in_last_hour(&self) -> usize {
        let now = self.clock.now();
        self.budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .count(now)
    }

    /// Validates `token` and returns the account it belongs to. Used before
    /// a new token is stored.
    pub async fn verify_token(&self, token: &str) -> Result<Account> {
        let credentials = Credentials::new(token);
        match credentials.flavor {
            Flavor::Track => self.track_account(&credentials).await,
            Flavor::Focus => self.focus_account(&credentials).await,
        }
    }

    /// The account of the stored token.
    pub async fn account(&self) -> Result<Account> {
        self.verify_token(&self.credentials()?.token).await
    }

    /// Time entries overlapping `[start, end)`. Toggl Track filters by start
    /// time, so callers should start the range early enough to include
    /// entries that cross into it.
    pub async fn time_entries(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TimeEntry>> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => self.track_time_entries(&credentials, start, end).await,
            Flavor::Focus => self.focus_time_entries(&credentials, start, end).await,
        }
    }

    /// The most recent entries (roughly the last two weeks).
    pub async fn recent_time_entries(&self) -> Result<Vec<TimeEntry>> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => self.track_recent_time_entries(&credentials).await,
            Flavor::Focus => self.focus_recent_time_entries(&credentials).await,
        }
    }

    /// The running work entry, or `None` if no timer is running.
    pub async fn current_time_entry(&self) -> Result<Option<TimeEntry>> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => self.track_current_time_entry(&credentials).await,
            Flavor::Focus => self.focus_current_time_entry(&credentials).await,
        }
    }

    /// Starts a running time entry.
    pub async fn start_time_entry(&self, entry: &StartEntry) -> Result<TimeEntry> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => self.track_start(&credentials, entry).await,
            Flavor::Focus => self.focus_start(&credentials, entry).await,
        }
    }

    /// Stops the running entry. Succeeds if the timer was already stopped.
    pub async fn stop_time_entry(&self, workspace_id: i64, entry_id: i64) -> Result<()> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => self.track_stop(&credentials, workspace_id, entry_id).await,
            Flavor::Focus => self.focus_stop(&credentials, None).await,
        }
    }

    /// Stops the running entry with its end at `stop`, which may be in the
    /// future. `start` is the entry's start time.
    pub async fn stop_time_entry_at(
        &self,
        workspace_id: i64,
        entry_id: i64,
        start: DateTime<Utc>,
        stop: DateTime<Utc>,
    ) -> Result<()> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => {
                self.track_set_stop(&credentials, workspace_id, entry_id, start, stop)
                    .await
            }
            Flavor::Focus => self.focus_stop(&credentials, Some(stop)).await,
        }
    }

    /// Whether [`Self::move_entry_stop`] is available for the stored token.
    pub fn can_move_entry_stop(&self) -> bool {
        self.credentials()
            .is_ok_and(|credentials| credentials.flavor == Flavor::Track)
    }

    /// Changes the end of a finished entry. Only Toggl Track supports this;
    /// see [`Self::can_move_entry_stop`].
    pub async fn move_entry_stop(
        &self,
        workspace_id: i64,
        entry_id: i64,
        start: DateTime<Utc>,
        stop: DateTime<Utc>,
    ) -> Result<()> {
        let credentials = self.credentials()?;
        match credentials.flavor {
            Flavor::Track => {
                self.track_set_stop(&credentials, workspace_id, entry_id, start, stop)
                    .await
            }
            Flavor::Focus => Err(TogglError::Unsupported("editing a finished entry")),
        }
    }

    // --- HTTP core, shared by `track` and `focus` -------------------------

    fn credentials(&self) -> Result<Credentials> {
        self.credentials
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(TogglError::MissingToken)
    }

    pub(super) fn url(&self, flavor: Flavor, path: &str, query: &[(&str, String)]) -> Result<Url> {
        let base = match flavor {
            Flavor::Track => &self.track_base,
            Flavor::Focus => &self.focus_base,
        };
        Ok(Url::parse_with_params(&format!("{base}{path}"), query)?)
    }

    /// Sends a request, records it in the rate budget and maps error
    /// statuses. Successful responses (including 204) are returned as is.
    pub(super) async fn send<B: Serialize + ?Sized>(
        &self,
        credentials: &Credentials,
        method: Method,
        url: Url,
        body: Option<&B>,
    ) -> Result<Response> {
        let mut request = self.http.request(method, url);
        request = match credentials.flavor {
            Flavor::Track => request.basic_auth(&credentials.token, Some("api_token")),
            Flavor::Focus => request.bearer_auth(&credentials.token),
        };
        if let Some(body) = body {
            request = request.json(body);
        }

        self.budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(self.clock.now());

        let response = request.send().await.map_err(TogglError::Network)?;
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(TogglError::Unauthorized),
            StatusCode::PAYMENT_REQUIRED => Err(TogglError::QuotaExceeded),
            StatusCode::TOO_MANY_REQUESTS => Err(TogglError::RateLimited),
            status => Err(TogglError::Status {
                status: status.as_u16(),
                detail: error_detail(response).await,
            }),
        }
    }

    /// Sends a request and decodes a JSON body.
    pub(super) async fn json<B, T>(
        &self,
        credentials: &Credentials,
        method: Method,
        url: Url,
        body: Option<&B>,
    ) -> Result<T>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let response = self.send(credentials, method, url, body).await?;
        response.json::<T>().await.map_err(TogglError::Decode)
    }
}

/// Extracts a short error message from an error response body. Toggl 2.0
/// returns `{ "error_description": ... }`, Toggl Track plain text.
async fn error_detail(response: Response) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error_description: Option<String>,
        error: Option<String>,
    }
    let text = response.text().await.ok()?;
    let detail = serde_json::from_str::<ErrorBody>(&text)
        .ok()
        .and_then(|body| body.error_description.or(body.error))
        .unwrap_or(text);
    let detail = detail.trim().trim_matches('"');
    (!detail.is_empty()).then(|| detail.chars().take(MAX_ERROR_DETAIL).collect())
}

pub(super) fn rfc3339(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::clock::SystemClock;
    use wiremock::MockServer;

    /// A client whose both APIs point at `server`, with `token` set.
    pub(crate) fn client(server: &MockServer, token: &str, clock: Arc<dyn Clock>) -> TogglClient {
        let client =
            TogglClient::with_base_urls(server.uri(), server.uri(), clock).expect("client");
        client.set_token(Some(token.to_owned()));
        client
    }

    pub(crate) fn system_clock() -> Arc<dyn Clock> {
        Arc::new(SystemClock)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{client, system_clock};
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn detects_flavor_from_token() {
        assert_eq!(Flavor::detect("toggl_sk_abc"), Flavor::Focus);
        assert_eq!(
            Flavor::detect("1971800d4d82861d8f2c1651fea4d212"),
            Flavor::Track
        );
    }

    #[tokio::test]
    async fn maps_error_statuses() {
        for (status, check) in [
            (
                401,
                (|e| matches!(e, TogglError::Unauthorized)) as fn(&TogglError) -> bool,
            ),
            (402, |e| matches!(e, TogglError::QuotaExceeded)),
            (429, |e| matches!(e, TogglError::RateLimited)),
            (500, |e| matches!(e, TogglError::Status { status: 500, .. })),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            let client = client(&server, "token", system_clock());
            let err = client.current_time_entry().await.expect_err("should fail");
            assert!(check(&err), "status {status} mapped to {err:?}");
        }
    }

    #[tokio::test]
    async fn includes_error_description_from_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/organizations/1/workspaces/2/tracking/start"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "error": "required_fields_missing",
                "error_description": "Project is required"
            })))
            .mount(&server)
            .await;
        let client = client(&server, "toggl_sk_test", system_clock());
        *client.focus_context.lock().expect("lock") = Some(FocusContext {
            organization_id: 1,
            workspace_id: 2,
        });
        let entry = StartEntry {
            workspace_id: 2,
            project_id: None,
            task_id: None,
            description: None,
            tags: Default::default(),
            billable: false,
            start: Utc::now(),
        };
        let err = client.start_time_entry(&entry).await.expect_err("422");
        assert!(
            matches!(&err, TogglError::Status { status: 422, detail: Some(d) } if d == "Project is required"),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn fails_fast_without_token() {
        let server = MockServer::start().await;
        let client = TogglClient::with_base_urls(server.uri(), server.uri(), system_clock())
            .expect("client");
        let err = client.current_time_entry().await.expect_err("no token");
        assert!(matches!(err, TogglError::MissingToken));
        assert_eq!(client.requests_in_last_hour(), 0);
    }
}
