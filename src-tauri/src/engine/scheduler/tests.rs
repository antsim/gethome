//! Scheduler tests against a mocked Toggl API and a manual clock.

use super::*;
use crate::clock::testing::ManualClock;
use crate::engine::state::tests::entry;
use crate::toggl::{track, TimeEntry};
use chrono::TimeZone;
use chrono_tz::Europe::Helsinki;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ENTRIES: &str = "/me/time_entries";
const CURRENT: &str = "/me/time_entries/current";

struct Harness {
    server: MockServer,
    clock: Arc<ManualClock>,
    engine: SyncEngine,
}

impl Harness {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let start = Helsinki
            .with_ymd_and_hms(2026, 1, 5, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let clock = Arc::new(ManualClock::new(start));
        let client = crate::toggl::client::testing::client(&server, "token", clock.clone());
        let engine = SyncEngine::new(
            Arc::new(client),
            clock.clone(),
            PollConfig::default(),
            Helsinki,
            Some(1),
        );
        Self {
            server,
            clock,
            engine,
        }
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Advances the clock second by second, ticking like the real runner.
    async fn run_for(&mut self, duration: Duration) {
        for _ in 0..duration.num_seconds() {
            self.clock.advance(Duration::seconds(1));
            self.engine.tick().await;
        }
    }

    async fn requests_to(&self, p: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.method.as_str() == "GET" && r.url.path() == p)
            .count()
    }

    async fn total_requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .len()
    }

    fn snapshot(&self) -> EngineSnapshot {
        self.engine.subscribe().borrow().clone()
    }
}

/// A 200 response with entries in Toggl Track wire format.
fn json_entries(entries: &[TimeEntry]) -> ResponseTemplate {
    let body: Vec<_> = entries.iter().map(track::to_json).collect();
    ResponseTemplate::new(200).set_body_json(body)
}

/// A 200 response with one entry in Toggl Track wire format.
fn json_entry(entry: &TimeEntry) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(track::to_json(entry))
}

/// A running entry started at 08:00 local on the test day.
fn running_entry(id: i64) -> TimeEntry {
    let start = Helsinki
        .with_ymd_and_hms(2026, 1, 5, 8, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    entry(id, start, None)
}

async fn mount(server: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(server)
        .await;
}

#[tokio::test]
async fn first_tick_fetches_today_and_publishes_state() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(1)])).await;

    h.engine.tick().await;

    let snapshot = h.snapshot();
    let day = snapshot.day.expect("day loaded");
    assert_eq!(day.running.and_then(|r| r.id), Some(1));
    assert_eq!(snapshot.sync.warning, SyncWarning::None);
    assert_eq!(snapshot.sync.last_synced, Some(h.now()));
}

#[tokio::test]
async fn request_budget_stays_below_limit_in_normal_use() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(1)])).await;
    mount(&h.server, CURRENT, json_entry(&running_entry(1))).await;

    h.engine.tick().await;
    h.run_for(Duration::hours(1)).await;

    let total = h.total_requests().await;
    assert!(total <= 20, "made {total} requests in an hour");
    assert_eq!(h.requests_to(CURRENT).await, 14);
}

#[tokio::test]
async fn changed_running_entry_triggers_full_sync() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(1)])).await;
    // Toggl now reports a different running entry.
    mount(&h.server, CURRENT, json_entry(&running_entry(2))).await;

    h.engine.tick().await;
    h.run_for(Duration::minutes(4)).await;

    assert_eq!(h.requests_to(CURRENT).await, 1);
    assert_eq!(h.requests_to(ENTRIES).await, 2);
}

#[tokio::test]
async fn stopped_timer_in_toggl_is_detected_as_break() {
    let mut h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path(ENTRIES))
        .respond_with(json_entries(&[running_entry(1)]))
        .up_to_n_times(1)
        .mount(&h.server)
        .await;
    let stopped = entry(1, running_entry(1).start, Some(h.now()));
    mount(&h.server, ENTRIES, json_entries(&[stopped])).await;
    mount(
        &h.server,
        CURRENT,
        ResponseTemplate::new(200).set_body_string("null"),
    )
    .await;

    h.engine.tick().await;
    h.run_for(Duration::minutes(4)).await;

    let day = h.snapshot().day.expect("day");
    assert!(day.running.is_none());
    assert_eq!(day.completed_secs, 3600);
}

