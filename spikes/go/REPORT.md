# Go spike for `speak`: report

2026-11-19. Apple M5 Pro (15 cores), 24 GB, macOS 27.0.1. Go 1.27.4 via `mise` (local `mise.toml`). No global packages were installed. Linux runs used `--rm` Docker containers based on `debian:trixie-slim`.

**[M]** means measured here; **[I]** means inferred, or read in source or docs but not tested.

## Verdict

Go works for this product:
- The pipeline ports cleanly and its output matches kokoro-onnx bit for bit.
- Startup is about 2× faster than the Python tool.
- Audio, TUI and cross-compilation all have good pure-Go options.

The weak point is not Go itself but **ONNX Runtime bindings**:
- The mature binding (`yalue/onnxruntime_go`) needs cgo and pins the newest ORT C API (v31).
- The cgo-free binding (`shota3506/onnxruntime-purego`) is unstable and does not compile for Windows.

If we choose Go, plan for a small in-house purego ORT binding (about 250 LOC **[I]**). With it, every target builds with `CGO_ENABLED=0` from one machine; this spike already shows that for darwin and linux.

The hardest distribution problems are not Go-specific:
- ORT has no Intel macOS builds after 1.23.
- The microsoft/onnxruntime GitHub repo returns 404.
- Debian's ORT 1.21 is too old for both bindings.
- The espeak-ng CLI does not reproduce `espeak_TextToPhonemes` exactly.

## What was built

The code is in `spikes/go`: about 2,200 lines of Go, plus 121 generated lines (the vocab table) and 73 lines of Python parity tooling in `ref/`.

| Path | Contents |
|---|---|
| `internal/tts` | `Backend` interface and registry; library lookup |
| `internal/kokoro` | Port of kokoro-onnx 0.4.7 (pipeline below) |
| `internal/espeak` | Three G2P routes behind one interface (below) |
| `cmd/speak-espeak` | 44-line GPL helper that runs `espeak_TextToPhonemes` in its own process, over stdin/stdout |
| `internal/ort` | Two ORT bindings behind one interface: `yalue/onnxruntime_go` (cgo) by default, or `shota3506/onnxruntime-purego` (no cgo) with `-tags purego` |
| `internal/audio` | WAV writer, and streaming playback through `ebitengine/oto/v3` with a stall watchdog |
| `internal/text` | speak.py's sentence chunker and lexicon. RE2 has no look-behind, so both boundary checks are hand-written |
| `internal/tui` | `speak voices` picker (bubbletea v2) |
| `cmd/speak` | The CLI: `-v`, `-v '?'`, `-r`, `--speed`, `-f`, `-o`, args or stdin, lexicon, config, exit codes. Debug flags: `--debug-tokens`, `--raw`, `--timings` |

The Kokoro pipeline in `internal/kokoro`:
1. `phonemize`.
2. `_split_phonemes`: batches of at most 510 code points, split at `.,!?;`.
3. Vocab ids, padded with 0 at both ends.
4. Style vector `voice[len(tokens)]`.
5. ORT inputs: `tokens` int64 [1,n], `style` f32 [1,256], `speed` f32 [1].
6. Output is 24 kHz f32 audio, then librosa-style `trim` (60 dB, frame 2048, hop 512).

`voices-v1.0.bin` is a NumPy `.npz`: a zip of `<voice>.npy` files, each `<f4` with shape (510,1,256). The spike reads one voice lazily.

The three G2P routes in `internal/espeak`:
- libespeak-ng loaded in-process through purego.
- The `espeak-ng` CLI as a subprocess, either one process per call or one long-lived process.
- The GPL helper process.

All three share a line-for-line port of phonemizer's punctuation preserve/restore, `_postprocess_line` and voice selection.

Playback starts after the first sentence. A goroutine pushes each sentence's audio into a channel that oto reads while later sentences are still synthesising. The audio device opens while the model is loading.

Assets:
- ORT 1.32.0 native libraries come from the **NuGet** package `Microsoft.ML.OnnxRuntime`, in `third_party/ort-nuget`.
- libespeak-ng 1.52.0 and its data come from the PyPI `espeakng-loader` wheels. The macOS dylib is byte-identical to the venv's copy, and the data is identical to `~/.local/share/speak/es`.
- The models were read in place, read-only.

