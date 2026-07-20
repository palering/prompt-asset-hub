use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_SIDEBAR_WIDTH: u16 = 238;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub storage: StorageConfig,
    pub ui: UiConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageConfig {
    pub database_backend: String,
    pub data_root: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct UiConfig {
    pub default_asset_view: String,
    pub sidebar_collapsed: bool,
    pub sidebar_width: u16,
}

#[derive(Clone, Debug)]
pub struct ConfigLocation {
    pub config_path: PathBuf,
    pub base_dir: PathBuf,
    pub default_data_root: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            storage: StorageConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_backend: "sqlite".to_owned(),
            data_root: ".local/data".to_owned(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_asset_view: "card".to_owned(),
            sidebar_collapsed: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        }
    }
}

impl AppConfig {
    pub fn normalize(&mut self) {
        self.version = CONFIG_VERSION;
        self.storage.database_backend = self.storage.database_backend.to_ascii_lowercase();
        if !matches!(self.storage.database_backend.as_str(), "sqlite" | "turso") {
            "sqlite".clone_into(&mut self.storage.database_backend);
        }
        if self.storage.data_root.trim().is_empty() {
            ".local/data".clone_into(&mut self.storage.data_root);
        }
        if !matches!(self.ui.default_asset_view.as_str(), "card" | "list") {
            "card".clone_into(&mut self.ui.default_asset_view);
        }
        self.ui.sidebar_width = self.ui.sidebar_width.clamp(210, 360);
    }
}

pub fn locate(app: &tauri::AppHandle) -> Result<ConfigLocation, String> {
    if cfg!(debug_assertions) {
        let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        return Ok(ConfigLocation {
            config_path: base_dir.join(".local/config.toml"),
            base_dir,
            default_data_root: ".local/data".to_owned(),
        });
    }

    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    Ok(ConfigLocation {
        config_path: config_dir.join("config.toml"),
        base_dir: config_dir,
        default_data_root: path_text(&data_dir),
    })
}

pub fn load_or_create(location: &ConfigLocation) -> Result<AppConfig, String> {
    if location.config_path.exists() {
        return load(&location.config_path);
    }

    let mut config = AppConfig::default();
    config
        .storage
        .data_root
        .clone_from(&location.default_data_root);
    save(&location.config_path, &config)?;
    Ok(config)
}

pub fn load(path: &Path) -> Result<AppConfig, String> {
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut config: AppConfig = toml::from_str(&contents).map_err(|error| error.to_string())?;
    config.normalize();
    Ok(config)
}

pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    let mut normalized = config.clone();
    normalized.normalize();
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent directory".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let contents = toml::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
    std::fs::write(path, contents).map_err(|error| error.to_string())
}

pub fn resolve_data_root(base_dir: &Path, configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, load, path_text, resolve_data_root, save};

    #[test]
    fn persists_and_normalizes_workspace_configuration() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        let path = directory.path().join("config.toml");
        let mut config = AppConfig::default();
        config.storage.data_root = "data/library".to_owned();
        config.ui.default_asset_view = "list".to_owned();
        config.ui.sidebar_width = 900;
        save(&path, &config).expect("save configuration");

        let loaded = load(&path).expect("load configuration");
        assert_eq!(loaded.ui.default_asset_view, "list");
        assert_eq!(loaded.ui.sidebar_width, 360);
        assert_eq!(
            resolve_data_root(directory.path(), &loaded.storage.data_root),
            directory.path().join("data/library")
        );
        assert_eq!(path_text(&path), path.to_string_lossy());
    }
}
