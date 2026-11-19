// SPDX-License-Identifier: GPL-3.0-or-later
//
// speak-espeak is a tiny separate program that links libespeak-ng (GPL-3.0-or-later)
// and answers G2P requests for speak over stdin/stdout, so the speak binary itself stays
// Apache-2.0 and never loads espeak-ng. It calls espeak_TextToPhonemes exactly like
// kokoro-onnx/phonemizer, which the espeak-ng CLI cannot do (its trace runs CalcPitches,
// promoting a stress in clauses that have no primary stress).
//
// Protocol: request line "LANG\tCHUNK", response line "PHONEMES" (raw espeak IPA with '_').
package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	"github.com/peetzweg/speak/spikes/go/internal/espeak"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: speak-espeak LIBESPEAK DATA_DIR")
		os.Exit(2)
	}
	e, err := espeak.Open(os.Args[1], os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, "speak-espeak:", err)
		os.Exit(3)
	}
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 1<<20), 1<<20)
	out := bufio.NewWriter(os.Stdout)
	for in.Scan() {
		lang, chunk, _ := strings.Cut(in.Text(), "\t")
		ph, err := e.Raw(chunk, lang)
		if err != nil {
			fmt.Fprintln(os.Stderr, "speak-espeak:", err)
			os.Exit(1)
		}
		out.WriteString(ph + "\n")
		out.Flush()
	}
}
