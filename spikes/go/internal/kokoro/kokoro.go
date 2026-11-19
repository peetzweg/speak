// Package kokoro is the Kokoro-82M (ONNX, v1.0) backend: a port of kokoro-onnx 0.4.7's
// Kokoro.create pipeline. text -> espeak-ng IPA -> vocab ids -> ONNX (tokens, style, speed)
// -> 24 kHz float32 audio, trimmed per batch.
package kokoro

import (
	"context"
	"fmt"
	"iter"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/peetzweg/speak/spikes/go/internal/espeak"
	"github.com/peetzweg/speak/spikes/go/internal/ort"
	"github.com/peetzweg/speak/spikes/go/internal/text"
	"github.com/peetzweg/speak/spikes/go/internal/tts"
)

const (
	sampleRate       = 24000
	maxPhonemeLength = 510
	modelFile        = "kokoro-v1.0.onnx"
	voicesFile_      = "voices-v1.0.bin"
)

var langs = map[byte]struct{ code, name string }{
	'a': {"en-us", "American English"}, 'b': {"en-gb", "British English"}, 'e': {"es", "Spanish"},
	'f': {"fr-fr", "French"}, 'h': {"hi", "Hindi"}, 'i': {"it", "Italian"}, 'j': {"ja", "Japanese"},
	'p': {"pt-br", "Brazilian Portuguese"}, 'z': {"cmn", "Mandarin"},
}

func init() { tts.Register("kokoro", Open) }

// G2P turns text into kokoro-onnx-compatible IPA (espeak in-process or as a subprocess
// today; a misaki-style lexicon+fallback chain later).
type G2P interface {
	Phonemize(text, lang string) (string, error)
}

// Kokoro is a loaded model.
type Kokoro struct {
	g2p     G2P
	g2pName string
	sess    ort.Session
	voices  *voicesFile
	styleMu sync.Mutex
	styles  map[string][][]float32
	ortLib  string
}

// Open loads espeak-ng, onnxruntime, the model and the voices file.
func Open(o tts.Options) (tts.Backend, error) {
	espeakLib := tts.FindLib("SPEAK_ESPEAK_LIB", tts.LibName("espeak-ng"), o.LibDirs)
	ortLib := tts.FindLib("SPEAK_ORT_LIB", tts.LibName("onnxruntime"), o.LibDirs)
	t := time.Now()
	trace := func(what string) {
		if os.Getenv("SPEAK_TRACE") != "" {
			fmt.Fprintf(os.Stderr, "trace %-14s %6.1f ms\n", what, float64(time.Since(t).Microseconds())/1000)
		}
		t = time.Now()
	}
	var g G2P
	var gname string
	switch mode := os.Getenv("SPEAK_G2P"); mode {
	case "proc", "spawn": // espeak-ng as a separate program (the Apache-2.0 plan)
		bin := os.Getenv("SPEAK_ESPEAK_BIN")
		if bin == "" {
			bin = "espeak-ng"
		}
		if _, err := exec.LookPath(bin); err != nil {
			return nil, fmt.Errorf("espeak-ng program not found (%s): install espeak-ng or set SPEAK_ESPEAK_BIN", bin)
		}
		g = &espeak.Proc{Bin: bin, DataDir: os.Getenv("SPEAK_ESPEAK_DATA"), Spawn: mode == "spawn"}
		gname = "espeak-ng subprocess (" + mode + ")"
	case "helper": // our own GPL helper program running espeak_TextToPhonemes
		bin := os.Getenv("SPEAK_ESPEAK_HELPER")
		if bin == "" {
			bin = "speak-espeak"
		}
		g, gname = &espeak.Helper{Bin: bin, Lib: espeakLib, DataDir: o.ESpeakData}, "speak-espeak helper process"
	default: // libespeak-ng loaded in-process via purego (GPL combination)
		es, err := espeak.Open(espeakLib, o.ESpeakData)
		if err != nil {
			return nil, err
		}
		g, gname = es, "libespeak-ng "+es.Version+" in-process"
	}
	trace("espeak init")
	v, err := openVoices(filepath.Join(o.ModelDir, voicesFile_))
	if err != nil {
		return nil, err
	}
	trace("voices index")
	s, err := ort.Open(ortLib, filepath.Join(o.ModelDir, modelFile), []string{"tokens", "style", "speed"}, []string{"audio"}, 0)
	if err != nil {
		return nil, err
	}
	trace("ort session")
	return &Kokoro{g2p: g, g2pName: gname, sess: s, voices: v, styles: map[string][][]float32{}, ortLib: ortLib}, nil
}

func (k *Kokoro) Info() tts.Info {
	return tts.Info{Name: "kokoro", Model: "Kokoro-82M v1.0 (ONNX)", SampleRate: sampleRate, License: "Apache-2.0",
		Runtime: fmt.Sprintf("onnxruntime %s via %s; G2P %s", ort.Version(), ort.Binding, k.g2pName)}
}

