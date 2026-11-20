//! speak: say(1), but with the best open text-to-speech model, running on your own machine.
//!
//! Exit codes (stable, for scripts and agents):
//!   0 ok · 1 synthesis or audio failure · 2 usage · 3 model or espeak-ng missing · 4 unknown voice

mod audio;
mod backend;
mod config;
mod deferred;
mod fallback;
mod g2p;
mod kokoro;
mod lexicon;
mod models;
mod normalize;
mod npz;
mod onnx;
mod paths;
mod picker;
mod text;
mod trim;
mod vocab;

use std::ffi::OsString;
use std::fmt::Display;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::Parser;

use backend::{Backend, Missing, Unusable};
use config::Config;
use g2p::Mode;
use lexicon::Lexicon;
use models::Model;

const AFTER_HELP: &str = "\
More:
  speak voices                     pick the default voice (plain list when not on a terminal)
  speak models                     list models; speak models download|rm NAME
  speak lexicon add WORD SPOKEN…   pronunciations; also rm WORD | list [--builtin] | try WORD | path

Environment: SPEAK_VOICE, SPEAK_MODEL, SPEAK_HOME (models, default ~/.local/share/speak),
SPEAK_LEXICON, SPEAK_ESPEAK (espeak-ng program), SPEAK_NO_DOWNLOAD=1.
Exit codes: 0 ok, 1 synthesis or audio failure, 2 usage, 3 model or espeak-ng missing, 4 unknown voice.";

#[derive(Parser, Debug)]
#[command(name = "speak", version, about = "say(1) with a local neural voice. Nothing leaves the machine.", after_help = AFTER_HELP)]
struct Args {
    /// Text to speak; read from stdin when there is none
    text: Vec<String>,
    /// Voice, or '?' to list the voices
    #[arg(short, long)]
    voice: Option<String>,
    /// Words per minute, like say (175 = normal speed)
    #[arg(short, long, value_name = "WPM")]
    rate: Option<f32>,
    /// Speed multiplier, 0.5 to 2.0 (overrides -r)
    #[arg(long)]
    speed: Option<f32>,
    /// Read the text from a file ('-' for stdin)
    #[arg(short = 'f', long = "input-file", value_name = "FILE")]
    input_file: Option<PathBuf>,
    /// Write audio to a file instead of playing it (.wav; other formats via ffmpeg)
    #[arg(short = 'o', long = "output-file", value_name = "FILE")]
    output_file: Option<PathBuf>,
    /// Model to use (default: the best one installed)
    #[arg(short, long)]
    model: Option<String>,
    /// Print the phonemes, batches and token ids as JSON instead of speaking
    #[arg(long)]
    analyze: bool,
    /// Print timings as one JSON line on stderr
    #[arg(long)]
    timing: bool,
    /// G2P: auto (misaki for English) or espeak (kokoro-onnx's exact path, for regression tests)
    #[arg(long, hide = true, value_parser = ["auto", "espeak"])]
    g2p: Option<String>,

    // say(1) flags that are accepted and ignored, so scripts written for say keep working.
    #[arg(short = 'a', long = "audio-device", hide = true)]
    _audio_device: Option<String>,
    #[arg(short = 'n', long = "network-send", hide = true)]
    _network_send: Option<String>,
    #[arg(short = 'i', long = "interactive", hide = true, num_args = 0..=1, require_equals = true, default_missing_value = "")]
    _interactive: Option<String>,
    #[arg(long = "progress", hide = true)]
    _progress: bool,
    #[arg(long = "file-format", hide = true)]
    _file_format: Option<String>,
    #[arg(long = "data-format", hide = true)]
    _data_format: Option<String>,
    #[arg(long = "channels", hide = true)]
    _channels: Option<String>,
    #[arg(long = "bit-rate", hide = true)]
    _bit_rate: Option<String>,
    #[arg(long = "quality", hide = true)]
    _quality: Option<String>,
}