async fn assert_backoff_after_status(status: u16, expected: SyncWarning) {
    let mut h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path(ENTRIES))
        .respond_with(json_entries(&[running_entry(1)]))
        .up_to_n_times(1)
        .mount(&h.server)
        .await;
    mount(&h.server, CURRENT, ResponseTemplate::new(status)).await;

    h.engine.tick().await;
    let day_before = h.snapshot().day;
    h.run_for(Duration::minutes(4)).await;
    assert_eq!(h.requests_to(CURRENT).await, 1);
    assert_eq!(h.snapshot().sync.warning, expected);
    assert_eq!(h.snapshot().day, day_before, "last known state is kept");

    // No retry before the first backoff step (1 min) has passed.
    h.run_for(Duration::seconds(59)).await;
    assert_eq!(h.requests_to(CURRENT).await, 1);
    h.run_for(Duration::seconds(1)).await;
    assert_eq!(h.requests_to(CURRENT).await, 2);

    // The second failure waits two minutes.
    h.run_for(Duration::seconds(119)).await;
    assert_eq!(h.requests_to(CURRENT).await, 2);
    h.run_for(Duration::seconds(1)).await;
    assert_eq!(h.requests_to(CURRENT).await, 3);
}

#[tokio::test]
async fn quota_exceeded_backs_off() {
    assert_backoff_after_status(402, SyncWarning::RateLimited).await;
}

#[tokio::test]
async fn rate_limited_backs_off() {
    assert_backoff_after_status(429, SyncWarning::RateLimited).await;
}

#[tokio::test]
async fn server_error_backs_off_as_offline() {
    assert_backoff_after_status(503, SyncWarning::Offline).await;
}

#[tokio::test]
async fn unauthorized_pauses_polling_until_token_changes() {
    let mut h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path(ENTRIES))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&h.server)
        .await;
    mount(&h.server, ENTRIES, json_entries(&[])).await;

    h.engine.tick().await;
    assert_eq!(h.snapshot().sync.warning, SyncWarning::AuthError);
    h.run_for(Duration::minutes(30)).await;
    assert_eq!(h.total_requests().await, 1);

    h.engine.token_changed(Some(1));
    h.engine.tick().await;
    assert_eq!(h.total_requests().await, 2);
    assert_eq!(h.snapshot().sync.warning, SyncWarning::None);
}

#[tokio::test]
async fn wake_from_sleep_triggers_full_sync() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[])).await;

    h.engine.tick().await;
    h.clock
        .advance(Duration::minutes(30) - Duration::seconds(5));
    h.engine.tick().await;

    assert_eq!(h.requests_to(ENTRIES).await, 2);
}

#[tokio::test]
async fn day_change_triggers_full_sync() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[])).await;
    mount(
        &h.server,
        CURRENT,
        ResponseTemplate::new(200).set_body_string("null"),
    )
    .await;

    h.engine.tick().await;
    // Jump to 23:59:50 with regular ticks every few minutes, then cross midnight.
    h.clock
        .advance(Duration::hours(14) + Duration::minutes(59) + Duration::seconds(50));
    h.engine.tick().await;
    let before = h.requests_to(ENTRIES).await;
    h.run_for(Duration::seconds(15)).await;

    assert_eq!(h.requests_to(ENTRIES).await, before + 1);
    let date = h.snapshot().day.expect("day").date;
    assert_eq!(date, local_date(h.now(), Helsinki));
}

#[tokio::test]
async fn no_token_reports_warning_without_requests() {
    let mut h = Harness::new().await;
    h.engine.client.set_token(None);
    h.engine.tick().await;
    assert_eq!(h.snapshot().sync.warning, SyncWarning::NoToken);
    assert_eq!(h.total_requests().await, 0);
}

#[tokio::test]
async fn manual_refresh_is_throttled() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[])).await;
    h.engine.tick().await;

    assert_eq!(
        h.engine.refresh_now().await.expect("ok"),
        RefreshOutcome::Refreshed
    );
    h.clock.advance(Duration::seconds(10));
    assert_eq!(
        h.engine.refresh_now().await.expect("ok"),
        RefreshOutcome::Throttled
    );
    assert_eq!(h.requests_to(ENTRIES).await, 2);
}

