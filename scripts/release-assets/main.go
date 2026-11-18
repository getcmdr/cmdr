// Release assets: the release asset manifest's command line, for the release pipeline's shell
// steps. The manifest itself, and what reads it, is `manifest/manifest.go`.
//
// # Usage
//
//	./scripts/release-assets.sh plan -version X.Y.Z -vetted "macos windows" -attested "macos-aarch64 macos-x86_64 frontend windows-x86_64" -subject-dir release-assets
//	./scripts/release-assets.sh latest-json -version X.Y.Z -assets release.json -sigs DIR -notes notes.txt -base-url URL -pub-date DATE [-newer-than X.Y.Z] -out latest.json
//	./scripts/release-assets.sh validate-latest -version X.Y.Z -assets release.json -sigs DIR -base-url URL [-newer-than X.Y.Z] -file latest.json
//	./scripts/release-assets.sh checksum-files -version X.Y.Z -assets release.json
//	./scripts/release-assets.sh stage -version X.Y.Z -os windows -from ARTIFACT -advisories LIST.json -out DIR
//
// `stage` is the trusted side's intake of a build artifact: exactly the files the manifest names
// for that OS and version, plus the advisory list from the checkout (`stage.go`).
// `plan` appends the pipeline's step outputs to `$GITHUB_OUTPUT` (or prints them when it's unset);
// `latest-json` writes the update manifest, only once `validateLatest` passes it;
// `validate-latest` runs the same validator on a manifest as published (the read-back);
// `checksum-files` prints the downloads to hash into
// `checksums.txt`, one per line. `-assets` is `gh api repos/<repo>/releases/tags/<tag>` or
// `gh release view <tag> --json assets` saved to a file. The pipeline's use of each:
// `docs/guides/releasing.md` § The release asset manifest.
package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"cmdr/scripts/release-assets/manifest"
)

var versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]*[0-9A-Za-z])?$`)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "release-assets: %v\n", err)
		os.Exit(1)
	}
}

// options are the parsed flags every command reads its own from.
type options struct {
	version, vetted, attested, subjectDir, assets, sigs, notes, baseURL, pubDate, out, file string
	osName, from, advisories, carriedBy, previous, newerThan                                string
}

func run(args []string) error {
	if len(args) == 0 {
		return errors.New("name a command: plan, latest-json, validate-latest, stage, or checksum-files")
	}
	var o options
	fs := flag.NewFlagSet(args[0], flag.ContinueOnError)
	fs.StringVar(&o.version, "version", "", "the release's version, without the leading v")
	fs.StringVar(&o.vetted, "vetted", "", "plan: the OSes `vet-update-archives` vets, space-separated")
	fs.StringVar(&o.attested, "attested", "", "plan: the SBOM slugs `attest` attests, space-separated")
	fs.StringVar(&o.subjectDir, "subject-dir", "release-assets", "plan: where `attest` downloads the release")
	fs.StringVar(&o.assets, "assets", "", "the release's assets as JSON (`gh api …/releases/tags/<tag>`)")
	fs.StringVar(&o.sigs, "sigs", "", "latest-json: the directory holding the update payloads' `.sig` files")
	fs.StringVar(&o.notes, "notes", "", "latest-json: the release notes file")
	fs.StringVar(&o.baseURL, "base-url", "", "latest-json: where the update payloads are served")
	fs.StringVar(&o.pubDate, "pub-date", "", "latest-json: the publication time, RFC 3339 UTC")
	fs.StringVar(&o.out, "out", "", "latest-json: where to write the manifest; stage: the fresh staging directory")
	fs.StringVar(&o.file, "file", "", "validate-latest: the manifest to check")
	fs.StringVar(&o.osName, "os", "", "stage: the OS whose build artifact this is")
	fs.StringVar(&o.from, "from", "", "stage: the downloaded build artifact")
	fs.StringVar(&o.advisories, "advisories", "", "stage: the checkout's advisory list")
	fs.StringVar(&o.carriedBy, "carried-by", "", "latest-json, validate-latest, checksum-files: a latest.json of this version whose platforms fix which legs the release carries (without it: the legs whole on the release, signatures included)")
	fs.StringVar(&o.previous, "previous", "", "latest-json: the latest.json an earlier attempt uploaded, whose legs a re-run keeps if it passes the validator (else a warning, and the snapshot decides)")
	fs.StringVar(&o.newerThan, "newer-than", "", "latest-json, validate-latest: the live manifest's version, which this one must move past (empty: unknown, or a deliberate republish)")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if !versionPattern.MatchString(o.version) {
		return fmt.Errorf("-version must look like 0.62.0, got %q", o.version)
	}
	release := manifest.For(o.version)

	switch args[0] {
	case "plan":
		return runPlan(o, release)
	case "latest-json", "validate-latest":
		return runLatest(o, release, args[0] == "validate-latest")
	case "stage":
		return runStage(o, release)
	case "checksum-files":
		return runChecksumFiles(o, release)
	}
	return fmt.Errorf("unknown command %q: plan, latest-json, validate-latest, stage, or checksum-files", args[0])
}

func runPlan(o options, release manifest.Release) error {
	if problems := manifest.ShippingProblems(manifest.ShippedOSes); len(problems) > 0 {
		return fmt.Errorf("`ShippedOSes` is inconsistent: %s", strings.Join(problems, "; "))
	}
	// A platform this version leaves out (before its `Since`, or a prerelease) gets a line in the
	// run's summary, so a skipped Windows leg is never a silent one.
	for _, line := range manifest.Skipped(o.version) {
		fmt.Fprintf(os.Stderr, "::notice::%s\n", line)
	}
	outs, err := plan(planInput{
		release: release, wiring: manifest.All(o.version), subjectDir: o.subjectDir,
		vetted: osList(o.vetted), attested: strings.Fields(o.attested),
	})
	if err != nil {
		return err
	}
	return appendOutputs(formatOutputs(outs))
}

func runLatest(o options, release manifest.Release, validate bool) error {
	have, err := readReleaseAssets(o.assets)
	if err != nil {
		return err
	}
	keys, err := trustedKeyIDs(os.Getenv("RELEASE_ASSETS_ROOT"))
	if err != nil {
		return fmt.Errorf("reading the updater keys the app trusts (run this through scripts/release-assets.sh): %w", err)
	}
	in := latestInput{
		release: release, assets: have, sigDir: o.sigs, pubDate: o.pubDate,
		baseURL: strings.TrimSuffix(o.baseURL, "/"), trustedKeyIDs: keys, newerThan: o.newerThan,
	}
	if prev, ok := previousLegs(in, o.previous); ok {
		in.release = prev
	} else if in.release, err = carriedRelease(in.release, have, o.carriedBy, o.sigs); err != nil {
		return err
	}
	if validate {
		return validateFile(o.file, in.rules())
	}
	return writeLatest(in, o.notes, o.out)
}

func runStage(o options, release manifest.Release) error {
	if o.osName == "" {
		return errNoOS
	}
	res, err := stage(stageInput{release: release, os: manifest.OS(o.osName), from: o.from, advisories: o.advisories, out: o.out})
	if err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "Staged %s\n", strings.Join(res.files, ", "))
	return appendOutputs(formatOutputs(res.outputs()))
}

func runChecksumFiles(o options, release manifest.Release) error {
	have, err := readReleaseAssets(o.assets)
	if err != nil {
		return err
	}
	if release, err = carriedRelease(release, have, o.carriedBy, ""); err != nil {
		return err
	}
	files, err := manifest.Present(release.Downloads(), assetNames(have))
	if err != nil {
		return err
	}
	for _, name := range manifest.Names(files) {
		fmt.Println(name)
	}
	return nil
}

// carriedRelease is release as it carries its legs (`manifest/legs.go`): the ones the
// `latest.json` at carriedBy names, when given, or else the ones whole among the release's assets,
// with a `::warning::` for each leg the release ships without. A macOS-only release has no legs, so
// it comes back as it went in.
func carriedRelease(release manifest.Release, assets []releaseAsset, carriedBy, sigDir string) (manifest.Release, error) {
	if len(release.LegOSes()) == 0 {
		// Nothing to carry, so nothing to read: a macOS-only release behaves as it always did, a
		// damaged earlier `latest.json` included (a re-run overwrites it).
		return release, nil
	}
	if carriedBy != "" {
		raw, err := os.ReadFile(carriedBy)
		if err != nil {
			return manifest.Release{}, err
		}
		return release.CarriedBy(raw)
	}
	whole, lacking := release.CarriedWhole(finishedAssets(assets, sigDir))
	for _, osName := range release.LegOSes() {
		if missing, ok := lacking[osName]; ok {
			fmt.Fprintf(os.Stderr, "::warning::%s\n", manifest.LackingLine(release.Version, osName, missing))
		}
	}
	return whole, nil
}

// finishedAssets are the names of the assets a leg can count: fully saved (GitHub's `state` is
// `uploaded`, never a half-saved `starter`), not empty, and, for a signature, on disk in sigDir
// (when given) since `latest-json` reads it from there. Anything else leaves its leg incomplete, so a
// Windows upload still going when `publish` looks costs the leg, never the macOS release.
func finishedAssets(assets []releaseAsset, sigDir string) []string {
	var names []string
	for _, a := range assets {
		if a.State != "uploaded" || a.Size == 0 {
			continue
		}
		if sigDir != "" && strings.HasSuffix(a.Name, ".sig") {
			if info, err := os.Stat(filepath.Join(sigDir, a.Name)); err != nil || info.Size() == 0 {
				continue
			}
		}
		names = append(names, a.Name)
	}
	return names
}

// previousLegs is the release as an earlier `publish` attempt's `latest.json` (at path) carried
// its legs, when that file is one to keep: this version's, naming whole legs, and passing
// `validateLatest` against the release as it stands. A damaged, half-saved, or stale one is never
// carried: it gets a `::warning::` and the snapshot decides instead, which is safe until the
// mirror holds the release (`latest.json` itself is never mirrored). Only a release with legs reads
// it; a macOS-only one has nothing to keep.
func previousLegs(in latestInput, path string) (manifest.Release, bool) {
	if path == "" || len(in.release.LegOSes()) == 0 {
		return manifest.Release{}, false
	}
	raw, err := os.ReadFile(path)
	if err == nil {
		var carried manifest.Release
		if carried, err = in.release.CarriedBy(raw); err == nil {
			rules := in.rules()
			rules.release = carried
			if err = validateLatest(raw, rules); err == nil {
				return carried, true
			}
		}
	}
	fmt.Fprintf(os.Stderr, "::warning::The latest.json an earlier attempt uploaded isn't one to keep (%v), so this release's contents come from the release as it stands.\n", oneLine(err))
	return manifest.Release{}, false
}

// oneLine folds an error onto one line, for a workflow annotation.
func oneLine(err error) string {
	return strings.Join(strings.Fields(err.Error()), " ")
}

// validateFile runs the validator on a manifest as published.
func validateFile(path string, r latestRules) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	return validateLatest(raw, r)
}

// writeLatest builds the manifest with the release notes at notesPath and writes it to out,
// only once it validates.
func writeLatest(in latestInput, notesPath, out string) error {
	body, err := os.ReadFile(notesPath)
	if err != nil {
		return err
	}
	in.notes = string(body)
	if strings.TrimSpace(in.notes) == "" {
		fmt.Fprintf(os.Stderr, "release-assets: warning: no release notes for %s (CHANGELOG.md has no section for it); installs show the update with no notes\n", in.release.Version)
	}
	doc, err := buildLatest(in)
	if err != nil {
		return err
	}
	return os.WriteFile(out, []byte(doc), 0o644)
}

func osList(s string) []manifest.OS {
	var out []manifest.OS
	for _, f := range strings.Fields(s) {
		out = append(out, manifest.OS(f))
	}
	return out
}

// appendOutputs adds step outputs to `$GITHUB_OUTPUT`, or prints them outside Actions.
func appendOutputs(s string) error {
	path := os.Getenv("GITHUB_OUTPUT")
	if path == "" {
		fmt.Print(s)
		return nil
	}
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY|os.O_CREATE, 0o644)
	if err != nil {
		return err
	}
	if _, err := f.WriteString(s); err != nil {
		_ = f.Close()
		return err
	}
	return f.Close()
}
