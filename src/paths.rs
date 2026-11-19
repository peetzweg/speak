//! Where speak keeps things. Every location can be redirected through the environment, which is
//! also how the tests stay away from the owner's real files.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `$SPEAK_HOME`, default `~/.local/share/speak`. Models live in `models/` below it.
pub fn data_home() -> PathBuf {
    env_path("SPEAK_HOME").unwrap_or_else(|| home().join(".local/share/speak"))
}

/// `$XDG_CONFIG_HOME/speak`, default `~/.config/speak` (on macOS too, like most CLI tools).
pub fn config_dir() -> PathBuf {
    env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join("speak")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// `$SPEAK_LEXICON`, default `<config dir>/lexicon.txt`.
pub fn lexicon_file() -> PathBuf {
    env_path("SPEAK_LEXICON").unwrap_or_else(|| config_dir().join("lexicon.txt"))
}

/// Disposable state (checksum stamps): `$XDG_CACHE_HOME/speak`, else the platform cache directory.
pub fn cache_dir() -> PathBuf {
    if let Some(p) = env_path("XDG_CACHE_HOME") {
        return p.join("speak");
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Caches/speak")
    } else if cfg!(windows) {
        env_path("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData/Local")).join("speak")
    } else {
        home().join(".cache/speak")
    }
}

/// Is `name` an executable on `PATH`?
pub fn which(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&file)).find(|p| p.is_file())
}
