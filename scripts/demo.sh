#!/usr/bin/env bash
# Record the speak demo: a terminal session (vhs) typing speak commands, with the real Kokoro
# audio laid under each command at the moment it runs.
#
#   scripts/demo.sh          # README: out/demo/demo.mp4 (with sound) and out/demo/demo.gif
#   scripts/demo.sh 16x9     # social posts: out/demo-16x9/demo.mp4, the terminal at 1920x1080
#
# Needs vhs, ffmpeg, bc and speak (SPEAK_BIN, default: on PATH). While vhs records, `speak` is a
# stand-in that waits as long as the real clip lasts, so the prompt comes back exactly when the
# voice ends; nothing plays out loud.
set -euo pipefail
FORMAT=${1:-readme}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SPEAK=${SPEAK_BIN:-$(command -v speak)}
LATENCY=0.45 # seconds from Enter to the first sound (speak's time to first audio)
TYPING=0.045 # seconds per typed character
case $FORMAT in
  readme)
    OUT=$ROOT/out/demo
    PROMPT='$ '
    LOOK="Output \"$OUT/demo.gif\"
Set FontSize 26
Set Width 1280
Set Height 420
Set Padding 36"
    ;;
  16x9)
    # Larger type, and a blank line before each prompt so the session fills the frame.
    OUT=$ROOT/out/demo-16x9
    PROMPT='\n$ '
    LOOK="Set FontSize 40
Set LineHeight 1.3
Set Width 1920
Set Height 1080
Set Padding 60"
    ;;
  *)
    echo "usage: scripts/demo.sh [16x9]" >&2
    exit 2
    ;;
esac
rm -rf "$OUT" && mkdir -p "$OUT/clips" "$OUT/config"

# The session, one command per entry: speak's options and the text. An entry with no text is
# typed as a comment.
OPTS=(""                                                        "-v bm_george"                                      "-v ef_dora"                 "--background"                       "# agents keep working while it talks" "-r 210")
TEXT=("Hi, I am speak. A neural voice that runs on your own machine." "No cloud and no API keys. Like say, only better." "Y también hablo español." "Build finished. All tests passed." ""                                      "And faster when you are in a hurry.")

# Render each clip with the real speak, without the user's config or pronunciations.
DUR=()
for i in "${!TEXT[@]}"; do
  [[ -n ${TEXT[$i]} ]] || { DUR[$i]=0; continue; }
  opts=${OPTS[$i]/--background/}
  # shellcheck disable=SC2086 # the options are words on purpose
  XDG_CONFIG_HOME=$OUT/config SPEAK_LEXICON=$OUT/config/none.txt \
    "$SPEAK" $opts -o "$OUT/clips/$i.wav" -- "${TEXT[$i]}"
  DUR[$i]=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT/clips/$i.wav")
  echo "${DUR[$i]}" >"$OUT/clips/$(printf '%s' "${TEXT[$i]}" | shasum | cut -c1-12).dur"
done

typed() { # the command as typed in the session
  if [[ -z ${TEXT[$1]} ]]; then echo "${OPTS[$1]}"; else echo "speak ${OPTS[$1]:+${OPTS[$1]} }\"${TEXT[$1]}\""; fi
}

TAPE=$OUT/demo.tape
cat >"$TAPE" <<EOF
Output "$OUT/demo.mp4"
$LOOK
Set Shell bash
Set Framerate 30
Set TypingSpeed ${TYPING}s
Set Theme "Catppuccin Mocha"
Set CursorBlink false
Hide
Type@1ms \`export PS1='$PROMPT' HISTFILE=/dev/null; speak() { case " \$* " in *' --background '*) return;; esac; sleep \$(echo "$LATENCY + \$(cat $OUT/clips/\$(printf '%s' "\${@: -1}" | shasum | cut -c1-12).dur)" | bc); }; clear\`
Enter
Sleep 1s
Show
Sleep 800ms
EOF

# Write the visible part and, alongside, when each spoken command's Enter lands in the video.
t=0.8
: >"$OUT/timeline.txt"
for i in "${!OPTS[@]}"; do
  c=$(typed "$i")
  printf 'Type `%s`\nSleep 300ms\nEnter\n' "$c" >>"$TAPE"
  t=$(echo "$t + ${#c} * $TYPING + 0.3" | bc)
  [[ -n ${TEXT[$i]} ]] && echo "$i $t" >>"$OUT/timeline.txt"
  if [[ ${OPTS[$i]} == --background ]]; then
    pause=0.9
  elif [[ -n ${TEXT[$i]} ]]; then
    pause=$(echo "$LATENCY + ${DUR[$i]} + 0.8" | bc)
  else # a comment typed while the background voice talks: wait for it to finish
    pause=$(echo "${DUR[$((i - 1))]} - ${#c} * $TYPING" | bc)
  fi
  printf 'Sleep %sms\n' "$(echo "$pause * 1000 / 1" | bc)" >>"$TAPE"
  t=$(echo "$t + $pause" | bc)
done
printf 'Type `brew install peetzweg/tap/speak`\nSleep 3s\n' >>"$TAPE"

vhs "$TAPE" >/dev/null 2>&1
VIDEO=$OUT/demo.mp4

# The audio track: each clip at its command's Enter plus the latency, muxed into the video.
inputs=() filters="" mix="" n=0
while read -r i at; do
  inputs+=(-i "$OUT/clips/$i.wav")
  filters+="[$n:a]adelay=$(echo "($at + $LATENCY) * 1000 / 1" | bc):all=1[a$n];"
  mix+="[a$n]"
  n=$((n + 1))
done <"$OUT/timeline.txt"
length=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$VIDEO")
ffmpeg -v error -y "${inputs[@]}" -filter_complex "${filters}${mix}amix=inputs=$n:normalize=0,apad[aout]" \
  -map '[aout]' -t "$length" "$OUT/audio.wav"
ffmpeg -v error -y -i "$VIDEO" -i "$OUT/audio.wav" -map 0:v -map 1:a -c:v copy -c:a aac -b:a 160k \
  -movflags +faststart "$OUT/demo-sound.mp4"
mv "$OUT/demo-sound.mp4" "$OUT/demo.mp4"
echo "$OUT/demo.mp4 (${length} s)"
