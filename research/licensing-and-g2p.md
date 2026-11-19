# Licensing, G2P and model options for `speak`

Researched 2026-11-19. This is not legal advice. **[S]** marks a fact taken from a linked primary source (or from code I read). **[I]** marks my inference or judgement.

## Recommendation

**1. Licence `speak` under Apache-2.0.** Keep espeak-ng a separate program: run the `espeak-ng` executable as a subprocess. Do not link it statically and do not `dlopen` libespeak-ng.
- With espeak-ng out of process, nothing constrains `speak`'s licence, release archives contain only our code, and every channel can depend on the distro's espeak-ng. [I, from §1]
- Apache-2.0 matches Kokoro, misaki and sherpa-onnx. The FSF recommends it over other permissive licences for substantial programs, and it is GPLv3-compatible, so a GPL build stays possible later. [S]
- If the Rust prototype wins, `MIT OR Apache-2.0` follows crate convention and works equally well. [I]
- Choose **GPL-3.0-or-later** only if you want espeak-ng inside the binary, either statically linked or loaded in-process. Then license all of `speak` that way. Every other dependency (Apache-2.0, MIT, CC-BY model data) is compatible with it. [S/I]
- The ecosystem is moving away from in-process espeak-ng:
  - sherpa-onnx removed it in October 2026 to stay Apache-2.0.
  - Piper went the other way and became GPL.
  - kokoro-onnx, Kokoros and kokoro-js ship it in-process under permissive labels, which is the problem we want to avoid. [S, §1.4]

**2. Where espeak-ng comes from.** It is needed only for English words missing from the lexicon and for Kokoro's es/fr/hi/it/pt-br voices.

| Channel | How |
|---|---|
| Homebrew tap (macOS + Linux) | `depends_on "espeak-ng"`. The homebrew-core formula is at 1.52.0, GPL-3.0-or-later, with bottles for Apple Silicon, Intel macOS (Ventura and Sonoma only) and Linux arm64/x86_64 [S] |
| homebrew-core (later) | Same dependency, plus `depends_on "onnxruntime"` (in core, MIT) rather than vendoring ONNX Runtime. Core says "a separate project should not be bundled when a maintained Homebrew formula can provide the same dependency" [S]. Once English needs no espeak-ng, drop the hard dependency so users who set `HOMEBREW_FORBIDDEN_LICENSES` to exclude GPL can still install [S/I] |
| GitHub release archives | Ship `speak` only and tell users to install espeak-ng [I]. If you ever bundle it (for example in a Windows zip), see the aggregate rules in §1.3 |
| `.deb` / `.rpm` (nfpm / GoReleaser) | `Depends:` or `Recommends: espeak-ng`. It is packaged in Debian (1.52.0+dfsg-5), Fedora (1.52.0) and Arch `extra` (1.52.0) [S] |
| AUR | `depends=(espeak-ng onnxruntime-cpu)`. Both are in Arch `extra` [S] |
| Windows | winget `eSpeak-NG.eSpeak-NG` (1.52.0), Scoop `main/espeak-ng`, or the upstream `espeak-ng.msi` [S]. `speak` finds `espeak-ng.exe` on `PATH` or through an env var [I] |

**3. G2P plan.** G2P (grapheme-to-phoneme) turns spelling into the phonemes Kokoro takes as input.

v1, English, using a misaki-compatible pipeline:
1. User lexicon.
2. Built-in lexicon: misaki's us/gb gold and silver dictionaries.
3. Numbers and currency expanded by our own code. Do not port num2words, which is LGPL.
4. Words still unknown go to the `espeak-ng` subprocess, and its IPA is mapped to Kokoro's phoneme set with misaki's `from_espeak` table.
5. If espeak-ng is missing, warn and spell the word out.

Prototype notes:
- **Rust:** use `misaki-rs` with `default-features = false` (no espeak) and plug in our own `Fallback`.
- **Go:** no misaki port exists. Embed the JSON lexicons and port misaki's English rules, about 740 lines of Python. [S/I]

v1, other Kokoro languages: es, fr, hi, it and pt-br go through the espeak-ng subprocess, which is required for them. Japanese and Chinese are out of scope for v1.

v1.x: replace the English fallback with misaki's 3 MB BART G2P model (Apache-2.0), exported to ONNX and run on the ONNX Runtime we already ship. English then needs no GPL code at all, and espeak-ng becomes "optional, for more languages". Kitten TTS 2 already ships this chain: misaki lexicon, then CMUdict, then a small BART G2P (§5). [S/I]

Later:
- espeak-free G2P for other languages. MisakiSharp's espeak "replay" matches espeak 83–96% for es/fr/it/pt [S].
- Or keep espeak-ng for those languages.

Known risk: nobody has documented where misaki's dictionaries come from, and they may include Wiktionary (CC BY-SA) entries (§2.1). The fallback plan is CMUdict (BSD-style) through an ARPAbet mapping. [S/I]

**4. Models.**
- Download on first run from Hugging Face `onnx-community/Kokoro-82M-v1.0-ONNX`, pinned to a commit, and check the SHA-256.
- Default to `model_quantized.onnx` (8-bit, 92 MB) if it passes a blind A/B against fp32 (326 MB). [I]
- Fetch each voice (`voices/<name>.bin`, 512 KB) the first time it is used.
- Credit Kokoro-82M (Apache-2.0) with a link to the model card. If we ever mirror the weights, include the Apache-2.0 text. [S, §3]

