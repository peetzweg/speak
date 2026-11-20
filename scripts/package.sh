#!/bin/sh
# Package a release build into dist/speak-VERSION-TARGET.tar.gz (plus a .sha256 file):
#
#   bin/speak                        the release binary
#   libexec/speak/espeak-ng          espeak-ng (GPL-3.0-or-later), run as a separate program
#   libexec/speak/espeak-ng-data/    its data
#   LICENSE-MIT LICENSE-APACHE NOTICE README.md
#   licenses/espeak-ng-COPYING
#   licenses/onnxruntime/            ONNX Runtime's licence and third-party notices
#   licenses/THIRD-PARTY.md          every linked crate's licence (cargo-about, see about.toml)
#
# speak finds the bundled espeak-ng through its own path (<exe>/../libexec/speak), so the
# unpacked directory works from anywhere. Run after `cargo build --release --locked` and
# scripts/build-espeak.sh. Needs cargo-about (`cargo install cargo-about --locked --features cli`):
#
#   scripts/package.sh 0.1.0 aarch64-apple-darwin
set -eu
VERSION=$1
TARGET=$2
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ESPEAK=$ROOT/third_party/espeak-ng
NAME=speak-$VERSION-$TARGET
STAGE=$ROOT/dist/$NAME

[ -x "$ROOT/target/release/speak" ] || { echo "no release build: cargo build --release --locked" >&2; exit 1; }
[ -x "$ESPEAK/bin/espeak-ng" ] || { echo "no espeak-ng: scripts/build-espeak.sh" >&2; exit 1; }
command -v cargo-about >/dev/null || { echo "no cargo-about: cargo install cargo-about --locked --features cli" >&2; exit 1; }

rm -rf "$STAGE" "$STAGE.tar.gz" "$STAGE.tar.gz.sha256"
mkdir -p "$STAGE/bin" "$STAGE/libexec/speak" "$STAGE/licenses"
cp "$ROOT/target/release/speak" "$STAGE/bin/"
cp "$ESPEAK/bin/espeak-ng" "$STAGE/libexec/speak/"
cp -R "$ESPEAK/share/espeak-ng-data" "$STAGE/libexec/speak/"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$ROOT/NOTICE" "$ROOT/README.md" "$STAGE/"
cp "$ESPEAK/COPYING" "$STAGE/licenses/espeak-ng-COPYING"
cp -R "$ROOT/licenses/onnxruntime" "$STAGE/licenses/"
(cd "$ROOT" && cargo about generate --locked --fail about.hbs -o "$STAGE/licenses/THIRD-PARTY.md")

tar -C "$ROOT/dist" -czf "$STAGE.tar.gz" "$NAME"
cd "$ROOT/dist"
if command -v sha256sum >/dev/null; then
  sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256"
else
  shasum -a 256 "$NAME.tar.gz" >"$NAME.tar.gz.sha256"
fi
echo "dist/$NAME.tar.gz"
