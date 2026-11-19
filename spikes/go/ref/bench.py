"""Spawn speak N times; report exec -> first audio sample (play mode) and RTF (file mode)."""
import json, os, statistics, subprocess, sys, time
binary, mode, n, text = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
env = dict(os.environ, SPEAK_LEXICON="/nonexistent")
rows = []
for i in range(n):
    args = [binary, "--timings"] + (["-o", "out/bench.wav"] if mode == "file" else []) + [text]
    t0 = time.time_ns()
    p = subprocess.Popen(args, stderr=subprocess.PIPE, text=True, env=env)
    first = None
    timings = {}
    for line in p.stderr:
        if line.startswith("FIRST_AUDIO"):
            first = (int(line.split()[1]) - t0) / 1e9
        elif line.startswith("{"):
            timings = json.loads(line)
    p.wait()
    wall = (time.time_ns() - t0) / 1e9
    rows.append(dict(timings, exec_to_first_audio_s=first, wall_s=wall))
for k in ("exec_to_first_audio_s", "first_chunk_since_start_s", "load_s", "synth_s", "audio_s", "rtf", "wall_s"):
    vals = [r[k] for r in rows if r.get(k) is not None]
    if vals:
        print(f"{k:28} median={statistics.median(vals):.3f} min={min(vals):.3f} max={max(vals):.3f} (n={len(vals)})")