/// A failed run: the exit code and a one-line message (empty if already reported).
struct Exit {
    code: u8,
    msg: String,
}

fn exit(code: u8, msg: impl Display) -> Exit {
    Exit { code, msg: msg.to_string() }
}

impl From<anyhow::Error> for Exit {
    fn from(e: anyhow::Error) -> Self {
        exit(if is_missing(&e) { 3 } else { 1 }, format!("{e:#}"))
    }
}

fn is_missing(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Missing>().is_some() || e.downcast_ref::<Unusable>().is_some()
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().collect();
    let rest = argv.get(2..).unwrap_or_default();
    let result = match argv.get(1).and_then(|a| a.to_str()) {
        Some("lexicon" | "lex") => lexicon_cmd(rest),
        Some("voices") => voices_cmd(&argv[1..]),
        Some("models") => models_cmd(rest),
        _ => parse::<Args>(&argv).and_then(speak),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !e.msg.is_empty() {
                eprintln!("speak: {}", e.msg);
            }
            ExitCode::from(e.code)
        }
    }
}

/// clap, but with usage errors as one `speak: …` line and exit code 2.
fn parse<P: Parser>(argv: &[OsString]) -> Result<P, Exit> {
    P::try_parse_from(argv).map_err(|e| {
        use clap::error::ErrorKind;
        if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
            let _ = e.print();
            return exit(0, "");
        }
        let first = e.to_string().lines().next().unwrap_or_default().trim_start_matches("error: ").to_string();
        exit(2, format!("{first} (see speak --help)"))
    })
}

fn model_for(flag: Option<String>, cfg: &Config, home: &Path) -> Result<&'static Model, Exit> {
    match config::resolve(flag, env("SPEAK_MODEL"), cfg.model.clone()) {
        Some(name) => models::find(&name).ok_or_else(|| exit(2, format!("unknown model '{name}' (available: {})", models::names()))),
        None => Ok(models::best(home)),
    }
}

/// The backend's default voice (every model so far is a Kokoro build).
fn default_voice(_model: &Model) -> &'static str {
    kokoro::DEFAULT_VOICE
}

fn read_text(a: &Args) -> Result<String, Exit> {
    let mut s = String::new();
    match &a.input_file {
        Some(p) if p.as_os_str() == "-" => std::io::stdin().read_to_string(&mut s).map(|_| s).map_err(|e| exit(1, e)),
        Some(p) => std::fs::read_to_string(p).map_err(|e| exit(1, format!("{}: {e}", p.display()))),
        None if !a.text.is_empty() => Ok(a.text.join(" ")),
        None if !std::io::stdin().is_terminal() => std::io::stdin().read_to_string(&mut s).map(|_| s).map_err(|e| exit(1, e)),
        None => Err(exit(2, "no text (pass it as arguments, with -f, or on stdin)")),
    }
}

