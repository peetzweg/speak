# Rust spike for `speak`: report

Machine: Apple M5 Pro, macOS 27.0.1, Rust 1.95.0. Linux runs: Docker Desktop on the same Mac, `rust:1.95-trixie` (Debian 13); amd64 runs under emulation.
Numbers are from a warm page cache. **[M]** = measured here, **[I]** = inferred (read in source or docs, not exercised).

## Verdict

Rust is a good fit for `speak`.

- **Parity:** it reproduces kokoro-onnx exactly, both the tokens and the audio, with espeak-ng run as a separate process.
- **Packaging:** each target gets one binary with ONNX Runtime linked in.
- **Speed:** first audio comes about twice as fast as the Python reference, and synthesis is about 25% faster.
- **Size:** the core is about 1,300 lines.

The weak spots are packaging problems that any language would hit:

1. **Intel Mac:** no static or prebuilt ONNX Runtime exists for it.
2. **Older Linux:** pyke's prebuilt ONNX Runtime needs glibc 2.38 or newer.
3. **Name clash:** espeak-ng's Homebrew install probably puts a `speak` symlink in `bin/`, which collides with our binary. *(Confirmed after the spike: espeak-ng 1.52.0's `Makefile.am` install hook runs `ln -s speak-ng speak`, and the Homebrew formula runs a plain `make install`.)*

None of these block macOS arm64 or current Linux.

## What was built (`spikes/rust`)

About 1,330 non-blank lines, including a 116-line generated vocab table.

| File | Purpose |
|---|---|
| `src/backend.rs` | The `Backend` trait and a registry (see "Pluggable models") |
| `src/kokoro.rs` | Kokoro-82M: phonemes → tokens → style row picked by token count → ONNX → silence trim. The model session loads lazily; the voice list comes from the voices npz. |
| `src/g2p.rs` | A line-for-line port of phonemizer's espeak path (punctuation preserve/restore and post-processing). Two engines: the **`espeak-ng` subprocess (default)**, and in-process libespeak-ng behind the cargo feature `espeak-inprocess` (a GPL build, kept for comparison). |
| `src/npz.rs`, `src/trim.rs`, `src/vocab.rs` | Reader for voices-v1.0.bin (a stored zip of float32 `.npy` arrays, shape 510×1×256). Port of librosa's `trim`. kokoro-onnx's 114-symbol vocab. |
| `src/audio.rs` | Streaming playback with rodio/cpal: synthesis runs ahead on a thread and playback starts with the first sentence. 16-bit WAV writer. |
| `src/text.rs` | speak.py's sentence chunking and lexicon (whole-word, case-insensitive, longest entry first), plus the config file |
| `src/picker.rs` | The interactive `speak voices` picker (crossterm) |
| `src/main.rs` | say-style CLI (clap): `-v`, `-v '?'`, `-r`, `--speed`, `-f`, `-o`, stdin, and say's flags accepted but ignored. Exit codes. |
| `build.rs` | Links clang's compiler runtime on macOS (needed by ONNX Runtime). With the feature on, compiles espeak-ng 1.52.0 with `cc` (no cmake, bindgen or network). |
| `parity/` | `reference.py` (kokoro-onnx), `check.sh`, `corpus.txt` (21 lines), `audio.py`, and the saved `reference-macos.jsonl` |
| `scripts/` | `fetch-espeak.sh` (espeak-ng 1.52.0 source, sha256-pinned) and `linux-docker.sh` (Linux build and tests) |
| `experiments/misaki-probe/` | Fit check of `misaki-rs` |

**Running it:** `scripts/fetch-espeak.sh` is only needed for the in-process feature. Then `cargo build --release` and `SPEAK_ESPEAK=<path to espeak-ng> target/release/speak "Hello"`.

**Environment variables:**
- `SPEAK_HOME`, `SPEAK_VOICE`, `SPEAK_LEXICON`
- `SPEAK_ESPEAK` (the program) and `SPEAK_ESPEAK_DATA` (optional; the `espeak-ng-data` directory itself, not its parent)
- `SPEAK_G2P=inprocess` (use the in-process engine)
- `SPEAK_EP=auto` (let ONNX Runtime pick CoreML)
- `SPEAK_TIMING`, `SPEAK_DEBUG`

