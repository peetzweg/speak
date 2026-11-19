//go:build darwin || linux || freebsd

// Package dl opens shared libraries at runtime without cgo (via purego).
package dl

import "github.com/ebitengine/purego"

// Open dlopens a shared library and returns its handle.
func Open(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_GLOBAL)
}
