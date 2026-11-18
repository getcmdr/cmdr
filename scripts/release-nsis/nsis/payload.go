package nsis

import (
	"bytes"
	"debug/pe"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/ulikunitz/xz/lzma"
)

// An NSIS installer is its stub (a PE) followed, at a 512-byte boundary, by a 28-byte "first
// header" and the compressed data. Tauri's template compresses with `SetCompressor /SOLID lzma`, so
// the data is one LZMA stream (5 property bytes, then the raw stream). Decompressed, it's a run
// of blocks, each a 4-byte little-endian length and that many bytes: first the script's header
// (its instructions and string table), then each packed file in the order the script names them.
// Reading it runs nothing: the installer's code is the stub, which this never executes.
// (Verified on tauri-cli 2.12's `Cmdr_0.61.0_x64-setup.exe`, NSIS 3 Unicode, against `7z l`,
// 2026-11-18.)

const (
	firstHeaderSig = 0xDEADBEEF
	firstHeaderLen = 28
	// maxBlock bounds one decompressed block, the size check a hostile length can't pass: the app
	// itself, the largest file, is about 150 MiB.
	maxBlock = 1 << 30
)

var nullsoftInst = []byte("NullsoftInst")

// payload is what `readPayload` found in the installer's data.
type payload struct {
	// header is the script's header block, its string table included.
	header []byte
	// apps are the version resources of the packed PE files whose ProductName is Cmdr's.
	apps []versionInfo
}

// versionInfo is the part of a version resource the vet reads.
type versionInfo struct {
	fixed, product, name, company string
}

func readVersionInfo(f *pe.File) (versionInfo, error) {
	block, err := versionResource(f)
	if err != nil {
		return versionInfo{}, err
	}
	fixed, err := fixedProductVersion(block)
	if err != nil {
		return versionInfo{}, err
	}
	product, _ := versionString(block, "ProductVersion")
	name, _ := versionString(block, "ProductName")
	company, _ := versionString(block, "CompanyName")
	return versionInfo{fixed: fixed, product: product, name: name, company: company}, nil
}

// readPayload decompresses the installer's data and keeps its header and the version resources
// of the packed files named productName. It holds one block in memory at a time.
func readPayload(path, productName string) (payload, error) {
	file, err := os.Open(path)
	if err != nil {
		return payload{}, err
	}
	defer func() { _ = file.Close() }()
	start, err := findFirstHeader(file)
	if err != nil {
		return payload{}, err
	}
	if _, err := file.Seek(start+firstHeaderLen, io.SeekStart); err != nil {
		return payload{}, err
	}
	props := make([]byte, 5)
	if _, err := io.ReadFull(file, props); err != nil {
		return payload{}, fmt.Errorf("its data is cut short: %w", err)
	}
	if props[0] != 0x5D {
		return payload{}, errors.New("its data isn't one solid LZMA stream, which Tauri's NSIS template makes")
	}
	// The classic `.lzma` header the decoder reads: the properties, then an unknown size.
	head := append(props, bytes.Repeat([]byte{0xFF}, 8)...)
	stream, err := lzma.ReaderConfig{DictCap: 1 << 27}.NewReader(io.MultiReader(bytes.NewReader(head), file))
	if err != nil {
		return payload{}, fmt.Errorf("its LZMA stream doesn't open: %w", err)
	}

	var out payload
	for i := 0; ; i++ {
		block, err := nextBlock(stream)
		if err != nil {
			if i == 0 {
				return payload{}, fmt.Errorf("its script header doesn't read: %w", err)
			}
			// The stream ends without a marker after the last block (and a CRC may follow it), so
			// the first block that doesn't read whole is the end.
			return out, nil
		}
		if i == 0 {
			out.header = block
			continue
		}
		if info, ok := packedApp(block, productName); ok {
			out.apps = append(out.apps, info)
		}
	}
}

func nextBlock(r io.Reader) ([]byte, error) {
	var size uint32
	if err := binary.Read(r, binary.LittleEndian, &size); err != nil {
		return nil, err
	}
	if size > maxBlock {
		return nil, fmt.Errorf("a block claims %d bytes", size)
	}
	block := make([]byte, size)
	if _, err := io.ReadFull(r, block); err != nil {
		return nil, err
	}
	return block, nil
}

// packedApp reads a packed file's version resource if it's a PE named productName.
func packedApp(block []byte, productName string) (versionInfo, bool) {
	if len(block) < 2 || block[0] != 'M' || block[1] != 'Z' {
		return versionInfo{}, false
	}
	f, err := pe.NewFile(bytes.NewReader(block))
	if err != nil {
		return versionInfo{}, false
	}
	defer func() { _ = f.Close() }()
	info, err := readVersionInfo(f)
	if err != nil || info.name != productName {
		return versionInfo{}, false
	}
	return info, true
}

// findFirstHeader finds the first header, which NSIS looks for at each 512-byte boundary after
// the stub.
func findFirstHeader(f *os.File) (int64, error) {
	buf := make([]byte, firstHeaderLen)
	for at := int64(512); ; at += 512 {
		if _, err := f.ReadAt(buf, at); err != nil {
			return 0, errors.New("isn't an NSIS installer: no NSIS header after the stub")
		}
		if binary.LittleEndian.Uint32(buf[4:]) == firstHeaderSig && bytes.Equal(buf[8:20], nullsoftInst) {
			return at, nil
		}
	}
}

// headerHasString reports whether the script header's string table holds s, as NSIS 3 Unicode
// stores a literal: UTF-16LE with a terminating NUL, on an even offset. Not necessarily at a
// string's start: NSIS stores a string that's the tail of another once, inside it (the real
// installer keeps "Rymdskottkärra AB" as the end of a longer one).
func headerHasString(header []byte, s string) bool {
	needle := utf16Bytes(s)
	for from := 0; ; {
		i := bytes.Index(header[from:], needle)
		if i < 0 {
			return false
		}
		if (from+i)%2 == 0 {
			return true
		}
		from += i + 1
	}
}
