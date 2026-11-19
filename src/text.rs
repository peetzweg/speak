//! Model-independent text handling: say's embedded commands, sentence chunking, JSON strings.

use std::sync::OnceLock;

use regex::Regex;

/// Drop say(1)'s embedded commands (`[[rate 200]]`, `[[slnc 500]]`, …) instead of reading them out.
pub fn strip_say_commands(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[[^\]]*\]\]").unwrap()).replace_all(text, " ").into_owned()
}

/// Words after which a full stop does not end the sentence.
const ABBREVIATIONS: &[&str] = &["Mr", "Mrs", "Ms", "Dr", "Prof", "Sr", "Jr", "St", "vs", "e.g", "i.e", "cf", "approx", "a.m", "p.m"];

fn ends_with_abbreviation(sentence: &str) -> bool {
    let Some(word) = sentence.strip_suffix('.').and_then(|s| s.rsplit([' ', '(']).next()) else { return false };
    // An initial: "J." or the last letter of "U.S.".
    let last = word.rsplit('.').next().unwrap_or(word);
    ABBREVIATIONS.iter().any(|a| a.eq_ignore_ascii_case(word)) || (last.chars().count() == 1 && last.chars().all(char::is_uppercase))
}

/// Sentences, so playback can start after the first one (speak.py's chunking): whitespace is
/// collapsed, text is cut after `.`, `!`, `?` or `…` followed by a space (not after common
/// abbreviations or initials), and sentences over 400 bytes are cut at a comma.
pub fn chunks(text: &str) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut prev = None;
    for (i, c) in text.char_indices() {
        if c == ' ' && matches!(prev, Some('.' | '!' | '?' | '…')) && !ends_with_abbreviation(&text[start..i]) {
            sentences.push(&text[start..i]);
            start = i + 1;
        }
        prev = Some(c);
    }
    sentences.push(&text[start..]);

    let mut out = Vec::new();
    for s in sentences {
        let mut s = s.to_string();
        while s.len() > 400 {
            let limit = s.floor_char_boundary(400);
            let cut = match s[..limit].rfind(", ") {
                Some(c) if c > 100 => c + 1,
                _ => limit,
            };
            out.push(s[..cut].trim().to_string());
            s = s[cut..].trim().to_string();
        }
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

/// A JSON string literal, escaped like Python's `json.dumps(ensure_ascii=False)`.
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences() {
        assert_eq!(chunks("Hello there.  How are\nyou? Fine!Thanks… ok"), vec!["Hello there.", "How are you?", "Fine!Thanks…", "ok"]);
        assert_eq!(chunks("   "), Vec::<String>::new());
        // A full stop inside a token (versions, decimals, URLs) is not a sentence end.
        assert_eq!(chunks("Shipped v0.3.5 on example.com today. Done."), vec!["Shipped v0.3.5 on example.com today.", "Done."]);
    }

    #[test]
    fn keeps_abbreviations_and_initials_together() {
        assert_eq!(
            chunks("Dr. Smith met Mr. J. Doe vs. the U.S. team. Then left."),
            vec!["Dr. Smith met Mr. J. Doe vs. the U.S. team.", "Then left."]
        );
        assert_eq!(chunks("Use tools, e.g. grep. It works."), vec!["Use tools, e.g. grep.", "It works."]);
    }

    #[test]
    fn long_sentences_split_at_commas() {
        let long = format!("{}, {}, {}.", "a".repeat(150), "b".repeat(200), "c".repeat(150));
        let c = chunks(&long);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], format!("{}, {},", "a".repeat(150), "b".repeat(200)));
        assert!(c.iter().all(|s| s.len() <= 400));
        // No comma past byte 100: hard cut at 400 (on a char boundary).
        let ü = "ü".repeat(300);
        let c = chunks(&ü);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].len(), 400);
    }

    #[test]
    fn strips_embedded_say_commands() {
        assert_eq!(strip_say_commands("[[slnc 500]]Hello [[rate 200]]world"), " Hello  world");
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json_str("a\"b\\c\nü\u{1}"), "\"a\\\"b\\\\c\\nü\\u0001\"");
    }
}
