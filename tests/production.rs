//! Productions: subtitles, the timing manifest, per-paragraph files, pauses, voices and loudness,
//! end to end. Tests that need the Kokoro model skip (with a note) when it isn't available.

mod common;

use std::path::Path;

use common::*;
use serde_json::Value;

const SCRIPT: &str = "\
[[voice af_heart]]Welcome to the catch-up. This week the team shipped three releases, fixed the frontend deploy, and moved the whole pipeline to the new runners.

[[voice bm_george]]Thanks. When the frontend deploy failed on Tuesday, Anouk traced it to the RPC endpoint, which had moved without notice. [[slnc 800]] It works again.

[[rset]]That's all for today.
";

fn secs_to_samples(v: &Value) -> usize {
    (v.as_f64().unwrap() * 24_000.0).round() as usize
}

/// (start, end, text) of every SRT or VTT cue, in samples.
fn cues(subs: &str) -> Vec<(usize, usize, String)> {
    let stamp = |s: &str| {
        let s = s.replace(',', ".");
        let p: Vec<f64> = s.split(':').map(|x| x.parse().unwrap()).collect();
        ((p[0] * 3600.0 + p[1] * 60.0 + p[2]) * 24_000.0).round() as usize
    };
    subs.split("\n\n")
        .filter_map(|block| {
            let mut lines = block.lines().skip_while(|l| !l.contains(" --> "));
            let (a, b) = lines.next()?.split_once(" --> ")?;
            Some((stamp(a), stamp(b), lines.collect::<Vec<_>>().join("\n")))
        })
        .collect()
}

/// Integrated loudness of a file as ffmpeg's loudnorm filter measures it (BS.1770).
fn ffmpeg_lufs(path: &Path) -> Option<f64> {
    let o = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-af", "loudnorm=print_format=json", "-f", "null", "-"])
        .output()
        .ok()?;
    let err = String::from_utf8_lossy(&o.stderr);
    let i = err.find("\"input_i\"")?;
    err[i..].split('"').nth(3)?.parse().ok()
}

#[test]
fn production_flags_are_checked_first() {
    for args in [
        &["--subtitles", "x.txt", "hi"][..],
        &["--loudness", "-16", "hi"],
        &["--loudness", "3", "-o", "x.wav", "hi"],
        &["--pauses", "soon", "hi"],
    ] {
        let o = run(speak(None).args(args));
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).starts_with("speak: ") && stderr(&o).lines().count() == 1, "{args:?}: {}", stderr(&o));
    }
    // Without the model, productions fail (exit 3) instead of falling back to the system voice.
    let dir = tmpdir("noprod");
    let o = run(speak(None).arg("-o").arg(dir.join("x.wav")).arg("--subtitles").arg(dir.join("x.srt")).arg("Hello."));
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert!(!dir.join("x.wav").exists());
}

