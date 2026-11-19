//! `~/.config/speak/config.toml` holds the defaults `voice` and `model`.
//!
//! Only flat `key = "value"` lines are read. When speak writes a key it rewrites that one line and
//! leaves everything else in the file (comments, other keys) as it was.

use std::path::Path;

#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub voice: Option<String>,
    pub model: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        Self::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    pub fn parse(src: &str) -> Self {
        Self { voice: get(src, "voice"), model: get(src, "model") }
    }
}

/// flag > environment > config file; `None` means "use the backend's default".
pub fn resolve(flag: Option<String>, env: Option<String>, file: Option<String>) -> Option<String> {
    [flag, env, file].into_iter().flatten().find(|v| !v.is_empty())
}

/// The value of a top-level `key = "value"` (or `'value'`, or a bare word) line.
pub fn get(src: &str, key: &str) -> Option<String> {
    src.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.trim();
        let value = match v.chars().next()? {
            q @ ('"' | '\'') => v[1..].split(q).next()?.to_string(),
            _ => v.split('#').next()?.trim().to_string(),
        };
        (!value.is_empty()).then_some(value)
    })
}

/// `src` with `key = "value"` replacing an existing `key` line, or appended.
pub fn with_value(src: &str, key: &str, value: &str) -> String {
    let line = format!("{key} = \"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""));
    let mut replaced = false;
    let mut out: Vec<String> = src
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, _)) if k.trim() == key && !l.trim_start().starts_with('#') => {
                replaced = true;
                line.clone()
            }
            _ => l.to_string(),
        })
        .collect();
    if !replaced {
        out.push(line);
    }
    out.join("\n") + "\n"
}

pub fn set(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let src = std::fs::read_to_string(path).unwrap_or_default();
    std::fs::write(path, with_value(&src, key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn precedence_is_flag_env_file() {
        assert_eq!(resolve(s("flag"), s("env"), s("file")), s("flag"));
        assert_eq!(resolve(None, s("env"), s("file")), s("env"));
        assert_eq!(resolve(None, None, s("file")), s("file"));
        assert_eq!(resolve(None, None, None), None);
        // An empty SPEAK_VOICE= does not hide the config file.
        assert_eq!(resolve(None, s(""), s("file")), s("file"));
    }

    #[test]
    fn parses_flat_keys() {
        let src = "# speak\nvoice = \"bm_george\"  # British\nmodel='kokoro-int8'\nother = 1\n[table]\n";
        assert_eq!(Config::parse(src), Config { voice: s("bm_george"), model: s("kokoro-int8") });
        assert_eq!(get("voice = af_heart # bare", "voice"), s("af_heart"));
        assert_eq!(get("voices = x", "voice"), None);
        assert_eq!(get("voice = \"\"", "voice"), None);
    }

    #[test]
    fn writing_keeps_other_lines() {
        let src = "# my settings\nvoice = \"af_heart\"\nmodel = \"kokoro\"\n";
        assert_eq!(with_value(src, "voice", "bm_george"), "# my settings\nvoice = \"bm_george\"\nmodel = \"kokoro\"\n");
        assert_eq!(with_value("", "voice", "af_bella"), "voice = \"af_bella\"\n");
        assert_eq!(with_value("# voice = \"x\"\n", "voice", "y"), "# voice = \"x\"\nvoice = \"y\"\n");
    }
}
