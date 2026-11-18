package main

import (
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

func windowsAssets(t *testing.T, r manifest.Release, names ...string) []releaseAsset {
	t.Helper()
	var out []releaseAsset
	for name := range expectedAssets(r.Core(), true) {
		out = append(out, releaseAsset{Name: name, Size: 30 << 20})
	}
	for _, n := range names {
		out = append(out, releaseAsset{Name: n, Size: 30 << 20})
	}
	return out
}

// A release that shipped without its Windows leg is complete; one with part of the leg isn't.
func TestEvidenceTakesALegWholeOrNotAtAll(t *testing.T) {
	r := manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)
	if _, err := checkAssets(windowsAssets(t, r), r, true); err != nil {
		t.Fatalf("a release without its Windows leg: %v", err)
	}
	leg := append(r.LegNeeds(manifest.Windows, true), "Cmdr_0.70.0_windows_x64.rust.cdx.json")
	if n, err := checkAssets(windowsAssets(t, r, leg...), r, true); err != nil || n != len(expectedAssets(r, true)) {
		t.Fatalf("a release with the whole leg: %d expected, %v", n, err)
	}
	if _, err := checkAssets(windowsAssets(t, r, leg[0]), r, true); err == nil || !strings.Contains(err.Error(), "the windows files are there only in part") {
		t.Fatalf("half a leg passed: %v", err)
	}
}
