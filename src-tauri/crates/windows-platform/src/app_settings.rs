//! App-wide preferences that are not owned by one feature: display language and
//! the opt-in update check. Stored beside the other app data as
//! `app-settings.json`, written atomically.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use crate::cleanup::{read_json, write_json};
use crate::i18n::{self, LanguagePreference};

pub const APP_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub schema_version: u32,
    pub language: LanguagePreference,
    /// Off by default: nothing is contacted until the user turns it on.
    pub update_check: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: APP_SETTINGS_SCHEMA_VERSION,
            language: LanguagePreference::System,
            update_check: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppSettingsError {
    Invalid,
    Io,
}

pub struct AppSettingsService {
    path: PathBuf,
    current: Mutex<AppSettings>,
}

impl AppSettingsService {
    /// Loads saved settings. A missing, unreadable or invalid file falls back
    /// to defaults (every opt-in off) instead of blocking startup.
    pub fn new(app_data: &Path) -> Self {
        let path = app_data.join("app-settings.json");
        let current = read_json::<AppSettings>(&path)
            .ok()
            .filter(|settings| settings.schema_version == APP_SETTINGS_SCHEMA_VERSION)
            .unwrap_or_default();
        i18n::set_preference(current.language);
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> AppSettings {
        self.current.lock().map(|guard| *guard).unwrap_or_default()
    }

    pub fn set(&self, settings: AppSettings) -> Result<AppSettings, AppSettingsError> {
        if settings.schema_version != APP_SETTINGS_SCHEMA_VERSION {
            return Err(AppSettingsError::Invalid);
        }
        let mut current = self.current.lock().map_err(|_| AppSettingsError::Io)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| AppSettingsError::Io)?;
        }
        write_json(&self.path, &settings, true).map_err(|_| AppSettingsError::Io)?;
        *current = settings;
        i18n::set_preference(settings.language);
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "sdk-app-settings-{name}-{}",
            nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_keep_every_opt_in_off() {
        let dir = temp_dir("defaults");
        let service = AppSettingsService::new(&dir);
        assert_eq!(service.get(), AppSettings::default());
        assert!(!service.get().update_check);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saved_settings_survive_a_restart() {
        let dir = temp_dir("roundtrip");
        let saved = AppSettings {
            language: LanguagePreference::Es419,
            update_check: true,
            ..AppSettings::default()
        };
        AppSettingsService::new(&dir).set(saved).unwrap();
        assert_eq!(AppSettingsService::new(&dir).get(), saved);
        let raw = std::fs::read_to_string(dir.join("app-settings.json")).unwrap();
        assert!(raw.contains("\"language\":\"es-419\""));
        assert!(raw.contains("\"updateCheck\":true"));
        AppSettingsService::new(&dir)
            .set(AppSettings::default())
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_files_and_versions_fall_back_or_are_rejected() {
        let dir = temp_dir("invalid");
        std::fs::write(
            dir.join("app-settings.json"),
            b"{\"schemaVersion\":1,\"language\":\"fr\",\"updateCheck\":true}",
        )
        .unwrap();
        assert_eq!(AppSettingsService::new(&dir).get(), AppSettings::default());
        let service = AppSettingsService::new(&dir);
        let wrong = AppSettings {
            schema_version: 2,
            ..AppSettings::default()
        };
        assert_eq!(service.set(wrong), Err(AppSettingsError::Invalid));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
