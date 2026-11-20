#!/usr/bin/env python3
"""Assemble a narrated video from speak's manifest and one image per paragraph.

    python3 assemble.py --manifest voice.json --slides slides --out video.mp4 \
        [--subtitles voice.srt] [--chapters chapters.txt] [--audio voice.wav] [--tail 1.0]

Slide N is shown for paragraph N. The cut to the next slide falls halfway through the pause
between two paragraphs, so a new slide appears just before its narration starts. The last slide
stays up for --tail seconds after the audio ends. Chapters follow the slides; their titles come
from --chapters (one line per paragraph) or else from each paragraph's first sentence.
Needs ffmpeg on PATH. Standard library only.
"""

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

IMAGE_TYPES = {".png", ".jpg", ".jpeg", ".webp"}


def fail(msg: str) -> None:
    print(f"assemble: {msg}", file=sys.stderr)
    sys.exit(2)


def encoder() -> list[str]:
    """libx264 when ffmpeg has it, else the platform's hardware H.264 encoder."""
    out = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    if " libx264 " in out:
        return ["-c:v", "libx264", "-tune", "stillimage", "-crf", "20", "-preset", "medium"]
    if " h264_videotoolbox " in out:
        return ["-c:v", "h264_videotoolbox", "-b:v", "4M"]
    return ["-c:v", "mpeg4", "-q:v", "3"]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--manifest", required=True, type=Path)
    ap.add_argument("--slides", required=True, type=Path, help="directory of images, one per paragraph, in name order")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--audio", type=Path, help="default: the manifest's audio file")
    ap.add_argument("--subtitles", type=Path, help=".srt or .vtt, added as a soft subtitle track")
    ap.add_argument("--chapters", type=Path, help="one chapter title per line, one per paragraph")
    ap.add_argument("--tail", type=float, default=1.0, help="seconds to hold the last slide after the audio (default 1)")
    ap.add_argument("--fps", type=int, default=30)
    args = ap.parse_args()

    if not shutil.which("ffmpeg"):
        fail("ffmpeg not found on PATH")
    manifest = json.loads(args.manifest.read_text())
    paragraphs = manifest["paragraphs"]
    audio = args.audio or (args.manifest.parent / manifest["audio"] if manifest.get("audio") else None)
    if not audio or not audio.is_file():
        fail(f"audio file not found ({audio}); pass --audio")
    slides = sorted(p for p in args.slides.iterdir() if p.suffix.lower() in IMAGE_TYPES)
    if len(slides) != len(paragraphs):
        fail(f"{len(slides)} images in {args.slides} but {len(paragraphs)} paragraphs in the manifest")

    total = manifest["duration"] + args.tail
    cuts = [0.0]
    for prev, nxt in zip(paragraphs, paragraphs[1:]):
        cuts.append((prev["end"] + nxt["start"]) / 2)
    cuts.append(total)

    if args.chapters:
        titles = [t.strip() for t in args.chapters.read_text().splitlines() if t.strip()]
        if len(titles) != len(paragraphs):
            fail(f"{len(titles)} chapter titles but {len(paragraphs)} paragraphs")
    else:
        titles = []
        for p in paragraphs:
            first = p["sentences"][0]["text"] if p.get("sentences") else ""
            titles.append(first if len(first) <= 60 else first[:57].rstrip() + "...")

    with tempfile.TemporaryDirectory() as tmp:
        concat = Path(tmp) / "slides.txt"
        lines = []
        for img, start, end in zip(slides, cuts, cuts[1:]):
            lines += [f"file '{img.resolve()}'", f"duration {end - start:.6f}"]
        lines.append(f"file '{slides[-1].resolve()}'")  # the concat demuxer ignores the last duration otherwise
        concat.write_text("\n".join(lines) + "\n")

        meta = Path(tmp) / "chapters.txt"
        chapter_lines = [";FFMETADATA1"]
        for title, start, end in zip(titles, cuts, cuts[1:]):
            safe = title.replace("\\", "\\\\").replace("=", "\\=").replace(";", "\\;").replace("#", "\\#")
            chapter_lines += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={round(start * 1000)}", f"END={round(end * 1000)}", f"title={safe}"]
        meta.write_text("\n".join(chapter_lines) + "\n")

        cmd = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
               "-f", "concat", "-safe", "0", "-i", str(concat), "-i", str(audio)]
        maps = ["-map", "0:v", "-map", "1:a"]
        n = 2
        if args.subtitles:
            cmd += ["-i", str(args.subtitles)]
            maps += ["-map", f"{n}:s", "-c:s", "mov_text", "-metadata:s:s:0", "language=eng"]
            n += 1
        cmd += ["-i", str(meta)]
        maps += ["-map_chapters", str(n)]
        cmd += maps + [
            "-vf", f"scale=trunc(iw/2)*2:trunc(ih/2)*2,fps={args.fps},format=yuv420p",
            *encoder(),
            "-af", "apad", "-c:a", "aac", "-b:a", "192k",
            "-t", f"{total:.3f}", "-movflags", "+faststart", str(args.out),
        ]
        result = subprocess.run(cmd)
        if result.returncode != 0:
            sys.exit(result.returncode)

    print(f"{args.out}  {total:.1f} s, {len(slides)} slides")
    for title, start in zip(titles, cuts):
        print(f"  {int(start // 60)}:{start % 60:04.1f}  {title}")


if __name__ == "__main__":
    main()
