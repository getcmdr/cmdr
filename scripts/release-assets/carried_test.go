package main

import (
	"os"
	"path/filepath"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// A macOS-only release reads no earlier `latest.json` at all, so a damaged upload (the
// documented "re-run failed jobs" case) is overwritten on the re-run, as it always was.
func TestAMacOSOnlyReleaseIgnoresADamagedEarlierManifest(t *testing.T) {
	bad := filepath.Join(t.TempDir(), "latest.json")
	must(t, os.WriteFile(bad, []byte(`{"version":"0.61`), 0o644))
	r := manifest.For("0.61.0")
	got, err := carriedRelease(r, nil, bad, "")
	if err != nil || len(got.Files) != len(r.Files) {
		t.Fatalf("a macOS-only release with a damaged carried-by: %v, %v", manifest.Names(got.Files), err)
	}
	if _, ok := previousLegs(latestInput{release: r}, bad); ok {
		t.Fatal("a macOS-only release kept an earlier manifest")
	}
}

// A leg counts only fully saved, non-empty assets, and a signature only once it's on disk: a
// Windows upload still going when `publish` looks costs the leg, never the release.
func TestALegCountsOnlyFinishedUploads(t *testing.T) {
	dir := t.TempDir()
	must(t, os.WriteFile(filepath.Join(dir, "a.exe.sig"), []byte("sig"), 0o644))
	assets := []releaseAsset{
		{Name: "a.exe", Size: 10, State: "uploaded"},
		{Name: "a.exe.sig", Size: 3, State: "uploaded"},
		{Name: "b.exe", Size: 10, State: "starter"},
		{Name: "c.zip", Size: 0, State: "uploaded"},
		{Name: "d.json.sig", Size: 3, State: "uploaded"}, // not downloaded
	}
	got := finishedAssets(assets, dir)
	if len(got) != 2 || got[0] != "a.exe" || got[1] != "a.exe.sig" {
		t.Fatalf("finished = %v, want a.exe and its signature", got)
	}
}

// With a Windows leg, an earlier `latest.json` that's damaged, another version's, or that
// doesn't pass the validator is never carried: the snapshot decides.
func TestADamagedEarlierManifestIsNeverCarried(t *testing.T) {
	in := latestInput{release: manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)}
	dir := t.TempDir()
	for name, body := range map[string]string{
		"empty":   ``,
		"cut":     `{"version":"0.70`,
		"other":   `{"version":"0.69.0","platforms":{}}`,
		"invalid": `{"version":"0.70.0","platforms":{"darwin-universal":{}}}`,
	} {
		path := filepath.Join(dir, name)
		must(t, os.WriteFile(path, []byte(body), 0o644))
		if _, ok := previousLegs(in, path); ok {
			t.Errorf("a %s earlier manifest was carried", name)
		}
	}
	if _, ok := previousLegs(in, filepath.Join(dir, "missing")); ok {
		t.Error("an earlier manifest that didn't download was carried")
	}
}
