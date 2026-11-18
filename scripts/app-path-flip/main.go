// app-path-flip: the compiler-driven codemod for the `Volume` trait's flip to app paths
// (docs/specs/app-path-plan.md § "4. Flip the trait"). It runs `cargo check` on one crate,
// rewrites only the spans rustc points at, by error code and type pair, and re-runs until the
// crate checks or no rule applies. What's left is the crate's residue, listed at the end.
//
// Usage, from the repo root (README.md beside this file says when to pass what):
//
//	go -C scripts/app-path-flip run . -p cmdr-archive [-p more…] [-cargo-args "--features x"] [-report FILE]
//
// Deterministic: rules read only rustc's diagnostics and the files, edits apply in a fixed
// order, and `cargo fmt` formats the result, so a re-run on the same base gives the same tree.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type multiFlag []string

func (m *multiFlag) String() string     { return strings.Join(*m, ",") }
func (m *multiFlag) Set(v string) error { *m = append(*m, v); return nil }

func main() {
	var crates multiFlag
	flag.Var(&crates, "p", "a crate to flip (repeatable; flipped in the order given)")
	cargoArgs := flag.String("cargo-args", "", "extra `cargo check` arguments, space-separated (`--features testing`)")
	maxRounds := flag.Int("max-rounds", 40, "stop after this many check-and-rewrite rounds per crate")
	reportPath := flag.String("report", "", "write the rule counts and the residue as JSON here")
	root := flag.String("root", "", "the repo root (default: found from the working directory)")
	skipFmt := flag.Bool("no-fmt", false, "leave the rewritten files unformatted")
	var auditDirs multiFlag
	flag.Var(&auditDirs, "audit-imports", "list each import the codemod adds that sits right under an attribute, in this dir (repeatable), and exit")
	flag.Parse()
	if len(auditDirs) > 0 {
		os.Exit(auditImports(*root, auditDirs))
	}
	if len(crates) == 0 {
		fmt.Fprintln(os.Stderr, "usage: app-path-flip -p <crate> [-p <crate>…] [-cargo-args ARGS] [-report FILE]")
		os.Exit(2)
	}
	repo, err := repoRoot(*root)
	if err != nil {
		fail(err)
	}
	var reports []crateReport
	for _, crate := range crates {
		report, err := flipCrate(repo, crate, strings.Fields(*cargoArgs), *maxRounds, !*skipFmt)
		if err != nil {
			fail(err)
		}
		printReport(report)
		reports = append(reports, report)
	}
	if *reportPath != "" {
		data, _ := json.MarshalIndent(reports, "", "  ")
		if err := os.WriteFile(*reportPath, append(data, '\n'), 0o644); err != nil {
			fail(err)
		}
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "app-path-flip:", err)
	os.Exit(1)
}

// crateReport is what one crate's run did and left.
type crateReport struct {
	Crate   string         `json:"crate"`
	Rounds  int            `json:"rounds"`
	ByRule  map[string]int `json:"byRule"`
	ByFile  map[string]int `json:"byFile"`
	Residue []string       `json:"residue"`
	Checks  bool           `json:"checks"`
}

func repoRoot(given string) (string, error) {
	if given != "" {
		return filepath.Abs(given)
	}
	out, err := exec.Command("git", "rev-parse", "--show-toplevel").Output()
	if err != nil {
		return "", fmt.Errorf("finding the repo root: %w", err)
	}
	return strings.TrimSpace(string(out)), nil
}

// flipCrate runs the fixpoint on one crate.
func flipCrate(repo, crate string, cargoArgs []string, maxRounds int, format bool) (crateReport, error) {
	report := crateReport{Crate: crate, ByRule: map[string]int{}, ByFile: map[string]int{}}
	dir, err := crateDir(repo, crate)
	if err != nil {
		return report, err
	}
	rustFiles, err := listRust(repo, dir)
	if err != nil {
		return report, err
	}
	workspaceFiles, err := listRust(repo, "crates")
	if err != nil {
		return report, err
	}
	keep := func(file string) bool { return strings.HasPrefix(file, dir+"/") }
	rules := func(diags []diagnostic) ([]edit, error) {
		return roundEdits(repo, crate, rustFiles, workspaceFiles, diags)
	}
	last, checked, err := fixpoint(repo, crate, "check", cargoArgs, keep, maxRounds, &report, rules)
	if err != nil {
		return report, err
	}
	// The rewrites can make code clippy flags (a `vec!` now only iterated, a `to_path_buf()`
	// the bridge borrows anyway); the base was clippy-clean, so those lints are the
	// rewrites', and clippy's own machine-applicable fix for each is the type-preserving one.
	if checked && !hasErrors(last) {
		lints := func(diags []diagnostic) ([]edit, error) { return chooseEdits(clippyFixEdits(diags)), nil }
		if last, checked, err = fixpoint(repo, crate, "clippy", cargoArgs, keep, maxRounds, &report, lints); err != nil {
			return report, err
		}
	}
	report.Checks = checked && !hasErrors(last)
	report.Residue = residueLines(last)
	if format {
		if err := runQuiet(repo, "cargo", "fmt", "-p", crate); err != nil {
			return report, err
		}
	}
	return report, nil
}

