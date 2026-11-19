package espeak

import (
	"bufio"
	"fmt"
	"io"
	"os/exec"
	"strings"
	"sync"
)

// Helper talks to the separate GPL `speak-espeak` program (cmd/speak-espeak), which runs
// espeak_TextToPhonemes in its own process: exact kokoro-onnx parity, no GPL code here.
type Helper struct {
	Bin, Lib, DataDir string

	mu  sync.Mutex
	cmd *exec.Cmd
	in  io.WriteCloser
	out *bufio.Reader
}

func (h *Helper) Phonemize(text, lang string) (string, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.cmd == nil {
		h.cmd = exec.Command(h.Bin, h.Lib, h.DataDir)
		var err error
		if h.in, err = h.cmd.StdinPipe(); err != nil {
			return "", err
		}
		o, err := h.cmd.StdoutPipe()
		if err != nil {
			return "", err
		}
		h.out = bufio.NewReader(o)
		if err := h.cmd.Start(); err != nil {
			return "", fmt.Errorf("start %s: %w", h.Bin, err)
		}
	}
	return phonemize(text, func(cs []string) ([]string, error) {
		var b strings.Builder
		for _, c := range cs {
			b.WriteString(lang + "\t" + strings.ReplaceAll(c, "\n", " ") + "\n")
		}
		if _, err := io.WriteString(h.in, b.String()); err != nil {
			return nil, err
		}
		out := make([]string, len(cs))
		for i := range cs {
			l, err := h.out.ReadString('\n')
			if err != nil {
				return nil, fmt.Errorf("speak-espeak exited: %w", err)
			}
			out[i] = strings.TrimSuffix(l, "\n")
		}
		return out, nil
	})
}

func (h *Helper) Close() {
	if h.cmd != nil {
		h.in.Close()
		h.cmd.Wait()
	}
}
