//! Speaking a script: its sentences, pauses and voices in order, as one stream of samples, and
//! the timeline of where each paragraph, sentence and subtitle cue landed in it.
//!
//! Pauses: between two sentences of a paragraph `Pauses::sentence`, between paragraphs
//! `Pauses::paragraph`; an explicit `[[slnc N]]` takes the place of either where it stands.

use std::collections::HashMap;
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::backend::Backend;
use crate::lexicon::{self, Lexicon};
use crate::script::{self, Item, Script};
use crate::timeline::{self, Paragraph, Sentence, Timeline};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pauses {
    pub sentence_ms: u32,
    pub paragraph_ms: u32,
}

impl Default for Pauses {
    fn default() -> Self {
        Self { sentence_ms: 120, paragraph_ms: 600 }
    }
}

pub struct Settings {
    pub pauses: Pauses,
    /// Split long sentences into subtitle cues (and find where each begins).
    pub cues: bool,
}

#[derive(Default)]
pub struct Progress {
    pub first_chunk: Option<Duration>,
    pub samples: usize,
}

/// Speak `script` into `tx`, each sentence with the pronunciations in `lexicons` for its voice.
/// Returns the timeline of what was sent (also when synthesis failed or the receiver hung up
/// part-way).
pub fn render(
    be: &mut dyn Backend,
    script: &Script,
    lexicons: &HashMap<String, Lexicon>,
    settings: &Settings,
    t0: Instant,
    tx: &SyncSender<Vec<f32>>,
) -> (Result<()>, Timeline, Progress) {
    let rate = be.info().sample_rate;
    let mut r = Renderer { tx, pos: 0, timeline: Timeline { rate, ..Timeline::default() }, progress: Progress::default(), t0 };
    let result = r.script(be, script, lexicons, settings);
    r.timeline.len = r.pos;
    (result, r.timeline, r.progress)
}

struct Renderer<'a> {
    tx: &'a SyncSender<Vec<f32>>,
    pos: usize,
    timeline: Timeline,
    progress: Progress,
    t0: Instant,
}

/// The listener went away (playback stopped): not an error, just the end.
struct HungUp;

