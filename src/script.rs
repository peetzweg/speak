//! What to say, in order: paragraphs (separated by blank lines) of sentences and pauses, each
//! sentence with its voice and speed, read from text with say(1)'s embedded commands.
//!
//! * `[[slnc N]]`: N ms of silence, in place of the automatic pause where it stands.
//! * `[[rate N]]`: N words per minute from here on, like `-r`; `[[rate +N]]`/`[[rate -N]]` relative.
//! * `[[voice NAME]]` (speak's own): speak on in another voice.
//! * `[[rset]]`: back to the voice and rate of the command line.
//!
//! Every other `[[…]]` is dropped, never read out. Several commands may share one `[[…]]`,
//! separated by `;`, as in say.

use std::sync::OnceLock;

use regex::Regex;

use crate::text;

/// say's normal speaking rate, in words per minute (speed 1.0).
pub const NORMAL_WPM: f32 = 175.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Sentence {
    /// As written (subtitles and the manifest show this; the lexicon applies when it is spoken).
    pub text: String,
    pub voice: String,
    pub speed: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Say(Sentence),
    /// Silence, in milliseconds.
    Pause(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    /// Paragraphs with something in them, in order.
    pub paragraphs: Vec<Vec<Item>>,
}

impl Script {
    pub fn sentences(&self) -> impl Iterator<Item = &Sentence> {
        self.paragraphs.iter().flatten().filter_map(|i| match i {
            Item::Say(s) => Some(s),
            Item::Pause(_) => None,
        })
    }

    /// Every voice the script uses, in order of first use.
    pub fn voices(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for s in self.sentences() {
            if !v.contains(&s.voice.as_str()) {
                v.push(&s.voice);
            }
        }
        v
    }
}

fn command_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\]]*)\]\]").unwrap())
}

/// Parse `text`, starting in `voice` at `speed` (0.5 to 2.0).
pub fn parse(text: &str, voice: &str, speed: f32) -> Script {
    static BLANK_LINE: OnceLock<Regex> = OnceLock::new();
    let blank = BLANK_LINE.get_or_init(|| Regex::new(r"\n[ \t\r\x0b\x0c]*\n").unwrap());
    let mut state = (voice.to_string(), speed * NORMAL_WPM);
    let mut script = Script::default();
    for paragraph in blank.split(text) {
        let mut items = Vec::new();
        let mut last = 0;
        for c in command_re().captures_iter(paragraph) {
            let m = c.get(0).unwrap();
            say(&paragraph[last..m.start()], &state, &mut items);
            last = m.end();
            for command in c[1].split(';') {
                apply(command, voice, speed, &mut state, &mut items);
            }
        }
        say(&paragraph[last..], &state, &mut items);
        if !items.is_empty() {
            script.paragraphs.push(items);
        }
    }
    script
}

fn say(text: &str, (voice, wpm): &(String, f32), items: &mut Vec<Item>) {
    let speed = (wpm / NORMAL_WPM).clamp(0.5, 2.0);
    items.extend(text::chunks(text).into_iter().map(|text| Item::Say(Sentence { text, voice: voice.clone(), speed })));
}

fn apply(command: &str, voice: &str, speed: f32, state: &mut (String, f32), items: &mut Vec<Item>) {
    let mut words = command.split_whitespace();
    let (Some(name), arg) = (words.next(), words.next()) else { return };
    match (name.to_ascii_lowercase().as_str(), arg) {
        ("slnc", Some(ms)) => {
            if let Ok(ms) = ms.parse::<f64>()
                && ms > 0.0
            {
                items.push(Item::Pause(ms.min(600_000.0).round() as u32));
            }
        }
        ("rate", Some(n)) => {
            let relative = n.starts_with(['+', '-']);
            if let Ok(n) = n.parse::<f32>() {
                let wpm = if relative { state.1 + n } else { n };
                state.1 = wpm.clamp(0.5 * NORMAL_WPM, 2.0 * NORMAL_WPM);
            }
        }
        ("voice", Some(v)) => state.0 = v.to_string(),
        ("rset", _) => *state = (voice.to_string(), speed * NORMAL_WPM),
        _ => {}
    }
}