fn speak(a: Args) -> Result<(), Exit> {
    let t0 = Instant::now();
    let home = paths::data_home();
    let cfg = Config::load(&paths::config_file());
    let model = model_for(a.model.clone(), &cfg, &home)?;
    let mode = if a.g2p.as_deref() == Some("espeak") { Mode::Espeak } else { Mode::Auto };
    let voice = config::resolve(a.voice.clone(), env("SPEAK_VOICE"), cfg.voice.clone()).unwrap_or_else(|| default_voice(model).into());
    if voice == "?" {
        return list_voices(model, &home);
    }

    let raw = text::strip_say_commands(&read_text(&a)?);
    if raw.trim().is_empty() {
        return Ok(());
    }
    let markup = lexicon::to_markup(&lexicon_for(&Lexicon::load(&paths::lexicon_file()), &voice, mode).apply(&raw));
    if a.analyze {
        return analyze(&markup, &voice, mode, a.timing);
    }
    let speed = a.speed.or(a.rate.map(|r| r / 175.0)).unwrap_or(1.0).clamp(0.5, 2.0);
    let wpm = a.rate.or(a.speed.map(|s| s * 175.0));
    let out = a.output_file.as_deref().map(audio::file_out).transpose().map_err(|e| exit(1, format!("{e:#}")))?;

    // Without the model, or without espeak-ng where a voice needs it, the system voice speaks.
    let say_instead = |why: anyhow::Error| speak_with_system_voice(&why, &markup, wpm, a.output_file.as_deref());
    if g2p::needs_espeak(mode, &voice) && g2p::espeak::find_program().is_none() {
        let why = anyhow::Error::new(Missing(format!("espeak-ng not found, and voice {voice} needs it (install it or set SPEAK_ESPEAK)")));
        return if mode == Mode::Espeak { Err(why.into()) } else { say_instead(why) };
    }
    let download = env("SPEAK_NO_DOWNLOAD").is_none();
    let opened = models::ensure(&home, model, download).and_then(|files| backend::open(model, files, &voice, mode));
    let mut be = match opened {
        Ok(be) => be,
        Err(e) if is_missing(&e) => return say_instead(e),
        Err(e) => return Err(e.into()),
    };
    match be.voices() {
        Ok(vs) if vs.iter().any(|v| v.id == voice) => {}
        Ok(_) => return Err(exit(4, format!("unknown voice '{voice}' (try: speak -v '?')"))),
        Err(e) => return Err(e.into()),
    }

    let rate = be.info().sample_rate;
    let sentences = text::chunks(&markup);
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(8);
    // Synthesis runs ahead on its own thread; playback starts with the first sentence.
    let synth = std::thread::spawn(move || {
        let mut t = Timing::default();
        let result = (|| -> anyhow::Result<()> {
            let gap = vec![0.0f32; (0.12 * rate as f64) as usize];
            for (i, s) in sentences.iter().enumerate() {
                if i > 0 && tx.send(gap.clone()).is_err() {
                    return Ok(());
                }
                for chunk in be.synthesize(s, &voice, speed)? {
                    let chunk = chunk?;
                    t.first_chunk.get_or_insert(t0.elapsed());
                    t.samples += chunk.samples.len();
                    if tx.send(chunk.samples).is_err() {
                        return Ok(()); // the player stopped
                    }
                }
            }
            Ok(())
        })();
        t.synth_done = Some(t0.elapsed());
        t.stats = be.stats();
        (result, t)
    });
    let played = match &out {
        Some(file) => {
            let pcm: Vec<f32> = rx.iter().flatten().collect();
            if pcm.is_empty() { Ok(()) } else { audio::write(file, &pcm, rate) }
        }
        None => audio::play(rx, rate),
    };
    let (synthesized, t) = synth.join().map_err(|_| exit(1, "synthesis thread panicked"))?;
    if let Err(e) = synthesized {
        // The model failed to load: nothing was said yet, so the system voice can still say it.
        if t.samples == 0 && is_missing(&e) {
            return say_instead(e);
        }
        return Err(e.into());
    }
    played.map_err(|e| exit(1, format!("{e:#}")))?;
    if a.timing || env("SPEAK_TIMING").is_some() {
        t.print(t0, rate, model.name);
    }
    Ok(())
}

/// The pronunciations for `voice`: the user's, over speak's built-in English ones for an English
/// voice (not in `--g2p espeak`, which reproduces kokoro-onnx).
fn lexicon_for(user: &Lexicon, voice: &str, mode: Mode) -> Lexicon {
    if mode == Mode::Auto && g2p::language(voice).starts_with("en") { user.over(lexicon::builtin()) } else { user.clone() }
}

fn speak_with_system_voice(why: &anyhow::Error, markup: &str, wpm: Option<f32>, out: Option<&Path>) -> Result<(), Exit> {
    eprintln!("speak: {why:#}; using the system voice");
    fallback::speak(&lexicon::plain(markup), wpm, out).map_err(|e| exit(3, format!("the system voice failed too: {e:#}")))
}

