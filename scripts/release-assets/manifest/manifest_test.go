package manifest

import (
	"maps"
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

// Windows' rows are wired and off: a release publishes macOS's files and nothing new, the
// advisory list included, until David adds Windows to `ShippedOSes`.
func TestWindowsShipsOnlyOnceTurnedOn(t *testing.T) {
	if !slices.Equal(ShippedOSes, []OS{MacOS}) {
		t.Fatalf("ShippedOSes = %v: turning Windows on is David's call (docs/guides/releasing.md § Windows releases); update this test with it", ShippedOSes)
	}
	r := For("0.70.0")
	if r.Ships(Windows) {
		t.Errorf("a release ships Windows files: %v", Names(r.Files))
	}
	if _, ok := r.AdvisoryList(); ok {
		t.Error("a macOS-only release ships the advisory list, which only Windows and Linux installs read")
	}
	if err := All("0.70.0").Validate(); err != nil {
		t.Fatalf("every row together: %v", err)
	}
}

func windowsRelease(t *testing.T) (Release, map[string]File) {
	t.Helper()
	r := ForOSes("0.70.0", MacOS, Windows)
	if err := r.Validate(); err != nil {
		t.Fatal(err)
	}
	byName := map[string]File{}
	for _, f := range r.Files {
		byName[f.Name] = f
	}
	return r, byName
}

func TestAWindowsReleaseIsRowsAlone(t *testing.T) {
	r, byName := windowsRelease(t)
	setup := byName["Cmdr_0.70.0_x64-setup.exe"]
	if !setup.Update() || !setup.Download() || setup.Signature() != "Cmdr_0.70.0_x64-setup.exe.sig" {
		t.Fatalf("the NSIS installer should be both a download and the update payload: %+v", setup)
	}
	if got := Names(r.Subjects(byName["Cmdr_0.70.0_windows_x64.rust.cdx.json"])); !slices.Equal(got, []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_x64.msix"}) {
		t.Fatalf("the Windows Rust SBOM describes %v", got)
	}
}

// The policy template is a download but no build; the advisory list is signed but neither.
func TestTheWindowsReleasesExtraFiles(t *testing.T) {
	r, byName := windowsRelease(t)
	policy := byName["cmdr-group-policy-0.70.0.zip"]
	if !policy.Download() || policy.Binary() || policy.Signed() {
		t.Fatalf("the Group Policy template is a download, not a build or a signed file: %+v", policy)
	}
	list, ok := r.AdvisoryList()
	if !ok || !list.Signed() || list.Download() || list.Binary() || list.Signature() != "Cmdr_0.70.0_security-advisories.json.sig" {
		t.Fatalf("a Windows release ships the signed advisory list: %+v, %v", list, ok)
	}
	if got, want := Names(r.Signed()), append(Names(r.Updates()), list.Name); !slices.Equal(got, want) {
		t.Fatalf("signed = %v, want the update payloads then the list %v", got, want)
	}
}

// Off macOS the updater reads the version only from the signature (`--app-version`), and only
// under the keys `install_kind.rs` asks for, so the rows pin both: a Windows or Linux update
// payload signs its version under `windows-x86_64` / `linux-x86_64`, and macOS's archives don't.
func TestOffMacOSUpdatePayloadsSignTheirVersionUnderAKeyTheAppReads(t *testing.T) {
	linuxRows := []Asset{
		{OS: Linux, Arch: X86_64, Kind: AppImage, File: "Cmdr_{version}_amd64.AppImage", UpdaterKey: "linux-x86_64", Targets: []string{"x86_64-unknown-linux-gnu"}},
		{OS: Linux, Arch: X86_64, Kind: RustSBOM, File: "Cmdr_{version}_linux_x64.rust.cdx.json", Targets: []string{"x86_64-unknown-linux-gnu"}},
	}
	rows := append(slices.Clone(Assets), linuxRows...)
	r := Resolve("0.70.0", rows)
	if err := r.Validate(); err != nil {
		t.Fatal(err)
	}
	signed := map[string]bool{}
	for _, f := range r.Updates() {
		signed[f.UpdaterKey] = f.SignsVersion()
	}
	want := map[string]bool{"darwin-universal": false, "darwin-aarch64": false, "darwin-x86_64": false, "windows-x86_64": true, "linux-x86_64": true}
	if !maps.Equal(signed, want) {
		t.Fatalf("SignsVersion by updater key = %v, want %v", signed, want)
	}

	bad := make([]Asset, len(rows))
	copy(bad, rows)
	bad[slices.IndexFunc(bad, func(a Asset) bool { return a.Kind == NSIS })].UpdaterKey = "win64"
	err := Resolve("0.70.0", bad).Validate()
	if err == nil || !strings.Contains(err.Error(), "updater key win64, which no windows x86_64 install reads") {
		t.Fatalf("an updater key the app never reads passed: %v", err)
	}
}

func TestValidateCatchesABuildWithoutAnSBOM(t *testing.T) {
	rows := slices.DeleteFunc(slices.Clone(Assets), func(a Asset) bool { return a.OS == Windows && a.Kind == RustSBOM })
	err := Resolve("0.70.0", rows).Validate()
	if err == nil || !strings.Contains(err.Error(), "x86_64-pc-windows-msvc, which no Rust SBOM row describes") {
		t.Fatalf("a Windows build with no SBOM row passed: %v", err)
	}
}

func TestValidateRefusesAnAdvisoryListTiedToOneOS(t *testing.T) {
	rows := append(slices.Clone(Assets), Asset{OS: Windows, Kind: AdvisoryList, File: "Cmdr_{version}_win-advisories.json"})
	err := Resolve("0.70.0", rows).Validate()
	if err == nil || !strings.Contains(err.Error(), "belongs to no one OS") {
		t.Fatalf("an OS-bound advisory list passed: %v", err)
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
