//! Grapheme-to-phoneme: text (with the lexicon's `[word](/phonemes/)` markup) → the phoneme
//! string a phoneme-reading model such as Kokoro takes.
//!
//! * English voices: misaki (with the user's phoneme entries). Words its lexicons don't know go to
//!   `espeak-ng` when it is installed, else to misaki's neural network: English needs nothing else.
//! * Other languages: `espeak-ng` for everything, converted the way Kokoro's pipeline does.
//! * `Mode::Espeak` (hidden `--g2p espeak`): kokoro-onnx 0.4.7's exact output, for regression tests.

pub mod english;
pub mod espeak;
pub mod neural;

use anyhow::Result;

use crate::deferred::Deferred;
use crate::lexicon::{self, Segment};
use english::English;
use espeak::{Program, Subprocess};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Espeak,
}

/// Kokoro voice id → espeak-ng language, from the id's first letter (`af_heart` → `en-us`).
pub fn language(voice: &str) -> &'static str {
    match voice.as_bytes().first() {
        Some(b'b') => "en-gb",
        Some(b'e') => "es",
        Some(b'f') => "fr-fr",
        Some(b'h') => "hi",
        Some(b'i') => "it",
        Some(b'j') => "ja",
        Some(b'p') => "pt-br",
        Some(b'z') => "cmn",
        _ => "en-us",
    }
}

/// Does `voice` need espeak-ng in this mode? Only misaki-English can do without it.
pub fn needs_espeak(mode: Mode, voice: &str) -> bool {
    mode == Mode::Espeak || !language(voice).starts_with("en")
}

pub struct G2p {
    mode: Mode,
    program: Option<Program>,
    us: Option<Deferred<English>>,
    gb: Option<Deferred<English>>,
    /// One `espeak-ng` child per (language, tied output).
    children: Vec<(&'static str, bool, Subprocess)>,
}

impl G2p {
    pub fn new(mode: Mode) -> Self {
        Self { mode, program: espeak::find_program(), us: None, gb: None, children: Vec::new() }
    }

    /// The espeak-ng this G2P will use, if any.
    pub fn espeak(&self) -> Option<&Program> {
        self.program.as_ref()
    }

    /// Start building what `voice` needs on a background thread.
    pub fn prepare(&mut self, voice: &str) {
        if self.mode == Mode::Auto {
            let _ = self.english(language(voice) == "en-gb", false);
        }
    }

    fn english(&mut self, british: bool, wait: bool) -> Option<&mut English> {
        let slot = if british { &mut self.gb } else { &mut self.us };
        let program = self.program.clone();
        let deferred = slot.get_or_insert_with(|| Deferred::spawn(move || English::new(british, program)));
        wait.then(|| deferred.get())
    }

    fn child(&mut self, lang: &'static str, tie: bool) -> Result<&mut Subprocess> {
        let i = match self.children.iter().position(|(l, t, _)| *l == lang && *t == tie) {
            Some(i) => i,
            None => {
                let program = self
                    .program
                    .as_ref()
                    .ok_or_else(|| crate::backend::Missing("espeak-ng not found (install it or set SPEAK_ESPEAK)".into()))?;
                self.children.push((lang, tie, Subprocess::spawn(program, lang, tie)?));
                self.children.len() - 1
            }
        };
        Ok(&mut self.children[i].2)
    }

    /// Phonemes for `text` spoken by `voice`. Not yet filtered to a model's vocabulary.
    pub fn phonemize(&mut self, text: &str, voice: &str) -> Result<String> {
        let lang = language(voice);
        match self.mode {
            Mode::Auto if lang.starts_with("en") => self.english(lang == "en-gb", true).unwrap().phonemize(text),
            Mode::Auto => self.spliced(text, |g2p, t| espeak::misaki_g2p(g2p.child(lang, true)?, t)),
            Mode::Espeak => self.spliced(text, |g2p, t| espeak::kokoro_onnx(g2p.child(lang, false)?, t)),
        }
    }

    /// Phonemize the text between lexicon phoneme entries with `f`; splice the entries in as-is.
    fn spliced(&mut self, text: &str, mut f: impl FnMut(&mut Self, &str) -> Result<String>) -> Result<String> {
        let segments = lexicon::from_markup(text);
        if let [Segment::Text(t)] = segments.as_slice() {
            return f(self, t);
        }
        let mut parts = Vec::with_capacity(segments.len());
        for seg in segments {
            match seg {
                Segment::Text(t) if espeak::py_strip(&t).is_empty() => {}
                Segment::Text(t) => parts.push(espeak::py_strip(&f(self, &t)?).to_string()),
                Segment::Phonemes { phonemes, .. } => parts.push(phonemes),
            }
        }
        Ok(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_languages() {
        assert_eq!(language("af_heart"), "en-us");
        assert_eq!(language("bm_george"), "en-gb");
        assert_eq!(language("ff_siwis"), "fr-fr");
        assert_eq!(language("pm_alex"), "pt-br");
        assert_eq!(language("zf_xiaobei"), "cmn");
    }
}
