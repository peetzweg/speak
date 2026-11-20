//! Where everything that was said landed in the audio, to the sample: the source of subtitles
//! (SRT, WebVTT) and of the JSON timing manifest.

use std::path::Path;

use crate::text::json_str;

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// As displayed: one line or two.
    pub text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sentence {
    /// Index into `Timeline::paragraphs`.
    pub paragraph: usize,
    pub text: String,
    pub voice: String,
    pub start: usize,
    pub end: usize,
    pub cues: Vec<Cue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub start: usize,
    pub end: usize,
    /// Where `--split` wrote it.
    pub file: Option<String>,
}

/// Loudness normalisation applied to the written audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    /// Integrated loudness before and after, LUFS.
    pub before: f64,
    pub after: f64,
    pub gain_db: f64,
    /// Sample peak after, dBFS.
    pub peak_db: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timeline {
    pub rate: u32,
    /// Total length in samples.
    pub len: usize,
    pub paragraphs: Vec<Paragraph>,
    pub sentences: Vec<Sentence>,
}

/// Cue boundaries from where each part after the first began (`marks`), clamped into the sentence;
/// a part that took no time is shown with the next one (or the one before, at the end).
pub fn cues(parts: &[String], marks: &[usize], start: usize, end: usize) -> Vec<Cue> {
    // Shown as written, minus inline pronunciations: `[Anouk](/ɑnˈuk/)` is displayed as `Anouk`.
    let wrap = |t: &str| crate::script::wrap(&crate::lexicon::plain(t));
    if parts.len() < 2 || marks.len() + 1 != parts.len() {
        return vec![Cue { text: wrap(&parts.join(" ")), start, end }];
    }
    let mut bounds = vec![start];
    for &m in marks {
        bounds.push(m.clamp(*bounds.last().unwrap(), end));
    }
    bounds.push(end);
    let mut cues: Vec<(String, usize, usize)> = Vec::new();
    let mut pending: Vec<&str> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        pending.push(part);
        let from = cues.last().map_or(start, |c| c.2);
        if bounds[i + 1] > from {
            cues.push((pending.join(" "), from, bounds[i + 1]));
            pending.clear();
        }
    }
    if !pending.is_empty() {
        match cues.last_mut() {
            Some(last) => last.0 = format!("{} {}", last.0, pending.join(" ")),
            None => cues.push((pending.join(" "), start, end)),
        }
    }
    cues.into_iter().map(|(text, start, end)| Cue { text: wrap(&text), start, end }).collect()
}

impl Timeline {
    fn secs(&self, samples: usize) -> f64 {
        samples as f64 / self.rate.max(1) as f64
    }

    fn ms(&self, samples: usize) -> u64 {
        (samples as u64 * 1000 + self.rate as u64 / 2) / self.rate.max(1) as u64
    }

    fn cues(&self) -> impl Iterator<Item = &Cue> {
        self.sentences.iter().flat_map(|s| &s.cues).filter(|c| c.end > c.start)
    }

