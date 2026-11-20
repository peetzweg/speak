//! espeak-ng, always as a separate `espeak-ng` program (never linked or loaded in-process).
//!
//! Three users:
//! * non-English voices (`misaki_g2p`, the conversion Kokoro's own pipeline uses),
//! * English words misaki doesn't know, when espeak-ng is installed (`english_word`, see
//!   `english.rs`; without it, misaki's neural model reads them),
//! * the hidden `--g2p espeak` mode (`kokoro_onnx`), which reproduces exactly what kokoro-onnx
//!   0.4.7 feeds the model, for regression tests against its saved output.
//!
//! All three go through phonemizer-compatible punctuation handling: text is cut at punctuation
//! runs, each piece is phonemized on its own and the runs are put back. This is a reimplementation
//! of that observable behaviour, checked against kokoro-onnx's output in `tests/parity.rs`.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use regex::{Match, Regex};

/// An `espeak-ng` executable and, if it isn't the program's own, the directory that holds its
/// `espeak-ng-data` (what `--path` takes).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub path: PathBuf,
    pub data_parent: Option<PathBuf>,
}

impl Program {
    /// A command for this espeak-ng. With its own data directory it runs from there with
    /// `--path=.`: espeak-ng keeps the data path in a 160-byte buffer and silently cuts a longer
    /// one, so an archive unpacked deep in a directory tree would otherwise fail. Paths in the
    /// arguments must therefore be absolute.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        if let Some(d) = &self.data_parent {
            cmd.current_dir(d).arg("--path=.");
        }
        cmd
    }
}

/// Find espeak-ng: a copy bundled next to speak (`<exe>/../libexec/speak/espeak-ng`, data beside
/// it), then `$SPEAK_ESPEAK`, then `PATH`. `$SPEAK_ESPEAK_DATA` (an `espeak-ng-data` directory)
/// overrides the data for any of them.
pub fn find_program() -> Option<Program> {
    let exe = std::env::current_exe().ok().map(|e| std::fs::canonicalize(&e).unwrap_or(e));
    let env = |k| std::env::var_os(k).filter(|v: &OsString| !v.is_empty()).map(PathBuf::from);
    locate(exe.as_deref(), env("SPEAK_ESPEAK"), std::env::var_os("PATH"), env("SPEAK_ESPEAK_DATA"))
}

pub fn locate(exe: Option<&Path>, env_program: Option<PathBuf>, path_var: Option<OsString>, env_data: Option<PathBuf>) -> Option<Program> {
    let name = format!("espeak-ng{}", std::env::consts::EXE_SUFFIX);
    let bundled = exe.and_then(Path::parent).and_then(Path::parent).map(|prefix| prefix.join("libexec/speak"));
    let found = bundled
        .filter(|dir| dir.join(&name).is_file())
        .map(|dir| Program { path: dir.join(&name), data_parent: Some(dir) })
        .or_else(|| env_program.map(|path| Program { path, data_parent: None }))
        .or_else(|| {
            let path = std::env::split_paths(&path_var?).map(|d| d.join(&name)).find(|p| p.is_file())?;
            Some(Program { path, data_parent: None })
        });
    found.map(|mut p| {
        if let Some(data) = env_data {
            p.data_parent = Some(data.parent().map(Path::to_path_buf).unwrap_or(data));
        }
        p
    })
}

/// Turns one punctuation-free piece of text into espeak's IPA, clauses joined by spaces.
pub trait Engine: Send {
    fn phonemes(&mut self, text: &str) -> Result<String>;
}

/// A long-lived `espeak-ng -q --ipa` child. One line in; espeak-ng flushes its phonemes line by
/// line, and a sentinel word sent after each request marks where the answer ends.
pub struct Subprocess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    sentinel: String,
}

const SENTINEL: &str = "qxzspeaksentinel";

impl Subprocess {
    /// `tie`: join the letters of multi-letter phonemes with U+0361 instead of separating every
    /// phoneme with `_` (phonemizer's two output modes).
    pub fn spawn(program: &Program, lang: &str, tie: bool) -> Result<Self> {
        let mut cmd = program.command();
        cmd.args(["-q", "--ipa", if tie { "--tie" } else { "--sep=_" }, "-v", lang]);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("running {}", program.path.display()))?;
        let stdin = child.stdin.take().context("espeak-ng stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("espeak-ng stdout")?);
        let mut me = Self { child, stdin, stdout, sentinel: String::new() };
        writeln!(me.stdin, "{SENTINEL}")?;
        me.stdin.flush()?;
        me.sentinel = me.read_line()?;
        if me.sentinel.is_empty() {
            bail!("espeak-ng produced no output (is voice '{lang}' installed?)");
        }
        Ok(me)
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            bail!("espeak-ng exited unexpectedly");
        }
        Ok(line.trim_end_matches(['\n', '\r']).to_string())
    }
}

