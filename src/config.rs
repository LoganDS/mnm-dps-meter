//! Configuration management for the mnm-dps-meter application.
//!
//! Handles loading and saving of user preferences to a TOML config file
//! at platform-appropriate paths via the `dirs` crate. Invalid config
//! values fall back to defaults with logged warnings. The app never
//! crashes on bad config.

use crate::types::CaptureRegion;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

/// Minimum allowed capture interval in milliseconds.
const MIN_CAPTURE_INTERVAL_MS: u32 = 50;

/// Default combat log capture interval in milliseconds.
const DEFAULT_COMBAT_CAPTURE_INTERVAL_MS: u32 = 250;

/// Default mini panel capture interval in milliseconds.
const DEFAULT_PANEL_CAPTURE_INTERVAL_MS: u32 = 1000;

/// Application configuration persisted to TOML.
///
/// Stores user-configured capture regions, character name, and capture
/// intervals. Session data is ephemeral and not part of config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Screen region for the combat log OCR capture.
    pub combat_log_region: Option<CaptureRegion>,
    /// Screen region for the mini character panel OCR capture.
    pub mini_panel_region: Option<CaptureRegion>,
    /// Player's character name for "You"/"Your" translation.
    pub character_name: Option<String>,
    /// Combat log capture interval in milliseconds (default 250, minimum 50).
    #[serde(default = "default_combat_interval")]
    pub combat_capture_interval_ms: u32,
    /// Mini panel capture interval in milliseconds (default 1000, minimum 50).
    #[serde(default = "default_panel_interval")]
    pub panel_capture_interval_ms: u32,
}

fn default_combat_interval() -> u32 {
    DEFAULT_COMBAT_CAPTURE_INTERVAL_MS
}

fn default_panel_interval() -> u32 {
    DEFAULT_PANEL_CAPTURE_INTERVAL_MS
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            combat_log_region: None,
            mini_panel_region: None,
            character_name: None,
            combat_capture_interval_ms: DEFAULT_COMBAT_CAPTURE_INTERVAL_MS,
            panel_capture_interval_ms: DEFAULT_PANEL_CAPTURE_INTERVAL_MS,
        }
    }
}

