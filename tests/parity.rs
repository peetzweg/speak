//! Regression test: `--g2p espeak` must still produce exactly what kokoro-onnx 0.4.7 feeds
//! the model (phonemes, batches and token ids saved in `tests/parity/reference-macos.jsonl`,
//! made with `tests/parity/reference.py`), and audio within a few 16-bit steps of its output.
//! Needs espeak-ng 1.52 (`scripts/build-espeak.sh`); the audio check also needs the model.

mod common;

use common::*;

fn espeak_152() -> bool {
    let Some(es) = espeak() else {
        eprintln!("skipping: espeak-ng not found");
        return false;
    };
    let version = std::process::Command::new(&es)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if !version.contains(" 1.52") {
        eprintln!("skipping: the reference was made with espeak-ng 1.52, found: {}", version.trim());
        return false;
    }
    true
}

#[test]
fn phonemes_and_tokens_match_kokoro_onnx() {
    if !espeak_152() {
        return;
    }
    let dir = repo().join("tests/parity");
    let corpus = std::fs::read_to_string(dir.join("corpus.txt")).unwrap();
    let reference = std::fs::read_to_string(dir.join("reference-macos.jsonl")).unwrap();
    let mut mismatches = Vec::new();
    let lines: Vec<&str> = corpus.lines().collect();
    for (line, want) in lines.iter().zip(reference.lines()) {
        let text = line.replace("\\n", "\n");
        let o = run(speak(None).args(["--g2p=espeak", "--analyze", "-v", "af_heart", "--"]).arg(&text));
        assert!(o.status.success(), "{}", stderr(&o));
        let got = stdout(&o);
        if got.trim_end() != want {
            mismatches.push(format!("{line}\n  want {want}\n  got  {}", got.trim_end()));
        }
    }
    assert_eq!(lines.len(), reference.lines().count());
    assert!(mismatches.is_empty(), "{} of {} lines differ:\n{}", mismatches.len(), lines.len(), mismatches.join("\n"));
}

#[test]
fn audio_matches_kokoro_onnx() {
    if !espeak_152() {
        return;
    }
    let Some(home) = model_home() else { return };
    let out = tmpdir("parity").join("kruapi.wav");
    let text = "KruAPI shipped v0.3.5 on Tuesday, and Anouk's game had 2,000 bots.";
    let o = run(speak(Some(&home)).args(["--g2p=espeak", "-v", "af_heart", "-o"]).arg(&out).arg(text));
    assert!(o.status.success(), "{}", stderr(&o));
    let (_, _, _, ours) = read_wav(&out);
    let (_, _, _, reference) = read_wav(&repo().join("tests/parity/kokoro-onnx-kruapi.wav"));
    assert_eq!(ours.len(), reference.len());
    // ONNX Runtime picks CPU kernels by what the processor supports, so the same tokens give
    // slightly different numbers on different machines (Linux arm64: correlation 0.998; GitHub's
    // macOS runners: up to 2764 16-bit steps apart). Only the shape of the signal is compared by
    // default. On the machine that recorded the reference (an Apple M5 Pro), kokoro-onnx and speak
    // agree to within a 16-bit step; set SPEAK_TEST_EXACT_AUDIO=1 there to check that too.
    let dot = |x: &[i16], y: &[i16]| x.iter().zip(y).map(|(a, b)| *a as f64 * *b as f64).sum::<f64>();
    let corr = dot(&ours, &reference) / (dot(&ours, &ours) * dot(&reference, &reference)).sqrt();
    assert!(corr > 0.99, "correlation with kokoro-onnx's audio is only {corr:.4}");
    if std::env::var_os("SPEAK_TEST_EXACT_AUDIO").is_some() {
        let max_diff = ours.iter().zip(&reference).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(max_diff <= 3, "max sample difference {max_diff} (16-bit steps)");
    }
}