/// Subtitle lines: at most this many characters, and at most two lines per cue (the common
/// broadcast guideline).
pub const LINE: usize = 42;

fn len(s: &str) -> usize {
    s.chars().count()
}

/// `cue` broken into at most two lines of at most `LINE` characters, as evenly as possible;
/// `None` if it can't be.
fn wrap_strict(cue: &str) -> Option<String> {
    if len(cue) <= LINE {
        return Some(cue.to_string());
    }
    cue.match_indices(' ')
        .map(|(i, _)| (&cue[..i], &cue[i + 1..]))
        .filter(|(a, b)| len(a) <= LINE && len(b) <= LINE)
        // Balanced, and a little bottom-heavy when it can't be exact.
        .min_by_key(|(a, b)| (len(a).abs_diff(len(b)), len(a) > len(b)))
        .map(|(a, b)| format!("{a}\n{b}"))
}

/// The cue's text as displayed: one line, or two balanced lines.
pub fn wrap(cue: &str) -> String {
    wrap_strict(cue).unwrap_or_else(|| {
        // Too long for two lines (a very long word): break at the space nearest the middle.
        let mid = len(cue) / 2;
        match cue.match_indices(' ').min_by_key(|(i, _)| len(&cue[..*i]).abs_diff(mid)) {
            Some((i, _)) => format!("{}\n{}", &cue[..i], &cue[i + 1..]),
            None => cue.to_string(),
        }
    })
}

/// Words a clause often starts with: a good place to break when there is no punctuation.
const CLAUSE_WORDS: &[&str] = &[
    "and", "but", "or", "nor", "so", "yet", "because", "which", "who", "whom", "whose", "that", "when", "while", "where", "although",
    "though", "if", "unless", "until", "since", "as", "then", "after", "before", "whether", "instead",
];

/// How much worse it is to end a cue after `word` (followed by `next`): 0 at a clause boundary.
fn break_cost(word: &str, next: &str) -> u32 {
    let clause_end = word.ends_with([',', ';', ':', '.', '!', '?', '…', '—', '–', ')', '"', '”']) || matches!(next, "—" | "–" | "-");
    let next_word = next.trim_start_matches(['(', '"', '“']).to_lowercase();
    if clause_end {
        0
    } else if CLAUSE_WORDS.contains(&next_word.as_str()) {
        2
    } else {
        5
    }
}

