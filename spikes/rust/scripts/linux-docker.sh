#!/bin/sh
# Build and test the spike on Linux in a throwaway container (no audio device: WAV output only).
# Usage: scripts/linux-docker.sh [arm64|amd64]
set -eu
ARCH=${1:-arm64}
SPIKE=$(cd "$(dirname "$0")/.." && pwd)
docker run --rm --platform "linux/$ARCH" \
  -v "$SPIKE:/src" \
  -v "$HOME/.local/share/speak/models:/models:ro" \
  -e CARGO_HOME=/src/.docker-cache/cargo-$ARCH -e XDG_CACHE_HOME=/src/.docker-cache/xdg-$ARCH \
  -e CARGO_TARGET_DIR=/src/.docker-cache/target-$ARCH \
  -w /src rust:1.95-trixie sh -c '
set -eu
apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends espeak-ng libasound2-dev pkg-config >/dev/null
echo "== $(uname -m) $(. /etc/os-release; echo $PRETTY_NAME) | $(espeak-ng --version | head -1)"
OUT=/src/out/linux-'"$ARCH"'
mkdir -p $OUT /tmp/home/models
ln -sf /models/kokoro-v1.0.onnx /models/voices-v1.0.bin /tmp/home/models/
t=$(date +%s); cargo build --release --features espeak-inprocess -q 2>&1 | grep -v "^warning" | head -20 || true
cp $CARGO_TARGET_DIR/release/speak $OUT/speak-inproc
cargo build --release -q 2>&1 | grep -v "^warning" | head -20 || true
cp $CARGO_TARGET_DIR/release/speak $OUT/speak
echo "== build $(( $(date +%s) - t ))s; size $(stat -c %s $OUT/speak) bytes (inproc $(stat -c %s $OUT/speak-inproc))"
echo "== ldd"; ldd $OUT/speak
echo "== max GLIBC symbol: $(objdump -T $OUT/speak | grep -o "GLIBC_[0-9.]*" | sort -V | tail -1)"
export SPEAK_HOME=/tmp/home SPEAK_LEXICON=/dev/null
S="Hello Philip. This is the Rust spike."
echo "== synth (system espeak-ng subprocess)"; SPEAK_DEBUG=1 $OUT/speak --timing -o $OUT/hello.wav "$S"
P="Speak is a small command line tool that reads text aloud with a neural voice that runs entirely on this computer. It starts talking after the first sentence is ready, while the rest of the paragraph is still being synthesised in the background."
echo "== 43-word paragraph"; SPEAK_DEBUG=1 $OUT/speak --timing -o $OUT/para.wav "$P"
echo "== play without audio device (expect exit 1)"; $OUT/speak "$S" || echo "exit $?"
echo "== parity: in-process espeak-ng 1.52.0 + macOS espeak-ng-data"
SPEAK_G2P=inprocess SPEAK_ESPEAK_DATA=/src/out/espeak-ng-data REF=/src/parity/reference-macos.jsonl /src/parity/check.sh $OUT/speak-inproc 2>&1 | cut -c1-300
echo "== parity: subprocess, Debian espeak-ng + its own data"
REF=/src/parity/reference-macos.jsonl /src/parity/check.sh $OUT/speak 2>&1 | cut -c1-300
echo "== parity: subprocess, Debian espeak-ng + macOS espeak-ng-data"
SPEAK_ESPEAK_DATA=/src/out/espeak-ng-data REF=/src/parity/reference-macos.jsonl /src/parity/check.sh $OUT/speak 2>&1 | cut -c1-300
'