#[derive(Default)]
struct Timing {
    first_chunk: Option<Duration>,
    synth_done: Option<Duration>,
    samples: usize,
    stats: backend::Stats,
}

impl Timing {
    fn print(&self, t0: Instant, rate: u32, model: &str) {
        let ms = |d: Option<Duration>| d.map_or(-1.0, |d| d.as_secs_f64() * 1e3);
        let first_played = audio::FIRST_SAMPLE_AT.get().map(|x| x.duration_since(t0));
        let audio_s = self.samples as f64 / rate as f64;
        eprintln!(
            "{{\"model\": \"{model}\", \"first_chunk_ms\": {:.0}, \"first_sample_played_ms\": {:.0}, \"load_wait_ms\": {:.0}, \"infer_ms\": {:.0}, \"synth_done_ms\": {:.0}, \"total_ms\": {:.0}, \"audio_s\": {audio_s:.3}, \"rtf\": {:.3}}}",
            ms(self.first_chunk),
            ms(first_played),
            self.stats.load_wait_ms,
            self.stats.infer_ms,
            ms(self.synth_done),
            t0.elapsed().as_secs_f64() * 1e3,
            if audio_s > 0.0 { self.stats.infer_ms / 1e3 / audio_s } else { 0.0 },
        );
    }
}

/// `--analyze`: what the model would be fed, as JSON. Needs G2P only, no model files.
fn analyze(markup: &str, voice: &str, mode: Mode, timing: bool) -> Result<(), Exit> {
    let t = Instant::now();
    let mut g2p = g2p::G2p::new(mode);
    if g2p::needs_espeak(mode, voice) && g2p.espeak().is_none() {
        return Err(exit(3, format!("espeak-ng not found, and voice {voice} needs it (install it or set SPEAK_ESPEAK)")));
    }
    let a = kokoro::analyze(&mut g2p, markup, voice)?;
    let batches: Vec<String> = a.batches.iter().map(|b| text::json_str(b)).collect();
    let tokens: Vec<String> =
        a.tokens.iter().map(|t| format!("[{}]", t.iter().map(i64::to_string).collect::<Vec<_>>().join(", "))).collect();
    println!(
        "{{\"phonemes\": {}, \"batches\": [{}], \"tokens\": [{}]}}",
        text::json_str(&a.phonemes),
        batches.join(", "),
        tokens.join(", ")
    );
    if timing {
        eprintln!("{{\"g2p_ms\": {:.1}}}", t.elapsed().as_secs_f64() * 1e3);
    }
    Ok(())
}

fn open_for_voices(model: &'static Model, home: &Path, voice: &str) -> Result<Box<dyn Backend>, Exit> {
    let files = models::ensure(home, model, env("SPEAK_NO_DOWNLOAD").is_none())?;
    Ok(backend::open(model, files, voice, Mode::Auto)?)
}

/// `-v '?'`: one voice per line, like say.
fn list_voices(model: &'static Model, home: &Path) -> Result<(), Exit> {
    let be = open_for_voices(model, home, default_voice(model))?;
    for v in be.voices()? {
        println!("{:<16} {}", v.id, v.label);
    }
    Ok(())
}

#[derive(Parser, Debug)]
#[command(name = "speak voices", about = "Pick the default voice: ↑/↓ browse, space to hear, enter to save.")]
struct VoicesArgs {
    /// Model whose voices to show
    #[arg(short, long)]
    model: Option<String>,
}

