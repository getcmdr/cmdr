package checks

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// groupPolicyDriftExit is the generator's "the committed files are stale" exit code
// (`scripts/group-policy/main.go`), apart from a failure to generate at all.
const groupPolicyDriftExit = 3

// RunDesktopGroupPolicyFresh fails when the committed Group Policy template
// (`apps/desktop/packaging/group-policy/`) differs from what the app's policy definition
// (`managed_policy/policies.json`) generates: a policy added, renamed, or reworded without
// regenerating, or a hand edit to a generated file. Admins load these files into Group Policy and
// Intune, so a stale one sets a value the app doesn't read.
//
// Same shape as `desktop-shipped-locales-fresh`: in `--ci` mode drift fails; locally the check
// regenerates the files and reports the change, so the dev commits them with the edit that caused
// it.
func RunDesktopGroupPolicyFresh(ctx *CheckContext) (CheckResult, error) {
	// Built, then run, because `go run` turns every nonzero exit into 1.
	binDir, err := os.MkdirTemp("", "group-policy-")
	if err != nil {
		return CheckResult{}, err
	}
	defer func() { _ = os.RemoveAll(binDir) }()
	bin := filepath.Join(binDir, "group-policy")
	buildCmd := exec.Command("go", "build", "-o", bin, ".")
	buildCmd.Dir = filepath.Join(ctx.RootDir, "scripts", "group-policy")
	if output, err := RunCommand(buildCmd, true); err != nil {
		return CheckResult{}, fmt.Errorf("couldn't build the Group Policy generator:\n%s", indentOutput(strings.TrimSpace(output)))
	}

	checkCmd := exec.Command(bin, "-root", ctx.RootDir, "-check")
	output, err := RunCommand(checkCmd, true)
	if err == nil {
		return Success("Group Policy template in sync with policies.json"), nil
	}
	var exitErr *exec.ExitError
	if !errors.As(err, &exitErr) || exitErr.ExitCode() != groupPolicyDriftExit {
		return CheckResult{}, fmt.Errorf("the Group Policy generator failed:\n%s", indentOutput(strings.TrimSpace(output)))
	}
	if ctx.CI {
		return CheckResult{}, fmt.Errorf("the Group Policy template is stale. Run ./scripts/group-policy.sh and commit it:\n%s", indentOutput(strings.TrimSpace(output)))
	}
	writeCmd := exec.Command(bin, "-root", ctx.RootDir)
	if output, err := RunCommand(writeCmd, true); err != nil {
		return CheckResult{}, fmt.Errorf("the Group Policy generator failed:\n%s", indentOutput(strings.TrimSpace(output)))
	}
	return SuccessWithChanges("Group Policy template regenerated from policies.json"), nil
}
