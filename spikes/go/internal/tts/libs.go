package tts

import (
	"os"
	"path/filepath"
	"runtime"
)

// LibName returns the platform file names for a shared library, most specific first.
func LibName(base string) []string {
	switch runtime.GOOS {
	case "darwin":
		return []string{"lib" + base + ".dylib", "lib" + base + ".1.dylib"}
	case "windows":
		return []string{base + ".dll", "lib" + base + ".dll"}
	default: // distro packages often ship only the SONAME (no -dev symlink)
		return []string{"lib" + base + ".so", "lib" + base + ".so.1"}
	}
}

// LibDirs is where speak looks for bundled libraries, relative to the executable:
// <prefix>/bin/speak -> <prefix>/lib (Homebrew/tarball layout), next to the binary
// (Windows zip), and lib/<goos>-<goarch> (this spike's layout).
func LibDirs() []string {
	exe, err := os.Executable()
	if err != nil {
		return nil
	}
	if r, err := filepath.EvalSymlinks(exe); err == nil {
		exe = r
	}
	d := filepath.Dir(exe)
	plat := runtime.GOOS + "-" + runtime.GOARCH
	return []string{filepath.Join(d, "..", "lib"), filepath.Join(d, "lib"), d,
		filepath.Join(d, "..", "lib", plat), filepath.Join(d, "lib", plat)}
}

// FindLib resolves a library: $env, then the bundled dirs, then the system loader's
// search path (bare name, so dlopen consults DYLD_*/LD_LIBRARY_PATH, ld.so.cache, PATH).
func FindLib(env string, names []string, dirs []string) string {
	if v := os.Getenv(env); v != "" {
		return v
	}
	for _, d := range dirs {
		for _, n := range names {
			p := filepath.Join(d, n)
			if _, err := os.Stat(p); err == nil {
				if abs, err := filepath.Abs(p); err == nil {
					return abs
				}
				return p
			}
		}
	}
	for _, d := range []string{"/opt/homebrew/lib", "/usr/local/lib"} {
		for _, n := range names {
			if p := filepath.Join(d, n); fileExists(p) {
				return p
			}
		}
	}
	return names[len(names)-1]
}

func fileExists(p string) bool { _, err := os.Stat(p); return err == nil }
