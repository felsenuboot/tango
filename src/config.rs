//! Paths and user preferences: a small JSON file, no GSettings schema to install.

use std::path::PathBuf;

use gtk::glib;
use serde::{Deserialize, Serialize};

use crate::dict::sources;

const APP_DIR_NAME: &str = "tango";

/// `#[serde(default)]` means a config file may omit any field, or be from an older version,
/// and the missing fields take the values from `Default`. Fields a newer file has and this
/// version does not know are ignored, which is serde's default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Order of the gloss blocks in an entry, ISO 639-2 codes as JMdict uses them.
    pub gloss_languages: Vec<String>,
    /// "system" | "light" | "dark", see `ui::theme::Scheme`.
    pub color_scheme: String,
    /// Dictionaries in search priority order. A known source missing here counts as enabled,
    /// after the listed ones; see `source_settings`.
    pub sources: Vec<SourceSetting>,
    pub window: WindowState,
    /// Kanji page: drop the parts no remaining kanji contains from the grid instead of greying
    /// them out.
    pub hide_unusable_radicals: bool,
    /// Show what WaniKani knows about an entry and its kanji (the token itself is in the keyring).
    pub show_wanikani: bool,
    /// Grid views (#83): an opened list as cards in the content pane; tiles instead of rows in
    /// the sidebar; both for search results as well.
    pub list_cards: bool,
    pub list_tiles: bool,
    pub grid_search: bool,
    #[serde(skip)]
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSetting {
    pub id: String,
    pub enabled: bool,
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
            sources: Vec::new(),
            window: WindowState::default(),
            hide_unusable_radicals: false,
            show_wanikani: true,
            list_cards: true,
            list_tiles: false,
            grid_search: false,
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

    /// Every known source in priority order with its enabled flag: the listed ones first,
    /// ids the registry does not know dropped, sources the file does not mention appended.
    pub fn source_settings(&self) -> Vec<SourceSetting> {
        let mut out: Vec<SourceSetting> = self
            .sources
            .iter()
            .filter(|s| sources::by_id(&s.id).is_some())
            .cloned()
            .collect();
        for source in sources::SOURCES {
            if !out.iter().any(|s| s.id == source.id) {
                out.push(SourceSetting {
                    id: source.id.to_string(),
                    enabled: true,
                });
            }
        }
        out
    }

    /// Ids of the sources search should look in, best first.
    pub fn enabled_sources(&self) -> Vec<String> {
        self.source_settings()
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| s.id)
            .collect()
    }

    pub fn set_source_enabled(&mut self, id: &str, enabled: bool) {
        self.sources = self.source_settings();
        if let Some(s) = self.sources.iter_mut().find(|s| s.id == id) {
            s.enabled = enabled;
        }
    }

    /// Moves a source one place up in the search order.
    pub fn move_source_up(&mut self, id: &str) {
        self.sources = self.source_settings();
        if let Some(i) = self.sources.iter().position(|s| s.id == id)
            && i > 0
        {
            self.sources.swap(i, i - 1);
        }
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

/// The user's own data (word lists); `TANGO_USER_DB` points test runs elsewhere.
pub fn user_database_path() -> PathBuf {
    match std::env::var_os("TANGO_USER_DB") {
        Some(p) => PathBuf::from(p),
        None => data_dir().join("user.sqlite"),
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

    #[test]
    fn source_settings_cover_every_known_source() {
        let mut cfg = Config::default();
        let settings = cfg.source_settings();
        assert_eq!(settings.len(), sources::SOURCES.len());
        assert!(settings.iter().all(|s| s.enabled));
        assert_eq!(cfg.enabled_sources()[0], "jmdict");

        cfg.sources = vec![SourceSetting {
            id: "gone".into(),
            enabled: true,
        }];
        assert!(cfg.source_settings().iter().all(|s| s.id != "gone"));

        cfg.set_source_enabled("jmdict", false);
        assert!(!cfg.enabled_sources().contains(&"jmdict".to_string()));
        assert!(cfg.enabled_sources().contains(&"wadoku".to_string()));
        cfg.move_source_up("jmdict"); // first already: no-op, no panic
        assert_eq!(cfg.source_settings()[0].id, "jmdict");
    }

    #[test]
    fn ignores_fields_from_other_versions() {
        let dir = std::env::temp_dir().join(format!("tango-config-old-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"from_the_future": true, "gloss_languages": ["ger"]}"#).unwrap();
        let cfg = Config::load_from(path);
        assert_eq!(cfg.gloss_languages, ["ger"]);
        assert_eq!(cfg.color_scheme, "system");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
