//! English G2P: misaki (lexicons, POS tagging, numbers) via misaki-rs. Words misaki doesn't know go
//! to the `espeak-ng` program when there is one, as in Kokoro's own pipeline; without it, to
//! misaki's neural network (`neural.rs`), so English needs nothing besides speak.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::Result;
use misaki_rs::fallback::FallbackError;

use super::espeak::{self, Program, Subprocess};
use super::neural::{self, Network};
use crate::normalize;

pub struct English {
    g2p: misaki_rs::G2P,
    british: bool,
}

impl English {
    /// Takes 40–150 ms (misaki-rs parses its embedded lexicons), so callers build it on a thread.
    /// The fallback (espeak-ng's process or the network) starts later, with the first word that
    /// needs it.
    pub fn new(british: bool, espeak: Option<Program>) -> Self {
        let fallback: Box<dyn misaki_rs::Fallback> = match espeak {
            Some(program) => Box::new(EspeakFallback { british, program, child: Mutex::new(None) }),
            None => Box::new(NeuralFallback::new(british)),
        };
        Self::with_fallback(british, fallback)
    }

    fn with_fallback(british: bool, fallback: Box<dyn misaki_rs::Fallback>) -> Self {
        let lang = if british { misaki_rs::Language::EnglishGB } else { misaki_rs::Language::EnglishUS };
        let mut g2p = misaki_rs::G2P::with_fallback(lang, Some(fallback));
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

/// With `SPEAK_DEBUG`, one line per word a fallback read: which one, what it said, how long it took.
fn debug(by: &str, word: &str, ps: &str, since: Instant) {
    if std::env::var_os("SPEAK_DEBUG").is_some() {
        eprintln!("speak: G2P fallback ({by}): {word} -> /{ps}/ ({:.1} ms)", since.elapsed().as_secs_f64() * 1e3);
    }
}

/// misaki's `EspeakFallback` backed by an `espeak-ng` child, started on the first unknown word.
struct EspeakFallback {
    british: bool,
    program: Program,
    child: Mutex<Option<Subprocess>>,
}

impl misaki_rs::Fallback for EspeakFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        let t = Instant::now();
        let fail = |e: anyhow::Error| FallbackError::Espeak { word: word.to_string(), error: format!("{e:#}") };
        let mut child = self.child.lock().map_err(|e| FallbackError::MutexPoisoned(e.to_string()))?;
        if child.is_none() {
            *child = Some(Subprocess::spawn(&self.program, if self.british { "en" } else { "en-us" }, true).map_err(fail)?);
        }
        let ps = espeak::english_word(child.as_mut().unwrap(), word, self.british).map_err(fail)?;
        debug("espeak-ng", word, &ps, t);
        if ps.is_empty() { Err(FallbackError::NoPhonemes { word: word.to_string() }) } else { Ok(ps) }
    }
}

/// misaki's neural network, for when there is no espeak-ng. It is loaded by the first word that
/// needs it, and answers are kept for the life of the process, so a repeated word is decoded once.
struct NeuralFallback {
    british: bool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// `None` until a word needs it; the error if it couldn't be loaded.
    network: Option<Result<Network, String>>,
    known: HashMap<String, String>,
    warned: bool,
}

impl NeuralFallback {
    fn new(british: bool) -> Self {
        Self { british, state: Mutex::new(State::default()) }
    }
}

/// A lowercase word without a vowel letter (`ssh`, `npm`, `http`): an abbreviation, read letter by
/// letter. The network, trained on dictionary words, makes up a word for these ("http" → /t/).
fn is_abbreviation(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase()) && !word.contains(['a', 'e', 'i', 'o', 'u', 'y'])
}

