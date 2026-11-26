//! Työaikalaskuri – a menu bar app that shows how much of today's working
//! time is left, based on Toggl Track time entries.
//!
//! See `PLAN.md` for the design and `CLAUDE.md` for conventions.

mod clock;
mod commands;
mod engine;
mod error;
mod service;
mod settings;
mod toggl;
mod tray;
mod windows;

use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_window_state::StateFlags;

use crate::service::AppService;

/// Builds and runs the application. Returns an error if startup fails.
pub fn run() -> Result<(), tauri::Error> {
    let app = tauri::Builder::default()
        // Must be registered first: a second launch just shows the window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_timer_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                // Visibility is persisted in our own settings.
                .with_state_flags(StateFlags::POSITION | StateFlags::SIZE)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            let service = AppService::start(handle)?;
            service.reconcile_autostart(handle);
            let visible = service.settings().timer_window_visible;
            app.manage(service);

            tray::create(handle, handle_menu_event)?;
            windows::set_timer_window_visible(handle, visible)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the floating window (e.g. Cmd+W) only hides it.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == windows::TIMER_WINDOW {
                    api.prevent_close();
                    set_timer_window_visible(window.app_handle(), false);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::refresh_now,
            commands::stop_timer,
            commands::resume_timer,
            commands::get_settings,
            commands::save_settings,
            commands::set_api_token,
            commands::clear_api_token,
            commands::test_connection,
            commands::toggle_window,
            commands::open_settings,
            commands::list_timezones,
            commands::get_day_summary,
            commands::is_summary_open,
            commands::show_summary,
            commands::close_summary,
            commands::finish_day,
            commands::copy_to_clipboard,
        ])
        .build(tauri::generate_context!())?;

    app.run(|_app, event| {
        // A tray app keeps running when its last window closes; only an
        // explicit exit (the "Lopeta" menu item) has an exit code.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(service) = app.try_state::<AppService>() else {
        return;
    };
    match event.id().as_ref() {
        tray::menu_id::TOGGLE_WINDOW => {
            if let Err(err) = service.toggle_timer_window(app) {
                log::error!("could not toggle window: {err}");
            }
        }
        tray::menu_id::TIMER_CONTROL => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let service = app.state::<AppService>();
                if let Err(err) = service.toggle_timer().await {
                    log::error!("could not toggle timer: {err}");
                }
            });
        }
        tray::menu_id::FINISH_DAY => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = app.state::<AppService>().finish_day(&app).await {
                    log::error!("could not finish the day: {err}");
                }
            });
        }
        tray::menu_id::SHOW_SUMMARY => {
            if let Err(err) = service.show_summary(app) {
                log::error!("could not show the day summary: {err}");
            }
        }
        tray::menu_id::REFRESH => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = app.state::<AppService>().refresh().await {
                    log::warn!("manual refresh failed: {err}");
                }
            });
        }
        tray::menu_id::SETTINGS => {
            if let Err(err) = windows::open_settings_window(app) {
                log::error!("could not open settings: {err}");
            }
        }
        tray::menu_id::QUIT => {
            // Restore the compact size first so that window-state saves it.
            if let Err(err) = service.close_summary(app) {
                log::warn!("could not close the day summary: {err}");
            }
            app.exit(0);
        }
        other => log::debug!("unhandled menu item {other}"),
    }
}

fn show_timer_window(app: &AppHandle) {
    set_timer_window_visible(app, true);
}

fn set_timer_window_visible(app: &AppHandle, visible: bool) {
    let result = match app.try_state::<AppService>() {
        Some(service) => service.set_timer_window_visible(app, visible),
        None => windows::set_timer_window_visible(app, visible).map_err(Into::into),
    };
    if let Err(err) = result {
        log::error!("could not change window visibility: {err}");
    }
}
