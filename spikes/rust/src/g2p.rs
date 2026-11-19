//! Grapheme-to-phoneme via espeak-ng, reproducing exactly what kokoro-onnx 0.4.7 gets from
//! `phonemizer.phonemize(text, lang, preserve_punctuation=True, with_stress=True)`
//! (phonemizer-fork 3.3.1, espeak backend, default separator, strip=False, keep-flags).
//!
//! The quirky parts are deliberate ports, not bugs of this file: phonemizer hides punctuation from
//! espeak by splitting the text at every punctuation run (so "2,000" is phonemized as "2" and
//! "000") and glues the marks back afterwards. Matching that is what makes token ids identical.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use regex::Regex;

/// phonemizer's `Punctuation.default_marks()`.
const MARKS: &str = ";:,.!?¡¿—…\"«»“”(){}[]";

/// Something that turns one punctuation-free chunk into espeak's IPA, phonemes separated by '_',
/// clauses joined by ' ' (what phonemizer's `EspeakWrapper.text_to_phonemes` returns).
pub trait Engine: Send {
    fn clause_phonemes(&mut self, text: &str) -> Result<String>;
}

/// Default engine: the `espeak-ng` program as a long-lived child process (keeps speak's own
/// binary free of GPL code). One line in, one line per clause out; a sentinel line marks the end.
pub struct Subprocess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    sentinel: String,
}

const SENTINEL_TEXT: &str = "qxzspeaksentinel";

impl Subprocess {
    /// `program`: path to espeak-ng; `data`: optional espeak-ng-data dir (default: the program's own).
    pub fn spawn(program: &Path, lang: &str, data: Option<&Path>) -> Result<Self> {
        let mut cmd = Command::new(program);
        cmd.args(["-q", "--ipa", "--sep=_", "-v", lang]);
        if let Some(d) = data {
            cmd.arg(format!("--path={}", d.parent().unwrap_or(d).display()));
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("running {} (install espeak-ng or set SPEAK_ESPEAK)", program.display()))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut me = Self { child, stdin, stdout, sentinel: String::new() };
        writeln!(me.stdin, "{SENTINEL_TEXT}")?;
        me.stdin.flush()?;
        me.sentinel = me.read_line()?;
        if me.sentinel.is_empty() {
            bail!("espeak-ng produced no output (voice '{lang}' missing?)");
        }
        Ok(me)
    }

    fn read_line(&mut self) -> Result<String> {
        let mut l = String::new();
        if self.stdout.read_line(&mut l)? == 0 {
            bail!("espeak-ng exited");
        }
        Ok(l.trim_end_matches(['\n', '\r']).to_string())
    }
}

impl Engine for Subprocess {
    fn clause_phonemes(&mut self, text: &str) -> Result<String> {
        // Each stdin line is synthesised separately (espeak-ng.c line mode, flushed per line).
        writeln!(self.stdin, "{}", text.replace(['\n', '\r'], " "))?;
        writeln!(self.stdin, "{SENTINEL_TEXT}")?;
        self.stdin.flush()?;
        let mut out: Vec<String> = Vec::new();
        loop {
            let l = self.read_line()?;
            if l == self.sentinel {
                break;
            }
            if !l.is_empty() {
                out.push(l);
            }
        }
        Ok(out.join(" "))
    }
}

impl Drop for Subprocess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Find espeak-ng: $SPEAK_ESPEAK, else `espeak-ng` on PATH.
pub fn find_program() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SPEAK_ESPEAK") {
        return Some(PathBuf::from(p));
    }
    let exe = if cfg!(windows) { "espeak-ng.exe" } else { "espeak-ng" };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(exe)).find(|p| p.is_file())
}

/// Optional in-process engine (cargo feature `espeak-inprocess`): libespeak-ng 1.52.0 compiled from
/// source by build.rs and linked statically. Makes the binary a GPL-3.0 combination; kept for
/// the parity comparison and as a measured alternative.
#[cfg(feature = "espeak-inprocess")]
pub mod inprocess {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;
    use std::sync::Mutex;

    use anyhow::{Result, bail};

    #[allow(non_camel_case_types, non_upper_case_globals, dead_code)]
    mod sys {
        use std::ffi::{c_char, c_int, c_uchar, c_void};
        #[repr(C)]
        pub struct espeak_VOICE {
            pub name: *const c_char,
            pub languages: *const c_char,
            pub identifier: *const c_char,
            pub gender: c_uchar,
            pub age: c_uchar,
            pub variant: c_uchar,
            pub xx1: c_uchar,
            pub score: c_int,
            pub spare: *mut c_void,
        }
        pub const AUDIO_OUTPUT_SYNCHRONOUS: c_int = 2;
        pub const espeakINITIALIZE_DONT_EXIT: c_int = 0x8000;
        pub const espeakCHARS_UTF8: c_int = 1;
        unsafe extern "C" {
            pub fn espeak_Initialize(output: c_int, buflength: c_int, path: *const c_char, options: c_int) -> c_int;
            pub fn espeak_ListVoices(spec: *mut espeak_VOICE) -> *mut *const espeak_VOICE;
            pub fn espeak_SetVoiceByName(name: *const c_char) -> c_int;
            pub fn espeak_TextToPhonemes(textptr: *mut *const c_void, textmode: c_int, phonememode: c_int) -> *const c_char;
        }
    }