Reproduce, in the spike dir with Go from `mise`:
```
go build -o bin/speak ./cmd/speak                                      # cgo binding
CGO_ENABLED=0 go build -tags purego -o bin/speak-purego ./cmd/speak    # no cgo
bin/speak "Hello Philip. This is the Go spike."                        # plays [M]
bin/speak -o out/hello.wav "Hello Philip. This is the Go spike."       # 16-bit 24 kHz WAV [M]
```

## Parity with kokoro-onnx

Test sentence: "KruAPI shipped v0.3.5 on Tuesday, and Anouk's game had 2,000 bots."

The Go side is `--debug-tokens`. The Python side is `ref/ref.py`, which calls kokoro-onnx's own `Tokenizer.phonemize`, `_split_phonemes` and `tokenize`. Both ran with `SPEAK_LEXICON=/nonexistent`, so the real lexicon did not rewrite "KruAPI" or "Anouk". **[M]**
```
go phonemes: kɹˈuː ˌeɪpˌiːˈaɪ ʃˈɪpt vˈiː zˈiəɹoʊ.θɹˈiː.fˈaɪv ˌɔn tˈuːzdeɪ, ænd ˈænaʊks ɡˈeɪm hæd tˈuː,zˈiəɹoʊzˈiəɹoʊ zˈiəɹoʊ bˈɑːts.
py phonemes: kɹˈuː ˌeɪpˌiːˈaɪ ʃˈɪpt vˈiː zˈiəɹoʊ.θɹˈiː.fˈaɪv ˌɔn tˈuːzdeɪ, ænd ˈænaʊks ɡˈeɪm hæd tˈuː,zˈiəɹoʊzˈiəɹoʊ zˈiəɹoʊ bˈɑːts.
go ids: [0,62,123,156,63,158,16,157,47,102,58,157,51,158,156,43,102,16,131,156,102,58,62,16,64,156,51,158,16,68,156,51,83,123,57,135,4,16,119,123,156,51,158,4,16,48,156,43,102,64,16,157,76,56,16,62,156,63,158,68,46,47,102,3,16,72,56,46,16,156,51,158,57,135,56,157,138,62,61,16,92,156,47,102,55,16,50,72,46,16,62,156,63,158,3,16,68,156,51,83,123,57,135,68,156,51,83,123,57,135,16,68,156,51,83,123,57,135,16,44,156,69,158,62,61,4,0]
py ids: [0,62,123,156,63,158,16,157,47,102,58,157,51,158,156,43,102,16,131,156,102,58,62,16,64,156,51,158,16,68,156,51,83,123,57,135,4,16,119,123,156,51,158,4,16,48,156,43,102,64,16,157,76,56,16,62,156,63,158,68,46,47,102,3,16,72,56,46,16,156,51,158,57,135,56,157,138,62,61,16,92,156,47,102,55,16,50,72,46,16,62,156,63,158,3,16,68,156,51,83,123,57,135,68,156,51,83,123,57,135,16,68,156,51,83,123,57,135,16,44,156,69,158,62,61,4,0]
```

Phonemes, batches and ids are all identical.
- The ids shown are the batch actually fed to the model. `_split_phonemes` adds a space after `.` and `,`, so they are not simply `tokenize(phonemes)`.
- "2,000" comes out garbled because phonemizer splits the text at the comma before espeak sees it. The port reproduces that faithfully.

| Check | In-process libespeak-ng | `espeak-ng` CLI subprocess | GPL helper process |
|---|---|---|---|
| 14 cases vs kokoro-onnx: test sentence, 9-line punctuation/Unicode corpus, multi-line text that splits into 2 batches, en-gb/es/fr voices | 14/14 identical [M] | 13/14 [M] | 14/14 [M] |
| 80 prose lines (844 words), compared with the in-process route | n/a (baseline) | 77/80 identical; the other 3 differ by one stress mark each [M] | 80/80 [M] |
| Audio from `Kokoro.create(test sentence)` vs Python, macOS | **bit-identical** with both bindings [M] | n/a | bit-identical [M] |
| Linux arm64 and amd64 in Docker: tokens vs macOS Python | identical, both bindings [M] | identical, using Debian's espeak-ng 1.52.0+dfsg [M] | not run |
| Linux arm64 audio vs macOS Python | same length, correlation 0.998, max diff 0.089 [M]; different ORT build, not a Go issue [I] | not run | not run |

Why the CLI differs:
- The CLI prints its phoneme trace from the synthesis path, which runs `CalcPitches` (espeak-ng 1.52.0, `intonation.c:count_pitch_vowels`).
- When a clause has no primary stress, that code promotes the last syllable with the highest stress to primary. `espeak_TextToPhonemes`, which kokoro-onnx uses, never does this.
- Example: `what?!` gives `wˌʌt` from the library but `wˈʌt` from the CLI. The same happens to "but,", "with" and "was" at the end of a chunk.
- This cannot be reversed from the CLI output.

