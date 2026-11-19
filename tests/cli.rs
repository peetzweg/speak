//! End-to-end tests of the `speak` binary. Tests that need the Kokoro model or espeak-ng skip
//! (with a note) when they aren't available.

mod common;

use std::io::Write;
use std::process::Stdio;

use common::*;

#[test]
fn version_help_and_usage_errors() {
    let o = run(speak(None).arg("--version"));
    assert!(o.status.success());
    assert!(stdout(&o).starts_with("speak 0.1"));
    assert!(run(speak(None).arg("--help")).status.success());

    for args in [&["--bogus"][..], &["-m", "nope", "hi"], &["lexicon", "frobnicate"], &["models", "frob"], &["models", "download", "nope"]]
    {
        let o = run(speak(None).args(args));
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
        let err = stderr(&o);
        assert!(err.starts_with("speak: ") && err.lines().count() == 1, "{args:?}: {err}");
    }
}

#[test]
fn empty_input_is_a_quiet_no_op() {
    let o = run(&mut speak(None));
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty() && o.stderr.is_empty());
}

#[test]
fn lexicon_commands_match_speak_py() {
    let dir = tmpdir("lex");
    let file = dir.join("lexicon.txt");
    let lex = |args: &[&str]| run(speak(None).env("SPEAK_LEXICON", &file).arg("lexicon").args(args));

    let o = lex(&["add", "KruAPI", "crew", "A", "P", "I"]);
    assert!(o.status.success());
    assert_eq!(stdout(&o), "KruAPI = crew A P I\n");
    assert!(lex(&["add", "zeta", "zee"]).status.success());
    assert!(lex(&["add", "kokoro", "/kˈOkəɹO/"]).status.success());
    // Same word, different case: replaced.
    assert!(lex(&["set", "kruapi", "crew", "A", "P", "I"]).status.success());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "# speak pronunciations: `word = spoken form`, one per line. Matches whole words, ignoring case.\n\
         kokoro = /kˈOkəɹO/\nkruapi = crew A P I\nzeta = zee\n"
    );
    assert_eq!(stdout(&lex(&["list"])), "kokoro  =  /kˈOkəɹO/\nkruapi  =  crew A P I\nzeta    =  zee\n");
    assert_eq!(stdout(&lex(&[])), stdout(&lex(&["ls"])));
    assert_eq!(stdout(&lex(&["path"])).trim_end(), file.display().to_string());

    let o = lex(&["rm", "ZETA"]);
    assert_eq!(stdout(&o), "removed ZETA\n");
    let o = lex(&["rm", "zeta"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stderr(&o), "speak: no entry for 'zeta'\n");

    // Phonemes must be in Kokoro's alphabet.
    let o = lex(&["add", "bad", "/hɛ1lo/"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert_eq!(lex(&["add"]).status.code(), Some(2));
    assert_eq!(lex(&["rm"]).status.code(), Some(2));

    // Entries apply to what is spoken.
    let o = run(speak(None).env("SPEAK_LEXICON", &file).args(["--analyze", "Kokoro by KruAPI"]));
    assert!(o.status.success(), "{}", stderr(&o));
    let ph = phonemes(&o);
    assert!(ph.starts_with("kˈOkəɹO bI kɹˈu ˌApˌiˈI"), "{ph}");
}

#[test]
fn english_reads_numbers_versions_and_dates() {
    let o = run(speak(None).args(["--analyze", "They shipped v0.3.5 to 2,000 users on 2025-03-03."]));
    assert!(o.status.success(), "{}", stderr(&o));
    let ph = phonemes(&o);
    for want in ["vˈɜɹʒən zˈɪɹO", "tˈu θˈWzᵊnd", "mˈɑɹʧ θˈɜɹd"] {
        assert!(ph.contains(want), "{want} not in {ph}");
    }
    // say's interactive flag is ignored without eating the text; -f reads files and stdin.
    let o = run(speak(None).args(["--analyze", "-i", "--progress", "hello"]));
    assert!(phonemes(&o).contains("həlˈO"), "{}", stdout(&o));
    let file = tmpdir("text").join("t.txt");
    std::fs::write(&file, "hello").unwrap();
    assert!(phonemes(&run(speak(None).args(["--analyze", "-f"]).arg(&file))).contains("həlˈO"));
    let mut child = speak(None).args(["--analyze", "-f", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"hello").unwrap();
    assert!(phonemes(&child.wait_with_output().unwrap()).contains("həlˈO"));
}

#[test]
fn unknown_words_use_espeak_ng() {
    if espeak().is_none() {
        eprintln!("skipping: espeak-ng not found");
        return;
    }
    let o = run(speak(None).args(["--analyze", "kubernetes"]));
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(phonemes(&o), "kˌubəɹnˈɛTiz");
    // Non-English voices go through espeak-ng entirely.
    let o = run(speak(None).args(["--analyze", "-v", "ef_dora", "¿Qué tal?"]));
    assert_eq!(phonemes(&o), "kˈe tˈal?"); // ¿ is not in Kokoro's vocabulary
}

#[test]
fn without_espeak_unknown_words_are_spelled() {
    let o = run(speak(None).env_remove("SPEAK_ESPEAK").env("PATH", "").args(["--analyze", "Xqz"]));
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(phonemes(&o), "ˈɛks kjˈu zˈi");
    assert!(stderr(&o).contains("espeak-ng not found"));
    // ...but a non-English voice can't do without it.
    let o = run(speak(None).env_remove("SPEAK_ESPEAK").env("PATH", "").args(["--analyze", "-v", "ff_siwis", "Bonjour"]));
    assert_eq!(o.status.code(), Some(3));
}

#[test]
fn finds_a_bundled_espeak_next_to_the_binary() {
    let Some(es) = espeak() else { return eprintln!("skipping: espeak-ng not found") };
    let Some(data) =
        std::fs::canonicalize(&es).ok().and_then(|p| Some(p.parent()?.parent()?.join("share/espeak-ng-data"))).filter(|d| d.is_dir())
    else {
        return eprintln!("skipping: no espeak-ng-data next to {}", es.display());
    };
    // <prefix>/bin/speak + <prefix>/libexec/speak/{espeak-ng, espeak-ng-data}
    let prefix = tmpdir("bundle");
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    std::fs::create_dir_all(prefix.join("libexec/speak")).unwrap();
    let exe = prefix.join("bin").join(format!("speak{}", std::env::consts::EXE_SUFFIX));
    std::fs::hard_link(env!("CARGO_BIN_EXE_speak"), &exe)
        .or_else(|_| std::fs::copy(env!("CARGO_BIN_EXE_speak"), &exe).map(|_| ()))
        .unwrap();
    symlink(&std::fs::canonicalize(&es).unwrap(), &prefix.join("libexec/speak/espeak-ng"));
    symlink(&data, &prefix.join("libexec/speak/espeak-ng-data"));
    // No $SPEAK_ESPEAK and nothing on PATH: only the bundled copy can answer.
    let mut cmd = std::process::Command::new(&exe);
    isolate(&mut cmd, None);
    let o = run(cmd.env_remove("SPEAK_ESPEAK").env("PATH", "").args(["--analyze", "-v", "ef_dora", "hola"]));
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(phonemes(&o), "ˈola");
}

#[test]
fn models_list_and_missing_model() {
    let o = run(speak(None).arg("models"));
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(out.contains("kokoro ") && out.contains("kokoro-int8") && out.contains("available"), "{out}");
    let o = run(speak(None).args(["models", "rm", "kokoro"]));
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn falls_back_to_the_system_voice_without_a_model() {
    let out = tmpdir("fallback").join("out.wav");
    let o = run(speak(None).arg("-o").arg(&out).arg("Hello from the fallback."));
    let err = stderr(&o);
    if cfg!(target_os = "macos") || espeak().is_some() {
        assert_eq!(o.status.code(), Some(0), "{err}");
        assert!(err.contains("using the system voice") && err.lines().count() == 1, "{err}");
        let (_, channels, _, samples) = read_wav(&out);
        assert_eq!(channels, 1);
        assert!(samples.len() > 10_000);
    } else {
        assert_eq!(o.status.code(), Some(3), "{err}");
    }
}

#[test]
fn writes_a_wav_file() {
    let Some(home) = model_home() else { return };
    let dir = tmpdir("wav");
    let out = dir.join("hello.wav");
    let o = run(speak(Some(&home)).arg("-o").arg(&out).args(["--timing", "Hello there. This is a test of speak."]));
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stderr(&o).contains("\"rtf\""), "{}", stderr(&o));
    let (rate, channels, bits, samples) = read_wav(&out);
    assert_eq!((rate, channels, bits), (24_000, 1, 16));
    let secs = samples.len() as f64 / 24_000.0;
    assert!((1.0..6.0).contains(&secs), "{secs} s");
    let rms = (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    assert!(rms > 300.0, "too quiet: rms {rms}");

    // Faster speech is shorter.
    let fast = dir.join("fast.wav");
    assert!(run(speak(Some(&home)).arg("-o").arg(&fast).args(["-r", "300", "Hello there. This is a test of speak."])).status.success());
    assert!(read_wav(&fast).3.len() < samples.len());

    // Other formats go through ffmpeg (or afconvert on macOS).
    if cfg!(target_os = "macos") {
        let aiff = dir.join("hello.aiff");
        assert!(run(speak(Some(&home)).arg("-o").arg(&aiff).arg("Hi.")).status.success());
        assert!(std::fs::metadata(&aiff).unwrap().len() > 1000);
    }
}

#[test]
fn voices_and_voice_selection() {
    let Some(home) = model_home() else { return };
    let o = run(speak(Some(&home)).args(["-v", "?"]));
    assert!(o.status.success(), "{}", stderr(&o));
    let list = stdout(&o);
    assert!(list.lines().count() >= 50 && list.contains("af_heart") && list.contains("American English, female"), "{list}");
    // Not on a terminal: `speak voices` prints the same list.
    assert_eq!(stdout(&run(speak(Some(&home)).arg("voices"))), list);

    let out = tmpdir("voice").join("x.wav");
    let o = run(speak(Some(&home)).args(["-v", "xx_nobody", "-o"]).arg(&out).arg("Hi."));
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));

    // flag > SPEAK_VOICE > config file
    let config = tmpdir("config");
    std::fs::create_dir_all(config.join("speak")).unwrap();
    std::fs::write(config.join("speak/config.toml"), "voice = \"xx_from_config\"\n").unwrap();
    let with_config = |envs: &[(&str, &str)], args: &[&str]| {
        let mut cmd = speak(Some(&home));
        cmd.env("XDG_CONFIG_HOME", &config).envs(envs.iter().copied()).args(args).arg("-o").arg(&out).arg("Hi.");
        run(&mut cmd)
    };
    let o = with_config(&[], &[]);
    assert!(o.status.code() == Some(4) && stderr(&o).contains("xx_from_config"), "{}", stderr(&o));
    let o = with_config(&[("SPEAK_VOICE", "xx_from_env")], &[]);
    assert!(stderr(&o).contains("xx_from_env"), "{}", stderr(&o));
    let o = with_config(&[("SPEAK_VOICE", "xx_from_env")], &["-v", "bm_george"]);
    assert!(o.status.success(), "{}", stderr(&o));
}