fn voices_cmd(argv: &[OsString]) -> Result<(), Exit> {
    let a: VoicesArgs = parse(argv)?;
    let home = paths::data_home();
    let cfg = Config::load(&paths::config_file());
    let model = model_for(a.model, &cfg, &home)?;
    let current = config::resolve(None, env("SPEAK_VOICE"), cfg.voice.clone()).unwrap_or_else(|| default_voice(model).into());
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return list_voices(model, &home); // agents and pipes get the plain list
    }
    let be = open_for_voices(model, &home, &current)?;
    match picker::run(be, model.name, &current)? {
        Some(v) => {
            let path = paths::config_file();
            config::set(&path, "voice", &v).map_err(|e| exit(1, format!("{}: {e}", path.display())))?;
            eprintln!("speak: default voice is now {v} ({})", path.display());
            Ok(())
        }
        None => Ok(()),
    }
}

fn models_cmd(args: &[OsString]) -> Result<(), Exit> {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let home = paths::data_home();
    let named = |name: &str| models::find(name).ok_or_else(|| exit(2, format!("unknown model '{name}' (available: {})", models::names())));
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["list" | "ls"] => {
            let cfg = Config::load(&paths::config_file());
            let current = model_for(None, &cfg, &home).ok().map(|m| m.name);
            for m in models::CATALOG {
                let status = if models::is_installed(&home, m) { "installed" } else { "available" };
                let default = if Some(m.name) == current { "  (default)" } else { "" };
                println!("{:<12} {:<9} {:>7}  {}{default}", m.name, status, models::megabytes(m.size()), m.summary);
            }
            Ok(())
        }
        ["download" | "pull", name] => {
            let m = named(name)?;
            let fetched = models::install(&home, m).map_err(|e| exit(1, format!("{e:#}")))?;
            let verb = if fetched.is_empty() { "already installed" } else { "installed" };
            println!("{} {verb} in {}", m.name, models::dir(&home).display());
            Ok(())
        }
        ["rm" | "remove", name] => {
            let m = named(name)?;
            let removed = models::remove(&home, m).map_err(|e| exit(1, format!("{e:#}")))?;
            if removed.is_empty() {
                return Err(exit(1, format!("{} is not installed in {}", m.name, models::dir(&home).display())));
            }
            for p in removed {
                println!("removed {}", p.display());
            }
            Ok(())
        }
        _ => Err(exit(2, "usage: speak models [list] | download NAME | rm NAME")),
    }
}

/// speak.py's `lexicon` command: same file, same output.
fn lexicon_cmd(args: &[OsString]) -> Result<(), Exit> {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let path = paths::lexicon_file();
    let mut lex = Lexicon::load(&path);
    let save = |lex: &Lexicon| lex.save(&path).map_err(|e| exit(1, format!("{}: {e}", path.display())));
    let (sub, rest) = args.split_first().map_or(("list", &[][..]), |(s, r)| (s.as_str(), r));
    match (sub, rest) {
        ("add" | "set", [word, spoken @ ..]) if !spoken.is_empty() => {
            let spoken = spoken.join(" ");
            if let Some(ph) = lexicon::phonemes_of(&spoken) {
                let unknown: String = ph.chars().filter(|&c| vocab::id(c).is_none()).collect();
                if !unknown.is_empty() {
                    return Err(exit(
                        2,
                        format!("phonemes contain symbols Kokoro doesn't know: {unknown} (use the misaki/Kokoro alphabet, e.g. /kˈOkəɹO/)"),
                    ));
                }
            }
            lex.add(word, &spoken);
            save(&lex)?;
            println!("{word} = {spoken}");
        }
        ("rm" | "remove" | "del", [word]) => {
            if !lex.remove(word) {
                let builtin =
                    if lexicon::builtin().get(word).is_some() { "; it is built in, and an entry you add replaces it" } else { "" };
                return Err(exit(1, format!("no entry for '{word}'{builtin}")));
            }
            save(&lex)?;
            println!("removed {word}");
        }
        ("list" | "ls", flags @ ([] | [_])) => {
            let entries = match flags {
                [] => lex.sorted(),
                [f] if f == "--builtin" => lexicon::builtin().sorted(),
                _ => return Err(exit(2, "usage: speak lexicon list [--builtin]")),
            };
            let width = entries.iter().map(|(w, _)| w.chars().count()).max().unwrap_or(0);
            for (w, s) in entries {
                println!("{w:<width$}  =  {s}");
            }
        }
        ("path", []) => println!("{}", path.display()),
        ("try", words) if !words.is_empty() => {
            let word = words.join(" ");
            let cfg = Config::load(&paths::config_file());
            let voice = config::resolve(None, env("SPEAK_VOICE"), cfg.voice).unwrap_or_else(|| kokoro::DEFAULT_VOICE.into());
            let (line, markup) = try_entry(&lex, &lexicon_for(&lex, &voice, Mode::Auto), &word);
            println!("{line}");
            if let Ok(a) = kokoro::analyze(&mut g2p::G2p::new(Mode::Auto), &markup, &voice) {
                println!("/{}/", a.phonemes);
            }
            let mut argv: Vec<OsString> = vec!["speak".into(), "--".into()];
            argv.push(word.into());
            return speak(parse(&argv)?);
        }
        _ => return Err(exit(2, "usage: speak lexicon add WORD SPOKEN... | rm WORD | list [--builtin] | try WORD | path")),
    }
    Ok(())
}

