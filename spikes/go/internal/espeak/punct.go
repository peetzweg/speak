package espeak

import (
	"regexp"
	"strings"
	"unicode"
)

// Port of phonemizer.punctuation.Punctuation (preserve/restore) with the default marks.
// Quirks are kept on purpose (string search instead of match offsets, "B"/"E" decided
// by startswith/endswith), because the model was trained on what this code produces.

const defaultMarks = `;:,.!?¡¿—…"«»“”(){}[]`

// Python's \s in a str pattern is Unicode whitespace; RE2's \s is ASCII only.
const pyWS = `\t\n\v\f\r \x{1c}-\x{1f}\x{85}\x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}`

var marksRe = func() *regexp.Regexp {
	var cls strings.Builder
	for _, r := range defaultMarks {
		cls.WriteString(regexp.QuoteMeta(string(r)))
	}
	return regexp.MustCompile(`([` + pyWS + `]*[` + cls.String() + `]+[` + pyWS + `]*)+`)
}()

type markIndex struct {
	index    int
	mark     string
	position byte // 'B'egin, 'E'nd, 'I'nside, 'A'lone
}

func preserve(lines []string) ([]string, []markIndex) {
	var text []string
	var marks []markIndex
	for num, line := range lines {
		t, m := preserveLine(line, num)
		text = append(text, t...)
		marks = append(marks, m...)
	}
	out := text[:0]
	for _, t := range text {
		if t != "" {
			out = append(out, t)
		}
	}
	return out, marks
}

func preserveLine(line string, num int) ([]string, []markIndex) {
	matches := marksRe.FindAllString(line, -1)
	if len(matches) == 0 {
		return []string{line}, nil
	}
	if len(matches) == 1 && matches[0] == line {
		return nil, []markIndex{{num, line, 'A'}}
	}
	var marks []markIndex
	for i, m := range matches {
		pos := byte('I')
		if i == 0 && strings.HasPrefix(line, m) {
			pos = 'B'
		} else if i == len(matches)-1 && strings.HasSuffix(line, m) {
			pos = 'E'
		}
		marks = append(marks, markIndex{num, m, pos})
	}
	var out []string
	for _, m := range marks {
		// split = line.split(mark); prefix, suffix = split[0], mark.join(split[1:])
		if i := strings.Index(line, m.mark); i >= 0 {
			out = append(out, line[:i])
			line = line[i+len(m.mark):]
		} else {
			out = append(out, line)
			line = ""
		}
	}
	return append(out, line), marks
}

// restore is Punctuation.restore with sep.word=" " and strip=False.
func restore(text []string, marks []markIndex) []string {
	var out []string
	pos := 0
	for len(text) > 0 || len(marks) > 0 {
		switch {
		case len(marks) == 0:
			for _, l := range text {
				if !strings.HasSuffix(l, " ") {
					l += " "
				}
				out = append(out, l)
			}
			text = nil
		case len(text) == 0:
			var b strings.Builder
			for _, m := range marks {
				b.WriteString(m.mark)
			}
			out = append(out, b.String())
			marks = nil
		default:
			cur := marks[0]
			if cur.index != pos {
				out = append(out, text[0])
				text = text[1:]
				pos++
				continue
			}
			marks = marks[1:]
			mark := cur.mark
			text[0] = strings.TrimSuffix(text[0], " ")
			sp := " "
			if strings.HasSuffix(mark, " ") {
				sp = ""
			}
			switch cur.position {
			case 'B':
				text[0] = mark + text[0]
			case 'E':
				out = append(out, text[0]+mark+sp)
				text = text[1:]
				pos++
			case 'A':
				out = append(out, mark+sp)
				pos++
			default:
				if len(text) == 1 {
					text[0] += mark
				} else {
					first := text[0]
					text = text[1:]
					text[0] = first + mark + text[0]
				}
			}
		}
	}
	return out
}

// pyIsSpace matches Python's str.isspace closely enough for strip().
func pyIsSpace(r rune) bool { return unicode.IsSpace(r) || (r >= 0x1c && r <= 0x1f) }

func pyStrip(s string) string { return strings.TrimFunc(s, pyIsSpace) }

// PyStrip is Python's str.strip() (exported for the kokoro tokenizer).
func PyStrip(s string) string { return pyStrip(s) }
