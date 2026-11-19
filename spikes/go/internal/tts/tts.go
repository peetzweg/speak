// Package tts is the seam between the speak CLI and the models it can drive.
//
// Every local model (Kokoro today; Kitten TTS, Piper, an MLX or llama.cpp-style model,
// or a local HTTP server later) is a Backend. The CLI only does text plumbing
// (lexicon, args/stdin), audio output and config; everything model-specific,
// including whether there is a G2P step at all, lives behind this interface.
package tts

import (
	"context"
	"fmt"
	"iter"
	"sort"
)

// Info describes a backend and the audio it produces.
type Info struct {
	Name       string // registry key, e.g. "kokoro"
	Model      string // e.g. "Kokoro-82M v1.0 (ONNX)"
	SampleRate int    // Hz, mono float32 PCM
	License    string // model weights licence
	Runtime    string // e.g. "onnxruntime 1.32.0 via yalue/onnxruntime_go"
}

// Voice is one selectable speaker.
type Voice struct {
	ID       string // what -v takes, e.g. "af_heart"
	Language string // BCP-47-ish, e.g. "en-us"
	Gender   string // "female", "male" or ""
	Label    string // human description for `speak -v '?'` and the picker
}

// Request is one utterance.
type Request struct {
	Text  string
	Voice string
	Speed float32 // 1.0 = model default; the CLI maps say's -r WPM onto this
}

// Chunk is a run of mono float32 PCM at Info().SampleRate, in playback order.
type Chunk struct {
	PCM  []float32
	Text string // the source text this chunk speaks (progress, captions)
}

// Backend is a loaded model. Implementations must be safe to call from one
// goroutine at a time; the CLI never synthesises two requests concurrently.
type Backend interface {
	Info() Info
	Voices() []Voice
	// Synthesize yields audio in order, as soon as each piece is ready, so playback
	// can start after the first sentence. Breaking out of the range loop (or
	// cancelling ctx) stops synthesis.
	Synthesize(ctx context.Context, req Request) iter.Seq2[Chunk, error]
	Close() error
}

// Options are the host facts a backend may need to locate its runtime.
type Options struct {
	ModelDir   string // where this backend's downloaded files live
	LibDirs    []string
	ESpeakData string // short path (<~140 chars) to the dir containing espeak-ng-data
}

// Factory opens a backend.
type Factory func(Options) (Backend, error)

var registry = map[string]Factory{}

// Register makes a backend available by name; called from the backend package's init.
func Register(name string, f Factory) { registry[name] = f }

// Open opens the named backend.
func Open(name string, o Options) (Backend, error) {
	f, ok := registry[name]
	if !ok {
		return nil, fmt.Errorf("unknown backend %q (have %v)", name, Names())
	}
	return f(o)
}

// Names lists registered backends.
func Names() []string {
	var n []string
	for k := range registry {
		n = append(n, k)
	}
	sort.Strings(n)
	return n
}
