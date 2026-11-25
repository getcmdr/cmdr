package manifest

import (
	"fmt"
	"strconv"
	"strings"
)

// Shipping is one platform a release builds and publishes, from the version that turned it on.
type Shipping struct {
	OS OS
	// Since is the first version that ships it; empty for every version. A version before it
	// resolves as it was published, so the tools that read an old release (`release-evidence`, the
	// mirror, `release-finish` on a draft tagged before the switch) expect only what it shipped.
	Since string
	// Prereleases is whether a prerelease version (`0.80.0-beta.1`) ships it. Off for Windows: the
	// Store's MSIX version is `X.Y.Z.0`, with no room for a prerelease, so a prerelease tag builds
	// no Windows leg at all and the plan says so.
	Prereleases bool
	// StorePackageSince (Windows only) is the first version whose Windows leg carries the Microsoft
	// Store's MSIX; empty: none yet. Its own switch so the NSIS installer can ship before Partner
	// Center's identity exists: a planned MSIX packs with the release identity
	// (`apps/desktop/packaging/msix/identity.json`) and fails on a placeholder, so set it only once
	// that identity is committed. A version before it resolves as it was published, as for `Since`.
	StorePackageSince string
}

// storePackageShips reports whether a release of version under shipping carries the Store's MSIX.
func storePackageShips(version string, shipping []Shipping) bool {
	for _, s := range shipping {
		if s.OS == Windows {
			return s.StorePackageSince != "" && compareCore(version, s.StorePackageSince) >= 0
		}
	}
	return false
}

// ShippedOSes are the platforms a release builds and publishes: the one switch for a new platform.
// Every tool reads it through `For` (the pipeline's plan, `latest.json`, the checksums, the mirror,
// `release-finish`, `release-evidence`), so they can't disagree about what a release holds.
// Windows' rows and jobs are wired and tested, and off until David turns them on
// (`docs/guides/releasing.md` § Windows releases lists what that waits on); ❌ don't add it here
// without his go. Turning it on is `{OS: Windows, Since: "<the next version>"}`, never an empty
// `Since`: that would rewrite what every past release should have carried.
var ShippedOSes = []Shipping{{OS: MacOS, Prereleases: true}}

// shippedFor is the OSes a release of version ships under shipping, plus a line per OS it skips
// for the version's sake (before its `Since`, or a prerelease), for the plan's log.
func shippedFor(version string, shipping []Shipping) (oses []OS, skipped []string) {
	pre := strings.ContainsAny(version, "-+")
	for _, s := range shipping {
		switch {
		case s.Since != "" && compareCore(version, s.Since) < 0:
			skipped = append(skipped, fmt.Sprintf("%s ships from %s, so %s carries no %s files", s.OS, s.Since, version, s.OS))
		case pre && !s.Prereleases:
			skipped = append(skipped, fmt.Sprintf("%s is a prerelease, and %s ships final versions only, so this release builds no %s files", version, s.OS, s.OS))
		default:
			oses = append(oses, s.OS)
			if s.OS == Windows && !storePackageShips(version, shipping) {
				skipped = append(skipped, fmt.Sprintf("%s ships Windows without the Microsoft Store's MSIX (its `StorePackageSince` is %q), so the Windows files are the NSIS installer and the rest", version, s.StorePackageSince))
			}
		}
	}
	return oses, skipped
}

// Skipped explains each shipped platform a release of version leaves out, one line each.
func Skipped(version string) []string {
	_, skipped := shippedFor(version, ShippedOSes)
	return skipped
}

// compareCore orders two versions by their `X.Y.Z`, ignoring any prerelease or build suffix.
// A part that isn't a number counts as 0; `ShippingProblems` refuses such a `Since`.
func compareCore(a, b string) int {
	pa, pb := coreParts(a), coreParts(b)
	for i := range pa {
		if pa[i] != pb[i] {
			if pa[i] < pb[i] {
				return -1
			}
			return 1
		}
	}
	return 0
}

func coreParts(v string) [3]int {
	core, _, _ := strings.Cut(v, "-")
	core, _, _ = strings.Cut(core, "+")
	var out [3]int
	for i, p := range strings.SplitN(core, ".", 3) {
		out[i], _ = strconv.Atoi(p)
	}
	return out
}

// plainVersion reports whether v is a plain `X.Y.Z` of numbers.
func plainVersion(v string) bool {
	parts := strings.Split(v, ".")
	if len(parts) != 3 {
		return false
	}
	for _, p := range parts {
		if _, err := strconv.Atoi(p); err != nil {
			return false
		}
	}
	return true
}

// ShippingProblems checks a shipping list: every `Since` a plain `X.Y.Z`, no OS twice, and macOS
// on from the start (every release so far carried it).
func ShippingProblems(shipping []Shipping) []string {
	var problems []string
	seen := map[OS]bool{}
	for _, s := range shipping {
		if seen[s.OS] {
			problems = append(problems, fmt.Sprintf("%s ships twice", s.OS))
		}
		seen[s.OS] = true
		if s.Since != "" && !plainVersion(s.Since) {
			problems = append(problems, fmt.Sprintf("%s ships since %q, which isn't a plain X.Y.Z", s.OS, s.Since))
		}
		if s.StorePackageSince != "" {
			switch {
			case s.OS != Windows:
				problems = append(problems, fmt.Sprintf("%s sets StorePackageSince, which only Windows has", s.OS))
			case !plainVersion(s.StorePackageSince):
				problems = append(problems, fmt.Sprintf("the Store's MSIX ships since %q, which isn't a plain X.Y.Z", s.StorePackageSince))
			case s.Since != "" && compareCore(s.StorePackageSince, s.Since) < 0:
				problems = append(problems, fmt.Sprintf("the Store's MSIX ships since %s, before Windows does (%s)", s.StorePackageSince, s.Since))
			}
		}
		if s.OS == MacOS && (s.Since != "" || !s.Prereleases) {
			problems = append(problems, "macos ships in every release, prereleases included")
		}
	}
	if !seen[MacOS] {
		problems = append(problems, "no release would ship macos")
	}
	return problems
}