impl misaki_rs::Fallback for NeuralFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        let t = Instant::now();
        let none = || FallbackError::NoPhonemes { word: word.to_string() };
        // The network reads letters. misaki hands it anything (a dash, `#`, an emoji, another
        // script) and gets an invented word back (`-` comes out as "dee"); a token without a
        // single letter is left unread instead, as Kokoro's pipeline does without a fallback.
        if !word.chars().any(|c| c.is_ascii_alphabetic()) {
            return Err(none());
        }
        // Abbreviations, and words too long for the network (misaki fails on those): spelled.
        if is_abbreviation(word) || word.chars().count() > neural::MAX_LETTERS {
            let ps = spell(word, self.british).ok_or_else(none)?;
            debug("spelled", word, &ps, t);
            return Ok(ps);
        }
        let mut state = self.state.lock().map_err(|e| FallbackError::MutexPoisoned(e.to_string()))?;
        if let Some(ps) = state.known.get(word) {
            return Ok(ps.clone());
        }
        let british = self.british;
        let network = state.network.get_or_insert_with(|| Network::load(british).map_err(|e| format!("{e:#}")));
        let result = match network {
            Ok(network) => network.predict(word).map_err(|e| format!("{e:#}")),
            Err(e) => Err(e.clone()),
        };
        match result {
            // misaki's rating doesn't travel through misaki-rs's `Fallback`, and speak has no use for it.
            Ok((ps, _rating)) => {
                debug("network", word, &ps, t);
                state.known.insert(word.to_string(), ps.clone());
                Ok(ps)
            }
            Err(e) => {
                if !std::mem::replace(&mut state.warned, true) {
                    eprintln!("speak: the G2P network failed ({e}); spelling out unknown words like '{word}'");
                }
                spell(word, british).ok_or_else(none)
            }
        }
    }
}

