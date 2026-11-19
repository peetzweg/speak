"""Synthesise TEXT with kokoro-onnx (af_heart, 1.0, en-us), write PCM_16 WAV, and compare to Rust WAVs.

Usage: python -I audio.py "TEXT" out/parity-py.wav out/parity-rs.wav [more.wav ...]
"""
import sys

import numpy as np
import soundfile as sf
from kokoro_onnx import Kokoro
from kokoro_onnx.config import EspeakConfig

H = "~/.local/share/speak"
k = Kokoro(f"{H}/models/kokoro-v1.0.onnx", f"{H}/models/voices-v1.0.bin", espeak_config=EspeakConfig(data_path=f"{H}/es"))
audio, sr = k.create(sys.argv[1], voice="af_heart", speed=1.0, lang="en-us")
sf.write(sys.argv[2], audio, sr)
py, _ = sf.read(sys.argv[2], dtype="float32")
for path in sys.argv[3:]:
    rs, _ = sf.read(path, dtype="float32")
    n = min(len(py), len(rs))
    d = np.abs(py[:n] - rs[:n])
    corr = float(np.corrcoef(py[:n], rs[:n])[0, 1])
    print(f"{path}: samples py={len(py)} rs={len(rs)} max|diff|={d.max():.5f} mean|diff|={d.mean():.6f} corr={corr:.6f}")