The hello sentence was written to WAV (`out/hello.wav`, 2.53 s) and played through CoreAudio on this Mac **[M]**. Exit codes are: 0 ok, 1 synthesis or audio failure, 2 usage, 3 model or espeak-ng missing, 4 unknown voice.

## Parity with kokoro-onnx

The target sentence produces **identical** phonemes and token ids in every mode **[M]**:

```
kɹˈuː ˌeɪpˌiːˈaɪ ʃˈɪpt vˈiː zˈiəɹoʊ.θɹˈiː.fˈaɪv ˌɔn tˈuːzdeɪ, ænd ˈænaʊks ɡˈeɪm hæd tˈuː,zˈiəɹoʊzˈiəɹoʊ zˈiəɹoʊ bˈɑːts.
[0, 62, 123, 156, 63, 158, 16, 157, 47, 102, 58, 157, 51, 158, 156, 43, 102, 16, 131, 156, 102, 58, 62, 16, 64, 156, 51, 158, 16, 68, 156, 51, 83, 123, 57, 135, 4, 16, 119, 123, 156, 51, 158, 4, 16, 48, 156, 43, 102, 64, 16, 157, 76, 56, 16, 62, 156, 63, 158, 68, 46, 47, 102, 3, 16, 72, 56, 46, 16, 156, 51, 158, 57, 135, 56, 157, 138, 62, 61, 16, 92, 156, 47, 102, 55, 16, 50, 72, 46, 16, 62, 156, 63, 158, 3, 16, 68, 156, 51, 83, 123, 57, 135, 68, 156, 51, 83, 123, 57, 135, 16, 68, 156, 51, 83, 123, 57, 135, 16, 44, 156, 69, 158, 62, 61, 4, 0]
```

The token list is the batch actually fed to the model. kokoro-onnx's `_split_phonemes` re-inserts a space after `.` and `,`, which is why it differs slightly from the phoneme string.

The corpus has 21 lines covering numbers, money, dates, curly quotes, unicode, URLs, multi-line input and one sentence longer than 510 phonemes.

| Check | Result |
|---|---|
| macOS, in-process libespeak-ng 1.52.0 with the reference's espeak-ng-data | 21/21 identical **[M]** |
| macOS, `espeak-ng` 1.52.0 **subprocess** with the same data (a CLI built in scratch; nothing installed globally) | 21/21 identical **[M]** |
| Linux arm64 and amd64, in-process | 21/21 identical **[M]** |
| Linux arm64 and amd64, Debian's packaged `espeak-ng` 1.52.0 with **its own data** | 21/21 identical **[M]** |
| Audio for the "KruAPI…" sentence, CPU execution provider vs kokoro-onnx | Same length (176,640 samples), max abs diff 3e-5 (one 16-bit step), correlation 1.000000 **[M]** |
| Audio with `ort`'s default device policy (CoreML) | Correlation 0.997, max abs diff 0.12: not bit-identical **[M]** |

*Note added when comparing spikes: the Go spike found the `espeak-ng` CLI differs from the library in about 0.4% of words (a stress mark promoted in clauses with no primary stress, e.g. `what?!`). The 21-line corpus here may simply not contain such a clause; check before relying on subprocess parity.*

Things that bit, worth knowing:

- **Run parity checks with `SPEAK_LEXICON=/dev/null`.** The first apparent mismatch ("KruAPI", "Anouk") was the port correctly applying `~/.config/speak/lexicon.txt`, which has entries for both words. kokoro-onnx has no lexicon.
- **The reference mangles numbers, and the port copies that on purpose.** phonemizer splits text at every punctuation mark before espeak sees it. So "2,000" becomes "two, zero zero zero" and "v0.3.5" becomes "vee zero. three. five". Matching it took a literal port of `Punctuation.preserve/restore` (~120 lines). The product should fix this, either by giving espeak whole sentences or by using misaki, which reads "2,000" as "two thousand" **[M]**.
- **`ort` silently registers CoreML on macOS.** Its default device policy (MaxEfficiency) adds about 2.5 s to model load (2.75 s vs 0.25 s) and changes the numbers. The prototype forces `AutoDevicePolicy::PreferCPU` **[M]**.
- **Subprocess protocol:** espeak-ng's line mode flushes stdout after every input line. One `espeak-ng -q --ipa --sep=_ -v en-us` child runs per language, with a sentinel line sent after each chunk to mark where each chunk's output ends. The CLI writes the same per-clause strings as `espeak_TextToPhonemes`.
- **Static-linking gotchas:**
  - espeak-ng's archive must come before `ucd-tools` on the link line for GNU ld; macOS ld64 doesn't care.
  - On macOS, overwriting a binary that has already run with `cp` gets the new copy killed with signal 9. Delete it first.

