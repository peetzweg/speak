//! The user's pronunciation lexicon, compatible with speak.py's file and commands.
//!
//! One `word = spoken form` per line, `#` comments. Entries match whole words, ignoring case,
//! longest entry first. A spoken form written as `/phonemes/` (Kokoro/misaki alphabet) bypasses
//! G2P for that word; any other spoken form is a respelling that goes through G2P.

use std::io::Write;
use std::path::Path;

use regex::{Regex, RegexBuilder};

pub const HEADER: &str = "# speak pronunciations: `word = spoken form`, one per line. Matches whole words, ignoring case.";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Lexicon {
    /// In file order; keys are unique (exact match), like the Python dict speak.py uses.
    entries: Vec<(String, String)>,
}

/// A piece of text after the lexicon ran: plain text for G2P, or a word with exact phonemes.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text(String),
    Phonemes { word: String, phonemes: String },
}

/// `/kˈOkəɹO/` → `kˈOkəɹO`; `None` for a respelling.
pub fn phonemes_of(spoken: &str) -> Option<&str> {
    spoken.strip_prefix('/')?.strip_suffix('/').filter(|p| !p.is_empty())
}

impl Lexicon {
    pub fn parse(src: &str) -> Self {
        let mut lex = Self::default();
        for line in src.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((word, spoken)) = line.split_once('=') else { continue };
            let (word, spoken) = (word.trim(), spoken.trim());
            if word.is_empty() || spoken.is_empty() {
                continue;
            }
            // A repeated key keeps its first position and takes the later value (dict semantics).
            match lex.entries.iter_mut().find(|(w, _)| w == word) {
                Some(e) => e.1 = spoken.to_string(),
                None => lex.entries.push((word.to_string(), spoken.to_string())),
            }
        }
        lex
    }

    /// A missing or unreadable file is an empty lexicon.
    pub fn load(path: &Path) -> Self {
        Self::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    /// Sorted by lower-cased word, as the file is written and `list` prints it.
    pub fn sorted(&self) -> Vec<(&str, &str)> {
        let mut v: Vec<(&str, &str)> = self.entries.iter().map(|(w, s)| (w.as_str(), s.as_str())).collect();
        v.sort_by_key(|(w, _)| w.to_lowercase());
        v
    }

    /// Add or replace; any entry spelled the same apart from case is replaced.
    pub fn add(&mut self, word: &str, spoken: &str) {
        self.remove(word);
        self.entries.push((word.to_string(), spoken.to_string()));
    }

    /// Remove every entry spelled like `word` apart from case; false if there was none.
    pub fn remove(&mut self, word: &str) -> bool {
        let before = self.entries.len();
        let lower = word.to_lowercase();
        self.entries.retain(|(w, _)| w.to_lowercase() != lower);
        self.entries.len() != before
    }

    pub fn render(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for (w, s) in self.sorted() {
            out.push_str(&format!("{w} = {s}\n"));
        }
        out
    }

    /// Write the whole file (header plus sorted entries), replacing it atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.tmp");
        std::fs::File::create(&tmp)?.write_all(self.render().as_bytes())?;
        std::fs::rename(&tmp, path)
    }

    /// Apply every entry to `text`, longest word first. Respellings are substituted in the text
    /// (so a later, shorter entry can still match inside them, as in speak.py); phoneme entries
    /// cut the text into segments.
    pub fn apply(&self, text: &str) -> Vec<Segment> {
        let mut segments = vec![Segment::Text(text.to_string())];
        let mut order: Vec<&(String, String)> = self.entries.iter().collect();
        order.sort_by_key(|(w, _)| std::cmp::Reverse(w.chars().count()));
        for (word, spoken) in order {
            let Ok(re) = RegexBuilder::new(&regex::escape(word)).case_insensitive(true).build() else { continue };
            segments = segments
                .into_iter()
                .flat_map(|seg| match seg {
                    Segment::Text(t) => substitute(&re, &t, spoken),
                    other => vec![other],
                })
                .collect();
        }
        segments
    }
}

/// `\w` or `-`: what may not touch a match on either side (speak.py's `(?<![\w-])…(?![\w-])`).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Byte ranges of whole-word, non-overlapping matches, scanning left to right like Python's `re`.
fn whole_word_matches(re: &Regex, text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(m) = re.find_at(text, pos) {
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if before.is_some_and(is_word_char) || after.is_some_and(is_word_char) {
            // Retry one character later, as a regex with look-around would.
            pos = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        } else {
            out.push(m.range());
            pos = m.end();
        }
        if pos > text.len() {
            break;
        }
    }
    out
}

fn substitute(re: &Regex, text: &str, spoken: &str) -> Vec<Segment> {
    let matches = whole_word_matches(re, text);
    if matches.is_empty() {
        return vec![Segment::Text(text.to_string())];
    }
    match phonemes_of(spoken) {
        None => {
            let mut out = String::with_capacity(text.len());
            let mut last = 0;
            for r in matches {
                out.push_str(&text[last..r.start]);
                out.push_str(spoken);
                last = r.end;
            }
            out.push_str(&text[last..]);
            vec![Segment::Text(out)]
        }
        Some(ph) => {
            let mut out = Vec::new();
            let mut last = 0;
            for r in matches {
                if r.start > last {
                    out.push(Segment::Text(text[last..r.start].to_string()));
                }
                out.push(Segment::Phonemes { word: text[r.clone()].to_string(), phonemes: ph.to_string() });
                last = r.end;
            }
            if last < text.len() {
                out.push(Segment::Text(text[last..].to_string()));
            }
            out
        }
    }
}