#[test]
fn subtitles_manifest_split_and_loudness() {
    let Some(home) = model_home() else { return };
    let dir = tmpdir("production");
    let script = dir.join("script.txt");
    std::fs::write(&script, SCRIPT).unwrap();
    let o = run(speak(Some(&home))
        .arg("-f")
        .arg(&script)
        .arg("-o")
        .arg(dir.join("out.wav"))
        .arg("--subtitles")
        .arg(dir.join("out.srt"))
        .arg("--manifest")
        .arg(dir.join("out.json"))
        .arg("--split")
        .arg(dir.join("parts"))
        .args(["--loudness", "-16"]));
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{}{}", stdout(&o), stderr(&o));

    let (rate, _, _, pcm) = read_wav(&dir.join("out.wav"));
    assert_eq!(rate, 24_000);
    let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("out.json")).unwrap()).unwrap();
    assert_eq!(m["sample_rate"], 24_000);
    assert_eq!(secs_to_samples(&m["duration"]), pcm.len());
    assert_eq!(m["audio"].as_str().unwrap(), dir.join("out.wav").display().to_string());

    // Paragraphs and sentences, with their voices.
    let paragraphs = m["paragraphs"].as_array().unwrap();
    assert_eq!(paragraphs.len(), 3);
    let sentences: Vec<(usize, &str, &str)> = paragraphs
        .iter()
        .enumerate()
        .flat_map(|(p, para)| {
            para["sentences"].as_array().unwrap().iter().map(move |s| (p, s["voice"].as_str().unwrap(), s["text"].as_str().unwrap()))
        })
        .collect();
    assert_eq!(
        sentences.iter().map(|s| (s.0, s.1)).collect::<Vec<_>>(),
        [(0, "af_heart"), (0, "af_heart"), (1, "bm_george"), (1, "bm_george"), (1, "bm_george"), (2, "af_heart")]
    );
    assert_eq!(
        sentences[3].2,
        "When the frontend deploy failed on Tuesday, Anouk traced it to the RPC endpoint, which had moved without notice."
    );

    // Every sample is accounted for: between sentences there is nothing but silence, exactly.
    let spans: Vec<(usize, usize)> = paragraphs
        .iter()
        .flat_map(|p| p["sentences"].as_array().unwrap().iter().map(|s| (secs_to_samples(&s["start"]), secs_to_samples(&s["end"]))))
        .collect();
    let mut at = 0;
    for (start, end) in &spans {
        assert!(*start >= at && end > start);
        assert!(pcm[at..*start].iter().all(|&s| s == 0), "noise before {start}");
        at = *end;
    }
    assert!(pcm[at..].iter().all(|&s| s == 0));
    let gap = |a: usize, b: usize| (spans[b].0 - spans[a].1) as f64 / 24.0;
    assert_eq!(gap(0, 1), 120.0); // sentence pause
    assert_eq!(gap(1, 2), 600.0); // paragraph pause
    assert_eq!(gap(3, 4), 800.0); // [[slnc 800]] in place of the sentence pause

    // Split files are exact slices of the whole.
    for (i, para) in paragraphs.iter().enumerate() {
        let file = dir.join("parts").join(format!("{:03}.wav", i + 1));
        assert_eq!(para["file"].as_str().unwrap(), file.display().to_string());
        let (start, end) = (secs_to_samples(&para["start"]), secs_to_samples(&para["end"]));
        assert_eq!(read_wav(&file).3, pcm[start..end], "paragraph {i}");
    }

    // Subtitles: one cue per sentence, long ones in clauses that tile the sentence, never more
    // than two lines of 42 characters.
    let subs = cues(&std::fs::read_to_string(dir.join("out.srt")).unwrap());
    for (_, _, text) in &subs {
        assert!(text.lines().count() <= 2 && text.lines().all(|l| l.chars().count() <= 42), "{text:?}");
    }
    let mut seen = 0;
    for (sentence, (s, e)) in sentences.iter().zip(&spans) {
        let mine: Vec<&(usize, usize, String)> = subs.iter().filter(|c| c.0 + 12 >= *s && c.1 <= e + 12).collect();
        assert!(!mine.is_empty(), "{}", sentence.2);
        assert_eq!(mine.iter().map(|c| c.2.replace('\n', " ")).collect::<Vec<_>>().join(" "), sentence.2);
        assert!(mine[0].0.abs_diff(*s) <= 12 && mine.last().unwrap().1.abs_diff(*e) <= 12, "{}", sentence.2); // ms rounding
        for w in mine.windows(2) {
            assert_eq!(w[0].1, w[1].0, "clauses of one sentence follow on directly");
        }
        seen += mine.len();
    }
    assert_eq!(seen, subs.len(), "every cue belongs to a sentence");
    assert!(subs.len() > sentences.len(), "the long sentences should be in several cues: {subs:?}");

    // Loudness: our measure, the file, and ffmpeg's measure agree.
    let l = &m["loudness"];
    assert!((l["integrated"].as_f64().unwrap() + 16.0).abs() < 0.1, "{l}");
    assert!(l["peak_dbfs"].as_f64().unwrap() <= -1.0 + 0.01, "{l}");
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap() as f64 / 32768.0;
    assert!(20.0 * peak.log10() <= -0.99, "peak {peak}");
    if let Some(lufs) = ffmpeg_lufs(&dir.join("out.wav")) {
        assert!((lufs + 16.0).abs() < 0.5, "ffmpeg reads {lufs} LUFS");
    } else {
        eprintln!("note: ffmpeg not found, loudness not cross-checked");
    }

    // The same script without --loudness: its loudness as measured before normalising matches
    // ffmpeg's; and without --subtitles the audio is identical (cue boundaries change nothing).
    let plain = dir.join("plain.wav");
    let o = run(speak(Some(&home)).arg("-f").arg(&script).arg("-o").arg(&plain).arg("--subtitles").arg(dir.join("plain.vtt")));
    assert!(o.status.success(), "{}", stderr(&o));
    let no_subs = dir.join("nosubs.wav");
    assert!(run(speak(Some(&home)).arg("-f").arg(&script).arg("-o").arg(&no_subs)).status.success());
    assert_eq!(std::fs::read(&plain).unwrap(), std::fs::read(&no_subs).unwrap());
    let vtt = std::fs::read_to_string(dir.join("plain.vtt")).unwrap();
    assert!(vtt.starts_with("WEBVTT\n\n00:00:00.000 --> "), "{vtt}");
    assert_eq!(cues(&vtt).len(), subs.len());
    if let Some(lufs) = ffmpeg_lufs(&plain) {
        let before = l["before"].as_f64().unwrap();
        assert!((lufs - before).abs() < 0.5, "ffmpeg {lufs}, speak {before}");
    }
}

#[test]
fn manifest_on_stdout_and_custom_pauses() {
    let Some(home) = model_home() else { return };
    let dir = tmpdir("manifest");
    let o = run(speak(Some(&home))
        .args(["--manifest", "-", "--pauses", "300,1000", "-r", "220", "-o"])
        .arg(dir.join("a.wav"))
        .arg("One. Two.\n\nThree."));
    assert!(o.status.success(), "{}", stderr(&o));
    let m: Value = serde_json::from_str(&stdout(&o)).expect("only the manifest on stdout");
    assert!(m.get("loudness").is_none());
    let p = m["paragraphs"].as_array().unwrap();
    let s = |p: &Value, i: usize| (secs_to_samples(&p["sentences"][i]["start"]), secs_to_samples(&p["sentences"][i]["end"]));
    assert_eq!(s(&p[0], 1).0 - s(&p[0], 0).1, 300 * 24);
    assert_eq!(s(&p[1], 0).0 - s(&p[0], 1).1, 1000 * 24);
    assert!(p[0].get("file").is_none());
}

#[test]
fn rate_command_changes_speed_mid_script() {
    let Some(home) = model_home() else { return };
    let dir = tmpdir("rate");
    let o = run(speak(Some(&home))
        .args(["--manifest", "-", "-o"])
        .arg(dir.join("r.wav"))
        .arg("The same words again. [[rate 300]]The same words again."));
    assert!(o.status.success(), "{}", stderr(&o));
    let m: Value = serde_json::from_str(&stdout(&o)).unwrap();
    let s = &m["paragraphs"][0]["sentences"];
    let len = |i: usize| s[i]["end"].as_f64().unwrap() - s[i]["start"].as_f64().unwrap();
    assert!(len(1) < len(0) * 0.8, "{} vs {}", len(1), len(0));
}