// fixpoint runs `cargo <tool>` and applies what `rules` makes of its diagnostics until a
// round makes no edit, answering the last round's diagnostics and whether cargo succeeded.
func fixpoint(repo, crate, tool string, cargoArgs []string, keep func(string) bool, maxRounds int, report *crateReport,
	rules func([]diagnostic) ([]edit, error)) ([]diagnostic, bool, error) {
	var last []diagnostic
	checked := false
	for round := 1; round <= maxRounds; round++ {
		report.Rounds++
		diags, ok, err := cargoRun(repo, tool, crate, cargoArgs, keep)
		if err != nil {
			return nil, false, err
		}
		last, checked = diags, ok
		edits, err := rules(diags)
		if err != nil {
			return nil, false, err
		}
		fmt.Fprintf(os.Stderr, "%s %s round %d: %d diagnostics, %d edits\n", crate, tool, round, len(diags), len(edits))
		if len(edits) == 0 {
			break
		}
		if err := applyEdits(repo, edits); err != nil {
			return nil, false, err
		}
		for _, e := range edits {
			report.ByRule[e.rule]++
			report.ByFile[e.file]++
		}
	}
	return last, checked, nil
}

// clippyFixable are the lints a rewrite can cause, each fixed by clippy's own suggestion.
var clippyFixable = map[string]bool{
	"clippy::useless_vec": true, "clippy::unnecessary_to_owned": true, "clippy::needless_borrow": true,
	"clippy::needless_borrows_for_generic_args": true, "clippy::redundant_clone": true, "unused_imports": true,
	"clippy::unnecessary_mut_passed": true, "clippy::redundant_closure": true, "clippy::op_ref": true,
}

func clippyFixEdits(diags []diagnostic) []edit {
	var edits []edit
	for i := range diags {
		d := &diags[i]
		if d.Level != "warning" || !clippyFixable[d.code()] || d.primary().Macro != "" {
			continue
		}
		for _, child := range d.Children {
			for _, s := range child.Spans {
				if s.SuggestedReplacement == nil || s.SuggestionApplicability == nil || *s.SuggestionApplicability != "MachineApplicable" {
					continue
				}
				edits = append(edits, edit{file: s.FileName, start: s.ByteStart, end: s.ByteEnd, text: *s.SuggestedReplacement, rule: "clippy fix " + strings.TrimPrefix(d.code(), "clippy::")})
			}
		}
	}
	return edits
}

func hasErrors(diags []diagnostic) bool {
	for i := range diags {
		if diags[i].Level == "error" {
			return true
		}
	}
	return false
}

func residueLines(diags []diagnostic) []string {
	var lines []string
	for i := range diags {
		d := &diags[i]
		p := d.primary()
		lines = append(lines, fmt.Sprintf("%s:%d: %s[%s]: %s %s", p.FileName, p.LineStart, d.Level, d.code(), d.Message, p.label()))
	}
	return lines
}

func printReport(r crateReport) {
	fmt.Printf("== %s: %d rounds, checks: %v\n", r.Crate, r.Rounds, r.Checks)
	rules := make([]string, 0, len(r.ByRule))
	for rule := range r.ByRule {
		rules = append(rules, rule)
	}
	sort.Strings(rules)
	for _, rule := range rules {
		fmt.Printf("  %-40s %d\n", rule, r.ByRule[rule])
	}
	if len(r.Residue) > 0 {
		fmt.Printf("  residue (%d):\n", len(r.Residue))
		for _, line := range r.Residue {
			fmt.Printf("    %s\n", line)
		}
	}
}

// crateDir finds a workspace member's directory, relative to the repo root.
func crateDir(repo, crate string) (string, error) {
	out, err := exec.Command("cargo", "metadata", "--no-deps", "--format-version", "1", "--manifest-path", filepath.Join(repo, "Cargo.toml")).Output()
	if err != nil {
		return "", fmt.Errorf("cargo metadata: %w", err)
	}
	var meta struct {
		Packages []struct {
			Name         string `json:"name"`
			ManifestPath string `json:"manifest_path"`
		} `json:"packages"`
	}
	if err := json.Unmarshal(out, &meta); err != nil {
		return "", err
	}
	for _, p := range meta.Packages {
		if p.Name == crate {
			rel, err := filepath.Rel(repo, filepath.Dir(p.ManifestPath))
			return filepath.ToSlash(rel), err
		}
	}
	return "", fmt.Errorf("no workspace member named %q", crate)
}

func listRust(repo, dir string) ([]string, error) {
	var files []string
	err := filepath.WalkDir(filepath.Join(repo, dir), func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() && (d.Name() == "target" || strings.HasPrefix(d.Name(), ".")) {
			return filepath.SkipDir
		}
		if !d.IsDir() && strings.HasSuffix(path, ".rs") {
			rel, _ := filepath.Rel(repo, path)
			files = append(files, filepath.ToSlash(rel))
		}
		return nil
	})
	sort.Strings(files)
	return files, err
}

func cargoRun(repo, tool, crate string, extra []string, keep func(string) bool) ([]diagnostic, bool, error) {
	args := append([]string{tool, "-p", crate, "--all-targets", "--keep-going", "--message-format=json"}, extra...)
	cmd := exec.Command("cargo", args...)
	cmd.Dir = repo
	cmd.Stderr = nil
	out, err := cmd.Output()
	if err != nil {
		if _, isExit := err.(*exec.ExitError); !isExit {
			return nil, false, fmt.Errorf("cargo %s -p %s: %w", tool, crate, err)
		}
	}
	diags, parseErr := parseDiagnostics(out, keep)
	return diags, err == nil, parseErr
}

func runQuiet(repo, name string, args ...string) error {
	cmd := exec.Command(name, args...)
	cmd.Dir = repo
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("%s %s: %w\n%s", name, strings.Join(args, " "), err, out)
	}
	return nil
}