## Measurements

| Metric | Rust | Python reference (same Mac) |
|---|---|---|
| Process start → first audio sample pulled by CoreAudio, "Hello Philip. This is the Rust spike." | **501–544 ms** over 5 runs (median ≈508); in-process espeak 501–512 ms **[M]** | speak.py takes 0.82–1.12 s to *write* "Hello Philip." to WAV, before afplay even starts **[M]** |
| What the first-sentence time is made of (`-o`) | 458–535 ms: model load 230–255 ms, first 16-token inference ~210 ms, espeak-ng start 20–50 ms **[M]** | import 0.17 s + model load 0.20 s + first create 0.8 s **[M]** |
| RTF, 43-word paragraph (14.16 s of audio), inference only | **0.127** (1.80 s) **[M]** | 0.17–0.19 **[M]** |
| Same paragraph, end to end incl. process start and model load | 2.06 s, RTF 0.146 **[M]** | 2.9–3.1 s **[M]** |
| Peak RSS | 551 MB (hello, playing), 658 MB (paragraph) **[M]** | 711 MB **[M]** |
| Release binary (stripped, LTO, ONNX Runtime static) | macOS **21.1 MB** (8.2 MB gzipped); in-process espeak adds 0.28 MB. Linux arm64 27.4 MB, amd64 27.2 MB **[M]** | — |
| G2P cost per sentence, subprocess / in-process | First call 20–50 ms / 6–10 ms. After that 3.7–5.2 ms / 0.08–0.18 ms **[M]** | — |
| `speak --version` | ≤ 20 ms **[M]** | — |
| Linux arm64 container: model load / hello first chunk / paragraph RTF | 350 ms / 555 ms / 0.104 **[M]** | — |
| Linux amd64 (emulated, so not representative) | 700 ms / 1.18 s / 0.21 **[M]** | — |

