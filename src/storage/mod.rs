// SPDX-License-Identifier: MPL-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::models::{AppSettings, DeckWorkspaceConfig};

/// Resolves the platform-specific configuration file path for Kotodama settings.
pub fn default_settings_path() -> Result<PathBuf> {
    Ok(kotodama_config_dir("settings")?.join("settings.json"))
}

/// Resolves the platform-specific configuration file path for Kotodama deck workspace configs.
pub fn default_deck_config_path() -> Result<PathBuf> {
    Ok(kotodama_config_dir("deck configuration")?.join("deck_config.json"))
}

/// Loads persisted [`AppSettings`] from `path`, returning defaults when no file exists yet.
pub fn load_settings(path: &Path) -> Result<AppSettings> {
    load_json_or_default(path, "settings")
}

/// Persists [`AppSettings`] as pretty-printed JSON at `path`, creating parent directories as needed.
pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<()> {
    save_json_pretty(path, settings, "settings")
}

/// Loads persisted [`DeckWorkspaceConfig`] from `path`, returning an empty workspace when missing.
pub fn load_deck_workspace(path: &Path) -> Result<DeckWorkspaceConfig> {
    load_json_or_default(path, "deck workspace")
}

/// Persists [`DeckWorkspaceConfig`] as pretty-printed JSON at `path`, creating parent directories as needed.
pub fn save_deck_workspace(path: &Path, workspace: &DeckWorkspaceConfig) -> Result<()> {
    save_json_pretty(path, workspace, "deck workspace")
}

fn kotodama_config_dir(purpose: &str) -> Result<PathBuf> {
    let config_root = dirs::config_dir().with_context(|| {
        format!("Could not determine the system configuration directory for Kotodama {purpose}")
    })?;
    Ok(config_root.join("kotodama"))
}

fn load_json_or_default<T: Default + DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let contents = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(T::default()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to read {label} file at {}", path.display())
            });
        }
    };

    serde_json::from_str::<T>(&contents).with_context(|| {
        format!(
            "Failed to parse {label} JSON at {}",
            path.display()
        )
    })
}

fn save_json_pretty<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create {label} directory at {}",
                parent.display()
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(value)
        .with_context(|| format!("Failed to serialize Kotodama {label} to JSON"))?;

    fs::write(path, serialized).with_context(|| {
        format!("Failed to write {label} file at {}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    use chrono::Utc;

    use super::*;
    use crate::models::{DeckConfig, DeckSource, FieldMapping, ThemePreference};

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

    #[test]
    fn deck_workspace_defaults_when_missing_and_round_trips_multiple_mappings() -> Result<()> {
        let dir = unique_temp_dir("deck_workspace");
        let path = dir.join("nested").join("deck_config.json");

        assert_eq!(load_deck_workspace(&path)?, DeckWorkspaceConfig::default());

        let core_mapping = FieldMapping {
            word_field: "Expression".to_string(),
            kana_field: "Reading".to_string(),
            sentence_field: Some("Sentence".to_string()),
            word_audio_field: "WordAudio".to_string(),
            sentence_audio_field: Some("SentenceAudio".to_string()),
        };
        let jlpt_mapping = FieldMapping {
            word_field: "Word".to_string(),
            kana_field: "Kana".to_string(),
            sentence_field: None,
            word_audio_field: "Audio".to_string(),
            sentence_audio_field: None,
        };

        let mut saved_mappings = BTreeMap::new();
        saved_mappings.insert("Core 2k".to_string(), core_mapping.clone());
        saved_mappings.insert("JLPT N4".to_string(), jlpt_mapping);

        let workspace = DeckWorkspaceConfig {
            active_deck: Some(DeckConfig {
                deck_name: "Core 2k".to_string(),
                source: DeckSource::LocalCollection {
                    collection_path: PathBuf::from("/tmp/collection.anki2"),
                    media_dir: PathBuf::from("/tmp/collection.media"),
                },
                field_mapping: core_mapping,
                last_synced_at: Some(Utc::now()),
                flagged_audio_count: 2,
            }),
            saved_mappings,
        };

        save_deck_workspace(&path, &workspace)?;
        let loaded = load_deck_workspace(&path)?;
        assert_eq!(loaded, workspace);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn load_deck_workspace_returns_error_on_corrupt_json() -> Result<()> {
        let dir = unique_temp_dir("deck_corrupt");
        fs::create_dir_all(&dir)?;
        let path = dir.join("deck_config.json");
        fs::write(&path, "not valid json")?;

        assert!(load_deck_workspace(&path).is_err());

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }
}
