# speak

`say`, but with the best open text-to-speech model, running on your own machine. After a one-time
model download nothing leaves the computer: no API keys, no per-character cost, works offline.
Built for people and for agents (Claude Code calls `speak` to get your attention): it starts
talking after the first sentence, stays quiet on stdout, and has stable exit codes.

The default model is [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) (54 voices, 24 kHz).
Models are pluggable backends; when a better open model appears it becomes the default.

## Install from source

Needs Rust (stable) and, on Linux, `libasound2-dev` (ALSA headers). macOS on Apple silicon and
Linux x86_64/arm64 with glibc 2.38 or newer (Debian 13, Ubuntu 24.04) are supported.

```sh
cargo install --path .
```

ONNX Runtime is linked into the binary, so there is nothing else to install. Optionally install
`espeak-ng` (`brew install espeak-ng`, `apt install espeak-ng`): English works without it, but
words outside speak's English lexicon are then spelled out, and the Spanish, French, Hindi,
Italian, Portuguese, Japanese and Chinese voices need it. `scripts/build-espeak.sh` builds
espeak-ng 1.52.0 into `third_party/` instead; point `SPEAK_ESPEAK` at it. (Homebrew's espeak-ng
also installs a `speak` link; make sure `~/.cargo/bin` comes first in your `PATH`.)

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

## Pronunciations

```sh
speak lexicon add kruapi "crew A P I"   # add or replace a respelling
speak lexicon add kokoro /kˈOkəɹO/      # exact phonemes (Kokoro/misaki alphabet)
speak lexicon try kruapi                # print what it becomes and hear it
speak lexicon list | rm WORD | path
```

The lexicon is a plain text file, `~/.config/speak/lexicon.txt` (or `$SPEAK_LEXICON`), one
`word = spoken form` per line, `#` for comments. Entries match whole words, ignoring case,
longest entry first. `lexicon try` prints the phonemes too, a good starting point for a
`/phonemes/` entry.

English goes through [misaki](https://github.com/hexgrad/misaki)'s lexicons (via
[misaki-rs](https://github.com/MicheleYin/misaki-rs)), which read numbers, money, dates,
versions and times naturally ("2,000" is "two thousand", "v0.3.5" is "version zero point three
point five"). Words misaki doesn't know go to espeak-ng, run as a separate program.

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
first job: it takes phonemes in and makes the voice itself. speak does English pronunciation
with misaki and asks espeak-ng only for Kokoro's other languages and for English words misaki
doesn't know.

## Development

```sh
scripts/build-espeak.sh && export SPEAK_ESPEAK=$PWD/third_party/espeak-ng/bin/espeak-ng
cargo test                        # unit, CLI and kokoro-onnx regression tests
scripts/linux-docker.sh arm64     # the same on Linux, in Docker
scripts/ab.sh                     # fp32 vs int8 samples and numbers into out/ab/
```

Tests that need the model use `$SPEAK_TEST_MODELS` (default `~/.local/share/speak/models`)
read-only and skip when it is missing. `spikes/` holds the Go and Rust prototypes this grew from.

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
- [kokoro-onnx](https://github.com/thewh1teagle/kokoro-onnx) by thewh1teagle: the inference
  pipeline speak follows, and the model files it downloads (MIT).
- [ONNX Runtime](https://onnxruntime.ai) by Microsoft (MIT), linked in through
  [ort](https://github.com/pykeio/ort) by pyke (MIT or Apache-2.0).
- [espeak-ng](https://github.com/espeak-ng/espeak-ng): pronunciation for the other languages
  (GPL-3.0-or-later), run as a separate program.
- [librosa](https://github.com/librosa/librosa): the silence trimming (ISC).
- The Rust crates in `Cargo.lock`, all under permissive licences.

[NOTICE](NOTICE) has the details.

## Licence

speak is licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your
option: anyone can use, change, package or embed it, for any purpose, as long as the notices
stay. espeak-ng is never linked into speak, only run as a separate program, so its GPL covers
espeak-ng alone.