/// A sentence cut into subtitle cues: one cue if it fits in two lines, else as few cues as will
/// fit, cut at clause boundaries (punctuation, then words like "and" or "which") where possible
/// and never into slivers. The cues joined with spaces give the sentence back.
pub fn cue_parts(sentence: &str) -> Vec<String> {
    if wrap_strict(sentence).is_some() {
        return vec![sentence.to_string()];
    }
    let words: Vec<&str> = sentence.split(' ').collect();
    let n = words.len();
    // best[j]: the cheapest way to cut words[..j] into cues, and where its last cue starts.
    let mut best: Vec<Option<(u32, usize)>> = vec![None; n + 1];
    best[0] = Some((0, 0));
    for j in 1..=n {
        for i in 0..j {
            let Some((before, _)) = best[i] else { continue };
            let cue = words[i..j].join(" ");
            let fits = wrap_strict(&cue).is_some();
            if !fits && j - i > 1 {
                continue; // only a single overlong word may break the limit
            }
            let mut cost = before + 100; // as few cues as possible
            if !fits {
                cost += 1000;
            }
            if j < n {
                cost += break_cost(words[j - 1], words[j]);
            }
            if len(&cue) < 16 {
                cost += 8; // a sliver on screen for half a second is unreadable
            }
            if best[j].is_none_or(|(c, _)| cost < c) {
                best[j] = Some((cost, i));
            }
        }
    }
    let mut cuts = vec![n];
    while let Some(&j) = cuts.last()
        && j > 0
    {
        cuts.push(best[j].map_or(0, |(_, i)| i));
    }
    cuts.reverse();
    cuts.windows(2).map(|w| words[w[0]..w[1]].join(" ")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(text: &str, voice: &str, speed: f32) -> Item {
        Item::Say(Sentence { text: text.into(), voice: voice.into(), speed })
    }

    #[test]
    fn paragraphs_sentences_and_pauses() {
        let s = parse("One. Two.\n\n  \nThree [[slnc 500]] four.\n[[cmnt ignored]]Five.\n\n\n", "af_heart", 1.0);
        assert_eq!(
            s.paragraphs,
            vec![
                vec![say("One.", "af_heart", 1.0), say("Two.", "af_heart", 1.0)],
                vec![say("Three", "af_heart", 1.0), Item::Pause(500), say("four.", "af_heart", 1.0), say("Five.", "af_heart", 1.0)],
            ]
        );
        // A single newline is not a paragraph break.
        assert_eq!(parse("a\nb", "v", 1.0).paragraphs.len(), 1);
        assert!(parse("[[volm 0.5]] \n\n [[voice x]]", "v", 1.0).paragraphs.is_empty());
    }

    #[test]
    fn voices_and_rate_carry_on_until_changed() {
        let s = parse("Hi. [[voice bm_george]]Hello.\n\nStill me. [[rate 350]]Fast. [[rate -175]]Normal. [[rset]]Back.", "af_heart", 0.8);
        let got: Vec<(&str, &str, f32)> = s.sentences().map(|s| (s.text.as_str(), s.voice.as_str(), s.speed)).collect();
        assert_eq!(
            got,
            [
                ("Hi.", "af_heart", 0.8),
                ("Hello.", "bm_george", 0.8),
                ("Still me.", "bm_george", 0.8),
                ("Fast.", "bm_george", 2.0),
                ("Normal.", "bm_george", 1.0),
                ("Back.", "af_heart", 0.8),
            ]
        );
        assert_eq!(s.voices(), ["af_heart", "bm_george"]);
        // Out-of-range rates are clamped like -r; several commands may share one block.
        let s = parse("[[rate 1000; voice af_bella]]x", "af_heart", 1.0);
        assert_eq!(s.paragraphs[0], [say("x", "af_bella", 2.0)]);
    }

    #[test]
    fn short_sentences_are_one_cue() {
        assert_eq!(cue_parts("Hello there."), ["Hello there."]);
        assert_eq!(wrap("Hello there."), "Hello there.");
        let two = "This sentence is longer than one line but fits in two of them.";
        assert_eq!(cue_parts(two), [two]);
        assert_eq!(wrap(two), "This sentence is longer than one\nline but fits in two of them.");
    }

    #[test]
    fn long_sentences_split_at_clauses() {
        let s =
            "When the frontend deploy failed on Tuesday, Anouk traced it to the RPC endpoint, which had moved without notice to anyone.";
        let parts = cue_parts(s);
        assert_eq!(
            parts,
            [
                "When the frontend deploy failed on Tuesday,",
                "Anouk traced it to the RPC endpoint, which had moved without notice to anyone."
            ]
        );
        for p in &parts {
            assert!(wrap(p).lines().count() <= 2 && wrap(p).lines().all(|l| len(l) <= LINE), "{p}");
        }
        assert_eq!(parts.join(" "), s);

        // Without punctuation, before a clause word rather than anywhere.
        let s =
            "The team moved the deploy pipeline to the new runners last week and nobody noticed anything until the cache filled up again";
        let parts = cue_parts(s);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[1].starts_with("and ") || parts[1].starts_with("until "), "{parts:?}");
        assert_eq!(parts.join(" "), s);

        // A word too long for any line still gets a cue of its own.
        let url = format!("See https://example.com/{} for details, and then reply.", "x".repeat(80));
        let parts = cue_parts(&url);
        assert_eq!(parts.join(" "), url);
        assert!(parts.iter().any(|p| p.starts_with("https://") || p.contains(" https://")));
    }
}
