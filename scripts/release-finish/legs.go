package main

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

// A release's Windows leg never holds the laptop (`scripts/release-assets/manifest/legs.go`): the
// core (macOS) must be whole and sign, while a leg is signed only when all of it is on the draft
// and passes the same provenance and vetting, and is otherwise left out with a line saying why.
// The dispatched `publish` then carries exactly the legs whose signatures are up.

// legSigned are the files os's leg adds to what the core signs (its update payloads and the
// advisory list it brings), in manifest order.
func legSigned(release manifest.Release, os manifest.OS) []string {
	core := manifest.Names(release.Core().Signed())
	var out []string
	for _, name := range manifest.Names(release.Carrying(os).Signed()) {
		if !slices.Contains(core, name) {
			out = append(out, name)
		}
	}
	return out
}

// legsOnDraft are the legs whole on the draft (unsigned: the laptop is about to sign them), each
// with the files it adds to the signing, and a line for each leg the draft lacks.
func legsOnDraft(release manifest.Release, rel releaseInfo) (legs []manifest.OS, lines []string) {
	have := make([]string, 0, len(rel.Assets))
	for _, a := range rel.Assets {
		have = append(have, a.Name)
	}
	complete, lacking := release.CompleteLegs(have, false)
	for _, os := range release.LegOSes() {
		if missing, ok := lacking[os]; ok {
			lines = append(lines, manifest.LackingLine(release.Version, os, missing))
		}
	}
	return complete, lines
}

// verifyNames are what `-verify-dir` checks in dir: every core signed file (each must be there),
// plus each other signed file of the release whose `.sig` is in dir (a leg the release carries).
func verifyNames(release manifest.Release, dir string) []string {
	names := manifest.Names(release.Core().Signed())
	for _, name := range manifest.Names(release.Signed()) {
		if slices.Contains(names, name) {
			continue
		}
		if _, err := os.Stat(filepath.Join(dir, name+".sig")); err == nil {
			names = append(names, name)
		}
	}
	return names
}

// legJobNames are the display names of the Windows leg's jobs in `release-pipeline.yml`
// (`legs_test.go` holds them to it). The laptop doesn't wait for them: the build run is signable
// once every other job is done.
var legJobNames = []string{"Build the Windows packages", "Sign and upload the Windows packages", "Attest the Windows packages"}

// lastCoreJob is the build run's last job the core needs before it can be signed (`attest`, whose
// provenance the laptop verifies), so its being done means every core job before it is too.
const lastCoreJob = "Attest provenance and publish SBOMs"

// jobIs matches a job's name as `gh run view` reports it, which prefixes the caller's job name
// for a reusable workflow (`Release / Build the Windows packages`).
func jobIs(name, want string) bool { return name == want || strings.HasSuffix(name, " / "+want) }

func isLegJob(name string) bool {
	return slices.ContainsFunc(legJobNames, func(want string) bool { return jobIs(name, want) })
}

// coreDone reports whether the build run's core is done before the run is (its leg jobs still
// going), and the core jobs that didn't succeed if so. Not done until `lastCoreJob` has finished.
func coreDone(jobs []ciJob) (done bool, failed []string) {
	sawLast := false
	for _, j := range jobs {
		if isLegJob(j.Name) {
			continue
		}
		if j.Status != "completed" {
			return false, nil
		}
		if jobIs(j.Name, lastCoreJob) {
			sawLast = true
		}
		if j.Conclusion != "success" && j.Conclusion != "skipped" {
			failed = append(failed, fmt.Sprintf("%s (%s)", j.Name, j.Conclusion))
		}
	}
	if !sawLast {
		return false, nil
	}
	return true, failed
}