Options, in order of preference:
1. Accept it. It changes stress marks only, on short function-word chunks. Whether it is audible is untested.
2. Ship the separate GPL `speak-espeak` helper. It gives exact parity and the main binary stays Apache-2.0 (licence reading [I]).
3. Go misaki-first, so espeak only sees unknown words.

## Measurements (macOS, Apple Silicon)

| Metric | Value |
|---|---|
| Exec → first audio sample handed to the device (first sentence "Hello Philip.") | **0.42 s** median (0.420–0.457, n=5, warm page cache) [M]. The first run of a freshly built binary took 0.93 s [M], probably macOS's first-launch check [I]. A true cold boot was not measured: `purge` needs sudo |
| Where the startup time goes | ORT session creation 188 ms (240 on first run), libespeak-ng init 0.7 ms (6.7 first), voices index 0.1 ms, first sentence synthesis ≈0.2 s [M] |
| Python reference: exec → first sentence written (before `afplay` starts) | 0.84 s median [M] |
| Real-time factor, 45-word paragraph (14.06 s of audio) | **0.143** cgo / 0.152 purego, median of 5. Synthesis 2.0 s, wall 2.24 s. speak.py on the same text: 3.32 s wall [M] |
| Peak memory (paragraph, `-o`) | 683 MB max RSS (652 MB footprint) cgo, 658 MB purego, Python 686 MB [M]. Mostly ORT and the fp32 model |
| Binary size (stripped) | darwin/arm64 **4.9 MB** for both bindings; linux purego 5.4 MB (arm64) / 5.6 MB (amd64); 7.6 MB unstripped; GPL helper 2.1 MB [M] |
| What ships beside the binary | libonnxruntime 45 MB (mac arm64) / 26 MB (linux arm64) / 34 MB (linux x64); libespeak-ng 0.5–0.6 MB; espeak-ng-data 19 MB in full, of which English needs about 1.5 MB [I] |

G2P cost for the test sentence (5 punctuation chunks), all measured:

| Route | Per sentence | Startup |
|---|---|---|
| In-process | 0.055 ms | 7 ms |
| GPL helper | 0.08 ms | 20 ms |
| Long-lived `espeak-ng` CLI | 4.7 ms (6.1 ms on Linux). It still synthesises audio for every line, even with `-q` | 27 ms |
| One `espeak-ng` spawn per chunk | 66 ms on macOS (about 14 ms per spawn), 13 ms on Linux | n/a |

Synthesis costs about 0.2 s per short sentence, so every G2P route except spawn-per-chunk is noise by comparison. A warm daemon would remove the ~190 ms of ORT session creation [I].

How the binary finds its runtime pieces [M]:

