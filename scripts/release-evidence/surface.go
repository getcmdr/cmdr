package main

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

type releaseAsset struct {
	Name string `json:"name"`
	Size int64  `json:"size"`
}

// Below these, an asset is a truncated upload or an empty bundle, not a release.
const (
	minBundleBytes = 20 << 20
	minSBOMBytes   = 10 << 10
)

// expectedAssets is every file a release carries (`docs/guides/releasing.md` § What a release
// publishes), each with the size below which it can't be real: the release asset manifest's files
// (`scripts/release-assets/manifest`), the update payloads' signatures, and the three files every
// release adds around them. withPkg asks for the optional files too (the pkg is the only one).
func expectedAssets(version string, withPkg bool) map[string]int64 {
	return expectedAssetsOf(manifest.For(version), withPkg)
}

// expectedAssetsOf is `expectedAssets` for r, the release as it carries its legs.
func expectedAssetsOf(r manifest.Release, withPkg bool) map[string]int64 {
	want := map[string]int64{
		"latest.json":                         1,
		"checksums.txt":                       1,
		"Cmdr_" + r.Version + ".intoto.jsonl": 1,
	}
	for _, f := range r.Files {
		switch {
		case f.Optional && !withPkg:
			continue
		case f.SBOM():
			want[f.Name] = minSBOMBytes
		case !f.Binary():
			// The Group Policy zip and the advisory list are small files, not builds.
			want[f.Name] = 1
		default:
			want[f.Name] = minBundleBytes
		}
		if f.Signed() {
			want[f.Signature()] = 1
		}
	}
	return want
}

// checkAssets lists every expected asset that's missing or too small to be real.
func checkAssets(assets []releaseAsset, version string, withPkg bool) error {
	_, err := checkAssetsOf(assets, manifest.For(version), withPkg)
	return err
}

// checkAssetsOf is `checkAssets` for r, answering how many assets it expected. A leg
// (`manifest/legs.go`) the release carries whole is expected in full; one it shipped without
// isn't, but part of one is a problem: a release carries a leg whole or not at all.
func checkAssetsOf(assets []releaseAsset, r manifest.Release, withPkg bool) (int, error) {
	have := map[string]int64{}
	names := make([]string, 0, len(assets))
	for _, a := range assets {
		have[a.Name] = a.Size
		names = append(names, a.Name)
	}
	var problems []string
	complete, lacking := r.CompleteLegs(names, true)
	for os, missing := range lacking {
		if len(missing) < len(r.LegNeeds(os, true)) {
			problems = append(problems, fmt.Sprintf("the %s files are there only in part (missing %s)", os, strings.Join(missing, ", ")))
		}
	}
	want := expectedAssetsOf(r.Carrying(complete...), withPkg)
	for name, min := range want {
		size, ok := have[name]
		switch {
		case !ok:
			problems = append(problems, name+" is missing")
		case size < min:
			problems = append(problems, fmt.Sprintf("%s is only %d bytes", name, size))
		}
	}
	if len(problems) == 0 {
		return len(want), nil
	}
	slices.Sort(problems)
	return 0, fmt.Errorf("the release's assets aren't complete:\n  - %s", strings.Join(problems, "\n  - "))
}

// checkManifest reads `latest.json` and returns its update archive URLs, requiring the version,
// each update payload's platform, and each URL naming that release file, on the EU mirror
// or (releases from before it) on GitHub. The release pipeline itself refuses a new manifest
// off the mirror (`scripts/release-mirror`); this only checks what's live answers.
func checkManifest(body []byte, version, repo string) ([]string, error) {
	var m struct {
		Version   string `json:"version"`
		Platforms map[string]struct {
			URL string `json:"url"`
		} `json:"platforms"`
	}
	if err := json.Unmarshal(body, &m); err != nil {
		return nil, fmt.Errorf("latest.json isn't JSON: %w", err)
	}
	if m.Version != version {
		return nil, fmt.Errorf("latest.json says version %q, not %q", m.Version, version)
	}
	// The legs this manifest carries (`manifest/legs.go`): a Windows leg the release shipped
	// without has no platform to check.
	carried, err := manifest.For(version).CarriedBy(body)
	if err != nil {
		return nil, err
	}
	var urls []string
	updates := carried.Updates()
	slices.SortFunc(updates, func(a, b manifest.File) int { return strings.Compare(a.UpdaterKey, b.UpdaterKey) })
	for _, f := range updates {
		p, ok := m.Platforms[f.UpdaterKey]
		if !ok {
			return nil, fmt.Errorf("latest.json has no %s platform", f.UpdaterKey)
		}
		mirrored := fmt.Sprintf("%sv%s/%s", mirrorHost, version, f.Name)
		github := fmt.Sprintf("https://github.com/%s/releases/download/v%s/%s", repo, version, f.Name)
		if p.URL != mirrored && p.URL != github {
			return nil, fmt.Errorf("latest.json's %s URL is %s, not %s", f.UpdaterKey, p.URL, mirrored)
		}
		urls = append(urls, p.URL)
	}
	return urls, nil
}
