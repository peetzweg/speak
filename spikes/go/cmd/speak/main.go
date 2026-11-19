// speak: say(1) with a local neural voice. Go spike.
//
//	speak "Hello"                 speak -v af_heart -r 200 "Faster"
//	echo hi | speak               speak -f notes.txt -o out.wav
//	speak -v '?'                  speak voices        (interactive picker on a TTY)
//
// Exit codes (stable, for scripts and agents): 0 ok, 1 synthesis/audio error,
// 2 usage error, 3 model or runtime library missing, 130 interrupted.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"time"

	"golang.org/x/term"

	"github.com/peetzweg/speak/spikes/go/internal/audio"
	"github.com/peetzweg/speak/spikes/go/internal/kokoro"
	"github.com/peetzweg/speak/spikes/go/internal/text"
	"github.com/peetzweg/speak/spikes/go/internal/tts"
	"github.com/peetzweg/speak/spikes/go/internal/tui"
)

var t0 = time.Now()

const (
	exitOK, exitErr, exitUsage, exitMissing, exitInterrupted = 0, 1, 2, 3, 130
	baseWPM                                                 = 175.0
)

type exitError struct {
	code int
	err  error
}

func (e exitError) Error() string { return e.err.Error() }

func fail(code int, format string, a ...any) error {
	return exitError{code, fmt.Errorf(format, a...)}
}

func main() {
	err := run(os.Args[1:])
	if err == nil {
		return
	}
	code := exitErr
	var ee exitError
	if errors.As(err, &ee) {
		code = ee.code
	}
	if errors.Is(err, context.Canceled) {
		code = exitInterrupted
	}
	if code != exitInterrupted {
		fmt.Fprintln(os.Stderr, "speak:", err)
	}
	os.Exit(code)
}

type opts struct {
	voice, file, out, raw, backend string
	rate, speed                    float64
	debugTokens, timings, quiet    bool
}

func run(args []string) error {
	if len(args) > 0 && args[0] == "voices" {
		return voicesCmd(args[1:])
	}
	fs := flag.NewFlagSet("speak", flag.ContinueOnError)
	var o opts
	cfg := loadConfig()
	for _, n := range []string{"v", "voice"} {
		fs.StringVar(&o.voice, n, envOr("SPEAK_VOICE", cfg["voice"], "af_heart"), "voice, or '?' to list")
	}
	for _, n := range []string{"r", "rate"} {
		fs.Float64Var(&o.rate, n, 0, "words per minute (default 175)")
	}
	for _, n := range []string{"f", "input-file"} {
		fs.StringVar(&o.file, n, "", "read text from file ('-' = stdin)")
	}
	for _, n := range []string{"o", "output-file"} {
		fs.StringVar(&o.out, n, "", "write WAV instead of playing")
	}
	fs.Float64Var(&o.speed, "speed", 0, "speed multiplier 0.5..2.0 (overrides -r)")
	fs.StringVar(&o.backend, "backend", envOr("SPEAK_BACKEND", cfg["backend"], "kokoro"), "model backend")
	fs.BoolVar(&o.debugTokens, "debug-tokens", false, "print phonemes/batches/token ids as JSON and exit")
	fs.StringVar(&o.raw, "raw", "", "write Kokoro.create(text) as raw float32 LE (parity check)")
	fs.BoolVar(&o.timings, "timings", false, "print timing JSON to stderr")
	fs.BoolVar(&o.quiet, "q", false, "no warnings")
	fs.SetOutput(io.Discard)
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			fs.SetOutput(os.Stdout)
			fs.PrintDefaults()
			return nil
		}
		return fail(exitUsage, "%v", err)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	// Open the audio device while the model loads.
	var streamCh chan streamResult
	if o.out == "" && o.raw == "" && !o.debugTokens && o.voice != "?" {
		streamCh = make(chan streamResult, 1)
		go func() { s, err := audio.NewStream(24000); streamCh <- streamResult{s, err} }()
	}

	tLoad := time.Now()
	b, err := openBackend(o.backend)
	if err != nil {
		return err
	}
	defer b.Close()
	loadDur := time.Since(tLoad)

	if o.voice == "?" {
		for _, v := range b.Voices() {
			fmt.Printf("%-16s %s\n", v.ID, v.Label)
		}
		return nil
	}
	if !hasVoice(b, o.voice) {
		return fail(exitUsage, "unknown voice %q (try: speak -v '?')", o.voice)
	}

	txt, err := readText(fs.Args(), o.file)
	if err != nil {
		return err
	}
	if strings.TrimSpace(txt) == "" {
		return nil
	}
	lx, err := text.LoadLexicon(lexiconPath())
	if err != nil {
		return fail(exitUsage, "lexicon: %v", err)
	}
	txt = lx.Apply(txt)

	speed := 1.0
	if o.speed != 0 {
		speed = o.speed
	} else if o.rate != 0 {
		speed = o.rate / baseWPM
	}
	speed = max(0.5, min(2.0, speed))

	if os.Getenv("SPEAK_BENCH_G2P") != "" {
		return benchG2P(b, txt)
	}
	if o.debugTokens || o.raw != "" {
		return debug(b, txt, o, float32(speed))
	}

	req := tts.Request{Text: txt, Voice: o.voice, Speed: float32(speed)}
	rate := b.Info().SampleRate
	gap := make([]float32, int(0.12*float64(rate)))
	var synth time.Duration
	var samples int
	var firstChunk time.Duration

	if o.out != "" {
		if !strings.EqualFold(filepath.Ext(o.out), ".wav") {
			return fail(exitUsage, "only .wav output in the spike: %s", o.out)
		}
		var all []float32
		ts := time.Now()
		for c, err := range b.Synthesize(ctx, req) {
			if err != nil {
				return err
			}
			if firstChunk == 0 {
				firstChunk = time.Since(t0)
			}
			all = append(append(all, c.PCM...), gap...)
			samples += len(c.PCM)
		}
		synth = time.Since(ts)
		if err := audio.WriteWAV(o.out, all, rate); err != nil {
			return err
		}
		return report(o, map[string]any{"mode": "file", "load_s": loadDur.Seconds(), "first_chunk_since_start_s": firstChunk.Seconds(),
			"synth_s": synth.Seconds(), "audio_s": float64(samples) / float64(rate), "rtf": synth.Seconds() / (float64(samples) / float64(rate))})
	}

	sr := <-streamCh
	if sr.err != nil {
		return fail(exitErr, "audio device: %v", sr.err)
	}
	st := sr.s
	var firstAudio time.Time
	go func() {
		firstAudio = <-st.First
		if o.timings {
			fmt.Fprintf(os.Stderr, "FIRST_AUDIO %d\n", firstAudio.UnixNano())
		}
	}()
	ts := time.Now()
	for c, err := range b.Synthesize(ctx, req) {
		if err != nil {
			st.Stop()
			return err
		}
		if firstChunk == 0 {
			firstChunk = time.Since(t0)
		}
		samples += len(c.PCM)
		st.Push(c.PCM)
		st.Push(gap)
	}
	synth = time.Since(ts)
	st.Close()
	done := make(chan error, 1)
	go func() { done <- st.Wait() }()
	select {
	case err := <-done:
		if err != nil {
			return err
		}
	case <-ctx.Done():
		st.Stop()
		return ctx.Err()
	}
	return report(o, map[string]any{"mode": "play", "load_s": loadDur.Seconds(), "first_chunk_since_start_s": firstChunk.Seconds(),
		"first_audio_since_start_s": firstAudio.Sub(t0).Seconds(), "synth_s": synth.Seconds(),
		"audio_s": float64(samples) / float64(rate), "rtf": synth.Seconds() / (float64(samples) / float64(rate))})
}

