# speak: requirements

`speak` is a convenient way to use local text-to-speech models: `say`, but with the best open model available, running on your own machine. People use it, and so do agents (Claude Code calls `speak` to get your attention).

## Principles

- **Local by default.** After the first-run model download nothing leaves the machine. No API keys, no per-character cost, works offline.
- **The best model is the default.** Models are pluggable backends. When a better open model appears, it gets added and becomes the default; older ones stay selectable.
- **Built for agents as much as for people.** Fast start, quiet output, predictable exit codes, stdin-friendly, nothing interactive unless asked.
- **Drop-in for `say`.** The common `say` flags work the same way.

## Command line

```sh
speak "Hello there"            # speak it
echo "piped" | speak           # stdin
speak -f notes.txt             # file ('-' for stdin)
speak -v bm_george "Hi"        # voice
speak -v '?'                   # list voices
speak -r 200 "Faster"          # words per minute, like say (default 175)
speak -o out.m4a "Saved"       # write a file instead of playing
speak -m kokoro "..."          # pick a model/backend explicitly (default: the best installed)
```

- Playback starts after the first sentence while the rest is still being generated.
- `say` flags `speak` doesn't support are accepted and ignored, so scripts written for `say` don't break.
- If the model can't load, fall back to the platform's built-in voice (macOS `say`; equivalent elsewhere) so a message still gets out, and say so on stderr.

## Voices

- `speak voices`: a small interactive picker. Browse voices (grouped by model and language), press a key to hear a sample, select one to save it as the default.
- The default voice and model live in a config file (e.g. `~/.config/speak/config.toml`), also settable with flags or env (`SPEAK_VOICE`, `SPEAK_MODEL`).

## Pronunciations

```sh
speak lexicon add kruapi "crew A P I"   # add or replace
speak lexicon try kruapi                # hear it
speak lexicon list | rm WORD | path
```

- Plain-text file at `~/.config/speak/lexicon.txt`, one `word = spoken form` per line, easy to edit by hand and keep in dotfiles so every machine shares it.
- Whole-word, case-insensitive, longest entry first.
- Later: exact phoneme entries for models that take phonemes (Kokoro does), for words a respelling can't get right.

## Models

- v1 backend: Kokoro-82M (ONNX), 54 voices, 24 kHz.
- Model files are downloaded on first use into a cache directory, verified by checksum, never bundled in the package.
- A backend declares its voices, languages, sample rate and whether it needs phonemes; the CLI, lexicon, playback and file output work the same for every backend.

## Platforms and distribution

- macOS (arm64, x86_64) and Linux (x86_64, arm64) first; Windows later, and nothing in the design may block it.
- Homebrew tap `peetzweg/tap` (`brew install peetzweg/tap/speak`), GitHub release binaries; Linux packages if cheap.
- Licence chosen so all of the above stay possible (see `research/licensing-and-g2p.md`).

## Reference

The working Python prototype is `~/.local/share/speak/speak.py` (kokoro-onnx, espeak-ng). Spikes comparing Go and Rust are in `spikes/`.