impl Engine for Subprocess {
    fn phonemes(&mut self, text: &str) -> Result<String> {
        writeln!(self.stdin, "{}\n{SENTINEL}", text.replace(['\n', '\r'], " "))?;
        self.stdin.flush()?;
        let mut clauses = Vec::new();
        loop {
            let line = self.read_line()?;
            if line == self.sentinel {
                return Ok(clauses.join(" "));
            }
            if !line.is_empty() {
                clauses.push(line);
            }
        }
    }
}

impl Drop for Subprocess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How phonemizer was configured by the caller being reproduced.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    /// Tied multi-letter phonemes (`a^ɪ`) instead of plain concatenation (`aɪ`).
    pub tie: bool,
    /// Drop espeak's language-switch flags such as `(en)`.
    pub remove_flags: bool,
}

const KOKORO_ONNX: Style = Style { tie: false, remove_flags: false };
const MISAKI_G2P: Style = Style { tie: true, remove_flags: true };
const MISAKI_FALLBACK: Style = Style { tie: true, remove_flags: false };

/// Python's `str.strip()`: Unicode whitespace plus the ASCII separators 0x1c..0x1f.
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Runs of punctuation (with the whitespace around them) that phonemizer keeps away from espeak.
fn punctuation_runs() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"(\s*[{}]+\s*)+", regex::escape(";:,.!?¡¿—…\"«»“”(){}[]"))).unwrap())
}

/// Clean up espeak's output for one piece of text: one space after every word, stress kept.
fn tidy(raw: &str, style: Style) -> String {
    static UNDERSCORES: OnceLock<Regex> = OnceLock::new();
    static FLAGS: OnceLock<Regex> = OnceLock::new();
    let line = py_strip(raw).replace('\n', " ").replace("  ", " ");
    // espeak-ng can leave stray separators at the end of a word.
    let line = UNDERSCORES.get_or_init(|| Regex::new("_+").unwrap()).replace_all(&line, "_").replace("_ ", " ");
    let line =
        if style.remove_flags { FLAGS.get_or_init(|| Regex::new(r"\(.+?\)").unwrap()).replace_all(&line, "").into_owned() } else { line };
    if line.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(line.len() + 8);
    for word in line.split(' ') {
        let word = py_strip(word);
        if style.tie {
            out.push_str(&word.replace('\u{361}', "^"));
        } else {
            out.extend(word.chars().filter(|&c| c != '_'));
        }
        out.push(' ');
    }
    out
}

fn ensure_trailing_space(mut s: String) -> String {
    if !s.ends_with(' ') {
        s.push(' ');
    }
    s
}

/// Phonemize `text` the way phonemizer's espeak backend does with punctuation preserved:
/// each non-blank line is cut at punctuation runs, the pieces between them are phonemized
/// separately, and the runs are put back verbatim. Lines are joined with `\n`.
pub fn phonemize(engine: &mut dyn Engine, text: &str, style: Style) -> Result<String> {
    let lines: Vec<&str> = text.split('\n').filter(|l| !py_strip(l).is_empty()).collect();
    let runs: Vec<Vec<Match>> = lines.iter().map(|l| punctuation_runs().find_iter(l).collect()).collect();
    // A line that is nothing but punctuation contributes no text to phonemize.
    let only_punctuation = |i: usize| runs[i].len() == 1 && runs[i][0].as_str() == lines[i];
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let later_runs = runs[i + 1..].iter().any(|r| !r.is_empty());
        let later_text = (i + 1..lines.len()).any(|j| !only_punctuation(j));
        if runs[i].is_empty() {
            let ph = tidy(&engine.phonemes(line)?, style);
            out.push(if later_runs { ph } else { ensure_trailing_space(ph) });
            continue;
        }
        if only_punctuation(i) {
            let mark = line.to_string();
            // Trailing punctuation-only lines keep no space after them.
            out.push(if later_text { ensure_trailing_space(mark) } else { mark });
            continue;
        }
        // Every piece followed by punctuation loses its trailing space; the run supplies spacing.
        let mut s = String::new();
        let mut cursor = 0;
        for m in &runs[i] {
            let piece = &line[cursor..m.start()];
            if !piece.is_empty() {
                s.push_str(tidy(&engine.phonemes(piece)?, style).trim_end_matches(' '));
            }
            s.push_str(m.as_str());
            cursor = m.end();
        }
        let tail = &line[cursor..];
        if tail.is_empty() {
            // Ends in punctuation: one space after it unless the run already ends in one.
            s = ensure_trailing_space(s);
        } else {
            let ph = tidy(&engine.phonemes(tail)?, style);
            // After a lone opening run ("(word", "¿hola") the word loses its space too, and gets
            // it back only when no later line has punctuation.
            let opening_only = runs[i].len() == 1 && runs[i][0].start() == 0;
            s.push_str(if opening_only { ph.trim_end_matches(' ') } else { &ph });
            if !later_runs {
                s = ensure_trailing_space(s);
            }
        }
        out.push(s);
    }
    Ok(out.join("\n"))
}

