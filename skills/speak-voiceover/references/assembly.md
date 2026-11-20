# Assembling the result with ffmpeg

`scripts/assemble.py` covers the common case: one image per paragraph, plus the narration,
subtitles and chapters, giving an MP4. The recipes below cover variations. They assume
`voice.wav`, `voice.srt` and `voice.json` from:

```sh
speak -f script.txt -o voice.wav --subtitles voice.srt --manifest voice.json --loudness -16
```

## Slides from a deck

- Keynote, PowerPoint or Google Slides: export as images and name them `001.png`, `002.png`, ….
- PDF: `pdftoppm -png -r 150 deck.pdf slides/s` gives `s-1.png`, `s-2.png`, …. Rename them so
  they sort correctly once there are ten or more.
- HTML slides: screenshot each one with Playwright at 1920×1080.

## Burned-in subtitles instead of a subtitle track

Use this for social media, where players often ignore subtitle tracks:

```sh
ffmpeg -i video.mp4 -vf "subtitles=voice.srt:force_style='FontSize=22,MarginV=40'" \
  -c:a copy video-burned.mp4
```

## Crossfades between slides

Render one clip per slide: one image held for the paragraph's span, from the manifest. Then
join the clips with `xfade`:

```sh
ffmpeg -i a.mp4 -i b.mp4 -filter_complex "xfade=transition=fade:duration=0.5:offset=OFFSET" out.mp4
```

`OFFSET` is the length of the first clip minus 0.5. Chain one `xfade` per cut, and lay the
narration over the result unchanged. A fade of 0.3–0.5 s placed in the paragraph pause doesn't
cut into speech: the manifest gives each pause as one paragraph's `end` to the next one's
`start`.

## Music bed under the voice

```sh
ffmpeg -i voice.wav -stream_loop -1 -i bed.mp3 -filter_complex \
  "[1:a]volume=0.12[bed];[0:a][bed]amix=inputs=2:duration=first:dropout_transition=0,loudnorm=I=-16:TP=-1" \
  -c:a aac -b:a 192k mixed.m4a
```

The final `loudnorm` brings the mix back to −16 LUFS. For ducking, put
`sidechaincompress=threshold=0.05:ratio=8` on the bed, keyed by the voice.

## Audio-only formats

```sh
ffmpeg -i voice.wav -c:a aac -b:a 160k episode.m4a          # podcasts, Apple
ffmpeg -i voice.wav -c:a libmp3lame -q:a 2 episode.mp3      # everywhere
```

Chapters in an m4a: write the `;FFMETADATA1` chapters file as `assemble.py` does, then:

```sh
ffmpeg -i voice.wav -i chapters.txt -map 0:a -map_chapters 1 -c:a aac -b:a 160k episode.m4a
```

## Checks before handing over

```sh
ffprobe -v error -show_entries format=duration:stream=codec_type,codec_name -of compact video.mp4
ffprobe -v error -show_chapters -of compact=p=0 video.mp4
ffmpeg -i video.mp4 -af ebur128=framelog=quiet -f null - 2>&1 | grep -A2 Summary   # about -16 LUFS
```
