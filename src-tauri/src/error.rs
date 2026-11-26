//! Error type returned by Tauri commands.
//!
//! Errors are serialized to the UI as `{ code, message }`, where `message` is
//! a Finnish, user-facing text. The `Display` implementation (English) is
//! meant for logs.

use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::engine::ControlError;
use crate::settings::persistence::PersistenceError;
use crate::settings::secrets::SecretError;
use crate::settings::SettingsError;
use crate::toggl::TogglError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Toggl(#[from] TogglError),
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("autostart: {0}")]
    Autostart(String),
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error("the background engine is not running")]
    EngineStopped,
    #[error("API token must not be empty")]
    EmptyToken,
}

impl AppError {
    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Toggl(err) | Self::Control(ControlError::Toggl(err)) => toggl_code(err),
            Self::Control(ControlError::NotRunning) => "notRunning",
            Self::Control(ControlError::AlreadyRunning) => "alreadyRunning",
            Self::Settings(_) => "invalidSettings",
            Self::Persistence(_) => "storage",
            Self::Secret(_) => "keychain",
            Self::Autostart(_) => "autostart",
            Self::Clipboard(_) => "clipboard",
            Self::EmptyToken => "emptyToken",
            Self::Tauri(_) | Self::EngineStopped => "internal",
        }
    }

    /// Finnish message shown to the user.
    pub fn user_message(&self) -> String {
        match self {
            Self::Toggl(err) | Self::Control(ControlError::Toggl(err)) => toggl_message(err),
            Self::Control(ControlError::NotRunning) => "Ajastin ei ole käynnissä.".into(),
            Self::Control(ControlError::AlreadyRunning) => "Ajastin on jo käynnissä.".into(),
            Self::Settings(SettingsError::Target) => {
                "Päivän tavoitteen on oltava 0–24 tuntia puolen tunnin tarkkuudella.".into()
            }
            Self::Settings(SettingsError::TimeZone(name)) => {
                format!("Tuntematon aikavyöhyke: {name}")
            }
            Self::Settings(SettingsError::Opacity) => "Läpinäkyvyyden on oltava 60–100 %.".into(),
            Self::Persistence(_) => "Asetusten tallennus epäonnistui.".into(),
            Self::Secret(_) => "Avainnippuun (Keychain) ei päästy käsiksi.".into(),
            Self::Autostart(_) => "Automaattisen käynnistyksen asetus epäonnistui.".into(),
            Self::Clipboard(_) => "Leikepöydälle kopiointi epäonnistui.".into(),
            Self::EmptyToken => "API-token ei voi olla tyhjä.".into(),
            Self::Tauri(_) | Self::EngineStopped => "Sovelluksen sisäinen virhe.".into(),
        }
    }
}

fn toggl_code(err: &TogglError) -> &'static str {
    match err {
        TogglError::MissingToken => "noToken",
        TogglError::Unauthorized => "authError",
        TogglError::QuotaExceeded | TogglError::RateLimited => "rateLimited",
        TogglError::Network(_) | TogglError::Status { .. } => "offline",
        TogglError::NoWorkspace => "noWorkspace",
        TogglError::Unsupported(_) => "unsupported",
        TogglError::Decode(_) | TogglError::Url(_) => "invalidResponse",
    }
}

fn toggl_message(err: &TogglError) -> String {
    match err {
        TogglError::MissingToken => "Toggl API -tokenia ei ole asetettu.".into(),
        TogglError::Unauthorized => "Toggl hylkäsi API-tokenin. Tarkista token asetuksista.".into(),
        TogglError::QuotaExceeded | TogglError::RateLimited => {
            "Toggl-yhteys rajoitettu. Yritä hetken päästä uudelleen.".into()
        }
        TogglError::Network(_) => "Toggliin ei saada yhteyttä.".into(),
        TogglError::Status { status, detail } => match detail {
            Some(detail) => format!("Toggl palautti virheen (HTTP {status}): {detail}"),
            None => format!("Toggl palautti virheen (HTTP {status})."),
        },
        TogglError::NoWorkspace => "Toggl-tililläsi ei ole käytettävissä olevaa workspacea.".into(),
        TogglError::Unsupported(_) => "Toiminto ei ole käytettävissä tässä Toggl-versiossa.".into(),
        TogglError::Decode(_) | TogglError::Url(_) => {
            "Toggl palautti odottamattoman vastauksen.".into()
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.user_message())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_finnish_message() {
        let err = AppError::from(ControlError::Toggl(TogglError::RateLimited));
        let json = serde_json::to_value(&err).expect("json");
        assert_eq!(json["code"], "rateLimited");
        assert_eq!(
            json["message"],
            "Toggl-yhteys rajoitettu. Yritä hetken päästä uudelleen."
        );
    }
}
