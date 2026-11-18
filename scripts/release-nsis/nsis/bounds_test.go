package nsis

import (
	"bytes"
	"encoding/binary"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/ulikunitz/xz/lzma"
)

// fakeData writes an NSIS-shaped file: 512 bytes of stub, the first header, then an LZMA stream
// (properties first, no size) of the given decompressed bytes.
func fakeData(t *testing.T, decompressed []byte) string {
	t.Helper()
	var compressed bytes.Buffer
	w, err := lzma.WriterConfig{EOSMarker: true}.NewWriter(&compressed)
	must(t, err)
	_, err = w.Write(decompressed)
	must(t, err)
	must(t, w.Close())
	stream := compressed.Bytes()
	// The classic header is properties (5 bytes) then the size (8); NSIS keeps only the first.
	stream = append(stream[:5:5], stream[13:]...)

	var out bytes.Buffer
	out.Write(make([]byte, 512))
	head := make([]byte, firstHeaderLen)
	binary.LittleEndian.PutUint32(head[4:], firstHeaderSig)
	copy(head[8:], nullsoftInst)
	out.Write(head)
	out.Write(stream)
	path := filepath.Join(t.TempDir(), "setup.exe")
	must(t, os.WriteFile(path, out.Bytes(), 0o644))
	return path
}

func blocks(sizes ...uint32) []byte {
	var b bytes.Buffer
	for _, s := range sizes {
		_ = binary.Write(&b, binary.LittleEndian, s) // a bytes.Buffer write can't fail
		b.Write(bytes.Repeat([]byte{'x'}, int(s)))
	}
	return b.Bytes()
}

// A block claiming a gigabyte with a few bytes behind it allocates nothing up front: the stream
// ends, and the header still reads.
func TestAHostileBlockLengthAllocatesNothing(t *testing.T) {
	data := append(blocks(16), 0, 0, 0, 0x3f) // the next block claims 1 GiB - 1 B…
	data = append(data, []byte("short")...)   // …and holds five
	p, err := readPayload(fakeData(t, data), "Cmdr")
	if err != nil || len(p.header) != 16 {
		t.Fatalf("header %d bytes, %v", len(p.header), err)
	}
}

// Past the total cap or the time limit, the vet refuses rather than reading on.
func TestDecompressionIsBoundedInBytesAndTime(t *testing.T) {
	saved := limits
	t.Cleanup(func() { limits = saved })

	path := fakeData(t, blocks(16, 4096, 4096, 4096))
	limits.total = 8 << 10
	if _, err := readPayload(path, "Cmdr"); !errors.Is(err, errTooMuch) {
		t.Fatalf("past the byte cap: %v", err)
	}
	limits = saved
	limits.time = -time.Second
	if _, err := readPayload(path, "Cmdr"); !errors.Is(err, errTooSlow) {
		t.Fatalf("past the time limit: %v", err)
	}
	limits = saved
	if _, err := readPayload(path, "Cmdr"); err != nil {
		t.Fatalf("within both: %v", err)
	}
}
