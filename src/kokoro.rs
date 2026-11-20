//! Kokoro-82M (v1.0, ONNX), run like kokoro-onnx 0.4.7's `Kokoro.create`: phonemes → batches of
//! at most 510 → token ids → style row picked by length → ONNX Runtime → silence trim.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use ort::session::Session;
use ort::value::Tensor;

use crate::backend::{Backend, Chunk, ChunkIter, Files, Info, Missing, Stats, Unusable, Voice};
use crate::deferred::Deferred;
use crate::g2p::{G2p, Mode, espeak::py_strip};
use crate::{npz, trim, vocab};

pub const SAMPLE_RATE: u32 = 24_000;
pub const DEFAULT_VOICE: &str = "af_heart";
const MAX_PHONEMES: usize = 510;

pub struct Kokoro {
    voices: PathBuf,
    session: Deferred<Result<Session, String>>,
    styles: HashMap<String, Vec<f32>>,
    g2p: G2p,
    stats: Stats,
}

/// What the model is fed for `text`: the phoneme string, its batches and their token ids
/// (each wrapped in the pad id 0).
pub struct Analysis {
    pub phonemes: String,
    pub batches: Vec<String>,
    pub tokens: Vec<Vec<i64>>,
}

/// Phonemes, batches and tokens for `text`. Needs no model files, only G2P.
pub fn analyze(g2p: &mut G2p, text: &str, voice: &str) -> Result<Analysis> {
    let raw = g2p.phonemize(text, voice)?;
    let kept: String = raw.chars().filter(|&c| vocab::id(c).is_some()).collect();
    let phonemes = py_strip(&kept).to_string();
    let batches = split_phonemes(&phonemes);
    let tokens = batches
        .iter()
        .map(|b| {
            let ids = b.chars().take(MAX_PHONEMES).filter_map(vocab::id);
            std::iter::once(0).chain(ids).chain(std::iter::once(0)).collect()
        })
        .collect();
    Ok(Analysis { phonemes, batches, tokens })
}

/// kokoro-onnx's `_split_phonemes`: batches shorter than 510 characters, cut after `.,!?;`.
pub fn split_phonemes(phonemes: &str) -> Vec<String> {
    const CUT: &str = ".,!?;";
    let mut parts = Vec::new();
    let mut last = 0;
    for (i, c) in phonemes.char_indices() {
        if CUT.contains(c) {
            parts.push(&phonemes[last..i]);
            parts.push(&phonemes[i..i + c.len_utf8()]);
            last = i + c.len_utf8();
        }
    }
    parts.push(&phonemes[last..]);
    let mut batches = Vec::new();
    let mut current = String::new();
    for part in parts.into_iter().map(py_strip).filter(|p| !p.is_empty()) {
        if current.chars().count() + part.chars().count() + 1 >= MAX_PHONEMES {
            batches.push(py_strip(&current).to_string());
            current = part.to_string();
        } else if CUT.contains(part) {
            current.push_str(part);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(part);
        }
    }
    if !current.is_empty() {
        batches.push(py_strip(&current).to_string());
    }
    batches
}

