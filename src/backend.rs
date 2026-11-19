//! The seam that keeps speak model-agnostic.
//!
//! The front end (CLI, lexicon, sentence chunking, playback, file output, voice picker) only talks
//! to `dyn Backend`. A backend owns its runtime, sample rate, voices and whether it reads phonemes.
//! Adding a model means a module, a `models::CATALOG` entry and an arm in `open`.

use std::path::PathBuf;

use anyhow::Result;

use crate::g2p::Mode;
use crate::models::Model;

/// Mono f32 PCM at `Info::sample_rate`.
pub struct Chunk {
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub id: String,
    /// Display name of the language, used to group voices.
    pub language: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Info {
    pub sample_rate: u32,
}

/// Where the time went, for `--timing`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Time spent waiting for the model to finish loading.
    pub load_wait_ms: f64,
    /// Total model inference time.
    pub infer_ms: f64,
}

pub type ChunkIter<'a> = Box<dyn Iterator<Item = Result<Chunk>> + 'a>;

pub trait Backend: Send {
    fn info(&self) -> Info;
    fn voices(&self) -> Result<Vec<Voice>>;
    /// Synthesise one sentence. `text` has the lexicon applied; phoneme entries arrive as
    /// `[word](/phonemes/)` markup (a backend without phoneme input would speak the word).
    /// An iterator so a backend can yield audio before the whole sentence is done.
    fn synthesize<'a>(&'a mut self, text: &str, voice: &str, speed: f32) -> Result<ChunkIter<'a>>;
    fn stats(&self) -> Stats {
        Stats::default()
    }
}

/// Model files (or espeak-ng, where it is required) are missing: exit code 3.
#[derive(Debug)]
pub struct Missing(pub String);

/// The model is present but can't be used (failed checksum, failed to load): exit code 3.
#[derive(Debug)]
pub struct Unusable(pub String);

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Missing {}

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Unusable {}

/// Paths of an installed model's files.
pub struct Files {
    pub model: PathBuf,
    pub voices: PathBuf,
}

pub fn open(model: &Model, files: Files, voice: &str, mode: Mode) -> Result<Box<dyn Backend>> {
    match model.backend {
        "kokoro" => Ok(Box::new(crate::kokoro::Kokoro::open(files, voice, mode)?)),
        other => anyhow::bail!("unknown backend '{other}'"),
    }
}
