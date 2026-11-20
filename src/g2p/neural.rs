//! misaki's neural fallback for English words its lexicons don't know (`FallbackNetwork` in
//! misaki/en.py, what misaki uses when no other fallback is given).
//!
//! One small BART per dialect (PeterReid/graphemes_to_phonemes_en_us and _en_gb, Apache-2.0, 0.76M
//! parameters each), exported to ONNX by `scripts/export-g2p.py` and embedded in the binary. The
//! ONNX graph is one decoding step without a cache; the greedy loop around it reproduces what
//! misaki gets from transformers' `generate()` with the models' generation config. The
//! vocabularies come from the ONNX metadata, where the export put the checkpoint's own.
//! `tests/g2p/` checks the port against misaki's Python output.

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use ort::session::Session;
use ort::value::Tensor;

static EN_US: &[u8] = include_bytes!("../../assets/g2p/en_us.onnx");
static EN_GB: &[u8] = include_bytes!("../../assets/g2p/en_gb.onnx");

const BOS: i64 = 1;
const EOS: i64 = 2;
const UNK: i64 = 3;
/// misaki calls `generate()` without a length, so transformers' default applies: 20 new tokens,
/// the last of them forced to `</s>` (the models set `forced_eos_token_id`). A pronunciation is
/// cut after 19 phonemes, 18 when the model starts with `<s>` (as it usually does), as in misaki.
const MAX_NEW_TOKENS: usize = 20;
/// The models' `max_position_embeddings`: `<s>` + 62 letters + `</s>`. misaki fails on longer
/// words (an IndexError in the position embedding).
pub const MAX_LETTERS: usize = 62;

pub struct Network {
    session: Session,
    graphemes: HashMap<char, i64>,
    phonemes: Vec<char>,
}

impl Network {
    /// Builds the ONNX Runtime session from the embedded model: a few milliseconds.
    pub fn load(british: bool) -> Result<Self> {
        let t = Instant::now();
        let model = if british { EN_GB } else { EN_US };
        let session = crate::onnx::cpu_session(1)?.commit_from_memory(model).context("loading the G2P network")?;
        let meta = session.metadata()?;
        let vocab = |key: &str| meta.custom(key).with_context(|| format!("the G2P network has no {key}"));
        // misaki: `{g: i for i, g in enumerate(grapheme_chars)}`, so for a repeated character
        // (the four `_` placeholders of the special tokens) the last index wins.
        let graphemes = vocab("grapheme_chars")?.chars().zip(0..).collect();
        let phonemes = vocab("phoneme_chars")?.chars().collect();
        let positions: usize = vocab("max_position_embeddings")?.parse()?;
        drop(meta);
        if positions != MAX_LETTERS + 2 {
            bail!("the G2P network takes {positions} positions, expected {}", MAX_LETTERS + 2);
        }
        if std::env::var_os("SPEAK_DEBUG").is_some() {
            eprintln!(
                "speak: G2P network ({}) loaded in {:.1} ms",
                if british { "en-gb" } else { "en-us" },
                t.elapsed().as_secs_f64() * 1e3
            );
        }
        Ok(Self { session, graphemes, phonemes })
    }

    /// misaki's `graphemes_to_tokens`: `<s>`, one token per character (`<unk>` for characters the
    /// model doesn't know), `</s>`.
    fn tokens(&self, word: &str) -> Vec<i64> {
        let ids = word.chars().map(|c| self.graphemes.get(&c).copied().unwrap_or(UNK));
        std::iter::once(BOS).chain(ids).chain(std::iter::once(EOS)).collect()
    }

    /// misaki's `tokens_to_phonemes`: special tokens dropped, ids without a phoneme ignored.
    fn phonemes(&self, tokens: &[i64]) -> String {
        tokens.iter().filter(|&&t| t > UNK).filter_map(|&t| self.phonemes.get(t as usize)).collect()
    }

    /// The most likely token after `decoded` (one step of greedy search).
    fn step(&mut self, input: &[i64], decoded: &[i64]) -> Result<i64> {
        let inputs = ort::inputs![
            "input_ids" => Tensor::from_array(([1usize, input.len()], input.to_vec()))?,
            "decoder_input_ids" => Tensor::from_array(([1usize, decoded.len()], decoded.to_vec()))?,
        ];
        let out = self.session.run(inputs)?;
        let (_, logits) = out["logits"].try_extract_tensor::<f32>()?;
        // torch.argmax: the first of equal maxima.
        let (best, _) = logits.iter().enumerate().fold((0, f32::NEG_INFINITY), |(bi, bv), (i, &v)| if v > bv { (i, v) } else { (bi, bv) });
        Ok(best as i64)
    }

    /// misaki's `FallbackNetwork.__call__`: the phonemes for `word` (as written: case matters)
    /// and misaki's rating for them, always 1 (the lowest; lexicon entries are 3 to 5).
    pub fn predict(&mut self, word: &str) -> Result<(String, u8)> {
        let input = self.tokens(word);
        if input.len() > MAX_LETTERS + 2 {
            bail!("'{word}' is longer than the G2P network's {MAX_LETTERS} letters");
        }
        // Greedy search, as `generate()` runs it: start from `decoder_start_token_id` (`<s>`),
        // stop at `</s>`, which is forced at the last step.
        let mut decoded = vec![BOS];
        while decoded.len() <= MAX_NEW_TOKENS {
            let next = if decoded.len() == MAX_NEW_TOKENS { EOS } else { self.step(&input, &decoded)? };
            decoded.push(next);
            if next == EOS {
                break;
            }
        }
        Ok((self.phonemes(&decoded), 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_like_misaki() {
        let net = Network::load(false).unwrap();
        // `_` is one of the special-token placeholders, so it reads as `<unk>` (3), like `é`.
        assert_eq!(net.tokens("ab_é"), vec![BOS, net.graphemes[&'a'], net.graphemes[&'b'], UNK, UNK, EOS]);
        assert_eq!(net.phonemes(&[BOS, 0, 3, 4, 1000, EOS]), "A");
    }

    /// misaki's own `FallbackNetwork` output (tests/g2p/reference.py) for every word in
    /// tests/g2p/words.txt, US and British.
    #[test]
    fn matches_misaki() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/g2p");
        for (dialect, british) in [("us", false), ("gb", true)] {
            let path = dir.join(format!("reference-{dialect}.tsv"));
            let Ok(reference) = std::fs::read_to_string(&path) else {
                eprintln!("skipping: {} not found", path.display());
                continue;
            };
            let mut net = Network::load(british).unwrap();
            let mut differ = Vec::new();
            let lines: Vec<&str> = reference.lines().collect();
            for line in &lines {
                let [word, phonemes, rating] = line.split('\t').collect::<Vec<_>>()[..] else {
                    panic!("bad line in {}: {line}", path.display())
                };
                let got = net.predict(word).unwrap();
                if (got.0.as_str(), got.1.to_string().as_str()) != (phonemes, rating) {
                    differ.push(format!("{word}: misaki /{phonemes}/ {rating}, speak /{}/ {}", got.0, got.1));
                }
            }
            assert!(lines.len() > 500, "{}: only {} words", path.display(), lines.len());
            assert!(differ.is_empty(), "{dialect}: {} of {} words differ:\n{}", differ.len(), lines.len(), differ.join("\n"));
        }
    }
}
