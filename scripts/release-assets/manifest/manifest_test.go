package manifest

import (
	"slices"
	"strings"
	"testing"
)

func TestTheManifestIsConsistent(t *testing.T) {
	if err := For("0.62.0").Validate(); err != nil {
		t.Fatal(err)
	}
}

// The file lists every release up to v0.61.0 shipped, so moving to the manifest changed no name.
func TestTheManifestNamesWhatReleasesShip(t *testing.T) {
	r := For("0.61.0")
	cases := []struct {
		what string
		got  []string
		want []string
	}{
		{"updates", Names(r.Updates()), []string{
			"Cmdr_0.61.0_universal.app.tar.gz", "Cmdr_0.61.0_aarch64.app.tar.gz", "Cmdr_0.61.0_x64.app.tar.gz",
		}},
		{"downloads", Names(r.Downloads()), []string{
			"Cmdr_0.61.0_aarch64.dmg", "Cmdr_0.61.0_x64.dmg", "Cmdr_0.61.0_universal.dmg", "Cmdr_0.61.0_universal.pkg",
		}},
		{"required binaries", Names(Required(r.Binaries())), []string{
			"Cmdr_0.61.0_universal.app.tar.gz", "Cmdr_0.61.0_aarch64.app.tar.gz", "Cmdr_0.61.0_x64.app.tar.gz",
			"Cmdr_0.61.0_aarch64.dmg", "Cmdr_0.61.0_x64.dmg", "Cmdr_0.61.0_universal.dmg",
		}},
		{"SBOMs", Names(r.SBOMs()), []string{
			"Cmdr_0.61.0_aarch64.rust.cdx.json", "Cmdr_0.61.0_x64.rust.cdx.json", "Cmdr_0.61.0_frontend.cdx.json",
		}},
	}
	for _, c := range cases {
		if !slices.Equal(c.got, c.want) {
			t.Errorf("%s = %v, want %v", c.what, c.got, c.want)
		}
	}
	var keys []string
	for _, f := range r.Updates() {
		keys = append(keys, f.UpdaterKey)
	}
	if want := []string{"darwin-universal", "darwin-aarch64", "darwin-x86_64"}; !slices.Equal(keys, want) {
		t.Errorf("updater keys = %v, want %v (the platform order of every latest.json so far)", keys, want)
	}
}

// Each Rust SBOM describes the builds of its arch plus the universal ones, and the frontend SBOM
// all of them: the bindings the `attest` job has always made.
func TestSBOMSubjects(t *testing.T) {
	r := For("0.61.0")
	want := map[string][]string{
		"macos-aarch64": {"Cmdr_0.61.0_universal.app.tar.gz", "Cmdr_0.61.0_aarch64.app.tar.gz", "Cmdr_0.61.0_aarch64.dmg", "Cmdr_0.61.0_universal.dmg", "Cmdr_0.61.0_universal.pkg"},
		"macos-x86_64":  {"Cmdr_0.61.0_universal.app.tar.gz", "Cmdr_0.61.0_x64.app.tar.gz", "Cmdr_0.61.0_x64.dmg", "Cmdr_0.61.0_universal.dmg", "Cmdr_0.61.0_universal.pkg"},
		"frontend":      Names(r.Binaries()),
	}
	for _, s := range r.SBOMs() {
		if got := Names(r.Subjects(s)); !slices.Equal(got, want[s.Slug()]) {
			t.Errorf("%s subjects = %v, want %v", s.Slug(), got, want[s.Slug()])
		}
	}
}

