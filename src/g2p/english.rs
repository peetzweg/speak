//! English G2P: misaki (lexicons, POS tagging, numbers) via misaki-rs, with the `espeak-ng`
//! program as the fallback for words misaki doesn't know.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use misaki_rs::fallback::FallbackError;

use super::espeak::{self, Program, Subprocess};
use crate::normalize;

pub struct English {
    g2p: misaki_rs::G2P,
    british: bool,
}

impl English {
    /// Takes 40–150 ms (misaki-rs parses its embedded lexicons), so callers build it on a thread.
    pub fn new(british: bool, espeak: Option<Program>) -> Self {
        let lang = if british { misaki_rs::Language::EnglishGB } else { misaki_rs::Language::EnglishUS };
        let fallback = EspeakFallback { british, program: espeak, child: Mutex::new(None), warned: AtomicBool::new(false) };
        let mut g2p = misaki_rs::G2P::with_fallback(lang, Some(Box::new(fallback)));
        g2p.unk = String::new(); // like Kokoro's pipeline: a word nothing can read is skipped
        Self { g2p, british }
    }

    /// Phonemes in misaki's alphabet. `text` may contain `[word](/phonemes/)` lexicon markup.
    pub fn phonemize(&self, text: &str) -> Result<String> {
        let text = normalize::english(text, self.british);
        let (ps, _) = self.g2p.g2p(&text)?;
        Ok(ps)
    }
}

/// misaki's `Fallback` backed by an `espeak-ng` child, started on the first unknown word.
/// Without espeak-ng the word is spelled out letter by letter.
struct EspeakFallback {
    british: bool,
    program: Option<Program>,
    child: Mutex<Option<Subprocess>>,
    warned: AtomicBool,
}

impl misaki_rs::Fallback for EspeakFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        let fail = |e: anyhow::Error| FallbackError::Espeak { word: word.to_string(), error: format!("{e:#}") };
        let Some(program) = &self.program else {
            if !self.warned.swap(true, Ordering::Relaxed) {
                eprintln!("speak: espeak-ng not found, spelling out unknown words like '{word}' (install espeak-ng or set SPEAK_ESPEAK)");
            }
            return spell(word, self.british).ok_or(FallbackError::NoPhonemes { word: word.to_string() });
        };
        let mut child = self.child.lock().map_err(|e| FallbackError::MutexPoisoned(e.to_string()))?;
        if child.is_none() {
            *child = Some(Subprocess::spawn(program, if self.british { "en" } else { "en-us" }, true).map_err(fail)?);
        }
        let ps = espeak::english_word(child.as_mut().unwrap(), word, self.british).map_err(fail)?;
        if ps.is_empty() { Err(FallbackError::NoPhonemes { word: word.to_string() }) } else { Ok(ps) }
    }
}

/// Letter names in misaki's alphabet, for when there is no espeak-ng.
fn spell(word: &str, british: bool) -> Option<String> {
    const US: [&str; 26] = [
        "ˈA",
        "bˈi",
        "sˈi",
        "dˈi",
        "ˈi",
        "ˈɛf",
        "ʤˈi",
        "ˈAʧ",
        "ˈI",
        "ʤˈA",
        "kˈA",
        "ˈɛl",
        "ˈɛm",
        "ˈɛn",
        "ˈO",
        "pˈi",
        "kjˈu",
        "ˈɑɹ",
        "ˈɛs",
        "tˈi",
        "jˈu",
        "vˈi",
        "dˈʌbᵊlju",
        "ˈɛks",
        "wˈI",
        "zˈi",
    ];
    let letters: Vec<&str> = word
        .chars()
        .filter_map(|c| {
            let i = (c.to_ascii_lowercase() as u32).checked_sub('a' as u32).filter(|i| *i < 26)? as usize;
            Some(match (british, c.to_ascii_lowercase()) {
                (true, 'o') => "ˈQ",
                (true, 'r') => "ˈɑː",
                (true, 'z') => "zˈɛd",
                _ => US[i],
            })
        })
        .collect();
    (!letters.is_empty()).then(|| letters.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_without_espeak() {
        assert_eq!(spell("Tru", false).as_deref(), Some("tˈi ˈɑɹ jˈu"));
        assert_eq!(spell("oz", true).as_deref(), Some("ˈQ zˈɛd"));
        assert_eq!(spell("–", false), None);
    }

    #[test]
    fn misaki_reads_numbers_and_versions() {
        let en = English::new(false, None);
        let ps = en.phonemize("They had 2,000 bots and shipped v0.3.5.").unwrap();
        assert!(ps.contains("θˈWzᵊnd"), "{ps}");
        assert!(ps.contains("vˈɜɹʒən zˈɪɹO"), "{ps}");
    }

    #[test]
    fn phoneme_markup_bypasses_g2p() {
        let en = English::new(false, None);
        assert_eq!(en.phonemize("say [Kokoro](/kˈOkəɹO/) please").unwrap(), "sˈA kˈOkəɹO plˈiz");
    }
}
