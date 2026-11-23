//! Application service: wires the engine, settings, token storage and the
//! Tauri app together. Registered as Tauri state and used by commands and
//! the tray menu.
//!
//! Two background tasks run for the lifetime of the app:
//! - the **engine loop** ticks [`SyncEngine`] every second and executes
//!   commands sent to it (stop/resume/refresh), so that all Toggl state
//!   changes are serialized;
//! - the **UI loop** computes the [`TimerViewModel`] every second (and
//!   immediately on any state change), emits it to the webviews and updates
//!   the tray title. It never waits for network requests.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono_tz::Tz;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::MissedTickBehavior;

use crate::clock::{Clock, SystemClock};
use crate::engine::{
    compute, ControlError, EngineSnapshot, PollConfig, RefreshOutcome, SyncEngine, TimerViewModel,
};
use crate::error::AppError;
use crate::settings::persistence::SettingsRepository;
use crate::settings::secrets::{KeychainStore, SecretStore};
use crate::settings::{system_time_zone, Settings, WeeklyTargets};
use crate::toggl::{Account, TogglClient, TogglError};
use crate::{tray, windows};

/// Event carrying a [`TimerViewModel`], emitted every second.
pub const TIMER_STATE_EVENT: &str = "timer-state";
/// Event carrying the new [`Settings`] after they change.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

const TICK: Duration = Duration::from_secs(1);
const COMMAND_QUEUE: usize = 16;

type Reply<T> = oneshot::Sender<T>;

enum EngineCommand {
    Refresh(Reply<Result<RefreshOutcome, TogglError>>),
    Stop(WeeklyTargets, Reply<Result<(), ControlError>>),
    Resume(Reply<Result<(), ControlError>>),
    TokenChanged(Option<i64>),
    TimeZoneChanged(Tz),
}

/// Settings plus account details, as shown in the settings window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub account: Option<Account>,
    pub has_token: bool,
    pub system_timezone: String,
}

pub struct AppService {
    clock: Arc<dyn Clock>,
    client: Arc<TogglClient>,
    commands: mpsc::Sender<EngineCommand>,
    snapshot: watch::Receiver<EngineSnapshot>,
    settings: watch::Sender<Settings>,
    repository: SettingsRepository,
    secrets: Box<dyn SecretStore>,
    /// Serializes settings writes (window toggles and the settings window).
    settings_write: Mutex<()>,
}

impl AppService {
    /// Loads settings and the token and starts the background tasks.
    pub fn start(app: &AppHandle) -> Result<Self, AppError> {
        let repository = SettingsRepository::open(app)?;
        let settings = repository.load_settings();
        let account = repository.load_account();
        let secrets: Box<dyn SecretStore> =
            Box::new(KeychainStore::new(app.config().identifier.clone()));

        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let client = Arc::new(TogglClient::new(clock.clone())?);
        match secrets.load_token() {
            Ok(token) => client.set_token(token),
            Err(err) => log::error!("could not read API token: {err}"),
        }

        let engine = SyncEngine::new(
            client.clone(),
            clock.clone(),
            PollConfig::default(),
            settings.time_zone(),
            account.map(|a| a.workspace_id),
        );
        let snapshot = engine.subscribe();
        let (settings_tx, settings_rx) = watch::channel(settings);
        let (commands, command_rx) = mpsc::channel(COMMAND_QUEUE);

        tauri::async_runtime::spawn(run_engine(engine, command_rx));
        tauri::async_runtime::spawn(run_ui(
            app.clone(),
            clock.clone(),
            snapshot.clone(),
            settings_rx,
        ));

        Ok(Self {
            clock,
            client,
            commands,
            snapshot,
            settings: settings_tx,
            repository,
            secrets,
            settings_write: Mutex::new(()),
        })
    }

    pub fn view_model(&self) -> TimerViewModel {
        view_model(
            &self.snapshot.borrow(),
            &self.settings.borrow(),
            self.clock.as_ref(),
        )
    }

    pub fn settings(&self) -> Settings {
        self.settings.borrow().clone()
    }

    pub fn settings_view(&self) -> SettingsView {
        SettingsView {
            settings: self.settings(),
            account: self.repository.load_account(),
            has_token: self.client.has_token(),
            system_timezone: system_time_zone().name().to_owned(),
        }
    }

    /// Validates, persists and applies new settings.
    pub fn save_settings(&self, app: &AppHandle, new: Settings) -> Result<Settings, AppError> {
        new.validate()?;
        let _guard = self
            .settings_write
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let old = self.settings();
        if new.autostart != old.autostart {
            apply_autostart(app, new.autostart)?;
        }
        if new.timer_window_visible != old.timer_window_visible {
            windows::set_timer_window_visible(app, new.timer_window_visible)?;
        }
        self.repository.save_settings(&new)?;
        if new.time_zone() != old.time_zone() {
            self.send(EngineCommand::TimeZoneChanged(new.time_zone()));
        }
        self.settings.send_replace(new.clone());
        app.emit(SETTINGS_CHANGED_EVENT, &new)?;
        Ok(new)
    }

    pub fn set_timer_window_visible(&self, app: &AppHandle, visible: bool) -> Result<(), AppError> {
        let settings = Settings {
            timer_window_visible: visible,
            ..self.settings()
        };
        self.save_settings(app, settings).map(drop)
    }

