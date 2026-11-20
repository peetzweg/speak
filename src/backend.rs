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
    /// For [`Backend::synthesize_parts`]: where each part after the first begins, as offsets into
    /// `samples` (0 when a part begins with this chunk).
    pub marks: Vec<usize>,
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
    /// Synthesise one sentence given as consecutive parts (subtitle cues), so the caller learns
    /// where each part begins: the chunks carry exactly `parts.len() - 1` marks between them. A
    /// backend that can find the boundaries inside one utterance (Kokoro, from its predicted
    /// durations) keeps the sentence's prosody; this default speaks the parts one by one.
    fn synthesize_parts<'a>(&'a mut self, parts: &[String], voice: &str, speed: f32) -> Result<ChunkIter<'a>> {
        one_by_one(self, parts, voice, speed)
    }
    fn stats(&self) -> Stats {
        Stats::default()
    }
}

/// [`Backend::synthesize_parts`] for a backend that can't see inside an utterance: each part is
/// its own utterance.
pub fn one_by_one<B: Backend + ?Sized>(be: &mut B, parts: &[String], voice: &str, speed: f32) -> Result<ChunkIter<'static>> {
    let mut chunks = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let mut marked = i == 0;
        for chunk in be.synthesize(part, voice, speed)? {
            let mut chunk = chunk?;
            if !marked {
                chunk.marks.insert(0, 0);
                marked = true;
            }
            chunks.push(Ok(chunk));
        }
        if !marked {
            chunks.push(Ok(Chunk { samples: Vec::new(), marks: vec![0] }));
        }
    }
    Ok(Box::new(chunks.into_iter()))
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

/// `marks`: the caller will use [`Backend::synthesize_parts`] and wants boundaries inside
/// sentences (subtitles), which may cost a backend extra set-up.
pub fn open(model: &Model, files: Files, voice: &str, mode: Mode, marks: bool) -> Result<Box<dyn Backend>> {
    match model.backend {
        "kokoro" => Ok(Box::new(crate::kokoro::Kokoro::open(files, voice, mode, marks)?)),
        other => anyhow::bail!("unknown backend '{other}'"),
    }
}
