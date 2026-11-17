package main

import (
	"encoding/json"
	"os"
	"slices"
	"testing"
)

// v0.61.0 as published (`../release-assets/testdata/v0.61.0/`): the mirror's picks and manifest
// check against a real release, so a change to how they're derived can't drift from what ships.
func publishedRelease(t *testing.T) (names []string, manifest []byte) {
	t.Helper()
	raw, err := os.ReadFile("../release-assets/testdata/v0.61.0/release.json")
	if err != nil {
		t.Fatal(err)
	}
	var rel struct {
		Assets []struct {
			Name string `json:"name"`
		} `json:"assets"`
	}
	if err := json.Unmarshal(raw, &rel); err != nil {
		t.Fatal(err)
	}
	for _, a := range rel.Assets {
		names = append(names, a.Name)
	}
	manifest, err = os.ReadFile("../release-assets/testdata/v0.61.0/latest.json")
	if err != nil {
		t.Fatal(err)
	}
	return names, manifest
}

func TestMirroredAssetsOfAPublishedRelease(t *testing.T) {
	names, manifest := publishedRelease(t)
	got, err := mirroredAssets(names, "0.61.0")
	if err != nil {
		t.Fatal(err)
	}
	want := []string{
		"Cmdr_0.61.0_aarch64.app.tar.gz", "Cmdr_0.61.0_aarch64.app.tar.gz.sig", "Cmdr_0.61.0_aarch64.dmg",
		"Cmdr_0.61.0_universal.app.tar.gz", "Cmdr_0.61.0_universal.app.tar.gz.sig", "Cmdr_0.61.0_universal.dmg",
		"Cmdr_0.61.0_universal.pkg",
		"Cmdr_0.61.0_x64.app.tar.gz", "Cmdr_0.61.0_x64.app.tar.gz.sig", "Cmdr_0.61.0_x64.dmg",
		"checksums.txt",
	}
	if !slices.Equal(got, want) {
		t.Fatalf("mirroredAssets =\n  %v\nwant\n  %v", got, want)
	}
	if err := checkManifest(manifest, "0.61.0", defaultHost); err != nil {
		t.Fatalf("v0.61.0's published latest.json: %v", err)
	}
}
