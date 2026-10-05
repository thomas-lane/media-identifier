//! Settings and API keys on disk.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mi_sources::ApiKeys;
use mi_types::{ApiKeyProvider, Settings};
use serde::{Deserialize, Serialize};

/// File name of the settings, in the app config folder.
pub const SETTINGS_FILE: &str = "settings.json";
/// File name of the API keys, in the app config folder.
pub const KEYS_FILE: &str = "api-keys.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredKeys {
    subdl: Option<String>,
    tmdb: Option<String>,
}

/// Settings (`settings.json`) and API keys (`api-keys.json`) in the app config folder.
///
/// Keys are kept in their own file, readable only by the user on macOS (mode 0600), so the
/// settings file can be shared when reporting a problem without leaking keys, and so keys never
/// reach the UI. Files are written to a temporary name and renamed, so a crash never leaves a
/// half-written file. A file that cannot be parsed is renamed to `<name>.invalid` and defaults
/// are used, so a damaged file never stops the app from starting.
#[derive(Debug)]
pub struct SettingsStore {
    dir: PathBuf,
    settings: Mutex<Settings>,
    keys: Mutex<StoredKeys>,
}

impl SettingsStore {
    /// Loads both files from `dir` (missing files mean defaults).
    pub fn load(dir: &Path) -> io::Result<Self> {
        let settings = read_json(&dir.join(SETTINGS_FILE))?.unwrap_or_default();
        let keys = read_json(&dir.join(KEYS_FILE))?.unwrap_or_default();
        Ok(Self {
            dir: dir.to_path_buf(),
            settings: Mutex::new(settings),
            keys: Mutex::new(keys),
        })
    }

    /// Current settings.
    pub fn settings(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    /// Replaces and saves the settings.
    pub fn save(&self, settings: Settings) -> io::Result<()> {
        write_json(&self.dir.join(SETTINGS_FILE), &settings, false)?;
        *self.settings.lock().expect("settings lock") = settings;
        Ok(())
    }

    /// Sets or clears (with `None` or an empty string) one API key and saves the keys file.
    pub fn set_key(&self, provider: ApiKeyProvider, key: Option<String>) -> io::Result<()> {
        let key = key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
        let mut keys = self.keys.lock().expect("keys lock").clone();
        match provider {
            ApiKeyProvider::Subdl => keys.subdl = key,
            ApiKeyProvider::Tmdb => keys.tmdb = key,
        }
        write_json(&self.dir.join(KEYS_FILE), &keys, true)?;
        *self.keys.lock().expect("keys lock") = keys;
        Ok(())
    }

    /// Whether a key is stored for `provider`.
    pub fn has_key(&self, provider: ApiKeyProvider) -> bool {
        let keys = self.keys.lock().expect("keys lock");
        match provider {
            ApiKeyProvider::Subdl => keys.subdl.is_some(),
            ApiKeyProvider::Tmdb => keys.tmdb.is_some(),
        }
    }

    /// The keys, for `mi-sources`.
    pub fn api_keys(&self) -> ApiKeys {
        let keys = self.keys.lock().expect("keys lock");
        ApiKeys {
            subdl: keys.subdl.clone(),
            tmdb: keys.tmdb.clone(),
        }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            tracing::warn!("{} is not valid ({e}); using defaults", path.display());
            let mut invalid = path.as_os_str().to_owned();
            invalid.push(".invalid");
            std::fs::rename(path, PathBuf::from(invalid))?;
            Ok(None)
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T, private: bool) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    std::fs::write(&tmp, json)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_types::SpeechModel;

    #[test]
    fn missing_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        assert_eq!(store.settings(), Settings::default());
        assert!(!store.has_key(ApiKeyProvider::Subdl));
    }

    #[test]
    fn settings_and_keys_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        let settings = Settings {
            speech_model: SpeechModel::Fast,
            ..Settings::default()
        };
        store.save(settings.clone()).unwrap();
        store
            .set_key(ApiKeyProvider::Tmdb, Some("  abc  ".into()))
            .unwrap();

        let reloaded = SettingsStore::load(dir.path()).unwrap();
        assert_eq!(reloaded.settings(), settings);
        assert_eq!(reloaded.api_keys().tmdb.as_deref(), Some("abc"));
        assert!(!reloaded.has_key(ApiKeyProvider::Subdl));
    }

    #[test]
    fn keys_are_not_written_into_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        store.save(Settings::default()).unwrap();
        store
            .set_key(ApiKeyProvider::Subdl, Some("secret-key".into()))
            .unwrap();
        let settings_text = std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap();
        assert!(!settings_text.contains("secret-key"));
    }

    #[test]
    fn empty_key_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        store
            .set_key(ApiKeyProvider::Subdl, Some("k".into()))
            .unwrap();
        store
            .set_key(ApiKeyProvider::Subdl, Some("   ".into()))
            .unwrap();
        assert!(!store.has_key(ApiKeyProvider::Subdl));
    }

    #[cfg(unix)]
    #[test]
    fn keys_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        store
            .set_key(ApiKeyProvider::Subdl, Some("k".into()))
            .unwrap();
        let mode = std::fs::metadata(dir.path().join(KEYS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_damaged_settings_file_is_set_aside_and_defaults_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), b"{not json").unwrap();
        let store = SettingsStore::load(dir.path()).unwrap();
        assert_eq!(store.settings(), Settings::default());
        assert!(dir.path().join("settings.json.invalid").exists());
    }
}