/// What kokoro-onnx 0.4.7 phonemizes `text` to: phonemizer with punctuation kept and stress
/// marks on. (kokoro-onnx then drops characters outside the vocabulary and strips; the model
/// does that for every G2P path.)
pub fn kokoro_onnx(engine: &mut dyn Engine, text: &str) -> Result<String> {
    phonemize(engine, py_strip(text), KOKORO_ONNX)
}

/// misaki's `EspeakG2P` (what Kokoro's own pipeline uses for es, fr, hi, it, pt-br, ja, zh):
/// tied espeak IPA with diphthongs and affricates mapped to Kokoro's single-letter symbols.
pub fn misaki_g2p(engine: &mut dyn Engine, text: &str) -> Result<String> {
    // Parentheses travel as guillemets so they survive as punctuation.
    let text = text.replace('«', "\u{201c}").replace('»', "\u{201d}").replace('(', "«").replace(')', "»");
    let mut ps = py_strip(&phonemize(engine, &text, MISAKI_G2P)?).to_string();
    // Sorted by key, as misaki iterates them.
    const E2M: &[(&str, &str)] = &[
        ("a^ɪ", "I"),
        ("a^ʊ", "W"),
        ("d^z", "ʣ"),
        ("d^ʒ", "ʤ"),
        ("e^ɪ", "A"),
        ("o^ʊ", "O"),
        ("s^s", "S"),
        ("t^s", "ʦ"),
        ("t^ʃ", "ʧ"),
        ("ə^ʊ", "Q"),
        ("ɔ^ɪ", "Y"),
    ];
    for (old, new) in E2M {
        ps = ps.replace(old, new);
    }
    Ok(ps.replace(['^', '-'], "").replace('«', "(").replace('»', ")"))
}

/// misaki's `EspeakFallback` for one English word: tied espeak IPA converted to misaki's
/// alphabet (US or British conventions).
pub fn english_word(engine: &mut dyn Engine, word: &str, british: bool) -> Result<String> {
    let ps = phonemize(engine, word, MISAKI_FALLBACK)?;
    Ok(espeak_to_misaki(py_strip(&ps), british))
}