    pub fn toggle_timer_window(&self, app: &AppHandle) -> Result<(), AppError> {
        self.set_timer_window_visible(app, !windows::is_timer_window_visible(app))
    }

    /// Makes sure the OS login item matches the saved setting.
    pub fn reconcile_autostart(&self, app: &AppHandle) {
        let wanted = self.settings().autostart;
        let actual = app.autolaunch().is_enabled().unwrap_or(!wanted);
        if wanted != actual {
            if let Err(err) = apply_autostart(app, wanted) {
                log::warn!("could not reconcile autostart: {err}");
            }
        }
    }

    /// Validates the token against Toggl, then stores it in the Keychain.
    pub async fn set_api_token(&self, token: String) -> Result<Account, AppError> {
        let token = token.trim().to_owned();
        if token.is_empty() {
            return Err(AppError::EmptyToken);
        }
        let account = self.client.verify_token(&token).await?;
        self.secrets.save_token(&token)?;
        self.repository.save_account(Some(&account))?;
        self.client.set_token(Some(token));
        self.send(EngineCommand::TokenChanged(Some(account.workspace_id)));
        Ok(account)
    }

    /// Removes the token from the Keychain.
    pub fn clear_api_token(&self) -> Result<(), AppError> {
        self.secrets.delete_token()?;
        self.repository.save_account(None)?;
        self.client.set_token(None);
        self.send(EngineCommand::TokenChanged(None));
        Ok(())
    }

    /// Checks the stored token against Toggl and refreshes the cached account.
    pub async fn test_connection(&self) -> Result<Account, AppError> {
        let account = self.client.account().await?;
        self.repository.save_account(Some(&account))?;
        Ok(account)
    }

    pub async fn refresh(&self) -> Result<RefreshOutcome, AppError> {
        Ok(self.request(EngineCommand::Refresh).await??)
    }

    pub async fn stop_timer(&self) -> Result<(), AppError> {
        let targets = self.settings.borrow().daily_targets;
        Ok(self
            .request(|reply| EngineCommand::Stop(targets, reply))
            .await??)
    }

    pub async fn resume_timer(&self) -> Result<(), AppError> {
        Ok(self.request(EngineCommand::Resume).await??)
    }

    /// Stops the timer if it runs, otherwise resumes work.
    pub async fn toggle_timer(&self) -> Result<(), AppError> {
        if self.view_model().running {
            self.stop_timer().await
        } else {
            self.resume_timer().await
        }
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(Reply<T>) -> EngineCommand,
    ) -> Result<T, AppError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| AppError::EngineStopped)?;
        response.await.map_err(|_| AppError::EngineStopped)
    }

    fn send(&self, command: EngineCommand) {
        if self.commands.try_send(command).is_err() {
            log::error!("engine command queue is full or closed");
        }
    }
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), AppError> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|err| AppError::Autostart(err.to_string()))
}

fn view_model(snapshot: &EngineSnapshot, settings: &Settings, clock: &dyn Clock) -> TimerViewModel {
    compute(
        snapshot.day.as_ref(),
        &snapshot.sync,
        clock.now(),
        settings.time_zone(),
        &settings.daily_targets,
    )
}

async fn run_engine(mut engine: SyncEngine, mut commands: mpsc::Receiver<EngineCommand>) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = ticker.tick() => engine.tick().await,
            command = commands.recv() => match command {
                None => break,
                Some(EngineCommand::Refresh(reply)) => {
                    let _ = reply.send(engine.refresh_now().await);
                }
                Some(EngineCommand::Stop(targets, reply)) => {
                    let _ = reply.send(engine.stop_timer(&targets).await);
                }
                Some(EngineCommand::Resume(reply)) => {
                    let _ = reply.send(engine.resume_timer().await);
                }
                Some(EngineCommand::TokenChanged(workspace_id)) => {
                    engine.token_changed(workspace_id);
                    engine.tick().await;
                }
                Some(EngineCommand::TimeZoneChanged(tz)) => engine.set_time_zone(tz),
            },
        }
    }
}

async fn run_ui(
    app: AppHandle,
    clock: Arc<dyn Clock>,
    mut snapshot: watch::Receiver<EngineSnapshot>,
    mut settings: watch::Receiver<Settings>,
) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            changed = snapshot.changed() => if changed.is_err() { break },
            changed = settings.changed() => if changed.is_err() { break },
        }
        let settings = settings.borrow_and_update().clone();
        let vm = view_model(&snapshot.borrow_and_update(), &settings, clock.as_ref());

        if let Err(err) = app.emit(TIMER_STATE_EVENT, &vm) {
            log::warn!("could not emit timer state: {err}");
        }
        if let Some(tray_icon) = app.tray_by_id(tray::TRAY_ID) {
            let title = tray::format_title(&vm, settings.show_seconds);
            if let Err(err) = tray_icon.set_title(Some(title)) {
                log::warn!("could not update tray title: {err}");
            }
        }
        if let Some(menu) = app.try_state::<tray::TrayMenu>() {
            menu.update(vm.running, windows::is_timer_window_visible(&app));
        }
    }
}