func TestPresentKeepsOptionalFilesOnlyWhenThere(t *testing.T) {
	r := For("0.61.0")
	all := Names(r.Files)
	got, err := Present(r.Downloads(), all)
	if err != nil || len(got) != 4 {
		t.Fatalf("Present with everything = %v, %v", Names(got), err)
	}
	noPkg := slices.DeleteFunc(slices.Clone(all), func(n string) bool { return strings.HasSuffix(n, ".pkg") })
	got, err = Present(r.Downloads(), noPkg)
	if err != nil || slices.Contains(Names(got), "Cmdr_0.61.0_universal.pkg") || len(got) != 3 {
		t.Fatalf("Present without the pkg = %v, %v", Names(got), err)
	}
	noDMG := slices.DeleteFunc(slices.Clone(all), func(n string) bool { return n == "Cmdr_0.61.0_x64.dmg" })
	if _, err := Present(r.Downloads(), noDMG); err == nil || !strings.Contains(err.Error(), "Cmdr_0.61.0_x64.dmg") {
		t.Fatalf("Present without a DMG should name it: %v", err)
	}
}

// windowsRows are what a Windows release adds (`docs/specs/windows-port-plan.md` task 52): rows
// and nothing else. Tests in this module use them to prove a new platform needs no schema change.
var windowsRows = []Asset{
	{OS: Windows, Arch: X86_64, Kind: NSIS, File: "Cmdr_{version}_x64-setup.exe", UpdaterKey: "windows-x86_64", Targets: []string{"x86_64-pc-windows-msvc"}},
	{OS: Windows, Arch: X86_64, Kind: MSIX, File: "Cmdr_{version}_x64.msix", Targets: []string{"x86_64-pc-windows-msvc"}},
	{OS: Windows, Arch: X86_64, Kind: RustSBOM, File: "Cmdr_{version}_windows_x64.rust.cdx.json", Targets: []string{"x86_64-pc-windows-msvc"}},
}

func TestAWindowsReleaseIsRowsAlone(t *testing.T) {
	r := Resolve("0.70.0", append(slices.Clone(Assets), windowsRows...))
	if err := r.Validate(); err != nil {
		t.Fatal(err)
	}
	setup := r.Files[len(Assets)]
	if !setup.Update() || !setup.Download() || setup.Signature() != "Cmdr_0.70.0_x64-setup.exe.sig" {
		t.Fatalf("the NSIS installer should be both a download and the update payload: %+v", setup)
	}
	if got := Names(r.Subjects(r.Files[len(r.Files)-1])); !slices.Equal(got, []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_x64.msix"}) {
		t.Fatalf("the Windows Rust SBOM describes %v", got)
	}
}

func TestValidateCatchesABuildWithoutAnSBOM(t *testing.T) {
	rows := append(slices.Clone(Assets), windowsRows[:2]...)
	err := Resolve("0.70.0", rows).Validate()
	if err == nil || !strings.Contains(err.Error(), "x86_64-pc-windows-msvc, which no Rust SBOM row describes") {
		t.Fatalf("a Windows build with no SBOM row passed: %v", err)
	}
}

func TestValidateCatchesDuplicatesAndBadRows(t *testing.T) {
	rows := append(slices.Clone(Assets),
		Asset{OS: MacOS, Arch: AArch64, Kind: DMG, File: "Cmdr_{version}_aarch64.dmg", Targets: []string{macARM}},
		Asset{OS: MacOS, Arch: AArch64, Kind: AppArchive, File: "Cmdr_{version}_other.app.tar.gz", UpdaterKey: "darwin-aarch64", Optional: true, Targets: []string{macARM}},
		Asset{OS: MacOS, Arch: AArch64, Kind: "zip", File: "Cmdr_aarch64.zip", Targets: []string{macARM}},
	)
	err := Resolve("0.62.0", rows).Validate()
	if err == nil {
		t.Fatal("a broken manifest passed")
	}
	for _, want := range []string{
		"Cmdr_0.62.0_aarch64.dmg is listed twice",
		"updater key darwin-aarch64 is on two files",
		"can't be optional",
		`unknown kind "zip"`,
		"Cmdr_aarch64.zip doesn't carry the version",
	} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("the error doesn't say %q:\n%v", want, err)
		}
	}
}
