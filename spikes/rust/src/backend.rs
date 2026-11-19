//! The seam that keeps `speak` model-agnostic.
//!
//! The front end (CLI, lexicon, sentence splitting, playback, WAV/file output, voice picker)
//! only talks to `dyn Backend`. A backend owns its runtime (ONNX today; candle/MLX/llama.cpp or a
//! local HTTP server later), its sample rate, its voice list and whether it needs phonemes at all.
//! Shared helpers such as `g2p::Espeak` are a library a backend may use, not part of the trait.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// A piece of mono f32 PCM at `Info::sample_rate`.
pub struct Chunk {
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // language: for the picker's filter later
pub struct Voice {
    pub id: String,
    /// BCP-47-ish language tag, e.g. "en-us".
    pub language: String,
    pub label: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // id/model/assets: for `speak --about` and the first-run downloader (not built)
pub struct Info {
    pub id: &'static str,
    pub model: String,
    pub sample_rate: u32,
    pub default_voice: &'static str,
    /// Files the backend needs under SPEAK_HOME (for first-run download + checksum).
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Asset {
    pub path: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

pub type ChunkIter<'a> = Box<dyn Iterator<Item = Result<Chunk>> + 'a>;

pub trait Backend: Send {
    fn info(&self) -> Info;
    fn voices(&self) -> Result<Vec<Voice>>;
    /// Synthesise one utterance (already lexicon-substituted and sentence-split by the front end).
    /// An iterator so a backend that can stream inside an utterance yields audio early; Kokoro
    /// yields one chunk per internal phoneme batch.
    fn synthesize<'a>(&'a mut self, text: &str, voice: &str, speed: f32) -> Result<ChunkIter<'a>>;
}

/// Backend registry. Adding a model = one module + one arm here; the newest good one becomes
/// the default by changing `DEFAULT`.
pub const DEFAULT: &str = "kokoro";

pub fn open(name: &str, home: &Path) -> Result<Box<dyn Backend>> {
    match name {
        "kokoro" => Ok(Box::new(crate::kokoro::Kokoro::open(&KokoroPaths::under(home))?)),
        // "kitten" => Ok(Box::new(crate::kitten::Kitten::open(home)?)),   // ONNX + same espeak G2P
        // "piper"  => Ok(Box::new(crate::piper::Piper::open(home)?)),     // ONNX VITS, own phoneme map, 22.05 kHz
        // "http"   => Ok(Box::new(crate::http::Server::connect(url)?)),   // any local TTS server
        other => bail!("unknown backend '{other}' (available: kokoro)"),
    }
}

pub struct KokoroPaths {
    pub model: PathBuf,
    pub voices: PathBuf,
    pub espeak_data: PathBuf,
}

impl KokoroPaths {
    pub fn under(home: &Path) -> Self {
        Self {
            model: home.join("models/kokoro-v1.0.onnx"),
            voices: home.join("models/voices-v1.0.bin"),
            espeak_data: home.join("es/espeak-ng-data"),
        }
    }
}
