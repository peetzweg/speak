#!/bin/sh
# Build speak, run the test suite and one `-o` synthesis on Linux in a throwaway container.
# The repository and the model files are mounted read-only; cargo's caches, the build and the
# output WAV go to .docker-cache/<arch>/ (gitignored). No audio device: file output only.
#
#   scripts/linux-docker.sh [arm64|amd64]     (env: MODELS=dir with kokoro-v1.0.onnx, voices-v1.0.bin)
set -eu
ARCH=${1:-arm64}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
MODELS=${MODELS:-$HOME/.local/share/speak/models}
CACHE=$ROOT/.docker-cache/$ARCH
mkdir -p "$CACHE"
docker run --rm --platform "linux/$ARCH" \
  -v "$ROOT:/src:ro" -v "$MODELS:/models:ro" -v "$CACHE:/cache" \
  -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target \
  -e SPEAK_TEST_MODELS=/models -e SPEAK_ESPEAK=/usr/bin/espeak-ng \
  -w /src rust:1.95-trixie sh -euc '
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends espeak-ng libasound2-dev pkg-config >/dev/null
echo "== $(uname -m), $(. /etc/os-release; echo "$PRETTY_NAME"), $(espeak-ng --version | cut -d" " -f1-4)"
cargo test --locked 2>&1 | grep -E "^(test |test result|error|warning: unused)|skipping|panicked" || true
cargo build --locked --release -q
BIN=/cache/target/release/speak
echo "== binary $(stat -c %s $BIN) bytes; newest glibc symbol $(objdump -T $BIN | grep -o "GLIBC_[0-9.]*" | sort -V | tail -1)"
mkdir -p /tmp/home/models /cache/out
ln -sf /models/kokoro-v1.0.onnx /models/voices-v1.0.bin /tmp/home/models/
# The first run also checksums the model files once; the second shows the warm start.
for run in first warm; do
  echo "== $run run"
  SPEAK_HOME=/tmp/home XDG_CACHE_HOME=/tmp/cache SPEAK_LEXICON=/dev/null \
    $BIN --timing -o /cache/out/hello.wav "Hello from Linux. KruAPI shipped v0.3.5 on 2025-03-03 to 2,000 users."
done
echo "== wrote .docker-cache/'"$ARCH"'/out/hello.wav ($(stat -c %s /cache/out/hello.wav) bytes)"
'