#[tokio::test]
async fn stop_timer_updates_state_optimistically_and_verifies() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(7)])).await;
    Mock::given(method("PATCH"))
        .and(path("/workspaces/1/time_entries/7/stop"))
        .respond_with(json_entry(&entry(7, running_entry(7).start, Some(h.now()))))
        .expect(1)
        .mount(&h.server)
        .await;

    h.engine.tick().await;
    h.engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .expect("stopped");

    let day = h.snapshot().day.expect("day");
    assert!(day.running.is_none());
    assert_eq!(day.completed_secs, 3600);

    // The next tick verifies with a full fetch.
    h.run_for(Duration::seconds(1)).await;
    assert_eq!(h.requests_to(ENTRIES).await, 2);
}

#[tokio::test]
async fn failed_stop_restores_previous_state() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(7)])).await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&h.server)
        .await;

    h.engine.tick().await;
    let before = h.snapshot().day;
    assert!(h
        .engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .is_err());
    assert_eq!(h.snapshot().day, before);
    assert_eq!(h.snapshot().sync.warning, SyncWarning::Offline);
}

#[tokio::test]
async fn resume_continues_latest_entry_from_earlier_days() {
    let mut h = Harness::new().await;
    // Nothing today…
    mount(&h.server, ENTRIES, json_entries(&[])).await;
    h.engine.tick().await;

    // …so the template comes from recent entries (unfiltered request).
    h.server.reset().await;
    let last_week = entry(
        3,
        h.now() - Duration::days(3),
        Some(h.now() - Duration::days(3) + Duration::hours(1)),
    );
    Mock::given(method("GET"))
        .and(path(ENTRIES))
        .respond_with(json_entries(&[last_week]))
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/workspaces/1/time_entries"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "description": "entry 3",
            "project_id": 10,
            "duration": -1,
            "created_with": "tyoaikalaskuri",
        })))
        .respond_with(json_entry(&entry(4, h.now(), None)))
        .expect(1)
        .mount(&h.server)
        .await;

    h.engine.resume_timer().await.expect("resumed");

    let running = h.snapshot().day.and_then(|d| d.running).expect("running");
    assert_eq!(running.id, Some(4));
    assert!(matches!(
        h.engine.resume_timer().await,
        Err(ControlError::AlreadyRunning)
    ));
}

#[tokio::test]
async fn adopts_running_entry_that_started_before_fetched_range() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[])).await;
    // A timer left running for three days is not in today's list.
    let old = entry(5, h.now() - Duration::days(3), None);
    mount(&h.server, CURRENT, json_entry(&old)).await;

    h.engine.tick().await;
    h.run_for(Duration::minutes(8)).await;

    // One full sync after the first poll, none after the second.
    assert_eq!(h.requests_to(CURRENT).await, 2);
    assert_eq!(h.requests_to(ENTRIES).await, 2);
    let running = h.snapshot().day.and_then(|d| d.running).expect("running");
    assert_eq!(running.id, Some(5));
}

#[tokio::test]
async fn syncs_with_toggl_2_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/me/landing-workspace"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "organization_id": 10, "workspace_id": 20, "reason": "default"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let start = Helsinki
        .with_ymd_and_hms(2026, 1, 5, 8, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    Mock::given(method("GET"))
        .and(path("/organizations/10/workspaces/20/time-entries/stream"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1, "workspace_id": 20, "start": start, "duration": 3600, "type": "activity"},
            {"id": 2, "workspace_id": 20, "start": start + Duration::hours(1),
             "duration": 900, "type": "break"},
            {"id": 3, "workspace_id": 20, "start": start + Duration::minutes(75),
             "type": "activity"}
        ])))
        .mount(&server)
        .await;

    let clock = Arc::new(ManualClock::new(start + Duration::hours(2)));
    let client = crate::toggl::client::testing::client(&server, "toggl_sk_test", clock.clone());
    let mut engine = SyncEngine::new(
        Arc::new(client),
        clock,
        PollConfig::default(),
        Helsinki,
        None,
    );
    engine.tick().await;

    let day = engine.subscribe().borrow().day.clone().expect("day");
    assert_eq!(day.completed_secs, 3600, "the break does not count");
    assert_eq!(day.running.and_then(|r| r.id), Some(3));
}

