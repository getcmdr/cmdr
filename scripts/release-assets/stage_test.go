package main

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

var windowsLegFiles = []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_x64.msix", "cmdr-group-policy-0.70.0.zip"}

// stageFixture is a build artifact holding names, each a small file, and the checkout's
// advisory list.
func stageFixture(t *testing.T, names ...string) stageInput {
	t.Helper()
	dir := t.TempDir()
	from := filepath.Join(dir, "artifact")
	must(t, os.Mkdir(from, 0o755))
	for _, n := range names {
		must(t, os.WriteFile(filepath.Join(from, n), []byte("built "+n), 0o644))
	}
	advisories := filepath.Join(dir, "security-advisories.json")
	must(t, os.WriteFile(advisories, []byte(`{"from":"the checkout"}`), 0o644))
	return stageInput{
		release: manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows), os: manifest.Windows,
		from: from, advisories: advisories, out: filepath.Join(dir, "staged"),
	}
}

func TestStageTakesExactlyTheManifestsFiles(t *testing.T) {
	in := stageFixture(t, windowsLegFiles...)
	res, err := stage(in)
	if err != nil {
		t.Fatal(err)
	}
	list := "Cmdr_0.70.0_security-advisories.json"
	if want := append(slices.Clone(windowsLegFiles), list); !slices.Equal(res.files, want) {
		t.Errorf("files = %v, want %v", res.files, want)
	}
	if want := []string{"Cmdr_0.70.0_x64-setup.exe", list}; !slices.Equal(res.signed, want) {
		t.Errorf("signed = %v, want %v", res.signed, want)
	}
	if want := []string{"Cmdr_0.70.0_x64-setup.exe"}; !slices.Equal(res.signsVersion, want) {
		t.Errorf("signs-version = %v, want %v", res.signsVersion, want)
	}
	entries, err := os.ReadDir(in.out)
	must(t, err)
	if len(entries) != len(res.files) {
		t.Fatalf("staged %d files, reported %v", len(entries), res.files)
	}
	got, err := os.ReadFile(filepath.Join(in.out, list))
	must(t, err)
	if string(got) != `{"from":"the checkout"}` {
		t.Fatalf("the advisory list came from somewhere other than the checkout: %s", got)
	}
}

// A compromised build hook that drops a macOS-named file, a stray signature, or its own
// advisory list into the artifact gets the job refused, each file named, before any key is near.
func TestStageRefusesAnythingTheManifestDoesntName(t *testing.T) {
	for _, extra := range []string{
		"Cmdr_0.70.0_universal.dmg",
		"Cmdr_0.70.0_universal.app.tar.gz",
		"Cmdr_0.70.0_x64-setup.exe.sig",
		"Cmdr_0.70.0_security-advisories.json",
		"latest.json",
	} {
		in := stageFixture(t, append(slices.Clone(windowsLegFiles), extra)...)
		_, err := stage(in)
		if err == nil || !strings.Contains(err.Error(), extra+" isn't a windows file of 0.70.0") {
			t.Errorf("an artifact with %s passed: %v", extra, err)
		}
		if _, statErr := os.Stat(in.out); statErr == nil {
			t.Errorf("a refused artifact with %s still staged files", extra)
		}
	}
}

func TestStageRefusesAMissingFileOrALink(t *testing.T) {
	in := stageFixture(t, windowsLegFiles[1:]...)
	if _, err := stage(in); err == nil || !strings.Contains(err.Error(), "Cmdr_0.70.0_x64-setup.exe is missing") {
		t.Errorf("an artifact without the installer passed: %v", err)
	}

	in = stageFixture(t, windowsLegFiles[1:]...)
	target := filepath.Join(filepath.Dir(in.from), "elsewhere")
	must(t, os.WriteFile(target, []byte("x"), 0o644))
	must(t, os.Symlink(target, filepath.Join(in.from, windowsLegFiles[0])))
	if _, err := stage(in); err == nil || !strings.Contains(err.Error(), "Cmdr_0.70.0_x64-setup.exe isn't a regular file") {
		t.Errorf("a symlinked installer passed: %v", err)
	}
}

func TestStageRefusesAnOSTheReleaseDoesntShip(t *testing.T) {
	in := stageFixture(t, windowsLegFiles...)
	in.release = manifest.ForOSes("0.70.0", manifest.MacOS)
	if _, err := stage(in); err == nil || !strings.Contains(err.Error(), "ships no windows files") {
		t.Errorf("staging an unshipped OS passed: %v", err)
	}
}
