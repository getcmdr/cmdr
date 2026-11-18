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
	outs, err := plan(planInput{release: manifest.For("0.62.0"), wiring: manifest.All("0.62.0"), vetted: vettedAll, attested: attestedAll, subjectDir: "release-assets"})
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

// The wiring the pipeline has: every OS with update rows vetted, every SBOM row attested.
var (
	vettedAll   = []manifest.OS{manifest.MacOS, manifest.Windows}
	attestedAll = append(slices.Clone(attestedToday), "windows-x86_64")
)

func windowsPlanInput(version string) planInput {
	return planInput{
		release: manifest.ForOSes(version, manifest.MacOS, manifest.Windows), wiring: manifest.All(version),
		vetted: vettedAll, attested: attestedAll, subjectDir: "release-assets",
	}
}

// Windows' jobs are wired before Windows ships: the plan holds the vetting and attest steps to
// every row, and tells the Windows jobs to sit out.
func TestPlanTodayWiresWindowsAndSkipsIt(t *testing.T) {
	outs, err := plan(planInput{release: manifest.For("0.62.0"), wiring: manifest.All("0.62.0"), vetted: vettedAll, attested: attestedAll, subjectDir: "release-assets"})
	must(t, err)
	got := outputsOf(t, outs)
	for k, want := range map[string]string{"ships-windows": "false", "ships-linux": "false", "signed-files": got["update-archives"]} {
		if got[k] != want {
			t.Errorf("%s = %q, want %q", k, got[k], want)
		}
	}
	for _, k := range []string{"update-archives-windows", "uploads-windows", "sbom-windows-x86_64", "signs-version", "advisory-list"} {
		if _, ok := got[k]; ok {
			t.Errorf("a macOS-only release plans %s", k)
		}
	}
}

// A Windows release's leg never holds the macOS core: `signed-files`, `update-archives`, and
// `binaries` (what `publish` and `attest` require) are the core's, and the Windows builds are
// subject patterns that may match nothing, since the leg joins only when it's whole.
func TestPlanForAWindowsRelease(t *testing.T) {
	outs, err := plan(windowsPlanInput("0.70.0"))
	must(t, err)
	got := outputsOf(t, outs)
	core := "Cmdr_0.70.0_universal.app.tar.gz Cmdr_0.70.0_aarch64.app.tar.gz Cmdr_0.70.0_x64.app.tar.gz"
	want := map[string]string{
		"ships-windows":                "true",
		"uploads-windows":              "Cmdr_0.70.0_x64-setup.exe Cmdr_0.70.0_x64.msix cmdr-group-policy-0.70.0.zip",
		"update-archives-windows":      "Cmdr_0.70.0_x64-setup.exe",
		"update-archives":              core,
		"signed-files":                 core,
		"sbom-windows-x86_64":          "Cmdr_0.70.0_windows_x64.rust.cdx.json",
		"sbom-windows-x86_64-subjects": "release-assets/Cmdr_0.70.0_x64-setup[.]exe\nrelease-assets/Cmdr_0.70.0_x64[.]msix",
	}
	for k, v := range want {
		if got[k] != v {
			t.Errorf("%s =\n%s\nwant\n%s", k, got[k], v)
		}
	}
	if !strings.Contains(got["rust-sboms"], "x86_64-pc-windows-msvc Cmdr_0.70.0_windows_x64.rust.cdx.json") {
		t.Errorf("rust-sboms = %s", got["rust-sboms"])
	}
	if strings.Contains(got["binaries"], "x64-setup") || strings.Contains(got["binaries"], "msix") {
		t.Errorf("binaries = %s (the core's required builds only)", got["binaries"])
	}
	if !strings.Contains(got["sbom-frontend-subjects"], "release-assets/Cmdr_0.70.0_x64[.]msix") {
		t.Errorf("sbom-frontend-subjects = %s (the Windows builds as patterns)", got["sbom-frontend-subjects"])
	}
}

func TestPlanRefusesAnUnvettedOS(t *testing.T) {
	in := windowsPlanInput("0.70.0")
	for _, release := range []manifest.Release{in.release, manifest.For("0.70.0")} {
		in.release, in.vetted = release, []manifest.OS{manifest.MacOS}
		if _, err := plan(in); err == nil || !strings.Contains(err.Error(), "windows update payloads") {
			t.Fatalf("Windows rows nobody vets passed the plan (shipped: %v): %v", release.Ships(manifest.Windows), err)
		}
	}
}

func TestPlanRefusesAnSBOMWithoutItsAttestStep(t *testing.T) {
	in := windowsPlanInput("0.70.0")
	in.release = manifest.For("0.70.0") // unshipped rows count too
	in.attested = attestedToday
	if _, err := plan(in); err == nil || !strings.Contains(err.Error(), "missing: [windows-x86_64]") {
		t.Fatalf("an SBOM nobody attests passed the plan: %v", err)
	}
	in.attested = append(slices.Clone(attestedAll), "linux-x86_64")
	if _, err := plan(in); err == nil || !strings.Contains(err.Error(), "no longer in the manifest: [linux-x86_64]") {
		t.Fatalf("an attest step for no SBOM passed the plan: %v", err)
	}
}