impl Renderer<'_> {
    fn samples(&self, ms: u32) -> usize {
        (u64::from(ms) * u64::from(self.timeline.rate) / 1000) as usize
    }

    fn send(&mut self, samples: Vec<f32>) -> Result<(), HungUp> {
        self.pos += samples.len();
        if samples.is_empty() {
            return Ok(());
        }
        self.tx.send(samples).map_err(|_| HungUp)
    }

    fn script(&mut self, be: &mut dyn Backend, script: &Script, lexicons: &HashMap<String, Lexicon>, settings: &Settings) -> Result<()> {
        // The paragraph of the last thing said, while nothing but sentences came after it.
        let mut said_in: Option<usize> = None;
        for items in &script.paragraphs {
            let paragraph = self.timeline.paragraphs.len();
            let mut start = None;
            for item in items {
                match item {
                    Item::Pause(ms) => {
                        start.get_or_insert(self.pos);
                        if self.send(vec![0.0; self.samples(*ms)]).is_err() {
                            return Ok(());
                        }
                        said_in = None;
                    }
                    Item::Say(s) => {
                        let gap = match said_in {
                            Some(p) if p == paragraph => self.samples(settings.pauses.sentence_ms),
                            Some(_) => self.samples(settings.pauses.paragraph_ms),
                            None => 0,
                        };
                        let written = if settings.cues { script::cue_parts(&s.text) } else { vec![s.text.clone()] };
                        let spoken: Vec<String> = match lexicons.get(&s.voice) {
                            Some(lex) => written.iter().map(|w| lexicon::to_markup(&lex.apply(w))).collect(),
                            None => written.clone(),
                        };
                        let mut begun: Option<usize> = None;
                        let mut marks = Vec::new();
                        for chunk in be.synthesize_parts(&spoken, &s.voice, s.speed)? {
                            let chunk = chunk?;
                            if begun.is_none() && !chunk.samples.is_empty() {
                                // The pause before a sentence is only sent once it has something to say.
                                if self.send(vec![0.0; gap]).is_err() {
                                    return Ok(());
                                }
                                begun = Some(self.pos);
                                start.get_or_insert(self.pos);
                                self.progress.first_chunk.get_or_insert(self.t0.elapsed());
                            }
                            marks.extend(chunk.marks.iter().map(|m| self.pos + m));
                            self.progress.samples += chunk.samples.len();
                            if self.send(chunk.samples).is_err() {
                                return Ok(());
                            }
                        }
                        let Some(begun) = begun else { continue };
                        self.timeline.sentences.push(Sentence {
                            paragraph,
                            text: s.text.clone(),
                            voice: s.voice.clone(),
                            start: begun,
                            end: self.pos,
                            cues: timeline::cues(&written, &marks, begun, self.pos),
                        });
                        said_in = Some(paragraph);
                    }
                }
            }
            if let Some(start) = start {
                self.timeline.paragraphs.push(Paragraph { start, end: self.pos, file: None });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Chunk, ChunkIter, Info, Voice};

    /// 1 ms of audio (24 samples) per character, a mark wherever the backend default puts one.
    struct Fake;
    impl Backend for Fake {
        fn info(&self) -> Info {
            Info { sample_rate: 24_000 }
        }
        fn voices(&self) -> Result<Vec<Voice>> {
            Ok(vec![])
        }
        fn synthesize<'a>(&'a mut self, text: &str, _voice: &str, _speed: f32) -> Result<ChunkIter<'a>> {
            let n = text.chars().filter(|c| c.is_alphanumeric()).count() * 24;
            Ok(Box::new(std::iter::once(Ok(Chunk { samples: vec![0.5; n], marks: vec![] }))))
        }
    }

    fn run(text: &str, cues: bool) -> (Vec<f32>, Timeline) {
        let (tx, rx) = std::sync::mpsc::sync_channel(1000);
        let settings = Settings { pauses: Pauses::default(), cues };
        let (r, t, _) = render(&mut Fake, &script::parse(text, "af_heart", 1.0), &HashMap::new(), &settings, Instant::now(), &tx);
        r.unwrap();
        drop(tx);
        (rx.iter().flatten().collect(), t)
    }

    #[test]
    fn pauses_between_sentences_and_paragraphs() {
        let (pcm, t) = run("Ab. Cd.\n\nEf [[slnc 50]] gh.\n\n[[slnc 10]]Ij.", false);
        fn at(s: &Sentence) -> (&str, usize, usize, usize) {
            (s.text.as_str(), s.start, s.end, s.paragraph)
        }
        let ms = 24;
        // "Ab." 2 ms, sentence pause 120 ms, "Cd." 2 ms, paragraph pause 600 ms, "Ef" 2 ms, 50 ms,
        // "gh." 2 ms; the last paragraph starts with an explicit pause instead of the automatic one.
        assert_eq!(
            t.sentences.iter().map(at).collect::<Vec<_>>(),
            [
                ("Ab.", 0, 2 * ms, 0),
                ("Cd.", 122 * ms, 124 * ms, 0),
                ("Ef", 724 * ms, 726 * ms, 1),
                ("gh.", 776 * ms, 778 * ms, 1),
                ("Ij.", 788 * ms, 790 * ms, 2),
            ]
        );
        assert_eq!(
            t.paragraphs.iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(),
            [(0, 124 * ms), (724 * ms, 778 * ms), (778 * ms, 790 * ms)]
        );
        assert_eq!(t.len, pcm.len());
        assert_eq!(pcm.len(), 790 * ms);
        // Sound exactly where the timeline says, silence everywhere else.
        for (i, s) in pcm.iter().enumerate() {
            let inside = t.sentences.iter().any(|x| (x.start..x.end).contains(&i));
            assert_eq!(*s != 0.0, inside, "sample {i}");
        }
    }

    #[test]
    fn cues_split_long_sentences() {
        let long =
            "When the frontend deploy failed on Tuesday, Anouk traced it to the RPC endpoint, which had moved without notice to anyone.";
        let (_, t) = run(&format!("Short one. {long}"), true);
        assert_eq!(t.sentences[0].cues.len(), 1);
        let c = &t.sentences[1].cues;
        assert_eq!(c.len(), 2);
        // Spoken one after another by the fake backend: the boundary is where the second part began.
        let first: usize = c[0].text.chars().filter(|c| c.is_alphanumeric()).count() * 24;
        assert_eq!((c[0].start, c[0].end, c[1].end), (t.sentences[1].start, t.sentences[1].start + first, t.sentences[1].end));
        assert_eq!(c[1].start, c[0].end);
        // Without --subtitles a sentence is one piece.
        let (_, t) = run(long, false);
        assert_eq!(t.sentences[0].cues.len(), 1);
    }

    #[test]
    fn each_voice_gets_its_own_lexicon() {
        let lexicons =
            HashMap::from([("af_heart".to_string(), Lexicon::parse("ab = abcdef\n")), ("bm_george".to_string(), Lexicon::default())]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1000);
        let settings = Settings { pauses: Pauses::default(), cues: true };
        let script = script::parse("Ab. [[voice bm_george]]Ab.", "af_heart", 1.0);
        let (r, t, _) = render(&mut Fake, &script, &lexicons, &settings, Instant::now(), &tx);
        r.unwrap();
        drop(rx);
        // The fake speaks 1 ms per letter: "abcdef" for af_heart, "Ab" as written for bm_george.
        let lens: Vec<usize> = t.sentences.iter().map(|s| (s.end - s.start) / 24).collect();
        assert_eq!(lens, [6, 2]);
        // Subtitles keep the written form.
        assert_eq!(t.sentences[0].cues[0].text, "Ab.");
    }

    #[test]
    fn voices_and_empty_sentences() {
        let (_, t) = run("[[voice bm_george]]Hi. ... [[voice af_bella]]Yo.", false);
        let got: Vec<(&str, &str)> = t.sentences.iter().map(|s| (s.text.as_str(), s.voice.as_str())).collect();
        // "..." says nothing: no sentence, and no doubled pause.
        assert_eq!(got, [("Hi.", "bm_george"), ("Yo.", "af_bella")]);
        assert_eq!(t.sentences[1].start - t.sentences[0].end, 120 * 24);
    }
}
