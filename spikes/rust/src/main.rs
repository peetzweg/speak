//! speak (Rust spike): say(1)-style CLI on a local TTS backend (Kokoro-82M via ONNX Runtime).
//!
//! Exit codes (stable, for scripts and agents):
//!   0 ok · 1 synthesis/audio failure · 2 usage error · 3 model/assets missing · 4 unknown voice

mod audio;
mod backend;
mod g2p;
mod kokoro;
mod npz;
mod picker;
mod text;
mod trim;
mod vocab;

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::Parser;

#[derive(Debug)]
pub struct MissingAsset(pub String);
impl std::fmt::Display for MissingAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "missing model asset {}", self.0)
    }
}
impl std::error::Error for MissingAsset {}

#[derive(Parser, Debug)]
#[command(name = "speak", version, about = "say(1) with a local neural voice. Nothing leaves the machine.")]
struct Args {
    text: Vec<String>,
    /// Voice name, or '?' to list voices
    #[arg(short, long)]
    voice: Option<String>,
    /// Words per minute (175 = normal speed)
    #[arg(short, long)]
    rate: Option<f32>,
    /// Speed multiplier 0.5..2.0 (overrides -r)
    #[arg(long)]
    speed: Option<f32>,
    /// Read text from a file, '-' for stdin
    #[arg(short = 'f', long = "input-file")]
    input_file: Option<PathBuf>,
    /// Write a WAV file instead of playing
    #[arg(short = 'o', long = "output-file")]
    output_file: Option<PathBuf>,
    /// TTS backend
    #[arg(long, default_value = backend::DEFAULT)]
    backend: String,
    /// Print kokoro-onnx-compatible phonemes, batches and token ids as JSON, then exit
    #[arg(long, hide = true)]
    analyze: bool,
    /// Print timings as JSON on stderr
    #[arg(long, hide = true)]
    timing: bool,
    // say(1) flags accepted and ignored
    #[arg(long = "data-format", hide = true)]
    _data_format: Option<String>,
    #[arg(long = "file-format", hide = true)]
    _file_format: Option<String>,
    #[arg(long = "channels", hide = true)]
    _channels: Option<String>,
    #[arg(long = "bit-rate", hide = true)]
    _bit_rate: Option<String>,
    #[arg(long = "quality", hide = true)]
    _quality: Option<String>,
    #[arg(short = 'a', long = "audio-device", hide = true)]
    _audio_device: Option<String>,
    #[arg(short = 'n', long = "network-send", hide = true)]
    _network_send: Option<String>,
    #[arg(short = 'i', long = "interactive", hide = true, num_args = 0..=1)]
    _interactive: Option<Option<String>>,
    #[arg(long = "progress", hide = true)]
    _progress: bool,
}

fn speak_home() -> PathBuf {
    std::env::var_os("SPEAK_HOME").map(PathBuf::from).unwrap_or_else(|| text::home().join(".local/share/speak"))
}

fn fail(code: u8, msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("speak: {msg}");
    ExitCode::from(code)
}

fn open_backend(name: &str) -> Result<Box<dyn backend::Backend>, ExitCode> {
    backend::open(name, &speak_home()).map_err(|e| {
        if e.downcast_ref::<MissingAsset>().is_some() { fail(3, format!("{e} (set SPEAK_HOME)")) } else { fail(1, e) }
    })
}

#[derive(Default)]
struct Timing {
    first_chunk: Option<Duration>,
    synth_done: Option<Duration>,
    samples: usize,
}

