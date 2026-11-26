// SPDX-License-Identifier: MPL-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::models::AppSettings;

/// Resolves the platform-specific configuration file path for Kotodama settings.
pub fn default_settings_path() -> Result<PathBuf> {
    let config_root = dirs::config_dir()
        .context("Could not determine the system configuration directory for Kotodama settings")?;
    Ok(config_root.join("kotodama").join("settings.json"))
}

/// Loads persisted [`AppSettings`] from `path`, returning defaults when no file exists yet.
pub fn load_settings(path: &Path) -> Result<AppSettings> {
    let contents = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(AppSettings::default()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to read settings file at {}", path.display())
            });
        }
    };

    serde_json::from_str::<AppSettings>(&contents).with_context(|| {
        format!(
            "Failed to parse settings JSON at {}",
            path.display()
        )
    })
}

/// Persists [`AppSettings`] as pretty-printed JSON at `path`, creating parent directories as needed.
pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create settings directory at {}",
                parent.display()
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(settings)
        .context("Failed to serialize Kotodama settings to JSON")?;

    fs::write(path, serialized).with_context(|| {
        format!("Failed to write settings file at {}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::models::ThemePreference;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn unique_temp_dir(label: &str) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "kotodama_test_{label}_{}_{}",
            std::process::id(),
            id
        ))
    }

    #[test]
    fn load_settings_returns_default_when_file_missing() -> Result<()> {
        let dir = unique_temp_dir("missing");
        let path = dir.join("settings.json");
        let loaded = load_settings(&path)?;
        assert_eq!(loaded, AppSettings::default());
        assert_eq!(loaded.theme_preference, ThemePreference::System);
        Ok(())
    }

    #[test]
    fn save_and_load_settings_round_trips_theme_preference() -> Result<()> {
        let dir = unique_temp_dir("roundtrip");
        let path = dir.join("nested").join("settings.json");
        let settings = AppSettings {
            theme_preference: ThemePreference::Dark,
            blind_mode: true,
            ..AppSettings::default()
        };

        save_settings(&path, &settings)?;
        let loaded = load_settings(&path)?;
        assert_eq!(loaded, settings);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn load_settings_returns_error_on_corrupt_json() -> Result<()> {
        let dir = unique_temp_dir("corrupt");
        fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");
        fs::write(&path, "{ not valid json")?;

        let result = load_settings(&path);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }
}
