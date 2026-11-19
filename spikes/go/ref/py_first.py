"""Python baseline: what speak.py does before afplay starts (imports, load, first sentence)."""
import sys, time
import numpy as np, soundfile as sf
import onnxruntime as ort
ort.set_default_logger_severity(3)
from kokoro_onnx import Kokoro
from kokoro_onnx.config import EspeakConfig
import os
H = os.path.expanduser("~/.local/share/speak")
k = Kokoro(H + "/models/kokoro-v1.0.onnx", H + "/models/voices-v1.0.bin", espeak_config=EspeakConfig(data_path=H + "/es"))
a, sr = k.create(sys.argv[1], voice="af_heart", speed=1.0, lang="en-us")
sf.write("out/py-first.wav", a, sr)
print(f"FIRST_AUDIO {time.time_ns()}", file=sys.stderr)
