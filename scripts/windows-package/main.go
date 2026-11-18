// Windows package: makes the Windows release's two packages beside the NSIS installer, from the
// one compiled `Cmdr.exe`: the Microsoft Store's MSIX and the Group Policy template zip. What
// each holds and why: `apps/desktop/packaging/msix/DETAILS.md`; the release job that runs it:
// `docs/guides/releasing.md` § Windows releases.
//
// # Usage
//
//	./scripts/windows-package.sh msix -version X.Y.Z -exe Cmdr.exe -out Cmdr_X.Y.Z_x64.msix [-identity release|dev]
//	./scripts/windows-package.sh verify-msix -version X.Y.Z [-identity release|dev] Cmdr_X.Y.Z_x64.msix
//	./scripts/windows-package.sh group-policy -version X.Y.Z -out cmdr-group-policy-X.Y.Z.zip
//
// `msix` stages the package (the exe, the bundle's resources from `tauri.conf.json`, the logos,
// and the manifest with its identity and version filled in), refuses anything that would make it
// a different app than the one the Store should get, packs it with Microsoft's winapp CLI (on
// `PATH`, unsigned: the Store signs it), and verifies the result. `-identity release` (the
// default) takes the Partner Center identity from `identity.json` and fails loudly while it's
// still a placeholder; `dev` takes a local test identity, which runs unpacked in Developer Mode
// (`apps/desktop/packaging/msix/DETAILS.md` § The two identities). `verify-msix` re-runs the
// checks on a built package. `group-policy` zips the committed ADMX and ADML files.
package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintf(os.Stderr, "windows-package: %v\n", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		return errors.New("name a command: msix, verify-msix, or group-policy")
	}
	fs := flag.NewFlagSet(args[0], flag.ContinueOnError)
	root := fs.String("root", "", "the repo root (scripts/windows-package.sh sets it)")
	version := fs.String("version", "", "the release's version, without the leading v")
	exe := fs.String("exe", "", "msix: the compiled Cmdr.exe, as `tauri build --no-bundle` left it")
	out := fs.String("out", "", "msix, group-policy: the file to write")
	identity := fs.String("identity", "release", "msix, verify-msix: `release` (Partner Center's) or `dev` (a local test identity)")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if *root == "" {
		return errors.New("-root is required (run it through scripts/windows-package.sh)")
	}
	msixVersion, err := packageVersion(*version)
	if err != nil {
		return err
	}
	switch args[0] {
	case "msix":
		if *exe == "" || *out == "" {
			return errors.New("msix needs -exe and -out")
		}
		id, err := loadIdentity(*root, *identity)
		if err != nil {
			return err
		}
		return buildMSIX(msixInput{root: *root, exe: *exe, out: *out, identity: id, version: msixVersion, pack: winappPack})
	case "verify-msix":
		if fs.NArg() != 1 {
			return errors.New("verify-msix takes one package")
		}
		id, err := loadIdentity(*root, *identity)
		if err != nil {
			return err
		}
		return verifyMSIX(fs.Arg(0), id, msixVersion)
	case "group-policy":
		if *out == "" {
			return errors.New("group-policy needs -out")
		}
		return zipGroupPolicy(filepath.Join(*root, groupPolicyDir), *out)
	}
	return fmt.Errorf("unknown command %q: msix, verify-msix, or group-policy", args[0])
}
