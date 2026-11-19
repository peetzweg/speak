//! Kokoro-82M (v1.0 ONNX export) backend, matching kokoro-onnx 0.4.7's `Kokoro.create`.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use ort::session::Session;
use ort::session::builder::AutoDevicePolicy;
use ort::value::Tensor;

use crate::backend::{Asset, Backend, Chunk, ChunkIter, Info, KokoroPaths, Voice};
use crate::g2p::{self, Engine, py_strip};
use crate::{npz, trim, vocab};

pub const SAMPLE_RATE: u32 = 24_000;
const MAX_PHONEME_LENGTH: usize = 510;

pub struct Kokoro {
    paths: KokoroPaths,
    session: Option<Session>,
    styles: HashMap<String, Vec<f32>>, // voice -> (510 * 256) floats
    vocab: HashMap<char, i64>,
    g2p: Option<(String, Box<dyn Engine>)>, // (espeak language, engine)
}

/// Kokoro voice id prefix -> espeak language (as in speak.py).
pub fn lang_for(voice: &str) -> &'static str {
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
        Some(b'z') => "Mandarin",
        _ => "?",
    }
}

/// What kokoro-onnx feeds the model for `text`: the phoneme string, its batches, and the
/// zero-padded token ids per batch.
pub struct Analysis {
    pub phonemes: String,
    pub batches: Vec<String>,
    pub tokens: Vec<Vec<i64>>,
}

impl Kokoro {
    pub fn open(paths: &KokoroPaths) -> Result<Self> {
        for p in [&paths.model, &paths.voices] {
            if !p.exists() {
                return Err(crate::MissingAsset(p.display().to_string()).into());
            }
        }
        Ok(Self {
            paths: KokoroPaths { model: paths.model.clone(), voices: paths.voices.clone(), espeak_data: paths.espeak_data.clone() },
            session: None,
            styles: HashMap::new(),
            vocab: vocab::VOCAB.iter().copied().collect(),
            g2p: None,
        })
    }

