//! Output: streaming playback through rodio/cpal (CoreAudio, ALSA, WASAPI) and WAV files.

use std::io::Write;
use std::num::NonZero;
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rodio::Source;

/// Set by the audio thread the first time the device pulls a sample of ours.
pub static FIRST_SAMPLE_AT: OnceLock<Instant> = OnceLock::new();

/// A mono f32 buffer that stamps `FIRST_SAMPLE_AT` when the output device first reads from it.
struct Probe {
    data: std::vec::IntoIter<f32>,
    rate: u32,
    len: usize,
}

impl Iterator for Probe {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        FIRST_SAMPLE_AT.get_or_init(Instant::now);
        self.data.next()
    }
}

impl Source for Probe {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.data.len())
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.len as f64 / self.rate as f64))
    }
}

/// Play chunks as they arrive on `rx` (the synthesiser runs ahead on another thread).
pub fn play(rx: Receiver<Vec<f32>>, rate: u32) -> Result<()> {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink().context("no audio output device")?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    for samples in rx {
        let len = samples.len();
        player.append(Probe { data: samples.into_iter(), rate, len });
    }
    player.sleep_until_end();
    Ok(())
}

/// 16-bit PCM mono WAV (what soundfile writes for speak.py's `-o x.wav`).
pub fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32) -> Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?);
    let data_len = (samples.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        // libsndfile's float -> PCM_16 conversion: scale by 0x7FFF, round, clip.
        let v = (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}
