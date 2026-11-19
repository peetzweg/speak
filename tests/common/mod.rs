//! Shared test helpers. Every run of the binary gets its own config, lexicon, cache and model
//! directories, so the tests never touch the real ones. Model files are only ever symlinked
//! (read-only) from `$SPEAK_TEST_MODELS`, default `~/.local/share/speak/models`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A fresh, empty directory under cargo's per-target temp dir.
pub fn tmpdir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// espeak-ng for the tests: `$SPEAK_ESPEAK`, the copy `scripts/build-espeak.sh` builds, or PATH.
pub fn espeak() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SPEAK_ESPEAK").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let built = repo().join("third_party/espeak-ng/bin/espeak-ng");
    if built.is_file() {
        return Some(built);
    }
    let name = format!("espeak-ng{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&name)).find(|p| p.is_file())
}

/// A temporary SPEAK_HOME holding symlinks to the fp32 model, if the files are available.
pub fn model_home() -> Option<PathBuf> {
    let src = std::env::var_os("SPEAK_TEST_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/speak/models"));
    let files = [("kokoro-v1.0.onnx", 325_532_387), ("voices-v1.0.bin", 28_214_398)];
    if !files.iter().all(|(f, size)| std::fs::metadata(src.join(f)).is_ok_and(|m| m.len() == *size)) {
        eprintln!("skipping: Kokoro model files not found in {} (set SPEAK_TEST_MODELS)", src.display());
        return None;
    }
    let home = tmpdir("home");
    std::fs::create_dir_all(home.join("models")).unwrap();
    for (f, _) in files {
        symlink(&src.join(f), &home.join("models").join(f));
    }
    Some(home)
}

pub fn symlink(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).unwrap();
}

/// The speak binary with an isolated environment. `home` is SPEAK_HOME (empty if `None`).
pub fn speak(home: Option<&Path>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_speak"));
    isolate(&mut cmd, home);
    cmd
}

/// Point every location speak reads or writes at fresh temporary paths.
pub fn isolate(cmd: &mut Command, home: Option<&Path>) {
    let scratch = tmpdir("env");
    cmd.env("SPEAK_HOME", home.map_or_else(|| scratch.join("home"), Path::to_path_buf))
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("SPEAK_LEXICON", scratch.join("lexicon.txt"))
        // Shared, so the model files are hashed once per test run rather than once per test.
        .env("XDG_CACHE_HOME", PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cache"))
        .env("SPEAK_NO_DOWNLOAD", "1")
        .env_remove("SPEAK_VOICE")
        .env_remove("SPEAK_MODEL")
        .env_remove("SPEAK_ESPEAK_DATA")
        .env_remove("SPEAK_TIMING")
        .stdin(Stdio::null());
    match espeak() {
        Some(p) => cmd.env("SPEAK_ESPEAK", p),
        None => cmd.env_remove("SPEAK_ESPEAK"),
    };
}

pub fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("running speak")
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The `"phonemes"` value of `--analyze` output.
pub fn phonemes(o: &Output) -> String {
    let s = stdout(o);
    let start = s.find("\"phonemes\": \"").expect("phonemes in output") + 13;
    s[start..s[start..].find("\", \"batches\"").unwrap() + start].to_string()
}

/// (sample rate, channels, bits, samples) of a PCM WAV file.
pub fn read_wav(path: &Path) -> (u32, u16, u16, Vec<i16>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let mut i = 12;
    let (mut rate, mut channels, mut bits) = (0, 0, 0);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let samples = body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            return (rate, channels, bits, samples);
        }
        i += 8 + len + (len & 1);
    }
    panic!("no data chunk in {}", path.display());
}