    /// SubRip: `HH:MM:SS,mmm --> HH:MM:SS,mmm`, times rounded to the millisecond.
    pub fn srt(&self) -> String {
        let mut out = String::new();
        for (i, c) in self.cues().enumerate() {
            out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, self.stamp(c.start, ','), self.stamp(c.end, ','), c.text));
        }
        out
    }

    /// WebVTT.
    pub fn vtt(&self) -> String {
        let mut out = String::from("WEBVTT\n\n");
        for c in self.cues() {
            out.push_str(&format!("{} --> {}\n{}\n\n", self.stamp(c.start, '.'), self.stamp(c.end, '.'), c.text));
        }
        out
    }

    /// Subtitles in the format `path`'s extension names (.srt or .vtt).
    pub fn subtitles(&self, path: &Path) -> Option<String> {
        match path.extension()?.to_string_lossy().to_lowercase().as_str() {
            "srt" => Some(self.srt()),
            "vtt" => Some(self.vtt()),
            _ => None,
        }
    }

    fn stamp(&self, samples: usize, sep: char) -> String {
        let ms = self.ms(samples);
        format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
    }

    /// The timing manifest: times in seconds (to the microsecond, so `round(t * sample_rate)` is
    /// the sample), paragraphs with their sentences, and what was written.
    pub fn manifest(&self, audio: Option<&str>, loudness: Option<&Loudness>) -> String {
        let t = |s: usize| format!("{:.6}", self.secs(s));
        let mut o = String::from("{\n");
        o.push_str(&format!("  \"sample_rate\": {},\n  \"duration\": {},\n", self.rate, t(self.len)));
        o.push_str(&format!("  \"audio\": {},\n", audio.map_or("null".into(), json_str)));
        if let Some(l) = loudness {
            o.push_str(&format!(
                "  \"loudness\": {{\"integrated\": {:.2}, \"before\": {:.2}, \"gain_db\": {:.2}, \"peak_dbfs\": {:.2}}},\n",
                l.after, l.before, l.gain_db, l.peak_db
            ));
        }
        o.push_str("  \"paragraphs\": [");
        for (p, para) in self.paragraphs.iter().enumerate() {
            o.push_str(if p == 0 { "\n" } else { ",\n" });
            o.push_str(&format!("    {{\"start\": {}, \"end\": {}", t(para.start), t(para.end)));
            if let Some(f) = &para.file {
                o.push_str(&format!(", \"file\": {}", json_str(f)));
            }
            o.push_str(", \"sentences\": [");
            for (i, s) in self.sentences.iter().filter(|s| s.paragraph == p).enumerate() {
                o.push_str(if i == 0 { "\n" } else { ",\n" });
                o.push_str(&format!(
                    "      {{\"start\": {}, \"end\": {}, \"voice\": {}, \"text\": {}}}",
                    t(s.start),
                    t(s.end),
                    json_str(&s.voice),
                    json_str(&s.text)
                ));
            }
            o.push_str("\n    ]}");
        }
        o.push_str("\n  ]\n}\n");
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    fn sample() -> Timeline {
        let s = |paragraph, text: &str, start, end| Sentence {
            paragraph,
            text: text.into(),
            voice: "af_heart".into(),
            start,
            end,
            cues: vec![Cue { text: text.into(), start, end }],
        };
        Timeline {
            rate: 24_000,
            len: 120_000,
            paragraphs: vec![
                Paragraph { start: 0, end: 50_000, file: Some("out/001.wav".into()) },
                Paragraph { start: 64_400, end: 120_000, file: None },
            ],
            sentences: vec![s(0, "Hello \"there\".", 0, 24_000), s(0, "Two.", 26_880, 50_000), s(1, "Three.", 64_400, 120_000)],
        }
    }

    #[test]
    fn subtitle_formats() {
        let t = sample();
        assert_eq!(
            t.srt(),
            "1\n00:00:00,000 --> 00:00:01,000\nHello \"there\".\n\n2\n00:00:01,120 --> 00:00:02,083\nTwo.\n\n\
             3\n00:00:02,683 --> 00:00:05,000\nThree.\n\n"
        );
        assert!(t.vtt().starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello \"there\".\n\n"));
        assert_eq!(t.subtitles(Path::new("a.SRT")), Some(t.srt()));
        assert_eq!(t.subtitles(Path::new("a.txt")), None);
        // Hours roll over.
        assert_eq!(t.stamp(24_000 * 3_725 + 12, ','), "01:02:05,001");
    }

    #[test]
    fn manifest_is_json_with_sample_exact_times() {
        let m = sample().manifest(Some("out.wav"), None);
        assert!(m.contains("\"sample_rate\": 24000,\n  \"duration\": 5.000000,\n  \"audio\": \"out.wav\""), "{m}");
        assert!(m.contains("{\"start\": 0.000000, \"end\": 2.083333, \"file\": \"out/001.wav\", \"sentences\": ["), "{m}");
        assert!(m.contains("{\"start\": 1.120000, \"end\": 2.083333, \"voice\": \"af_heart\", \"text\": \"Two.\"}"), "{m}");
        assert!(m.contains("\"text\": \"Hello \\\"there\\\".\""), "{m}");
        let l = Loudness { before: -21.234, after: -16.0, gain_db: 5.234, peak_db: -1.0 };
        assert!(
            sample().manifest(None, Some(&l)).contains("\"audio\": null,\n  \"loudness\": {\"integrated\": -16.00, \"before\": -21.23")
        );
    }

    #[test]
    fn cues_follow_the_marks() {
        let c = cues(&parts(&["One part,", "two parts."]), &[1000], 100, 3000);
        assert_eq!(
            c,
            vec![Cue { text: "One part,".into(), start: 100, end: 1000 }, Cue { text: "two parts.".into(), start: 1000, end: 3000 }]
        );
        // A part that took no time is shown with its neighbour, and marks are kept in order.
        let c = cues(&parts(&["a,", "b,", "c."]), &[500, 500], 0, 900);
        assert_eq!(c.iter().map(|c| (c.text.as_str(), c.start, c.end)).collect::<Vec<_>>(), [("a,", 0, 500), ("b, c.", 500, 900)]);
        let c = cues(&parts(&["a,", "b."]), &[900], 0, 900);
        assert_eq!(c.iter().map(|c| (c.text.as_str(), c.start, c.end)).collect::<Vec<_>>(), [("a, b.", 0, 900)]);
        // Wrong number of marks: one cue for the sentence.
        assert_eq!(cues(&parts(&["a,", "b."]), &[], 0, 900).len(), 1);
    }

    #[test]
    fn cues_hide_inline_pronunciations() {
        let parts = ["[Anouk](/ɑnˈuk/) fixed it.".to_string()];
        assert_eq!(cues(&parts, &[], 0, 100)[0].text, "Anouk fixed it.");
    }
}
