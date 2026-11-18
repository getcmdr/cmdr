package manifest

import (
	"slices"
	"testing"
)

func TestTheCoreIsWhatAMacOSOnlyReleaseShips(t *testing.T) {
	r, _ := windowsRelease(t)
	if got, want := Names(r.Core().Files), Names(ForOSes("0.70.0", MacOS).Files); !slices.Equal(got, want) {
		t.Fatalf("core = %v, want macOS-only %v", got, want)
	}
	if got, want := Names(r.Carrying(Windows).Files), Names(r.Files); !slices.Equal(got, want) {
		t.Fatalf("carrying Windows = %v, want everything %v", got, want)
	}
	if got := r.LegOSes(); !slices.Equal(got, []OS{Windows}) {
		t.Fatalf("legs = %v", got)
	}
}

// The Windows leg is all or nothing: the installer, the MSIX, the policy zip, the advisory list,
// and (once signed) both signatures. The SBOM isn't part of it: `attest` uploads it later.
func TestALegCountsOnlyWhenAllOfItIsThere(t *testing.T) {
	r, _ := windowsRelease(t)
	core := Names(r.Core().Files)
	leg := []string{"Cmdr_0.70.0_x64-setup.exe", "Cmdr_0.70.0_x64.msix", "cmdr-group-policy-0.70.0.zip", "Cmdr_0.70.0_security-advisories.json"}
	sigs := []string{"Cmdr_0.70.0_x64-setup.exe.sig", "Cmdr_0.70.0_security-advisories.json.sig"}

	if got := r.LegNeeds(Windows, true); !slices.Equal(got, []string{leg[0], sigs[0], leg[1], leg[2], leg[3], sigs[1]}) {
		t.Fatalf("the Windows leg needs %v", got)
	}
	complete, lacking := r.CompleteLegs(slices.Concat(core, leg, sigs), true)
	if !slices.Equal(complete, []OS{Windows}) || len(lacking) != 0 {
		t.Fatalf("a whole leg: complete %v, lacking %v", complete, lacking)
	}
	complete, lacking = r.CompleteLegs(slices.Concat(core, leg, sigs[:1]), true)
	if len(complete) != 0 || !slices.Equal(lacking[Windows], sigs[1:]) {
		t.Fatalf("no advisory signature: complete %v, lacking %v", complete, lacking)
	}
	complete, _ = r.CompleteLegs(slices.Concat(core, leg), false)
	if !slices.Equal(complete, []OS{Windows}) {
		t.Fatalf("an unsigned draft's whole leg counts before signing: %v", complete)
	}
	complete, lacking = r.CompleteLegs(core, false)
	if len(complete) != 0 || len(lacking[Windows]) != len(leg) {
		t.Fatalf("no Windows files: complete %v, lacking %v", complete, lacking)
	}
}

func TestCarriedByFollowsThePublishedManifest(t *testing.T) {
	r, _ := windowsRelease(t)
	mac := `{"version":"0.70.0","platforms":{"darwin-universal":{},"darwin-aarch64":{},"darwin-x86_64":{}}}`
	got, err := r.CarriedBy([]byte(mac))
	if err != nil || got.Ships(Windows) {
		t.Fatalf("a macOS-only manifest: %v, %v", Names(got.Files), err)
	}
	win := `{"version":"0.70.0","platforms":{"darwin-universal":{},"windows-x86_64":{}}}`
	if got, err = r.CarriedBy([]byte(win)); err != nil || !got.Ships(Windows) {
		t.Fatalf("a manifest with Windows: %v, %v", Names(got.Files), err)
	}
	if _, err := r.CarriedBy([]byte(`{"version":"0.69.0","platforms":{}}`)); err == nil {
		t.Fatal("another version's manifest fixed this release's legs")
	}
}

func TestLegsWithKeysReadsWhatAManifestCarried(t *testing.T) {
	r, _ := windowsRelease(t)
	mac := []string{"darwin-universal", "darwin-aarch64", "darwin-x86_64"}
	if got, err := r.LegsWithKeys(mac); err != nil || len(got) != 0 {
		t.Fatalf("a macOS-only manifest carries %v, %v", got, err)
	}
	if got, err := r.LegsWithKeys(append(mac, "windows-x86_64")); err != nil || !slices.Equal(got, []OS{Windows}) {
		t.Fatalf("a manifest with Windows carries %v, %v", got, err)
	}
}
