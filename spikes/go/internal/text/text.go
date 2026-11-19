// Package text is the model-independent front end: sentence chunking for streaming
// playback and the user's pronunciation lexicon (both ported from speak.py).
package text

import (
	"bufio"
	"iter"
	"os"
	"regexp"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"
)

var reWS = regexp.MustCompile(`\s+`)

// Sentences is speak.py's chunks(): collapse whitespace, split after . ! ? … followed by
// a space, then cut sentences longer than 400 chars at ", " (if past char 100) or at 400.
// RE2 has no look-behind, so the split is a manual scan.
func Sentences(s string) iter.Seq[string] {
	return func(yield func(string) bool) {
		s = strings.TrimSpace(reWS.ReplaceAllString(s, " "))
		var parts []string
		start := 0
		for i, r := range s {
			if strings.ContainsRune(".!?…", r) {
				j := i + utf8.RuneLen(r)
				if j < len(s) && s[j] == ' ' {
					parts = append(parts, s[start:j])
					start = j + 1
				}
			}
		}
		parts = append(parts, s[start:])
		for _, p := range parts {
			rs := []rune(p)
			for len(rs) > 400 {
				cut := strings.LastIndex(string(rs[:400]), ", ")
				if cut >= 0 {
					cut = utf8.RuneCountInString(string(rs[:400])[:cut])
				}
				if cut > 100 {
					cut++
				} else {
					cut = 400
				}
				if !yield(strings.TrimSpace(string(rs[:cut]))) {
					return
				}
				rs = []rune(strings.TrimSpace(string(rs[cut:])))
			}
			if len(rs) > 0 && !yield(string(rs)) {
				return
			}
		}
	}
}

// Lexicon maps a word to its spoken form; matching is whole-word and case-insensitive.
type Lexicon map[string]string

// LoadLexicon reads `word = spoken form` lines; # comments and blank lines are skipped.
func LoadLexicon(path string) (Lexicon, error) {
	lx := Lexicon{}
	f, err := os.Open(path)
	if os.IsNotExist(err) {
		return lx, nil
	} else if err != nil {
		return nil, err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		w, s, ok := strings.Cut(line, "=")
		if w, s = strings.TrimSpace(w), strings.TrimSpace(s); ok && w != "" && s != "" {
			lx[w] = s
		}
	}
	return lx, sc.Err()
}

func isWordish(r rune) bool { return r == '_' || r == '-' || unicode.IsLetter(r) || unicode.IsDigit(r) || unicode.Is(unicode.Mn, r) }

// Apply replaces entries longest-first, like speak.py's (?<![\w-])word(?![\w-]) with
// re.IGNORECASE. Boundaries are checked by hand because RE2 lacks look-around.
func (lx Lexicon) Apply(s string) string {
	words := make([]string, 0, len(lx))
	for w := range lx {
		words = append(words, w)
	}
	sort.SliceStable(words, func(i, j int) bool { return utf8.RuneCountInString(words[i]) > utf8.RuneCountInString(words[j]) })
	for _, w := range words {
		re := regexp.MustCompile(`(?i)` + regexp.QuoteMeta(w))
		var b strings.Builder
		pos := 0
		for pos <= len(s) {
			loc := re.FindStringIndex(s[pos:])
			if loc == nil {
				break
			}
			a, z := pos+loc[0], pos+loc[1]
			before, _ := utf8.DecodeLastRuneInString(s[:a])
			after, _ := utf8.DecodeRuneInString(s[z:])
			if (a == 0 || !isWordish(before)) && (z == len(s) || !isWordish(after)) {
				b.WriteString(s[pos:a])
				b.WriteString(lx[w])
				pos = z
				continue
			}
			_, n := utf8.DecodeRuneInString(s[a:])
			b.WriteString(s[pos : a+n])
			pos = a + n
		}
		b.WriteString(s[min(pos, len(s)):])
		s = b.String()
	}
	return s
}
