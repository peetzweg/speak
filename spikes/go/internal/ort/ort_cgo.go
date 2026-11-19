//go:build !purego

package ort

import (
	"fmt"
	"sync"

	ort "github.com/yalue/onnxruntime_go"
)

// Binding names the ONNX Runtime binding compiled in.
const Binding = "yalue/onnxruntime_go (cgo)"

var envOnce sync.Once
var envErr error

type cgoSession struct {
	s *ort.DynamicAdvancedSession
}

// Open loads libonnxruntime from libPath (once per process) and creates a session.
func Open(libPath, model string, inputs, outputs []string, threads int) (Session, error) {
	envOnce.Do(func() {
		ort.SetSharedLibraryPath(libPath)
		envErr = ort.InitializeEnvironment(ort.WithLogLevelError())
	})
	if envErr != nil {
		return nil, fmt.Errorf("onnxruntime init (%s): %w", libPath, envErr)
	}
	opts, err := ort.NewSessionOptions()
	if err != nil {
		return nil, err
	}
	defer opts.Destroy()
	if threads > 0 {
		if err := opts.SetIntraOpNumThreads(threads); err != nil {
			return nil, err
		}
	}
	s, err := ort.NewDynamicAdvancedSession(model, inputs, outputs, opts)
	if err != nil {
		return nil, err
	}
	return &cgoSession{s}, nil
}

func (c *cgoSession) Run(inputs []Tensor) ([]float32, error) {
	vals := make([]ort.Value, len(inputs))
	for i, t := range inputs {
		var v ort.Value
		var err error
		if t.I64 != nil {
			v, err = ort.NewTensor(ort.NewShape(t.Shape...), t.I64)
		} else {
			v, err = ort.NewTensor(ort.NewShape(t.Shape...), t.F32)
		}
		if err != nil {
			return nil, err
		}
		defer v.Destroy()
		vals[i] = v
	}
	outs := []ort.Value{nil}
	if err := c.s.Run(vals, outs); err != nil {
		return nil, err
	}
	defer outs[0].Destroy()
	t, ok := outs[0].(*ort.Tensor[float32])
	if !ok {
		return nil, fmt.Errorf("unexpected output type %T", outs[0])
	}
	return append([]float32(nil), t.GetData()...), nil
}

func (c *cgoSession) Close() error { return c.s.Destroy() }

// Version reports the loaded onnxruntime version.
func Version() string { return ort.GetVersion() }
