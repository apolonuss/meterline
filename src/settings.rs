use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: Theme,
    pub default_sync_days: i64,
    pub startup_panel: StartupPanel,
    pub hide_values: bool,
    pub default_tray_metric: TrayMetric,
    pub live_refresh: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Balanced,
            default_sync_days: 31,
            startup_panel: StartupPanel::Home,
            hide_values: false,
            default_tray_metric: TrayMetric::Spend,
            live_refresh: true,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str::<Self>(&text)
                .unwrap_or_default()
                .normalized()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => {
                Err(err).with_context(|| format!("could not read settings from {}", path.display()))
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(&self.clone().normalized())?;
        std::fs::write(path, bytes)
            .with_context(|| format!("could not write settings to {}", path.display()))
    }

    pub fn cycle_theme(&mut self) {
        self.theme = self.theme.next();
    }

    pub fn cycle_sync_days(&mut self) {
        self.default_sync_days = match self.default_sync_days {
            7 => 31,
            31 => 90,
            _ => 7,
        };
    }

    pub fn cycle_startup_panel(&mut self) {
        self.startup_panel = self.startup_panel.next();
    }

    pub fn cycle_tray_metric(&mut self) {
        self.default_tray_metric = self.default_tray_metric.next();
    }

    pub fn toggle_live_refresh(&mut self) {
        self.live_refresh = !self.live_refresh;
    }

    pub fn normalized(mut self) -> Self {
        if !matches!(self.default_sync_days, 7 | 31 | 90) {
            self.default_sync_days = 31;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Balanced,
    OpenAi,
    Claude,
    Mono,
}

impl Theme {
    fn next(self) -> Self {
        match self {
            Theme::Balanced => Theme::OpenAi,
            Theme::OpenAi => Theme::Claude,
            Theme::Claude => Theme::Mono,
            Theme::Mono => Theme::Balanced,
        }
    }
}

impl Display for Theme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Theme::Balanced => "balanced",
            Theme::OpenAi => "openai",
            Theme::Claude => "claude",
            Theme::Mono => "mono",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupPanel {
    #[default]
    Home,
    Providers,
    Chats,
    Imports,
}

impl StartupPanel {
    pub fn index(self) -> usize {
        match self {
            StartupPanel::Home => 0,
            StartupPanel::Providers => 2,
            StartupPanel::Chats => 3,
            StartupPanel::Imports => 4,
        }
    }

    fn next(self) -> Self {
        match self {
            StartupPanel::Home => StartupPanel::Providers,
            StartupPanel::Providers => StartupPanel::Chats,
            StartupPanel::Chats => StartupPanel::Imports,
            StartupPanel::Imports => StartupPanel::Home,
        }
    }
}

impl Display for StartupPanel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StartupPanel::Home => "home",
            StartupPanel::Providers => "providers",
            StartupPanel::Chats => "chats",
            StartupPanel::Imports => "imports",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayMetric {
    #[default]
    Spend,
    Tokens,
    Chats,
    Sync,
}

impl TrayMetric {
    pub fn index(self) -> usize {
        match self {
            TrayMetric::Spend => 0,
            TrayMetric::Tokens => 1,
            TrayMetric::Chats => 2,
            TrayMetric::Sync => 3,
        }
    }

    fn next(self) -> Self {
        match self {
            TrayMetric::Spend => TrayMetric::Tokens,
            TrayMetric::Tokens => TrayMetric::Chats,
            TrayMetric::Chats => TrayMetric::Sync,
            TrayMetric::Sync => TrayMetric::Spend,
        }
    }
}

impl Display for TrayMetric {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TrayMetric::Spend => "spend",
            TrayMetric::Tokens => "tokens",
            TrayMetric::Chats => "live",
            TrayMetric::Sync => "sync",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_settings_load_defaults() {
        let dir = tempdir().unwrap();
        let settings = AppSettings::load(&dir.path().join("settings.json")).unwrap();
        assert_eq!(settings, AppSettings::default());
    }

    #[test]
    fn settings_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = AppSettings {
            theme: Theme::Claude,
            default_sync_days: 90,
            startup_panel: StartupPanel::Imports,
            hide_values: true,
            default_tray_metric: TrayMetric::Sync,
            live_refresh: false,
        };

        settings.save(&path).unwrap();
        assert_eq!(AppSettings::load(&path).unwrap(), settings);
    }

    #[test]
    fn invalid_settings_fall_back_to_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(path.as_path(), "{ definitely not settings").unwrap();
        assert_eq!(AppSettings::load(&path).unwrap(), AppSettings::default());
    }

    #[test]
    fn invalid_sync_days_normalize_to_default() {
        let settings = AppSettings {
            default_sync_days: 365,
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(settings.default_sync_days, 31);
    }
}
