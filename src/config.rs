//! Paths and user preferences: a small JSON file, no GSettings schema to install.

use std::path::PathBuf;

use gtk::glib;
use serde::{Deserialize, Serialize};

const APP_DIR_NAME: &str = "tango";

/// `#[serde(default)]` means a config file may omit any field, or be from an older version,
/// and the missing fields take the values from `Default`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Order of the gloss blocks in an entry, ISO 639-2 codes as JMdict uses them.
    pub gloss_languages: Vec<String>,
    /// "system" | "light" | "dark"
    pub color_scheme: String,
    pub window: WindowState,
    #[serde(skip)]
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1100,
            height: 750,
            maximized: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gloss_languages: vec!["eng".into(), "ger".into()],
            color_scheme: "system".into(),
            window: WindowState::default(),
            path: config_dir().join("config.json"),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        Self::load_from(config_dir().join("config.json"))
    }

    pub fn load_from(path: PathBuf) -> Self {
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("could not parse {}: {e}", path.display());
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                log::warn!("could not read {}: {e}", path.display());
                Self::default()
            }
        };
        cfg.path = path;
        cfg
    }

    /// Writes via a temp file and rename, so a crash mid-write never leaves a half config.
    pub fn save(&self) {
        let tmp = self.path.with_extension("json.tmp");
        let result = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|text| std::fs::write(&tmp, text))
            .and_then(|_| std::fs::rename(&tmp, &self.path));
        if let Err(e) = result {
            log::warn!("could not write {}: {e}", self.path.display());
        }
    }
}

fn app_dir(base: PathBuf) -> PathBuf {
    let dir = base.join(APP_DIR_NAME);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create {}: {e}", dir.display());
    }
    dir
}

pub fn config_dir() -> PathBuf {
    app_dir(glib::user_config_dir())
}

pub fn data_dir() -> PathBuf {
    app_dir(glib::user_data_dir())
}

pub fn cache_dir() -> PathBuf {
    app_dir(glib::user_cache_dir())
}

pub fn database_path() -> PathBuf {
    match std::env::var_os("TANGO_DB") {
        Some(p) => PathBuf::from(p),
        None => data_dir().join("tango.sqlite"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("tango-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let mut cfg = Config::load_from(path.clone());
        assert_eq!(cfg.gloss_languages, ["eng", "ger"]);
        cfg.gloss_languages = vec!["ger".into(), "eng".into()];
        cfg.save();
        let again = Config::load_from(path);
        assert_eq!(again.gloss_languages, ["ger", "eng"]);
        assert_eq!(again.window.width, 1100); // untouched fields keep their defaults
        std::fs::remove_dir_all(dir).unwrap();
    }
}
