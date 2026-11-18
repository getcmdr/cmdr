package manifest

import (
	"encoding/json"
	"fmt"
	"maps"
	"slices"
	"strings"
)

// A release's files split into its core (macOS's rows and the frontend SBOM: every release so far)
// and one leg per other OS (that OS's rows, plus the advisory list its installs read). A leg is all
// or nothing, and it never holds the core: a release carries a leg only when the whole of it is
// on the release, signatures included, by the time the release's contents are fixed (`publish`'s
// snapshot in `ci` mode, the laptop's signing in `local` mode). Otherwise the release ships its
// core alone, and that OS's installs stay where they are until a release carries their leg.
// `docs/guides/releasing.md` § Windows releases.

// Leg reports the OS whose leg this file belongs to, or "" for a core file. The advisory list
// belongs to no OS, so it rides with the legs: `LegOSes` reports it under every leg it serves.
func (f File) Leg() OS {
	if f.OS == "" || f.OS == MacOS {
		return ""
	}
	return f.OS
}

// LegOSes are the OSes with a leg in r, in manifest order.
func (r Release) LegOSes() []OS {
	var oses []OS
	for _, f := range r.Files {
		if os := f.Leg(); os != "" && !slices.Contains(oses, os) {
			oses = append(oses, os)
		}
	}
	return oses
}

// Core is r without any leg: what a release ships whatever its other OSes do.
func (r Release) Core() Release { return r.Carrying() }

// Carrying is r with only the legs of oses: the core, those OSes' rows, and the advisory list when
// one of them has an update payload that signs its version (the installs that read the list from
// `latest.json`, `shipped`).
func (r Release) Carrying(oses ...OS) Release {
	out := Release{Version: r.Version}
	var lists []File
	signsVersion := false
	for _, f := range r.Files {
		switch {
		case f.Kind == AdvisoryList:
			lists = append(lists, f)
			continue
		case f.Leg() != "" && !slices.Contains(oses, f.Leg()):
			continue
		case f.SignsVersion():
			signsVersion = true
		}
		out.Files = append(out.Files, f)
	}
	if signsVersion {
		out.Files = append(out.Files, lists...)
	}
	return out
}

// LegNeeds are the release assets os's leg can't ship without: its required rows (not the SBOMs,
// which `attest` uploads after the release's contents are fixed), the advisory list it brings, and
// with signatures, each signed one's `.sig`.
func (r Release) LegNeeds(os OS, withSignatures bool) []string {
	var names []string
	for _, f := range r.Carrying(os).Files {
		if (f.Leg() != os && f.Kind != AdvisoryList) || f.SBOM() || f.Optional {
			continue
		}
		names = append(names, f.Name)
		if withSignatures && f.Signed() {
			names = append(names, f.Signature())
		}
	}
	return names
}

// CompleteLegs splits r's legs by whether every asset each needs is in have (the release's asset
// names): `withSignatures` once the signatures are in (the `publish` side), without them before
// the laptop signs (`local` mode's draft). Each incomplete leg comes with what it lacks, for the
// warning that says the release ships without it.
func (r Release) CompleteLegs(have []string, withSignatures bool) (complete []OS, lacking map[OS][]string) {
	lacking = map[OS][]string{}
	for _, os := range r.LegOSes() {
		var missing []string
		for _, name := range r.LegNeeds(os, withSignatures) {
			if !slices.Contains(have, name) {
				missing = append(missing, name)
			}
		}
		if len(missing) == 0 {
			complete = append(complete, os)
		} else {
			lacking[os] = missing
		}
	}
	return complete, lacking
}

// LegsWithKeys are the OSes of r's legs whose update payloads are all among keys (a published
// `latest.json`'s `platforms`): the legs that manifest carried. A leg with a payload in keys and
// another missing is an error, never half a leg.
func (r Release) LegsWithKeys(keys []string) ([]OS, error) {
	var oses []OS
	for _, os := range r.LegOSes() {
		in, out := 0, 0
		for _, f := range r.Updates() {
			if f.Leg() != os {
				continue
			}
			if slices.Contains(keys, f.UpdaterKey) {
				in++
			} else {
				out++
			}
		}
		switch {
		case in > 0 && out > 0:
			return nil, fmt.Errorf("the update manifest carries part of the %s leg only", os)
		case in > 0:
			oses = append(oses, os)
		}
	}
	return oses, nil
}

// CarriedWhole is r with the legs that are whole among have (signatures included), plus what each
// other leg lacked: the release's contents at the moment they're fixed.
func (r Release) CarriedWhole(have []string) (Release, map[OS][]string) {
	complete, lacking := r.CompleteLegs(have, true)
	return r.Carrying(complete...), lacking
}

// CarriedBy is r with the legs a `latest.json` of this version carries, the record every step
// after `publish` writes it follows (checksums, the mirror, the read-back, and a re-run of
// `publish` itself), so they can't disagree about which legs shipped.
func (r Release) CarriedBy(latestJSON []byte) (Release, error) {
	var m struct {
		Version   string                     `json:"version"`
		Platforms map[string]json.RawMessage `json:"platforms"`
	}
	if err := json.Unmarshal(latestJSON, &m); err != nil {
		return Release{}, fmt.Errorf("the update manifest that fixes the legs isn't JSON: %w", err)
	}
	if m.Version != r.Version {
		return Release{}, fmt.Errorf("the update manifest that fixes the legs is %q's, not %s's", m.Version, r.Version)
	}
	oses, err := r.LegsWithKeys(slices.Collect(maps.Keys(m.Platforms)))
	if err != nil {
		return Release{}, err
	}
	return r.Carrying(oses...), nil
}

// LackingLine says what an incomplete leg lacked, for the run's warning.
func LackingLine(version string, os OS, missing []string) string {
	return fmt.Sprintf("%s ships without its %s files: the release lacks %s. %s installs stay on their version until a release carries them (docs/guides/releasing.md § Windows releases)",
		version, os, strings.Join(missing, ", "), os)
}
