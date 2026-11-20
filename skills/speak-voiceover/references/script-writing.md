# Writing a script for speak

A script is read by a voice, not a reader. Write it the way a person would say it.

## Structure

- One paragraph per slide or scene; a blank line between paragraphs. The manifest, `--split` and
  `assemble.py` count paragraphs, so the number of paragraphs must equal the number of slides.
- Two to four sentences per paragraph is a comfortable slide. Sentences of 8–25 words.
- The first sentence of a paragraph becomes its default chapter title in `assemble.py`; make it
  a good one or pass `--chapters`.
- About 150 words is one minute of narration at the default rate.

## What the voice reads well

- Numbers, money, dates, times, versions: "2,000 bots", "$4.50", "on 3 March", "v0.3.5" (read as
  "version zero point three point five").
- Common developer words: speak has a built-in list (`speak lexicon list --builtin`) so redis,
  JSON, nginx, numpy, stdin and a few hundred others come out right.
- Capitalised abbreviations are spelled out (API, CLI, AWS) unless the built-in list says
  otherwise (JSON is "jay-son").

## What to avoid

- URLs, file paths, commands, code: say what they are ("the config file", "the deploy script").
- Symbols and emoji: they are skipped. Write "and", "per cent", "versus" instead of `&`, `%`,
  `vs` in odd places.
- Parentheses and asides: make them their own sentence.
- Lists with bullets: turn them into a sentence ("three things: speed, size and cost").

## Pronunciation

1. Find the risky words: `SPEAK_DEBUG=1 speak -f script.txt -o /tmp/probe.wav 2>&1 | grep 'G2P fallback'`
   lists every word no dictionary knew, with how it was guessed.
2. Hear one: `speak lexicon try WORD` prints the phonemes and plays the word.
3. Fix it, in order of preference:
   - Inline, for this script only: `[Anouk](/ɑnˈuk/)`. Phonemes use Kokoro's alphabet; copy what
     `lexicon try` printed and edit it.
   - For good, with the user's agreement: `speak lexicon add Anouk "ah-nook"`. This writes the
     user's own pronunciation file, which is often kept in their dotfiles.

## Delivery

| Need | Use |
|---|---|
| A beat before a reveal | `[[slnc 800]]` |
| A slower section | `[[rate 150]]` … `[[rset]]` |
| A second speaker | `[[voice bm_george]]` at the start of their turn |
| More air overall | `--pauses 300,1000` (sentence, paragraph in ms) or `-r 160` |

`[[slnc N]]` replaces the automatic pause at that point, so `[[slnc 800]]` gives exactly 800 ms.

## Dialogue

```text
[[voice af_heart]]Welcome back. Today: why the deploy failed.

[[voice bm_george]]It was the RPC endpoint. It moved without notice.

[[voice af_heart]]And the fix?

[[voice bm_george]]A config change, already shipped.
```

Each turn is its own paragraph, so `--split` gives one file per turn and the manifest records
each sentence's voice.
