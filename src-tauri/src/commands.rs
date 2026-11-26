//! Tauri commands invoked from the Angular UI. Kept thin: the logic lives in
//! [`AppService`].

use tauri::{AppHandle, State};

use crate::engine::{DaySummary, RefreshOutcome, TimerViewModel};
use crate::error::AppError;
use crate::service::{AppService, SettingsView};
use crate::settings::Settings;
use crate::toggl::Account;
use crate::windows;

type CommandResult<T> = Result<T, AppError>;

#[tauri::command]
pub fn get_state(service: State<'_, AppService>) -> TimerViewModel {
    service.view_model()
}

#[tauri::command]
pub async fn refresh_now(service: State<'_, AppService>) -> CommandResult<RefreshOutcome> {
    service.refresh().await
}

#[tauri::command]
pub async fn stop_timer(service: State<'_, AppService>) -> CommandResult<()> {
    service.stop_timer().await
}

#[tauri::command]
pub async fn resume_timer(service: State<'_, AppService>) -> CommandResult<()> {
    service.resume_timer().await
}

#[tauri::command]
pub fn get_settings(service: State<'_, AppService>) -> SettingsView {
    service.settings_view()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    service: State<'_, AppService>,
    settings: Settings,
) -> CommandResult<Settings> {
    // Window visibility is controlled by the tray and the window itself; a
    // settings form opened earlier may hold a stale value.
    let settings = Settings {
        timer_window_visible: service.settings().timer_window_visible,
        ..settings
    };
    service.save_settings(&app, settings)
}

#[tauri::command]
pub async fn set_api_token(
    service: State<'_, AppService>,
    token: String,
) -> CommandResult<Account> {
    service.set_api_token(token).await
}

#[tauri::command]
pub fn clear_api_token(service: State<'_, AppService>) -> CommandResult<()> {
    service.clear_api_token()
}

#[tauri::command]
pub async fn test_connection(service: State<'_, AppService>) -> CommandResult<Account> {
    service.test_connection().await
}

#[tauri::command]
pub fn toggle_window(app: AppHandle, service: State<'_, AppService>) -> CommandResult<()> {
    service.toggle_timer_window(&app)
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> CommandResult<()> {
    Ok(windows::open_settings_window(&app)?)
}

#[tauri::command]
pub fn list_timezones() -> Vec<&'static str> {
    chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect()
}

#[tauri::command]
pub fn get_day_summary(service: State<'_, AppService>) -> DaySummary {
    service.day_summary()
}

#[tauri::command]
pub fn is_summary_open(service: State<'_, AppService>) -> bool {
    service.is_summary_open()
}

#[tauri::command]
pub fn show_summary(app: AppHandle, service: State<'_, AppService>) -> CommandResult<()> {
    service.show_summary(&app)
}

#[tauri::command]
pub fn close_summary(app: AppHandle, service: State<'_, AppService>) -> CommandResult<()> {
    service.close_summary(&app)
}

#[tauri::command]
pub async fn finish_day(app: AppHandle, service: State<'_, AppService>) -> CommandResult<()> {
    service.finish_day(&app).await
}

#[tauri::command]
pub fn copy_to_clipboard(
    app: AppHandle,
    service: State<'_, AppService>,
    text: String,
) -> CommandResult<()> {
    service.copy_to_clipboard(&app, text)
}
