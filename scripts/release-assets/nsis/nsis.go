package nsis

import (
	"bytes"
	"debug/pe"
	"encoding/binary"
	"errors"
	"fmt"
	"strings"
	"unicode/utf16"
)

// Vet is the Windows half of `vet-update-archives`: the NSIS installer an install's updater
// would run must be this release's. Off macOS the updater can't open an installer before it runs
// (`apps/desktop/src-tauri/src/updater/DETAILS.md` § Windows and Linux), so it trusts the version
// the signature names (`--app-version`); an installer built from another version but signed as
// this one would install that other version under this one's name. So the installer's own version
// resource (what Tauri's NSIS template stamps: `VIProductVersion`, `ProductVersion`) must be the
// release's.
//
// It also refuses an Authenticode-signed installer: nothing pins a signer yet (Windows port plan
// task 50, after D3 step 3's Azure Artifact Signing), and a signature nobody checks is one an
// attacker's build could carry too. Signing the direct download comes with that check.
func Vet(path, version string) error {
	f, err := pe.Open(path)
	if err != nil {
		return fmt.Errorf("isn't a Windows executable: %w", err)
	}
	defer func() { _ = f.Close() }()

	if size := securityDirectorySize(f); size != 0 {
		return fmt.Errorf("carries an Authenticode signature (%d bytes) that nothing checks yet: pin its signer first (Windows port plan task 50)", size)
	}
	block, err := versionResource(f)
	if err != nil {
		return err
	}
	fixed, err := fixedProductVersion(block)
	if err != nil {
		return err
	}
	if want := numericVersion(version); fixed != want {
		return fmt.Errorf("its version resource says %s, not %s", fixed, want)
	}
	product, ok := versionString(block, "ProductVersion")
	if !ok {
		return errors.New("its version resource has no ProductVersion string")
	}
	if product != version {
		return fmt.Errorf("its ProductVersion is %q, not %q", product, version)
	}
	return nil
}

// numericVersion is a semver's first three numbers plus a zero build number, the shape of a
// version resource's fixed product version (`0.62.0-beta.1` → `0.62.0.0`).
func numericVersion(version string) string {
	core, _, _ := strings.Cut(version, "-")
	core, _, _ = strings.Cut(core, "+")
	return core + ".0"
}

func securityDirectorySize(f *pe.File) uint32 {
	switch oh := f.OptionalHeader.(type) {
	case *pe.OptionalHeader32:
		return oh.DataDirectory[pe.IMAGE_DIRECTORY_ENTRY_SECURITY].Size
	case *pe.OptionalHeader64:
		return oh.DataDirectory[pe.IMAGE_DIRECTORY_ENTRY_SECURITY].Size
	}
	return 0
}

// rtVersion is `RT_VERSION`, the resource type of a `VS_VERSIONINFO` block.
const rtVersion = 16

// versionResource reads the first `RT_VERSION` resource's bytes: the resource tree's type level,
// then the first name and the first language under it.
func versionResource(f *pe.File) ([]byte, error) {
	rsrc := f.Section(".rsrc")
	if rsrc == nil {
		return nil, errors.New("has no resources, so no version resource")
	}
	data, err := rsrc.Data()
	if err != nil {
		return nil, fmt.Errorf("its resources don't read: %w", err)
	}
	offset, leaf, err := resourceEntry(data, 0, rtVersion)
	for level := 0; err == nil && !leaf && level < 2; level++ {
		offset, leaf, err = resourceEntry(data, offset, anyEntry)
	}
	switch {
	case err != nil:
		return nil, err
	case !leaf || int(offset)+8 > len(data):
		return nil, errors.New("its version resource tree is malformed")
	}
	rva := binary.LittleEndian.Uint32(data[offset:])
	size := binary.LittleEndian.Uint32(data[offset+4:])
	start := int64(rva) - int64(rsrc.VirtualAddress)
	if start < 0 || start+int64(size) > int64(len(data)) {
		return nil, errors.New("its version resource points outside the resource section")
	}
	return data[start : start+int64(size)], nil
}

// anyEntry asks `resourceEntry` for a directory's first entry, named or numbered.
const anyEntry = -1

// resourceEntry finds, in the resource directory at dir, the first entry with the numeric id
// wantID (`anyEntry`: the first of any), and answers where it points and whether that's a leaf
// (a data entry). Named entries come first in a directory, and never match an id.
func resourceEntry(data []byte, dir uint32, wantID int) (uint32, bool, error) {
	if int(dir)+16 > len(data) {
		return 0, false, errors.New("its resource directory is cut short")
	}
	named := int(binary.LittleEndian.Uint16(data[dir+12:]))
	ids := int(binary.LittleEndian.Uint16(data[dir+14:]))
	for i := range named + ids {
		at := int(dir) + 16 + 8*i
		if at+8 > len(data) {
			break
		}
		id := binary.LittleEndian.Uint32(data[at:])
		target := binary.LittleEndian.Uint32(data[at+4:])
		if wantID != anyEntry && (i < named || id != uint32(wantID)) {
			continue
		}
		return target &^ 0x80000000, target&0x80000000 == 0, nil
	}
	return 0, false, errors.New("has no version resource")
}

// fixedProductVersion reads `VS_FIXEDFILEINFO`'s product version, which sits after the block's
// header and its `VS_VERSION_INFO` key, aligned to four bytes.
func fixedProductVersion(block []byte) (string, error) {
	key := utf16Bytes("VS_VERSION_INFO")
	if len(block) < 6+len(key) || !bytes.Equal(block[6:6+len(key)], key) {
		return "", errors.New("its version resource isn't a VS_VERSIONINFO block")
	}
	at := align4(6 + len(key))
	if at+24 > len(block) || binary.LittleEndian.Uint32(block[at:]) != 0xFEEF04BD {
		return "", errors.New("its version resource has no fixed file info")
	}
	ms := binary.LittleEndian.Uint32(block[at+16:])
	ls := binary.LittleEndian.Uint32(block[at+20:])
	return fmt.Sprintf("%d.%d.%d.%d", ms>>16, ms&0xffff, ls>>16, ls&0xffff), nil
}

// versionString finds a `StringFileInfo` value by its key: the key's UTF-16 text and terminator,
// then the value from the next four-byte boundary up to its own terminator.
func versionString(block []byte, key string) (string, bool) {
	needle := utf16Bytes(key)
	for from := 0; ; {
		i := bytes.Index(block[from:], needle)
		if i < 0 {
			return "", false
		}
		at := from + i
		from = at + 2
		// A key starts on an even offset, after its struct's six-byte header.
		if at%2 != 0 || at < 6 {
			continue
		}
		var units []uint16
		for p := align4(at + len(needle)); p+1 < len(block); p += 2 {
			u := binary.LittleEndian.Uint16(block[p:])
			if u == 0 {
				return string(utf16.Decode(units)), true
			}
			units = append(units, u)
		}
		return "", false
	}
}

// utf16Bytes is s in UTF-16LE with its terminating NUL, as resource keys are stored.
func utf16Bytes(s string) []byte {
	var b []byte
	for _, u := range append(utf16.Encode([]rune(s)), 0) {
		b = binary.LittleEndian.AppendUint16(b, u)
	}
	return b
}

func align4(n int) int { return (n + 3) &^ 3 }
