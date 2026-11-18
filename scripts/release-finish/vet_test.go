package main

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// With Windows shipped, the macOS archives still go to `cmdr-vet-update` and the NSIS installer
// gets its own check, which refuses a file that isn't this release's installer; the advisory
// list has nothing to vet.
func TestVetSendsEachUpdatePayloadToItsOwnCheck(t *testing.T) {
	release := manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)
	names := manifest.Names(release.Signed())
	var gotArgs []string
	run := func(dir, name string, args ...string) (string, error) {
		gotArgs = args
		return "", nil
	}
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "Cmdr_0.70.0_x64-setup.exe"), []byte("not an installer"), 0o644); err != nil {
		t.Fatal(err)
	}
	err := vetArchives(run, "/repo", release, dir, names)
	if err == nil || !strings.Contains(err.Error(), "refusing to sign Cmdr_0.70.0_x64-setup.exe: it isn't a Windows executable") {
		t.Fatalf("a broken Windows installer passed: %v", err)
	}
	if err := vetArchives(run, "/repo", release, dir, names[:3]); err != nil {
		t.Fatal(err)
	}
	if slices.ContainsFunc(gotArgs, func(a string) bool { return strings.Contains(a, "setup.exe") || strings.Contains(a, "advisories") }) {
		t.Fatalf("cmdr-vet-update got a file that isn't a macOS archive: %v", gotArgs)
	}
}

// The gate runs the app's own check through `cmdr-vet-update`, on every archive, at the release's
// version, with the lockfile as committed.
func TestVetRunsTheAppsCheckOnEveryArchive(t *testing.T) {
	var gotDir, gotName string
	var gotArgs []string
	run := func(dir, name string, args ...string) (string, error) {
		gotDir, gotName, gotArgs = dir, name, args
		return "", nil
	}
	names := signedNames("0.53.0")
	if err := vetArchives(run, "/repo", manifest.For("0.53.0"), "/work", names); err != nil {
		t.Fatalf("want a pass, got %v", err)
	}
	if gotDir != "/repo" || gotName != "cargo" {
		t.Fatalf("ran %s in %s, want cargo in /repo", gotName, gotDir)
	}
	want := []string{"run", "--quiet", "--release", "--locked", "-p", "cmdr-update-trust", "--bin", "cmdr-vet-update", "--",
		"--version", "0.53.0"}
	for _, name := range names {
		want = append(want, filepath.Join("/work", name))
	}
	if !slices.Equal(gotArgs, want) {
		t.Fatalf("args %v, want %v", gotArgs, want)
	}
}

// ❗ An archive the updater would refuse must never get signed: signing it would publish a release
// every install refuses to take, and no later update could reach them.
func TestVetRefusesWhenTheCheckRefuses(t *testing.T) {
	run := func(string, string, ...string) (string, error) {
		return "    Cmdr_0.53.0_x64.app.tar.gz: REFUSED, the bundle isn't signed by Cmdr's Developer ID", errors.New("exit status 1")
	}
	err := vetArchives(run, "/repo", manifest.For("0.53.0"), "/work", signedNames("0.53.0"))
	if err == nil || !strings.Contains(err.Error(), "refusing to sign") {
		t.Fatalf("want a refusal to sign, got %v", err)
	}
}
