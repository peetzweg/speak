// Package espeak is a cgo-free binding to libespeak-ng (loaded at runtime with purego)
// plus a faithful port of the text -> IPA path that kokoro-onnx uses:
// phonemizer.phonemize(text, lang, preserve_punctuation=True, with_stress=True).
package espeak

import (
	"fmt"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"

	"github.com/peetzweg/speak/spikes/go/internal/dl"
)

const (
	audioOutputSynchronous = 2
	charsUTF8              = 1
	// phonemizer: phonemes_mode = ord('_') << 8 | 0x02  (IPA, '_' between phonemes)
	phonemeModeIPAUnderscore = int32('_')<<8 | 0x02
	// espeak-ng copies the data path into a 160-byte buffer as "<path>/espeak-ng-data"
	// and silently truncates longer paths.
	maxDataPath = 160 - len("/espeak-ng-data") - 1
)

// ESpeak is a loaded libespeak-ng. It is not reentrant; calls are serialised.
type ESpeak struct {
	mu             sync.Mutex
	initialize     func(output int32, buflen int32, path string, options int32) int32
	setVoiceByName func(name string) int32
	listVoices     func(spec uintptr) uintptr
	textToPhonemes func(textptr *uintptr, textmode int32, phonememode int32) uintptr
	info           func(path *uintptr) uintptr
	voice          string // language currently selected
	Version        string
}

// Open loads libespeak-ng from libPath and initialises it with dataDir, the directory
// that contains espeak-ng-data.
func Open(libPath, dataDir string) (*ESpeak, error) {
	if len(dataDir) > maxDataPath {
		return nil, fmt.Errorf("espeak-ng data path is %d bytes; espeak-ng truncates beyond %d: %s", len(dataDir), maxDataPath, dataDir)
	}
	h, err := dl.Open(libPath)
	if err != nil {
		return nil, fmt.Errorf("load espeak-ng: %w", err)
	}
	e := &ESpeak{}
	purego.RegisterLibFunc(&e.initialize, h, "espeak_Initialize")
	purego.RegisterLibFunc(&e.setVoiceByName, h, "espeak_SetVoiceByName")
	purego.RegisterLibFunc(&e.listVoices, h, "espeak_ListVoices")
	purego.RegisterLibFunc(&e.textToPhonemes, h, "espeak_TextToPhonemes")
	purego.RegisterLibFunc(&e.info, h, "espeak_Info")
	if rc := e.initialize(audioOutputSynchronous, 0, dataDir, 0); rc <= 0 {
		return nil, fmt.Errorf("espeak_Initialize failed (%d) with data %s", rc, dataDir)
	}
	var p uintptr
	e.Version = cstr(e.info(&p))
	return e, nil
}

// setLanguage mirrors phonemizer's EspeakWrapper.set_voice: pick the first voice whose
// primary language equals lang and select it by identifier.
func (e *ESpeak) setLanguage(lang string) error {
	if e.voice == lang {
		return nil
	}
	ident := ""
	arr := e.listVoices(0)
	for i := uintptr(0); ; i++ {
		vp := *(*uintptr)(unsafe.Pointer(arr + i*unsafe.Sizeof(uintptr(0))))
		if vp == 0 {
			break
		}
		// espeak_VOICE { const char *name; const char *languages; const char *identifier; ... }
		langs := *(*uintptr)(unsafe.Pointer(vp + unsafe.Sizeof(uintptr(0))))
		id := *(*uintptr)(unsafe.Pointer(vp + 2*unsafe.Sizeof(uintptr(0))))
		// languages = <priority byte><lang>\0...; phonemizer reads it as a C string and drops byte 0
		if l := cstr(langs); len(l) > 1 && l[1:] == lang {
			ident = cstr(id)
			break
		}
	}
	if ident == "" {
		return fmt.Errorf("espeak-ng has no voice for language %q", lang)
	}
	if rc := e.setVoiceByName(ident); rc != 0 {
		return fmt.Errorf("espeak_SetVoiceByName(%s) = %d", ident, rc)
	}
	e.voice = lang
	return nil
}

// rawPhonemes is EspeakWrapper.text_to_phonemes: call espeak_TextToPhonemes clause by
// clause until the text pointer is exhausted, joining non-empty results with a space.
func (e *ESpeak) rawPhonemes(text string) string {
	buf := append([]byte(text), 0)
	p := new(uintptr)
	*p = uintptr(unsafe.Pointer(&buf[0]))
	var out []string
	for *p != 0 {
		if r := cstr(e.textToPhonemes(p, charsUTF8, phonemeModeIPAUnderscore)); r != "" {
			out = append(out, r)
		}
	}
	runtime.KeepAlive(buf)
	return strings.Join(out, " ")
}

var reUnderscores = regexp.MustCompile(`_+`)

// postprocessLine is EspeakBackend._postprocess_line with with_stress=True, tie=False,
// strip=False, separator(phone="", word=" ") and the keep-flags language-switch policy.
func postprocessLine(line string) string {
	line = pyStrip(line)
	line = strings.ReplaceAll(line, "\n", " ")
	line = strings.ReplaceAll(line, "  ", " ")
	line = reUnderscores.ReplaceAllString(line, "_")
	line = strings.ReplaceAll(line, "_ ", " ")
	if line == "" {
		return ""
	}
	var b strings.Builder
	for _, w := range strings.Split(line, " ") {
		w = pyStrip(w) + "_"
		b.WriteString(strings.ReplaceAll(w, "_", ""))
		b.WriteByte(' ')
	}
	return b.String()
}

// Phonemize is phonemizer.phonemize(text, lang, backend="espeak", preserve_punctuation=True,
// with_stress=True) for a str input, using libespeak-ng in-process.
func (e *ESpeak) Phonemize(text, lang string) (string, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if err := e.setLanguage(lang); err != nil {
		return "", err
	}
	return phonemize(text, func(cs []string) ([]string, error) {
		out := make([]string, len(cs))
		for i, c := range cs {
			out[i] = e.rawPhonemes(c)
		}
		return out, nil
	})
}

// phonemize is the backend-independent part of phonemizer: per-line punctuation split,
// raw espeak IPA per chunk, postprocessing, punctuation restored, lines joined by "\n".
func phonemize(text string, raw func([]string) ([]string, error)) (string, error) {
	var lines []string
	for _, l := range strings.Split(strings.Trim(text, "\n"), "\n") {
		l = strings.Trim(l, "\n")
		if pyStrip(l) != "" {
			lines = append(lines, l)
		}
	}
	if len(lines) == 0 {
		return "", nil
	}
	chunks, marks := preserve(lines)
	ph, err := raw(chunks)
	if err != nil {
		return "", err
	}
	for i := range ph {
		ph[i] = postprocessLine(ph[i])
	}
	return strings.Join(restore(ph, marks), "\n"), nil
}

func cstr(p uintptr) string {
	if p == 0 {
		return ""
	}
	n := 0
	for *(*byte)(unsafe.Pointer(p + uintptr(n))) != 0 {
		n++
	}
	return string(unsafe.Slice((*byte)(unsafe.Pointer(p)), n))
}

// Raw is one espeak_TextToPhonemes pass over a punctuation-free chunk (for the helper).
func (e *ESpeak) Raw(chunk, lang string) (string, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if err := e.setLanguage(lang); err != nil {
		return "", err
	}
	return e.rawPhonemes(chunk), nil
}