    fn session(&mut self) -> Result<&mut Session> {
        if self.session.is_none() {
            let t = std::time::Instant::now();
            let env = ort::init().with_name("speak").build().map_err(|e| anyhow::anyhow!("{e}"))?;
            // ort's default policy (MaxEfficiency) silently registers CoreML on macOS: ~2.7 s extra load
            // here and different numerics from kokoro-onnx. Use the CPU EP like the Python reference.
            let policy = match std::env::var("SPEAK_EP").as_deref() {
                Ok("auto") => AutoDevicePolicy::MaxEfficiency,
                _ => AutoDevicePolicy::PreferCPU,
            };
            let s = Session::builder(&env)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .with_auto_device(policy)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .commit_from_file(&self.paths.model)
                .map_err(|e| anyhow::anyhow!("loading {}: {e}", self.paths.model.display()))?;
            self.session = Some(s);
            if std::env::var_os("SPEAK_DEBUG").is_some() {
                eprintln!("session load {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
            }
        }
        Ok(self.session.as_mut().unwrap())
    }

    fn style(&mut self, voice: &str, n_tokens: usize) -> Result<Vec<f32>> {
        if !self.styles.contains_key(voice) {
            let (shape, data) = npz::read(&self.paths.voices, voice)?;
            if shape.len() != 3 || shape[2] != 256 {
                bail!("unexpected voice shape {shape:?}");
            }
            self.styles.insert(voice.to_string(), data);
        }
        let d = &self.styles[voice];
        let row = n_tokens.min(d.len() / 256 - 1);
        Ok(d[row * 256..(row + 1) * 256].to_vec())
    }

    /// The espeak engine for `lang`: the `espeak-ng` subprocess by default, or libespeak-ng
    /// in-process with SPEAK_G2P=inprocess when built with `--features espeak-inprocess`.
    fn engine(&mut self, lang: &str) -> Result<&mut dyn Engine> {
        if self.g2p.as_ref().map(|(l, _)| l.as_str()) != Some(lang) {
            self.g2p = None; // stop the old child first
            let data = std::env::var_os("SPEAK_ESPEAK_DATA").map(std::path::PathBuf::from);
            let engine: Box<dyn Engine> = match std::env::var("SPEAK_G2P").as_deref() {
                #[cfg(feature = "espeak-inprocess")]
                Ok("inprocess") => Box::new(g2p::inprocess::InProcess::new(data.as_deref().unwrap_or(&self.paths.espeak_data), lang)?),
                #[cfg(not(feature = "espeak-inprocess"))]
                Ok("inprocess") => bail!("this build has no in-process espeak (cargo feature espeak-inprocess)"),
                _ => {
                    let prog = g2p::find_program().ok_or_else(|| crate::MissingAsset("espeak-ng program (install espeak-ng or set SPEAK_ESPEAK)".into()))?;
                    Box::new(g2p::Subprocess::spawn(&prog, lang, data.as_deref())?)
                }
            };
            self.g2p = Some((lang.to_string(), engine));
        }
        Ok(self.g2p.as_mut().unwrap().1.as_mut())
    }

    pub fn analyze(&mut self, text: &str, lang: &str) -> Result<Analysis> {
        let vocab = self.vocab.clone();
        let phonemes = g2p::phonemize(self.engine(lang)?, text, |c| vocab.contains_key(&c))?;
        let batches = split_phonemes(&phonemes);
        let tokens = batches
            .iter()
            .map(|b| {
                let ids = b.chars().take(MAX_PHONEME_LENGTH).filter_map(|c| self.vocab.get(&c).copied());
                std::iter::once(0).chain(ids).chain(std::iter::once(0)).collect()
            })
            .collect();
        Ok(Analysis { phonemes, batches, tokens })
    }

    fn infer(&mut self, tokens: &[i64], voice: &str, speed: f32) -> Result<Vec<f32>> {
        let style = self.style(voice, tokens.len() - 2)?;
        let t = Tensor::from_array(([1usize, tokens.len()], tokens.to_vec()))?;
        let s = Tensor::from_array(([1usize, 256], style))?;
        let v = Tensor::from_array(([1usize], vec![speed]))?;
        let sess = self.session()?;
        let t0 = std::time::Instant::now();
        let out = sess.run(ort::inputs!["tokens" => t, "style" => s, "speed" => v])?;
        if std::env::var_os("SPEAK_DEBUG").is_some() {
            eprintln!("run {} tokens {:.0} ms", tokens.len(), t0.elapsed().as_secs_f64() * 1e3);
        }
        let (_, audio) = out["audio"].try_extract_tensor::<f32>()?;
        Ok(trim::trim(audio).to_vec())
    }
}

/// `Kokoro._split_phonemes`: batches of < 510 chars, preferring punctuation boundaries.
pub fn split_phonemes(phonemes: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut last = 0;
    for (i, c) in phonemes.char_indices() {
        if ".,!?;".contains(c) {
            parts.push(&phonemes[last..i]);
            parts.push(&phonemes[i..i + c.len_utf8()]);
            last = i + c.len_utf8();
        }
    }
    parts.push(&phonemes[last..]);
    let mut batches = Vec::new();
    let mut current = String::new();
    for part in parts {
        let part = py_strip(part);
        if part.is_empty() {
            continue;
        }
        if current.chars().count() + part.chars().count() + 1 >= MAX_PHONEME_LENGTH {
            batches.push(py_strip(&current).to_string());
            current = part.to_string();
        } else if ".,!?;".contains(part) {
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

impl Backend for Kokoro {
    fn info(&self) -> Info {
        Info {
            id: "kokoro",
            model: "Kokoro-82M v1.0 (ONNX)".into(),
            sample_rate: SAMPLE_RATE,
            default_voice: "af_heart",
            assets: vec![
                Asset { path: "models/kokoro-v1.0.onnx", url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx", sha256: "TODO" },
                Asset { path: "models/voices-v1.0.bin", url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin", sha256: "TODO" },
            ],
        }
    }

    fn voices(&self) -> Result<Vec<Voice>> {
        Ok(npz::list(&self.paths.voices)?
            .into_iter()
            .map(|id| {
                let g = match id.as_bytes().get(1) {
                    Some(b'f') => "female",
                    Some(b'm') => "male",
                    _ => "",
                };
                Voice { label: format!("{}, {g}", language_name(&id)), language: lang_for(&id).into(), id }
            })
            .collect())
    }

    fn synthesize<'a>(&'a mut self, text: &str, voice: &str, speed: f32) -> Result<ChunkIter<'a>> {
        if !(0.5..=2.0).contains(&speed) {
            bail!("speed must be between 0.5 and 2.0");
        }
        let a = self.analyze(text, lang_for(voice)).context("phonemizing")?;
        let voice = voice.to_string();
        let mut batches = a.tokens.into_iter();
        Ok(Box::new(std::iter::from_fn(move || {
            let tokens = batches.next()?;
            Some(self.infer(&tokens, &voice, speed).map(|samples| Chunk { samples }))
        })))
    }
}
