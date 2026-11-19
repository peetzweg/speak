"""kokoro-onnx 0.4.7 reference: phonemes, batches and token ids per input line, as JSON lines.

Usage: ~/.local/share/speak/venv/bin/python -I reference.py corpus.txt
       ~/.local/share/speak/venv/bin/python -I reference.py --text "one sentence"
A literal backslash-n in a corpus line becomes a newline. No lexicon is applied (kokoro-onnx has none).
"""
import json
import sys

from kokoro_onnx import Kokoro
from kokoro_onnx.config import EspeakConfig
from kokoro_onnx.tokenizer import Tokenizer

tok = Tokenizer(EspeakConfig(data_path="~/.local/share/speak/es"))


def analyze(text):
    ph = tok.phonemize(text, "en-us")
    batches = Kokoro._split_phonemes(None, ph)
    return {"phonemes": ph, "batches": batches, "tokens": [[0, *tok.tokenize(b), 0] for b in batches]}


if sys.argv[1] == "--text":
    lines = [sys.argv[2]]
else:
    lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
for line in lines:
    print(json.dumps(analyze(line.replace("\\n", "\n")), ensure_ascii=False))