/// The espeak → misaki symbol conversion of misaki's `EspeakFallback` (misaki 0.9.4).
pub fn espeak_to_misaki(ps: &str, british: bool) -> String {
    // Longest first; equal lengths keep misaki's order.
    const E2M: &[(&str, &str)] = &[
        ("ʔˌn\u{329}", "ʔn"),
        ("ʔn\u{329}", "ʔn"),
        ("a^ɪ", "I"),
        ("a^ʊ", "W"),
        ("d^ʒ", "ʤ"),
        ("e^ɪ", "A"),
        ("t^ʃ", "ʧ"),
        ("ɔ^ɪ", "Y"),
        ("ə^l", "ᵊl"),
        ("ʲo", "jo"),
        ("ʲə", "jə"),
        ("e", "A"),
        ("ʲ", ""),
        ("ɚ", "əɹ"),
        ("r", "ɹ"),
        ("x", "k"),
        ("ç", "k"),
        ("ɐ", "ə"),
        ("ɬ", "l"),
        ("\u{303}", ""),
    ];
    let mut ps = ps.to_string();
    for (old, new) in E2M {
        ps = ps.replace(old, new);
    }
    // A syllabic mark makes the consonant before it a schwa + consonant: `n̩` → `ᵊn`.
    let mut out = String::with_capacity(ps.len());
    let mut chars = ps.chars().peekable();
    while let Some(c) = chars.next() {
        if chars.peek() == Some(&'\u{329}') && !c.is_whitespace() {
            chars.next();
            out.push('ᵊ');
            out.push(c);
        } else if c != '\u{329}' {
            out.push(c);
        }
    }
    let mut ps = out;
    if british {
        ps = ps.replace("e^ə", "ɛː").replace("iə", "ɪə").replace("ə^ʊ", "Q");
    } else {
        ps = ps.replace("o^ʊ", "O").replace("ɜːɹ", "ɜɹ").replace("ɜː", "ɜɹ").replace("ɪə", "iə").replace('ː', "");
    }
    ps.replace('o', "ɔ").replace('ɾ', "T").replace('ʔ', "t").replace('^', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what it is asked and answers with the input upper-cased, one "phoneme" per letter.
    struct Echo(Vec<String>);
    impl Engine for Echo {
        fn phonemes(&mut self, text: &str) -> Result<String> {
            self.0.push(text.to_string());
            Ok(text
                .split_whitespace()
                .map(|w| w.to_uppercase().chars().map(String::from).collect::<Vec<_>>().join("_"))
                .collect::<Vec<_>>()
                .join(" "))
        }
    }

    fn run(text: &str) -> (String, Vec<String>) {
        let mut e = Echo(Vec::new());
        let out = phonemize(&mut e, text, KOKORO_ONNX).unwrap();
        (out, e.0)
    }

    #[test]
    fn punctuation_is_kept_away_from_espeak() {
        let (out, asked) = run("hello, my world!");
        assert_eq!(asked, vec!["hello", "my world"]);
        assert_eq!(out, "HELLO, MY WORLD! ");
        // "2,000" reaches espeak as "2" and "000": the kokoro-onnx number quirk.
        assert_eq!(run("had 2,000 bots.").1, vec!["had 2", "000 bots"]);
    }

    #[test]
    fn opening_and_inner_runs() {
        assert_eq!(run("(hi) there").0, "(HI) THERE ");
        assert_eq!(run("¿hola").0, "¿HOLA ");
        assert_eq!(run("“quoted,” she said — then left… quietly.").0, "“QUOTED,” SHE SAID — THEN LEFT… QUIETLY. ");
    }

    #[test]
    fn punctuation_only_and_multiple_lines() {
        assert_eq!(run("?!").0, "?!");
        assert_eq!(run("one.\n\ntwo").0, "ONE. \nTWO ");
        assert_eq!(run("hi\n...").0, "HI \n...");
        assert_eq!(run("...\nhi").0, "... \nHI ");
        // A line opened by punctuation loses its trailing space while later lines have punctuation.
        assert_eq!(run("(a\nb.").0, "(A\nB. ");
    }

    #[test]
    fn tidy_modes() {
        assert_eq!(tidy("h_ə_l_ˈoʊ w_ˈɜː_l_d__ ", KOKORO_ONNX), "həlˈoʊ wˈɜːld ");
        assert_eq!(tidy("həlˈo\u{361}ʊ (en)wˈɜːld(fr)", MISAKI_G2P), "həlˈo^ʊ wˈɜːld ");
        assert_eq!(tidy("", KOKORO_ONNX), "");
    }

    #[test]
    fn misaki_conversions() {
        assert_eq!(espeak_to_misaki("həlˈo^ʊ", false), "həlˈO");
        assert_eq!(espeak_to_misaki("wˈɜːld", false), "wˈɜɹld");
        assert_eq!(espeak_to_misaki("ˈɛnd^ʒɪn", false), "ˈɛnʤɪn");
        assert_eq!(espeak_to_misaki("bˈʌʔn\u{329}", false), "bˈʌtn");
        assert_eq!(espeak_to_misaki("bˈɒtl\u{329}", true), "bˈɒtᵊl");
        assert_eq!(espeak_to_misaki("ɡˈə^ʊ", true), "ɡˈQ");
        assert_eq!(espeak_to_misaki("bˈɛɾɚ", false), "bˈɛTəɹ");
    }

    #[test]
    fn lookup_order() {
        let dir = std::env::temp_dir().join(format!("speak-locate-{}", std::process::id()));
        let libexec = dir.join("prefix/libexec/speak");
        let bin = dir.join("prefix/bin");
        let other = dir.join("other");
        for d in [&libexec, &bin, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        let name = format!("espeak-ng{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(other.join(&name), "").unwrap();
        let exe = bin.join("speak");
        let path_var = Some(other.clone().into_os_string());

        // PATH only.
        let p = locate(Some(&exe), None, path_var.clone(), None).unwrap();
        assert_eq!(p, Program { path: other.join(&name), data_parent: None });
        // $SPEAK_ESPEAK beats PATH.
        let p = locate(Some(&exe), Some("/x/espeak-ng".into()), path_var.clone(), None).unwrap();
        assert_eq!(p.path, PathBuf::from("/x/espeak-ng"));
        // The bundled copy beats both and brings its data directory.
        std::fs::write(libexec.join(&name), "").unwrap();
        let p = locate(Some(&exe), Some("/x/espeak-ng".into()), path_var.clone(), None).unwrap();
        assert_eq!(p, Program { path: libexec.join(&name), data_parent: Some(libexec.clone()) });
        // $SPEAK_ESPEAK_DATA names espeak-ng-data itself; --path takes its parent.
        let p = locate(Some(&exe), None, path_var, Some("/d/espeak-ng-data".into())).unwrap();
        assert_eq!(p.data_parent, Some(PathBuf::from("/d")));
        assert_eq!(locate(None, None, None, None), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
