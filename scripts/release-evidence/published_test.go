package main

import (
	"encoding/json"
	"os"
	"slices"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// v0.61.0 as published (`../release-assets/testdata/v0.61.0/`): the evidence checks against a
// real release, so a change to how the expected assets are derived can't drift from what ships.
func TestEvidenceChecksPassAPublishedRelease(t *testing.T) {
	raw, err := os.ReadFile("../release-assets/testdata/v0.61.0/release.json")
	if err != nil {
		t.Fatal(err)
	}
	var rel struct {
		Assets []releaseAsset `json:"assets"`
	}
	if err := json.Unmarshal(raw, &rel); err != nil {
		t.Fatal(err)
	}
	if _, err := checkAssets(rel.Assets, manifest.For("0.61.0"), true); err != nil {
		t.Fatal(err)
	}
	if got := len(expectedAssets(manifest.For("0.61.0"), true)); got != len(rel.Assets) {
		t.Fatalf("expectedAssets lists %d files, v0.61.0 shipped %d", got, len(rel.Assets))
	}

	manifest, err := os.ReadFile("../release-assets/testdata/v0.61.0/latest.json")
	if err != nil {
		t.Fatal(err)
	}
	urls, err := checkManifest(manifest, "0.61.0", "getcmdr/cmdr")
	if err != nil {
		t.Fatal(err)
	}
	want := []string{
		"https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_aarch64.app.tar.gz",
		"https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_universal.app.tar.gz",
		"https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_x64.app.tar.gz",
	}
	if !slices.Equal(urls, want) {
		t.Fatalf("checkManifest = %v, want %v", urls, want)
	}
}
