//! Output: streaming playback (rodio/cpal: CoreAudio, ALSA, WASAPI) and files. WAV is written
//! natively; other formats are converted with ffmpeg, or on macOS with the built-in afconvert.

use std::io::Write;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rodio::Source;

/// When the output device first pulled one of our samples (for `--timing`).
pub static FIRST_SAMPLE_AT: OnceLock<Instant> = OnceLock::new();

struct Pcm {
    data: std::vec::IntoIter<f32>,
    rate: u32,
    len: usize,
}

impl Iterator for Pcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        FIRST_SAMPLE_AT.get_or_init(Instant::now);
        self.data.next()
    }
}

impl Source for Pcm {
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

/// Play chunks as they arrive (synthesis runs ahead on another thread).
pub fn play(rx: Receiver<Vec<f32>>, rate: u32) -> Result<()> {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink().context("no audio output device")?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    for samples in rx {
        let len = samples.len();
        player.append(Pcm { data: samples.into_iter(), rate, len });
    }
    player.sleep_until_end();
    Ok(())
}

/// 16-bit PCM mono WAV, converted like libsndfile does (what speak.py's soundfile wrote).
pub fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes()); // PCM
    b.extend(1u16.to_le_bytes()); // mono
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for &s in samples {
        b.extend(((s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
    }
    b
}

/// How a `-o` file gets written.
#[derive(Debug)]
pub enum FileOut {
    Wav(PathBuf),
    Convert { path: PathBuf, tool: Vec<String> },
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Decide up front how to write `path`, so a missing converter fails before any synthesis.
pub fn file_out(path: &Path) -> Result<FileOut> {
    let ext = extension(path);
    if ext == "wav" || ext == "wave" {
        return Ok(FileOut::Wav(path.to_path_buf()));
    }
    if let Some(ffmpeg) = crate::paths::which("ffmpeg") {
        let tool = vec![ffmpeg.display().to_string(), "-v".into(), "error".into(), "-y".into(), "-i".into()];
        return Ok(FileOut::Convert { path: path.to_path_buf(), tool });
    }
    if cfg!(target_os = "macos") {
        let format = match ext.as_str() {
            "aiff" | "aif" => Some(["AIFF", "BEI16"]),
            "aifc" => Some(["AIFC", "BEI16"]),
            "caf" => Some(["caff", "LEI16"]),
            "m4a" => Some(["m4af", "aac"]),
            _ => None,
        };
        if let Some([f, d]) = format {
            let tool = ["/usr/bin/afconvert", "-f", f, "-d", d].map(String::from).to_vec();
            return Ok(FileOut::Convert { path: path.to_path_buf(), tool });
        }
    }
    bail!("writing .{ext} files needs ffmpeg, which is not installed (or write .wav)")
}

pub fn write(out: &FileOut, samples: &[f32], rate: u32) -> Result<()> {
    match out {
        FileOut::Wav(path) => {
            let mut f = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
            f.write_all(&wav_bytes(samples, rate))?;
            Ok(())
        }
        FileOut::Convert { path, tool } => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            let tmp = std::env::temp_dir().join(format!("speak-{}-{nanos}.wav", std::process::id()));
            std::fs::write(&tmp, wav_bytes(samples, rate))?;
            let status = Command::new(&tool[0])
                .args(&tool[1..])
                .arg(&tmp)
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .status()
                .with_context(|| format!("running {}", tool[0]));
            let _ = std::fs::remove_file(&tmp);
            if !status?.success() {
                bail!("{} could not write {}", tool[0], path.display());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_samples() {
        let b = wav_bytes(&[0.0, 1.0, -1.0, 0.5, 2.0], 24_000);
        assert_eq!(&b[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 36 + 10);
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1); // mono
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), 16);
        let pcm: Vec<i16> = b[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(pcm, vec![0, 32767, -32767, 16384, 32767]);
    }

    #[test]
    fn output_formats() {
        assert!(matches!(file_out(Path::new("a.WAV")).unwrap(), FileOut::Wav(_)));
        // .aiff works on macOS even without ffmpeg; elsewhere it needs ffmpeg.
        if cfg!(target_os = "macos") {
            assert!(matches!(file_out(Path::new("a.aiff")).unwrap(), FileOut::Convert { .. }));
        }
    }
}