/// Segments back to one string, with phoneme entries as misaki's inline `[word](/phonemes/)`
/// markup. That is the form backends receive: misaki reads it natively, the espeak path splices
/// the phonemes in, and a backend without phoneme input would speak the word.
pub fn to_markup(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| match s {
            Segment::Text(t) => t.clone(),
            Segment::Phonemes { word, phonemes } => format!("[{word}](/{phonemes}/)"),
        })
        .collect()
}

/// The inverse of [`to_markup`]: split `[word](/phonemes/)` markers out of `text`.
pub fn from_markup(text: &str) -> Vec<Segment> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\(/([^)/]+)/+\)").unwrap());
    let mut out = Vec::new();
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(0).unwrap();
        if m.start() > last {
            out.push(Segment::Text(text[last..m.start()].to_string()));
        }
        out.push(Segment::Phonemes { word: c[1].to_string(), phonemes: c[2].to_string() });
        last = m.end();
    }
    if last < text.len() {
        out.push(Segment::Text(text[last..].to_string()));
    }
    out
}

/// Text for a voice that cannot take phonemes (e.g. the platform fallback): markers become words.
pub fn plain(text: &str) -> String {
    from_markup(text)
        .into_iter()
        .map(|s| match s {
            Segment::Text(t) => t,
            Segment::Phonemes { word, .. } => word,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Lexicon {
        Lexicon::parse(src)
    }

    fn apply(l: &Lexicon, text: &str) -> String {
        to_markup(&l.apply(text))
    }

    #[test]
    fn parses_like_speak_py() {
        let l = lex("# comment\n\n KruAPI = crew A P I \nno equals sign\n= empty word\nempty =\nx = a = b\nKruAPI = again\n");
        assert_eq!(l.entries, vec![("KruAPI".into(), "again".into()), ("x".into(), "a = b".into())]);
    }

    #[test]
    fn whole_words_ignoring_case() {
        let l = lex("kruapi = crew A P I\n");
        assert_eq!(apply(&l, "KruAPI shipped. kruapi, KRUAPI!"), "crew A P I shipped. crew A P I, crew A P I!");
        // Not inside other words, and hyphens and underscores count as word characters.
        assert_eq!(apply(&l, "KruAPIs kruapi-host my_kruapi xkruapi"), "KruAPIs kruapi-host my_kruapi xkruapi");
        // Apostrophes and other punctuation are boundaries.
        assert_eq!(apply(&l, "KruAPI's (kruapi) \"kruapi\""), "crew A P I's (crew A P I) \"crew A P I\"");
    }

    #[test]
    fn longest_entry_first() {
        let l = lex("kruapi = crew A P I\nkruapi-host = crew A P I host\n");
        assert_eq!(apply(&l, "kruapi-host and kruapi"), "crew A P I host and crew A P I");
    }

    #[test]
    fn later_entries_see_earlier_respellings() {
        // speak.py substitutes entry by entry over the whole text, so this chains.
        let l = lex("dotJS = dot J S\ndot = point\n");
        assert_eq!(apply(&l, "dotJS"), "point J S");
    }

    #[test]
    fn retries_after_a_rejected_match() {
        let l = lex("aa = X\n");
        assert_eq!(apply(&l, "aaa aa"), "aaa X");
        assert_eq!(apply(&l, "aa aa"), "X X");
    }

    #[test]
    fn unicode_words() {
        let l = lex("Anouk = ah-nook\ncafé = caff\n");
        assert_eq!(apply(&l, "Anouk, ANOUK and Anouks. Café"), "ah-nook, ah-nook and Anouks. caff");
        assert_eq!(apply(&l, "cafés"), "cafés");
    }

    #[test]
    fn phoneme_entries_become_segments() {
        let l = lex("kokoro = /kˈOkəɹO/\n");
        let segs = l.apply("Kokoro and kokoro-onnx.");
        assert_eq!(
            segs,
            vec![Segment::Phonemes { word: "Kokoro".into(), phonemes: "kˈOkəɹO".into() }, Segment::Text(" and kokoro-onnx.".into()),]
        );
        assert_eq!(to_markup(&segs), "[Kokoro](/kˈOkəɹO/) and kokoro-onnx.");
        assert_eq!(from_markup(&to_markup(&segs)), segs);
        assert_eq!(plain(&to_markup(&segs)), "Kokoro and kokoro-onnx.");
    }

    #[test]
    fn respellings_never_touch_phoneme_segments() {
        let l = lex("kokoro = /kˈOkəɹO/\nO = oh\n");
        assert_eq!(apply(&l, "kokoro O"), "[kokoro](/kˈOkəɹO/) oh");
    }

    #[test]
    fn add_replaces_case_insensitive_duplicates() {
        let mut l = lex("KruAPI = old\nother = x\n");
        l.add("kruapi", "crew A P I");
        assert_eq!(l.entries, vec![("other".into(), "x".into()), ("kruapi".into(), "crew A P I".into())]);
        assert!(l.remove("KRUAPI"));
        assert!(!l.remove("kruapi"));
    }

    #[test]
    fn file_is_written_sorted_with_header() {
        let l = lex("zeta = z\nAlpha = a\nbeta = /bˈAtə/\n");
        assert_eq!(l.render(), format!("{HEADER}\nAlpha = a\nbeta = /bˈAtə/\nzeta = z\n"));
        assert_eq!(Lexicon::parse(&l.render()), lex("Alpha = a\nbeta = /bˈAtə/\nzeta = z\n"));
    }

    #[test]
    fn phonemes_need_both_slashes() {
        assert_eq!(phonemes_of("/kˈOkəɹO/"), Some("kˈOkəɹO"));
        assert_eq!(phonemes_of("/half"), None);
        assert_eq!(phonemes_of("//"), None);
        assert_eq!(phonemes_of("crew A P I"), None);
    }
}
