package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

// stageInput is `stage`: the trusted side's intake of a build job's artifact.
type stageInput struct {
	release manifest.Release
	os      manifest.OS
	// from is the downloaded artifact, written by a job that ran every third-party build hook.
	from string
	// advisories is the advisory list in this job's own checkout, which the release publishes
	// under the manifest's name when it ships one.
	advisories string
	// out is a fresh directory the staged files go into, under their manifest names.
	out string
}

// stageResult is what the signing and upload steps loop over, every name from the manifest.
type stageResult struct {
	files, signed, signsVersion []string
}

// legUploads are the files an OS's build job hands on: its rows, minus the SBOMs (`sbom` makes
// those) and the advisory list (the trusted side writes that from its checkout).
func legUploads(r manifest.Release, osName manifest.OS) []manifest.File {
	var out []manifest.File
	for _, f := range r.Files {
		if f.OS == osName && !f.SBOM() {
			out = append(out, f)
		}
	}
	return out
}

// stage takes from the artifact exactly the files the release asset manifest says this OS's
// build makes for this version, and nothing else, the way `sign` takes exactly one `Cmdr` from
// each `compiled-*` artifact: the artifact's file list is the build job's word, and that job ran
// third-party code. Anything extra (a file named like a macOS asset, a stray `.sig`), anything
// missing, and anything that isn't a regular file fails it, each named. The advisory list comes
// from the checkout, never the artifact, so the updater key never signs bytes the build chose.
func stage(in stageInput) (stageResult, error) {
	leg := legUploads(in.release, in.os)
	if len(leg) == 0 {
		return stageResult{}, fmt.Errorf("release %s ships no %s files, so there's nothing to stage", in.release.Version, in.os)
	}
	want := manifest.Names(leg)
	entries, err := os.ReadDir(in.from)
	if err != nil {
		return stageResult{}, err
	}
	var problems []string
	have := map[string]bool{}
	for _, e := range entries {
		name := e.Name()
		switch {
		case !slices.Contains(want, name):
			problems = append(problems, fmt.Sprintf("%s isn't a %s file of %s", name, in.os, in.release.Version))
		case !e.Type().IsRegular():
			problems = append(problems, fmt.Sprintf("%s isn't a regular file (%s)", name, e.Type()))
		default:
			have[name] = true
		}
	}
	for _, name := range want {
		if !have[name] && !slices.ContainsFunc(entries, func(e os.DirEntry) bool { return e.Name() == name }) {
			problems = append(problems, fmt.Sprintf("%s is missing", name))
		}
	}
	if len(problems) > 0 {
		return stageResult{}, fmt.Errorf("the %s artifact must hold exactly %s:\n  - %s", in.os, strings.Join(want, ", "), strings.Join(problems, "\n  - "))
	}

	if err := os.Mkdir(in.out, 0o755); err != nil {
		return stageResult{}, fmt.Errorf("the staging directory must be new: %w", err)
	}
	files := slices.Clone(leg)
	for _, f := range leg {
		if err := copyRegular(filepath.Join(in.from, f.Name), filepath.Join(in.out, f.Name)); err != nil {
			return stageResult{}, err
		}
	}
	if list, ok := in.release.AdvisoryList(); ok {
		if in.advisories == "" {
			return stageResult{}, fmt.Errorf("release %s ships %s: name the checkout's advisory list with -advisories", in.release.Version, list.Name)
		}
		if err := copyRegular(in.advisories, filepath.Join(in.out, list.Name)); err != nil {
			return stageResult{}, err
		}
		files = append(files, list)
	}

	var res stageResult
	for _, f := range files {
		res.files = append(res.files, f.Name)
		if f.Signed() {
			res.signed = append(res.signed, f.Name)
		}
		if f.SignsVersion() {
			res.signsVersion = append(res.signsVersion, f.Name)
		}
	}
	return res, nil
}

// copyRegular copies one regular file, refusing a symlink or anything else at src.
func copyRegular(src, dst string) error {
	info, err := os.Lstat(src)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() {
		return fmt.Errorf("%s isn't a regular file", src)
	}
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer func() { _ = in.Close() }()
	out, err := os.OpenFile(dst, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		_ = out.Close()
		return err
	}
	return out.Close()
}

// stageOutputs are the step outputs the signing and upload steps read.
func (r stageResult) outputs() []output {
	return []output{
		{"files", strings.Join(r.files, " ")},
		{"signed", strings.Join(r.signed, " ")},
		{"signs-version", strings.Join(r.signsVersion, " ")},
	}
}

var errNoOS = errors.New("stage: name the OS whose artifact this is with -os")
