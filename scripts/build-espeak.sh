#!/bin/sh
# Build espeak-ng 1.52.0 (GPL-3.0-or-later) from its pinned release source into third_party/espeak-ng
# (gitignored). speak only ever runs it as a separate program; nothing here is linked into speak.
#
#   scripts/build-espeak.sh
#   export SPEAK_ESPEAK="$PWD/third_party/espeak-ng/bin/espeak-ng"
#
# Needs cmake and a C compiler. Audio output, sonic, klatt and mbrola are switched off: speak only
# asks espeak-ng for phonemes. The compiled-in data path is third_party/espeak-ng/share/espeak-ng-data.
set -eu
VERSION=1.52.0
URL=https://github.com/espeak-ng/espeak-ng/archive/refs/tags/$VERSION.tar.gz
SHA256=bb4338102ff3b49a81423da8a1a158b420124b055b60fa76cfb4b18677130a23

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX=$ROOT/third_party/espeak-ng
WORK=$ROOT/third_party/build
if [ -x "$PREFIX/bin/espeak-ng" ] && [ -d "$PREFIX/share/espeak-ng-data" ]; then
  echo "espeak-ng already built: $PREFIX/bin/espeak-ng"
  exit 0
fi
mkdir -p "$WORK"
TARBALL=$WORK/espeak-ng-$VERSION.tar.gz
[ -f "$TARBALL" ] || curl -fsSL -o "$TARBALL" "$URL"
if command -v sha256sum >/dev/null; then
  echo "$SHA256  $TARBALL" | sha256sum -c - >/dev/null
else
  echo "$SHA256  $TARBALL" | shasum -a 256 -c - >/dev/null
fi
rm -rf "$WORK/espeak-ng-$VERSION"
tar -xzf "$TARBALL" -C "$WORK"
SRC=$WORK/espeak-ng-$VERSION
LOG=$WORK/build.log
{
  cmake -S "$SRC" -B "$SRC/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DBUILD_SHARED_LIBS=OFF -DBUILD_TESTING=OFF -DESPEAK_BUILD_MANPAGES=OFF \
    -DUSE_ASYNC=OFF -DUSE_KLATT=OFF -DUSE_LIBPCAUDIO=OFF -DUSE_LIBSONIC=OFF -DUSE_MBROLA=OFF \
    -DUSE_SPEECHPLAYER=OFF &&
  cmake --build "$SRC/build" -j "$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)" &&
  cmake --install "$SRC/build"
} >"$LOG" 2>&1 || { tail -n 30 "$LOG"; echo "espeak-ng build failed (full log: $LOG)" >&2; exit 1; }
cp "$SRC/COPYING" "$PREFIX/COPYING"
echo "espeak-ng $VERSION: $PREFIX/bin/espeak-ng"