**5. Lexicon.**
- Keep `word = spoken form` and add `word = /phonemes/` (misaki's slash convention), written in Kokoro's phoneme alphabet.
- Accept standard IPA as input, normalise it to that alphabet, and reject symbols the model doesn't know.
- Apply entries per word inside our G2P pipeline, not as text substitution.
- For exchange with other tools, use W3C PLS import/export rather than espeak's `_extra` lists (§4).

**6. Other models (§5).**
- Keep Kokoro as the v1 default.
- Phonon 31M (Apache-2.0, 64.5 MB, no G2P) is the best next backend, and the only candidate to replace Kokoro as the English default, after listening tests.
- Kitten TTS 2 nano/mini (Apache-2.0, uses the same ONNX inputs as Kokoro) suits a tiny or fast mode.

---

## 1. Licence options given the dependencies

### 1.1 Dependency licences

| Component | Licence | Source |
|---|---|---|
| Kokoro-82M weights and voices | Apache-2.0 (model card YAML `license: apache-2.0`) [S] | [hexgrad/Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) |
| ONNX conversion | Apache-2.0 [S] | [onnx-community/Kokoro-82M-v1.0-ONNX](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX) |
| kokoro-onnx GitHub release assets | The repo is MIT. Its README says "kokoro-onnx: MIT, kokoro model: Apache 2.0" [S] | [thewh1teagle/kokoro-onnx](https://github.com/thewh1teagle/kokoro-onnx) |
| ONNX Runtime | MIT [S] (PyPI metadata in the prototype venv; Homebrew, Arch and Debian packages) | [formula](https://formulae.brew.sh/formula/onnxruntime) |
| espeak-ng | GPL-3.0-or-later; `getopt.c` is BSD-2, Windows only [S] | [README, "License Information"](https://github.com/espeak-ng/espeak-ng#license-information) |
| misaki (code) | Apache-2.0 [S] | [hexgrad/misaki](https://github.com/hexgrad/misaki) |
| phonemizer / phonemizer-fork | GPL-3.0 [S] | [bootphon/phonemizer](https://github.com/bootphon/phonemizer) |

Notes:
- **The current Python prototype is already a GPL combination.** `kokoro_onnx/tokenizer.py` loads the bundled `libespeak-ng.dylib` from `espeakng_loader` with `ctypes` and calls it through `phonemizer-fork`, which is GPL-3.0 itself. That is fine for local use, but it is not something to ship under a permissive licence. [S, code read]
- **Kokoro's training data.** The model card says it was trained on "permissive/non-copyrighted audio", including "synthetic audio generated by closed TTS models from large providers". It also lists CC BY 3.0/4.0 datasets (Koniwa, SIWIS) under a "Creative Commons Attribution" heading. [S] Whether that synthetic audio respected the providers' terms is hexgrad's risk, not a licence term on us. Linking to the model card covers the CC BY attribution hexgrad chose to give. [I]

### 1.2 What the FSF says (GPL FAQ, [gnu.org/licenses/gpl-faq](https://www.gnu.org/licenses/gpl-faq.html))

- [#MereAggregation](https://www.gnu.org/licenses/gpl-faq.html#MereAggregation):
  - "pipes, sockets and command-line arguments are communication mechanisms normally used between two separate programs."
  - "If modules are designed to run linked together in a shared address space, that almost surely means combining them into one program."
  - Caveat: "if the semantics of the communication are intimate enough, exchanging complex internal data structures, that too could be a basis to consider the two parts as combined."
- [#GPLStaticVsDynamic](https://www.gnu.org/licenses/gpl-faq.html#GPLStaticVsDynamic): "Linking a GPL covered work statically or dynamically with other modules is making a combined work based on the GPL covered work."
- [#IfLibraryIsGPL](https://www.gnu.org/licenses/gpl-faq.html#IfLibraryIsGPL): any software that uses a GPL library must be under a GPL-compatible licence, and "the work as a whole must be licensed under the GPL."
- [#GPLPlugins](https://www.gnu.org/licenses/gpl-faq.html#GPLPlugins): "If the main program dynamically links plug-ins, and they make function calls to each other and share data structures, we believe they form a single combined program."
- [#NFUseGPLPlugins](https://www.gnu.org/licenses/gpl-faq.html#NFUseGPLPlugins): "the terms of the GPL must be followed when the main program is distributed for use with these plug-ins". If they are separate works, "the license of the plug-in makes no requirements about the main program."
- [#GPLModuleLicense](https://www.gnu.org/licenses/gpl-faq.html#GPLModuleLicense): "you can give additional permission for the use of your code … under a license which is more lax than the GPL but compatible with the GPL."
- [Licence list, Apache-2.0](https://www.gnu.org/licenses/license-list.html#apache2): "compatible with version 3 of the GNU GPL" but "not compatible with GPL version 2". espeak-ng is GPL-3.0-or-later, so this is fine.
- [GPLv3 §5](https://www.gnu.org/licenses/gpl-3.0.txt): a covered work combined with "separate and independent works … not combined with it such as to form a larger program" is an **aggregate**, and that "does not cause this License to apply to the other parts."
- [GPLv3 §6(d)](https://www.gnu.org/licenses/gpl-3.0.txt): source may sit on a different server, but "you remain obligated to ensure that it is available for as long as needed". The FAQ adds that the sources "must correspond exactly to the binaries" ([#AnonFTPAndSendSources](https://www.gnu.org/licenses/gpl-faq.html#AnonFTPAndSendSources)).

### 1.3 The three integration options compared

| | (a) run the `espeak-ng` executable | (b) `dlopen` libespeak-ng at runtime | (c) link espeak-ng statically |
|---|---|---|---|
| **`speak`'s own licence** | Unconstrained; separate programs [S #MereAggregation]. Text in and a phoneme string out is arm's-length communication [I] | Must be GPL-compatible (Apache-2.0 and MIT both qualify), and distributing `speak` "for use with" the library brings GPL terms for the combination [S #NFUseGPLPlugins]. The source can stay Apache-2.0, but a shipped build that loads espeak-ng is effectively GPL [I] | The binary is a combined work under GPL-3.0-or-later [S #GPLStaticVsDynamic]. The source files may stay permissive [S #GPLModuleLicense]; simplest is to license all of `speak` GPL [I] |
| **Release archives that include espeak-ng** | Allowed as an aggregate. Include espeak-ng's `COPYING` and attach the exact espeak-ng source tarball to the same release (§6(d)) [S]. Better not to bundle on macOS or Linux [I] | The whole archive is a GPL combination: GPL text, plus source for `speak` and espeak-ng [S/I] | One GPL binary, with espeak-ng-data embedded (19 MB unpacked in the prototype venv) [S, measured] |
| **Homebrew tap** | `depends_on "espeak-ng"`; formula `license "Apache-2.0"` [I] | Works with brew's `libespeak-ng`; the bottle is a combined work [I] | Formula `license "GPL-3.0-or-later"` [I] |
| **homebrew-core** | Fine. espeak-ng and onnxruntime are both in core [S]. Gate: 75 stars, 30 forks or 30 watchers, or **225 stars, 90 forks or 90 watchers if self-submitted**; repos under 30 days old are ineligible; tagged stable releases required [S] | Same as (a). Core does not police how dependencies are linked [I] | Likely rejected if espeak-ng is vendored: core says "a separate project should not be bundled when a maintained Homebrew formula can provide the same dependency" [S] |
| **Debian / Fedora / AUR** | espeak-ng is packaged in all three [S]. Our own `.deb`/`.rpm`/AUR packages only need a dependency line. Official Debian main is unlikely while a model has to be downloaded; see below [S/I] | Routine (many permissive programs link GPL libraries) [I] | Distro guidelines discourage bundled copies of libraries; expect to be asked to link the system espeak-ng [I] |
| **Windows** | winget, Scoop or MSI install espeak-ng; `speak` locates the `.exe` [S/I] | Must find `espeak-ng.dll`; same GPL terms as archives [I] | One GPL `.exe` [I] |
| **Engineering** | A process spawn per utterance (or a long-lived pipe). We split at punctuation ourselves, as phonemizer does internally. espeak-ng's data path is the system default [I] | Go needs cgo or purego, Rust needs `libloading`. ABI and data-path drift. Fast in-process API (`espeak_TextToPhonemes`, what phonemizer uses) [I] | cgo cross-builds for every target. In Rust, `espeak-rs-sys` (MIT-labelled) builds espeak-ng from source, and the result is still GPL [S/I] |

**Debian archive policy** ([ch-archive](https://www.debian.org/doc/debian-policy/ch-archive.html)):
- Packages in main "must not require or recommend a package outside of main for compilation or execution". contrib is for packages that "require software outside of the distribution to either build or function". [S]
- A `speak` that downloads its model at first run therefore probably belongs in contrib. [I]
- The 2025 General Resolution on whether AI models meet the DFSG [was withdrawn](https://www.debian.org/vote/2025/vote_002) [S].
- The deep-learning team's draft [ML-Policy](https://salsa.debian.org/deeplearning-team/ml-policy/-/raw/master/ML-Policy.rst) is "UNOFFICIAL" [S]. Under it Kokoro, with no released training data, would be a "ToxicCandy" or "NonFree" model, which means non-free if it were ever packaged. [I]

**Fedora:** Fedora Legal has said weights under a licence acceptable for content can be packaged, deciding case by case ([2024 thread](https://lists.fedorahosted.org/archives/list/legal@lists.fedoraproject.org/2024/4/)) [S]. Not needed if the model is downloaded at runtime. [I]

### 1.4 How others handled espeak-ng

| Project | Licence | espeak-ng integration | Source |
|---|---|---|---|
| sherpa-onnx 2.0 | Apache-2.0 | **Removed it** to stay Apache-2.0. Replaced it with a built-in `lexicon.txt` front end plus "phonemize outside sherpa-onnx". Its Kokoro English lexicon is misaki's gold and silver dictionaries, flattened | [Breaking Change: Remove espeak-ng and piper-phonemize dependency](https://github.com/k2-fsa/sherpa-onnx/issues/3731), [Remove espeak-ng and piper-phonemize from sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/pull/4055) (merged 2026-10-20), [generate_lexicon_en.py](https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/kokoro/v1.0/generate_lexicon_en.py) [S] |
| Piper | MIT → GPL-3.0 | Development moved to `OHF-Voice/piper1-gpl`, which "embeds espeak-ng" | [piper1-gpl](https://github.com/OHF-Voice/piper1-gpl) [S]; AUR `piper-tts` is GPL-3.0-or-later [S] |
| kokoro-onnx (our prototype) | MIT | Loads the bundled libespeak-ng in-process through `espeakng_loader` and phonemizer (GPL). README: "It's recommend to use misaki g2p package from v1.0" | [README](https://github.com/thewh1teagle/kokoro-onnx) [S] |
| Kokoros (Rust) | Apache-2.0 | Depends on `espeak-rs` (MIT bindings that build espeak-ng) | [kokoros/Cargo.toml](https://github.com/lucasjinreal/Kokoros) [S]; the binary's GPL obligations are not addressed [I] |
| KokoroSharp (C#) | MIT | Moved to **MisakiSharp** (Apache-2.0), a native port of misaki whose `EspeakReplay` reproduces espeak output without espeak: English unknown words 88.1%, pt 96.2%, es 95.5%, it 85.8%, fr 83.6%, hi 53.6% | [MisakiSharp](https://github.com/Lyrcaxis/MisakiSharp) [S] |
| kokoro-js | Apache-2.0 | npm `phonemizer` (labelled Apache-2.0, "using eSpeak NG", built as WASM) | npm metadata [S]; the label looks inconsistent with espeak-ng's GPL [I] |
| go-kokoro-tts (Go) | MIT | Runs the `espeak-ng` executable from `PATH`, which is option (a) | [README](https://github.com/chasedputnam/go-kokoro-tts) [S] |

---

## 2. G2P options and what Kokoro loses without espeak-ng

### 2.1 Options

| Option | Licence | Size | English quality / notes |
|---|---|---|---|
| **misaki lexicons** (`us_gold`, `us_silver`, `gb_*`) | Code Apache-2.0. **Where the data came from is undocumented**: see the open issue [Source and license of the English dictionaries (us/gb gold and silver)](https://github.com/hexgrad/misaki/issues/113) (filed 2026-11-10, no answer yet) [S] | 3.0 + 3.1 MB JSON for US; 90,201 gold + 93,361 silver entries; some are POS-keyed, e.g. `read` has `DEFAULT`/`VBD` [S] | The G2P "designed for Kokoro" [S]. My check: only 41% of gold entries appear in CMUdict, and entries like `'ndrangheta` look Wiktionary-derived, so CC BY-SA material is possible [I] |
| **misaki neural fallback** ([PeterReid/graphemes_to_phonemes_en_us](https://huggingface.co/PeterReid/graphemes_to_phonemes_en_us), plus `_gb`) | Apache-2.0 [S] | 3.0 MB safetensors (BART) [S] | misaki's default fallback when none is passed [S, `en.py`]. No published accuracy [S]. Needs an ONNX export for Go/Rust [I] |
| **CMUdict** | BSD-2-clause style ("Copyright (C) 1993-2015 Carnegie Mellon University") [S] | 126k words, ARPAbet [S] | Needs an ARPAbet-to-misaki mapping (stress digits sit on vowels rather than before syllables; no ᵊ or ɾ) and has no POS variants [I] |
| DeepPhonemizer | MIT code. Pretrained models: `en_us_cmudict_ipa_forward` trained on cmudict-ipa; `latin_ipa_forward` trained on WikiPron (Wiktionary, CC BY-SA) [S] | PyTorch checkpoints | README says only "comparable to state-of-art". Inactive since 2023 [S] |
| OpenPhonemizer | BSD-3-Clause-Clear code and weights. Trained on a CC-BY-SA dataset "generated using phonemizer/espeak". **Archived** [S] | 73.5–175 MB | Imitates espeak. Whether a model trained on GPL program output is affected is unsettled; the FSF says output is generally not covered ([#WhatCaseIsOutputGPL](https://www.gnu.org/licenses/gpl-faq.html#WhatCaseIsOutputGPL)) [S/I] |
| CharsiuG2P (ByT5) | MIT code; **no licence on the weights** on Hugging Face [S] | 29 MB (tiny, 8 layers) [S] | Average over 100 languages: PER 0.107, WER 0.314 [S]. Too weak as a primary English G2P [I] |
| gruut / [voicegarden-lexicons](https://github.com/AACTools/voicegarden-lexicons) | gruut MIT (archived). voicegarden bundles are MIT/BSD, except its Wiktionary scrapes (CC BY-SA) [S] | per language | Uses gruut/Piper symbol conventions, **not** espeak/misaki, so mapping would be needed for Kokoro [S/I] |
| espeak-ng | GPL-3.0-or-later | 0.5 MB library + 19 MB data [S] | Best coverage, 100+ languages; Kokoro's non-English voices were trained on its phonemes [S] |

**Rust:** [`misaki-rs`](https://github.com/MicheleYin/misaki-rs) (MIT).
- Claims feature parity with misaki 0.9.4 for English. It embeds the lexicons and a POS tagger, and has a `Fallback` trait.
- The `espeak` feature is on by default; disable it with `default-features = false`. Unknown words then become `❓`. [S]
- Small project: 10 stars. [S]

**Go:** no G2P library or misaki port found. Ports exist in Swift (MisakiSwift), C# (MisakiSharp) and Dart (misakid), all Apache-2.0, and can serve as references. [S, GitHub search]

### 2.2 What Kokoro loses without espeak-ng [S unless marked]

- **English words in the lexicon: nothing.** The official `KPipeline` uses misaki's lexicon first and espeak only as fallback ([pipeline.py](https://github.com/hexgrad/kokoro/blob/main/kokoro/pipeline.py)).
  - Our prototype phonemizes *all* English with espeak and passes raw espeak IPA through without misaki's `from_espeak` mapping (`kokoro_onnx/tokenizer.py`).
  - So moving to a misaki-style lexicon should match the phoneme conventions of Kokoro v1.0 better than today, not worse. [I]
- **English words not in the lexicon:** with no fallback, `KPipeline` logs "OOD words will be skipped" (`unk=''`): the word is silently dropped.
  - Developer vocabulary is where this hurts. My check: 19 of 27 sampled terms are missing from gold and silver (`github kubernetes nginx homebrew json yaml api cli npm postgres macos iphone golang onnx kokoro wifi url http localhost`). [S, own check]
  - misaki spells all-caps proper nouns letter by letter (`get_NNP`), which covers some acronyms. [S]
  - A fallback plus the user lexicon is essential for a CLI. [I]
- **Numbers, currency, ordinals and years** are handled by misaki itself through `num2words` (LGPL-2.1), not espeak. A port has to rewrite this. [S/I]
- **es, fr, hi, it, pt-br:** espeak-ng only, in the official pipeline and in VOICES.md. These voices were trained on espeak phonemes, so any replacement has to imitate espeak's conventions (MisakiSharp's approach). [S/I]
- **ja, zh** need their own G2P (pyopenjtalk + unidic; jieba + pypinyin), not espeak. Not planned for v1. [S/I]

---

## 3. Model distribution

| Source | Files and sizes | Notes |
|---|---|---|
| **HF [onnx-community/Kokoro-82M-v1.0-ONNX](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX)** (recommended) | `onnx/model.onnx` 325.5 MB (fp32) · `model_fp16` 163.2 · `model_quantized` 92.4 (8-bit) · `model_q8f16` 86.0 · `model_uint8` 177.5 · `model_uint8f16` 114.2 · `model_q4` 305.2 · `model_q4f16` 154.6. Plus `voices/*.bin`: 55 files of 524,288 bytes each (float32, reshape to `[-1,1,256]`) [S] | Apache-2.0. Last commit `1939ad2a8e416c0acfeecc08a694d14ef25f2231` (2025-02-08) [S] |
| GitHub [kokoro-onnx releases](https://github.com/thewh1teagle/kokoro-onnx/releases) `model-files-v1.0` | `kokoro-v1.0.onnx` 325 MB · `.int8` 92 MB · `.fp16` 177 MB · `voices-v1.0.bin` 28 MB (a single NumPy archive, which the prototype reads with `np.load`) [S] | What the prototype uses. `model-files-v1.1` adds `kokoro-v1.1-zh` [S] |
| HF [hexgrad/Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) | `kokoro-v1_0.pth` 327 MB (PyTorch) and `voices/*.pt` [S] | The upstream original; no ONNX |

Recommendations:
- **Pin downloads.** Use `https://huggingface.co/<repo>/resolve/<commit>/<path>` URLs and check SHA-256. The Hugging Face tree API returns each file's LFS `oid`, which is its SHA-256 (e.g. `model_quantized.onnx` is `fbae9257…`). Read the hashes in the release script rather than hand-copying them. [S/I]
- **Per-voice files** from onnx-community are simpler to parse in Go/Rust than kokoro-onnx's NumPy archive, and can be fetched lazily. [I]
- **Quantisation trade-offs:**
  - The card claims the model "is resilient to quantization" but gives only audio samples, no metrics. [S]
  - q4 saves nothing (305 MB). fp16 halves the download, but [I] the ONNX Runtime CPU backend runs many fp16 ops through casts, so it may not be faster.
  - Default to `model_quantized` (92 MB) if a blind A/B against fp32 passes; otherwise ship fp32. Benchmark latency in the prototypes. [I]
  - Distilled 7M Kokoro variants exist (about 30 MB fp32, 8.7 MB int8, en-US; [sherpa-onnx notes](https://github.com/k2-fsa/sherpa-onnx/tree/master/scripts/kokoro/7m-distill)). Their licence was not checked. [S/I]
- **ONNX Runtime:** on 2026-11-19, `github.com/microsoft/onnxruntime` (including its release downloads) returned HTTP 404, even though the Homebrew formula still points there. [S]
  - Get ONNX Runtime from package managers (brew `onnxruntime` 1.32.0, Arch `onnxruntime-cpu`, Debian `onnxruntime`) or from PyPI wheels. Don't hard-code GitHub release URLs until this is clarified. [I]
  - When bundling ONNX Runtime, include its MIT licence and third-party notices. [I]
- **Attribution:**
  - Apache-2.0 §4 applies when we *redistribute*: give recipients "a copy of this License", mark modified files, and keep NOTICE contents. The Kokoro repos have no NOTICE file. ([licence text](https://www.apache.org/licenses/LICENSE-2.0.txt), §4(a)–(d)) [S]
  - Downloading straight from Hugging Face is not redistribution by `speak`. [I] Still, credit Kokoro-82M (hexgrad, Apache-2.0, link to the card) and the other components in the README and in `speak --version` or a `speak credits` command. [I]
  - Mirroring or re-quantising the weights into our own releases triggers §4(a)–(b). [S]

---

## 4. Lexicon design

| System | User pronunciation mechanism |
|---|---|
| espeak-ng | `<lang>_extra` word lists are compiled into `<lang>_dict` with `espeak-ng --compile=<lang>` ([dictionary.md](https://github.com/espeak-ng/espeak-ng/blob/master/docs/dictionary.md)). Inline `[[phonemes]]` when phoneme input is enabled (`option_phoneme_input`, "allow [[phonemes]] in input", in [translate.c](https://github.com/espeak-ng/espeak-ng/blob/master/src/libespeak-ng/translate.c)). Both use espeak's ASCII phoneme mnemonics, not IPA. The SSML docs list `<sub alias>` but not `<phoneme>` [S] |
| misaki / Kokoro | Inline `[Kokoro](/kˈOkəɹO/)` in the input text. Lexicon values are phoneme strings, optionally keyed by POS (`{"DEFAULT": …, "VBD": …}`). Kokoro also accepts a finished phoneme string (kokoro-onnx `create(..., is_phonemes=True)`) [S] |
| sherpa-onnx | `lexicon.txt`: `word p1 p2 …` or `word1 word2 || p1 p2 …` [S] |
| W3C PLS 1.0 | XML `<lexeme><grapheme>` holding `<phoneme>` (with `alphabet="ipa"`) and/or `<alias>` ("orthographic substitutions"). The first `prefer="true"` entry wins, else the first in document order ([spec](https://www.w3.org/TR/pronunciation-lexicon/)). Amazon Polly requires PLS ([docs](https://docs.aws.amazon.com/polly/latest/dg/managing-lexicons.html)); gruut accepts inline PLS in SSML [S] |
| macOS `say` | `[[inpt PHON]] AY1yIY2SAX [[inpt TEXT]]` embedded commands, using Apple's phoneme set ([Speech Synthesis Programming Guide](https://developer.apple.com/library/archive/documentation/UserExperience/Conceptual/SpeechSynthesisProgrammingGuide/FineTuning/FineTuning.html)) [S] |
| CMUdict | `WORD  W ER1 D` (ARPAbet, stress digits on vowels) [S] |

Proposal for `speak` [I]:
- **File format.** Keep the plain-text file and allow both entry kinds:
  ```
  kruapi  = crew A P I            # respelling: fed back through G2P
  kokoro  = /kˈOkəɹO/             # phonemes: bypass G2P (misaki slash convention)
  ```
- **Phoneme alphabet.** Store phonemes in Kokoro/misaki's alphabet: 49 English symbols, with diphthongs as single letters `A I O W Y Q` ([EN_PHONES.md](https://github.com/hexgrad/misaki/blob/main/EN_PHONES.md)).
  - Accept IPA on `speak lexicon add WORD --ipa …` and normalise it: `eɪ→A`, `aɪ→I`, `oʊ→O`, `aʊ→W`, `ɔɪ→Y`, `dʒ→ʤ`, `tʃ→ʧ`, `r→ɹ`, `g→ɡ`.
  - Validate against the model vocabulary (114 symbols in kokoro-onnx's `config.json` [S]) and reject unknown symbols.
- **Where entries apply.** Apply them per word inside the G2P pipeline (user lexicon → built-in lexicon → fallback), not as regex text substitution. Phoneme entries can then be spliced directly into the token stream.
- **Iteration loop.** `speak lexicon try WORD` should print the phonemes it would use, so users can copy and edit them.
- **Other backends.** Phoneme entries only work for backends that take phonemes (§5). Text-only backends use the respelling, so allow an entry to carry both, and warn when a phoneme-only entry can't be honoured.
- **Interchange.** Use PLS (`speak lexicon import/export --pls`) because it already models both phoneme and alias entries. Optionally import CMUdict-style ARPAbet. Do not adopt espeak `_extra` lists: they need compiling into espeak's data directory, use espeak's mnemonics, and work only with espeak.
- **Inline markup.** Optionally accept misaki's `[word](/phonemes/)` inline syntax. Strip `say`'s `[[…]]` embedded commands rather than speaking them.

---

## 5. Other local models worth supporting

Snapshot fetched 2026-11-19. Same tags: **[S]** comes from a model card, licence file, the Hugging Face API or the leaderboard page; **[I]** is inference. Spot-checked against the Hugging Face API: Phonon 31M is apache-2.0 (64.5 MB q8 GGUF), Kitten TTS 2 nano is apache-2.0, Pocket TTS is cc-by-4.0.

"AA Elo" is the [Artificial Analysis TTS arena](https://artificialanalysis.ai/text-to-speech/leaderboard). It is crowd-voted, and each rank is a range. Kokoro-82M v1.0 scores **1060** (rank 62–71) [S]. "—" means the model is not listed.

### Laptop-class candidates (CPU, up to about 1 GB)

| Model (release) | Licence: weights / code | Size | Runtime | Text input | Cloning | Languages | vs Kokoro |
|---|---|---|---|---|---|---|---|
| **Kokoro-82M** (baseline) | Apache-2.0 / Apache-2.0 | 82M params; ONNX int8 92 MB, fp32 326 MB | ONNX (onnx-community), MLX | **phonemes** (misaki, espeak-ng fallback) | no | en + 8 others | 1060 |
| **Phonon 31M en** (Gradium, 2026-10) | **Apache-2.0** / runtime MIT OR Apache-2.0 | 31M params; GGUF q8 **64.5 MB** | Own Rust runtime: crate `ptts`, C header, CLI, `brew install gradium-ai/tap/ptts`, WASM, Core ML. No ONNX | raw text (SentencePiece); the runtime expands numbers and dates | ~100 preset voices (Apache-2.0); cloning only in the hosted studio | **en only** | **1110** (rank 41–48) |
| Phonon 40M en / fr / de / es / pt / multilingual | **CC-BY-NC-4.0** ⚠ | 74.6 MB | same | same | same | 5 languages | vendor test: 2117 vs Kokoro 2043 |
| **Kitten TTS 2 nano / mini / pico** (2026-11-13) | **Apache-2.0** (models, voices, G2P) / Apache-2.0 | pico 4–8 MB per language; nano 8.8–17 MB per language; mini 47–83 MB | ONNX int8/int4, with the same inputs and outputs as Kokoro (`input_ids`, `style[1,256]`, `speed`) | **phonemes**. English: misaki lexicon, then CMUdict, then a 0.75M-parameter BART G2P, with no espeak. Other languages: **espeak-ng** | no | nano/pico 8; mini 16 | — (a vendor table gives "Kitten TTS" 7.4% WER vs Kokoro 3.5%, version not stated) |
| **Pocket TTS** (Kyutai, 2026-01) | **CC-BY-4.0** (attribution required) / MIT | 100M params; English weights 219 MB; experimental 2–18M variants | Official runtime is PyTorch only (~6× realtime on an M4 with 2 cores). Community ports: ONNX, Rust (candle), MLX, sherpa-onnx | raw text | **yes, locally**. The cloning repo needs a Hugging Face click-through; the non-cloning repo does not | en fr de pt it es (+nl) | — |
| Supertonic 3 (Supertone, 2026-05) | **BigScience OpenRAIL-M** ⚠ (its use restrictions must be passed on downstream) / MIT | ~99M params; ~400 MB ONNX | ONNX, with official examples in Rust, Go, C++, Swift and more | raw text | via Voice Builder, which shut down 2026-08-31 | 31 | — |
| **Piper** (OHF-Voice/piper1-gpl) | Engine **GPL-3.0-or-later** ⚠. Voices: the project grants no licence; each voice's dataset licence applies | 63–114 MB per voice | ONNX, libpiper (C/C++) | **phonemes, from embedded espeak-ng** | no | ~35 | — ([I] below Kokoro) |
| Magpie TTS Multilingual 357M (NVIDIA) | **NVIDIA Open Model License** ⚠: commercial use allowed and a Notice file required; the licence ends if you bypass guardrails, and NVIDIA can change the terms | 364M params; GGUF q8 332 MB | NeMo/PyTorch, GGUF, NeMo-Speech.cpp | text, plus **inline IPA** for custom words | no (removed) | 12 | 1063 (≈ Kokoro) |
| Chatterbox Turbo (Resemble, 2025-12) | MIT / MIT | 350M params; official ONNX, q4 set ~0.7 GB | PyTorch; official ONNX repo; MLX (community) | raw text | yes, a reference clip is required. The Python package adds a Perth watermark | en (Chatterbox Multilingual 500M covers 23) | 1020 (original Chatterbox) |
| NeuTTS Air / Nano / 2E (Neuphonic) | Air Apache-2.0. Nano and 2E: "NeuTTS Open License 1.0" in the GitHub README, though the 2E card says apache-2.0 | 552M / 229M / 236M params | PyTorch; GGUF backbone; ONNX codec decoder only | Air and Nano take **phonemes** (front end not named; [I] espeak via phonemizer). 2E takes text | Air and Nano: yes | en; Nano also fr de es | Nano 8.7% WER vs Kokoro 3.5% (Gradium's table) |
| Soprano 1.1 80M | Apache-2.0 | 80M params | transformers / lmdeploy; claims 20× realtime on CPU | raw text, no number expansion | no | en | — |

### Larger, GPU-class or restricted (not candidates for the default)

| Model | Weights licence | Size | Note |
|---|---|---|---|
| Qwen3-TTS 0.6B / 1.7B | Apache-2.0 / Apache-2.0 | 0.9B / 1.9B params | 10 languages, 3-second cloning, voice design. PyTorch, plus GGUF from ggml-org. AA lists "Qwen3 TTS" at 929, below Kokoro ([I] probably the hosted Flash model) |
| Ming-3.0-tts-0.5B (Ant) | MIT | ~1B params, 3 GB | 7 languages, cloning. Tested only on data-centre GPUs; community ONNX and MLX builds exist. AA "Ming-3.0-tts" 1182 (variant not stated) |
| Orpheus 3B / Maya1 3B | Cards say Apache-2.0, but Orpheus is fine-tuned from Llama-3.2-3B-Instruct, so [I] the Llama 3.2 licence likely applies ⚠ | 3.3–3.8B params | GGUF + SNAC decoder; English. Maya1 AA 1047 |
| Sesame CSM-1B | Apache-2.0, but gated, and it also needs Llama-3.2-1B, which is gated too ⚠ | 1.55B params | README requires a CUDA GPU |
| Dia / Dia2 (1B, 2B) | Apache-2.0 / Apache-2.0 | 1.1–1.9B params | English dialogue, up to 2 minutes; CUDA 12.8+ (falls back to CPU) |
| Zonos v0.1 | Apache-2.0 / Apache-2.0 | 1.6B params | **eSpeak phonemes (GPL)**. Needs 6 GB+ VRAM; README says CPU "won't be sufficient for interactive use". AA 1000 |
| VibeVoice 1.5B / Realtime-0.5B | MIT | 2.7B / ~1B params | Microsoft removed the 1.5B TTS code in September 2025 over misuse. The 1.5B scores AA 954 |
| Kyutai TTS 1.6B en/fr | CC-BY-4.0 | 1.6B params | streaming; built for GPUs |
| VoxCPM 1.5 | Apache-2.0 | 0.8B params | en/zh, cloning, PyTorch |
| Kitten TTS 2 (1.7B) | **Stellon Labs Community License** ⚠: commercial users must register, the licence **ends above $1M revenue or funding**, a "Powered by KittenML" credit is required, and distillation is banned | 0.5–3.5 GB | 20+ languages, cloning |
| **Non-commercial or research-only** ⚠ | F5-TTS (CC-BY-NC because of its Emilia training data; code MIT); Spark-TTS (CC-BY-NC-SA); XTTS-v2 (Coqui Public Model License); Fish S2 Pro / Drama 3 (Fish Audio Research License; 4.6B / 36B MoE; AA 1101 / 1279); Voxtral-4B-TTS (CC-BY-NC-4.0; AA 1094); Breeze TTS 2 (research and non-commercial; AA 1229); Llasa-1B (CC-BY-NC); OuteTTS-1.0-1B (CC-BY-NC-SA; the 0.6B is Apache-2.0); Kani-TTS (LFM 1.0); IndexTTS-2 (custom bilibili licence) | | |

### Notes

- **Piper's licence moved [S].** `rhasspy/piper` (MIT) is archived and says "Development has moved" to `OHF-Voice/piper1-gpl`, which is GPL-3.0 and "embeds espeak-ng for phonemization".
- **Each Piper voice needs its own licence check [S].** The project "makes no license grant of its own over voice models".
  - You trace each voice's MODEL_CARD back through every voice it was fine-tuned from.
  - `en_US-lessac-*` comes from the Lessac Blizzard corpus, which is limited to "research and exploration", and "a large share" of voices descend from it.
  - `COMMERCIAL_USE.md` lists voices with a clean history, such as `en_US-joe-medium` and `de_DE-thorsten-medium`, both based on the CC0 rdh voice. Copies downloaded before the late-2026 retrain do not qualify.
  - So Piper brings both a GPL engine and per-voice licence work.
- **What this means for the lexicon [I].** Only backends that take phonemes can honour exact-phoneme lexicon entries: Kokoro, the small Kitten TTS 2 models, Piper, NeuTTS Air/Nano, and Magpie (through inline IPA). Text-input backends (Phonon, Pocket, Supertonic, Chatterbox, Qwen3, Soprano) can only use respellings. This is why §4 keeps a respelling alongside the phonemes.
- **Read the weights licence, not the repo badge [S].** Several Apache or MIT code repos ship non-commercial weights (F5, Spark, Phonon 40M).
- **Non-OSI weights licences [I].** OpenRAIL-M, NVIDIA's licence, Stellon and LFM are all non-OSI, which likely keeps those weights out of Debian main and homebrew-core bottles. Downloading them at first run, as with Kokoro, sidesteps that.

### Recommendation

1. **Next backend: Phonon 31M (English).**
   - Apache-2.0 weights and a permissive Rust runtime with a crate and a C header. Rust can link it directly; Go would use cgo.
   - 64.5 MB, and it needs no G2P, so no GPL question.
   - Streams audio as it generates. Vendor numbers on an M5 with 4 threads: 7 ms to first audio, against 517 ms for Kokoro.
   - Already distributed through a Homebrew tap.
   - Caveats: only the English model is Apache; it is a month old; apart from AA, the benchmarks are the vendor's own; the lexicon can only use respellings; and [I] it uses its own runtime rather than ONNX Runtime.
   - It is built on Kyutai's Pocket TTS and Mimi codec, so credit Kyutai in NOTICE. [I] That is a courtesy; the card does not require it.
2. **Second: Kitten TTS 2 nano/mini.**
   - Apache-2.0 throughout, with the same ONNX inputs as Kokoro and a phoneme front end, so [I] it is a small addition once the Kokoro path exists.
   - At 8.8–83 MB it suits a "tiny/fast" mode or low-memory Linux machines, and English needs no espeak-ng.
   - Its quality against Kokoro is unproven; it is not on the AA arena.
   - If local voice cloning matters more than size, take **Pocket TTS** instead. It is CC-BY-4.0 (attribution required), and its official runtime is PyTorch, so it would go through a community ONNX or Rust port.
3. **Does anything beat Kokoro as the default today?**
   - For English, only Phonon 31M combines permissive weights, laptop size and independent arena evidence: AA 1110 vs 1060, with non-overlapping rank ranges.
   - Everything else ranked above Kokoro is non-commercial (Fish, Voxtral, Breeze) or a multi-GB GPU-class model (Ming-3.0, Step-Audio-EditX, Qwen3-TTS).
   - [I] Keep Kokoro as the v1 default: it is multilingual, and its phoneme input makes exact lexicon entries possible. Ship Phonon as `--model phonon`, and switch the English default only after blind listening tests on the text `speak` typically reads (CLI output, identifiers, numbers).

### Links (§5)

- AA arena: https://artificialanalysis.ai/text-to-speech/leaderboard
- Phonon: https://huggingface.co/gradium/phonon-31m-en · https://gradium.ai/blog/release-phonon-open-weight · https://github.com/gradium-ai/xn-ptts
- Kitten TTS: https://github.com/KittenML/KittenTTS (`docs/kitten-tts-2-tiny.md`) · https://huggingface.co/KittenML/kitten-tts-2-nano · Stellon licence: https://huggingface.co/KittenML/kitten-tts-2/blob/main/LICENSE.md
- Pocket TTS: https://huggingface.co/kyutai/pocket-tts · https://github.com/kyutai-labs/pocket-tts
- Supertonic (archived): https://github.com/supertone-inc/supertonic · https://huggingface.co/Supertone/supertonic-3
- Piper: https://github.com/rhasspy/piper · https://github.com/OHF-Voice/piper1-gpl (`docs/LICENSING.md`, `docs/COMMERCIAL_USE.md`) · https://huggingface.co/rhasspy/piper-voices
- Magpie: https://huggingface.co/nvidia/magpie_tts_multilingual_357m · https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/
- Chatterbox: https://huggingface.co/ResembleAI/chatterbox-turbo · https://huggingface.co/ResembleAI/chatterbox-turbo-ONNX
- NeuTTS: https://github.com/neuphonic/neutts
- Qwen3-TTS: https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base
- Ming: https://huggingface.co/inclusionAI/Ming-3.0-tts-0.5B
- Soprano: https://huggingface.co/ekwek/Soprano-1.1-80M
- Zonos: https://github.com/Zyphra/Zonos
- F5-TTS (licence section of the README): https://github.com/SWivid/F5-TTS
- VibeVoice: https://github.com/microsoft/VibeVoice
- Dia2: https://github.com/nari-labs/dia2
- CSM: https://github.com/SesameAILabs/csm

---

## Sources (§1–4)

- Licences: [FSF GPL FAQ](https://www.gnu.org/licenses/gpl-faq.html) · [GPLv3 text](https://www.gnu.org/licenses/gpl-3.0.txt) · [FSF licence list](https://www.gnu.org/licenses/license-list.html#apache2) · [Apache-2.0 text](https://www.apache.org/licenses/LICENSE-2.0.txt)
- Homebrew: [Acceptable Formulae](https://docs.brew.sh/Acceptable-Formulae) · [Licence Guidelines](https://docs.brew.sh/Licence-Guidelines) · [Package Acceptance Policy](https://docs.brew.sh/Package-Acceptance-Policy) · [espeak-ng formula](https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/e/espeak-ng.rb)
- Distros: [Debian Policy ch. 2](https://www.debian.org/doc/debian-policy/ch-archive.html) · [Debian GR 2025/002 (withdrawn)](https://www.debian.org/vote/2025/vote_002) · [Debian ML-Policy draft](https://salsa.debian.org/deeplearning-team/ml-policy) · [Debian espeak-ng](https://sources.debian.org/src/espeak-ng/) · [Arch espeak-ng](https://archlinux.org/packages/extra/x86_64/espeak-ng/) · [winget manifests](https://github.com/microsoft/winget-pkgs/tree/master/manifests/e/eSpeak-NG/eSpeak-NG) · [Scoop espeak-ng](https://github.com/ScoopInstaller/Main/blob/master/bucket/espeak-ng.json)
- espeak-ng: [repo](https://github.com/espeak-ng/espeak-ng) · [releases (1.52.0, MSI)](https://github.com/espeak-ng/espeak-ng/releases)
- Kokoro and misaki: [kokoro pipeline.py](https://github.com/hexgrad/kokoro/blob/main/kokoro/pipeline.py) · [VOICES.md](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md) · [misaki en.py](https://github.com/hexgrad/misaki/blob/main/misaki/en.py) · [misaki-rs](https://github.com/MicheleYin/misaki-rs) · [MisakiSharp](https://github.com/Lyrcaxis/MisakiSharp)
- G2P: [CMUdict LICENSE](https://github.com/cmusphinx/cmudict/blob/master/LICENSE) · [DeepPhonemizer](https://github.com/as-ideas/DeepPhonemizer) · [OpenPhonemizer](https://github.com/NeuralVox/OpenPhonemizer) · [CharsiuG2P](https://github.com/lingjzhu/CharsiuG2P) · [gruut](https://github.com/rhasspy/gruut)
- Prototype code read: `~/.local/share/speak/speak.py`, plus `kokoro_onnx/{__init__,tokenizer,config}.py` and `espeakng_loader/__init__.py` in its venv.
