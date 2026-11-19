package main

import (
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/peetzweg/speak/spikes/go/internal/kokoro"
	"github.com/peetzweg/speak/spikes/go/internal/tts"
)

// benchG2P (SPEAK_BENCH_G2P=N): phonemize text N times with the configured G2P and print
// the first call (includes subprocess start) and the mean of the rest.
func benchG2P(b tts.Backend, txt string) error {
	n, _ := strconv.Atoi(os.Getenv("SPEAK_BENCH_G2P"))
	k := b.(*kokoro.Kokoro)
	var first, rest time.Duration
	for i := 0; i < n; i++ {
		t := time.Now()
		if _, err := k.Phonemize(txt, "en-us"); err != nil {
			return err
		}
		if i == 0 {
			first = time.Since(t)
		} else {
			rest += time.Since(t)
		}
	}
	fmt.Fprintf(os.Stderr, "g2p=%s first_call_ms=%.2f mean_ms=%.3f (n=%d)\n", os.Getenv("SPEAK_G2P"),
		float64(first.Microseconds())/1000, float64(rest.Microseconds())/1000/float64(max(1, n-1)), n)
	return nil
}