    /// espeak-ng keeps global state and is not thread-safe; every call goes through this lock.
    static ESPEAK: Mutex<Option<String>> = Mutex::new(None);

    pub struct InProcess {
        lang: String,
    }

    impl InProcess {
        pub fn new(data_dir: &Path, lang: &str) -> Result<Self> {
            let mut state = ESPEAK.lock().unwrap();
            // espeak-ng wants the *parent* of espeak-ng-data and silently truncates paths at 160 bytes.
            let parent = data_dir.parent().unwrap_or(data_dir).to_string_lossy().into_owned();
            if parent.len() >= 160 {
                bail!("espeak-ng data path is {} bytes; espeak-ng truncates at 160", parent.len());
            }
            if state.is_none() {
                let c = CString::new(parent)?;
                // DONT_EXIT: otherwise espeak calls exit(1) on a bad data path (phonemizer passes 0).
                let rate = unsafe { sys::espeak_Initialize(sys::AUDIO_OUTPUT_SYNCHRONOUS, 0, c.as_ptr(), sys::espeakINITIALIZE_DONT_EXIT) };
                if rate <= 0 {
                    bail!("espeak-ng failed to initialise with data at {}", data_dir.display());
                }
                *state = Some(String::new());
            }
            set_voice(&mut state, lang)?;
            Ok(Self { lang: lang.to_string() })
        }
    }

    fn set_voice(state: &mut Option<String>, lang: &str) -> Result<()> {
        if state.as_deref() == Some(lang) {
            return Ok(());
        }
        let c = CString::new(voice_identifier(lang)?)?;
        if unsafe { sys::espeak_SetVoiceByName(c.as_ptr()) } != 0 {
            bail!("espeak-ng could not load voice for {lang}");
        }
        *state = Some(lang.to_string());
        Ok(())
    }

    /// First voice (espeak sorts by relevance) whose primary language equals `lang`, like
    /// phonemizer's `EspeakWrapper.set_voice`.
    fn voice_identifier(lang: &str) -> Result<String> {
        let list = unsafe { sys::espeak_ListVoices(std::ptr::null_mut()) };
        let mut i = 0;
        while !list.is_null() {
            let v = unsafe { *list.add(i) };
            if v.is_null() {
                break;
            }
            let v = unsafe { &*v };
            // `languages` is [priority byte][name\0]...; phonemizer reads the first name only.
            if unsafe { CStr::from_ptr(v.languages.add(1)) }.to_string_lossy() == lang {
                return Ok(unsafe { CStr::from_ptr(v.identifier) }.to_string_lossy().into_owned());
            }
            i += 1;
        }
        bail!("espeak-ng has no voice for language '{lang}'")
    }

    impl super::Engine for InProcess {
        /// `EspeakWrapper.text_to_phonemes`: IPA, '_' between phonemes, clause by clause.
        fn clause_phonemes(&mut self, text: &str) -> Result<String> {
            let mut state = ESPEAK.lock().unwrap();
            set_voice(&mut state, &self.lang)?;
            let c = CString::new(text)?;
            let mut ptr: *const c_void = c.as_ptr().cast();
            let mode = (('_' as i32) << 8) | 0x02; // espeakPHONEMES_IPA, '_' separator
            let mut out: Vec<String> = Vec::new();
            while !ptr.is_null() {
                let p: *const c_char = unsafe { sys::espeak_TextToPhonemes(&mut ptr, sys::espeakCHARS_UTF8, mode) };
                if !p.is_null() {
                    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
                    if !s.is_empty() {
                        out.push(s);
                    }
                }
            }
            Ok(out.join(" "))
        }
    }
}

/// `kokoro_onnx.Tokenizer.phonemize(text, lang)`: phonemizer + drop chars outside the vocab.
pub fn phonemize(engine: &mut dyn Engine, text: &str, keep: impl Fn(char) -> bool) -> Result<String> {
    let text = py_strip(text);
    let mut lines_out = Vec::new();
    // phonemizer: str2list splits on os.linesep and drops blank lines.
    let lines: Vec<&str> = text.split('\n').filter(|l| !py_strip(l).is_empty()).collect();
    if !lines.is_empty() {
        let (chunks, marks) = preserve(&lines);
        let mut phonemized = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            phonemized.push(postprocess_line(&engine.clause_phonemes(chunk)?));
        }
        lines_out = restore(phonemized, marks);
    }
    let joined = lines_out.join("\n");
    let filtered: String = joined.chars().filter(|&c| keep(c)).collect();
    Ok(py_strip(&filtered).to_string())
}

