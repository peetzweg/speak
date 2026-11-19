#!/bin/sh
# Fetch the espeak-ng 1.52.0 release source (GPL-3.0-or-later) that build.rs compiles and links statically.
# Only the library + ucd-tools sources are kept (~2 MB). Same release as the espeakng_loader wheel kokoro-onnx uses.
set -eu
cd "$(dirname "$0")/.."
DEST=third_party/espeak-ng
[ -f "$DEST/src/libespeak-ng/speech.c" ] && exit 0
URL=https://github.com/espeak-ng/espeak-ng/archive/refs/tags/1.52.0.tar.gz
SHA=bb4338102ff3b49a81423da8a1a158b420124b055b60fa76cfb4b18677130a23
TMP=$(mktemp -d)
curl -fsSL -o "$TMP/es.tgz" "$URL"
echo "$SHA  $TMP/es.tgz" | shasum -a 256 -c - >/dev/null
tar -xzf "$TMP/es.tgz" -C "$TMP"
mkdir -p "$DEST/src"
cp -R "$TMP/espeak-ng-1.52.0/src/libespeak-ng" "$TMP/espeak-ng-1.52.0/src/ucd-tools" "$TMP/espeak-ng-1.52.0/src/include" "$DEST/src/"
cp "$TMP/espeak-ng-1.52.0/COPYING" "$DEST/"
rm -rf "$TMP"
echo "espeak-ng 1.52.0 source in $DEST"