The model weights dominate memory (the 325 MB ONNX file plus ONNX Runtime's buffers). The Rust audio duration includes a 0.12 s gap between sentences.

## How the binary finds its dependencies at runtime

**ONNX Runtime: nothing to find at runtime.**
- `ort` 2.0.0-rc.15 with `download-binaries` fetches pyke's prebuilt **static** `libonnxruntime.a` (ONNX Runtime 1.31) from `cdn.pyke.io` at *build* time.
- It checks the download against a hash pinned in `ort-sys`, caches it in `~/Library/Caches/ort.pyke.io`, and links it in.
- `otool -L` shows only system libraries: libc++, Foundation, CoreML, AudioToolbox, CoreAudio, CoreFoundation, libSystem, libiconv and libobjc **[M]**.
- The download doesn't use GitHub, so the microsoft/onnxruntime 404 doesn't affect it. Builds in fresh containers still worked **[M]**.

Two gotchas with pyke's macOS archive:
- It needs clang's runtime library (`clang_rt.osx`, for `__isPlatformVersionAtLeast`). `build.rs` adds it **[M]**.
- Its objects are built for macOS 13.4 or later, so that is the real minimum whatever `MACOSX_DEPLOYMENT_TARGET` says **[M]**.

**espeak-ng in the default build:**
- Runs the `espeak-ng` program found via `$SPEAK_ESPEAK` or `PATH`, with that install's own data. `$SPEAK_ESPEAK_DATA` overrides the data (its parent is passed as `--path`).
- If the program is missing, speak exits with code 3 and a hint.

**espeak-ng in the in-process build:**
- It is linked statically.
- Data comes from `$SPEAK_ESPEAK_DATA` or `$SPEAK_HOME/es/espeak-ng-data`.
- The parent path must stay under 160 bytes, because espeak-ng silently truncates longer paths.

**Linux runtime libraries** (from `ldd`) **[M]**: libstdc++.so.6, libasound.so.2, libgcc_s, libm and libc. It needs **glibc 2.38 or newer** on both architectures, because pyke's ONNX Runtime objects reference `__isoc23_strtol*`. The 2.39 symbols are weak references from Rust's standard library.

## Distribution, per OS

**macOS arm64: works [M].**
- ONNX Runtime: static, from pyke. One self-contained binary.
- espeak-ng: `brew install espeak-ng` (1.52.0, arm64 bottles), declared in the formula with `depends_on "espeak-ng"`.
- Audio: cpal → CoreAudio, no extra dependencies **[M]**.

**macOS x86_64: no ONNX Runtime anywhere [M].**
- pyke's binary table has no `x86_64-apple-darwin`.
- The PyPI onnxruntime 1.32 wheels are macOS arm64 only.
- Homebrew's `onnxruntime` has no Intel bottles.
- Options: build ONNX Runtime from source in CI **[I]**, use `ort`'s `load-dynamic` against a library the user provides, or drop Intel.
- espeak-ng has Homebrew Intel bottles for Sonoma and Ventura (checked via the Homebrew API). Audio would use CoreAudio.
- Not attempted: it would need a new rustup target (a global change) plus ONNX Runtime built from source.

**Linux x86_64 and arm64 (glibc): built and run in Docker on both architectures [M].**
- ONNX Runtime: static, from pyke, for both architectures.
  - Needs glibc 2.38 or newer: Debian 13 and Ubuntu 24.04 are fine; Debian 12, Ubuntu 22.04 and RHEL 9 are not **[M]**.
  - pyke has no musl build, so a fully static binary isn't possible **[I]**.
- espeak-ng: the distro package.
  - Debian 13 ships 1.52.0 and produces identical phonemes **[M]**. It installs only `espeak-ng` and `speak-ng` **[M]**.
  - Older distros ship 1.51, so phonemes may differ **[I]**.
- Audio: cpal → ALSA.
  - Building needs `libasound2-dev`; running needs `libasound.so.2` **[M]**. PipeWire and PulseAudio should work through their ALSA plugins **[I]**.
  - With no audio device, speak exits cleanly with code 1, but ALSA prints 8 lines of noise to stderr first **[M]**. `snd_lib_error_set_handler` would silence it.

**Windows x86_64 and arm64: not built (needs MSVC or `cargo-xwin`).**
- ONNX Runtime: static, from pyke (`*-pc-windows-msvc`). Those builds include DirectML and link against D3D12, DXGI and DirectML **[I, from ort-sys's build script]**.
- espeak-ng has no standard package manager. Options: the official MSI, located via the `HKLM\Software\eSpeak NG` registry key that espeak-ng itself reads **[I]**; or ship `espeak-ng.exe` and its data in the zip alongside speak, as the licensing doc allows.
- Audio: cpal → WASAPI, no extra dependencies **[I]**. The picker already ignores the key-release events Windows sends.

**cargo-dist and Homebrew tap [M].** `dist` 0.32.0 was run in a scratch copy, not in the spike folder:
- `dist init --yes --installer homebrew --installer shell` with 5 targets, `tap = "peetzweg/homebrew-tap"` and `publish-jobs = ["homebrew"]`.
- It generated `release.yml`, which builds on GitHub runners (macos-14, macos-15-intel, ubuntu-22.04, ubuntu-24.04-arm, windows-2022) and publishes `speak.rb` to the tap.
- `dist build --artifacts=global` produced a working formula skeleton: one download URL per OS/architecture, then `bin.install "speak"`.

Gaps found:
- **The generated formula has no `depends_on "espeak-ng"` [M].** `[dist.dependencies.homebrew]` only installs into CI. We'll need a hand-maintained formula or a post-processing step **[I]**.
- **The default x86_64 Linux runner is `ubuntu-22.04`, which has glibc 2.35.** Linking pyke's ONNX Runtime there should fail **[I]**. Pin `ubuntu-24.04`.
- **The `x86_64-apple-darwin` job will fail** because no ONNX Runtime binary exists for it **[I]**.
- **Name clash:** espeak-ng 1.52.0's install step runs `ln -s speak-ng speak` in `bin/`, and Homebrew builds espeak-ng with `make install`, so `brew install espeak-ng` puts a `speak` symlink in `$(brew --prefix)/bin`, which collides with ours. Debian strips it **[M]**.

## Pluggable models

```rust
pub trait Backend: Send {
    fn info(&self) -> Info;                 // id, model, sample_rate, default_voice, assets (url + sha256 for first-run download)
    fn voices(&self) -> Result<Vec<Voice>>; // id, language, label
    fn synthesize<'a>(&'a mut self, text: &str, voice: &str, speed: f32)
        -> Result<Box<dyn Iterator<Item = Result<Chunk>> + 'a>>; // mono f32 PCM at info().sample_rate
}
pub fn open(name: &str, home: &Path) -> Result<Box<dyn Backend>>; // registry; DEFAULT = "kokoro"
```

- **What the front end owns:** lexicon, sentence splitting, playback, WAV output, the picker and exit codes. It only sees `dyn Backend`, so the sample rate and voice list always come from the backend.
- **G2P is a shared library, not part of the trait.** A backend may use `g2p::Engine` and `g2p::phonemize`. Backends that take raw text, such as a local HTTP server or an end-to-end model, simply don't.
- **Streaming:** `synthesize` returns an iterator, so a backend can stream within a sentence. Kokoro yields one chunk per phoneme batch.

How other models would slot in **[I]**:
- **Kitten TTS:** also ONNX, also takes espeak IPA, also uses a voices npz, also 24 kHz. Roughly a 150-line module reusing `g2p`, `npz` and the session code, plus one registry line.
- **Piper (ONNX VITS):** different inputs (`input`, `input_lengths`, `scales`, optionally `sid`). The phoneme-id map and sample rate (often 22.05 kHz) come from its JSON config. It needs its own token mapping with begin/end/pad markers. Its phonemes come from piper-phonemize, a fork of espeak, so exact parity with Piper's own tooling would need checking.
- **MLX, candle, llama.cpp-style runtimes:** each becomes its own module behind a cargo feature, so the default binary doesn't grow.
- **Local HTTP server:** a client behind the same trait.

One rough edge: in `ort` 2.0, running a session needs exclusive (`&mut`) access. A warm daemon serving parallel requests would need one session per worker or a mutex.

## Agents as users

- **Quiet by default.** stdout is silent unless output is asked for. Errors are a single `speak: …` line on stderr.
- **Stable exit codes,** as listed above.
- **Plain output when not on a terminal.** `-v '?'` and `speak voices` print a plain list when stdin or stderr isn't a terminal.
- **`--analyze`** prints phonemes, batches and tokens as JSON, for debugging pronunciation.
- **`--timing`** prints one JSON timing line.
- **Startup:** about 0.5 s, mostly model load and the first inference. A warm daemon would remove it.
- **Remaining noise:** ALSA's stderr output on headless Linux. espeak-ng's own warnings are already discarded.

## Voice picker

`speak voices` uses **crossterm 0.29** (MIT). It supports macOS and Linux terminals and the Windows console and Windows Terminal (Windows not tested).

How it works:
- Up/Down or j/k browse the list.
- Space plays a sample. It is synthesised on a worker thread that owns the backend, and only the newest request is played.
- Enter saves `voice = "…"` to `~/.config/speak/config.toml`.
- q or Esc quits.

Tested through a pseudo-terminal: three `j` presses and Enter moved from af_heart to af_nicole and wrote the config file, exit 0 **[M]**. The audio preview was not exercised automatically.

Other crates considered:
- **ratatui:** worth it if the picker grows (filters, columns).
- **inquire / dialoguer:** can't play a sample without selecting it, short of hacks.

## G2P plan: misaki-rs fit check ([M] unless marked)

`misaki-rs` 0.11.0 is MIT and actively maintained. The repo was created in January 2026 and has commits from today; it has 10 stars, 15 forks and one maintainer. `default-features = false` removes espeak.

Results from the probe:
- **Startup and speed:** construction takes 40–46 ms, then about 3 ms per sentence.
- **Size:** the probe binary is 22.5 MB, because the lexicons (about 12 MB of JSON) and a 6 MB tagger are embedded. Adding it to speak would take the binary to roughly 40 MB **[I]**.
- **Alphabet:** output uses misaki's alphabet (`A I O W Y` and so on), which Kokoro's vocab accepts.
- **Gets right:**
  - "2,000" → "two thousand"
  - "$1,234.56" → dollars and cents
  - the two readings of "read"
- **Unknown words come out as `❓`:** the made-up names in the test sentence, and the colon in "10:30am". They need the espeak-ng subprocess as its `Fallback`. That is about a 10-line implementation plus misaki's espeak-to-misaki conversion table (about 60 lines, MIT), which today only exists behind its `espeak` feature **[I]**.

It fits the licensing doc's plan for v1: user lexicon, then misaki, then espeak-ng for unknown words. The risks are the unclear source of misaki's dictionaries and the single maintainer.

## Kokoros (lucasjinreal/Kokoros)

Apache-2.0. Useful as a reference for loading voices and calling `ort`, but not something to depend on **[I, from reading the code]**:
- Its `phonemizer.rs` is a stub (`todo!()`); it calls espeak-rs directly.
- It doesn't port phonemizer's punctuation handling, so its tokens won't match kokoro-onnx.
- It builds its vocab from Kokoro's symbol list through a HashMap with a duplicated `'` character, so that character's token id can change from run to run.
- It pulls in tokio, reqwest and opus/mp3 encoders.
- espeak-rs builds espeak-ng in-process, which makes the result GPL.

## Lines of code

About 1,330 non-blank, non-comment lines:

| File | Lines |
|---|---|
| `g2p.rs` (incl. ~110 for the in-process engine) | 329 |
| `main.rs` | 246 |
| `kokoro.rs` | 219 |
| `vocab.rs` (generated) | 116 |
| `text.rs` | 99 |
| `picker.rs` | 69 |
| `audio.rs` | 67 |
| `build.rs` | 63 |
| `backend.rs` | 52 |
| `npz.rs` | 37 |
| `trim.rs` | 30 |

## Risks and open questions

1. **Intel Mac:** needs a self-built ONNX Runtime, or we drop it.
2. **Linux glibc ≥ 2.38 floor** from pyke's ONNX Runtime. We could lower it by building ONNX Runtime on manylinux_2_28, or by using `load-dynamic` with the PyPI or distro library **[I]**. Debian 13's `libonnxruntime1.21` is old **[M]**.
3. **Upstream dependence:** we'd rely on pyke's CDN and on `ort` 2.0.0-rc, whose API changes between release candidates (rc.15 added an `Environment` argument). Mitigations: pin the version, mirror the hashed archive, or point `ORT_LIB_LOCATION` at our own build **[I]**.
4. **espeak-ng version drift** between distros (1.51 vs 1.52) changes phonemes **[I]**. Moving English to misaki shrinks this.
5. **Homebrew `bin/speak` clash:** confirmed in espeak-ng's `Makefile.am`; settle the command and formula name before release.
6. **First-run download** (325 MB model + 28 MB voices) with checksum: the per-backend asset list is there, the downloader isn't built.
7. **Unexplained:** the Linux arm64 container is faster than native macOS (RTF 0.104 vs 0.127).
8. **Not verified:** a Windows build, Linux audio on real hardware, the picker's audio preview, and memory on long texts.

## Housekeeping

- **Installed nothing globally:** no Homebrew packages, no rustup targets.
- **espeak-ng CLI for the macOS subprocess parity runs:** built from the 1.52.0 release in the session scratchpad (`…/scratchpad/ext/es152/espeak-ng-1.52.0/build/src/espeak-ng`), which is temporary.
- **Docker:**
  - Every container ran with `--rm`; none are left running.
  - The `rust:1.95-trixie` image is still on this Mac (4.56 GB on disk). Remove it with `docker rmi rust:1.95-trixie`.
  - Cargo and target caches for the Linux builds are in `.docker-cache/` inside the spike folder (1.3 GB, gitignored).
- **`out/` (gitignored):** macOS binaries `speak` and `speak-inproc`, Linux binaries for both architectures, WAV files and the Linux logs, and a copy of espeak-ng-data.
- **Not committed.** `.gitignore` covers `target/`, `/out`, `/third_party` and `/.docker-cache`.
