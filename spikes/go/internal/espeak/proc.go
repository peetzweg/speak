package espeak

import (
	"bufio"
	"fmt"
	"io"
	"os/exec"
	"strings"
	"sync"
)

// Proc runs the `espeak-ng` program as a separate process (no GPL code in our binary).
// Mode "persistent" keeps one `espeak-ng -q --ipa=1 -v LANG` per language and feeds it
// one chunk per stdin line (the CLI flushes stdout after each line); mode "spawn" runs
// one process per chunk.
type Proc struct {
	Bin, DataDir string
	Spawn        bool // one process per chunk instead of a long-lived one

	mu    sync.Mutex
	procs map[string]*proc
}

type proc struct {
	cmd      *exec.Cmd
	in       io.WriteCloser
	out      *bufio.Reader
	sentinel string // the phoneme line espeak prints for our sentinel word
}

const sentinelWord = "zzqxjsentinel"

// --ipa=1 is mode 0x02 with '_' separators: exactly what phonemizer asks espeak_TextToPhonemes for.
func (p *Proc) args(lang string) []string {
	a := []string{"-q", "--ipa=1", "-v", lang}
	if p.DataDir != "" {
		a = append(a, "--path="+p.DataDir)
	}
	return a
}

// Phonemize is phonemizer.phonemize via the espeak-ng program.
func (p *Proc) Phonemize(text, lang string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return phonemize(text, func(cs []string) ([]string, error) { return p.rawAll(cs, lang) })
}

func joinClauses(out string) string {
	var parts []string
	for _, l := range strings.Split(out, "\n") {
		if l != "" { // TextToPhonemes results are joined only when non-empty
			parts = append(parts, l)
		}
	}
	return strings.Join(parts, " ")
}

// rawAll phonemizes the chunks of one text. In persistent mode all chunks go down the
// pipe in one write, each followed by a sentinel line, so a sentence costs one round trip.
func (p *Proc) rawAll(chunks []string, lang string) ([]string, error) {
	out := make([]string, len(chunks))
	var piped []int
	var buf strings.Builder
	for i, c := range chunks {
		// The CLI's line mode reads with fgets(1000): longer chunks would be split mid-text.
		if p.Spawn || len(c) >= 999 || strings.ContainsAny(c, "\r\n") {
			cmd := exec.Command(p.Bin, append(p.args(lang), c)...)
			cmd.Stderr = nil // distro builds print ALSA noise even with -q; keep agents' stderr clean
			b, err := cmd.Output()
			if err != nil {
				return nil, fmt.Errorf("espeak-ng: %w", err)
			}
			out[i] = joinClauses(string(b))
			continue
		}
		piped = append(piped, i)
		buf.WriteString(c + "\n" + sentinelWord + "\n")
	}
	if len(piped) == 0 {
		return out, nil
	}
	pr, err := p.get(lang)
	if err != nil {
		return nil, err
	}
	if _, err := io.WriteString(pr.in, buf.String()); err != nil {
		return nil, err
	}
	for _, i := range piped {
		var lines []string
		for {
			l, err := pr.out.ReadString('\n')
			if err != nil {
				return nil, fmt.Errorf("espeak-ng exited: %w", err)
			}
			if l = strings.TrimSuffix(l, "\n"); l == pr.sentinel {
				break
			}
			lines = append(lines, l)
		}
		out[i] = joinClauses(strings.Join(lines, "\n"))
	}
	return out, nil
}

func (p *Proc) get(lang string) (*proc, error) {
	if pr, ok := p.procs[lang]; ok {
		return pr, nil
	}
	cmd := exec.Command(p.Bin, p.args(lang)...)
	cmd.Stderr = nil // nil = /dev/null (see above)
	in, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	outPipe, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("start espeak-ng (%s): %w", p.Bin, err)
	}
	pr := &proc{cmd: cmd, in: in, out: bufio.NewReader(outPipe)}
	// learn what the sentinel looks like in this language
	if _, err := io.WriteString(in, sentinelWord+"\n"); err != nil {
		return nil, err
	}
	l, err := pr.out.ReadString('\n')
	if err != nil {
		return nil, fmt.Errorf("espeak-ng handshake: %w", err)
	}
	pr.sentinel = strings.TrimSuffix(l, "\n")
	if p.procs == nil {
		p.procs = map[string]*proc{}
	}
	p.procs[lang] = pr
	return pr, nil
}

// Close stops the long-lived processes.
func (p *Proc) Close() {
	for _, pr := range p.procs {
		pr.in.Close()
		pr.cmd.Wait()
	}
}
