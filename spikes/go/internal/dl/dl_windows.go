//go:build windows

// Package dl opens shared libraries at runtime without cgo (via purego).
package dl

import "syscall"

// Open loads a DLL and returns its handle (untested in the spike).
func Open(path string) (uintptr, error) {
	h, err := syscall.LoadLibrary(path)
	return uintptr(h), err
}
