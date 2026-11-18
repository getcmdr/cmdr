package main

import (
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// A release that shipped without its Windows leg mirrors its macOS files and misses nothing; one
// that carried it mirrors the leg too.
func TestTheMirrorFollowsTheLegsTheManifestCarried(t *testing.T) {
	r := manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)
	names := fullRelease("0.70.0", true)
	mac := []byte(`{"version":"0.70.0","platforms":{"darwin-universal":{},"darwin-aarch64":{},"darwin-x86_64":{}}}`)
	got, err := carried(r, names, mac)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := mirroredAssets(names, got); err != nil {
		t.Fatalf("a macOS-only manifest still wants the Windows files: %v", err)
	}
	win := []byte(`{"version":"0.70.0","platforms":{"darwin-universal":{},"darwin-aarch64":{},"darwin-x86_64":{},"windows-x86_64":{}}}`)
	if got, err = carried(r, names, win); err != nil {
		t.Fatal(err)
	}
	if _, err := mirroredAssets(names, got); err == nil || !strings.Contains(err.Error(), "Cmdr_0.70.0_x64-setup.exe") {
		t.Fatalf("a manifest carrying Windows didn't want its installer: %v", err)
	}
	// No manifest: the legs whole on the release, and this one has no Windows files at all.
	if got, err = carried(r, names, nil); err != nil || got.Ships(manifest.Windows) {
		t.Fatalf("no Windows files on the release, yet the mirror carries %v (%v)", manifest.Names(got.Files), err)
	}
}