fn load_session(path: &Path) -> Result<Session, String> {
    let t = Instant::now();
    // The CPU provider, which is what kokoro-onnx (the reference) uses.
    let session = crate::onnx::cpu_session(0)
        .and_then(|mut builder| Ok(builder.commit_from_file(path)?))
        .map_err(|e| format!("loading {}: {e:#}", path.display()))?;
    if std::env::var_os("SPEAK_DEBUG").is_some() {
        eprintln!("speak: model loaded in {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    Ok(session)
}

fn language_name(voice: &str) -> &'static str {
    match voice.as_bytes().first() {
        Some(b'a') => "American English",
        Some(b'b') => "British English",
        Some(b'e') => "Spanish",
        Some(b'f') => "French",
        Some(b'h') => "Hindi",
        Some(b'i') => "Italian",
        Some(b'j') => "Japanese",
        Some(b'p') => "Brazilian Portuguese",
        Some(b'z') => "Mandarin Chinese",
        _ => "Other",
    }
}

impl Kokoro {
    /// Starts loading the model and the G2P that `voice` needs in the background.
    pub fn open(files: Files, voice: &str, mode: Mode) -> Result<Self> {
        for p in [&files.model, &files.voices] {
            if !p.is_file() {
                return Err(Missing(format!("{} not found", p.display())).into());
            }
        }
        let model = files.model.clone();
        let mut g2p = G2p::new(mode);
        g2p.prepare(voice);
        Ok(Self {
            voices: files.voices,
            session: Deferred::spawn(move || load_session(&model)),
            styles: HashMap::new(),
            g2p,
            stats: Stats::default(),
        })
    }

    fn session(&mut self) -> Result<&mut Session> {
        let t = Instant::now();
        let s = self.session.get();
        self.stats.load_wait_ms += t.elapsed().as_secs_f64() * 1e3;
        s.as_mut().map_err(|e| Unusable(e.clone()).into())
    }

    /// The style vector for `voice` at `n_tokens` phonemes (row n of the 510×256 table).
    fn style(&mut self, voice: &str, n_tokens: usize) -> Result<Vec<f32>> {
        if !self.styles.contains_key(voice) {
            let (shape, data) = npz::read(&self.voices, voice)?;
            if shape.len() != 3 || shape[2] != 256 {
                bail!("unexpected shape {shape:?} for voice {voice}");
            }
            self.styles.insert(voice.to_string(), data);
        }
        let table = &self.styles[voice];
        let row = n_tokens.min(table.len() / 256 - 1);
        Ok(table[row * 256..(row + 1) * 256].to_vec())
    }

    fn infer(&mut self, tokens: &[i64], voice: &str, speed: f32) -> Result<Vec<f32>> {
        let style = self.style(voice, tokens.len() - 2)?;
        let inputs = ort::inputs![
            "tokens" => Tensor::from_array(([1usize, tokens.len()], tokens.to_vec()))?,
            "style" => Tensor::from_array(([1usize, 256], style))?,
            "speed" => Tensor::from_array(([1usize], vec![speed]))?,
        ];
        let session = self.session()?;
        let t = Instant::now();
        let out = session.run(inputs)?;
        let (_, audio) = out["audio"].try_extract_tensor::<f32>()?;
        let samples = trim::trim(audio).to_vec();
        drop(out);
        self.stats.infer_ms += t.elapsed().as_secs_f64() * 1e3;
        Ok(samples)
    }
}

impl Backend for Kokoro {
    fn info(&self) -> Info {
        Info { sample_rate: SAMPLE_RATE }
    }

    fn voices(&self) -> Result<Vec<Voice>> {
        let mut voices: Vec<Voice> = npz::list(&self.voices)?
            .into_iter()
            .map(|id| {
                let gender = match id.as_bytes().get(1) {
                    Some(b'f') => "female",
                    Some(b'm') => "male",
                    _ => "",
                };
                let language = language_name(&id).to_string();
                Voice { label: format!("{language}, {gender}"), language, id }
            })
            .collect();
        // Grouped by language in Kokoro's order (English first), then by name.
        voices.sort_by_key(|v| ("abefhijpz".find(v.id.chars().next().unwrap_or('~')).unwrap_or(99), v.id.clone()));
        Ok(voices)
    }

    fn synthesize<'a>(&'a mut self, text: &str, voice: &str, speed: f32) -> Result<ChunkIter<'a>> {
        if !(0.5..=2.0).contains(&speed) {
            bail!("speed must be between 0.5 and 2.0");
        }
        let a = analyze(&mut self.g2p, text, voice).context("phonemizing")?;
        let voice = voice.to_string();
        let mut batches = a.tokens.into_iter().filter(|t| t.len() > 2);
        Ok(Box::new(std::iter::from_fn(move || {
            let tokens = batches.next()?;
            Some(self.infer(&tokens, &voice, speed).map(|samples| Chunk { samples }))
        })))
    }

    fn stats(&self) -> Stats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_cut_at_punctuation_under_510() {
        assert_eq!(split_phonemes("həlˈO, wˈɜɹld. ðɪs ɪz"), vec!["həlˈO, wˈɜɹld. ðɪs ɪz"]);
        let clause = format!("{},", "a".repeat(300));
        let b = split_phonemes(&format!("{clause} {clause} end."));
        assert_eq!(b, vec![clause.clone(), format!("{clause} end.")]);
        assert!(split_phonemes("").is_empty());
    }
}
