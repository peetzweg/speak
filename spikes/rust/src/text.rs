//! Front-end text handling, independent of the model: lexicon and sentence chunking (ported from speak.py).

use std::path::{Path, PathBuf};

/// `~/.config/speak/lexicon.txt` lines `word = spoken form`; '#' comments.
pub fn read_lexicon(path: &Path) -> Vec<(String, String)> {
    let Ok(src) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((w, s)) = line.split_once('=') else { continue };
        let (w, s) = (w.trim(), s.trim());
        if !w.is_empty() && !s.is_empty() {
            out.retain(|(k, _)| k != w);
            out.push((w.to_string(), s.to_string()));
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Whole-word, case-insensitive replacement, longest entry first (so "kruapi-host" beats "kruapi").
/// Equivalent to speak.py's `(?<![\w-])WORD(?![\w-])` with re.IGNORECASE.
pub fn apply_lexicon(text: &str, entries: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort_by_key(|(w, _)| std::cmp::Reverse(w.chars().count()));
    let mut text = text.to_string();
    for (word, spoken) in sorted {
        let re = regex::RegexBuilder::new(&regex::escape(word)).case_insensitive(true).build().unwrap();
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for m in re.find_iter(&text) {
            let before = text[..m.start()].chars().next_back();
            let after = text[m.end()..].chars().next();
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
            out.push_str(&text[last..m.start()]);
            out.push_str(spoken);
            last = m.end();
        }
        out.push_str(&text[last..]);
        text = out;
    }
    text
}

/// Sentences, so playback starts after the first one; sentences over 400 chars split at commas.
pub fn chunks(text: &str) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut sentences = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if c == ' ' && k > 0 && matches!(chars[k - 1].1, '.' | '!' | '?' | '…') {
            sentences.push(&text[start..i]);
            start = i + 1;
        }
    }
    sentences.push(&text[start..]);
    let mut out = Vec::new();
    for s in sentences {
        let mut s = s.to_string();
        while s.len() > 400 {
            let cut = match s[..s.floor_char_boundary(400)].rfind(", ") {
                Some(c) if c > 100 => c + 1,
                _ => s.floor_char_boundary(400),
            };
            out.push(s[..cut].trim().to_string());
            s = s[cut..].trim().to_string();
        }
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("speak")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// `config.toml`-ish: only `key = "value"` lines. Enough for `voice` and `backend`.
pub fn config_get(key: &str) -> Option<String> {
    let src = std::fs::read_to_string(config_dir().join("config.toml")).ok()?;
    src.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

pub fn config_set(key: &str, value: &str) -> std::io::Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let src = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = src.lines().filter(|l| l.split_once('=').is_none_or(|(k, _)| k.trim() != key)).map(String::from).collect();
    lines.push(format!("{key} = \"{value}\""));
    std::fs::write(&path, lines.join("\n") + "\n")?;
    Ok(path)
}
