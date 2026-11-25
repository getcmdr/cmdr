package manifest

import (
	"slices"
	"strings"
	"testing"
)

// The day Windows is switched on, as the turn-on step writes it with Partner Center's identity in.
var windowsFrom080 = []Shipping{{OS: MacOS, Prereleases: true}, {OS: Windows, Since: "0.80.0", StorePackageSince: "0.80.0"}}

// Windows on before the Store account clears: the NSIS installer from 0.80.0, the MSIX from 0.82.0.
var windowsStoreLater = []Shipping{{OS: MacOS, Prereleases: true}, {OS: Windows, Since: "0.80.0", StorePackageSince: "0.82.0"}}

func hasKind(r Release, kind Kind) bool {
	return slices.ContainsFunc(r.Files, func(f File) bool { return f.Kind == kind })
}

// The NSIS installer ships without the Store's MSIX until `StorePackageSince`, so a placeholder
// Store identity never holds the Windows leg back; from then on the leg carries both, and a
// release before it still resolves as it was published.
func TestTheStorePackageShipsOnItsOwnSwitch(t *testing.T) {
	for _, c := range []struct {
		version   string
		shipping  []Shipping
		nsis, msi bool
	}{
		{"0.81.0", windowsStoreLater, true, false},
		{"0.82.0", windowsStoreLater, true, true},
		{"0.80.0", windowsFrom080, true, true},
		{"0.81.0", []Shipping{{OS: MacOS, Prereleases: true}, {OS: Windows, Since: "0.80.0"}}, true, false},
		{"0.79.0", windowsStoreLater, false, false},
	} {
		r := ForShipping(c.version, c.shipping)
		if hasKind(r, NSIS) != c.nsis || hasKind(r, MSIX) != c.msi {
			t.Errorf("%s under %v ships %v", c.version, c.shipping, Names(r.Files))
		}
	}
	_, skipped := shippedFor("0.81.0", windowsStoreLater)
	if len(skipped) != 1 || !strings.Contains(skipped[0], "without the Microsoft Store's MSIX") {
		t.Fatalf("the plan's log line = %q", skipped)
	}
	if _, skipped := shippedFor("0.82.0", windowsStoreLater); len(skipped) != 0 {
		t.Fatalf("a version with the MSIX skips %q", skipped)
	}
}

func TestTheShippingListIsConsistent(t *testing.T) {
	if problems := ShippingProblems(ShippedOSes); len(problems) > 0 {
		t.Fatal(problems)
	}
	if problems := ShippingProblems(windowsFrom080); len(problems) > 0 {
		t.Fatal(problems)
	}
	if problems := ShippingProblems(windowsStoreLater); len(problems) > 0 {
		t.Fatal(problems)
	}
}

// Turning Windows on changes what releases from then on carry, never what an older one did: the
// mirror, `release-evidence`, and `release-finish` read a past release as it was published.
func TestTurningAPlatformOnLeavesPastReleasesAsTheyWere(t *testing.T) {
	for _, c := range []struct {
		version string
		windows bool
	}{
		{"0.61.0", false},
		{"0.79.9", false},
		{"0.80.0", true},
		{"0.80.1", true},
		{"1.0.0", true},
	} {
		r := ForShipping(c.version, windowsFrom080)
		if r.Ships(Windows) != c.windows {
			t.Errorf("%s ships Windows: %v, want %v", c.version, r.Ships(Windows), c.windows)
		}
		if !r.Ships(MacOS) {
			t.Errorf("%s ships no macOS files", c.version)
		}
	}
	if got, want := Names(ForShipping("0.79.0", windowsFrom080).Files), Names(ForOSes("0.79.0", MacOS).Files); !slices.Equal(got, want) {
		t.Fatalf("a version before the switch resolves to %v, want macOS's %v", got, want)
	}
}

// A prerelease tag skips the Windows leg (the Store's MSIX version is `X.Y.Z.0`), and says so.
func TestAPrereleaseShipsNoWindowsLeg(t *testing.T) {
	r := ForShipping("0.81.0-beta.1", windowsFrom080)
	if r.Ships(Windows) || !r.Ships(MacOS) {
		t.Fatalf("0.81.0-beta.1 ships %v", Names(r.Files))
	}
	if _, ok := r.AdvisoryList(); ok {
		t.Error("a prerelease with no Windows leg ships the advisory list")
	}
	_, skipped := shippedFor("0.81.0-beta.1", windowsFrom080)
	if len(skipped) != 1 || !strings.Contains(skipped[0], "is a prerelease") || !strings.Contains(skipped[0], "windows") {
		t.Fatalf("the plan's log line = %q", skipped)
	}
	if _, skipped := shippedFor("0.81.0", windowsFrom080); len(skipped) != 0 {
		t.Fatalf("a final version skips %q", skipped)
	}
}

func TestShippingProblems(t *testing.T) {
	bad := []Shipping{{OS: MacOS}, {OS: Windows, Since: "0.80"}, {OS: Windows}}
	got := strings.Join(ShippingProblems(bad), "\n")
	for _, want := range []string{"windows ships twice", `"0.80", which isn't a plain X.Y.Z`, "prereleases included"} {
		if !strings.Contains(got, want) {
			t.Errorf("the problems don't say %q:\n%s", want, got)
		}
	}
	store := []Shipping{{OS: MacOS, Prereleases: true, StorePackageSince: "0.80.0"}, {OS: Windows, Since: "0.80.0", StorePackageSince: "0.79.0"}}
	got = strings.Join(ShippingProblems(store), "\n")
	for _, want := range []string{"macos sets StorePackageSince", "before Windows does"} {
		if !strings.Contains(got, want) {
			t.Errorf("the problems don't say %q:\n%s", want, got)
		}
	}
	if got := ShippingProblems([]Shipping{{OS: Windows}}); !slices.Contains(got, "no release would ship macos") {
		t.Errorf("a list without macOS passed: %v", got)
	}
}
