# speak

`say`, but with the best open text-to-speech model, running on your own machine. After a one-time
model download nothing leaves the computer: no API keys, no per-character cost, works offline.
Built for people and for agents (Claude Code calls `speak` to get your attention): it starts
talking after the first sentence, stays quiet on stdout, and has stable exit codes.

The default model is [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) (54 voices, 24 kHz).
Models are pluggable backends; when a better open model appears it becomes the default.

https://github.com/user-attachments/assets/48f8eb0a-c2f2-410d-a401-9ebe7607eaec

```sh
brew install peetzweg/tap/speak
speak "Hello there"
```

## Install

macOS on Apple silicon, and Linux x86_64/arm64 with glibc 2.39 or newer (Ubuntu 24.04, Debian 13):

```sh
brew install peetzweg/tap/speak
```

Or take the archive for your platform from the [releases](https://github.com/peetzweg/speak/releases),
unpack it anywhere and put its `bin/` on your `PATH`, keeping `libexec/` beside it. Linux needs
ALSA (`libasound2`). The first run downloads the model (354 MB, see [Models](#models)).

Releases include espeak-ng 1.52.0 for the Spanish, French, Hindi, Italian, Portuguese, Japanese
and Chinese voices and for English words outside speak's dictionaries. speak runs it as a separate
program; its source is attached to each release. Homebrew's own `espeak-ng` formula also installs
a `speak` command, so the two formulas can't be installed side by side.

### From source

Needs Rust (stable) and, on Linux, `libasound2-dev` (ALSA headers).

```sh
cargo install --path .
```

ONNX Runtime is linked into the binary, so English needs nothing besides speak. Optionally
install `espeak-ng` (`brew install espeak-ng`, `apt install espeak-ng`) for the voices and words
above. `scripts/build-espeak.sh` builds espeak-ng 1.52.0 into `third_party/` instead; point
`SPEAK_ESPEAK` at it. (Homebrew's espeak-ng also installs a `speak` link; make sure
`~/.cargo/bin` comes first in your `PATH`.)

## Usage

```sh
speak "Hello there"            # speak it
echo "piped" | speak           # stdin
speak -f notes.txt             # a file ('-' for stdin)
speak -v bm_george "Hi"        # pick a voice; speak -v '?' lists them
speak -r 200 "Faster"          # words per minute, like say (175 is normal)
speak -o out.wav "Saved"       # write a file; .m4a, .mp3 … via ffmpeg (.aiff/.m4a also via afconvert on macOS)
speak -m kokoro-int8 "..."     # pick a model (default: the best one installed)
speak voices                   # browse voices, space to hear one, enter to make it the default
```

say's other flags are accepted and ignored. `--analyze` prints the phonemes and token ids the
model gets as JSON; `--timing` prints one JSON line of timings on stderr.

Exit codes: 0 ok, 1 synthesis or audio failure, 2 usage, 3 model or espeak-ng missing,
4 unknown voice. If the model can't be used, speak says the text with the system voice (macOS
`say`; `espeak-ng` or `spd-say` on Linux), notes it in one line on stderr, and exits 0.

## For agents

```sh
speak "Build finished: 3 tests failed"     # waits its turn if another speak is talking
speak --background "Deploy is done"         # returns at once and speaks detached
speak stop                                  # end the speech playing now
speak stop --all                            # ... and everything waiting for its turn
```

Several agent sessions can call `speak` at once: the processes take turns at the speakers, first
come first served, instead of talking over each other. A waiting speak synthesises while it
waits, so it starts the moment its turn comes; writing a file (`-o`) never waits.
`--background` checks the flags, the text and the voices before it returns (exit 2 or 4), then
speaks detached. A speak ended by `speak stop` exits 0. stdout stays empty unless you ask for
output (`--manifest -`, `--analyze`, `-v '?'`). The queue is a directory of OS-locked files in
`$XDG_RUNTIME_DIR/speak` (else the cache directory), so a speak that crashes blocks nobody.

## For productions

```sh
speak -f script.txt -o voice.wav --subtitles voice.srt --manifest voice.json --loudness -16
speak -f slides.txt --split slides --manifest slides.json --loudness -16   # slides/001.wav, 002.wav, ...
```

For agents, [`skills/speak-voiceover`](skills/speak-voiceover/SKILL.md) is a skill with the whole
recipe: writing for the ear, the pronunciation pass, rendering, and assembling slides, narration,
subtitles and chapters into an MP4 (`scripts/assemble.py`). For Claude Code:
`ln -s "$PWD/skills/speak-voiceover" ~/.claude/skills/speak-voiceover`.

A script is plain text. A blank line starts a paragraph, and say's embedded commands work:

```text
[[voice af_heart]]Welcome back. Here is what happened this week.

[[voice bm_george]]Thanks. The deploy failed on Tuesday. [[slnc 800]] It works again.

[[rset]][[rate 160]]That's all for today.
```

- `[[slnc N]]` is N ms of silence, in place of the automatic pause where it stands; `[[rate N]]`
  is N words per minute from there on (`+N`/`-N`: relative); `[[voice NAME]]` (speak's own)
  switches voice; `[[rset]]` goes back to the command line's voice and rate. Other `[[...]]`
  commands are skipped, never read out.
- Pauses are 120 ms after a sentence and 600 ms after a paragraph; `--pauses 300,1000` changes them.
- `--subtitles FILE.srt` (or `.vtt`): one cue per sentence. A sentence that doesn't fit in two
  lines of 42 characters is split at clause boundaries. Cue times are where the audio is, to the
  millisecond: inside a sentence they come from Kokoro's own predicted durations, so the audio is
  identical with or without subtitles. Subtitles show the text as written; the lexicon changes only
  how it is spoken.
- `--manifest FILE.json` (`-` for stdout): the sample rate and duration, every paragraph's start,
  end and `--split` file, and every sentence's text, voice, start and end, in seconds to the
  microsecond (`round(t * sample_rate)` is the sample). This drives slide timing and chapters.
- `--split DIR`: each paragraph in its own WAV, an exact slice of the whole render without the
  pauses between paragraphs.
- `--loudness -16`: integrated loudness per ITU-R BS.1770 (what ffmpeg's `ebur128` and `loudnorm`
  measure) for everything written, with a limiter that keeps sample peaks under −1 dBFS. One gain
  for the whole render, so split files keep their relative levels; the manifest records the
  loudness before and after.

The manifest for the script above:

```json
{
  "sample_rate": 24000,
  "duration": 10.474667,
  "audio": "voice.wav",
  "loudness": {"integrated": -16.00, "before": -21.26, "gain_db": 5.31, "peak_dbfs": -1.00},
  "paragraphs": [
    {"start": 0.000000, "end": 2.701333, "sentences": [
      {"start": 0.000000, "end": 0.981333, "voice": "af_heart", "text": "Welcome back."},
      {"start": 1.101333, "end": 2.701333, "voice": "af_heart", "text": "Here is what happened this week."}
    ]},
    {"start": 3.301333, "end": 8.594667, "sentences": [
      {"start": 3.301333, "end": 4.048000, "voice": "bm_george", "text": "Thanks."},
```

## Pronunciations

```sh
speak lexicon add kruapi "crew A P I"   # add or replace a respelling
speak lexicon add gif /ʤˈɪf/            # exact phonemes (Kokoro/misaki alphabet)
speak lexicon try kruapi                # print what it becomes and hear it
speak lexicon list | rm WORD | path
speak lexicon list --builtin            # speak's own developer words
```

The lexicon is a plain text file, `~/.config/speak/lexicon.txt` (or `$SPEAK_LEXICON`), one
`word = spoken form` per line, `#` for comments. Entries match whole words, ignoring case,
longest entry first. `lexicon try` prints the phonemes too, a good starting point for a
`/phonemes/` entry.

English voices also get speak's built-in list of about 370 words from developer and agent text
that misaki reads wrong or not at all: `redis`, `kubectl`, `JSON`, `YAML`, `stdout`, `nginx`,
`Vite`, `enum` and the like. Its entries are respellings misaki's dictionaries read by
themselves, so a word sounds the same with or without espeak-ng and follows the voice's US or
GB accent. Your own entry for a word replaces the built-in one, for example
`speak lexicon add kubectl "kube C T L"`, and an entry that spells the word as it is
(`speak lexicon add sudo sudo`) gives it back to misaki. `add` and `rm` only change your file.
The list is [`assets/lexicon/en.txt`](assets/lexicon/en.txt); words with more than one common
reading (`sudo`, `kubectl`, `SQLite`, `nginx`, …) are marked there.

English goes through [misaki](https://github.com/hexgrad/misaki)'s lexicons (via
[misaki-rs](https://github.com/MicheleYin/misaki-rs)), which read numbers, money, dates,
versions and times naturally ("2,000" is "two thousand", "v0.3.5" is "version zero point three
point five"). Words misaki's dictionaries lack go to espeak-ng when it is installed, run as a separate
program; it is the better reader of abbreviations and names. Without it, misaki's own neural
model (3 MB per dialect, built into speak) guesses them, and lowercase abbreviations without a
vowel (`ssh`, `npm`) are spelled out. So a word is read by the first of: your lexicon, the
built-in list, misaki's dictionaries, espeak-ng or the network. A `speak lexicon add` entry
fixes any word, and `SPEAK_DEBUG=1` shows which words were read which way.

## Configuration

`~/.config/speak/config.toml`:

```toml
voice = "bm_george"
model = "kokoro"
```

A flag beats the environment (`SPEAK_VOICE`, `SPEAK_MODEL`), which beats the file, which beats
the model's default.

## Models

```sh
speak models                      # what is installed and available
speak models download kokoro-int8
speak models rm kokoro-int8
```

| Model | Download | Notes |
|---|---|---|
| `kokoro` | 354 MB | Kokoro-82M v1.0, fp32 (default) |
| `kokoro-int8` | 121 MB | 8-bit quantised: about twice as fast on CPU, 260 MB less memory |

The first run downloads the default model into `$SPEAK_HOME/models` (default
`~/.local/share/speak/models`) from pinned URLs. Each file is checked against a pinned SHA-256
before it is used; an interrupted download resumes. Files already there (for example from an
earlier Python install) are verified once and used as they are. Set `SPEAK_NO_DOWNLOAD=1` to
never download.

## How it compares

| | Voice | Runs | Licence |
|---|---|---|---|
| **speak** | neural, open models (Kokoro-82M today) | on your machine, macOS and Linux | MIT or Apache-2.0 |
| `say` | Apple's voices | on your Mac | part of macOS |
| [sag](https://github.com/steipete/sag) | ElevenLabs | on ElevenLabs' servers: API key, paid per character | MIT |
| [espeak-ng](https://github.com/espeak-ng/espeak-ng) | formant synthesis: clear but robotic | anywhere, 100+ languages | GPL-3.0-or-later |
| [Piper](https://github.com/OHF-Voice/piper1-gpl) | neural (VITS), one model per voice | on your machine | GPL-3.0-or-later; each voice has its own licence |

**speak and espeak-ng.** espeak-ng does two jobs. It works out how text is pronounced, with
rules and dictionaries for over 100 languages, and it turns that pronunciation into sound with
a formant synthesizer, which is where its robotic voice comes from. Kokoro needs only the
first job: it takes phonemes in and makes the voice itself. speak does English pronunciation with misaki and asks espeak-ng only for Kokoro's other
languages and, when it is installed, for English words misaki's dictionaries lack; without
espeak-ng, misaki's neural model guesses those.

## Development

```sh
scripts/build-espeak.sh && export SPEAK_ESPEAK=$PWD/third_party/espeak-ng/bin/espeak-ng
cargo test                        # unit, CLI and kokoro-onnx regression tests
scripts/linux-docker.sh arm64     # the same on Linux, in Docker
scripts/ab.sh                     # fp32 vs int8 samples and numbers into out/ab/
scripts/export-g2p.py             # rebuild assets/g2p/*.onnx (pinned Python environment in its header)
```

Tests that need the model use `$SPEAK_TEST_MODELS` (default `~/.local/share/speak/models`)
read-only and skip when it is missing. `SPEAK_AUDIO=null` plays to nowhere in real time (the queue
tests use it), and `SPEAK_DEBUG=1` also logs places in line and playback times. `SPEAK_TEST_EXACT_AUDIO=1` also checks that the audio
matches kokoro-onnx's to within a 16-bit step, which holds only on the machine that recorded the
reference (an Apple M5 Pro). `spikes/` holds the Go and Rust prototypes this grew from.

## Credits

speak copies its interface from macOS's `say`, so anything that calls `say` can call `speak`
instead. The idea of a `say` with a modern voice comes from [sag](https://github.com/steipete/sag)
by Peter Steinberger, which does it with ElevenLabs' hosted voices; speak is the local
counterpart.

The work it stands on:

- [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) by hexgrad: the voice (Apache-2.0).
- [misaki](https://github.com/hexgrad/misaki) by hexgrad: English pronunciation, its
  dictionaries and rules (Apache-2.0), through [misaki-rs](https://github.com/MicheleYin/misaki-rs)
  (MIT).
- [graphemes_to_phonemes_en_us](https://huggingface.co/PeterReid/graphemes_to_phonemes_en_us) and
  [_en_gb](https://huggingface.co/PeterReid/graphemes_to_phonemes_en_gb) by PeterReid: the neural
  model misaki uses for words its dictionaries lack (Apache-2.0), converted to ONNX and built
  into speak.
- [kokoro-onnx](https://github.com/thewh1teagle/kokoro-onnx) by thewh1teagle: the inference
  pipeline speak follows, and the model files it downloads (MIT).
- [ONNX Runtime](https://onnxruntime.ai) by Microsoft (MIT), linked in through
  [ort](https://github.com/pykeio/ort) by pyke (MIT or Apache-2.0).
- [espeak-ng](https://github.com/espeak-ng/espeak-ng): pronunciation for the other languages
  and, when installed, for English words misaki's dictionaries lack (GPL-3.0-or-later), run as a
  separate program.
- [librosa](https://github.com/librosa/librosa): the silence trimming (ISC).
- The Rust crates in `Cargo.lock`, all under permissive licences.

[NOTICE](NOTICE) has the details.

## Licence

speak is licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your
option: anyone can use, change, package or embed it, for any purpose, as long as the notices
stay. espeak-ng is never linked into speak, only run as a separate program, so its GPL covers
espeak-ng alone.