/// `EspeakBackend._postprocess_line` with separator(word=' ', phone=''), strip=False, no tie,
/// with_stress=True, keep-flags.
fn postprocess_line(line: &str) -> String {
    static US: OnceLock<Regex> = OnceLock::new();
    let line = py_strip(line).replace('\n', " ").replace("  ", " ");
    let line = US.get_or_init(|| Regex::new("_+").unwrap()).replace_all(&line, "_");
    let line = line.replace("_ ", " ");
    if line.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for word in line.split(' ') {
        // word.strip() + '_' then '_' -> phone separator '' removes every underscore.
        out.push_str(&py_strip(word).replace('_', ""));
        out.push(' ');
    }
    out
}

#[derive(Debug, Clone)]
struct Mark {
    index: usize,
    mark: String,
    position: char, // 'B'egin, 'E'nd, 'I'nside, 'A'lone
}

fn marks_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"(\s*[{}]+\s*)+", regex::escape(MARKS))).unwrap())
}

/// `Punctuation.preserve`: split each line at punctuation runs, remember the runs.
fn preserve(lines: &[&str]) -> (Vec<String>, Vec<Mark>) {
    let mut text = Vec::new();
    let mut marks = Vec::new();
    for (num, line) in lines.iter().enumerate() {
        let matches: Vec<regex::Match> = marks_re().find_iter(line).collect();
        if matches.is_empty() {
            text.push(line.to_string());
            continue;
        }
        if matches.len() == 1 && matches[0].as_str() == *line {
            marks.push(Mark { index: num, mark: line.to_string(), position: 'A' });
            continue;
        }
        let mut line_marks = Vec::new();
        let last = matches.len() - 1;
        for (i, m) in matches.iter().enumerate() {
            let g = m.as_str();
            let position = if i == 0 && line.starts_with(g) {
                'B'
            } else if i == last && line.ends_with(g) {
                'E'
            } else {
                'I'
            };
            line_marks.push(Mark { index: num, mark: g.to_string(), position });
        }
        let mut rest: String = line.to_string();
        for m in &line_marks {
            let (prefix, suffix) = match rest.find(&m.mark) {
                Some(at) => (rest[..at].to_string(), rest[at + m.mark.len()..].to_string()),
                None => (rest.clone(), String::new()),
            };
            text.push(prefix);
            rest = suffix;
        }
        text.push(rest);
        marks.extend(line_marks);
    }
    (text.into_iter().filter(|l| !l.is_empty()).collect(), marks)
}

/// `Punctuation.restore` with sep.word = ' ', strip = False.
fn restore(mut text: Vec<String>, mut marks: Vec<Mark>) -> Vec<String> {
    const SEP: &str = " ";
    let mut out = Vec::new();
    let mut pos = 0usize;
    text.reverse(); // use as a stack: last element is text[0]
    marks.reverse();
    while !text.is_empty() || !marks.is_empty() {
        if marks.is_empty() {
            while let Some(mut line) = text.pop() {
                if !line.ends_with(SEP) {
                    line.push_str(SEP);
                }
                out.push(line);
            }
        } else if text.is_empty() {
            let all: String = marks.iter().rev().map(|m| m.mark.as_str()).collect();
            out.push(all);
            marks.clear();
        } else if marks.last().unwrap().index == pos {
            let m = marks.pop().unwrap();
            let n = text.len();
            let first = text.last_mut().unwrap();
            if first.ends_with(SEP) {
                first.truncate(first.len() - SEP.len());
            }
            let tail = if m.mark.ends_with(SEP) { "" } else { SEP };
            match m.position {
                'B' => *first = format!("{}{}", m.mark, first),
                'E' => {
                    let t = text.pop().unwrap();
                    out.push(format!("{t}{}{tail}", m.mark));
                    pos += 1;
                }
                'A' => {
                    out.push(format!("{}{tail}", m.mark));
                    pos += 1;
                }
                _ if n == 1 => first.push_str(&m.mark),
                _ => {
                    let t0 = text.pop().unwrap();
                    let t1 = text.last_mut().unwrap();
                    *t1 = format!("{t0}{}{t1}", m.mark);
                }
            }
        } else {
            out.push(text.pop().unwrap());
            pos += 1;
        }
    }
    out
}

/// Python's `str.strip()` (Unicode whitespace).
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_restore_roundtrip_shape() {
        let (t, m) = preserve(&["hello, my world!"]);
        assert_eq!(t, vec!["hello", "my world"]);
        assert_eq!(m.iter().map(|m| m.position).collect::<String>(), "IE");
        let r = restore(vec!["A ".into(), "B C ".into()], m);
        assert_eq!(r, vec!["A, B C! "]);
    }
}