fn main() -> ExitCode {
    let t0 = Instant::now();
    if std::env::args().nth(1).as_deref() == Some("voices") {
        return voices_cmd();
    }
    let a = Args::parse();
    let voice_flag = a.voice.clone().or_else(|| std::env::var("SPEAK_VOICE").ok()).or_else(|| text::config_get("voice"));

    let mut be = match open_backend(&a.backend) {
        Ok(b) => b,
        Err(code) => return code,
    };
    let info = be.info();
    if voice_flag.as_deref() == Some("?") {
        for v in be.voices().unwrap_or_default() {
            println!("{:<16} {}", v.id, v.label);
        }
        return ExitCode::SUCCESS;
    }
    let voice = voice_flag.unwrap_or_else(|| info.default_voice.to_string());

    let raw = match read_text(&a) {
        Ok(t) => t,
        Err(code) => return code,
    };
    if raw.trim().is_empty() {
        return ExitCode::SUCCESS;
    }
    let lexicon = std::env::var_os("SPEAK_LEXICON").map(PathBuf::from).unwrap_or_else(|| text::config_dir().join("lexicon.txt"));
    let input = text::apply_lexicon(&raw, &text::read_lexicon(&lexicon));

    if a.analyze {
        let mut k = match kokoro::Kokoro::open(&backend::KokoroPaths::under(&speak_home())) {
            Ok(k) => k,
            Err(e) => return fail(3, e),
        };
        let lang = kokoro::lang_for(&voice);
        let t = Instant::now();
        let first = k.analyze(&input, lang);
        if a.timing && first.is_ok() {
            // G2P cost: first call includes starting espeak (process spawn or library init).
            let cold = t.elapsed();
            let t2 = Instant::now();
            let n = 20;
            for _ in 0..n {
                let _ = k.analyze(&input, lang);
            }
            eprintln!(
                "{{\"g2p_first_ms\": {:.2}, \"g2p_warm_ms\": {:.3}, \"since_start_ms\": {:.2}}}",
                cold.as_secs_f64() * 1e3,
                t2.elapsed().as_secs_f64() * 1e3 / n as f64,
                t0.elapsed().as_secs_f64() * 1e3
            );
        }
        return match first {
            Ok(r) => {
                let batches: Vec<String> = r.batches.iter().map(|b| json_str(b)).collect();
                let tokens: Vec<String> =
                    r.tokens.iter().map(|t| format!("[{}]", t.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", "))).collect();
                println!("{{\"phonemes\": {}, \"batches\": [{}], \"tokens\": [{}]}}", json_str(&r.phonemes), batches.join(", "), tokens.join(", "));
                ExitCode::SUCCESS
            }
            Err(e) => fail(1, e),
        };
    }

    match be.voices() {
        Ok(vs) if vs.iter().any(|v| v.id == voice) => {}
        Ok(_) => return fail(4, format!("unknown voice '{voice}' (try: speak -v '?')")),
        Err(e) => return fail(1, e),
    }
    let speed = a.speed.unwrap_or_else(|| a.rate.map_or(1.0, |r| r / 175.0)).clamp(0.5, 2.0);
    let sentences = text::chunks(&input);
    let rate = info.sample_rate;

    // Synthesis runs ahead on its own thread; playback starts with the first sentence.
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(4);
    let synth = std::thread::spawn(move || -> anyhow::Result<Timing> {
        let mut t = Timing::default();
        let gap = vec![0.0f32; (0.12 * rate as f64) as usize];
        let n = sentences.len();
        for (i, s) in sentences.iter().enumerate() {
            for chunk in be.synthesize(s, &voice, speed)? {
                let chunk = chunk?;
                t.first_chunk.get_or_insert(t0.elapsed());
                t.samples += chunk.samples.len();
                let _ = tx.send(chunk.samples);
            }
            if i + 1 < n {
                t.samples += gap.len();
                let _ = tx.send(gap.clone());
            }
        }
        t.synth_done = Some(t0.elapsed());
        Ok(t)
    });

    let out = if let Some(path) = &a.output_file {
        let pcm: Vec<f32> = rx.iter().flatten().collect();
        audio::write_wav(path, &pcm, rate)
    } else {
        audio::play(rx, rate)
    };
    let t = match synth.join() {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return fail(1, format!("{e:#}")),
        Err(_) => return fail(1, "synthesis thread panicked"),
    };
    if let Err(e) = out {
        return fail(1, format!("{e:#}"));
    }
    if a.timing || std::env::var_os("SPEAK_TIMING").is_some() {
        let ms = |d: Option<Duration>| d.map_or(-1.0, |d| d.as_secs_f64() * 1000.0);
        let first_sample = audio::FIRST_SAMPLE_AT.get().map(|x| x.duration_since(t0));
        eprintln!(
            "{{\"first_chunk_ms\": {:.0}, \"first_sample_played_ms\": {:.0}, \"synth_done_ms\": {:.0}, \"total_ms\": {:.0}, \"audio_s\": {:.3}}}",
            ms(t.first_chunk),
            ms(first_sample),
            ms(t.synth_done),
            t0.elapsed().as_secs_f64() * 1000.0,
            t.samples as f64 / rate as f64
        );
    }
    ExitCode::SUCCESS
}

fn read_text(a: &Args) -> Result<String, ExitCode> {
    let mut s = String::new();
    match &a.input_file {
        Some(p) if p.as_os_str() == "-" => std::io::stdin().read_to_string(&mut s).map(|_| s).map_err(|e| fail(1, e)),
        Some(p) => std::fs::read_to_string(p).map_err(|e| fail(1, format!("{}: {e}", p.display()))),
        None if !a.text.is_empty() => Ok(a.text.join(" ")),
        None if !std::io::stdin().is_terminal() => std::io::stdin().read_to_string(&mut s).map(|_| s).map_err(|e| fail(1, e)),
        None => Err(fail(2, "no text (pass it as arguments, with -f, or on stdin)")),
    }
}

fn voices_cmd() -> ExitCode {
    let be = match open_backend(backend::DEFAULT) {
        Ok(b) => b,
        Err(code) => return code,
    };
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        // Non-interactive (agents, pipes): plain list on stdout, like `-v '?'`.
        for v in be.voices().unwrap_or_default() {
            println!("{:<16} {}", v.id, v.label);
        }
        return ExitCode::SUCCESS;
    }
    let current = text::config_get("voice").unwrap_or_else(|| be.info().default_voice.to_string());
    match picker::run(be, &current) {
        Ok(Some(v)) => match text::config_set("voice", &v) {
            Ok(p) => {
                eprintln!("default voice: {v} ({})", p.display());
                ExitCode::SUCCESS
            }
            Err(e) => fail(1, e),
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(e) => fail(1, e),
    }
}

/// JSON string literal with the same escaping as Python's json.dumps(ensure_ascii=False).
fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
