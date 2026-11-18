package nsis

import (
	"bytes"
	"debug/pe"
	"encoding/binary"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// fakeInstaller writes a minimal PE32 the way an NSIS installer is laid out where it matters
// here: one `.rsrc` section holding a `VS_VERSIONINFO` (fixed product version plus a
// `ProductVersion` string), and an optional Authenticode directory size.
func fakeInstaller(t *testing.T, fixed [4]uint16, product string, signed bool) string {
	t.Helper()
	const rsrcRVA, rsrcFile = 0x1000, 0x200

	var block bytes.Buffer
	block.Write(make([]byte, 6)) // wLength, wValueLength, wType: not read
	block.Write(utf16Bytes("VS_VERSION_INFO"))
	block.Write(make([]byte, align4(block.Len())-block.Len()))
	info := make([]byte, 52)
	binary.LittleEndian.PutUint32(info[0:], 0xFEEF04BD)
	binary.LittleEndian.PutUint32(info[16:], uint32(fixed[0])<<16|uint32(fixed[1]))
	binary.LittleEndian.PutUint32(info[20:], uint32(fixed[2])<<16|uint32(fixed[3]))
	block.Write(info)
	block.Write(make([]byte, 6))
	block.Write(utf16Bytes("ProductName"))
	block.Write(make([]byte, align4(block.Len())-block.Len()))
	block.Write(utf16Bytes("Cmdr"))
	block.Write(make([]byte, align4(block.Len())-block.Len()))
	if product != "" {
		block.Write(make([]byte, 6))
		block.Write(utf16Bytes("ProductVersion"))
		block.Write(make([]byte, align4(block.Len())-block.Len()))
		block.Write(utf16Bytes(product))
	}

	rsrc := make([]byte, 0x58)
	dir := func(at, id, target uint32) {
		binary.LittleEndian.PutUint16(rsrc[at+14:], 1)
		binary.LittleEndian.PutUint32(rsrc[at+16:], id)
		binary.LittleEndian.PutUint32(rsrc[at+20:], target)
	}
	dir(0x00, rtVersion, 0x80000000|0x18)
	dir(0x18, 1, 0x80000000|0x30)
	dir(0x30, 0x409, 0x48)
	binary.LittleEndian.PutUint32(rsrc[0x48:], rsrcRVA+0x58)
	binary.LittleEndian.PutUint32(rsrc[0x4c:], uint32(block.Len()))
	rsrc = append(rsrc, block.Bytes()...)

	var out bytes.Buffer
	dos := make([]byte, 0x40)
	copy(dos, "MZ")
	binary.LittleEndian.PutUint32(dos[0x3c:], 0x40)
	out.Write(dos)
	out.WriteString("PE\x00\x00")
	must(t, binary.Write(&out, binary.LittleEndian, pe.FileHeader{
		Machine: pe.IMAGE_FILE_MACHINE_I386, NumberOfSections: 1, SizeOfOptionalHeader: 224, Characteristics: 0x102,
	}))
	oh := pe.OptionalHeader32{Magic: 0x10b, SectionAlignment: 0x1000, FileAlignment: 0x200, NumberOfRvaAndSizes: 16}
	if signed {
		oh.DataDirectory[pe.IMAGE_DIRECTORY_ENTRY_SECURITY] = pe.DataDirectory{VirtualAddress: 0x400, Size: 0x100}
	}
	must(t, binary.Write(&out, binary.LittleEndian, oh))
	must(t, binary.Write(&out, binary.LittleEndian, pe.SectionHeader32{
		Name: [8]uint8{'.', 'r', 's', 'r', 'c'}, VirtualSize: uint32(len(rsrc)), VirtualAddress: rsrcRVA,
		SizeOfRawData: uint32(len(rsrc)), PointerToRawData: rsrcFile,
	}))
	out.Write(make([]byte, rsrcFile-out.Len()))
	out.Write(rsrc)

	path := filepath.Join(t.TempDir(), "Cmdr_0.70.0_x64-setup.exe")
	must(t, os.WriteFile(path, out.Bytes(), 0o644))
	return path
}

func expect(version string) Expect {
	return Expect{Version: version, ProductName: "Cmdr", Publisher: "Test Publisher AB"}
}

func TestVetNSISPassesThisReleasesStub(t *testing.T) {
	must(t, vetStub(fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.0", false), expect("0.70.0")))
}

// Each way an installer's stub can be some other build than the one its signature names.
func TestVetNSISRefusesAnotherBuildsStub(t *testing.T) {
	cases := []struct {
		name    string
		path    string
		version string
		want    string
	}{
		{"an older build", fakeInstaller(t, [4]uint16{0, 69, 0, 0}, "0.69.0", false), "0.70.0", "says 0.69.0.0, not 0.70.0.0"},
		{"a string that disagrees", fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.1", false), "0.70.0", `ProductVersion is "0.70.1"`},
		{"no version string", fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "", false), "0.70.0", "no ProductVersion string"},
		{"an Authenticode signature nothing pins", fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.0", true), "0.70.0", "task 50"},
	}
	for _, c := range cases {
		err := Vet(c.path, expect(c.version))
		if err == nil || !strings.Contains(err.Error(), c.want) {
			t.Errorf("%s: got %v, want an error saying %q", c.name, err, c.want)
		}
	}
	notPE := filepath.Join(t.TempDir(), "setup.exe")
	must(t, os.WriteFile(notPE, []byte("not a program"), 0o644))
	if err := Vet(notPE, expect("0.70.0")); err == nil || !strings.Contains(err.Error(), "isn't a Windows executable") {
		t.Errorf("a non-PE file: %v", err)
	}
	other := expect("0.70.0")
	other.ProductName = "Other"
	if err := vetStub(fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.0", false), other); err == nil || !strings.Contains(err.Error(), `ProductName is "Cmdr", not "Other"`) {
		t.Errorf("another product's installer: %v", err)
	}
}

// A prerelease's fixed version is its three numbers; its string is the whole version.
func TestVetNSISReadsAPrereleaseVersion(t *testing.T) {
	must(t, vetStub(fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.0-beta.1", false), expect("0.70.0-beta.1")))
}

// Real NSIS installers from `testdata/gen.sh` (makensis, Tauri's layout: solid LZMA, Unicode):
// the payload's version resource and the uninstall entry are read without running anything.
func TestVetNSISReadsThePayloadAndTheUninstallEntry(t *testing.T) {
	must(t, Vet("testdata/good-0.70.0-setup.exe", expect("0.70.0")))
	cases := []struct {
		name, file string
		want       Expect
		says       string
	}{
		{"an app of another version inside", "stale-app-0.70.0-setup.exe", expect("0.70.0"), "packs a Cmdr of version 0.69.0"},
		{"an uninstall entry naming another publisher", "other-publisher-0.70.0-setup.exe", expect("0.70.0"), `never writes "Test Publisher AB"`},
		{"an app by another publisher", "good-0.70.0-setup.exe", Expect{Version: "0.70.0", ProductName: "Cmdr", Publisher: "Someone Else"}, `published by "Test Publisher AB", not "Someone Else"`},
	}
	for _, c := range cases {
		err := Vet(filepath.Join("testdata", c.file), c.want)
		if err == nil || !strings.Contains(err.Error(), c.says) {
			t.Errorf("%s: got %v, want an error saying %q", c.name, err, c.says)
		}
	}
	// A stub that's right with nothing packed behind it (the fake above) isn't an installer.
	if err := Vet(fakeInstaller(t, [4]uint16{0, 70, 0, 0}, "0.70.0", false), expect("0.70.0")); err == nil || !strings.Contains(err.Error(), "isn't an NSIS installer") {
		t.Errorf("a bare stub: %v", err)
	}
}

func TestExpectFromRepoReadsTheTauriConfig(t *testing.T) {
	got, err := ExpectFromRepo("../../..", "0.70.0")
	must(t, err)
	if got.ProductName != "Cmdr" || got.Publisher == "" || got.Version != "0.70.0" {
		t.Fatalf("ExpectFromRepo = %+v", got)
	}
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}
