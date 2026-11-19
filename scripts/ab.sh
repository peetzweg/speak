#!/bin/sh
# Compare kokoro (fp32) with kokoro-int8: render the same three sentences with both into out/ab/
# (named, plus a blind copy whose key is in out/ab/blind/KEY.txt) and measure load time, first
# chunk, real-time factor and peak memory on a 43-word paragraph.
#
#   SPEAK_HOME=<dir whose models/ holds both models> scripts/ab.sh [path/to/speak] [runs]
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=${1:-$ROOT/target/release/speak}
RUNS=${2:-5}
OUT=$ROOT/out/ab
mkdir -p "$OUT/blind"
LEX=$(mktemp)
export SPEAK_LEXICON="$LEX" # empty lexicon: both models get the same input

set -- \
  "Hello Philip. The build finished in 3.5 seconds, and all 42 tests passed." \
  "Kokoro is an open-weight speech model with eighty-two million parameters; despite its size, it sounds natural, even in long and winding sentences like this one." \
  "Did you know that version 0.3.5 shipped on March 3rd, 2025, with 2,000 users on day one? I didn't!"
i=1
for s in "$@"; do
  for m in kokoro kokoro-int8; do "$BIN" -m "$m" -o "$OUT/$i-$m.wav" "$s"; done
  i=$((i + 1))
done
: > "$OUT/blind/KEY.txt"
for i in 1 2 3; do
  # A coin flip per sentence decides which model is A.
  if [ $(( $(od -An -N1 -tu1 /dev/urandom) % 2 )) -eq 0 ]; then a=kokoro b=kokoro-int8; else a=kokoro-int8 b=kokoro; fi
  cp "$OUT/$i-$a.wav" "$OUT/blind/$i-A.wav"
  cp "$OUT/$i-$b.wav" "$OUT/blind/$i-B.wav"
  echo "$i: A=$a B=$b" >> "$OUT/blind/KEY.txt"
done

P="Speak is a small command line tool that reads text aloud with a neural voice that runs entirely on this computer. It starts talking after the first sentence is ready, while the rest of the paragraph is still being synthesised in the background."
case "$(uname)" in Darwin) TIME="/usr/bin/time -l" ;; *) TIME="/usr/bin/time -v" ;; esac
for m in kokoro kokoro-int8; do
  for r in $(seq "$RUNS"); do
    SPEAK_DEBUG=1 $TIME "$BIN" -m "$m" --timing -o "$OUT/paragraph-$m.wav" "$P" 2>"$OUT/run.log" || { cat "$OUT/run.log"; exit 1; }
    load=$(sed -n 's/.*model loaded in \([0-9]*\) ms.*/\1/p' "$OUT/run.log")
    rss=$(awk '/maximum resident set size/ {print int($1 / 1048576)} /Maximum resident set size/ {print int($NF / 1024)}' "$OUT/run.log")
    json=$(grep '^{' "$OUT/run.log")
    echo "$m run $r: load ${load} ms, peak RSS ${rss} MB, $json"
  done
done
rm -f "$LEX" "$OUT/run.log"
echo "samples in $OUT (blind copies in $OUT/blind)"
