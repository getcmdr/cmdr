package main

import (
	"slices"
	"strings"
	"testing"

	"cmdr/scripts/release-assets/manifest"
)

var attestedToday = []string{"macos-aarch64", "macos-x86_64", "frontend"}

func outputsOf(t *testing.T, outs []output) map[string]string {
	t.Helper()
	m := map[string]string{}
	for _, o := range outs {
		if _, dup := m[o.key]; dup {
			t.Fatalf("output %s twice", o.key)
		}
		m[o.key] = o.value
	}
	return m
}

func TestPlanForToday(t *testing.T) {
	outs, err := plan(planInput{release: manifest.For("0.62.0"), vetted: []manifest.OS{manifest.MacOS}, attested: attestedToday, subjectDir: "release-assets"})
	must(t, err)
	got := outputsOf(t, outs)
	want := map[string]string{
		"update-archives":       "Cmdr_0.62.0_universal.app.tar.gz Cmdr_0.62.0_aarch64.app.tar.gz Cmdr_0.62.0_x64.app.tar.gz",
		"update-archives-macos": "Cmdr_0.62.0_universal.app.tar.gz Cmdr_0.62.0_aarch64.app.tar.gz Cmdr_0.62.0_x64.app.tar.gz",
		"rust-sboms":            "aarch64-apple-darwin Cmdr_0.62.0_aarch64.rust.cdx.json\nx86_64-apple-darwin Cmdr_0.62.0_x64.rust.cdx.json",
		"frontend-sbom":         "Cmdr_0.62.0_frontend.cdx.json",
		"sbom-macos-aarch64":    "Cmdr_0.62.0_aarch64.rust.cdx.json",
		// The pkg is optional, so it's a pattern that may match nothing (`subjectPattern`).
		"sbom-macos-aarch64-subjects": "release-assets/Cmdr_0.62.0_universal.app.tar.gz\nrelease-assets/Cmdr_0.62.0_aarch64.app.tar.gz\nrelease-assets/Cmdr_0.62.0_aarch64.dmg\nrelease-assets/Cmdr_0.62.0_universal.dmg\nrelease-assets/Cmdr_0.62.0_universal[.]pkg",
	}
	for k, v := range want {
		if got[k] != v {
			t.Errorf("%s =\n%s\nwant\n%s", k, got[k], v)
		}
	}
	if !strings.Contains(got["binaries"], "Cmdr_0.62.0_x64.dmg") || strings.Contains(got["binaries"], ".pkg") {
		t.Errorf("binaries = %s (every required build, no optional pkg)", got["binaries"])
	}
	formatted := formatOutputs(outs)
	if !strings.Contains(formatted, "rust-sboms<<CMDR_RELEASE_ASSETS_EOF\naarch64-apple-darwin") || !strings.Contains(formatted, "frontend-sbom=Cmdr_0.62.0_frontend.cdx.json\n") {
		t.Errorf("formatOutputs:\n%s", formatted)
	}
}

func TestPlanRefusesAnUnvettedOS(t *testing.T) {
	r := manifest.Resolve("0.70.0", append(slices.Clone(manifest.Assets), windowsRows...))
	_, err := plan(planInput{release: r, vetted: []manifest.OS{manifest.MacOS}, attested: append(slices.Clone(attestedToday), "windows-x86_64")})
	if err == nil || !strings.Contains(err.Error(), "windows update payloads") {
		t.Fatalf("an unvetted Windows payload passed the plan: %v", err)
	}
	outs, err := plan(planInput{release: r, vetted: []manifest.OS{manifest.MacOS, manifest.Windows}, attested: append(slices.Clone(attestedToday), "windows-x86_64")})
	must(t, err)
	if got := outputsOf(t, outs)["update-archives-windows"]; got != "Cmdr_0.70.0_x64-setup.exe" {
		t.Fatalf("update-archives-windows = %q", got)
	}
}

func TestPlanRefusesAnSBOMWithoutItsAttestStep(t *testing.T) {
	r := manifest.Resolve("0.70.0", append(slices.Clone(manifest.Assets), windowsRows...))
	_, err := plan(planInput{release: r, vetted: []manifest.OS{manifest.MacOS, manifest.Windows}, attested: attestedToday})
	if err == nil || !strings.Contains(err.Error(), "missing: [windows-x86_64]") {
		t.Fatalf("an SBOM nobody attests passed the plan: %v", err)
	}
	_, err = plan(planInput{release: manifest.For("0.62.0"), vetted: []manifest.OS{manifest.MacOS}, attested: append(slices.Clone(attestedToday), "linux-x86_64")})
	if err == nil || !strings.Contains(err.Error(), "no longer in the manifest: [linux-x86_64]") {
		t.Fatalf("an attest step for no SBOM passed the plan: %v", err)
	}
}
