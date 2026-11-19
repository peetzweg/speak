//go:build purego

package ort

import (
	"context"
	"fmt"
	"sync"

	ort "github.com/shota3506/onnxruntime-purego/onnxruntime"
)

// Binding names the ONNX Runtime binding compiled in.
const Binding = "shota3506/onnxruntime-purego (no cgo)"

var (
	once sync.Once
	rt   *ort.Runtime
	env  *ort.Env
	err0 error
)

type pgSession struct {
	s       *ort.Session
	outputs []string
}

// Open loads libonnxruntime from libPath (once per process) and creates a session.
func Open(libPath, model string, inputs, outputs []string, threads int) (Session, error) {
	once.Do(func() {
		rt, err0 = ort.NewRuntime(libPath, 23) // C API v23; newer libs serve older API versions
		if err0 == nil {
			env, err0 = rt.NewEnv("speak", ort.LoggingLevelError)
		}
	})
	if err0 != nil {
		return nil, fmt.Errorf("onnxruntime init (%s): %w", libPath, err0)
	}
	s, err := rt.NewSession(env, model, &ort.SessionOptions{IntraOpNumThreads: threads})
	if err != nil {
		return nil, err
	}
	return &pgSession{s, outputs}, nil
}

func (p *pgSession) Run(inputs []Tensor) ([]float32, error) {
	in := map[string]*ort.Value{}
	for _, t := range inputs {
		var v *ort.Value
		var err error
		if t.I64 != nil {
			v, err = ort.NewTensorValue(rt, t.I64, t.Shape)
		} else {
			v, err = ort.NewTensorValue(rt, t.F32, t.Shape)
		}
		if err != nil {
			return nil, err
		}
		defer v.Close()
		in[t.Name] = v
	}
	out, err := p.s.Run(context.Background(), in, ort.WithOutputNames(p.outputs...))
	if err != nil {
		return nil, err
	}
	for _, v := range out {
		defer v.Close()
	}
	data, _, err := ort.GetTensorData[float32](out[p.outputs[0]])
	return data, err
}

func (p *pgSession) Close() error { p.s.Close(); return nil }

// Version reports the loaded onnxruntime version.
func Version() string { return rt.GetVersionString() }