/// Local time on the test day (Helsinki is UTC+2 in January).
fn local(h: u32, m: u32) -> DateTime<Utc> {
    Helsinki
        .with_ymd_and_hms(2026, 1, 5, h, m, 0)
        .unwrap()
        .with_timezone(&Utc)
}

#[tokio::test]
async fn pause_near_the_end_ends_the_entry_when_the_target_is_reached() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(7)])).await;
    // 7:30 target from 08:00 is full at 15:30 local = 13:30 UTC.
    Mock::given(method("PUT"))
        .and(path("/workspaces/1/time_entries/7"))
        .and(body_json(serde_json::json!({
            "stop": "2026-01-05T13:30:00Z",
            "duration": 27000
        })))
        .respond_with(json_entry(&entry(7, local(8, 0), Some(local(15, 30)))))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("PATCH"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&h.server)
        .await;

    h.engine.tick().await;
    h.clock.set(local(15, 20));
    h.engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .expect("stopped");

    let day = h.snapshot().day.expect("day");
    assert!(day.running.is_none());
    assert_eq!(day.completed_secs, 27000);
    assert_eq!(day.last_stop, Some(local(15, 30)));
}

#[tokio::test]
async fn pause_with_more_time_left_stops_now() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(7)])).await;
    Mock::given(method("PATCH"))
        .and(path("/workspaces/1/time_entries/7/stop"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&h.server)
        .await;

    h.engine.tick().await;
    h.clock.set(local(15, 15));
    h.engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .expect("stopped");
    assert_eq!(
        h.snapshot().day.and_then(|d| d.last_stop),
        Some(local(15, 15))
    );
}

#[tokio::test]
async fn resuming_before_the_scheduled_end_moves_it_back() {
    let mut h = Harness::new().await;
    mount(&h.server, ENTRIES, json_entries(&[running_entry(7)])).await;
    Mock::given(method("PUT"))
        .and(path("/workspaces/1/time_entries/7"))
        .and(body_json(serde_json::json!({
            "stop": "2026-01-05T13:30:00Z",
            "duration": 27000
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&h.server)
        .await;
    // Resumed at 15:25: the first entry now ends then.
    Mock::given(method("PUT"))
        .and(path("/workspaces/1/time_entries/7"))
        .and(body_json(serde_json::json!({
            "stop": "2026-01-05T13:25:00Z",
            "duration": 26700
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/workspaces/1/time_entries"))
        .respond_with(json_entry(&entry(8, local(15, 25), None)))
        .expect(1)
        .mount(&h.server)
        .await;

    h.engine.tick().await;
    h.clock.set(local(15, 20));
    h.engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .expect("stopped");
    h.clock.set(local(15, 25));
    h.engine.resume_timer().await.expect("resumed");

    let day = h.snapshot().day.expect("day");
    assert_eq!(day.completed_secs, 26700);
    assert_eq!(day.running.and_then(|r| r.id), Some(8));
}

#[tokio::test]
async fn toggl_2_pause_near_the_end_stops_now() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/me/landing-workspace"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "organization_id": 10, "workspace_id": 20, "reason": "default"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/organizations/10/workspaces/20/time-entries/stream"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 3, "workspace_id": 20, "start": local(8, 0), "type": "activity"}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/organizations/10/workspaces/20/tracking/stop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let clock = Arc::new(ManualClock::new(local(9, 0)));
    let client = crate::toggl::client::testing::client(&server, "toggl_sk_test", clock.clone());
    let mut engine = SyncEngine::new(
        Arc::new(client),
        clock.clone(),
        PollConfig::default(),
        Helsinki,
        None,
    );
    engine.tick().await;
    clock.set(local(15, 20));
    engine
        .stop_timer(&WeeklyTargets::default())
        .await
        .expect("stopped");

    let day = engine.subscribe().borrow().day.clone().expect("day");
    assert_eq!(day.last_stop, Some(local(15, 20)));
}
