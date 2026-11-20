---
name: speak-voiceover
description: Voice-overs and spoken output with the local `speak` CLI (Kokoro text-to-speech, runs offline). Narrates slide decks, explainer and catch-up videos, podcasts and two-voice dialogues, writing sample-exact subtitles (.srt/.vtt), a JSON timing manifest, one audio file per slide and loudness-normalised audio, then assembles an MP4 with chapters and subtitles via ffmpeg. Also covers spoken notifications while an agent works. Use when asked to "make a voice-over", "narrate this", "add narration to the slides", "make a narrated video", "turn this into a podcast", "read this aloud", "make subtitles for the narration", or to tell the user something out loud. Do NOT use for speech-to-text or transcription, or for cloud TTS services such as ElevenLabs.
---

# speak: voice-overs and spoken output

`speak` is `say(1)` with a local neural voice. Everything below runs offline once the model is
downloaded. `speak --help` is the full reference; this skill is the recipe.

## Critical rules

- **Never play long audio out loud unasked.** Productions always write files (`-o`, `--split`).
  Play a result only when the user asks to hear it.
- **Write for the ear.** No URLs, file paths, code, emoji or bullet symbols in a script: emoji and
  symbols are skipped, and code comes out as a jumble of characters. See
  [references/script-writing.md](references/script-writing.md).
- **Check names and jargon before rendering**: `speak lexicon try WORD` prints how a word will be
  read and plays it. `speak lexicon add` writes
  the user's global pronunciation file (often in their dotfiles): ask before adding entries.
- **Paragraph = slide/scene.** A blank line starts a paragraph; the manifest, `--split` and the
  assembly script all key off paragraphs.
- Exit codes: 0 ok, 1 synthesis/audio failure, 2 usage, 3 model or espeak-ng missing, 4 unknown
  voice. Production flags never fall back to the system voice.

## Live use (notifications while working)

```sh
speak "Build finished: three tests failed."      # waits its turn if another speak is talking
speak --background "Deploy is done."              # returns at once, speaks detached
speak stop                                        # stop what is playing (--all: and the queue)
```

One or two short sentences, the point first. Several agents can call `speak` at once; they take
turns instead of talking over each other.

## Production workflow

### Step 1: Check the tools

```sh
speak --version && speak models     # model installed? if not: speak models download kokoro
speak -v '?'                         # voices; af_heart is the default
command -v ffmpeg                    # needed only for video assembly and mp3/m4a
```

Useful voices: `af_heart` (US, female, default), `am_michael` (US, male), `bf_lily` (UK, female),
`bm_george` (UK, male). Respect a default the user set (`~/.config/speak/config.toml`) unless the
production calls for another.

### Step 2: Write the script

Plain text, one paragraph per slide or scene, written to be heard. Embedded commands (say's
syntax) control delivery:

| Command | Effect |
|---|---|
| `[[slnc 800]]` | 800 ms of silence, replacing the automatic pause at that point |
| `[[rate 160]]`, `[[rate +20]]` | words per minute from here on (absolute or relative) |
| `[[voice bm_george]]` | switch voice (dialogue); `[[rset]]` returns to the command line's voice and rate |

Other `[[...]]` commands are skipped, never read out. Details and style rules:
[references/script-writing.md](references/script-writing.md).

### Step 3: Pronunciation pass

```sh
SPEAK_DEBUG=1 speak -f script.txt -o /tmp/probe.wav 2>&1 | grep 'G2P fallback'
```

Each line names a word no dictionary knew and how it was guessed. For each word that matters
(product names, people, jargon) run `speak lexicon try WORD`. Fix it with inline phonemes in the
script, `[Kokoro](/kˈOkəɹO/)`, or (with the user's OK) `speak lexicon add WORD "respelling"`.

### Step 4: Render

```sh
speak -f script.txt -o voice.wav \
  --subtitles voice.srt --manifest voice.json --split parts \
  --loudness -16 --pauses 300,1000
```

- `--subtitles`: one cue per sentence (long ones split at clauses), timed to the millisecond.
- `--manifest`: sample rate, duration, loudness, and every paragraph and sentence with start,
  end, voice and text. Drives slide timing and chapters.
- `--split DIR`: `DIR/001.wav`, `002.wav`, … one per paragraph, exact slices of `voice.wav`.
- `--loudness -16`: BS.1770 integrated loudness (−16 LUFS suits video and podcasts; −23 for
  broadcast), peaks kept under −1 dBFS.
- `--pauses S,P`: ms after a sentence and after a paragraph (default 120,600; narration reads
  better at about 300,1000).

### Step 5: Verify before assembling

- Read `voice.json`: paragraph count equals the slide count; durations are plausible
  (roughly 150 words per minute).
- If the user wants to hear it, play one paragraph: `afplay parts/001.wav` (macOS) or
  `ffplay -nodisp -autoexit parts/001.wav`.

### Step 6: Assemble

Video from one image per paragraph (`slides/001.png`, `002.png`, … same count as paragraphs):

```sh
python3 scripts/assemble.py --manifest voice.json --slides slides --out video.mp4 \
  --subtitles voice.srt --chapters chapters.txt
```

`chapters.txt` is optional, one title per paragraph. The script holds each slide until the next
paragraph starts, muxes the audio, adds soft subtitles and chapters, and prints the result.
Audio only (podcast): `ffmpeg -i voice.wav -c:a aac -b:a 160k episode.m4a`.
More recipes (burned-in subtitles, music bed, crossfades):
[references/assembly.md](references/assembly.md).

## Examples

**Narrated video from a deck.** "Make a narrated video of these slides." Export slides to
PNGs; write one paragraph of narration per slide; pronunciation pass; render with `--manifest`
and `--subtitles`; assemble with the slides; report the path, duration and chapter list.

**Two-voice podcast.** "Turn these notes into a short podcast with two hosts." Write a dialogue,
each turn starting with `[[voice af_heart]]` or `[[voice bm_george]]`, a paragraph per exchange;
render with `--loudness -16 --subtitles episode.srt`; convert to m4a.

**Voice notification.** Blocked on the user's input: `speak "I need your decision on the
licence."` and continue only after they answer.

## Troubleshooting

| Symptom | Fix |
|---|---|
| exit 3, "model missing" | `speak models download kokoro` (354 MB, once) |
| exit 3 for a non-English voice | install espeak-ng (`brew install espeak-ng`, `apt install espeak-ng`) |
| exit 4, unknown voice | `speak -v '?'` for the exact names |
| exit 2 with `--loudness` | it needs `-o` or `--split` |
| a word sounds wrong | `speak lexicon try WORD`, then inline `[word](/phonemes/)` or `speak lexicon add` |
| silence where a symbol was | symbols and emoji aren't read; write the words out |
| slide count ≠ paragraph count | `assemble.py` refuses; fix blank lines in the script or the slides |
