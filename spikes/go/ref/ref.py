"""Reference: what kokoro-onnx feeds the model, for parity checks against the Go port.

Prints one JSON line per text: phonemes (Tokenizer.phonemize), the batches from
Kokoro._split_phonemes, and the exact token ids per batch as _create_audio builds them
(0-padded both ends). With DUMP_AUDIO=path, also writes Kokoro.create() audio as raw float32 LE.

Run with: ~/.local/share/speak/venv/bin/python -I ref.py "text" ...
"""
import json, os, sys
import numpy as np
from kokoro_onnx import Kokoro
from kokoro_onnx.config import EspeakConfig, MAX_PHONEME_LENGTH

HOME = os.path.expanduser("~/.local/share/speak")
k = Kokoro(HOME + "/models/kokoro-v1.0.onnx", HOME + "/models/voices-v1.0.bin",
           espeak_config=EspeakConfig(data_path=HOME + "/es"))
for text in sys.argv[1:]:
    ph = k.tokenizer.phonemize(text, os.environ.get("REF_LANG", "en-us"))
    batches = k._split_phonemes(ph)
    ids = [[0, *k.tokenizer.tokenize(b[:MAX_PHONEME_LENGTH]), 0] for b in batches]
    print(json.dumps({"text": text, "phonemes": ph, "batches": batches, "ids": ids}, ensure_ascii=False))
    if os.environ.get("DUMP_AUDIO"):
        audio, sr = k.create(text, voice="af_heart", speed=1.0, lang="en-us")
        np.asarray(audio, dtype="<f4").tofile(os.environ["DUMP_AUDIO"])
        print(f"wrote {len(audio)} samples @ {sr} Hz", file=sys.stderr)
