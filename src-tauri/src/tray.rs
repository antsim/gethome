//! Menu bar (tray) icon: title formatting and the menu.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use crate::engine::{SyncWarning, TimerStatus, TimerViewModel};

pub const TRAY_ID: &str = "main";

/// Menu item identifiers.
pub mod menu_id {
    pub const TOGGLE_WINDOW: &str = "toggle-window";
    pub const TIMER_CONTROL: &str = "timer-control";
    pub const FINISH_DAY: &str = "finish-day";
    pub const SHOW_SUMMARY: &str = "show-summary";
    pub const REFRESH: &str = "refresh";
    pub const SETTINGS: &str = "settings";
    pub const QUIT: &str = "quit";
}

/// Text shown next to the tray icon, e.g. `⏱ 3:42:15` or `⏸ 3:42 Tauolla`.
pub fn format_title(vm: &TimerViewModel, show_seconds: bool) -> String {
    let title = match vm.status {
        TimerStatus::Working => format!("⏱ {}", format_duration(vm.remaining_secs, show_seconds)),
        TimerStatus::OnBreak => format!("⏸ {} Tauolla", format_duration(vm.remaining_secs, false)),
        TimerStatus::NotStarted => format!("⏱ {}", format_duration(vm.remaining_secs, false)),
        TimerStatus::Done => format!("✓ +{}", format_duration(-vm.remaining_secs, false)),
    };
    if vm.sync_warning == SyncWarning::None {
        title
    } else {
        format!("{title} ⚠")
    }
}

/// Formats seconds as `H:MM` or `H:MM:SS`. Negative values are shown as zero.
pub fn format_duration(secs: i64, show_seconds: bool) -> String {
    let secs = secs.max(0);
    let (hours, minutes, seconds) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if show_seconds {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}")
    }
}

/// Handles to menu items whose text changes with the app state.
pub struct TrayMenu {
    toggle_window: MenuItem<Wry>,
    timer_control: MenuItem<Wry>,
    /// Last applied `(running, window_visible)`, to avoid needless updates.
    applied: Mutex<Option<(bool, bool)>>,
}

impl TrayMenu {
    /// Updates item texts to match the current state.
    pub fn update(&self, running: bool, window_visible: bool) {
        let mut applied = self.applied.lock().unwrap_or_else(|e| e.into_inner());
        if *applied == Some((running, window_visible)) {
            return;
        }
        let window_text = if window_visible {
            "Piilota ikkuna"
        } else {
            "Näytä ikkuna"
        };
        let timer_text = if running {
            "Pysäytä ajastin"
        } else {
            "Jatka työtä"
        };
        if let Err(err) = self
            .toggle_window
            .set_text(window_text)
            .and_then(|()| self.timer_control.set_text(timer_text))
        {
            log::warn!("could not update tray menu: {err}");
            return;
        }
        *applied = Some((running, window_visible));
    }
}

/// Creates the tray icon and registers its menu as app state.
pub fn create(
    app: &AppHandle,
    on_menu_event: impl Fn(&AppHandle, MenuEvent) + Send + Sync + 'static,
) -> tauri::Result<TrayIcon> {
    let toggle_window = MenuItem::with_id(
        app,
        menu_id::TOGGLE_WINDOW,
        "Näytä ikkuna",
        true,
        None::<&str>,
    )?;
    let timer_control = MenuItem::with_id(
        app,
        menu_id::TIMER_CONTROL,
        "Jatka työtä",
        true,
        None::<&str>,
    )?;
    let finish_day = MenuItem::with_id(
        app,
        menu_id::FINISH_DAY,
        "Lopeta työpäivä",
        true,
        None::<&str>,
    )?;
    let show_summary = MenuItem::with_id(
        app,
        menu_id::SHOW_SUMMARY,
        "Päivän kirjaukset…",
        true,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(app, menu_id::REFRESH, "Päivitä nyt", true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        menu_id::SETTINGS,
        "Asetukset…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let quit = MenuItem::with_id(app, menu_id::QUIT, "Lopeta", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle_window,
            &timer_control,
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &finish_day,
            &show_summary,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .title("⏱ –:––")
        .tooltip("Työaikalaskuri")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    app.manage(TrayMenu {
        toggle_window,
        timer_control,
        applied: Mutex::new(None),
    });
    Ok(tray)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(status: TimerStatus, remaining_secs: i64) -> TimerViewModel {
        TimerViewModel {
            status,
            remaining_secs,
            worked_secs: 0,
            target_secs: 27_000,
            break_secs: 0,
            eta: None,
            running: status == TimerStatus::Working,
            last_synced: None,
            sync_warning: SyncWarning::None,
        }
    }

    const REMAINING: i64 = 3 * 3600 + 42 * 60 + 15;

    #[test]
    fn formats_working_with_and_without_seconds() {
        assert_eq!(
            format_title(&vm(TimerStatus::Working, REMAINING), true),
            "⏱ 3:42:15"
        );
        assert_eq!(
            format_title(&vm(TimerStatus::Working, REMAINING), false),
            "⏱ 3:42"
        );
    }

    #[test]
    fn formats_break_not_started_and_overtime() {
        assert_eq!(
            format_title(&vm(TimerStatus::OnBreak, REMAINING), true),
            "⏸ 3:42 Tauolla"
        );
        assert_eq!(
            format_title(&vm(TimerStatus::NotStarted, 27_000), true),
            "⏱ 7:30"
        );
        assert_eq!(
            format_title(&vm(TimerStatus::Done, -12 * 60), true),
            "✓ +0:12"
        );
    }

    #[test]
    fn appends_warning_marker() {
        let mut model = vm(TimerStatus::Working, REMAINING);
        model.sync_warning = SyncWarning::Offline;
        assert_eq!(format_title(&model, false), "⏱ 3:42 ⚠");
    }

    #[test]
    fn negative_duration_formats_as_zero() {
        assert_eq!(format_duration(-5, true), "0:00:00");
    }
}