type streamResult struct {
	s   *audio.Stream
	err error
}

func report(o opts, m map[string]any) error {
	if o.timings {
		j, _ := json.Marshal(m)
		fmt.Fprintln(os.Stderr, string(j))
	}
	return nil
}

func debug(b tts.Backend, txt string, o opts, speed float32) error {
	k, ok := b.(*kokoro.Kokoro)
	if !ok {
		return fail(exitUsage, "--debug-tokens needs the kokoro backend")
	}
	lang := kokoro.LangFor(o.voice)
	if o.raw != "" {
		pcm, err := k.Create(txt, o.voice, speed, lang)
		if err != nil {
			return err
		}
		return audio.WriteRawF32(o.raw, pcm)
	}
	ph, err := k.Phonemize(txt, lang)
	if err != nil {
		return err
	}
	batches := kokoro.SplitPhonemes(ph)
	ids := make([][]int64, len(batches))
	for i, bt := range batches {
		ids[i] = kokoro.Tokenize(bt)
	}
	enc := json.NewEncoder(os.Stdout)
	enc.SetEscapeHTML(false)
	return enc.Encode(map[string]any{"text": txt, "phonemes": ph, "batches": batches, "ids": ids})
}

func hasVoice(b tts.Backend, v string) bool {
	for _, x := range b.Voices() {
		if x.ID == v {
			return true
		}
	}
	return false
}

func openBackend(name string) (tts.Backend, error) {
	home := envOr("SPEAK_HOME", "", filepath.Join(userHome(), ".local/share/speak"))
	o := tts.Options{ModelDir: filepath.Join(home, "models"), LibDirs: tts.LibDirs(), ESpeakData: espeakData(home)}
	for _, f := range []string{"kokoro-v1.0.onnx", "voices-v1.0.bin"} {
		if _, err := os.Stat(filepath.Join(o.ModelDir, f)); err != nil {
			return nil, fail(exitMissing, "model file missing: %s (first-run download not in the spike)", filepath.Join(o.ModelDir, f))
		}
	}
	b, err := tts.Open(name, o)
	if err != nil {
		return nil, fail(exitMissing, "%v", err)
	}
	return b, nil
}