/// `word`'s letters read out the way misaki reads an all-capitals word (`get_NNP`): its own letter
/// names for the dialect, run together, the last one stressed (`ssh` → /ˌɛsˌɛsˈAʧ/).
fn spell(word: &str, british: bool) -> Option<String> {
    // us_gold / gb_gold entries for A–Z.
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
    const GB: [&str; 26] = [
        "ˈA",
        "bˈiː",
        "sˈiː",
        "dˈiː",
        "ˈiː",
        "ˈɛf",
        "ʤˈiː",
        "ˈAʧ",
        "ˈI",
        "ʤˈA",
        "kˈA",
        "ˈɛl",
        "ˈɛm",
        "ˈɛn",
        "ˈQ",
        "pˈiː",
        "kjˈuː",
        "ˈɑː",
        "ˈɛs",
        "tˈiː",
        "jˈuː",
        "vˈiː",
        "dˈʌbᵊljuː",
        "ˈɛks",
        "wˈI",
        "zˈiː",
    ];
    let names = if british { &GB } else { &US };
    let letters: String =
        word.chars().filter(char::is_ascii_alphabetic).map(|c| names[(c.to_ascii_lowercase() as u8 - b'a') as usize]).collect();
    if letters.is_empty() {
        return None;
    }
    // apply_stress(ps, 0) makes every stress secondary; then the last one becomes primary.
    let ps = letters.replace('ˈ', "ˌ");
    let last = ps.rfind('ˌ')?;
    Some(format!("{}ˈ{}", &ps[..last], &ps[last + 'ˌ'.len_utf8()..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use misaki_rs::Fallback;

    #[test]
    fn spells_like_misaki_reads_capitals() {
        assert_eq!(spell("http", false).as_deref(), Some("ˌAʧtˌitˌipˈi"));
        assert_eq!(spell("oz", true).as_deref(), Some("ˌQzˈiː"));
        assert_eq!(spell("–", false), None);
        // The same as misaki's own reading of the word in capitals.
        let en = English::new(false, None);
        assert_eq!(en.phonemize("NPM").unwrap(), spell("npm", false).unwrap());
    }

    #[test]
    fn abbreviations() {
        for word in ["http", "ssh", "npm", "zsh", "scp", "llvm", "pnpm", "grpc"] {
            assert!(is_abbreviation(word), "{word}");
        }
        for word in ["rsync", "nginx", "kubectl", "Npm", "HTTP", "k8s", "co-op", ""] {
            assert!(!is_abbreviation(word), "{word}");
        }
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

    #[test]
    fn without_espeak_unknown_words_go_to_the_network() {
        let en = English::new(false, None);
        // Neither word is in misaki's lexicons; the network's flap (ɾ) becomes misaki's T.
        assert_eq!(en.phonemize("kubernetes and pytorch").unwrap(), "kˌubəɹnˈits ænd pˈITəɹk");
        // An abbreviation is spelled, like espeak-ng does.
        assert_eq!(en.phonemize("over ssh").unwrap(), "ˈOvəɹ ˌɛsˌɛsˈAʧ");
    }

    #[test]
    fn what_the_network_cant_take() {
        let fallback = NeuralFallback::new(false);
        // No letter at all: left unread, where misaki's network would invent a word.
        assert!(matches!(fallback.phonemize("-"), Err(FallbackError::NoPhonemes { .. })));
        assert!(matches!(fallback.phonemize("日本"), Err(FallbackError::NoPhonemes { .. })));
        // Longer than the network's 62 letters: spelled.
        let long = "ab".repeat(32);
        assert_eq!(fallback.phonemize(&long).unwrap(), spell(&long, false).unwrap());
        // Characters outside its alphabet are `<unk>` to it, as in misaki.
        assert!(!fallback.phonemize("naïve").unwrap().is_empty());
        // Answers are kept.
        let ps = fallback.phonemize("nginx").unwrap();
        assert_eq!(fallback.state.lock().unwrap().known["nginx"], ps);
    }

    /// Notes every word misaki hands its fallback, and reads none of them.
    struct Recorder(std::sync::Arc<Mutex<Vec<String>>>);

    impl Fallback for Recorder {
        fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
            self.0.lock().unwrap().push(word.to_string());
            Err(FallbackError::NoPhonemes { word: word.to_string() })
        }
    }

    /// `word` the way speak reads it: through the built-in lexicon, then misaki.
    fn builtin_reading(en: &English, word: &str) -> String {
        en.phonemize(&crate::lexicon::to_markup(&crate::lexicon::builtin().apply(word))).unwrap()
    }

    #[test]
    fn builtin_entries_need_no_fallback() {
        // misaki's dictionaries read every built-in spoken form by themselves, so it sounds the
        // same with or without espeak-ng, in either accent.
        for british in [false, true] {
            let asked = std::sync::Arc::new(Mutex::new(Vec::new()));
            let en = English::with_fallback(british, Box::new(Recorder(asked.clone())));
            for (word, spoken) in crate::lexicon::builtin().sorted() {
                let ps = builtin_reading(&en, word);
                let asked = std::mem::take(&mut *asked.lock().unwrap());
                let dialect = if british { "GB" } else { "US" };
                assert!(asked.is_empty(), "{word} = {spoken} ({dialect}): misaki's dictionaries lack {asked:?}");
                assert!(!ps.trim().is_empty(), "{word} = {spoken} ({dialect}) reads as nothing");
            }
        }
    }

    #[test]
    fn builtin_readings() {
        let none = || Box::new(Recorder(Default::default()));
        let (us, gb) = (English::with_fallback(false, none()), English::with_fallback(true, none()));
        // (text, US, GB or "" when it is the same as US)
        let cases = [
            ("redis", "ɹˈɛdɪs", ""),
            ("JSON", "ʤˈAsᵊn", ""),
            ("YAML", "jˈæmˌɛl", "jˈamˌɛl"),
            ("kubectl", "kjˈub kəntɹˈOl", "kjˈuːb kəntɹˈQl"),
            ("stdout", "stˈændəɹd ˈWt", "stˈandəd ˈWt"),
            ("enum", "ˈinˌʌm", "ˈiːnˌʌm"),
            ("Deno", "dˈinˌO", "dˈiːnˌQ"),
            ("vite", "vˈit", ""),
            ("numpy", "nˈʌm pˈI", ""),
            ("url", "jˌuˌɑɹˈɛl", "jˌuːˌɑːˈɛl"),
            ("wifi", "wˈIfˌI", ""),
            ("nginx", "ˈɛnʤən ˈɛks", "ˈɛnʤɪn ˈɛks"),
            ("wget", "dˈʌbᵊlju ɡˈɛt", "dˈʌbᵊljuː ɡˈɛt"),
            ("Postgres", "pˈOst ɡɹɛs", "pˈQst ɡɹɛs"),
            ("AWS", "ˌAdˌʌbᵊljuˈɛs", "ˌAdˌʌbᵊljuːˈɛs"),
            ("sudo", "sˈu dˈu", "sˈuː dˈuː"),
            ("C#", "sˈi ʃˈɑɹp", "sˈiː ʃˈɑːp"),
            ("async", "ˈAsˌɪŋk", ""),
            ("TODO", "tə dˈu", "tə dˈuː"),
        ];
        for (text, want_us, want_gb) in cases {
            assert_eq!(builtin_reading(&us, text), want_us, "{text} (US)");
            assert_eq!(builtin_reading(&gb, text), if want_gb.is_empty() { want_us } else { want_gb }, "{text} (GB)");
        }
    }
}
