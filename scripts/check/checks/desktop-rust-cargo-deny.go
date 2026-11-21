package checks

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// RunCargoDeny enforces license and dependency policies.
//
// `deny.toml` lives at the workspace root, and the check runs from there. The
// policy was always workspace-wide in effect — cargo-deny reads the whole
// `cargo metadata` graph regardless of where it starts — but the config sat inside
// one member, which read as if it governed only that member. With more than one
// crate in the graph, that reading would have been wrong in a way nobody noticed.
//
// Its advisories follow the fix-age policy `govulncheck` and `osv-scanner` share
// (`vuln-fix-age.go`): one whose fix is under six days old warns, then fails. The fix comes
// from the advisory's OSV record and is dated by crates.io, the same way osv-scanner's are.
// A license, ban, or source failure fails at once, as before.
func RunCargoDeny(ctx *CheckContext) (CheckResult, error) {
	if _, err := os.Stat(DenyConfigPath(ctx.RootDir)); os.IsNotExist(err) {
		return Skipped("no deny.toml"), nil
	}

	if err := ensureCargoDeny(); err != nil {
		return CheckResult{}, err
	}

	// `advisories` is in the set: `deny.toml` scopes the graph to the macOS targets we
	// ship and limits `unmaintained` to workspace crates, so this lane stays green on
	// Tauri's unfixable transitive noise while still failing on a real RUSTSEC
	// vulnerability anywhere in a shipped dependency.
	sections := []string{"advisories", "licenses", "bans", "sources"}
	cmd := exec.Command("cargo", append([]string{"deny", "--format", "json", "check"}, sections...)...)
	cmd.Dir = ctx.RootDir
	output, runErr := RunCommand(cmd, true)
	advisories, others := parseCargoDenyDiagnostics(output)
	if runErr != nil && len(advisories) == 0 && len(others) == 0 {
		return CheckResult{}, fmt.Errorf("cargo-deny check failed\n%s", indentOutput(output))
	}
	if len(others) > 0 {
		// Its human-readable report shows a license or ban failure far better than the JSON does.
		human := exec.Command("cargo", append([]string{"deny", "check"}, sections...)...)
		human.Dir = ctx.RootDir
		humanOutput, _ := RunCommand(human, true)
		return CheckResult{}, fmt.Errorf("cargo-deny check failed\n%s", indentOutput(humanOutput))
	}

	releases := newRegistryReleaseTimes()
	blocking, waiting := splitCargoDenyAdvisories(advisories, time.Now(), releases.osvFix, releases.fixReleased)
	var blockingLines []string
	for _, a := range blocking {
		blockingLines = append(blockingLines, "[Cargo.lock] "+a.describe())
	}
	sort.Strings(blockingLines)
	pending := map[vulnPendingFix]*vulnPendingGroup{}
	for _, a := range waiting {
		addPendingFix(pending, a, "Cargo.lock")
	}
	return vulnVerdict("cargo-deny", blockingLines,
		"Upgrade past the fix, or, if it can't reach what we ship, ignore it with a reason in deny.toml.",
		pending, "Advisories, licenses, and deps OK")
}

// cargoDenyVersion is the cargo-deny every run uses. It lives in this file because
// `.github/workflows/advisories.yml` keys its tool cache on this file's hash, so a bump
// here reinstalls there too.
const cargoDenyVersion = "0.19.6"

// ensureCargoDeny installs cargo-deny at cargoDenyVersion unless that exact version is
// already on PATH. Any other version gets replaced: checking only for presence let a dev
// machine keep 0.18.9 while CI ran the pin, and 0.18.9 predates `[advisories] unsound`
// (0.19.0, default `workspace`), so local runs flagged transitive unsound advisories CI
// never sees (the `heapless` 0.8.0 set, 2026-11-21).
func ensureCargoDeny() error {
	if CommandExists("cargo-deny") {
		output, _ := exec.Command("cargo", "deny", "--version").Output()
		if cargoToolVersionIs(string(output), "cargo-deny", cargoDenyVersion) {
			return nil
		}
	}
	// `--force`, so a different installed version gets replaced instead of refused.
	installCmd := exec.Command("cargo", "install", "cargo-deny", "--version", cargoDenyVersion, "--locked", "--force")
	if _, err := RunCommand(installCmd, true); err != nil {
		return fmt.Errorf("failed to install cargo-deny: %w", err)
	}
	return nil
}

type cargoDenyLine struct {
	Type   string `json:"type"`
	Fields struct {
		Code     string `json:"code"`
		Severity string `json:"severity"`
		Message  string `json:"message"`
		Advisory *struct {
			ID      string `json:"id"`
			Package string `json:"package"`
			Title   string `json:"title"`
		} `json:"advisory"`
		Graphs []struct {
			Krate struct {
				Name    string `json:"name"`
				Version string `json:"version"`
			} `json:"Krate"`
		} `json:"graphs"`
	} `json:"fields"`
}

// parseCargoDenyDiagnostics reads `cargo deny --format json check`: one JSON object per line,
// mixed with cargo's plain progress lines. It returns the error-level advisories, one per ID and
// crate version (cargo-deny can repeat one), and a line per other error-level diagnostic (a
// license, ban, or source failure), which no grace period applies to.
func parseCargoDenyDiagnostics(output string) (advisories []vulnAdvisory, others []string) {
	seen := map[string]bool{}
	for _, raw := range strings.Split(output, "\n") {
		var line cargoDenyLine
		if !strings.HasPrefix(raw, "{") || json.Unmarshal([]byte(raw), &line) != nil {
			continue
		}
		f := line.Fields
		if line.Type != "diagnostic" || f.Severity != "error" {
			continue
		}
		krate, version := "", ""
		if len(f.Graphs) > 0 {
			krate, version = f.Graphs[0].Krate.Name, f.Graphs[0].Krate.Version
		}
		if f.Advisory == nil {
			others = append(others, fmt.Sprintf("%s %s@%s: %s", f.Code, krate, version, f.Message))
			continue
		}
		if krate == "" {
			krate = f.Advisory.Package
		}
		key := f.Advisory.ID + "\x00" + krate + "\x00" + version
		if seen[key] {
			continue
		}
		seen[key] = true
		advisories = append(advisories, vulnAdvisory{
			ID: f.Advisory.ID, Summary: f.Advisory.Title, Ecosystem: "crates.io", Module: krate, Version: version,
		})
	}
	return advisories, others
}

// splitCargoDenyAdvisories names each advisory's fix (cargo-deny's JSON doesn't), then applies
// the shared fix-age split.
func splitCargoDenyAdvisories(
	advisories []vulnAdvisory,
	now time.Time,
	withFix func(vulnAdvisory) vulnAdvisory,
	fixReleased func(vulnAdvisory) (time.Time, bool),
) (blocking, waiting []vulnAdvisory) {
	filled := make([]vulnAdvisory, len(advisories))
	for i, a := range advisories {
		filled[i] = withFix(a)
	}
	return splitByFixAge(filled, now, fixReleased)
}

// DenyConfigPath is the single place the cargo-deny config's location is written
// down. `desktop-third-party-notices` derives its accepted-license list from the
// same file rather than duplicating it, so both have to agree on where it is.
func DenyConfigPath(rootDir string) string {
	return filepath.Join(rootDir, "deny.toml")
}
