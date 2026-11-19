// Package ort is the small slice of ONNX Runtime the backends need, with two
// interchangeable bindings chosen at build time:
//
//	default:      github.com/yalue/onnxruntime_go  (cgo; dlopens libonnxruntime)
//	-tags purego: github.com/shota3506/onnxruntime-purego (no cgo; dlopens via purego)
package ort

// Tensor is a named input. Exactly one of I64/F32 is set.
type Tensor struct {
	Name  string
	Shape []int64
	I64   []int64
	F32   []float32
}

// Session runs one model and returns its first output as float32.
type Session interface {
	Run(inputs []Tensor) ([]float32, error)
	Close() error
}