// espeakData finds the directory containing espeak-ng-data: $SPEAK_ESPEAK_DATA, then
// <prefix>/share/speak or <prefix>/share next to the binary, then $SPEAK_HOME/es.
func espeakData(home string) string {
	if v := os.Getenv("SPEAK_ESPEAK_DATA"); v != "" {
		return v
	}
	if exe, err := os.Executable(); err == nil {
		if r, err := filepath.EvalSymlinks(exe); err == nil {
			exe = r
		}
		d := filepath.Dir(exe)
		for _, c := range []string{filepath.Join(d, "..", "share", "speak"), filepath.Join(d, "..", "share"), filepath.Join(d, "share")} {
			if _, err := os.Stat(filepath.Join(c, "espeak-ng-data", "phontab")); err == nil {
				return filepath.Clean(c)
			}
		}
	}
	return filepath.Join(home, "es")
}

func readText(args []string, file string) (string, error) {
	switch {
	case file == "-":
		b, err := io.ReadAll(os.Stdin)
		return string(b), err
	case file != "":
		b, err := os.ReadFile(file)
		if err != nil {
			return "", fail(exitUsage, "%v", err)
		}
		return string(b), nil
	case len(args) > 0:
		return strings.Join(args, " "), nil
	case !term.IsTerminal(int(os.Stdin.Fd())):
		b, err := io.ReadAll(os.Stdin)
		return string(b), err
	}
	return "", fail(exitUsage, "no text (pass it as arguments, with -f, or on stdin)")
}

func voicesCmd(args []string) error {
	b, err := openBackend(envOr("SPEAK_BACKEND", loadConfig()["backend"], "kokoro"))
	if err != nil {
		return err
	}
	defer b.Close()
	cur := envOr("SPEAK_VOICE", loadConfig()["voice"], "af_heart")
	interactive := term.IsTerminal(int(os.Stdin.Fd())) && term.IsTerminal(int(os.Stdout.Fd()))
	if len(args) > 0 && args[0] == "--list" || !interactive && os.Getenv("SPEAK_PICKER_TEST") == "" {
		for _, v := range b.Voices() {
			fmt.Printf("%-16s %s\n", v.ID, v.Label)
		}
		return nil
	}
	play := func(v string) error {
		st, err := audio.NewStream(b.Info().SampleRate)
		if err != nil {
			return err
		}
		for c, err := range b.Synthesize(context.Background(), tts.Request{Text: "Hi, I'm " + v + ". This is how I sound.", Voice: v, Speed: 1}) {
			if err != nil {
				return err
			}
			st.Push(c.PCM)
		}
		st.Close()
		return st.Wait()
	}
	chosen, err := tui.Pick(b.Voices(), cur, play)
	if err != nil {
		return err
	}
	if chosen != "" {
		if err := saveConfig("voice", chosen); err != nil {
			return err
		}
		fmt.Fprintf(os.Stderr, "default voice: %s (%s)\n", chosen, configPath())
	}
	return nil
}

func envOr(k, cfg, def string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	if cfg != "" {
		return cfg
	}
	return def
}

func userHome() string { h, _ := os.UserHomeDir(); return h }

func configDir() string {
	if v := os.Getenv("XDG_CONFIG_HOME"); v != "" {
		return filepath.Join(v, "speak")
	}
	if d, err := os.UserConfigDir(); err == nil && os.PathSeparator == '\\' {
		return filepath.Join(d, "speak") // %AppData%\speak on Windows
	}
	return filepath.Join(userHome(), ".config", "speak")
}

func configPath() string  { return envOr("SPEAK_CONFIG", "", filepath.Join(configDir(), "config.toml")) }
func lexiconPath() string { return envOr("SPEAK_LEXICON", "", filepath.Join(configDir(), "lexicon.txt")) }

// loadConfig reads `key = "value"` lines (a TOML subset; a real build would use a TOML lib).
func loadConfig() map[string]string {
	m := map[string]string{}
	b, err := os.ReadFile(configPath())
	if err != nil {
		return m
	}
	for _, l := range strings.Split(string(b), "\n") {
		if k, v, ok := strings.Cut(l, "="); ok && !strings.HasPrefix(strings.TrimSpace(l), "#") {
			m[strings.TrimSpace(k)] = strings.Trim(strings.TrimSpace(v), `"`)
		}
	}
	return m
}

func saveConfig(key, val string) error {
	m := loadConfig()
	m[key] = val
	var b strings.Builder
	for k, v := range m {
		fmt.Fprintf(&b, "%s = %q\n", k, v)
	}
	if err := os.MkdirAll(filepath.Dir(configPath()), 0o755); err != nil {
		return err
	}
	return os.WriteFile(configPath(), []byte(b.String()), 0o644)
}
