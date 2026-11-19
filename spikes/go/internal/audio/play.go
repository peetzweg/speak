package audio

import (
	"encoding/binary"
	"errors"
	"io"
	"math"
	"sync"
	"time"

	"github.com/ebitengine/oto/v3"
)

// Stream plays float32 chunks as they arrive: Push from the synthesis goroutine,
// Close when done, Wait for the device to drain.
type Stream struct {
	ch        chan []float32
	cur       []byte
	player    *oto.Player
	firstOnce sync.Once
	First     chan time.Time // receives the moment the first samples reach oto
	pushed    int            // samples queued
	start     time.Time      // first Push
	rate      int
}

var (
	ctxOnce sync.Once
	otoCtx  *oto.Context
	ctxErr  error
	ctxRate int
)

// NewStream opens the default output device (once per process) at rate Hz mono.
func NewStream(rate int) (*Stream, error) {
	ctxOnce.Do(func() {
		// oto can block forever when there is no audio server/device (seen in a
		// headless Linux container), so give up after a few seconds.
		done := make(chan struct{})
		go func() {
			var ready chan struct{}
			otoCtx, ready, ctxErr = oto.NewContext(&oto.NewContextOptions{
				SampleRate: rate, ChannelCount: 1, Format: oto.FormatFloat32LE,
				BufferSize: 40 * time.Millisecond,
			})
			if ctxErr == nil {
				<-ready
			}
			close(done)
		}()
		select {
		case <-done:
		case <-time.After(3 * time.Second):
			ctxErr = errors.New("no audio output device answered within 3s (use -o FILE)")
		}
		ctxRate = rate
	})
	if ctxErr != nil {
		return nil, ctxErr
	}
	s := &Stream{ch: make(chan []float32, 8), First: make(chan time.Time, 1), rate: rate}
	s.player = otoCtx.NewPlayer(s)
	s.player.Play()
	return s, nil
}

// Push queues samples for playback (blocks if 8 chunks are already queued).
func (s *Stream) Push(pcm []float32) {
	if s.start.IsZero() {
		s.start = time.Now()
	}
	s.pushed += len(pcm)
	s.ch <- pcm
}

// Close marks the end of the stream.
func (s *Stream) Close() { close(s.ch) }

// Stop aborts playback immediately.
func (s *Stream) Stop() { s.player.Pause() }

// Wait blocks until everything pushed has been played. It fails if playback takes
// 3s longer than the audio itself: a "device" that never drains (headless Linux with
// a dummy sink) must not hang a script forever.
func (s *Stream) Wait() error {
	deadline := s.start.Add(time.Duration(float64(s.pushed)/float64(s.rate)*float64(time.Second)) + 3*time.Second)
	for s.player.IsPlaying() {
		if time.Now().After(deadline) {
			s.player.Pause()
			return errors.New("audio device is not playing (stalled); use -o FILE")
		}
		time.Sleep(10 * time.Millisecond)
	}
	return nil
}

// Read implements io.Reader for oto; it blocks until the next chunk arrives.
func (s *Stream) Read(p []byte) (int, error) {
	for len(s.cur) == 0 {
		pcm, ok := <-s.ch
		if !ok {
			return 0, io.EOF
		}
		s.cur = make([]byte, len(pcm)*4)
		for i, v := range pcm {
			binary.LittleEndian.PutUint32(s.cur[i*4:], math.Float32bits(v))
		}
	}
	n := copy(p, s.cur)
	s.cur = s.cur[n:]
	s.firstOnce.Do(func() { s.First <- time.Now() })
	return n, nil
}