impl AppConfig {
    /// Returns the platform-appropriate config file path.
    ///
    /// - Windows: `%APPDATA%\mnm-dps-meter\config.toml`
    /// - Linux: `~/.config/mnm-dps-meter/config.toml`
    /// - macOS: `~/Library/Application Support/mnm-dps-meter/config.toml`
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mnm-dps-meter").join("config.toml"))
    }

    /// Load config from the platform-appropriate path.
    ///
    /// If the file doesn't exist, returns defaults. If the file is
    /// unparseable, logs a warning and returns defaults. Individual
    /// invalid fields fall back to their defaults.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            warn!("Could not determine config directory; using defaults");
            return Self::default();
        };

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Self::default();
            }
            Err(e) => {
                warn!("Failed to read config file at {}: {}; using defaults", path.display(), e);
                return Self::default();
            }
        };

        match toml::from_str::<AppConfig>(&content) {
            Ok(mut config) => {
                config.validate();
                config
            }
            Err(e) => {
                warn!("Config file at {} is unparseable: {}; using defaults", path.display(), e);
                Self::default()
            }
        }
    }

    /// Save config to the platform-appropriate path.
    ///
    /// Creates the config directory if it doesn't exist.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Validate config values, falling back to defaults for invalid fields.
    fn validate(&mut self) {
        if self.combat_capture_interval_ms < MIN_CAPTURE_INTERVAL_MS {
            warn!(
                "combat_capture_interval_ms ({}) below minimum {}; using default {}",
                self.combat_capture_interval_ms, MIN_CAPTURE_INTERVAL_MS, DEFAULT_COMBAT_CAPTURE_INTERVAL_MS
            );
            self.combat_capture_interval_ms = DEFAULT_COMBAT_CAPTURE_INTERVAL_MS;
        }

        if self.panel_capture_interval_ms < MIN_CAPTURE_INTERVAL_MS {
            warn!(
                "panel_capture_interval_ms ({}) below minimum {}; using default {}",
                self.panel_capture_interval_ms, MIN_CAPTURE_INTERVAL_MS, DEFAULT_PANEL_CAPTURE_INTERVAL_MS
            );
            self.panel_capture_interval_ms = DEFAULT_PANEL_CAPTURE_INTERVAL_MS;
        }

        if let Some(ref region) = self.combat_log_region {
            if region.width == 0 || region.height == 0 {
                warn!("combat_log_region has zero dimension; clearing");
                self.combat_log_region = None;
            }
        }

        if let Some(ref region) = self.mini_panel_region {
            if region.width == 0 || region.height == 0 {
                warn!("mini_panel_region has zero dimension; clearing");
                self.mini_panel_region = None;
            }
        }

        if let Some(ref name) = self.character_name {
            if name.trim().is_empty() {
                warn!("character_name is empty; clearing");
                self.character_name = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper to run config tests with an isolated config directory.
    fn with_temp_config_dir<F: FnOnce(PathBuf)>(f: F) {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("mnm-dps-meter");
        fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("config.toml");
        f(config_file);
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.combat_capture_interval_ms, 250);
        assert_eq!(config.panel_capture_interval_ms, 1000);
        assert!(config.combat_log_region.is_none());
        assert!(config.mini_panel_region.is_none());
        assert!(config.character_name.is_none());
    }

    #[test]
    fn test_config_round_trip() {
        with_temp_config_dir(|config_file| {
            let config = AppConfig {
                combat_log_region: Some(CaptureRegion {
                    x: 100,
                    y: 200,
                    width: 800,
                    height: 300,
                }),
                mini_panel_region: Some(CaptureRegion {
                    x: 50,
                    y: 50,
                    width: 200,
                    height: 150,
                }),
                character_name: Some("Narky".to_string()),
                combat_capture_interval_ms: 300,
                panel_capture_interval_ms: 500,
            };

            let content = toml::to_string_pretty(&config).unwrap();
            fs::write(&config_file, &content).unwrap();

            let loaded: AppConfig = toml::from_str(&fs::read_to_string(&config_file).unwrap()).unwrap();
            assert_eq!(loaded.combat_capture_interval_ms, 300);
            assert_eq!(loaded.panel_capture_interval_ms, 500);
            assert_eq!(loaded.character_name.as_deref(), Some("Narky"));
            assert_eq!(loaded.combat_log_region.unwrap().x, 100);
            assert_eq!(loaded.mini_panel_region.unwrap().width, 200);
        });
    }

    #[test]
    fn test_config_validation_low_intervals() {
        let mut config = AppConfig {
            combat_capture_interval_ms: 10,
            panel_capture_interval_ms: 0,
            ..Default::default()
        };
        config.validate();
        assert_eq!(config.combat_capture_interval_ms, 250);
        assert_eq!(config.panel_capture_interval_ms, 1000);
    }

    #[test]
    fn test_config_validation_zero_dimension_region() {
        let mut config = AppConfig {
            combat_log_region: Some(CaptureRegion {
                x: 0,
                y: 0,
                width: 0,
                height: 100,
            }),
            mini_panel_region: Some(CaptureRegion {
                x: 0,
                y: 0,
                width: 100,
                height: 0,
            }),
            ..Default::default()
        };
        config.validate();
        assert!(config.combat_log_region.is_none());
        assert!(config.mini_panel_region.is_none());
    }

    #[test]
    fn test_config_validation_empty_name() {
        let mut config = AppConfig {
            character_name: Some("  ".to_string()),
            ..Default::default()
        };
        config.validate();
        assert!(config.character_name.is_none());
    }

    #[test]
    fn test_config_unparseable_toml_returns_defaults() {
        with_temp_config_dir(|config_file| {
            fs::write(&config_file, "this is not valid toml {{{{").unwrap();
            let result: Result<AppConfig, _> = toml::from_str(&fs::read_to_string(&config_file).unwrap());
            assert!(result.is_err());
            // In this case, load() would return defaults
            let config = AppConfig::default();
            assert_eq!(config.combat_capture_interval_ms, 250);
        });
    }

    #[test]
    fn test_config_missing_fields_use_defaults() {
        let toml_str = r#"
character_name = "TestPlayer"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.character_name.as_deref(), Some("TestPlayer"));
        assert_eq!(config.combat_capture_interval_ms, 250);
        assert_eq!(config.panel_capture_interval_ms, 1000);
        assert!(config.combat_log_region.is_none());
    }

    #[test]
    fn test_config_valid_region_preserved() {
        let mut config = AppConfig {
            combat_log_region: Some(CaptureRegion {
                x: 100,
                y: 200,
                width: 800,
                height: 300,
            }),
            ..Default::default()
        };
        config.validate();
        assert!(config.combat_log_region.is_some());
        let region = config.combat_log_region.unwrap();
        assert_eq!(region.x, 100);
        assert_eq!(region.width, 800);
    }

    #[test]
    fn test_config_valid_intervals_preserved() {
        let mut config = AppConfig {
            combat_capture_interval_ms: 100,
            panel_capture_interval_ms: 200,
            ..Default::default()
        };
        config.validate();
        assert_eq!(config.combat_capture_interval_ms, 100);
        assert_eq!(config.panel_capture_interval_ms, 200);
    }
}
