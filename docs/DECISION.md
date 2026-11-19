# Go or Rust: spike comparison

Both spikes ported kokoro-onnx's pipeline, matched its phonemes and token ids exactly, and ran on macOS arm64 plus Linux arm64/amd64 in Docker. Reports: `spikes/go/REPORT.md`, `spikes/rust/REPORT.md`. Licensing and G2P research: `research/licensing-and-g2p.md`.

## Side by side (Apple M5 Pro, warm cache)

| | Go | Rust |
|---|---|---|
| Exact parity with kokoro-onnx | yes (14/14; audio bit-identical) | yes (21/21; audio within one 16-bit step) |
| First audio | 0.42 s | 0.50 s |
| Real-time factor (inference) | 0.143 | 0.127 |
| Peak memory (fp32 model) | ~680 MB | ~550–660 MB |
| Binary | 4.9 MB, **plus** ONNX Runtime as a shared library (26–45 MB) installed or shipped next to it | 21 MB, ONNX Runtime **linked in** (pyke's static build), nothing else to find |
| ONNX Runtime binding | needs our own ~250-line cgo-free binding (mature one needs cgo; cgo-free one is unstable and not Windows-capable) | `ort` 2.0 (release candidate; API still moving), static libs from pyke's CDN |
| Cross-compiling | trivial with a cgo-free binding (all targets from one Mac) | per-target CI runners via cargo-dist (generated workflow works) |
| Linux floor | glibc (purego), ORT from Homebrew/vendored; Debian's ORT 1.21 too old | glibc ≥ 2.38 with pyke's static ORT (Debian 13, Ubuntu 24.04+) unless we load ORT dynamically |
| Intel Mac | no current ORT for either (last x86_64 build is 1.23) | same |
| GPL-free G2P (misaki) | no port exists: ~1,000 lines to write | `misaki-rs` (MIT) works with espeak off; +~18 MB of embedded dictionaries |
| Future model backends | ONNX via our binding; MLX/ggml only as sidecar processes | ONNX via `ort`; candle, mlx-rs, llama.cpp bindings available in-process behind cargo features |
| Voice picker | bubbletea v2 (pure Go), worked | crossterm, worked |
| Release tooling | GoReleaser + tap (config sketched, `goreleaser check` parses) | cargo-dist + tap (workflow and formula generated; needs `depends_on "espeak-ng"` added by hand) |
| Familiarity | the owner's crawlers (matcrawl) and sag are Go | new stack for the owner's tools |

## Recommendation: Rust

1. **One self-contained binary.** ONNX Runtime is linked in, so users and agents never hit a missing or too-old shared library. Go has to ship or depend on ORT, and Debian's is already too old for both Go bindings.
2. **The GPL-free pronunciation plan is already half-built.** `misaki-rs` exists and handles numbers, money and context-dependent words that espeak mangles; Go would need a ~1,000-line port before it can drop espeak for English.
3. **"Best model as the default" needs the richest local-ML ecosystem.** Rust can run future models in-process (ONNX, candle, MLX, llama.cpp-style); Go would push most of them into sidecar processes.

What Rust costs: a bigger binary (21 MB vs 5 MB + ORT), dependence on pyke's CDN and a pre-1.0 `ort` (pin versions, mirror the archive), a glibc 2.38 floor on Linux unless ORT is loaded dynamically, and slower builds.

## Shared findings, whichever language wins

- **The name `speak` collides with espeak-ng.** espeak-ng 1.52.0's `make install` adds a `bin/speak` symlink, and Homebrew's formula keeps it, so a Homebrew `speak` that also depends on Homebrew's `espeak-ng` cannot link its binary. Options: rename the command, or avoid depending on Homebrew's espeak-ng (bundle our own espeak-ng program under `libexec`, or go misaki-first so espeak-ng is optional).
- **espeak-ng as a separate process** (the Apache-2.0-friendly route) matched the library in Rust's 21-line corpus, but the Go spike found a stress-mark difference in ~0.4% of words (clauses without primary stress, e.g. `what?!`). Misaki-first makes this mostly moot.
- **Intel Macs:** drop for v1.
- **Numbers:** kokoro-onnx's punctuation splitting reads "2,000" as "two, zero zero zero"; both ports copied it for parity. The product should fix it (misaki does).
- **Memory:** ~600 MB with the fp32 model; try the 8-bit model (92 MB) next.
