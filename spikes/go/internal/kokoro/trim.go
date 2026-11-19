package kokoro

import "math"

// trim is kokoro_onnx.trim.trim (librosa.effects.trim) with its defaults:
// top_db=60, ref=max, frame_length=2048, hop_length=512, centred RMS frames.
// librosa averages in float32; this accumulates in float64, which can only matter
// for a frame sitting exactly on the -60 dB threshold.
func trim(y []float32) []float32 {
	const frame, hop, topDB = 2048, 512, 60.0
	n := len(y)
	if n == 0 {
		return y
	}
	nFrames := 1 + n/hop
	rms := make([]float64, nFrames)
	maxRMS := 0.0
	for j := range rms {
		sum := 0.0
		for k := j*hop - frame/2; k < j*hop+frame/2; k++ {
			if k >= 0 && k < n {
				v := float64(y[k])
				sum += v * v
			}
		}
		r := float64(float32(math.Sqrt(float64(float32(sum / frame)))))
		rms[j] = r
		maxRMS = math.Max(maxRMS, r)
	}
	ref := 10 * math.Log10(math.Max(1e-10, maxRMS*maxRMS))
	first, last := -1, -1
	for j, r := range rms {
		if 10*math.Log10(math.Max(1e-10, r*r))-ref > -topDB {
			if first < 0 {
				first = j
			}
			last = j
		}
	}
	if first < 0 {
		return y[:0]
	}
	return y[first*hop : min(n, (last+1)*hop)]
}
