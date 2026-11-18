package main

import (
	"slices"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

// A Windows or Linux payload's signature must name its version, or the updater off macOS refuses
// it; macOS archives keep the command they've always been signed with.
func TestSignerArgsNameTheVersionOnlyOffMacOS(t *testing.T) {
	release := manifest.ForOSes("0.70.0", manifest.MacOS, manifest.Windows)

	got := signerArgs(release, "Cmdr_0.70.0_x64-setup.exe", "/work/Cmdr_0.70.0_x64-setup.exe")
	want := []string{"exec", "tauri", "signer", "sign", "--app-version", "0.70.0", "/work/Cmdr_0.70.0_x64-setup.exe"}
	if !slices.Equal(got, want) {
		t.Errorf("Windows installer: %v, want %v", got, want)
	}

	got = signerArgs(release, "Cmdr_0.70.0_aarch64.app.tar.gz", "/work/Cmdr_0.70.0_aarch64.app.tar.gz")
	want = []string{"exec", "tauri", "signer", "sign", "/work/Cmdr_0.70.0_aarch64.app.tar.gz"}
	if !slices.Equal(got, want) {
		t.Errorf("macOS archive: %v, want %v", got, want)
	}

	// The advisory list is no installer: the app reads it as a list, never binding a version.
	got = signerArgs(release, "Cmdr_0.70.0_security-advisories.json", "/work/Cmdr_0.70.0_security-advisories.json")
	want = []string{"exec", "tauri", "signer", "sign", "/work/Cmdr_0.70.0_security-advisories.json"}
	if !slices.Equal(got, want) {
		t.Errorf("advisory list: %v, want %v", got, want)
	}
}