| Piece | Lookup order |
|---|---|
| Shared libraries | `$SPEAK_ORT_LIB` / `$SPEAK_ESPEAK_LIB`, then `<exe>/../lib`, `<exe>/lib`, the exe dir and `<exe>/../lib/<os>-<arch>`, then `/opt/homebrew/lib` and `/usr/local/lib`, then the bare library name for the system loader. Libraries are loaded at runtime with dlopen (through purego, or yalue's own dlopen) |
| espeak-ng data | `$SPEAK_ESPEAK_DATA`, then `<exe>/../share/speak`, `<exe>/../share`, then `$SPEAK_HOME/es` |
| espeak-ng program (subprocess mode) | `$SPEAK_ESPEAK_BIN`, then `PATH`. If it is missing, `speak` exits with code 3 |

Data paths longer than 144 bytes are rejected up front, because espeak silently truncates `<path>/espeak-ng-data` at 160 characters.

## Distribution analysis

ORT bindings for Go (measured unless marked):

| | yalue/onnxruntime_go | shota3506/onnxruntime-purego | Our own purego binding |
|---|---|---|---|
| Licence | MIT | MIT | ours |
| Maintenance | v1.38.0 (2026-10-23), active, one main maintainer | untagged, last commit 2026-03-15, README says "unstable" | about 250 LOC covering about 15 C API calls [I] |
| cgo | **yes** (C wrapper, then dlopen) | no | no |
| ORT C API version requested | 31, so it needs ORT ≥ 1.31 | 23, so it needs ORT ≥ 1.23 | our choice; a low one (say 17) would accept old distro and Intel builds [I] |
| Windows | builds (cgo via zig) | **does not compile** (`purego.Dlopen` is missing on Windows) | LoadLibrary plus `purego.RegisterLibFunc` [I] |

Static vs shared ORT:
- The official artifacts (NuGet, PyPI, Homebrew) ship only shared libraries. NuGet has `.so`, `.dylib` and `.dll` plus import `.lib` files, and no static archives [M].
- A static ORT would mean building ORT and its C++ dependencies (abseil, protobuf, onnx, re2) for every target, and would force cgo [I].
- So for Go, ORT ships as a shared library or comes from the OS.

Where to get ORT, since `github.com/microsoft/onnxruntime` and its release URLs returned 404 today on both web and API [M]:

| Source | What it has [M] |
|---|---|
| NuGet `Microsoft.ML.OnnxRuntime` 1.32.0 | osx-arm64, linux-x64/arm64, win-x64/arm64. **No osx-x64** |
| PyPI wheels | 1.23.0 is the last release with macOS x86_64 wheels; from 1.24.1 on, macOS arm64 only |
| Homebrew `onnxruntime` 1.32.0 | Bottles for arm64 macOS (sequoia, tahoe, golden_gate) and arm64/x86_64 Linux. **No Intel macOS bottle** |
| Debian trixie `libonnxruntime1.21` | **Both bindings refuse it** ("API version [23]/[31] is not available") |
| Arch `onnxruntime-cpu` | Per the licensing doc; untested here |

Per OS:

| OS | ORT | espeak-ng | Audio | Status |
|---|---|---|---|---|
| macOS arm64 | Homebrew `onnxruntime`, or vendor the NuGet/PyPI dylib as a tap resource | Homebrew `espeak-ng` (CLI, or library for the helper) | oto (AudioToolbox via purego, no cgo) | Fully working [M] |
| macOS x86_64 | Nothing current. Pin ORT 1.23.x (x86_64 wheel) with a binding that requests API ≤ 23, or build ORT from source | Homebrew bottles exist for Intel ventura/sonoma | oto | Cross-compiles with both bindings [M]; never run, as this Mac has no Rosetta |
| Linux x86_64/arm64 | Homebrew on Linux, or vendor the NuGet/PyPI `.so`. Distro packages may be too old | Distro `espeak-ng` (Debian 1.52.0+dfsg works and matched the test sentence) | oto: pure-Go PulseAudio, ALSA fallback via dlopen of `libasound.so.2`, no cgo | Ran in Docker on arm64 (native) and amd64 (emulated), both bindings, WAV output [M] |
| Windows | NuGet `onnxruntime.dll` next to the exe | `espeak-ng.exe` on PATH via winget, Scoop or msi (per research doc) | oto (WASAPI/winmm), no cgo | The cgo build compiles via zig; never run [M]. The purego build is blocked by the binding |

Audio: `ebitengine/oto/v3` 3.5.1 (Apache-2.0) needs no cgo on any of the three OSes.
- Playback works on macOS [M].
- In a headless container, oto opened a device that never drained, and `speak` hung forever [M]. A watchdog now exits 1 and suggests `-o` if playback runs more than 3 s past the audio length [M].
- PipeWire should work through its PulseAudio server [I].

cgo cross-compilation:
- The purego build avoids the problem. One Mac produced darwin amd64/arm64 and linux amd64/arm64 binaries with a plain `go build`, in seconds [M].
- Those binaries are still dynamically linked to libc, because purego needs dlopen. That means glibc; musl/Alpine is untested [I].
- The cgo build cannot cross-compile with Apple clang: `runtime/cgo` fails on missing Linux headers [M].
- With `zig cc` (zig 0.17.1 via mise), linux arm64 and amd64 built in 14–18 s and ran in Docker, and windows/amd64 built [M].
- darwin/amd64 builds natively with clang in 3 s [M].
- For GoReleaser with cgo, the alternatives are goreleaser-cross images or native runners per OS [I].

GoReleaser and the Homebrew tap (sketch in `.goreleaser.yaml`):
- Builds use `CGO_ENABLED=0` and `-tags purego` for darwin/linux × amd64/arm64.
- Archives contain only `speak`, the licence and the readme.
- A `brews:` formula declares `onnxruntime` and `espeak-ng` as dependencies, both from homebrew-core.
- `goreleaser check` (2.20.2 via mise) parsed the file and failed only because the spike repo has no commits or remote [M].
- Catch: on Intel macOS, depending on `onnxruntime` means building it from source. Either drop Intel macOS or vendor ORT 1.23 in the tap [I].
- If exact parity matters, `speak-espeak` would be a separate GPL formula that depends on `espeak-ng` [I].

## Owner requirements

### 1. Pluggable models

The prototype uses this interface (`internal/tts/tts.go`):
```go
type Backend interface {
	Info() Info                 // name, model, SampleRate, licence, runtime
	Voices() []Voice            // ID, Language, Gender, Label
	Synthesize(ctx context.Context, req Request) iter.Seq2[Chunk, error] // PCM chunks in order
	Close() error
}
func Register(name string, f Factory) // backends self-register; default comes from config/flag
```

The CLI only talks to this interface. Lexicon, sentence streaming, audio, config and exit codes don't depend on the model, and Kokoro is just one registered package.

A second ONNX backend reuses `internal/ort`, the `.npz` reader, the `G2P` interface, `trim`, and audio output at its own sample rate. Estimates:

| Backend | What it needs | Estimate [I] |
|---|---|---|
| Kitten TTS | Same inputs as Kokoro (ids, style, speed) and a voices `.npz` | about 100–150 LOC |
| Piper | JSON config with `phoneme_id_map`, pad/bos/eos interleaving, 22.05 kHz, inputs `input`/`input_lengths`/`scales`, its own espeak handling | about 200 LOC |
| Local HTTP server | net/http and streaming responses; idiomatic Go | small |
| MLX or ggml (llama.cpp-style) | In-process would need cgo, so a sidecar process is the realistic route | not estimated |

### 2. Agents

| Exit code | Meaning |
|---|---|
| 0 | ok |
| 1 | synthesis or audio failure |
| 2 | usage error, unknown voice, bad flag |
| 3 | model, runtime library or espeak-ng missing |
| 130 | interrupted (SIGINT) |

All codes were verified [M].
- A successful `-o` run writes nothing to stdout or stderr.
- `-v '?'` prints one voice per line.
- No TTY is assumed, and stdin is read only when piped.
- The subprocess's stderr is discarded, because Debian's espeak-ng prints ALSA errors even with `-q` [M].
- A daemon on a unix socket is straightforward in Go [I].

### 3. Voices picker

- Built with `charm.land/bubbletea/v2` 2.1.2 (MIT, pure Go).
- Keys: ↑/↓ or j/k to move, space to synthesise and play a sample, enter to save `voice = "…"` to `$SPEAK_CONFIG` (default `~/.config/speak/config.toml`), q to quit.
- Driven through a pseudo-terminal with `script(1)` [M]: `jj` then enter saved `af_kore`, and space played a sample.
- bubbletea supports macOS and Linux terminals and the Windows console [I]; the Windows binary compiles with it [M].
- Without a TTY, `speak voices` prints the plain list instead.

### 4. G2P per the licensing plan

- Go has no misaki port. Porting misaki's English rules (about 740 lines of Python) plus embedding its JSON lexicons is roughly 1,000 Go LOC [I].
- Watch point: misaki is regex-heavy and RE2 has no look-around. That came up twice here (lexicon boundaries, sentence split); both were easy to hand-code.

## Risks and open questions

**ORT is the main risk:**
- There are no upstream GitHub releases right now.
- There are no Intel macOS builds after 1.23.
- Distro versions lag (Debian ships 1.21).
- Each Go binding has a single main maintainer.

An own purego binding (low API version, Windows-capable) is the de-risking step [I].

**Other risks:**
- The espeak-ng CLI's stress promotion changed about 0.4% of words in the prose sample. Whether that is audible needs a listening test; otherwise choose between the GPL helper and misaki-first G2P.
- Memory is about 650 MB RSS with the fp32 model. The int8 model (92 MB) should cut this a lot; untested [I].
- purego relies on `unsafe` pointer handling (`go vet` warns about it) and on purego's internal fake-cgo shim.
- espeak-ng is not reentrant; the spike serialises calls with a mutex.
- Intel macOS and Windows were never actually run.

**Open questions:**
- Do we drop Intel macOS?
- Do we vendor ORT in release archives, or depend on Homebrew and distro packages?
- Do we want a daemon to get first audio under 0.25 s?

**Clean-up:**
- `third_party/` (493 MB of downloads), `bin/` and `out/` can be deleted.
- Nothing outside `spikes/go` was changed except the mise install cache (go 1.27.4, zig 0.17.1, goreleaser 2.20.2) and the `debian:trixie-slim` images pulled for linux/arm64 and linux/amd64.
