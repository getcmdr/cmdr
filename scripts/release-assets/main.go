// Release assets: the release asset manifest's command line, for the release pipeline's shell
// steps. The manifest itself, and what reads it, is `manifest/manifest.go`.
//
// # Usage
//
//	./scripts/release-assets.sh plan -version X.Y.Z -vetted "macos windows" -attested "macos-aarch64 macos-x86_64 frontend windows-x86_64" -subject-dir release-assets
//	./scripts/release-assets.sh latest-json -version X.Y.Z -assets release.json -sigs DIR -notes notes.txt -base-url URL -pub-date DATE [-newer-than X.Y.Z] -out latest.json
//	./scripts/release-assets.sh validate-latest -version X.Y.Z -assets release.json -sigs DIR -base-url URL [-newer-than X.Y.Z] -file latest.json
//	./scripts/release-assets.sh checksum-files -version X.Y.Z -assets release.json
//	./scripts/release-assets.sh vet-nsis -version X.Y.Z Cmdr_X.Y.Z_x64-setup.exe
//
// `vet-nsis` refuses a Windows installer whose version resource isn't the release's, or that
// carries an Authenticode signature nothing checks yet (`nsis/nsis.go`).
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
	"cmdr/scripts/release-assets/nsis"
)

var versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]*[0-9A-Za-z])?$`)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "release-assets: %v\n", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		return errors.New("name a command: plan, latest-json, validate-latest, or checksum-files")
	}
	fs := flag.NewFlagSet(args[0], flag.ContinueOnError)
	version := fs.String("version", "", "the release's version, without the leading v")
	vetted := fs.String("vetted", "", "plan: the OSes `vet-update-archives` vets, space-separated")
	attested := fs.String("attested", "", "plan: the SBOM slugs `attest` attests, space-separated")
	subjectDir := fs.String("subject-dir", "release-assets", "plan: where `attest` downloads the release")
	assets := fs.String("assets", "", "the release's assets as JSON (`gh api …/releases/tags/<tag>`)")
	sigs := fs.String("sigs", "", "latest-json: the directory holding the update payloads' `.sig` files")
	notes := fs.String("notes", "", "latest-json: the release notes file")
	baseURL := fs.String("base-url", "", "latest-json: where the update payloads are served")
	pubDate := fs.String("pub-date", "", "latest-json: the publication time, RFC 3339 UTC")
	out := fs.String("out", "", "latest-json: where to write the manifest")
	file := fs.String("file", "", "validate-latest: the manifest to check")
	newerThan := fs.String("newer-than", "", "latest-json, validate-latest: the live manifest's version, which this one must move past (empty: unknown, or a deliberate republish)")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if !versionPattern.MatchString(*version) {
		return fmt.Errorf("-version must look like 0.62.0, got %q", *version)
	}
	release := manifest.For(*version)

	switch args[0] {
	case "plan":
		outs, err := plan(planInput{
			release: release, wiring: manifest.All(*version), subjectDir: *subjectDir,
			vetted: osList(*vetted), attested: strings.Fields(*attested),
		})
		if err != nil {
			return err
		}
		return appendOutputs(formatOutputs(outs))
	case "latest-json", "validate-latest":
		have, err := readReleaseAssets(*assets)
		if err != nil {
			return err
		}
		keys, err := trustedKeyIDs(os.Getenv("RELEASE_ASSETS_ROOT"))
		if err != nil {
			return fmt.Errorf("reading the updater keys the app trusts (run this through scripts/release-assets.sh): %w", err)
		}
		in := latestInput{
			release: release, assets: have, sigDir: *sigs, pubDate: *pubDate,
			baseURL: strings.TrimSuffix(*baseURL, "/"), trustedKeyIDs: keys, newerThan: *newerThan,
		}
		if args[0] == "validate-latest" {
			return validateFile(*file, in.rules())
		}
		return writeLatest(in, *notes, *out)
	case "vet-nsis":
		return vetNSISFiles(*version, fs.Args())
	case "checksum-files":
		have, err := readReleaseAssets(*assets)
		if err != nil {
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
	return fmt.Errorf("unknown command %q: plan, latest-json, validate-latest, vet-nsis, or checksum-files", args[0])
}

// vetNSISFiles runs `nsis.Vet` on each installer, printing a verdict per file, and fails if any
// is refused (or none was named: a gate that vets nothing must not pass).
func vetNSISFiles(version string, paths []string) error {
	if len(paths) == 0 {
		return errors.New("vet-nsis: name the installers to vet")
	}
	refused := 0
	for _, p := range paths {
		if err := nsis.Vet(p, version); err != nil {
			fmt.Printf("    %s: REFUSED, it %v\n", filepath.Base(p), err)
			refused++
			continue
		}
		fmt.Printf("    %s: the NSIS installer of %s, unsigned, as the updater expects today\n", filepath.Base(p), version)
	}
	if refused > 0 {
		return fmt.Errorf("%d of %d Windows installers would install something other than %s", refused, len(paths), version)
	}
	return nil
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
