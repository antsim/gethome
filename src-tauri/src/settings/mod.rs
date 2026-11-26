//! User settings: the model, persistence (`tauri-plugin-store`) and the API
//! token (macOS Keychain via `keyring`).

pub mod persistence;
pub mod secrets;

use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// Default daily target: 7 h 30 min.
pub const DEFAULT_TARGET_MINUTES: u32 = 7 * 60 + 30;
const MAX_TARGET_MINUTES: u32 = 24 * 60;
/// Targets are whole or half hours, because time is booked in half hours.
pub const TARGET_STEP_MINUTES: u32 = 30;
const FALLBACK_TIME_ZONE: Tz = chrono_tz::Europe::Helsinki;
pub const MIN_OPACITY: u8 = 60;
pub const MAX_OPACITY: u8 = 100;

/// Daily working-time targets in minutes, Monday first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WeeklyTargets(pub [u32; 7]);

impl Default for WeeklyTargets {
    fn default() -> Self {
        let d = DEFAULT_TARGET_MINUTES;
        Self([d, d, d, d, d, 0, 0])
    }
}

impl WeeklyTargets {
    /// Target in seconds for the given date.
    pub fn secs_for(&self, date: NaiveDate) -> i64 {
        let index = date.weekday().num_days_from_monday() as usize;
        i64::from(self.0[index]) * 60
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Minutes per weekday, Monday first.
    pub daily_targets: WeeklyTargets,
    /// IANA time zone name. `None` follows the system time zone.
    pub timezone: Option<String>,
    pub show_seconds: bool,
    pub autostart: bool,
    /// Floating window opacity in percent (60–100).
    pub window_opacity: u8,
    /// Whether the floating window is shown. Persisted across restarts.
    pub timer_window_visible: bool,
    /// Open the day summary automatically when the target is reached.
    pub auto_show_summary: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            daily_targets: WeeklyTargets::default(),
            timezone: None,
            show_seconds: true,
            autostart: false,
            window_opacity: MAX_OPACITY,
            timer_window_visible: true,
            auto_show_summary: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("daily target must be 0–24 hours in steps of {TARGET_STEP_MINUTES} minutes")]
    Target,
    #[error("unknown time zone: {0}")]
    TimeZone(String),
    #[error("opacity must be between {MIN_OPACITY} and {MAX_OPACITY} percent")]
    Opacity,
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self
            .daily_targets
            .0
            .iter()
            .any(|m| *m > MAX_TARGET_MINUTES || m % TARGET_STEP_MINUTES != 0)
        {
            return Err(SettingsError::Target);
        }
        if let Some(name) = &self.timezone {
            name.parse::<Tz>()
                .map_err(|_| SettingsError::TimeZone(name.clone()))?;
        }
        if !(MIN_OPACITY..=MAX_OPACITY).contains(&self.window_opacity) {
            return Err(SettingsError::Opacity);
        }
        Ok(())
    }

    /// Returns a copy with out-of-range values replaced by defaults. Used for
    /// settings loaded from disk, which may have been edited by hand.
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if self.daily_targets.0.iter().any(|m| *m > MAX_TARGET_MINUTES) {
            self.daily_targets = defaults.daily_targets;
        }
        // Older versions allowed any minute; round to the nearest half hour.
        for minutes in &mut self.daily_targets.0 {
            let step = TARGET_STEP_MINUTES;
            *minutes = ((*minutes + step / 2) / step * step).min(MAX_TARGET_MINUTES);
        }
        if self
            .timezone
            .as_deref()
            .is_some_and(|name| name.parse::<Tz>().is_err())
        {
            self.timezone = None;
        }
        self.window_opacity = self.window_opacity.clamp(MIN_OPACITY, MAX_OPACITY);
        self
    }

    /// The effective time zone: the configured one, else the system's.
    pub fn time_zone(&self) -> Tz {
        self.timezone
            .as_deref()
            .and_then(|name| name.parse().ok())
            .unwrap_or_else(system_time_zone)
    }
}

/// The operating system's time zone, falling back to `Europe/Helsinki`.
pub fn system_time_zone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(FALLBACK_TIME_ZONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_target_by_weekday() {
        let mut targets = WeeklyTargets::default();
        targets.0[4] = 6 * 60; // Friday
        let monday = NaiveDate::from_ymd_opt(2026, 1, 5).expect("date");
        let friday = NaiveDate::from_ymd_opt(2026, 1, 9).expect("date");
        let sunday = NaiveDate::from_ymd_opt(2026, 1, 11).expect("date");
        assert_eq!(targets.secs_for(monday), 450 * 60);
        assert_eq!(targets.secs_for(friday), 360 * 60);
        assert_eq!(targets.secs_for(sunday), 0);
    }

    #[test]
    fn validates_settings() {
        assert!(Settings::default().validate().is_ok());
        let bad_tz = Settings {
            timezone: Some("Mars/Base".into()),
            ..Settings::default()
        };
        assert!(matches!(bad_tz.validate(), Err(SettingsError::TimeZone(_))));
        let bad_opacity = Settings {
            window_opacity: 10,
            ..Settings::default()
        };
        assert_eq!(bad_opacity.validate(), Err(SettingsError::Opacity));
        let mut targets = WeeklyTargets::default();
        targets.0[0] = 7 * 60 + 45;
        let bad_target = Settings {
            daily_targets: targets,
            ..Settings::default()
        };
        assert_eq!(bad_target.validate(), Err(SettingsError::Target));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"showSeconds": false}"#).expect("json");
        assert!(!settings.show_seconds);
        assert_eq!(settings.daily_targets, WeeklyTargets::default());
    }

    #[test]
    fn sanitizes_values_loaded_from_disk() {
        let mut targets = WeeklyTargets::default();
        targets.0[0] = 7 * 60 + 45;
        targets.0[1] = 7 * 60 + 10;
        let loaded = Settings {
            timezone: Some("Nope".into()),
            window_opacity: 5,
            daily_targets: targets,
            ..Settings::default()
        };
        let clean = loaded.sanitized();
        assert_eq!(clean.timezone, None);
        assert_eq!(clean.window_opacity, MIN_OPACITY);
        assert_eq!(clean.daily_targets.0[0], 8 * 60);
        assert_eq!(clean.daily_targets.0[1], 7 * 60);
        assert!(clean.validate().is_ok());
    }
}