/// `lexicon try`'s first line (what `word` becomes under `lex`, the user's entries `user` over
/// any built-in ones, and whether built-in entries did it), and the markup it is spoken from.
fn try_entry(user: &Lexicon, lex: &Lexicon, word: &str) -> (String, String) {
    let (segments, used) = lex.apply_traced(word);
    let markup = lexicon::to_markup(&segments);
    let spoken: String = segments
        .into_iter()
        .map(|s| match s {
            lexicon::Segment::Text(t) => t,
            lexicon::Segment::Phonemes { phonemes, .. } => format!("/{phonemes}/"),
        })
        .collect();
    if used.is_empty() {
        return (format!("{word} (no entry, as spelled)"), markup);
    }
    let builtin: Vec<&str> = used.into_iter().filter(|w| user.get(w).is_none()).collect();
    let line = match builtin.as_slice() {
        [] => format!("{word} -> {spoken}"),
        [w] if w.to_lowercase() == word.to_lowercase() => format!("{word} -> {spoken} (built in)"),
        ws => format!("{word} -> {spoken} (built in: {})", ws.join(", ")),
    };
    (line, markup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexicon_try_says_when_a_builtin_entry_did_it() {
        let user = Lexicon::parse("kruapi = crew A P I\n");
        let lex = user.over(lexicon::builtin());
        let line = |word: &str| try_entry(&user, &lex, word).0;
        assert_eq!(line("Redis"), "Redis -> /ɹˈɛdɪs/ (built in)");
        assert_eq!(line("KruAPI"), "KruAPI -> crew A P I");
        assert_eq!(line("kubectl and KruAPI"), "kubectl and KruAPI -> cube control and crew A P I (built in: kubectl)");
        assert_eq!(line("hello"), "hello (no entry, as spelled)");
        assert_eq!(try_entry(&user, &lex, "Redis").1, "[Redis](/ɹˈɛdɪs/)");
        // The user's entry for a built-in word is the user's.
        let user = Lexicon::parse("redis = ree dis\n");
        assert_eq!(try_entry(&user, &user.over(lexicon::builtin()), "redis").0, "redis -> ree dis");
    }

    #[test]
    fn builtin_lexicon_is_for_english_in_the_normal_mode() {
        let user = Lexicon::parse("kruapi = crew A P I\n");
        assert!(lexicon_for(&user, "af_heart", Mode::Auto).get("redis").is_some());
        assert!(lexicon_for(&user, "bm_george", Mode::Auto).get("redis").is_some());
        assert_eq!(lexicon_for(&user, "ff_siwis", Mode::Auto), user);
        assert_eq!(lexicon_for(&user, "af_heart", Mode::Espeak), user);
    }
}
