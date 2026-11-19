package kokoro

import (
	"archive/zip"
	"encoding/binary"
	"fmt"
	"io"
	"math"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// voicesFile is voices-v1.0.bin: a NumPy .npz (zip of <voice>.npy), each a
// little-endian float32 array of shape (510, 1, 256) = one style vector per token count.
type voicesFile struct {
	z     *zip.ReadCloser
	files map[string]*zip.File
}

func openVoices(path string) (*voicesFile, error) {
	z, err := zip.OpenReader(path)
	if err != nil {
		return nil, fmt.Errorf("open voices %s: %w", path, err)
	}
	v := &voicesFile{z: z, files: map[string]*zip.File{}}
	for _, f := range z.File {
		if name, ok := strings.CutSuffix(f.Name, ".npy"); ok {
			v.files[name] = f
		}
	}
	return v, nil
}

func (v *voicesFile) names() []string {
	var n []string
	for k := range v.files {
		n = append(n, k)
	}
	sort.Strings(n)
	return n
}

var (
	reDescr = regexp.MustCompile(`'descr':\s*'([^']+)'`)
	reShape = regexp.MustCompile(`'shape':\s*\(([^)]*)\)`)
	reOrder = regexp.MustCompile(`'fortran_order':\s*(True|False)`)
)

// load reads one voice as rows of 256 floats (row i = style for i tokens).
func (v *voicesFile) load(name string) ([][]float32, error) {
	f, ok := v.files[name]
	if !ok {
		return nil, fmt.Errorf("unknown voice %q", name)
	}
	r, err := f.Open()
	if err != nil {
		return nil, err
	}
	defer r.Close()
	b, err := io.ReadAll(r)
	if err != nil {
		return nil, err
	}
	if len(b) < 10 || string(b[:6]) != "\x93NUMPY" {
		return nil, fmt.Errorf("%s: not a .npy", name)
	}
	var hlen, off int
	if b[6] == 1 {
		hlen, off = int(binary.LittleEndian.Uint16(b[8:10])), 10
	} else {
		hlen, off = int(binary.LittleEndian.Uint32(b[8:12])), 12
	}
	hdr := string(b[off : off+hlen])
	data := b[off+hlen:]
	if m := reDescr.FindStringSubmatch(hdr); m == nil || m[1] != "<f4" {
		return nil, fmt.Errorf("%s: want dtype <f4, header %q", name, hdr)
	}
	if m := reOrder.FindStringSubmatch(hdr); m != nil && m[1] == "True" {
		return nil, fmt.Errorf("%s: fortran order not supported", name)
	}
	m := reShape.FindStringSubmatch(hdr)
	if m == nil {
		return nil, fmt.Errorf("%s: no shape in %q", name, hdr)
	}
	var dims []int
	for _, s := range strings.Split(m[1], ",") {
		if s = strings.TrimSpace(s); s != "" {
			d, err := strconv.Atoi(s)
			if err != nil {
				return nil, err
			}
			dims = append(dims, d)
		}
	}
	if len(dims) != 3 || dims[1] != 1 {
		return nil, fmt.Errorf("%s: unexpected shape %v", name, dims)
	}
	rows, width := dims[0], dims[2]
	if len(data) < rows*width*4 {
		return nil, fmt.Errorf("%s: short data", name)
	}
	out := make([][]float32, rows)
	for i := range out {
		row := make([]float32, width)
		for j := range row {
			row[j] = math.Float32frombits(binary.LittleEndian.Uint32(data[(i*width+j)*4:]))
		}
		out[i] = row
	}
	return out, nil
}
