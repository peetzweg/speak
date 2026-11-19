#!/bin/sh
# Compare phonemes/batches/token ids of the Rust binary against kokoro-onnx for every corpus line.
# Usage: parity/check.sh [path/to/speak]   (env: SPEAK_G2P, SPEAK_ESPEAK, SPEAK_ESPEAK_DATA)
set -u
cd "$(dirname "$0")"
BIN=${1:-../out/speak}
# REF=file.jsonl reuses saved kokoro-onnx output (e.g. inside a Linux container without Python).
if [ -n "${REF:-}" ]; then cp "$REF" /tmp/speak-parity-py.jsonl; else
  ~/.local/share/speak/venv/bin/python -I reference.py corpus.txt > /tmp/speak-parity-py.jsonl
fi
: > /tmp/speak-parity-rs.jsonl
while IFS= read -r line; do
  text=$(printf '%s' "$line" | sed 's/\\n/\
/g')
  SPEAK_LEXICON=/dev/null "$BIN" --analyze -v af_heart -- "$text" >> /tmp/speak-parity-rs.jsonl
done < corpus.txt
n=$(wc -l < corpus.txt | tr -d ' ')
same=$(paste -d '\n' /tmp/speak-parity-py.jsonl /tmp/speak-parity-rs.jsonl | awk 'NR%2{a=$0;next} a==$0{c++} END{print c+0}')
echo "identical lines: $same / $n"
paste -d '\n' /tmp/speak-parity-py.jsonl /tmp/speak-parity-rs.jsonl | awk -v n=0 'NR%2{a=$0;next} a!=$0{n++; print "--- line " NR/2 "\npy: " a "\nrs: " $0}'
