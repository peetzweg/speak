// Package audio writes WAV files and plays PCM streams.
package audio

import (
	"encoding/binary"
	"io"
	"math"
	"os"
)

// WriteWAV writes mono float32 samples as 16-bit PCM WAV (what soundfile writes by default).
func WriteWAV(path string, pcm []float32, rate int) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return EncodeWAV(f, pcm, rate)
}

// EncodeWAV encodes mono float32 samples as 16-bit PCM WAV.
func EncodeWAV(w io.Writer, pcm []float32, rate int) error {
	n := uint32(len(pcm) * 2)
	h := []any{
		[]byte("RIFF"), 36 + n, []byte("WAVE"), []byte("fmt "), uint32(16), uint16(1), uint16(1),
		uint32(rate), uint32(rate * 2), uint16(2), uint16(16), []byte("data"), n,
	}
	for _, v := range h {
		if err := binary.Write(w, binary.LittleEndian, v); err != nil {
			return err
		}
	}
	buf := make([]byte, len(pcm)*2)
	for i, s := range pcm {
		v := math.Max(-1, math.Min(1, float64(s)))
		binary.LittleEndian.PutUint16(buf[i*2:], uint16(int16(math.Round(v*32767))))
	}
	_, err := w.Write(buf)
	return err
}

// WriteRawF32 writes little-endian float32 samples (for bit-level parity checks).
func WriteRawF32(path string, pcm []float32) error {
	buf := make([]byte, len(pcm)*4)
	for i, s := range pcm {
		binary.LittleEndian.PutUint32(buf[i*4:], math.Float32bits(s))
	}
	return os.WriteFile(path, buf, 0o644)
}
