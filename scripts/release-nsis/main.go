// Release NSIS: vets a Windows NSIS installer for the release pipeline (`windows-build`,
// `windows-publish`, `vet-update-archives`) without running it, and is the package
// `release-finish` vets with on the laptop. What it checks: `nsis/nsis.go`.
//
// Its own module, so the one third-party module it needs (`github.com/ulikunitz/xz`, for the
// installer's LZMA payload) builds only into the Windows leg's jobs and `release-finish`, never
// into the macOS release's `guard` and `publish`, whose `release-assets` stays stdlib-only.
//
// # Usage
//
//	./scripts/release-nsis.sh -version X.Y.Z Cmdr_X.Y.Z_x64-setup.exe...
package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"

	"cmdr/scripts/release-nsis/nsis"
)

var versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]*[0-9A-Za-z])?$`)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "release-nsis: %v\n", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	fs := flag.NewFlagSet("release-nsis", flag.ContinueOnError)
	version := fs.String("version", "", "the release's version, without the leading v")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if !versionPattern.MatchString(*version) {
		return fmt.Errorf("-version must look like 0.62.0, got %q", *version)
	}
	return vetFiles(os.Getenv("RELEASE_NSIS_ROOT"), *version, fs.Args())
}

// vetFiles runs `nsis.Vet` on each installer, printing a verdict per file, and fails if any is
// refused (or none was named: a gate that vets nothing must not pass).
func vetFiles(root, version string, paths []string) error {
	if len(paths) == 0 {
		return errors.New("name the installers to vet")
	}
	want, err := nsis.ExpectFromRepo(root, version)
	if err != nil {
		return fmt.Errorf("reading the app's name and publisher (run this through scripts/release-nsis.sh): %w", err)
	}
	refused := 0
	for _, p := range paths {
		if err := nsis.Vet(p, want); err != nil {
			fmt.Printf("    %s: REFUSED, it %v\n", filepath.Base(p), err)
			refused++
			continue
		}
		fmt.Printf("    %s: the NSIS installer of %s %s by %s, packing that build, unsigned, as the updater expects today\n", filepath.Base(p), want.ProductName, version, want.Publisher)
	}
	if refused > 0 {
		return fmt.Errorf("%d of %d Windows installers would install something other than %s", refused, len(paths), version)
	}
	return nil
}
