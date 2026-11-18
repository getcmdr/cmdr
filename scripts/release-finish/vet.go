package main

import (
	"bytes"
	"fmt"
	"os/exec"
	"path/filepath"

	"cmdr/scripts/release-assets/manifest"
	"cmdr/scripts/release-assets/nsis"
)

// runner runs a command in a directory and answers its combined output, so tests can stand in
// for cargo.
type runner func(dir, name string, args ...string) (string, error)

// vetArchives refuses to go on unless every update payload among names is one the updater would
// install. A macOS archive's bundle passes the check the macOS updater runs before installing:
// unpacked with the installer's own extraction, code-signed by Cmdr's Developer ID, and naming
// this release's version. It runs the app's own code (`crates/cmdr-update-trust`, binary
// `cmdr-vet-update`), so the gate and the updater can't drift. A Windows NSIS installer must carry
// this release's version in its version resource (`nsis.Vet`), since the updater there trusts the
// version its signature names. Without this, an archive the updater refuses (or one installing
// another version) could be signed and published, and every install would be stuck. Files that
// aren't update payloads (the advisory list) have nothing to vet.
func vetArchives(run runner, root string, release manifest.Release, dir string, names []string) error {
	version := release.Version
	byName := map[string]manifest.File{}
	for _, f := range release.Files {
		byName[f.Name] = f
	}
	var mac []string
	for _, name := range names {
		f, ok := byName[name]
		switch {
		case !ok:
			return fmt.Errorf("refusing to sign %s: it isn't in the release asset manifest", name)
		case f.OS == manifest.MacOS && f.Update():
			mac = append(mac, name)
		case f.Kind == manifest.NSIS:
			step("Checking %s's version resource", name)
			if err := nsis.Vet(filepath.Join(dir, name), version); err != nil {
				return fmt.Errorf("refusing to sign %s: it %w", name, err)
			}
			fmt.Printf("    %s: the NSIS installer of %s\n", name, version)
		case f.Update():
			return fmt.Errorf("refusing to sign %s: nothing here vets a %s %s update payload yet", name, f.OS, f.Kind)
		}
	}
	if len(mac) == 0 {
		return nil
	}
	step("Checking each bundle's code signature the way the updater does")
	args := []string{"run", "--quiet", "--release", "--locked", "-p", "cmdr-update-trust", "--bin", "cmdr-vet-update", "--",
		"--version", version}
	for _, name := range mac {
		args = append(args, filepath.Join(dir, name))
	}
	out, err := run(root, "cargo", args...)
	fmt.Println(out)
	if err != nil {
		return fmt.Errorf("refusing to sign: the updater would refuse at least one of these archives (above), so no install could take this release: %w", err)
	}
	return nil
}

// runCombined is the real runner.
func runCombined(dir, name string, args ...string) (string, error) {
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	var out bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &out
	err := cmd.Run()
	return string(bytes.TrimRight(out.Bytes(), "\n")), err
}