func (k *Kokoro) Voices() []tts.Voice {
	var out []tts.Voice
	for _, n := range k.voices.names() {
		l := langs[n[0]]
		g := map[byte]string{'f': "female", 'm': "male"}[n[1]]
		out = append(out, tts.Voice{ID: n, Language: l.code, Gender: g, Label: strings.TrimSpace(l.name + ", " + g)})
	}
	return out
}

func (k *Kokoro) Close() error {
	if p, ok := k.g2p.(interface{ Close() }); ok {
		p.Close()
	}
	return k.sess.Close()
}

// LangFor returns the espeak language for a voice, as speak.py does (first letter).
func LangFor(voice string) string {
	if l, ok := langs[voice[0]]; ok {
		return l.code
	}
	return "en-us"
}

// Phonemize is kokoro_onnx.tokenizer.Tokenizer.phonemize.
func (k *Kokoro) Phonemize(text, lang string) (string, error) {
	ph, err := k.g2p.Phonemize(espeak.PyStrip(text), lang)
	if err != nil {
		return "", err
	}
	var b strings.Builder
	for _, r := range ph {
		if _, ok := vocab[r]; ok {
			b.WriteRune(r)
		}
	}
	return espeak.PyStrip(b.String()), nil
}

var rePunct = regexp.MustCompile(`[.,!?;]`)

// SplitPhonemes is Kokoro._split_phonemes (lengths in code points, like Python's len).
func SplitPhonemes(ph string) []string {
	// re.split(r"([.,!?;])", s) keeping the delimiters
	var parts []string
	last := 0
	for _, m := range rePunct.FindAllStringIndex(ph, -1) {
		parts = append(parts, ph[last:m[0]], ph[m[0]:m[1]])
		last = m[1]
	}
	parts = append(parts, ph[last:])

	var batches []string
	cur := ""
	for _, p := range parts {
		p = espeak.PyStrip(p)
		if p == "" {
			continue
		}
		if utf8.RuneCountInString(cur)+utf8.RuneCountInString(p)+1 >= maxPhonemeLength {
			batches = append(batches, espeak.PyStrip(cur))
			cur = p
		} else if strings.Contains(".,!?;", p) {
			cur += p
		} else {
			if cur != "" {
				cur += " "
			}
			cur += p
		}
	}
	if cur != "" {
		batches = append(batches, espeak.PyStrip(cur))
	}
	return batches
}

// Tokenize maps phonemes (truncated to 510 code points) to ids and pads with 0 both ends.
func Tokenize(ph string) []int64 {
	ids := []int64{0}
	n := 0
	for _, r := range ph {
		if n == maxPhonemeLength {
			break
		}
		n++
		if id, ok := vocab[r]; ok {
			ids = append(ids, id)
		}
	}
	return append(ids, 0)
}

func (k *Kokoro) style(voice string) ([][]float32, error) {
	k.styleMu.Lock()
	defer k.styleMu.Unlock()
	if s, ok := k.styles[voice]; ok {
		return s, nil
	}
	s, err := k.voices.load(voice)
	if err != nil {
		return nil, err
	}
	k.styles[voice] = s
	return s, nil
}

// Create is Kokoro.create(text, voice, speed, lang) with trim=True.
func (k *Kokoro) Create(text, voice string, speed float32, lang string) ([]float32, error) {
	st, err := k.style(voice)
	if err != nil {
		return nil, err
	}
	ph, err := k.Phonemize(text, lang)
	if err != nil {
		return nil, err
	}
	var audio []float32
	for _, batch := range SplitPhonemes(ph) {
		ids := Tokenize(batch)
		row := min(len(ids)-2, len(st)-1) // voice[len(tokens)]; kokoro-onnx would IndexError at 510
		out, err := k.sess.Run([]ort.Tensor{
			{Name: "tokens", Shape: []int64{1, int64(len(ids))}, I64: ids},
			{Name: "style", Shape: []int64{1, int64(len(st[row]))}, F32: st[row]},
			{Name: "speed", Shape: []int64{1}, F32: []float32{speed}},
		})
		if err != nil {
			return nil, err
		}
		audio = append(audio, trim(out)...)
	}
	return audio, nil
}

// Synthesize splits text into sentences (speak.py's chunks) and yields one chunk each.
func (k *Kokoro) Synthesize(ctx context.Context, req tts.Request) iter.Seq2[tts.Chunk, error] {
	return func(yield func(tts.Chunk, error) bool) {
		if req.Speed < 0.5 || req.Speed > 2 {
			yield(tts.Chunk{}, fmt.Errorf("speed %.2f outside 0.5..2.0", req.Speed))
			return
		}
		lang := LangFor(req.Voice)
		for s := range text.Sentences(req.Text) {
			if ctx.Err() != nil {
				yield(tts.Chunk{}, ctx.Err())
				return
			}
			pcm, err := k.Create(s, req.Voice, req.Speed, lang)
			if !yield(tts.Chunk{PCM: pcm, Text: s}, err) || err != nil {
				return
			}
		}
	}
}
