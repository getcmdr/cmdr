package main

import (
	"fmt"
	"slices"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

// output is one GitHub Actions step output.
type output struct {
	key, value string
}

type planInput struct {
	// release is what this release publishes (`manifest.For`); every output comes from it.
	release manifest.Release
	// wiring is every row, shipped or not (`manifest.All`), which `vetted` and `attested` are
	// held to: a platform's vetting and attest steps are in place before it ships.
	wiring manifest.Release
	// vetted are the OSes `vet-update-archives` can vet. An update payload for any other OS
	// fails the plan: an archive nobody vetted must not reach `latest.json`.
	vetted []manifest.OS
	// attested are the SBOM slugs the `attest` job has a step for. A manifest SBOM it doesn't
	// attest, or a step for an SBOM the manifest dropped, fails the plan.
	attested []string
	// subjectDir is where `attest` downloads the release, prefixed to each subject path.
	subjectDir string
}

// plan turns the manifest into the release pipeline's step outputs, made once in `guard` so the
// jobs after it need no Go and agree on one list:
//
//   - `update-archives`: the core's update payloads (`manifest.Release.Core`: macOS's),
//     space-separated.
//   - `signed-files`: the core's files with a `.sig`, which `publish` requires the signatures of.
//     A leg's (`manifest/legs.go`) are its own job's, and the release carries the leg only whole.
//   - `update-archives-<os>`: one OS's update payloads, for that OS's vetting.
//   - `ships-<os>`: `true` or `false` for each OS beyond macOS, which gates that OS's jobs, and
//     `uploads-<os>` (a shipped OS only): the files its build job puts on the release.
//   - `binaries`: every required core build of the app, which `attest` refuses to go without.
//   - `rust-sboms`: one `<target> <file>` line per Rust SBOM, for `sbom` to generate.
//   - `frontend-sbom`: the frontend SBOM's filename.
//   - `sbom-<slug>` and `sbom-<slug>-subjects`: each SBOM's file and the builds it's attested
//     against, one path per line.
func plan(in planInput) ([]output, error) {
	r := in.release
	for _, rel := range []manifest.Release{in.wiring, r} {
		if err := rel.Validate(); err != nil {
			return nil, err
		}
	}
	if err := checkWiring(in); err != nil {
		return nil, err
	}
	out := append(signingOutputs(r), platformOutputs(r)...)
	for _, osName := range updateOSes(r) {
		var names []string
		for _, f := range r.Updates() {
			if f.OS == osName {
				names = append(names, f.Name)
			}
		}
		out = append(out, output{"update-archives-" + string(osName), strings.Join(names, " ")})
	}

	out = append(out, output{"binaries", strings.Join(manifest.Names(manifest.Required(r.Core().Binaries())), " ")})

	var rust []string
	for _, s := range r.SBOMs() {
		switch s.Kind {
		case manifest.RustSBOM:
			if len(s.Targets) != 1 {
				return nil, fmt.Errorf("%s describes %d targets; a Rust SBOM is one `cargo cyclonedx --target`", s.Name, len(s.Targets))
			}
			rust = append(rust, s.Targets[0]+" "+s.Name)
		case manifest.FrontendSBOM:
			out = append(out, output{"frontend-sbom", s.Name})
		}
		var subjects []string
		for _, f := range r.Subjects(s) {
			subjects = append(subjects, subjectPattern(in.subjectDir, f))
		}
		out = append(out,
			output{"sbom-" + s.Slug(), s.Name},
			output{"sbom-" + s.Slug() + "-subjects", strings.Join(subjects, "\n")})
	}
	out = append(out, output{"rust-sboms", strings.Join(rust, "\n")})
	return out, nil
}

// signingOutputs are `update-archives` and `signed-files`: the core's (`manifest.Release.Core`),
// which `publish` requires whatever the other OSes do. A leg's signed files are its own job's
// (`windows-publish`'s `stage`), and `publish` takes a leg only when it's whole.
func signingOutputs(r manifest.Release) []output {
	core := r.Core()
	return []output{
		{"update-archives", strings.Join(manifest.Names(core.Updates()), " ")},
		{"signed-files", strings.Join(manifest.Names(core.Signed()), " ")},
	}
}

// platformOutputs are `ships-<os>` for each OS beyond macOS, and `uploads-<os>` for a shipped one.
func platformOutputs(r manifest.Release) []output {
	var out []output
	for _, osName := range []manifest.OS{manifest.Windows, manifest.Linux} {
		out = append(out, output{"ships-" + string(osName), fmt.Sprint(r.Ships(osName))})
		var uploads []string
		for _, f := range r.Files {
			if f.OS == osName && !f.SBOM() {
				uploads = append(uploads, f.Name)
			}
		}
		if len(uploads) > 0 {
			out = append(out, output{"uploads-" + string(osName), strings.Join(uploads, " ")})
		}
	}
	return out
}

// updateOSes are the OSes with update payloads in r, in manifest order.
func updateOSes(r manifest.Release) []manifest.OS {
	var oses []manifest.OS
	for _, f := range r.Updates() {
		if !slices.Contains(oses, f.OS) {
			oses = append(oses, f.OS)
		}
	}
	return oses
}

// checkWiring holds the pipeline's vetting and attest steps to every row, shipped or not.
func checkWiring(in planInput) error {
	for _, osName := range updateOSes(in.wiring) {
		if !slices.Contains(in.vetted, osName) {
			return fmt.Errorf("the manifest has %s update payloads, but `vet-update-archives` vets only %v: add a vetting step for %s first", osName, in.vetted, osName)
		}
	}
	var slugs []string
	for _, s := range in.wiring.SBOMs() {
		slugs = append(slugs, s.Slug())
	}
	missing, extra := diff(slugs, in.attested)
	if len(missing) > 0 || len(extra) > 0 {
		return fmt.Errorf("the `attest` job attests the SBOMs %v, but the manifest has %v: give each SBOM its attest step (missing: %v, no longer in the manifest: %v)",
			in.attested, slugs, missing, extra)
	}
	return nil
}

// subjectPattern is a file's line in an `actions/attest` `subject-path`. The action fails on a
// literal path that matches no file but lets a glob match nothing (verified in `actions/attest`'s
// `src/subject.ts`, `isLiteralPath`, 2026-11-18), so a required core file is a literal (its
// absence fails the attestation) and an optional one, or a leg's (which the release may ship
// without), is a pattern: `[.]` before its extension matches the one name and turns the line into
// a glob.
func subjectPattern(dir string, f manifest.File) string {
	if !f.Optional && f.Leg() == "" {
		return dir + "/" + f.Name
	}
	i := strings.LastIndex(f.Name, ".")
	return dir + "/" + f.Name[:i] + "[.]" + f.Name[i+1:]
}

// diff lists what's in want but not in have, and in have but not in want.
func diff(want, have []string) (missing, extra []string) {
	for _, w := range want {
		if !slices.Contains(have, w) {
			missing = append(missing, w)
		}
	}
	for _, h := range have {
		if !slices.Contains(want, h) {
			extra = append(extra, h)
		}
	}
	return missing, extra
}

// formatOutputs writes outputs in the `$GITHUB_OUTPUT` format, a heredoc for any multi-line one.
func formatOutputs(outs []output) string {
	var b strings.Builder
	for _, o := range outs {
		if strings.Contains(o.value, "\n") {
			fmt.Fprintf(&b, "%s<<CMDR_RELEASE_ASSETS_EOF\n%s\nCMDR_RELEASE_ASSETS_EOF\n", o.key, o.value)
		} else {
			fmt.Fprintf(&b, "%s=%s\n", o.key, o.value)
		}
	}
	return b.String()
}
